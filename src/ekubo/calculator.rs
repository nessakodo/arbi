use std::path::Path;

use super::evaluation::{evaluate_path, EvaluatePathResult};
use super::paths::{path_with_tokens_to_path, PathWithTokens};
use super::state::{LoadError, State};
use super::swap::{hex_to_u256, U256};

/// A swap request specifying input token, amount, and desired output token
#[derive(Debug, Clone)]
pub struct SwapRequest {
    /// The token address being swapped from (as hex string)
    pub token_in: String,
    /// The amount of token_in to swap (as U256)
    pub token_amount: U256,
    /// The token address being swapped to (as hex string)
    pub token_out: String,
}

/// Result of evaluating a single path
#[derive(Debug, Clone)]
pub struct PathEvaluation {
    /// The path that was evaluated
    pub path: PathWithTokens,
    /// The evaluation result (amount_out, swaps, etc.)
    pub result: EvaluatePathResult,
    /// Number of hops in this path
    pub hop_count: usize,
}

/// Result of calculating all paths for a swap
#[derive(Debug)]
pub struct SwapCalculation {
    /// The original swap request
    pub request: SwapRequest,
    /// All evaluated paths, sorted by amount_out descending
    pub paths: Vec<PathEvaluation>,
    /// The best path (highest output), if any
    pub best_path: Option<PathEvaluation>,
    /// Total number of paths found
    pub total_paths: usize,
}

/// Parse a hex string token address to U256
fn parse_token_address(s: &str) -> Result<U256, LoadError> {
    hex_to_u256(s)
        .map_err(|e| LoadError::Parse(format!("Failed to parse token address '{}': {}", s, e)))
}

/// Calculate all possible swap paths and their outputs
///
/// # Arguments
/// * `json_path` - Path to the JSON file containing pool data
/// * `swap` - The swap request with token_in, token_amount, and token_out
///
/// # Returns
/// * `SwapCalculation` containing all evaluated paths sorted by output amount
pub fn calculate_swap<P: AsRef<Path>>(
    json_path: P,
    swap: SwapRequest,
) -> Result<SwapCalculation, LoadError> {
    // Parse token addresses
    let source = parse_token_address(&swap.token_in)?;
    let destination = parse_token_address(&swap.token_out)?;

    // Load pools and compute paths
    let state = State::from_json_file(json_path, source, destination)?;

    calculate_swap_from_state(&state, swap)
}

/// Calculate all possible swap paths from a pre-loaded state
///
/// # Arguments
/// * `state` - The state containing pools and paths
/// * `swap` - The swap request with token_in, token_amount, and token_out
///
/// # Returns
/// * `SwapCalculation` containing all evaluated paths sorted by output amount
pub fn calculate_swap_from_state(
    state: &State,
    swap: SwapRequest,
) -> Result<SwapCalculation, LoadError> {
    let total_paths = state.paths.len();

    // Evaluate each path
    let mut evaluations: Vec<PathEvaluation> = state
        .paths
        .iter()
        .map(|path_with_tokens| {
            let path = path_with_tokens_to_path(path_with_tokens);
            let result = evaluate_path(&path, swap.token_amount);
            PathEvaluation {
                path: path_with_tokens.clone(),
                result,
                hop_count: path_with_tokens.len(),
            }
        })
        .collect();

    // Sort by amount_out descending (best first)
    evaluations.sort_by(|a, b| {
        b.result
            .amount_out
            .partial_cmp(&a.result.amount_out)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Get the best path (first after sorting)
    let best_path = evaluations.first().cloned();

    Ok(SwapCalculation {
        request: swap,
        paths: evaluations,
        best_path,
        total_paths,
    })
}

/// Calculate swap and return only successful paths (those with SwapInfo::Ok)
pub fn calculate_swap_successful<P: AsRef<Path>>(
    json_path: P,
    swap: SwapRequest,
) -> Result<SwapCalculation, LoadError> {
    let mut calculation = calculate_swap(json_path, swap)?;

    // Filter to only successful swaps
    calculation
        .paths
        .retain(|eval| eval.result.info == super::swap::SwapInfo::Ok);

    // Update best path
    calculation.best_path = calculation.paths.first().cloned();

    Ok(calculation)
}

impl SwapRequest {
    /// Create a new swap request with U256 amount
    pub fn new(
        token_in: impl Into<String>,
        token_amount: U256,
        token_out: impl Into<String>,
    ) -> Self {
        Self {
            token_in: token_in.into(),
            token_amount,
            token_out: token_out.into(),
        }
    }

    /// Create a new swap request from u128 amount (convenience)
    pub fn from_u128(
        token_in: impl Into<String>,
        token_amount: u128,
        token_out: impl Into<String>,
    ) -> Self {
        Self {
            token_in: token_in.into(),
            token_amount: U256::from(token_amount),
            token_out: token_out.into(),
        }
    }
}

impl SwapCalculation {
    /// Get the best output amount, or 0 if no paths found
    pub fn best_amount_out(&self) -> U256 {
        self.best_path
            .as_ref()
            .map(|p| p.result.amount_out)
            .unwrap_or(U256::ZERO)
    }

    /// Check if any valid path was found
    pub fn has_paths(&self) -> bool {
        !self.paths.is_empty()
    }

    /// Get paths with at most N hops
    pub fn paths_with_max_hops(&self, max_hops: usize) -> Vec<&PathEvaluation> {
        self.paths
            .iter()
            .filter(|p| p.hop_count <= max_hops)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swap_request_new() {
        let swap = SwapRequest::new(
            "0x04718f5a0fc34cc1af16a1cdee98ffb20c31f5cd61d6ab07201858f4287c938d",
            U256::from(1000u128),
            "0x049d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7",
        );

        assert_eq!(swap.token_amount, U256::from(1000u128));
        assert!(swap.token_in.starts_with("0x"));
        assert!(swap.token_out.starts_with("0x"));
    }

    #[test]
    fn test_parse_token_address() {
        let addr = parse_token_address("0x123").unwrap();
        assert_eq!(addr, U256::from(0x123u64));

        let addr = parse_token_address("abc").unwrap();
        assert_eq!(addr, U256::from(0xabcu64));

        // Test large address (252-bit Starknet felt)
        let addr = parse_token_address(
            "0x04718f5a0fc34cc1af16a1cdee98ffb20c31f5cd61d6ab07201858f4287c938d",
        )
        .unwrap();
        assert!(addr > U256::ZERO);
    }

    #[test]
    fn test_swap_calculation_helpers() {
        let swap = SwapRequest::from_u128("0x1", 100, "0x2");
        let calculation = SwapCalculation {
            request: swap,
            paths: vec![],
            best_path: None,
            total_paths: 0,
        };

        assert_eq!(calculation.best_amount_out(), U256::ZERO);
        assert!(!calculation.has_paths());
    }
}
