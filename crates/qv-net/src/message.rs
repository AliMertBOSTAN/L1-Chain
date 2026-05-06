//! Network message types and serialization.
//!
//! Every message exchanged over the wire is an [`Envelope`] containing a
//! protocol version tag, a [`MessageKind`] discriminant, and the payload
//! encoded as bincode bytes.  Size limits are enforced at the codec boundary.

use qv_core::{Block, BlockHash, BlockHeader, Transaction};
use serde::{Deserialize, Serialize};

use crate::NetError;

/// Current wire protocol version.
pub const PROTOCOL_VERSION: u16 = 1;

/// Maximum serialized message size (4 MiB).
pub const MAX_MESSAGE_SIZE: usize = 4 * 1024 * 1024;

/// Maximum number of headers in a single `GetHeaders` / `Headers` exchange.
pub const MAX_HEADERS_PER_MSG: usize = 2000;

/// Maximum number of block hashes in a `GetBlocks` request.
pub const MAX_BLOCK_LOCATORS: usize = 64;

/// High-level message kind tag for the gossip topic router.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MessageKind {
    /// A full block announcement.
    Block,
    /// A single transaction (clear or encrypted).
    Transaction,
    /// A VRF leadership proof for a slot.
    VrfProof,
    /// A vote / attestation.
    Vote,
    /// Block header request.
    GetHeaders,
    /// Block header response.
    Headers,
    /// Full block request.
    GetBlocks,
    /// Ping / keep-alive.
    Ping,
    /// Pong / keep-alive reply.
    Pong,
}

/// VRF proof announcement broadcast by a slot leader.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VrfProofMsg {
    /// Slot number the proof is for.
    pub slot: u64,
    /// Raw VRF output bytes.
    pub vrf_output: Vec<u8>,
    /// Raw VRF proof bytes.
    pub vrf_proof: Vec<u8>,
    /// SHA3-256 hash of the producer's VRF public key.
    pub producer_key_hash: [u8; 32],
}

/// A vote / attestation (placeholder — concrete fields depend on finality design).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoteMsg {
    /// Slot that is being attested.
    pub slot: u64,
    /// Block hash being voted for.
    pub block_hash: BlockHash,
    /// Voter's key hash.
    pub voter_key_hash: [u8; 32],
    /// Signature bytes.
    pub signature: Vec<u8>,
}

/// Request for block headers starting from a set of locator hashes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetHeadersMsg {
    /// Locator hashes (newest first), up to [`MAX_BLOCK_LOCATORS`].
    pub locator_hashes: Vec<BlockHash>,
    /// Stop hash (zero-hash means "give me as many as you can").
    pub stop_hash: BlockHash,
}

/// Response carrying block headers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadersMsg {
    /// Headers in ascending height order.
    pub headers: Vec<BlockHeader>,
}

/// Request for full blocks by hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetBlocksMsg {
    /// Block hashes requested, up to [`MAX_BLOCK_LOCATORS`].
    pub hashes: Vec<BlockHash>,
}

/// Ping message with a nonce for latency measurement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingMsg {
    /// Opaque nonce; the receiver must echo it in `PongMsg`.
    pub nonce: u64,
}

/// Pong reply echoing the ping nonce.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PongMsg {
    /// The nonce from the corresponding `PingMsg`.
    pub nonce: u64,
}

/// Top-level network message — all payloads are wrapped in this enum.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkMessage {
    /// Full block announcement (typically via gossip).
    Block(Box<Block>),
    /// Transaction announcement.
    Transaction(Box<Transaction>),
    /// VRF leadership proof.
    VrfProof(VrfProofMsg),
    /// Vote / attestation.
    Vote(VoteMsg),
    /// Header request (request-response).
    GetHeaders(GetHeadersMsg),
    /// Header response.
    Headers(HeadersMsg),
    /// Block request.
    GetBlocks(GetBlocksMsg),
    /// Ping.
    Ping(PingMsg),
    /// Pong.
    Pong(PongMsg),
}

impl NetworkMessage {
    /// Determine the [`MessageKind`] tag for this message.
    #[must_use]
    pub fn kind(&self) -> MessageKind {
        match self {
            Self::Block(_) => MessageKind::Block,
            Self::Transaction(_) => MessageKind::Transaction,
            Self::VrfProof(_) => MessageKind::VrfProof,
            Self::Vote(_) => MessageKind::Vote,
            Self::GetHeaders(_) => MessageKind::GetHeaders,
            Self::Headers(_) => MessageKind::Headers,
            Self::GetBlocks(_) => MessageKind::GetBlocks,
            Self::Ping(_) => MessageKind::Ping,
            Self::Pong(_) => MessageKind::Pong,
        }
    }

    /// Whether this message type should be propagated via gossip.
    #[must_use]
    pub fn is_gossip(&self) -> bool {
        matches!(
            self,
            Self::Block(_) | Self::Transaction(_) | Self::VrfProof(_) | Self::Vote(_)
        )
    }

