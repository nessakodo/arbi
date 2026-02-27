//! ekubo-arb - Starknet arbitrage bot monitoring Ekubo DEX pools
//!
//! This library provides:
//! - Ekubo pool state management and swap simulation
//! - Cyclic arbitrage path finding and evaluation
//! - Real-time event streaming via WebSocket
//! - Transaction building, signing, and broadcasting via JSON-RPC
//! - Health check HTTP server for Kubernetes probes

pub mod account;
pub mod arbitrager;
pub mod constants;
pub mod dashboard;
pub mod ekubo;
pub mod errors;
pub mod gas;
pub mod health;
pub mod opportunity;
pub mod rpc;
pub mod transaction;
pub mod ws;

pub use arbitrager::{run_arbitrager, Arbitrager, ArbitragerConfig, ArbitragerError, Simulator};
pub use opportunity::ArbitrageOpportunity;

pub use ekubo::state::{EvaluationRouteResult, GlobalOptimalResult, State};

pub use account::Account;

pub use constants::CHAIN_ID_MAINNET;

pub use dashboard::state::DashboardState;

pub use health::{start_health_server, HealthState, DEFAULT_HEALTH_PORT};
