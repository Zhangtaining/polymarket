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
use super::gamma::{GammaClient, MarketTokens};

#[derive(Debug, Clone, Serialize)]
struct SubscribeMessage {
    #[serde(rename = "type")]
    msg_type: String,
    assets_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct BookMessage {
    asset_id: Option<String>,
    market: Option<String>,
    bids: Option<Vec<OrderBookLevel>>,
    asks: Option<Vec<OrderBookLevel>>,
}

#[derive(Debug, Clone, Deserialize)]
struct PriceChangeMessage {
    market: Option<String>,
    price_changes: Option<Vec<PriceChange>>,
}

#[derive(Debug, Clone, Deserialize)]
struct PriceChange {
    asset_id: String,
    price: Option<String>,
    side: Option<String>,
    best_bid: Option<String>,
    best_ask: Option<String>,
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

#[derive(Debug, Clone, Default)]
pub struct ActiveMarket {
    pub up_token_id: String,
    pub down_token_id: String,
    pub condition_id: String,
    pub slug: String,            // Market slug for URLs (e.g. "btc-updown-15m-1769961600")
    pub title: String,
    pub start_time: String,      // When window starts (ISO8601)
    pub end_date: String,        // When window ends (ISO8601)
    pub target_price: Option<f64>, // BTC price at window start
}

pub struct PolymarketService {
    config: PolymarketConfig,
    gamma_client: GammaClient,
    active_market: Arc<RwLock<ActiveMarket>>,
    quote_state: Arc<RwLock<QuoteState>>,
    update_tx: broadcast::Sender<PolymarketQuote>,
    running: Arc<RwLock<bool>>,
}

impl PolymarketService {
    pub fn new(config: PolymarketConfig) -> Self {
        let (tx, _) = broadcast::channel(1000);
        let gamma_client = GammaClient::new(config.btc_15m_event_id.clone());
        Self {
            config,
            gamma_client,
            active_market: Arc::new(RwLock::new(ActiveMarket::default())),
            quote_state: Arc::new(RwLock::new(QuoteState::default())),
            update_tx: tx,
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Fetch the current market tokens from Gamma API
    pub async fn refresh_market_tokens(&self) -> Result<MarketTokens> {
        let tokens = self.gamma_client.get_current_btc_15m_market().await?;

        let mut market = self.active_market.write();
        market.up_token_id = tokens.up_token_id.clone();
        market.down_token_id = tokens.down_token_id.clone();
        market.condition_id = tokens.condition_id.clone();
        market.slug = tokens.slug.clone();
        market.title = tokens.title.clone();
        market.start_time = tokens.start_time.clone();
        market.end_date = tokens.end_date.clone();
        market.target_price = None; // Will be set when we get BTC price at window start

        tracing::info!(
            "Active market: {} | Start: {} | End: {}",
            tokens.title,
            tokens.start_time,
            tokens.end_date
        );

        Ok(tokens)
    }

    /// Set the target price (BTC price at window start)
    pub fn set_target_price(&self, price: f64) {
        let mut market = self.active_market.write();
        if market.target_price.is_none() {
            market.target_price = Some(price);
            tracing::info!("Target price set: ${:.2}", price);
        }
    }

    /// Force set the target price (overwrites existing)
    pub fn force_set_target_price(&self, price: f64) {
        let mut market = self.active_market.write();
        market.target_price = Some(price);
        tracing::info!("Target price set (from scraper): ${:.2}", price);
    }

    /// Clear the target price (for new window)
    pub fn clear_target_price(&self) {
        let mut market = self.active_market.write();
        market.target_price = None;
    }

    /// Fetch the price to beat from the Polymarket website
    pub async fn fetch_price_to_beat_from_page(&self) -> Option<f64> {
        let market = self.get_active_market();
        // Use current market slug from Gamma API for the Polymarket URL
        let slug = if !market.slug.is_empty() {
            market.slug
        } else {
            // Fallback: compute from current 15-min window
            let now = chrono::Utc::now().timestamp();
            format!("btc-updown-15m-{}", (now / 900) * 900)
        };

        match super::price_scraper::fetch_price_to_beat(&slug).await {
            Ok(Some(data)) => {
                tracing::info!("Scraped price to beat: ${:.2} from {}", data.open_price, slug);
                Some(data.open_price)
            }
            Ok(None) => {
                tracing::warn!("No price data found for {}", slug);
                None
            }
            Err(e) => {
                tracing::warn!("Failed to scrape price to beat: {:?}", e);
                None
            }
        }
    }

    pub fn get_active_market(&self) -> ActiveMarket {
        self.active_market.read().clone()
    }

    /// Get remaining time in seconds until window ends
    pub fn get_remaining_secs(&self) -> Option<i64> {
        let market = self.active_market.read();
        if market.end_date.is_empty() {
            return None;
        }

        // Parse end_date (ISO8601 format)
        if let Ok(end_time) = chrono::DateTime::parse_from_rfc3339(&market.end_date) {
            let now = chrono::Utc::now();
            let remaining = end_time.signed_duration_since(now).num_seconds();
            Some(remaining.max(0))
        } else {
            None
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

        // Fetch initial market tokens
        if let Err(e) = self.refresh_market_tokens().await {
            tracing::error!("Failed to fetch initial market tokens: {:?}", e);
            return Err(e);
        }

        loop {
            if !*self.running.read() {
                break;
            }

            if let Err(e) = self.run_connection().await {
                tracing::error!("Polymarket connection error: {:?}, reconnecting...", e);
                // Refresh tokens on reconnection in case market changed
                if let Err(refresh_err) = self.refresh_market_tokens().await {
                    tracing::warn!("Failed to refresh market tokens: {:?}", refresh_err);
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }

        Ok(())
    }

    async fn run_connection(&self) -> Result<()> {
        let market = self.get_active_market();

        if market.up_token_id.is_empty() || market.down_token_id.is_empty() {
            anyhow::bail!("No active market tokens available");
        }

        tracing::info!("Connecting to Polymarket WebSocket...");

        let (ws_stream, _) = connect_async(&self.config.ws_url)
            .await
            .context("Failed to connect to Polymarket WS")?;

        let (mut write, mut read) = ws_stream.split();

        // Subscribe to both Up and Down token markets
        let subscribe_msg = SubscribeMessage {
            msg_type: "subscribe".to_string(),
            assets_ids: vec![
                market.up_token_id.clone(),
                market.down_token_id.clone(),
            ],
        };

        let msg_str = serde_json::to_string(&subscribe_msg)?;
        write.send(Message::Text(msg_str)).await?;

        tracing::info!(
            "Subscribed to Polymarket market: {} (Up: {}..., Down: {}...)",
            market.title,
            &market.up_token_id[..20.min(market.up_token_id.len())],
            &market.down_token_id[..20.min(market.down_token_id.len())]
        );

        // Track when to check for new market (every 60 seconds)
        let mut last_market_check = std::time::Instant::now();
        let market_check_interval = Duration::from_secs(60);

        while let Some(msg) = read.next().await {
            if !*self.running.read() {
                break;
            }

            // Periodically check for new market
            if last_market_check.elapsed() > market_check_interval {
                last_market_check = std::time::Instant::now();
                let current_condition = self.get_active_market().condition_id;
                match self.gamma_client.check_for_new_market(&current_condition).await {
                    Ok(Some(new_tokens)) => {
                        tracing::info!("Market changed! Reconnecting to new market...");
                        {
                            let mut active = self.active_market.write();
                            active.up_token_id = new_tokens.up_token_id;
                            active.down_token_id = new_tokens.down_token_id;
                            active.condition_id = new_tokens.condition_id;
                            active.slug = new_tokens.slug.clone();
                            active.title = new_tokens.title;
                            active.start_time = new_tokens.start_time;
                            active.end_date = new_tokens.end_date;
                            active.target_price = None; // Reset for new window
                        }
                        // Break to reconnect with new tokens
                        break;
                    }
                    Ok(None) => {
                        // Same market, continue
                    }
                    Err(e) => {
                        tracing::warn!("Failed to check for new market: {:?}", e);
                    }
                }
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
        // Try parsing as price change message first (most common)
        if let Ok(msg) = serde_json::from_str::<PriceChangeMessage>(text) {
            if msg.price_changes.is_some() {
                self.process_price_changes(msg);
                return;
            }
        }

        // Try parsing as book message array (initial subscription)
        if let Ok(msgs) = serde_json::from_str::<Vec<BookMessage>>(text) {
            for msg in msgs {
                self.process_book_message(msg);
            }
            return;
        }

        // Try parsing as single book message
        if let Ok(msg) = serde_json::from_str::<BookMessage>(text) {
            self.process_book_message(msg);
            return;
        }

        tracing::debug!("Unrecognized WS message: {}", &text[..100.min(text.len())]);
    }

    fn process_price_changes(&self, msg: PriceChangeMessage) {
        let now = chrono::Utc::now().timestamp_millis();
        let market = self.active_market.read();
        let mut state = self.quote_state.write();

        if let Some(changes) = msg.price_changes {
            for change in changes {
                let is_up = change.asset_id == market.up_token_id;
                let is_down = change.asset_id == market.down_token_id;

                if !is_up && !is_down {
                    continue;
                }

                state.last_update_ms = now;

                // Update from best_bid/best_ask in price change
                if let Some(bid) = &change.best_bid {
                    if let Ok(price) = bid.parse::<f64>() {
                        if is_up {
                            state.yes_bid = Some(price);
                        } else {
                            state.no_bid = Some(price);
                        }
                    }
                }

                if let Some(ask) = &change.best_ask {
                    if let Ok(price) = ask.parse::<f64>() {
                        if is_up {
                            state.yes_ask = Some(price);
                        } else {
                            state.no_ask = Some(price);
                        }
                    }
                }

                // Emit update
                let quote = PolymarketQuote {
                    token_id: change.asset_id.clone(),
                    side: if is_up { "UP".to_string() } else { "DOWN".to_string() },
                    best_bid: if is_up { state.yes_bid } else { state.no_bid },
                    best_bid_size: None,
                    best_ask: if is_up { state.yes_ask } else { state.no_ask },
                    best_ask_size: None,
                    t_recv_ms: now,
                };

                drop(state);
                drop(market);
                let _ = self.update_tx.send(quote);
                return;
            }
        }
    }

    fn process_book_message(&self, msg: BookMessage) {
        let now = chrono::Utc::now().timestamp_millis();
        let market = self.active_market.read();
        let mut state = self.quote_state.write();

        let asset_id = match &msg.asset_id {
            Some(id) => id,
            None => return,
        };

        let is_up = asset_id == &market.up_token_id;
        let is_down = asset_id == &market.down_token_id;
        drop(market);

        if !is_up && !is_down {
            return;
        }

        state.last_update_ms = now;

        // Get best bid (highest price) from bids sorted ascending
        if let Some(bids) = &msg.bids {
            if let Some(best) = bids.last() {
                // Bids are sorted ascending, so last is best (highest)
                if let Ok(price) = best.price.parse::<f64>() {
                    if is_up {
                        state.yes_bid = Some(price);
                        state.yes_bid_size = best.size.parse().ok();
                    } else {
                        state.no_bid = Some(price);
                        state.no_bid_size = best.size.parse().ok();
                    }
                }
            }
        }

        // Get best ask (lowest price) from asks
        if let Some(asks) = &msg.asks {
            if let Some(best) = asks.first() {
                // Take first ask (assuming sorted ascending = lowest first)
                if let Ok(price) = best.price.parse::<f64>() {
                    if is_up {
                        state.yes_ask = Some(price);
                        state.yes_ask_size = best.size.parse().ok();
                    } else {
                        state.no_ask = Some(price);
                        state.no_ask_size = best.size.parse().ok();
                    }
                }
            }
        }
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
