use anyhow::{Context, Result};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

const CLOB_API_BASE: &str = "https://clob.polymarket.com";

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone)]
pub struct ClobCredentials {
    pub api_key: String,
    pub secret: String,
    pub passphrase: String,
    pub wallet_address: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderRequest {
    pub token_id: String,
    pub price: String,
    pub size: String,
    pub side: String, // "BUY" or "SELL"
    #[serde(rename = "type")]
    pub order_type: String, // "GTC", "FOK", "GTD"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiration: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderResponse {
    pub order_id: Option<String>,
    pub status: Option<String>,
    pub error_msg: Option<String>,
    #[serde(default)]
    pub success: bool,
    /// HTTP status code (populated after deserialization)
    #[serde(skip)]
    pub http_status: Option<u16>,
    /// Raw response body for debugging (populated after deserialization)
    #[serde(skip)]
    pub raw_body: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrderBookResponse {
    pub market: Option<String>,
    pub asset_id: Option<String>,
    pub bids: Option<Vec<OrderBookLevel>>,
    pub asks: Option<Vec<OrderBookLevel>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrderBookLevel {
    pub price: String,
    pub size: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MidpointResponse {
    pub mid: Option<String>,
}

pub struct ClobClient {
    client: reqwest::Client,
    credentials: Option<ClobCredentials>,
}

impl ClobClient {
    pub fn new(credentials: Option<ClobCredentials>) -> Self {
        Self {
            client: reqwest::Client::new(),
            credentials,
        }
    }

    /// Generate HMAC-SHA256 signature for a request
    fn sign_request(&self, timestamp: &str, method: &str, path: &str, body: &str) -> Result<String> {
        let creds = self.credentials.as_ref().context("No credentials configured")?;

        // Message format: timestamp + method + path + body
        // Must match: https://github.com/Polymarket/py-clob-client/blob/main/py_clob_client/signing/hmac.py
        let message = format!("{}{}{}{}", timestamp, method, path, body);

        // Decode URL-safe base64 secret (Polymarket uses URL-safe base64)
        use base64::{engine::general_purpose::URL_SAFE, Engine};
        let secret_bytes = URL_SAFE.decode(&creds.secret)
            .context("Failed to decode API secret")?;

        tracing::debug!(
            "HMAC sign: message='{}{}{}{}' (body_len={}), secret_bytes_len={}",
            timestamp, method, path,
            if body.len() > 80 { &body[..80] } else { body },
            body.len(),
            secret_bytes.len(),
        );

        // Create HMAC
        let mut mac = HmacSha256::new_from_slice(&secret_bytes)
            .context("Invalid HMAC key length")?;
        mac.update(message.as_bytes());

        // Get signature and URL-safe base64 encode
        let result = mac.finalize();
        let signature = URL_SAFE.encode(result.into_bytes());

        Ok(signature)
    }

    /// Add authentication headers to a request
    fn add_auth_headers(
        &self,
        mut builder: reqwest::RequestBuilder,
        method: &str,
        path: &str,
        body: &str,
    ) -> Result<reqwest::RequestBuilder> {
        let creds = self.credentials.as_ref().context("No credentials configured")?;

        let timestamp = chrono::Utc::now().timestamp().to_string();
        let signature = self.sign_request(&timestamp, method, path, body)?;

        tracing::info!(
            "AUTH headers: method={} path={} timestamp={} api_key={}...{} address={} sig={}...{}",
            method,
            path,
            &timestamp,
            &creds.api_key[..creds.api_key.len().min(8)],
            &creds.api_key[creds.api_key.len().saturating_sub(4)..],
            &creds.wallet_address,
            &signature[..signature.len().min(8)],
            &signature[signature.len().saturating_sub(4)..],
        );

        builder = builder
            .header("POLY_ADDRESS", &creds.wallet_address)
            .header("POLY_API_KEY", &creds.api_key)
            .header("POLY_PASSPHRASE", &creds.passphrase)
            .header("POLY_TIMESTAMP", &timestamp)
            .header("POLY_SIGNATURE", &signature);

        Ok(builder)
    }

    /// Run a multi-step auth diagnostic at startup.
    pub async fn check_auth(&self) -> Result<String> {
        let creds = self.credentials.as_ref().context("No credentials configured")?;
        let mut results = Vec::new();

        // Step 1: Public endpoint — verify connectivity
        tracing::info!("[AUTH DIAG] Step 1: Testing connectivity (GET /time)...");
        let resp = self.client
            .get(format!("{}/time", CLOB_API_BASE))
            .send()
            .await;
        match resp {
            Ok(r) => {
                let status = r.status();
                let body = r.text().await.unwrap_or_default();
                tracing::info!("[AUTH DIAG] GET /time -> HTTP {} body={}", status, &body[..body.len().min(200)]);
                results.push(format!("connectivity: OK ({})", status));
            }
            Err(e) => {
                tracing::error!("[AUTH DIAG] GET /time -> FAILED: {:?}", e);
                results.push(format!("connectivity: FAILED ({})", e));
            }
        }

        // Step 2: Authenticated endpoint — verify HMAC credentials
        tracing::info!("[AUTH DIAG] Step 2: Testing L2 auth (GET /api-keys)...");
        let path = "/api-keys";
        let url = format!("{}{}", CLOB_API_BASE, path);
        let builder = self.client.get(&url);
        let builder = self.add_auth_headers(builder, "GET", path, "")?;
        let resp = builder.send().await;
        match resp {
            Ok(r) => {
                let status = r.status();
                let body = r.text().await.unwrap_or_default();
                tracing::info!("[AUTH DIAG] GET /api-keys -> HTTP {} body={}", status, &body[..body.len().min(500)]);
                if status.is_success() {
                    results.push(format!("L2 auth: OK ({})", status));
                } else {
                    results.push(format!("L2 auth: FAILED HTTP {} - {}", status, &body[..body.len().min(200)]));
                }
            }
            Err(e) => {
                tracing::error!("[AUTH DIAG] GET /api-keys -> FAILED: {:?}", e);
                results.push(format!("L2 auth: FAILED ({})", e));
            }
        }

        // Step 3: Try GET /orders (another authenticated endpoint)
        tracing::info!("[AUTH DIAG] Step 3: Testing L2 auth (GET /orders)...");
        let path = "/orders";
        let url = format!("{}{}", CLOB_API_BASE, path);
        let builder = self.client.get(&url);
        let builder = self.add_auth_headers(builder, "GET", path, "")?;
        let resp = builder.send().await;
        match resp {
            Ok(r) => {
                let status = r.status();
                let body = r.text().await.unwrap_or_default();
                tracing::info!("[AUTH DIAG] GET /orders -> HTTP {} body={}", status, &body[..body.len().min(500)]);
                if status.is_success() {
                    results.push(format!("GET /orders: OK ({})", status));
                } else {
                    results.push(format!("GET /orders: FAILED HTTP {} - {}", status, &body[..body.len().min(200)]));
                }
            }
            Err(e) => {
                tracing::error!("[AUTH DIAG] GET /orders -> FAILED: {:?}", e);
                results.push(format!("GET /orders: FAILED ({})", e));
            }
        }

        let summary = results.join(" | ");
        tracing::info!("[AUTH DIAG] Summary: {}", &summary);
        tracing::info!(
            "[AUTH DIAG] Credentials: api_key={}...{}, address={}, secret_len={}, passphrase_len={}",
            &creds.api_key[..creds.api_key.len().min(8)],
            &creds.api_key[creds.api_key.len().saturating_sub(4)..],
            &creds.wallet_address,
            creds.secret.len(),
            creds.passphrase.len(),
        );

        if summary.contains("L2 auth: FAILED") {
            anyhow::bail!("Auth diagnostic failed: {}", summary);
        }

        Ok(summary)
    }

    /// Get the current order book for a token
    pub async fn get_order_book(&self, token_id: &str) -> Result<OrderBookResponse> {
        let url = format!("{}/book?token_id={}", CLOB_API_BASE, token_id);

        let response = self.client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch order book")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Order book request failed: {} - {}", status, text);
        }

        response.json().await.context("Failed to parse order book response")
    }

    /// Get the midpoint price for a token
    pub async fn get_midpoint(&self, token_id: &str) -> Result<Option<f64>> {
        let url = format!("{}/midpoint?token_id={}", CLOB_API_BASE, token_id);

        let response = self.client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch midpoint")?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let resp: MidpointResponse = response.json().await?;
        Ok(resp.mid.and_then(|m| m.parse().ok()))
    }

    /// Place an order on Polymarket
    pub async fn place_order(&self, order: OrderRequest) -> Result<OrderResponse> {
        if self.credentials.is_none() {
            anyhow::bail!("Cannot place orders without API credentials");
        }

        let path = "/order";
        let url = format!("{}{}", CLOB_API_BASE, path);
        let body = serde_json::to_string(&order)?;

        tracing::info!(
            "Placing order: {} {} @ {} (size: {})",
            order.side,
            order.token_id[..20.min(order.token_id.len())].to_string() + "...",
            order.price,
            order.size
        );

        let builder = self.client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(body.clone());

        let builder = self.add_auth_headers(builder, "POST", path, &body)?;

        tracing::debug!("Order request body: {}", &body);

        let response = builder
            .send()
            .await
            .context("Failed to send order request")?;

        let status = response.status();
        let resp_headers = response.headers().clone();
        let response_text = response.text().await.unwrap_or_default();

        tracing::info!("Order response: HTTP {} - {}", status, &response_text[..response_text.len().min(500)]);
        if let Some(req_id) = resp_headers.get("x-request-id") {
            tracing::info!("Order x-request-id: {:?}", req_id);
        }

        if !status.is_success() {
            // Try to parse error response
            if let Ok(mut err_resp) = serde_json::from_str::<OrderResponse>(&response_text) {
                err_resp.http_status = Some(status.as_u16());
                err_resp.raw_body = Some(response_text);
                return Ok(err_resp);
            }
            anyhow::bail!("Order request failed: {} - {}", status, response_text);
        }

        let mut resp: OrderResponse = serde_json::from_str(&response_text)
            .context("Failed to parse order response")?;
        resp.http_status = Some(status.as_u16());
        resp.raw_body = Some(response_text);
        Ok(resp)
    }

    /// Cancel an order
    pub async fn cancel_order(&self, order_id: &str) -> Result<bool> {
        if self.credentials.is_none() {
            anyhow::bail!("Cannot cancel orders without API credentials");
        }

        let path = "/order";
        let url = format!("{}{}", CLOB_API_BASE, path);

        #[derive(Serialize)]
        struct CancelRequest<'a> {
            #[serde(rename = "orderID")]
            order_id: &'a str,
        }

        let body = serde_json::to_string(&CancelRequest { order_id })?;

        let builder = self.client
            .delete(&url)
            .header("Content-Type", "application/json")
            .body(body.clone());

        let builder = self.add_auth_headers(builder, "DELETE", path, &body)?;

        let response = builder
            .send()
            .await
            .context("Failed to send cancel request")?;

        Ok(response.status().is_success())
    }

    /// Get open orders
    pub async fn get_open_orders(&self) -> Result<Vec<serde_json::Value>> {
        if self.credentials.is_none() {
            anyhow::bail!("Cannot get orders without API credentials");
        }

        let path = "/orders";
        let url = format!("{}{}", CLOB_API_BASE, path);

        let builder = self.client.get(&url);
        let builder = self.add_auth_headers(builder, "GET", path, "")?;

        let response = builder
            .send()
            .await
            .context("Failed to fetch open orders")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Get orders failed: {} - {}", status, text);
        }

        response.json().await.context("Failed to parse orders response")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_serialization() {
        let order = OrderRequest {
            token_id: "123456".to_string(),
            price: "0.65".to_string(),
            size: "10".to_string(),
            side: "BUY".to_string(),
            order_type: "GTC".to_string(),
            expiration: None,
        };

        let json = serde_json::to_string(&order).unwrap();
        assert!(json.contains("tokenId"));
        assert!(json.contains("\"price\":\"0.65\""));
    }

    #[tokio::test]
    async fn test_get_order_book_no_auth() {
        // This should work without credentials (public endpoint)
        let client = ClobClient::new(None);
        // Note: Would need a valid token ID to actually test
        // let result = client.get_order_book("some_token_id").await;
    }
}
