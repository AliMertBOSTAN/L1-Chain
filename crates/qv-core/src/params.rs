//! Protocol parameters — every tunable that must match across every node.
//!
//! All ledger-level invariants (slot duration, finality depth, supply, fee
//! model, ...) live here so they can be loaded once from `config/genesis.toml`
//! and handed to the rest of the workspace. Values come from `CLAUDE.md`'s
//! immutable architectural decisions; if you're considering changing one,
//! open an ADR first.
//!
//! # Why a single struct
//!
//! Scattering these constants across `const` items invites drift — every
//! new crate ends up with its own copy. Carrying them inside [`ProtocolParams`]
//! forces the node to be explicit about which parameter set it's running.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::types::{Amount, Slot};

// ============================================================================
// Errors
// ============================================================================

/// Errors produced when loading or validating protocol parameters.
#[derive(Debug, Error)]
pub enum ParamsError {
    /// Input TOML / JSON could not be parsed.
    #[error("failed to parse parameters: {0}")]
    Parse(String),

    /// Values parsed successfully but are mutually inconsistent.
    #[error("invalid parameter value: {0}")]
    Invalid(&'static str),
}

// ============================================================================
// Networks
// ============================================================================

/// Logical network identifier. Two blocks with different `NetworkId`s are
/// incompatible even if every other field matches, so the id is included in
/// the genesis hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkId {
    /// Production mainnet.
    Mainnet,
    /// Public testnet.
    Testnet,
    /// Local developer network.
    Devnet,
    /// Transient in-process testing network.
    Ephemeral,
}

impl NetworkId {
    /// Stable two-byte magic used in p2p handshakes.
    #[must_use]
    pub const fn magic(self) -> [u8; 2] {
        match self {
            Self::Mainnet => [0x51, 0x56], // "QV"
            Self::Testnet => [0x54, 0x56], // "TV"
            Self::Devnet => [0x44, 0x56],  // "DV"
            Self::Ephemeral => [0x45, 0x56],
        }
    }
}

// ============================================================================
// Sub-sections
// ============================================================================

/// Ouroboros-Praos consensus parameters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsensusParams {
    /// Slot duration in milliseconds. CLAUDE.md: 2 seconds.
    pub slot_duration_ms: u64,
    /// Slots per epoch. CLAUDE.md: 21_600 (≈ 12 h at 2 s/slot).
    pub epoch_slots: u64,
    /// Number of confirmations required for finality. CLAUDE.md: `k = 50`.
    pub k_finality: u64,
    /// Fraction of honest stake assumed, expressed as numerator / denominator.
    ///
    /// Default is `51 / 100` (a ≥51% honest stake assumption).
    pub honest_stake_num: u32,
    /// Denominator for [`honest_stake_num`]. Must be non-zero.
    pub honest_stake_den: u32,
}

impl ConsensusParams {
    /// Default mainnet consensus values derived from CLAUDE.md.
    #[must_use]
    pub const fn mainnet() -> Self {
        Self {
            slot_duration_ms: 2_000,
            epoch_slots: 21_600,
            k_finality: 50,
            honest_stake_num: 51,
            honest_stake_den: 100,
        }
    }

    /// Expected wall-clock epoch length in seconds.
    #[must_use]
    pub const fn epoch_seconds(&self) -> u64 {
        // slot_duration_ms may be large; cap multiplication via saturating.
        self.epoch_slots
            .saturating_mul(self.slot_duration_ms)
            .saturating_div(1_000)
    }

    /// Validate internal consistency.
    pub fn validate(&self) -> Result<(), ParamsError> {
        if self.slot_duration_ms == 0 {
            return Err(ParamsError::Invalid("slot_duration_ms must be > 0"));
        }
        if self.epoch_slots == 0 {
            return Err(ParamsError::Invalid("epoch_slots must be > 0"));
        }
        if self.k_finality == 0 {
            return Err(ParamsError::Invalid("k_finality must be > 0"));
        }
        if self.honest_stake_den == 0 {
            return Err(ParamsError::Invalid("honest_stake_den must be > 0"));
        }
        if self.honest_stake_num == 0 {
            return Err(ParamsError::Invalid("honest_stake_num must be > 0"));
        }
        if self.honest_stake_num > self.honest_stake_den {
            return Err(ParamsError::Invalid(
                "honest_stake_num must not exceed honest_stake_den",
            ));
        }
        Ok(())
    }
}

impl Default for ConsensusParams {
    fn default() -> Self {
        Self::mainnet()
    }
}

/// Ledger-layer limits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerParams {
    /// Maximum serialised block size in bytes.
    pub max_block_bytes: u32,
    /// Maximum serialised transaction size in bytes.
    pub max_tx_bytes: u32,
    /// Maximum number of inputs per transaction.
    pub max_tx_inputs: u32,
    /// Maximum number of outputs per transaction.
    pub max_tx_outputs: u32,
    /// Maximum number of transactions in a block.
    pub max_block_txs: u32,
    /// Maximum locking-script size in bytes.
    pub max_script_bytes: u32,
    /// Maximum datum size in bytes.
    pub max_datum_bytes: u32,
}

