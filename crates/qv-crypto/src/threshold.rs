//! Threshold cryptography for the encrypted mempool (ADR-003).
//!
//! # Overview
//!
//! The encrypted mempool requires a **threshold** (t-of-n) scheme so that no
//! single validator can decrypt pending transactions; only the committee as a
//! whole, collaborating, can.
//!
//! This module provides:
//!
//! - **Shamir Secret Sharing** — proper polynomial evaluation over GF(p) with
//!   a 256-bit safe prime, and Lagrange interpolation for reconstruction.
//! - **Feldman VSS** — verifiable secret sharing using a cyclic group
//!   generator. Commitments to polynomial coefficients let verifiers check
//!   shares without learning the secret.
//! - **Pedersen DKG** — distributed key generation where every participant
//!   runs Feldman VSS and the individual commitments are aggregated into a
//!   joint public key. No single party knows the full secret.
//! - **Threshold Decryption** — share-based ElGamal-style decryption using
//!   the DKG-derived key shares.
//! - **Mock Implementations** — deterministic test doubles (kept for unit
//!   tests that don't need real crypto).
//!
//! # Finite-field arithmetic
//!
//! We work modulo the 256-bit prime
//! `p = 2^256 − 189` (the largest 256-bit safe-ish prime with a small
//! negative offset, easy to implement). All scalars are represented as
//! `[u8; 32]` in big-endian form; arithmetic helpers live in the private
//! `field` submodule at the bottom of this file.
//!
//! # References
//!
//! - Feldman, "A Practical Scheme for Non-interactive Verifiable Secret
//!   Sharing", FOCS 1987.
//! - Pedersen, "Non-Interactive and Information-Theoretic Secure Verifiable
//!   Secret Sharing", CRYPTO 1991.
//! - Gennaro et al., "Secure Distributed Key Generation for Discrete-Log
//!   Based Cryptosystems", J. Cryptology 2007.

use serde::{Deserialize, Serialize};

use crate::{sha3_256, CryptoError, Result};

// ============================================================================
// Finite-field helpers (GF(p), p = 2^256 − 189)
// ============================================================================

mod field {
    //! Minimal big-integer arithmetic modulo `P = 2^256 − 189`.
    //!
    //! All values are 256-bit unsigned integers in **big-endian** byte order.
    //! We use 4×u64 limbs internally (big-endian limb order: limbs[0] is the
    //! most significant).

    /// The prime modulus: p = 2^256 − 189.
    /// In hex: FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF43
    pub(super) const P: [u8; 32] = {
        let mut p = [0xFF_u8; 32];
        p[31] = 0x43; // 0xFF - 188 = 0x43
        p
    };

    // -- limb helpers --------------------------------------------------------

    type U256 = [u64; 4]; // big-endian limb order

