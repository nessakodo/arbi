use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use super::evaluation::{
    evaluate_path, Direction, EvaluatePathResult, Hop, Path as EvalPath, PoolWithTokens,
};
use super::events::{PoolEvent, PoolId, Transaction, UpdateEvent, UpdateTickEvent};
use super::paths::{get_paths, get_paths_with_max_hops, PathWithTokens};
use super::pool::PoolExt;
use super::swap::{hex_to_u256, u256_to_hex, Pool, SwapInfo, Tick, U256};

/// JSON representation of a tick
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonTick {
    pub tick: i64,
    pub net_liquidity_delta_diff: String,
}

/// JSON representation of a pool
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonPool {
    pub token0: String,
    pub token1: String,
    pub fee: String,
    pub tick_spacing: String,
    pub extension: String,
    pub sqrt_ratio: String,
    pub liquidity: String,
    pub tick: i64,
    pub ticks: Vec<JsonTick>,
}

/// Error type for JSON loading
#[derive(Debug)]
pub enum LoadError {
    Io(io::Error),
    Json(serde_json::Error),
    Parse(String),
}

impl From<io::Error> for LoadError {
    fn from(err: io::Error) -> Self {
        LoadError::Io(err)
    }
}

impl From<serde_json::Error> for LoadError {
    fn from(err: serde_json::Error) -> Self {
        LoadError::Json(err)
    }
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "IO error: {}", e),
            LoadError::Json(e) => write!(f, "JSON error: {}", e),
            LoadError::Parse(s) => write!(f, "Parse error: {}", s),
        }
    }
}

impl std::error::Error for LoadError {}

/// Parse a hex string (with or without 0x prefix) to U256
fn parse_hex_to_u256(s: &str) -> Result<U256, LoadError> {
    hex_to_u256(s).map_err(|e| LoadError::Parse(format!("Failed to parse hex '{}': {}", s, e)))
}

/// Parse hex to u128 for liquidity
fn parse_hex_to_u128(s: &str) -> Result<u128, LoadError> {
    let s = s.trim().strip_prefix("0x").unwrap_or(s);
    u128::from_str_radix(s, 16)
        .map_err(|e| LoadError::Parse(format!("Failed to parse hex '{}' as u128: {}", s, e)))
}

/// Parse hex to i64 for tick_spacing
fn parse_hex_to_i64(s: &str) -> Result<i64, LoadError> {
    let s = s.trim().strip_prefix("0x").unwrap_or(s);
    i64::from_str_radix(s, 16)
        .map_err(|e| LoadError::Parse(format!("Failed to parse hex '{}' as i64: {}", s, e)))
}

/// Convert a JsonPool to a PoolWithTokens
fn json_pool_to_pool_with_tokens(json: &JsonPool) -> Result<PoolWithTokens, LoadError> {
    let token0 = parse_hex_to_u256(&json.token0)?;
    let token1 = parse_hex_to_u256(&json.token1)?;
    let tick_spacing = parse_hex_to_i64(&json.tick_spacing)?;
    let extension = parse_hex_to_u256(&json.extension)?;

    // Parse sqrt_ratio as U256 for precision
    let sqrt_ratio = hex_to_u256(&json.sqrt_ratio)
        .map_err(|e| LoadError::Parse(format!("sqrt_ratio: {}", e)))?;
    // Parse fee as U256
    let fee = hex_to_u256(&json.fee).map_err(|e| LoadError::Parse(format!("fee: {}", e)))?;
    // Parse liquidity as u128
    let liquidity = parse_hex_to_u128(&json.liquidity)?;

    let ticks: Result<Vec<Tick>, LoadError> = json
        .ticks
        .iter()
        .map(|t| {
            let delta = t.net_liquidity_delta_diff.parse::<i128>().map_err(|e| {
                LoadError::Parse(format!(
                    "Failed to parse delta '{}': {}",
                    t.net_liquidity_delta_diff, e
                ))
            })?;
            Ok(Tick {
                tick: t.tick,
                delta,
            })
        })
        .collect();

    let pool = Pool::new(ticks?, json.tick, sqrt_ratio, liquidity, fee);

    let pool_id = PoolId::from_values(token0, token1, fee, tick_spacing, extension);
    Ok(PoolWithTokens::from_pool_id(pool, &pool_id))
}

/// Ensure hex string has 0x prefix
fn ensure_hex_prefix(s: &str) -> String {
    if s.starts_with("0x") || s.starts_with("0X") {
        s.to_string()
    } else {
        format!("0x{}", s)
    }
}

/// Format a hex string as 0x + 64 zero-padded lowercase hex chars (for Starknet felts/addresses)
fn format_hex_64(s: &str) -> String {
    let s = s.trim().strip_prefix("0x").unwrap_or(s);
    let s = s.strip_prefix("0X").unwrap_or(s);
    format!("0x{:0>64}", s.to_lowercase())
}

