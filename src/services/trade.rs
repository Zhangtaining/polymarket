use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::Arc;
use uuid::Uuid;

use crate::config::TradingConfig;
use crate::events::{TradeEvent, TradeSide};
use crate::logger::JsonlLogger;
use crate::services::PolymarketService;
use super::clob::{ClobClient, ClobCredentials, OrderRequest};

/// A single user action for display in the TUI action log.
#[derive(Debug, Clone)]
pub struct ActionLogEntry {
    pub timestamp_ms: i64,
    pub description: String,
}

impl ActionLogEntry {
    pub fn now(description: impl Into<String>) -> Self {
        Self {
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            description: description.into(),
        }
    }

    /// Format for display: "HH:MM:SS | description"
    pub fn format_short(&self) -> String {
        let dt = chrono::DateTime::from_timestamp_millis(self.timestamp_ms)
            .unwrap_or_else(chrono::Utc::now);
        format!("{} | {}", dt.format("%H:%M:%S"), self.description)
    }
}

const ACTION_LOG_CAP: usize = 100;

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
    clob_client: ClobClient,
    logger: Arc<JsonlLogger>,
    state: Arc<RwLock<TradingState>>,
    action_log: Arc<RwLock<VecDeque<ActionLogEntry>>>,
    dry_run: bool,
    credentials_debug: Option<ClobCredentials>,
}

impl TradeService {
    pub fn new(
        config: TradingConfig,
        polymarket: Arc<PolymarketService>,
        credentials: Option<ClobCredentials>,
        logger: Arc<JsonlLogger>,
        dry_run: bool,
    ) -> Self {
        let state = TradingState::new(&config);
        let credentials_debug = credentials.clone();
        let clob_client = ClobClient::new(credentials);
        Self {
            config,
            polymarket,
            clob_client,
            logger,
            state: Arc::new(RwLock::new(state)),
            action_log: Arc::new(RwLock::new(VecDeque::with_capacity(ACTION_LOG_CAP))),
            dry_run,
            credentials_debug,
        }
    }

    /// Format loaded credentials for debugging display in the action log.
    fn credentials_debug_string(&self) -> String {
        match &self.credentials_debug {
            Some(creds) => format!(
                "ENV: api_key={}, secret={}..., passphrase={}..., wallet={}",
                creds.api_key,
                &creds.secret[..creds.secret.len().min(12)],
                &creds.passphrase[..creds.passphrase.len().min(12)],
                creds.wallet_address,
            ),
            None => "ENV: <no credentials loaded>".to_string(),
        }
    }

    fn record_action(&self, entry: ActionLogEntry) {
        let mut log = self.action_log.write();
        if log.len() >= ACTION_LOG_CAP {
            log.pop_front();
        }
        log.push_back(entry);
    }

    /// Returns a clone of recent action log entries (newest last).
    pub fn get_action_log(&self) -> Vec<ActionLogEntry> {
        self.action_log.read().iter().cloned().collect()
    }

    pub fn get_state(&self) -> TradingState {
        self.state.read().clone()
    }

    pub fn toggle_kill_switch(&self) {
        let mut state = self.state.write();
        state.kill_switch_active = !state.kill_switch_active;
        let label = if state.kill_switch_active { "ON" } else { "OFF" };
        self.record_action(ActionLogEntry::now(format!("Kill switch → {}", label)));
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
        self.record_action(ActionLogEntry::now(format!(
            "Size {} {} → {:.1}",
            if delta >= 0.0 { "+" } else { "" },
            delta,
            new_size
        )));
        tracing::info!("Size adjusted to: {}", new_size);
    }

