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
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use std::io::{self, Stdout};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::events::TradeSide;
use crate::services::{BinanceBookService, PolymarketService, SignalService, TradeService};

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
    signal: Arc<SignalService>,
    trade: Arc<TradeService>,
    command_tx: mpsc::Sender<TuiCommand>,
    command_rx: mpsc::Receiver<TuiCommand>,
    dry_run: bool,
}

impl App {
    pub fn new(
        binance: Arc<BinanceBookService>,
        polymarket: Arc<PolymarketService>,
        signal: Arc<SignalService>,
        trade: Arc<TradeService>,
        dry_run: bool,
    ) -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            binance,
            polymarket,
            signal,
            trade,
            command_tx: tx,
            command_rx: rx,
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
                Constraint::Length(3),  // Header
                Constraint::Length(8),  // Binance panel
                Constraint::Length(8),  // Polymarket panel
                Constraint::Length(6),  // Signal panel
                Constraint::Length(8),  // Trading config panel
                Constraint::Min(0),     // Hotkeys help
            ])
            .split(f.size());

        self.render_header(f, chunks[0]);
        self.render_binance_panel(f, chunks[1]);
        self.render_polymarket_panel(f, chunks[2]);
        self.render_signal_panel(f, chunks[3]);
        self.render_trading_panel(f, chunks[4]);
        self.render_help_panel(f, chunks[5]);
    }

    fn render_header(&self, f: &mut Frame, area: Rect) {
        let mode = if self.dry_run { "DRY RUN" } else { "LIVE" };
        let mode_style = if self.dry_run {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        };

        let header = Paragraph::new(Line::from(vec![
            Span::raw("Polymarket BTC 15-min Monitor | Mode: "),
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

        let content = if let Some(u) = update {
            let mid: f64 = u.mid.to_string().parse().unwrap_or(0.0);
            let bid: f64 = u.best_bid.to_string().parse().unwrap_or(0.0);
            let ask: f64 = u.best_ask.to_string().parse().unwrap_or(0.0);

            format!(
                "Mid: ${:.2}\nBid: ${:.2} | Ask: ${:.2}\nSpread: ${:.2}\n\
                 Returns: 1s={:+.4}% | 3s={:+.4}% | 10s={:+.4}%\n\
                 Imbalance (top5): {:+.3}",
                mid,
                bid,
                ask,
                ask - bid,
                ret_1s.unwrap_or(0.0) * 100.0,
                ret_3s.unwrap_or(0.0) * 100.0,
                ret_10s.unwrap_or(0.0) * 100.0,
                u.imbalance_top5
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

        let yes_spread = match (quotes.yes_bid, quotes.yes_ask) {
            (Some(b), Some(a)) => format!("{:.3}", a - b),
            _ => "N/A".to_string(),
        };

        let no_spread = match (quotes.no_bid, quotes.no_ask) {
            (Some(b), Some(a)) => format!("{:.3}", a - b),
            _ => "N/A".to_string(),
        };

        let _stale_color = if stale_ms > 5000 {
            Color::Red
        } else if stale_ms > 2000 {
            Color::Yellow
        } else {
            Color::Green
        };

        let content = format!(
            "YES: Bid={:.3} | Ask={:.3} | Spread={}\n\
             NO:  Bid={:.3} | Ask={:.3} | Spread={}\n\n\
             Staleness: {}ms",
            quotes.yes_bid.unwrap_or(0.0),
            quotes.yes_ask.unwrap_or(0.0),
            yes_spread,
            quotes.no_bid.unwrap_or(0.0),
            quotes.no_ask.unwrap_or(0.0),
            no_spread,
            if stale_ms == i64::MAX { "N/A".to_string() } else { stale_ms.to_string() }
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

    fn render_help_panel(&self, f: &mut Frame, area: Rect) {
        let content = vec![
            Line::from("Hotkeys:"),
            Line::from("  y: Buy YES  |  n: Buy NO  |  k: Toggle Kill Switch  |  q: Quit"),
            Line::from("  +/-: Adjust size  |  [/]: Max YES price  |  {/}: Max NO price"),
        ];

        let panel = Paragraph::new(content)
            .block(Block::default().borders(Borders::ALL).title("Help"));

        f.render_widget(panel, area);
    }
}
