# QuantumVault L1 — Incident Response Runbook

**Status**: AŞAMA 14 — Security Hardening  
**Severity Levels**: P0 (Critical), P1 (High), P2 (Medium), P3 (Low)  
**Activation**: Any confirmed security incident

---

## Incident Classification

| Incident | Severity | MTTD | MTTR | Escalation |
|----------|----------|------|------|------------|
| Consensus halted (no new blocks) | P0 | <1min | <30min | CEO + Core Team |
| Reorg > k blocks detected | P0 | <5min | <2h | SEC + Exchange Partners |
| RPC returning false balances | P0 | <5min | <1h | Node Operators |
| Memory DoS (node crash on malformed tx) | P1 | <5min | <1h | Dev Team |
| RPC rate limit bypass (API abuse) | P1 | <10min | <2h | Ops Team |
| Validator key compromise (single pool) | P1 | <15min | <4h | Key Management Team |
| Mempool censorship (transactions blocked) | P2 | <30min | <4h | Network Ops |
| Slow block validation (>1s latency) | P2 | <30min | <2h | Performance Team |
| Stealth address privacy leak | P2 | <1h | <4h | Privacy Team |
| Oracle price manipulation | P2 | <1h | <2h | DeFi Team |

---

## P0: Consensus Halt

**Symptom**: No new blocks for >2 slots (>4 seconds)

### Detection

```bash
# Monitor block production
watch -n 1 'curl -s http://localhost:8080/rpc -d "{\"method\":\"qv_chainTip\"}" | jq .result.height'

# Alert if height unchanged for >10 seconds
```

### Diagnosis (First 5 minutes)

1. **Check network connectivity**: Are other nodes producing blocks?
   ```bash
   # Connect to 5 trusted nodes
   for peer in peer1.testnet peer2.testnet peer3.testnet; do
     curl http://$peer:8080/rpc -d '{"method":"qv_chainTip"}' | jq .result.height
   done
   ```

2. **Check slot clock**: Is local time synchronized?
   ```bash
   ntpstat  # Should show "synchronized"
   timedatectl  # Check time sync
   ```

3. **Check logs for panics**:
   ```bash
   grep -i panic /var/log/qv-node.log | tail -20
   ```

4. **Check validator status** (if running miner):
   ```bash
   curl http://localhost:9090/metrics | grep validator_slots_elected
   ```

### Containment (5–30 minutes)

**If consensus is broken (not hardware issue)**:

1. **Pause automated trading** on DeFi
   - Send emergency signal to DEX to halt swaps
   - Command: `curl -X POST http://defi.localhost/admin/pause`

2. **Notify validators** immediately
   - Slack: `@validators Network halt detected. Status: INVESTIGATING`
   - Email: `validators@quantumvault.com`

3. **Spin up recovery node**
   - Clone state from known-good backup
   - Verify block height matches network

**If hardware failure**:

1. **Failover to standby node**
   - Point RPC DNS to backup (`node2.quantumvault.com`)
   - Monitor syncing

2. **Investigate failed hardware**
   - Check disk: `smartctl -a /dev/sda`
   - Check memory: `memtest86`

### Recovery (30 minutes – 2 hours)

1. **Restart node** with clean state:
   ```bash
   systemctl stop qv-node
   rm -rf /var/lib/qv-node/rocksdb  # WARNING: wipes UTXO set
   systemctl start qv-node
   # Node will resync from peers
   ```

2. **If restart fails, check logs**:
   ```bash
   journalctl -u qv-node -n 100 --no-pager
   ```

3. **Roll back to known-good version**:
   ```bash
   git checkout v1.2.3  # Last known good
   cargo build --release
   systemctl restart qv-node
   ```

4. **Notify status page**:
   - Set incident status to "INVESTIGATING"
   - ETA for resolution based on root cause

### Communication

- **First update** (5 min): "Consensus monitoring alert triggered. Investigating."
- **Second update** (15 min): Root cause identified + mitigation in progress
- **Final update** (after recovery): "Network resumed at block N. Post-mortem scheduled."

---

## P0: Reorg > k Blocks

