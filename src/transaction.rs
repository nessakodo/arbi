//! Transaction builder — pure computation for Starknet INVOKE v3 transactions.
//!
//! This module handles calldata encoding, Poseidon hashing, ECDSA signing,
//! and JSON payload construction. No network I/O lives here.

use starknet::core::{
    crypto::{HashFunction, Signature},
    types::Felt,
};

// =============================================================================
// Types
// =============================================================================

/// Resource bounds for a specific gas type (L1_GAS, L2_GAS, L1_DATA_GAS)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourceBound {
    /// Maximum amount of this resource to consume
    pub max_amount: u64,
    /// Maximum price per unit of this resource
    pub max_price_per_unit: u128,
}

impl ResourceBound {
    /// Create a new resource bound
    pub fn new(max_amount: u64, max_price_per_unit: u128) -> Self {
        Self {
            max_amount,
            max_price_per_unit,
        }
    }
}

/// All resource bounds for an INVOKE v3 transaction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceBounds {
    pub l1_gas: ResourceBound,
    pub l2_gas: ResourceBound,
    pub l1_data_gas: ResourceBound,
}

impl ResourceBounds {
    /// Create new resource bounds with all fields specified
    pub fn new(l1_gas: ResourceBound, l2_gas: ResourceBound, l1_data_gas: ResourceBound) -> Self {
        Self {
            l1_gas,
            l2_gas,
            l1_data_gas,
        }
    }

    /// Create resource bounds with sensible defaults for mainnet
    pub fn default_mainnet() -> Self {
        Self {
            l1_gas: ResourceBound::new(100, 0x2d8788b3d04a),
            l2_gas: ResourceBound::new(20_000_000, 0x2540BE400),
            l1_data_gas: ResourceBound::new(2000, 0x4a8ae43c2),
        }
    }
}

impl Default for ResourceBounds {
    fn default() -> Self {
        Self::default_mainnet()
    }
}

/// A call to be executed as part of the transaction
#[derive(Debug, Clone)]
pub struct StarknetCall {
    /// Contract address to call
    pub to: Felt,
    /// Selector of the function to call
    pub selector: Felt,
    /// Calldata for the function
    pub calldata: Vec<Felt>,
}

impl StarknetCall {
    /// Create a new call
    pub fn new(to: Felt, selector: Felt, calldata: Vec<Felt>) -> Self {
        Self {
            to,
            selector,
            calldata,
        }
    }
}

/// Configuration for a transaction
#[derive(Debug, Clone)]
pub struct TransactionConfig {
    /// Sender account address
    pub sender_address: Felt,
    /// Chain ID
    pub chain_id: Felt,
    /// Current nonce of the sender account
    pub nonce: Felt,
    /// Tip for priority (in fri)
    pub tip: u64,
    /// Resource bounds for gas
    pub resource_bounds: ResourceBounds,
}

impl TransactionConfig {
    /// Create a new transaction config
    pub fn new(
        sender_address: Felt,
        chain_id: Felt,
        nonce: Felt,
        tip: u64,
        resource_bounds: ResourceBounds,
    ) -> Self {
        Self {
            sender_address,
            chain_id,
            nonce,
            tip,
            resource_bounds,
        }
    }
}

// =============================================================================
// Free functions — pure transaction computation (no network)
// =============================================================================

/// Build the execute calldata from a list of calls.
///
/// The calldata format for OpenZeppelin accounts is:
/// [num_calls, call1_to, call1_selector, call1_calldata_len, ...call1_calldata, call2_to, ...]
pub fn build_execute_calldata(calls: &[StarknetCall]) -> Vec<Felt> {
    let mut calldata = vec![Felt::from(calls.len() as u64)];

    for call in calls {
        calldata.push(call.to);
        calldata.push(call.selector);
        calldata.push(Felt::from(call.calldata.len() as u64));
        calldata.extend(call.calldata.iter().cloned());
    }

    calldata
}

