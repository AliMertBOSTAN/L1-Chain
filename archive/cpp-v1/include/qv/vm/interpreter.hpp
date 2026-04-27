#pragma once

#include <vector>
#include <cstdint>
#include <memory>
#include <optional>
#include <stdexcept>
#include "script.hpp"

namespace qv::vm {

/**
 * @brief Transaction context for script execution
 *
 * Provides immutable context needed to validate locking/unlocking scripts.
 * This is the ONLY external state the interpreter receives.
 */
struct TransactionContext {
  /**
   * @brief Transaction ID (32 bytes, SHA-256)
   */
  std::vector<std::uint8_t> tx_id;

  /**
   * @brief Input index being spent
   */
  std::uint32_t input_index = 0;

  /**
   * @brief Output index of the UTXO being spent
   */
  std::uint32_t output_index = 0;

  /**
   * @brief The UTXO's locking script
   */
  Script utxo_script;

  /**
   * @brief Amount of the UTXO in satoshis
   */
  std::uint64_t utxo_amount = 0;

  /**
   * @brief Current block height
   */
  std::uint32_t block_height = 0;

  /**
   * @brief Median time past (for CLTV)
   */
  std::uint32_t median_time_past = 0;

  /**
   * @brief Sequence number of the input
   */
  std::uint32_t sequence = 0;
};

/**
 * @brief Execution result
 */
struct ExecutionResult {
  bool success = false;
  std::string error_message;
  std::uint64_t gas_used = 0;
};

/**
 * @brief Stack-based DSL interpreter
 *
 * Executes UTXO locking/unlocking scripts in a stack-based virtual machine.
 * No network access, no state mutation — pure function.
 *
 * Execution model:
 * 1. Load unlocking script onto stack
 * 2. Execute unlocking script
 * 3. Load locking script onto stack
 * 4. Execute locking script
 * 5. Stack must contain exactly one truthy value
 */
class Interpreter {
 public:
  /**
   * @brief Execution limits
   */
  struct Config {
    std::uint64_t max_stack_size = 1000;
    std::uint64_t max_script_size = 10240;  // 10 KB
    std::uint64_t max_gas = 10000000;       // 10M gas
    bool enable_strict_flags = true;        // Strict validation
  };

  /**
   * @brief Construct interpreter with configuration
   * @param config Execution limits and flags
   */
  explicit Interpreter(const Config& config = Config{});

  /**
   * @brief Execute locking + unlocking scripts
   *
   * @param unlocking_script The spending proof
   * @param locking_script The UTXO constraint
   * @param context Transaction context (immutable)
   * @return ExecutionResult with success flag and gas used
   */
  ExecutionResult execute(const Script& unlocking_script,
                          const Script& locking_script,
                          const TransactionContext& context);

  /**
   * @brief Execute a single script
   *
   * Used for script validation without dual-script semantics.
   *
   * @param script The script to execute
   * @param context Transaction context
   * @return ExecutionResult
   */
  ExecutionResult execute_single(const Script& script,
                                 const TransactionContext& context);

  /**
   * @brief Get gas used from last execution
   */
  std::uint64_t last_gas_used() const { return last_gas_used_; }

  /**
   * @brief Reset interpreter state
   */
  void reset();

 private:
  /**
   * @brief Internal stack representation
   */
  using Stack = std::vector<std::vector<std::uint8_t>>;

  /**
   * @brief Execute opcode
   * @return true if execution should continue
   */
  bool execute_opcode(Opcode op, const TransactionContext& context);

  /**
   * @brief Stack manipulation helpers
   */
  void push_stack(const std::vector<std::uint8_t>& value);
  std::vector<std::uint8_t> pop_stack();
  const std::vector<std::uint8_t>& peek_stack(std::size_t depth = 0) const;
  bool stack_empty() const { return stack_.empty(); }

  /**
   * @brief Opcode handlers
   */
  bool op_push(const std::vector<std::uint8_t>& data);
  bool op_dup();
  bool op_drop();
  bool op_swap();
  bool op_hash();
  bool op_hash160();
  bool op_equal();
  bool op_verify_sig(const TransactionContext& context);
  bool op_checksig_pqc(const TransactionContext& context);
  bool op_checkmultisig_pqc(const TransactionContext& context);
  bool op_checklocktimeverify(const TransactionContext& context);
  bool op_if(Script::Bytes& remaining_script, std::size_t& pc);
  bool op_else(Script::Bytes& remaining_script, std::size_t& pc);
  bool op_endif(Script::Bytes& remaining_script, std::size_t& pc);
  bool op_return();
  bool op_add();
  bool op_sub();
  bool op_equal_verify();

  /**
   * @brief Crypto helpers
   */
  std::vector<std::uint8_t> sha256(const std::vector<std::uint8_t>& data);
  std::vector<std::uint8_t> hash160(const std::vector<std::uint8_t>& data);
  bool verify_signature(const std::vector<std::uint8_t>& message,
                       const std::vector<std::uint8_t>& signature,
                       const std::vector<std::uint8_t>& pubkey,
                       const TransactionContext& context);
  bool verify_pqc_signature(const std::vector<std::uint8_t>& message,
                            const std::vector<std::uint8_t>& signature,
                            const std::vector<std::uint8_t>& pubkey);

  /**
   * @brief Utility
   */
  bool is_true(const std::vector<std::uint8_t>& value) const;
  std::int64_t bytes_to_int(const std::vector<std::uint8_t>& data) const;
  std::vector<std::uint8_t> int_to_bytes(std::int64_t value) const;

  /**
   * @brief State
   */
  Stack stack_;
  Config config_;
  std::uint64_t gas_used_ = 0;
  std::uint64_t last_gas_used_ = 0;
  std::string error_message_;
};

}  // namespace qv::vm
