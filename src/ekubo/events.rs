use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::LazyLock;

use starknet::core::types::Felt;
use tracing::warn;

use super::pool::{TickBounds, UpdateTick};
use super::swap::{hex_to_u256, U256};

/// Event selector for PositionUpdated event
pub const POSITION_UPDATED_KEY: &str =
    "0x03a7adca3546c213ce791fabf3b04090c163e419c808c9830fb343a4a395946e";

/// Event selector for PositionUpdated event as Felt (pre-parsed)
pub const POSITION_UPDATED_KEY_FELT: Felt =
    Felt::from_hex_unchecked("0x03a7adca3546c213ce791fabf3b04090c163e419c808c9830fb343a4a395946e");

/// Event selector for Swapped event
pub const SWAPPED_KEY: &str = "0x0157717768aca88da4ac4279765f09f4d0151823d573537fbbeb950cdbd9a870";

/// Event selector for Swapped event as Felt (pre-parsed)
pub const SWAPPED_KEY_FELT: Felt =
    Felt::from_hex_unchecked("0x0157717768aca88da4ac4279765f09f4d0151823d573537fbbeb950cdbd9a870");

/// Pre-normalized event keys for fast comparison (computed once)
static NORMALIZED_POSITION_KEY: LazyLock<String> =
    LazyLock::new(|| normalize_hex(POSITION_UPDATED_KEY));
static NORMALIZED_SWAPPED_KEY: LazyLock<String> = LazyLock::new(|| normalize_hex(SWAPPED_KEY));

/// Pool identifier with pre-parsed values for fast lookups.
/// Stores both original hex strings (for display/serialization) and parsed U256 values (for computation).
/// Uses a pre-computed u64 hash key for O(1) HashMap lookups without string allocation.
#[derive(Debug, Clone)]
pub struct PoolId {
    // Original hex strings (kept for compatibility/display)
    pub token0_hex: String,
    pub token1_hex: String,
    pub fee_hex: String,
    pub tick_spacing_hex: String,
    pub extension_hex: String,
    // Pre-parsed U256 values (for computation)
    pub token0: U256,
    pub token1: U256,
    pub fee: U256,
    pub tick_spacing: i64,
    pub extension: U256,
    // Pre-computed hash key for fast lookups
    key_hash: u64,
}

impl PoolId {
    /// Create a new PoolId from hex strings, parsing values upfront
    pub fn new(
        token0: impl Into<String>,
        token1: impl Into<String>,
        fee: impl Into<String>,
        tick_spacing: impl Into<String>,
        extension: impl Into<String>,
    ) -> Self {
        let token0_hex = token0.into();
        let token1_hex = token1.into();
        let fee_hex = fee.into();
        let tick_spacing_hex = tick_spacing.into();
        let extension_hex = extension.into();

        // Parse hex values upfront (for computation, not hashing)
        let token0_val = parse_hex_to_u256(&token0_hex).unwrap_or(U256::ZERO);
        let token1_val = parse_hex_to_u256(&token1_hex).unwrap_or(U256::ZERO);
        let fee_val = parse_hex_to_u256(&fee_hex).unwrap_or(U256::ZERO);
        let tick_spacing_val = parse_hex_to_i64(&tick_spacing_hex).unwrap_or(0);
        let extension_val = parse_hex_to_u256(&extension_hex).unwrap_or(U256::ZERO);

        // Compute hash key using normalized strings
        let key_hash = compute_pool_key_hash_from_strings(
            &token0_hex,
            &token1_hex,
            &fee_hex,
            &tick_spacing_hex,
            &extension_hex,
        );

        Self {
            token0_hex,
            token1_hex,
            fee_hex,
            tick_spacing_hex,
            extension_hex,
            token0: token0_val,
            token1: token1_val,
            fee: fee_val,
            tick_spacing: tick_spacing_val,
            extension: extension_val,
            key_hash,
        }
    }

    /// Create from pre-parsed U256 values
    pub fn from_values(
        token0: U256,
        token1: U256,
        fee: U256,
        tick_spacing: i64,
        extension: U256,
    ) -> Self {
        // Convert to hex strings for consistent hashing
        let token0_hex = format!("{:x}", token0);
        let token1_hex = format!("{:x}", token1);
        let fee_hex = format!("{:x}", fee);
        let tick_spacing_hex = format!("{:#x}", Felt::from(tick_spacing as i128));
        let extension_hex = format!("{:x}", extension);

        let key_hash = compute_pool_key_hash_from_strings(
            &token0_hex,
            &token1_hex,
            &fee_hex,
            &tick_spacing_hex,
            &extension_hex,
        );

        Self {
            token0_hex,
            token1_hex,
            fee_hex,
            tick_spacing_hex,
            extension_hex,
            token0,
            token1,
            fee,
            tick_spacing,
            extension,
            key_hash,
        }
    }

