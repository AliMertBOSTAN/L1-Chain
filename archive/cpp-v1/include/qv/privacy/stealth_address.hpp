#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

namespace qv::privacy {

/// Type aliases for cryptographic key material
using bytes = std::vector<uint8_t>;

/// Public key from Kyber KEM (post-quantum key encapsulation mechanism)
struct KyberPublicKey {
    bytes data;
};

/// Public key from Dilithium (post-quantum digital signature scheme)
struct DilithiumPublicKey {
    bytes data;
};

/// Secret key from Kyber
struct KyberSecretKey {
    bytes data;
};

/// Secret key from Dilithium
struct DilithiumSecretKey {
    bytes data;
};

/// Stealth address keys holder for a recipient
/// The view_key allows scanning the blockchain for outputs destined to this address
/// The spend_key is derived per-output to enable spending without revealing identity
struct StealthAddressKeys {
    KyberPublicKey view_key;        ///< Used by recipient to identify incoming outputs
    DilithiumPublicKey spend_key;   ///< Base spend key (ephemeral per output)
};

/// Output created by sender using Kyber KEM
/// Contains encrypted information allowing only recipient to recover ephemeral spend key
struct EphemeralOutput {
    bytes stealth_pubkey;      ///< Ephemeral Dilithium public key (only recipient can derive)
    bytes encrypted_payload;   ///< Encrypted additional data (amount, metadata)
    bytes kem_ciphertext;      ///< Kyber ciphertext encapsulating shared secret
};

/// Information recovered by recipient after scanning blockchain
struct RecoveredOutput {
    bytes ephemeral_spend_pubkey;   ///< Recovered ephemeral public key
    bytes decrypted_payload;        ///< Decrypted payload (amount, metadata, etc.)
    uint64_t output_index;          ///< Index in transaction output list
};

/// Stealth Address Generator
///
/// Implements privacy through KEM-based stealth addresses:
/// - Recipient publishes view_key (Kyber) and spend_key (Dilithium)
/// - Sender creates ephemeral output using Kyber KEM to hide recipient
/// - Recipient scans chain with view_secret_key to identify owned outputs
/// - Recipient recovers ephemeral spend key to authorize spending
///
/// This design ensures:
/// - Output recipient is not publicly linkable to real spend key
/// - Only recipient can identify and spend their outputs
/// - Sender cannot link outputs to recipient's published keys
class StealthAddressGenerator {
public:
    StealthAddressGenerator() = default;
    ~StealthAddressGenerator() = default;

    /// Generate stealth address keys for a recipient
    ///
    /// Creates a new KyberPublicKey/KyberSecretKey pair for viewing outputs
    /// and a base DilithiumPublicKey/DilithiumSecretKey pair for spending.
    ///
    /// @return StealthAddressKeys with public view_key and spend_key for publishing
    /// @throws std::runtime_error if key generation fails
    StealthAddressKeys generate_stealth_keys();

    /// Create an ephemeral output for a recipient
    ///
    /// Sender-side operation that:
    /// 1. Generates ephemeral Dilithium keypair
    /// 2. Uses Kyber KEM to encapsulate shared secret to recipient's view_key
    /// 3. Encrypts ephemeral spend pubkey and amount payload
    /// 4. Returns output containing all encrypted material
    ///
    /// @param recipient_view_key The recipient's published Kyber public key
    /// @param recipient_spend_key The recipient's published Dilithium public key
    /// @param amount The amount being sent (will be encrypted)
    /// @return EphemeralOutput containing encrypted recipient identifiers
    /// @throws std::runtime_error if KEM or encryption fails
    EphemeralOutput create_output(
        const KyberPublicKey& recipient_view_key,
        const DilithiumPublicKey& recipient_spend_key,
        uint64_t amount
    );

    /// Scan an output to check if it belongs to this recipient
    ///
    /// Recipient-side operation that:
    /// 1. Uses view_secret_key to decrypt the Kyber ciphertext
    /// 2. Extracts shared secret and validates ciphertext
    /// 3. Decrypts payload containing ephemeral spend pubkey
    /// 4. Returns RecoveredOutput if decryption succeeds, nullopt otherwise
    ///
    /// @param view_secret_key The recipient's Kyber secret key
    /// @param output The EphemeralOutput to scan
    /// @return RecoveredOutput with decrypted ephemeral key and payload, or nullopt if not for this recipient
    /// @throws std::runtime_error if KEM decapsulation fails
    std::optional<RecoveredOutput> scan_output(
        const KyberSecretKey& view_secret_key,
        const EphemeralOutput& output
    );

    /// Recover the ephemeral spend key for spending
    ///
    /// Recipient-side operation that:
    /// 1. Combines recovered ephemeral spend pubkey with spend_secret_key
    /// 2. Derives final Dilithium secret key for authorizing spends
    /// 3. This derived key can sign transactions spending this output
    ///
    /// @param spend_secret_key The recipient's base Dilithium secret key
    /// @param recovered_info The RecoveredOutput from scan_output()
    /// @return DilithiumSecretKey for signing this output's spend
    /// @throws std::runtime_error if key derivation fails
    DilithiumSecretKey recover_spend_key(
        const DilithiumSecretKey& spend_secret_key,
        const RecoveredOutput& recovered_info
    );

private:
    // TODO: Private helper methods for Kyber and Dilithium operations
    // - kem_encapsulate(pubkey) -> (shared_secret, ciphertext)
    // - kem_decapsulate(ciphertext, secret_key) -> (shared_secret, success)
    // - encrypt_payload(plaintext, shared_secret) -> ciphertext
    // - decrypt_payload(ciphertext, shared_secret) -> plaintext
};

} // namespace qv::privacy
