# QuantumVault

> Quantum-resistant, UTXO-based, client-side-validated Layer 1 blockchain
> with DeFi primitives and encrypted mempool.

**Status:** Pre-alpha. Architecture frozen; implementation in progress.

## What

QuantumVault is an L1 blockchain designed for the post-quantum era. It
combines:

- **Post-quantum cryptography**: Dilithium/ML-DSA signatures, X25519+Kyber
  hybrid KEM for network handshakes. Every classical primitive is paired
  with a PQC counterpart as insurance against NIST surprises.
- **UTXO + client-side validation**: the L1 core never runs smart contracts.
  It only verifies signatures, double-spends, and locking-script satisfaction.
- **Ouroboros Praos PoS**: pure Nakamoto-style proof-of-stake with VRF slot
  leader selection, 2-second slots, k-deep finality.
- **DeFi via shared UTXO pattern** (Cardano eUTXO–inspired): pools are single
  UTXOs whose locking scripts enforce invariants (x·y=k for AMMs, etc.).
- **Encrypted mempool**: transactions are threshold-encrypted to a validator
  committee, decrypted only at block production — MEV's attack surface shrinks
  to zero.
- **Privacy as opt-in**: stealth addresses by default, confidential amounts
  as an explicit "privacy mode".

For the full rationale, read [`docs/ABSTRACT.md`](docs/ABSTRACT.md).

## Quick start

Requires Nix with flakes enabled.

```bash
nix develop           # enters devshell with rustc, liboqs, rocksdb, tools
just build            # cargo build --workspace
just test             # cargo nextest run
just ci               # full local CI gate
```

Without Nix:

```bash
# Install Rust 1.78+ and native deps: clang, pkg-config, libssl, liboqs, rocksdb
cargo build --workspace
cargo test --workspace
```

## Repository layout

```
crates/
  qv-common/       shared types
  qv-crypto/       hash, PQC sign, hybrid KEM, VRF, KES, threshold
  qv-core/         UTXO, Transaction, Block, Merkle
  qv-script/       Script VM (stack-based, introspection, covenants)
  qv-consensus/    Ouroboros Praos
  qv-privacy/      stealth addresses, opt-in confidential amounts
  qv-storage/      RocksDB-backed stores
  qv-net/          libp2p transport + gossip
  qv-mempool/      clear + encrypted pool, batcher
  qv-defi/         AMM, lending, oracle primitives
  qv-node/         full node binary
  qv-wallet/       CLI wallet
  qv-miner/        stake pool operator
docs/              design docs and ADRs
archive/cpp-v1/    the original C++ implementation, kept for reference
```

## Design documents

- [`docs/ABSTRACT.md`](docs/ABSTRACT.md) — project overview, audience, rationale
- [`docs/ARCHITECTURE_V2.md`](docs/ARCHITECTURE_V2.md) — current architecture
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — production roadmap & open-work inventory (historical MASTER_PLAN archived under `archive/docs-v1/`)
- [`docs/ADR/`](docs/ADR/) — architecture decision records

## Contributing

See [`CLAUDE.md`](CLAUDE.md) for coding conventions, branch policy, and CI gates.
All changes touching consensus rules or cryptography require an ADR.

## License

Apache-2.0 OR MIT, at your option.
