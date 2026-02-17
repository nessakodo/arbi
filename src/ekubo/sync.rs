//! State synchronization module for syncing Ekubo pool state with on-chain events.
//!
//! This module provides functionality to:
//! - Initialize state from a JSON snapshot file
//! - Fetch events from Starknet via JSON-RPC
//! - Parse Ekubo contract events (PositionUpdated, Swapped)
//! - Apply events to update pool state

use std::path::Path;

use serde::Deserialize;
use starknet::core::types::Felt;
use thiserror::Error;
use tracing::{debug, info};

use crate::constants::EKUBO_CORE_ADDRESS;
use crate::rpc::RPC;

use super::events::{PoolEvent, Transaction, POSITION_UPDATED_KEY, SWAPPED_KEY};
use super::state::{LoadError, State};

/// Errors that can occur during sync
#[derive(Error, Debug)]
pub enum SyncError {
    #[error("Load error: {0}")]
    Load(#[from] LoadError),

    #[error("Provider error: {0}")]
    Provider(String),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("Parse error: {0}")]
    Parse(String),
}

/// Event from the RPC response
#[derive(Deserialize, Debug, Clone)]
pub struct RpcEvent {
    pub block_hash: String,
    pub block_number: u64,
    pub data: Vec<String>,
    pub from_address: String,
    pub keys: Vec<String>,
    pub transaction_hash: String,
}

/// Events page from the RPC response
#[derive(Deserialize, Debug)]
pub struct EventsPage {
    /// List of events in this page
    pub events: Vec<RpcEvent>,
    /// Token for fetching the next page (None if this is the last page)
    pub continuation_token: Option<String>,
}

/// Configuration for syncing state
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Ekubo core contract address
    pub ekubo_address: Felt,
    /// Chunk size for fetching events (max events per request)
    pub chunk_size: u64,
}

impl SyncConfig {
    /// Create a new sync configuration with default settings
    pub fn new() -> Result<Self, SyncError> {
        let ekubo_address = Felt::from_hex(EKUBO_CORE_ADDRESS)
            .map_err(|e| SyncError::Parse(format!("Invalid Ekubo address: {}", e)))?;

        Ok(Self {
            ekubo_address,
            chunk_size: 1000,
        })
    }

    /// Create config with a custom Ekubo contract address
    pub fn with_ekubo_address(mut self, address: &str) -> Result<Self, SyncError> {
        self.ekubo_address = Felt::from_hex(address)
            .map_err(|e| SyncError::Parse(format!("Invalid Ekubo address: {}", e)))?;
        Ok(self)
    }

    /// Set the chunk size for event fetching
    pub fn with_chunk_size(mut self, size: u64) -> Self {
        self.chunk_size = size;
        self
    }
}

/// Result of syncing state
#[derive(Debug, Clone)]
pub struct SyncResult {
    /// Number of events processed
    pub events_processed: usize,
    /// Number of transactions applied
    pub transactions_applied: usize,
    /// Starting block number
    pub from_block: u64,
    /// Final block number
    pub to_block: u64,
}

/// Initialize state from JSON and sync with on-chain events
///
/// This function:
/// 1. Loads pool state from a JSON snapshot file
/// 2. Fetches all Ekubo events from `from_block` to `to_block`
/// 3. Parses events into pool updates
/// 4. Applies all events to the state via `apply_tx`
///
/// # Arguments
/// * `json_path` - Path to the JSON file containing initial pool state
/// * `from_block` - Starting block number (inclusive)
/// * `to_block` - Ending block number (inclusive)
/// * `config` - Sync configuration
///
/// # Returns
/// * Tuple of (synced State, SyncResult with statistics)
///
/// # Example
/// ```ignore
/// let config = SyncConfig::new()?;
/// let (state, result) = init_and_sync_state("pools.json", 100000, 100500, &config).await?;
/// println!("Synced {} events", result.events_processed);
/// ```
pub async fn init_and_sync_state<P: AsRef<Path>>(
    rpc: &RPC,
    json_path: P,
    from_block: u64,
    to_block: u64,
    config: &SyncConfig,
) -> Result<(State, SyncResult), SyncError> {
    info!("Loading initial state from JSON...");
    let mut state = State::from_json_file_no_paths(&json_path)?;
    info!("Loaded {} pools from JSON", state.pool_count());

    let result = sync_state(rpc, &mut state, from_block, to_block, config).await?;
    Ok((state, result))
}

