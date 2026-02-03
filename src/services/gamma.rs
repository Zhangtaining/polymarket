use anyhow::{Context, Result};
use serde::Deserialize;

const GAMMA_API_BASE: &str = "https://gamma-api.polymarket.com";
const FIFTEEN_MINUTES_SECS: i64 = 900;

#[derive(Debug, Clone, Deserialize)]
pub struct GammaEvent {
    pub id: String,
    pub ticker: String,
    pub slug: String,
    pub title: String,
    pub active: bool,
    pub closed: bool,
    pub markets: Vec<GammaMarket>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GammaMarket {
    pub id: String,
    pub question: String,
    pub condition_id: String,
    pub slug: String,
    pub end_date: String,
    pub active: bool,
    pub closed: bool,
    pub outcomes: Option<String>,
    pub outcome_prices: Option<String>,
    pub clob_token_ids: Option<String>,
    pub best_bid: Option<f64>,
    pub best_ask: Option<f64>,
    pub accepting_orders: Option<bool>,
    pub events: Option<Vec<GammaEventInfo>>,
    pub event_start_time: Option<String>,  // When the 15-min window starts (e.g., "2026-02-01T15:30:00Z")
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GammaEventInfo {
    pub id: String,
    pub start_time: Option<String>,  // When the 15-min window starts
}

#[derive(Debug, Clone)]
pub struct MarketTokens {
    pub up_token_id: String,
    pub down_token_id: String,
    pub condition_id: String,
    pub market_id: String,
    pub slug: String,         // Market slug (e.g., "btc-updown-15m-1769961600")
    pub title: String,
    pub start_time: String,   // When the 15-min window starts
    pub end_date: String,     // When the 15-min window ends
}

pub struct GammaClient {
    client: reqwest::Client,
    coin_slug_prefix: String,
}

impl GammaClient {
    pub fn new(_btc_15m_event_id: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            coin_slug_prefix: "btc-updown-15m".to_string(),
        }
    }

    /// Calculate the current 15-minute window timestamp (rounded down)
    fn get_current_window_timestamp() -> i64 {
        let now = chrono::Utc::now().timestamp();
        (now / FIFTEEN_MINUTES_SECS) * FIFTEEN_MINUTES_SECS
    }

    /// Fetch market by slug
    async fn get_market_by_slug(&self, slug: &str) -> Result<Option<GammaMarket>> {
        let url = format!("{}/markets/slug/{}", GAMMA_API_BASE, slug);

        tracing::debug!("Fetching market from: {}", url);

        let response = self.client.get(&url).send().await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            anyhow::bail!("Gamma API returned status: {}", response.status());
        }

        let market: GammaMarket = response.json().await?;
        Ok(Some(market))
    }

    /// Fetch the current active BTC 15-minute market tokens
    /// Tries current window, then next window, then previous window
    pub async fn get_current_btc_15m_market(&self) -> Result<MarketTokens> {
        let current_ts = Self::get_current_window_timestamp();

        // Try current, next, and previous windows
        let timestamps = [
            current_ts,
            current_ts + FIFTEEN_MINUTES_SECS,
            current_ts - FIFTEEN_MINUTES_SECS,
        ];

        for ts in timestamps {
            let slug = format!("{}-{}", self.coin_slug_prefix, ts);
            tracing::info!("Trying BTC 15M market slug: {}", slug);

            match self.get_market_by_slug(&slug).await {
                Ok(Some(market)) => {
                    // Check if market is accepting orders
                    if market.accepting_orders.unwrap_or(false) && !market.closed {
                        return self.parse_market_tokens(&market);
                    }
                    tracing::debug!(
                        "Market {} not accepting orders (accepting={:?}, closed={})",
                        slug,
                        market.accepting_orders,
                        market.closed
                    );
                }
                Ok(None) => {
                    tracing::debug!("Market {} not found", slug);
                }
                Err(e) => {
                    tracing::warn!("Error fetching market {}: {:?}", slug, e);
                }
            }
        }

        anyhow::bail!(
            "No active BTC 15M market found (tried timestamps: {:?})",
            timestamps
        )
    }

    fn parse_market_tokens(&self, market: &GammaMarket) -> Result<MarketTokens> {
        let clob_token_ids = market
            .clob_token_ids
            .as_ref()
            .context("Market has no clobTokenIds")?;

        // Parse the token IDs - they come as JSON string like "[\"token1\", \"token2\"]"
        let token_ids: Vec<String> = serde_json::from_str(clob_token_ids)
            .context("Failed to parse clobTokenIds")?;

        if token_ids.len() < 2 {
            anyhow::bail!("Expected at least 2 token IDs, got {}", token_ids.len());
        }

        // Get start_time - prefer eventStartTime at market level, then events array, then end_date
        let start_time = market
            .event_start_time
            .clone()
            .or_else(|| {
                market.events
                    .as_ref()
                    .and_then(|events| events.first())
                    .and_then(|e| e.start_time.clone())
            })
            .unwrap_or_else(|| market.end_date.clone()); // Fallback to end_date

        // First token is "Up", second is "Down"
        Ok(MarketTokens {
            up_token_id: token_ids[0].clone(),
            down_token_id: token_ids[1].clone(),
            condition_id: market.condition_id.clone(),
            market_id: market.id.clone(),
            slug: market.slug.clone(),
            title: market.question.clone(),
            start_time,
            end_date: market.end_date.clone(),
        })
    }

    /// Check if the current market has changed (new 15-min window)
    pub async fn check_for_new_market(&self, current_condition_id: &str) -> Result<Option<MarketTokens>> {
        let tokens = self.get_current_btc_15m_market().await?;

        if tokens.condition_id != current_condition_id {
            tracing::info!(
                "New BTC 15M market detected: {} -> {}",
                current_condition_id,
                tokens.condition_id
            );
            Ok(Some(tokens))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_btc_15m_market() {
        let client = GammaClient::new("unused".to_string());
        let result = client.get_current_btc_15m_market().await;

        match result {
            Ok(tokens) => {
                println!("Up token: {}", tokens.up_token_id);
                println!("Down token: {}", tokens.down_token_id);
                println!("Condition ID: {}", tokens.condition_id);
                println!("Title: {}", tokens.title);
                println!("End date: {}", tokens.end_date);
                assert!(!tokens.up_token_id.is_empty());
                assert!(!tokens.down_token_id.is_empty());
            }
            Err(e) => {
                println!("Error (may be expected if no active market): {:?}", e);
            }
        }
    }

    #[test]
    fn test_window_timestamp() {
        let ts = GammaClient::get_current_window_timestamp();
        // Should be divisible by 900 (15 minutes)
        assert_eq!(ts % FIFTEEN_MINUTES_SECS, 0);
        println!("Current window timestamp: {}", ts);
    }
}
