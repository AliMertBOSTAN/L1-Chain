# QuantumVault L1 — Performance Targets

**Phase**: AŞAMA 14 — Security Hardening  
**Baseline Date**: 2026-04-27 (pre-mainnet)  
**Target**: Production-grade performance on commodity hardware

---

## Performance Targets Table

| Operation | Baseline | 2026 Goal | 2027 Stretch | Notes |
|-----------|----------|-----------|--------------|-------|
| **Block Validation** |
| Block header validation | <10ms | <10ms | <5ms | Version, slot, height, VRF check |
| Block body parsing (50 txs) | <50ms | <50ms | <30ms | Deserialization + structure checks |
| Merkle root computation (50 txs) | <20ms | <20ms | <10ms | Binary tree hash |
| UTXO set apply (50 inputs) | <100ms | <100ms | <50ms | Insert/remove from BTreeMap |
| **Total block validation** | **<200ms** | **<200ms** | **<100ms** | Median latency (50 tx block) |
| Block validation (p99) | <500ms | <500ms | <250ms | Worst-case latency |
| **Transaction Signature Verification** |
| Single Dilithium sig verify | <1ms | <1ms | <0.5ms | PQC signature only |
| Throughput (batched) | >1000 ops/sec | >1000 ops/sec | >5000 ops/sec | Parallel sig verify |
| **Cryptographic Operations** |
| SHA3-256 (1KB) | <0.1ms | <0.1ms | <0.05ms | Hash throughput |
| BLAKE3 (1KB) | <0.05ms | <0.05ms | <0.03ms | Hash throughput |
| Hybrid KEM encapsulate | <1ms | <1ms | <0.5ms | X25519 + Kyber |
| Hybrid KEM decapsulate | <1ms | <1ms | <0.5ms | X25519 + Kyber |
| **UTXO & State Persistence** |
| UTXO commitment root (1M entries) | <100ms | <100ms | <50ms | BTreeMap iteration + hash |
| Block store insert | <10ms | <10ms | <5ms | RocksDB write |
| Block store query by height | <1ms | <1ms | <1ms | RocksDB indexed lookup |
| UTXO set snapshot | <500ms | <500ms | <250ms | Full UTXO duplication |
| **Script VM** |
| Script decode (1KB) | <0.5ms | <0.5ms | <0.3ms | Opcode parsing |
| Script execute (p2pkh_pqc) | <5ms | <5ms | <2ms | Simple script |
| Script execute (multisig, 10 keys) | <50ms | <50ms | <20ms | Complex script |
| Opcode throughput | >50k ops/ms | >50k ops/ms | >100k ops/ms | Per-opcode execute time |
| **Network** |
| Gossip publish (1KB block) | <1ms | <1ms | <0.5ms | Local broadcast |
| Peer discovery (find 10 peers) | <100ms | <100ms | <50ms | Kademlia DHT query |
| Connection establish (Noise) | <50ms | <50ms | <20ms | Handshake + TLS |
| Message rate limit check | <0.1ms | <0.1ms | <0.05ms | Per-message overhead |
| **Mempool** |
| Clear pool insert (1KB tx) | <5ms | <5ms | <2ms | Fee sorting |
| Double-spend detection | <1ms | <1ms | <0.5ms | Spent outputs check |
| Deterministic sort (100 txs) | <10ms | <10ms | <5ms | Fee-based ordering |
| Encrypted pool decrypt (threshold) | <50ms | <50ms | <20ms | Threshold KEM + Kyber |
| **Privacy** |
| Stealth address scan (1000 outputs) | <100ms | <100ms | <50ms | View-tag filter + KEM |
| Confidential amount proof verify | <10ms | <10ms | <5ms | Bulletproof (opt-in) |
| **Full Node Sync** |
| Block download (100 blocks, 1MB total) | <500ms | <500ms | <250ms | P2P gossip |
| Block validation pipeline (100 blocks) | <10s | <10s | <5s | Sequential validate + apply |
| State sync (1M UTXO snapshot) | <2s | <2s | <1s | Snapshot download + load |
| **RPC** |
| Get balance query | <1ms | <1ms | <1ms | UTXO set lookup |
| Get transaction | <0.5ms | <0.5ms | <0.5ms | Block store query |
| Submit transaction | <5ms | <5ms | <2ms | Mempool insert + broadcast |
| Subscribe to blocks | <1ms | <1ms | <1ms | Gossip subscription |

---

## Hardware Assumptions

- **CPU**: 2-core Xeon E5 equivalent (~2.5 GHz, single-threaded performance)
- **RAM**: 8 GB minimum, 32 GB recommended
- **Storage**: SSD (RocksDB optimized for 100MB–1GB blocks)
- **Network**: Gigabit Ethernet (1 Gbps)

