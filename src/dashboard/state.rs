use std::collections::VecDeque;
use std::sync::atomic::AtomicU64;

use parking_lot::RwLock;
use serde::Serialize;
use tokio::sync::watch;

const MAX_OPPORTUNITY_HISTORY: usize = 500;
const MAX_PNL_HISTORY: usize = 200;

/// Current bot state snapshot, published after each batch
#[derive(Debug, Clone, Serialize)]
pub struct DashboardSnapshot {
    pub timestamp_ms: u64,
    pub current_block: u64,
    pub ws_connected: bool,
    pub broadcast_enabled: bool,
    pub pool_count: usize,
    pub path_count: usize,
    pub cycle_token_count: usize,
    pub gas_prices: GasPriceSnapshot,
    pub counters: CounterSnapshot,
    pub config: ConfigSnapshot,
}

#[derive(Debug, Clone, Serialize)]
pub struct GasPriceSnapshot {
    pub l1_gas_price: u128,
    pub l2_gas_price: u128,
    pub l1_data_gas_price: u128,
    pub block_number: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CounterSnapshot {
    pub uptime_secs: u64,
    pub transactions_processed: u64,
    pub reactions_sent: u64,
    pub batches_evaluated: u64,
    pub opportunities_found: u64,
    pub opportunities_above_threshold: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigSnapshot {
    pub min_profit_hbip: i128,
    pub tip_percentage: u64,
    pub max_hops: usize,
    pub broadcast: bool,
    pub tokens: Vec<String>,
}

/// A historical opportunity record
#[derive(Debug, Clone, Serialize)]
pub struct OpportunityRecord {
    pub timestamp_ms: u64,
    pub block: u64,
    pub token: String,
    pub amount_in: String,
    pub amount_out: String,
    pub profit: i128,
    pub profit_hbip: i128,
    pub hop_count: usize,
    pub path_display: String,
    pub executed: bool,
    pub tx_hash: Option<String>,
}

/// A P&L data point (tracked per broadcast)
#[derive(Debug, Clone, Serialize)]
pub struct PnlRecord {
    pub timestamp_ms: u64,
    pub block: u64,
    pub token: String,
    pub profit: i128,
    pub profit_hbip: i128,
    pub tx_hash: String,
    pub success: bool,
}

/// Shared dashboard state container
pub struct DashboardState {
    snapshot_tx: watch::Sender<DashboardSnapshot>,
    snapshot_rx: watch::Receiver<DashboardSnapshot>,
    opportunities: RwLock<VecDeque<OpportunityRecord>>,
    pnl_history: RwLock<VecDeque<PnlRecord>>,
    pub batches_evaluated: AtomicU64,
    pub opportunities_found: AtomicU64,
    pub opportunities_above_threshold: AtomicU64,
}

impl DashboardState {
    pub fn new() -> Self {
        let initial = DashboardSnapshot {
            timestamp_ms: now_ms(),
            current_block: 0,
            ws_connected: false,
            broadcast_enabled: false,
            pool_count: 0,
            path_count: 0,
            cycle_token_count: 0,
            gas_prices: GasPriceSnapshot {
                l1_gas_price: 0,
                l2_gas_price: 0,
                l1_data_gas_price: 0,
                block_number: 0,
            },
            counters: CounterSnapshot {
                uptime_secs: 0,
                transactions_processed: 0,
                reactions_sent: 0,
                batches_evaluated: 0,
                opportunities_found: 0,
                opportunities_above_threshold: 0,
            },
            config: ConfigSnapshot {
                min_profit_hbip: 0,
                tip_percentage: 0,
                max_hops: 3,
                broadcast: false,
                tokens: vec![],
            },
        };
        let (tx, rx) = watch::channel(initial);
        Self {
            snapshot_tx: tx,
            snapshot_rx: rx,
            opportunities: RwLock::new(VecDeque::new()),
            pnl_history: RwLock::new(VecDeque::new()),
            batches_evaluated: AtomicU64::new(0),
            opportunities_found: AtomicU64::new(0),
            opportunities_above_threshold: AtomicU64::new(0),
        }
    }

    /// Get a clone of the watch receiver for SSE streaming
    pub fn subscribe(&self) -> watch::Receiver<DashboardSnapshot> {
        self.snapshot_rx.clone()
    }

    /// Get the latest snapshot
    pub fn current_snapshot(&self) -> DashboardSnapshot {
        self.snapshot_rx.borrow().clone()
    }

    /// Publish a new snapshot (non-blocking, overwrites previous)
    pub fn publish_snapshot(&self, snapshot: DashboardSnapshot) {
        let _ = self.snapshot_tx.send(snapshot);
    }

    /// Record an opportunity to the ring buffer
    pub fn record_opportunity(&self, record: OpportunityRecord) {
        let mut buf = self.opportunities.write();
        if buf.len() >= MAX_OPPORTUNITY_HISTORY {
            buf.pop_front();
        }
        buf.push_back(record);
    }

    /// Record a P&L entry to the ring buffer
    pub fn record_pnl(&self, record: PnlRecord) {
        let mut buf = self.pnl_history.write();
        if buf.len() >= MAX_PNL_HISTORY {
            buf.pop_front();
        }
        buf.push_back(record);
    }

    /// Mark the most recent opportunity as executed with a tx hash
    pub fn mark_last_opportunity_executed(&self, tx_hash: String) {
        let mut buf = self.opportunities.write();
        if let Some(last) = buf.back_mut() {
            last.executed = true;
            last.tx_hash = Some(tx_hash);
        }
    }

    /// Get recent opportunities (newest first)
    pub fn get_opportunities(&self, limit: usize) -> Vec<OpportunityRecord> {
        let buf = self.opportunities.read();
        buf.iter().rev().take(limit).cloned().collect()
    }

    /// Get P&L history (newest first)
    pub fn get_pnl_history(&self, limit: usize) -> Vec<PnlRecord> {
        let buf = self.pnl_history.read();
        buf.iter().rev().take(limit).cloned().collect()
    }
}

impl Default for DashboardState {
    fn default() -> Self {
        Self::new()
    }
}

/// Current unix timestamp in milliseconds
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
