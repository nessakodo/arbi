//! Core arbitrager implementation: state management, syncing, evaluation, and broadcasting.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use starknet::core::types::Felt;
use thiserror::Error;
use tokio::sync::watch;
use tracing::{debug, error, info, warn};

use crate::account::Account;
use crate::constants::{EKUBO_ROUTER_ADDRESS, STRK_TOKEN_ADDRESS};
use crate::ekubo::events::Transaction;
use crate::ekubo::paths::PathWithTokens;
use crate::ekubo::state::{EvaluationRouteResult, State};
use crate::ekubo::swap::{hex_to_u256, u256_to_felt, U256};
use crate::ekubo::sync::{
    fetch_events, group_events_by_tx, RpcEvent, SyncConfig, SyncError, SyncResult,
};
use crate::errors::ProviderError;
use crate::gas::{BlockHeader, GasPriceCache};
use crate::rpc::RPC;
use crate::ws::{WsEvent, WsEventSource};

use crate::opportunity::ArbitrageOpportunity;

/// Errors that can occur during arbitrage operations
#[derive(Error, Debug)]
pub enum ArbitragerError {
    #[error("State loading error: {0}")]
    StateLoad(String),

    #[error("Sync error: {0}")]
    Sync(#[from] SyncError),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Account error: {0}")]
    Account(String),
}

impl From<Box<dyn std::error::Error + Send + Sync>> for ArbitragerError {
    fn from(e: Box<dyn std::error::Error + Send + Sync>) -> Self {
        ArbitragerError::Account(e.to_string())
    }
}

impl From<reqwest::Error> for ArbitragerError {
    fn from(e: reqwest::Error) -> Self {
        ArbitragerError::Http(e.to_string())
    }
}

impl From<crate::ekubo::state::LoadError> for ArbitragerError {
    fn from(e: crate::ekubo::state::LoadError) -> Self {
        ArbitragerError::StateLoad(e.to_string())
    }
}

impl From<ProviderError> for ArbitragerError {
    fn from(e: ProviderError) -> Self {
        ArbitragerError::Http(e.to_string())
    }
}

/// A batch of transactions (grouped events) sent from an event source to the coordinator
#[derive(Debug)]
pub struct Batch {
    pub block: u64,
    pub transactions: Vec<Transaction>,
}

/// Configuration for the arbitrager
#[derive(Debug, Clone)]
pub struct ArbitragerConfig {
    /// Path to the JSON file containing initial pool state
    pub json_path: String,
    /// Starting block number for syncing
    pub from_block: u64,
    /// RPC URL for fetching events
    pub rpc_url: String,
    /// WebSocket URL for real-time event streaming
    pub rpc_ws_url: String,
    /// Account address used for transaction signing (hex)
    pub account_address: String,
    /// Private key for the account (hex)
    pub account_private_key: String,
    /// Broadcast transactions on-chain
    pub broadcast: bool,
    /// Minimum profit in hundredth basis points to trigger execution
    /// 100 = 1 BIP = 0.01%, 10000 = 1%
    pub min_profit_hbip: i128,
    /// Base amounts per token for multi-token arbitrage search
    /// Maps token address (U256) to base amount to search around
    pub tokens: HashMap<U256, U256>,
}

impl ArbitragerConfig {
    /// Create a new config with default settings
    pub fn new(
        json_path: impl Into<String>,
        rpc_url: impl Into<String>,
        rpc_ws_url: impl Into<String>,
        account_address: impl Into<String>,
        account_private_key: impl Into<String>,
    ) -> Self {
        let mut tokens = HashMap::new();

        // STRK: 10_000 tokens (18 decimals) = 10_000 * 10^18
        // This base amount may need to be adjusted based on observed profitable opportunity optima
        if let Ok(strk) = crate::ekubo::swap::hex_to_u256(STRK_TOKEN_ADDRESS) {
            tokens.insert(strk, U256::from(10_000_000_000_000_000_000_000u128));
        }

        Self {
            json_path: json_path.into(),
            from_block: 0,
            rpc_url: rpc_url.into(),
            rpc_ws_url: rpc_ws_url.into(),
            account_address: account_address.into(),
            account_private_key: account_private_key.into(),
            broadcast: false,
            min_profit_hbip: 100,
            tokens,
        }
    }

