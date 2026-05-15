//! Genesis ceremony — multi-party trusted setup for mainnet genesis block.
//!
//! # Overview
//!
//! The genesis ceremony coordinates N participants to collectively produce:
//! 1. A verifiable epoch nonce (from combined randomness contributions)
//! 2. An initial stake distribution (from registered validators)
//! 3. The genesis block (built deterministically from ceremony output)
//! 4. A ceremony transcript (publicly auditable proof of correct execution)
//!
//! # Security Properties
//!
//! - **Randomness**: Epoch nonce is `SHA3-256(sorted contributions)` — secure as long as
//!   at least one participant contributes honest randomness.
//! - **Verifiability**: Each contribution is signed with Dilithium; transcript is
//!   publicly verifiable by any observer.
//! - **Determinism**: Given the same set of verified contributions, any party
//!   independently derives the same genesis block.
//! - **No trusted dealer**: No single party controls the epoch nonce or stake distribution.
//!
//! # Protocol Phases
//!
//! ```text
//! Phase 0: Setup      — Coordinator publishes ceremony parameters
//! Phase 1: Register   — Participants submit signed registration (VRF key + stake pledge)
//! Phase 2: Contribute — Each participant submits a signed randomness contribution
//! Phase 3: Finalize   — Coordinator aggregates, builds genesis, publishes transcript
//! ```
//!
//! # Usage
//!
//! ```text
//! use qv_node::ceremony::{CeremonyParams, CeremonyCoordinator, Participant};
//!
//! // Coordinator sets up the ceremony.
//! let params = CeremonyParams::mainnet_default();
//! let mut coordinator = CeremonyCoordinator::new(params);
//!
//! // Each participant produces a signed registration with their PQC keypair,
//! // stake pledge, and VRF/KES public keys (see `Participant::create_registration`).
//! coordinator.accept_registration(registration)?;
//!
//! // Each participant submits a signed randomness contribution
//! // (see `Participant::create_contribution`).
//! coordinator.accept_contribution(contribution)?;
//!
//! // Coordinator finalizes — yields genesis block, transcript, and epoch nonce.
//! let result = coordinator.finalize()?;
//! // result.genesis_block, result.transcript, result.epoch_nonce
//! ```

use qv_core::{
    merkle_root_of, Amount, Block, BlockHash, BlockHeader, Hash256, Height, Script, Slot,
    Timestamp, Transaction, TxOutput, UtxoCommitment, BLOCK_VERSION,
};
use qv_crypto::{
    sha3_256, sign_pqc, verify_pqc, DilithiumLevel, PqcPublicKey, PqcSecretKey, PqcSignature,
};
use qv_script::templates::{p2pkh_pqc, pubkey_hash};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;
use tracing::info;

// ============================================================================
// Error types
// ============================================================================

/// Errors that can occur during the genesis ceremony.
#[derive(Debug, Error)]
pub enum CeremonyError {
    /// Ceremony is not in the expected phase for this operation.
    #[error("wrong phase: expected {expected}, currently in {current}")]
    WrongPhase {
        expected: &'static str,
        current: &'static str,
    },

    /// A registration failed validation.
    #[error("invalid registration: {0}")]
    InvalidRegistration(String),

    /// A contribution failed validation.
    #[error("invalid contribution: {0}")]
    InvalidContribution(String),

    /// Duplicate participant detected.
    #[error("duplicate participant: {0}")]
    DuplicateParticipant(String),

    /// Insufficient participants to finalize.
    #[error("insufficient participants: need {needed}, have {have}")]
    InsufficientParticipants { needed: usize, have: usize },

    /// Contribution count doesn't match registration count.
    #[error("missing contributions: {registered} registered, {contributed} contributed")]
    MissingContributions {
        registered: usize,
        contributed: usize,
    },

    /// Cryptographic verification failed.
    #[error("signature verification failed: {0}")]
    SignatureVerification(String),

    /// Serialization error.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// Total stake exceeds monetary supply cap.
    #[error("total stake {total} exceeds supply cap {cap}")]
    StakeOverflow { total: u64, cap: u64 },
}

/// Result alias for ceremony operations.
pub type CeremonyResult<T> = Result<T, CeremonyError>;

// ============================================================================
// Ceremony parameters
// ============================================================================