**Symptom**: Chain tip changes by >50 blocks (e.g., height 1000 → 950)

### Detection

```bash
# Monitor finality
watch -n 5 'curl -s http://localhost:8080/rpc -d "{\"method\":\"qv_finalityHeight\"}" | jq .result'

# Alert if finality_height < current_height - 50
```

### Diagnosis (First 10 minutes)

1. **Verify it's not a false alarm** (node resyncing):
   - Check if other nodes report same reorg
   - Check if reorg is continuing (height keeps dropping)

2. **Analyze the reorg**:
   ```bash
   curl -X POST http://localhost:8080/rpc \
     -d '{"method":"qv_getBlock","params":{"height":1000}}' | jq .result.hash
   
   curl -X POST http://localhost:8080/rpc \
     -d '{"method":"qv_getBlock","params":{"height":950}}' | jq .result.hash
   # Compare: Are these the same validator? Same epoch?
   ```

3. **Check for double-signing**:
   ```bash
   grep "double_sign" /var/log/qv-node.log | tail -5
   ```

### Containment (10–60 minutes)

1. **Pause DEX** (loss of finality is unacceptable):
   - `curl -X POST http://defi.localhost/admin/pause_all`
   - Message: "Finality compromise detected. Liquidity paused pending investigation."

2. **Notify validators** that >1/3 of stake may be attacking:
   - Slack: `@validators ALERT: Reorg >k=50 blocks (past finality depth) detected at Epoch X block Y. Potential >1/3 byzantine-stake attack (PoS analog of "51%"; in Ouroboros Praos the safety threshold is honest stake > 2/3).`

3. **Collect evidence**:
   - Save block headers for both chains
   - Identify which validators produced blocks in reorg period
   - Check if any double-signed

4. **Activate emergency procedures**:
   - If attacker is identified (double-sign), notify exchange + law enforcement
   - If reorg is due to partition, trigger network merge protocol

### Recovery (1–4 hours)

1. **Manual hard fork** (if necessary):
   - Validators vote on canonical chain (block hash)
   - Majority (>2/3) stake votes to canonicalize longer chain
   - Updated genesis config published

2. **Verify network cohesion**:
   ```bash
   # Connect to 20 nodes; check all agree on block hash at height N
   python3 scripts/verify_consensus.py --height 950
   ```

3. **Resume operations**:
   - Re-enable DEX
   - Publish post-mortem within 24h

---

## P0: RPC Balance Mismatch

**Symptom**: `qv_getBalance` returns different values on different nodes

### Detection

```bash
# Compare balance across nodes
for node in node{1,2,3}.quantumvault.com:8080; do
  echo "$node:"
  curl http://$node/rpc -d "{\"method\":\"qv_getBalance\",\"params\":[\"<address>\"]}" | jq .result
done
```

### Diagnosis (First 5 minutes)

1. **Determine which node is correct**:
   - Compare UTXO commitment root: `qv_getUtxoCommitmentRoot`
   - Node with majority is canonical

2. **Check if consensus divergence** (reorg in progress):
   - If heights differ, nodes are in different states; expected temporarily

3. **If same height + different roots**, consensus is broken:
   - Follow **P0 Consensus Halt** runbook

### Containment

1. **Withdraw from exchange addresses** (prevent false credits):
   - Disable all RPC endpoints except canonical node
   - Flush DNS cache: `sudo systemctl restart systemd-resolved`

2. **Notify exchange partners**:
   - "RPC balance discrepancy detected. Using canonical node only. Deposits/withdrawals paused."

### Recovery

1. **Resync diverged nodes**:
   ```bash
   systemctl stop qv-node  # On diverged node
   rm -rf /var/lib/qv-node/rocksdb
   systemctl start qv-node  # Resync from canonical peer
   ```

2. **Verify balance matches**:
   ```bash
   curl http://node:8080/rpc -d "{\"method\":\"qv_getBalance\",\"params\":[\"<address>\"]}"
   ```

---

## P1: Memory DoS (Node Crash)

**Symptom**: Node crashes with `out of memory` after processing malformed transaction

### Detection

