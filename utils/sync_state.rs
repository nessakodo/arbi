//! Example: Sync state from JSON to the latest on-chain block
//!
//! This example demonstrates how to:
//! 1. Load pool state from a JSON snapshot ({from}.json)
//! 2. Fetch events from block {from + 1} to {to}
//! 3. Apply all events to update the state
//! 4. Export the updated state to {to}.json
//!
//! Usage:
//!   cargo run --example sync_state -- --rpc-url https://your-rpc.com --from 6570500 --to 6633730

use clap::Parser;
use ekubo_arb::ekubo::sync::{init_and_sync_state, SyncConfig};
use ekubo_arb::rpc::RPC;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(name = "sync_state")]
#[command(about = "Sync Ekubo pool state from JSON with on-chain events")]
struct Args {
    /// Starknet RPC URL
    #[arg(long)]
    rpc_url: String,

    /// Snapshot block number. Loads {from}.json and syncs from block {from + 1}
    #[arg(long)]
    from: u64,

    /// Ending block number
    #[arg(long)]
    to: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for logs
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("ekubo_arb=info".parse()?),
        )
        .init();

    let args = Args::parse();
    let json_path = format!("{}.json", args.from);

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║              Sync State to Block                             ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // Configure sync
    let rpc = RPC::new(args.rpc_url.clone());
    let config = SyncConfig::new()?;

    println!("Configuration:");
    println!("  JSON Path:   {}", json_path);
    println!("  From Block:  {}", args.from + 1);
    println!("  To Block:    {}", args.to);
    println!();

    // Time the sync operation
    let start = Instant::now();

    println!("Loading state and syncing events...");

    let (state, result) =
        init_and_sync_state(&rpc, &json_path, args.from + 1, args.to, &config).await?;

    let sync_duration = start.elapsed();

    println!();
    println!("Sync Results:");
    println!("  Pools in state:       {}", state.pool_count());
    println!("  Events processed:     {}", result.events_processed);
    println!("  Transactions applied: {}", result.transactions_applied);
    println!("  Sync duration:        {:?}", sync_duration);
    println!();

    // Export updated state to JSON
    let output_path = format!("{}.json", args.to);
    state.export_to_json_file(&output_path)?;
    println!("Exported state to: {}", output_path);

    println!();
    println!("═══════════════════════════════════════════════════════════════");
    println!("Total execution time: {:?}", start.elapsed());
    println!("═══════════════════════════════════════════════════════════════");

    Ok(())
}