/// Configuration for the genesis ceremony.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CeremonyParams {
    /// Network identifier (mainnet, testnet, devnet).
    pub network: String,

    /// Minimum number of participants required to finalize.
    pub min_participants: usize,

    /// Maximum number of participants allowed.
    pub max_participants: usize,

    /// Maximum total stake (smallest units) — cannot exceed monetary supply.
    pub max_total_stake: u64,

    /// Minimum stake per participant (smallest units).
    pub min_stake_per_participant: u64,

    /// Dilithium security level required for participant keys ("Level2", "Level3", "Level5").
    pub required_dilithium_level: String,

    /// Domain separator for contribution signing.
    pub domain: String,

    /// Genesis timestamp (Unix seconds). Zero means "use ceremony finalization time".
    pub genesis_timestamp: u64,
}

impl CeremonyParams {
    /// Parse the Dilithium level from the string field.
    pub fn dilithium_level(&self) -> DilithiumLevel {
        match self.required_dilithium_level.as_str() {
            "Level2" => DilithiumLevel::Level2,
            "Level5" => DilithiumLevel::Level5,
            _ => DilithiumLevel::Level3, // default
        }
    }

    /// Default parameters for mainnet ceremony.
    pub fn mainnet_default() -> Self {
        Self {
            network: "mainnet".to_string(),
            min_participants: 3,
            max_participants: 100,
            max_total_stake: 2_100_000_000_000_000, // 21M QV in smallest units
            min_stake_per_participant: 1_000_000_000, // 10 QV minimum
            required_dilithium_level: "Level3".to_string(),
            domain: "QuantumVault/GenesisCeremony/v1/mainnet".to_string(),
            genesis_timestamp: 0,
        }
    }

    /// Default parameters for testnet ceremony.
    pub fn testnet_default() -> Self {
        Self {
            network: "testnet".to_string(),
            min_participants: 2,
            max_participants: 50,
            max_total_stake: 2_100_000_000_000_000,
            min_stake_per_participant: 100_000_000,
            required_dilithium_level: "Level3".to_string(),
            domain: "QuantumVault/GenesisCeremony/v1/testnet".to_string(),
            genesis_timestamp: 0,
        }
    }

    /// Minimal parameters for devnet (single participant OK).
    pub fn devnet_default() -> Self {
        Self {
            network: "devnet".to_string(),
            min_participants: 1,
            max_participants: 10,
            max_total_stake: 2_100_000_000_000_000,
            min_stake_per_participant: 1,
            required_dilithium_level: "Level3".to_string(),
            domain: "QuantumVault/GenesisCeremony/v1/devnet".to_string(),
            genesis_timestamp: 0,
        }
    }
}

// ============================================================================
// Participant data structures
// ============================================================================

/// A participant's registration message (Phase 1).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Registration {
    /// Participant's Dilithium public key (identity).
    pub identity_key: Vec<u8>,

    /// VRF public key for slot leader election.
    pub vrf_key: Vec<u8>,

    /// Initial stake pledge (smallest units).
    pub stake_pledge: u64,

    /// Optional reward address (defaults to identity key hash).
    pub reward_address: Option<Vec<u8>>,

    /// Signature over the registration payload (Dilithium).
    pub signature: Vec<u8>,
}

/// A participant's randomness contribution (Phase 2).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Contribution {
    /// Participant's Dilithium public key (must match a registration).
    pub identity_key: Vec<u8>,

    /// 32 bytes of entropy contributed by this participant.
    pub randomness: [u8; 32],

    /// Signature over `domain || randomness` (Dilithium).
    pub signature: Vec<u8>,
}

/// Verified registration (internal representation after signature check).
#[derive(Clone, Debug)]
struct VerifiedRegistration {
    identity_key: PqcPublicKey,
    _vrf_key: Vec<u8>,
    stake_pledge: u64,
    _reward_address: Vec<u8>,
}

/// Verified contribution (internal representation after signature check).
#[derive(Clone, Debug)]
struct VerifiedContribution {
    _identity_key_hash: [u8; 32],
    randomness: [u8; 32],
}

// ============================================================================
// Ceremony state machine
// ============================================================================

/// The current phase of the ceremony.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CeremonyPhase {
    /// Accepting registrations.
    Registration,
    /// Accepting randomness contributions.
    Contribution,
    /// Ceremony finalized — no more inputs accepted.
    Finalized,
}

impl CeremonyPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Registration => "Registration",
            Self::Contribution => "Contribution",
            Self::Finalized => "Finalized",
        }
    }
}

/// The output of a successfully finalized ceremony.
#[derive(Clone, Debug)]
pub struct CeremonyResult2 {
    /// The genesis block, built deterministically from ceremony data.
    pub genesis_block: Block,

