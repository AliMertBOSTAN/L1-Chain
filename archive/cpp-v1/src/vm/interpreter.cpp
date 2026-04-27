#include "qv/vm/interpreter.hpp"
#include <algorithm>
#include <cstring>

namespace qv::vm {

Interpreter::Interpreter(const Config& config) : config_(config) {}

ExecutionResult Interpreter::execute(const Script& unlocking_script,
                                      const Script& locking_script,
                                      const TransactionContext& context) {
  // TODO: Implement dual-script execution
  // 1. Execute unlocking script
  // 2. Copy stack state
  // 3. Execute locking script with saved state
  // 4. Verify final stack contains exactly one truthy value

  ExecutionResult result;
  result.success = false;
  result.error_message = "execute(unlocking, locking) not yet implemented";

  return result;
}

ExecutionResult Interpreter::execute_single(const Script& script,
                                             const TransactionContext& context) {
  // TODO: Implement single script execution
  // 1. Parse script bytecode
  // 2. Execute opcodes one by one
  // 3. Check stack constraints
  // 4. Return result with gas used

  ExecutionResult result;
  result.success = false;
  result.error_message = "execute_single() not yet implemented";
  result.gas_used = 0;

  return result;
}

void Interpreter::reset() {
  stack_.clear();
  gas_used_ = 0;
  error_message_.clear();
}

// ============ Stack Operations ============

void Interpreter::push_stack(const std::vector<std::uint8_t>& value) {
  // TODO: Implement with stack size check
  if (stack_.size() >= config_.max_stack_size) {
    error_message_ = "Stack overflow";
    return;
  }
  stack_.push_back(value);
}

std::vector<std::uint8_t> Interpreter::pop_stack() {
  // TODO: Implement with error handling
  if (stack_.empty()) {
    error_message_ = "Stack underflow";
    return {};
  }
  auto value = stack_.back();
  stack_.pop_back();
  return value;
}

const std::vector<std::uint8_t>& Interpreter::peek_stack(std::size_t depth) const {
  // TODO: Implement with bounds checking
  static const std::vector<std::uint8_t> empty;
  if (depth >= stack_.size()) {
    return empty;
  }
  return stack_[stack_.size() - 1 - depth];
}

// ============ Opcode Handlers ============

bool Interpreter::op_push(const std::vector<std::uint8_t>& data) {
  // TODO: Implement OP_PUSH
  push_stack(data);
  gas_used_ += data.size();
  return error_message_.empty();
}

bool Interpreter::op_dup() {
  // TODO: Implement OP_DUP
  if (stack_.empty()) {
    error_message_ = "Stack underflow in OP_DUP";
    return false;
  }
  push_stack(peek_stack(0));
  gas_used_ += 1;
  return true;
}

bool Interpreter::op_drop() {
  // TODO: Implement OP_DROP
  if (stack_.empty()) {
    error_message_ = "Stack underflow in OP_DROP";
    return false;
  }
  pop_stack();
  gas_used_ += 1;
  return true;
}

bool Interpreter::op_swap() {
  // TODO: Implement OP_SWAP
  if (stack_.size() < 2) {
    error_message_ = "Stack underflow in OP_SWAP";
    return false;
  }
  std::swap(stack_[stack_.size() - 1], stack_[stack_.size() - 2]);
  gas_used_ += 1;
  return true;
}

bool Interpreter::op_hash() {
  // TODO: Implement OP_HASH (SHA-256)
  if (stack_.empty()) {
    error_message_ = "Stack underflow in OP_HASH";
    return false;
  }
  auto value = pop_stack();
  auto hash = sha256(value);
  push_stack(hash);
  gas_used_ += 50;  // Arbitrary gas cost for hashing
  return true;
}

bool Interpreter::op_hash160() {
  // TODO: Implement OP_HASH160 (RIPEMD-160(SHA-256))
  if (stack_.empty()) {
    error_message_ = "Stack underflow in OP_HASH160";
    return false;
  }
  auto value = pop_stack();
  auto hash = hash160(value);
  push_stack(hash);
  gas_used_ += 50;
  return true;
}

bool Interpreter::op_equal() {
  // TODO: Implement OP_EQUAL
  if (stack_.size() < 2) {
    error_message_ = "Stack underflow in OP_EQUAL";
    return false;
  }
  auto b = pop_stack();
  auto a = pop_stack();
  bool equal = (a == b);
  push_stack(equal ? std::vector<std::uint8_t>{1} : std::vector<std::uint8_t>{0});
  gas_used_ += 1;
  return true;
}

bool Interpreter::op_equal_verify() {
  // TODO: Implement OP_EQUALVERIFY
  if (!op_equal()) return false;
  if (stack_.empty() || !is_true(peek_stack(0))) {
    error_message_ = "OP_EQUALVERIFY failed";
    return false;
  }
  pop_stack();
  return true;
}

bool Interpreter::op_verify_sig(const TransactionContext& context) {
  // TODO: Implement OP_VERIFY_SIG (ECDSA)
  if (stack_.size() < 3) {
    error_message_ = "Stack underflow in OP_VERIFY_SIG";
    return false;
  }
  auto pubkey = pop_stack();
  auto sig = pop_stack();
  auto msg = pop_stack();

  bool valid = verify_signature(msg, sig, pubkey, context);
  if (!valid) {
    error_message_ = "Signature verification failed";
    return false;
  }
  gas_used_ += 200;
  return true;
}

bool Interpreter::op_checksig_pqc(const TransactionContext& context) {
  // TODO: Implement OP_CHECKSIG_PQC (post-quantum)
  if (stack_.size() < 3) {
    error_message_ = "Stack underflow in OP_CHECKSIG_PQC";
    return false;
  }
  auto pubkey = pop_stack();
  auto sig = pop_stack();
  auto msg = pop_stack();

  bool valid = verify_pqc_signature(msg, sig, pubkey);
  push_stack(valid ? std::vector<std::uint8_t>{1} : std::vector<std::uint8_t>{0});
  gas_used_ += 500;  // Higher cost for PQC
  return true;
}

bool Interpreter::op_checkmultisig_pqc(const TransactionContext& context) {
  // TODO: Implement OP_CHECKMULTISIG_PQC
  error_message_ = "OP_CHECKMULTISIG_PQC not yet implemented";
  return false;
}

bool Interpreter::op_checklocktimeverify(const TransactionContext& context) {
  // TODO: Implement OP_CHECKLOCKTIMEVERIFY (CSV equivalent)
  if (stack_.empty()) {
    error_message_ = "Stack underflow in OP_CHECKLOCKTIMEVERIFY";
    return false;
  }
  auto locktime_bytes = peek_stack(0);
  // TODO: Parse and check locktime against context.block_height
  return true;
}

bool Interpreter::op_if(Script::Bytes& remaining_script, std::size_t& pc) {
  // TODO: Implement OP_IF with script state
  return true;
}

bool Interpreter::op_else(Script::Bytes& remaining_script, std::size_t& pc) {
  // TODO: Implement OP_ELSE
  return true;
}

bool Interpreter::op_endif(Script::Bytes& remaining_script, std::size_t& pc) {
  // TODO: Implement OP_ENDIF
  return true;
}

bool Interpreter::op_return() {
  // TODO: Implement OP_RETURN (always fails)
  error_message_ = "OP_RETURN executed";
  return false;
}

bool Interpreter::op_add() {
  // TODO: Implement OP_ADD
  if (stack_.size() < 2) {
    error_message_ = "Stack underflow in OP_ADD";
    return false;
  }
  auto b = bytes_to_int(pop_stack());
  auto a = bytes_to_int(pop_stack());
  push_stack(int_to_bytes(a + b));
  gas_used_ += 1;
  return true;
}

bool Interpreter::op_sub() {
  // TODO: Implement OP_SUB
  if (stack_.size() < 2) {
    error_message_ = "Stack underflow in OP_SUB";
    return false;
  }
  auto b = bytes_to_int(pop_stack());
  auto a = bytes_to_int(pop_stack());
  push_stack(int_to_bytes(a - b));
  gas_used_ += 1;
  return true;
}

// ============ Crypto Helpers ============

std::vector<std::uint8_t> Interpreter::sha256(
    const std::vector<std::uint8_t>& data) {
  // TODO: Implement SHA-256 using cryptographic library
  // Placeholder: return 32 zero bytes
  return std::vector<std::uint8_t>(32, 0);
}

std::vector<std::uint8_t> Interpreter::hash160(
    const std::vector<std::uint8_t>& data) {
  // TODO: Implement RIPEMD-160(SHA-256)
  // Placeholder: return 20 zero bytes
  return std::vector<std::uint8_t>(20, 0);
}

bool Interpreter::verify_signature(const std::vector<std::uint8_t>& message,
                                    const std::vector<std::uint8_t>& signature,
                                    const std::vector<std::uint8_t>& pubkey,
                                    const TransactionContext& context) {
  // TODO: Implement ECDSA signature verification
  return false;  // Placeholder
}

bool Interpreter::verify_pqc_signature(
    const std::vector<std::uint8_t>& message,
    const std::vector<std::uint8_t>& signature,
    const std::vector<std::uint8_t>& pubkey) {
  // TODO: Implement post-quantum signature verification (Dilithium/CRYSTALS)
  return false;  // Placeholder
}

// ============ Utility ============

bool Interpreter::is_true(const std::vector<std::uint8_t>& value) const {
  // TODO: Implement Bitcoin-style truthiness
  // Empty = false, [0x00, ...] = false, otherwise true
  if (value.empty()) return false;
  for (auto byte : value) {
    if (byte != 0) return true;
  }
  return false;
}

std::int64_t Interpreter::bytes_to_int(
    const std::vector<std::uint8_t>& data) const {
  // TODO: Implement little-endian to int conversion
  std::int64_t result = 0;
  for (std::size_t i = 0; i < data.size() && i < 8; ++i) {
    result |= (static_cast<std::int64_t>(data[i]) << (i * 8));
  }
  return result;
}

std::vector<std::uint8_t> Interpreter::int_to_bytes(std::int64_t value) const {
  // TODO: Implement int to little-endian conversion
  std::vector<std::uint8_t> result;
  while (value != 0) {
    result.push_back(static_cast<std::uint8_t>(value & 0xFF));
    value >>= 8;
  }
  if (result.empty()) result.push_back(0);
  return result;
}

}  // namespace qv::vm
