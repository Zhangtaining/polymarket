use crate::config::SignalConfig;
use crate::events::{SignalEvent, TradeSide};
use crate::services::{BinanceBookService, PolymarketService};
use parking_lot::RwLock;
use std::sync::Arc;
use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub struct SignalState {
    pub suggested_side: Option<TradeSide>,
    pub confidence: f64,
    pub reasons: Vec<String>,
    pub binance_ret_1s: f64,
    pub binance_ret_3s: f64,
    pub poly_lag_ms: i64,
}

impl Default for SignalState {
    fn default() -> Self {
        Self {
            suggested_side: None,
            confidence: 0.0,
            reasons: Vec::new(),
            binance_ret_1s: 0.0,
            binance_ret_3s: 0.0,
            poly_lag_ms: 0,
        }
    }
}

pub struct SignalService {
    config: SignalConfig,
    binance: Arc<BinanceBookService>,
    polymarket: Arc<PolymarketService>,
    signal_state: Arc<RwLock<SignalState>>,
    signal_tx: broadcast::Sender<SignalEvent>,
}

impl SignalService {
    pub fn new(
        config: SignalConfig,
        binance: Arc<BinanceBookService>,
        polymarket: Arc<PolymarketService>,
    ) -> Self {
        let (tx, _) = broadcast::channel(100);
        Self {
            config,
            binance,
            polymarket,
            signal_state: Arc::new(RwLock::new(SignalState::default())),
            signal_tx: tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SignalEvent> {
        self.signal_tx.subscribe()
    }

    pub fn get_signal_state(&self) -> SignalState {
        self.signal_state.read().clone()
    }

    pub fn compute_signal(&self) -> SignalState {
        let mut state = SignalState::default();
        let mut reasons = Vec::new();

        // Get Binance returns
        let ret_1s = self.binance.get_returns(1000).unwrap_or(0.0);
        let ret_3s = self.binance.get_returns(3000).unwrap_or(0.0);
        let _ret_10s = self.binance.get_returns(10000).unwrap_or(0.0);

        state.binance_ret_1s = ret_1s;
        state.binance_ret_3s = ret_3s;

        // Get Polymarket staleness
        let poly_stale_ms = self.polymarket.get_staleness_ms();
        state.poly_lag_ms = poly_stale_ms;

        // Check for significant Binance move
        let significant_up_1s = ret_1s > self.config.binance_return_threshold_1s;
        let significant_down_1s = ret_1s < -self.config.binance_return_threshold_1s;
        let significant_up_3s = ret_3s > self.config.binance_return_threshold_3s;
        let significant_down_3s = ret_3s < -self.config.binance_return_threshold_3s;

        // Check if Polymarket might be lagging
        let poly_lagging = poly_stale_ms > self.config.poly_lag_threshold_ms as i64;

        // Compute signal
        let mut score = 0.0;
        let mut suggested_side: Option<TradeSide> = None;

        // Strong signal: 1s move with poly lag
        if significant_up_1s && poly_lagging {
            score += 0.5;
            suggested_side = Some(TradeSide::Yes);
            reasons.push(format!(
                "BTC up {:.4}% in 1s, Poly lag {}ms",
                ret_1s * 100.0,
                poly_stale_ms
            ));
        } else if significant_down_1s && poly_lagging {
            score += 0.5;
            suggested_side = Some(TradeSide::No);
            reasons.push(format!(
                "BTC down {:.4}% in 1s, Poly lag {}ms",
                ret_1s.abs() * 100.0,
                poly_stale_ms
            ));
        }

        // Additional confidence from 3s confirmation
        if significant_up_3s && matches!(suggested_side, Some(TradeSide::Yes)) {
            score += 0.3;
            reasons.push(format!("3s uptrend confirms: {:.4}%", ret_3s * 100.0));
        } else if significant_down_3s && matches!(suggested_side, Some(TradeSide::No)) {
            score += 0.3;
            reasons.push(format!("3s downtrend confirms: {:.4}%", ret_3s.abs() * 100.0));
        }

        // Only signal if above threshold
        if score < self.config.min_confidence {
            suggested_side = None;
            score = 0.0;
            reasons.clear();
        }

        state.suggested_side = suggested_side;
        state.confidence = score;
        state.reasons = reasons;

        // Update internal state and emit
        *self.signal_state.write() = state.clone();

        if state.suggested_side.is_some() {
            let event = SignalEvent {
                t_recv_ms: chrono::Utc::now().timestamp_millis(),
                suggested_side: state
                    .suggested_side
                    .map(|s| s.to_string())
                    .unwrap_or("NONE".to_string()),
                confidence: state.confidence,
                reasons: state.reasons.clone(),
                binance_ret_1s: state.binance_ret_1s,
                binance_ret_3s: state.binance_ret_3s,
                poly_lag_ms: state.poly_lag_ms,
            };
            let _ = self.signal_tx.send(event);
        }

        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BinanceConfig, PolymarketConfig};

    fn make_signal_config() -> SignalConfig {
        SignalConfig {
            binance_return_threshold_1s: 0.001,
            binance_return_threshold_3s: 0.002,
            poly_lag_threshold_ms: 500,
            min_confidence: 0.5,
        }
    }

    fn make_binance_config() -> BinanceConfig {
        BinanceConfig {
            ws_url: "wss://test".to_string(),
            rest_url: "https://test".to_string(),
            symbol: "BTCUSD".to_string(),
            snapshot_limit: 100,
        }
    }

    fn make_poly_config() -> PolymarketConfig {
        PolymarketConfig {
            ws_url: "wss://test".to_string(),
            rest_url: "https://test".to_string(),
            gamma_url: "https://gamma-api.polymarket.com".to_string(),
            btc_15m_event_id: "194059".to_string(),
            yes_token_id: "yes".to_string(),
            no_token_id: "no".to_string(),
            condition_id: "cond".to_string(),
        }
    }

    #[test]
    fn test_signal_state_default() {
        let state = SignalState::default();
        assert!(state.suggested_side.is_none());
        assert_eq!(state.confidence, 0.0);
        assert!(state.reasons.is_empty());
    }

    #[test]
    fn test_signal_service_creation() {
        let binance = Arc::new(BinanceBookService::new(make_binance_config()));
        let poly = Arc::new(PolymarketService::new(make_poly_config()));
        let signal = SignalService::new(make_signal_config(), binance, poly);

        let state = signal.get_signal_state();
        assert!(state.suggested_side.is_none());
    }

    #[test]
    fn test_compute_signal_no_data() {
        let binance = Arc::new(BinanceBookService::new(make_binance_config()));
        let poly = Arc::new(PolymarketService::new(make_poly_config()));
        let signal = SignalService::new(make_signal_config(), binance, poly);

        // With no data, should return no signal
        let state = signal.compute_signal();
        assert!(state.suggested_side.is_none());
        assert_eq!(state.confidence, 0.0);
    }
}
