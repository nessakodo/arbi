//! WebSocket event source implementation.
//!
//! Subscribes to `starknet_subscribeEvents` over WebSocket for real-time push-based
//! event delivery.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;
use tracing::info;

use crate::arbitrager::Batch;
use crate::constants::EKUBO_CORE_ADDRESS;
use crate::ekubo::events::{PoolEvent, Transaction, POSITION_UPDATED_KEY, SWAPPED_KEY};
use crate::errors::EventSourceError;

/// Messages sent from the WS worker to the coordinator.
#[derive(Debug)]
pub enum WsEvent {
    /// A normal batch of events.
    Batch(Batch),
    /// The WS reconnected — coordinator should backfill from `last_block + 1`.
    Reconnected { last_block: u64 },
}

/// How long to wait for more events from the same tx before flushing.
const FLUSH_DELAY: Duration = Duration::from_millis(10);

/// How long to wait for a WS message before considering the stream idle.
const READ_TIMEOUT: Duration = Duration::from_secs(180);

/// Delay between reconnection attempts.
const RECONNECT_DELAY: Duration = Duration::from_secs(2);

// =============================================================================
// WebSocket subscription types
// =============================================================================

/// A JSON-RPC message from the WebSocket (covers both responses and notifications).
#[derive(Deserialize, Debug)]
struct WsMessage {
    /// Subscription response: the subscription ID; Notification: the event params
    result: Option<serde_json::Value>,
    params: Option<WsNotificationParams>,
    error: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct WsNotificationParams {
    result: WsEmittedEvent,
}

#[derive(Deserialize, Debug)]
struct WsEmittedEvent {
    block_number: u64,
    transaction_hash: String,
    keys: Vec<String>,
    data: Vec<String>,
}

struct Event {
    block_number: u64,
    transaction_hash: String,
    pool_event: PoolEvent,
}

// =============================================================================
// WebSocket event source
// =============================================================================

/// WebSocket-based event source that subscribes to Ekubo events.
pub struct WsEventSource {
    ws_url: String,
    shutdown: watch::Receiver<bool>,
    worker_handle: Option<tokio::task::JoinHandle<()>>,
}

impl WsEventSource {
    pub fn new(ws_url: String, shutdown: watch::Receiver<bool>) -> Self {
        Self {
            ws_url,
            shutdown,
            worker_handle: None,
        }
    }

    /// Spawn the WebSocket worker and return a receiver for events and reconnection signals.
    pub fn start(&mut self, block: u64) -> mpsc::Receiver<WsEvent> {
        let (tx, rx) = mpsc::channel::<WsEvent>(100);
        let handle = spawn_ws_worker(self.ws_url.clone(), tx, block, self.shutdown.clone());
        self.worker_handle = Some(handle);
        rx
    }

    /// Wait for the worker task to finish.
    pub async fn stop(&mut self) {
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.await;
        }
    }
}

/// Spawn the WebSocket event reader task.
///
/// Connects, subscribes, and streams events into the channel. Reconnects on failure.
fn spawn_ws_worker(
    ws_url: String,
    tx: mpsc::Sender<WsEvent>,
    start_block: u64,
    mut shutdown: watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut block = start_block;
        let mut first_connect = true;
        loop {
            tokio::select! {
                result = run_ws_stream(&ws_url, &tx, &mut block) => {
                    match result {
                        Ok(()) => {
                            info!("WebSocket stream ended cleanly");
                            break;
                        }
                        Err(e) => {
                            info!(error = %e, last_block = block, "WebSocket stream failed, reconnecting...");
                            // Signal coordinator to backfill and advance past the last
                            // fully-processed block. Skip on first connect failure — no
                            // events have been processed yet, so retry from the same block.
                            if !first_connect {
                                let _ = tx.send(WsEvent::Reconnected { last_block: block }).await;
                                block += 1;
                            }
                            tokio::time::sleep(RECONNECT_DELAY).await;
                        }
                    }
                }
                _ = shutdown.changed() => {
                    info!("Shutdown signal received, WebSocket worker stopping");
                    break;
                }
            }
            first_connect = false;
        }
    })
}