    pub fn with_from_block(mut self, from_block: u64) -> Self {
        self.from_block = from_block;
        self
    }

    pub fn with_broadcast(mut self, broadcast: bool) -> Self {
        self.broadcast = broadcast;
        self
    }

    pub fn with_min_profit_hbip(mut self, min_profit_hbip: i128) -> Self {
        self.min_profit_hbip = min_profit_hbip;
        self
    }

    pub fn with_tokens(mut self, tokens: HashMap<U256, U256>) -> Self {
        self.tokens = tokens;
        self
    }
}

/// Gas price update interval (5 minutes)
const GAS_PRICE_UPDATE_INTERVAL: Duration = Duration::from_secs(5 * 60);

// =============================================================================
// Simulator - Lightweight state evaluation without RPC/accounts
// =============================================================================

/// Lightweight simulator for evaluating arbitrage opportunities from JSON state
///
/// This struct provides a simple way to load state from JSON and evaluate
/// routes without needing RPC connections or account credentials.
///
/// # Example
/// ```ignore
/// use ekubo_arb::arbitrager::Simulator;
/// use ekubo_arb::ekubo::swap::U256;
///
/// // Load state from JSON and initialize paths for STRK token
/// let token = "0x04718f5a0fc34cc1af16a1cdee98ffb20c31f5cd61d6ab07201858f4287c938d";
/// let mut sim = Simulator::from_json_file("pools.json", token)?;
///
/// // Evaluate best route at a specific amount
/// let amount = U256::from(1_000_000_000_000_000_000_000u128); // 1000 tokens
/// if let Some(best) = sim.get_best(amount) {
///     println!("Best route: {} profit ({} hbip)", best.profit, best.profit_hbip);
/// }
///
/// // Or use the ternary search to find optimal amount
/// if let Some(opp) = sim.evaluate_optimal(amount) {
///     println!("Optimal: {} in -> {} out, profit {}", opp.amount_in, opp.amount_out, opp.profit);
/// }
/// ```
pub struct Simulator {
    state: State,
    base_amount: U256,
    token: U256,
}

impl Simulator {
    /// Create a simulator from a JSON file
    ///
    /// Loads pools from the JSON file and initializes paths for the given token
    /// (arbitrage paths from token back to itself).
    pub fn from_json_file<P: AsRef<Path>>(path: P, token: &str) -> Result<Self, ArbitragerError> {
        let mut state = State::from_json_file_no_paths(path)?;
        state
            .init(token, token)
            .map_err(|e| ArbitragerError::StateLoad(e.to_string()))?;

        let token_u256 = hex_to_u256(token)
            .map_err(|e| ArbitragerError::StateLoad(format!("Invalid token address: {}", e)))?;

        Ok(Self {
            state,
            base_amount: U256::from(1_000_000_000_000_000_000_000u128), // Default 1000 tokens
            token: token_u256,
        })
    }

    /// Set the base amount for evaluation
    pub fn with_amount(mut self, amount: U256) -> Self {
        self.base_amount = amount;
        self
    }

    /// Get a reference to the state
    pub fn state(&self) -> &State {
        &self.state
    }

    /// Get a mutable reference to the state
    pub fn state_mut(&mut self) -> &mut State {
        &mut self.state
    }

    /// Get the best route at a specific amount (single evaluation)
    pub fn get_best(&self, amount: U256) -> Option<EvaluationRouteResult> {
        self.state.get_best(amount)
    }

