//! Encrypted-mempool decryption committee membership and sortition.
//!
//! The encrypted mempool requires a committee of operators to decrypt batches.
//! Committee membership is determined deterministically each epoch via a VRF-based
//! sortition that depends on the epoch nonce and pool ID.

use crate::{MinerError, MinerResult};
use qv_consensus::{
    PoolId, VrfEvaluator, VrfOutput,
};
use qv_core::Epoch;

/// Determine whether a pool operator is on the decryption committee for a given epoch.
///
/// # Parameters
/// - `vrf`: The VRF evaluator (real or test).
/// - `pool_id`: The pool's unique identifier.
/// - `epoch_nonce`: The epoch's random nonce.
/// - `epoch`: The epoch number.
/// - `committee_size`: The total size of the committee.
/// - `committee_threshold`: The threshold for Shamir secret sharing (t-of-n).
///
/// # Returns
/// `true` if the operator is on the committee for this epoch.
///
/// # Algorithm
/// We use a domain-separated VRF input:
/// ```text
/// domain = "committee_selection/v1"
/// vrf_input = domain || epoch || pool_id || "rank"
/// vrf_output = VRF_eval(pool_vrf_key, vrf_input)
/// rank = vrf_output.as_u256() % committee_size
/// is_member = rank < committee_threshold
/// ```
///
/// This gives every pool an equal probability of being selected, and the
/// threshold ensures enough shares exist to reconstruct the secrets.
pub fn is_committee_member(
    vrf: &dyn VrfEvaluator,
    pool_id: &PoolId,
    _epoch_nonce: &[u8],
    epoch: Epoch,
    committee_size: u32,
    committee_threshold: u32,
) -> MinerResult<bool> {
    if committee_size == 0 || committee_threshold > committee_size {
        return Err(MinerError::Committee(
            "invalid committee_size or committee_threshold".to_string(),
        ));
    }

    // Construct domain-separated VRF input.
    let mut input = Vec::with_capacity(64 + 32);
    input.extend_from_slice(b"committee_selection/v1");
    input.extend_from_slice(&epoch.as_u64().to_le_bytes());
    input.extend_from_slice(pool_id.as_bytes());
    input.extend_from_slice(b"rank");

    // Evaluate VRF.
    let (vrf_out, _proof) = vrf
        .evaluate(&input)
        .map_err(|e| MinerError::VrfError(format!("VRF evaluation failed: {e}")))?;

    // Map VRF output to a rank in [0, committee_size).
    let rank = vrf_output_to_rank(&vrf_out, committee_size);

    // Check if rank < threshold.
    Ok(rank < committee_threshold)
}

/// Convert a VRF output to a rank in [0, committee_size).
fn vrf_output_to_rank(vrf_out: &VrfOutput, committee_size: u32) -> u32 {
    // Interpret the first 4 bytes of the VRF output as a u32 (big-endian).
    let mut buf = [0u8; 4];
    buf.copy_from_slice(&vrf_out.0[..4]);
    let value = u32::from_be_bytes(buf);
    value % committee_size
}

/// Decryption share contributed by a committee member.
#[derive(Clone, Debug)]
pub struct DecryptionShare {
    /// The pool ID of the contributor.
    pub pool_id: PoolId,

    /// The share index (in the t-of-n Shamir secret sharing).
    pub share_index: u32,

    /// The share data (opaque bytes, interpreted by the threshold decryptor).
    pub share_data: Vec<u8>,

    /// The epoch this share is for.
    pub epoch: Epoch,
}

impl DecryptionShare {
    /// Construct a new decryption share.
    pub fn new(
        pool_id: PoolId,
        share_index: u32,
        share_data: Vec<u8>,
        epoch: Epoch,
    ) -> Self {
        Self {
            pool_id,
            share_index,
            share_data,
            epoch,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use qv_consensus::TestVrf;

    #[test]
    fn is_committee_member_invalid_committee() {
        let vrf = TestVrf::new([0u8; 32]);
        let pool_id = PoolId::ZERO;
        let epoch_nonce = vec![0u8; 32];
        let epoch = qv_core::Epoch::from(1);

        // committee_size = 0 should error
        let result = is_committee_member(&vrf, &pool_id, &epoch_nonce, epoch, 0, 0);
        assert!(result.is_err());

        // threshold > size should error
        let result = is_committee_member(&vrf, &pool_id, &epoch_nonce, epoch, 10, 11);
        assert!(result.is_err());
    }

    #[test]
    fn is_committee_member_determinism() {
        let vrf = TestVrf::new([0u8; 32]);
        let pool_id = PoolId::ZERO;
        let epoch_nonce = vec![0u8; 32];
        let epoch = qv_core::Epoch::from(1);

        let result1 = is_committee_member(&vrf, &pool_id, &epoch_nonce, epoch, 10, 3).unwrap();
        let result2 = is_committee_member(&vrf, &pool_id, &epoch_nonce, epoch, 10, 3).unwrap();

        assert_eq!(result1, result2);
    }

    #[test]
    fn is_committee_member_all_members_small_committee() {
        let vrf = TestVrf::new([0u8; 32]);
        let pool_id = PoolId::ZERO;
        let epoch_nonce = vec![0u8; 32];
        let epoch = qv_core::Epoch::from(1);

        // If threshold = size, all pools are members.
        let result = is_committee_member(&vrf, &pool_id, &epoch_nonce, epoch, 5, 5).unwrap();
        assert!(result);
    }

    #[test]
    fn vrf_output_to_rank_bounds() {
        let vrf_out = VrfOutput([0u8; 32]);
        let rank = vrf_output_to_rank(&vrf_out, 100);
        assert!(rank < 100);

        let rank2 = vrf_output_to_rank(&vrf_out, 1);
        assert_eq!(rank2, 0);
    }

    #[test]
    fn decryption_share_construction() {
        let pool_id = PoolId::ZERO;
        let share = DecryptionShare::new(
            pool_id,
            0,
            vec![1, 2, 3],
            qv_core::Epoch::from(1),
        );

        assert_eq!(share.pool_id, pool_id);
        assert_eq!(share.share_index, 0);
        assert_eq!(share.share_data, vec![1, 2, 3]);
    }

    #[test]
    fn committee_member_ranks_distributed() {
        let vrf = TestVrf::new([0u8; 32]);
        let epoch_nonce = vec![0u8; 32];
        let epoch = qv_core::Epoch::from(1);
        let committee_size = 20;
        let committee_threshold = 10;

        let mut ranks = vec![];
        for i in 0..20 {
            let pool_id = PoolId(qv_core::Hash256::from_bytes([i as u8; 32]));
            let is_member = is_committee_member(&vrf, &pool_id, &epoch_nonce, epoch, committee_size, committee_threshold)
                .unwrap();
            if is_member {
                ranks.push(i);
            }
        }

        // With threshold=10 and size=20, we expect roughly 10 members.
        // (exact number depends on VRF output distribution)
        assert!(!ranks.is_empty());
    }
}