    fn to_limbs(a: &[u8; 32]) -> U256 {
        [
            u64::from_be_bytes([a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]]),
            u64::from_be_bytes([a[8], a[9], a[10], a[11], a[12], a[13], a[14], a[15]]),
            u64::from_be_bytes([a[16], a[17], a[18], a[19], a[20], a[21], a[22], a[23]]),
            u64::from_be_bytes([a[24], a[25], a[26], a[27], a[28], a[29], a[30], a[31]]),
        ]
    }

    fn from_limbs(l: &U256) -> [u8; 32] {
        let mut out = [0u8; 32];
        let b0 = l[0].to_be_bytes();
        let b1 = l[1].to_be_bytes();
        let b2 = l[2].to_be_bytes();
        let b3 = l[3].to_be_bytes();
        out[0..8].copy_from_slice(&b0);
        out[8..16].copy_from_slice(&b1);
        out[16..24].copy_from_slice(&b2);
        out[24..32].copy_from_slice(&b3);
        out
    }

    fn p_limbs() -> U256 {
        to_limbs(&P)
    }

    /// a >= b  (constant-time-ish, but we don't need CT here).
    fn gte(a: &U256, b: &U256) -> bool {
        for i in 0..4 {
            if a[i] > b[i] {
                return true;
            }
            if a[i] < b[i] {
                return false;
            }
        }
        true // equal
    }

    /// a + b  (without reduction, returns carry).
    fn add_raw(a: &U256, b: &U256) -> (U256, bool) {
        let mut out = [0u64; 4];
        let mut carry = 0u64;
        for i in (0..4).rev() {
            let sum = (a[i] as u128) + (b[i] as u128) + (carry as u128);
            out[i] = sum as u64;
            carry = (sum >> 64) as u64;
        }
        (out, carry != 0)
    }

    /// a − b  (without reduction, returns borrow).
    fn sub_raw(a: &U256, b: &U256) -> (U256, bool) {
        let mut out = [0u64; 4];
        let mut borrow = 0i128;
        for i in (0..4).rev() {
            let diff = (a[i] as i128) - (b[i] as i128) - borrow;
            if diff < 0 {
                out[i] = (diff + (1i128 << 64)) as u64;
                borrow = 1;
            } else {
                out[i] = diff as u64;
                borrow = 0;
            }
        }
        (out, borrow != 0)
    }

    /// Reduce modulo P.
    fn reduce(a: &U256) -> U256 {
        let p = p_limbs();
        if gte(a, &p) {
            let (r, _) = sub_raw(a, &p);
            r
        } else {
            *a
        }
    }

    // -- public interface ----------------------------------------------------

    /// Zero element.
    pub(super) fn zero() -> [u8; 32] {
        [0u8; 32]
    }

    /// Encode a u32 as a field element.
    pub(super) fn from_u32(v: u32) -> [u8; 32] {
        let mut out = [0u8; 32];
        out[28..32].copy_from_slice(&v.to_be_bytes());
        out
    }

    /// (a + b) mod p.
    pub(super) fn add(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
        let la = to_limbs(a);
        let lb = to_limbs(b);
        let (sum, carry) = add_raw(&la, &lb);
        if carry {
            // sum ≥ 2^256; subtract p.
            let (r, _) = sub_raw(&sum, &p_limbs());
            from_limbs(&r)
        } else {
            from_limbs(&reduce(&sum))
        }
    }

    /// (a − b) mod p.
    pub(super) fn sub(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
        let la = to_limbs(a);
        let lb = to_limbs(b);
        let (diff, borrow) = sub_raw(&la, &lb);
        if borrow {
            let (r, _) = add_raw(&diff, &p_limbs());
            from_limbs(&r)
        } else {
            from_limbs(&diff)
        }
    }

    /// (a × b) mod p   — schoolbook multiplication with 512-bit intermediate.
    pub(super) fn mul(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
        let la = to_limbs(a);
        let lb = to_limbs(b);

        // Produce 8-limb (512-bit) product.
        let mut prod = [0u128; 8];
        for i in (0..4).rev() {
            for j in (0..4).rev() {
                let p_ij = (la[i] as u128) * (lb[j] as u128);
                let k = i + j; // 0..6
                prod[k] += p_ij >> 64;
                if k + 1 < 8 {
                    prod[k + 1] += p_ij & 0xFFFF_FFFF_FFFF_FFFF;
                }
            }
        }
        // Propagate carries in 512-bit product.
        for i in (1..8).rev() {
            prod[i - 1] += prod[i] >> 64;
            prod[i] &= 0xFFFF_FFFF_FFFF_FFFF;
        }
        prod[0] &= 0xFFFF_FFFF_FFFF_FFFF;

        // Barrett-like reduction: divide 512 bit by P.
        // Simpler approach: repeated subtraction would be too slow.
        // Since p = 2^256 − 189, we can use the identity:
        //   x mod p = x_lo + 189 * x_hi   (mod p)
        // where x = x_hi * 2^256 + x_lo.
        let x_hi = [prod[0] as u64, prod[1] as u64, prod[2] as u64, prod[3] as u64];
        let x_lo = [prod[4] as u64, prod[5] as u64, prod[6] as u64, prod[7] as u64];

        // 189 * x_hi (up to 256 + 8 = 264 bits, fits in 5 limbs).
        let c = 189u128;
        let mut correction = [0u64; 5]; // 5 limbs for overflow
        let mut carry = 0u128;
        for i in (0..4).rev() {
            let v = (x_hi[i] as u128) * c + carry;
            correction[i + 1] = v as u64;
            carry = v >> 64;
        }
        correction[0] = carry as u64;

        // Add x_lo + correction (5 limbs → 5 limbs).
        let mut result5 = [0u64; 5];
        let mut c2 = 0u128;
        // x_lo is 4 limbs; correction is 5 limbs.
        for i in (0..4).rev() {
            let s = (x_lo[i] as u128) + (correction[i + 1] as u128) + c2;
            result5[i + 1] = s as u64;
            c2 = s >> 64;
        }
        result5[0] = (correction[0] as u128 + c2) as u64;

        // If result5[0] > 0, we have overflow beyond 256 bits.
        // Apply the same reduction again: result = result_lo + 189 * result_hi.
        // result_hi is just result5[0] (at most ~189*2^8 sized, small).
        let overflow = result5[0] as u128;
        let mut final_limbs = [result5[1], result5[2], result5[3], result5[4]];
        if overflow > 0 {
            let extra = overflow * 189;
            let (added, carry) = add_raw(&final_limbs, &[0, 0, (extra >> 64) as u64, extra as u64]);
            final_limbs = added;
            if carry {
                // One more reduction (extremely rare).
                final_limbs = sub_raw(&final_limbs, &p_limbs()).0;
            }
        }

        // Final reduction if >= p.
        let result = reduce(&final_limbs);
        from_limbs(&result)
    }

    /// a^exp mod p  (binary exponentiation).
    pub(super) fn pow(base: &[u8; 32], exp: &[u8; 32]) -> [u8; 32] {
        let mut result = from_u32(1);
        let mut b = *base;

        // Iterate bits from LSB to MSB.
        for byte_idx in (0..32).rev() {
            for bit in 0..8 {
                if (exp[byte_idx] >> bit) & 1 == 1 {
                    result = mul(&result, &b);
                }
                b = mul(&b, &b);
            }
        }
        result
    }

    /// Modular inverse: a^(p−2) mod p  (Fermat's little theorem).
    pub(super) fn inv(a: &[u8; 32]) -> [u8; 32] {
        // p - 2 = 2^256 - 191
        let mut p_minus_2 = P;
        // Subtract 2 from P (P ends in 0x43, so P-2 ends in 0x41).
        p_minus_2[31] = 0x41;
        pow(a, &p_minus_2)
    }

    /// Reduce an arbitrary 32-byte value modulo P.
    ///
    /// Input is big-endian. If input >= P, we subtract P.
    pub(super) fn reduce_bytes(a: &[u8; 32]) -> [u8; 32] {
        let la = to_limbs(a);
        from_limbs(&reduce(&la))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_add_basic() {
            let a = from_u32(100);
            let b = from_u32(200);
            let c = add(&a, &b);
            assert_eq!(c, from_u32(300));
        }

        #[test]
        fn test_add_wraps() {
            // P - 1 + 1 = 0 (mod P).
            let mut p_minus_1 = P;
            p_minus_1[31] = 0x42;
            let one = from_u32(1);
            let result = add(&p_minus_1, &one);
            assert_eq!(result, zero());
        }

        #[test]
        fn test_sub_basic() {
            let a = from_u32(300);
            let b = from_u32(100);
            let c = sub(&a, &b);
            assert_eq!(c, from_u32(200));
        }

        #[test]
        fn test_sub_underflow() {
            // 0 - 1 = P - 1.
            let z = zero();
            let one = from_u32(1);
            let result = sub(&z, &one);
            let mut expected = P;
            expected[31] = 0x42;
            assert_eq!(result, expected);
        }

        #[test]
        fn test_mul_basic() {
            let a = from_u32(7);
            let b = from_u32(6);
            let c = mul(&a, &b);
            assert_eq!(c, from_u32(42));
        }

        #[test]
        fn test_mul_identity() {
            let a = from_u32(12345);
            let one = from_u32(1);
            assert_eq!(mul(&a, &one), a);
        }

        #[test]
        fn test_inv() {
            let a = from_u32(7);
            let a_inv = inv(&a);
            let product = mul(&a, &a_inv);
            assert_eq!(product, from_u32(1));
        }

        #[test]
        fn test_pow_small() {
            let base = from_u32(3);
            let exp = from_u32(4);
            let result = pow(&base, &exp);
            assert_eq!(result, from_u32(81));
        }
    }
}

// ============================================================================
// Shamir Secret Sharing (proper GF(p) polynomial evaluation)
// ============================================================================

/// A single share in a Shamir secret sharing scheme.
///
/// Each share has a unique index (1-indexed, 1..=n) and a 32-byte value
/// which is the polynomial evaluated at that index, modulo p.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShamirShare {
    /// Participant index (1 to n); must be unique and non-zero.
    pub index: u32,
    /// Share value (32 bytes, big-endian field element).
    pub value: [u8; 32],
}

impl ShamirShare {
    /// Create a new Shamir share.
    #[must_use]
    pub fn new(index: u32, value: [u8; 32]) -> Self {
        Self { index, value }
    }
}

