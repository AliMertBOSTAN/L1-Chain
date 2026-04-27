#include "qv/net/peer.hpp"

#include <algorithm>
#include <iostream>
#include <stdexcept>

namespace qv::net {

bool PeerStore::add_peer(const PeerInfo& peer_info) {
    // TODO: Implement thread-safe peer addition
    // 1. Acquire write lock on peers_lock_
    // 2. Check if peer_info.id is already in peers map
    // 3. If new or updated, add/update entry and return true
    // 4. If identical, return false
    // 5. Release lock

    if (peer_info.id.empty() || peer_info.address.empty() || peer_info.port == 0) {
        throw std::invalid_argument("Invalid peer info: empty id, address, or port");
    }

    std::cout << "[PeerStore] Adding peer: " << peer_info.id << " at "
              << peer_info.address << ":" << peer_info.port << std::endl;

    return true;
}

const PeerInfo* PeerStore::get_peer(const PeerId& peer_id) const {
    // TODO: Implement thread-safe peer lookup
    // 1. Acquire read lock on peers_lock_
    // 2. Find peer in peers map
    // 3. Return pointer to entry if found, nullptr otherwise
    // 4. Release lock

    std::cout << "[PeerStore] Looking up peer: " << peer_id << std::endl;
    return nullptr;
}

bool PeerStore::update_last_seen(const PeerId& peer_id) {
    // TODO: Implement thread-safe timestamp update
    // 1. Acquire write lock on peers_lock_
    // 2. Find peer in map
    // 3. If found, update peer_info.last_seen to current time
    // 4. Release lock
    // 5. Return success/failure

    std::cout << "[PeerStore] Updating last_seen for peer: " << peer_id << std::endl;
    return true;
}

std::vector<PeerInfo> PeerStore::get_all_peers() const {
    // TODO: Implement thread-safe peer list retrieval
    // 1. Acquire read lock
    // 2. Copy all PeerInfo from peers map to vector
    // 3. Release lock
    // 4. Return vector

    std::cout << "[PeerStore] Retrieving all peers" << std::endl;
    return std::vector<PeerInfo>();
}

std::vector<PeerInfo> PeerStore::get_active_peers(size_t limit) const {
    // TODO: Implement thread-safe active peer retrieval
    // 1. Acquire read lock
    // 2. Copy all PeerInfo from map to vector
    // 3. Sort by last_seen (descending)
    // 4. If limit > 0, truncate to first limit entries
    // 5. Release lock
    // 6. Return vector

    std::cout << "[PeerStore] Retrieving active peers (limit=" << limit << ")" << std::endl;
    return std::vector<PeerInfo>();
}

bool PeerStore::remove_peer(const PeerId& peer_id) {
    // TODO: Implement thread-safe peer removal
    // 1. Acquire write lock
    // 2. Find peer in map
    // 3. If found, erase from map
    // 4. Release lock
    // 5. Return success/failure

    std::cout << "[PeerStore] Removing peer: " << peer_id << std::endl;
    return true;
}

size_t PeerStore::peer_count() const {
    // TODO: Implement thread-safe count retrieval
    // 1. Acquire read lock
    // 2. Return peers.size()
    // 3. Release lock

    return 0;
}

void PeerStore::clear() {
    // TODO: Implement thread-safe clear
    // 1. Acquire write lock
    // 2. Clear peers map
    // 3. Release lock

    std::cout << "[PeerStore] Clearing all peers" << std::endl;
}

} // namespace qv::net
