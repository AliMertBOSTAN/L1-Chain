/// Hash function tests.
///
/// Includes NIST known-answer tests (KATs) for SHA3-256 and
/// avalanche / determinism / streaming equivalence tests.

#include "qv/crypto/hash.hpp"

#include <gtest/gtest.h>

#include <array>
#include <string>
#include <vector>

using namespace qv::crypto;

namespace {

std::vector<uint8_t> bytes(std::string_view s) {
    return {s.begin(), s.end()};
}

std::string hex(const HashDigest& d) {
    static const char* digits = "0123456789abcdef";
    std::string out(d.size() * 2, '0');
    for (size_t i = 0; i < d.size(); ++i) {
        out[2 * i]     = digits[(d[i] >> 4) & 0xF];
        out[2 * i + 1] = digits[d[i] & 0xF];
    }
    return out;
}

}  // namespace

// ----------------------------------------------------------------------------
// SHA3-256 Known-Answer Tests (from NIST FIPS 202)
// ----------------------------------------------------------------------------

TEST(SHA3HashTest, EmptyInputMatchesNistKat) {
    // NIST SHA3-256("") = a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a
    auto result = sha3_256({});
    ASSERT_TRUE(result.has_value());
    EXPECT_EQ(hex(*result),
              "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a");
}

TEST(SHA3HashTest, AbcMatchesNistKat) {
    // NIST SHA3-256("abc") = 3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532
    auto input = bytes("abc");
    auto result = sha3_256(input);
    ASSERT_TRUE(result.has_value());
    EXPECT_EQ(hex(*result),
              "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532");
}

TEST(SHA3HashTest, Determinism) {
    auto input = bytes("QuantumVault");
    auto a = sha3_256(input);
    auto b = sha3_256(input);
    ASSERT_TRUE(a.has_value() && b.has_value());
    EXPECT_EQ(*a, *b);
}

TEST(SHA3HashTest, AvalancheOneBitFlip) {
    auto a = sha3_256(bytes("hello"));
    auto b = sha3_256(bytes("hellp"));  // single-character change
    ASSERT_TRUE(a.has_value() && b.has_value());
    EXPECT_NE(*a, *b);
}

TEST(SHA3HashTest, DoubleHashEqualsHashOfHash) {
    auto input = bytes("merkle");
    auto first = sha3_256(input);
    ASSERT_TRUE(first.has_value());
    auto second = sha3_256(std::span<const uint8_t>(first->data(), first->size()));
    ASSERT_TRUE(second.has_value());

    auto dbl = double_sha3_256(input);
    ASSERT_TRUE(dbl.has_value());
    EXPECT_EQ(*dbl, *second);
}

TEST(SHA3HashTest, LargeInputOneMegabyte) {
    std::vector<uint8_t> big(1024 * 1024, 0x42);
    auto result = sha3_256(big);
    ASSERT_TRUE(result.has_value());
}

TEST(SHA3HashTest, StreamingMatchesOneShot) {
    auto input = bytes("The quick brown fox jumps over the lazy dog");
    auto one_shot = sha3_256(input);
    ASSERT_TRUE(one_shot.has_value());

    Hasher h(HashAlgorithm::SHA3_256);
    ASSERT_TRUE(h.update(std::span<const uint8_t>(input.data(), 10)).has_value());
    ASSERT_TRUE(h.update(std::span<const uint8_t>(input.data() + 10,
                                                   input.size() - 10)).has_value());
    auto streamed = h.finalize();
    ASSERT_TRUE(streamed.has_value());
    EXPECT_EQ(*one_shot, *streamed);
}