    /// Get all routes sorted by output (best first)
    pub fn get_all_routes(&self, amount: U256) -> Vec<EvaluationRouteResult> {
        self.state.get_all_routes(amount)
    }

    /// Evaluate a specific path at a given amount
    pub fn evaluate_path(
        &self,
        path: &PathWithTokens,
        amount: U256,
    ) -> Option<EvaluationRouteResult> {
        self.state.evaluate_path_at_amount(path, amount)
    }

    /// Find the optimal amount using ternary search (0.5X to 1.5X of base_amount)
    ///
    /// First finds the best path at base_amount by profit_hbip, then optimizes
    /// the amount for that path over 5 iterations.
    pub fn evaluate_optimal(&self, base_amount: U256) -> Option<ArbitrageOpportunity> {
        // Find best path at base amount (by profit_hbip, matching State logic)
        let best_at_base = self.state.get_best(base_amount)?;
        let best_path = best_at_base.path.clone();

        const SEARCH_ITERATIONS: usize = 5;

        let mut low = base_amount / U256::from(2u64); // 50%
        let mut high = base_amount + base_amount / U256::from(2u64); // 150%

        let mut best_result = best_at_base;

        for _ in 0..SEARCH_ITERATIONS {
            let third = (high - low) / U256::from(3u64);
            let mid1 = low + third;
            let mid2 = high - third;

            let r1 = self.state.evaluate_path_at_amount(&best_path, mid1);
            let r2 = self.state.evaluate_path_at_amount(&best_path, mid2);

            match (r1, r2) {
                (Some(res1), Some(res2)) => {
                    if res1.profit > res2.profit {
                        high = mid2;
                        if res1.profit > best_result.profit {
                            best_result = res1;
                        }
                    } else {
                        low = mid1;
                        if res2.profit > best_result.profit {
                            best_result = res2;
                        }
                    }
                }
                (Some(res1), None) => {
                    high = mid2;
                    if res1.profit > best_result.profit {
                        best_result = res1;
                    }
                }
                (None, Some(res2)) => {
                    low = mid1;
                    if res2.profit > best_result.profit {
                        best_result = res2;
                    }
                }
                (None, None) => break,
            }
        }

        // Also evaluate at boundaries and base amount
        for amt in [base_amount, low, high] {
            if let Some(r) = self.state.evaluate_path_at_amount(&best_path, amt) {
                if r.profit > best_result.profit {
                    best_result = r;
                }
            }
        }

        Some(ArbitrageOpportunity {
            token: self.token,
            amount_in: best_result.amount_in,
            amount_out: best_result.amount_out,
            profit: best_result.profit,
            profit_hbip: best_result.profit_hbip,
            hop_count: best_result.hop_count,
            path: best_result.path,
            result: best_result.result,
        })
    }

