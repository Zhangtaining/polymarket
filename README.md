# Polymarket BTC 15-min Monitor

Real-time monitoring and manual trading CLI for Polymarket BTC 15-minute prediction markets.

## Features

- **Binance Order Book**: Real-time BTCUSDT order book with returns and imbalance metrics
- **Polymarket Quotes**: YES/NO token price tracking with staleness detection
- **Signal Generation**: Detects divergence between Binance moves and Polymarket updates
- **TUI Interface**: Terminal UI with hotkey-based manual trading
- **Safety Guardrails**: Kill-switch, size limits, max price limits, spread/staleness checks
- **JSONL Logging**: Structured logs with daily rotation

## Quick Start

```bash
# Build
cargo build

# Run tests
cargo test

# Run with TUI (dry-run mode)
cargo run -- --dry-run

# Run headless (for data collection)
cargo run -- --headless --dry-run
```

## Hotkeys

| Key | Action |
|-----|--------|
| `y` | Buy YES |
| `n` | Buy NO |
| `k` | Toggle kill-switch |
| `+`/`-` | Adjust size (+/-5) |
| `[`/`]` | Adjust max YES price (+/-0.01) |
| `{`/`}` | Adjust max NO price (+/-0.01) |
| `q` | Quit |

## Configuration

Edit `config/default.toml`:

```toml
[general]
dry_run = true
snapshot_rate_hz = 1

[binance]
ws_url = "wss://stream.binance.us:9443/ws/btcusd@depth@100ms"
rest_url = "https://api.binance.us/api/v3/depth"
symbol = "BTCUSD"

[polymarket]
yes_token_id = "your_yes_token_id"
no_token_id = "your_no_token_id"

[trading]
default_size = 10.0
max_size = 100.0
max_price_yes = 0.95
max_price_no = 0.95
max_spread = 0.10
stale_quote_threshold_ms = 5000
```

## Log Files

Logs are written to `data/logs/YYYY-MM-DD/`:

- `events_snapshot.jsonl` - Market data snapshots (1Hz)
- `trades.jsonl` - Order attempts and results
- `health.jsonl` - System health events

## Safety Features

Orders are blocked if:
- Kill-switch is active
- Size exceeds max_size
- Limit price exceeds max_price
- Spread exceeds max_spread
- Quote is stale (> stale_quote_threshold_ms)

## Development

```bash
# Lint
cargo clippy -- -D warnings

# Format
cargo fmt
```