impl LedgerParams {
    /// Default mainnet ledger limits.
    #[must_use]
    pub const fn mainnet() -> Self {
        Self {
            max_block_bytes: 7_500_000, // ~7.5 MB target
            max_tx_bytes: 200_000,
            max_tx_inputs: 1_024,
            max_tx_outputs: 1_024,
            max_block_txs: 10_000,
            max_script_bytes: 64 * 1024,
            max_datum_bytes: 16 * 1024,
        }
    }

    /// Validate internal consistency.
    pub fn validate(&self) -> Result<(), ParamsError> {
        if self.max_block_bytes == 0 {
            return Err(ParamsError::Invalid("max_block_bytes must be > 0"));
        }
        if self.max_tx_bytes == 0 {
            return Err(ParamsError::Invalid("max_tx_bytes must be > 0"));
        }
        if self.max_tx_bytes > self.max_block_bytes {
            return Err(ParamsError::Invalid(
                "max_tx_bytes must not exceed max_block_bytes",
            ));
        }
        if self.max_tx_inputs == 0 || self.max_tx_outputs == 0 {
            return Err(ParamsError::Invalid(
                "tx input/output limits must be > 0",
            ));
        }
        if self.max_block_txs == 0 {
            return Err(ParamsError::Invalid("max_block_txs must be > 0"));
        }
        Ok(())
    }
}

impl Default for LedgerParams {
    fn default() -> Self {
        Self::mainnet()
    }
}

/// Supply / monetary parameters. Follows Bitcoin's deflationary template.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonetaryParams {
    /// Total supply in the smallest indivisible unit.
    ///
    /// 21_000_000 × 10^8 = 2_100_000_000_000_000 (fits easily in `u64`).
    pub total_supply: Amount,
    /// Initial block reward (smallest units).
    pub initial_block_reward: Amount,
    /// Interval, in blocks, between halving events.
    pub halving_interval_blocks: u64,
    /// Minimum fee (smallest units per byte of serialized tx).
    pub min_fee_per_byte: u64,
}

impl MonetaryParams {
    /// Default mainnet monetary values.
    #[must_use]
    pub const fn mainnet() -> Self {
        Self {
            // 21 M coins × 10^8 units/coin
            total_supply: Amount::from_smallest_units(21_000_000 * 100_000_000),
            // Matches Bitcoin's 50 BTC initial subsidy
            initial_block_reward: Amount::from_smallest_units(50 * 100_000_000),
            // ~4 years at 2-s slots (but we target far fewer blocks per second,
            // so treat as a starting point — finalised in Aşama 4 tokenomics).
            halving_interval_blocks: 210_000,
            min_fee_per_byte: 1,
        }
    }

    /// Validate internal consistency.
    pub fn validate(&self) -> Result<(), ParamsError> {
        if self.total_supply.as_u64() == 0 {
            return Err(ParamsError::Invalid("total_supply must be > 0"));
        }
        if self.initial_block_reward > self.total_supply {
            return Err(ParamsError::Invalid(
                "initial_block_reward must not exceed total_supply",
            ));
        }
        if self.halving_interval_blocks == 0 {
            return Err(ParamsError::Invalid(
                "halving_interval_blocks must be > 0",
            ));
        }
        Ok(())
    }
}

impl Default for MonetaryParams {
    fn default() -> Self {
        Self::mainnet()
    }
}

// ============================================================================
// Top-level
// ============================================================================

/// The complete protocol-parameter bundle. Consumers should treat
/// [`ProtocolParams`] as opaque — read what you need via the accessors.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolParams {
    /// Which logical network this parameter set describes.
    pub network: NetworkId,
    /// Slot at which the chain started.
    pub genesis_slot: Slot,
    /// Unix seconds at which [`ProtocolParams::genesis_slot`] begins.
    pub genesis_time: u64,
    /// Consensus sub-parameters.
    pub consensus: ConsensusParams,
    /// Ledger sub-parameters.
    pub ledger: LedgerParams,
    /// Monetary sub-parameters.
    pub monetary: MonetaryParams,
}

impl ProtocolParams {
    /// Default mainnet parameters derived from CLAUDE.md.
    #[must_use]
    pub fn mainnet() -> Self {
        Self {
            network: NetworkId::Mainnet,
            genesis_slot: Slot::GENESIS,
            genesis_time: 0,
            consensus: ConsensusParams::mainnet(),
            ledger: LedgerParams::mainnet(),
            monetary: MonetaryParams::mainnet(),
        }
    }

    /// Default testnet parameters — same shape as mainnet with a different
    /// network id and faster epochs (1000 slots).
    #[must_use]
    pub fn testnet() -> Self {
        Self {
            network: NetworkId::Testnet,
            consensus: ConsensusParams {
                epoch_slots: 1_000,
                ..ConsensusParams::mainnet()
            },
            ..Self::mainnet()
        }
    }

