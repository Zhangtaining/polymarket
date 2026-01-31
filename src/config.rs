use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub general: GeneralConfig,
    pub binance: BinanceConfig,
    pub polymarket: PolymarketConfig,
    pub trading: TradingConfig,
    pub signal: SignalConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GeneralConfig {
    pub dry_run: bool,
    pub snapshot_rate_hz: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BinanceConfig {
    pub ws_url: String,
    pub rest_url: String,
    pub symbol: String,
    pub snapshot_limit: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolymarketConfig {
    pub ws_url: String,
    pub rest_url: String,
    pub yes_token_id: String,
    pub no_token_id: String,
    pub condition_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TradingConfig {
    pub default_size: f64,
    pub max_size: f64,
    pub max_price_yes: f64,
    pub max_price_no: f64,
    pub max_spread: f64,
    pub stale_quote_threshold_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SignalConfig {
    pub binance_return_threshold_1s: f64,
    pub binance_return_threshold_3s: f64,
    pub poly_lag_threshold_ms: u64,
    pub min_confidence: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    pub log_dir: String,
    pub rotation: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        let settings = config::Config::builder()
            .add_source(config::File::with_name("config/default"))
            .add_source(config::Environment::with_prefix("POLY"))
            .build()?;

        let config: Config = settings.try_deserialize()?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_load() {
        // This test requires the config file to exist
        let config = Config::load();
        assert!(config.is_ok(), "Config should load successfully");
    }
}
