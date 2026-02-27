//! Constants module for Starknet configuration values.
//!
//! This module centralizes all configuration constants used throughout the application,
//! including contract addresses and chain IDs.

use starknet::core::types::Felt;

// =============================================================================
// Contract Addresses
// =============================================================================

/// STRK token contract address on mainnet
pub const STRK_TOKEN_ADDRESS: &str =
    "0x04718f5a0fc34cc1af16a1cdee98ffb20c31f5cd61d6ab07201858f4287c938d";

/// ETH token contract address on mainnet
pub const ETH_TOKEN_ADDRESS: &str =
    "0x049d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7";

/// USDC token contract address on mainnet (6 decimals)
pub const USDC_TOKEN_ADDRESS: &str =
    "0x053c91253bc9682c04929ca02ed00b3e423f6710d2ee7e0d5ebb06f3ecf368a8";

/// USDT token contract address on mainnet (6 decimals)
pub const USDT_TOKEN_ADDRESS: &str =
    "0x068f5c6a61780768455de69077e07e89787839bf8166decfbf92b645209c0fb8";

/// Ekubo Core contract address on Starknet mainnet
pub const EKUBO_CORE_ADDRESS: &str =
    "0x00000005dd3d2f4429af886cd1a3b08289dbcea99a294197e9eb43b0e0325b4b";

/// Ekubo Router contract address on Starknet mainnet
pub const EKUBO_ROUTER_ADDRESS: &str =
    "0x04505a9f06f2bd639b6601f37a4dc0908bb70e8e0e0c34b1220827d64f4fc066";

// =============================================================================
// Chain IDs
// =============================================================================

/// Mainnet chain ID ("SN_MAIN")
pub const CHAIN_ID_MAINNET: Felt = Felt::from_hex_unchecked("0x534e5f4d41494e");

// =============================================================================
// Transaction Config
// =============================================================================

/// Percentage of expected profit to use as transaction tip (0–100)
pub const DEFAULT_TIP_PERCENTAGE: u64 = 0;