    /// Ephemeral in-process parameters used by tests.
    #[must_use]
    pub fn ephemeral() -> Self {
        Self {
            network: NetworkId::Ephemeral,
            consensus: ConsensusParams {
                slot_duration_ms: 100,
                epoch_slots: 50,
                k_finality: 3,
                honest_stake_num: 2,
                honest_stake_den: 3,
            },
            ledger: LedgerParams {
                max_block_bytes: 1 << 20,
                max_tx_bytes: 1 << 16,
                max_tx_inputs: 8,
                max_tx_outputs: 8,
                max_block_txs: 32,
                max_script_bytes: 1 << 12,
                max_datum_bytes: 1 << 10,
            },
            monetary: MonetaryParams {
                total_supply: Amount::from_smallest_units(1_000_000),
                initial_block_reward: Amount::from_smallest_units(100),
                halving_interval_blocks: 10,
                min_fee_per_byte: 0,
            },
            ..Self::mainnet()
        }
    }

    /// Validate every sub-section.
    pub fn validate(&self) -> Result<(), ParamsError> {
        self.consensus.validate()?;
        self.ledger.validate()?;
        self.monetary.validate()?;
        Ok(())
    }

    /// Deserialize from a TOML string (`config/genesis.toml`).
    pub fn from_toml(s: &str) -> Result<Self, ParamsError> {
        let p: Self = toml::from_str(s).map_err(|e| ParamsError::Parse(e.to_string()))?;
        p.validate()?;
        Ok(p)
    }

    /// Serialize to TOML.
    pub fn to_toml(&self) -> Result<String, ParamsError> {
        toml::to_string_pretty(self).map_err(|e| ParamsError::Parse(e.to_string()))
    }

    /// Deserialize from a JSON string.
    pub fn from_json(s: &str) -> Result<Self, ParamsError> {
        let p: Self =
            serde_json::from_str(s).map_err(|e| ParamsError::Parse(e.to_string()))?;
        p.validate()?;
        Ok(p)
    }

    /// Serialize to JSON.
    pub fn to_json(&self) -> Result<String, ParamsError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| ParamsError::Parse(e.to_string()))
    }
}

impl Default for ProtocolParams {
    fn default() -> Self {
        Self::mainnet()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::integer_division
)]
mod tests {
    use super::*;

    #[test]
    fn mainnet_defaults_validate() {
        ProtocolParams::mainnet().validate().unwrap();
    }

    #[test]
    fn testnet_defaults_validate() {
        ProtocolParams::testnet().validate().unwrap();
    }

    #[test]
    fn ephemeral_defaults_validate() {
        ProtocolParams::ephemeral().validate().unwrap();
    }

    #[test]
    fn consensus_epoch_seconds_matches_claude_md() {
        // 21_600 slots × 2 s = 43_200 s = 12 h
        let cp = ConsensusParams::mainnet();
        assert_eq!(cp.epoch_seconds(), 43_200);
    }

    #[test]
    fn consensus_rejects_zero_slot_duration() {
        let bad = ConsensusParams {
            slot_duration_ms: 0,
            ..ConsensusParams::mainnet()
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn consensus_rejects_honest_stake_over_one() {
        let bad = ConsensusParams {
            honest_stake_num: 101,
            honest_stake_den: 100,
            ..ConsensusParams::mainnet()
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn ledger_rejects_tx_bigger_than_block() {
        let bad = LedgerParams {
            max_tx_bytes: 10,
            max_block_bytes: 5,
            ..LedgerParams::mainnet()
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn monetary_total_supply_is_21m_coins() {
        let m = MonetaryParams::mainnet();
        assert_eq!(m.total_supply.as_u64(), 21_000_000 * 100_000_000);
    }

    #[test]
    fn toml_roundtrip_mainnet() {
        let p = ProtocolParams::mainnet();
        let s = p.to_toml().unwrap();
        let back = ProtocolParams::from_toml(&s).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn json_roundtrip_mainnet() {
        let p = ProtocolParams::mainnet();
        let s = p.to_json().unwrap();
        let back = ProtocolParams::from_json(&s).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn from_toml_validates() {
        // Force an invalid k_finality via a hand-written TOML.
        let toml_str = r#"
            network = "mainnet"
            genesis_slot = 0
            genesis_time = 0

            [consensus]
            slot_duration_ms = 2000
            epoch_slots = 21600
            k_finality = 0
            honest_stake_num = 51
            honest_stake_den = 100

            [ledger]
            max_block_bytes = 7500000
            max_tx_bytes = 200000
            max_tx_inputs = 1024
            max_tx_outputs = 1024
            max_block_txs = 10000
            max_script_bytes = 65536
            max_datum_bytes = 16384

            [monetary]
            total_supply = 2100000000000000
            initial_block_reward = 5000000000
            halving_interval_blocks = 210000
            min_fee_per_byte = 1
        "#;
        let err = ProtocolParams::from_toml(toml_str).unwrap_err();
        assert!(matches!(err, ParamsError::Invalid(_)));
    }

    #[test]
    fn network_magic_is_two_bytes() {
        for net in [
            NetworkId::Mainnet,
            NetworkId::Testnet,
            NetworkId::Devnet,
            NetworkId::Ephemeral,
        ] {
            let m = net.magic();
            assert_eq!(m.len(), 2);
        }
    }
}
