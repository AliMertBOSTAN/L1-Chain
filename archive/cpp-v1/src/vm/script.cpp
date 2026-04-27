#include "qv/vm/script.hpp"
#include <algorithm>

namespace qv::vm {

// ============ Script Implementation ============

Script::Script(const Bytes& bytecode) : bytecode_(bytecode) {}

Script::Script(Bytes&& bytecode) noexcept : bytecode_(std::move(bytecode)) {}

Script::Bytes Script::serialize() const {
  // TODO: Implement serialization with length prefix
  Bytes result;
  // For now, just return the bytecode as-is
  return bytecode_;
}

std::optional<Script> Script::deserialize(const Bytes& data) {
  // TODO: Implement deserialization
  // Should parse length prefix and extract bytecode
  return Script(data);
}

bool Script::operator==(const Script& other) const {
  return bytecode_ == other.bytecode_;
}

// ============ ScriptBuilder Implementation ============

ScriptBuilder& ScriptBuilder::op(Opcode opcode) {
  bytecode_.push_back(static_cast<std::uint8_t>(opcode));
  return *this;
}

ScriptBuilder& ScriptBuilder::push_bytes(const Script::Bytes& data) {
  // TODO: Implement length-prefixed push
  // Format: <length> <data>
  if (data.size() < 0x4B) {
    bytecode_.push_back(static_cast<std::uint8_t>(data.size()));
  } else if (data.size() < 0x100) {
    bytecode_.push_back(0x4C);  // OP_PUSHDATA1
    bytecode_.push_back(static_cast<std::uint8_t>(data.size()));
  } else if (data.size() < 0x10000) {
    bytecode_.push_back(0x4D);  // OP_PUSHDATA2
    bytecode_.push_back(static_cast<std::uint8_t>(data.size() & 0xFF));
    bytecode_.push_back(static_cast<std::uint8_t>((data.size() >> 8) & 0xFF));
  }
  bytecode_.insert(bytecode_.end(), data.begin(), data.end());
  return *this;
}

ScriptBuilder& ScriptBuilder::push_byte(std::uint8_t byte) {
  bytecode_.push_back(byte);
  return *this;
}

ScriptBuilder& ScriptBuilder::push_int(std::int64_t value) {
  // TODO: Implement CScript-style int encoding
  Script::Bytes bytes;
  while (value > 0) {
    bytes.push_back(static_cast<std::uint8_t>(value & 0xFF));
    value >>= 8;
  }
  if (bytes.empty()) bytes.push_back(0);
  return push_bytes(bytes);
}

ScriptBuilder& ScriptBuilder::push_pubkey(const Script::Bytes& pubkey) {
  return push_bytes(pubkey);
}

ScriptBuilder& ScriptBuilder::push_signature(const Script::Bytes& sig) {
  return push_bytes(sig);
}

Script ScriptBuilder::build() {
  Script script(bytecode_);
  bytecode_.clear();
  return script;
}

// ============ Standard Script Templates ============

Script scripts::p2pkh_pqc(const Script::Bytes& pubkey_hash) {
  // OP_DUP OP_HASH160 <hash> OP_EQUAL OP_VERIFY_SIG
  // TODO: Implement p2pkh template
  return ScriptBuilder()
      .op(Opcode::OP_DUP)
      .op(Opcode::OP_HASH)
      .push_bytes(pubkey_hash)
      .op(Opcode::OP_EQUAL)
      .op(Opcode::OP_VERIFY_SIG)
      .build();
}

Script scripts::p2sh_pqc(const Script::Bytes& script_hash) {
  // OP_HASH160 <hash> OP_EQUAL
  // TODO: Implement p2sh template
  return ScriptBuilder()
      .op(Opcode::OP_HASH)
      .push_bytes(script_hash)
      .op(Opcode::OP_EQUAL)
      .build();
}

Script scripts::multisig_pqc(std::uint8_t m, std::uint8_t n,
                              const std::vector<Script::Bytes>& pubkeys) {
  // <m> <pubkey1> ... <pubkeyn> <n> OP_CHECKMULTISIG_PQC
  // TODO: Implement multisig template
  ScriptBuilder builder;
  builder.push_byte(m);
  for (const auto& pubkey : pubkeys) {
    builder.push_bytes(pubkey);
  }
  builder.push_byte(n);
  builder.op(Opcode::OP_CHECKMULTISIG_PQC);
  return builder.build();
}

Script scripts::timelocked(std::uint32_t locktime, const Script& inner_script) {
  // <locktime> OP_CHECKLOCKTIMEVERIFY OP_DROP <inner_script>
  // TODO: Implement timelock template
  ScriptBuilder builder;
  // Push locktime (little-endian)
  std::vector<std::uint8_t> locktime_bytes;
  locktime_bytes.push_back(static_cast<std::uint8_t>(locktime & 0xFF));
  locktime_bytes.push_back(static_cast<std::uint8_t>((locktime >> 8) & 0xFF));
  locktime_bytes.push_back(static_cast<std::uint8_t>((locktime >> 16) & 0xFF));
  locktime_bytes.push_back(static_cast<std::uint8_t>((locktime >> 24) & 0xFF));
  builder.push_bytes(locktime_bytes);
  builder.op(Opcode::OP_CHECKLOCKTIMEVERIFY);
  builder.op(Opcode::OP_DROP);
  const auto& inner_bytes = inner_script.bytecode();
  builder.push_bytes(inner_bytes);
  return builder.build();
}

Script scripts::stealth_address(const Script::Bytes& ephemeral_pubkey,
                                const Script::Bytes& address_hash) {
  // <ephemeral_pubkey> OP_STEALTH_VERIFY <address_hash> OP_EQUAL
  // TODO: Implement stealth script
  return ScriptBuilder()
      .push_bytes(ephemeral_pubkey)
      .op(Opcode::OP_STEALTH_VERIFY)
      .push_bytes(address_hash)
      .op(Opcode::OP_EQUAL)
      .build();
}

}  // namespace qv::vm
