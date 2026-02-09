use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use std::io::{self, Stdout};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::events::TradeSide;
use crate::services::{BinanceBookService, ChainlinkService, PolymarketService, SignalService, TradeService};
use super::log_buffer::TuiLogBuffer;

pub enum TuiCommand {
    BuyYes,
    BuyNo,
    ToggleKillSwitch,
    IncrementSize,
    DecrementSize,
    IncrementMaxPriceYes,
    DecrementMaxPriceYes,
    IncrementMaxPriceNo,
    DecrementMaxPriceNo,
    Quit,
}

pub struct App {
    binance: Arc<BinanceBookService>,
    polymarket: Arc<PolymarketService>,
    chainlink: Arc<ChainlinkService>,
    signal: Arc<SignalService>,
    trade: Arc<TradeService>,
    command_tx: mpsc::Sender<TuiCommand>,
    command_rx: mpsc::Receiver<TuiCommand>,
    log_buffer: TuiLogBuffer,
    dry_run: bool,
}

impl App {
    pub fn new(
        binance: Arc<BinanceBookService>,
        polymarket: Arc<PolymarketService>,
        chainlink: Arc<ChainlinkService>,
        signal: Arc<SignalService>,
        trade: Arc<TradeService>,
        log_buffer: TuiLogBuffer,
        dry_run: bool,
    ) -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            binance,
            polymarket,
            chainlink,
            signal,
            trade,
            command_tx: tx,
            command_rx: rx,
            log_buffer,
            dry_run,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = self.run_app(&mut terminal).await;

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

