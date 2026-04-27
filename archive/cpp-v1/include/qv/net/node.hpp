#pragma once

#include "qv/net/transport.hpp"
#include "qv/net/gossip.hpp"
#include "qv/net/peer.hpp"

#include <cstdint>
#include <string>
#include <memory>
#include <vector>

namespace qv::net {

/// Node configuration
struct NodeConfig {
    std::string listen_address;     ///< Address to bind to (e.g., "0.0.0.0")
    uint16_t listen_port;           ///< Port to listen on
    size_t max_peer_connections;    ///< Maximum number of peers to connect to
    size_t gossip_fanout;           ///< Messages to send to fanout peers
    size_t max_dedup_cache;         ///< Deduplication cache size
    std::vector<PeerInfo> bootstrap_peers;  ///< Initial peer list for bootstrap
    uint32_t peer_timeout_secs;     ///< Seconds before removing inactive peers
};

/// Top-level node orchestration
///
/// The Node class brings together all components of the QuantumVault protocol:
/// - Transport layer (network I/O with HybridKEM handshake)
/// - GossipProtocol (message dissemination)
/// - BlockValidator (validates blocks before accepting)
/// - ChainState (tracks canonical chain)
/// - UTXOSet (tracks unspent outputs)
/// - Mempool (holds unconfirmed transactions)
///
/// Responsibilities:
/// - Start/stop node operation
/// - Accept blocks and validate against chain rules
/// - Accept transactions and validate against UTXO rules
/// - Maintain peer connections
/// - Orchestrate message broadcasting
/// - Handle consensus protocol integration
class Node {
public:
    Node(const NodeConfig& config);
    ~Node();

    /// Start the node
    ///
    /// Performs:
    /// 1. Initialize transport layer
    /// 2. Start listening for inbound connections
    /// 3. Connect to bootstrap peers
    /// 4. Start gossip protocol
    /// 5. Start background threads for validation and processing
    ///
    /// @return Result indicating if node started successfully
    Result<void> start();

    /// Stop the node
    ///
    /// Performs:
    /// 1. Stop accepting connections
    /// 2. Gracefully disconnect peers
    /// 3. Stop gossip protocol
    /// 4. Shutdown background threads
    /// 5. Flush pending state to disk
    ///
    /// @return Result indicating if node stopped successfully
    Result<void> stop();

    /// Check if node is running
    ///
    /// @return true if node is started and operational
    bool is_running() const;

    /// Add a bootstrap peer
    ///
    /// Useful for discovering the network during node startup.
    ///
    /// @param peer_info Information about bootstrap peer
    /// @return Result indicating if peer was added
    Result<void> add_bootstrap_peer(const PeerInfo& peer_info);

    /// Get node statistics
    ///
    /// Aggregates statistics from all subsystems.
    ///
    /// @return Gossip protocol statistics
    GossipStats get_stats() const;

    /// Get current peer count
    ///
    /// @return Number of connected peers
    size_t get_peer_count() const;

    /// Get list of connected peers
    ///
    /// @return Vector of connected PeerInfo
    std::vector<PeerInfo> get_connected_peers() const;

    /// Manually add a peer connection
    ///
    /// Attempts to connect to the specified peer.
    ///
    /// @param peer_info Information about peer to connect to
    /// @return Result indicating if connection was established
    Result<void> connect_peer(const PeerInfo& peer_info);

    /// Disconnect from a peer
    ///
    /// @param peer_id The peer identifier
    /// @return Result indicating if disconnection succeeded
    Result<void> disconnect_peer(const PeerId& peer_id);

private:
    NodeConfig config_;
    bool is_running_;

    // TODO: Initialize these component pointers in constructor
    // std::shared_ptr<Transport> transport_;
    // std::shared_ptr<GossipProtocol> gossip_;
    // std::shared_ptr<PeerStore> peer_store_;

    // TODO: Add blockchain components when available
    // std::shared_ptr<BlockValidator> block_validator_;
    // std::shared_ptr<ChainState> chain_state_;
    // std::shared_ptr<UTXOSet> utxo_set_;
    // std::shared_ptr<Mempool> mempool_;

    // TODO: Add background thread management
    // std::thread peer_discovery_thread_;
    // std::thread block_validation_thread_;
    // std::thread transaction_processing_thread_;
    // std::atomic<bool> running_;

    /// Handle new block received via gossip
    /// Internal: Called by gossip protocol when BLOCK message arrives
    void on_block_received(const NetworkMessage& message);

    /// Handle new transaction received via gossip
    /// Internal: Called by gossip protocol when TRANSACTION message arrives
    void on_transaction_received(const NetworkMessage& message);

    /// Handle consensus vote received via gossip
    /// Internal: Called by gossip protocol when CONSENSUS_VOTE message arrives
    void on_vote_received(const NetworkMessage& message);

    /// Background thread: peer discovery and maintenance
    /// Periodically connects to new peers and removes inactive ones
    void peer_discovery_loop();

    /// Background thread: block validation and chain updates
    /// Validates blocks and updates chain state
    void block_validation_loop();

    /// Background thread: transaction validation and mempool management
    /// Validates transactions and maintains mempool
    void transaction_processing_loop();
};

} // namespace qv::net
