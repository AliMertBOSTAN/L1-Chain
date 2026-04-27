#pragma once

#include "types.hpp"
#include "result.hpp"
#include <vector>
#include <memory>

namespace qv::crypto {
/// Forward declaration of Signature type from crypto module
struct Signature;
} // namespace qv::crypto

namespace qv::core {

/// TxInput: Represents an input to a transaction (spends a previous output)
struct TxInput {
    /// Reference to the UTXO being spent
    OutPoint prev_output;

    /// Signature proving authorization to spend the previous output
    /// In a real implementation, this would be from qv::crypto
    bytes signature;

    /// Witness data for script validation (stack-based proof)
    bytes witness_data;

    /// Default constructor
    TxInput() = default;

    /// Constructor with values
    TxInput(const OutPoint& prev, const bytes& sig, const bytes& witness)
        : prev_output(prev), signature(sig), witness_data(witness) {}

    /// Check if this input is valid (non-empty signature and valid outpoint)
    [[nodiscard]] bool is_valid() const noexcept {
        return !signature.empty();
    }
};

/// TxOutput: Represents an output of a transaction (creates a new UTXO)
struct TxOutput {
    /// Amount of value in this output (in satoshi units)
    Amount value;

    /// Locking script that defines spending conditions
    /// Could be a pubkey hash, script code, or other conditions
    bytes locking_script;

    /// Optional stealth address data for enhanced privacy
    /// Used for privacy-enhanced transactions (empty if not applicable)
    bytes stealth_address_data;

    /// Default constructor
    TxOutput() = default;

    /// Constructor with values
    TxOutput(Amount val, const bytes& lock_script, const bytes& stealth = {})
        : value(val), locking_script(lock_script), stealth_address_data(stealth) {}

    /// Check if this output is valid (positive value, non-empty script)
    [[nodiscard]] bool is_valid() const noexcept {
        return value > 0 && !locking_script.empty();
    }
};

/// Transaction: Core transaction structure for QuantumVault
class Transaction {
public:
    /// Transaction version (for forward compatibility)
    std::uint32_t version = 1;

    /// Inputs (UTXOs being spent)
    std::vector<TxInput> inputs;

    /// Outputs (new UTXOs being created)
    std::vector<TxOutput> outputs;

    /// Lock time (absolute block height or timestamp lock)
    /// 0 = no lock, >0 = block height or timestamp depending on value
    std::uint64_t lock_time = 0;

    /// Default constructor
    Transaction() = default;

    /// Constructor with all fields
    Transaction(
        std::uint32_t ver,
        std::vector<TxInput> ins,
        std::vector<TxOutput> outs,
        std::uint64_t lock = 0
    ) : version(ver), inputs(std::move(ins)), outputs(std::move(outs)), lock_time(lock) {}

    /// Check if this transaction is valid
    /// Validates structure constraints (non-empty inputs/outputs, valid ranges)
    [[nodiscard]] bool is_valid() const noexcept;

    /// Check if all inputs are valid
    [[nodiscard]] bool inputs_valid() const noexcept;

    /// Check if all outputs are valid
    [[nodiscard]] bool outputs_valid() const noexcept;

    /// Get total input value (sum of values from previous outputs)
    /// Note: Requires looking up the UTXOs from the input outpoints
    /// This is a placeholder and would need UTXO set context
    [[nodiscard]] Amount total_input_value() const noexcept {
        return 0; // Would require UTXO lookup
    }

    /// Get total output value
    [[nodiscard]] Amount total_output_value() const noexcept;

    /// Compute the transaction ID (SHA256 double hash of serialized transaction)
    [[nodiscard]] Result<TxId, std::string> compute_txid() const;

    /// Serialize transaction to bytes
    /// Format: version || input_count || inputs || output_count || outputs || lock_time
    [[nodiscard]] Result<bytes, std::string> to_bytes() const;

    /// Deserialize transaction from bytes
    /// Returns error if deserialization fails
    static Result<Transaction, std::string> from_bytes(const bytes& data);

    /// Get transaction size in bytes
    [[nodiscard]] std::size_t serialized_size() const noexcept;

    /// Check if this transaction is a coinbase transaction (creates new coins)
    /// Coinbase transactions have exactly one input with a null outpoint
    [[nodiscard]] bool is_coinbase() const noexcept;

    /// Verify the lock time constraints are satisfied
    /// @param block_height Current block height for evaluation
    /// @param block_time Current block timestamp for evaluation
    [[nodiscard]] bool verify_locktime(Height block_height, Timestamp block_time) const noexcept;
};

} // namespace qv::core
