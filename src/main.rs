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
use crate::services::{BinanceBookService, PolymarketService, SignalService, TradeService};
use crate::tui::App;

#[derive(Parser, Debug)]
#[command(name = "polymarket-monitor")]
#[command(about = "Realtime BTC monitor for Polymarket 15-min markets")]
struct Args {
    /// Run in dry-run mode (no real orders)
    #[arg(long, default_value = "true")]
    dry_run: bool,

    /// Run without TUI (headless mode for testing)
    #[arg(long)]
    headless: bool,

    /// Snapshot rate in Hz
    #[arg(long, default_value = "1")]
    snapshot_hz: u32,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("polymarket_monitor=info".parse().unwrap())
                .add_directive("tokio_tungstenite=warn".parse().unwrap())
                .add_directive("tungstenite=warn".parse().unwrap()),
        )
        .init();

    // Load config
    let config = Config::load()?;
    tracing::info!("Config loaded successfully");
    tracing::info!("Mode: {}", if args.dry_run { "DRY RUN" } else { "LIVE" });

    // Initialize logger
    let logger = JsonlLogger::new(&config.logging.log_dir)?;
    tracing::info!("Logger initialized at {}", config.logging.log_dir);

    // Log startup
    logger.log_health(HealthEvent {
        t_recv_ms: chrono::Utc::now().timestamp_millis(),
        event_type: "startup".to_string(),
        message: format!("Starting in {} mode", if args.dry_run { "dry_run" } else { "live" }),
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
    let trade = Arc::new(TradeService::new(
        config.trading.clone(),
        polymarket.clone(),
        logger.clone(),
        args.dry_run,
    ));

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

    // Start snapshot logging
    let snapshot_interval_ms = 1000 / args.snapshot_hz.max(1) as u64;
    let logger_clone = logger.clone();
    let binance_snapshot = binance.clone();
    let polymarket_snapshot = polymarket.clone();
    let signal_snapshot = signal.clone();
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_millis(snapshot_interval_ms));
        loop {
            interval.tick().await;

            // Compute signal
            let sig = signal_snapshot.compute_signal();

            // Get Binance data
            let binance_update = binance_snapshot.get_current_update();
            let ret_1s = binance_snapshot.get_returns(1000);
            let ret_3s = binance_snapshot.get_returns(3000);
            let ret_10s = binance_snapshot.get_returns(10000);

            // Get Polymarket data
            let poly_quotes = polymarket_snapshot.get_quote_state();
            let poly_stale = polymarket_snapshot.get_staleness_ms();

            let snapshot = SnapshotEvent {
                t_recv_ms: chrono::Utc::now().timestamp_millis(),
                binance_mid: binance_update.as_ref().map(|u| u.mid.to_string().parse().unwrap_or(0.0)),
                binance_best_bid: binance_update.as_ref().map(|u| u.best_bid.to_string().parse().unwrap_or(0.0)),
                binance_best_ask: binance_update.as_ref().map(|u| u.best_ask.to_string().parse().unwrap_or(0.0)),
                binance_ret_1s: ret_1s,
                binance_ret_3s: ret_3s,
                binance_ret_10s: ret_10s,
                binance_obi_top5: binance_update.as_ref().map(|u| u.imbalance_top5),
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
        let mut app = App::new(binance.clone(), polymarket.clone(), signal.clone(), trade.clone(), args.dry_run);
        app.run().await?;
    }

    // Shutdown
    binance.stop();
    polymarket.stop();

    logger.log_health(HealthEvent {
        t_recv_ms: chrono::Utc::now().timestamp_millis(),
        event_type: "shutdown".to_string(),
        message: "Graceful shutdown".to_string(),
        component: "main".to_string(),
    })?;

    tracing::info!("Shutdown complete");
    Ok(())
}