    /// The derived epoch nonce (SHA3-256 of combined randomness).
    pub epoch_nonce: [u8; 32],

    /// Full ceremony transcript for public audit.
    pub transcript: CeremonyTranscript,
}

/// Publicly auditable ceremony transcript.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CeremonyTranscript {
    /// Ceremony parameters used.
    pub params: CeremonyParams,

    /// All registrations in submission order.
    pub registrations: Vec<Registration>,

    /// All contributions in submission order.
    pub contributions: Vec<Contribution>,

    /// Derived epoch nonce (for verification).
    pub epoch_nonce: [u8; 32],

    /// Genesis block hash.
    pub genesis_block_hash: String,

    /// Total stake allocated.
    pub total_stake: u64,

    /// Number of participants.
    pub participant_count: usize,

    /// Finalization timestamp (Unix seconds).
    pub finalized_at: u64,
}

// ============================================================================
// Coordinator
// ============================================================================

/// Orchestrates the genesis ceremony, validates inputs, and produces the genesis block.
pub struct CeremonyCoordinator {
    params: CeremonyParams,
    phase: CeremonyPhase,
    /// Registrations keyed by identity key hash (sorted for determinism).
    registrations: BTreeMap<[u8; 32], VerifiedRegistration>,
    /// Contributions keyed by identity key hash.
    contributions: BTreeMap<[u8; 32], VerifiedContribution>,
    /// Raw registrations (for transcript).
    raw_registrations: Vec<Registration>,
    /// Raw contributions (for transcript).
    raw_contributions: Vec<Contribution>,
}

impl CeremonyCoordinator {
    /// Create a new ceremony coordinator with the given parameters.
    pub fn new(params: CeremonyParams) -> Self {
        info!(
            network = %params.network,
            min_participants = params.min_participants,
            max_participants = params.max_participants,
            "genesis ceremony initialized"
        );

        Self {
            params,
            phase: CeremonyPhase::Registration,
            registrations: BTreeMap::new(),
            contributions: BTreeMap::new(),
            raw_registrations: Vec::new(),
            raw_contributions: Vec::new(),
        }
    }

    /// Current ceremony phase.
    pub fn phase(&self) -> CeremonyPhase {
        self.phase
    }

    /// Number of registered participants.
    pub fn registration_count(&self) -> usize {
        self.registrations.len()
    }

    /// Number of contributions received.
    pub fn contribution_count(&self) -> usize {
        self.contributions.len()
    }

    /// Accept a participant's registration. Must be in Registration phase.
    pub fn accept_registration(&mut self, reg: Registration) -> CeremonyResult<()> {
        if self.phase != CeremonyPhase::Registration {
            return Err(CeremonyError::WrongPhase {
                expected: "Registration",
                current: self.phase.as_str(),
            });
        }

        if self.registrations.len() >= self.params.max_participants {
            return Err(CeremonyError::InvalidRegistration(
                "maximum participants reached".to_string(),
            ));
        }

        // Validate stake pledge.
        if reg.stake_pledge < self.params.min_stake_per_participant {
            return Err(CeremonyError::InvalidRegistration(format!(
                "stake {} below minimum {}",
                reg.stake_pledge, self.params.min_stake_per_participant
            )));
        }

        // Parse identity key (level from ceremony params).
        let level = self.params.dilithium_level();
        let identity_key =
            PqcPublicKey::from_bytes(level, reg.identity_key.clone()).map_err(|e| {
                CeremonyError::InvalidRegistration(format!("invalid identity key: {e}"))
            })?;

        // Verify key level matches requirement.
        if identity_key.level() != level {
            return Err(CeremonyError::InvalidRegistration(format!(
                "key level {:?} does not match required {:?}",
                identity_key.level(),
                level
            )));
        }

        // Compute identity key hash for deduplication.
        let key_hash = sha3_256(&reg.identity_key);

        if self.registrations.contains_key(&key_hash) {
            return Err(CeremonyError::DuplicateParticipant(hex::encode(
                &key_hash[..8],
            )));
        }

        // Verify signature over the registration payload.
        let payload = self.registration_signing_payload(&reg);
        let sig = PqcSignature::from_bytes(level, reg.signature.clone()).map_err(|e| {
            CeremonyError::SignatureVerification(format!("invalid signature bytes: {e}"))
        })?;

        match verify_pqc(&identity_key, &payload, &sig) {
            Ok(true) => {}
            Ok(false) => {
                return Err(CeremonyError::SignatureVerification(
                    "signature verification returned false".to_string(),
                ));
            }
            Err(e) => {
                return Err(CeremonyError::SignatureVerification(format!(
                    "verification error: {e}"
                )));
            }
        }

        // Check total stake won't overflow.
        let current_total: u64 = self.registrations.values().map(|r| r.stake_pledge).sum();
        let new_total =
            current_total
                .checked_add(reg.stake_pledge)
                .ok_or(CeremonyError::StakeOverflow {
                    total: u64::MAX,
                    cap: self.params.max_total_stake,
                })?;
        if new_total > self.params.max_total_stake {
            return Err(CeremonyError::StakeOverflow {
                total: new_total,
                cap: self.params.max_total_stake,
            });
        }

        // Reward address defaults to hash of identity key.
        let reward_address = reg
            .reward_address
            .clone()
            .unwrap_or_else(|| key_hash.to_vec());

        // Store verified registration.
        let verified = VerifiedRegistration {
            identity_key,
            _vrf_key: reg.vrf_key.clone(),
            stake_pledge: reg.stake_pledge,
            _reward_address: reward_address,
        };

        self.registrations.insert(key_hash, verified);
        self.raw_registrations.push(reg);

        info!(
            participant = hex::encode(&key_hash[..8]),
            stake = self.registrations[&key_hash].stake_pledge,
            total_registered = self.registrations.len(),
            "registration accepted"
        );

        Ok(())
    }

