use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::config::PolymarketConfig;
use crate::events::PolymarketQuote;

#[derive(Debug, Clone, Serialize)]
struct SubscribeMessage {
    #[serde(rename = "type")]
    msg_type: String,
    assets_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct MarketMessage {
    #[serde(rename = "asset_id")]
    asset_id: Option<String>,
    market: Option<String>,
    #[serde(rename = "event_type")]
    event_type: Option<String>,
    // Best bid/ask from book updates
    bids: Option<Vec<OrderBookLevel>>,
    asks: Option<Vec<OrderBookLevel>>,
    // Price tick updates
    price: Option<String>,
    side: Option<String>,
    size: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OrderBookLevel {
    price: String,
    size: String,
}

#[derive(Debug, Clone, Default)]
pub struct QuoteState {
    pub yes_bid: Option<f64>,
    pub yes_bid_size: Option<f64>,
    pub yes_ask: Option<f64>,
    pub yes_ask_size: Option<f64>,
    pub no_bid: Option<f64>,
    pub no_bid_size: Option<f64>,
    pub no_ask: Option<f64>,
    pub no_ask_size: Option<f64>,
    pub last_update_ms: i64,
}

pub struct PolymarketService {
    config: PolymarketConfig,
    quote_state: Arc<RwLock<QuoteState>>,
    update_tx: broadcast::Sender<PolymarketQuote>,
    running: Arc<RwLock<bool>>,
}

impl PolymarketService {
    pub fn new(config: PolymarketConfig) -> Self {
        let (tx, _) = broadcast::channel(1000);
        Self {
            config,
            quote_state: Arc::new(RwLock::new(QuoteState::default())),
            update_tx: tx,
            running: Arc::new(RwLock::new(false)),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PolymarketQuote> {
        self.update_tx.subscribe()
    }

    pub fn get_quote_state(&self) -> QuoteState {
        self.quote_state.read().clone()
    }

    pub fn get_staleness_ms(&self) -> i64 {
        let state = self.quote_state.read();
        if state.last_update_ms == 0 {
            return i64::MAX;
        }
        chrono::Utc::now().timestamp_millis() - state.last_update_ms
    }

    pub async fn start(&self) -> Result<()> {
        *self.running.write() = true;

        loop {
            if !*self.running.read() {
                break;
            }

            if let Err(e) = self.run_connection().await {
                tracing::error!("Polymarket connection error: {:?}, reconnecting...", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }

        Ok(())
    }

    async fn run_connection(&self) -> Result<()> {
        tracing::info!("Connecting to Polymarket WebSocket...");

        let (ws_stream, _) = connect_async(&self.config.ws_url)
            .await
            .context("Failed to connect to Polymarket WS")?;

        let (mut write, mut read) = ws_stream.split();

        // Subscribe to both YES and NO token markets
        let subscribe_msg = SubscribeMessage {
            msg_type: "subscribe".to_string(),
            assets_ids: vec![
                self.config.yes_token_id.clone(),
                self.config.no_token_id.clone(),
            ],
        };

        let msg_str = serde_json::to_string(&subscribe_msg)?;
        write.send(Message::Text(msg_str)).await?;

        tracing::info!("Subscribed to Polymarket markets");

        while let Some(msg) = read.next().await {
            if !*self.running.read() {
                break;
            }

            match msg {
                Ok(Message::Text(text)) => {
                    self.handle_message(&text);
                }
                Ok(Message::Ping(data)) => {
                    if let Err(e) = write.send(Message::Pong(data)).await {
                        tracing::error!("Failed to send pong: {:?}", e);
                    }
                }
                Ok(Message::Close(_)) => {
                    tracing::warn!("Polymarket WebSocket closed");
                    break;
                }
                Err(e) => {
                    tracing::error!("WebSocket error: {:?}", e);
                    break;
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn handle_message(&self, text: &str) {
        // Try parsing as market message
        if let Ok(msg) = serde_json::from_str::<MarketMessage>(text) {
            self.process_market_message(msg);
        } else {
            // Log unrecognized message for debugging
            tracing::debug!("Unrecognized message: {}", text);
        }
    }

    fn process_market_message(&self, msg: MarketMessage) {
        let now = chrono::Utc::now().timestamp_millis();
        let mut state = self.quote_state.write();

        // Determine if this is YES or NO token
        let is_yes = msg.asset_id.as_ref() == Some(&self.config.yes_token_id);
        let is_no = msg.asset_id.as_ref() == Some(&self.config.no_token_id);

        if !is_yes && !is_no {
            return;
        }

        state.last_update_ms = now;

        // Update from order book levels
        if let Some(bids) = &msg.bids {
            if let Some(best) = bids.first() {
                let price = best.price.parse().ok();
                let size = best.size.parse().ok();
                if is_yes {
                    state.yes_bid = price;
                    state.yes_bid_size = size;
                } else {
                    state.no_bid = price;
                    state.no_bid_size = size;
                }
            }
        }

        if let Some(asks) = &msg.asks {
            if let Some(best) = asks.first() {
                let price = best.price.parse().ok();
                let size = best.size.parse().ok();
                if is_yes {
                    state.yes_ask = price;
                    state.yes_ask_size = size;
                } else {
                    state.no_ask = price;
                    state.no_ask_size = size;
                }
            }
        }

        // Emit update
        let quote = PolymarketQuote {
            token_id: msg.asset_id.unwrap_or_default(),
            side: if is_yes { "YES".to_string() } else { "NO".to_string() },
            best_bid: if is_yes { state.yes_bid } else { state.no_bid },
            best_bid_size: if is_yes { state.yes_bid_size } else { state.no_bid_size },
            best_ask: if is_yes { state.yes_ask } else { state.no_ask },
            best_ask_size: if is_yes { state.yes_ask_size } else { state.no_ask_size },
            t_recv_ms: now,
        };

        drop(state);
        let _ = self.update_tx.send(quote);
    }

    pub fn stop(&self) {
        *self.running.write() = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quote_state_default() {
        let state = QuoteState::default();
        assert!(state.yes_bid.is_none());
        assert!(state.no_bid.is_none());
        assert_eq!(state.last_update_ms, 0);
    }
}
