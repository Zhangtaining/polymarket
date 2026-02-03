use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const RTDS_WS_URL: &str = "wss://ws-live-data.polymarket.com";

#[derive(Debug, Clone, Serialize)]
struct SubscribeMessage {
    action: String,
    subscriptions: Vec<Subscription>,
}

#[derive(Debug, Clone, Serialize)]
struct Subscription {
    topic: String,
    #[serde(rename = "type")]
    sub_type: String,
    filters: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RtdsMessage {
    topic: Option<String>,
    #[serde(rename = "type")]
    msg_type: Option<String>,
    timestamp: Option<i64>,
    payload: Option<ChainlinkPayload>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChainlinkPayload {
    symbol: Option<String>,
    timestamp: Option<i64>,
    value: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct ChainlinkPriceState {
    pub btc_price: Option<f64>,
    pub timestamp_ms: i64,
}

pub struct ChainlinkService {
    price_state: Arc<RwLock<ChainlinkPriceState>>,
    running: Arc<RwLock<bool>>,
}

impl ChainlinkService {
    pub fn new() -> Self {
        Self {
            price_state: Arc::new(RwLock::new(ChainlinkPriceState::default())),
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Get the current Chainlink BTC/USD price
    pub fn get_btc_price(&self) -> Option<f64> {
        self.price_state.read().btc_price
    }

    /// Get the current price state
    pub fn get_price_state(&self) -> ChainlinkPriceState {
        self.price_state.read().clone()
    }

    pub async fn start(&self) -> Result<()> {
        *self.running.write() = true;

        loop {
            if !*self.running.read() {
                break;
            }

            if let Err(e) = self.run_connection().await {
                tracing::error!("Chainlink RTDS connection error: {:?}, reconnecting...", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }

        Ok(())
    }

    async fn run_connection(&self) -> Result<()> {
        tracing::info!("Connecting to Polymarket RTDS for Chainlink prices...");

        let (ws_stream, _) = connect_async(RTDS_WS_URL)
            .await
            .context("Failed to connect to RTDS WebSocket")?;

        let (mut write, mut read) = ws_stream.split();

        // Subscribe to Chainlink BTC/USD prices
        let subscribe_msg = SubscribeMessage {
            action: "subscribe".to_string(),
            subscriptions: vec![Subscription {
                topic: "crypto_prices_chainlink".to_string(),
                sub_type: "*".to_string(),
                filters: r#"{"symbol":"btc/usd"}"#.to_string(),
            }],
        };

        let msg_str = serde_json::to_string(&subscribe_msg)?;
        write.send(Message::Text(msg_str)).await?;
        tracing::info!("Subscribed to Chainlink BTC/USD prices");

        // Ping interval for keepalive
        let mut ping_interval = tokio::time::interval(Duration::from_secs(5));

        loop {
            tokio::select! {
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            self.handle_message(&text);
                        }
                        Some(Ok(Message::Ping(data))) => {
                            if let Err(e) = write.send(Message::Pong(data)).await {
                                tracing::error!("Failed to send pong: {:?}", e);
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            tracing::warn!("RTDS WebSocket closed");
                            break;
                        }
                        Some(Err(e)) => {
                            tracing::error!("RTDS WebSocket error: {:?}", e);
                            break;
                        }
                        None => break,
                        _ => {}
                    }
                }
                _ = ping_interval.tick() => {
                    if let Err(e) = write.send(Message::Ping(vec![])).await {
                        tracing::error!("Failed to send ping: {:?}", e);
                        break;
                    }
                }
            }

            if !*self.running.read() {
                break;
            }
        }

        Ok(())
    }

    fn handle_message(&self, text: &str) {
        if let Ok(msg) = serde_json::from_str::<RtdsMessage>(text) {
            if let Some(payload) = msg.payload {
                if let Some(price) = payload.value {
                    let mut state = self.price_state.write();
                    state.btc_price = Some(price);
                    state.timestamp_ms = payload.timestamp.unwrap_or_else(|| {
                        chrono::Utc::now().timestamp_millis()
                    });

                    tracing::debug!("Chainlink BTC/USD: ${:.2}", price);
                }
            }
        }
    }

    pub fn stop(&self) {
        *self.running.write() = false;
    }
}

impl Default for ChainlinkService {
    fn default() -> Self {
        Self::new()
    }
}
