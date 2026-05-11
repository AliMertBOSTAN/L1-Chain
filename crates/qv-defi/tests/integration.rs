//! Cross-module integration tests for qv-defi.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use qv_core::{Amount, Hash256, TxId};
use qv_defi::*;

// ============================================================================
// AMM Integration Tests
// ============================================================================

#[test]
fn test_amm_swap_e2e() {
    // Create a pool
    let pool_id = Hash256::from_bytes([0xAA; 32]);
    let token_a = Hash256::from_bytes([1; 32]);
    let token_b = Hash256::from_bytes([2; 32]);

    let mut pool = PoolState::new(
        pool_id,
        PoolDatum::new(token_a, token_b, 10_000, 10_000, 30),
    );

    // Swap 1000 A for B
    let (output, fee) = compute_swap_output(10_000, 10_000, 1_000, 30).unwrap();

    // Apply to pool
    pool.apply_swap(SwapDirection::AtoB, 1_000, output).unwrap();

    // Verify invariant is preserved
    assert!(pool.datum.invariant() >= 10_000u128 * 10_000u128);
    assert_eq!(pool.datum.reserve_a, 11_000);
    assert!(pool.datum.reserve_b < 10_000);
}

#[test]
fn test_amm_add_remove_liquidity() {
    let (lp_add, mut datum) = compute_add_liquidity(10_000, 10_000, 10_000, 5_000, 5_000).unwrap();
    assert_eq!(lp_add, 5_000);
    assert_eq!(datum.lp_total, 15_000);

    let (amount_a, amount_b, final_datum) =
        compute_remove_liquidity(datum.reserve_a, datum.reserve_b, datum.lp_total, 5_000).unwrap();

    assert_eq!(amount_a, 5_000);
    assert_eq!(amount_b, 5_000);
    assert_eq!(final_datum.lp_total, 10_000);
}

#[test]
fn test_amm_invariant_preservation() {
    let initial_inv = 10_000u128 * 10_000u128;

    // Perform multiple swaps
    let mut pool = PoolState::new(
        Hash256::from_bytes([0xAA; 32]),
        PoolDatum::new(
            Hash256::from_bytes([1; 32]),
            Hash256::from_bytes([2; 32]),
            10_000,
            10_000,
            30,
        ),
    );

    for i in 1..=5 {
        let (out, _) = compute_swap_output(
            pool.datum.reserve_a,
            pool.datum.reserve_b,
            1_000,
            30,
        )
        .unwrap();

        pool.apply_swap(SwapDirection::AtoB, 1_000, out).unwrap();

        let new_inv = pool.datum.invariant();
        assert!(new_inv >= initial_inv, "Invariant violated at swap {}", i);
    }
}

// ============================================================================
// Lending Integration Tests
// ============================================================================

// FIXME envanter D-07: InterestAccrualOverflow with realistic slots/year
#[test]
#[ignore]
fn test_lending_full_lifecycle() {
    // Create pool
    let mut pool = lending::LendingPoolDatum::new(
        Hash256::from_bytes([1; 32]),
        Hash256::from_bytes([2; 32]),
        100_000,
        30_000,
    );
    pool.validate().unwrap();

    // User deposits
    let deposit = 50_000;
    let ctokens = compute_deposit(deposit, pool.total_collateral, 100_000).unwrap();
    assert!(ctokens > 0);

    // User creates position
    let mut position = lending::LendingPosition {
        collateral_shares: ctokens,
        debt: 20_000,
        last_interest_update: 0,
    };

    // Check collateralization
    assert!(position.is_collateralized(deposit, position.debt, 7500));

    // Accrue interest. `accrue_interest` is a free function that mutates the
    // pool's `interest_multiplier_q64` field; we capture the before/after to
    // verify it grew.
    let original_mult = pool.interest_multiplier_q64;
    lending::accrue_interest(&mut pool, 365 * 86400, 525_600).unwrap();
    assert!(pool.interest_multiplier_q64 >= original_mult);

    // Repay half
    let repay_amount = 10_000;
    position.debt = position.debt.saturating_sub(repay_amount);
    assert_eq!(position.debt, 10_000);

    // Withdraw — `compute_max_borrow` takes `(collateral, ltv_max_bps)`.
    let max_borrow = compute_max_borrow(deposit, 7500);
    assert!(max_borrow > 0);
}

// FIXME envanter D-08: health_factor calculation off — should be > 1.0 when
// over-collateralized, but Q.64 conversion path is asymmetric.
#[test]
#[ignore]
fn test_lending_liquidation_scenario() {
    let mut position = lending::LendingPosition {
        collateral_shares: 1000,
        debt: 800,
        last_interest_update: 0,
    };

    // Check health factor near liquidation threshold
    let hf = position.health_factor(1000, 8000).unwrap();
    // health = (1000 * 8000) / 800 = 10 in Q.64, >> 1.0
    assert!(hf > (1u128 << 64));

    // If price drops, collateral value drops
    let hf_drop = position.health_factor(900, 8000).unwrap();
    // health = (900 * 8000) / 800 = 9
    assert!(hf_drop < hf);
}