    /// Evaluate using the configured base_amount
    pub fn evaluate(&self) -> Option<ArbitrageOpportunity> {
        self.evaluate_optimal(self.base_amount)
    }
}

// =============================================================================
// Arbitrager - Full arbitrager with RPC sync and transaction broadcasting
// =============================================================================

/// The main arbitrager that manages state and monitors for opportunities
pub struct Arbitrager {
    /// Configuration
    config: ArbitragerConfig,
    /// Pool state
    state: State,
    /// RPC client for JSON-RPC calls (sync, nonce, block headers)
    rpc: Arc<RPC>,
    /// WebSocket event source for real-time event streaming
    event_source: WsEventSource,
    /// Gas price cache (Arc for sharing with background tasks)
    gas_price_cache: Arc<GasPriceCache>,
    /// Account used for transaction signing
    account: Account,
    /// Parsed Ekubo router contract address (for swap calls)
    ekubo_router_address: Felt,
    /// Shutdown signal receiver
    shutdown: watch::Receiver<bool>,
    /// Health state for updating counters (None in tests / headless mode)
    health_state: Option<Arc<crate::HealthState>>,
}

impl Arbitrager {
    /// Create a new arbitrager and initialize state from JSON
    pub async fn new(
        config: ArbitragerConfig,
        shutdown: watch::Receiver<bool>,
        health_state: Option<Arc<crate::HealthState>>,
    ) -> Result<Self, ArbitragerError> {
        info!(
            json_path = %config.json_path,
            from_block = config.from_block,
            "Initializing arbitrager"
        );

        // Load initial state from JSON
        let state = Self::init_from_json(&config.json_path).await?;

        // RPC client — used for nonce, sync, and block headers
        let rpc = Arc::new(RPC::new(config.rpc_url.clone()));

        // WebSocket event source for real-time event streaming
        let ws = WsEventSource::new(config.rpc_ws_url.clone(), shutdown.clone());

        let gas_price_cache = Arc::new(GasPriceCache::default_mainnet());

        let account_address = Felt::from_hex(&config.account_address)
            .map_err(|e| ArbitragerError::Config(format!("Invalid account_address: {}", e)))?;

        let account_private_key = Felt::from_hex(&config.account_private_key)
            .map_err(|e| ArbitragerError::Config(format!("Invalid account_private_key: {}", e)))?;

        let ekubo_router_address = Felt::from_hex(EKUBO_ROUTER_ADDRESS)
            .map_err(|e| ArbitragerError::Config(format!("Invalid ekubo router address: {}", e)))?;

        // Fetch the current nonce from RPC
        let nonce = rpc
            .get_nonce(account_address)
            .await
            .map_err(|e| ArbitragerError::Account(format!("Failed to get nonce: {e}")))?;

        let account = Account::new(account_private_key, account_address, nonce);

        info!(nonce = %nonce, account_address = %account_address, "Account nonce set");

        Ok(Self {
            config,
            state,
            rpc,
            event_source: ws,
            gas_price_cache,
            account,
            ekubo_router_address,
            shutdown,
            health_state,
        })
    }

    /// Initialize state from a JSON file
    pub async fn init_from_json<P: AsRef<Path>>(path: P) -> Result<State, ArbitragerError> {
        let state = State::from_json_file_no_paths(path)?;
        info!(pool_count = state.pool_count(), "Loaded pools from JSON");
        Ok(state)
    }

    /// Sync state with on-chain events from `from_block` to `to_block`
    ///
    /// Fetches events first (async), then applies them to state.
    pub async fn sync(
        &mut self,
        from_block: u64,
        to_block: u64,
    ) -> Result<SyncResult, ArbitragerError> {
        info!(from_block, to_block, "Syncing blocks");

        let sync_config = SyncConfig::new()?;
        let ekubo_address = format!("{:#x}", sync_config.ekubo_address);

        // Phase 1: Fetch all events (async)
        let transactions = self
            .fetch_all_events(&ekubo_address, from_block, to_block, &sync_config)
            .await?;

        info!(
            transaction_count = transactions.len(),
            "Fetched transactions"
        );

        // Phase 2: Apply events to state
        let result = self.apply_transactions(transactions, from_block, to_block);

        info!(
            events_processed = result.events_processed,
            transactions_applied = result.transactions_applied,
            "Sync complete"
        );

        Ok(result)
    }

    /// Fetch all events from RPC (async, does not hold any locks)
    ///
    /// Collects all raw events first, then groups by transaction once at the end.
    /// This avoids splitting transactions that span page boundaries.
    async fn fetch_all_events(
        &self,
        ekubo_address: &str,
        from_block: u64,
        to_block: u64,
        sync_config: &SyncConfig,
    ) -> Result<Vec<Transaction>, ArbitragerError> {
        let mut all_events: Vec<RpcEvent> = Vec::new();
        let mut continuation_token: Option<String> = None;

        loop {
            let events_page = fetch_events(
                &self.rpc,
                ekubo_address,
                from_block,
                to_block,
                sync_config.chunk_size,
                continuation_token.clone(),
            )
            .await?;

            info!(
                event_count = events_page.events.len(),
                continuation_token = ?events_page.continuation_token,
                "Fetched events page"
            );

            all_events.extend(events_page.events);

            if events_page.continuation_token.is_none() {
                break;
            }

            continuation_token = events_page.continuation_token;
        }

        Ok(group_events_by_tx(&all_events))
    }

