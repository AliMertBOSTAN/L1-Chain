#pragma once

#include <cstdint>
#include <string>
#include <vector>
#include <memory>
#include <unordered_map>

namespace qv::net {

/// Type aliases for network operations
using bytes = std::vector<uint8_t>;
using PeerId = std::string;  ///< Derived from SHA3-256(PQC public key)

/// Post-quantum cryptographic public key for peer identification and handshake
struct PublicKey {
    bytes data;
};

/// Complete information about a peer in the network
struct PeerInfo {
    PeerId id;              ///< Unique peer identifier (hash of public key)
    std::string address;    ///< IP address or hostname
    uint16_t port;          ///< Network port for connections
    PublicKey key;          ///< PQC public key for handshake and verification
    uint64_t last_seen;     ///< Unix timestamp of last communication
};

/// Manages peer discovery and connectivity information
///
/// The PeerStore maintains:
/// - Known peers and their addresses
/// - Peer reputation/health metrics
/// - Connection state tracking
/// - Peer bootstrap information for network join
///
/// Thread-safe operations for concurrent peer discovery and management.
class PeerStore {
public:
    PeerStore() = default;
    ~PeerStore() = default;

    /// Register a new peer in the store
    ///
    /// @param peer_info Information about the peer to register
    /// @return true if peer was added or updated, false if already present and identical
    /// @throws std::invalid_argument if peer_info is malformed
    bool add_peer(const PeerInfo& peer_info);

    /// Retrieve a peer by its ID
    ///
    /// @param peer_id The peer's identifier
    /// @return PeerInfo if found, nullptr otherwise
    const PeerInfo* get_peer(const PeerId& peer_id) const;

    /// Update last seen timestamp for a peer
    ///
    /// Called when peer communication succeeds to track peer liveness.
    ///
    /// @param peer_id The peer's identifier
    /// @return true if peer exists and was updated, false otherwise
    bool update_last_seen(const PeerId& peer_id);

    /// Get a list of all known peers
    ///
    /// @return Vector of all PeerInfo in the store
    std::vector<PeerInfo> get_all_peers() const;

    /// Get a list of peers sorted by last seen (most recent first)
    ///
    /// Useful for selecting peers to connect to, prioritizing active peers.
    ///
    /// @param limit Maximum number of peers to return (0 = all)
    /// @return Vector of sorted PeerInfo
    std::vector<PeerInfo> get_active_peers(size_t limit = 0) const;

    /// Remove a peer from the store
    ///
    /// @param peer_id The peer's identifier
    /// @return true if peer was removed, false if not found
    bool remove_peer(const PeerId& peer_id);

    /// Get total peer count
    ///
    /// @return Number of peers currently stored
    size_t peer_count() const;

    /// Clear all peers (use with caution)
    void clear();

private:
    // TODO: Use thread-safe storage (e.g., RWLock + std::unordered_map or a concurrent hashmap)
    // std::unordered_map<PeerId, PeerInfo> peers;
    // mutable std::shared_mutex peers_lock;

    // TODO: Implement peer eviction strategy
    // - Remove peers that haven't been seen for N seconds
    // - Keep connection limits (max_peer_connections)
    // - Track peer reputation scores
};

} // namespace qv::net