/// Convert a PoolWithTokens to JsonPool for serialization
fn pool_with_tokens_to_json(pool: &PoolWithTokens) -> JsonPool {
    let ticks: Vec<JsonTick> = pool
        .pool
        .ticks
        .iter()
        .map(|t| JsonTick {
            tick: t.tick,
            net_liquidity_delta_diff: format!("{}", t.delta),
        })
        .collect();

    JsonPool {
        token0: format_hex_64(&pool.token0_hex),
        token1: format_hex_64(&pool.token1_hex),
        fee: ensure_hex_prefix(&pool.fee_hex),
        tick_spacing: ensure_hex_prefix(&pool.tick_spacing_hex),
        extension: ensure_hex_prefix(&pool.extension_hex),
        // Use U256 hex for lossless serialization
        sqrt_ratio: u256_to_hex(&pool.pool.sqrt_ratio),
        liquidity: format!("0x{:x}", pool.pool.liquidity),
        tick: pool.pool.tick,
        ticks,
    }
}

/// Calculate profit from amount_out and amount_in (both U256)
/// Returns (profit as i128, profit_hbip as i128)
/// profit_hbip is in hundredth basis points: 100 = 1 BIP = 0.01%, 10000 = 1%
pub fn calculate_profit(amount_out: U256, amount_in: U256) -> (i128, i128) {
    // Calculate profit difference
    let (profit_u256, is_negative) = if amount_out >= amount_in {
        (amount_out - amount_in, false)
    } else {
        (amount_in - amount_out, true)
    };

    // Convert profit to i128 (clamp if too large)
    let max_i128 = U256::from(i128::MAX as u128);
    let profit_abs = if profit_u256 > max_i128 {
        i128::MAX
    } else {
        // Safe conversion: use try_into for proper U256 to u128 conversion
        let low_128: u128 = (profit_u256 & U256::from(u128::MAX))
            .try_into()
            .unwrap_or(0);
        low_128 as i128
    };
    let profit = if is_negative { -profit_abs } else { profit_abs };

    // Calculate profit in hundredth basis points: (profit_u256 * 1_000_000) / amount_in
    // 100 = 1 BIP = 0.01%, 10000 = 1%
    let profit_hbip = if amount_in.is_zero() {
        0i128
    } else {
        // Use U256 arithmetic to avoid overflow
        let scaled = profit_u256 * U256::from(1_000_000u128);
        let pct_u256 = scaled / amount_in;

        // Convert to i128
        let pct_abs = if pct_u256 > max_i128 {
            i128::MAX
        } else {
            let low_128: u128 = (pct_u256 & U256::from(u128::MAX)).try_into().unwrap_or(0);
            low_128 as i128
        };

        if is_negative {
            -pct_abs
        } else {
            pct_abs
        }
    };

    (profit, profit_hbip)
}

/// Result of get_best operation
#[derive(Debug, Clone)]
pub struct EvaluationRouteResult {
    /// The path taken
    pub path: PathWithTokens,
    /// Full evaluation result with swap details
    pub result: EvaluatePathResult,
    /// Number of hops in the route
    pub hop_count: usize,
    /// Input amount
    pub amount_in: U256,
    /// Output amount
    pub amount_out: U256,
    /// Profit (as i128, positive or negative)
    pub profit: i128,
    /// Profit in hundredth basis points
    pub profit_hbip: i128,
}

/// Global optimal result with profit normalized to a quote token
#[derive(Debug, Clone)]
pub struct GlobalOptimalResult {
    /// The token being arbitraged (token_in == token_out)
    pub token: U256,
    /// The optimal input amount
    pub amount_in: U256,
    /// Output amount after the cycle
    pub amount_out: U256,
    /// Profit in native token units
    pub profit: i128,
    /// Profit in hundredth basis points
    pub profit_hbip: i128,
    /// The optimal path
    pub path: PathWithTokens,
    /// Full evaluation result
    pub result: EvaluatePathResult,
}

/// State holding pools and paths with efficient lookup indices.
/// Uses u64 hash keys for O(1) lookups without string allocation.
#[derive(Debug, Clone)]
pub struct State {
    /// All pools in the state
    pub pools: Vec<PoolWithTokens>,
    /// All paths in the state
    pub paths: Vec<PathWithTokens>,
    /// Index: pool_key_hash -> pool index in pools vec (O(1) lookup)
    pool_index: HashMap<u64, usize>,
    /// Index: pool_key_hash -> list of path indices that contain this pool
    paths_by_pool: HashMap<u64, Vec<usize>>,
    /// Index: pool_key_hash -> direction -> list of path indices
    paths_by_pool_directed: HashMap<u64, HashMap<Direction, Vec<usize>>>,
    /// Token in address
    token_in: Option<U256>,
    /// Token out address
    token_out: Option<U256>,
    /// All cycle paths grouped by source token: token -> Vec<path_index>
    all_cycles: HashMap<U256, Vec<usize>>,
    /// Prebuilt paths for each token (references stored as clones for efficient lookup)
    paths_by_token: HashMap<U256, Vec<PathWithTokens>>,
    /// Index: pool_key_hash -> set of tokens that have cycles through this pool
    tokens_by_pool: HashMap<u64, HashSet<U256>>,
    /// Maximum hops for path finding
    max_hops: usize,
}

impl State {
    /// Create a new empty state
    pub fn new() -> Self {
        Self {
            pools: Vec::new(),
            paths: Vec::new(),
            pool_index: HashMap::new(),
            paths_by_pool: HashMap::new(),
            paths_by_pool_directed: HashMap::new(),
            token_in: None,
            token_out: None,
            all_cycles: HashMap::new(),
            paths_by_token: HashMap::new(),
            tokens_by_pool: HashMap::new(),
            max_hops: super::paths::DEFAULT_MAX_HOPS,
        }
    }