    /// Apply transactions to state
    fn apply_transactions(
        &mut self,
        transactions: Vec<Transaction>,
        from_block: u64,
        to_block: u64,
    ) -> SyncResult {
        let mut events_processed = 0;
        let mut transactions_applied = 0;

        for tx in transactions {
            let (success_count, _affected_pools) = self.state.apply_tx(tx);
            events_processed += success_count;
            transactions_applied += 1;
        }

        SyncResult {
            events_processed,
            transactions_applied,
            from_block,
            to_block,
        }
    }

    /// Initialize paths for arbitrage evaluation (all token cycles)
    ///
    /// Computes cycle paths (token -> ... -> same token) for ALL tokens in the pool graph.
    /// This enables multi-token arbitrage search.
    pub fn init_paths(&mut self) -> Result<usize, ArbitragerError> {
        self.state.init_all_cycles();

        // Prebuild paths for the tokens we're configured to trade
        let tokens: Vec<U256> = self.config.tokens.keys().copied().collect();
        self.state.init_paths_for_tokens(&tokens);

        let path_count = self.state.path_count();
        let token_count = self.state.cycle_token_count();
        info!(
            path_count,
            token_count,
            configured_tokens = tokens.len(),
            "Computed arbitrage cycles for all tokens"
        );
        Ok(path_count)
    }

    // =========================================================================
    // Simulation / Testing API
    // =========================================================================

    /// Get a reference to the current state
    ///
    /// Useful for simulation and testing to inspect pool states, paths, etc.
    pub fn state(&self) -> &State {
        &self.state
    }

    /// Get a mutable reference to the current state
    ///
    /// Useful for simulation and testing to manually apply events or modify pools.
    pub fn state_mut(&mut self) -> &mut State {
        &mut self.state
    }

    /// Get a reference to the configuration
    pub fn config(&self) -> &ArbitragerConfig {
        &self.config
    }

    /// Evaluate and return the best arbitrage opportunity across all paths
    ///
    /// This uses the ternary search optimization to find the optimal trade amount
    /// around the configured amount (0.5X to 1.5X range, 3 iterations).
    ///
    /// # Example
    /// ```ignore
    /// let arbitrager = Arbitrager::new(config).await?;
    /// arbitrager.sync(from_block, to_block).await?;
    /// arbitrager.init_paths()?;
    ///
    /// if let Some(opportunity) = arbitrager.evaluate_all() {
    ///     println!("Found opportunity: {} profit", opportunity.profit);
    /// }
    /// ```
    pub fn evaluate_all(&self) -> Option<ArbitrageOpportunity> {
        // Get all pool key hashes from the state
        let all_pools: Vec<u64> = self.state.pools.iter().map(|p| p.key_hash()).collect();
        self.evaluate_best_opportunity(&all_pools)
    }

    /// Evaluate a specific set of pools by their key hashes
    ///
    /// This is useful when you know which pools were affected by a transaction
    /// and want to evaluate only paths that use those pools.
    pub fn evaluate_pools(&self, pool_key_hashes: &[u64]) -> Option<ArbitrageOpportunity> {
        self.evaluate_best_opportunity(pool_key_hashes)
    }

    /// Fetch block header with gas prices from RPC
    pub async fn fetch_block_header(&self, block_id: &str) -> Result<BlockHeader, ArbitragerError> {
        Ok(self.rpc.get_block_header(block_id).await?)
    }