```bash
# Monitor memory usage
watch -n 5 'ps aux | grep qv-node | grep -v grep'

# Alert if RSS > 90% of available
```

### Diagnosis (5–10 minutes)

1. **Identify the crashing input**:
   ```bash
   # Check node logs
   journalctl -u qv-node -n 20 --no-pager | grep -i memory
   ```

2. **Reproduce locally**:
   ```bash
   # Run with RUST_BACKTRACE=1 on a test node
   RUST_BACKTRACE=1 cargo run --release --bin qv-node < <(cat crash.tx.bin)
   ```

### Containment (10–30 minutes)

1. **Block the malicious transaction** at network level:
   - Add tx hash to mempool blacklist
   - Command: `systemctl reload qv-node` (reload blocklist)

2. **Restart affected nodes** with increased memory limits:
   ```bash
   # /etc/systemd/system/qv-node.service
   [Service]
   MemoryLimit=16G  # Increase from 8G
   ```

### Recovery

1. **Apply fix**:
   - If bounds check missing: add validation in `qv-script` or `qv-core`
   - If unbounded loop: add iteration limit + gas meter
   - Create test case with crashing input

2. **Roll out patch**:
   - Tag release `v1.2.4-hotfix`
   - Notify validators of security update
   - Orchestrate synchronized upgrade

---

## P1: Validator Key Compromise

**Symptom**: Rogue transactions signed by validator's key; double-signing detected

### Detection

```bash
# Monitor for double-signing
grep "equivocation" /var/log/qv-node.log

# OR check validator metrics
curl http://validator-1.quantumvault.com:9090/metrics | grep slashing
```

### Diagnosis (5–15 minutes)

1. **Confirm the key is compromised**:
   ```bash
   # Check if attacker is producing blocks
   curl http://localhost:8080/rpc \
     -d '{"method":"qv_getBlock","params":{"height":N}}' \
     | jq .result.producer
   ```

2. **Identify which key (VRF, KES, cold)**:
   - VRF key: Can forge slot leader proofs
   - KES key: Can only sign current epoch
   - Cold key: Can control pool registration

### Containment (15–60 minutes)

1. **Disable the compromised pool** (if KES or VRF):
   - Validator rotates out (delegation moves to other pools)
   - Command: `qv-miner --deregister --pool-id <hex>`

2. **Slash the validator** (slashing committee votes):
   - On-chain vote to penalize for double-signing
   - 10% of stake slashed if vote passes

3. **Recover the key** (if KES):
   - KES rotates at epoch boundary; old key is unusable
   - Wait for next epoch; operator continues with fresh key

### Recovery

1. **Rotate validator keys**:
   - Generate new VRF + KES keypairs
   - Update pool registration with new keys
   - Monitor for no further unauthorized signing

2. **Investigate compromised key**:
   - Check if key was stolen from disk, memory, or HSM
   - Conduct security audit of key storage

---

## P2: Mempool Censorship

**Symptom**: User transactions accepted by local mempool but never make it into blocks

### Detection

```bash
# Check if transaction is in mempool
curl http://localhost:8080/rpc \
  -d '{"method":"qv_getMempoolStatus"}' | jq '.result | length'

# Monitor if tx ever confirms (after 100 blocks)
sleep 200  # Wait 100 blocks (at 2s/block)
curl http://localhost:8080/rpc \
  -d '{"method":"qv_getTransaction","params":["<txid>"]}' | jq '.result.confirmed'
```

### Diagnosis (30 minutes)

1. **Check if validators are censoring**:
   - If all validators skip low-fee txs: Likely intentional filtering
   - If only some skip: Likely individual pool policy

2. **Verify transaction is valid**:
   ```bash
   qv-wallet verify-tx <tx.hex>
   ```

3. **Check if fee is sufficient**:
   - Minimum fee = (size_bytes * 100) satoshis
   - If below minimum: Expected behavior

### Mitigation (1–4 hours)

1. **Increase fee and resubmit**:
   ```bash
   qv-wallet rebuild-tx --input <tx> --fee-rate 200  # 2x fee
   ```

2. **Report to Ops team**:
   - If systematic: Investigate validator pool policies
   - Consider governance action (fee negotiation)

