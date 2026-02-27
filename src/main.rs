//! ekubo-arb main entry point
//!
//! Initializes the health server and starts the arbitrager with robust error handling.

use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use ekubo_arb::{run_arbitrager, start_health_server, ArbitragerConfig, DashboardState, HealthState};
use tokio::signal;
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, EnvFilter};

/// Maximum number of consecutive failures before giving up
const MAX_CONSECUTIVE_FAILURES: u32 = 10;

/// Base delay between retries (doubles each attempt, capped at 60s)
const BASE_RETRY_DELAY_SECS: u64 = 2;

/// Maximum retry delay
const MAX_RETRY_DELAY_SECS: u64 = 60;

#[derive(Parser)]
#[command(name = "ekubo-arb")]
#[command(about = "Starknet arbitrage bot with health monitoring")]
struct Args {
    /// RPC URL for fetching events and state
    #[arg(long, env = "APP_RPC_URL")]
    rpc_url: String,

    /// Account address for transaction signing (hex format)
    #[arg(long, env = "APP_ACCOUNT_ADDRESS")]
    account_address: String,

    /// Private key for the account (hex format)
    #[arg(long, env = "APP_ACCOUNT_PRIVATE_KEY")]
    account_private_key: String,

    /// Snapshot block number. Loads {from}.json and syncs from block {from + 1}
    #[arg(long, env = "APP_FROM_BLOCK")]
    from: u64,

    /// Broadcast transactions on-chain
    #[arg(long, env = "APP_BROADCAST", default_value_t = false)]
    broadcast: bool,

    /// Minimum profit in hundredth basis points to trigger execution
    /// 100 = 1 BIP = 0.01%, 10000 = 1%
    #[arg(long, env = "APP_MIN_PROFIT_HBIP", default_value_t = 100)]
    min_profit_hbip: i128,

    /// Percentage of profit to tip (0–100, default 0 keeps all profit)
    #[arg(long, env = "APP_TIP_PERCENTAGE", default_value_t = 0)]
    tip_percentage: u64,

    /// Maximum hops in arbitrage paths (default 3, max 4)
    #[arg(long, env = "APP_MAX_HOPS", default_value_t = 3)]
    max_hops: usize,

    /// Health server port
    #[arg(long, env = "APP_HEALTH_PORT", default_value_t = 8080)]
    health_port: u16,

    /// WebSocket URL for real-time event streaming
    #[arg(long, env = "APP_RPC_WS_URL")]
    rpc_ws_url: String,
}

fn init_logging() {
    let use_json = std::env::var("APP_LOG_FORMAT")
        .map(|v| v.to_lowercase() == "json")
        .unwrap_or(false);

    // Use EnvFilter to respect RUST_LOG environment variable, defaulting to "info"
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    if use_json {
        fmt()
            .json()
            .flatten_event(true)
            .with_env_filter(env_filter)
            .init();
    } else {
        fmt().with_env_filter(env_filter).with_target(false).init();
    }
}