    /// Update gas prices from the provider
    pub async fn update_gas_prices(&self, block_number: u64) -> Result<(), ArbitragerError> {
        let header = self.fetch_block_header(&block_number.to_string()).await?;
        self.gas_price_cache
            .update_from_header(&header, block_number);
        debug!(
            block_number,
            l1_gas_price = self.gas_price_cache.l1_gas_price(),
            l2_gas_price = self.gas_price_cache.l2_gas_price(),
            l1_data_gas_price = self.gas_price_cache.l1_data_gas_price(),
            "Updated gas prices"
        );
        Ok(())
    }

    /// Run the main arbitrage loop with multi-worker architecture
    ///
    /// This function:
    /// 1. Loads state from JSON (done in `new`)
    /// 2. Syncs state from `from_block` using on-chain events
    /// 3. Initializes arbitrage paths
    /// 4. Spawns block pollers (current + next) to monitor the configured event source
    /// 5. Spawns 1 gas price updater (runs every 5 minutes)
    /// 6. Runs the coordinator loop inline to process transactions
    ///
    /// If `health_state` is provided, it will be updated when the arbitrager is ready.
    pub async fn run(&mut self) -> Result<(), ArbitragerError> {
        info!(
            from_block = self.config.from_block,
            tokens_count = self.config.tokens.len(),
            "Starting arbitrager"
        );

        // Initial sync from from_block (sync will fetch events up to from_block)
        let from_block = self.config.from_block;
        let to_block = self.rpc.get_latest_block_number().await?;

        self.sync(from_block, to_block).await?;

        // Update gas prices from the latest block
        self.update_gas_prices(to_block).await?;

        // Signal gas prices are ready
        if let Some(ref hs) = self.health_state {
            hs.set_gas_prices_ready(true);
        }

        // Initialize arbitrage paths
        self.init_paths()?;

        // Catch-up: a second RPC sync closes the gap that opened during the
        // first (possibly long) sync + path init.
        let latest_block = self.rpc.get_latest_block_number().await?;
        let mut block = to_block + 1;
        if block <= latest_block {
            info!(from = block, to = latest_block, "Catch-up sync (RPC)");
            self.sync(block, latest_block).await?;
            block = latest_block + 1;
        }

        // Signal workers are ready
        if let Some(ref hs) = self.health_state {
            hs.set_workers_ready(true);
            info!("Health state: ready");
        }

        // Spawn event source workers
        let mut fetch_rx = self.event_source.start(block);

        info!(base_block = block, "Event source workers spawned");

        let mut coordinator_shutdown = self.shutdown.clone();
        let mut gas_price_interval = tokio::time::interval(GAS_PRICE_UPDATE_INTERVAL);
        gas_price_interval.tick().await; // consume the immediate first tick

        info!("Starting coordinator loop");

        // Main coordinator loop - process each Batch as a single unit
        loop {
            tokio::select! {
                ws_event = fetch_rx.recv() => {
                    let Some(ws_event) = ws_event else { break };
                    match ws_event {
                        WsEvent::Batch(fetched) => {
                            if let Some(ref hs) = self.health_state {
                                hs.update_last_block_fetch();
                            }
                            debug!(
                                block = fetched.block,
                                tx_count = fetched.transactions.len(),
                                "Received Batch"
                            );
                            self.process_batch(fetched.transactions, fetched.block)
                                .await;
                        }
                        WsEvent::Reconnected { last_block } => {
                            self.backfill_on_reconnect(last_block).await;
                        }
                    }
                }
                _ = gas_price_interval.tick() => {
                    match self.rpc.get_latest_block_number().await {
                        Ok(block_number) => {
                            match self.rpc.get_block_header(&block_number.to_string()).await {
                                Ok(header) => {
                                    self.gas_price_cache.update_from_header(&header, block_number);
                                    debug!(block = block_number, "Updated gas prices");
                                }
                                Err(e) => {
                                    debug!(error = %e, "Failed to fetch block header for gas prices");
                                }
                            }
                        }
                        Err(e) => {
                            debug!(error = %e, "Failed to get latest block number for gas prices");
                        }
                    }
                }
                _ = coordinator_shutdown.changed() => {
                    info!("Shutdown signal received, coordinator stopping");
                    break;
                }
            }
        }

        self.event_source.stop().await;

        Ok(())
    }

