# Stage-1 Realtime Monitor + CLI Hotkeys for Polymarket BTC 15-min

## Key goals (Stage-1)

| Goal | Description | Why it matters | Out of scope (Stage-1) |
|---|---|---|---|
| Realtime feeds | Stream and maintain **Binance BTCUSDT perp order book** and **Polymarket CLOB** best bid/ask for the chosen YES/NO tokens | Enables lead/lag observation and fast manual reactions | Automatic market discovery for “current 15-min market” (we hardcode token IDs) |
| CLI hotkeys trading | Provide a fast terminal UI with hotkeys: `y` buy YES, `n` buy NO, `k` kill-switch | Low-friction “manual execution” before full automation | Fully autonomous trading / hedging logic |
| Strong logging | Log snapshots + trades + health events (JSONL), rotated by day | Enables later analysis/backtest and debugging | Full tick-by-tick depth storage (unless sampled) |
| Safety guardrails | Size limits, max price limits, spread/liquidity checks, kill-switch | Prevents accidental losses and fat-finger trades | Advanced risk engine and portfolio management |

---

## System components

| Component | Responsibilities | Inputs | Outputs | Stop criteria (definition of done) |
|---|---|---|---|---|
| BinanceBookService | Maintain local book using REST snapshot + WS diffs; compute mid, returns, imbalance | WS depth diffs + REST snapshot | Top-of-book + derived metrics events | Local book stays consistent (no sequence gaps); publishes metrics at configured rate |
| PolyMarketService | Subscribe to Polymarket market WS; track YES/NO best bid/ask and updates | WS market messages + optional REST fallback | YES/NO quotes + staleness metrics | Receives updates continuously; handles reconnect; provides best bid/ask |
| SignalService | Detect “Binance moved, Poly lagged” heuristic and emit suggestion | Binance metrics + Poly quotes | Signal events: suggested side, confidence, reasons | Emits signals when thresholds met; does not crash on missing data |
| TradeService | Place orders via Polymarket CLOB; enforce safety checks; expose hotkey actions | Hotkey commands + Poly quotes + config | Order request/response + fills logs | Successful paper order in dry-run; real order in live mode with max-price protection |
| TUI/CLI | Render status; accept hotkeys; show config and current signal | Internal state bus | Commands to TradeService | Hotkeys work; display updates smoothly; kill-switch blocks orders |
| Logger | Append-only JSONL snapshot logs + trade logs + health logs; daily rotation | Normalized events | `.jsonl` files | Files created; entries valid JSON; rotation works; no secrets logged |

---

## Implementation plan (milestones)

### Milestone 0 — Repo skeleton + config + logging base

| Item | Task | Deliverable | Acceptance / Stop criteria |
|---|---|---|---|
| M0.1 | Create Rust workspace/binary, module layout, config loader | `cargo run` prints loaded config | `cargo build` succeeds; config parsed; invalid config fails with clear error |
| M0.2 | Implement JSONL logger with rotation (by date) | `data/logs/YYYY-MM-DD/*.jsonl` created | Starting app creates log directory + file; at least 1 log line written within 5s |
| M0.3 | Define normalized event schema | Rust structs + serialization | Logs contain well-formed JSON objects with `t_recv_ms` |

**Stop criteria:**  
- ✅ `cargo build` passes  
- ✅ Run produces a log file with at least one valid JSON line

---

### Milestone 1 — Binance order book (correct local book)

| Item | Task | Deliverable | Acceptance / Stop criteria |
|---|---|---|---|
| M1.1 | WS client connect to Binance depth diff stream | Connection + incoming messages | Reconnect on drop; no panic for 10 minutes |
| M1.2 | REST snapshot fetch + diff-buffer apply per Binance spec | Local book data structure | Book update id monotonic; rejects invalid sequences; logs sequence gaps |
| M1.3 | Derived metrics: best bid/ask, mid, returns(1s/3s/10s), imbalance topN | Periodic `BinanceSnapshot` events | Snapshot event emitted at configured rate (e.g., 4Hz or 1Hz) |

**Stop criteria:**  
- ✅ Local book initializes (snapshot + diffs) and stays “in sync” for 30 min (no sequence gap errors)  
- ✅ Snapshot logs show non-null bid/ask/mid updating

---

### Milestone 2 — Polymarket market data (YES/NO quotes)

| Item | Task | Deliverable | Acceptance / Stop criteria |
|---|---|---|---|
| M2.1 | WS connect to Polymarket CLOB market channel | Connection + subscription | Receives market updates; reconnect works |
| M2.2 | Hardcode YES/NO token IDs in config and subscribe | YES/NO quote tracker | Quote updates appear in logs; includes bid/ask + sizes |
| M2.3 | Staleness metric (time since last Poly update) | `poly_last_update_ms` | Staleness updates correctly; warnings logged if stale > threshold |

**Stop criteria:**  
- ✅ Polymarket quotes for both YES and NO appear in snapshot logs  
- ✅ If WS disconnects, it reconnects and resumes updates automatically

---

### Milestone 3 — Unified snapshots + basic signal

