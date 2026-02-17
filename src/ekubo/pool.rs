use super::swap::{hex_to_u256, U256};

/// A tick in the liquidity pool
#[derive(Debug, Clone)]
pub struct Tick {
    pub tick: i64,
    pub delta: i128,
}

/// Pool state with U256 for precise sqrt_ratio calculations
#[derive(Debug, Clone)]
pub struct Pool {
    pub ticks: Vec<Tick>,
    pub tick: i64,
    /// sqrt_ratio as U256 (Q128.128 fixed-point format)
    pub sqrt_ratio: U256,
    pub liquidity: u128,
    /// fee as U256 (Q128.128 fixed-point format)
    pub fee: U256,
}

impl Pool {
    /// Create a new Pool with U256 sqrt_ratio
    pub fn new(ticks: Vec<Tick>, tick: i64, sqrt_ratio: U256, liquidity: u128, fee: U256) -> Self {
        Self {
            ticks,
            tick,
            sqrt_ratio,
            liquidity,
            fee,
        }
    }

    /// Create a Pool from hex strings (for loading from JSON)
    pub fn from_hex(
        ticks: Vec<Tick>,
        tick: i64,
        sqrt_ratio_hex: &str,
        liquidity: u128,
        fee_hex: &str,
    ) -> Result<Self, String> {
        let sqrt_ratio = hex_to_u256(sqrt_ratio_hex)?;
        let fee = hex_to_u256(fee_hex)?;
        Ok(Self {
            ticks,
            tick,
            sqrt_ratio,
            liquidity,
            fee,
        })
    }
}

/// Tick bounds for update_tick
#[derive(Debug, Clone)]
pub struct TickBounds {
    pub lower: i64,
    pub upper: i64,
}

/// Parameters for updating a tick
#[derive(Debug, Clone)]
pub struct UpdateTick {
    pub bounds: TickBounds,
    pub delta: i128,
}

/// Extension trait for Pool with update methods
pub trait PoolExt {
    /// Update tick bounds with a delta value
    fn update_tick(&mut self, update: UpdateTick);

    /// Update pool state directly (from U256 values)
    fn update(&mut self, liquidity: u128, sqrt_ratio: U256, tick: i64);
}

impl PoolExt for Pool {
    /// Update tick bounds with a delta value
    ///
    /// Updates both upper and lower bounds of the tick range,
    /// and adjusts liquidity if bounds are below current tick.
    fn update_tick(&mut self, update: UpdateTick) {
        let UpdateTick { bounds, delta } = update;

        // Update upper bound - use binary search since ticks are sorted
        {
            let search_result = self.ticks.binary_search_by(|t| t.tick.cmp(&bounds.upper));

            match search_result {
                Ok(idx) => {
                    self.ticks[idx].delta -= delta;
                    // Remove tick if delta became zero
                    if self.ticks[idx].delta == 0 {
                        self.ticks.remove(idx);
                    }
                }
                Err(insert_pos) => {
                    self.ticks.insert(
                        insert_pos,
                        Tick {
                            tick: bounds.upper,
                            delta: -delta,
                        },
                    );
                }
            }

            if bounds.upper <= self.tick {
                self.liquidity = self.liquidity.wrapping_sub_signed(delta);
            }
        }

        // Update lower bound - use binary search since ticks are sorted
        {
            let search_result = self.ticks.binary_search_by(|t| t.tick.cmp(&bounds.lower));

            match search_result {
                Ok(idx) => {
                    self.ticks[idx].delta += delta;
                    // Remove tick if delta became zero
                    if self.ticks[idx].delta == 0 {
                        self.ticks.remove(idx);
                    }
                }
                Err(insert_pos) => {
                    self.ticks.insert(
                        insert_pos,
                        Tick {
                            tick: bounds.lower,
                            delta,
                        },
                    );
                }
            }

            if bounds.lower <= self.tick {
                self.liquidity = self.liquidity.wrapping_add_signed(delta);
            }
        }
    }

    /// Update pool state directly (from U256 values)
    fn update(&mut self, liquidity: u128, sqrt_ratio: U256, tick: i64) {
        self.liquidity = liquidity;
        self.sqrt_ratio = sqrt_ratio;
        self.tick = tick;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_pool() -> Pool {
        Pool::from_hex(
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
            36927003,
            "0x6389f7f2203147955d5b12e80a8286b94becf0a",
            582808287348687200,
            "0x68db8bac710cb4000000000000000",
        )
        .unwrap()
    }

    #[test]
    fn test_update_tick_new_bounds() {
        let mut pool = create_test_pool();

        pool.update_tick(UpdateTick {
            bounds: TickBounds {
                lower: -500,
                upper: 500,
            },
            delta: 1000,
        });

        // Should have 5 ticks now (3 original + 2 new)
        assert_eq!(pool.ticks.len(), 5);

        // Check they're still sorted
        for i in 1..pool.ticks.len() {
            assert!(pool.ticks[i - 1].tick < pool.ticks[i].tick);
        }

        // Find the new ticks
        let lower = pool.ticks.iter().find(|t| t.tick == -500).unwrap();
        assert_eq!(lower.delta, 1000);

        let upper = pool.ticks.iter().find(|t| t.tick == 500).unwrap();
        assert_eq!(upper.delta, -1000);
    }

    #[test]
    fn test_update_tick_existing_bounds() {
        let mut pool = create_test_pool();
        let original_delta = pool.ticks[1].delta; // tick at 0

        pool.update_tick(UpdateTick {
            bounds: TickBounds {
                lower: 0,
                upper: 1000,
            },
            delta: 500,
        });

        // Should still have 3 ticks
        assert_eq!(pool.ticks.len(), 3);

        // Lower bound (0) should have delta increased
        let lower = pool.ticks.iter().find(|t| t.tick == 0).unwrap();
        assert_eq!(lower.delta, original_delta + 500);

        // Upper bound (1000) should have delta decreased
        let upper = pool.ticks.iter().find(|t| t.tick == 1000).unwrap();
        assert_eq!(upper.delta, 5000 - 500);
    }

    #[test]
    fn test_update_tick_liquidity_adjustment() {
        let mut pool = create_test_pool();
        pool.tick = 100;
        let original_liquidity = pool.liquidity;

        // Both bounds below current tick
        pool.update_tick(UpdateTick {
            bounds: TickBounds {
                lower: -2000,
                upper: -1500,
            },
            delta: 1000,
        });

        // Liquidity should be adjusted: +delta for lower, -delta for upper
        // Net effect: 0 (both below current tick)
        assert_eq!(pool.liquidity, original_liquidity);
    }

    #[test]
    fn test_update() {
        let mut pool = create_test_pool();
        let new_ratio = U256::from(2u128) << 128; // 2.0 in Q128.128

        pool.update(5000, new_ratio, 200);

        assert_eq!(pool.liquidity, 5000);
        assert_eq!(pool.sqrt_ratio, new_ratio);
        assert_eq!(pool.tick, 200);
    }
}
