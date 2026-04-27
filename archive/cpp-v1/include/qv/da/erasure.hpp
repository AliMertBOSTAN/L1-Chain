#pragma once

#include <vector>
#include <cstdint>
#include <memory>
#include <optional>

namespace qv::da {

/**
 * @brief Erasure-coded data shard
 *
 * Result of Reed-Solomon encoding. Can recover original data
 * from any k shards (where k is the number of data shards).
 */
struct Shard {
  /**
   * @brief Shard index (0 to k+m-1)
   */
  std::uint16_t index = 0;

  /**
   * @brief Shard data
   */
  std::vector<std::uint8_t> data;

  /**
   * @brief Is this a data shard (0 to k-1)?
   */
  bool is_data_shard() const { return index < k_total_; }

  /**
   * @brief Serialization
   */
  std::vector<std::uint8_t> serialize() const;
  static std::optional<Shard> deserialize(
      const std::vector<std::uint8_t>& data);

 private:
  friend class ErasureCoder;
  std::uint16_t k_total_ = 0;  // Total data shards
};

/**
 * @brief Result of erasure coding
 */
struct EncodingResult {
  /**
   * @brief All shards (data + parity)
   */
  std::vector<Shard> shards;

  /**
   * @brief Number of data shards (k)
   */
  std::uint16_t k_data_shards = 0;

  /**
   * @brief Number of parity shards (m)
   */
  std::uint16_t m_parity_shards = 0;

  /**
   * @brief Original data size
   */
  std::uint64_t original_size = 0;
};

/**
 * @brief Reed-Solomon erasure coder
 *
 * Encodes data into k data shards + m parity shards.
 * Can recover original data from any k shards.
 *
 * Typical configuration:
 * - k=4, m=2: Can tolerate 2 shard losses
 * - k=256, m=256: Can tolerate 256 shard losses
 */
class ErasureCoder {
 public:
  /**
   * @brief Encode data into shards
   *
   * @param data The data to encode
   * @param k_data_shards Number of data shards
   * @param m_parity_shards Number of parity shards
   * @return EncodingResult with all shards, or nullopt on error
   *
   * Example:
   *   auto result = coder.encode(my_data, 4, 2);
   *   // result contains 6 shards total
   *   // Can recover from any 4 shards
   */
  std::optional<EncodingResult> encode(
      const std::vector<std::uint8_t>& data,
      std::uint16_t k_data_shards,
      std::uint16_t m_parity_shards) const;

  /**
   * @brief Decode data from shards
   *
   * @param shards The shards (only need k shards, can provide more)
   * @param k_data_shards Number of original data shards
   * @param original_size Size of original data (for trimming)
   * @return Decoded data, or nullopt on error
   *
   * Example:
   *   auto decoded = coder.decode(available_shards, 4, original_size);
   */
  std::optional<std::vector<std::uint8_t>> decode(
      const std::vector<Shard>& shards,
      std::uint16_t k_data_shards,
      std::uint64_t original_size) const;

  /**
   * @brief Verify shard integrity (optional checksum)
   * @param shard The shard to verify
   * @return true if shard is valid
   */
  bool verify_shard(const Shard& shard) const;

 private:
  // TODO: Integrate with actual Reed-Solomon library
  // (e.g., Jerasure, ISA-L, or custom implementation)
};

}  // namespace qv::da
