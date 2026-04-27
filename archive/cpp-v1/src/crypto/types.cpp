/// SecureBytes implementation with secure zeroization on destruction.
///
/// Uses OpenSSL's OPENSSL_cleanse() which is guaranteed not to be optimized away
/// by the compiler. This is essential for cryptographic secret material
/// (private keys, shared secrets, sensitive intermediate values).

#include "qv/crypto/types.hpp"

#include <openssl/crypto.h>

#include <algorithm>
#include <cstring>

namespace qv::crypto {

SecureBytes::SecureBytes(size_t size) : m_data(size, 0) {}

SecureBytes::SecureBytes(const uint8_t* data, size_t size) : m_data(size) {
    if (size > 0 && data != nullptr) {
        std::memcpy(m_data.data(), data, size);
    }
}

SecureBytes::~SecureBytes() {
    zero();
}

SecureBytes::SecureBytes(const SecureBytes& other) : m_data(other.m_data) {}

SecureBytes& SecureBytes::operator=(const SecureBytes& other) {
    if (this != &other) {
        zero();
        m_data = other.m_data;
    }
    return *this;
}

SecureBytes::SecureBytes(SecureBytes&& other) noexcept : m_data(std::move(other.m_data)) {
    other.m_data.clear();
}

SecureBytes& SecureBytes::operator=(SecureBytes&& other) noexcept {
    if (this != &other) {
        zero();
        m_data = std::move(other.m_data);
        other.m_data.clear();
    }
    return *this;
}

uint8_t* SecureBytes::data() {
    return m_data.data();
}

const uint8_t* SecureBytes::data() const {
    return m_data.data();
}

size_t SecureBytes::size() const {
    return m_data.size();
}

bool SecureBytes::empty() const {
    return m_data.empty();
}

void SecureBytes::resize(size_t new_size) {
    if (new_size < m_data.size()) {
        // Zero out the soon-to-be-dropped tail before shrinking.
        OPENSSL_cleanse(m_data.data() + new_size, m_data.size() - new_size);
    }
    m_data.resize(new_size, 0);
}

void SecureBytes::clear() {
    zero();
    m_data.clear();
}

void SecureBytes::zero() {
    if (!m_data.empty()) {
        OPENSSL_cleanse(m_data.data(), m_data.size());
    }
}

uint8_t& SecureBytes::operator[](size_t idx) {
    return m_data[idx];
}

const uint8_t& SecureBytes::operator[](size_t idx) const {
    return m_data[idx];
}

std::span<uint8_t> SecureBytes::span() {
    return {m_data.data(), m_data.size()};
}

std::span<const uint8_t> SecureBytes::const_span() const {
    return {m_data.data(), m_data.size()};
}

}  // namespace qv::crypto
