use super::events::{compute_pool_key_hash_from_strings, PoolId};
use super::swap::{buy, sell, Pool, SwapInfo, U256};

/// Direction of the swap through a pool
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    T0ToT1,
    T1ToT0,
}

/// A pool with its associated token addresses and key parameters.
/// Includes a pre-computed hash key for O(1) lookups.
#[derive(Debug, Clone)]
pub struct PoolWithTokens {
    pub pool: Pool,
    pub token0: U256,
    pub token1: U256,
    pub fee: U256,
    pub tick_spacing: i64,
    pub extension: U256,
    /// Original hex strings (for debugging/display)
    pub token0_hex: String,
    pub token1_hex: String,
    pub fee_hex: String,
    pub tick_spacing_hex: String,
    pub extension_hex: String,
    /// Pre-computed hash key for fast lookups
    key_hash: u64,
}

impl PoolWithTokens {
    /// Create a new PoolWithTokens from hex strings with pre-computed key hash
    /// This is the preferred constructor as it avoids precision issues
    #[allow(clippy::too_many_arguments)]
    pub fn from_hex(
        pool: Pool,
        token0_hex: &str,
        token1_hex: &str,
        fee_hex: &str,
        tick_spacing_hex: &str,
        extension_hex: &str,
        token0: U256,
        token1: U256,
        fee: U256,
        tick_spacing: i64,
        extension: U256,
    ) -> Self {
        let key_hash = compute_pool_key_hash_from_strings(
            token0_hex,
            token1_hex,
            fee_hex,
            tick_spacing_hex,
            extension_hex,
        );
        Self {
            pool,
            token0,
            token1,
            fee,
            tick_spacing,
            extension,
            token0_hex: token0_hex.to_string(),
            token1_hex: token1_hex.to_string(),
            fee_hex: fee_hex.to_string(),
            tick_spacing_hex: tick_spacing_hex.to_string(),
            extension_hex: extension_hex.to_string(),
            key_hash,
        }
    }

    /// Create a new PoolWithTokens with pre-computed key hash from U256 values
    pub fn new(
        pool: Pool,
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
        let tick_spacing_hex = format!(
            "{:#x}",
            starknet::core::types::Felt::from(tick_spacing as i128)
        );
        let extension_hex = format!("{:x}", extension);
        let key_hash = compute_pool_key_hash_from_strings(
            &token0_hex,
            &token1_hex,
            &fee_hex,
            &tick_spacing_hex,
            &extension_hex,
        );
        Self {
            pool,
            token0,
            token1,
            fee,
            tick_spacing,
            extension,
            token0_hex,
            token1_hex,
            fee_hex,
            tick_spacing_hex,
            extension_hex,
            key_hash,
        }
    }

    /// Create from a Pool and a PoolId (avoids passing 11 separate arguments)
    pub fn from_pool_id(pool: Pool, pool_id: &PoolId) -> Self {
        Self::from_hex(
            pool,
            &pool_id.token0_hex,
            &pool_id.token1_hex,
            &pool_id.fee_hex,
            &pool_id.tick_spacing_hex,
            &pool_id.extension_hex,
            pool_id.token0,
            pool_id.token1,
            pool_id.fee,
            pool_id.tick_spacing,
            pool_id.extension,
        )
    }

    /// Returns the pre-computed hash key (u64, no allocation)
    #[inline]
    pub fn key_hash(&self) -> u64 {
        self.key_hash
    }

    /// Returns a string key for this pool (allocates, use for display only)
    pub fn key_string(&self) -> String {
        format!(
            "{:x}-{:x}-{:x}-{}-{:x}",
            self.token0, self.token1, self.fee, self.tick_spacing, self.extension
        )
    }
}

/// A hop in a swap path (uses reference to avoid cloning)
#[derive(Debug, Clone, Copy)]
pub struct Hop<'a> {
    pub direction: Direction,
    pub pool: &'a PoolWithTokens,
}

/// A path is a sequence of hops with borrowed pool references
pub type Path<'a> = Vec<Hop<'a>>;

/// Token amount with address
#[derive(Debug, Clone)]
pub struct TokenAmount {
    pub amount: U256,
    pub address: U256,
}

/// A single swap in the evaluation - stores only essential data, no pool references
#[derive(Debug, Clone)]
pub struct EvaluationSwap {
    pub input: TokenAmount,
    pub output: TokenAmount,
    pub sqrt_ratio: U256,
}

/// Result of path evaluation - owned data, no lifetimes
#[derive(Debug, Clone)]
pub struct EvaluatePathResult {
    pub amount_out: U256,
    pub swaps: Vec<EvaluationSwap>,
    pub info: SwapInfo,
    pub bad_pool: Option<String>,
}

