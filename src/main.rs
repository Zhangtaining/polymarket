// Allow dead_code for fields reserved for future use
#![allow(dead_code)]

mod config;
mod events;
mod logger;
mod services;
mod tui;

use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tokio::time::{interval, Duration};

use crate::config::Config;
use crate::events::{HealthEvent, SnapshotEvent};
use crate::logger::JsonlLogger;
use crate::services::{BinanceBookService, ChainlinkService, ClobClient, ClobCredentials, PolymarketService, SignalService, TradeService};
use crate::tui::{App, TuiLogBuffer, TuiLogLayer};

#[derive(Parser, Debug)]
#[command(name = "polymarket-monitor")]
#[command(about = "Realtime BTC monitor for Polymarket 15-min markets")]
struct Args {
    /// Run in dry-run mode (no real orders)
    #[arg(long, default_value = "true")]
    dry_run: bool,

    /// Disable dry-run (place real orders). Overrides --dry-run.
    #[arg(long = "no-dry-run")]
    no_dry_run: bool,

    /// Run without TUI (headless mode for testing)
    #[arg(long)]
    headless: bool,

    /// Snapshot rate in Hz
    #[arg(long, default_value = "1")]
    snapshot_hz: u32,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file (if exists)
    dotenvy::dotenv().ok();

    let args = Args::parse();
    let dry_run = args.dry_run && !args.no_dry_run;

    // Initialize tracing: file logs + in-memory buffer for TUI display
    let log_buffer = TuiLogBuffer::new();

    let (writer, _tracing_guard) = if args.headless {
        tracing_appender::non_blocking(std::io::stderr())
    } else {
        let _ = std::fs::create_dir_all("data/logs");
        let file_appender = tracing_appender::rolling::daily("data/logs", "polymarket.log");
        tracing_appender::non_blocking(file_appender)
    };

    let env_filter = tracing_subscriber::EnvFilter::from_default_env()
        .add_directive("polymarket_monitor=debug".parse().unwrap())
        .add_directive("tokio_tungstenite=warn".parse().unwrap())
        .add_directive("tungstenite=warn".parse().unwrap());

    {
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;
        tracing_subscriber::registry()
            .with(env_filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(writer)
            )
            .with(TuiLogLayer::new(log_buffer.clone()))
            .init();
    }

    // Load config
    let config = Config::load()?;
    tracing::info!("Config loaded successfully");
    tracing::info!("Mode: {}", if dry_run { "DRY RUN" } else { "LIVE" });

    // Initialize logger
    let logger = JsonlLogger::new(&config.logging.log_dir)?;
    tracing::info!("Logger initialized at {}", config.logging.log_dir);

    // Log startup
    logger.log_health(HealthEvent {
        t_recv_ms: chrono::Utc::now().timestamp_millis(),
        event_type: "startup".to_string(),
        message: format!("Starting in {} mode", if dry_run { "dry_run" } else { "live" }),
        component: "main".to_string(),
    })?;

    // Create services
    let binance = Arc::new(BinanceBookService::new(config.binance.clone()));
    let polymarket = Arc::new(PolymarketService::new(config.polymarket.clone()));
    let signal = Arc::new(SignalService::new(
        config.signal.clone(),
        binance.clone(),
        polymarket.clone(),
    ));
    // Create CLOB credentials if available
    let clob_credentials = if !config.polymarket.api_key.is_empty()
        && !config.polymarket.api_secret.is_empty()
        && !config.polymarket.passphrase.is_empty()
        && !config.polymarket.wallet_address.is_empty()
    {
        tracing::info!("CLOB API credentials configured (wallet: {})", &config.polymarket.wallet_address);
        tracing::info!("  API key: {}...{}", &config.polymarket.api_key[..8], &config.polymarket.api_key[config.polymarket.api_key.len().saturating_sub(4)..]);
        tracing::info!("  Passphrase: {}...", &config.polymarket.passphrase[..8]);
        Some(ClobCredentials {
            api_key: config.polymarket.api_key.clone(),
            secret: config.polymarket.api_secret.clone(),
            passphrase: config.polymarket.passphrase.clone(),
            wallet_address: config.polymarket.wallet_address.clone(),
        })
    } else {
        tracing::warn!("Missing CLOB API credentials - live trading disabled");
        if config.polymarket.api_key.is_empty() { tracing::warn!("  - Missing: api_key"); }
        if config.polymarket.api_secret.is_empty() { tracing::warn!("  - Missing: api_secret"); }
        if config.polymarket.passphrase.is_empty() { tracing::warn!("  - Missing: passphrase"); }
        if config.polymarket.wallet_address.is_empty() { tracing::warn!("  - Missing: wallet_address"); }
        None
    };

    // Run a quick auth check before starting services
    if let Some(ref creds) = clob_credentials {
        tracing::info!("Running CLOB API auth check...");
        let test_client = ClobClient::new(Some(creds.clone()));
        match test_client.check_auth().await {
            Ok(body) => tracing::info!("Auth check PASSED: {}", &body[..body.len().min(200)]),
            Err(e) => tracing::error!("Auth check FAILED: {:?}", e),
        }
    }

    let trade = Arc::new(TradeService::new(
        config.trading.clone(),
        polymarket.clone(),
        clob_credentials,
        logger.clone(),
        dry_run,
    ));

    // Create Chainlink service for accurate target price
    let chainlink = Arc::new(ChainlinkService::new());

    // Start Binance service
    let binance_clone = binance.clone();
    tokio::spawn(async move {
        if let Err(e) = binance_clone.start().await {
            tracing::error!("Binance service error: {:?}", e);
        }
    });