    /// Set the maximum number of hops for path finding
    pub fn set_max_hops(&mut self, max_hops: usize) {
        self.max_hops = max_hops;
    }

    /// Create a state from pools, computing paths between source and destination
    pub fn from_pools(pools: Vec<PoolWithTokens>, source: U256, destination: U256) -> Self {
        let paths = get_paths(&pools, source, destination);
        let mut state = Self {
            pools,
            paths,
            pool_index: HashMap::new(),
            paths_by_pool: HashMap::new(),
            paths_by_pool_directed: HashMap::new(),
            token_in: Some(source),
            token_out: Some(destination),
            all_cycles: HashMap::new(),
            paths_by_token: HashMap::new(),
            tokens_by_pool: HashMap::new(),
            max_hops: super::paths::DEFAULT_MAX_HOPS,
        };
        state.rebuild_pool_index();
        state.rebuild_indices();
        state
    }

    /// Create a state from pre-computed pools and paths
    pub fn from_pools_and_paths(pools: Vec<PoolWithTokens>, paths: Vec<PathWithTokens>) -> Self {
        let mut state = Self {
            pools,
            paths,
            pool_index: HashMap::new(),
            paths_by_pool: HashMap::new(),
            paths_by_pool_directed: HashMap::new(),
            token_in: None,
            token_out: None,
            all_cycles: HashMap::new(),
            paths_by_token: HashMap::new(),
            tokens_by_pool: HashMap::new(),
            max_hops: super::paths::DEFAULT_MAX_HOPS,
        };
        state.rebuild_pool_index();
        state.rebuild_indices();
        state
    }

    /// Rebuild the pool_index HashMap for O(1) lookups
    fn rebuild_pool_index(&mut self) {
        self.pool_index.clear();
        for (idx, pool) in self.pools.iter().enumerate() {
            self.pool_index.insert(pool.key_hash(), idx);
        }
    }

    /// Get a mutable reference to a pool by its key hash (O(1) lookup)
    pub fn get_pool_mut(&mut self, key_hash: u64) -> Option<&mut PoolWithTokens> {
        let idx = *self.pool_index.get(&key_hash)?;
        self.pools.get_mut(idx)
    }

    /// Get a mutable reference to a pool by PoolId (O(1) lookup)
    pub fn get_pool_mut_by_id(&mut self, pool_id: &PoolId) -> Option<&mut PoolWithTokens> {
        self.get_pool_mut(pool_id.key_hash())
    }

