//! Ouroboros Praos PoS consensus engine for QuantumVault L1.
//!
//! This crate implements the core consensus logic: slot/epoch time mapping,
//! VRF-based leader election, block validation, fork choice with k-deep
//! finality, stake pool management, and block reward distribution.
//!
//! # Module overview
//!
//! | Module               | Purpose                                              |
//! |----------------------|------------------------------------------------------|
//! | [`slot`]             | `SlotClock` — maps between slots, epochs, wall-clock |
//! | [`epoch`]            | `EpochNonce`, `EpochInfo`, boundary detection         |
//! | [`stake`]            | `StakePool`, `StakeDistribution`, delegation          |
//! | [`leader_schedule`]  | VRF leader election, `VrfEvaluator` trait             |
//! | [`block_validator`]  | Consensus-level block header validation               |
//! | [`chain_state`]      | Fork choice, `ChainState`, k-deep finality            |
//! | [`rewards`]          | Block subsidy, halving, fee distribution              |
//!
//! # Design principles
//!
//! - **Trait-based crypto**: VRF and KES are behind traits so the engine can
//!   be tested with deterministic mocks while the real primitives are finalised
//!   (ADR-004 / ADR-005).
//! - **Deterministic**: No floating-point in consensus-critical paths outside
//!   the VRF threshold comparison, which is bounded and monotonic.
//! - **Composable**: This crate consumes `qv-core` types (blocks, params) and
//!   `qv-crypto` (hashing, signatures). It does *not* touch storage or
//!   networking; those compose on top.
//!
//! # Usage
//!
//! ```text
//! // 1. Build a SlotClock from ProtocolParams
//! let clock = SlotClock::from_params(&params);
//!
//! // 2. Snapshot stake distribution at epoch boundary
//! let dist = StakeDistribution::snapshot(epoch, &pools, &delegations)?;
//!
//! // 3. Check leadership each slot
//! if let Some((output, proof)) = check_leadership(&vrf, &pool_id, &nonce, slot, &dist)? {
//!     // produce block
//! }
//!
//! // 4. Validate received blocks
//! validate_block(&block, &ctx, &vrf, &kes)?;
//!
//! // 5. Update chain state
//! chain_state.add_block(entry)?;
//! ```

#![forbid(unsafe_code)]

pub mod block_validator;
pub mod chain_state;
pub mod epoch;
pub mod leader_schedule;
pub mod rewards;
pub mod slot;
pub mod stake;

// ---------------------------------------------------------------------------
// Re-exports: stable public surface
// ---------------------------------------------------------------------------

// slot
pub use slot::{SlotClock, SlotInfo};

// epoch
pub use epoch::{EpochBoundary, EpochInfo, EpochNonce};

// stake
pub use stake::{Delegation, PoolId, StakeDistribution, StakeError, StakePool};

// leader election
pub use leader_schedule::{
    check_leadership, leader_threshold, verify_leadership, vrf_input, LeaderError, TestVrf,
    VrfEvaluator, VrfOutput, VrfProof, ACTIVE_SLOT_COEFF,
};

// block validation
pub use block_validator::{
    validate_block, validate_block_header, BlockValidationContext, BlockValidationError,
    KesVerifier, TestKesVerifier,
};

// chain state
pub use chain_state::{ChainEntry, ChainError, ChainState};

// rewards
pub use rewards::{
    block_subsidy, cumulative_emission, distribute_reward, is_emission_exhausted,
    total_block_reward, RewardError, RewardShare,
};

// ---------------------------------------------------------------------------
// Aggregate error
// ---------------------------------------------------------------------------

use thiserror::Error;

/// Aggregate error for the `qv-consensus` crate.
///
/// Downstream crates can bubble up any consensus-layer error via `?`.
#[derive(Debug, Error)]
pub enum ConsensusError {
    /// Stake-related error (pool registration, distribution).
    #[error(transparent)]
    Stake(#[from] StakeError),

    /// Leader election error (VRF failure, no stake).
    #[error(transparent)]
    Leader(#[from] LeaderError),

    /// Block validation error (slot, height, VRF proof, KES sig).
    #[error(transparent)]
    BlockValidation(#[from] BlockValidationError),

    /// Chain state error (unknown parent, fork too deep).
    #[error(transparent)]
    Chain(#[from] ChainError),

    /// Reward computation error.
    #[error(transparent)]
    Reward(#[from] RewardError),
}

/// Convenience alias.
pub type ConsensusResult<T> = core::result::Result<T, ConsensusError>;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn consensus_error_wraps_stake_error() {
        let e: ConsensusError = StakeError::ZeroDelegation.into();
        assert!(matches!(
            e,
            ConsensusError::Stake(StakeError::ZeroDelegation)
        ));
    }

    #[test]
    fn consensus_error_wraps_chain_error() {
        let e: ConsensusError = ChainError::DuplicateBlock(qv_core::BlockHash::ZERO).into();
        assert!(matches!(
            e,
            ConsensusError::Chain(ChainError::DuplicateBlock(_))
        ));
    }

    #[test]
    fn consensus_error_display_is_transparent() {
        let inner = StakeError::ZeroDelegation;
        let expected = inner.to_string();
        let outer: ConsensusError = inner.into();
        assert_eq!(outer.to_string(), expected);
    }

    #[test]
    fn public_surface_is_reachable() {
        // Sanity: headline types are accessible from crate root.
        let _: Option<SlotClock> = None;
        let _: Option<EpochNonce> = None;
        let _: Option<StakeDistribution> = None;
        let _: Option<VrfOutput> = None;
        let _: Option<ChainState> = None;
        let _: Option<RewardShare> = None;
    }
}
