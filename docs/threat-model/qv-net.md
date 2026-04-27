# Threat Model: qv-net

**Module**: P2P networking (libp2p, gossip, transport, rate limiting)  
**Public API**: `NetworkNode`, `Envelope`, `NetworkMessage`, `RateLimiter`, `PeerStore`  
**Threat Count**: 9 (1 Critical, 3 High, 5 Medium)

---

## Assets & Trust Boundaries

### Assets
1. **Network integrity** — messages not tampered with, authenticated sender
   - Integrity: CRITICAL (MITM = arbitrary messages accepted)
2. **Peer reputation** — track malicious peers, prevent Sybil attacks
   - Integrity: CRITICAL (poisoned reputation = honest peers evicted)
3. **Rate limiting** — prevent resource exhaustion from flooding
   - Availability: CRITICAL (DoS = consensus halted)
4. **Message ordering** — transitive gossip maintains causal ordering
   - Availability: MEDIUM (out-of-order = temporary forking)

### Trust Boundaries
- **Input**: Raw TCP packets from untrusted peers
- **Processing**: Noise protocol handshake, serialization, dedup cache
- **Output**: Trusted messages to consensus/mempool layers
- **Attacker control**: Full network topology (BGP hijack, Sybil nodes)

---

## STRIDE Threat Matrix

| Threat | STRIDE | Severity | Status | Mitigation |
|--------|--------|----------|--------|------------|
| 1. MITM attack (Noise cipher break) | Spoofing | Critical | Partial | Assume Noise protocol secure; certificate pinning future |
| 2. Sybil attack (unlimited peer slots) | Denial of Service | High | Mitigated | Max peer count, reputation-based eviction |
| 3. Message flood (4MB × 1000/sec) | Denial of Service | High | Mitigated | Message size limit 4MB, rate limit per peer |
| 4. Gossip loop (message rebroadcast infinitely) | Denial of Service | High | Mitigated | SeenCache dedup, short bloom filter TTL |
| 5. Peer reputation poisoning (false slander) | Tampering | Medium | Partial | Reputation is local; cannot be slandered remotely |
| 6. Connection state confusion (TCP reset spoofing) | Denial of Service | Medium | Mitigated | Noise protocol prevents spoofed close |
| 7. GossipSub topic hijacking (malicious subscribe) | Tampering | Medium | Mitigated | Topics are read-only; validator validates message content |
| 8. Peer list pollution (invalid addresses) | Denial of Service | Medium | Partial | Kademlia validates addresses; failed connects penalize peer |
| 9. Timestamp-based DoS (future-dated messages) | Denial of Service | Medium | Partial | Validators should reject messages with slot > current+k |

---

## Detailed Threat Analysis (Abbreviated)

### Threat 1: MITM Attack (Critical)
- **Scenario**: Attacker intercepts Noise handshake; decrypts all traffic
- **Impact**: Arbitrary message injection; double-spend; consensus bypass
- **Status**: Partial — Assume Noise protocol secure; no certificate pinning yet
- **Mitigation**: Noise X pattern (DH3) provides authentication; gossip validators reject unsigned messages

### Threat 2: Sybil Attack (High)
- **Scenario**: Attacker controls 1000 nodes; floods network with junk messages
- **Impact**: DoS; genuine peers evicted due to reputation confusion
- **Status**: Mitigated — Max peer count (512 default), reputation decay
- **Mitigation**: Rate limiting, peer eviction, bootstrap from trusted seed nodes

### Threat 3: Message Flood (High)
- **Scenario**: Attacker sends 1000 4MB messages/second to single peer
- **Impact**: Bandwidth exhaustion; node unable to receive legitimate messages
- **Status**: Mitigated — Message size cap 4MB, per-peer rate limit
- **Mitigation**: RateLimiter tracks bytes/sec; exceeds → rate_limited error

### Threat 4: Gossip Loop (High)
- **Scenario**: Network misconfiguration; message is rebroadcast infinitely
- **Impact**: Bandwidth waste; consensus liveness degraded
- **Status**: Mitigated — SeenCache (Bloom filter) dedup
- **Mitigation**: SeenCache TTL = 10 minutes; messages dropped if seen recently

### Threat 5: Reputation Poisoning (Medium)
- **Scenario**: Attacker sends false "peer X is evil" gossip to confuse reputation
- **Impact**: Honest peers penalized; Sybil control improves
- **Status**: Partial — Reputation is local; no global slander risk
- **Mitigation**: Reputation stored locally; no trust transfer

### Threats 6–9: Covered in brief
- **Connection spoofing**: Mitigated by Noise encryption
- **Topic hijacking**: Mitigated by validator checking message signature
- **Peer address pollution**: Partial (bad addresses penalize peer connection)
- **Future-dated messages**: Partial (validators should enforce slot recency)

---

## Testing Strategy

- ✅ Peer store: add, remove, reputation decay
- ✅ Message envelope: encode/decode, size limit, version mismatch
- ✅ Rate limiter: bytes/sec tracking, burst allowance
- ✅ Gossip dedup: SeenCache bloom filter correctness
- [x] Fuzz: `network_envelope.rs` — random bytes → Envelope::decode (no panic)

---

## Audit Checklist

- [ ] Noise cipher is from libp2p-noise (verified version)
- [ ] Max peer count prevents Sybil (actual number in config)
- [ ] Rate limit bytes/sec threshold is enforced
- [ ] Reputation decay is exponential (old scores matter less)
- [ ] SeenCache is probabilistic (false positives acceptable)
- [ ] Message version mismatch is rejected (no version upgrade without coordination)

---

## References

- `crates/qv-net/src/transport.rs` — Noise protocol config
- `crates/qv-net/src/peer.rs` — PeerStore, reputation
- `crates/qv-net/src/message.rs` — Envelope, version handling
- `crates/qv-net/src/gossip.rs` — SeenCache, GossipSub topics
- [Noise Protocol](https://noiseprotocol.org/) — Authentication + encryption
- [libp2p Documentation](https://docs.rs/libp2p/) — Transport details
