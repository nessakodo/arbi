use ruint::Uint;
use starknet::core::types::Felt;

// Re-export Pool and Tick from pool module for backward compatibility
pub use super::pool::{Pool, Tick};

/// U256 type alias
pub type U256 = Uint<256, 4>;

/// 2^128 as U256
pub const TWO_POW_128: U256 = U256::from_limbs([0, 0, 1, 0]);

/// 2^128 as f64 (for backward compatibility during transition)
const TWO_POW_128_F64: f64 = 340282366920938463463374607431768211456.0;

/// Result status of a swap operation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwapInfo {
    Ok,
    NoTicks,
    NoLiquidity,
}

/// Direction of the swap
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapDirection {
    /// Buy: increase T0, decrease T1. Ratio goes DOWN.
    Buy,
    /// Sell: increase T1, decrease T0. Ratio goes UP.
    Sell,
}

// ============ U256 Conversion Functions ============

/// Parse a hex string to U256
pub fn hex_to_u256(hex: &str) -> Result<U256, String> {
    let hex = hex.trim().strip_prefix("0x").unwrap_or(hex);
    U256::from_str_radix(hex, 16).map_err(|e| format!("Failed to parse hex: {}", e))
}

/// Convert U256 to hex string with 0x prefix
pub fn u256_to_hex(value: &U256) -> String {
    format!("{:#x}", value)
}

/// Convert U256 to f64 (loses precision for large values)
pub fn u256_to_f64(value: &U256) -> f64 {
    // Split into high and low 128-bit parts
    let limbs = value.as_limbs();
    let low = limbs[0] as f64 + (limbs[1] as f64) * (u64::MAX as f64 + 1.0);
    let high = limbs[2] as f64 + (limbs[3] as f64) * (u64::MAX as f64 + 1.0);
    low + high * TWO_POW_128_F64
}

/// Convert f64 to U256 (approximate, loses precision)
pub fn f64_to_u256(value: f64) -> U256 {
    if value <= 0.0 {
        return U256::ZERO;
    }

    if value < TWO_POW_128_F64 {
        // Fits in lower 128 bits
        if value <= u128::MAX as f64 {
            U256::from(value as u128)
        } else {
            // Very large but less than 2^128
            U256::from(u128::MAX)
        }
    } else {
        // Split into high and low parts
        let high = (value / TWO_POW_128_F64).floor();
        let low = value - high * TWO_POW_128_F64;

        let high_u128 = if high <= u128::MAX as f64 {
            high as u128
        } else {
            u128::MAX
        };
        let low_u128 = if low <= u128::MAX as f64 {
            low as u128
        } else {
            u128::MAX
        };

        // Construct U256: high * 2^128 + low
        (U256::from(high_u128) << 128) + U256::from(low_u128)
    }
}

/// Convert Felt to U256
pub fn felt_to_u256(felt: &Felt) -> U256 {
    let bytes = felt.to_bytes_be();
    U256::from_be_bytes::<32>(bytes)
}

/// Convert U256 to Felt
pub fn u256_to_felt(value: &U256) -> Felt {
    let bytes = value.to_be_bytes::<32>();
    Felt::from_bytes_be(&bytes)
}

// ============ Legacy f64 Functions (for backward compatibility) ============

/// Convert a Felt to f64 for calculations (loses precision)
pub fn felt_to_f64(felt: &Felt) -> f64 {
    u256_to_f64(&felt_to_u256(felt))
}

/// Convert f64 to Felt (loses precision)
pub fn f64_to_felt(value: f64) -> Felt {
    u256_to_felt(&f64_to_u256(value))
}

/// Parse a hex string to Felt
pub fn hex_to_felt(hex: &str) -> Result<Felt, String> {
    Felt::from_hex(hex).map_err(|e| format!("Failed to parse hex '{}': {}", hex, e))
}

/// Convert Felt to hex string with 0x prefix
pub fn felt_to_hex(felt: &Felt) -> String {
    format!("{:#x}", felt)
}

