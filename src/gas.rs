//! Gas price types: block header parsing, caching, and resource bounds.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::debug;

use crate::transaction::{ResourceBound, ResourceBounds};

/// Gas price structure from a block header
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GasPrice {
    pub price_in_fri: String,
    pub price_in_wei: String,
}

impl GasPrice {
    /// Parse the fri price as u128, applying the gas price coefficient
    pub fn price_in_fri_with_coefficient(&self) -> u128 {
        let price = parse_hex_to_u128(&self.price_in_fri);
        apply_coefficient(price)
    }
}

/// Block header containing gas prices
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BlockHeader {
    /// Block number
    #[serde(default)]
    pub block_number: Option<u64>,
    /// L1 gas price
    #[serde(default)]
    pub l1_gas_price: Option<GasPrice>,
    /// L1 data gas price (blob gas)
    #[serde(default)]
    pub l1_data_gas_price: Option<GasPrice>,
    /// L2 gas price
    #[serde(default)]
    pub l2_gas_price: Option<GasPrice>,
}

/// Cached gas prices with coefficient applied
#[derive(Debug)]
pub struct GasPriceCache {
    /// L1 gas price in fri (with coefficient applied)
    l1_gas_price: AtomicU64,
    /// L2 gas price in fri (with coefficient applied)
    l2_gas_price: AtomicU64,
    /// L1 data gas price in fri (with coefficient applied)
    l1_data_gas_price: AtomicU64,
    /// Block number these prices are from
    block_number: AtomicU64,
}

impl GasPriceCache {
    /// Create a new gas price cache with the given prices
    pub fn new(l1_gas_price: u128, l2_gas_price: u128, l1_data_gas_price: u128) -> Self {
        Self {
            l1_gas_price: AtomicU64::new(saturating_u128_to_u64(l1_gas_price)),
            l2_gas_price: AtomicU64::new(saturating_u128_to_u64(l2_gas_price)),
            l1_data_gas_price: AtomicU64::new(saturating_u128_to_u64(l1_data_gas_price)),
            block_number: AtomicU64::new(0),
        }
    }

    /// Create with default mainnet values
    pub fn default_mainnet() -> Self {
        Self {
            l1_gas_price: AtomicU64::new(0x2d8788b3d04a),
            l2_gas_price: AtomicU64::new(0x2540BE400),
            l1_data_gas_price: AtomicU64::new(0x4a8ae43c2),
            block_number: AtomicU64::new(0),
        }
    }

    /// Update the cached prices from a block header
    pub fn update_from_header(&self, header: &BlockHeader, block_number: u64) {
        if let Some(ref price) = header.l1_gas_price {
            let value = saturating_u128_to_u64(price.price_in_fri_with_coefficient());
            self.l1_gas_price.store(value, Ordering::SeqCst);
        }
        if let Some(ref price) = header.l2_gas_price {
            let value = saturating_u128_to_u64(price.price_in_fri_with_coefficient());
            self.l2_gas_price.store(value, Ordering::SeqCst);
        }
        if let Some(ref price) = header.l1_data_gas_price {
            let value = saturating_u128_to_u64(price.price_in_fri_with_coefficient());
            self.l1_data_gas_price.store(value, Ordering::SeqCst);
        }
        self.block_number.store(block_number, Ordering::SeqCst);
        debug!(
            "Updated gas prices from block {}: l1={}, l2={}, l1_data={}",
            block_number,
            self.l1_gas_price(),
            self.l2_gas_price(),
            self.l1_data_gas_price()
        );
    }

    /// Get the cached L1 gas price
    pub fn l1_gas_price(&self) -> u128 {
        self.l1_gas_price.load(Ordering::SeqCst) as u128
    }

    /// Get the cached L2 gas price
    pub fn l2_gas_price(&self) -> u128 {
        self.l2_gas_price.load(Ordering::SeqCst) as u128
    }

    /// Get the cached L1 data gas price
    pub fn l1_data_gas_price(&self) -> u128 {
        self.l1_data_gas_price.load(Ordering::SeqCst) as u128
    }

    /// Get the block number these prices are from
    pub fn block_number(&self) -> u64 {
        self.block_number.load(Ordering::SeqCst)
    }

    /// Create ResourceBounds from the cached prices with default max amounts
    pub fn to_resource_bounds(&self) -> ResourceBounds {
        ResourceBounds {
            l1_gas: ResourceBound::new(100, self.l1_gas_price()),
            l2_gas: ResourceBound::new(5_000_000, self.l2_gas_price()),
            l1_data_gas: ResourceBound::new(2000, self.l1_data_gas_price()),
        }
    }
}

impl Default for GasPriceCache {
    fn default() -> Self {
        Self::default_mainnet()
    }
}

/// Saturating conversion from u128 to u64 (caps at u64::MAX instead of truncating)
fn saturating_u128_to_u64(value: u128) -> u64 {
    match u64::try_from(value) {
        Ok(v) => v,
        Err(_) => {
            tracing::warn!(value = %value, "Gas price truncated from u128 to u64::MAX");
            u64::MAX
        }
    }
}

/// Parse a hex string (with or without 0x prefix) to u128
fn parse_hex_to_u128(hex_str: &str) -> u128 {
    let clean = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    match u128::from_str_radix(clean, 16) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(hex = %hex_str, error = %e, "Failed to parse gas price hex");
            0
        }
    }
}

/// Apply the gas price coefficient (1.05) to a price using integer math
fn apply_coefficient(price: u128) -> u128 {
    price * 105 / 100
}