/// Run a single WebSocket session: connect, subscribe, read events until failure.
async fn run_ws_stream(
    ws_url: &str,
    tx: &mpsc::Sender<WsEvent>,
    block: &mut u64,
) -> Result<(), EventSourceError> {
    // Connect
    let (mut ws, _response) = tokio_tungstenite::connect_async(ws_url)
        .await
        .map_err(|e| EventSourceError::Http(format!("WebSocket connect failed: {e}")))?;

    // Subscribe to Ekubo events
    // TODO: set finality_status to PRE_CONFIRMED for lowest latency
    let start_block = *block;
    let subscribe_req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "starknet_subscribeEvents",
        "params": {
            "from_address": EKUBO_CORE_ADDRESS,
            "keys": [[SWAPPED_KEY, POSITION_UPDATED_KEY]],
            "block_id": { "block_number": start_block },
        },
        "id": 1
    });

    ws.send(Message::Text(subscribe_req.to_string()))
        .await
        .map_err(|e| EventSourceError::Http(format!("WebSocket send failed: {e}")))?;

    // Read subscription confirmation
    let sub_id = read_subscribe_response(&mut ws).await?;
    info!(subscription_id = %sub_id, start_block, "Subscribed to events");

    info!("WebSocket connected");

    // Stream events with micro-batching: flush after 10ms of silence so events
    // from the same transaction are grouped together. Block changes flush immediately.
    let mut pending: Vec<Event> = Vec::new();

    loop {
        // Use FLUSH_DELAY when we have pending events, READ_TIMEOUT otherwise
        let timeout = if pending.is_empty() {
            READ_TIMEOUT
        } else {
            FLUSH_DELAY
        };

        let msg = tokio::select! {
            msg = ws.next() => msg,
            _ = tokio::time::sleep(timeout) => {
                if pending.is_empty() {
                    return Err(EventSourceError::Http("WebSocket read timeout".to_string()));
                }
                // Flush pending events
                flush_pending(&mut pending, tx, block).await?;
                continue;
            }
        };

        let Some(msg_result) = msg else {
            flush_pending(&mut pending, tx, block).await?;
            return Ok(()); // Stream ended
        };

        let msg =
            msg_result.map_err(|e| EventSourceError::Http(format!("WebSocket read error: {e}")))?;

        let Message::Text(text) = msg else {
            continue;
        };

        let ws_msg: WsMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                info!(error = %e, text = %text, "Failed to parse WS message, skipping");
                continue;
            }
        };

        if let Some(error) = ws_msg.error {
            info!(error = %error, "WS error message received");
        }

        let Some(params) = ws_msg.params else {
            continue;
        };

        let event = params.result;

        let pool_event = match PoolEvent::from_rpc_data(&event.keys, &event.data) {
            Some(ev) => ev,
            None => {
                info!(
                    block_number = event.block_number,
                    tx_hash = %event.transaction_hash,
                    keys_count = event.keys.len(),
                    data_len = event.data.len(),
                    "Failed to parse WS event, skipping"
                );
                continue;
            }
        };

        // If block changed, flush pending events from the previous block first
        if !pending.is_empty() && pending[0].block_number != event.block_number {
            flush_pending(&mut pending, tx, block).await?;
        }

        pending.push(Event {
            block_number: event.block_number,
            transaction_hash: event.transaction_hash,
            pool_event,
        });
    }
}

/// Flush accumulated events: group by tx_hash into `Transaction`s, send as a `Batch`.
async fn flush_pending(
    pending: &mut Vec<Event>,
    tx: &mpsc::Sender<WsEvent>,
    block: &mut u64,
) -> Result<(), EventSourceError> {
    if pending.is_empty() {
        return Ok(());
    }

    let block_number = pending[0].block_number;

    // Group events by transaction_hash, preserving order (drain to avoid clones)
    let mut transactions: Vec<Transaction> = Vec::new();

    for ev in pending.drain(..) {
        if transactions
            .last()
            .is_some_and(|t| t.tx_hash == ev.transaction_hash)
        {
            // Same tx as previous — append to last transaction
            transactions.last_mut().unwrap().events.push(ev.pool_event);
        } else {
            // New transaction
            transactions.push(Transaction::new(
                ev.transaction_hash,
                block_number,
                vec![ev.pool_event],
            ));
        }
    }

    // Update block for reconnection resume
    *block = (*block).max(block_number);

    let msg = WsEvent::Batch(Batch {
        block: block_number,
        transactions,
    });

    tx.send(msg)
        .await
        .map_err(|_| EventSourceError::Http("Channel closed".to_string()))?;

    Ok(())
}

/// Wait for the subscription response and extract the subscription ID.
async fn read_subscribe_response(
    ws: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
              + SinkExt<Message>
              + Unpin),
) -> Result<String, EventSourceError> {
    while let Some(msg_result) = ws.next().await {
        let msg = msg_result.map_err(|e| EventSourceError::Http(format!("WS read error: {e}")))?;

        let Message::Text(text) = msg else {
            continue;
        };

        let ws_msg: WsMessage = serde_json::from_str(&text)
            .map_err(|e| EventSourceError::Http(format!("Bad subscribe response: {e}")))?;

        if let Some(error) = ws_msg.error {
            return Err(EventSourceError::Http(format!("Subscribe failed: {error}")));
        }

        if let Some(result) = ws_msg.result {
            return Ok(result.to_string());
        }
    }

    Err(EventSourceError::Http(
        "WebSocket closed before subscribe response".to_string(),
    ))
}