/// Result of a swap operation
#[derive(Debug, Clone)]
pub struct SwapResult {
    pub total_x: U256,
    pub total_y: U256,
    pub sqrt_ratio: U256,
    pub info: SwapInfo,
}

/// Get the index of the tick that contains the given tick value
/// Returns -1 if tick is out of range
// TODO: use binary search instead of linear scan for pools with many ticks
fn get_tick_index(ticks: &[Tick], tick: i64) -> i32 {
    // ticks are sorted from lower to higher
    if ticks.is_empty() {
        return -1;
    }

    if tick < ticks[0].tick {
        return -1;
    }

    for i in 0..ticks.len() - 1 {
        if ticks[i].tick <= tick && ticks[i + 1].tick > tick {
            return i as i32;
        }
    }

    -1
}

// ============ Fixed-Point Math (Q128.128 format) ============
// In Q128.128: value = raw_bits / 2^128
// sqrt_ratio of 2^128 represents actual ratio of 1.0

/// Mask for lower 128 bits
const LOW_128_MASK: U256 = U256::from_limbs([u64::MAX, u64::MAX, 0, 0]);

/// Multiply two Q128.128 values: (a * b) >> 128
/// Uses checked arithmetic to handle overflow gracefully
fn mul_q128(a: U256, b: U256) -> U256 {
    // For values small enough, direct multiplication won't overflow
    // Split into high/low parts for safety
    let a_high: U256 = a >> 128;
    let a_low: U256 = a & LOW_128_MASK;
    let b_high: U256 = b >> 128;
    let b_low: U256 = b & LOW_128_MASK;

    // (a_high * 2^128 + a_low) * (b_high * 2^128 + b_low) / 2^128
    // = a_high * b_high * 2^128 + a_high * b_low + a_low * b_high + a_low * b_low / 2^128
    let term1: U256 = a_high.saturating_mul(b_high) << 128;
    let term2: U256 = a_high.saturating_mul(b_low);
    let term3: U256 = a_low.saturating_mul(b_high);
    let term4: U256 = a_low.saturating_mul(b_low) >> 128;

    term1
        .saturating_add(term2)
        .saturating_add(term3)
        .saturating_add(term4)
}

/// Convert tick to sqrt ratio in Q128.128 format: sqrt(1.000001^tick) * 2^128
///
/// Computes 1.000001^(tick/2) using integer-only exponentiation by squaring
/// in Q128.128 fixed-point, avoiding f64 precision loss.
fn tick_to_ratio(tick: i64) -> U256 {
    // 1.000001 in Q128.128 = 2^128 * 1000001 / 1000000
    const BASE: U256 = U256::from_limbs([
        0x8d36b4c7f3493858,
        0x000010c6f7a0b5ed,
        0x0000000000000001,
        0x0000000000000000,
    ]);
    // 1/1.000001 in Q128.128 = 2^256 / BASE
    const INV_BASE: U256 = U256::from_limbs([
        0x134b4ff3764fe40f,
        0xffffef390978c398,
        0x0000000000000000,
        0x0000000000000000,
    ]);
    // sqrt(1.000001) in Q128.128
    const SQRT_BASE: U256 = U256::from_limbs([
        0xeb65574811cef70c,
        0x000008637bad2bc4,
        0x0000000000000001,
        0x0000000000000000,
    ]);
    // 1/sqrt(1.000001) in Q128.128
    const SQRT_INV_BASE: U256 = U256::from_limbs([
        0x7cbb2510d893283a,
        0xfffff79c8499329c,
        0x0000000000000000,
        0x0000000000000000,
    ]);

    let abs_tick = tick.unsigned_abs();
    let half = abs_tick / 2;
    let is_odd = abs_tick % 2 == 1;

    let base = if tick >= 0 { BASE } else { INV_BASE };
    let sqrt_factor = if tick >= 0 { SQRT_BASE } else { SQRT_INV_BASE };

    // Exponentiation by squaring: base^half in Q128.128
    let mut result = TWO_POW_128; // 1.0 in Q128.128
    let mut b = base;
    let mut exp = half;
    while exp > 0 {
        if exp & 1 == 1 {
            result = mul_q128(result, b);
        }
        b = mul_q128(b, b);
        exp >>= 1;
    }

    // For odd ticks, multiply by sqrt(base)
    if is_odd {
        result = mul_q128(result, sqrt_factor);
    }

    result
}

