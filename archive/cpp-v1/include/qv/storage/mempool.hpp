#pragma once

#include <cstdint>
#include <vector>
#include <memory>
#include <optional>
#include <set>
#include <map>
#include <string>

namespace qv::storage {

/**
 * @brief Transaction for mempool storage
 *
 * This is a minimal definition; the full Transaction type should be in core module.
 */
struct Transaction {
  // TODO: Define complete transaction fields
  // - version: uint32_t
  // - inputs: vector<Input>
  // - outputs: vector<Output>
  // - locktime: uint32_t
  // - tx_id: computed SHA-256

  std::vector<std::uint8_t> serialize() const;
  static std::optional<Transaction> deserialize(
      const std::vector<std::uint8_t>& data);

  std::vector<std::uint8_t> calculate_txid() const;
  std::uint64_t get_fee() const;  // Total inputs - Total outputs
  std::size_t get_size() const;   // Serialized size in bytes
};

/**
 * @brief Transaction ID
 */
using TxId = std::vector<std::uint8_t>;

/**
 * @brief Mempool entry metadata
 */
struct MempoolEntry {
  Transaction tx;
  std::uint64_t fee = 0;           // Satoshis
  std::uint64_t fee_rate = 0;      // Satoshis per byte
  std::uint32_t priority = 0;      // Age-adjusted priority
  std::uint64_t entry_time = 0;    // Timestamp when added
  std::size_t size = 0;            // Serialized size
  std::vector<TxId> dependencies;  // Input UTXOs from other mempool txs
};

/**
 * @brief Memory pool for pending transactions
 *
 * Maintains transactions awaiting confirmation:
 * - Fee-sorted priority queue for block candidates
 * - UTXO dependency tracking to prevent spending same output twice
 * - Eviction policy when pool is full (lowest fee rate first)
 * - Transaction rejection for policy violations
 */
class Mempool {
 public:
  /**
   * @brief Configuration
   */
  struct Config {
    std::uint64_t max_mempool_size = 300 * 1024 * 1024;  // 300 MB
    std::size_t max_transactions = 50000;
    std::uint64_t min_fee_rate = 1;                      // Satoshis/byte
    std::uint32_t expiry_time = 14 * 24 * 3600;          // 14 days
  };

  /**
   * @brief Construct mempool with configuration
   * @param config Mempool limits and policies
   */
  explicit Mempool(const Config& config = Config{});

  /**
   * @brief Add transaction to mempool
   *
   * Validates:
   * - Transaction is valid (signature, format)
   * - No double-spending (inputs not already in mempool)
   * - Fee meets minimum (if configured)
   * - Inputs are valid UTXOs
   *
   * @param tx The transaction to add
   * @return Result: success or error message
   */
  std::optional<std::string> add_transaction(const Transaction& tx);

  /**
   * @brief Remove transaction from mempool
   * @param txid The transaction ID
   * @return true if transaction was in mempool
   */
  bool remove_transaction(const TxId& txid);

  /**
   * @brief Get transaction from mempool
   * @param txid The transaction ID
   * @return Transaction if present
   */
  std::optional<Transaction> get_transaction(const TxId& txid) const;

  /**
   * @brief Get entry metadata
   * @param txid The transaction ID
   * @return Entry if present
   */
  std::optional<MempoolEntry> get_entry(const TxId& txid) const;

  /**
   * @brief Get transactions for block template
   *
   * Returns highest fee-rate transactions, respecting dependencies.
   *
   * @param max_count Maximum transactions to return
   * @param max_bytes Maximum total size in bytes
   * @return Transactions sorted by fee rate (highest first)
   */
  std::vector<Transaction> get_transactions_for_block(
      std::size_t max_count, std::uint64_t max_bytes = 1024 * 1024) const;

  /**
   * @brief Check if transaction is in mempool
   * @param txid The transaction ID
   * @return true if transaction is present
   */
  bool has_transaction(const TxId& txid) const;

  /**
   * @brief Check if input is already spent in mempool
   * @param input_outpoint The output being spent
   * @return true if already spent by another transaction
   */
  bool is_input_spent(const std::vector<std::uint8_t>& input_outpoint) const;

  /**
   * @brief Get transaction count
   */
  std::size_t size() const;

  /**
   * @brief Get total mempool size in bytes
   */
  std::uint64_t bytes() const;

  /**
   * @brief Get mempool info
   */
  struct Info {
    std::size_t transaction_count = 0;
    std::uint64_t total_bytes = 0;
    std::uint64_t total_fee = 0;
    std::uint64_t min_fee_rate = 0;
  };

  Info get_info() const;

  /**
   * @brief Clear entire mempool (for testing or reorgs)
   */
  void clear();

  /**
   * @brief Remove expired transactions
   *
   * Removes transactions older than config.expiry_time
   *
   * @param current_time Current timestamp
   * @return Number of transactions removed
   */
  std::size_t expire_old_transactions(std::uint64_t current_time);

  /**
   * @brief Evict lowest-fee transactions to reach target size
   *
   * Used when mempool exceeds limits.
   *
   * @param target_size Target total size in bytes
   * @return Number of transactions evicted
   */
  std::size_t evict_to_size(std::uint64_t target_size);

 private:
  /**
   * @brief Validate transaction for mempool acceptance
   */
  std::optional<std::string> validate_transaction(const Transaction& tx);

  /**
   * @brief Check for double-spend conflicts
   */
  bool has_conflict(const Transaction& tx) const;

  /**
   * @brief Calculate transaction fee rate
   */
  std::uint64_t calculate_fee_rate(const Transaction& tx) const;

  /**
   * @brief State
   */
  Config config_;

  // TODO: Replace with efficient data structures:
  // - map<TxId, MempoolEntry> for O(1) lookup
  // - multiset<MempoolEntry> for fee-rate sorting
  // - map<OutPoint, TxId> for spent input tracking
  // - etc.

  std::map<TxId, MempoolEntry> entries_;
  std::uint64_t total_bytes_ = 0;
};

}  // namespace qv::storage