    pub fn adjust_max_price(&self, side: TradeSide, delta: f64) {
        let mut state = self.state.write();
        match side {
            TradeSide::Yes => {
                state.max_price_yes = (state.max_price_yes + delta).clamp(0.01, 0.99);
                self.record_action(ActionLogEntry::now(format!(
                    "Max YES price {} {} → {:.2}",
                    if delta >= 0.0 { "+" } else { "" },
                    delta,
                    state.max_price_yes
                )));
                tracing::info!("Max YES price adjusted to: {}", state.max_price_yes);
            }
            TradeSide::No => {
                state.max_price_no = (state.max_price_no + delta).clamp(0.01, 0.99);
                self.record_action(ActionLogEntry::now(format!(
                    "Max NO price {} {} → {:.2}",
                    if delta >= 0.0 { "+" } else { "" },
                    delta,
                    state.max_price_no
                )));
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
        let max_price_yes = state.max_price_yes;
        let max_price_no = state.max_price_no;
        drop(state);

        // Use current market (best ask) as order price, capped by max price
        let quotes = self.polymarket.get_quote_state();
        let limit_price = match side {
            TradeSide::Yes => quotes
                .yes_ask
                .map(|ask| ask.min(max_price_yes))
                .unwrap_or(max_price_yes),
            TradeSide::No => quotes
                .no_ask
                .map(|ask| ask.min(max_price_no))
                .unwrap_or(max_price_no),
        };

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
                self.record_action(ActionLogEntry::now(format!(
                    "Buy {} @ {:.2} size {:.0} → rejected: {}",
                    side, limit_price, size, reason
                )));
                self.logger.log_trade(trade_event.clone())?;
                return Err(anyhow!("Order rejected: {}", reason));
            }
            RiskCheckResult::Allowed => {}
        }

        if self.dry_run {
            // Dry run - just log the intent
            trade_event.api_status = Some("dry_run_success".to_string());
            trade_event.t_resp_ms = Some(chrono::Utc::now().timestamp_millis());
            self.record_action(ActionLogEntry::now(format!(
                "Buy {} @ {:.2} size {:.0} → dry_run",
                side, limit_price, size
            )));
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

        // Live mode - place order via CLOB API
        let active_market = self.polymarket.get_active_market();

        // Get the appropriate token ID based on side
        // For BTC Up/Down markets: Yes = Up, No = Down
        let token_id = match side {
            TradeSide::Yes => &active_market.up_token_id,
            TradeSide::No => &active_market.down_token_id,
        };

        if token_id.is_empty() {
            trade_event.api_status = Some("no_active_market".to_string());
            trade_event.t_resp_ms = Some(chrono::Utc::now().timestamp_millis());
            self.record_action(ActionLogEntry::now(format!(
                "Buy {} @ {:.2} size {:.0} → no active market",
                side, limit_price, size
            )));
            self.logger.log_trade(trade_event.clone())?;
            return Err(anyhow!("No active market - token ID not available"));
        }

        // Live: send BUY for the chosen token (Yes=Up, No=Down). We never send SELL.
        let order_request = OrderRequest {
            token_id: token_id.clone(),
            price: format!("{:.2}", limit_price),
            size: format!("{:.0}", size),
            side: "BUY".to_string(),
            order_type: "GTC".to_string(), // Good Till Cancelled
            expiration: None,
        };

        tracing::info!(
            "[LIVE] Placing BUY order: side={} @ {} size {} (token {}...)",
            side,
            limit_price,
            size,
            &token_id[..20.min(token_id.len())]
        );

        match self.clob_client.place_order(order_request).await {
            Ok(response) => {
                trade_event.t_resp_ms = Some(chrono::Utc::now().timestamp_millis());

                if response.success {
                    trade_event.api_status = Some("success".to_string());
                    self.record_action(ActionLogEntry::now(format!(
                        "Buy {} @ {:.2} size {:.0} → success",
                        side, limit_price, size
                    )));
                    if let Some(order_id) = &response.order_id {
                        tracing::info!("[LIVE] Order placed successfully: {}", order_id);
                    }
                } else {
                    let error_msg = if let Some(msg) = &response.error_msg {
                        msg.clone()
                    } else {
                        // Build a detailed message from available fields
                        let mut parts = Vec::new();
                        if let Some(status) = &response.status {
                            parts.push(format!("status={}", status));
                        }
                        if let Some(http) = response.http_status {
                            parts.push(format!("http={}", http));
                        }
                        if let Some(raw) = &response.raw_body {
                            // Truncate raw body for display
                            let truncated = if raw.len() > 200 { &raw[..200] } else { raw.as_str() };
                            parts.push(format!("body={}", truncated));
                        }
                        if parts.is_empty() {
                            "Unknown error (no details in API response)".to_string()
                        } else {
                            parts.join(", ")
                        }
                    };
                    trade_event.api_status = Some(format!("error: {}", error_msg));
                    self.record_action(ActionLogEntry::now(format!(
                        "Buy {} @ {:.2} size {:.0} → error: {}",
                        side, limit_price, size, error_msg
                    )));
                    self.record_action(ActionLogEntry::now(self.credentials_debug_string()));
                    tracing::error!("[LIVE] Order failed: {}", error_msg);
                }

                self.logger.log_trade(trade_event.clone())?;
                Ok(trade_event)
            }
            Err(e) => {
                trade_event.t_resp_ms = Some(chrono::Utc::now().timestamp_millis());
                trade_event.api_status = Some(format!("error: {}", e));
                self.record_action(ActionLogEntry::now(format!(
                    "Buy {} @ {:.2} size {:.0} → error: {}",
                    side, limit_price, size, e
                )));
                self.record_action(ActionLogEntry::now(self.credentials_debug_string()));
                self.logger.log_trade(trade_event.clone())?;
                tracing::error!("[LIVE] Order error: {:?}", e);
                Err(e)
            }
        }
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
            gamma_url: "https://gamma-api.polymarket.com".to_string(),
            btc_15m_event_id: "194059".to_string(),
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
        let trade = TradeService::new(make_test_config(), poly, None, logger, true);

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
        let trade = TradeService::new(make_test_config(), poly, None, logger, true);

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
        let trade = TradeService::new(make_test_config(), poly, None, logger, true);

        assert!((trade.get_state().max_price_yes - 0.95).abs() < 0.001);
        trade.adjust_max_price(TradeSide::Yes, -0.05);
        assert!((trade.get_state().max_price_yes - 0.90).abs() < 0.001);
        trade.adjust_max_price(TradeSide::Yes, 0.20); // Should clamp to 0.99
        assert!((trade.get_state().max_price_yes - 0.99).abs() < 0.001);
        trade.adjust_max_price(TradeSide::Yes, -1.0); // Should clamp to 0.01
        assert!((trade.get_state().max_price_yes - 0.01).abs() < 0.001);
    }
}
