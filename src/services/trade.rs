use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::sync::Arc;
use uuid::Uuid;

use crate::config::TradingConfig;
use crate::events::{TradeEvent, TradeSide};
use crate::logger::JsonlLogger;
use crate::services::PolymarketService;

#[derive(Debug, Clone)]
pub struct TradingState {
    pub kill_switch_active: bool,
    pub current_size: f64,
    pub max_price_yes: f64,
    pub max_price_no: f64,
}

impl TradingState {
    pub fn new(config: &TradingConfig) -> Self {
        Self {
            kill_switch_active: false,
            current_size: config.default_size,
            max_price_yes: config.max_price_yes,
            max_price_no: config.max_price_no,
        }
    }
}

#[derive(Debug, Clone)]
pub enum RiskCheckResult {
    Allowed,
    Rejected(String),
}

pub struct TradeService {
    config: TradingConfig,
    polymarket: Arc<PolymarketService>,
    logger: Arc<JsonlLogger>,
    state: Arc<RwLock<TradingState>>,
    dry_run: bool,
}

impl TradeService {
    pub fn new(
        config: TradingConfig,
        polymarket: Arc<PolymarketService>,
        logger: Arc<JsonlLogger>,
        dry_run: bool,
    ) -> Self {
        let state = TradingState::new(&config);
        Self {
            config,
            polymarket,
            logger,
            state: Arc::new(RwLock::new(state)),
            dry_run,
        }
    }

    pub fn get_state(&self) -> TradingState {
        self.state.read().clone()
    }

    pub fn toggle_kill_switch(&self) {
        let mut state = self.state.write();
        state.kill_switch_active = !state.kill_switch_active;
        tracing::info!("Kill switch: {}", if state.kill_switch_active { "ACTIVE" } else { "OFF" });
    }

    pub fn set_kill_switch(&self, active: bool) {
        let mut state = self.state.write();
        state.kill_switch_active = active;
    }

    pub fn adjust_size(&self, delta: f64) {
        let mut state = self.state.write();
        let new_size = (state.current_size + delta).max(1.0).min(self.config.max_size);
        state.current_size = new_size;
        tracing::info!("Size adjusted to: {}", new_size);
    }

    pub fn adjust_max_price(&self, side: TradeSide, delta: f64) {
        let mut state = self.state.write();
        match side {
            TradeSide::Yes => {
                state.max_price_yes = (state.max_price_yes + delta).clamp(0.01, 0.99);
                tracing::info!("Max YES price adjusted to: {}", state.max_price_yes);
            }
            TradeSide::No => {
                state.max_price_no = (state.max_price_no + delta).clamp(0.01, 0.99);
                tracing::info!("Max NO price adjusted to: {}", state.max_price_no);
            }
        }
    }

    fn check_risk(&self, side: TradeSide, size: f64, limit_price: f64) -> RiskCheckResult {
        let state = self.state.read();

        // Kill switch check
        if state.kill_switch_active {
            return RiskCheckResult::Rejected("Kill switch is active".to_string());
        }

        // Size limit
        if size > self.config.max_size {
            return RiskCheckResult::Rejected(format!(
                "Size {} exceeds max size {}",
                size, self.config.max_size
            ));
        }

        // Get quote state
        let quotes = self.polymarket.get_quote_state();

        // Staleness check
        let stale_ms = self.polymarket.get_staleness_ms();
        if stale_ms > self.config.stale_quote_threshold_ms as i64 {
            return RiskCheckResult::Rejected(format!(
                "Quote stale by {}ms (threshold {}ms)",
                stale_ms, self.config.stale_quote_threshold_ms
            ));
        }

        // Price checks
        let (bid, ask, max_price) = match side {
            TradeSide::Yes => (quotes.yes_bid, quotes.yes_ask, state.max_price_yes),
            TradeSide::No => (quotes.no_bid, quotes.no_ask, state.max_price_no),
        };

        if limit_price > max_price {
            return RiskCheckResult::Rejected(format!(
                "Limit price {} exceeds max price {}",
                limit_price, max_price
            ));
        }

        // Spread check
        if let (Some(b), Some(a)) = (bid, ask) {
            let spread = a - b;
            if spread > self.config.max_spread {
                return RiskCheckResult::Rejected(format!(
                    "Spread {} exceeds max spread {}",
                    spread, self.config.max_spread
                ));
            }
        }

        RiskCheckResult::Allowed
    }

