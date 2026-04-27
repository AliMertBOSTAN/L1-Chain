#include "qv/core/block.hpp"
#include <numeric>
#include <algorithm>
#include <sstream>
#include <cstring>

namespace qv::core {

// ============================================================================
// BlockHeader Implementation
// ============================================================================

Result<bytes, std::string> BlockHeader::to_bytes() const {
    try {
        bytes result;

        // Version (4 bytes)
        result.push_back((version >> 0) & 0xFF);
        result.push_back((version >> 8) & 0xFF);
        result.push_back((version >> 16) & 0xFF);
        result.push_back((version >> 24) & 0xFF);

        // Previous block hash (32 bytes)
        result.insert(result.end(), prev_hash.begin(), prev_hash.end());

        // Merkle root (32 bytes)
        result.insert(result.end(), merkle_root.begin(), merkle_root.end());

        // UTXO commitment (32 bytes)
        result.insert(result.end(), utxo_commitment.begin(), utxo_commitment.end());

        // Timestamp (8 bytes)
        std::uint64_t ts = timestamp;
        result.push_back((ts >> 0) & 0xFF);
        result.push_back((ts >> 8) & 0xFF);
        result.push_back((ts >> 16) & 0xFF);
        result.push_back((ts >> 24) & 0xFF);
        result.push_back((ts >> 32) & 0xFF);
        result.push_back((ts >> 40) & 0xFF);
        result.push_back((ts >> 48) & 0xFF);
        result.push_back((ts >> 56) & 0xFF);

        // Nonce (8 bytes)
        std::uint64_t n = nonce;
        result.push_back((n >> 0) & 0xFF);
        result.push_back((n >> 8) & 0xFF);
        result.push_back((n >> 16) & 0xFF);
        result.push_back((n >> 24) & 0xFF);
        result.push_back((n >> 32) & 0xFF);
        result.push_back((n >> 40) & 0xFF);
        result.push_back((n >> 48) & 0xFF);
        result.push_back((n >> 56) & 0xFF);

        // Difficulty target (8 bytes)
        std::uint64_t diff = difficulty_target;
        result.push_back((diff >> 0) & 0xFF);
        result.push_back((diff >> 8) & 0xFF);
        result.push_back((diff >> 16) & 0xFF);
        result.push_back((diff >> 24) & 0xFF);
        result.push_back((diff >> 32) & 0xFF);
        result.push_back((diff >> 40) & 0xFF);
        result.push_back((diff >> 48) & 0xFF);
        result.push_back((diff >> 56) & 0xFF);

        // PoS proof length (2 bytes) and data
        if (pos_proof.size() > 0xFFFF) {
            return Result<bytes, std::string>("PoS proof too large");
        }
        std::uint16_t proof_len = static_cast<std::uint16_t>(pos_proof.size());
        result.push_back((proof_len >> 0) & 0xFF);
        result.push_back((proof_len >> 8) & 0xFF);
        result.insert(result.end(), pos_proof.begin(), pos_proof.end());

        return Result<bytes, std::string>(result);
    } catch (const std::exception& e) {
        return Result<bytes, std::string>(
            std::string("BlockHeader serialization error: ") + e.what()
        );
    }
}

Result<BlockHeader, std::string> BlockHeader::from_bytes(const bytes& data) {
    // TODO: Implement header deserialization
    return Result<BlockHeader, std::string>(
        std::string("BlockHeader deserialization not yet implemented")
    );
}

std::size_t BlockHeader::serialized_size() const noexcept {
    // version (4) + prev_hash (32) + merkle_root (32) + utxo_commitment (32)
    // + timestamp (8) + nonce (8) + difficulty_target (8) + pos_proof_len (2) + pos_proof data
    return 4 + 32 + 32 + 32 + 8 + 8 + 8 + 2 + pos_proof.size();
}

// ============================================================================
// Block Implementation
// ============================================================================

bool Block::is_valid() const noexcept {
    // Blocks must have at least one transaction (or be genesis)
    if (transactions.empty()) {
        return false;
    }

    // All transactions must be valid
    if (!transactions_valid()) {
        return false;
    }

    // Verify merkle root matches transactions
    if (!verify_merkle_root()) {
        return false;
    }

    return true;
}

bool Block::transactions_valid() const noexcept {
    return std::all_of(transactions.begin(), transactions.end(),
        [](const Transaction& tx) { return tx.is_valid(); });
}

Result<BlockHash, std::string> Block::compute_hash() const {
    auto header_bytes = header.to_bytes();
    if (!header_bytes.is_ok()) {
        return Result<BlockHash, std::string>(header_bytes.error());
    }

    // TODO: Implement proper SHA256 double hash
    // For now, this is a stub implementation

    BlockHash hash{};
    const auto& bytes_data = header_bytes.value();
    std::size_t copy_size = std::min(bytes_data.size(), hash.size());
    std::copy(bytes_data.begin(), bytes_data.begin() + copy_size, hash.begin());

    return Result<BlockHash, std::string>(hash);
}

bool Block::verify_merkle_root() const noexcept {
    auto computed_root = compute_merkle_root();
    if (!computed_root.is_ok()) {
        return false;
    }

    return computed_root.value() == header.merkle_root;
}

Result<HashDigest, std::string> Block::compute_merkle_root() const {
    if (transactions.empty()) {
        HashDigest empty{};
        return Result<HashDigest, std::string>(empty);
    }

    // Compute transaction IDs
    std::vector<TxId> tx_hashes;
    tx_hashes.reserve(transactions.size());

    for (const auto& tx : transactions) {
        auto txid = tx.compute_txid();
        if (!txid.is_ok()) {
            return Result<HashDigest, std::string>(txid.error());
        }
        tx_hashes.push_back(txid.value());
    }

    return merkle_root_from_hashes(tx_hashes);
}

Result<HashDigest, std::string> Block::merkle_root_from_hashes(
    const std::vector<TxId>& tx_hashes
) {
    if (tx_hashes.empty()) {
        HashDigest empty{};
        return Result<HashDigest, std::string>(empty);
    }

    if (tx_hashes.size() == 1) {
        HashDigest root;
        std::copy(tx_hashes[0].begin(), tx_hashes[0].end(), root.begin());
        return Result<HashDigest, std::string>(root);
    }

    // Build merkle tree from bottom up
    std::vector<HashDigest> current_level;
    current_level.reserve(tx_hashes.size());

    // Convert TxIds to HashDigests
    for (const auto& txid : tx_hashes) {
        HashDigest hd;
        std::copy(txid.begin(), txid.end(), hd.begin());
        current_level.push_back(hd);
    }

    // Build tree upwards
    while (current_level.size() > 1) {
        std::vector<HashDigest> next_level;
        next_level.reserve((current_level.size() + 1) / 2);

        for (std::size_t i = 0; i < current_level.size(); i += 2) {
            HashDigest combined;

            // Combine two hashes
            if (i + 1 < current_level.size()) {
                // Two children: hash(left || right)
                for (std::size_t j = 0; j < 32; ++j) {
                    combined[j] = current_level[i][j] ^ current_level[i + 1][j];
                }
            } else {
                // Single child: duplicate and hash
                combined = current_level[i];
            }

            next_level.push_back(combined);
        }

        current_level = std::move(next_level);
    }

    return Result<HashDigest, std::string>(current_level[0]);
}

Result<HashDigest, std::string> Block::compute_utxo_commitment() const {
    // TODO: Implement proper UTXO commitment
    // This would typically involve hashing the resulting UTXO set state
    // For now, compute a simple hash of transaction outputs

    bytes commitment_data;

    for (const auto& tx : transactions) {
        for (const auto& output : tx.outputs) {
            // Add output value (8 bytes)
            std::uint64_t val = output.value;
            commitment_data.push_back((val >> 0) & 0xFF);
            commitment_data.push_back((val >> 8) & 0xFF);
            commitment_data.push_back((val >> 16) & 0xFF);
            commitment_data.push_back((val >> 24) & 0xFF);
            commitment_data.push_back((val >> 32) & 0xFF);
            commitment_data.push_back((val >> 40) & 0xFF);
            commitment_data.push_back((val >> 48) & 0xFF);
            commitment_data.push_back((val >> 56) & 0xFF);

            // Add locking script
            commitment_data.insert(commitment_data.end(),
                output.locking_script.begin(),
                output.locking_script.end());
        }
    }

    // Create commitment from data
    HashDigest commitment{};
    std::size_t copy_size = std::min(commitment_data.size(), commitment.size());
    std::copy(commitment_data.begin(), commitment_data.begin() + copy_size, commitment.begin());

    return Result<HashDigest, std::string>(commitment);
}

std::size_t Block::total_inputs() const noexcept {
    return std::accumulate(transactions.begin(), transactions.end(),
        std::size_t(0),
        [](std::size_t sum, const Transaction& tx) {
            return sum + tx.inputs.size();
        });
}

std::size_t Block::total_outputs() const noexcept {
    return std::accumulate(transactions.begin(), transactions.end(),
        std::size_t(0),
        [](std::size_t sum, const Transaction& tx) {
            return sum + tx.outputs.size();
        });
}

std::size_t Block::serialized_size() const noexcept {
    std::size_t size = header.serialized_size();
    for (const auto& tx : transactions) {
        size += tx.serialized_size();
    }
    return size;
}

Result<bytes, std::string> Block::to_bytes() const {
    try {
        bytes result;

        // Serialize header
        auto header_bytes = header.to_bytes();
        if (!header_bytes.is_ok()) {
            return Result<bytes, std::string>(header_bytes.error());
        }
        result.insert(result.end(),
            header_bytes.value().begin(),
            header_bytes.value().end());

        // Transaction count (varint, simplified)
        if (transactions.size() > 252) {
            return Result<bytes, std::string>("Too many transactions in block");
        }
        result.push_back(static_cast<std::uint8_t>(transactions.size()));

        // Serialize each transaction
        for (const auto& tx : transactions) {
            auto tx_bytes = tx.to_bytes();
            if (!tx_bytes.is_ok()) {
                return Result<bytes, std::string>(tx_bytes.error());
            }
            result.insert(result.end(),
                tx_bytes.value().begin(),
                tx_bytes.value().end());
        }

        return Result<bytes, std::string>(result);
    } catch (const std::exception& e) {
        return Result<bytes, std::string>(
            std::string("Block serialization error: ") + e.what()
        );
    }
}

Result<Block, std::string> Block::from_bytes(const bytes& data) {
    // TODO: Implement block deserialization
    return Result<Block, std::string>(
        std::string("Block deserialization not yet implemented")
    );
}

} // namespace qv::core
