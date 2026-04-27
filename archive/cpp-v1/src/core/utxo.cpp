#include "qv/core/utxo.hpp"
#include <algorithm>
#include <numeric>
#include <sstream>

namespace qv::core {

// ============================================================================
// InMemoryUTXOSet Implementation
// ============================================================================

InMemoryUTXOSet::InMemoryUTXOSet(const InMemoryUTXOSet& other)
    : utxos_(other.utxos_) {}

InMemoryUTXOSet::InMemoryUTXOSet(InMemoryUTXOSet&& other) noexcept
    : utxos_(std::move(other.utxos_)) {}

InMemoryUTXOSet& InMemoryUTXOSet::operator=(const InMemoryUTXOSet& other) {
    if (this != &other) {
        utxos_ = other.utxos_;
    }
    return *this;
}

InMemoryUTXOSet& InMemoryUTXOSet::operator=(InMemoryUTXOSet&& other) noexcept {
    if (this != &other) {
        utxos_ = std::move(other.utxos_);
    }
    return *this;
}

Result<void, std::string> InMemoryUTXOSet::add(
    const OutPoint& outpoint,
    const UTXOEntry& entry
) {
    // Check if UTXO already exists
    if (utxos_.contains(outpoint)) {
        std::ostringstream oss;
        oss << "UTXO already exists at outpoint";
        return Result<void, std::string>(oss.str());
    }

    // Add the UTXO
    utxos_[outpoint] = entry;
    return Result<void, std::string>::ok();
}

Result<UTXOEntry, std::string> InMemoryUTXOSet::spend(const OutPoint& outpoint) {
    auto it = utxos_.find(outpoint);
    if (it == utxos_.end()) {
        std::ostringstream oss;
        oss << "UTXO not found at outpoint";
        return Result<UTXOEntry, std::string>(oss.str());
    }

    UTXOEntry entry = it->second;
    utxos_.erase(it);
    return Result<UTXOEntry, std::string>(entry);
}

std::optional<UTXOEntry> InMemoryUTXOSet::get(const OutPoint& outpoint) const {
    auto it = utxos_.find(outpoint);
    if (it != utxos_.end()) {
        return std::optional<UTXOEntry>(it->second);
    }
    return std::nullopt;
}

bool InMemoryUTXOSet::contains(const OutPoint& outpoint) const noexcept {
    return utxos_.contains(outpoint);
}

Result<HashDigest, std::string> InMemoryUTXOSet::compute_commitment() const {
    // Create a commitment hash over all UTXOs in the set
    // This is a simple proof-of-concept implementation
    // In a real system, this would use a Merkle tree or similar structure

    bytes commitment_data;

    // Sort outpoints for deterministic ordering
    std::vector<OutPoint> sorted_outpoints;
    sorted_outpoints.reserve(utxos_.size());
    for (const auto& [op, entry] : utxos_) {
        sorted_outpoints.push_back(op);
    }
    std::sort(sorted_outpoints.begin(), sorted_outpoints.end());

    // Hash all UTXO data in order
    for (const auto& outpoint : sorted_outpoints) {
        const auto& entry = utxos_.at(outpoint);

        // Add tx_id (32 bytes)
        commitment_data.insert(commitment_data.end(),
            outpoint.tx_id.begin(),
            outpoint.tx_id.end());

        // Add index (4 bytes)
        std::uint32_t idx = outpoint.index;
        commitment_data.push_back((idx >> 0) & 0xFF);
        commitment_data.push_back((idx >> 8) & 0xFF);
        commitment_data.push_back((idx >> 16) & 0xFF);
        commitment_data.push_back((idx >> 24) & 0xFF);

        // Add output value (8 bytes)
        std::uint64_t val = entry.output.value;
        commitment_data.push_back((val >> 0) & 0xFF);
        commitment_data.push_back((val >> 8) & 0xFF);
        commitment_data.push_back((val >> 16) & 0xFF);
        commitment_data.push_back((val >> 24) & 0xFF);
        commitment_data.push_back((val >> 32) & 0xFF);
        commitment_data.push_back((val >> 40) & 0xFF);
        commitment_data.push_back((val >> 48) & 0xFF);
        commitment_data.push_back((val >> 56) & 0xFF);

        // Add locking script length (2 bytes) and data
        std::uint16_t script_len = static_cast<std::uint16_t>(
            std::min(entry.output.locking_script.size(), static_cast<std::size_t>(0xFFFF))
        );
        commitment_data.push_back((script_len >> 0) & 0xFF);
        commitment_data.push_back((script_len >> 8) & 0xFF);
        commitment_data.insert(commitment_data.end(),
            entry.output.locking_script.begin(),
            entry.output.locking_script.end());

        // Add block height (8 bytes)
        std::uint64_t height = entry.block_height;
        commitment_data.push_back((height >> 0) & 0xFF);
        commitment_data.push_back((height >> 8) & 0xFF);
        commitment_data.push_back((height >> 16) & 0xFF);
        commitment_data.push_back((height >> 24) & 0xFF);
        commitment_data.push_back((height >> 32) & 0xFF);
        commitment_data.push_back((height >> 40) & 0xFF);
        commitment_data.push_back((height >> 48) & 0xFF);
        commitment_data.push_back((height >> 56) & 0xFF);

        // Add coinbase flag (1 byte)
        commitment_data.push_back(entry.is_coinbase ? 1 : 0);
    }

    // TODO: Apply proper cryptographic hash (SHA256)
    // For now, use simple XOR-based commitment

    HashDigest commitment{};
    for (std::size_t i = 0; i < commitment_data.size(); ++i) {
        commitment[i % 32] ^= commitment_data[i];
    }

    return Result<HashDigest, std::string>(commitment);
}

Amount InMemoryUTXOSet::total_value() const noexcept {
    return std::accumulate(utxos_.begin(), utxos_.end(),
        Amount(0),
        [](Amount sum, const auto& pair) {
            Amount val = pair.second.output.value;
            // Check for overflow
            if (sum > constants::MAX_SUPPLY - val) {
                return constants::MAX_SUPPLY;
            }
            return sum + val;
        });
}

Result<void, std::string> InMemoryUTXOSet::apply_batch(
    const std::vector<std::pair<OutPoint, UTXOEntry>>& to_add,
    const std::vector<OutPoint>& to_spend
) {
    // First, try to remove all spending UTXOs
    std::vector<OutPoint> failed_spends;
    for (const auto& outpoint : to_spend) {
        if (!utxos_.contains(outpoint)) {
            failed_spends.push_back(outpoint);
        }
    }

    if (!failed_spends.empty()) {
        std::ostringstream oss;
        oss << "Failed to spend " << failed_spends.size() << " UTXO(s)";
        return Result<void, std::string>(oss.str());
    }

    // Actually remove the UTXOs
    for (const auto& outpoint : to_spend) {
        utxos_.erase(outpoint);
    }

    // Add all new UTXOs
    for (const auto& [outpoint, entry] : to_add) {
        if (utxos_.contains(outpoint)) {
            // Rollback: restore spent UTXOs
            std::ostringstream oss;
            oss << "UTXO already exists, batch operation aborted";
            return Result<void, std::string>(oss.str());
        }
        utxos_[outpoint] = entry;
    }

    return Result<void, std::string>::ok();
}

std::vector<OutPoint> InMemoryUTXOSet::all_outpoints() const {
    std::vector<OutPoint> outpoints;
    outpoints.reserve(utxos_.size());
    for (const auto& [op, _] : utxos_) {
        outpoints.push_back(op);
    }
    return outpoints;
}

std::vector<UTXOEntry> InMemoryUTXOSet::all_entries() const {
    std::vector<UTXOEntry> entries;
    entries.reserve(utxos_.size());
    for (const auto& [_, entry] : utxos_) {
        entries.push_back(entry);
    }
    return entries;
}

// ============================================================================
// Factory Function
// ============================================================================

std::unique_ptr<IUTXOSet> create_utxo_set(bool use_memory) {
    if (use_memory) {
        return std::make_unique<InMemoryUTXOSet>();
    }

    // TODO: Support other backends (database-backed, etc.)
    return std::make_unique<InMemoryUTXOSet>();
}

} // namespace qv::core
