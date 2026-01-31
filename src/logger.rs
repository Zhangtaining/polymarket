use anyhow::Result;
use chrono::{Local, NaiveDate};
use parking_lot::Mutex;
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;

use crate::events::{HealthEvent, SnapshotEvent, TradeEvent};

pub struct JsonlLogger {
    base_dir: PathBuf,
    current_date: Mutex<NaiveDate>,
    snapshot_writer: Mutex<Option<BufWriter<File>>>,
    trade_writer: Mutex<Option<BufWriter<File>>>,
    health_writer: Mutex<Option<BufWriter<File>>>,
}

impl JsonlLogger {
    pub fn new(base_dir: &str) -> Result<Arc<Self>> {
        let base_path = PathBuf::from(base_dir);
        fs::create_dir_all(&base_path)?;

        let today = Local::now().date_naive();
        let logger = Arc::new(Self {
            base_dir: base_path,
            current_date: Mutex::new(today),
            snapshot_writer: Mutex::new(None),
            trade_writer: Mutex::new(None),
            health_writer: Mutex::new(None),
        });

        logger.ensure_writers()?;

        // Log startup event
        logger.log_health(HealthEvent {
            t_recv_ms: chrono::Utc::now().timestamp_millis(),
            event_type: "startup".to_string(),
            message: "Logger initialized".to_string(),
            component: "logger".to_string(),
        })?;

        Ok(logger)
    }

    fn get_date_dir(&self, date: NaiveDate) -> PathBuf {
        self.base_dir.join(date.format("%Y-%m-%d").to_string())
    }

    fn ensure_writers(&self) -> Result<()> {
        let today = Local::now().date_naive();
        let mut current_date = self.current_date.lock();

        if *current_date != today || self.snapshot_writer.lock().is_none() {
            *current_date = today;
            let date_dir = self.get_date_dir(today);
            fs::create_dir_all(&date_dir)?;

            // Create/open snapshot file
            let snapshot_path = date_dir.join("events_snapshot.jsonl");
            let snapshot_file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(snapshot_path)?;
            *self.snapshot_writer.lock() = Some(BufWriter::new(snapshot_file));

            // Create/open trade file
            let trade_path = date_dir.join("trades.jsonl");
            let trade_file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(trade_path)?;
            *self.trade_writer.lock() = Some(BufWriter::new(trade_file));

            // Create/open health file
            let health_path = date_dir.join("health.jsonl");
            let health_file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(health_path)?;
            *self.health_writer.lock() = Some(BufWriter::new(health_file));
        }

        Ok(())
    }

    fn write_json<T: Serialize>(&self, writer: &Mutex<Option<BufWriter<File>>>, event: &T) -> Result<()> {
        self.ensure_writers()?;
        let mut guard = writer.lock();
        if let Some(ref mut w) = *guard {
            let json = serde_json::to_string(event)?;
            writeln!(w, "{}", json)?;
            w.flush()?;
        }
        Ok(())
    }

    pub fn log_snapshot(&self, event: SnapshotEvent) -> Result<()> {
        self.write_json(&self.snapshot_writer, &event)
    }

    pub fn log_trade(&self, event: TradeEvent) -> Result<()> {
        self.write_json(&self.trade_writer, &event)
    }

    pub fn log_health(&self, event: HealthEvent) -> Result<()> {
        self.write_json(&self.health_writer, &event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_logger_creates_files() {
        let dir = tempdir().unwrap();
        let logger = JsonlLogger::new(dir.path().to_str().unwrap()).unwrap();

        let snapshot = SnapshotEvent::default();
        logger.log_snapshot(snapshot).unwrap();

        let today = Local::now().date_naive();
        let date_dir = dir.path().join(today.format("%Y-%m-%d").to_string());
        assert!(date_dir.join("events_snapshot.jsonl").exists());
        assert!(date_dir.join("health.jsonl").exists());
    }
}
