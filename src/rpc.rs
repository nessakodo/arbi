//! JSON-RPC client for interacting with Starknet, including transaction
//! broadcasting, gas price fetching, and nonce queries.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use starknet::core::types::Felt;
use std::time::Duration;

use tracing::debug;

use crate::ekubo::sync::EventsPage;
use crate::errors::ProviderError;
use crate::gas::{BlockHeader, GasPrice};

/// Generic JSON-RPC response envelope.
#[derive(Deserialize)]
struct JsonRpcResponse<T> {
    result: Option<T>,
    error: Option<serde_json::Value>,
}

pub struct RPC {
    client: reqwest::Client,
    rpc_url: String,
}

impl RPC {
    /// Build an optimized HTTP client for low-latency TX broadcasting
    fn build_optimized_client() -> reqwest::Client {
        reqwest::Client::builder()
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build http client")
    }

    /// Create a new RPC client with the given URL
    pub fn new(rpc_url: String) -> Self {
        Self {
            client: Self::build_optimized_client(),
            rpc_url,
        }
    }

    /// Make a JSON-RPC call, handling the response envelope automatically.
    ///
    /// Sends the request, deserializes the JSON-RPC response, checks for
    /// errors, and extracts the `result` field.
    async fn rpc_call<T: DeserializeOwned>(
        &self,
        request: &impl Serialize,
    ) -> Result<T, ProviderError> {
        let response = self.client.post(&self.rpc_url).json(request).send().await?;

        let text = response.text().await?;
        let resp: JsonRpcResponse<T> = serde_json::from_str(&text)?;

        if let Some(error) = resp.error {
            return Err(ProviderError::Rejection(error.to_string()));
        }

        resp.result
            .ok_or_else(|| ProviderError::InvalidResponse("Missing result".into()))
    }

    /// Get the nonce for an account from the RPC provider
    pub async fn get_nonce(&self, address: Felt) -> Result<u64, ProviderError> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "starknet_getNonce",
            "params": {
                "block_id": "latest",
                "contract_address": format!("{:#x}", address)
            },
            "id": 1
        });

        let nonce_hex: String = self.rpc_call(&request).await?;

        let nonce = Felt::from_hex(&nonce_hex)
            .map_err(|e| ProviderError::InvalidResponse(format!("Invalid nonce hex: {}", e)))?;

        let nonce_u64: u64 = nonce
            .try_into()
            .map_err(|e| ProviderError::InvalidResponse(format!("Invalid nonce: {}", e)))?;

        Ok(nonce_u64)
    }

    /// Broadcast a signed transaction via `starknet_addInvokeTransaction`.
    ///
    /// Accepts the payload produced by [`build_v3_payload`] and wraps it in
    /// the JSON-RPC envelope.
    pub async fn broadcast(&self, payload: &serde_json::Value) -> Result<Felt, ProviderError> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "starknet_addInvokeTransaction",
            "params": { "invoke_transaction": payload },
            "id": 1
        });
        #[derive(Deserialize)]
        struct BroadcastResult {
            transaction_hash: String,
        }

        let result: BroadcastResult = self.rpc_call(&request).await?;

        Felt::from_hex(&result.transaction_hash)
            .map_err(|e| ProviderError::InvalidResponse(format!("Invalid tx hash: {}", e)))
    }

    /// Fetch block header with gas prices via `starknet_getBlockWithTxHashes`.
    pub async fn get_block_header(&self, block_id: &str) -> Result<BlockHeader, ProviderError> {
        let block_id_json = if block_id == "latest" || block_id == "pending" {
            serde_json::json!(block_id)
        } else if let Ok(num) = block_id.parse::<u64>() {
            serde_json::json!({ "block_number": num })
        } else {
            serde_json::json!({ "block_hash": block_id })
        };

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "starknet_getBlockWithTxHashes",
            "params": { "block_id": block_id_json },
            "id": 1
        });

        let block: serde_json::Value = self.rpc_call(&request).await?;

        fn extract_gas_price(block: &serde_json::Value, key: &str) -> Option<GasPrice> {
            let obj = block.get(key)?;
            Some(GasPrice {
                price_in_fri: obj.get("price_in_fri")?.as_str()?.to_string(),
                price_in_wei: obj.get("price_in_wei")?.as_str()?.to_string(),
            })
        }

        Ok(BlockHeader {
            block_number: block.get("block_number").and_then(|v| v.as_u64()),
            l1_gas_price: extract_gas_price(&block, "l1_gas_price"),
            l1_data_gas_price: extract_gas_price(&block, "l1_data_gas_price"),
            l2_gas_price: extract_gas_price(&block, "l2_gas_price"),
        })
    }

    /// Get the latest block number via `starknet_blockNumber`.
    pub async fn get_latest_block_number(&self) -> Result<u64, ProviderError> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "starknet_blockNumber",
            "params": {},
            "id": 1
        });

        self.rpc_call(&request).await
    }

    /// Fetch events via `starknet_getEvents`.
    pub async fn get_events(
        &self,
        address: &str,
        from_block: u64,
        to_block: u64,
        keys: &[&[&str]],
        chunk_size: u64,
        continuation_token: Option<String>,
    ) -> Result<EventsPage, ProviderError> {
        let keys_json: Vec<Vec<&str>> = keys.iter().map(|k| k.to_vec()).collect();

        let mut filter = serde_json::json!({
            "from_block": {"block_number": from_block},
            "to_block": {"block_number": to_block},
            "address": address,
            "keys": keys_json,
            "chunk_size": chunk_size
        });

        if let Some(token) = continuation_token {
            filter["continuation_token"] = serde_json::json!(token);
        }

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "starknet_getEvents",
            "params": {"filter": filter},
            "id": 1
        });

        debug!(
            "starknet_getEvents: {}",
            serde_json::to_string(&request).unwrap_or_default()
        );

        self.rpc_call(&request).await
    }
}