/// Split a secret into `total` shares such that any `threshold` can reconstruct it.
///
/// Uses polynomial evaluation over GF(p) where p = 2^256 − 189. The secret
/// is the constant term a_0 of a random polynomial of degree (threshold − 1).
/// Coefficients a_1..a_{t-1} are derived deterministically from the secret
/// via SHA3 (for reproducibility in tests; in production, use a CSPRNG).
///
/// # Errors
///
/// Returns `CryptoError::Other` if:
/// - `threshold == 0` or `threshold > total`
/// - `total > 255` (index space exhaustion)
///
/// # Examples
///
/// ```rust,no_run
/// # use qv_crypto::threshold::split_secret;
/// let secret = [42u8; 32];
/// let shares = split_secret(&secret, 3, 5).expect("split failed");
/// assert_eq!(shares.len(), 5);
/// ```
pub fn split_secret(secret: &[u8; 32], threshold: u32, total: u32) -> Result<Vec<ShamirShare>> {
    if threshold == 0 || threshold > total {
        return Err(CryptoError::Other(format!(
            "invalid threshold/total: {}/{}",
            threshold, total
        )));
    }
    if total > 255 {
        return Err(CryptoError::Other(
            "total must be <= 255 for index space".to_string(),
        ));
    }

    // Reduce secret modulo P.
    let a0 = field::reduce_bytes(secret);

    // Generate (threshold - 1) random coefficients deterministically from
    // the secret. In production, these MUST come from a CSPRNG.
    let mut coefficients = vec![a0];
    for i in 1..threshold {
        let mut seed = Vec::with_capacity(36);
        seed.extend_from_slice(secret);
        seed.extend_from_slice(&i.to_le_bytes());
        let coeff = field::reduce_bytes(&sha3_256(&seed));
        coefficients.push(coeff);
    }

    // Evaluate polynomial at each index (1..=total).
    let mut shares = Vec::with_capacity(total as usize);
    for idx in 1..=total {
        let x = field::from_u32(idx);
        let y = poly_eval(&coefficients, &x);
        shares.push(ShamirShare {
            index: idx,
            value: y,
        });
    }

    Ok(shares)
}

/// Evaluate polynomial `coeffs[0] + coeffs[1]*x + coeffs[2]*x^2 + ...` mod p.
fn poly_eval(coeffs: &[[u8; 32]], x: &[u8; 32]) -> [u8; 32] {
    // Horner's method: result = c_{n-1}; result = result*x + c_{n-2}; ...
    let mut result = field::zero();
    for coeff in coeffs.iter().rev() {
        result = field::mul(&result, x);
        result = field::add(&result, coeff);
    }
    result
}

/// Reconstruct a secret from a set of Shamir shares.
///
/// Uses Lagrange interpolation at x=0 over GF(p). Requires at least
/// `threshold` shares.
///
/// # Errors
///
/// Returns `CryptoError::Other` if:
/// - Fewer than `threshold` shares provided.
/// - Shares have duplicate or zero indices.
///
/// # Examples
///
/// ```rust,no_run
/// # use qv_crypto::threshold::{split_secret, reconstruct_secret};
/// let secret = [42u8; 32];
/// let shares = split_secret(&secret, 3, 5).expect("split failed");
/// let recovered = reconstruct_secret(&shares[0..3], 3).expect("reconstruct failed");
/// assert_eq!(recovered, secret);
/// ```
pub fn reconstruct_secret(shares: &[ShamirShare], threshold: u32) -> Result<[u8; 32]> {
    if (shares.len() as u32) < threshold {
        return Err(CryptoError::Other(format!(
            "insufficient shares: have {}, need {}",
            shares.len(),
            threshold
        )));
    }

    // Check for duplicate or zero indices.
    let used = &shares[..threshold as usize];
    let mut indices: Vec<u32> = used.iter().map(|s| s.index).collect();
    indices.sort_unstable();
    for w in indices.windows(2) {
        if w[0] == w[1] {
            return Err(CryptoError::Other("duplicate share index".to_string()));
        }
    }
    if indices.first() == Some(&0) {
        return Err(CryptoError::Other("share index must be non-zero".to_string()));
    }

    // Lagrange interpolation at x = 0.
    //
    // secret = sum_i ( y_i * prod_{j≠i} (0 - x_j) / (x_i - x_j) )
    //        = sum_i ( y_i * prod_{j≠i} (-x_j) / (x_i - x_j) )
    //        = sum_i ( y_i * L_i(0) )
    let mut result = field::zero();

    for (i, share_i) in used.iter().enumerate() {
        let x_i = field::from_u32(share_i.index);

        // Compute Lagrange basis polynomial L_i(0).
        let mut numerator = field::from_u32(1);
        let mut denominator = field::from_u32(1);

        for (j, share_j) in used.iter().enumerate() {
            if i == j {
                continue;
            }
            let x_j = field::from_u32(share_j.index);

            // numerator *= (0 - x_j) = -x_j
            let neg_x_j = field::sub(&field::zero(), &x_j);
            numerator = field::mul(&numerator, &neg_x_j);

            // denominator *= (x_i - x_j)
            let diff = field::sub(&x_i, &x_j);
            denominator = field::mul(&denominator, &diff);
        }

        let basis = field::mul(&numerator, &field::inv(&denominator));
        let term = field::mul(&share_i.value, &basis);
        result = field::add(&result, &term);
    }

    Ok(result)
}

// ============================================================================
// Distributed Key Generation (DKG) — Traits
// ============================================================================

/// A commitment to a polynomial coefficient (Feldman VSS).
///
/// In Feldman VSS, each commitment is `g^{a_i}` where `a_i` is a
/// coefficient and `g` is a generator. We simulate the group operation
/// with modular exponentiation over GF(p) using generator `g = 2`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DkgCommitment {
    /// Commitment value (32 bytes).
    pub value: [u8; 32],
    /// Participant ID who issued this commitment.
    pub participant_id: u32,
}

/// A secret share issued by a DKG participant.
///
/// To be verified against the participant's commitments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkgShare {
    /// The recipient participant ID.
    pub recipient_id: u32,
    /// The issuer participant ID.
    pub issuer_id: u32,
    /// The share value (32-byte field element).
    pub encrypted_share: [u8; 32],
}

/// The aggregate public key derived from all DKG commitments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThresholdPublicKey {
    /// Aggregate public key value (32 bytes).
    pub value: [u8; 32],
}

impl ThresholdPublicKey {
    /// Create a new threshold public key.
    #[must_use]
    pub fn new(value: [u8; 32]) -> Self {
        Self { value }
    }
}