#[test]
fn test_lending_collateral_ratio_computation() {
    let collateral = 1_000_000u64;
    let debt_1 = 500_000u64; // 50% LTV
    let debt_2 = 750_000u64; // 75% LTV (at max)
    let debt_3 = 850_000u64; // 85% LTV (over max)

    let ltv_max = 7500u16; // 75%

    let pos_1 = lending::LendingPosition {
        collateral_shares: collateral,
        debt: debt_1,
        last_interest_update: 0,
    };
    assert!(pos_1.is_collateralized(collateral, debt_1, ltv_max));

    let pos_2 = lending::LendingPosition {
        collateral_shares: collateral,
        debt: debt_2,
        last_interest_update: 0,
    };
    assert!(pos_2.is_collateralized(collateral, debt_2, ltv_max));

    let pos_3 = lending::LendingPosition {
        collateral_shares: collateral,
        debt: debt_3,
        last_interest_update: 0,
    };
    assert!(!pos_3.is_collateralized(collateral, debt_3, ltv_max));
}

// ============================================================================
// Oracle Integration Tests
// ============================================================================

// FIXME envanter D-09: validator price spread (1000..1900) > 90% causes
// `aggregate_median` to fail manipulation detection at default threshold.
// Test data span needs tightening.
#[test]
#[ignore]
fn test_oracle_median_with_multiple_validators() {
    let mut window = OracleWindow::new(Hash256::from_bytes([1; 32]), 10);

    // Add observations from 3 validators
    for i in 1..=3 {
        let obs = PriceObservation::new(
            Hash256::from_bytes([1; 32]),
            (1000 + i * 100) as u128,
            i as u64,
            Hash256::from_bytes([i as u8 + 1; 32]),
            vec![i as u8],
        );
        window.add_observation(obs).unwrap();
    }

    let prices = window.prices();
    let median = aggregate_median(&prices, 500).unwrap();
    assert!(median > 0);
}

#[test]
fn test_oracle_twap_across_window() {
    let obs = vec![
        PriceObservation::new(
            Hash256::from_bytes([1; 32]),
            100u128,
            0,
            Hash256::from_bytes([2; 32]),
            vec![],
        ),
        PriceObservation::new(
            Hash256::from_bytes([1; 32]),
            150u128,
            10,
            Hash256::from_bytes([3; 32]),
            vec![],
        ),
        PriceObservation::new(
            Hash256::from_bytes([1; 32]),
            200u128,
            20,
            Hash256::from_bytes([4; 32]),
            vec![],
        ),
    ];

    let twap = compute_twap(&obs).unwrap();
    // TWAP = (100 * 10 + 150 * 10) / 20 = 2500 / 20 = 125
    assert_eq!(twap, 125);
}

#[test]
fn test_oracle_manipulation_rejection() {
    let prices = vec![
        100u128,  // Normal
        105u128,  // Normal
        1000u128, // Outlier (> 1% deviation)
    ];

    // With tight tolerance, should reject
    let result = aggregate_median(&prices, 100); // 1% max deviation
    assert!(result.is_err());

    // With loose tolerance, should accept (max u16 = 65535 bps ≈ 655%)
    let result = aggregate_median(&prices, 65_000); // 650% — effectively no check
    assert!(result.is_ok());
}

// ============================================================================
// Intent Integration Tests
// ============================================================================

#[test]
fn test_intent_swap_full_flow() {
    let pool_id = Hash256::from_bytes([1; 32]);
    let order_id = TxId::from_bytes([2; 32]);

    let intent = OrderIntent::new_swap(
        order_id,
        pool_id,
        Amount::from_smallest_units(1_000),
        Amount::from_smallest_units(900),
        50, // 0.5% slippage
        1000,
    )
    .with_stealth(vec![0xFF; 32]);

    assert!(intent.validate(500).is_ok());
    assert!(matches!(intent.kind, OrderKind::Swap { .. }));
    assert!(intent.owner_stealth_pk.is_some());
}

#[test]
fn test_intent_bundle_batch_execution() {
    let mut bundle = IntentBundle::new(Hash256::from_bytes([0xAA; 32]), 1000);

    let pool_1 = Hash256::from_bytes([1; 32]);
    let pool_2 = Hash256::from_bytes([2; 32]);

    // Add swaps to two pools
    for i in 1..=3 {
        let intent = OrderIntent::new_swap(
            TxId::from_bytes([i as u8; 32]),
            if i % 2 == 0 { pool_1 } else { pool_2 },
            Amount::from_smallest_units(1_000 * i as u64),
            Amount::from_smallest_units(900 * i as u64),
            50,
            1000,
        );
        bundle.add_order(intent);
    }

    // Validate bundle
    assert!(bundle.validate(500).is_ok());

    // Filter orders by pool
    let pool_1_orders = bundle.orders_for_pool(pool_1);
    assert_eq!(pool_1_orders.len(), 1); // i=2

    let pool_2_orders = bundle.orders_for_pool(pool_2);
    assert_eq!(pool_2_orders.len(), 2); // i=1,3
}