/// Fetch all transactions from a block range without applying them
///
/// Fetches all events from the Ekubo contract first, then groups them into transactions.
/// This ensures transactions spanning chunk boundaries are not split.
///
/// # Arguments
/// * `from_block` - Starting block number (inclusive)
/// * `to_block` - Ending block number (inclusive)
/// * `config` - Sync configuration
///
/// # Returns
/// * `Vec<Transaction>` containing all parsed transactions in order
pub async fn fetch_transactions(
    rpc: &RPC,
    from_block: u64,
    to_block: u64,
    config: &SyncConfig,
) -> Result<Vec<Transaction>, SyncError> {
    info!(
        "Fetching transactions from block {} to block {}",
        from_block, to_block
    );

    let ekubo_address = format!("{:#x}", config.ekubo_address);

    // Collect all events first
    let mut all_events: Vec<RpcEvent> = Vec::new();
    let mut continuation_token: Option<String> = None;

    loop {
        debug!(
            "Fetching events: from={}, to={}, address={}",
            from_block, to_block, ekubo_address
        );

        let events_page = fetch_events(
            rpc,
            &ekubo_address,
            from_block,
            to_block,
            config.chunk_size,
            continuation_token.clone(),
        )
        .await?;

        info!(
            "Fetched {} events in this batch (continuation: {:?})",
            events_page.events.len(),
            events_page.continuation_token
        );

        all_events.extend(events_page.events);

        // Check if there are more events
        if events_page.continuation_token.is_none() {
            break;
        }
        continuation_token = events_page.continuation_token;

        debug!("Fetched {} events so far", all_events.len());
    }

    // Group all events by transaction hash (preserves order)
    let transactions = group_events_by_tx(&all_events);

    info!(
        "Fetch complete: {} events grouped into {} transactions from blocks {}-{}",
        all_events.len(),
        transactions.len(),
        from_block,
        to_block
    );

    Ok(transactions)
}

/// Sync an existing state with on-chain events
///
/// Fetches events from the Ekubo contract and applies them to the state.
///
/// # Arguments
/// * `state` - Mutable reference to the state to update
/// * `from_block` - Starting block number (inclusive)
/// * `to_block` - Ending block number (inclusive)
/// * `config` - Sync configuration
///
/// # Returns
/// * `SyncResult` containing statistics about the sync operation
pub async fn sync_state(
    rpc: &RPC,
    state: &mut State,
    from_block: u64,
    to_block: u64,
    config: &SyncConfig,
) -> Result<SyncResult, SyncError> {
    // Fetch all transactions
    let transactions = fetch_transactions(rpc, from_block, to_block, config).await?;

    // Apply transactions to state
    let result = apply_transactions(state, transactions, from_block, to_block);

    Ok(result)
}

/// Apply a list of transactions to the state
///
/// # Arguments
/// * `state` - Mutable reference to the state to update
/// * `transactions` - List of transactions to apply
/// * `from_block` - Starting block number (for result metadata)
/// * `to_block` - Ending block number (for result metadata)
///
/// # Returns
/// * `SyncResult` containing statistics about the sync operation
pub fn apply_transactions(
    state: &mut State,
    transactions: Vec<Transaction>,
    from_block: u64,
    to_block: u64,
) -> SyncResult {
    let mut events_processed = 0;
    let mut transactions_applied = 0;
    let mut unmatched_pools = 0;

    for tx in transactions {
        let event_count = tx.events.len();
        let (success_count, _affected_pools) = state.apply_tx(tx);
        events_processed += success_count;
        transactions_applied += 1;
        unmatched_pools += event_count - success_count;
    }

    if unmatched_pools > 0 {
        debug!(
            "{} events matched pools in state, {} did not",
            events_processed, unmatched_pools
        );
    }

    info!(
        "Applied {} events in {} transactions",
        events_processed, transactions_applied
    );

    SyncResult {
        events_processed,
        transactions_applied,
        from_block,
        to_block,
    }
}

/// Fetch Ekubo events from the RPC for a block range.
pub async fn fetch_events(
    rpc: &RPC,
    address: &str,
    from_block: u64,
    to_block: u64,
    chunk_size: u64,
    continuation_token: Option<String>,
) -> Result<EventsPage, SyncError> {
    let keys: &[&[&str]] = &[&[SWAPPED_KEY, POSITION_UPDATED_KEY]];

    rpc.get_events(
        address,
        from_block,
        to_block,
        keys,
        chunk_size,
        continuation_token,
    )
    .await
    .map_err(|e| SyncError::Http(e.to_string()))
}

/// Group emitted events by transaction hash and convert to Transaction objects
/// Preserves the order of events as they appear in the RPC response
pub fn group_events_by_tx(events: &[RpcEvent]) -> Vec<Transaction> {
    let mut transactions: Vec<Transaction> = Vec::new();

    for event in events {
        if let Some(pool_event) = PoolEvent::from_rpc_data(&event.keys, &event.data) {
            // Check if this event belongs to the current (last) transaction
            if let Some(last_tx) = transactions.last_mut() {
                if last_tx.tx_hash == event.transaction_hash {
                    // Same transaction, append event
                    last_tx.events.push(pool_event);
                    continue;
                }
            }

            // New transaction
            transactions.push(Transaction::new(
                event.transaction_hash.clone(),
                event.block_number,
                vec![pool_event],
            ));
        }
    }

    transactions
}
