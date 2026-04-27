#pragma once

#include <vector>
#include <cstdint>
#include <memory>
#include <optional>
#include <map>
#include "erasure.hpp"

namespace qv::da {

/**
 * @brief Data aging stages for storage optimization
 *
 * HOT -> WARM -> COLD progression based on block age:
 * - HOT (0-7 days): Full block data (for fast access)
 * - WARM (7-30 days): Erasure-coded shards only
 * - COLD (30+ days): PQC-STARK proof of availability only
 */
enum class AgeStage {
  HOT,   // Full data, all nodes
  WARM,  // Erasure shards, subset of nodes
  COLD,  // Proof only, archival
};

/**
 * @brief Block data at different age stages
 */
struct AgedBlockData {
  /**
   * @brief Block height
   */
  std::uint32_t block_height = 0;

  /**
   * @brief Block hash (for identification)
   */
  std::vector<std::uint8_t> block_hash;

  /**
   * @brief Current age stage
   */
  AgeStage stage = AgeStage::HOT;

  /**
   * @brief Timestamp when block was created
   */
  std::uint64_t creation_time = 0;

  /**
   * @brief HOT stage: Full block data
   */
  std::vector<std::uint8_t> full_data;

  /**
   * @brief WARM stage: Erasure-coded shards
   */
  std::vector<Shard> erasure_shards;

  /**
   * @brief COLD stage: PQC-STARK proof
   */
  std::vector<std::uint8_t> pqc_stark_proof;

  /**
   * @brief Metadata for COLD proof (which shards it covers)
   */
  struct ColdProofMetadata {
    std::vector<std::uint16_t> covered_shard_indices;
    std::uint32_t proof_depth = 0;
  } cold_metadata;
};

/**
 * @brief Data availability via aging
 *
 * Implements a three-tier storage strategy for blocks:
 * 1. HOT: Full blockchain data for recent blocks
 * 2. WARM: Erasure-coded shards for blocks 1-4 weeks old
 * 3. COLD: PQC-STARK proofs for archival (6+ months)
 *
 * This reduces storage burden while maintaining provable availability.
 */
class DataAging {
 public:
  /**
   * @brief Configuration
   */
  struct Config {
    // Age thresholds (in seconds)
    std::uint64_t hot_threshold = 7 * 24 * 3600;      // 7 days
    std::uint64_t warm_threshold = 30 * 24 * 3600;    // 30 days
    std::uint64_t cold_threshold = 180 * 24 * 3600;   // 6 months

    // Erasure coding parameters (for WARM stage)
    std::uint16_t k_data_shards = 4;     // Can recover from any 4 shards
    std::uint16_t m_parity_shards = 2;   // 2 parity shards
  };

  /**
   * @brief Construct with configuration
   * @param config Age thresholds and erasure parameters
   */
  explicit DataAging(const Config& config = Config{});

  /**
   * @brief Add block data (initially HOT)
   * @param block_height Block height
   * @param block_hash Block hash
   * @param data Full block data
   * @return Result indicating success
   */
  std::optional<std::string> add_block(
      std::uint32_t block_height,
      const std::vector<std::uint8_t>& block_hash,
      const std::vector<std::uint8_t>& data);

  /**
   * @brief Promote block to next stage
   *
   * HOT -> WARM: Compute erasure shards, optionally drop full data
   * WARM -> COLD: Compute PQC-STARK proof, drop shards
   *
   * @param block_hash The block to promote
   * @return Result indicating success
   */
  std::optional<std::string> promote(const std::vector<std::uint8_t>& block_hash);

  /**
   * @brief Demote block to previous stage
   *
   * COLD -> WARM: Recover full data from shards (requires shards in storage)
   * WARM -> HOT: Already full data, no-op
   *
   * @param block_hash The block to demote
   * @return Result indicating success
   */
  std::optional<std::string> demote(const std::vector<std::uint8_t>& block_hash);

  /**
   * @brief Get block data at current stage
   * @param block_hash The block hash
   * @return AgedBlockData if found
   */
  std::optional<AgedBlockData> get_block(
      const std::vector<std::uint8_t>& block_hash) const;

  /**
   * @brief Get current stage of a block
   * @param block_hash The block hash
   * @return AgeStage if found
   */
  std::optional<AgeStage> get_stage(
      const std::vector<std::uint8_t>& block_hash) const;

  /**
   * @brief Recover full data from WARM or COLD stage
   *
   * Reconstructs the original block data from:
   * - WARM: Any k erasure shards
   * - COLD: Proof of availability (may require network fetch)
   *
   * @param block_hash The block hash
   * @return Original block data, or nullopt if recovery fails
   */
  std::optional<std::vector<std::uint8_t>> recover_data(
      const std::vector<std::uint8_t>& block_hash);

  /**
   * @brief Verify COLD proof (PQC-STARK based)
   *
   * Cryptographically verifies that data was available at block creation.
   * Does NOT recover the data, only proves availability.
   *
   * @param block_hash The block hash
   * @param genesis_hash The chain's genesis block hash (for chain context)
   * @return true if proof is valid
   */
  bool verify_cold_proof(const std::vector<std::uint8_t>& block_hash,
                         const std::vector<std::uint8_t>& genesis_hash) const;

  /**
   * @brief Auto-age old blocks
   *
   * Call periodically to automatically promote old blocks to next stage.
   *
   * @param current_time Current timestamp
   * @return Number of blocks promoted
   */
  std::size_t auto_age_blocks(std::uint64_t current_time);

  /**
   * @brief Get storage stats
   */
  struct StorageStats {
    std::size_t hot_blocks = 0;      // Full data
    std::size_t warm_blocks = 0;     // Shards only
    std::size_t cold_blocks = 0;     // Proofs only
    std::uint64_t hot_bytes = 0;
    std::uint64_t warm_bytes = 0;
    std::uint64_t cold_bytes = 0;
  };

  StorageStats get_stats() const;

 private:
  /**
   * @brief Compute PQC-STARK proof for COLD storage
   */
  std::optional<std::vector<std::uint8_t>> compute_stark_proof(
      const std::vector<std::uint8_t>& data,
      const std::vector<std::uint8_t>& block_hash);

  /**
   * @brief Verify PQC-STARK proof
   */
  bool verify_stark_proof(const std::vector<std::uint8_t>& proof,
                          const std::vector<std::uint8_t>& block_hash,
                          const std::vector<std::uint8_t>& genesis_hash) const;

  /**
   * @brief State
   */
  Config config_;
  ErasureCoder coder_;
  std::map<std::vector<std::uint8_t>, AgedBlockData> blocks_;
};

}  // namespace qv::da