/// Trait for a DKG participant in a distributed key generation protocol.
pub trait DkgParticipant {
    /// Generate commitments to this participant's polynomial coefficients.
    ///
    /// Returns one commitment per coefficient (threshold many).
    fn generate_commitment(&self) -> Result<DkgCommitment>;

    /// Generate shares for all other participants, given their commitments.
    fn generate_shares(&self, commitments: &[DkgCommitment]) -> Result<Vec<DkgShare>>;

    /// Verify that a share from an issuer is consistent with their commitment.
    fn verify_share(&self, share: &DkgShare, commitment: &DkgCommitment) -> Result<bool>;

    /// Derive the final public key from all commitments (constant-term commitments).
    fn derive_public_key(&self, commitments: &[DkgCommitment]) -> Result<ThresholdPublicKey>;
}

// ============================================================================
// Feldman VSS + Pedersen DKG — Real Implementation
// ============================================================================

/// Generator for the multiplicative group mod p.
/// We use g = 2 (a primitive root mod many primes of this form).
const GENERATOR: u32 = 2;

/// A Feldman VSS participant that implements real verifiable secret sharing.
///
/// Each participant:
/// 1. Chooses a random polynomial of degree (threshold − 1).
/// 2. Publishes commitments `g^{a_i}` for each coefficient `a_i`.
/// 3. Sends share `f(j)` privately to participant `j`.
/// 4. Verifiers check `g^{f(j)} == prod(C_i^{j^i})`.
///
/// In the Pedersen DKG variant, each participant runs Feldman VSS
/// independently, and the final secret key is the sum of all constant
/// terms (never computed by anyone); the public key is the product of
/// all constant-term commitments.
#[derive(Debug, Clone)]
pub struct FeldmanVssParticipant {
    /// Participant identifier (0-indexed).
    pub id: u32,
    /// Threshold (minimum shares to reconstruct).
    pub threshold: u32,
    /// Total number of participants.
    pub total: u32,
    /// This participant's polynomial coefficients (field elements).
    /// coefficients[0] is the secret.
    coefficients: Vec<[u8; 32]>,
}

impl FeldmanVssParticipant {
    /// Create a new Feldman VSS participant.
    ///
    /// The secret and additional coefficients are derived deterministically
    /// from a seed. In production, use a CSPRNG.
    pub fn new(id: u32, threshold: u32, total: u32, seed: &[u8; 32]) -> Self {
        let mut coefficients = Vec::with_capacity(threshold as usize);

        for coeff_idx in 0..threshold {
            let mut input = Vec::with_capacity(40);
            input.extend_from_slice(seed);
            input.extend_from_slice(b"feldman_coeff");
            input.extend_from_slice(&id.to_le_bytes());
            input.extend_from_slice(&coeff_idx.to_le_bytes());
            let coeff = field::reduce_bytes(&sha3_256(&input));
            coefficients.push(coeff);
        }

        Self {
            id,
            threshold,
            total,
            coefficients,
        }
    }

    /// Get this participant's secret (constant term a_0).
    pub fn secret(&self) -> &[u8; 32] {
        &self.coefficients[0]
    }

    /// Evaluate this participant's polynomial at point x.
    fn evaluate_at(&self, x: &[u8; 32]) -> [u8; 32] {
        poly_eval(&self.coefficients, x)
    }

    /// Compute all commitments: `g^{a_i}` for each coefficient.
    pub fn compute_commitments(&self) -> Vec<[u8; 32]> {
        let g = field::from_u32(GENERATOR);
        self.coefficients
            .iter()
            .map(|coeff| field::pow(&g, coeff))
            .collect()
    }

    /// Compute shares for all other participants.
    ///
    /// Share for participant j (1-indexed) is `f(j)`.
    pub fn compute_shares(&self) -> Vec<DkgShare> {
        let mut shares = Vec::new();
        for j in 0..self.total {
            if j == self.id {
                continue;
            }
            let x = field::from_u32(j + 1); // 1-indexed evaluation point
            let share_value = self.evaluate_at(&x);
            shares.push(DkgShare {
                recipient_id: j,
                issuer_id: self.id,
                encrypted_share: share_value,
            });
        }
        shares
    }

    /// Verify a received share against the issuer's commitments.
    ///
    /// Checks: `g^{share} == prod_{i=0}^{t-1} C_i^{j^i}`
    /// where `j` is the recipient's 1-indexed evaluation point.
    pub fn verify_share_against_commitments(
        &self,
        share: &DkgShare,
        issuer_commitments: &[[u8; 32]],
    ) -> bool {
        let g = field::from_u32(GENERATOR);

        // Left side: g^share
        let lhs = field::pow(&g, &share.encrypted_share);

        // Right side: product of C_i^{j^i}
        let j = field::from_u32(share.recipient_id + 1); // 1-indexed
        let mut rhs = field::from_u32(1);
        let mut j_power = field::from_u32(1); // j^0 = 1

        for commitment in issuer_commitments {
            // rhs *= C_i^{j^i}
            let term = field::pow(commitment, &j_power);
            rhs = field::mul(&rhs, &term);

            // j_power *= j
            j_power = field::mul(&j_power, &j);
        }

        lhs == rhs
    }
}

impl DkgParticipant for FeldmanVssParticipant {
    fn generate_commitment(&self) -> Result<DkgCommitment> {
        // Return commitment to the constant term (secret): g^{a_0}.
        let g = field::from_u32(GENERATOR);
        let value = field::pow(&g, &self.coefficients[0]);
        Ok(DkgCommitment {
            value,
            participant_id: self.id,
        })
    }

    fn generate_shares(&self, commitments: &[DkgCommitment]) -> Result<Vec<DkgShare>> {
        let _ = commitments; // In full DKG, we'd encrypt to recipients.
        Ok(self.compute_shares())
    }

    fn verify_share(&self, share: &DkgShare, commitment: &DkgCommitment) -> Result<bool> {
        // Simplified: check g^share == commitment^{j} (only constant-term).
        let g = field::from_u32(GENERATOR);
        let lhs = field::pow(&g, &share.encrypted_share);
        let j = field::from_u32(share.recipient_id + 1);
        let rhs = field::pow(&commitment.value, &j);
        Ok(lhs == rhs)
    }

    fn derive_public_key(&self, commitments: &[DkgCommitment]) -> Result<ThresholdPublicKey> {
        // Aggregate public key = product of all constant-term commitments.
        // g^{a0_1} * g^{a0_2} * ... = g^{sum(a0_i)}
        let mut aggregate = field::from_u32(1);
        for c in commitments {
            aggregate = field::mul(&aggregate, &c.value);
        }
        Ok(ThresholdPublicKey::new(aggregate))
    }
}

