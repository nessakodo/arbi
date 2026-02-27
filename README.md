# arbi

Starknet arbitrage bot that monitors [Ekubo](https://ekubo.org/) DEX pools for cyclic arbitrage opportunities. It subscribes to new blocks via WebSocket, maintains local pool state by replaying swap events, evaluates multi-hop routes across ETH/USDC/USDT/STRK pairs, and broadcasts profitable INVOKE v3 transactions through the Ekubo router contract.

Includes a real-time monitoring dashboard on the same port as the health server.

---

## Architecture

```
Starknet Node (WS)
    │
    ▼
┌────────────────────────────────────────────────┐
│  arbi process (single binary)                  │
│                                                │
│  WebSocket ──► Event Replay ──► Pool State     │
│                                    │           │
│                               Path Finder      │
│                              (DFS, 2-4 hops)   │
│                                    │           │
│                              Evaluator          │
│                          (ternary search)       │
│                                    │           │
│                              Broadcaster        │
│                          (INVOKE v3 tx)         │
│                                    │           │
│  DashboardState ◄── snapshots ─────┘           │
│       │                                        │
│  Axum HTTP Server (:8080)                      │
│   ├─ /health, /ready, /metrics                 │
│   ├─ /api/snapshot, /api/events (SSE)          │
│   ├─ /api/opportunities, /api/pnl             │
│   └─ /* (dashboard static files)              │
└────────────────────────────────────────────────┘
```

## Prerequisites

- **Rust 1.90+** (uses features stabilized in recent nightlies)
- **Node.js 18+** (for building the dashboard frontend)
- A **Starknet JSON-RPC endpoint** (e.g., Alchemy, Infura, Blast)
- A **Starknet WebSocket endpoint** that supports `starknet_subscribeEvents`
- A **funded Starknet account** (address + private key in hex)
- A **pool-state snapshot** JSON file (see Utilities below)

## Quick Start

### 1. Clone and configure

```bash
git clone <repo-url> arbi
cd arbi
cp .env.example .env
```

Edit `.env` with your credentials:

```env
APP_RPC_URL=https://starknet-mainnet.g.alchemy.com/v2/YOUR_KEY
APP_RPC_WS_URL=wss://starknet-mainnet.g.alchemy.com/v2/YOUR_KEY
APP_ACCOUNT_ADDRESS=0xYOUR_ACCOUNT_ADDRESS
APP_ACCOUNT_PRIVATE_KEY=0xYOUR_PRIVATE_KEY
APP_FROM_BLOCK=6633730
APP_BROADCAST=false
APP_MIN_PROFIT_HBIP=100
```

### 2. Build the dashboard

```bash
cd dashboard
npm install
npm run build
cd ..
```

This produces `dashboard/dist/` which is served by the bot on port 8080.

### 3. Build and run

```bash
cargo build --release
cargo run --release
```

Open `http://localhost:8080` to see the dashboard.

### 4. Paper trading (default)

With `APP_BROADCAST=false`, the bot evaluates opportunities but does not send transactions. Use this to verify everything works, observe opportunity frequency, and tune parameters.

### 5. Go live

Once satisfied with paper trading results:

```env
APP_BROADCAST=true
APP_MIN_PROFIT_HBIP=100    # 1 basis point minimum
APP_TIP_PERCENTAGE=0       # keep all profit (default)
APP_MAX_HOPS=3             # 2-4 hops per route
```

Restart the bot. The dashboard header chip will switch from PAPER to LIVE.

---

## Configuration

All options are set via CLI flags or environment variables.

| Variable | Required | Default | Description |
|---|---|---|---|
| `APP_RPC_URL` | yes | — | Starknet JSON-RPC endpoint |
| `APP_RPC_WS_URL` | yes | — | Starknet WebSocket endpoint |
| `APP_ACCOUNT_ADDRESS` | yes | — | Account address (hex) |
| `APP_ACCOUNT_PRIVATE_KEY` | yes | — | Account private key (hex) |
| `APP_FROM_BLOCK` | yes | — | Snapshot block number — loads `{block}.json`, syncs from block+1 |
| `APP_BROADCAST` | no | `false` | Broadcast transactions on-chain |
| `APP_MIN_PROFIT_HBIP` | no | `100` | Minimum profit in hundredth basis points (100 = 1 bip = 0.01%) |
| `APP_TIP_PERCENTAGE` | no | `0` | Percentage of profit to use as Starknet v3 tip (0–100) |
| `APP_MAX_HOPS` | no | `3` | Maximum hops per arbitrage path (2–4) |
| `APP_HEALTH_PORT` | no | `8080` | Health + dashboard server port |
| `RUST_LOG` | no | `info` | Log level filter (e.g., `debug`, `ekubo_arb=debug`) |
| `APP_LOG_FORMAT` | no | `text` | Set to `json` for structured JSON logging |

### Parameter tuning guide

| Parameter | Lower value | Higher value |
|---|---|---|
| `MIN_PROFIT_HBIP` | More executions, smaller profits, higher gas risk | Fewer executions, only high-conviction trades |
| `TIP_PERCENTAGE` | Keep more profit, lower priority in block | Higher priority, less profit kept |
| `MAX_HOPS` | Fewer paths, faster evaluation | More paths, more opportunities, slower evaluation |

**Recommended starting values:** `MIN_PROFIT_HBIP=100`, `TIP_PERCENTAGE=0`, `MAX_HOPS=3`

---

## Dashboard

The monitoring dashboard is served at `http://localhost:{APP_HEALTH_PORT}` (default 8080).

### Layout

| Left Column | Center | Right Column |
|---|---|---|
| Opportunities Found | Profit Chart (time series) | Alerts |
| Above Threshold | Execution Log | Live Opportunity Feed |
| Batches Evaluated | | Transaction Log |
| Gas Prices (L1/L2) | | |
| Config (read-only) | | |

### Real-time updates

The dashboard connects via Server-Sent Events (SSE) to `/api/events`. The header shows connection status:
- **PAPER** / **LIVE** — broadcast mode
- **WS** — WebSocket connection to Starknet node
- **SSE OFF** — dashboard lost connection to bot (auto-reconnects)

### API Endpoints

| Method | Path | Description |
|---|---|---|
| GET | `/health` | Liveness probe (always 200) |
| GET | `/ready` | Readiness probe (200 when gas prices loaded + workers ready) |
| GET | `/metrics` | Basic JSON metrics |
| GET | `/api/snapshot` | Full current bot state |
| GET | `/api/opportunities?limit=50` | Recent opportunity history |
| GET | `/api/pnl?limit=100` | P&L history |
| GET | `/api/events` | SSE stream of snapshot updates |

### Development mode

For frontend development with hot reload:

```bash
cd dashboard
npm run dev
```

Vite proxies API requests to `localhost:8080` (the running bot).

---

## Utilities

Standalone tools in `utils/`, registered as Cargo examples.

### sync_state

Advance a pool-state JSON snapshot to a newer block by replaying on-chain events:

```bash
cargo run --example sync_state -- --rpc-url https://your-rpc.com --from 6570500 --to 6633730
```

Loads `6570500.json`, fetches events for blocks 6570501–6633730, writes `6633730.json`.

### find_opportunity

Evaluate arbitrage routes from a local state snapshot (no RPC needed):

```bash
cargo run --example find_opportunity -- --json-path 6633730.json --amount 5000000000000000000000
```

Options: `--token` (hex, defaults to STRK), `--amount` (wei).

### calculate_swap

Compute swap paths with detailed hop-by-hop results:

```bash
cargo run --example calculate_swap
```

Note: token and amount are hardcoded in `utils/calculate_swap.rs`.

---

## Deployment

### systemd (recommended for VPS)

Create `/etc/systemd/system/arbi.service`:

```ini
[Unit]
Description=arbi - Starknet arbitrage bot
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=arbi
WorkingDirectory=/opt/arbi
EnvironmentFile=/opt/arbi/.env
ExecStart=/opt/arbi/target/release/ekubo-arb
Restart=always
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

```bash
# Deploy
sudo cp target/release/ekubo-arb /opt/arbi/
sudo cp -r dashboard/dist /opt/arbi/dashboard/dist
sudo cp .env /opt/arbi/.env
sudo cp *.json /opt/arbi/

# Enable and start
sudo systemctl daemon-reload
sudo systemctl enable arbi
sudo systemctl start arbi

# Check status / logs
sudo systemctl status arbi
sudo journalctl -u arbi -f
```

### Docker (alternative)

```dockerfile
FROM rust:1.93 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM node:20-alpine AS frontend
WORKDIR /app/dashboard
COPY dashboard/package.json dashboard/package-lock.json ./
RUN npm ci
COPY dashboard/ ./
RUN npm run build

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/ekubo-arb /usr/local/bin/
COPY --from=frontend /app/dashboard/dist /opt/arbi/dashboard/dist
WORKDIR /opt/arbi
ENTRYPOINT ["ekubo-arb"]
```

### Monitoring

- **Health check**: `curl http://localhost:8080/health`
- **Readiness**: `curl http://localhost:8080/ready` (returns 503 until fully initialized)
- **Dashboard**: `http://localhost:8080` in browser

The bot automatically retries on failure with exponential backoff (2s → 60s, max 10 retries).

---

## Investment & Risk

### Minimum investment

- **Gas reserve**: ~$50 in ETH on Starknet for transaction fees
- **Working capital**: The bot uses your account's token balances for swaps. Start with at least **$500** across supported tokens (ETH, USDC, USDT, STRK)
- **Recommended**: $2,000+ for meaningful returns. Larger capital allows capturing bigger opportunities

### Tokens

The bot monitors arbitrage cycles for:
- **ETH** (WETH on Starknet)
- **USDC** (Bridged USDC)
- **USDT** (Bridged USDT)
- **STRK** (Starknet native token)

Your account needs balances in whichever tokens you want the bot to trade.

### Risk factors

| Risk | Mitigation |
|---|---|
| **Failed transactions** | You pay gas even if the trade reverts. Tight slippage bounds (1%) reduce this |
| **MEV competition** | Other bots may front-run. Speed (PRE_CONFIRMED finality) and low tip help |
| **Smart contract risk** | Only interacts with Ekubo router (audited, battle-tested) |
| **Stale state** | If WebSocket drops, pool state drifts. Bot detects and reconnects |
| **Gas spikes** | L1 data gas can spike, eating profits. Monitor gas panel on dashboard |

### Realistic expectations

- Arbitrage is **competitive**. Profits depend on market activity and inefficiency
- Most opportunities are small (< 10 basis points)
- Paper trade first to see real opportunity frequency in current market
- This is not passive income — monitor regularly, tune parameters
- Returns are highly variable: some days zero opportunities, some days many

---

## License

MIT