    /// Apply transactions from a fetched block
    fn apply_fetched_transactions(
        &mut self,
        transactions: Vec<Transaction>,
    ) -> (usize, Vec<u64>, Vec<String>) {
        let mut events_applied = 0usize;
        let mut affected_pools: Vec<u64> = Vec::new();
        let mut tx_hashes: Vec<String> = Vec::new();

        for tx in transactions {
            let tx_hash = tx.tx_hash.clone();
            let (count, affected) = self.state.apply_tx(tx);

            tx_hashes.push(tx_hash);
            events_applied += count;
            if count > 0 {
                affected_pools.extend(affected);
            }
        }

        (events_applied, affected_pools, tx_hashes)
    }

    /// Process a batch of transactions as a single unit, evaluate once, and broadcast if profitable
    async fn process_batch(&mut self, transactions: Vec<Transaction>, block_number: u64) {
        debug!(
            block = block_number,
            tx_count = transactions.len(),
            "Processing batch"
        );

        let start = std::time::Instant::now();

        let (events_applied, affected_pools, tx_hashes) =
            self.apply_fetched_transactions(transactions);

        if events_applied == 0 {
            info!(
                block = block_number,
                tx_hashes = ?tx_hashes.join(", "),
                "Processed batch, no events applied"
            );
            return;
        }

        let apply_time = start.elapsed();
        debug!(
            block = block_number,
            events_applied,
            affected_pools = affected_pools.len(),
            apply_time_us = apply_time.as_micros(),
            "Applied transactions, starting evaluation"
        );

        // Evaluate opportunities ONCE for the entire batch
        let best = self.evaluate_best_opportunity(&affected_pools);

        let eval_time = start.elapsed();
        debug!(
            block = block_number,
            eval_time_us = eval_time.as_micros(),
            found_opportunity = best.is_some(),
            "Evaluation complete"
        );

        if let Some(ref hs) = self.health_state {
            hs.inc_transactions();
        }

        let Some(best) = best else {
            info!(
                block = block_number,
                batch_size = tx_hashes.len(),
                tx_hashes = ?tx_hashes.join(", "),
                events_applied,
                "Processed batch, no opportunity"
            );
            return;
        };

        // Check minimum profit threshold
        if best.profit_hbip < self.config.min_profit_hbip {
            info!(
                block = block_number,
                batch_size = tx_hashes.len(),
                tx_hashes = ?tx_hashes.join(", "),
                events_applied,
                apply_time = apply_time.as_millis(),
                eval_time = eval_time.as_millis(),
                profit_hbip = best.profit_hbip,
                "Processed batch, opportunity below minimum profit threshold"
            );
            return;
        }

        info!(
            block = block_number,
            batch_size = tx_hashes.len(),
            tx_hashes = ?tx_hashes.join(", "),
            events_applied,
            apply_time = apply_time.as_millis(),
            eval_time = eval_time.as_millis(),
            token = %format!("{:x}", best.token),
            profit_hbip = best.profit_hbip,
            amount_in = %best.amount_in,
            amount_out = %best.amount_out,
            "Processed batch, found best opportunity"
        );

        if !self.config.broadcast {
            info!("Broadcast disabled - skipping transaction broadcast");
            return;
        }

        debug!(
            block = block_number,
            token = %format!("{:#x}", best.token),
            profit_hbip = best.profit_hbip,
            "Building swap transaction"
        );

        // Build and broadcast
        let token_address = u256_to_felt(&best.token);
        let calls = best.build_swap_calls(token_address, self.ekubo_router_address);

        let payload =
            match self
                .account
                .build_payload(&self.gas_price_cache, calls, best.profit as u64)
            {
                Ok(p) => p,
                Err(e) => {
                    error!(error = %e, "Failed to build transaction payload");
                    return;
                }
            };

        let build_time = start.elapsed();
        debug!(
            block = block_number,
            build_time_us = build_time.as_micros(),
            "Broadcasting transaction"
        );

        let broadcast_result = self.rpc.broadcast(&payload).await;

        match broadcast_result {
            Ok(tx_hash) => {
                self.account.increase_nonce();
                if let Some(ref hs) = self.health_state {
                    hs.inc_reactions();
                }

                let broadcast_time = start.elapsed();

                info!(
                    tx_hash = %format!("{:#x}", tx_hash),
                    build_time = build_time.as_millis(),
                    broadcast_time = broadcast_time.as_millis(),
                    "Transaction broadcast successfully"
                );
            }
            Err(e) => {
                error!(error = %e, "Failed to broadcast transaction");
            }
        }
    }