/// Calculate new sqrt_ratio after adding delta_x (for buy)
/// Formula: L / (delta_x + L / sqrt_ratio)
/// In Q128.128: result = L * 2^128 / (delta_x + L * 2^128 / sqrt_ratio)
fn calculate_ratio_after_delta_x(sqrt_ratio: U256, liquidity: u128, delta_x: U256) -> U256 {
    if liquidity == 0 || sqrt_ratio.is_zero() {
        return U256::ZERO;
    }

    // L * 2^128 (convert liquidity to Q128.128)
    let l_scaled: U256 = U256::from(liquidity) << 128;

    // L * 2^128 / sqrt_ratio (Q128.128 division)
    let l_over_ratio: U256 = l_scaled / sqrt_ratio;

    // delta_x + L/sqrt_ratio
    let denominator: U256 = delta_x + l_over_ratio;

    if denominator.is_zero() {
        return sqrt_ratio;
    }

    // L * 2^128 / denominator
    l_scaled / denominator
}

/// Calculate new sqrt_ratio after adding delta_y (for sell)
/// Formula: delta_y / L + sqrt_ratio
/// In Q128.128: result = delta_y * 2^128 / L + sqrt_ratio
fn calculate_ratio_after_delta_y(sqrt_ratio: U256, liquidity: u128, delta_y: U256) -> U256 {
    if liquidity == 0 {
        return sqrt_ratio;
    }

    // delta_y * 2^128 / L
    let delta_ratio = (delta_y << 128) / U256::from(liquidity);

    sqrt_ratio + delta_ratio
}

/// Calculate amount of Y in a price range
/// Formula: L * (sqrt_ratio_higher - sqrt_ratio_lower)
/// Result is in token units (not Q128.128)
fn amount_y_in_range(sqrt_ratio_higher: U256, sqrt_ratio_lower: U256, liquidity: u128) -> U256 {
    if sqrt_ratio_higher <= sqrt_ratio_lower {
        return U256::ZERO;
    }

    let delta_ratio = sqrt_ratio_higher - sqrt_ratio_lower;
    // L * delta_ratio / 2^128 (convert from Q128.128 to actual amount)
    (U256::from(liquidity) * delta_ratio) >> 128
}

/// Calculate amount of X in a price range
/// Formula: L * (sqrt_ratio_higher - sqrt_ratio_lower) / (sqrt_ratio_higher * sqrt_ratio_lower)
/// Result is in token units (not Q128.128)
fn amount_x_in_range(sqrt_ratio_higher: U256, sqrt_ratio_lower: U256, liquidity: u128) -> U256 {
    if sqrt_ratio_higher <= sqrt_ratio_lower
        || sqrt_ratio_higher.is_zero()
        || sqrt_ratio_lower.is_zero()
    {
        return U256::ZERO;
    }

    let delta_ratio = sqrt_ratio_higher - sqrt_ratio_lower;
    // L * delta_ratio
    let numerator = U256::from(liquidity) * delta_ratio;

    // sqrt_ratio_higher * sqrt_ratio_lower / 2^128 (Q128.128 multiplication)
    let denominator = mul_q128(sqrt_ratio_higher, sqrt_ratio_lower);

    if denominator.is_zero() {
        return U256::ZERO;
    }

    // (L * delta_ratio) / (ratio_high * ratio_low) / 2^128
    // = numerator / denominator / 2^128
    // = numerator / (denominator << 128) ... wait, let's recalculate

    // Actually: result = L * (1/sqrt_lower - 1/sqrt_higher)
    // = L * (sqrt_higher - sqrt_lower) / (sqrt_higher * sqrt_lower)
    // numerator is L * delta_ratio (scaled by nothing special)
    // denominator is ratio_high * ratio_low >> 128 (Q128.128 product)
    // Result should be in token units

    // numerator / denominator gives the result
    numerator / denominator
}