/// Calculate retry delay with exponential backoff
fn retry_delay(attempt: u32) -> Duration {
    let delay_secs = BASE_RETRY_DELAY_SECS.saturating_mul(2u64.saturating_pow(attempt));
    Duration::from_secs(delay_secs.min(MAX_RETRY_DELAY_SECS))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    let args = Args::parse();

    // Initialize health state - starts as NOT ready
    let health_state = Arc::new(HealthState::new());

    // Initialize dashboard state for API
    let dashboard_state = Arc::new(DashboardState::new());

    // Start health + dashboard server in the background
    let health_port = args.health_port;
    let health_state_clone = Arc::clone(&health_state);
    let dashboard_state_clone = Arc::clone(&dashboard_state);
    tokio::spawn(async move {
        start_health_server(health_state_clone, Some(dashboard_state_clone), health_port).await;
    });

    let json_path = format!("{}.json", args.from);

    info!(
        health_port,
        json_path = %json_path,
        from_block = args.from,
        broadcast = args.broadcast,
        "ekubo-arb starting"
    );

    // Build arbitrager config
    let config = ArbitragerConfig::new(
        &json_path,
        &args.rpc_url,
        &args.rpc_ws_url,
        &args.account_address,
        &args.account_private_key,
    )
    .with_from_block(args.from + 1)
    .with_broadcast(args.broadcast)
    .with_min_profit_hbip(args.min_profit_hbip)
    .with_tip_percentage(args.tip_percentage)
    .with_max_hops(args.max_hops);

    // Create shutdown channel for graceful shutdown
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Run the arbitrager with retry logic
    let health_state_for_run = Arc::clone(&health_state);
    let dashboard_state_for_run = Arc::clone(&dashboard_state);
    let mut arbitrager_handle = tokio::spawn(async move {
        run_with_retries(config, health_state_for_run, dashboard_state_for_run, shutdown_rx).await
    });

    // Wait for either shutdown signal or arbitrager completion
    tokio::select! {
        _ = shutdown_signal() => {
            info!("Shutdown signal received, gracefully stopping...");
            // Mark as not ready so k8s stops sending traffic
            health_state.set_gas_prices_ready(false);
            health_state.set_workers_ready(false);
            // Signal all workers to stop gracefully
            let _ = shutdown_tx.send(true);
        }
        result = &mut arbitrager_handle => {
            match result {
                Ok(Ok(())) => info!("Arbitrager completed successfully"),
                Ok(Err(e)) => error!(error = %e, "Arbitrager failed after all retries"),
                Err(e) => error!(error = %e, "Arbitrager task panicked"),
            }
        }
    }

    // Wait for the arbitrager to finish cleanup after shutdown signal
    if !arbitrager_handle.is_finished() {
        match arbitrager_handle.await {
            Ok(Ok(())) => info!("Arbitrager shut down cleanly"),
            Ok(Err(e)) => error!(error = %e, "Arbitrager error during shutdown"),
            Err(e) => error!(error = %e, "Arbitrager task panicked during shutdown"),
        }
    }

    info!("ekubo-arb shutting down");
    Ok(())
}

/// Run the arbitrager with exponential backoff retry logic
async fn run_with_retries(
    config: ArbitragerConfig,
    health_state: Arc<HealthState>,
    dashboard_state: Arc<DashboardState>,
    shutdown: tokio::sync::watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut consecutive_failures = 0u32;

    loop {
        // Mark as not ready while (re)initializing
        health_state.set_gas_prices_ready(false);
        health_state.set_workers_ready(false);

        // Check if shutdown was requested before starting a new attempt
        if *shutdown.borrow() {
            info!("Shutdown requested, not retrying");
            return Ok(());
        }

        info!(attempt = consecutive_failures + 1, "Starting arbitrager");

        match run_arbitrager(
            config.clone(),
            Some(Arc::clone(&health_state)),
            Some(Arc::clone(&dashboard_state)),
            shutdown.clone(),
        )
        .await
        {
            Ok(()) => {
                // Normal exit (shouldn't happen since run() loops forever)
                info!("Arbitrager exited normally");
                return Ok(());
            }
            Err(e) => {
                consecutive_failures += 1;
                let delay = retry_delay(consecutive_failures - 1);

                error!(
                    error = %e,
                    consecutive_failures,
                    retry_delay_secs = delay.as_secs(),
                    "Arbitrager failed"
                );

                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    error!(
                        max_failures = MAX_CONSECUTIVE_FAILURES,
                        "Too many consecutive failures, giving up"
                    );
                    return Err(Box::new(e));
                }

                warn!(delay_secs = delay.as_secs(), "Retrying after delay");
                tokio::time::sleep(delay).await;
            }
        }
    }
}

/// Wait for shutdown signal (SIGTERM or SIGINT)
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
