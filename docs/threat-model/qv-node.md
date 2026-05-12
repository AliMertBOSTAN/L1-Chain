# Threat Model: qv-node

**Module**: Full node orchestration, RPC, block sync, state management  
**Public API**: `Node`, `RpcServer`, `BlockEvent`, chain state queries  
**Threat Count**: 7 (1 Critical, 2 High, 4 Medium)

---

## Assets & Trust Boundaries

### Assets
1. **Node liveness** — able to accept new blocks + validate consensus
   - Availability: CRITICAL (downtime = loss of sync)
2. **RPC integrity** — responses accurately reflect chain state
   - Integrity: CRITICAL (false balances = incorrect app logic)
3. **State machine safety** — blocks applied atomically
   - Consistency: CRITICAL (partial apply = divergence)
4. **Peer trust** — honest block propagation
   - Availability: CRITICAL (all-evil peers = partition)

### Trust Boundaries
- **Input**: Blocks from P2P network (qv-net), RPC requests from clients
- **Processing**: Block validation, UTXO updates, finality tracking
- **Output**: RPC responses, block events to wallets
- **Attacker**: Network (blocks), RPC clients (queries)

---

## STRIDE Threat Matrix

| Threat | STRIDE | Severity | Status | Mitigation |
|--------|--------|----------|--------|------------|
| 1. RPC account enumeration (leak user keys) | Information Disclosure | Critical | Mitigated | RPC is unauthenticated; should not expose keys |
| 2. Node crash on malformed block (panic) | Denial of Service | High | Mitigated | `#![forbid(unsafe_code)]` + clippy `unwrap_used/expect_used/panic`/`indexing_slicing/integer_division/float_arithmetic` "deny" — yapısal olarak panic-free; fuzz target `node_integration.rs` mevcut |
| 3. Block sync gap (missed blocks, stalled) | Denial of Service | High | Mitigated | Retry logic, peer rotation, backpressure |
| 8. Node shutdown ungraceful (in-flight tx loss) | Denial of Service | Medium | Mitigated | N-04 closed 2026-05-06: `Node::shutdown` gossip kanalı kapatır + tip/mempool snapshot ile graceful flush |
| 4. RPC result corruption (wrong balance returned) | Tampering | Medium | Mitigated | RPC layer validates chain state before response |
| 5. State machine divergence (apply != revert) | Tampering | Medium | Mitigated | apply/revert are tested as inverse operations |
| 6. Rate limit bypass (RPC exhaustion) | Denial of Service | Medium | Mitigated | RPC rate limiting per IP/method |
| 7. Metric poisoning (false node metrics reported) | Tampering | Medium | Partial | Metrics are internal; not consensus-critical |

---

## Detailed Threat Analysis (Abbreviated)

### Threat 1: RPC Account Enumeration (Critical)
- **Scenario**: Attacker queries RPC to enumerate all accounts + balances on node
- **Impact**: If node stores unencrypted keys, attacker gains access
- **Status**: Mitigated — RPC does not expose private keys; balances are public
- **Mitigation**: Wallet is separate from node; keys stored locally with encryption

### Threat 2: Node Crash on Malformed Block (High)
- **Scenario**: Attacker sends malformed block; node panics
- **Impact**: DoS; node down; consensus participation halted
- **Status**: Partial — All error paths should return error (not panic); fuzz testing required
- **Mitigation**: Block validation rejects structurally invalid blocks; panic-free code

### Threat 3: Block Sync Gap (High)
- **Scenario**: Node misses blocks during sync; state becomes stale
- **Impact**: Consensus lagging; RPC returns out-of-date balances
- **Status**: Mitigated — Retry logic; peer rotation; backpressure on incoming blocks
- **Mitigation**: Block sync has timeout + retry; request from multiple peers

### Threat 4–7: Covered briefly
- **RPC corruption**: Validated before response
- **State divergence**: apply/revert are tested
- **Rate limit bypass**: RPC enforces limits per IP
- **Metric poisoning**: Internal; not consensus-critical

---

## Testing Strategy

- ✅ Block sync: request, validate, apply to state
- ✅ RPC: balance query, tx submission, chain tip
- ✅ Node lifecycle: start, sync, shutdown gracefully
- [x] Fuzz: `node_integration.rs` — block stream → apply (no panic, state consistent)

---

## Audit Checklist

- [ ] Block validation happens before state update (no partial applies)
- [ ] RPC does not expose unencrypted private keys
- [ ] Rate limiting is enforced per RPC endpoint
- [ ] Node gracefully handles peer disconnection (reconnect logic)
- [ ] Metrics do not leak sensitive information
- [ ] Block sync timeout prevents indefinite waiting
- [ ] State machine is atomic (apply + revert are inverses)

---

## References

- `crates/qv-node/src/node.rs` — Node main loop
- `crates/qv-node/src/rpc.rs` — RPC server
- `crates/qv-node/src/config.rs` — Node configuration