---

## Scalability Targets

| Dimension | Current | 2026 Goal | 2027 Stretch | Bottleneck |
|-----------|---------|-----------|--------------|------------|
| **Throughput** |
| Transactions per second | 1000 tx/s | 5000 tx/s | 10000 tx/s | Block validation + script VM |
| Blocks per second | 0.5 (2s slot) | 0.5 (same) | 1.0 (1s slot) | Network latency |
| **Latency** |
| Median confirmation time | 100s (50 blocks) | 100s (same) | 50s (25 blocks) | Consensus finality |
| Block propagation (p99) | <1s | <1s | <0.5s | Network traversal |
| **Storage** |
| Chain size (1 year, 1KB blocks) | ~32 GB | ~150 GB | ~300 GB | Block storage |
| UTXO set (10M entries) | ~2 GB | ~10 GB | ~20 GB | RocksDB overhead |
| **State** |
| Accounts (addressable) | ∞ (UTXO) | ∞ | ∞ | No account model |
| Contracts | Unlimited | Unlimited | Unlimited | Script templates |

---

## Profiling & Measurement

### Benchmarks

```bash
# In-crate benchmarks (criterion)
cargo bench -p qv-crypto -- sha3_256
cargo bench -p qv-script -- script_execute
cargo bench -p qv-consensus -- leader_election

# Comprehensive benchmark suite
cargo bench --all --release
```

### Flamegraphs

```bash
# Install perf + flamegraph tools
cargo install flamegraph

# Profile a specific binary
cargo flamegraph --bin qv-node -- --profile perf

# Analyze output
open flamegraph.svg
```

### Memory Profiling

```bash
# Valgrind (Linux)
valgrind --leak-check=full --show-leak-kinds=all cargo run --release

# Heaptrack (Linux)
heaptrack cargo run --release
heaptrack_gui heaptrack.<pid>.gz
```

---

## Testing Infrastructure

### Automated Benchmarking

Weekly runs on dedicated hardware:

```yaml
# .github/workflows/bench.yml
- name: Run benchmarks
  run: cargo bench --all --release
- name: Compare against baseline
  run: |
    python3 scripts/compare_bench.py \
      baseline.json results.json \
      --threshold 10%  # Fail if >10% regression
```

### Load Testing

```bash
# Generate load with jq-based RPC calls
for i in {1..10000}; do
  curl -X POST http://localhost:8080/rpc \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","id":'$i',"method":"qv_getBalance","params":["<addr>"]}' &
done
wait
```

---

## Known Bottlenecks & Optimization Roadmap

### Current Bottlenecks

1. **Block validation latency** (200ms) — Merkle root computation, signature verification
   - Mitigation: Parallel sig verification, batched hashing (2026)
2. **UTXO commitment** (100ms for 1M entries) — BTreeMap iteration + SHA3
   - Mitigation: Merkle-Patricia tree or accumulator (2027)
3. **Script VM execution** (5–50ms per script) — Opcode dispatch, gas metering
   - Mitigation: JIT compilation or bytecode caching (2027)
4. **RocksDB write amplification** — Compaction pauses >100ms
   - Mitigation: Tune RocksDB options, use faster SSDs (2026)

### Optimization Priorities

- **P0** (Critical for mainnet): Block validation <500ms p99
- **P1** (Important): Transaction throughput >1000 tx/s
- **P2** (Nice-to-have): UTXO commitment <50ms
- **P3** (Future): Full-node sync <5 minutes

---

## Regression Detection

### CI Thresholds

- **Block validation time** increases >10% → Fail
- **Signature verify throughput** decreases <900 ops/sec → Fail
- **UTXO commitment** increases >150ms → Warn

### Automated Alerts

```yaml
# If benchmark degrades:
- name: Alert on regression
  if: ${{ steps.bench.outputs.regression > 10 }}
  run: |
    gh issue create --title "Performance regression detected" \
      --body "See benchmark results: ${{ steps.bench.outputs.report_url }}"
```

---

## References

- [`benches/`](.) — Criterion benchmark suite
- [Criterion.rs Docs](https://bheisler.github.io/criterion.rs/book/) — Benchmarking framework
- [Flamegraph Guide](https://www.brendangregg.com/flamegraphs.html) — CPU profiling
- [RocksDB Tuning](https://github.com/facebook/rocksdb/wiki/RocksDB-Tuning-Guide) — Database optimization

---

**Last Updated**: 2026-04-27  
**Maintained By**: Security Team  
**Contact**: alimert930@gmail.com
