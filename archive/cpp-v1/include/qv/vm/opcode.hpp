#pragma once

#include <cstdint>
#include <string>
#include <unordered_map>
#include <vector>

namespace qv::vm {

/**
 * @brief Opcode enum for the QuantumVault DSL interpreter
 *
 * The DSL is a stack-based, Forth-like language for UTXO locking/unlocking scripts.
 * It runs CLIENT-SIDE only; the L1 only validates spend proofs.
 *
 * Opcodes are organized by category:
 * - Stack operations: OP_PUSH, OP_DUP, OP_DROP
 * - Cryptography: OP_HASH, OP_VERIFY_SIG, OP_CHECKSIG_PQC
 * - Post-quantum: OP_CHECKSIG_PQC, OP_CHECKMULTISIG_PQC
 * - Time-locks: OP_CHECKLOCKTIMEVERIFY
 * - Control flow: OP_IF, OP_ELSE, OP_ENDIF, OP_RETURN
 * - Logic: OP_EQUAL, OP_CHECKMULTISIG_PQC
 */
enum class Opcode : std::uint8_t {
  // Stack operations
  OP_PUSH = 0x00,
  OP_DUP = 0x01,
  OP_DROP = 0x02,
  OP_SWAP = 0x03,
  OP_ROT = 0x04,
  OP_OVER = 0x05,

  // Cryptography
  OP_HASH = 0x10,           // SHA-256
  OP_HASH160 = 0x11,        // RIPEMD-160(SHA-256)
  OP_VERIFY_SIG = 0x12,     // ECDSA signature verification
  OP_CHECKSIG = 0x13,       // Classic CHECKSIG
  OP_CHECKSIG_PQC = 0x14,   // Post-quantum signature (Dilithium/CRYSTALS)

  // Arithmetic & Logic
  OP_ADD = 0x20,
  OP_SUB = 0x21,
  OP_MUL = 0x22,
  OP_DIV = 0x23,
  OP_MOD = 0x24,
  OP_EQUAL = 0x25,
  OP_EQUALVERIFY = 0x26,

  // Comparison
  OP_LT = 0x30,
  OP_LE = 0x31,
  OP_GT = 0x32,
  OP_GE = 0x33,

  // Bitwise
  OP_AND = 0x40,
  OP_OR = 0x41,
  OP_XOR = 0x42,
  OP_NOT = 0x43,

  // Time-locks
  OP_CHECKLOCKTIMEVERIFY = 0x50,  // CSV equivalent
  OP_CHECKSEQUENCEVERIFY = 0x51,

  // Control flow
  OP_IF = 0x60,
  OP_NOTIF = 0x61,
  OP_ELSE = 0x62,
  OP_ENDIF = 0x63,
  OP_VERIFY = 0x64,
  OP_RETURN = 0x65,

  // Multi-sig
  OP_CHECKMULTISIG_PQC = 0x70,      // Post-quantum multisig

  // Advanced
  OP_MERKLE_VERIFY = 0x80,          // Merkle proof verification
  OP_STEALTH_VERIFY = 0x81,         // Stealth address verification
  OP_THRESHOLD_SIG = 0x82,          // Threshold signature scheme

  // Utility
  OP_RESERVED = 0xFF,
};

/**
 * @brief Metadata for an opcode
 */
struct OpcodeMetadata {
  Opcode opcode;
  std::string name;
  int arity;              // Number of stack arguments consumed (-1 for variable)
  std::string description;
};

/**
 * @brief Opcode registry and utilities
 */
class OpcodeRegistry {
 public:
  /**
   * @brief Get metadata for an opcode
   * @param op The opcode
   * @return OpcodeMetadata for the given opcode
   */
  static OpcodeMetadata get_metadata(Opcode op);

  /**
   * @brief Get the name of an opcode
   * @param op The opcode
   * @return String name (e.g., "OP_DUP")
   */
  static std::string get_name(Opcode op);

  /**
   * @brief Parse opcode from string
   * @param name The opcode name (e.g., "OP_DUP")
   * @return The corresponding Opcode, or std::nullopt if not found
   */
  static std::optional<Opcode> parse(const std::string& name);

  /**
   * @brief Check if opcode is valid
   * @param op The opcode byte
   * @return true if valid opcode
   */
  static bool is_valid(std::uint8_t op);
};

}  // namespace qv::vm