#[test]
fn test_intent_builder_computed_slippage() {
    // Build intent with auto-computed min_receive
    let intent = SwapIntentBuilder::new()
        .order_id(TxId::from_bytes([1; 32]))
        .pool_id(Hash256::from_bytes([2; 32]))
        .offer_amount(Amount::from_smallest_units(1_000))
        .max_slippage_bps(100) // 1%
        .deadline_slot(1000)
        .build()
        .unwrap();

    // min_receive should be 1000 - (1000 * 100 / 10000) = 990
    assert_eq!(intent.min_receive.0, 990);
    assert!(intent.validate(500).is_ok());
}

// ============================================================================
// Cross-Module Integration Tests
// ============================================================================

// FIXME envanter D-10: oracle observation timestamp validity — observation
// `slot=0` rejected at `validate(1000, 150)` because the staleness window
// math underflows. Test fixture and validation contract out of sync.
#[test]
#[ignore]
fn test_amm_oracle_feedback_loop() {
    // AMM produces price
    let mut pool = PoolState::new(
        Hash256::from_bytes([1; 32]),
        PoolDatum::new(
            Hash256::from_bytes([10; 32]),
            Hash256::from_bytes([20; 32]),
            10_000,
            15_000,
            30,
        ),
    );

    // Price = reserve_b / reserve_a = 1.5 in Q64
    let pool_price = ((pool.datum.reserve_b as u128) << 64) / (pool.datum.reserve_a as u128);

    // Oracle observes this price
    let obs = PriceObservation::new(
        pool.pool_id,
        pool_price,
        100,
        Hash256::from_bytes([99; 32]),
        vec![],
    );

    // Validate observation
    assert!(obs.validate(1000, 150).is_ok());
}

#[test]
fn test_intent_to_amm_execution_flow() {
    let pool_id = Hash256::from_bytes([1; 32]);

    // User submits intent
    let intent = OrderIntent::new_swap(
        TxId::from_bytes([2; 32]),
        pool_id,
        Amount::from_smallest_units(1_000),
        Amount::from_smallest_units(800),
        50,
        1000,
    );
    assert!(intent.validate(500).is_ok());

    // Pool state
    let mut pool = PoolState::new(
        pool_id,
        PoolDatum::new(
            Hash256::from_bytes([10; 32]),
            Hash256::from_bytes([20; 32]),
            10_000,
            10_000,
            30,
        ),
    );

    // Batcher executes the intent against pool
    if let Some((output, _fee)) = compute_swap_output(
        pool.datum.reserve_a,
        pool.datum.reserve_b,
        intent.offer_amount.0,
        pool.datum.fee_bps,
    ) {
        // Check slippage
        if output >= intent.min_receive.0 {
            pool.apply_swap(SwapDirection::AtoB, intent.offer_amount.0, output)
                .unwrap();

            // Pool state updated
            assert_eq!(pool.datum.reserve_a, 11_000);
            assert!(pool.datum.reserve_b < 10_000);
        }
    }
}

// FIXME envanter D-10: same oracle observation validity issue as
// `test_amm_oracle_feedback_loop`.
#[test]
#[ignore]
fn test_lending_oracle_price_feedback() {
    let collateral = Hash256::from_bytes([1; 32]);
    let mut pool = lending::LendingPoolDatum::new(
        collateral,
        Hash256::from_bytes([2; 32]),
        1_000_000,
        500_000,
    );
    pool.validate().unwrap();

    // Oracle reports price
    let price_obs = PriceObservation::new(
        collateral,
        1u128 << 64, // 1.0 price
        100,
        Hash256::from_bytes([3; 32]),
        vec![],
    );
    assert!(price_obs.validate(1000, 150).is_ok());

    // Use price to check position health
    let position = lending::LendingPosition {
        collateral_shares: 100_000,
        debt: 50_000,
        last_interest_update: 0,
    };

    // Collateral value = 100_000 * 1.0 = 100_000
    let hf = position.health_factor(100_000, 8000).unwrap();
    assert!(hf > (1u128 << 64)); // Safe
}

#[test]
fn test_roundtrip_serialization() {
    // AMM pool datum
    let pool = PoolDatum::new(
        Hash256::from_bytes([1; 32]),
        Hash256::from_bytes([2; 32]),
        1_000_000,
        2_000_000,
        30,
    );
    let bytes = pool.to_bytes().unwrap();
    let decoded: PoolDatum = PoolDatum::from_bytes(&bytes).unwrap();
    assert_eq!(pool, decoded);

    // Intent
    let intent = OrderIntent::new_swap(
        TxId::from_bytes([1; 32]),
        Hash256::from_bytes([2; 32]),
        Amount::from_smallest_units(1000),
        Amount::from_smallest_units(900),
        50,
        1000,
    );
    let bytes = intent.to_bytes().unwrap();
    let decoded = OrderIntent::from_bytes(&bytes).unwrap();
    assert_eq!(intent, decoded);

    // Intent Bundle
    let mut bundle = IntentBundle::new(Hash256::from_bytes([0xAA; 32]), 1000);
    bundle.add_order(intent);
    let bytes = bundle.to_bytes().unwrap();
    let decoded = IntentBundle::from_bytes(&bytes).unwrap();
    assert_eq!(bundle, decoded);
}
