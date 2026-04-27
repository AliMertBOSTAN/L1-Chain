#include "qv/core/transaction.hpp"
#include <numeric>
#include <algorithm>
#include <sstream>

namespace qv::core {

bool Transaction::is_valid() const noexcept {
    // Must have at least one input and one output
    if (inputs.empty() || outputs.empty()) {
        return false;
    }

    // Check size constraints
    if (inputs.size() > constants::MAX_TX_INPUTS ||
        outputs.size() > constants::MAX_TX_OUTPUTS) {
        return false;
    }

    // All inputs must be valid
    if (!inputs_valid()) {
        return false;
    }

    // All outputs must be valid
    if (!outputs_valid()) {
        return false;
    }

    // Total output value must not exceed max supply
    Amount total_out = total_output_value();
    if (total_out > constants::MAX_SUPPLY) {
        return false;
    }

    return true;
}

bool Transaction::inputs_valid() const noexcept {
    return std::all_of(inputs.begin(), inputs.end(),
        [](const TxInput& input) { return input.is_valid(); });
}

bool Transaction::outputs_valid() const noexcept {
    return std::all_of(outputs.begin(), outputs.end(),
        [](const TxOutput& output) { return output.is_valid(); });
}

Amount Transaction::total_output_value() const noexcept {
    return std::accumulate(outputs.begin(), outputs.end(),
        Amount(0),
        [](Amount sum, const TxOutput& output) {
            // Check for overflow
            if (sum > constants::MAX_SUPPLY - output.value) {
                return constants::MAX_SUPPLY; // Saturate on overflow
            }
            return sum + output.value;
        });
}

Result<TxId, std::string> Transaction::compute_txid() const {
    // Serialize the transaction
    auto serialized = to_bytes();
    if (!serialized.is_ok()) {
        return Result<TxId, std::string>(serialized.error());
    }

    const auto& tx_bytes = serialized.value();

    // TODO: Implement proper SHA256 double hash
    // For now, this is a stub that returns a placeholder
    // In a real implementation, use a crypto library (e.g., OpenSSL, libsodium)

    TxId txid{};
    // Placeholder: Use first 32 bytes of serialized data (obviously not cryptographically secure)
    std::size_t copy_size = std::min(tx_bytes.size(), txid.size());
    std::copy(tx_bytes.begin(), tx_bytes.begin() + copy_size, txid.begin());

    return Result<TxId, std::string>(txid);
}

Result<bytes, std::string> Transaction::to_bytes() const {
    try {
        bytes result;

        // Version (4 bytes, little-endian)
        result.push_back((version >> 0) & 0xFF);
        result.push_back((version >> 8) & 0xFF);
        result.push_back((version >> 16) & 0xFF);
        result.push_back((version >> 24) & 0xFF);

        // Input count (varint encoding, simplified to 1 byte for small counts)
        if (inputs.size() > 252) {
            return Result<bytes, std::string>(
                std::string("Transaction has too many inputs: ") + std::to_string(inputs.size())
            );
        }
        result.push_back(static_cast<std::uint8_t>(inputs.size()));

        // Serialize each input
        for (const auto& input : inputs) {
            // Previous output hash (32 bytes)
            result.insert(result.end(), input.prev_output.tx_id.begin(), input.prev_output.tx_id.end());

            // Previous output index (4 bytes)
            std::uint32_t idx = input.prev_output.index;
            result.push_back((idx >> 0) & 0xFF);
            result.push_back((idx >> 8) & 0xFF);
            result.push_back((idx >> 16) & 0xFF);
            result.push_back((idx >> 24) & 0xFF);

            // Signature length and data
            if (input.signature.size() > 0xFFFF) {
                return Result<bytes, std::string>("Signature too large");
            }
            std::uint16_t sig_len = static_cast<std::uint16_t>(input.signature.size());
            result.push_back((sig_len >> 0) & 0xFF);
            result.push_back((sig_len >> 8) & 0xFF);
            result.insert(result.end(), input.signature.begin(), input.signature.end());

            // Witness data length and data
            if (input.witness_data.size() > 0xFFFF) {
                return Result<bytes, std::string>("Witness data too large");
            }
            std::uint16_t witness_len = static_cast<std::uint16_t>(input.witness_data.size());
            result.push_back((witness_len >> 0) & 0xFF);
            result.push_back((witness_len >> 8) & 0xFF);
            result.insert(result.end(), input.witness_data.begin(), input.witness_data.end());
        }

        // Output count
        if (outputs.size() > 252) {
            return Result<bytes, std::string>(
                std::string("Transaction has too many outputs: ") + std::to_string(outputs.size())
            );
        }
        result.push_back(static_cast<std::uint8_t>(outputs.size()));

        // Serialize each output
        for (const auto& output : outputs) {
            // Value (8 bytes, little-endian)
            std::uint64_t val = output.value;
            result.push_back((val >> 0) & 0xFF);
            result.push_back((val >> 8) & 0xFF);
            result.push_back((val >> 16) & 0xFF);
            result.push_back((val >> 24) & 0xFF);
            result.push_back((val >> 32) & 0xFF);
            result.push_back((val >> 40) & 0xFF);
            result.push_back((val >> 48) & 0xFF);
            result.push_back((val >> 56) & 0xFF);

            // Locking script length and data
            if (output.locking_script.size() > 0xFFFF) {
                return Result<bytes, std::string>("Locking script too large");
            }
            std::uint16_t script_len = static_cast<std::uint16_t>(output.locking_script.size());
            result.push_back((script_len >> 0) & 0xFF);
            result.push_back((script_len >> 8) & 0xFF);
            result.insert(result.end(), output.locking_script.begin(), output.locking_script.end());

            // Stealth address data length and data
            if (output.stealth_address_data.size() > 0xFFFF) {
                return Result<bytes, std::string>("Stealth address data too large");
            }
            std::uint16_t stealth_len = static_cast<std::uint16_t>(output.stealth_address_data.size());
            result.push_back((stealth_len >> 0) & 0xFF);
            result.push_back((stealth_len >> 8) & 0xFF);
            result.insert(result.end(), output.stealth_address_data.begin(), output.stealth_address_data.end());
        }

        // Lock time (8 bytes)
        std::uint64_t lt = lock_time;
        result.push_back((lt >> 0) & 0xFF);
        result.push_back((lt >> 8) & 0xFF);
        result.push_back((lt >> 16) & 0xFF);
        result.push_back((lt >> 24) & 0xFF);
        result.push_back((lt >> 32) & 0xFF);
        result.push_back((lt >> 40) & 0xFF);
        result.push_back((lt >> 48) & 0xFF);
        result.push_back((lt >> 56) & 0xFF);

        return Result<bytes, std::string>(result);
    } catch (const std::exception& e) {
        return Result<bytes, std::string>(
            std::string("Serialization error: ") + e.what()
        );
    }
}

Result<Transaction, std::string> Transaction::from_bytes(const bytes& data) {
    // TODO: Implement deserialization
    // This is a stub that returns an error
    return Result<Transaction, std::string>(
        std::string("Transaction deserialization not yet implemented")
    );
}

std::size_t Transaction::serialized_size() const noexcept {
    auto serialized = to_bytes();
    if (serialized.is_ok()) {
        return serialized.value().size();
    }
    return 0;
}

bool Transaction::is_coinbase() const noexcept {
    // Coinbase transactions have exactly one input with a null outpoint (all zeros)
    if (inputs.size() != 1) {
        return false;
    }

    const auto& input = inputs[0];
    TxId null_id{};
    return (input.prev_output.tx_id == null_id && input.prev_output.index == 0xFFFFFFFF);
}

bool Transaction::verify_locktime(Height block_height, Timestamp block_time) const noexcept {
    if (lock_time == 0) {
        return true; // No lock time
    }

    // If lock_time >= 500,000,000, it's a timestamp
    constexpr std::uint64_t LOCKTIME_THRESHOLD = 500'000'000;

    if (lock_time < LOCKTIME_THRESHOLD) {
        // Block height lock
        return block_height >= lock_time;
    } else {
        // Timestamp lock
        return block_time >= lock_time;
    }
}

} // namespace qv::core
