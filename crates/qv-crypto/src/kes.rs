//! Key-Evolving Signatures (KES) for forward-secure block signing.
//!
//! # Status: skeleton, pending ADR
//!
//! A KES scheme lets the block producer evolve its secret key over epochs
//! so that compromise of today's key cannot forge yesterday's blocks. This
//! closes the "long-range attack" window that Nakamoto PoS is otherwise
//! vulnerable to.
//!
//! # Candidate designs
//!
//! 1. **Sum composition over Dilithium** (PQC-safe)
//!    - Build a binary-tree KES where each leaf is a fresh Dilithium
//!      keypair. Evolve = discard left subtree.
//!    - Key size grows as O(log N) where N = epoch count.
//!    - Fully post-quantum; sits on top of an already-deployed primitive.
//!
//! 2. **Cardano-style Ed25519-sum KES** (classical)
//!    - Literal port of Cardano's KES implementation. Fast, small.
//!    - Not PQC → conflicts with our hybrid philosophy.
//!
//! We'll most likely ship #1 (Dilithium-sum KES) as that matches our
//! broader stance. This decision will be captured in ADR-005.

// Placeholder types will be added once ADR-005 is written.