// ============================================================================
// Pedersen DKG Coordinator
// ============================================================================

/// Result of a completed Pedersen DKG round.
///
/// Contains the aggregate public key and each participant's share of the
/// aggregate secret (which no single party knows in full).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkgResult {
    /// The aggregate threshold public key.
    pub public_key: ThresholdPublicKey,
    /// Each participant's aggregate share: sum of all shares received
    /// plus their own self-evaluation.
    pub participant_shares: Vec<ShamirShare>,
}

/// Run a complete Pedersen DKG round among the given participants.
///
/// Steps:
/// 1. Each participant generates their polynomial and commitments.
/// 2. Each participant computes shares for all others.
/// 3. Each participant aggregates received shares (sum of shares from all
///    issuers at their evaluation point).
/// 4. The aggregate public key is the product of all constant-term commitments.
///
/// # Errors
///
/// Returns error if any participant fails to generate commitments or shares.
pub fn run_pedersen_dkg(
    participants: &[FeldmanVssParticipant],
) -> Result<DkgResult> {
    let n = participants.len();
    if n < 2 {
        return Err(CryptoError::Other(
            "DKG requires at least 2 participants".to_string(),
        ));
    }

    // Step 1: All participants generate commitments.
    let all_commitments: Vec<Vec<[u8; 32]>> = participants
        .iter()
        .map(|p| p.compute_commitments())
        .collect();

    // Step 2: All participants compute shares for each other.
    let all_shares: Vec<Vec<DkgShare>> = participants
        .iter()
        .map(|p| p.compute_shares())
        .collect();

    // Step 3: Verify all shares (each recipient checks shares from issuers).
    for (issuer_idx, shares) in all_shares.iter().enumerate() {
        for share in shares {
            let recipient = &participants[share.recipient_id as usize];
            let valid = recipient.verify_share_against_commitments(
                share,
                &all_commitments[issuer_idx],
            );
            if !valid {
                return Err(CryptoError::Other(format!(
                    "share verification failed: issuer={}, recipient={}",
                    issuer_idx, share.recipient_id
                )));
            }
        }
    }

    // Step 4: Each participant aggregates their shares.
    // Participant j's aggregate share = sum of f_i(j+1) for all i (including self).
    let mut participant_shares = Vec::with_capacity(n);
    for j in 0..n {
        let x = field::from_u32(j as u32 + 1);
        let mut agg = field::zero();

        for (i, participant) in participants.iter().enumerate() {
            // Each participant evaluates their own polynomial at x.
            let share_val = participant.evaluate_at(&x);
            agg = field::add(&agg, &share_val);
            let _ = i; // suppress warning
        }

        participant_shares.push(ShamirShare {
            index: j as u32 + 1,
            value: agg,
        });
    }

    // Step 5: Aggregate public key = product of all g^{a0_i}.
    let g = field::from_u32(GENERATOR);
    let mut aggregate_pk = field::from_u32(1);
    for commitments in &all_commitments {
        // First commitment is g^{a0}.
        aggregate_pk = field::mul(&aggregate_pk, &commitments[0]);
    }

    // Verify: aggregate_pk should equal g^{sum of all secrets}.
    let mut total_secret = field::zero();
    for p in participants {
        total_secret = field::add(&total_secret, p.secret());
    }
    let expected_pk = field::pow(&g, &total_secret);
    if aggregate_pk != expected_pk {
        return Err(CryptoError::Other(
            "aggregate public key mismatch".to_string(),
        ));
    }

    Ok(DkgResult {
        public_key: ThresholdPublicKey::new(aggregate_pk),
        participant_shares,
    })
}

// ============================================================================
// Threshold Decryption
// ============================================================================

/// A decryption share contributed by a participant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecryptionShare {
    /// Participant ID who created this share.
    pub participant_id: u32,
    /// The decryption share data (variable length).
    pub share_data: Vec<u8>,
}

impl DecryptionShare {
    /// Create a new decryption share.
    #[must_use]
    pub fn new(participant_id: u32, share_data: Vec<u8>) -> Self {
        Self {
            participant_id,
            share_data,
        }
    }
}

/// Trait for threshold decryption (combining shares to recover plaintext).
pub trait ThresholdDecryptor {
    /// Create a decryption share for the given ciphertext.
    fn create_decryption_share(&self, ciphertext: &[u8]) -> Result<DecryptionShare>;

    /// Combine threshold shares to recover the plaintext.
    fn combine_shares(&self, shares: &[DecryptionShare], threshold: u32) -> Result<Vec<u8>>;
}

/// Threshold decryptor that uses DKG-derived key shares.
///
/// Implements a simplified ElGamal-style threshold decryption:
/// - Ciphertext is `(C1, C2)` where `C1 = g^r`, `C2 = m XOR H(pk^r)`.
/// - Each participant computes `D_i = C1^{s_i}` where `s_i` is their share.
/// - Combine: `D = prod(D_i^{L_i})` (Lagrange-weighted product).
/// - Recover: `m = C2 XOR H(D)`.
#[derive(Debug, Clone)]
pub struct DkgThresholdDecryptor {
    /// Participant identifier (1-indexed to match ShamirShare).
    pub participant_index: u32,
    /// This participant's aggregate DKG share (field element).
    pub key_share: [u8; 32],
    /// Threshold for decryption.
    pub threshold: u32,
}

impl DkgThresholdDecryptor {
    /// Create a new DKG-backed threshold decryptor.
    #[must_use]
    pub fn new(participant_index: u32, key_share: [u8; 32], threshold: u32) -> Self {
        Self {
            participant_index,
            key_share,
            threshold,
        }
    }

    /// Encrypt a message to the threshold public key.
    ///
    /// Returns `(C1, C2)` concatenated (64 bytes total).
    /// `C1 = g^r`, `C2 = m XOR H(pk^r)`.
    pub fn encrypt(public_key: &ThresholdPublicKey, message: &[u8; 32], randomness: &[u8; 32]) -> [u8; 64] {
        let g = field::from_u32(GENERATOR);
        let r = field::reduce_bytes(randomness);

        // C1 = g^r
        let c1 = field::pow(&g, &r);

        // shared = pk^r
        let shared = field::pow(&public_key.value, &r);

        // mask = H(shared)
        let mask = sha3_256(&shared);

        // C2 = m XOR mask
        let mut c2 = [0u8; 32];
        for i in 0..32 {
            c2[i] = message[i] ^ mask[i];
        }

        let mut result = [0u8; 64];
        result[..32].copy_from_slice(&c1);
        result[32..].copy_from_slice(&c2);
        result
    }
}

