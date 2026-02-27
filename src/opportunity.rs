use starknet::core::types::Felt;
use starknet::core::utils::get_selector_from_name;

use crate::ekubo::evaluation::EvaluatePathResult;
use crate::ekubo::paths::PathWithTokens;
use crate::ekubo::swap::{u256_to_felt, U256};
use crate::transaction::StarknetCall;

/// Result of arbitrage evaluation
#[derive(Debug, Clone)]
pub struct ArbitrageOpportunity {
    /// Token being arbitraged (token_in == token_out)
    pub token: U256,
    /// Amount in (as U256)
    pub amount_in: U256,
    /// Amount out (as U256)
    pub amount_out: U256,
    /// Profit (as i128, positive or negative)
    pub profit: i128,
    /// Profit in hundredth basis points: 100 = 1 BIP = 0.01%, 10000 = 1%
    pub profit_hbip: i128,
    /// Number of hops in the route
    pub hop_count: usize,
    /// The path taken for this opportunity
    pub path: PathWithTokens,
    /// The evaluation result including internal swap details
    pub result: EvaluatePathResult,
}

impl ArbitrageOpportunity {
    /// Format the path as a human-readable string showing token flow
    pub fn format_path(&self) -> String {
        if self.path.is_empty() {
            return String::from("(empty path)");
        }

        let pool_keys: Vec<String> = self
            .path
            .iter()
            .map(|hop| {
                format!(
                    "[{}-{}-{}-{}-{}]",
                    truncate_hex_str(&hop.pool.token0_hex),
                    truncate_hex_str(&hop.pool.token1_hex),
                    truncate_hex_str(&hop.pool.fee_hex),
                    truncate_hex_str(&hop.pool.tick_spacing_hex),
                    truncate_hex_str(&hop.pool.extension_hex),
                )
            })
            .collect();

        pool_keys.join(" -> ")
    }

    /// Format each hop of the route as an array for Cairo consumption
    pub fn format_route_arrays(&self) -> Vec<String> {
        self.get_route_calldata()
            .iter()
            .map(|hop_calldata| {
                format!(
                    "[\n        {:#x},\n        {:#x},\n        {:#x},\n        {:#x},\n        {:#x},\n        {:#x},\n        {:#x},\n        {:#x},\n    ]",
                    hop_calldata[0],
                    hop_calldata[1],
                    hop_calldata[2],
                    hop_calldata[3],
                    hop_calldata[4],
                    hop_calldata[5],
                    hop_calldata[6],
                    hop_calldata[7]
                )
            })
            .collect()
    }

    /// Get each hop of the route as a vector of Felts for Starknet calldata
    pub fn get_route_calldata(&self) -> Vec<Vec<Felt>> {
        self.path
            .iter()
            .enumerate()
            .map(|(i, hop)| {
                let ratio = self.result.swaps[i].sqrt_ratio;
                let ratio_limbs = ratio.as_limbs();
                let ratio_low =
                    Felt::from((ratio_limbs[0] as u128) | ((ratio_limbs[1] as u128) << 64));
                let ratio_high =
                    Felt::from((ratio_limbs[2] as u128) | ((ratio_limbs[3] as u128) << 64));

                vec![
                    u256_to_felt(&hop.pool.token0),
                    u256_to_felt(&hop.pool.token1),
                    u256_to_felt(&hop.pool.fee),
                    Felt::from(hop.pool.tick_spacing as i128),
                    u256_to_felt(&hop.pool.extension),
                    ratio_low,
                    ratio_high,
                    Felt::ZERO, // skip_ahead
                ]
            })
            .collect()
    }

    /// Build Starknet calls for executing this arbitrage opportunity
    ///
    /// # Arguments
    /// * `token_address` - The token address for transfer and clear_minimum
    /// * `ekubo_router_address` - The Ekubo router contract address
    ///
    /// Returns a vector of calls: multihop_swap and clear_minimum
    pub fn build_swap_calls(
        &self,
        token_address: Felt,
        ekubo_router_address: Felt,
        min_realization_bps: u64,
        min_profit_floor_fri: u128,
    ) -> Vec<StarknetCall> {
        let route_hops = self.get_route_calldata();
        let (amount_low, amount_high) = split_u256_to_felts(&self.amount_in);
        let mut swap_calldata = vec![Felt::from(route_hops.len() as u64)];
        for hop in route_hops {
            swap_calldata.extend(hop);
        }

        // Add TokenAmount argument (token, amount_low)
        swap_calldata.extend(vec![token_address, amount_low, amount_high]);

        // Protect against unexpected execution quality by requiring a minimum output.
        let expected_profit = self.profit.max(0) as u128;
        let realized_profit_floor = expected_profit.saturating_mul(min_realization_bps as u128) / 10_000;
        let required_profit = realized_profit_floor.max(min_profit_floor_fri);
        let minimum_out = self.amount_in.saturating_add(U256::from(required_profit));
        let (minimum_out_low, minimum_out_high) = split_u256_to_felts(&minimum_out);

        vec![
            StarknetCall {
                to: ekubo_router_address,
                selector: get_selector_from_name("multihop_swap").unwrap(),
                calldata: swap_calldata,
            },
            StarknetCall {
                to: ekubo_router_address,
                selector: get_selector_from_name("clear_minimum").unwrap(),
                calldata: vec![
                    token_address,
                    minimum_out_low,
                    minimum_out_high,
                ],
            },
        ]
    }
}

fn split_u256_to_felts(value: &U256) -> (Felt, Felt) {
    let limbs = value.as_limbs();
    let low = Felt::from((limbs[0] as u128) | ((limbs[1] as u128) << 64));
    let high = Felt::from((limbs[2] as u128) | ((limbs[3] as u128) << 64));
    (low, high)
}

/// Truncate a hex string to show prefix (first 6 chars after 0x)
fn truncate_hex_str(hex: &str) -> String {
    let hex = hex.trim_start_matches("0x");
    if hex.len() > 8 {
        format!("0x{}", &hex[..6])
    } else {
        format!("0x{}", hex)
    }
}
