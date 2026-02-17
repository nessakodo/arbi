//! Example: Find best arbitrage opportunity from a JSON state file
//!
//! This example demonstrates how to:
//! 1. Load pool state from a JSON snapshot
//! 2. Compute arbitrage paths for a given token
//! 3. Find the best arbitrage opportunity
//!
//! Usage:
//!   cargo run --example find_opportunity -- --json-path 5602540.json --amount 400000000000000000000

use clap::Parser;
use ekubo_arb::ekubo::state::State;
use ekubo_arb::ekubo::swap::U256;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(name = "find_opportunity")]
#[command(about = "Find best arbitrage opportunity from a JSON state file")]
struct Args {
    /// Path to the JSON file with pool state
    #[arg(long, default_value = "5362485.json")]
    json_path: String,

    /// Token address for arbitrage (defaults to STRK)
    #[arg(
        long,
        default_value = "0x04718f5a0fc34cc1af16a1cdee98ffb20c31f5cd61d6ab07201858f4287c938d"
    )]
    token: String,

    /// Amount in wei (full precision, e.g., 400000000000000000000 for 400 tokens with 18 decimals)
    #[arg(long, default_value = "400000000000000000000")]
    amount: String,

    /// Minimum profit percentage to show (e.g., 0.01 for 0.01%)
    #[arg(long, default_value = "0.0")]
    min_profit_pct: f64,

    /// Show top N routes (0 = only best)
    #[arg(long, default_value = "0")]
    top_n: usize,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║         Find Arbitrage Opportunity                           ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // Load state from JSON
    println!("Loading state from: {}", args.json_path);
    let start = Instant::now();

    let mut state = State::from_json_file_no_paths(&args.json_path)?;
    let load_duration = start.elapsed();

    println!("  Pools loaded:     {}", state.pool_count());
    println!("  Load time:        {:?}", load_duration);
    println!();

    // Initialize paths for the token
    println!("Computing arbitrage paths for token:");
    println!("  {}", args.token);

    let path_start = Instant::now();
    state.init(&args.token, &args.token)?;
    let path_duration = path_start.elapsed();

    println!("  Paths computed:   {}", state.path_count());
    println!("  Computation time: {:?}", path_duration);
    println!();

    // Parse amount from string (supports large values)
    let amount = U256::from_str_radix(&args.amount, 10)
        .map_err(|e| format!("Invalid amount '{}': {}", args.amount, e))?;

    // Convert min profit % to hundredth BIP (100 = 1 BIP = 0.01%, 10000 = 1%)
    let min_profit_hbip = (args.min_profit_pct * 10000.0) as i128;

    println!("═══════════════════════════════════════════════════════════════");
    println!("Searching for opportunities...");
    println!("  Amount:          {}", amount);
    println!("  Min profit:      {}%", args.min_profit_pct);
    println!("═══════════════════════════════════════════════════════════════");
    println!();

    let eval_start = Instant::now();

    if args.top_n > 0 {
        // Show top N routes (not necessarily profitable)
        let routes = state.get_all_routes(amount);
        let eval_duration = eval_start.elapsed();

        let mut all_routes: Vec<_> = routes.iter().collect();

        // Sort by profit descending (best first, including negative)
        all_routes.sort_by(|a, b| b.profit_hbip.cmp(&a.profit_hbip));

        if all_routes.is_empty() {
            println!("No routes found.");
        } else {
            println!(
                "Found {} routes (showing top {}):",
                all_routes.len(),
                args.top_n.min(all_routes.len())
            );
            println!();

            for (i, route) in all_routes.iter().take(args.top_n).enumerate() {
                let profit_pct = route.profit_hbip as f64 / 10000.0;

                let profit_indicator = if route.profit > 0 { "✓" } else { "✗" };

                println!("  {} Route #{}", profit_indicator, i + 1);
                println!("    Hops:       {}", route.hop_count);
                println!("    Amount In:  {}", route.amount_in);
                println!("    Amount Out: {}", route.result.amount_out);
                println!("    Profit:     {} ({:.6}%)", route.profit, profit_pct);

                // Show path details
                print!("    Path:       ");
                for (j, hop) in route.path.iter().enumerate() {
                    if j > 0 {
                        print!(" -> ");
                    }
                    print!(
                        "[{}..{}]",
                        &hop.pool.token0_hex[..10.min(hop.pool.token0_hex.len())],
                        &hop.pool.token1_hex[..10.min(hop.pool.token1_hex.len())]
                    );
                }
                println!();
                println!();
            }
        }

        println!("Evaluation time: {:?}", eval_duration);
    } else {
        // Just show best route
        if let Some(best) = state.get_best(amount) {
            let eval_duration = eval_start.elapsed();
            let profit_pct = best.profit_hbip as f64 / 10000.0;

            if best.profit_hbip < min_profit_hbip {
                println!("Best route found but below minimum profit threshold:");
            } else {
                println!("Best Route Found:");
            }

            println!();
            println!("  Hops:        {}", best.hop_count);
            println!("  Amount In:   {}", best.amount_in);
            println!("  Amount Out:  {}", best.result.amount_out);
            println!("  Profit:      {} ({:.6}%)", best.profit, profit_pct);
            println!();

            // Show path details
            println!("  Path:");
            for (i, hop) in best.path.iter().enumerate() {
                println!(
                    "    Hop {}: {} -> {}",
                    i + 1,
                    &hop.pool.token0_hex,
                    &hop.pool.token1_hex
                );
                println!("           fee: {}", &hop.pool.fee_hex);
                println!("           tick_spacing: {}", hop.pool.tick_spacing);
            }
            println!();

            // Show swap details
            println!("  Swap Details:");
            for (i, swap) in best.result.swaps.iter().enumerate() {
                println!(
                    "    Swap {}: input={:?} output={:?}",
                    i + 1,
                    swap.input,
                    swap.output
                );
            }
            println!();

            println!("  Evaluation time: {:?}", eval_duration);

            // Check if profitable
            if best.profit > 0 && best.profit_hbip >= min_profit_hbip {
                println!();
                println!("  ✓ PROFITABLE OPPORTUNITY FOUND!");
                println!("    Expected profit: {} ({:.6}%)", best.profit, profit_pct);
            }
        } else {
            println!("No routes found for the given token.");
        }
    }

    println!();
    println!("═══════════════════════════════════════════════════════════════");
    println!("Total execution time: {:?}", start.elapsed());
    println!("═══════════════════════════════════════════════════════════════");

    Ok(())
}