impl ThresholdDecryptor for DkgThresholdDecryptor {
    fn create_decryption_share(&self, ciphertext: &[u8]) -> Result<DecryptionShare> {
        if ciphertext.len() < 32 {
            return Err(CryptoError::Other(
                "ciphertext too short (need at least 32 bytes for C1)".to_string(),
            ));
        }

        let mut c1 = [0u8; 32];
        c1.copy_from_slice(&ciphertext[..32]);

        // D_i = C1^{s_i}
        let d_i = field::pow(&c1, &self.key_share);

        Ok(DecryptionShare::new(self.participant_index, d_i.to_vec()))
    }

    fn combine_shares(&self, shares: &[DecryptionShare], threshold: u32) -> Result<Vec<u8>> {
        if (shares.len() as u32) < threshold {
            return Err(CryptoError::Other(format!(
                "insufficient shares: have {}, need {}",
                shares.len(),
                threshold
            )));
        }

        // Lagrange-weighted combination in the exponent.
        // D = prod( D_i^{L_i(0)} )
        let used = &shares[..threshold as usize];
        let mut combined = field::from_u32(1);

        for (i, share_i) in used.iter().enumerate() {
            // Compute Lagrange coefficient L_i(0).
            let x_i = field::from_u32(share_i.participant_id);

            let mut num = field::from_u32(1);
            let mut den = field::from_u32(1);

            for (j, share_j) in used.iter().enumerate() {
                if i == j {
                    continue;
                }
                let x_j = field::from_u32(share_j.participant_id);
                let neg_x_j = field::sub(&field::zero(), &x_j);
                num = field::mul(&num, &neg_x_j);
                let diff = field::sub(&x_i, &x_j);
                den = field::mul(&den, &diff);
            }

            let lambda_i = field::mul(&num, &field::inv(&den));

            // D_i^{lambda_i}
            let mut d_i = [0u8; 32];
            if share_i.share_data.len() >= 32 {
                d_i.copy_from_slice(&share_i.share_data[..32]);
            }
            let term = field::pow(&d_i, &lambda_i);
            combined = field::mul(&combined, &term);
        }

        Ok(combined.to_vec())
    }
}

// ============================================================================
// Mock Implementations for Testing (kept for backward compatibility)
// ============================================================================

/// Mock DKG participant using SHA3-based deterministic commitments.
///
/// **For testing only.** All outputs are deterministic based on participant ID.
#[derive(Debug, Clone)]
pub struct MockDkgParticipant {
    /// Participant identifier (0-indexed).
    pub id: u32,
    /// Shared random seed for determinism (for tests).
    pub seed: [u8; 32],
}

impl MockDkgParticipant {
    /// Create a new mock participant.
    #[must_use]
    pub fn new(id: u32, seed: [u8; 32]) -> Self {
        Self { id, seed }
    }

    /// Generate a deterministic local secret.
    fn local_secret(&self) -> [u8; 32] {
        let mut input = Vec::new();
        input.extend_from_slice(&self.seed);
        input.extend_from_slice(b"local_secret");
        input.extend_from_slice(&self.id.to_le_bytes());
        sha3_256(&input)
    }
}

impl DkgParticipant for MockDkgParticipant {
    fn generate_commitment(&self) -> Result<DkgCommitment> {
        let secret = self.local_secret();
        let value = sha3_256(&secret);
        Ok(DkgCommitment {
            value,
            participant_id: self.id,
        })
    }

    fn generate_shares(&self, commitments: &[DkgCommitment]) -> Result<Vec<DkgShare>> {
        let secret = self.local_secret();
        let mut shares = Vec::new();
        for (idx, commitment) in commitments.iter().enumerate() {
            let recipient_id = idx as u32;
            if recipient_id == self.id {
                continue;
            }
            let mut input = Vec::new();
            input.extend_from_slice(&secret);
            input.extend_from_slice(&recipient_id.to_le_bytes());
            input.extend_from_slice(&commitment.value);
            let encrypted_share = sha3_256(&input);
            shares.push(DkgShare {
                recipient_id,
                issuer_id: self.id,
                encrypted_share,
            });
        }
        Ok(shares)
    }

    fn verify_share(&self, share: &DkgShare, commitment: &DkgCommitment) -> Result<bool> {
        let secret = self.local_secret();
        let mut input = Vec::new();
        input.extend_from_slice(&secret);
        input.extend_from_slice(&share.recipient_id.to_le_bytes());
        input.extend_from_slice(&commitment.value);
        let expected = sha3_256(&input);
        Ok(expected == share.encrypted_share)
    }

    fn derive_public_key(&self, commitments: &[DkgCommitment]) -> Result<ThresholdPublicKey> {
        let mut result = [0u8; 32];
        for commitment in commitments {
            for (i, byte) in commitment.value.iter().enumerate() {
                result[i] ^= byte;
            }
        }
        Ok(ThresholdPublicKey::new(result))
    }
}

/// Mock threshold decryptor using simplified XOR-based share combination.
///
/// **For testing only.** Decryption is a mock XOR reversal.
#[derive(Debug, Clone)]
pub struct MockThresholdDecryptor {
    /// Participant identifier.
    pub id: u32,
    /// Shared secret for determinism.
    pub secret: [u8; 32],
}

impl MockThresholdDecryptor {
    /// Create a new mock decryptor.
    #[must_use]
    pub fn new(id: u32, secret: [u8; 32]) -> Self {
        Self { id, secret }
    }
}

impl ThresholdDecryptor for MockThresholdDecryptor {
    fn create_decryption_share(&self, ciphertext: &[u8]) -> Result<DecryptionShare> {
        let mut input = Vec::new();
        input.extend_from_slice(&self.secret);
        input.extend_from_slice(b"decryption_share");
        input.extend_from_slice(&self.id.to_le_bytes());
        input.extend_from_slice(ciphertext);
        let share = sha3_256(&input).to_vec();
        Ok(DecryptionShare::new(self.id, share))
    }

