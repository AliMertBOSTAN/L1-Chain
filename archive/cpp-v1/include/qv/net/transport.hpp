#pragma once

#include "qv/net/message.hpp"
#include "qv/net/peer.hpp"

#include <cstdint>
#include <string>
#include <memory>
#include <functional>
#include <optional>

namespace qv::net {

/// Result type for network operations
template<typename T>
struct Result {
    bool success;
    T value;
    std::string error_message;

    /// Create a successful result
    static Result<T> ok(const T& val) {
        return Result<T>{true, val, ""};
    }

    /// Create a failed result
    static Result<T> err(const std::string& msg) {
        return Result<T>{false, T{}, msg};
    }
};

/// Specialization for void results
template<>
struct Result<void> {
    bool success;
    std::string error_message;

    static Result<void> ok() {
        return Result<void>{true, ""};
    }

    static Result<void> err(const std::string& msg) {
        return Result<void>{false, msg};
    }
};

/// Represents an active connection to a remote peer
struct Connection {
    PeerId peer_id;
    std::string remote_address;
    uint16_t remote_port;
    uint64_t connection_time;  ///< Unix timestamp when connection established
    bool is_outbound;          ///< true if we initiated, false if they did

    /// Send a message over this connection
    ///
    /// @param message The NetworkMessage to send
    /// @return Result indicating success or failure
    virtual Result<void> send(const NetworkMessage& message) = 0;

    /// Receive a message over this connection (blocking)
    ///
    /// @param timeout_ms Maximum time to wait in milliseconds (0 = infinite)
    /// @return Received NetworkMessage, or nullopt on timeout/error
    virtual std::optional<NetworkMessage> receive(uint32_t timeout_ms = 0) = 0;

    /// Close this connection
    virtual void close() = 0;

    virtual ~Connection() = default;
};

/// Abstract transport layer interface
///
/// Handles:
/// - Outbound connections to remote peers
/// - Inbound connection acceptance
/// - Message serialization and sending
/// - Message reception and deserialization
/// - Hybrid KEM handshake for post-quantum key agreement
///
/// Implementations may use TCP, QUIC, or other transport protocols.
/// All connections must complete HybridKEM handshake before use.
class Transport {
public:
    /// Callback type for new inbound connections
    using ConnectionCallback = std::function<void(std::shared_ptr<Connection>)>;

    Transport() = default;
    virtual ~Transport() = default;

    /// Establish outbound connection to a peer
    ///
    /// Performs:
    /// 1. TCP/QUIC connection to peer address
    /// 2. HybridKEM handshake (X25519 + Kyber):
    ///    - Exchange ephemeral X25519 keys
    ///    - Exchange and encapsulate Kyber ciphertexts
    ///    - Derive shared secret from both KEM outputs
    /// 3. Establish authenticated channel
    ///
    /// @param peer_info Information about peer to connect to
    /// @return Result containing Connection if successful
    virtual Result<std::shared_ptr<Connection>> connect(
        const PeerInfo& peer_info
    ) = 0;

    /// Listen for inbound connections on specified address/port
    ///
    /// Starts accepting inbound connections. For each new connection:
    /// 1. Accept TCP/QUIC connection
    /// 2. Perform HybridKEM handshake
    /// 3. Call connection_callback with established Connection
    ///
    /// @param address Address to bind to (e.g., "0.0.0.0", "127.0.0.1")
    /// @param port Port to listen on
    /// @param connection_callback Called for each new inbound connection
    /// @return Result indicating success or failure
    virtual Result<void> listen(
        const std::string& address,
        uint16_t port,
        ConnectionCallback connection_callback
    ) = 0;

    /// Stop listening for inbound connections
    virtual Result<void> stop_listening() = 0;

    /// Get list of active connections
    ///
    /// @return Vector of connected peers
    virtual std::vector<std::shared_ptr<Connection>> get_active_connections() const = 0;

    /// Get connection to specific peer
    ///
    /// @param peer_id The peer identifier
    /// @return Connection if peer is connected, nullptr otherwise
    virtual std::shared_ptr<Connection> get_connection(const PeerId& peer_id) const = 0;

    /// Close all active connections and shutdown transport
    virtual void shutdown() = 0;
};

/// Hybrid KEM utilities for post-quantum key agreement
///
/// Combines X25519 (classical ECDH) with Kyber (post-quantum KEM)
/// to protect against both classical and quantum adversaries.
namespace kem {

    /// Result of HybridKEM key agreement
    struct SharedSecret {
        bytes secret;           ///< Derived shared secret (32 bytes typically)
        bytes ephemeral_pk;     ///< Ephemeral public key for peer
        bytes kem_ciphertext;   ///< Kyber KEM ciphertext
    };

    /// Perform HybridKEM key agreement initiator side
    ///
    /// Generates ephemeral X25519 and Kyber keys, encapsulates Kyber ciphertext.
    ///
    /// @param peer_public_key The peer's KEM public key
    /// @return SharedSecret with ephemeral key, ciphertext, and derived secret
    /// @throws std::runtime_error if key generation fails
    SharedSecret initiate_handshake(const PublicKey& peer_public_key);

    /// Perform HybridKEM key agreement responder side
    ///
    /// Decapsulates ephemeral keys and ciphertexts to derive shared secret.
    ///
    /// @param our_secret_key Our KEM secret key
    /// @param peer_ephemeral_pk Peer's ephemeral X25519 public key
    /// @param peer_kem_ciphertext Peer's Kyber KEM ciphertext
    /// @return SharedSecret with derived secret
    /// @throws std::runtime_error if decapsulation fails
    SharedSecret respond_handshake(
        const bytes& our_secret_key,
        const bytes& peer_ephemeral_pk,
        const bytes& peer_kem_ciphertext
    );

} // namespace kem

} // namespace qv::net
