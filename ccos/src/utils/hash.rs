//! Hash utilities for CCOS.
//!
//! This module provides common hashing functions used across the CCOS system.

/// Simple FNV-1a 64-bit hash
///
/// FNV-1a (Fowler-Noll-Vo) is a non-cryptographic hash function
/// that provides good distribution and collision resistance for
/// general-purpose hashing.
///
/// # Arguments
/// * `s` - The string to hash
///
/// # Returns
/// A 64-bit hash value
///
/// # Example
/// ```
/// use ccos::utils::hash::fnv1a64;
///
/// let hash = fnv1a64("hello world");
/// assert_ne!(hash, 0);
/// ```
pub fn fnv1a64(s: &str) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = OFFSET_BASIS;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fnv1a64_consistency() {
        let test_str = "test_string";
        let hash1 = fnv1a64(test_str);
        let hash2 = fnv1a64(test_str);
        assert_eq!(hash1, hash2, "Hash should be deterministic");
    }

    #[test]
    fn test_fnv1a64_different_inputs() {
        let hash1 = fnv1a64("string1");
        let hash2 = fnv1a64("string2");
        assert_ne!(hash1, hash2, "Different strings should produce different hashes");
    }

    #[test]
    fn test_fnv1a64_empty_string() {
        let hash = fnv1a64("");
        assert_eq!(hash, 0xcbf29ce484222325, "Empty string should return OFFSET_BASIS");
    }
}