3. **Use alternative pool**:
   - Broadcast to node known to process low-fee txs
   - (Decentralization prevents monopoly on block space)

---

## P3: Performance Degradation

**Symptom**: Block validation time > 500ms (normal: 200ms)

### Detection

```bash
# Monitor block validation latency
curl http://localhost:9090/metrics | grep block_validation_ms
```

### Diagnosis (30 minutes)

1. **Identify bottleneck**:
   ```bash
   # Enable perf profiling
   cargo flamegraph --bin qv-node
   ```

2. **Common causes**:
   - Large block (>1MB): Wait for network sync
   - Script complexity: Complex script template (lending, AMM)
   - RocksDB compaction: Pause entire node; expected

### Mitigation (1–2 hours)

1. **If RocksDB compaction**: Normal; no action needed
2. **If large block**: Adjust max block size in consensus params (hard fork)
3. **If slow script**: Optimize opcodes or increase gas limit
4. **If memory pressure**: Add RAM or enable pruning

---

## General Incident Response Process

### Phase 1: Triage (0–15 minutes)

1. Confirm severity level (P0–P3)
2. Activate response team (Slack, email, SMS)
3. Open incident tracking issue (GitHub)

### Phase 2: Containment (15–120 minutes)

1. Identify root cause
2. Implement temporary fix (e.g., pause DeFi)
3. Communicate status to stakeholders

### Phase 3: Resolution (varies)

1. Apply permanent fix
2. Verify across network
3. Close incident

### Phase 4: Post-Mortem (within 48 hours)

1. Document what happened
2. Identify systemic issues
3. Update runbooks + monitoring
4. Public disclosure (if needed)

---

## Escalation Chain

```
Level 1: On-call Engineer (detects + diagnosis)
   ↓
Level 2: Team Lead (confirms severity + response)
   ↓
Level 3: Security Lead (P0 only; coordinate industry response)
   ↓
Level 4: CEO (P0 critical; public statement)
   ↓
Level 5: Legal (regulatory reporting, if needed)
```

---

## Communication Templates

### Template 1: Initial Incident Alert

```
Subject: INCIDENT: [Severity] [Type]
To: @on-call, @team-leads

Title: Consensus Halt Detected
Severity: P0 (Critical)
Detected: 2026-04-27 14:23 UTC
Duration: 5 minutes (ongoing)

Description:
No new blocks for >4 seconds. Last block: #1,000 (2026-04-27 14:18).

Impact:
- Network is halted; transactions are not confirmed
- All nodes report same chain height (no fork)

Initial Action:
- [x] Confirmed network-wide (not local)
- [x] Identified potential cause: [RocksDB compaction / validator failure]
- [ ] Patch in progress
- [ ] Waiting for [validator pool X] to restart

ETA: 30 minutes to resolution

Updates: https://status.quantumvault.com/incidents/2026-04-27-01
```

### Template 2: Resolution Notice

```
Subject: RESOLVED: Consensus Halt (4:23–14:45 UTC)

Timeline:
14:18 - Last block produced
14:23 - Alert triggered
14:25 - Root cause identified: Validator X offline due to hardware failure
14:40 - Failover to backup validator complete
14:45 - Network resumed at block #1,001

Impact:
- 22 minutes of downtime
- ~11 pending transactions were lost (submitted during halt, not confirmed)
- No blockchain corruption; state is consistent

Actions:
- [x] Validator hardware replaced + tested
- [x] Backup failover procedures validated
- [ ] Post-mortem document published (Thursday 10:00 UTC)

We apologize for the disruption. Our monitoring has been improved to detect this failure mode earlier.
```

---

## References

- [docs/threat-model/README.md](../threat-model/README.md) — Threat scenarios
- [SECURITY.md](../../SECURITY.md) — Disclosure policy
- [Monitoring Setup](./monitoring.md) — Alert configuration (future)
- [Recovery Procedures](./recovery.md) — Node recovery (future)

---

**Last Updated**: 2026-04-27  
**Maintained By**: Security + Ops Team  
**Review Cycle**: Quarterly