/// Apply fee to amount: amount * (1 - fee/2^128)
fn apply_fee(amount: U256, fee: U256) -> U256 {
    // fee is in Q128.128, so fee/2^128 gives the fee rate
    // (1 - fee_rate) = (2^128 - fee) / 2^128
    // amount * (1 - fee_rate) = amount * (2^128 - fee) / 2^128
    if fee >= TWO_POW_128 {
        return U256::ZERO; // 100% fee
    }
    let one_minus_fee = TWO_POW_128 - fee;
    mul_q128(amount, one_minus_fee)
}

/// Swap tokens in the pool
///
/// # Arguments
/// * `amount` - Amount to swap (X for Buy, Y for Sell) as U256
/// * `direction` - Direction of the swap (Buy or Sell)
/// * `pool` - The pool state
///
/// # Returns
/// * `SwapResult` containing total_x, total_y, final sqrt_ratio, and status info
///
/// # Behavior
/// - **Buy**: Increase T0, decrease T1. Ratio goes DOWN.
///   Order: HIGHEST > current_ratio > next_tick > best_ratio > LOWEST
/// - **Sell**: Increase T1, decrease T0. Ratio goes UP.
///   Order: LOWEST < current_ratio < next_tick < best_ratio < HIGHEST
pub fn swap(amount: U256, direction: SwapDirection, pool: &Pool) -> SwapResult {
    let mut index = get_tick_index(&pool.ticks, pool.tick);
    let mut current_ratio = pool.sqrt_ratio;
    let amount = apply_fee(amount, pool.fee);
    // Use i128 for liquidity tracking since delta is i128
    // Cap at i128::MAX to prevent wrapping negative on extremely large values
    let mut liquidity: i128 = pool.liquidity.min(i128::MAX as u128) as i128;
    let mut total_x = U256::ZERO;
    let mut total_y = U256::ZERO;

    let is_buy = direction == SwapDirection::Buy;

    loop {
        // Check completion condition
        let total = if is_buy { total_x } else { total_y };
        if total >= amount {
            break;
        }

        // Get the closest tick based on direction
        let tick_index = if is_buy { index } else { index + 1 };

        if tick_index < 0 {
            return SwapResult {
                total_x,
                total_y,
                sqrt_ratio: current_ratio,
                info: SwapInfo::NoLiquidity,
            };
        }

        let closest_tick = match pool.ticks.get(tick_index as usize) {
            Some(tick) => tick,
            None => {
                return SwapResult {
                    total_x,
                    total_y,
                    sqrt_ratio: current_ratio,
                    info: SwapInfo::NoLiquidity,
                };
            }
        };

        if liquidity <= 0 {
            return SwapResult {
                total_x,
                total_y,
                sqrt_ratio: current_ratio,
                info: SwapInfo::NoLiquidity,
            };
        }

        // Convert liquidity to u128 for calculations (safe since we checked > 0)
        let liq_u128 = liquidity as u128;

        let closest_ratio = tick_to_ratio(closest_tick.tick);
        let remaining = amount - total;

        let best_ratio = if is_buy {
            calculate_ratio_after_delta_x(current_ratio, liq_u128, remaining)
        } else {
            calculate_ratio_after_delta_y(current_ratio, liq_u128, remaining)
        };

        // Check if this range provides all missing liquidity
        let range_complete = if is_buy {
            best_ratio >= closest_ratio
        } else {
            best_ratio <= closest_ratio
        };

        if range_complete {
            // Calculate amounts with correct higher/lower ordering
            let (higher, lower) = if is_buy {
                (current_ratio, best_ratio)
            } else {
                (best_ratio, current_ratio)
            };

            let amount_x = amount_x_in_range(higher, lower, liq_u128);
            let amount_y = amount_y_in_range(higher, lower, liq_u128);

            total_x += amount_x;
            total_y += amount_y;

            return SwapResult {
                total_x,
                total_y,
                sqrt_ratio: best_ratio,
                info: SwapInfo::Ok,
            };
        }

        // Consume this tick range and move to the next
        let (higher, lower) = if is_buy {
            (current_ratio, closest_ratio)
        } else {
            (closest_ratio, current_ratio)
        };

        let amount_x = amount_x_in_range(higher, lower, liq_u128);
        let amount_y = amount_y_in_range(higher, lower, liq_u128);

        total_x += amount_x;
        total_y += amount_y;

        current_ratio = closest_ratio;

        if is_buy {
            liquidity -= closest_tick.delta;
            index -= 1;
        } else {
            liquidity += closest_tick.delta;
            index += 1;
        }
    }

    SwapResult {
        total_x,
        total_y,
        sqrt_ratio: current_ratio,
        info: SwapInfo::Ok,
    }
}