| Item | Task | Deliverable | Acceptance / Stop criteria |
|---|---|---|---|
| M3.1 | Combine Binance + Poly into unified 1Hz (or 4Hz) snapshot line | `events_snapshot.jsonl` | Each line includes Binance mid + returns + Poly YES/NO quotes |
| M3.2 | Implement simple divergence signal heuristic | `SignalEvent` | Signal triggers on synthetic tests; emits “suggest YES/NO” |
| M3.3 | Display signal on CLI (non-interactive first) | Terminal prints updated status | Prints refresh without flicker; does not spam excessively |

**Stop criteria:**  
- ✅ Unified snapshot lines present with both feeds  
- ✅ Signal triggers in controlled test (replay or forced thresholds)

---

### Milestone 4 — TUI + hotkeys (manual execution UX)

| Item | Task | Deliverable | Acceptance / Stop criteria |
|---|---|---|---|
| M4.1 | Terminal UI layout: Binance panel, Poly panel, signal panel, config panel | TUI screen | Updates at stable cadence; CPU usage reasonable |
| M4.2 | Hotkeys: `y` buy YES, `n` buy NO, `k` kill-switch, `+/-` size, `[` `]` max price | Interactive TUI | Key presses change state instantly; kill-switch blocks order sending |
| M4.3 | Dry-run mode (no real order placement) | `--dry-run` flag | Hotkeys produce “order intent” logs without network calls |

**Stop criteria:**  
- ✅ Manual hotkey test: pressing `y/n` produces an order intent log in dry-run  
- ✅ Kill-switch prevents any “send order” path

---

### Milestone 5 — Polymarket order placement (live trading path)

| Item | Task | Deliverable | Acceptance / Stop criteria |
|---|---|---|---|
| M5.1 | Implement Polymarket auth (per CLOB docs) and `POST /order` | `TradeService` live | Can place an order in sandbox/live env (small size) |
| M5.2 | Safety checks: max price, max size, spread cap, stale quote cap | Risk gate | Refuses orders with clear error message + log reason |
| M5.3 | Trade logging: request/response timing, client_order_id, fills | `trades.jsonl` | Every order attempt produces a log line; responses sanitized (no secrets) |

**Stop criteria:**  
- ✅ `cargo build` + `cargo test` pass  
- ✅ Live mode sends ONE small test order successfully (or fails gracefully with actionable error)  
- ✅ `trades.jsonl` contains order request + response entries

---

## Testing plan (Stage-1)

| Test type | What to test | How | Pass criteria |
|---|---|---|---|
| Compile | Rust build | `cargo build` | exits 0 |
| Lint (recommended) | Clippy | `cargo clippy -- -D warnings` | exits 0 |
| Unit tests | Order book apply logic, signal thresholds, config parsing | `cargo test` | exits 0 |
| Integration smoke | Connect to Binance WS + build local book | run binary for 10–30 min | no panic; snapshots update |
| Integration smoke | Connect to Polymarket WS and receive quotes | run binary for 10–30 min | quotes update; staleness not stuck |
| CLI hotkeys | `y/n/+/-/[ ]/k` | manual interactive run | state changes and logs produced |
| Logging | Snapshots + trades + health | inspect JSONL files | files exist; valid JSON; no secrets |

---

## Logging schema (minimum fields)

### Snapshot log (`events_snapshot.jsonl`)

| Field | Type | Description |
|---|---|---|
| `t_recv_ms` | int | local receive timestamp (ms) |
| `binance_mid` | float | computed mid |
| `binance_ret_1s` | float | 1-second return |
| `binance_obi_topN` | float | order book imbalance |
| `poly_yes_bid/ask` | float | best bid/ask |
| `poly_no_bid/ask` | float | best bid/ask |
| `poly_spread_yes/no` | float | ask - bid |
| `poly_stale_ms` | int | time since last update |
| `signal_side` | string | `YES` / `NO` / `NONE` |
| `signal_score` | float | heuristic score |

### Trade log (`trades.jsonl`)

| Field | Type | Description |
|---|---|---|
| `t_send_ms` / `t_resp_ms` | int | request/response times |
| `client_order_id` | string | unique client id |
| `side` | string | `YES` / `NO` |
| `size` | float | size in shares or USDC (choose one) |
| `limit_price` | float | max price |
| `post_only` | bool | maker intent |
| `mode` | string | `dry_run` / `live` |
| `risk_reject_reason` | string? | if blocked |
| `api_status` | string/int | response status |
| `fills` | array? | fill details if available |

---

## Stage-1 “stop criteria” (overall)

Stage-1 is complete when ALL are true:

| Category | Stop criteria |
|---|---|
| Build | ✅ `cargo build` succeeds |
| Tests | ✅ `cargo test` succeeds (and ideally `cargo clippy -- -D warnings`) |
| Binance feed | ✅ Local book initializes and snapshots update for 30 minutes without panic |
| Polymarket feed | ✅ YES/NO quotes update for 30 minutes without panic; reconnect works |
| CLI hotkeys | ✅ `y` and `n` create order intents in `--dry-run`; `k` blocks |
| Logging | ✅ Snapshot log exists and updates; trade log exists (at least in dry-run) |
| Safety | ✅ Orders blocked if quotes stale/spread too wide/max price exceeded |
| Secrets | ✅ No private keys/API secrets appear in any logs |

---

## Notes / assumptions (explicit)

- Token IDs are **hardcoded in config** for Stage-1 (no discovery).
- “Buy” is implemented as a **limit order with max price** (even if intended as marketable).
- Stage-1 focus is **data collection + manual execution**, not profitability.
