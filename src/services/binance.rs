use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::config::BinanceConfig;
use crate::events::BinanceBookUpdate;

#[derive(Debug, Clone, Deserialize)]
struct DepthSnapshot {
    #[serde(rename = "lastUpdateId")]
    last_update_id: u64,
    bids: Vec<(String, String)>,
    asks: Vec<(String, String)>,
}

// Spot API depth diff format
#[derive(Debug, Clone, Deserialize)]
struct DepthDiff {
    #[serde(rename = "e")]
    event_type: String,
    #[serde(rename = "E")]
    event_time: u64,
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "U")]
    first_update_id: u64,
    #[serde(rename = "u")]
    final_update_id: u64,
    // pu is only present in futures API, optional for spot
    #[serde(rename = "pu", default)]
    prev_final_update_id: Option<u64>,
    #[serde(rename = "b")]
    bids: Vec<(String, String)>,
    #[serde(rename = "a")]
    asks: Vec<(String, String)>,
}

#[derive(Debug)]
struct OrderBook {
    bids: BTreeMap<Decimal, Decimal>, // price -> qty (descending by price)
    asks: BTreeMap<Decimal, Decimal>, // price -> qty (ascending by price)
    last_update_id: u64,
    initialized: bool,
}

impl OrderBook {
    fn new() -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            last_update_id: 0,
            initialized: false,
        }
    }

    fn apply_snapshot(&mut self, snapshot: &DepthSnapshot) -> Result<()> {
        self.bids.clear();
        self.asks.clear();

        for (price_str, qty_str) in &snapshot.bids {
            let price: Decimal = price_str.parse()?;
            let qty: Decimal = qty_str.parse()?;
            if qty > Decimal::ZERO {
                self.bids.insert(price, qty);
            }
        }

        for (price_str, qty_str) in &snapshot.asks {
            let price: Decimal = price_str.parse()?;
            let qty: Decimal = qty_str.parse()?;
            if qty > Decimal::ZERO {
                self.asks.insert(price, qty);
            }
        }

        self.last_update_id = snapshot.last_update_id;
        self.initialized = true;
        Ok(())
    }

    fn apply_diff(&mut self, diff: &DepthDiff) -> Result<bool> {
        if !self.initialized {
            return Ok(false);
        }

        // Check sequence - different logic for spot vs futures
        // For spot API: first_update_id <= last_update_id + 1 AND final_update_id >= last_update_id + 1
        // For futures API: prev_final_update_id == last_update_id
        if let Some(pu) = diff.prev_final_update_id {
            // Futures API sequence check
            if pu != self.last_update_id {
                return Ok(false);
            }
        } else {
            // Spot API sequence check
            if diff.first_update_id > self.last_update_id + 1 {
                // Gap detected
                return Ok(false);
            }
            if diff.final_update_id < self.last_update_id + 1 {
                // Already processed
                return Ok(true);
            }
        }

        for (price_str, qty_str) in &diff.bids {
            let price: Decimal = price_str.parse()?;
            let qty: Decimal = qty_str.parse()?;
            if qty == Decimal::ZERO {
                self.bids.remove(&price);
            } else {
                self.bids.insert(price, qty);
            }
        }

        for (price_str, qty_str) in &diff.asks {
            let price: Decimal = price_str.parse()?;
            let qty: Decimal = qty_str.parse()?;
            if qty == Decimal::ZERO {
                self.asks.remove(&price);
            } else {
                self.asks.insert(price, qty);
            }
        }

        self.last_update_id = diff.final_update_id;
        Ok(true)
    }

    fn best_bid(&self) -> Option<(Decimal, Decimal)> {
        self.bids.iter().next_back().map(|(p, q)| (*p, *q))
    }

    fn best_ask(&self) -> Option<(Decimal, Decimal)> {
        self.asks.iter().next().map(|(p, q)| (*p, *q))
    }

    fn mid(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some((bid, _)), Some((ask, _))) => Some((bid + ask) / Decimal::from(2)),
            _ => None,
        }
    }

    fn imbalance_top_n(&self, n: usize) -> f64 {
        let bid_sum: Decimal = self.bids.iter().rev().take(n).map(|(_, q)| *q).sum();
        let ask_sum: Decimal = self.asks.iter().take(n).map(|(_, q)| *q).sum();

        let total = bid_sum + ask_sum;
        if total == Decimal::ZERO {
            return 0.0;
        }

        let imbalance = (bid_sum - ask_sum) / total;
        imbalance.to_string().parse().unwrap_or(0.0)
    }
}

