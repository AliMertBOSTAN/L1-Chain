#pragma once

#include <cstdint>
#include <vector>
#include <memory>
#include <optional>
#include <string>

namespace qv::storage {

/**
 * @brief Block header
 */
struct BlockHeader {
  // TODO: Define complete block header fields
  // - version: uint32_t
  // - previous_block_hash: 32 bytes
  // - merkle_root: 32 bytes
  // - timestamp: uint32_t
  // - difficulty_target: uint32_t
  // - nonce: uint64_t
  // - etc.

  std::vector<std::uint8_t> serialize() const;
  static std::optional<BlockHeader> deserialize(
      const std::vector<std::uint8_t>& data);
};

/**
 * @brief Complete block (header + transactions)
 */
struct Block {
  // TODO: Define complete block structure
  // - header: BlockHeader
  // - transactions: vector<Transaction>
  // - etc.

  std::vector<std::uint8_t> serialize() const;
  static std::optional<Block> deserialize(
      const std::vector<std::uint8_t>& data);
};

/**
 * @brief Block hash (256-bit, typically SHA-256)
 */
using BlockHash = std::vector<std::uint8_t>;

/**
 * @brief Block height
 */
using BlockHeight = std::uint32_t;

/**
 * @brief Result type for storage operations
 */
template <typename T>
using Result = std::optional<T>;

/**
 * @brief Persistent block storage interface
 *
 * Abstraction over RocksDB or LevelDB for block persistence.
 * Provides ACID operations on blocks.
 */
class BlockStore {
 public:
  virtual ~BlockStore() = default;

  /**
   * @brief Store a block
   * @param block The block to store
   * @return Result indicating success or error
   */
  virtual Result<void> put_block(const Block& block) = 0;

  /**
   * @brief Retrieve a block by hash
   * @param hash The block hash
   * @return Block if found, nullopt otherwise
   */
  virtual Result<Block> get_block(const BlockHash& hash) const = 0;

  /**
   * @brief Retrieve a block by height
   * @param height The block height
   * @return Block if found, nullopt otherwise
   */
  virtual Result<Block> get_block_by_height(BlockHeight height) const = 0;

  /**
   * @brief Retrieve a block header by hash
   * @param hash The block hash
   * @return BlockHeader if found, nullopt otherwise
   */
  virtual Result<BlockHeader> get_header(const BlockHash& hash) const = 0;

  /**
   * @brief Check if a block exists
   * @param hash The block hash
   * @return true if block exists
   */
  virtual bool has_block(const BlockHash& hash) const = 0;

  /**
   * @brief Get the current best block hash
   * @return Best block hash, or nullopt if chain is empty
   */
  virtual Result<BlockHash> get_best_block_hash() const = 0;

  /**
   * @brief Get the current chain height
   * @return Current block height
   */
  virtual BlockHeight get_chain_height() const = 0;

  /**
   * @brief Delete a block (for reorg)
   * @param hash The block hash
   * @return Result indicating success or error
   */
  virtual Result<void> delete_block(const BlockHash& hash) = 0;

  /**
   * @brief Compact the database
   * @return Result indicating success or error
   */
  virtual Result<void> compact() = 0;
};

/**
 * @brief RocksDB-based block store implementation
 */
class RocksDBBlockStore : public BlockStore {
 public:
  /**
   * @brief Construct with path to database
   * @param db_path Path to RocksDB directory
   */
  explicit RocksDBBlockStore(const std::string& db_path);

  ~RocksDBBlockStore() override;

  Result<void> put_block(const Block& block) override;
  Result<Block> get_block(const BlockHash& hash) const override;
  Result<Block> get_block_by_height(BlockHeight height) const override;
  Result<BlockHeader> get_header(const BlockHash& hash) const override;
  bool has_block(const BlockHash& hash) const override;
  Result<BlockHash> get_best_block_hash() const override;
  BlockHeight get_chain_height() const override;
  Result<void> delete_block(const BlockHash& hash) override;
  Result<void> compact() override;

 private:
  // TODO: Add RocksDB handle and initialization
  std::string db_path_;
};

/**
 * @brief Factory for creating block stores
 */
class BlockStoreFactory {
 public:
  /**
   * @brief Create a RocksDB-backed block store
   * @param db_path Path to database directory
   * @return Shared pointer to BlockStore
   */
  static std::shared_ptr<BlockStore> create_rocksdb(
      const std::string& db_path);

  /**
   * @brief Create an in-memory block store (for testing)
   * @return Shared pointer to BlockStore
   */
  static std::shared_ptr<BlockStore> create_in_memory();
};

}  // namespace qv::storage
