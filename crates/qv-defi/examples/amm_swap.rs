//! # AMM Swap — Full Lifecycle Example
//!
//! Demonstrates the complete QuantumVault DeFi flow using the Shared UTXO
//! pattern with a constant-product AMM (x·y ≥ k).
//!
//! ## What this example covers
//!
//! 1. Creating a liquidity pool with initial reserves.
//! 2. Adding liquidity and minting LP tokens.
//! 3. Executing swaps (both A→B and B→A) with fee deduction.
//! 4. Removing liquidity and burning LP tokens.
//! 5. Invariant verification at every step.
//!
//! ## Running
//!
//! ```bash
//! cargo run -p qv-defi --example amm_swap
//! ```
//!
//! ## Architecture
//!
//! In QuantumVault, each AMM pool is a single UTXO with a `PoolDatum`
//! that encodes reserve balances and LP supply. Swaps consume the old pool
//! UTXO and produce a new one with updated reserves. The script validator
//! checks the `x·y ≥ k` invariant on-chain — the swap math itself is
//! computed off-chain by the batcher.

use qv_core::Hash256;
use qv_defi::amm::{
    compute_add_liquidity, compute_remove_liquidity, compute_swap_output, PoolDatum, PoolState,
    SwapDirection,
};

fn main() {
    println!("========================================");
    println!("  QuantumVault AMM — Full Lifecycle Demo");
    println!("========================================\n");

    // ── Step 1: Create a pool ──────────────────────────────────────────
    // Token A: QV (native token)
    // Token B: USDT (stablecoin)
    // Fee: 30 bps (0.3%, standard Uniswap v2)

    let token_a = Hash256::ZERO; // Placeholder token ID
    let mut token_b_bytes = [0u8; 32];
    token_b_bytes[0] = 0x01;
    let token_b = Hash256::from(token_b_bytes);

    let pool_id_bytes = [0xAA; 32];
    let pool_id = Hash256::from(pool_id_bytes);

    let fee_bps = 30u16;

    let datum = PoolDatum::new(token_a, token_b, 0, 0, fee_bps);
    let mut pool = PoolState::new(pool_id, datum);

    println!("1. Pool created");
    println!("   Token A: QV  |  Token B: USDT");
    println!("   Fee: {} bps ({}%)", fee_bps, fee_bps as f64 / 100.0);
    println!(
        "   Reserves: A={}, B={}",
        pool.datum.reserve_a, pool.datum.reserve_b
    );
    println!();

    // ── Step 2: Add initial liquidity ──────────────────────────────────
    // Alice deposits 10,000 QV + 50,000 USDT (implied price: 1 QV = 5 USDT)

    let amount_a = 10_000u64;
    let amount_b = 50_000u64;

    let (lp_issued, updated_datum) = compute_add_liquidity(
        pool.datum.reserve_a,
        pool.datum.reserve_b,
        pool.datum.lp_total,
        amount_a,
        amount_b,
    )
    .expect("add liquidity failed");

    pool.datum = updated_datum;
    pool.datum.token_a_id = token_a;
    pool.datum.token_b_id = token_b;
    pool.datum.fee_bps = fee_bps;

    println!("2. Initial liquidity added (Alice)");
    println!("   Deposited: {} QV + {} USDT", amount_a, amount_b);
    println!("   LP tokens minted: {}", lp_issued);
    println!(
        "   Reserves: A={}, B={}",
        pool.datum.reserve_a, pool.datum.reserve_b
    );
    println!("   Invariant k = {}", pool.datum.invariant());
    println!();

    let k_initial = pool.datum.invariant();

    // ── Step 3: Swap A→B (Bob buys USDT with QV) ─────────────────────
    // Bob swaps 1,000 QV for USDT

    let swap_in = 1_000u64;
    let (swap_out, fee_collected) = compute_swap_output(
        pool.datum.reserve_a,
        pool.datum.reserve_b,
        swap_in,
        pool.datum.fee_bps,
    )
    .expect("swap computation failed");

    pool.apply_swap(SwapDirection::AtoB, swap_in, swap_out)
        .expect("apply swap failed");

    println!("3. Swap A→B (Bob)");
    println!("   Input:  {} QV", swap_in);
    println!("   Output: {} USDT", swap_out);
    println!("   Fee:    {} QV", fee_collected);
    println!(
        "   Effective price: 1 QV = {:.4} USDT",
        swap_out as f64 / swap_in as f64
    );
    println!(
        "   Reserves: A={}, B={}",
        pool.datum.reserve_a, pool.datum.reserve_b
    );
    println!(
        "   Invariant k = {} (was {})",
        pool.datum.invariant(),
        k_initial
    );
    assert!(
        pool.datum.invariant() >= k_initial,
        "INVARIANT VIOLATED: x·y must not decrease"
    );
    println!("   ✓ Invariant preserved (k' ≥ k)");
    println!();

    // ── Step 4: Swap B→A (Carol buys QV with USDT) ────────────────────

    let swap_in_b = 5_000u64;
    let k_before = pool.datum.invariant();

    let (swap_out_b, fee_b) = compute_swap_output(
        pool.datum.reserve_b,
        pool.datum.reserve_a,
        swap_in_b,
        pool.datum.fee_bps,
    )
    .expect("swap computation failed");

    pool.apply_swap(SwapDirection::BtoA, swap_in_b, swap_out_b)
        .expect("apply swap failed");

    println!("4. Swap B→A (Carol)");
    println!("   Input:  {} USDT", swap_in_b);
    println!("   Output: {} QV", swap_out_b);
    println!("   Fee:    {} USDT", fee_b);
    println!(
        "   Effective price: 1 QV = {:.4} USDT",
        swap_in_b as f64 / swap_out_b as f64
    );
    println!(
        "   Reserves: A={}, B={}",
        pool.datum.reserve_a, pool.datum.reserve_b
    );
    println!(
        "   Invariant k = {} (was {})",
        pool.datum.invariant(),
        k_before
    );
    assert!(
        pool.datum.invariant() >= k_before,
        "INVARIANT VIOLATED: x·y must not decrease"
    );
    println!("   ✓ Invariant preserved (k' ≥ k)");
    println!();

    // ── Step 5: Remove liquidity (Alice withdraws) ────────────────────
    // Alice burns half her LP tokens.

    let lp_to_burn = lp_issued / 2;

    let (withdraw_a, withdraw_b, updated_datum2) = compute_remove_liquidity(
        pool.datum.reserve_a,
        pool.datum.reserve_b,
        pool.datum.lp_total,
        lp_to_burn,
    )
    .expect("remove liquidity failed");

    pool.datum = updated_datum2;
    pool.datum.token_a_id = token_a;
    pool.datum.token_b_id = token_b;
    pool.datum.fee_bps = fee_bps;

    println!("5. Liquidity removed (Alice)");
    println!("   LP burned: {}", lp_to_burn);
    println!("   Received:  {} QV + {} USDT", withdraw_a, withdraw_b);
    println!("   Remaining LP: {}", pool.datum.lp_total);
    println!(
        "   Reserves: A={}, B={}",
        pool.datum.reserve_a, pool.datum.reserve_b
    );
    println!();

    // ── Step 6: Price impact demonstration ────────────────────────────
    // Show how a large swap has significant price impact.

    println!("6. Price impact analysis");
    println!(
        "   Current reserves: A={}, B={}",
        pool.datum.reserve_a, pool.datum.reserve_b
    );
    println!(
        "   Implied price: 1 QV = {:.4} USDT",
        pool.datum.reserve_b as f64 / pool.datum.reserve_a as f64
    );
    println!();

    for size in &[100u64, 500, 1000, 2000] {
        if let Some((out, _fee)) = compute_swap_output(
            pool.datum.reserve_a,
            pool.datum.reserve_b,
            *size,
            pool.datum.fee_bps,
        ) {
            let effective_price = out as f64 / *size as f64;
            let spot_price = pool.datum.reserve_b as f64 / pool.datum.reserve_a as f64;
            let slippage = (1.0 - effective_price / spot_price) * 100.0;
            println!(
                "   Swap {} QV → {} USDT | price: {:.4} | slippage: {:.2}%",
                size, out, effective_price, slippage
            );
        }
    }

    // ── Step 7: Datum serialization ──────────────────────────────────
    // Show how pool state is encoded for on-chain storage.

    println!();
    println!("7. On-chain encoding");
    let encoded = pool.datum.to_bytes().expect("encode failed");
    println!("   Datum size: {} bytes", encoded.len());
    let decoded = PoolDatum::from_bytes(&encoded).expect("decode failed");
    assert_eq!(decoded, pool.datum);
    println!("   ✓ Round-trip encode/decode verified");

    println!();
    println!("========================================");
    println!("  Demo complete — all invariants hold!");
    println!("========================================");
}