        result
    }

    async fn run_app(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
        loop {
            // Draw UI
            terminal.draw(|f| self.ui(f))?;

            // Poll for events with timeout
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    // Handle Ctrl+C
                    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                        break;
                    }

                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('y') => {
                            if let Err(e) = self.trade.place_order(TradeSide::Yes).await {
                                tracing::error!("Order error: {:?}", e);
                            }
                        }
                        KeyCode::Char('n') => {
                            if let Err(e) = self.trade.place_order(TradeSide::No).await {
                                tracing::error!("Order error: {:?}", e);
                            }
                        }
                        KeyCode::Char('k') => {
                            self.trade.toggle_kill_switch();
                        }
                        KeyCode::Char('+') | KeyCode::Char('=') => {
                            self.trade.adjust_size(5.0);
                        }
                        KeyCode::Char('-') | KeyCode::Char('_') => {
                            self.trade.adjust_size(-5.0);
                        }
                        KeyCode::Char('[') => {
                            self.trade.adjust_max_price(TradeSide::Yes, -0.01);
                        }
                        KeyCode::Char(']') => {
                            self.trade.adjust_max_price(TradeSide::Yes, 0.01);
                        }
                        KeyCode::Char('{') => {
                            self.trade.adjust_max_price(TradeSide::No, -0.01);
                        }
                        KeyCode::Char('}') => {
                            self.trade.adjust_max_price(TradeSide::No, 0.01);
                        }
                        _ => {}
                    }
                }
            }

            // Compute signal
            self.signal.compute_signal();
        }

        Ok(())
    }

    fn ui(&self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3),   // Header
                Constraint::Length(8),   // Binance panel
                Constraint::Length(7),   // Polymarket panel
                Constraint::Length(6),   // Signal panel
                Constraint::Length(6),   // Trading config panel
                Constraint::Min(4),      // Actions log (flexible)
                Constraint::Min(6),      // Logs console (flexible)
                Constraint::Length(10),  // Hotkeys help
            ])
            .split(f.size());

        self.render_header(f, chunks[0]);
        self.render_binance_panel(f, chunks[1]);
        self.render_polymarket_panel(f, chunks[2]);
        self.render_signal_panel(f, chunks[3]);
        self.render_trading_panel(f, chunks[4]);
        self.render_actions_panel(f, chunks[5]);
        self.render_logs_panel(f, chunks[6]);
        self.render_help_panel(f, chunks[7]);
    }

    fn render_header(&self, f: &mut Frame, area: Rect) {
        let mode = if self.dry_run { "DRY RUN" } else { "LIVE" };
        let mode_style = if self.dry_run {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        };

        let header = Paragraph::new(Line::from(vec![
            Span::raw("Polymarket Monitor | Mode: "),
            Span::styled(mode, mode_style),
        ]))
        .block(Block::default().borders(Borders::ALL).title("Status"));

        f.render_widget(header, area);
    }

    fn render_binance_panel(&self, f: &mut Frame, area: Rect) {
        let update = self.binance.get_current_update();
        let ret_1s = self.binance.get_returns(1000);
        let ret_3s = self.binance.get_returns(3000);
        let ret_10s = self.binance.get_returns(10000);
        let std_5m = self.binance.get_std_dev(300_000);

        let content = if let Some(u) = update {
            let mid: f64 = u.mid.to_string().parse().unwrap_or(0.0);
            let bid: f64 = u.best_bid.to_string().parse().unwrap_or(0.0);
            let ask: f64 = u.best_ask.to_string().parse().unwrap_or(0.0);

            format!(
                "Mid: ${:.2}\nBid: ${:.2} | Ask: ${:.2}\nSpread: ${:.2}\n\
                 Returns: 1s={:+.4}% | 3s={:+.4}% | 10s={:+.4}%\n\
                 Imbalance (top5): {:+.3} | Std Dev (5m): ${:.2}",
                mid,
                bid,
                ask,
                ask - bid,
                ret_1s.unwrap_or(0.0) * 100.0,
                ret_3s.unwrap_or(0.0) * 100.0,
                ret_10s.unwrap_or(0.0) * 100.0,
                u.imbalance_top5,
                std_5m.unwrap_or(0.0)
            )
        } else {
            "Connecting to Binance...".to_string()
        };

        let panel = Paragraph::new(content)
            .block(Block::default().borders(Borders::ALL).title("Binance BTCUSDT Perp"));

        f.render_widget(panel, area);
    }

    fn render_polymarket_panel(&self, f: &mut Frame, area: Rect) {
        let quotes = self.polymarket.get_quote_state();
        let stale_ms = self.polymarket.get_staleness_ms();
        let active_market = self.polymarket.get_active_market();
        let remaining_secs = self.polymarket.get_remaining_secs();
        let chainlink_price = self.chainlink.get_btc_price();

        let yes_spread = match (quotes.yes_bid, quotes.yes_ask) {
            (Some(b), Some(a)) => format!("{:.3}", a - b),
            _ => "N/A".to_string(),
        };

        let no_spread = match (quotes.no_bid, quotes.no_ask) {
            (Some(b), Some(a)) => format!("{:.3}", a - b),
            _ => "N/A".to_string(),
        };

        let target_price_str = match active_market.target_price {
            Some(p) => format!("${:.2}", p),
            None => "N/A".to_string(),
        };

        let chainlink_price_str = match chainlink_price {
            Some(p) => format!("${:.2}", p),
            None => "N/A".to_string(),
        };

        let remaining_str = match remaining_secs {
            Some(s) => {
                let mins = s / 60;
                let secs = s % 60;
                format!("{}m {}s", mins, secs)
            }
            None => "N/A".to_string(),
        };

        let truncate = |s: &str, n: usize| {
            if s.len() <= n {
                s.to_string()
            } else {
                format!("{}...", &s[..n])
            }
        };
        let slug_str = if active_market.slug.is_empty() {
            "N/A".to_string()
        } else {
            active_market.slug.clone()
        };
        let up_token_str = if active_market.up_token_id.is_empty() {
            "N/A".to_string()
        } else {
            truncate(&active_market.up_token_id, 24)
        };
        let down_token_str = if active_market.down_token_id.is_empty() {
            "N/A".to_string()
        } else {
            truncate(&active_market.down_token_id, 24)
        };

        let content = format!(
            "Slug: {} | Up: {} | Down: {}\n\
             Target (Price to Beat): {} | Chainlink Now: {}\n\
             Remaining: {} | Staleness: {}ms\n\
             UP:   Bid={:.3} | Ask={:.3} | Spread={}\n\
             DOWN: Bid={:.3} | Ask={:.3} | Spread={}",
            slug_str,
            up_token_str,
            down_token_str,
            target_price_str,
            chainlink_price_str,
            remaining_str,
            if stale_ms == i64::MAX { "N/A".to_string() } else { stale_ms.to_string() },
            quotes.yes_bid.unwrap_or(0.0),
            quotes.yes_ask.unwrap_or(0.0),
            yes_spread,
            quotes.no_bid.unwrap_or(0.0),
            quotes.no_ask.unwrap_or(0.0),
            no_spread,
        );

        let panel = Paragraph::new(content)
            .block(Block::default().borders(Borders::ALL).title("Polymarket Quotes"));

        f.render_widget(panel, area);
    }

    fn render_signal_panel(&self, f: &mut Frame, area: Rect) {
        let signal = self.signal.get_signal_state();

        let side_str = signal
            .suggested_side
            .map(|s| s.to_string())
            .unwrap_or("NONE".to_string());

        let side_color = match signal.suggested_side {
            Some(TradeSide::Yes) => Color::Green,
            Some(TradeSide::No) => Color::Red,
            None => Color::Gray,
        };

        let reasons = if signal.reasons.is_empty() {
            "No signal".to_string()
        } else {
            signal.reasons.join("; ")
        };

        let content = vec![
            Line::from(vec![
                Span::raw("Suggested: "),
                Span::styled(side_str, Style::default().fg(side_color).add_modifier(Modifier::BOLD)),
                Span::raw(format!(" (confidence: {:.2})", signal.confidence)),
            ]),
            Line::from(format!("Reasons: {}", reasons)),
        ];

        let panel = Paragraph::new(content)
            .block(Block::default().borders(Borders::ALL).title("Signal"));

        f.render_widget(panel, area);
    }

    fn render_trading_panel(&self, f: &mut Frame, area: Rect) {
        let state = self.trade.get_state();

        let kill_switch = if state.kill_switch_active {
            Span::styled("ACTIVE", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        } else {
            Span::styled("OFF", Style::default().fg(Color::Green))
        };

        let content = vec![
            Line::from(vec![Span::raw("Kill Switch: "), kill_switch]),
            Line::from(format!("Size: {:.1}", state.current_size)),
            Line::from(format!(
                "Max Price YES: {:.2} | Max Price NO: {:.2}",
                state.max_price_yes, state.max_price_no
            )),
        ];

        let panel = Paragraph::new(content)
            .block(Block::default().borders(Borders::ALL).title("Trading Config"));

        f.render_widget(panel, area);
    }

    fn render_actions_panel(&self, f: &mut Frame, area: Rect) {
        let entries = self.trade.get_action_log();
        let items: Vec<ListItem> = entries
            .iter()
            .map(|e| {
                let line = e.format_short();
                let style = if line.contains("Buy YES") {
                    Style::default().fg(Color::Green)
                } else if line.contains("Buy NO") {
                    Style::default().fg(Color::Red)
                } else if line.contains("Kill switch") {
                    Style::default().fg(Color::Yellow)
                } else if line.contains("Size") || line.contains("Max ") {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                };
                ListItem::new(line).style(style)
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Actions (your recent activity)"));

        f.render_widget(list, area);
    }

    fn render_logs_panel(&self, f: &mut Frame, area: Rect) {
        let entries = self.log_buffer.get_entries();
        // Show only the most recent entries that fit
        let visible_height = area.height.saturating_sub(2) as usize; // subtract border
        let skip = entries.len().saturating_sub(visible_height);
        let items: Vec<ListItem> = entries
            .iter()
            .skip(skip)
            .map(|e| {
                let line = e.format_short();
                let style = match e.level {
                    tracing::Level::ERROR => Style::default().fg(Color::Red),
                    tracing::Level::WARN  => Style::default().fg(Color::Yellow),
                    tracing::Level::INFO  => Style::default().fg(Color::White),
                    tracing::Level::DEBUG => Style::default().fg(Color::DarkGray),
                    _                     => Style::default().fg(Color::DarkGray),
                };
                ListItem::new(line).style(style)
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Logs (tracing output)"));

        f.render_widget(list, area);
    }

    fn render_help_panel(&self, f: &mut Frame, area: Rect) {
        let content = vec![
            Line::from(vec![
                Span::styled("Trading:", Style::default().add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("  y", Style::default().fg(Color::Green)),
                Span::raw(" Buy YES    "),
                Span::styled("n", Style::default().fg(Color::Red)),
                Span::raw(" Buy NO    "),
                Span::styled("k", Style::default().fg(Color::Yellow)),
                Span::raw(" Toggle Kill Switch"),
            ]),
            Line::from(vec![
                Span::styled("Size/Price:", Style::default().add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("  +/-", Style::default().fg(Color::Cyan)),
                Span::raw(" Adjust order size (±5)    "),
                Span::styled("[/]", Style::default().fg(Color::Cyan)),
                Span::raw(" Max YES price (±0.01)"),
            ]),
            Line::from(vec![
                Span::styled("  {/}", Style::default().fg(Color::Cyan)),
                Span::raw(" Max NO price (±0.01)"),
            ]),
            Line::from(vec![
                Span::styled("System:", Style::default().add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("  q", Style::default().fg(Color::Magenta)),
                Span::raw(" Quit    "),
                Span::styled("Ctrl+C", Style::default().fg(Color::Magenta)),
                Span::raw(" Force exit"),
            ]),
        ];

        let panel = Paragraph::new(content)
            .block(Block::default().borders(Borders::ALL).title("Hotkeys"));

        f.render_widget(panel, area);
    }
}
