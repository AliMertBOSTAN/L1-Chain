//! Threshold cryptography for the encrypted mempool (ADR-003).
//!
//! # Status: skeleton, pending spec work
//!
//! The encrypted mempool requires a **threshold Kyber** (t-of-n) scheme
//! so that no single validator can decrypt pending transactions; only the
//! committee as a whole, collaborating, can.
//!
//! # Components we will need
//!
//! - **Shamir secret sharing** over `Fp` for scalar secrets.
//! - **Kyber DKG (distributed key generation)** that emits a set of
//!   KEM secret-key shares such that any t of n shareholders can
//!   reconstruct a decapsulation oracle.
//! - **Per-slot ephemeral committee key** that is one-shot: once
//!   recovered and used, it is discarded.
//! - **Dilithium multi-signature** so validators can attest to the
//!   batch ordering deterministically.
//!
//! # References
//!
//! - Ferveo (Anoma) — BLS-based threshold; we are **not** using this
//!   because BLS12-381 is not quantum-safe.
//! - Gentry et al., "Threshold Cryptosystems from Compact Identity-Based
//!   Key Agreement" — foundational for lattice-based threshold schemes.
//! - Open research question: robust DKG with identifiable abort at slot
//!   cadence (2 seconds). Likely we'll do DKG per epoch (12 h) and reuse
//!   the distributed secret key with per-slot ephemeral randomness.

// Placeholder types and functions will be added in a later iteration.