    pub async fn place_order(&self, side: TradeSide) -> Result<TradeEvent> {
        let t_send_ms = chrono::Utc::now().timestamp_millis();
        let client_order_id = Uuid::new_v4().to_string();
        let state = self.state.read();
        let size = state.current_size;
        let limit_price = match side {
            TradeSide::Yes => state.max_price_yes,
            TradeSide::No => state.max_price_no,
        };
        drop(state);

        // Risk check
        let risk_result = self.check_risk(side, size, limit_price);

        let mut trade_event = TradeEvent {
            t_send_ms,
            t_resp_ms: None,
            client_order_id,
            side: side.to_string(),
            size,
            limit_price,
            post_only: true,
            mode: if self.dry_run { "dry_run".to_string() } else { "live".to_string() },
            risk_reject_reason: None,
            api_status: None,
            fills: None,
        };

        match risk_result {
            RiskCheckResult::Rejected(reason) => {
                trade_event.risk_reject_reason = Some(reason.clone());
                trade_event.t_resp_ms = Some(chrono::Utc::now().timestamp_millis());
                self.logger.log_trade(trade_event.clone())?;
                return Err(anyhow!("Order rejected: {}", reason));
            }
            RiskCheckResult::Allowed => {}
        }

        if self.dry_run {
            // Dry run - just log the intent
            trade_event.api_status = Some("dry_run_success".to_string());
            trade_event.t_resp_ms = Some(chrono::Utc::now().timestamp_millis());
            self.logger.log_trade(trade_event.clone())?;
            tracing::info!(
                "[DRY RUN] Order: {} {} @ {} (size: {})",
                side,
                trade_event.client_order_id,
                limit_price,
                size
            );
            return Ok(trade_event);
        }

        // Live mode - would call Polymarket API here
        // For now, just log that live trading is not yet implemented
        trade_event.api_status = Some("live_not_implemented".to_string());
        trade_event.t_resp_ms = Some(chrono::Utc::now().timestamp_millis());
        self.logger.log_trade(trade_event.clone())?;
        tracing::warn!("[LIVE] Order placement not implemented yet");

        Ok(trade_event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PolymarketConfig;
    use tempfile::tempdir;

    fn make_test_config() -> TradingConfig {
        TradingConfig {
            default_size: 10.0,
            max_size: 100.0,
            max_price_yes: 0.95,
            max_price_no: 0.95,
            max_spread: 0.10,
            stale_quote_threshold_ms: 5000,
        }
    }

    fn make_poly_config() -> PolymarketConfig {
        PolymarketConfig {
            ws_url: "wss://test".to_string(),
            rest_url: "https://test".to_string(),
            yes_token_id: "yes".to_string(),
            no_token_id: "no".to_string(),
            condition_id: "cond".to_string(),
        }
    }

    #[test]
    fn test_trading_state_new() {
        let config = make_test_config();

        let state = TradingState::new(&config);
        assert!(!state.kill_switch_active);
        assert_eq!(state.current_size, 10.0);
        assert_eq!(state.max_price_yes, 0.95);
    }

    #[test]
    fn test_kill_switch_toggle() {
        let dir = tempdir().unwrap();
        let logger = crate::logger::JsonlLogger::new(dir.path().to_str().unwrap()).unwrap();
        let poly = Arc::new(PolymarketService::new(make_poly_config()));
        let trade = TradeService::new(make_test_config(), poly, logger, true);

        assert!(!trade.get_state().kill_switch_active);
        trade.toggle_kill_switch();
        assert!(trade.get_state().kill_switch_active);
        trade.toggle_kill_switch();
        assert!(!trade.get_state().kill_switch_active);
    }

    #[test]
    fn test_size_adjustment() {
        let dir = tempdir().unwrap();
        let logger = crate::logger::JsonlLogger::new(dir.path().to_str().unwrap()).unwrap();
        let poly = Arc::new(PolymarketService::new(make_poly_config()));
        let trade = TradeService::new(make_test_config(), poly, logger, true);

        assert_eq!(trade.get_state().current_size, 10.0);
        trade.adjust_size(5.0);
        assert_eq!(trade.get_state().current_size, 15.0);
        trade.adjust_size(-20.0); // Should clamp to 1.0
        assert_eq!(trade.get_state().current_size, 1.0);
        trade.adjust_size(200.0); // Should clamp to max_size (100.0)
        assert_eq!(trade.get_state().current_size, 100.0);
    }

    #[test]
    fn test_max_price_adjustment() {
        let dir = tempdir().unwrap();
        let logger = crate::logger::JsonlLogger::new(dir.path().to_str().unwrap()).unwrap();
        let poly = Arc::new(PolymarketService::new(make_poly_config()));
        let trade = TradeService::new(make_test_config(), poly, logger, true);

        assert!((trade.get_state().max_price_yes - 0.95).abs() < 0.001);
        trade.adjust_max_price(TradeSide::Yes, -0.05);
        assert!((trade.get_state().max_price_yes - 0.90).abs() < 0.001);
        trade.adjust_max_price(TradeSide::Yes, 0.20); // Should clamp to 0.99
        assert!((trade.get_state().max_price_yes - 0.99).abs() < 0.001);
        trade.adjust_max_price(TradeSide::Yes, -1.0); // Should clamp to 0.01
        assert!((trade.get_state().max_price_yes - 0.01).abs() < 0.001);
    }
}