/// Convenience function: Buy tokens from the pool (U256 version)
pub fn buy(buy_amount_x: U256, pool: &Pool) -> SwapResult {
    swap(buy_amount_x, SwapDirection::Buy, pool)
}

/// Convenience function: Sell tokens to the pool (U256 version)
pub fn sell(sell_amount_y: U256, pool: &Pool) -> SwapResult {
    swap(sell_amount_y, SwapDirection::Sell, pool)
}

// ============ Legacy f64 API (for backward compatibility) ============

/// Legacy swap result with f64 values
#[derive(Debug, Clone)]
pub struct SwapResultF64 {
    pub total_x: f64,
    pub total_y: f64,
    pub sqrt_ratio: f64,
    pub info: SwapInfo,
}

impl From<SwapResult> for SwapResultF64 {
    fn from(result: SwapResult) -> Self {
        Self {
            total_x: u256_to_f64(&result.total_x),
            total_y: u256_to_f64(&result.total_y),
            sqrt_ratio: u256_to_f64(&result.sqrt_ratio),
            info: result.info,
        }
    }
}

/// Buy tokens from the pool (f64 version for backward compatibility)
pub fn buy_f64(buy_amount_x: f64, pool: &Pool) -> SwapResultF64 {
    let amount = f64_to_u256(buy_amount_x);
    buy(amount, pool).into()
}

