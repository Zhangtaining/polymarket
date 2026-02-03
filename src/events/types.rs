use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEvent {
    pub t_recv_ms: i64,
    // Binance data
    pub binance_mid: Option<f64>,
    pub binance_best_bid: Option<f64>,
    pub binance_best_ask: Option<f64>,
    pub binance_ret_1s: Option<f64>,
    pub binance_ret_3s: Option<f64>,
    pub binance_ret_10s: Option<f64>,
    pub binance_obi_top5: Option<f64>,
    pub binance_std_5m: Option<f64>,      // 5-minute price std dev
    // Polymarket data
    pub poly_yes_bid: Option<f64>,
    pub poly_yes_ask: Option<f64>,
    pub poly_no_bid: Option<f64>,
    pub poly_no_ask: Option<f64>,
    pub poly_spread_yes: Option<f64>,
    pub poly_spread_no: Option<f64>,
    pub poly_stale_ms: Option<i64>,
    pub poly_target_price: Option<f64>,   // BTC price at window start
    pub poly_remaining_secs: Option<i64>, // Seconds until window ends
    // Signal
    pub signal_side: String,
    pub signal_score: f64,
}

impl Default for SnapshotEvent {
    fn default() -> Self {
        Self {
            t_recv_ms: chrono::Utc::now().timestamp_millis(),
            binance_mid: None,
            binance_best_bid: None,
            binance_best_ask: None,
            binance_ret_1s: None,
            binance_ret_3s: None,
            binance_ret_10s: None,
            binance_obi_top5: None,
            binance_std_5m: None,
            poly_yes_bid: None,
            poly_yes_ask: None,
            poly_no_bid: None,
            poly_no_ask: None,
            poly_spread_yes: None,
            poly_spread_no: None,
            poly_stale_ms: None,
            poly_target_price: None,
            poly_remaining_secs: None,
            signal_side: "NONE".to_string(),
            signal_score: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeEvent {
    pub t_send_ms: i64,
    pub t_resp_ms: Option<i64>,
    pub client_order_id: String,
    pub side: String,
    pub size: f64,
    pub limit_price: f64,
    pub post_only: bool,
    pub mode: String,
    pub risk_reject_reason: Option<String>,
    pub api_status: Option<String>,
    pub fills: Option<Vec<FillInfo>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillInfo {
    pub price: f64,
    pub size: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthEvent {
    pub t_recv_ms: i64,
    pub event_type: String,
    pub message: String,
    pub component: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalEvent {
    pub t_recv_ms: i64,
    pub suggested_side: String,
    pub confidence: f64,
    pub reasons: Vec<String>,
    pub binance_ret_1s: f64,
    pub binance_ret_3s: f64,
    pub poly_lag_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinanceBookUpdate {
    pub best_bid: Decimal,
    pub best_bid_qty: Decimal,
    pub best_ask: Decimal,
    pub best_ask_qty: Decimal,
    pub mid: Decimal,
    pub imbalance_top5: f64,
    pub update_id: u64,
    pub t_recv_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolymarketQuote {
    pub token_id: String,
    pub side: String, // "YES" or "NO"
    pub best_bid: Option<f64>,
    pub best_bid_size: Option<f64>,
    pub best_ask: Option<f64>,
    pub best_ask_size: Option<f64>,
    pub t_recv_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeSide {
    Yes,
    No,
}

impl std::fmt::Display for TradeSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TradeSide::Yes => write!(f, "YES"),
            TradeSide::No => write!(f, "NO"),
        }
    }
}