    /// Get the pre-computed hash key (u64, no allocation)
    #[inline]
    pub fn key_hash(&self) -> u64 {
        self.key_hash
    }

    /// Get key as string (for display/debugging, allocates)
    pub fn key_string(&self) -> String {
        format!(
            "{:x}-{:x}-{:x}-{}-{:x}",
            self.token0, self.token1, self.fee, self.tick_spacing, self.extension
        )
    }
}

impl PartialEq for PoolId {
    fn eq(&self, other: &Self) -> bool {
        self.key_hash == other.key_hash
    }
}

impl Eq for PoolId {}

impl Hash for PoolId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key_hash.hash(state);
    }
}

/// Normalize a hex string: strip 0x prefix, remove leading zeros, lowercase
#[inline]
pub fn normalize_hex(s: &str) -> String {
    let s = s.trim().strip_prefix("0x").unwrap_or(s);
    let s = s.trim_start_matches('0');
    if s.is_empty() {
        "0".to_string()
    } else {
        s.to_lowercase()
    }
}

/// Compute a hash key from pool parameters using normalized hex strings
/// This avoids f64 precision loss for large 252-bit Starknet addresses
#[inline]
pub fn compute_pool_key_hash_from_strings(
    token0: &str,
    token1: &str,
    fee: &str,
    tick_spacing: &str,
    extension: &str,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    normalize_hex(token0).hash(&mut hasher);
    normalize_hex(token1).hash(&mut hasher);
    normalize_hex(fee).hash(&mut hasher);
    normalize_hex(tick_spacing).hash(&mut hasher);
    normalize_hex(extension).hash(&mut hasher);
    hasher.finish()
}

/// Parse a hex string to U256
fn parse_hex_to_u256(s: &str) -> Option<U256> {
    hex_to_u256(s).ok()
}

/// Parse a hex string to i64
fn parse_hex_to_i64(s: &str) -> Option<i64> {
    let s = s.trim().strip_prefix("0x").unwrap_or(s);
    i64::from_str_radix(s, 16).ok()
}

/// Event for updating tick bounds (position update)
#[derive(Debug, Clone)]
pub struct UpdateTickEvent {
    pub pool_id: PoolId,
    pub bounds: TickBounds,
    pub delta: i128,
}

impl UpdateTickEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        token0: impl Into<String>,
        token1: impl Into<String>,
        fee: impl Into<String>,
        tick_spacing: impl Into<String>,
        extension: impl Into<String>,
        lower: i64,
        upper: i64,
        delta: i128,
    ) -> Self {
        Self::with_pool_id(
            PoolId::new(token0, token1, fee, tick_spacing, extension),
            lower,
            upper,
            delta,
        )
    }

    pub fn with_pool_id(pool_id: PoolId, lower: i64, upper: i64, delta: i128) -> Self {
        Self {
            pool_id,
            bounds: TickBounds { lower, upper },
            delta,
        }
    }

    /// Convert to UpdateTick for applying to a pool
    pub fn to_update_tick(&self) -> UpdateTick {
        UpdateTick {
            bounds: self.bounds.clone(),
            delta: self.delta,
        }
    }
}

/// Event for direct pool state update (e.g., after a swap)
#[derive(Debug, Clone)]
pub struct UpdateEvent {
    pub pool_id: PoolId,
    /// Liquidity as u128
    pub liquidity: u128,
    /// sqrt_ratio as U256 for precision
    pub sqrt_ratio: U256,
    /// Tick value
    pub tick: i64,
}

impl UpdateEvent {
    /// Create a new UpdateEvent with Felt values
    #[allow(clippy::too_many_arguments)]
    pub fn new_felt(
        token0: impl Into<String>,
        token1: impl Into<String>,
        fee: impl Into<String>,
        tick_spacing: impl Into<String>,
        extension: impl Into<String>,
        liquidity: u128,
        sqrt_ratio_felt: Felt,
        tick: i64,
    ) -> Self {
        Self::with_pool_id_felt(
            PoolId::new(token0, token1, fee, tick_spacing, extension),
            liquidity,
            sqrt_ratio_felt,
            tick,
        )
    }

