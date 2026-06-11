# QuantumVault DeFi SDK Guide

> **Status (2026-05-12):** Bu doküman ileride sunulacak QuantumVault DeFi SDK'sı
> için **konsept ve pseudo-API rehberidir**. Kod örnekleri (`tx.sign(...)`,
> `SignatureType::*`, `AmmPool::*` vb.) gerçek `qv-defi` / `qv-wallet` Rust
> API'sini birebir yansıtmaz; gerçek API yüzeyleri için `crates/qv-defi/`,
> `crates/qv-wallet/`, ve `docs/SYSTEM_OVERVIEW.md §10/§14` referans alınmalıdır.
> İmza algoritması ADR-006 (2026-05-07) ile FIPS 204 ML-DSA olarak sabitlendi
> (eski "Dilithium" terminolojisi aynı algoritma ailesi — `SignatureType::MlDsa`).

Comprehensive guide for building DeFi applications on QuantumVault L1, including AMM, lending, oracle integration, and intent-based order flows.

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [AMM Integration](#amm-integration)
3. [Lending Protocol](#lending-protocol)
4. [Oracle Feeds](#oracle-feeds)
5. [Intent System](#intent-system)
6. [Script Development](#script-development)
7. [Wallet SDK Integration](#wallet-sdk-integration)
8. [Example Flows](#example-flows)
9. [Security Considerations](#security-considerations)
10. [Testing on Devnet](#testing-on-devnet)

---

## Architecture Overview

QuantumVault uses a **UTXO+eUTXO model** (Cardano-inspired) rather than accounts. This fundamentally changes how DeFi primitives are built.

### Why UTXO, Not Accounts?

- **Client-Side Validation (CSV):** L1 kernel stays deterministic and lightweight—never executes contracts
- **Parallelism:** Non-overlapping UTXOs can be spent in parallel without conflict
- **Privacy:** Each UTXO is separate; graph analysis is harder
- **Simple finality:** No account state reorg issues; blocks are immutable once signed

### Shared UTXO Pattern for DeFi

Instead of a shared account (like Uniswap's pool contract), **each pool is a single UTXO** with its reserves encoded in the `datum`:

```rust
pub struct PoolDatum {
    reserve_a: u64,              // Token A balance
    reserve_b: u64,              // Token B balance
    lp_total: u64,               // Total LP tokens issued
    fee_bps: u16,                // Fee in basis points (e.g., 30 = 0.3%)
    token_a: AssetId,            // Token A identifier
    token_b: AssetId,            // Token B identifier
}
```

**Every swap transaction:**
1. Consumes old pool UTXO (with old datum)
2. Produces new pool UTXO (with new datum satisfying x·y≥k invariant)
3. Outputs user's swapped tokens

The **locking script** (covenant) enforces the invariant—L1 verifies only the script's validation, not AMM logic itself.

### Script VM: Validation, Not Execution

QuantumVault scripts **never execute arbitrary code**. They only answer: "Can this UTXO be spent?"

- **Max script size:** 16 KB
- **Max gas:** 100,000 units
- **Deterministic:** no floating point, no nondeterminism
- **Stack-based:** similar to Bitcoin Script but post-quantum capable

---

## AMM Integration

### Creating an AMM Pool

Use the `qv-defi` crate's `AMM` module:

```rust
use qv_defi::amm::{PoolDatum, create_pool_script};
use qv_core::transaction::{TxBuilder, Datum};
use qv_crypto::PublicKey;

// 1. Create pool datum
let pool_datum = PoolDatum {
    reserve_a: 1_000_000,  // 1M tokens A
    reserve_b: 2_000_000,  // 2M tokens B
    lp_total: 1_414_213,   // sqrt(1M * 2M)
    fee_bps: 30,           // 0.3% fee
    token_a: AssetId::from_hex("a001"),
    token_b: AssetId::from_hex("b001"),
};

// 2. Create pool script (enforces x·y≥k)
let pool_script = create_pool_script(&pool_datum)?;

// 3. Create transaction
let tx = TxBuilder::new()
    .input(initial_funding_utxo)
    .output(
        value_satoshis(3_000_000),
        pool_script.clone(),
        Some(Datum::from_cbor(&pool_datum)?),
    )
    .fee(10_000)
    .build()?;

// 4. Sign and submit
let signed_tx = tx.sign(&pool_operator_key, SignatureType::MlDsa)?;
node_client.submit_transaction(&signed_tx).await?;
```

### Performing a Swap

```rust
use qv_defi::amm::compute_swap_output;

// Pool state from chain (as UTXO with datum)
let old_pool = fetch_pool_utxo("pool_id_abc").await?;
let mut pool_datum = old_pool.datum()?;

// User wants to swap 100k tokens A for tokens B
let input_a = 100_000;
let output_b = compute_swap_output(
    &pool_datum,
    input_a,
    TokenType::A,
)?;  // Returns: ~181_818 (after x·y≥k and fee deduction)

// Build swap transaction
let tx = TxBuilder::new()
    // Consume old pool UTXO
    .input(old_pool.outpoint(), Some(Redeemer::Swap))
    // Input user's tokens A
    .input(user_input_utxo, None)
    // Output new pool UTXO with updated datum
    .output_with_datum(
        pool_datum.reserve_a + input_a,  // x' = x + 100k
        Value::zero()
            .add(pool_datum.token_a, pool_datum.reserve_a + input_a)
            .add(pool_datum.token_b, pool_datum.reserve_b - output_b),
        pool_script.clone(),
        {
            pool_datum.reserve_a += input_a;
            pool_datum.reserve_b -= output_b;
            Datum::from_cbor(&pool_datum)?
        },
    )
    // Output user's tokens B
    .output(
        value_satoshis(0)
            .add(pool_datum.token_b, output_b),
        user_address_script.clone(),
        None,  // No datum needed
    )
    .fee(5_000)
    .build()?;

let signed_tx = tx.sign(&user_key, SignatureType::MlDsa)?;
node_client.submit_transaction(&signed_tx).await?;
```

### Adding Liquidity

```rust
use qv_defi::amm::compute_add_liquidity;

let old_pool = fetch_pool_utxo("pool_id").await?;
let mut pool_datum = old_pool.datum()?;

// User deposits 500k A and 1M B
let deposit_a = 500_000;
let deposit_b = 1_000_000;

let (lp_tokens, new_reserve_a, new_reserve_b) = 
    compute_add_liquidity(&pool_datum, deposit_a, deposit_b)?;
// lp_tokens = ~707_106

let tx = TxBuilder::new()
    .input(old_pool.outpoint(), Some(Redeemer::AddLiquidity))
    .input(user_tokens_a_utxo, None)
    .input(user_tokens_b_utxo, None)
    .output_with_datum(
        value_zero(),
        Value::zero()
            .add(pool_datum.token_a, new_reserve_a)
            .add(pool_datum.token_b, new_reserve_b),
        pool_script.clone(),
        {
            pool_datum.reserve_a = new_reserve_a;
            pool_datum.reserve_b = new_reserve_b;
            pool_datum.lp_total += lp_tokens;
            Datum::from_cbor(&pool_datum)?
        },
    )
    .output(
        value_zero()
            .add(AssetId::lp_token(&pool_datum), lp_tokens),
        user_address_script.clone(),
        None,
    )
    .fee(5_000)
    .build()?;

let signed = tx.sign(&user_key, SignatureType::MlDsa)?;
node_client.submit_transaction(&signed).await?;
```

---

## Lending Protocol

### On-Chain Lending Covenant (Faz 6 / D-6, ADR-013) — current API

As of D-6 the lending pool is enforced **on-chain** by the
`qv_script::templates::lending_pool_lock` covenant. The pool UTXO carries
the canonical 146-byte `LendingPoolDatum` encoding
(`LendingPoolDatum::to_canonical_bytes`), and four wallet-side builders in
`qv_defi::tx_helpers` assemble covenant-satisfying transactions:

```rust
use qv_defi::{
    build_lending_deposit_tx, build_lending_borrow_tx,
    build_lending_repay_tx, build_lending_withdraw_tx,
    sign_oracle_price, LendingRequestCore,
};

// Deposit / repay: price-less spend path (pool health can only improve).
let bundle = build_lending_deposit_tx(&req, 50_000)?;

// Borrow / withdraw: need a fresh oracle-signed price. The oracle
// operator signs `TAG ‖ pool_id ‖ price_scaled ‖ slot` with ML-DSA:
let price = sign_oracle_price(&oracle_secret, &oracle_public,
                              pool_id, price_scaled, current_slot)?;
let bundle = build_lending_borrow_tx(&req, 400_000, &price)?;

// `bundle.tx` carries the pool input's covenant witness already;
// the wallet signs `bundle.inputs_to_sign` (the user input) and broadcasts.
```

The covenant verifies: datum shape + pinned identity/risk parameters,
frozen interest fields, pool native-value preservation, script
continuity, and — on the borrow/withdraw path — price freshness
(`SLOT_NUMBER` window) plus a real `CHECKSIG_PQC` over the oracle message
and the division-free collateral check
`total_debt · K ≤ total_collateral · price_scaled` (`K` baked in from
`ltv_max_bps`, see `lending_ltv_factor`).

**Honest v1 limits (ADR-013):** single oracle key (t-of-n median is v2);
pool-aggregate LTV only — per-position enforcement and liquidation need
position UTXOs (v2); no on-chain interest accrual (interest fields are
frozen by the covenant; `accrue_interest` is off-chain quoting); token
settlement is datum-level accounting until native multi-asset outputs
land; **no CLI/RPC surface yet** — this layer stops at `tx_helpers`
(a `qv-wallet lend` CLI following the D-4 `swap` pattern is future work).

> The subsections below describe the longer-term design (per-position
> Merkle roots, position NFTs, redeemers) and do **not** reflect the
> current API.

### Lending Pool Architecture

Each lending pool has a single UTXO encoding collateral and debt:

```rust
pub struct LendingPoolDatum {
    collateral_id: AssetId,
    debt_id: AssetId,
    total_collateral: u64,           // Sum of all collateral
    total_debt: u64,                 // Sum of all debt
    interest_rate_params: InterestParams,
    positions_merkle_root: [u8; 32], // Proof of all user positions
}

pub struct InterestParams {
    base_rate_bps: u16,              // e.g., 100 = 1% annually
    utilization_multiplier: u16,     // Slope: 5% per 1% utilization
}

pub struct UserPosition {
    user_id: PublicKey,              // Position identifier
    collateral_amount: u64,
    debt_amount: u64,
}
```

### Depositing Collateral

```rust
use qv_defi::lending::{compute_deposit, LendingPoolDatum};

let pool = fetch_lending_pool_utxo("lending_pool_id").await?;
let mut pool_datum = pool.datum()?;

// User deposits 50k tokens as collateral
let deposit_amount = 50_000;
let new_total_collateral = pool_datum.total_collateral + deposit_amount;

// Update Merkle root for new position (simplified)
let new_root = recompute_positions_merkle(
    &pool_datum.positions_merkle_root,
    &user_pubkey,
    deposit_amount,
    0,  // No debt initially
)?;

let tx = TxBuilder::new()
    .input(pool.outpoint(), Some(Redeemer::Deposit))
    .input(user_collateral_utxo, None)
    .output_with_datum(
        value_zero(),
        Value::zero()
            .add(pool_datum.collateral_id, new_total_collateral)
            .add(pool_datum.debt_id, pool_datum.total_debt),
        lending_script.clone(),
        {
            pool_datum.total_collateral = new_total_collateral;
            pool_datum.positions_merkle_root = new_root;
            Datum::from_cbor(&pool_datum)?
        },
    )
    .output(
        value_zero()
            .add(AssetId::position_nft(&user_pubkey), 1),
        user_address_script.clone(),
        None,
    )
    .fee(5_000)
    .build()?;

let signed = tx.sign(&user_key, SignatureType::MlDsa)?;
node_client.submit_transaction(&signed).await?;
```

### Borrowing Against Collateral

```rust
use qv_defi::lending::{compute_max_borrow, health_factor};

let pool = fetch_lending_pool_utxo("lending_pool_id").await?;
let pool_datum = pool.datum()?;
let user_position = fetch_user_position(&pool_datum, &user_pubkey)?;

// Max borrow at 75% LTV
let max_borrow = compute_max_borrow(
    user_position.collateral_amount,
    &oracle_price_feed,
    0.75,  // LTV
)?;

let borrow_amount = 30_000;  // User wants to borrow 30k debt tokens

// Verify health factor will remain above 80% threshold
let new_health = health_factor(
    user_position.collateral_amount,
    user_position.debt_amount + borrow_amount,
    &oracle_price_feed,
)?;

if new_health < 1.2 {  // Liquidation threshold
    return Err("Insufficient collateral for borrow");
}

// Update pool state
let mut new_pool_datum = pool_datum.clone();
new_pool_datum.total_debt += borrow_amount;

// Accrue interest (simple linear model)
let interest_accrued = accrue_interest(
    &pool_datum,
    std::time::SystemTime::now(),
)?;

let tx = TxBuilder::new()
    .input(pool.outpoint(), Some(Redeemer::Borrow))
    .output_with_datum(
        value_zero(),
        Value::zero()
            .add(pool_datum.collateral_id, pool_datum.total_collateral)
            .add(pool_datum.debt_id, new_pool_datum.total_debt),
        lending_script.clone(),
        Datum::from_cbor(&new_pool_datum)?,
    )
    .output(
        value_zero()
            .add(pool_datum.debt_id, borrow_amount),
        user_address_script.clone(),
        None,
    )
    .fee(5_000)
    .build()?;

let signed = tx.sign(&user_key, SignatureType::MlDsa)?;
node_client.submit_transaction(&signed).await?;
```

### Liquidation

```rust
// Check if user is liquidatable
let user_position = fetch_user_position(&pool_datum, &liquidate_user)?;
let health = health_factor(
    user_position.collateral_amount,
    user_position.debt_amount,
    &oracle_price,
)?;

if health >= 1.0 {
    return Err("User not liquidatable (health > 1.0)");
}

// Liquidator repays 50% of debt, gets 55% of collateral (10% liquidation bonus)
let repay_amount = user_position.debt_amount / 2;
let collateral_reward = (user_position.collateral_amount / 2) + 
                        (user_position.collateral_amount / 20);

let mut new_pool_datum = pool_datum.clone();
new_pool_datum.total_debt -= repay_amount;
new_pool_datum.total_collateral -= collateral_reward;

let tx = TxBuilder::new()
    .input(pool.outpoint(), Some(Redeemer::Liquidate))
    .input(liquidator_debt_tokens, None)
    .output_with_datum(
        value_zero(),
        Value::zero()
            .add(pool_datum.collateral_id, new_pool_datum.total_collateral)
            .add(pool_datum.debt_id, new_pool_datum.total_debt),
        lending_script.clone(),
        Datum::from_cbor(&new_pool_datum)?,
    )
    .output(
        value_zero()
            .add(pool_datum.collateral_id, collateral_reward),
        liquidator_address_script.clone(),
        None,
    )
    .fee(5_000)
    .build()?;

let signed = tx.sign(&liquidator_key, SignatureType::MlDsa)?;
node_client.submit_transaction(&signed).await?;
```

---

## Oracle Feeds

### Oracle Module Architecture

```rust
pub struct PriceObservation {
    timestamp: u64,
    price: u128,              // Scaled 1e18
    token_a: AssetId,
    token_b: AssetId,
}

pub struct OracleDatum {
    observations: Vec<PriceObservation>,
    last_update: u64,
}
```

### Reading Prices with TWAP

```rust
use qv_defi::oracle::{OracleDatum, compute_twap};

// Fetch oracle UTXO (typically maintained by validators)
let oracle_utxo = fetch_oracle_utxo("oracle_key_hash").await?;
let oracle_datum = oracle_utxo.datum::<OracleDatum>()?;

// Compute time-weighted average price over 1 hour
let price_1h_twap = compute_twap(
    &oracle_datum.observations,
    3600,  // seconds
)?;

println!("TWAP (1h): {:.6}", price_1h_twap as f64 / 1e18);

// Check for price manipulation
const MAX_DEVIATION_BPS: u16 = 500;  // 5% max deviation
if let Some(latest) = oracle_datum.observations.last() {
    let deviation = ((latest.price as i128 - price_1h_twap as i128).abs() 
                     as u128 * 10000) / price_1h_twap;
    if deviation > MAX_DEVIATION_BPS as u128 {
        eprintln!("WARNING: Price manipulation detected");
    }
}
```

### Median Aggregation from Multiple Oracles

```rust
pub async fn aggregate_price(
    token_a: &AssetId,
    token_b: &AssetId,
    oracle_sources: &[&str],
) -> Result<u128> {
    let mut prices = Vec::new();
    
    for source in oracle_sources {
        let oracle = fetch_oracle_by_key(source).await?;
        let datum = oracle.datum::<OracleDatum>()?;
        if let Some(obs) = datum.observations.last() {
            if obs.token_a == *token_a && obs.token_b == *token_b {
                prices.push(obs.price);
            }
        }
    }
    
    if prices.is_empty() {
        return Err("No price observations available");
    }
    
    // Median (resistant to 1-2 outliers)
    prices.sort();
    let median = prices[prices.len() / 2];
    
    Ok(median)
}
```

### Validator-Signed Price Updates

Validators (slot leaders) periodically submit signed price observations:

```rust
pub async fn submit_price_observation(
    validator_key: &SecretKey,
    token_a: AssetId,
    token_b: AssetId,
    price: u128,
    oracle_utxo: &Outpoint,
) -> Result<()> {
    let obs = PriceObservation {
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
        price,
        token_a,
        token_b,
    };
    
    // Sign observation (ML-DSA-65, FIPS 204)
    let signature = validator_key.sign_dilithium(&obs.to_cbor())?;
    
    // Attach to oracle UTXO
    let mut oracle_datum = fetch_oracle_datum(&oracle_utxo).await?;
    oracle_datum.observations.push(obs);
    oracle_datum.observations.retain(|o| {
        // Keep only recent observations (last 24 hours)
        o.timestamp > (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() - 86400)
    });
    
    let tx = TxBuilder::new()
        .input(oracle_utxo.clone(), Some(Redeemer::UpdatePrice))
        .output_with_datum(
            value_zero(),
            Value::zero(),
            oracle_script.clone(),
            Datum::from_cbor(&oracle_datum)?,
        )
        .fee(1_000)
        .build()?;
    
    let signed = tx.sign(&validator_key, SignatureType::MlDsa)?;
    submit_transaction(&signed).await?;
    Ok(())
}
```

---

## Intent System

### Building and Submitting Intents

Intents are encrypted swap/lending orders submitted to the mempool. The slot leader decrypts and batches them deterministically.

```rust
use qv_defi::intents::{OrderIntent, SwapIntent, SwapIntentBuilder};
use qv_mempool::encrypted::threshold_kyber_encrypt;

// 1. Build swap intent
let intent = SwapIntentBuilder::new()
    .token_in(AssetId::from_hex("a001"))
    .token_out(AssetId::from_hex("b001"))
    .amount_in(100_000)
    .min_amount_out(180_000)  // Slippage protection
    .deadline(block_height + 100)  // Valid for ~100 blocks
    .user_address(user_pubkey.clone())
    .build()?;

// 2. Serialize intent
let intent_bytes = intent.to_cbor()?;

// 3. Encrypt with threshold Kyber (slot leader + committee threshold)
// Requires committee public keys from genesis
let encrypted_intent = threshold_kyber_encrypt(
    &intent_bytes,
    &committee_pubkeys,
    threshold,
)?;

// 4. Submit to encrypted mempool
let tx = TxBuilder::new()
    .mempool_message(
        MempoolMessage::EncryptedIntent(encrypted_intent),
    )
    .fee(1_000)
    .build()?;

node_client.submit_transaction(&tx).await?;
```

### Intent Batching (Automatic)

The slot leader decrypts intents and batches them deterministically:

```rust
// This happens automatically on the slot leader (not user code)
pub async fn batch_intents(
    slot: u64,
    encrypted_intents: Vec<Vec<u8>>,
    validator_key: &SecretKey,
    committee_key_shares: &[Vec<u8>],  // t-of-n Kyber shares
) -> Result<Vec<Transaction>> {
    // 1. Threshold decryption: combine committee shares
    let plaintext = threshold_kyber_decrypt(
        &encrypted_intents[0],
        committee_key_shares,
        threshold,
    )?;
    
    let intent: OrderIntent = OrderIntent::from_cbor(&plaintext)?;
    
    // 2. Deterministic ordering (no MEV)
    // Sort by intent hash modulo slot randomness
    let slot_seed = vrf_output(validator_key, slot);
    let mut intents_with_seed: Vec<_> = encrypted_intents
        .iter()
        .map(|i| (blake3::hash(i).as_bytes().to_vec(), i.clone()))
        .collect();
    
    // Deterministic shuffle via slot seed
    for (seed, intent) in &mut intents_with_seed {
        seed.extend_from_slice(&slot_seed);
    }
    intents_with_seed.sort_by_key(|(s, _)| blake3::hash(s).as_bytes().to_vec());
    
    // 3. Execute intents in order
    let mut txs = Vec::new();
    for (_, encrypted) in intents_with_seed {
        let plaintext = threshold_kyber_decrypt(&encrypted, committee_key_shares, threshold)?;
        let intent = OrderIntent::from_cbor(&plaintext)?;
        
        let tx = match intent {
            OrderIntent::Swap(swap) => execute_swap_intent(&swap).await?,
            OrderIntent::LimitOrder(limit) => execute_limit_order(&limit).await?,
            OrderIntent::LiquidityOp(lp) => execute_liquidity_op(&lp).await?,
            _ => continue,
        };
        txs.push(tx);
    }
    
    Ok(txs)
}
```

---

## Script Development

### Script VM Opcodes and Gas Costs

```rust
pub enum Opcode {
    // Stack operations (1 gas each)
    Push(Vec<u8>),
    Pop,
    Dup,
    Swap,
    
    // Arithmetic (5 gas each)
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    
    // Cryptographic (50-200 gas)
    CheckSigPqc,         // 150 gas (ML-DSA-65, FIPS 204)
    CheckSigPQC,         // 200 gas (post-quantum)
    Hash256,             // 50 gas
    Hash512,             // 75 gas
    
    // Introspection (10 gas each)
    InputValue,          // Read input amount
    InputScript,         // Read input locking script
    OutputValue,         // Read output amount
    OutputScript,        // Read output locking script
    InputCount,
    OutputCount,
    
    // Control flow (5 gas)
    If,
    IfElse,
    EndIf,
    Verify,              // Asserts top of stack is true; fails if false
}
```

### Simple Pay-to-PubKey-Hash (P2PKH_PQC)

```rust
use qv_script::{Script, Opcode};

pub fn create_p2pkh_pqc_script(pubkey_hash: &[u8; 32]) -> Script {
    Script::new()
        .push(pubkey_hash)
        .push(&1)  // Dummy signature index
        .opcode(Opcode::InputScript)
        .opcode(Opcode::Hash256)
        .opcode(Opcode::Equal)
        .push(&0)  // Index of witness signature
        .push(&1)  // Message to sign
        .opcode(Opcode::CheckSigPQC)
        .opcode(Opcode::Verify)
}

// Gas cost: ~250 (1 hash256 + 1 check_sig_pqc)
```

### Constant-Product AMM Invariant Covenant

```rust
pub fn create_amm_swap_script(pool_id: &[u8; 32]) -> Script {
    Script::new()
        // Load old reserves (from current UTXO datum)
        .push(&0)  // Redeemer index
        .opcode(Opcode::InputValue)  // [old_x_amount, old_y_amount]
        
        // Load new reserves (from output UTXO datum)
        .push(&0)  // Output index (must be pool UTXO)
        .opcode(Opcode::OutputValue)  // [new_x_amount, new_y_amount]
        
        // Verify invariant: new_x * new_y >= old_x * old_y * (1 - fee)
        .opcode(Opcode::Mul)  // new_x * new_y
        .swap()
        .opcode(Opcode::Mul)  // old_x * old_y
        .push(&9970)  // (10000 - fee_30bps) / 10000 ≈ 0.997
        .opcode(Opcode::Mul)
        .opcode(Opcode::GreaterOrEqual)
        .opcode(Opcode::Verify)
}

// Gas cost: ~60 (3 mul + 1 mul + 1 compare + 1 verify)
```

### Multisig Script (2-of-3)

```rust
pub fn create_multisig_pqc_script(
    pubkeys: &[[u8; 32]; 3],
    threshold: usize,
) -> Script {
    Script::new()
        // Collect signatures from witness
        .push(&3)  // Number of keys
        .push(&threshold)  // Threshold
        
        // Check each signature
        for (i, pubkey) in pubkeys.iter().enumerate() {
            Script::new()
                .push(pubkey)
                .push(&(i as u8))
                .opcode(Opcode::CheckSigPQC)
        }
        
        // Verify threshold met
        .opcode(Opcode::GreaterOrEqual)
        .opcode(Opcode::Verify)
}
```

### Custom Script Template: Simple Timelock

```rust
pub fn create_timelock_script(
    recipient_key_hash: &[u8; 32],
    unlock_height: u64,
) -> Script {
    Script::new()
        // Get current block height (introspection)
        .opcode(Opcode::BlockHeight)
        .push(&unlock_height)
        .opcode(Opcode::GreaterOrEqual)
        .opcode(Opcode::Verify)
        
        // Standard P2PKH check
        .push(recipient_key_hash)
        .push(&0)
        .opcode(Opcode::InputScript)
        .opcode(Opcode::Hash256)
        .opcode(Opcode::Equal)
        .opcode(Opcode::Verify)
}

// Gas cost: ~200
```

---

## Wallet SDK Integration

### Creating Transactions with TxBuilder

```rust
use qv_wallet::TxBuilder;
use qv_core::value::Value;

let tx = TxBuilder::new()
    // Add inputs (UTXOs to spend)
    .input(
        outpoint_1,  // Previous transaction hash + index
        Some(Redeemer::Swap),  // Optional redeemer for script
    )
    .input(outpoint_2, None)
    
    // Add outputs
    .output(
        Value::satoshis(50_000)
            .add(token_id, 100_000),
        recipient_script,
        None,  // No datum
    )
    .output_with_datum(
        Value::zero(),
        Value::satoshis(100_000),
        pool_script,
        Datum::from_cbor(&new_pool_state)?,
    )
    
    // Set fee (absolute satoshis)
    .fee(5_000)
    
    // Optional: set validity window
    .valid_from(current_height)
    .valid_until(current_height + 50)
    
    // Build
    .build()?;
```

### Signing Transactions

```rust
use qv_crypto::{SecretKey, SignatureType};

let secret_key = SecretKey::from_seed(&seed_bytes)?;

// Sign with ML-DSA-65 (post-quantum, FIPS 204)
let signed_tx = tx.sign(&secret_key, SignatureType::MlDsa)?;

// Witness format (attached to transaction)
// struct Witness {
//     signatures: Vec<Vec<u8>>,  // ML-DSA sigs (~3309 bytes each, ML-DSA-65)
//     scripts: Vec<Vec<u8>>,     // Locking scripts for each input
// }
```

### Building Stealth Address Transactions

```rust
use qv_privacy::stealth::{StealthAddress, generate_stealth};

// Recipient generates stealth address (one-time per transaction)
let (stealth_addr, view_secret) = generate_stealth(&recipient_public_key)?;

let tx = TxBuilder::new()
    .input(funding_utxo, None)
    .output(
        Value::satoshis(100_000),
        stealth_addr.to_script()?,
        None,
    )
    .fee(5_000)
    .build()?;

let signed = tx.sign(&sender_key, SignatureType::MlDsa)?;
submit_transaction(&signed).await?;

// Recipient can decrypt stealth data using view_secret
// (off-chain, not on-chain)
```

---

## Example Flows

### Complete AMM Swap Flow

```rust
async fn execute_amm_swap(
    pool_id: &str,
    user_key: &SecretKey,
    token_in: AssetId,
    amount_in: u64,
    min_amount_out: u64,
) -> Result<String> {
    // 1. Fetch pool and user UTXOs
    let pool_utxo = fetch_pool_utxo(pool_id).await?;
    let pool_datum = pool_utxo.datum::<PoolDatum>()?;
    let user_utxo = fetch_user_utxo(&user_key, token_in).await?;
    
    // 2. Compute output
    let amount_out = compute_swap_output(&pool_datum, amount_in, token_in)?;
    if amount_out < min_amount_out {
        return Err(format!(
            "Slippage: expected {}, got {}",
            min_amount_out, amount_out
        ));
    }
    
    // 3. Update pool datum
    let mut new_pool = pool_datum.clone();
    if token_in == pool_datum.token_a {
        new_pool.reserve_a += amount_in;
        new_pool.reserve_b -= amount_out;
    } else {
        new_pool.reserve_a -= amount_out;
        new_pool.reserve_b += amount_in;
    }
    
    // 4. Build transaction
    let user_address = user_key.public_key().to_script()?;
    let pool_script = create_pool_script(&pool_datum)?;
    
    let tx = TxBuilder::new()
        .input(pool_utxo.outpoint(), Some(Redeemer::Swap))
        .input(user_utxo.outpoint(), None)
        .output_with_datum(
            Value::zero(),
            Value::zero()
                .add(pool_datum.token_a, new_pool.reserve_a)
                .add(pool_datum.token_b, new_pool.reserve_b),
            pool_script,
            Datum::from_cbor(&new_pool)?,
        )
        .output(
            Value::satoshis(0)
                .add(
                    if token_in == pool_datum.token_a {
                        pool_datum.token_b
                    } else {
                        pool_datum.token_a
                    },
                    amount_out,
                ),
            user_address,
            None,
        )
        .fee(5_000)
        .build()?;
    
    // 5. Sign and submit
    let signed = tx.sign(&user_key, SignatureType::MlDsa)?;
    let tx_id = submit_transaction(&signed).await?;
    
    println!("Swap tx {}: {} -> {} (out={})", tx_id, token_in, 
             if token_in == pool_datum.token_a { "B" } else { "A" },
             amount_out);
    
    Ok(tx_id)
}
```

---

## Security Considerations

### Covenant Safety

**Issue:** Script can enforce invariants but cannot prevent reentrancy at higher layers.

**Mitigation:**
- UTXO model prevents re-entrancy by design (each UTXO is spent once)
- However, **ordering attacks** are possible in mempool
  - Solution: encrypted intents (ADR-003)

### Front-Running Protection

In traditional account-based systems, validators can reorder transactions for profit. QuantumVault mitigates this via:

1. **Encrypted Mempool:** Transactions encrypted until slot leader decrypts
2. **Deterministic Batching:** Once decrypted, ordering is fixed by slot seed (no validator choice)
3. **MEV Burn:** Any MEV extracted is distributed to validators fairly

```rust
// Encrypted intent → no front-runner can see it until committed
const MAX_SLIPPAGE_BPS: u16 = 50;  // 0.5% max slippage tolerance
let min_out = (amount_out * (10000 - MAX_SLIPPAGE_BPS)) / 10000;
```

### Datum Validation

**Issue:** Datum is user-provided; script must validate cryptographically.

**Pattern:**
```rust
// Bad: Trust datum without verification
let pool: PoolDatum = unsafe_deserialize(&datum)?;

// Good: Verify datum against Merkle root or hash
let expected_root = blake3::hash(&datum.to_cbor()?);
if expected_root != script_param_root {
    return Err("Datum mismatch");
}
```

### Precision Loss in Fixed-Point Math

Swaps use integer arithmetic to avoid rounding errors:

```rust
// Bad: Float arithmetic
let out = (input * pool.reserve_b) / pool.reserve_a;  // Precision loss

// Good: Integer math with scaling
// Use u128 internally, scale by 1e18
let scaled_input = input as u128 * 1_000_000_000_000_000_000;
let scaled_out = (scaled_input * pool.reserve_b as u128) / pool.reserve_a as u128;
let out = (scaled_out / 1_000_000_000_000_000_000) as u64;
```

---

## Testing on Devnet

### Starting a Local Devnet

```bash
cd devnet
bash scripts/genesis.sh
docker-compose up -d

# Verify nodes are running
curl http://localhost:9944/health
curl http://localhost:5001/health  # Faucet
```

### Getting Testnet Funds

```bash
# Use faucet to get tokens
curl -X POST http://localhost:5001/drip \
  -H "Content-Type: application/json" \
  -d '{"address": "devnet1qv...alice...", "amount": 1000000000}'

# Returns: {"tx_id": "abc123...", "amount": 1000000000}
```

### Submitting Test Transactions

```rust
#[tokio::test]
async fn test_amm_swap() {
    let client = NodeClient::connect("http://localhost:9944").await.unwrap();
    
    // Get initial balance
    let initial_balance = client.get_balance(&user_address).await.unwrap();
    assert!(initial_balance > 0);
    
    // Execute swap
    let tx_id = execute_amm_swap(
        "pool_abc",
        &user_key,
        token_a,
        100_000,
        90_000,  // Min output
    ).await.unwrap();
    
    // Wait for finality
    sleep(Duration::from_secs(105)).await;  // Wait for k=50 finality
    
    // Verify final state
    let final_balance = client.get_balance(&user_address).await.unwrap();
    assert!(final_balance < initial_balance);  // Spent tokens + fee
}
```

### Querying Pool State

```bash
# Get pool UTXO
curl -s http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "qv_getUtxo",
    "params": ["<pool_outpoint>"],
    "id": 1
  }' | jq '.result | .datum'
```

---

## References

- **crate qv-defi:** AMM, Lending, Oracle, Intent modules
- **crate qv-script:** Script VM, Opcode definitions, Templates
- **crate qv-wallet:** TxBuilder, Key management
- **crate qv-mempool:** Encrypted mempool interface
- **ADR-002:** UTXO + Covenants DeFi architecture
- **ADR-003:** MEV protection via encrypted mempool
- **DEVNET.md:** Devnet setup and operations
- **Cardano Plutus Docs:** Reference for eUTXO patterns

---

**Last Updated:** 2026-05-06
