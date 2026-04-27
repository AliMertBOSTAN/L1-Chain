#pragma once

#include <vector>
#include <cstdint>
#include <memory>
#include <optional>
#include "opcode.hpp"

namespace qv::vm {

/**
 * @brief Bytecode container for DSL scripts
 *
 * Scripts are immutable after construction. They represent either:
 * - Locking scripts (constraints that must be satisfied to spend a UTXO)
 * - Unlocking scripts (proofs that satisfy the locking script)
 *
 * Serialization format: variable-length bytecode with embedded push data.
 */
class Script {
 public:
  using Bytes = std::vector<std::uint8_t>;

  /**
   * @brief Default constructor (empty script)
   */
  Script() = default;

  /**
   * @brief Construct from bytecode
   * @param bytecode The raw script bytecode
   */
  explicit Script(const Bytes& bytecode);

  /**
   * @brief Construct from bytecode (move)
   */
  explicit Script(Bytes&& bytecode) noexcept;

  /**
   * @brief Get raw bytecode
   */
  const Bytes& bytecode() const { return bytecode_; }

  /**
   * @brief Get script size in bytes
   */
  std::size_t size() const { return bytecode_.size(); }

  /**
   * @brief Check if script is empty
   */
  bool empty() const { return bytecode_.empty(); }

  /**
   * @brief Serialize script to bytes (for transmission/storage)
   * @return Serialized script
   */
  Bytes serialize() const;

  /**
   * @brief Deserialize script from bytes
   * @param data Serialized script data
   * @return Result<Script> or error
   */
  static std::optional<Script> deserialize(const Bytes& data);

  /**
   * @brief Compare scripts for equality
   */
  bool operator==(const Script& other) const;
  bool operator!=(const Script& other) const { return !(*this == other); }

 private:
  Bytes bytecode_;
};

/**
 * @brief Fluent API for constructing scripts
 *
 * Example:
 *   Script script = ScriptBuilder()
 *     .push_bytes({0xaa, 0xbb})
 *     .op(Opcode::OP_HASH)
 *     .op(Opcode::OP_EQUAL)
 *     .build();
 */
class ScriptBuilder {
 public:
  /**
   * @brief Push an opcode
   */
  ScriptBuilder& op(Opcode opcode);

  /**
   * @brief Push raw bytes with length prefix
   * @param data The data to push
   */
  ScriptBuilder& push_bytes(const Script::Bytes& data);

  /**
   * @brief Push a single byte
   */
  ScriptBuilder& push_byte(std::uint8_t byte);

  /**
   * @brief Push an integer (little-endian)
   */
  ScriptBuilder& push_int(std::int64_t value);

  /**
   * @brief Push a public key
   * @param pubkey The public key bytes
   */
  ScriptBuilder& push_pubkey(const Script::Bytes& pubkey);

  /**
   * @brief Push a signature
   * @param sig The signature bytes
   */
  ScriptBuilder& push_signature(const Script::Bytes& sig);

  /**
   * @brief Build the final script
   */
  Script build();

  /**
   * @brief Get current bytecode without finalizing
   */
  Script::Bytes current_bytes() const { return bytecode_; }

 private:
  Script::Bytes bytecode_;
};

/**
 * @brief Standard script templates
 */
namespace scripts {

/**
 * @brief Pay-to-Public-Key-Hash (post-quantum variant)
 * @param pubkey_hash The 160-bit hash of the public key
 * @return Locking script: OP_DUP OP_HASH160 <hash> OP_EQUAL OP_VERIFY_SIG
 */
Script p2pkh_pqc(const Script::Bytes& pubkey_hash);

/**
 * @brief Pay-to-Script-Hash (post-quantum variant)
 * @param script_hash The 160-bit hash of the redeem script
 * @return Locking script: OP_HASH160 <hash> OP_EQUAL
 */
Script p2sh_pqc(const Script::Bytes& script_hash);

/**
 * @brief M-of-N Multisig (post-quantum variant)
 * @param m Threshold (signatures required)
 * @param n Total number of keys
 * @param pubkeys The public keys
 * @return Locking script: <m> <pubkey1> ... <pubkeyn> <n> OP_CHECKMULTISIG_PQC
 */
Script multisig_pqc(std::uint8_t m, std::uint8_t n,
                    const std::vector<Script::Bytes>& pubkeys);

/**
 * @brief Time-locked UTXO script
 * @param locktime Block height or timestamp
 * @param inner_script The unlocking condition
 * @return Locking script with OP_CHECKLOCKTIMEVERIFY
 */
Script timelocked(std::uint32_t locktime, const Script& inner_script);

/**
 * @brief Stealth address script (privacy-preserving)
 * @param ephemeral_pubkey Public key for DH exchange
 * @param address_hash Hashed stealth address
 * @return Locking script for stealth transactions
 */
Script stealth_address(const Script::Bytes& ephemeral_pubkey,
                       const Script::Bytes& address_hash);

}  // namespace scripts

}  // namespace qv::vm
