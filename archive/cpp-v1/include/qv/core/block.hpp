#pragma once

#include "types.hpp"
#include "transaction.hpp"
#include "result.hpp"
#include <vector>
#include <memory>

namespace qv::core {

/// BlockHeader: Metadata and commitment structure for a block
class BlockHeader {
public:
    /// Protocol version for this block
    std::uint32_t version = 1;

    /// Hash of the previous block (links to chain history)
    BlockHash prev_hash{};

    /// Merkle root of all transactions in this block
    HashDigest merkle_root{};

    /// UTXO commitment hash (allows light clients to verify unspent outputs)
    /// This is a hash of the UTXO set state after applying transactions
    HashDigest utxo_commitment{};

    /// Timestamp of block creation (Unix seconds)
    Timestamp timestamp = 0;

    /// Nonce used in PoW mining (for proof of work chains)
    std::uint64_t nonce = 0;

    /// Difficulty target for proof of work
    /// Encodes the target hash difficulty threshold
    std::uint64_t difficulty_target = 0;

    /// Proof of stake data (for PoS chains)
    /// Could contain validator signature, stake proof, etc.
    bytes pos_proof;

    /// Default constructor
    BlockHeader() = default;

    /// Constructor with all fields
    BlockHeader(
        std::uint32_t ver,
        const BlockHash& prev,
        const HashDigest& merkle,
        const HashDigest& utxo_commit,
        Timestamp ts,
        std::uint64_t n,
        std::uint64_t difficulty,
        const bytes& proof = {}
    ) : version(ver),
        prev_hash(prev),
        merkle_root(merkle),
        utxo_commitment(utxo_commit),
        timestamp(ts),
        nonce(n),
        difficulty_target(difficulty),
        pos_proof(proof) {}

    /// Serialize header to bytes for hashing
    [[nodiscard]] Result<bytes, std::string> to_bytes() const;

    /// Deserialize header from bytes
    static Result<BlockHeader, std::string> from_bytes(const bytes& data);

    /// Get serialized size of this header
    [[nodiscard]] std::size_t serialized_size() const noexcept;
};

/// Block: A complete block with header and transactions
class Block {
public:
    /// Block header containing metadata and commitments
    BlockHeader header;

    /// All transactions in this block
    std::vector<Transaction> transactions;

    /// Default constructor
    Block() = default;

    /// Constructor with header and transactions
    Block(const BlockHeader& hdr, std::vector<Transaction> txs)
        : header(hdr), transactions(std::move(txs)) {}

    /// Validate the block structure
    /// Checks:
    /// - Non-empty transaction list (unless genesis)
    /// - Valid transaction structures
    /// - Merkle root matches transactions
    /// - UTXO commitment is properly formed
    [[nodiscard]] bool is_valid() const noexcept;

    /// Validate all transactions in the block
    [[nodiscard]] bool transactions_valid() const noexcept;

    /// Compute the block hash (SHA256 double hash of header)
    [[nodiscard]] Result<BlockHash, std::string> compute_hash() const;

    /// Verify the merkle root against the transactions
    /// Returns true if merkle_root matches the computed merkle tree root
    [[nodiscard]] bool verify_merkle_root() const noexcept;

    /// Compute the merkle root of transactions
    /// Uses a binary tree of SHA256 hashes
    [[nodiscard]] Result<HashDigest, std::string> compute_merkle_root() const;

    /// Compute the UTXO commitment from the transactions
    /// In a full implementation, this would hash the resulting UTXO set state
    /// For now, it's a placeholder that could use transaction contents
    [[nodiscard]] Result<HashDigest, std::string> compute_utxo_commitment() const;

    /// Get total transaction count (including coinbase)
    [[nodiscard]] std::size_t tx_count() const noexcept {
        return transactions.size();
    }

    /// Get total input count across all transactions
    [[nodiscard]] std::size_t total_inputs() const noexcept;

    /// Get total output count across all transactions
    [[nodiscard]] std::size_t total_outputs() const noexcept;

    /// Calculate total block size in bytes
    [[nodiscard]] std::size_t serialized_size() const noexcept;

    /// Serialize the entire block to bytes
    [[nodiscard]] Result<bytes, std::string> to_bytes() const;

    /// Deserialize a block from bytes
    static Result<Block, std::string> from_bytes(const bytes& data);

    /// Check if this is a genesis block (no previous hash, specific structure)
    [[nodiscard]] bool is_genesis() const noexcept {
        constexpr BlockHash zero_hash{};
        return header.prev_hash == zero_hash && !transactions.empty();
    }

    /// Get the coinbase transaction (first transaction, if it exists)
    [[nodiscard]] const Transaction* get_coinbase() const noexcept {
        if (transactions.empty()) return nullptr;
        return &transactions[0];
    }

    /// Helper: Compute merkle root for a list of transaction hashes
    /// Uses a standard binary tree approach (Bitcoin-style)
    /// @param tx_hashes Vector of transaction IDs to hash
    /// @return Merkle root hash, or error if computation fails
    static Result<HashDigest, std::string> merkle_root_from_hashes(
        const std::vector<TxId>& tx_hashes
    );
};

} // namespace qv::core