    /// Advance from Registration phase to Contribution phase.
    ///
    /// Requires at least `min_participants` registrations.
    pub fn close_registration(&mut self) -> CeremonyResult<()> {
        if self.phase != CeremonyPhase::Registration {
            return Err(CeremonyError::WrongPhase {
                expected: "Registration",
                current: self.phase.as_str(),
            });
        }

        if self.registrations.len() < self.params.min_participants {
            return Err(CeremonyError::InsufficientParticipants {
                needed: self.params.min_participants,
                have: self.registrations.len(),
            });
        }

        self.phase = CeremonyPhase::Contribution;
        info!(
            participants = self.registrations.len(),
            "registration phase closed, accepting contributions"
        );
        Ok(())
    }

    /// Accept a participant's randomness contribution. Must be in Contribution phase.
    pub fn accept_contribution(&mut self, contrib: Contribution) -> CeremonyResult<()> {
        if self.phase != CeremonyPhase::Contribution {
            return Err(CeremonyError::WrongPhase {
                expected: "Contribution",
                current: self.phase.as_str(),
            });
        }

        // Compute identity key hash.
        let key_hash = sha3_256(&contrib.identity_key);

        // Verify participant is registered.
        let registration = self.registrations.get(&key_hash).ok_or_else(|| {
            CeremonyError::InvalidContribution("participant not registered".to_string())
        })?;

        // Check for duplicate contribution.
        if self.contributions.contains_key(&key_hash) {
            return Err(CeremonyError::DuplicateParticipant(hex::encode(
                &key_hash[..8],
            )));
        }

        // Verify signature over domain || randomness.
        let level = self.params.dilithium_level();
        let payload = self.contribution_signing_payload(&contrib);
        let sig = PqcSignature::from_bytes(level, contrib.signature.clone()).map_err(|e| {
            CeremonyError::SignatureVerification(format!("invalid signature bytes: {e}"))
        })?;

        match verify_pqc(&registration.identity_key, &payload, &sig) {
            Ok(true) => {}
            Ok(false) => {
                return Err(CeremonyError::SignatureVerification(
                    "contribution signature invalid".to_string(),
                ));
            }
            Err(e) => {
                return Err(CeremonyError::SignatureVerification(format!(
                    "verification error: {e}"
                )));
            }
        }

        // Reject all-zero randomness (likely a mistake).
        if contrib.randomness == [0u8; 32] {
            return Err(CeremonyError::InvalidContribution(
                "randomness must not be all zeros".to_string(),
            ));
        }

        // Store verified contribution.
        let verified = VerifiedContribution {
            _identity_key_hash: key_hash,
            randomness: contrib.randomness,
        };

        self.contributions.insert(key_hash, verified);
        self.raw_contributions.push(contrib);

        info!(
            participant = hex::encode(&key_hash[..8]),
            contributions = self.contributions.len(),
            total_expected = self.registrations.len(),
            "contribution accepted"
        );

        Ok(())
    }

