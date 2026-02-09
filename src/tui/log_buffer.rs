use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::Arc;
use tracing::Level;

const LOG_BUFFER_CAP: usize = 200;

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: Level,
    pub target: String,
    pub message: String,
}

impl LogEntry {
    /// Format for TUI display: "HH:MM:SS LEVEL target: message"
    pub fn format_short(&self) -> String {
        let lvl = match self.level {
            Level::ERROR => "ERROR",
            Level::WARN  => "WARN ",
            Level::INFO  => "INFO ",
            Level::DEBUG => "DEBUG",
            Level::TRACE => "TRACE",
        };
        format!("{} {} {}", self.timestamp, lvl, self.message)
    }
}

/// Shared ring buffer that collects tracing log entries for TUI display.
#[derive(Clone)]
pub struct TuiLogBuffer {
    entries: Arc<RwLock<VecDeque<LogEntry>>>,
}

impl TuiLogBuffer {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(VecDeque::with_capacity(LOG_BUFFER_CAP))),
        }
    }

    pub fn push(&self, entry: LogEntry) {
        let mut buf = self.entries.write();
        if buf.len() >= LOG_BUFFER_CAP {
            buf.pop_front();
        }
        buf.push_back(entry);
    }

    pub fn get_entries(&self) -> Vec<LogEntry> {
        self.entries.read().iter().cloned().collect()
    }
}

/// A tracing Layer that writes formatted log events into a TuiLogBuffer.
pub struct TuiLogLayer {
    buffer: TuiLogBuffer,
}

impl TuiLogLayer {
    pub fn new(buffer: TuiLogBuffer) -> Self {
        Self { buffer }
    }
}

impl<S> tracing_subscriber::Layer<S> for TuiLogLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        // Extract message from the event fields
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        let now = chrono::Utc::now();
        let timestamp = now.format("%H:%M:%S").to_string();

        let entry = LogEntry {
            timestamp,
            level: *event.metadata().level(),
            target: event.metadata().target().to_string(),
            message: visitor.message,
        };

        self.buffer.push(entry);
    }
}

/// Visitor that extracts the `message` field from tracing events.
#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
            // Strip surrounding quotes from Debug formatting
            if self.message.starts_with('"') && self.message.ends_with('"') {
                self.message = self.message[1..self.message.len() - 1].to_string();
            }
        } else if self.message.is_empty() {
            self.message = format!("{}={:?}", field.name(), value);
        } else {
            self.message.push_str(&format!(", {}={:?}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else if self.message.is_empty() {
            self.message = format!("{}={}", field.name(), value);
        } else {
            self.message.push_str(&format!(", {}={}", field.name(), value));
        }
    }
}