    // Start Polymarket service
    let polymarket_clone = polymarket.clone();
    tokio::spawn(async move {
        if let Err(e) = polymarket_clone.start().await {
            tracing::error!("Polymarket service error: {:?}", e);
        }
    });

    // Start Chainlink RTDS service for target price
    let chainlink_clone = chainlink.clone();
    tokio::spawn(async move {
        if let Err(e) = chainlink_clone.start().await {
            tracing::error!("Chainlink service error: {:?}", e);
        }
    });

    // Start snapshot logging
    let snapshot_interval_ms = 1000 / args.snapshot_hz.max(1) as u64;
    let logger_clone = logger.clone();
    let binance_snapshot = binance.clone();
    let polymarket_snapshot = polymarket.clone();
    let signal_snapshot = signal.clone();
    let chainlink_snapshot = chainlink.clone();
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_millis(snapshot_interval_ms));
        let mut last_condition_id = String::new();

        loop {
            interval.tick().await;

            // Compute signal
            let sig = signal_snapshot.compute_signal();

            // Get Binance data
            let binance_update = binance_snapshot.get_current_update();
            let ret_1s = binance_snapshot.get_returns(1000);
            let ret_3s = binance_snapshot.get_returns(3000);
            let ret_10s = binance_snapshot.get_returns(10000);
            let std_5m = binance_snapshot.get_std_dev(300_000); // 5 minutes

            // Get Chainlink price (this is what Polymarket uses for "Price to Beat")
            let chainlink_price = chainlink_snapshot.get_btc_price();

            // Get Polymarket data
            let poly_quotes = polymarket_snapshot.get_quote_state();
            let poly_stale = polymarket_snapshot.get_staleness_ms();
            let active_market = polymarket_snapshot.get_active_market();
            let remaining_secs = polymarket_snapshot.get_remaining_secs();

            // Set target price when market changes OR when window start time has passed
            if !active_market.condition_id.is_empty() {
                // Market changed - reset and try to fetch price to beat from page
                if active_market.condition_id != last_condition_id {
                    last_condition_id = active_market.condition_id.clone();
                    // Clear old target price for new window
                    polymarket_snapshot.clear_target_price();

                    // Try to fetch the actual price to beat from the page
                    let poly_clone = polymarket_snapshot.clone();
                    tokio::spawn(async move {
                        if let Some(price) = poly_clone.fetch_price_to_beat_from_page().await {
                            poly_clone.force_set_target_price(price);
                        }
                    });
                }

                // Fallback: if target price still not set after a few seconds, use Chainlink price
                if active_market.target_price.is_none() {
                    if let Some(price) = chainlink_price {
                        // Check if window has started
                        let window_started = if !active_market.start_time.is_empty() {
                            if let Ok(start_time) = chrono::DateTime::parse_from_rfc3339(&active_market.start_time) {
                                chrono::Utc::now() >= start_time
                            } else {
                                true // If can't parse, assume started
                            }
                        } else {
                            true // No start time, assume started
                        };

                        if window_started {
                            polymarket_snapshot.set_target_price(price);
                        }
                    }
                }
            }

            let snapshot = SnapshotEvent {
                t_recv_ms: chrono::Utc::now().timestamp_millis(),
                binance_mid: binance_update.as_ref().map(|u| u.mid.to_string().parse().unwrap_or(0.0)),
                binance_best_bid: binance_update.as_ref().map(|u| u.best_bid.to_string().parse().unwrap_or(0.0)),
                binance_best_ask: binance_update.as_ref().map(|u| u.best_ask.to_string().parse().unwrap_or(0.0)),
                binance_ret_1s: ret_1s,
                binance_ret_3s: ret_3s,
                binance_ret_10s: ret_10s,
                binance_obi_top5: binance_update.as_ref().map(|u| u.imbalance_top5),
                binance_std_5m: std_5m,
                poly_yes_bid: poly_quotes.yes_bid,
                poly_yes_ask: poly_quotes.yes_ask,
                poly_no_bid: poly_quotes.no_bid,
                poly_no_ask: poly_quotes.no_ask,
                poly_spread_yes: match (poly_quotes.yes_bid, poly_quotes.yes_ask) {
                    (Some(b), Some(a)) => Some(a - b),
                    _ => None,
                },
                poly_spread_no: match (poly_quotes.no_bid, poly_quotes.no_ask) {
                    (Some(b), Some(a)) => Some(a - b),
                    _ => None,
                },
                poly_stale_ms: if poly_stale == i64::MAX { None } else { Some(poly_stale) },
                poly_target_price: active_market.target_price,
                poly_remaining_secs: remaining_secs,
                signal_side: sig.suggested_side.map(|s| s.to_string()).unwrap_or("NONE".to_string()),
                signal_score: sig.confidence,
            };

            if let Err(e) = logger_clone.log_snapshot(snapshot) {
                tracing::error!("Failed to log snapshot: {:?}", e);
            }
        }
    });

    if args.headless {
        // Headless mode - just run forever
        tracing::info!("Running in headless mode. Press Ctrl+C to exit.");
        tokio::signal::ctrl_c().await?;
    } else {
        // Run TUI
        let mut app = App::new(binance.clone(), polymarket.clone(), chainlink.clone(), signal.clone(), trade.clone(), log_buffer.clone(), dry_run);
        app.run().await?;
    }

    // Shutdown
    binance.stop();
    polymarket.stop();
    chainlink.stop();

    logger.log_health(HealthEvent {
        t_recv_ms: chrono::Utc::now().timestamp_millis(),
        event_type: "shutdown".to_string(),
        message: "Graceful shutdown".to_string(),
        component: "main".to_string(),
    })?;

    tracing::info!("Shutdown complete");
    Ok(())
}
