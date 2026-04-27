/// Hybrid KEM tests: X25519 + Kyber/ML-KEM.
///
/// Primary property: sender's shared_secret from encapsulate() must equal
/// the receiver's decapsulated shared secret.

#include "qv/crypto/hybrid_kem.hpp"

#include <gtest/gtest.h>

using namespace qv::crypto;

TEST(HybridKEMTest, KeypairHasCorrectSizes) {
    HybridKEMConfig cfg{KyberParameterSet::Level3};
    auto kp = generate_hybrid_kem_keypair(cfg);
    ASSERT_TRUE(kp.has_value());
    EXPECT_EQ(kp->x25519_public_key.size(), 32u);
    EXPECT_EQ(kp->x25519_secret_key.size(), 32u);
    EXPECT_EQ(kp->kyber_public_key.size(), cfg.kyber_public_key_size());
    EXPECT_EQ(kp->kyber_secret_key.size(), cfg.kyber_secret_key_size());
}

TEST(HybridKEMTest, EncapsulateDecapsulateRoundtrip) {
    HybridKEMConfig cfg{KyberParameterSet::Level3};
    auto receiver = generate_hybrid_kem_keypair(cfg);
    ASSERT_TRUE(receiver.has_value());

    HybridKEM kem(cfg);
    auto encap = kem.encapsulate(*receiver);
    ASSERT_TRUE(encap.has_value());

    auto decap_ss = kem.decapsulate(*receiver, encap->ciphertext,
                                    receiver->x25519_public_key.const_span());
    ASSERT_TRUE(decap_ss.has_value());
    EXPECT_EQ(encap->shared_secret, *decap_ss);
}

TEST(HybridKEMTest, CiphertextSizeMatchesSpec) {
    HybridKEMConfig cfg{KyberParameterSet::Level3};
    auto kp = generate_hybrid_kem_keypair(cfg);
    ASSERT_TRUE(kp.has_value());

    HybridKEM kem(cfg);
    auto encap = kem.encapsulate(*kp);
    ASSERT_TRUE(encap.has_value());
    // Hybrid CT = 32-byte X25519 ephemeral pk + Kyber CT
    EXPECT_EQ(encap->ciphertext.size(),
              32u + cfg.kyber_ciphertext_size());
    EXPECT_EQ(encap->ciphertext.size(), kem.expected_hybrid_ciphertext_size());
}

TEST(HybridKEMTest, TamperedCiphertextYieldsDifferentSecret) {
    HybridKEMConfig cfg{KyberParameterSet::Level3};
    auto kp = generate_hybrid_kem_keypair(cfg);
    ASSERT_TRUE(kp.has_value());

    HybridKEM kem(cfg);
    auto encap = kem.encapsulate(*kp);
    ASSERT_TRUE(encap.has_value());

    // Flip a bit in the Kyber ciphertext portion (Kyber is IND-CCA:
    // decap still succeeds but yields a different shared secret).
    auto tampered = encap->ciphertext;
    tampered.back() ^= 0x01;

    auto decap_ss = kem.decapsulate(*kp, tampered, {});
    if (decap_ss.has_value()) {
        EXPECT_NE(*decap_ss, encap->shared_secret);
    }
}

TEST(HybridKEMTest, WrongRecipientKeyFails) {
    HybridKEMConfig cfg{KyberParameterSet::Level3};
    auto alice = generate_hybrid_kem_keypair(cfg);
    auto bob   = generate_hybrid_kem_keypair(cfg);
    ASSERT_TRUE(alice.has_value() && bob.has_value());

    HybridKEM kem(cfg);
    auto encap = kem.encapsulate(*alice);
    ASSERT_TRUE(encap.has_value());

    // Bob tries to decapsulate a ciphertext intended for Alice.
    auto decap_ss = kem.decapsulate(*bob, encap->ciphertext, {});
    if (decap_ss.has_value()) {
        EXPECT_NE(*decap_ss, encap->shared_secret);
    }
}

TEST(HybridKEMTest, InvalidCiphertextLengthRejected) {
    HybridKEMConfig cfg{KyberParameterSet::Level3};
    auto kp = generate_hybrid_kem_keypair(cfg);
    ASSERT_TRUE(kp.has_value());

    HybridKEM kem(cfg);
    std::vector<uint8_t> too_short(10, 0);
    auto decap_ss = kem.decapsulate(*kp, too_short, {});
    ASSERT_FALSE(decap_ss.has_value());
    EXPECT_EQ(decap_ss.error(), CryptoError::InvalidCiphertext);
}