    /// Finalize the ceremony: derive epoch nonce, build genesis block, produce transcript.
    ///
    /// Requires all registered participants to have contributed.
    pub fn finalize(mut self) -> CeremonyResult<CeremonyResult2> {
        if self.phase != CeremonyPhase::Contribution {
            return Err(CeremonyError::WrongPhase {
                expected: "Contribution",
                current: self.phase.as_str(),
            });
        }

        // All registered participants must contribute.
        if self.contributions.len() < self.registrations.len() {
            return Err(CeremonyError::MissingContributions {
                registered: self.registrations.len(),
                contributed: self.contributions.len(),
            });
        }

        self.phase = CeremonyPhase::Finalized;

        // ====================================================================
        // Step 1: Derive epoch nonce from combined randomness.
        // Sort contributions by key hash (BTreeMap already sorted) for determinism.
        // ====================================================================
        let epoch_nonce = self.derive_epoch_nonce();

        info!(
            epoch_nonce = hex::encode(epoch_nonce),
            "epoch nonce derived from {} contributions",
            self.contributions.len()
        );

        // ====================================================================
        // Step 2: Build genesis block outputs from registrations.
        // Each participant gets a p2pkh_pqc output locked to their identity key.
        // ====================================================================
        let genesis_block = self.build_genesis_block();

        let genesis_hash = genesis_block
            .hash()
            .map_err(|e| CeremonyError::Serialization(format!("block hash failed: {e}")))?;

        info!(
            genesis_hash = %genesis_hash,
            outputs = genesis_block.transactions[0].outputs.len(),
            "genesis block assembled"
        );

        // ====================================================================
        // Step 3: Produce ceremony transcript.
        // ====================================================================
        let total_stake: u64 = self.registrations.values().map(|r| r.stake_pledge).sum();

        let transcript = CeremonyTranscript {
            params: self.params.clone(),
            registrations: self.raw_registrations.clone(),
            contributions: self.raw_contributions.clone(),
            epoch_nonce,
            genesis_block_hash: genesis_hash.to_hex(),
            total_stake,
            participant_count: self.registrations.len(),
            finalized_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        };

        info!(
            total_stake = total_stake,
            participants = transcript.participant_count,
            "ceremony finalized successfully"
        );

        Ok(CeremonyResult2 {
            genesis_block,
            epoch_nonce,
            transcript,
        })
    }

    // ========================================================================
    // Internal helpers
    // ========================================================================

    /// Derive epoch nonce: `SHA3-256(domain || sorted_contributions)`.
    fn derive_epoch_nonce(&self) -> [u8; 32] {
        let mut hasher_input = Vec::new();

        // Domain separation.
        hasher_input.extend_from_slice(self.params.domain.as_bytes());
        hasher_input.extend_from_slice(b"/epoch_nonce/v1");

        // Append contributions in sorted key-hash order (BTreeMap guarantees this).
        for (key_hash, contrib) in &self.contributions {
            hasher_input.extend_from_slice(key_hash);
            hasher_input.extend_from_slice(&contrib.randomness);
        }

        sha3_256(&hasher_input)
    }

    /// Build the genesis block from verified registrations.
    #[allow(clippy::expect_used)] // SAFETY: genesis tx id is infallible by construction
    fn build_genesis_block(&self) -> Block {
        // Build outputs: one per participant, ordered by key hash (deterministic).
        let mut outputs = Vec::with_capacity(self.registrations.len());

        for reg in self.registrations.values() {
            let pk_hash = pubkey_hash(reg.identity_key.as_bytes());
            let locking_script = p2pkh_pqc(&pk_hash);
            let output = TxOutput::new(Amount::from(reg.stake_pledge), Script::new(locking_script));
            outputs.push(output);
        }

        // Genesis transaction: no inputs.
        let genesis_tx = Transaction::genesis(outputs);

        // Compute merkle root.
        let tx_id = genesis_tx
            .id()
            .expect("genesis transaction must produce a valid id");
        let merkle = merkle_root_of(&[tx_id]);

        // Timestamp: use configured value or default to zero.
        let timestamp = if self.params.genesis_timestamp > 0 {
            Timestamp::from_unix_secs(self.params.genesis_timestamp)
        } else {
            Timestamp::from_unix_secs(0)
        };

        let header = BlockHeader {
            version: BLOCK_VERSION,
            prev_hash: BlockHash::ZERO,
            height: Height::GENESIS,
            slot: Slot::GENESIS,
            timestamp,
            merkle_root: merkle,
            utxo_commitment: UtxoCommitment::ZERO,
            vrf_proof: Vec::new(),
            kes_sig: Vec::new(),
            producer_key_hash: Hash256::ZERO,
        };

        Block::new(header, vec![genesis_tx])
    }

