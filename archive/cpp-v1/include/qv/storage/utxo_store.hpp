#pragma once

#include <cstdint>
#include <vector>
#include <memory>
#include <optional>
#include <string>
#include <map>

namespace qv::storage {

/**
 * @brief Unspent Transaction Output
 *
 * Represents a single UTXO (coin) in the blockchain.
 */
struct UTXO {
  /**
   * @brief Transaction ID (32 bytes, SHA-256)
   */
  std::vector<std::uint8_t> tx_id;

  /**
   * @brief Output index within the transaction
   */
  std::uint32_t output_index = 0;

  /**
   * @brief Amount in satoshis
   */
  std::uint64_t amount = 0;

  /**
   * @brief Locking script (scriptPubKey)
   */
  std::vector<std::uint8_t> script;

  /**
   * @brief Block height where this UTXO was created
   */
  std::uint32_t block_height = 0;

  /**
   * @brief Is this UTXO coinbase output? (locked for maturation)
   */
  bool is_coinbase = false;

  /**
   * @brief Serialization
   */
  std::vector<std::uint8_t> serialize() const;
  static std::optional<UTXO> deserialize(
      const std::vector<std::uint8_t>& data);
};

/**
 * @brief UTXO identifier (txid:index pair)
 */
struct OutPoint {
  std::vector<std::uint8_t> tx_id;
  std::uint32_t output_index = 0;

  bool operator==(const OutPoint& other) const;
  bool operator<(const OutPoint& other) const;

  std::vector<std::uint8_t> serialize() const;
};

/**
 * @brief Template for defining UTXO set interface
 *
 * This would be the interface that core module expects.
 */
class UTXOSetInterface {
 public:
  virtual ~UTXOSetInterface() = default;

  /**
   * @brief Add a UTXO to the set
   */
  virtual void add_utxo(const OutPoint& outpoint, const UTXO& utxo) = 0;

  /**
   * @brief Remove a UTXO from the set
   */
  virtual void remove_utxo(const OutPoint& outpoint) = 0;

  /**
   * @brief Check if a UTXO exists
   */
  virtual bool has_utxo(const OutPoint& outpoint) const = 0;

  /**
   * @brief Get a UTXO by outpoint
   */
  virtual std::optional<UTXO> get_utxo(const OutPoint& outpoint) const = 0;
};

/**
 * @brief Persistent UTXO set backed by RocksDB
 *
 * Implements UTXOSetInterface and provides batch operations for
 * connecting/disconnecting blocks efficiently.
 */
class UTXOStore : public UTXOSetInterface {
 public:
  /**
   * @brief Construct with path to database
   * @param db_path Path to RocksDB directory
   */
  explicit UTXOStore(const std::string& db_path);

  ~UTXOStore() override;

  // UTXOSetInterface implementation
  void add_utxo(const OutPoint& outpoint, const UTXO& utxo) override;
  void remove_utxo(const OutPoint& outpoint) override;
  bool has_utxo(const OutPoint& outpoint) const override;
  std::optional<UTXO> get_utxo(const OutPoint& outpoint) const override;

  /**
   * @brief Batch operation: begin transaction
   *
   * Used for atomic block connect/disconnect operations.
   */
  void begin_batch();

  /**
   * @brief Batch operation: commit transaction
   */
  void commit_batch();

  /**
   * @brief Batch operation: rollback transaction
   */
  void rollback_batch();

  /**
   * @brief Get total UTXO count
   */
  std::uint64_t get_utxo_count() const;

  /**
   * @brief Get total value in UTXO set
   */
  std::uint64_t get_total_value() const;

  /**
   * @brief Snapshot UTXO set at block height
   * @param block_height Height to snapshot at
   * @return Map of outpoints to UTXOs
   */
  std::map<OutPoint, UTXO> snapshot_at_height(
      std::uint32_t block_height) const;

  /**
   * @brief Compact database
   */
  void compact();

 private:
  // TODO: Add RocksDB handle and batch operations
  std::string db_path_;
  bool in_batch_ = false;
};

/**
 * @brief Factory for creating UTXO stores
 */
class UTXOStoreFactory {
 public:
  /**
   * @brief Create RocksDB-backed UTXO store
   * @param db_path Path to database directory
   * @return Shared pointer to UTXOStore
   */
  static std::shared_ptr<UTXOStore> create_rocksdb(
      const std::string& db_path);

  /**
   * @brief Create in-memory UTXO store (for testing)
   * @return Shared pointer to UTXOStore
   */
  static std::shared_ptr<UTXOStore> create_in_memory();
};

}  // namespace qv::storage