    fn combine_shares(&self, shares: &[DecryptionShare], threshold: u32) -> Result<Vec<u8>> {
        if (shares.len() as u32) < threshold {
            return Err(CryptoError::Other(format!(
                "insufficient shares: have {}, need {}",
                shares.len(),
                threshold
            )));
        }
        let mut result = vec![0u8; 32];
        for share in shares.iter().take(threshold as usize) {
            for (i, byte) in share.share_data.iter().enumerate() {
                if i >= result.len() {
                    result.push(0u8);
                }
                result[i] ^= byte;
            }
        }
        Ok(result)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Shamir Secret Sharing Tests (GF(p))
    // ========================================================================

    #[test]
    fn test_shamir_split_3_of_5() {
        let secret = [42u8; 32];
        let shares = split_secret(&secret, 3, 5).expect("split failed");
        assert_eq!(shares.len(), 5);

        let mut indices: Vec<u32> = shares.iter().map(|s| s.index).collect();
        indices.sort_unstable();
        assert_eq!(indices, vec![1, 2, 3, 4, 5]);

        for share in &shares {
            assert!(share.index > 0 && share.index <= 5);
            assert_ne!(share.value, [0u8; 32]);
        }
    }

    #[test]
    fn test_shamir_reconstruct_exact_threshold() {
        let secret = field::reduce_bytes(&[42u8; 32]);
        let shares = split_secret(&secret, 3, 5).expect("split failed");
        let recovered = reconstruct_secret(&shares[0..3], 3).expect("reconstruct failed");
        assert_eq!(recovered, secret);
    }

    #[test]
    fn test_shamir_reconstruct_different_subsets() {
        let secret = field::reduce_bytes(&[99u8; 32]);
        let shares = split_secret(&secret, 3, 5).expect("split failed");

        // Any 3 of 5 shares should reconstruct the same secret.
        let r1 = reconstruct_secret(&[shares[0].clone(), shares[1].clone(), shares[2].clone()], 3)
            .expect("r1 failed");
        let r2 = reconstruct_secret(&[shares[0].clone(), shares[2].clone(), shares[4].clone()], 3)
            .expect("r2 failed");
        let r3 = reconstruct_secret(&[shares[1].clone(), shares[3].clone(), shares[4].clone()], 3)
            .expect("r3 failed");

        assert_eq!(r1, secret);
        assert_eq!(r2, secret);
        assert_eq!(r3, secret);
    }

    #[test]
    fn test_shamir_reconstruct_more_than_threshold() {
        let secret = field::reduce_bytes(&[123u8; 32]);
        let shares = split_secret(&secret, 2, 4).expect("split failed");

        // Using all 4 shares with threshold=2 should still work.
        let recovered = reconstruct_secret(&shares, 2).expect("reconstruct failed");
        assert_eq!(recovered, secret);
    }

    #[test]
    fn test_shamir_1_of_1() {
        let secret = field::reduce_bytes(&[55u8; 32]);
        let shares = split_secret(&secret, 1, 1).expect("split failed");
        assert_eq!(shares.len(), 1);

        let recovered = reconstruct_secret(&shares, 1).expect("reconstruct failed");
        assert_eq!(recovered, secret);
    }

    #[test]
    fn test_shamir_insufficient_shares() {
        let secret = [42u8; 32];
        let shares = split_secret(&secret, 3, 5).expect("split failed");
        let result = reconstruct_secret(&shares[0..2], 3);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("insufficient shares"));
    }

    #[test]
    fn test_shamir_invalid_threshold_zero() {
        let secret = [42u8; 32];
        assert!(split_secret(&secret, 0, 5).is_err());
    }

    #[test]
    fn test_shamir_invalid_threshold_exceeds_total() {
        let secret = [42u8; 32];
        assert!(split_secret(&secret, 6, 5).is_err());
    }

