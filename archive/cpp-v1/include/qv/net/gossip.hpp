#pragma once

#include "qv/net/message.hpp"
#include "qv/net/transport.hpp"

#include <cstdint>
#include <vector>
#include <memory>
#include <functional>
#include <unordered_set>

namespace qv::net {

/// Type aliases for blockchain data
using BlockData = std::vector<uint8_t>;
using TransactionData = std::vector<uint8_t>;
using VoteData = std::vector<uint8_t>;

/// Gossip protocol statistics
struct GossipStats {
    uint64_t blocks_relayed;        ///< Total blocks relayed
    uint64_t transactions_relayed;  ///< Total transactions relayed
    uint64_t votes_relayed;         ///< Total consensus votes relayed
    uint64_t duplicates_filtered;   ///< Messages dropped as duplicates
    uint64_t peers_connected;       ///< Current number of connected peers
};

/// Gossip-based message dissemination protocol
///
/// Implements rumor-mongering style propagation for blocks, transactions, and votes:
/// - Sends each message to a configurable number of peers (fanout)
/// - Tracks message hashes to avoid duplicate rebroadcasting
/// - Maintains in-memory cache of recently seen messages
/// - Provides callbacks for application-level message handling
/// - Metrics tracking for protocol efficiency
///
/// Message flow:
/// 1. Application calls broadcast_* with data
/// 2. Protocol wraps in NetworkMessage with type and nonce
/// 3. Message sent to fanout random peers
/// 4. Receiving peers check dedup cache
/// 5. If new, peer relays to its fanout peers
/// 6. Application receives via on_message callback
class GossipProtocol {
public:
    /// Callback for received messages
    using MessageHandler = std::function<void(const NetworkMessage&)>;

    GossipProtocol(
        std::shared_ptr<Transport> transport,
        size_t fanout = 3,
        size_t max_dedup_cache = 10000
    );

    ~GossipProtocol() = default;

    /// Broadcast a new block to the network
    ///
    /// Disseminates block to fanout random peers. Each peer will
    /// rebroadcast if not seen before (based on message hash).
    ///
    /// @param block Serialized block data (typically protobuf)
    /// @return Result indicating if broadcast was initiated
    Result<void> broadcast_block(const BlockData& block);

    /// Broadcast a new transaction to the network
    ///
    /// Disseminates transaction to fanout random peers.
    ///
    /// @param transaction Serialized transaction data
    /// @return Result indicating if broadcast was initiated
    Result<void> broadcast_transaction(const TransactionData& transaction);

    /// Broadcast a consensus vote to the network
    ///
    /// Disseminates vote to fanout random peers.
    ///
    /// @param vote Serialized vote data (vote type, block hash, validator sig, etc.)
    /// @return Result indicating if broadcast was initiated
    Result<void> broadcast_vote(const VoteData& vote);

    /// Register a handler for a specific message type
    ///
    /// Called when new message of this type is received and
    /// not in deduplication cache.
    ///
    /// @param type Message type to handle
    /// @param handler Callback function
    void on_message(MessageType type, MessageHandler handler);

    /// Start the gossip protocol
    ///
    /// Begins listening for and processing incoming messages.
    ///
    /// @return Result indicating if protocol started successfully
    Result<void> start();

    /// Stop the gossip protocol
    ///
    /// Stops message processing and listening.
    ///
    /// @return Result indicating if protocol stopped successfully
    Result<void> stop();

    /// Get current protocol statistics
    ///
    /// @return GossipStats with current metrics
    GossipStats get_stats() const;

    /// Configure fanout parameter
    ///
    /// Controls how many peers to send each message to.
    /// Larger fanout = faster propagation but more bandwidth.
    /// Smaller fanout = less bandwidth but slower propagation.
    ///
    /// @param new_fanout Number of peers to send to
    void set_fanout(size_t new_fanout);

    /// Get current fanout setting
    ///
    /// @return Current fanout value
    size_t get_fanout() const;

    /// Clear deduplication cache
    ///
    /// Useful for testing or extreme memory pressure.
    /// Warning: May cause message re-reception.
    void clear_dedup_cache();

    /// Manual deduplication check
    ///
    /// @param message_hash Hash of message to check
    /// @return true if message is in dedup cache, false otherwise
    bool is_deduplicated(const bytes& message_hash) const;

private:
    std::shared_ptr<Transport> transport_;
    size_t fanout_;
    size_t max_dedup_cache_;

    // TODO: Implement message deduplication with thread-safe storage
    // std::unordered_set<std::string> dedup_cache_;  // message hash -> true
    // mutable std::shared_mutex dedup_lock_;

    // TODO: Implement message handlers for each type
    // std::unordered_map<uint8_t, MessageHandler> handlers_;

    // TODO: Implement statistics tracking
    // GossipStats stats_;

    // TODO: Implement background message processing thread
    // std::thread message_processor_;
    // std::atomic<bool> running_;

    /// Process a received message
    /// Internal: Called when new message arrives
    void process_message(const NetworkMessage& message);

    /// Relay message to peers
    /// Internal: Called to propagate message to fanout peers
    void relay_message(const NetworkMessage& message);

    /// Select fanout random connected peers
    /// @return Vector of PeerIds to send to
    std::vector<PeerId> select_fanout_peers();
};

} // namespace qv::net
