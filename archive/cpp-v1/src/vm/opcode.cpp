#include "qv/vm/opcode.hpp"
#include <unordered_map>

namespace qv::vm {

// Static metadata registry
static const std::unordered_map<std::uint8_t, OpcodeMetadata> OPCODE_REGISTRY = {
    {static_cast<std::uint8_t>(Opcode::OP_PUSH),
     {Opcode::OP_PUSH, "OP_PUSH", -1, "Push data onto stack"}},
    {static_cast<std::uint8_t>(Opcode::OP_DUP),
     {Opcode::OP_DUP, "OP_DUP", 1, "Duplicate top stack item"}},
    {static_cast<std::uint8_t>(Opcode::OP_DROP),
     {Opcode::OP_DROP, "OP_DROP", 1, "Remove top stack item"}},
    {static_cast<std::uint8_t>(Opcode::OP_HASH),
     {Opcode::OP_HASH, "OP_HASH", 1, "SHA-256 hash top item"}},
    {static_cast<std::uint8_t>(Opcode::OP_EQUAL),
     {Opcode::OP_EQUAL, "OP_EQUAL", 2, "Check equality of top 2 items"}},
    {static_cast<std::uint8_t>(Opcode::OP_VERIFY_SIG),
     {Opcode::OP_VERIFY_SIG, "OP_VERIFY_SIG", 3,
      "Verify ECDSA signature"}},
    {static_cast<std::uint8_t>(Opcode::OP_CHECKSIG_PQC),
     {Opcode::OP_CHECKSIG_PQC, "OP_CHECKSIG_PQC", 3,
      "Verify post-quantum signature"}},
    {static_cast<std::uint8_t>(Opcode::OP_CHECKLOCKTIMEVERIFY),
     {Opcode::OP_CHECKLOCKTIMEVERIFY, "OP_CHECKLOCKTIMEVERIFY", 1,
      "Check locktime"}},
    {static_cast<std::uint8_t>(Opcode::OP_IF),
     {Opcode::OP_IF, "OP_IF", 1, "Start conditional block"}},
    {static_cast<std::uint8_t>(Opcode::OP_ELSE),
     {Opcode::OP_ELSE, "OP_ELSE", 0, "Else branch"}},
    {static_cast<std::uint8_t>(Opcode::OP_ENDIF),
     {Opcode::OP_ENDIF, "OP_ENDIF", 0, "End conditional"}},
    {static_cast<std::uint8_t>(Opcode::OP_RETURN),
     {Opcode::OP_RETURN, "OP_RETURN", 0, "Return (fail script)"}},
};

OpcodeMetadata OpcodeRegistry::get_metadata(Opcode op) {
  // TODO: Implement full registry lookup
  auto it = OPCODE_REGISTRY.find(static_cast<std::uint8_t>(op));
  if (it != OPCODE_REGISTRY.end()) {
    return it->second;
  }
  return {op, "OP_UNKNOWN", -1, "Unknown opcode"};
}

std::string OpcodeRegistry::get_name(Opcode op) {
  return get_metadata(op).name;
}

std::optional<Opcode> OpcodeRegistry::parse(const std::string& name) {
  // TODO: Implement string parsing
  if (name == "OP_PUSH") return Opcode::OP_PUSH;
  if (name == "OP_DUP") return Opcode::OP_DUP;
  if (name == "OP_DROP") return Opcode::OP_DROP;
  if (name == "OP_HASH") return Opcode::OP_HASH;
  if (name == "OP_EQUAL") return Opcode::OP_EQUAL;
  if (name == "OP_CHECKSIG_PQC") return Opcode::OP_CHECKSIG_PQC;
  if (name == "OP_IF") return Opcode::OP_IF;
  if (name == "OP_ELSE") return Opcode::OP_ELSE;
  if (name == "OP_ENDIF") return Opcode::OP_ENDIF;
  if (name == "OP_RETURN") return Opcode::OP_RETURN;
  // TODO: Add remaining opcodes
  return std::nullopt;
}

bool OpcodeRegistry::is_valid(std::uint8_t op) {
  // TODO: Implement full validation
  return OPCODE_REGISTRY.find(op) != OPCODE_REGISTRY.end() || op < 0x100;
}

}  // namespace qv::vm