    /// Construct the payload that participants sign for registration.
    fn registration_signing_payload(&self, reg: &Registration) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(self.params.domain.as_bytes());
        payload.extend_from_slice(b"/register/v1");
        payload.extend_from_slice(&reg.identity_key);
        payload.extend_from_slice(&reg.vrf_key);
        payload.extend_from_slice(&reg.stake_pledge.to_le_bytes());
        if let Some(ref addr) = reg.reward_address {
            payload.extend_from_slice(addr);
        }
        payload
    }

    /// Construct the payload that participants sign for contribution.
    fn contribution_signing_payload(&self, contrib: &Contribution) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(self.params.domain.as_bytes());
        payload.extend_from_slice(b"/contribute/v1");
        payload.extend_from_slice(&contrib.identity_key);
        payload.extend_from_slice(&contrib.randomness);
        payload
    }
}

// ============================================================================
// Participant helper (client-side)
// ============================================================================

/// Helper functions for ceremony participants (client-side operations).
pub struct Participant;

impl Participant {
    /// Create a signed registration message.
    pub fn create_registration(
        secret_key: &PqcSecretKey,
        public_key: &PqcPublicKey,
        stake_pledge: u64,
        vrf_key: &[u8],
        domain: &str,
    ) -> CeremonyResult<Registration> {
        let identity_key_bytes = public_key.as_bytes().to_vec();

        // Build the signing payload.
        let mut payload = Vec::new();
        payload.extend_from_slice(domain.as_bytes());
        payload.extend_from_slice(b"/register/v1");
        payload.extend_from_slice(&identity_key_bytes);
        payload.extend_from_slice(vrf_key);
        payload.extend_from_slice(&stake_pledge.to_le_bytes());

        // Sign.
        let signature = sign_pqc(secret_key, &payload)
            .map_err(|e| CeremonyError::SignatureVerification(format!("signing failed: {e}")))?;

        Ok(Registration {
            identity_key: identity_key_bytes,
            vrf_key: vrf_key.to_vec(),
            stake_pledge,
            reward_address: None,
            signature: signature.as_bytes().to_vec(),
        })
    }

    /// Create a signed randomness contribution.
    pub fn create_contribution(
        secret_key: &PqcSecretKey,
        public_key: &PqcPublicKey,
        randomness: [u8; 32],
        domain: &str,
    ) -> CeremonyResult<Contribution> {
        let identity_key_bytes = public_key.as_bytes().to_vec();

        // Build the signing payload.
        let mut payload = Vec::new();
        payload.extend_from_slice(domain.as_bytes());
        payload.extend_from_slice(b"/contribute/v1");
        payload.extend_from_slice(&identity_key_bytes);
        payload.extend_from_slice(&randomness);

        // Sign.
        let signature = sign_pqc(secret_key, &payload)
            .map_err(|e| CeremonyError::SignatureVerification(format!("signing failed: {e}")))?;

        Ok(Contribution {
            identity_key: identity_key_bytes,
            randomness,
            signature: signature.as_bytes().to_vec(),
        })
    }
}

// ============================================================================
// Transcript verification (independent verifier)
// ============================================================================