    /// Whether this message type is a request-response pair.
    #[must_use]
    pub fn is_request_response(&self) -> bool {
        matches!(
            self,
            Self::GetHeaders(_) | Self::Headers(_) | Self::GetBlocks(_)
        )
    }
}

/// Wire envelope: version + bincode payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Envelope {
    /// Protocol version tag.
    pub version: u16,
    /// Bincode-encoded [`NetworkMessage`].
    pub payload: Vec<u8>,
}

impl Envelope {
    /// Create an envelope for a `NetworkMessage`.
    pub fn encode(msg: &NetworkMessage) -> Result<Vec<u8>, NetError> {
        let payload =
            bincode::serialize(msg).map_err(|e| NetError::Codec(format!("encode: {e}")))?;

        if payload.len() > MAX_MESSAGE_SIZE {
            return Err(NetError::MessageTooLarge {
                size: payload.len(),
                limit: MAX_MESSAGE_SIZE,
            });
        }

        let envelope = Self {
            version: PROTOCOL_VERSION,
            payload,
        };
        bincode::serialize(&envelope).map_err(|e| NetError::Codec(format!("envelope: {e}")))
    }

    /// Decode an envelope from wire bytes, yielding the inner `NetworkMessage`.
    pub fn decode(bytes: &[u8]) -> Result<NetworkMessage, NetError> {
        if bytes.len() > MAX_MESSAGE_SIZE + 64 {
            return Err(NetError::MessageTooLarge {
                size: bytes.len(),
                limit: MAX_MESSAGE_SIZE,
            });
        }

        let envelope: Self =
            bincode::deserialize(bytes).map_err(|e| NetError::Codec(format!("decode: {e}")))?;

        if envelope.version != PROTOCOL_VERSION {
            return Err(NetError::UnsupportedVersion {
                got: envelope.version,
                expected: PROTOCOL_VERSION,
            });
        }

        bincode::deserialize(&envelope.payload)
            .map_err(|e| NetError::Codec(format!("payload: {e}")))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use qv_core::{BlockHash, BlockHeader, Height};

    use super::*;

    #[test]
    fn ping_pong_roundtrip() {
        let msg = NetworkMessage::Ping(PingMsg { nonce: 42 });
        let wire = Envelope::encode(&msg).unwrap();
        let decoded = Envelope::decode(&wire).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn message_kind_classification() {
        let ping = NetworkMessage::Ping(PingMsg { nonce: 0 });
        assert_eq!(ping.kind(), MessageKind::Ping);
        assert!(!ping.is_gossip());
        assert!(!ping.is_request_response());

        let get_hdrs = NetworkMessage::GetHeaders(GetHeadersMsg {
            locator_hashes: vec![],
            stop_hash: BlockHash::ZERO,
        });
        assert!(get_hdrs.is_request_response());
        assert!(!get_hdrs.is_gossip());
    }

    #[test]
    fn version_mismatch_rejected() {
        let msg = NetworkMessage::Pong(PongMsg { nonce: 99 });
        let payload =
            bincode::serialize(&msg).unwrap();

        let bad_envelope = Envelope {
            version: 999,
            payload,
        };
        let wire = bincode::serialize(&bad_envelope).unwrap();

        let err = Envelope::decode(&wire).unwrap_err();
        assert!(matches!(err, NetError::UnsupportedVersion { .. }));
    }

    #[test]
    fn get_headers_roundtrip() {
        let msg = NetworkMessage::GetHeaders(GetHeadersMsg {
            locator_hashes: vec![
                BlockHash::from_bytes([1; 32]),
                BlockHash::from_bytes([2; 32]),
            ],
            stop_hash: BlockHash::ZERO,
        });
        let wire = Envelope::encode(&msg).unwrap();
        let decoded = Envelope::decode(&wire).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn headers_roundtrip() {
        let mut h = BlockHeader::genesis_template();
        h.height = Height::from(5);
        let msg = NetworkMessage::Headers(HeadersMsg {
            headers: vec![h.clone()],
        });
        let wire = Envelope::encode(&msg).unwrap();
        let decoded = Envelope::decode(&wire).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn vrf_proof_roundtrip() {
        let msg = NetworkMessage::VrfProof(VrfProofMsg {
            slot: 42,
            vrf_output: vec![1, 2, 3],
            vrf_proof: vec![4, 5, 6],
            producer_key_hash: [0xAA; 32],
        });
        let wire = Envelope::encode(&msg).unwrap();
        let decoded = Envelope::decode(&wire).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn vote_roundtrip() {
        let msg = NetworkMessage::Vote(VoteMsg {
            slot: 10,
            block_hash: BlockHash::from_bytes([0xBB; 32]),
            voter_key_hash: [0xCC; 32],
            signature: vec![7, 8, 9],
        });
        let wire = Envelope::encode(&msg).unwrap();
        let decoded = Envelope::decode(&wire).unwrap();
        assert_eq!(decoded, msg);
    }
}
