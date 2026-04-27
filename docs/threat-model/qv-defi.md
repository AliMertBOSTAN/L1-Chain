# Threat Model: qv-defi

**Module**: AMM, lending, oracles, intent-based orders  
**Public API**: `PoolState`, `compute_swap_output()`, `PoolDatum`, `OrderIntent`, `OracleWindow`  
**Threat Count**: 10 (2 Critical, 3 High, 5 Medium)

---

## Assets & Trust Boundaries

### Assets
1. **AMM invariant** — x × y ≥ x' × y' (pool cannot be drained)
   - Integrity: CRITICAL (break = pool theft)
2. **Lending collateral** — borrowed coins must be repaid
   - Integrity: CRITICAL (liquidation skipped = protocol insolvency)
3. **Oracle accuracy** — price feed is not manipulated
   - Integrity: CRITICAL (bad price = liquidation/arbitrage failure)
4. **Intent atomicity** — swap must execute or fail completely
   - Consistency: CRITICAL (partial execution = loss)

### Trust Boundaries
- **Input**: User-submitted intents (untrusted) + datum with pool state
- **Processing**: Script VM validates invariants + batcher executes swaps
- **Output**: New pool state + user outputs
- **Attacker**: Pool provider, oracle provider, MEV extractor

---

## STRIDE Threat Matrix

| Threat | STRIDE | Severity | Status | Mitigation |
|--------|--------|----------|--------|------------|
| 1. AMM invariant violation (x×y broken) | Tampering | Critical | Partial | Script VM validates post-execution; batcher must ensure |
| 2. Liquidation bypass (bad debt remains) | Tampering | Critical | Partial | Liquidation logic in batcher; oracle price used |
| 3. Oracle price manipulation (attacker supplies feed) | Tampering | High | Mitigated | TWAP oracle uses validator-signed prices (future) |
| 4. Sandwich attack (order reordering for MEV) | Information Disclosure | High | Partial | Encrypted mempool + threshold decryption mitigates |
| 5. Slippage DoS (price moves between intent + execution) | Denial of Service | High | Mitigated | Intent includes min_amount_out; fails if violated |
| 6. Oracle staleness (old prices used) | Tampering | Medium | Partial | TWAP window prevents > K-block staleness |
| 7. Liquidation penalty theft (liquidator keeps excess) | Tampering | Medium | Partial | Liquidation bonus is fixed; excess returned to borrower |
| 8. Rounding error in interest accrual | Tampering | Medium | Mitigated | Checked arithmetic; dust returned to pool |
| 9. Intent cycle (A→B→C→A; creates coins) | Tampering | Medium | Mitigated | Batcher detects cycles via topological sort |
| 10. Collateral double-use (same UTXO in two intents) | Tampering | Medium | Mitigated | Mempool tracks spent outputs; prevents double-spending |

---

## Detailed Threat Analysis (Abbreviated)

### Threat 1: AMM Invariant Violation (Critical)
- **Scenario**: Script execution allows swap that breaks x×y ≥ x'×y'
- **Impact**: Pool drained; attacker gains free tokens
- **Status**: Partial — Script VM must validate; batcher ensures atomicity
- **Mitigation**: Template `amm_swap` includes AssertDatumHash covenant + invariant check in VM

### Threat 2: Liquidation Bypass (Critical)
- **Scenario**: Collateral price drops; borrower does not get liquidated; protocol becomes insolvent
- **Impact**: Bad debt accumulation; protocol bankruptcy
- **Status**: Partial — Oracle must remain accurate; liquidation enforced by batcher
- **Mitigation**: Lending pool covenant requires collateral ratio ≥ 150%; liquidation mandatory

### Threat 3: Oracle Manipulation (High)
- **Scenario**: Attacker supplies fake price to oracle; manipulates liquidation
- **Impact**: Honest borrowers unfairly liquidated; attacker benefits
- **Status**: Mitigated — Oracle uses validator-signed prices + TWAP aggregation
- **Mitigation**: Future: Each validator signs price observations; TWAP window prevents spike

### Threat 4: Sandwich Attack (High)
- **Scenario**: Attacker sees pending swap intent; inserts own swap before it; captures slippage
- **Impact**: User gets worse price; attacker extracts MEV
- **Status**: Partial — Encrypted mempool + threshold decryption + deterministic sorting
- **Mitigation**: MEV is non-zero (economics) but attacker cannot freely reorder batch

### Threat 5: Slippage DoS (High)
- **Scenario**: Price drops between intent submission + execution; swap fails
- **Impact**: User intent reverts; transaction fee wasted
- **Status**: Mitigated — Intent includes min_amount_out; fails if price too bad
- **Mitigation**: Intent validation checks `output_amount ≥ min_amount_out`

### Threats 6–10: Covered briefly
- **Oracle staleness**: TWAP window bounds staleness
- **Liquidation theft**: Fixed liquidation bonus
- **Rounding errors**: Checked arithmetic; dust to pool
- **Intent cycles**: Topological sort detects cycles
- **Collateral double-use**: Mempool prevents double-spending

---

## Testing Strategy

- ✅ AMM: swap computation, invariant validation, fee calculation
- ✅ Lending: collateral ratio, liquidation, interest accrual
- ✅ Oracle: TWAP, staleness, price aggregation
- ✅ Intents: topological sort, cycle detection, atomicity
- [x] Fuzz: `intents.rs` — random intent set → topological sort (deterministic, no cycles)

---

## Audit Checklist

- [ ] AMM invariant check happens before output release
- [ ] Lending collateral ratio is recalculated on interest accrual
- [ ] Oracle price is signed by validator (authentication)
- [ ] Intent min_amount_out is enforced (slippage limit)
- [ ] Liquidation bonus is fixed (not exploitable)
- [ ] Interest rate calculation uses safe fixed-point math
- [ ] Oracle window prevents prices >K blocks old
- [ ] Rounding always favors protocol (not users)

---

## References

- `crates/qv-defi/src/amm.rs` — Constant-product AMM
- `crates/qv-defi/src/lending.rs` — Lending pool, liquidation
- `crates/qv-defi/src/oracle.rs` — Oracle, TWAP
- `crates/qv-defi/src/intents.rs` — Intent encoding, batch execution
