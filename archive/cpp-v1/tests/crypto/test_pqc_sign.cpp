/// Dilithium / ML-DSA signature tests (liboqs-backed).

#include "qv/crypto/pqc_sign.hpp"

#include <gtest/gtest.h>

#include <string>
#include <vector>

using namespace qv::crypto;

namespace {

std::vector<uint8_t> bytes(std::string_view s) {
    return {s.begin(), s.end()};
}

}  // namespace

TEST(PQCSignTest, KeypairHasExpectedSizes) {
    DilithiumConfig cfg{DilithiumParameterSet::Level3};
    auto kp = generate_pqc_keypair(cfg);
    ASSERT_TRUE(kp.has_value());
    EXPECT_EQ(kp->public_key.size(), cfg.public_key_size());
    EXPECT_EQ(kp->secret_key.size(), cfg.secret_key_size());
    EXPECT_GT(cfg.signature_size(), 0u);
}

TEST(PQCSignTest, SignAndVerifyRoundtripLevel3) {
    auto kp = generate_pqc_keypair();
    ASSERT_TRUE(kp.has_value());

    auto msg = bytes("Transaction: Alice -> Bob 10 QV");
    auto sig = pqc_sign(kp->secret_key.const_span(), msg);
    ASSERT_TRUE(sig.has_value());

    auto ok = pqc_verify(kp->public_key.const_span(), msg, *sig);
    ASSERT_TRUE(ok.has_value());
    EXPECT_TRUE(*ok);
}

TEST(PQCSignTest, TamperedMessageFailsVerification) {
    auto kp = generate_pqc_keypair();
    ASSERT_TRUE(kp.has_value());

    auto msg = bytes("original");
    auto sig = pqc_sign(kp->secret_key.const_span(), msg);
    ASSERT_TRUE(sig.has_value());

    auto tampered = bytes("original!");
    auto ok = pqc_verify(kp->public_key.const_span(), tampered, *sig);
    ASSERT_TRUE(ok.has_value());
    EXPECT_FALSE(*ok);
}

TEST(PQCSignTest, WrongPublicKeyFailsVerification) {
    auto kp_a = generate_pqc_keypair();
    auto kp_b = generate_pqc_keypair();
    ASSERT_TRUE(kp_a.has_value() && kp_b.has_value());

    auto msg = bytes("sign with A, verify with B");
    auto sig = pqc_sign(kp_a->secret_key.const_span(), msg);
    ASSERT_TRUE(sig.has_value());

    auto ok = pqc_verify(kp_b->public_key.const_span(), msg, *sig);
    ASSERT_TRUE(ok.has_value());
    EXPECT_FALSE(*ok);
}

TEST(PQCSignTest, AllThreeLevelsRoundtrip) {
    for (auto level : {DilithiumParameterSet::Level2,
                       DilithiumParameterSet::Level3,
                       DilithiumParameterSet::Level5}) {
        DilithiumConfig cfg{level};
        auto kp = generate_pqc_keypair(cfg);
        ASSERT_TRUE(kp.has_value()) << "level=" << static_cast<int>(level);

        auto msg = bytes("level test");
        auto sig = pqc_sign(kp->secret_key.const_span(), msg, level);
        ASSERT_TRUE(sig.has_value());

        auto ok = pqc_verify(kp->public_key.const_span(), msg, *sig, level);
        ASSERT_TRUE(ok.has_value());
        EXPECT_TRUE(*ok);
    }
}

TEST(PQCSignTest, InvalidKeySizeRejected) {
    std::vector<uint8_t> bogus_sk(10, 0);
    auto sig = pqc_sign(bogus_sk, bytes("x"));
    EXPECT_FALSE(sig.has_value());
    EXPECT_EQ(sig.error(), CryptoError::InvalidKeySize);
}
