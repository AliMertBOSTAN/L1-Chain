//! Zeroize-on-drop byte buffer for secret material.
//!
//! [`SecureBytes`] should be used for any secret key, shared secret, or
//! other sensitive intermediate value. The buffer is zeroed on drop via
//! the [`zeroize`] crate, which uses volatile writes the compiler is not
//! allowed to optimize away.
//!
//! Equality comparisons go through [`subtle::ConstantTimeEq`] to avoid
//! leaking bits through timing side channels.

use core::fmt;
use core::ops::{Deref, DerefMut};

use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Secret byte buffer that zeroes its storage on drop and compares in
/// constant time.
///
/// Cloning intentionally allocates a fresh buffer; moving re-uses the
/// allocation without copying.
#[derive(Clone)]
pub struct SecureBytes {
    bytes: Vec<u8>,
}

impl SecureBytes {
    /// Construct an empty buffer.
    #[must_use]
    pub const fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    /// Construct a zero-filled buffer of the given length.
    #[must_use]
    pub fn zeros(len: usize) -> Self {
        Self {
            bytes: vec![0u8; len],
        }
    }

    /// Copy an existing slice into a fresh secure buffer.
    ///
    /// The source slice is **not** zeroed — the caller remains responsible
    /// for the lifetime of whatever produced it.
    #[must_use]
    pub fn from_slice(data: &[u8]) -> Self {
        Self {
            bytes: data.to_vec(),
        }
    }

    /// Consume a `Vec<u8>` into a secure buffer.
    ///
    /// The caller's vector is moved (no extra allocation). Useful when an
    /// upstream API handed us an owned byte vector we now want to protect.
    #[must_use]
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// Return an immutable byte slice over the buffer.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    /// Return a mutable byte slice over the buffer.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.bytes
    }

    /// Length of the buffer in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// `true` when the buffer holds no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Resize the buffer. If it shrinks, the tail bytes are zeroed before
    /// deallocation; if it grows, new bytes are zero-initialised.
    pub fn resize(&mut self, new_len: usize) {
        if new_len < self.bytes.len() {
            // Zero the region about to be dropped.
            self.bytes[new_len..].zeroize();
        }
        self.bytes.resize(new_len, 0);
    }

    /// Explicitly zero every byte in the buffer, leaving length unchanged.
    pub fn zero(&mut self) {
        self.bytes.zeroize();
    }
}

impl Default for SecureBytes {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for SecureBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.bytes
    }
}

impl DerefMut for SecureBytes {
    fn deref_mut(&mut self) -> &mut [u8] {
        &mut self.bytes
    }
}

impl AsRef<[u8]> for SecureBytes {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

impl Drop for SecureBytes {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

impl Zeroize for SecureBytes {
    fn zeroize(&mut self) {
        self.bytes.zeroize();
    }
}

impl ZeroizeOnDrop for SecureBytes {}

impl PartialEq for SecureBytes {
    fn eq(&self, other: &Self) -> bool {
        self.bytes.ct_eq(&other.bytes).into()
    }
}

impl Eq for SecureBytes {}

impl fmt::Debug for SecureBytes {
    /// Deliberately opaque: never prints the contents.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecureBytes({} bytes)", self.bytes.len())
    }
}

impl From<Vec<u8>> for SecureBytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self::from_vec(bytes)
    }
}

impl From<&[u8]> for SecureBytes {
    fn from(slice: &[u8]) -> Self {
        Self::from_slice(slice)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn empty_buffer() {
        let sb = SecureBytes::new();
        assert!(sb.is_empty());
        assert_eq!(sb.len(), 0);
    }

    #[test]
    fn roundtrip_from_slice() {
        let data = b"quantum-vault secret";
        let sb = SecureBytes::from_slice(data);
        assert_eq!(sb.as_slice(), data);
        assert_eq!(sb.len(), data.len());
    }

    #[test]
    fn zeros_constructor() {
        let sb = SecureBytes::zeros(16);
        assert_eq!(sb.len(), 16);
        assert!(sb.iter().all(|&b| b == 0));
    }

    #[test]
    fn resize_grow_and_shrink() {
        let mut sb = SecureBytes::from_slice(b"abcdefgh");
        sb.resize(4);
        assert_eq!(sb.as_slice(), b"abcd");
        sb.resize(6);
        assert_eq!(sb.as_slice(), &[b'a', b'b', b'c', b'd', 0, 0]);
    }

    #[test]
    fn zero_keeps_length() {
        let mut sb = SecureBytes::from_slice(b"secret");
        let len = sb.len();
        sb.zero();
        assert_eq!(sb.len(), len);
        assert!(sb.iter().all(|&b| b == 0));
    }

    #[test]
    fn equality_is_constant_time() {
        let a = SecureBytes::from_slice(b"same");
        let b = SecureBytes::from_slice(b"same");
        let c = SecureBytes::from_slice(b"diff");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn debug_does_not_leak() {
        let sb = SecureBytes::from_slice(b"super-secret");
        let rendered = format!("{sb:?}");
        assert!(!rendered.contains("super-secret"));
        assert!(rendered.contains("12 bytes"));
    }
}