    /// Build a Path using CURRENT pool states from self.pools
    /// This ensures evaluation uses up-to-date pool data instead of stale cached copies
    /// Returns references to pools in self.pools (no allocation)
    fn build_path_with_current_pools<'a>(
        &'a self,
        path_with_tokens: &'a PathWithTokens,
    ) -> EvalPath<'a> {
        path_with_tokens
            .iter()
            .map(|hop| {
                // Look up current pool state from self.pools
                let current_pool = self.get_pool(hop.pool.key_hash()).unwrap_or(&hop.pool); // Fallback to cached if not found

                Hop {
                    direction: hop.direction,
                    pool: current_pool,
                }
            })
            .collect()
    }

    /// Evaluate a specific path at a given amount
    ///
    /// Returns the result if the swap is successful, None otherwise.
    /// This is more efficient than `get_best_for_pools` when you already know
    /// which path you want to evaluate.
    pub fn evaluate_path_at_amount(
        &self,
        path: &PathWithTokens,
        amount: U256,
    ) -> Option<EvaluationRouteResult> {
        let eval_path = self.build_path_with_current_pools(path);
        let result = evaluate_path(&eval_path, amount);

        if result.info != SwapInfo::Ok {
            return None;
        }

        let amount_out = result.amount_out;
        let (profit, profit_hbip) = calculate_profit(amount_out, amount);

        Some(EvaluationRouteResult {
            path: path.clone(),
            result,
            hop_count: path.len(),
            amount_in: amount,
            amount_out,
            profit,
            profit_hbip,
        })
    }

    /// Initialize the state with token_in and token_out, computing all paths
    ///
    /// # Arguments
    /// * `token_in` - Token address as hex string (e.g., "0x04718...")
    /// * `token_out` - Token address as hex string
    pub fn init(&mut self, token_in: &str, token_out: &str) -> Result<(), LoadError> {
        let source = parse_hex_to_u256(token_in)?;
        let destination = parse_hex_to_u256(token_out)?;

        self.token_in = Some(source);
        self.token_out = Some(destination);

        // Compute paths
        let paths = get_paths(&self.pools, source, destination);
        self.paths = paths;
        self.rebuild_indices();

        Ok(())
    }

    /// Get the best route for the given amount (U256)
    ///
    /// Returns the path with the highest output amount that succeeds.
    /// Returns None if no valid paths exist.
    pub fn get_best(&self, amount: U256) -> Option<EvaluationRouteResult> {
        if self.paths.is_empty() {
            return None;
        }

        let mut best: Option<EvaluationRouteResult> = None;

        for path_with_tokens in &self.paths {
            let path = self.build_path_with_current_pools(path_with_tokens);
            let result = evaluate_path(&path, amount);

            // Only consider successful swaps
            if result.info != SwapInfo::Ok {
                continue;
            }

            // Calculate profit as i128 (can be negative)
            let amount_out = result.amount_out;
            let (profit, profit_hbip) = calculate_profit(amount_out, amount);

            let candidate = EvaluationRouteResult {
                path: path_with_tokens.clone(),
                result,
                hop_count: path_with_tokens.len(),
                amount_in: amount,
                amount_out,
                profit,
                profit_hbip,
            };

            match &best {
                None => best = Some(candidate),
                Some(current_best) => {
                    if candidate.amount_out > current_best.amount_out {
                        best = Some(candidate);
                    }
                }
            }
        }

        best
    }

    /// Get all routes evaluated for the given amount, sorted by output (best first)
    ///
    /// Only returns successful routes (SwapInfo::Ok)
    pub fn get_all_routes(&self, amount: U256) -> Vec<EvaluationRouteResult> {
        let mut results: Vec<EvaluationRouteResult> = Vec::new();

        for path_with_tokens in &self.paths {
            let path = self.build_path_with_current_pools(path_with_tokens);
            let result = evaluate_path(&path, amount);

            if result.info != SwapInfo::Ok {
                continue;
            }

            let amount_out = result.amount_out;
            let (profit, profit_hbip) = calculate_profit(amount_out, amount);

            results.push(EvaluationRouteResult {
                path: path_with_tokens.clone(),
                result,
                hop_count: path_with_tokens.len(),
                amount_in: amount,
                amount_out,
                profit,
                profit_hbip,
            });
        }

        // Sort by amount_out descending
        results.sort_by(|a, b| b.amount_out.cmp(&a.amount_out));

        results
    }

    /// Apply an event to the appropriate pool in the state
    ///
    /// Finds the pool matching the event's pool_id and applies the event.
    /// Returns true if the pool was found and updated, false otherwise.
    pub fn apply(&mut self, event: PoolEvent) -> bool {
        let pool_id = event.pool_id();

        // Use pre-computed key hash from PoolId (no parsing needed!)
        let key_hash = pool_id.key_hash();

        // O(1) lookup using the pool_index HashMap
        let pool = match self
            .pool_index
            .get(&key_hash)
            .and_then(|&idx| self.pools.get_mut(idx))
        {
            Some(p) => p,
            None => return false,
        };

        // Apply the event
        match event {
            PoolEvent::UpdateTick(e) => {
                pool.pool.update_tick(e.to_update_tick());
            }
            PoolEvent::Update(e) => {
                pool.pool.update(e.liquidity, e.sqrt_ratio, e.tick);
            }
        }

        true
    }

    /// Apply an UpdateTick event directly
    pub fn apply_update_tick(&mut self, event: UpdateTickEvent) -> bool {
        self.apply(PoolEvent::UpdateTick(event))
    }

    /// Apply an Update event directly
    pub fn apply_update(&mut self, event: UpdateEvent) -> bool {
        self.apply(PoolEvent::Update(event))
    }

    /// Apply a transaction (all events in order)
    /// Returns a tuple: (number of successful events, list of affected pool key hashes)
    pub fn apply_tx(&mut self, tx: Transaction) -> (usize, Vec<u64>) {
        let mut success_count = 0;
        let mut affected_pools_set = std::collections::HashSet::new();

        for event in tx.events {
            // Use apply_with_key to get the key hash without extra allocation
            if let Some(key_hash) = self.apply_with_key(event) {
                success_count += 1;
                affected_pools_set.insert(key_hash);
            }
        }

        (success_count, affected_pools_set.into_iter().collect())
    }

    /// Apply an event and return the pool key hash if successful
    /// Uses pre-parsed values from PoolId (no hex parsing at runtime)
    fn apply_with_key(&mut self, event: PoolEvent) -> Option<u64> {
        let pool_id = event.pool_id();

        // Use the pre-computed key hash from PoolId (no parsing needed!)
        let key_hash = pool_id.key_hash();

        // O(1) lookup
        let pool_idx = *self.pool_index.get(&key_hash)?;
        let pool = self.pools.get_mut(pool_idx)?;

        // Apply the event
        match event {
            PoolEvent::UpdateTick(e) => {
                pool.pool.update_tick(e.to_update_tick());
            }
            PoolEvent::Update(e) => {
                pool.pool.update(e.liquidity, e.sqrt_ratio, e.tick);
            }
        }

        Some(key_hash)
    }

    /// Get the best route among paths that use any of the specified pools.
    /// More efficient than get_all_routes_for_pools when you only need the best.
    pub fn get_best_for_pools(
        &self,
        amount: U256,
        pool_key_hashes: &[u64],
    ) -> Option<EvaluationRouteResult> {
        // Collect all path indices that use any of the specified pools
        let mut relevant_path_indices = std::collections::HashSet::new();
        for &key_hash in pool_key_hashes {
            if let Some(indices) = self.paths_by_pool.get(&key_hash) {
                for &idx in indices {
                    relevant_path_indices.insert(idx);
                }
            }
        }

        // Find the best without collecting all results
        let mut best: Option<(usize, U256, EvaluatePathResult)> = None;

        for idx in relevant_path_indices {
            let path_with_tokens = &self.paths[idx];
            let path = self.build_path_with_current_pools(path_with_tokens);
            let result = evaluate_path(&path, amount);

            if result.info != SwapInfo::Ok {
                continue;
            }

            let is_better = match &best {
                None => true,
                Some((_, best_amount, _)) => result.amount_out > *best_amount,
            };

            if is_better {
                best = Some((idx, result.amount_out, result));
            }
        }

        // Only clone the winning path
        best.map(|(idx, _, result)| {
            let path_with_tokens: &PathWithTokens = &self.paths[idx];
            let amount_out = result.amount_out;
            let (profit, profit_hbip) = calculate_profit(amount_out, amount);

            EvaluationRouteResult {
                path: path_with_tokens.clone(),
                result,
                hop_count: path_with_tokens.len(),
                amount_in: amount,
                amount_out,
                profit,
                profit_hbip,
            }
        })
    }

    /// Get all successful routes that use any of the specified pools, sorted by output (best first)
    pub fn get_all_routes_for_pools(
        &self,
        amount: U256,
        pool_key_hashes: &[u64],
    ) -> Vec<EvaluationRouteResult> {
        // Collect all path indices that use any of the specified pools
        let mut relevant_path_indices = std::collections::HashSet::new();
        for &key_hash in pool_key_hashes {
            if let Some(indices) = self.paths_by_pool.get(&key_hash) {
                for &idx in indices {
                    relevant_path_indices.insert(idx);
                }
            }
        }

        // Evaluate only the relevant paths
        let mut results: Vec<EvaluationRouteResult> = Vec::new();

        for idx in relevant_path_indices {
            let path_with_tokens = &self.paths[idx];
            let path = self.build_path_with_current_pools(path_with_tokens);
            let result = evaluate_path(&path, amount);

            if result.info != SwapInfo::Ok {
                continue;
            }

            let amount_out = result.amount_out;
            let (profit, profit_hbip) = calculate_profit(amount_out, amount);

            results.push(EvaluationRouteResult {
                path: path_with_tokens.clone(),
                result,
                hop_count: path_with_tokens.len(),
                amount_in: amount,
                amount_out,
                profit,
                profit_hbip,
            });
        }

        // Sort by amount_out descending
        results.sort_by(|a, b| b.result.amount_out.cmp(&a.result.amount_out));

        results
    }

    /// Get the best route among paths that use any of the specified pool IDs
    pub fn get_best_for_pool_ids(
        &self,
        amount: U256,
        pool_ids: &[PoolId],
    ) -> Option<EvaluationRouteResult> {
        // Use pre-computed key hashes from PoolIds (no parsing needed!)
        let key_hashes: Vec<u64> = pool_ids.iter().map(|id| id.key_hash()).collect();
        self.get_best_for_pools(amount, &key_hashes)
    }

    /// Rebuild the lookup indices from the current paths
    fn rebuild_indices(&mut self) {
        self.paths_by_pool.clear();
        self.paths_by_pool_directed.clear();

        for (path_idx, path) in self.paths.iter().enumerate() {
            for hop in path {
                let key_hash = hop.pool.key_hash();

                // Add to paths_by_pool
                self.paths_by_pool
                    .entry(key_hash)
                    .or_default()
                    .push(path_idx);

                // Add to paths_by_pool_directed
                self.paths_by_pool_directed
                    .entry(key_hash)
                    .or_default()
                    .entry(hop.direction)
                    .or_default()
                    .push(path_idx);
            }
        }
    }

    /// Get all paths that contain a specific pool
    /// Get all paths that contain a specific pool by key hash
    pub fn get_paths_by_pool(&self, key_hash: u64) -> Vec<&PathWithTokens> {
        self.paths_by_pool
            .get(&key_hash)
            .map(|indices| indices.iter().map(|&idx| &self.paths[idx]).collect())
            .unwrap_or_default()
    }

    /// Get all paths that contain a specific pool, returning clones
    pub fn get_paths_by_pool_cloned(&self, key_hash: u64) -> Vec<PathWithTokens> {
        self.paths_by_pool
            .get(&key_hash)
            .map(|indices| indices.iter().map(|&idx| self.paths[idx].clone()).collect())
            .unwrap_or_default()
    }

    /// Get all path indices that contain a specific pool
    pub fn get_path_indices_by_pool(&self, key_hash: u64) -> &[usize] {
        self.paths_by_pool
            .get(&key_hash)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get all paths that use a specific pool in a specific direction
    pub fn get_paths_by_pool_and_direction(
        &self,
        key_hash: u64,
        direction: Direction,
    ) -> Vec<&PathWithTokens> {
        self.paths_by_pool_directed
            .get(&key_hash)
            .and_then(|by_dir| by_dir.get(&direction))
            .map(|indices| indices.iter().map(|&idx| &self.paths[idx]).collect())
            .unwrap_or_default()
    }

    /// Get a pool by its key hash
    pub fn get_pool(&self, key_hash: u64) -> Option<&PoolWithTokens> {
        let idx = *self.pool_index.get(&key_hash)?;
        self.pools.get(idx)
    }

    /// Get a pool by PoolId
    pub fn get_pool_by_id(&self, pool_id: &PoolId) -> Option<&PoolWithTokens> {
        self.get_pool(pool_id.key_hash())
    }

    /// Add paths and update indices
    pub fn add_paths(&mut self, new_paths: Vec<PathWithTokens>) {
        let start_idx = self.paths.len();
        self.paths.extend(new_paths);

        // Update indices for only the new paths
        for path_idx in start_idx..self.paths.len() {
            let path = &self.paths[path_idx];
            for hop in path {
                let key_hash = hop.pool.key_hash();

                self.paths_by_pool
                    .entry(key_hash)
                    .or_default()
                    .push(path_idx);

                self.paths_by_pool_directed
                    .entry(key_hash)
                    .or_default()
                    .entry(hop.direction)
                    .or_default()
                    .push(path_idx);
            }
        }
    }

    /// Get the number of paths
    pub fn path_count(&self) -> usize {
        self.paths.len()
    }

    /// Get the number of pools
    pub fn pool_count(&self) -> usize {
        self.pools.len()
    }

    /// Check if a pool is used in any path (by key hash)
    pub fn pool_is_used(&self, key_hash: u64) -> bool {
        self.paths_by_pool.contains_key(&key_hash)
    }

    /// Load pools from a JSON file and compute paths between source and destination
    pub fn from_json_file<P: AsRef<Path>>(
        path: P,
        source: U256,
        destination: U256,
    ) -> Result<Self, LoadError> {
        let contents = fs::read_to_string(path)?;
        Self::from_json(&contents, source, destination)
    }

    /// Load pools from a JSON string and compute paths between source and destination
    pub fn from_json(json: &str, source: U256, destination: U256) -> Result<Self, LoadError> {
        let pools = Self::parse_pools_json(json)?;
        Ok(Self::from_pools(pools, source, destination))
    }

    /// Load pools from a JSON file without computing paths
    pub fn from_json_file_no_paths<P: AsRef<Path>>(path: P) -> Result<Self, LoadError> {
        let contents = fs::read_to_string(path)?;
        Self::from_json_no_paths(&contents)
    }

    /// Load pools from a JSON string without computing paths
    pub fn from_json_no_paths(json: &str) -> Result<Self, LoadError> {
        let pools = Self::parse_pools_json(json)?;
        Ok(Self::from_pools_and_paths(pools, Vec::new()))
    }

    /// Parse a JSON string into a vector of PoolWithTokens
    pub fn parse_pools_json(json: &str) -> Result<Vec<PoolWithTokens>, LoadError> {
        let json_pools: Vec<JsonPool> = serde_json::from_str(json)?;
        json_pools
            .iter()
            .map(json_pool_to_pool_with_tokens)
            .collect()
    }

    /// Add pools from JSON and optionally compute paths
    pub fn add_pools_from_json(
        &mut self,
        json: &str,
        source: Option<U256>,
        destination: Option<U256>,
    ) -> Result<(), LoadError> {
        let new_pools = Self::parse_pools_json(json)?;
        self.pools.extend(new_pools);

        // Compute and add paths if source and destination are provided
        if let (Some(src), Some(dst)) = (source, destination) {
            let new_paths = get_paths(&self.pools, src, dst);
            // Clear and rebuild to avoid duplicates
            self.paths = new_paths;
            self.rebuild_indices();
        }

        Ok(())
    }

    /// Export the state to a JSON file in the same format as the input
    ///
    /// # Arguments
    /// * `path` - The file path to write the JSON to
    ///
    /// # Returns
    /// * `Ok(())` on success, or a `LoadError` on failure
    pub fn export_to_json_file<P: AsRef<Path>>(&self, path: P) -> Result<(), LoadError> {
        let json = self.export_to_json()?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Export the state to a JSON string in the same format as the input
    ///
    /// # Returns
    /// * `Ok(String)` containing the JSON, or a `LoadError` on failure
    pub fn export_to_json(&self) -> Result<String, LoadError> {
        let json_pools: Vec<JsonPool> = self.pools.iter().map(pool_with_tokens_to_json).collect();
        serde_json::to_string_pretty(&json_pools).map_err(LoadError::from)
    }

    /// Export the state to a JSON string without pretty printing (compact format)
    ///
    /// # Returns
    /// * `Ok(String)` containing the JSON, or a `LoadError` on failure
    pub fn export_to_json_compact(&self) -> Result<String, LoadError> {
        let json_pools: Vec<JsonPool> = self.pools.iter().map(pool_with_tokens_to_json).collect();
        serde_json::to_string(&json_pools).map_err(LoadError::from)
    }

    // ============ All-Token Cycle Methods ============

    /// Get all unique tokens from the pools
    pub fn get_all_tokens(&self) -> Vec<U256> {
        let mut tokens: HashSet<U256> = HashSet::new();
        for pool in &self.pools {
            tokens.insert(pool.token0);
            tokens.insert(pool.token1);
        }
        tokens.into_iter().collect()
    }

    /// Initialize cycles for ALL tokens in the pool graph
    ///
    /// Computes cycle paths (token -> ... -> same token) for every token
    /// and builds indices for efficient lookup when pools change.
    /// Call this once after loading pools.
    pub fn init_all_cycles(&mut self) {
        // Get all unique tokens
        let all_tokens = self.get_all_tokens();
        info!(
            token_count = all_tokens.len(),
            pool_count = self.pools.len(),
            "init_all_cycles: starting cycle computation for all tokens"
        );

        // Clear existing cycle indices
        self.paths.clear();
        self.all_cycles.clear();
        self.tokens_by_pool.clear();
        self.paths_by_pool.clear();
        self.paths_by_pool_directed.clear();

        let mut tokens_with_cycles = 0usize;
        let mut total_paths = 0usize;

        // Compute cycles for each token
        for token in all_tokens {
            let cycle_paths = get_paths_with_max_hops(&self.pools, token, token, self.max_hops);

            if cycle_paths.is_empty() {
                continue;
            }

            tokens_with_cycles += 1;
            total_paths += cycle_paths.len();

            let start_idx = self.paths.len();
            let path_indices: Vec<usize> = (start_idx..start_idx + cycle_paths.len()).collect();

            // Store path indices for this token
            self.all_cycles.insert(token, path_indices);

            // Add paths and update indices
            for path in cycle_paths {
                let path_idx = self.paths.len();

                // Update tokens_by_pool and paths_by_pool indices
                for hop in &path {
                    let key_hash = hop.pool.key_hash();

                    // Track which tokens have cycles through this pool
                    self.tokens_by_pool
                        .entry(key_hash)
                        .or_default()
                        .insert(token);

                    // Standard paths_by_pool index
                    self.paths_by_pool
                        .entry(key_hash)
                        .or_default()
                        .push(path_idx);

                    // Directed index
                    self.paths_by_pool_directed
                        .entry(key_hash)
                        .or_default()
                        .entry(hop.direction)
                        .or_default()
                        .push(path_idx);
                }

                self.paths.push(path);
            }
        }

        info!(
            tokens_with_cycles,
            total_paths,
            pools_in_cycles = self.tokens_by_pool.len(),
            "init_all_cycles: completed"
        );
    }

    /// Prebuild paths for the given tokens.
    /// Call this after init_all_cycles with the tokens you want to trade.
    pub fn init_paths_for_tokens(&mut self, tokens: &[U256]) {
        self.paths_by_token.clear();

        for token in tokens {
            if let Some(path_indices) = self.all_cycles.get(token) {
                let paths: Vec<PathWithTokens> = path_indices
                    .iter()
                    .filter_map(|&idx| self.paths.get(idx).cloned())
                    .collect();

                self.paths_by_token.insert(*token, paths);
            }
        }

        info!(
            tokens_initialized = self.paths_by_token.len(),
            "init_paths_for_tokens: completed"
        );
    }

    /// Get prebuilt paths for a token
    pub fn get_paths_for_token(&self, token: &U256) -> Option<&Vec<PathWithTokens>> {
        self.paths_by_token.get(token)
    }

    /// Get the number of tokens with cycles
    pub fn cycle_token_count(&self) -> usize {
        self.all_cycles.len()
    }

    /// Get all tokens that have cycles through the given pool
    pub fn get_tokens_by_pool(&self, key_hash: u64) -> Vec<U256> {
        self.tokens_by_pool
            .get(&key_hash)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Get cycle path indices for a specific token
    pub fn get_cycle_indices(&self, token: U256) -> Option<&Vec<usize>> {
        self.all_cycles.get(&token)
    }

    // ============ Pool-Derived Pricing ============

    /// Get the quote (output amount) for swapping `amount` of `token` to `quote_token`
    ///
    /// Uses the shortest path through pools to derive the price.
    /// Returns None if no path exists or swap fails.
    pub fn get_quote(&self, token: U256, quote_token: U256, amount: U256) -> Option<U256> {
        if token == quote_token {
            return Some(amount);
        }

        if amount.is_zero() {
            return Some(U256::ZERO);
        }

        // Find paths from token to quote_token
        let pool = self.pools.iter().find(|p| {
            (p.token0 == token && p.token1 == quote_token)
                || (p.token0 == quote_token && p.token1 == token)
        })?;

        let result = evaluate_path(
            &vec![Hop {
                direction: if token == pool.token0 {
                    Direction::T0ToT1
                } else {
                    Direction::T1ToT0
                },
                pool,
            }],
            amount,
        );

        if result.info != SwapInfo::Ok {
            return None;
        }

        Some(result.amount_out)
    }

    /// Ternary search iterations for amount optimization
    const SEARCH_ITERATIONS: usize = 10;

    /// Find best cycle for a specific token (by profit_hbip)
    /// Then optimizes the amount using ternary search within 50% range
    fn find_best_cycle_for_token(
        &self,
        paths: &[&PathWithTokens],
        base_amount: U256,
    ) -> Option<EvaluationRouteResult> {
        // Step 1: Find best path at base amount
        let mut best_idx: Option<usize> = None;
        let mut best_hbip = i128::MIN;

        for (idx, path_with_tokens) in paths.iter().enumerate() {
            let path = self.build_path_with_current_pools(path_with_tokens);
            let result = evaluate_path(&path, base_amount);

            if result.info != SwapInfo::Ok {
                continue;
            }

            let (_, profit_hbip) = calculate_profit(result.amount_out, base_amount);

            if profit_hbip > best_hbip {
                best_hbip = profit_hbip;
                best_idx = Some(idx);
            }
        }

        let best_idx = best_idx?;
        let best_path = paths[best_idx];

        // Step 2: Optimize amount for the best path using ternary search
        // Search range: 20% to 300% of base_amount
        let mut low = base_amount / U256::from(5u64); // 20%
        let mut high = base_amount * U256::from(3u64); // 300%

        let mut best_result: Option<(U256, EvaluatePathResult, i128, i128)> = None;

        for _ in 0..Self::SEARCH_ITERATIONS {
            let third = (high - low) / U256::from(3u64);
            let mid1 = low + third;
            let mid2 = high - third;

            let eval_at = |amt: U256| -> Option<(EvaluatePathResult, i128, i128)> {
                let path = self.build_path_with_current_pools(best_path);
                let result = evaluate_path(&path, amt);
                if result.info != SwapInfo::Ok {
                    return None;
                }
                let (profit, profit_hbip) = calculate_profit(result.amount_out, amt);
                Some((result, profit, profit_hbip))
            };

            let r1 = eval_at(mid1);
            let r2 = eval_at(mid2);

            match (r1, r2) {
                (Some((res1, p1, h1)), Some((res2, p2, h2))) => {
                    if p1 > p2 {
                        high = mid2;
                        if best_result.as_ref().is_none_or(|(_, _, bp, _)| p1 > *bp) {
                            best_result = Some((mid1, res1, p1, h1));
                        }
                    } else {
                        low = mid1;
                        if best_result.as_ref().is_none_or(|(_, _, bp, _)| p2 > *bp) {
                            best_result = Some((mid2, res2, p2, h2));
                        }
                    }
                }
                (Some((res1, p1, h1)), None) => {
                    high = mid2;
                    if best_result.as_ref().is_none_or(|(_, _, bp, _)| p1 > *bp) {
                        best_result = Some((mid1, res1, p1, h1));
                    }
                }
                (None, Some((res2, p2, h2))) => {
                    low = mid1;
                    if best_result.as_ref().is_none_or(|(_, _, bp, _)| p2 > *bp) {
                        best_result = Some((mid2, res2, p2, h2));
                    }
                }
                (None, None) => break,
            }
        }

        // Also evaluate at the boundaries and base amount
        let eval_and_compare =
            |amt: U256, current: &mut Option<(U256, EvaluatePathResult, i128, i128)>| {
                let path = self.build_path_with_current_pools(best_path);
                let result = evaluate_path(&path, amt);
                if result.info == SwapInfo::Ok {
                    let (profit, profit_hbip) = calculate_profit(result.amount_out, amt);
                    if current.as_ref().is_none_or(|(_, _, bp, _)| profit > *bp) {
                        *current = Some((amt, result, profit, profit_hbip));
                    }
                }
            };

        eval_and_compare(base_amount, &mut best_result);
        eval_and_compare(low, &mut best_result);
        eval_and_compare(high, &mut best_result);

        best_result.map(|(amount_in, result, profit, profit_hbip)| {
            let amount_out = result.amount_out;
            EvaluationRouteResult {
                path: (*best_path).clone(),
                result,
                hop_count: best_path.len(),
                amount_in,
                amount_out,
                profit,
                profit_hbip,
            }
        })
    }

    /// Find optimal arbitrage when specific pools change
    ///
    /// Only evaluates tokens affected by the changed pools.
    /// More efficient than `find_global_optimal_quoted` for incremental updates.
    ///
    /// # Arguments
    /// * `changed_pool_hashes` - pool key hashes that were updated
    /// * `tokens` - token -> base amount to evaluate at
    pub fn find_optimal_for_changed_pools_quoted(
        &self,
        changed_pool_hashes: &[u64],
        tokens: &HashMap<U256, U256>,
    ) -> Option<GlobalOptimalResult> {
        let pool_set: HashSet<u64> = changed_pool_hashes.iter().copied().collect();

        let mut global_best: Option<GlobalOptimalResult> = None;

        for (token, amount) in tokens {
            // Get prebuilt paths for this token
            let all_paths = match self.paths_by_token.get(token) {
                Some(paths) => paths,
                None => continue,
            };

            // Filter to paths that contain at least one changed pool
            let paths: Vec<&PathWithTokens> = all_paths
                .iter()
                .filter(|path| {
                    path.iter()
                        .any(|hop| pool_set.contains(&hop.pool.key_hash()))
                })
                .collect();

            if paths.is_empty() {
                continue;
            }

            // Get best route (even if negative profit)
            let route = match self.find_best_cycle_for_token(&paths, *amount) {
                Some(r) => r,
                _ => continue,
            };

            debug!(
                token = %format!("{:x}", token),
                profit = route.profit,
                profit_hbip = route.profit_hbip,
                amount_in = %route.amount_in,
                amount_out = %route.amount_out,
                "find_optimal_for_changed_pools_quoted: profitable opportunity"
            );

            if global_best
                .as_ref()
                .is_none_or(|g| route.profit_hbip > g.profit_hbip)
            {
                global_best = Some(GlobalOptimalResult {
                    token: *token,
                    amount_in: route.amount_in,
                    amount_out: route.amount_out,
                    profit: route.profit,
                    profit_hbip: route.profit_hbip,
                    path: route.path,
                    result: route.result,
                });
            }
        }

        debug!(
            found_best = global_best.is_some(),
            "find_optimal_for_changed_pools_quoted: search complete"
        );

        global_best
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}