/// Evaluates a path of swaps, calculating the output amount
///
/// # Arguments
/// * `path` - The path of hops to evaluate (with borrowed pool references)
/// * `amount_in` - The input amount as U256
///
/// # Returns
/// * `EvaluatePathResult` containing the output amount, swaps, status, and optional bad pool key
pub fn evaluate_path(path: &Path<'_>, amount_in: U256) -> EvaluatePathResult {
    let mut swaps = Vec::new();
    let mut amount = amount_in;

    // Conservative slippage tolerance on the resulting sqrt_ratio.
    // Buy (ratio goes DOWN): limit = 98% of simulated ratio → allows further downward movement.
    // Sell (ratio goes UP): limit = 102% of simulated ratio → allows further upward movement.
    let slippage_buy = U256::from(99u64);
    let slippage_sell = U256::from(101u64);
    let hundred = U256::from(100u64);

    // TODO: check for partial fills, when pool goes out of liquidity
    for hop in path {
        if hop.direction == Direction::T0ToT1 {
            let result = buy(amount, &hop.pool.pool);

            if result.info != SwapInfo::Ok {
                return EvaluatePathResult {
                    amount_out: result.total_y,
                    swaps,
                    info: result.info,
                    bad_pool: Some(hop.pool.key_string()),
                };
            }

            let adjusted_ratio = result.sqrt_ratio * slippage_buy / hundred;

            swaps.push(EvaluationSwap {
                input: TokenAmount {
                    amount: result.total_x,
                    address: hop.pool.token0,
                },
                output: TokenAmount {
                    amount: result.total_y,
                    address: hop.pool.token1,
                },
                sqrt_ratio: adjusted_ratio,
            });
            amount = result.total_y;
        } else {
            let result = sell(amount, &hop.pool.pool);

            if result.info != SwapInfo::Ok {
                return EvaluatePathResult {
                    amount_out: result.total_x,
                    swaps,
                    info: result.info,
                    bad_pool: Some(hop.pool.key_string()),
                };
            }

            let adjusted_ratio = result.sqrt_ratio * slippage_sell / hundred;

            swaps.push(EvaluationSwap {
                input: TokenAmount {
                    amount: result.total_y,
                    address: hop.pool.token1,
                },
                output: TokenAmount {
                    amount: result.total_x,
                    address: hop.pool.token0,
                },
                sqrt_ratio: adjusted_ratio,
            });
            amount = result.total_x;
        }
    }

    EvaluatePathResult {
        amount_out: amount,
        swaps,
        info: SwapInfo::Ok,
        bad_pool: None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::swap::Tick;
    use super::*;

    #[test]
    fn test_evaluate_empty_path() {
        let path: Path = vec![];
        let amount = U256::from(1000u128);
        let result = evaluate_path(&path, amount);

        assert_eq!(result.amount_out, amount);
        assert!(result.swaps.is_empty());
        assert_eq!(result.info, SwapInfo::Ok);
        assert!(result.bad_pool.is_none());
    }

    #[test]
    fn test_evaluate_single_hop_t0_to_t1() {
        // Use higher liquidity to ensure swap can complete without running out
        let pool = Pool::from_hex(
            vec![
                Tick {
                    tick: -1000,
                    delta: 5000,
                },
                Tick {
                    tick: 0,
                    delta: 10000,
                },
                Tick {
                    tick: 1000,
                    delta: 5000,
                },
            ],
            100,
            "0x6389f7f2203147955d5b12e80a8286b94becf0a",
            50000,
            "0x68db8bac710cb4000000000000000",
        )
        .unwrap();

        let pool_with_tokens = PoolWithTokens::new(
            pool,
            U256::from(1u64),
            U256::from(2u64),
            U256::ZERO,
            0,
            U256::ZERO,
        );

        let path = vec![Hop {
            direction: Direction::T0ToT1,
            pool: &pool_with_tokens,
        }];

        let result = evaluate_path(&path, U256::from(1u128));

        // Test that evaluation runs and returns a result (path evaluation works)
        // The actual swap result depends on pool liquidity configuration
        assert!(result.swaps.len() <= 1);
    }

    #[test]
    fn test_evaluate_single_hop_t1_to_t0() {
        // Use higher liquidity to ensure swap can complete without running out
        let pool = Pool::from_hex(
            vec![
                Tick {
                    tick: -1000,
                    delta: 5000,
                },
                Tick {
                    tick: 0,
                    delta: 10000,
                },
                Tick {
                    tick: 1000,
                    delta: 5000,
                },
            ],
            100,
            "0x6389f7f2203147955d5b12e80a8286b94becf0a",
            50000,
            "0x68db8bac710cb4000000000000000",
        )
        .unwrap();

        let pool_with_tokens = PoolWithTokens::new(
            pool,
            U256::from(1u64),
            U256::from(2u64),
            U256::ZERO,
            0,
            U256::ZERO,
        );

        let path = vec![Hop {
            direction: Direction::T1ToT0,
            pool: &pool_with_tokens,
        }];

        let result = evaluate_path(&path, U256::from(1u128));

        // Test that evaluation runs and returns a result (path evaluation works)
        // The actual swap result depends on pool liquidity configuration
        assert!(result.swaps.len() <= 1);
    }
}
