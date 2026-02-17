# ekubo-arb

Starknet arbitrage bot that monitors [Ekubo](https://ekubo.org/) DEX pools for cyclic arbitrage opportunities.

## How it works

ekubo-arb subscribes to new blocks via a Starknet WebSocket endpoint, replays swap events to maintain local pool state, and evaluates multi-hop arbitrage routes. When a profitable route is found it builds and broadcasts an INVOKE v3 transaction through the Ekubo router contract.

## Requirements

- Rust 1.80+
- A Starknet JSON-RPC endpoint
- A Starknet WebSocket endpoint (`starknet_subscribeEvents`)
- A funded Starknet account (address + private key)
- A pool-state snapshot JSON file (see `APP_FROM_BLOCK`)

## Quick start

```bash
cp .env.example .env
# Fill in APP_RPC_URL, APP_RPC_WS_URL, APP_ACCOUNT_ADDRESS, APP_ACCOUNT_PRIVATE_KEY
cargo run --release
```

## Configuration

All options can be set via CLI flags or environment variables.

| Variable | Required | Description |
|---|---|---|
| `APP_RPC_URL` | yes | Starknet JSON-RPC endpoint |
| `APP_RPC_WS_URL` | yes | Starknet WebSocket endpoint |
| `APP_ACCOUNT_ADDRESS` | yes | Account address (hex) |
| `APP_ACCOUNT_PRIVATE_KEY` | yes | Account private key (hex) |
| `APP_FROM_BLOCK` | yes | Snapshot block number — loads `{block}.json` and syncs from block+1 |
| `APP_MIN_PROFIT_HBIP` | no | Minimum profit in hundredth basis points (default: `100` = 1 bip) |
| `APP_BROADCAST` | no | Broadcast transactions on-chain (default: `false`) |
| `APP_HEALTH_PORT` | no | Health server port (default: `8080`) |
| `RUST_LOG` | no | Log level filter (default: `info`) |
| `APP_LOG_FORMAT` | no | Set to `json` for structured logging |

### Transaction tip

The Starknet v3 tip is a per-gas-unit priority fee (like EIP-1559). It is derived as `DEFAULT_TIP_PERCENTAGE` of the expected profit divided by the L2 gas max amount. The percentage is controlled by the `DEFAULT_TIP_PERCENTAGE` constant in `src/constants.rs` (default: 40%).

## Utilities

Standalone tools in `utils/`, registered as Cargo examples.

### sync_state

Advance a pool-state JSON snapshot to a newer block by replaying on-chain events:

```bash
cargo run --example sync_state -- --rpc-url https://your-rpc.com --from 6570500 --to 6633730
```

Loads `6570500.json`, fetches events for blocks 6570501–6633730, and writes the updated state to `6633730.json`.

### find_opportunity

Evaluate arbitrage routes from a local state snapshot (no RPC needed):

```bash
cargo run --example find_opportunity -- --json-path 6633730.json --amount 5000000000000000000000
```

Options: `--token` (hex, defaults to STRK), `--amount` (wei).

### calculate_swap

Compute all swap paths and show detailed hop-by-hop results:

```bash
cargo run --example calculate_swap
```

Note: token and amount are currently hardcoded in the source (`utils/calculate_swap.rs`).

## Health endpoints

The built-in HTTP server exposes:

- `GET /health` — liveness probe
- `GET /ready` — readiness probe (gas prices loaded and workers running)

## License

MIT
