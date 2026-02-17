use ekubo_arb::ekubo::calculator::{calculate_swap, SwapRequest};
use ekubo_arb::ekubo::swap::{u256_to_f64, SwapInfo, U256};

fn main() {
    let token = "0x04718f5a0fc34cc1af16a1cdee98ffb20c31f5cd61d6ab07201858f4287c938d";
    // 15000 * 10^18
    let amount = U256::from(15000) * U256::from(10).pow(U256::from(18));

    let swap = SwapRequest::new(token, amount, token);

    println!("=== Swap Calculation ===");
    println!("Token In:  {}", swap.token_in);
    println!("Token Out: {}", swap.token_out);
    println!("Amount:    {}", swap.token_amount);
    println!();

    match calculate_swap("5539473.json", swap) {
        Ok(result) => {
            println!("Total paths found: {}", result.total_paths);
            println!();

            if result.paths.is_empty() {
                println!("No paths found between these tokens.");
                return;
            }

            // Show all paths
            println!("=== All Paths (sorted by output, best first) ===");
            for (i, path_eval) in result.paths.iter().enumerate() {
                let status = match &path_eval.result.info {
                    SwapInfo::Ok => "✓",
                    SwapInfo::NoTicks => "✗ NoTicks",
                    SwapInfo::NoLiquidity => "✗ NoLiquidity",
                };

                // Calculate profit using U256
                let amount_out = path_eval.result.amount_out;
                let (profit, is_negative) = if amount_out >= amount {
                    (amount_out - amount, false)
                } else {
                    (amount - amount_out, true)
                };

                // Convert to f64 for percentage calculation
                let amount_f64 = u256_to_f64(&amount);
                let profit_f64 = u256_to_f64(&profit);
                let profit_pct = if is_negative {
                    -(profit_f64 / amount_f64) * 100.0
                } else {
                    (profit_f64 / amount_f64) * 100.0
                };

                let profit_sign = if is_negative { "-" } else { "" };

                println!(
                    "{}. {} hops | Out: {} | Profit: {}{} ({:.4}%) | {}",
                    i + 1,
                    path_eval.hop_count,
                    amount_out,
                    profit_sign,
                    profit,
                    profit_pct,
                    status
                );

                // Show path details
                for (j, hop) in path_eval.path.iter().enumerate() {
                    let dir = match hop.direction {
                        ekubo_arb::ekubo::evaluation::Direction::T0ToT1 => "T0→T1",
                        ekubo_arb::ekubo::evaluation::Direction::T1ToT0 => "T1→T0",
                    };
                    println!("   └─ Hop {}: {} ({})", j + 1, hop.pool.key_string(), dir);
                }
                println!();
            }

            // Summary
            if let Some(best) = &result.best_path {
                println!("=== Best Path ===");
                println!("Hops: {}", best.hop_count);
                println!("Amount Out: {}", best.result.amount_out);

                let (profit, is_negative) = if best.result.amount_out >= amount {
                    (best.result.amount_out - amount, false)
                } else {
                    (amount - best.result.amount_out, true)
                };

                let profit_sign = if is_negative { "-" } else { "" };
                let amount_f64 = u256_to_f64(&amount);
                let profit_f64 = u256_to_f64(&profit);
                let profit_pct = if is_negative {
                    -(profit_f64 / amount_f64) * 100.0
                } else {
                    (profit_f64 / amount_f64) * 100.0
                };

                println!("Profit: {}{}", profit_sign, profit);
                println!("Profit %: {:.4}%", profit_pct);
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
        }
    }
}