pub struct BinanceBookService {
    config: BinanceConfig,
    book: Arc<RwLock<OrderBook>>,
    mid_history: Arc<RwLock<VecDeque<(i64, Decimal)>>>,
    update_tx: broadcast::Sender<BinanceBookUpdate>,
    running: Arc<RwLock<bool>>,
}

impl BinanceBookService {
    pub fn new(config: BinanceConfig) -> Self {
        let (tx, _) = broadcast::channel(1000);
        Self {
            config,
            book: Arc::new(RwLock::new(OrderBook::new())),
            mid_history: Arc::new(RwLock::new(VecDeque::with_capacity(1000))),
            update_tx: tx,
            running: Arc::new(RwLock::new(false)),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<BinanceBookUpdate> {
        self.update_tx.subscribe()
    }

    pub fn get_current_update(&self) -> Option<BinanceBookUpdate> {
        let book = self.book.read();
        if !book.initialized {
            return None;
        }

        let (best_bid, best_bid_qty) = book.best_bid()?;
        let (best_ask, best_ask_qty) = book.best_ask()?;
        let mid = book.mid()?;
        let imbalance = book.imbalance_top_n(5);

        Some(BinanceBookUpdate {
            best_bid,
            best_bid_qty,
            best_ask,
            best_ask_qty,
            mid,
            imbalance_top5: imbalance,
            update_id: book.last_update_id,
            t_recv_ms: chrono::Utc::now().timestamp_millis(),
        })
    }

    pub fn get_returns(&self, lookback_ms: i64) -> Option<f64> {
        let history = self.mid_history.read();
        if history.len() < 2 {
            return None;
        }

        let now = chrono::Utc::now().timestamp_millis();
        let cutoff = now - lookback_ms;

        // Find the oldest price within lookback window
        let old_price = history
            .iter()
            .find(|(ts, _)| *ts >= cutoff)
            .map(|(_, p)| *p)?;

        let current_price = history.back().map(|(_, p)| *p)?;

        if old_price == Decimal::ZERO {
            return None;
        }

        let ret = (current_price - old_price) / old_price;
        ret.to_string().parse().ok()
    }

    /// Calculate standard deviation of prices over the lookback period
    pub fn get_std_dev(&self, lookback_ms: i64) -> Option<f64> {
        let history = self.mid_history.read();
        if history.len() < 2 {
            return None;
        }

        let now = chrono::Utc::now().timestamp_millis();
        let cutoff = now - lookback_ms;

        // Collect prices within lookback window
        let prices: Vec<f64> = history
            .iter()
            .filter(|(ts, _)| *ts >= cutoff)
            .filter_map(|(_, p)| p.to_string().parse::<f64>().ok())
            .collect();

        if prices.len() < 2 {
            return None;
        }

        // Calculate mean
        let mean: f64 = prices.iter().sum::<f64>() / prices.len() as f64;

        // Calculate variance
        let variance: f64 = prices
            .iter()
            .map(|p| (p - mean).powi(2))
            .sum::<f64>()
            / (prices.len() - 1) as f64; // Sample std dev

        Some(variance.sqrt())
    }

    /// Get the current mid price
    pub fn get_mid_price(&self) -> Option<f64> {
        let book = self.book.read();
        book.mid().and_then(|m| m.to_string().parse().ok())
    }

    async fn fetch_snapshot(&self) -> Result<DepthSnapshot> {
        let url = format!(
            "{}?symbol={}&limit={}",
            self.config.rest_url, self.config.symbol, self.config.snapshot_limit
        );

        let client = reqwest::Client::new();
        let snapshot: DepthSnapshot = client.get(&url).send().await?.json().await?;
        Ok(snapshot)
    }

    pub async fn start(&self) -> Result<()> {
        *self.running.write() = true;

        loop {
            if !*self.running.read() {
                break;
            }

            if let Err(e) = self.run_connection().await {
                tracing::error!("Binance connection error: {:?}, reconnecting...", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }

        Ok(())
    }

    async fn run_connection(&self) -> Result<()> {
        tracing::info!("Connecting to Binance WebSocket...");

        // Connect to WebSocket
        let (ws_stream, _) = connect_async(&self.config.ws_url)
            .await
            .context("Failed to connect to Binance WS")?;

        let (mut write, mut read) = ws_stream.split();

        // Buffer messages while fetching snapshot (not currently used but kept for future diff buffering)
        let _buffer: Vec<DepthDiff> = Vec::new();

        // Fetch REST snapshot
        let snapshot = self.fetch_snapshot().await?;
        tracing::info!("Fetched Binance snapshot, lastUpdateId: {}", snapshot.last_update_id);

        // Apply snapshot
        {
            let mut book = self.book.write();
            book.apply_snapshot(&snapshot)?;
        }

        // Process buffered and incoming messages
        let mut needs_resync = false;

        while let Some(msg) = read.next().await {
            if !*self.running.read() {
                break;
            }

            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(diff) = serde_json::from_str::<DepthDiff>(&text) {
                        // Skip updates before our snapshot
                        if diff.final_update_id <= snapshot.last_update_id {
                            continue;
                        }

                        let mut book = self.book.write();
                        match book.apply_diff(&diff) {
                            Ok(true) => {
                                // Update successful
                                drop(book);
                                self.record_mid();
                                self.emit_update();
                            }
                            Ok(false) => {
                                // Need resync
                                tracing::warn!("Binance sequence gap, resyncing...");
                                needs_resync = true;
                                break;
                            }
                            Err(e) => {
                                tracing::error!("Error applying diff: {:?}", e);
                            }
                        }
                    }
                }
                Ok(Message::Ping(data)) => {
                    if let Err(e) = write.send(Message::Pong(data)).await {
                        tracing::error!("Failed to send pong: {:?}", e);
                    }
                }
                Ok(Message::Close(_)) => {
                    tracing::warn!("Binance WebSocket closed");
                    break;
                }
                Err(e) => {
                    tracing::error!("WebSocket error: {:?}", e);
                    break;
                }
                _ => {}
            }
        }

        if needs_resync {
            // Clear book state
            let mut book = self.book.write();
            *book = OrderBook::new();
        }

        Ok(())
    }

    fn record_mid(&self) {
        let book = self.book.read();
        if let Some(mid) = book.mid() {
            let now = chrono::Utc::now().timestamp_millis();
            let mut history = self.mid_history.write();
            history.push_back((now, mid));

            // Keep last 60 seconds of history
            let cutoff = now - 60_000;
            while let Some((ts, _)) = history.front() {
                if *ts < cutoff {
                    history.pop_front();
                } else {
                    break;
                }
            }
        }
    }

    fn emit_update(&self) {
        if let Some(update) = self.get_current_update() {
            let _ = self.update_tx.send(update);
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
    fn test_order_book_operations() {
        let mut book = OrderBook::new();

        let snapshot = DepthSnapshot {
            last_update_id: 100,
            bids: vec![
                ("100.0".to_string(), "1.0".to_string()),
                ("99.0".to_string(), "2.0".to_string()),
            ],
            asks: vec![
                ("101.0".to_string(), "1.5".to_string()),
                ("102.0".to_string(), "2.5".to_string()),
            ],
        };

        book.apply_snapshot(&snapshot).unwrap();
        assert!(book.initialized);

        let (bid, _) = book.best_bid().unwrap();
        let (ask, _) = book.best_ask().unwrap();
        assert_eq!(bid, Decimal::from(100));
        assert_eq!(ask, Decimal::from(101));

        let mid = book.mid().unwrap();
        assert_eq!(mid, Decimal::new(1005, 1)); // 100.5
    }

    #[test]
    fn test_imbalance_calculation() {
        let mut book = OrderBook::new();

        let snapshot = DepthSnapshot {
            last_update_id: 100,
            bids: vec![
                ("100.0".to_string(), "10.0".to_string()),
                ("99.0".to_string(), "10.0".to_string()),
            ],
            asks: vec![
                ("101.0".to_string(), "5.0".to_string()),
                ("102.0".to_string(), "5.0".to_string()),
            ],
        };

        book.apply_snapshot(&snapshot).unwrap();

        let imbalance = book.imbalance_top_n(2);
        // (20 - 10) / 30 = 0.333...
        assert!((imbalance - 0.333).abs() < 0.01);
    }
}