    /// Backfill missed events via RPC after a WebSocket reconnection.
    ///
    /// Fetches events from `last_ws_block + 1` to `latest - 1` and applies them.
    async fn backfill_on_reconnect(&mut self, last_ws_block: u64) {
        let latest = match self.rpc.get_latest_block_number().await {
            Ok(n) => n,
            Err(e) => {
                warn!(error = %e, "Backfill: failed to get latest block number");
                return;
            }
        };

        let from = last_ws_block + 1;
        // Leave a 1-block buffer so WS can still deliver the very latest
        let to = latest.saturating_sub(1);

        if from > to {
            debug!(from, to, "Backfill: no gap to fill");
            return;
        }

        match self.sync(from, to).await {
            Ok(result) => {
                info!(
                    from,
                    to,
                    events = result.events_processed,
                    txs = result.transactions_applied,
                    "Backfill: complete"
                );
            }
            Err(e) => {
                warn!(error = %e, from, to, "Backfill: sync failed");
            }
        }
    }

    /// Evaluate and return the best arbitrage opportunity for affected pools
    ///
    /// Searches ALL tokens that have cycles through the affected pools.
    /// Finds the globally optimal token + amount combination.
    /// Returns the best opportunity regardless of profit threshold.
    fn evaluate_best_opportunity(&self, affected_pools: &[u64]) -> Option<ArbitrageOpportunity> {
        if affected_pools.is_empty() {
            return None;
        }

        // Use the multi-token search that:
        // 1. Finds all tokens with cycles through affected pools
        // 2. For each token, finds the best cycle (by profit_hbip)
        // 3. Returns the globally best opportunity
        let best = self
            .state
            .find_optimal_for_changed_pools_quoted(affected_pools, &self.config.tokens)?;

        Some(ArbitrageOpportunity {
            token: best.token,
            amount_in: best.amount_in,
            amount_out: best.amount_out,
            profit: best.profit,
            profit_hbip: best.profit_hbip,
            hop_count: best.path.len(),
            path: best.path,
            result: best.result,
        })
    }

    /// Export the current state to a JSON file
    pub fn export_state<P: AsRef<Path>>(&self, path: P) -> Result<(), ArbitragerError> {
        self.state
            .export_to_json_file(path)
            .map_err(|e| ArbitragerError::StateLoad(e.to_string()))
    }
}

/// Initialize state from JSON, sync to latest block, and monitor for new events
///
/// This is a convenience function that combines all steps:
/// 1. Load state from JSON
/// 2. Sync from `from_block` to latest block
/// 3. Start monitoring for new events via WebSocket
pub async fn run_arbitrager(
    config: ArbitragerConfig,
    health_state: Option<Arc<crate::HealthState>>,
    shutdown: watch::Receiver<bool>,
) -> Result<(), ArbitragerError> {
    let mut arbitrager = Arbitrager::new(config, shutdown, health_state).await?;
    arbitrager.run().await
}