/// Verify a ceremony transcript independently.
///
/// This can be run by any observer to confirm:
/// 1. All registrations have valid signatures.
/// 2. All contributions have valid signatures.
/// 3. The epoch nonce was correctly derived.
/// 4. The genesis block hash matches the transcript.
pub fn verify_transcript(transcript: &CeremonyTranscript) -> CeremonyResult<bool> {
    // Rebuild from the transcript to check determinism.
    let mut coordinator = CeremonyCoordinator::new(transcript.params.clone());

    // Replay registrations.
    for reg in &transcript.registrations {
        coordinator.accept_registration(reg.clone())?;
    }
    coordinator.close_registration()?;

    // Replay contributions.
    for contrib in &transcript.contributions {
        coordinator.accept_contribution(contrib.clone())?;
    }

    // Finalize and compare.
    let result = coordinator.finalize()?;

    // Verify epoch nonce matches.
    if result.epoch_nonce != transcript.epoch_nonce {
        return Err(CeremonyError::InvalidContribution(
            "epoch nonce mismatch during transcript verification".to_string(),
        ));
    }

    // Verify genesis block hash matches.
    let computed_hash = result
        .genesis_block
        .hash()
        .map_err(|e| CeremonyError::Serialization(format!("hash failed: {e}")))?;

    if computed_hash.to_hex() != transcript.genesis_block_hash {
        return Err(CeremonyError::Serialization(
            "genesis block hash mismatch during transcript verification".to_string(),
        ));
    }

    Ok(true)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use qv_crypto::generate_pqc_keypair;

    /// Generate a test participant (keypair + registration + contribution).
    fn make_test_participant(
        params: &CeremonyParams,
        stake: u64,
        entropy: [u8; 32],
    ) -> (PqcSecretKey, PqcPublicKey, Registration, Contribution) {
        let kp = generate_pqc_keypair(DilithiumLevel::Level3).unwrap();
        let vrf_key = vec![42u8; 32]; // placeholder VRF key

        let reg = Participant::create_registration(
            &kp.secret,
            &kp.public,
            stake,
            &vrf_key,
            &params.domain,
        )
        .unwrap();

        let contrib =
            Participant::create_contribution(&kp.secret, &kp.public, entropy, &params.domain)
                .unwrap();

        (kp.secret, kp.public, reg, contrib)
    }

    #[test]
    fn ceremony_happy_path_single_participant() {
        let params = CeremonyParams::devnet_default();
        let mut coordinator = CeremonyCoordinator::new(params.clone());

        let (_, _, reg, contrib) = make_test_participant(&params, 1_000_000_000, [1u8; 32]);

        coordinator.accept_registration(reg).unwrap();
        coordinator.close_registration().unwrap();
        coordinator.accept_contribution(contrib).unwrap();

        let result = coordinator.finalize().unwrap();

        // Genesis block must be valid.
        result.genesis_block.validate_structure().unwrap();

        // Epoch nonce must not be zero.
        assert_ne!(result.epoch_nonce, [0u8; 32]);

        // Transcript must be complete.
        assert_eq!(result.transcript.participant_count, 1);
        assert_eq!(result.transcript.total_stake, 1_000_000_000);
    }

    #[test]
    fn ceremony_happy_path_multiple_participants() {
        let params = CeremonyParams::testnet_default();
        let mut coordinator = CeremonyCoordinator::new(params.clone());

        let (_, _, reg1, contrib1) = make_test_participant(&params, 5_000_000_000, [1u8; 32]);
        let (_, _, reg2, contrib2) = make_test_participant(&params, 3_000_000_000, [2u8; 32]);
        let (_, _, reg3, contrib3) = make_test_participant(&params, 2_000_000_000, [3u8; 32]);

        coordinator.accept_registration(reg1).unwrap();
        coordinator.accept_registration(reg2).unwrap();
        coordinator.accept_registration(reg3).unwrap();
        coordinator.close_registration().unwrap();

        coordinator.accept_contribution(contrib1).unwrap();
        coordinator.accept_contribution(contrib2).unwrap();
        coordinator.accept_contribution(contrib3).unwrap();

        let result = coordinator.finalize().unwrap();

        result.genesis_block.validate_structure().unwrap();
        assert_eq!(result.transcript.participant_count, 3);
        assert_eq!(result.transcript.total_stake, 10_000_000_000);

        // Genesis block should have 3 outputs.
        assert_eq!(result.genesis_block.transactions[0].outputs.len(), 3);
    }

    #[test]
    fn ceremony_determinism() {
        // Running the same ceremony twice with same inputs must produce same output.
        let params = CeremonyParams::devnet_default();

        let kp = generate_pqc_keypair(DilithiumLevel::Level3).unwrap();
        let vrf_key = vec![99u8; 32];
        let entropy = [7u8; 32];

        let reg = Participant::create_registration(
            &kp.secret,
            &kp.public,
            1_000_000,
            &vrf_key,
            &params.domain,
        )
        .unwrap();
        let contrib =
            Participant::create_contribution(&kp.secret, &kp.public, entropy, &params.domain)
                .unwrap();

        // Run 1
        let mut c1 = CeremonyCoordinator::new(params.clone());
        c1.accept_registration(reg.clone()).unwrap();
        c1.close_registration().unwrap();
        c1.accept_contribution(contrib.clone()).unwrap();
        let r1 = c1.finalize().unwrap();

        // Run 2
        let mut c2 = CeremonyCoordinator::new(params.clone());
        c2.accept_registration(reg).unwrap();
        c2.close_registration().unwrap();
        c2.accept_contribution(contrib).unwrap();
        let r2 = c2.finalize().unwrap();

        assert_eq!(r1.epoch_nonce, r2.epoch_nonce);
        assert_eq!(
            r1.genesis_block.hash().unwrap(),
            r2.genesis_block.hash().unwrap()
        );
    }

    #[test]
    fn ceremony_rejects_wrong_phase() {
        let params = CeremonyParams::devnet_default();
        let mut coordinator = CeremonyCoordinator::new(params.clone());

        let (_, _, _, contrib) = make_test_participant(&params, 1_000_000, [1u8; 32]);

        // Try to contribute before registration closed.
        let err = coordinator.accept_contribution(contrib).unwrap_err();
        assert!(matches!(err, CeremonyError::WrongPhase { .. }));
    }

    #[test]
    fn ceremony_rejects_duplicate_registration() {
        let params = CeremonyParams::devnet_default();
        let mut coordinator = CeremonyCoordinator::new(params.clone());

        let kp = generate_pqc_keypair(DilithiumLevel::Level3).unwrap();
        let vrf_key = vec![42u8; 32];

        let reg1 = Participant::create_registration(
            &kp.secret,
            &kp.public,
            1_000_000,
            &vrf_key,
            &params.domain,
        )
        .unwrap();
        let reg2 = Participant::create_registration(
            &kp.secret,
            &kp.public,
            2_000_000,
            &vrf_key,
            &params.domain,
        )
        .unwrap();

        coordinator.accept_registration(reg1).unwrap();
        let err = coordinator.accept_registration(reg2).unwrap_err();
        assert!(matches!(err, CeremonyError::DuplicateParticipant(_)));
    }

    #[test]
    fn ceremony_rejects_zero_randomness() {
        let params = CeremonyParams::devnet_default();
        let mut coordinator = CeremonyCoordinator::new(params.clone());

        let kp = generate_pqc_keypair(DilithiumLevel::Level3).unwrap();
        let vrf_key = vec![42u8; 32];

        let reg = Participant::create_registration(
            &kp.secret,
            &kp.public,
            1_000_000,
            &vrf_key,
            &params.domain,
        )
        .unwrap();

        // Create contribution with all-zero randomness.
        let zero_contrib =
            Participant::create_contribution(&kp.secret, &kp.public, [0u8; 32], &params.domain)
                .unwrap();

        coordinator.accept_registration(reg).unwrap();
        coordinator.close_registration().unwrap();
        let err = coordinator.accept_contribution(zero_contrib).unwrap_err();
        assert!(matches!(err, CeremonyError::InvalidContribution(_)));
    }

    #[test]
    fn ceremony_rejects_insufficient_participants() {
        let params = CeremonyParams::testnet_default(); // min = 2
        let mut coordinator = CeremonyCoordinator::new(params.clone());

        let (_, _, reg, _) = make_test_participant(&params, 1_000_000_000, [1u8; 32]);
        coordinator.accept_registration(reg).unwrap();

        // Only 1 participant, need 2.
        let err = coordinator.close_registration().unwrap_err();
        assert!(matches!(
            err,
            CeremonyError::InsufficientParticipants { .. }
        ));
    }

    #[test]
    fn ceremony_rejects_stake_overflow() {
        let mut params = CeremonyParams::devnet_default();
        params.max_total_stake = 1_000_000; // very low cap

        let mut coordinator = CeremonyCoordinator::new(params.clone());

        let (_, _, reg, _) = make_test_participant(&params, 2_000_000, [1u8; 32]);
        let err = coordinator.accept_registration(reg).unwrap_err();
        assert!(matches!(err, CeremonyError::StakeOverflow { .. }));
    }

    #[test]
    fn transcript_verification() {
        let params = CeremonyParams::devnet_default();
        let mut coordinator = CeremonyCoordinator::new(params.clone());

        let (_, _, reg, contrib) = make_test_participant(&params, 1_000_000_000, [5u8; 32]);
        coordinator.accept_registration(reg).unwrap();
        coordinator.close_registration().unwrap();
        coordinator.accept_contribution(contrib).unwrap();

        let result = coordinator.finalize().unwrap();

        // Verify the transcript independently.
        let valid = verify_transcript(&result.transcript).unwrap();
        assert!(valid);
    }
}
