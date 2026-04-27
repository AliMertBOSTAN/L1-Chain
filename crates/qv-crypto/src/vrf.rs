//! Verifiable Random Function (VRF) for Ouroboros Praos slot-leader election.
//!
//! # Status: skeleton, pending ADR
//!
//! Two candidate implementations are being evaluated:
//!
//! 1. **Ristretto255-based VRF** (IETF `draft-irtf-cfrg-vrf-15`)
//!    - Well-studied, production in Cardano, Polkadot.
//!    - NOT post-quantum (ristretto rests on DL on Curve25519).
//!    - Fast proofs (~96 bytes), fast verification.
//!
//! 2. **Lattice-based VRF** (academic; e.g. LB-VRF or CRYSTALS-style)
//!    - Post-quantum secure.
//!    - Larger proofs (several KB), slower.
//!    - Fewer audited reference implementations.
//!
//! We are tracking this decision in `docs/ADR/` (to be written as ADR-004).
//! For the MVP we intend to ship Ristretto VRF and layer a PQC VRF on top
//! once production-quality lattice VRF libraries exist — consistent with
//! the opt-in confidentiality philosophy (hybrid now, pure-PQC later).
//!
//! # Intended API (reserved)
//!
//! ```text
//! pub struct VrfKeyPair;
//! pub struct VrfProof(Vec<u8>);
//! pub struct VrfOutput([u8; 32]);
//!
//! pub fn generate() -> VrfKeyPair;
//! pub fn evaluate(sk: &SecretKey, msg: &[u8]) -> (VrfOutput, VrfProof);
//! pub fn verify(pk: &PublicKey, msg: &[u8], proof: &VrfProof) -> Result<VrfOutput>;
//! ```

// Placeholder types and functions will be added when ADR-004 lands.
