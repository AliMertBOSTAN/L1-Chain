#pragma once

#include <vector>
#include <cstdint>
#include <memory>
#include <optional>
#include <string>
#include <map>

namespace qv::da {

/**
 * @brief Sidechain data entry
 */
struct SidechainEntry {
  /**
   * @brief Transaction ID (32 bytes, SHA-256)
   */
  std::vector<std::uint8_t> tx_id;

  /**
   * @brief Offchain data
   */
  std::vector<std::uint8_t> data;

  /**
   * @brief Size of data
   */
  std::uint64_t size = 0;

  /**
   * @brief Hash of data (SHA-256)
   */
  std::vector<std::uint8_t> data_hash;

  /**
   * @brief Timestamp when data was stored
   */
  std::uint64_t stored_at = 0;

  /**
   * @brief Number of times this data was sampled successfully
   */
  std::uint32_t sample_count = 0;

  /**
   * @brief Last sample timestamp
   */
  std::uint64_t last_sample_time = 0;
};

/**
 * @brief Availability proof from sampling
 */
struct AvailabilityProof {
  /**
   * @brief Transaction ID being proved
   */
  std::vector<std::uint8_t> tx_id;

  /**
   * @brief Data hash that was sampled
   */
  std::vector<std::uint8_t> data_hash;

  /**
   * @brief Timestamp of proof
   */
  std::uint64_t proof_time = 0;

  /**
   * @brief Number of successful samples
   */
  std::uint32_t successful_samples = 0;

  /**
   * @brief Total samples attempted
   */
  std::uint32_t total_samples = 0;

  /**
   * @brief Availability confidence (0.0 to 1.0)
   */
  double availability_confidence = 0.0;
};

/**
 * @brief Data availability sidechain
 *
 * Stores transaction data offchain while maintaining availability proofs
 * via sampling. The L1 doesn't store the data, but samples it periodically
 * to verify it remains available.
 *
 * Use case: Large data objects (images, documents, archives) that don't
 * need to be stored by every node.
 */
class DASidechain {
 public:
  /**
   * @brief Configuration
   */
  struct Config {
    /**
     * @brief Maximum data size per transaction (1 GB)
     */
    std::uint64_t max_data_size = 1024 * 1024 * 1024;

    /**
     * @brief Sampling interval (how often to verify availability)
     */
    std::uint32_t sampling_interval = 3600;  // 1 hour

    /**
     * @brief Number of samples per verification round
     */
    std::uint16_t samples_per_round = 10;

    /**
     * @brief Confidence threshold for availability (0.0-1.0)
     */
    double confidence_threshold = 0.95;

    /**
     * @brief Data retention time (30 days)
     */
    std::uint64_t retention_time = 30 * 24 * 3600;
  };

  /**
   * @brief Construct sidechain with configuration
   * @param config DA sidechain limits and policies
   */
  explicit DASidechain(const Config& config = Config{});

  /**
   * @brief Store offchain data for a transaction
   *
   * @param tx_id The transaction ID
   * @param data The data to store offchain
   * @return Result: success or error message
   *
   * Returns error if:
   * - Data exceeds max_data_size
   * - tx_id is malformed
   * - Storage backend fails
   */
  std::optional<std::string> store_offchain_data(
      const std::vector<std::uint8_t>& tx_id,
      const std::vector<std::uint8_t>& data);

  /**
   * @brief Retrieve offchain data for a transaction
   *
   * @param tx_id The transaction ID
   * @return The stored data, or nullopt if not found
   */
  std::optional<std::vector<std::uint8_t>> retrieve_offchain_data(
      const std::vector<std::uint8_t>& tx_id) const;

  /**
   * @brief Check if transaction data exists
   * @param tx_id The transaction ID
   * @return true if data is stored
   */
  bool has_data(const std::vector<std::uint8_t>& tx_id) const;

  /**
   * @brief Verify availability via sampling
   *
   * Performs cryptographic sampling-based availability checks.
   * A node (or random set of nodes) challenges the network:
   * "Prove this data is available by returning a hash of byte range [i:j]"
   *
   * @param tx_id The transaction ID to verify
   * @return AvailabilityProof with confidence score
   */
  std::optional<AvailabilityProof> verify_availability(
      const std::vector<std::uint8_t>& tx_id);

  /**
   * @brief Respond to availability challenge
   *
   * Given a challenge (byte range), return the hash of that range.
   *
   * @param tx_id The transaction ID
   * @param start_byte Starting byte offset
   * @param end_byte Ending byte offset (exclusive)
   * @return Hash of data[start:end], or nullopt if out of bounds
   */
  std::optional<std::vector<std::uint8_t>> respond_to_challenge(
      const std::vector<std::uint8_t>& tx_id,
      std::uint64_t start_byte,
      std::uint64_t end_byte) const;

  /**
   * @brief Delete offchain data (after retention expires)
   * @param tx_id The transaction ID
   * @return true if data was deleted
   */
  bool delete_data(const std::vector<std::uint8_t>& tx_id);

  /**
   * @brief Cleanup expired data
   *
   * Call periodically to remove data older than retention_time.
   *
   * @param current_time Current timestamp
   * @return Number of entries deleted
   */
  std::size_t cleanup_expired(std::uint64_t current_time);

  /**
   * @brief Get sidechain stats
   */
  struct Stats {
    std::size_t total_entries = 0;
    std::uint64_t total_data_size = 0;
    std::uint32_t entries_with_proofs = 0;
    double average_confidence = 0.0;
  };

  Stats get_stats() const;

  /**
   * @brief Get entry metadata
   * @param tx_id The transaction ID
   * @return Entry if found
   */
  std::optional<SidechainEntry> get_entry(
      const std::vector<std::uint8_t>& tx_id) const;

 private:
  /**
   * @brief Generate random sampling challenge
   */
  struct Challenge {
    std::uint64_t start_byte = 0;
    std::uint64_t end_byte = 0;
  };

  Challenge generate_challenge(const std::vector<std::uint8_t>& data) const;

  /**
   * @brief Verify response to challenge
   */
  bool verify_challenge_response(
      const std::vector<std::uint8_t>& data,
      const Challenge& challenge,
      const std::vector<std::uint8_t>& response_hash) const;

  /**
   * @brief State
   */
  Config config_;
  std::map<std::vector<std::uint8_t>, SidechainEntry> entries_;
};

}  // namespace qv::da