    /// Create from a PoolId with Felt sqrt_ratio
    pub fn with_pool_id_felt(
        pool_id: PoolId,
        liquidity: u128,
        sqrt_ratio_felt: Felt,
        tick: i64,
    ) -> Self {
        use super::swap::felt_to_u256;
        Self {
            pool_id,
            liquidity,
            sqrt_ratio: felt_to_u256(&sqrt_ratio_felt),
            tick,
        }
    }

    /// Create a new UpdateEvent from U256 values (preferred)
    #[allow(clippy::too_many_arguments)]
    pub fn new_u256(
        token0: impl Into<String>,
        token1: impl Into<String>,
        fee: impl Into<String>,
        tick_spacing: impl Into<String>,
        extension: impl Into<String>,
        liquidity: u128,
        sqrt_ratio: U256,
        tick: i64,
    ) -> Self {
        Self {
            pool_id: PoolId::new(token0, token1, fee, tick_spacing, extension),
            liquidity,
            sqrt_ratio,
            tick,
        }
    }
}

/// Unified event enum that can represent any pool event
#[derive(Debug, Clone)]
pub enum PoolEvent {
    /// Update tick bounds (add/remove liquidity position)
    UpdateTick(UpdateTickEvent),
    /// Direct pool state update (swap occurred)
    Update(UpdateEvent),
}

/// A transaction containing multiple events to apply atomically
#[derive(Debug, Clone)]
pub struct Transaction {
    pub tx_hash: String,
    pub block: u64,
    pub events: Vec<PoolEvent>,
}

impl Transaction {
    pub fn new(tx_hash: impl Into<String>, block: u64, events: Vec<PoolEvent>) -> Self {
        Self {
            tx_hash: tx_hash.into(),
            block,
            events,
        }
    }

    /// Get all unique pool IDs affected by this transaction
    pub fn affected_pools(&self) -> Vec<&PoolId> {
        let mut pools = Vec::new();
        for event in &self.events {
            let pool_id = event.pool_id();
            if !pools.contains(&pool_id) {
                pools.push(pool_id);
            }
        }
        pools
    }
}

impl PoolEvent {
    /// Get the pool ID for this event
    pub fn pool_id(&self) -> &PoolId {
        match self {
            PoolEvent::UpdateTick(e) => &e.pool_id,
            PoolEvent::Update(e) => &e.pool_id,
        }
    }

    /// Create an UpdateTick event
    #[allow(clippy::too_many_arguments)]
    pub fn update_tick(
        token0: impl Into<String>,
        token1: impl Into<String>,
        fee: impl Into<String>,
        tick_spacing: impl Into<String>,
        extension: impl Into<String>,
        lower: i64,
        upper: i64,
        delta: i128,
    ) -> Self {
        PoolEvent::UpdateTick(UpdateTickEvent::new(
            token0,
            token1,
            fee,
            tick_spacing,
            extension,
            lower,
            upper,
            delta,
        ))
    }

    /// Parse an Ekubo contract event into a PoolEvent from its components
    pub fn from_rpc_data(keys: &[String], data: &[String]) -> Option<PoolEvent> {
        if keys.is_empty() {
            return None;
        }

        let event_key = &keys[0];

        // Normalize the key for comparison (RPC may return without leading zeros)
        let normalized_key = normalize_hex(event_key);

        // Check for PositionUpdated event (liquidity change)
        if normalized_key == *NORMALIZED_POSITION_KEY {
            return parse_position_updated_data(data);
        }

        // Check for Swapped event (pool state update)
        if normalized_key == *NORMALIZED_SWAPPED_KEY {
            return parse_swapped_data(data);
        }

        None
    }
}

