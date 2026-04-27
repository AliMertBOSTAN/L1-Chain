#pragma once

#include "qv/net/peer.hpp"

#include <cstdint>
#include <string>
#include <vector>
#include <memory>

namespace qv::net {

/// Message types for network protocol
enum class MessageType : uint8_t {
    BLOCK = 0,              ///< New block announcement
    TRANSACTION = 1,        ///< New transaction for mempool
    PEER_DISCOVERY = 2,     ///< Peer discovery / bootstrap
    PING = 3,              ///< Ping for liveness check
    PONG = 4,              ///< Response to ping
    CONSENSUS_VOTE = 5,    ///< Consensus vote message (for BFT or PoS)
    // TODO: Add more message types as protocol develops
};

/// Core network message format
///
/// All messages in the QuantumVault network follow this structure:
/// - Type indicates message purpose and payload format
/// - Payload contains serialized block, transaction, vote, etc.
/// - Sender identifies origin peer (for duplicate detection)
/// - Nonce provides uniqueness for deduplication
/// - Signature (optional) provides sender authentication
///
/// Payloads are typically protobuf-encoded for compatibility.
struct NetworkMessage {
    MessageType type;           ///< Message type indicator
    bytes payload;              ///< Serialized message content
    PeerId sender;              ///< ID of sending peer
    uint64_t nonce;             ///< Unique nonce for deduplication
    uint64_t timestamp;         ///< Unix timestamp of creation
    bytes signature;            ///< Optional PQC signature (empty if not signed)

    /// Calculate the message hash for deduplication
    ///
    /// Uses SHA3-256 of (type || payload || sender || nonce)
    ///
    /// @return 32-byte message hash
    bytes hash() const;

    /// Serialize message to bytes (typically protobuf format)
    ///
    /// TODO: Define protobuf message schema for NetworkMessage
    ///
    /// @return Serialized message bytes
    bytes serialize() const;

    /// Deserialize message from bytes
    ///
    /// @param data Serialized message bytes
    /// @return Deserialized NetworkMessage
    /// @throws std::invalid_argument if deserialization fails
    static NetworkMessage deserialize(const bytes& data);

    /// Verify message signature using sender's public key
    ///
    /// @param sender_pubkey The sender's PQC public key
    /// @return true if signature is valid, false otherwise
    bool verify_signature(const PublicKey& sender_pubkey) const;
};

/// Factory for creating network messages
class MessageBuilder {
public:
    MessageBuilder() = default;
    ~MessageBuilder() = default;

    /// Create a block announcement message
    ///
    /// @param block_data Serialized block (typically protobuf)
    /// @param sender_id The sender's peer ID
    /// @param nonce Unique nonce for this message
    /// @return NetworkMessage with type = BLOCK
    static NetworkMessage create_block_message(
        const bytes& block_data,
        const PeerId& sender_id,
        uint64_t nonce
    );

    /// Create a transaction message
    ///
    /// @param tx_data Serialized transaction (typically protobuf)
    /// @param sender_id The sender's peer ID
    /// @param nonce Unique nonce for this message
    /// @return NetworkMessage with type = TRANSACTION
    static NetworkMessage create_transaction_message(
        const bytes& tx_data,
        const PeerId& sender_id,
        uint64_t nonce
    );

    /// Create a peer discovery message
    ///
    /// @param peers List of known peers to share
    /// @param sender_id The sender's peer ID
    /// @param nonce Unique nonce for this message
    /// @return NetworkMessage with type = PEER_DISCOVERY
    static NetworkMessage create_peer_discovery_message(
        const std::vector<PeerInfo>& peers,
        const PeerId& sender_id,
        uint64_t nonce
    );

    /// Create a ping message
    ///
    /// @param sender_id The sender's peer ID
    /// @param nonce Unique nonce for this message
    /// @return NetworkMessage with type = PING
    static NetworkMessage create_ping_message(
        const PeerId& sender_id,
        uint64_t nonce
    );

    /// Create a pong message
    ///
    /// @param sender_id The sender's peer ID
    /// @param ping_nonce The nonce from the ping request
    /// @return NetworkMessage with type = PONG
    static NetworkMessage create_pong_message(
        const PeerId& sender_id,
        uint64_t ping_nonce
    );

    /// Create a consensus vote message
    ///
    /// @param vote_data Serialized vote (typically protobuf)
    /// @param sender_id The sender's peer ID
    /// @param nonce Unique nonce for this message
    /// @return NetworkMessage with type = CONSENSUS_VOTE
    static NetworkMessage create_consensus_vote_message(
        const bytes& vote_data,
        const PeerId& sender_id,
        uint64_t nonce
    );
};

} // namespace qv::net