/// Compute the transaction hash for an INVOKE v3 transaction.
///
/// The hash is computed using Poseidon following the Starknet specification:
/// H(invoke_prefix, version, sender, fee_fields_hash, paymaster_hash, chain_id, nonce, da_mode, account_deployment_hash, calldata_hash)
pub fn compute_invoke_v3_hash(config: &TransactionConfig, calldata: &[Felt]) -> Felt {
    let invoke_prefix = Felt::from_bytes_be_slice(b"invoke");
    let version = Felt::THREE;
    let poseidon = HashFunction::poseidon();

    // Pack gas bounds
    let l1_gas_bound = pack_gas_bound(
        b"L1_GAS",
        config.resource_bounds.l1_gas.max_amount,
        config.resource_bounds.l1_gas.max_price_per_unit,
    );
    let l2_gas_bound = pack_gas_bound(
        b"L2_GAS",
        config.resource_bounds.l2_gas.max_amount,
        config.resource_bounds.l2_gas.max_price_per_unit,
    );
    let l1_data_gas_bound = pack_gas_bound(
        b"L1_DATA",
        config.resource_bounds.l1_data_gas.max_amount,
        config.resource_bounds.l1_data_gas.max_price_per_unit,
    );

    // Compute fee fields hash
    let fee_fields_hash = poseidon.hash_many(&[
        Felt::from(config.tip),
        l1_gas_bound,
        l2_gas_bound,
        l1_data_gas_bound,
    ]);

    // Empty paymaster data hash
    let paymaster_data_hash = poseidon.hash_many(&[]);

    // DA mode (0 = L1)
    let da_mode = Felt::ZERO;

    // Empty account deployment data hash
    let account_deployment_data_hash = poseidon.hash_many(&[]);

    // Calldata hash
    let calldata_hash = poseidon.hash_many(calldata);

    // Final hash
    poseidon.hash_many(&[
        invoke_prefix,
        version,
        config.sender_address,
        fee_fields_hash,
        paymaster_data_hash,
        config.chain_id,
        config.nonce,
        da_mode,
        account_deployment_data_hash,
        calldata_hash,
    ])
}

/// Build the v3 transaction payload.
pub fn build_v3_payload(
    config: &TransactionConfig,
    calldata: &[Felt],
    signature: &Signature,
) -> serde_json::Value {
    let calldata_hex: Vec<String> = calldata.iter().map(|f| format!("{:#x}", f)).collect();

    serde_json::json!({
        "type": "INVOKE",
        "version": "0x3",
        "sender_address": format!("{:#x}", config.sender_address),
        "calldata": calldata_hex,
        "signature": [format!("{:#x}", signature.r), format!("{:#x}", signature.s)],
        "nonce": format!("{:#x}", config.nonce),
        "resource_bounds": {
            "l1_gas": {
                "max_amount": format!("{:#x}", config.resource_bounds.l1_gas.max_amount),
                "max_price_per_unit": format!("{:#x}", config.resource_bounds.l1_gas.max_price_per_unit)
            },
            "l2_gas": {
                "max_amount": format!("{:#x}", config.resource_bounds.l2_gas.max_amount),
                "max_price_per_unit": format!("{:#x}", config.resource_bounds.l2_gas.max_price_per_unit)
            },
            "l1_data_gas": {
                "max_amount": format!("{:#x}", config.resource_bounds.l1_data_gas.max_amount),
                "max_price_per_unit": format!("{:#x}", config.resource_bounds.l1_data_gas.max_price_per_unit)
            }
        },
        "tip": format!("{:#x}", config.tip),
        "paymaster_data": [],
        "account_deployment_data": [],
        "nonce_data_availability_mode": "L1",
        "fee_data_availability_mode": "L1"
    })
}

/// Pack gas bound into a single felt.
///
/// Format: [name (8 bytes)] [max_amount (8 bytes)] [max_price_per_unit (16 bytes)]
pub fn pack_gas_bound(name: &[u8], max_amount: u64, max_price_per_unit: u128) -> Felt {
    let mut buffer = [0u8; 32];
    let padding = 8 - name.len();
    buffer[padding..8].copy_from_slice(name);
    buffer[8..16].copy_from_slice(&max_amount.to_be_bytes());
    buffer[16..32].copy_from_slice(&max_price_per_unit.to_be_bytes());
    Felt::from_bytes_be(&buffer)
}
