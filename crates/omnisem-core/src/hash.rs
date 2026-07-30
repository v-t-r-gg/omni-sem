//! Content hashing with algorithm-prefixed digests.

use crate::domain::ContentHash;

/// Computes a BLAKE3 digest serialized as `blake3:<hex>`.
#[must_use]
pub fn blake3_hex(bytes: &[u8]) -> ContentHash {
    let digest = blake3::hash(bytes);
    ContentHash(format!("blake3:{digest}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_prefixed_and_stable() {
        let first = blake3_hex(b"hello");
        let second = blake3_hex(b"hello");
        assert_eq!(first, second);
        assert!(first.0.starts_with("blake3:"));
        assert_ne!(blake3_hex(b"hello"), blake3_hex(b"world"));
    }
}
