#pragma once

#include "types.hpp"
#include "transaction.hpp"
#include "result.hpp"
#include <optional>
#include <unordered_map>
#include <memory>

namespace qv::core {

/// UTXOEntry: Represents an unspent transaction output with metadata
struct UTXOEntry {
    /// The output data (value, script, stealth info)
    TxOutput output;

    /// Block height at which this UTXO was created
    Height block_height = 0;

    /// Whether this output is from a coinbase transaction
    /// Coinbase outputs have special spending rules (maturity period)
    bool is_coinbase = false;

    /// Default constructor
    UTXOEntry() = default;

    /// Constructor with values
    UTXOEntry(const TxOutput& out, Height height, bool coinbase = false)
        : output(out), block_height(height), is_coinbase(coinbase) {}

    /// Check if this UTXO is spendable at a given block height
    /// Coinbase UTXOs have a maturity period before spending
    [[nodiscard]] bool is_spendable(Height current_height, Height maturity_period = 100) const noexcept {
        if (!is_coinbase) return true;
        return (current_height - block_height) >= maturity_period;
    }
};

/// Custom hash function for OutPoint to use in unordered_map
struct OutPointHash {
    std::size_t operator()(const OutPoint& op) const noexcept {
        // Simple hash: combine first 8 bytes of tx_id with index
        std::size_t h1 = std::hash<std::uint32_t>{}(op.index);
        std::size_t h2 = 0;
        // Use first 8 bytes of tx_id as hash material
        if (op.tx_id.size() >= sizeof(std::uint64_t)) {
            for (std::size_t i = 0; i < 8; ++i) {
                h2 = (h2 << 8) | op.tx_id[i];
            }
        }
        return h1 ^ (h2 << 1);
    }
};

/// IUTXOSet: Abstract interface for UTXO set operations
class IUTXOSet {
public:
    virtual ~IUTXOSet() = default;

    /// Add a new UTXO to the set
    /// @param outpoint The OutPoint identifying this UTXO
    /// @param entry The UTXOEntry with output data and metadata
    /// @return Error if the UTXO already exists or insertion fails
    virtual Result<void, std::string> add(const OutPoint& outpoint, const UTXOEntry& entry) = 0;

    /// Spend (remove) a UTXO from the set
    /// @param outpoint The OutPoint to spend
    /// @return The UTXOEntry that was spent, or error if not found
    virtual Result<UTXOEntry, std::string> spend(const OutPoint& outpoint) = 0;

    /// Get a UTXO without removing it
    /// @param outpoint The OutPoint to retrieve
    /// @return Optional containing the UTXOEntry if found
    virtual std::optional<UTXOEntry> get(const OutPoint& outpoint) const = 0;

    /// Check if a UTXO exists in the set
    /// @param outpoint The OutPoint to check
    /// @return true if the UTXO exists, false otherwise
    virtual bool contains(const OutPoint& outpoint) const noexcept = 0;

    /// Get the total number of UTXOs in the set
    [[nodiscard]] virtual std::size_t size() const noexcept = 0;

    /// Check if the UTXO set is empty
    [[nodiscard]] virtual bool is_empty() const noexcept = 0;

    /// Compute a cryptographic commitment hash of the entire UTXO set
    /// This hash changes when any UTXO is added or removed
    /// Used for light client verification and consensus
    /// @return Hash digest of the UTXO set state, or error if computation fails
    virtual Result<HashDigest, std::string> compute_commitment() const = 0;

    /// Get the total value across all UTXOs
    [[nodiscard]] virtual Amount total_value() const noexcept = 0;

    /// Clear all UTXOs from the set
    virtual void clear() noexcept = 0;
};

/// InMemoryUTXOSet: In-memory implementation of UTXO set using a hash map
/// This is suitable for full nodes that maintain the entire UTXO set
class InMemoryUTXOSet : public IUTXOSet {
public:
    /// Default constructor
    InMemoryUTXOSet() = default;

    /// Copy constructor
    InMemoryUTXOSet(const InMemoryUTXOSet& other);

    /// Move constructor
    InMemoryUTXOSet(InMemoryUTXOSet&& other) noexcept;

    /// Copy assignment
    InMemoryUTXOSet& operator=(const InMemoryUTXOSet& other);

    /// Move assignment
    InMemoryUTXOSet& operator=(InMemoryUTXOSet&& other) noexcept;

    /// Destructor
    ~InMemoryUTXOSet() override = default;

    /// Add a new UTXO to the set
    Result<void, std::string> add(const OutPoint& outpoint, const UTXOEntry& entry) override;

    /// Spend (remove) a UTXO from the set
    Result<UTXOEntry, std::string> spend(const OutPoint& outpoint) override;

    /// Get a UTXO without removing it
    std::optional<UTXOEntry> get(const OutPoint& outpoint) const override;

    /// Check if a UTXO exists
    bool contains(const OutPoint& outpoint) const noexcept override;

    /// Get the number of UTXOs
    [[nodiscard]] std::size_t size() const noexcept override {
        return utxos_.size();
    }

    /// Check if empty
    [[nodiscard]] bool is_empty() const noexcept override {
        return utxos_.empty();
    }

    /// Compute commitment hash of the UTXO set
    Result<HashDigest, std::string> compute_commitment() const override;

    /// Get total value of all UTXOs
    [[nodiscard]] Amount total_value() const noexcept override;

    /// Clear all UTXOs
    void clear() noexcept override {
        utxos_.clear();
    }

    /// Batch apply multiple UTXOs
    /// Adds new UTXOs and spends (removes) specified ones
    /// Useful for applying block transactions atomically
    /// @param to_add Map of OutPoints to UTXOEntries to add
    /// @param to_spend Vector of OutPoints to remove
    /// @return Error if any operation fails
    Result<void, std::string> apply_batch(
        const std::vector<std::pair<OutPoint, UTXOEntry>>& to_add,
        const std::vector<OutPoint>& to_spend
    );

    /// Get a snapshot of all OutPoints in the set
    [[nodiscard]] std::vector<OutPoint> all_outpoints() const;

    /// Get a snapshot of all UTXOEntries in the set
    [[nodiscard]] std::vector<UTXOEntry> all_entries() const;

private:
    /// Internal storage: OutPoint -> UTXOEntry mapping
    std::unordered_map<OutPoint, UTXOEntry, OutPointHash> utxos_;
};

/// Factory function to create a new UTXO set instance
/// @param use_memory If true, creates InMemoryUTXOSet; in future, could support other backends
/// @return Unique pointer to IUTXOSet implementation
[[nodiscard]] std::unique_ptr<IUTXOSet> create_utxo_set(bool use_memory = true);

} // namespace qv::core