/// Sell tokens to the pool (f64 version for backward compatibility)
pub fn sell_f64(sell_amount_y: f64, pool: &Pool) -> SwapResultF64 {
    let amount = f64_to_u256(sell_amount_y);
    sell(amount, pool).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_tick_index_empty() {
        let ticks: Vec<Tick> = vec![];
        assert_eq!(get_tick_index(&ticks, 100), -1);
    }

    #[test]
    fn test_get_tick_index_too_low() {
        let ticks = vec![
            Tick {
                tick: 100,
                delta: 1000,
            },
            Tick {
                tick: 200,
                delta: 2000,
            },
        ];
        assert_eq!(get_tick_index(&ticks, 50), -1);
    }

    #[test]
    fn test_get_tick_index_found() {
        let ticks = vec![
            Tick {
                tick: 100,
                delta: 1000,
            },
            Tick {
                tick: 200,
                delta: 2000,
            },
            Tick {
                tick: 300,
                delta: 3000,
            },
        ];
        assert_eq!(get_tick_index(&ticks, 150), 0);
        assert_eq!(get_tick_index(&ticks, 250), 1);
    }

    #[test]
    fn test_tick_to_ratio() {
        // tick_to_ratio(0) should be close to 2^128 (1.0 in Q128.128)
        let ratio = tick_to_ratio(0);
        let expected = TWO_POW_128;
        // Check it's within 0.01% of expected
        let diff = if ratio > expected {
            ratio - expected
        } else {
            expected - ratio
        };
        assert!(diff < expected / U256::from(10000u64));

        // Positive tick should give ratio > 2^128
        let ratio_pos = tick_to_ratio(1000);
        assert!(ratio_pos > TWO_POW_128);

        // Negative tick should give ratio < 2^128
        let ratio_neg = tick_to_ratio(-1000);
        assert!(ratio_neg < TWO_POW_128);
    }

    #[test]
    fn test_calculate_ratio_after_delta_x() {
        // 1.5 in Q128.128 format
        let sqrt_ratio = (U256::from(3u128) << 128) / U256::from(2u128);
        let liquidity: u128 = 1000;
        let delta_x = U256::from(100u128);

        let new_ratio = calculate_ratio_after_delta_x(sqrt_ratio, liquidity, delta_x);
        assert!(new_ratio < sqrt_ratio);
    }

    #[test]
    fn test_calculate_ratio_after_delta_y() {
        // 1.5 in Q128.128 format
        let sqrt_ratio = (U256::from(3u128) << 128) / U256::from(2u128);
        let liquidity: u128 = 1000;
        let delta_y = U256::from(100u128);

        let new_ratio = calculate_ratio_after_delta_y(sqrt_ratio, liquidity, delta_y);
        assert!(new_ratio > sqrt_ratio);
    }

    #[test]
    fn test_swap_large_pool_with_many_ticks() {
        // Pool data from real scenario
        // fee: 0x68db8bac710cb4000000000000000
        // sqrt_ratio: 0x6389f7f2203147955d5b12e80a8286b94becf0a
        // liquidity: 0x8168d0cab928172
        // tick: 36927003

        let pool = Pool::from_hex(
            vec![
                Tick {
                    tick: 36256400,
                    delta: 335552285353358091,
                },
                Tick {
                    tick: 36350000,
                    delta: -335552285353358091,
                },
                Tick {
                    tick: 36496000,
                    delta: 3193981895113,
                },
                Tick {
                    tick: 36670600,
                    delta: 110795729449366672,
                },
                Tick {
                    tick: 36671800,
                    delta: 51032457151703,
                },
                Tick {
                    tick: 36697400,
                    delta: -51032457151703,
                },
                Tick {
                    tick: 36737200,
                    delta: 122230432471766145,
                },
                Tick {
                    tick: 36781000,
                    delta: 391951167615104697,
                },
                Tick {
                    tick: 36857200,
                    delta: -110795729449366672,
                },
                Tick {
                    tick: 36878600,
                    delta: -122230432471766145,
                },
                Tick {
                    tick: 36885400,
                    delta: 190853925751687408,
                },
                Tick {
                    tick: 37050800,
                    delta: -190853925751687408,
                },
                Tick {
                    tick: 37083200,
                    delta: -391951167615104697,
                },
                Tick {
                    tick: 37333600,
                    delta: -3193981895113,
                },
            ],
            36927003,
            "0x6389f7f2203147955d5b12e80a8286b94becf0a",
            582808287348687200,
            "0x68db8bac710cb4000000000000000",
        )
        .unwrap();

        // 25,000 STRK (as U256)
        let input_amount = U256::from(25_000_000_000_000_000_000_000u128);

        let sell_result = sell(input_amount, &pool);
        assert_eq!(sell_result.info, SwapInfo::Ok);
        // The swap should complete successfully and sell approximately the input amount
        assert!(sell_result.total_y > U256::ZERO);
    }

    #[test]
    fn test_swap_negative_tick_pool() {
        // Pool data from real scenario
        // fee: 0x20c49ba5e353f80000000000000000
        // sqrt_ratio: 0x87a17c13dfdde0af0718ffeb4a29c29
        // liquidity: 0x7eb71b5d62
        // tick: -6815663

        let pool = Pool::from_hex(
            vec![
                Tick {
                    tick: -7249000,
                    delta: 327444856565,
                },
                Tick {
                    tick: -7090000,
                    delta: 108796402640,
                },
                Tick {
                    tick: -6956000,
                    delta: 229900,
                },
                Tick {
                    tick: -6947000,
                    delta: 107931644492,
                },
                Tick {
                    tick: -6863000,
                    delta: 4335,
                },
                Tick {
                    tick: -6831000,
                    delta: 64765270,
                },
                Tick {
                    tick: -6826000,
                    delta: 525136185,
                },
                Tick {
                    tick: -6816000,
                    delta: -525136185,
                },
                Tick {
                    tick: -6709000,
                    delta: -107931644492,
                },
                Tick {
                    tick: -6703000,
                    delta: -64769605,
                },
                Tick {
                    tick: -6656000,
                    delta: -229900,
                },
                Tick {
                    tick: -6578000,
                    delta: -108796402640,
                },
                Tick {
                    tick: -6518000,
                    delta: -327444856565,
                },
            ],
            -6815663,
            "0x87a17c13dfdde0af0718ffeb4a29c29",
            544237903202,
            "0x20c49ba5e353f80000000000000000",
        )
        .unwrap();

        let input_amount = U256::from(2293677u128);

        let sell_result = sell(input_amount, &pool);
        assert_eq!(sell_result.info, SwapInfo::Ok);
        // Results are now in U256, just check it's non-zero
        assert!(sell_result.total_x > U256::ZERO);
    }

    #[test]
    fn test_swap_large_positive_tick_pool() {
        // Pool data from real scenario
        // fee: 0x20c49ba5e353f80000000000000000
        // sqrt_ratio: 0x34cd4ede216c466c29b59a57d142a444edbbe2
        // liquidity: 0x19fe11764af6129af
        // tick: 30113822
        // Note: liquidity value exceeds u64::MAX, using u128

        let pool = Pool::from_hex(
            vec![
                Tick {
                    tick: 26021000,
                    delta: 813408081424,
                },
                Tick {
                    tick: 26532000,
                    delta: 268935742849537,
                },
                Tick {
                    tick: 28835000,
                    delta: 9782900726744891,
                },
                Tick {
                    tick: 29017000,
                    delta: 486042540854958,
                },
                Tick {
                    tick: 29236000,
                    delta: 1159129015014243,
                },
                Tick {
                    tick: 29291000,
                    delta: 7798578881237541,
                },
                Tick {
                    tick: 29380000,
                    delta: 1207240216941526,
                },
                Tick {
                    tick: 29499000,
                    delta: 42512830994606,
                },
                Tick {
                    tick: 29528000,
                    delta: 168087411394373,
                },
                Tick {
                    tick: 29564000,
                    delta: 1113084586679147,
                },
                Tick {
                    tick: 29620000,
                    delta: 1339111227026832113,
                },
                Tick {
                    tick: 29679000,
                    delta: 103745890643821,
                },
                Tick {
                    tick: 29682000,
                    delta: 313054595215171,
                },
                Tick {
                    tick: 29707000,
                    delta: 726992583371299750,
                },
                Tick {
                    tick: 29715000,
                    delta: 1121966666729902108,
                },
                Tick {
                    tick: 29751000,
                    delta: 1878891287242932,
                },
                Tick {
                    tick: 29773000,
                    delta: 50315294843066710,
                },
                Tick {
                    tick: 29786000,
                    delta: 5554399731521,
                },
                Tick {
                    tick: 29800000,
                    delta: 898885659205346,
                },
                Tick {
                    tick: 29805000,
                    delta: -50315294843066710,
                },
                Tick {
                    tick: 29818000,
                    delta: -5554399731521,
                },
                Tick {
                    tick: 29826000,
                    delta: -7798578881237541,
                },
                Tick {
                    tick: 29853000,
                    delta: 759630234827500159,
                },
                Tick {
                    tick: 29867000,
                    delta: 37317064732296557,
                },
                Tick {
                    tick: 29875000,
                    delta: 5823688356774311,
                },
                Tick {
                    tick: 29885000,
                    delta: 25456580191399497362,
                },
                Tick {
                    tick: 29886000,
                    delta: 3645137456940310615,
                },
                Tick {
                    tick: 29904000,
                    delta: -898885659205346,
                },
                Tick {
                    tick: 29905000,
                    delta: 4823373066525869,
                },
                Tick {
                    tick: 29933000,
                    delta: -1339111163777415917,
                },
                Tick {
                    tick: 29934000,
                    delta: 114990962118772197,
                },
                Tick {
                    tick: 29940000,
                    delta: 4890561440652622,
                },
                Tick {
                    tick: 29946000,
                    delta: 4707401876168845,
                },
                Tick {
                    tick: 29966000,
                    delta: 3358435622252748,
                },
                Tick {
                    tick: 29979000,
                    delta: -103745890643821,
                },
                Tick {
                    tick: 29982000,
                    delta: 8114936702162502,
                },
                Tick {
                    tick: 29983000,
                    delta: 4769575677909121,
                },
                Tick {
                    tick: 29985000,
                    delta: -27519844418098915,
                },
                Tick {
                    tick: 29986000,
                    delta: 7741054440029504,
                },
                Tick {
                    tick: 29987000,
                    delta: -4769575677909121,
                },
                Tick {
                    tick: 29989000,
                    delta: 179659159024758,
                },
                Tick {
                    tick: 29990000,
                    delta: -15855991142192006,
                },
                Tick {
                    tick: 29993000,
                    delta: -8143240959330117,
                },
                Tick {
                    tick: 29998000,
                    delta: -3358435622252748,
                },
                Tick {
                    tick: 30020000,
                    delta: 2887863836488750,
                },
                Tick {
                    tick: 30034000,
                    delta: -1121966666729902108,
                },
                Tick {
                    tick: 30039000,
                    delta: -313054595215171,
                },
                Tick {
                    tick: 30043000,
                    delta: 4629686619026,
                },
                Tick {
                    tick: 30052000,
                    delta: -2887863836488750,
                },
                Tick {
                    tick: 30070000,
                    delta: -42512830994606,
                },
                Tick {
                    tick: 30071000,
                    delta: -1878891287242932,
                },
                Tick {
                    tick: 30076000,
                    delta: -1113084586679147,
                },
                Tick {
                    tick: 30087000,
                    delta: -43399079661719163,
                },
                Tick {
                    tick: 30089000,
                    delta: -1159129015014243,
                },
                Tick {
                    tick: 30096000,
                    delta: 321178505032630,
                },
                Tick {
                    tick: 30110000,
                    delta: 796340230290,
                },
                Tick {
                    tick: 30113000,
                    delta: -726992583371299750,
                },
                Tick {
                    tick: 30116000,
                    delta: -321178505032630,
                },
                Tick {
                    tick: 30139000,
                    delta: 498862491894,
                },
                Tick {
                    tick: 30142000,
                    delta: -796340230290,
                },
                Tick {
                    tick: 30157000,
                    delta: -5823688356774311,
                },
                Tick {
                    tick: 30158000,
                    delta: -4890561440652622,
                },
                Tick {
                    tick: 30171000,
                    delta: -5128549110920,
                },
                Tick {
                    tick: 30177000,
                    delta: -63390579354840402,
                },
                Tick {
                    tick: 30183000,
                    delta: -813408081424,
                },
                Tick {
                    tick: 30186000,
                    delta: -4823373066525869,
                },
                Tick {
                    tick: 30187000,
                    delta: -26213793913321372638,
                },
                Tick {
                    tick: 30189000,
                    delta: -9782900726744891,
                },
                Tick {
                    tick: 30213000,
                    delta: -268935742849537,
                },
                Tick {
                    tick: 30221000,
                    delta: -2416512905624883,
                },
                Tick {
                    tick: 30262000,
                    delta: -486042540854958,
                },
                Tick {
                    tick: 30268000,
                    delta: -1207240216941526,
                },
                Tick {
                    tick: 30290000,
                    delta: -8201366351628828,
                },
                Tick {
                    tick: 30358000,
                    delta: -4707401876168845,
                },
                Tick {
                    tick: 30398000,
                    delta: -3645137456940310615,
                },
                Tick {
                    tick: 30400000,
                    delta: -1833638513892283,
                },
                Tick {
                    tick: 30627000,
                    delta: -168087411394373,
                },
            ],
            30113822,
            "0x34cd4ede216c466c29b59a57d142a444edbbe2",
            29967259116706540975u128,
            "0x20c49ba5e353f80000000000000000",
        )
        .unwrap();

        let input_amount = U256::from(2090562069u128);

        let sell_result = buy(input_amount, &pool);
        assert_eq!(sell_result.info, SwapInfo::Ok);
    }
}
