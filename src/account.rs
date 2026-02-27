use starknet::{core::types::Felt, signers::SigningKey};
use tracing::debug;

use crate::constants::CHAIN_ID_MAINNET;
use crate::gas::GasPriceCache;
use crate::transaction::{
    build_execute_calldata, build_v3_payload, compute_invoke_v3_hash, StarknetCall,
    TransactionConfig,
};

pub struct Account {
    signer: SigningKey,
    address: Felt,
    nonce: u64,
}

impl Account {
    /// Create a new Account from a private key and address
    pub fn new(private_key: Felt, address: Felt, nonce: u64) -> Self {
        let signer = SigningKey::from_secret_scalar(private_key);

        Self {
            signer,
            address,
            nonce,
        }
    }

    pub fn get_nonce(&self) -> Felt {
        Felt::from(self.nonce)
    }

    pub fn increase_nonce(&mut self) {
        self.nonce += 1;
    }

    /// Get the account address
    pub fn address(&self) -> Felt {
        self.address
    }

    /// Build the signed transaction payload for broadcasting.
    /// `profit` is the raw expected profit in FRI — the per-unit tip is derived
    /// internally as `tip_percentage`% of profit / L2 gas max amount.
    pub fn build_payload(
        &mut self,
        gas_price_cache: &GasPriceCache,
        calls: Vec<StarknetCall>,
        expected_profit: u64,
        tip_percentage: u64,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        // Build execute calldata from the calls
        let execute_calldata: Vec<Felt> = build_execute_calldata(&calls);

        let nonce = self.get_nonce();

        // Create resource bounds from cached gas prices
        let resource_bounds = gas_price_cache.to_resource_bounds();

        // Tip (per gas unit) = tip_percentage% of profit / L2 gas max amount.
        // Starknet v3 tip is per-unit (like EIP-1559 priority fee).
        let desired_tip_total = (expected_profit as u128) * (tip_percentage as u128) / 100;
        let tip = (desired_tip_total / resource_bounds.l2_gas.max_amount as u128)
            .min(u64::MAX as u128) as u64;

        // Create transaction config
        let config =
            TransactionConfig::new(self.address, CHAIN_ID_MAINNET, nonce, tip, resource_bounds);

        debug!(
            address = %self.address,
            "Signing transaction"
        );

        // Compute transaction hash for INVOKE v3
        let tx_hash = compute_invoke_v3_hash(&config, &execute_calldata);

        // Sign the transaction hash
        let signature = self
            .signer
            .sign(&tx_hash)
            .map_err(|e| format!("Signing error: {}", e))?;

        let payload = build_v3_payload(&config, &execute_calldata, &signature);

        Ok(payload)
    }
}
