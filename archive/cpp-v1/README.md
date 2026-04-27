# QuantumVault v1 — C++ Reference Implementation (Archived)

**Status:** Archived on 2026-04-15. No longer maintained.

This directory contains the original C++20 implementation of QuantumVault L1.
It is kept as a reference and for historical record. All active development
has moved to the Rust workspace at the repository root (`crates/`).

## What's here

- `src/`, `include/`, `tests/`, `tools/` — C++ source tree
- `cmake/` — CMake modules (sanitizers, coverage, static analysis)
- `CMakeLists.txt`, `CMakePresets.json` — build configuration
- `flake.nix` — C++ Nix shell
- `.clang-format`, `.clang-tidy`, `.iwyu_mapping` — style & linting
- `scripts/` — git hooks, CI scripts, format/build helpers
- `config/genesis.toml` — v1 genesis parameters
- `root-docs/` — v1 documentation artifacts

## What was completed in v1

- Full skeleton (9 modules, ~16K lines)
- **Crypto layer fully implemented:**
  - `SecureBytes` with `OPENSSL_cleanse` zeroization
  - SHA3-256 (OpenSSL EVP) + BLAKE3 (`<blake3.h>`)
  - Dilithium/ML-DSA signatures (liboqs, Level 2/3/5)
  - Hybrid KEM: X25519 + Kyber/ML-KEM + SHA3-256 transcript-bound KDF
  - NIST KAT tests (SHA3 "", "abc"), roundtrip, tamper-rejection
- Build system (Nix flake + CMake + Ninja, presets for dev/release/sanitize)
- Code quality (clang-tidy, cppcheck, sanitizers, coverage, IWYU)
- Git hooks and CI pipeline scripts
- Testing strategy documentation

## Why the pivot?

See `../../docs/ARCHITECTURE_V2.md` and `../../docs/ADR/002-defi-architecture.md`.

Summary:
1. Language → Rust (memory safety, stronger async ecosystem, PQC crate availability)
2. Consensus: hybrid PoW+PoS → Ouroboros Praos (DeFi-friendly finality)
3. State model: UTXO+CSV → UTXO + Cardano eUTXO-style datum/validator
   (Shared UTXO Pattern for DeFi primitives)
4. MEV: not addressed in v1 → encrypted mempool + threshold Kyber decryption
5. Privacy: always-on stealth → opt-in confidential amounts

The underlying **philosophy** is unchanged: PQC-first, UTXO-first,
client-side validation, Nakamoto-style security.