/// Parse PositionUpdated event into UpdateTickEvent
fn parse_position_updated_data(data: &[String]) -> Option<PoolEvent> {
    // Need enough data fields: locker(1) + pool_key(5) + salt(1) + bounds(4) + liquidity(2) = 13
    if data.len() < 13 {
        warn!(
            "PositionUpdated event has insufficient data: {} fields (expected >= 13)",
            data.len()
        );
        return None;
    }

    // data[0] = locker (skip)
    // data[1..6] = pool key (token0, token1, fee, tick_spacing, extension)
    let pool_id = PoolId::new(
        data[1].clone(),
        data[2].clone(),
        data[3].clone(),
        data[4].clone(),
        data[5].clone(),
    );
    // data[6] = salt (skip)

    // Parse tick bounds with signed-magnitude representation
    let lower = parse_signed_i64(&data[7], &data[8]);
    let upper = parse_signed_i64(&data[9], &data[10]);
    let delta = parse_signed_i128(&data[11], &data[12]);

    let (lower, upper, delta) = match (lower, upper, delta) {
        (Some(l), Some(u), Some(d)) => (l, u, d),
        _ => {
            warn!("PositionUpdated event has unparseable tick/delta fields");
            return None;
        }
    };

    Some(PoolEvent::UpdateTick(UpdateTickEvent::with_pool_id(
        pool_id, lower, upper, delta,
    )))
}

/// Parse Swapped event into UpdateEvent
fn parse_swapped_data(data: &[String]) -> Option<PoolEvent> {
    // Need enough data fields: locker(1) + pool_key(5) + params(6) + delta(4) + after(5) = 21
    if data.len() < 21 {
        warn!(
            "Swapped event has insufficient data: {} fields (expected >= 21)",
            data.len()
        );
        return None;
    }

    // data[0] = locker (skip)
    // data[1..6] = pool key (token0, token1, fee, tick_spacing, extension)
    let pool_id = PoolId::new(
        data[1].clone(),
        data[2].clone(),
        data[3].clone(),
        data[4].clone(),
        data[5].clone(),
    );

    // The last 5 fields are: sqrt_ratio_after (2 felts), tick_after (mag, sign), liquidity_after
    let data_len = data.len();

    // sqrt_ratio_after is a u256 split into low/high - combine into Felt
    let sqrt_ratio_felt = match hex_u256_to_felt(&data[data_len - 5], &data[data_len - 4]) {
        Some(v) => v,
        None => {
            warn!("Swapped event has unparseable sqrt_ratio fields");
            return None;
        }
    };

    // tick_after with signed representation (mag, sign)
    let tick = match parse_signed_i64(&data[data_len - 3], &data[data_len - 2]) {
        Some(v) => v,
        None => {
            warn!("Swapped event has unparseable tick fields");
            return None;
        }
    };

    // liquidity_after as u128
    let liquidity = match parse_hex_u128(&data[data_len - 1]) {
        Some(v) => v,
        None => {
            warn!("Swapped event has unparseable liquidity field");
            return None;
        }
    };

    Some(PoolEvent::Update(UpdateEvent::with_pool_id_felt(
        pool_id,
        liquidity,
        sqrt_ratio_felt,
        tick,
    )))
}

/// Combine two hex strings (low, high) into a single Felt representing a U256
fn hex_u256_to_felt(low_hex: &str, high_hex: &str) -> Option<Felt> {
    let low = Felt::from_hex(low_hex).ok()?;
    let high = Felt::from_hex(high_hex).ok()?;

    // 2^128 as Felt
    let two_128 =
        Felt::from_hex("0x100000000000000000000000000000000").expect("constant 2^128 is valid");

    Some(high * two_128 + low)
}

/// Check if a hex sign field indicates negative (any non-zero value).
/// Handles all RPC formatting variants: "0x0", "0x00", "0x000", "0", etc.
fn is_negative_sign(hex: &str) -> bool {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    !hex.chars().all(|c| c == '0')
}

/// Parse signed-magnitude i64 from a (magnitude_hex, sign_hex) pair.
/// Parses magnitude as u64 to avoid misinterpreting large values.
fn parse_signed_i64(mag_hex: &str, sign_hex: &str) -> Option<i64> {
    let hex = mag_hex.strip_prefix("0x").unwrap_or(mag_hex);
    let mag = u64::from_str_radix(hex, 16).ok()?;
    let value = i64::try_from(mag).ok()?;
    Some(if is_negative_sign(sign_hex) {
        -value
    } else {
        value
    })
}

/// Parse signed-magnitude i128 from a (magnitude_hex, sign_hex) pair.
/// Parses magnitude as u128 to avoid misinterpreting large values.
fn parse_signed_i128(mag_hex: &str, sign_hex: &str) -> Option<i128> {
    let hex = mag_hex.strip_prefix("0x").unwrap_or(mag_hex);
    let mag = u128::from_str_radix(hex, 16).ok()?;
    let value = i128::try_from(mag).ok()?;
    Some(if is_negative_sign(sign_hex) {
        -value
    } else {
        value
    })
}