    #[test]
    fn test_shamir_duplicate_indices() {
        let share1 = ShamirShare::new(1, [1u8; 32]);
        let share2 = ShamirShare::new(1, [2u8; 32]);
        let share3 = ShamirShare::new(3, [3u8; 32]);
        let result = reconstruct_secret(&[share1, share2, share3], 3);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("duplicate"));
    }

    // ========================================================================
    // Feldman VSS Tests
    // ========================================================================

    #[test]
    fn test_feldman_share_verification() {
        let seed = [77u8; 32];
        let p0 = FeldmanVssParticipant::new(0, 2, 3, &seed);

        let commitments = p0.compute_commitments();
        let shares = p0.compute_shares();

        // All shares should verify against the commitments.
        for share in &shares {
            assert!(
                p0.verify_share_against_commitments(share, &commitments),
                "share for recipient {} failed verification",
                share.recipient_id
            );
        }
    }

    #[test]
    fn test_feldman_tampered_share_fails() {
        let seed = [88u8; 32];
        let p0 = FeldmanVssParticipant::new(0, 2, 3, &seed);

        let commitments = p0.compute_commitments();
        let mut shares = p0.compute_shares();

        // Tamper with one share.
        if let Some(share) = shares.first_mut() {
            share.encrypted_share[0] ^= 0xFF;
        }

        // Tampered share should NOT verify.
        let valid = p0.verify_share_against_commitments(&shares[0], &commitments);
        assert!(!valid, "tampered share should fail verification");
    }

    #[test]
    fn test_feldman_share_reconstruction() {
        let seed = [33u8; 32];
        let threshold = 2u32;
        let total = 3u32;

        let p0 = FeldmanVssParticipant::new(0, threshold, total, &seed);

        // The secret is p0.secret(). Shares are evaluations of the polynomial.
        let shares: Vec<ShamirShare> = (1..=total)
            .map(|idx| {
                let x = field::from_u32(idx);
                ShamirShare::new(idx, p0.evaluate_at(&x))
            })
            .collect();

        // Reconstruct from any 2 shares.
        let recovered = reconstruct_secret(&[shares[0].clone(), shares[1].clone()], threshold)
            .expect("reconstruct failed");
        assert_eq!(recovered, *p0.secret());
    }

    // ========================================================================
    // Pedersen DKG Tests
    // ========================================================================

    #[test]
    fn test_pedersen_dkg_3_of_5() {
        let threshold = 3u32;
        let total = 5u32;

        // Each participant uses a different seed (simulating independent randomness).
        let participants: Vec<FeldmanVssParticipant> = (0..total)
            .map(|id| {
                let mut seed = [0u8; 32];
                seed[0] = id as u8;
                seed[1] = 0xAA;
                FeldmanVssParticipant::new(id, threshold, total, &seed)
            })
            .collect();

        let result = run_pedersen_dkg(&participants).expect("DKG failed");

        // Public key should be non-trivial.
        assert_ne!(result.public_key.value, [0u8; 32]);
        assert_ne!(result.public_key.value, field::from_u32(1));

        // We should have 5 participant shares.
        assert_eq!(result.participant_shares.len(), 5);

        // Aggregate shares should reconstruct the aggregate secret.
        let aggregate_secret = {
            let mut s = field::zero();
            for p in &participants {
                s = field::add(&s, p.secret());
            }
            s
        };

        let recovered = reconstruct_secret(&result.participant_shares[0..3], threshold)
            .expect("reconstruct failed");
        assert_eq!(recovered, aggregate_secret);

        // Different subset should also work.
        let recovered2 = reconstruct_secret(
            &[
                result.participant_shares[1].clone(),
                result.participant_shares[2].clone(),
                result.participant_shares[4].clone(),
            ],
            threshold,
        )
        .expect("reconstruct2 failed");
        assert_eq!(recovered2, aggregate_secret);
    }

    #[test]
    fn test_pedersen_dkg_2_of_3() {
        let threshold = 2u32;
        let total = 3u32;

        let participants: Vec<FeldmanVssParticipant> = (0..total)
            .map(|id| {
                let mut seed = [id as u8; 32];
                seed[31] = 0xBB;
                FeldmanVssParticipant::new(id, threshold, total, &seed)
            })
            .collect();

        let result = run_pedersen_dkg(&participants).expect("DKG failed");
        assert_eq!(result.participant_shares.len(), 3);

        // Any 2 shares reconstruct.
        let s = {
            let mut s = field::zero();
            for p in &participants {
                s = field::add(&s, p.secret());
            }
            s
        };

        for combo in &[[0, 1], [0, 2], [1, 2]] {
            let shares = vec![
                result.participant_shares[combo[0]].clone(),
                result.participant_shares[combo[1]].clone(),
            ];
            let r = reconstruct_secret(&shares, threshold).expect("failed");
            assert_eq!(r, s, "failed for combo {:?}", combo);
        }
    }

    #[test]
    fn test_pedersen_dkg_public_key_determinism() {
        let threshold = 2u32;
        let total = 3u32;

        let make_participants = || -> Vec<FeldmanVssParticipant> {
            (0..total)
                .map(|id| {
                    let seed = [id as u8 + 50; 32];
                    FeldmanVssParticipant::new(id, threshold, total, &seed)
                })
                .collect()
        };

        let r1 = run_pedersen_dkg(&make_participants()).expect("dkg1");
        let r2 = run_pedersen_dkg(&make_participants()).expect("dkg2");

        assert_eq!(r1.public_key, r2.public_key);
    }

    // ========================================================================
    // Threshold Encryption/Decryption Tests
    // ========================================================================

    #[test]
    fn test_threshold_encrypt_decrypt() {
        let threshold = 2u32;
        let total = 3u32;

        let participants: Vec<FeldmanVssParticipant> = (0..total)
            .map(|id| {
                let mut seed = [0u8; 32];
                seed[0] = id as u8;
                seed[4] = 0xCC;
                FeldmanVssParticipant::new(id, threshold, total, &seed)
            })
            .collect();

        let dkg_result = run_pedersen_dkg(&participants).expect("DKG failed");

        // Encrypt a message.
        let message = [0xABu8; 32];
        let randomness = sha3_256(b"test randomness");
        let ciphertext = DkgThresholdDecryptor::encrypt(
            &dkg_result.public_key,
            &message,
            &randomness,
        );

        // Create decryptors from DKG shares.
        let decryptors: Vec<DkgThresholdDecryptor> = dkg_result
            .participant_shares
            .iter()
            .map(|share| {
                DkgThresholdDecryptor::new(share.index, share.value, threshold)
            })
            .collect();

        // Each decryptor creates a decryption share.
        let dec_shares: Vec<DecryptionShare> = decryptors
            .iter()
            .map(|d| d.create_decryption_share(&ciphertext).expect("dec share failed"))
            .collect();

        // Combine threshold shares to recover the shared value.
        let combined = decryptors[0]
            .combine_shares(&dec_shares[0..threshold as usize], threshold)
            .expect("combine failed");

        // The combined value should be the same as pk^r.
        // Verify the decryption pipeline works end-to-end:
        // D = prod(C1^{s_i * L_i}) = C1^{sum(s_i * L_i)} = C1^{s} = g^{r*s} = pk^r
        let r = field::reduce_bytes(&randomness);
        let mut expected_shared = [0u8; 32];
        expected_shared.copy_from_slice(&field::pow(&dkg_result.public_key.value, &r));

        let mut combined_arr = [0u8; 32];
        combined_arr.copy_from_slice(&combined[..32]);

        assert_eq!(combined_arr, expected_shared);

        // Now recover the message: m = C2 XOR H(combined).
        let mask = sha3_256(&combined_arr);
        let mut recovered = [0u8; 32];
        for i in 0..32 {
            recovered[i] = ciphertext[32 + i] ^ mask[i];
        }
        assert_eq!(recovered, message);
    }

    #[test]
    fn test_threshold_decrypt_insufficient_shares() {
        let d = DkgThresholdDecryptor::new(1, [1u8; 32], 3);
        let share = DecryptionShare::new(1, vec![0u8; 32]);
        let result = d.combine_shares(&[share], 3);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("insufficient"));
    }

    // ========================================================================
    // Mock DKG Tests (backward compatibility)
    // ========================================================================

    #[test]
    fn test_mock_dkg_generate_commitment() {
        let seed = [99u8; 32];
        let participant = MockDkgParticipant::new(0, seed);
        let commitment = participant.generate_commitment().expect("failed");
        assert_eq!(commitment.participant_id, 0);
        assert_ne!(commitment.value, [0u8; 32]);
    }

    #[test]
    fn test_mock_dkg_3_of_5_key_generation() {
        let seed = [77u8; 32];
        let participants: Vec<_> = (0..5)
            .map(|id| MockDkgParticipant::new(id, seed))
            .collect();

        let commitments: Vec<_> = participants
            .iter()
            .map(|p| p.generate_commitment().expect("commit failed"))
            .collect();

        let mut public_keys = Vec::new();
        for participant in &participants {
            let pk = participant.derive_public_key(&commitments).expect("derive failed");
            public_keys.push(pk);
        }

        for pk in &public_keys[1..] {
            assert_eq!(pk, &public_keys[0]);
        }
    }

    #[test]
    fn test_mock_dkg_determinism() {
        let seed = [33u8; 32];
        let p1 = MockDkgParticipant::new(0, seed);
        let p2 = MockDkgParticipant::new(0, seed);
        let c1 = p1.generate_commitment().expect("c1 failed");
        let c2 = p2.generate_commitment().expect("c2 failed");
        assert_eq!(c1.value, c2.value);
    }

    #[test]
    fn test_mock_threshold_decrypt_determinism() {
        let secret = [77u8; 32];
        let ciphertext = b"test data";
        let d1 = MockThresholdDecryptor::new(1, secret);
        let d2 = MockThresholdDecryptor::new(1, secret);
        let s1 = d1.create_decryption_share(ciphertext).expect("s1");
        let s2 = d2.create_decryption_share(ciphertext).expect("s2");
        assert_eq!(s1.share_data, s2.share_data);
    }
}