/// Parse hex to u128
fn parse_hex_u128(hex: &str) -> Option<u128> {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    u128::from_str_radix(hex, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create PositionUpdated event data array
    #[allow(clippy::too_many_arguments)]
    fn create_position_updated_data(
        token0: &str,
        token1: &str,
        fee: &str,
        tick_spacing: &str,
        extension: &str,
        lower_mag: i64,
        lower_negative: bool,
        upper_mag: i64,
        upper_negative: bool,
        delta_mag: i128,
        delta_negative: bool,
    ) -> Vec<String> {
        vec![
            "0x0".to_string(),                                      // locker (index 0)
            token0.to_string(),                                     // token0 (index 1)
            token1.to_string(),                                     // token1 (index 2)
            fee.to_string(),                                        // fee (index 3)
            tick_spacing.to_string(),                               // tick_spacing (index 4)
            extension.to_string(),                                  // extension (index 5)
            "0x0".to_string(),                                      // salt (index 6)
            format!("0x{:x}", lower_mag),                           // lower_mag (index 7)
            if lower_negative { "0x1" } else { "0x0" }.to_string(), // lower_sign (index 8)
            format!("0x{:x}", upper_mag),                           // upper_mag (index 9)
            if upper_negative { "0x1" } else { "0x0" }.to_string(), // upper_sign (index 10)
            format!("0x{:x}", delta_mag),                           // delta_mag (index 11)
            if delta_negative { "0x1" } else { "0x0" }.to_string(), // delta_sign (index 12)
        ]
    }

    #[test]
    fn test_parse_position_updated_positive_bounds_positive_delta() {
        let token0 = "0x049d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7";
        let token1 = "0x04718f5a0fc34cc1af16a1cdee98ffb20c31f5cd61d6ab07201858f4287c938d";
        let fee = "0x68db8bac710cb4000000000000000";
        let tick_spacing = "0x3e8";
        let extension = "0x0";

        let data = create_position_updated_data(
            token0,
            token1,
            fee,
            tick_spacing,
            extension,
            1000,  // lower_mag
            false, // lower_positive
            2000,  // upper_mag
            false, // upper_positive
            5000,  // delta_mag
            false, // delta_positive
        );

        let keys = vec![POSITION_UPDATED_KEY.to_string()];
        let result = PoolEvent::from_rpc_data(&keys, &data);

        assert!(result.is_some(), "Should parse PositionUpdated event");

        if let Some(PoolEvent::UpdateTick(event)) = result {
            assert_eq!(event.bounds.lower, 1000);
            assert_eq!(event.bounds.upper, 2000);
            assert_eq!(event.delta, 5000);
            assert_eq!(event.pool_id.tick_spacing, 1000); // 0x3e8 = 1000
        } else {
            panic!("Expected UpdateTick event");
        }
    }

    #[test]
    fn test_parse_position_updated_negative_bounds() {
        let data = create_position_updated_data(
            "0x1", "0x2", "0x100", "0x64", "0x0", 500,   // lower_mag
            true,  // lower_negative
            300,   // upper_mag
            true,  // upper_negative
            1000,  // delta_mag
            false, // delta_positive
        );

        let keys = vec![POSITION_UPDATED_KEY.to_string()];
        let result = PoolEvent::from_rpc_data(&keys, &data);

        if let Some(PoolEvent::UpdateTick(event)) = result {
            assert_eq!(event.bounds.lower, -500, "Lower bound should be negative");
            assert_eq!(event.bounds.upper, -300, "Upper bound should be negative");
            assert_eq!(event.delta, 1000);
        } else {
            panic!("Expected UpdateTick event");
        }
    }

    #[test]
    fn test_parse_position_updated_negative_delta() {
        let data = create_position_updated_data(
            "0x1", "0x2", "0x100", "0x64", "0x0", 1000,  // lower_mag
            false, // lower_positive
            2000,  // upper_mag
            false, // upper_positive
            3000,  // delta_mag
            true,  // delta_negative (removing liquidity)
        );

        let keys = vec![POSITION_UPDATED_KEY.to_string()];
        let result = PoolEvent::from_rpc_data(&keys, &data);

        if let Some(PoolEvent::UpdateTick(event)) = result {
            assert_eq!(event.bounds.lower, 1000);
            assert_eq!(event.bounds.upper, 2000);
            assert_eq!(
                event.delta, -3000,
                "Delta should be negative (liquidity removal)"
            );
        } else {
            panic!("Expected UpdateTick event");
        }
    }

    #[test]
    fn test_parse_position_updated_insufficient_data() {
        // Only 10 fields instead of required 13
        let data: Vec<String> = (0..10).map(|i| format!("0x{:x}", i)).collect();

        let keys = vec![POSITION_UPDATED_KEY.to_string()];
        let result = PoolEvent::from_rpc_data(&keys, &data);

        assert!(result.is_none(), "Should return None for insufficient data");
    }

    #[test]
    fn test_parse_position_updated_pool_id_matches() {
        let token0 = "0x049d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7";
        let token1 = "0x04718f5a0fc34cc1af16a1cdee98ffb20c31f5cd61d6ab07201858f4287c938d";
        let fee = "0x68db8bac710cb4000000000000000";
        let tick_spacing = "0x3e8";
        let extension = "0x0";

        let data = create_position_updated_data(
            token0,
            token1,
            fee,
            tick_spacing,
            extension,
            100,
            false,
            200,
            false,
            500,
            false,
        );

        let keys = vec![POSITION_UPDATED_KEY.to_string()];
        let result = PoolEvent::from_rpc_data(&keys, &data);

        if let Some(PoolEvent::UpdateTick(event)) = result {
            // Verify pool ID is constructed correctly
            let expected_pool_id = PoolId::new(token0, token1, fee, tick_spacing, extension);
            assert_eq!(
                event.pool_id.key_hash(),
                expected_pool_id.key_hash(),
                "Pool ID hash should match"
            );
        } else {
            panic!("Expected UpdateTick event");
        }
    }

    #[test]
    fn test_parse_position_updated_normalized_key() {
        // Test that event key comparison works with different hex formats
        let data = create_position_updated_data(
            "0x1", "0x2", "0x100", "0x64", "0x0", 100, false, 200, false, 500, false,
        );

        // Key without leading zeros (as RPC might return)
        let normalized_key = normalize_hex(POSITION_UPDATED_KEY);
        let keys = vec![format!("0x{}", normalized_key)];
        let result = PoolEvent::from_rpc_data(&keys, &data);

        assert!(
            result.is_some(),
            "Should parse event with normalized key format"
        );
    }

    #[test]
    fn test_parse_position_updated_empty_keys() {
        let data = create_position_updated_data(
            "0x1", "0x2", "0x100", "0x64", "0x0", 100, false, 200, false, 500, false,
        );

        let keys: Vec<String> = vec![];
        let result = PoolEvent::from_rpc_data(&keys, &data);

        assert!(result.is_none(), "Should return None for empty keys");
    }

    #[test]
    fn test_parse_position_updated_wrong_key() {
        let data = create_position_updated_data(
            "0x1", "0x2", "0x100", "0x64", "0x0", 100, false, 200, false, 500, false,
        );

        // Use a different event key (not PositionUpdated or Swapped)
        let keys = vec!["0x123456789abcdef".to_string()];
        let result = PoolEvent::from_rpc_data(&keys, &data);

        assert!(result.is_none(), "Should return None for unknown event key");
    }

    #[test]
    fn test_update_tick_event_to_update_tick_conversion() {
        let event = UpdateTickEvent::new(
            "0x1", "0x2", "0x100", "0x64", "0x0", -1000, // lower
            2000,  // upper
            5000,  // delta
        );

        let update = event.to_update_tick();

        assert_eq!(update.bounds.lower, -1000);
        assert_eq!(update.bounds.upper, 2000);
        assert_eq!(update.delta, 5000);
    }

    #[test]
    fn test_parse_position_updated_large_values() {
        // Test with large tick and delta values
        let data = create_position_updated_data(
            "0x1",
            "0x2",
            "0x100",
            "0x64",
            "0x0",
            29_620_000, // lower (realistic large tick)
            false,
            30_041_169, // upper (realistic large tick)
            false,
            1_339_111_227_026_832_113, // delta (large liquidity)
            false,
        );

        let keys = vec![POSITION_UPDATED_KEY.to_string()];
        let result = PoolEvent::from_rpc_data(&keys, &data);

        if let Some(PoolEvent::UpdateTick(event)) = result {
            assert_eq!(event.bounds.lower, 29_620_000);
            assert_eq!(event.bounds.upper, 30_041_169);
            assert_eq!(event.delta, 1_339_111_227_026_832_113);
        } else {
            panic!("Expected UpdateTick event");
        }
    }
}
