//! Symmetric encryption helpers for data at rest that must stay reversible
//! (the user's shop password, needed later in clear to log into the shop).
//!
//! Everything here is keyed by [`SecretKey`], derived from the server's
//! `SECRET_KEY` passphrase (SHA-256, a 32-byte AES-256 key). The storage
//! format is `hex(nonce) || hex(ciphertext+tag)` — a fresh random 12-byte
//! nonce per encryption, GCM's tag rides along the ciphertext.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use sha2::{Digest, Sha256};

// ============================== ERRORS ==============================

/// Why an encrypt/decrypt round trip failed (internal-only: collapses into
/// `AppError::Crypto` → a generic 500, details stay in the logs).
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    /// The stored string is not two well-formed, even-length hex slices.
    #[error("stored value is not valid hex(ncipher) shape")]
    InvalidHex,
    /// The AES-GCM cipher rejected the operation (key/tag mismatch).
    #[error("cipher error: {0}")]
    Cipher(aes_gcm::Error),
    /// The decrypted plaintext is not valid UTF-8.
    #[error("decrypted plaintext is not utf-8")]
    Plaintext,
}

// ============================== KEY ==============================

/// The AES-256 secret key derived once at startup from the `SECRET_KEY`
/// passphrase. Holding no key means the shop-credentials feature is refused
/// (fail-closed).
#[derive(Debug, Clone, Copy)]
pub struct SecretKey {
    /// The 32-byte SHA-256 of the passphrase.
    key: [u8; 32],
}

impl SecretKey {
    /// Derives the key material from the server's `SECRET_KEY` passphrase.
    pub fn from_passphrase(passphrase: &str) -> Self {
        Self {
            key: Sha256::digest(passphrase.as_bytes()).into(),
        }
    }

    /// Reads `SECRET_KEY` from the environment: `Some` when set to a
    /// non-blank value. An absent or blank passphrase is treated as *no key*
    /// (fail-closed) — an empty SHA-256 source would silently secure
    /// everything with a publicly derivable key.
    pub fn from_env() -> Option<Self> {
        std::env::var("SECRET_KEY")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .map(|v| Self::from_passphrase(&v))
    }
}

// ============================== ROUND TRIP ==============================

/// Encrypts a plaintext into the storable hex form (`nonce || cipher+tag`).
///
/// # Arguments
/// * `plaintext` - The clear value (e.g. the user's shop password)
/// # Returns
/// The hex-encoded `nonce || ciphertext` storage value
/// # Errors
/// Regex-free: only the cipher itself can fail (practically nothing can
/// encrypt wrong with a valid key)
pub fn encrypt(key: &SecretKey, plaintext: &str) -> Result<String, CryptoError> {
    let cipher = Aes256Gcm::new(key.key.as_slice().into());
    let nonce_b: [u8; 12] = rand::random();
    let ciphertext = cipher
        .encrypt(&Nonce::from(nonce_b), Payload::from(plaintext.as_bytes()))
        .map_err(CryptoError::Cipher)?;
    Ok(hex::encode(nonce_b) + &hex::encode(ciphertext))
}

/// Decrypts a value produced by [`encrypt`].
///
/// # Arguments
/// * `stored` - The stored hex form
/// # Returns
/// The clear plaintext
/// # Errors
/// [`CryptoError::InvalidHex`] on a malformed storage value (wrong length or
/// non-hex), [`CryptoError::Cipher`] on a wrong key / tampered data,
/// [`CryptoError::Plaintext`] when the plaintext is not UTF-8
pub fn decrypt(key: &SecretKey, stored: &str) -> Result<String, CryptoError> {
    const NONCE_BYTES: usize = 12;

    // The storage shape is exactly one 12-byte nonce followed by ciphertext.
    if stored.len() <= NONCE_BYTES * 2 || !stored.len().is_multiple_of(2) {
        return Err(CryptoError::InvalidHex);
    }
    let (nonce_hex, cipher_hex) = stored.split_at(NONCE_BYTES * 2);
    let nonce_bytes = hex::decode(nonce_hex).map_err(|_| CryptoError::InvalidHex)?;
    let ciphertext = hex::decode(cipher_hex).map_err(|_| CryptoError::InvalidHex)?;
    let Ok(nonce_b) = <[u8; NONCE_BYTES]>::try_from(nonce_bytes.as_slice()) else {
        return Err(CryptoError::InvalidHex);
    };

    let cipher = Aes256Gcm::new(key.key.as_slice().into());
    let plaintext = cipher
        .decrypt(&Nonce::from(nonce_b), Payload::from(ciphertext.as_slice()))
        .map_err(CryptoError::Cipher)?;
    String::from_utf8(plaintext).map_err(|_| CryptoError::Plaintext)
}

// ============================== TESTS ==============================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> SecretKey {
        SecretKey::from_passphrase("test-secret")
    }

    // A round trip gives back the exact plaintext.
    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = test_key();
        let stored = encrypt(&key, "mon mot de passe\navec retours").unwrap();
        assert_eq!(
            decrypt(&key, &stored).unwrap(),
            "mon mot de passe\navec retours"
        );
    }

    // Every encryption uses a fresh nonce: two storages of the same plaintext
    // differ (and both decrypt back).
    #[test]
    fn two_storages_differ_but_both_decrypt() {
        let key = test_key();
        let a = encrypt(&key, "same").unwrap();
        let b = encrypt(&key, "same").unwrap();
        assert_ne!(a, b, "nonce reuse would be a crypto bug");
        assert_eq!(decrypt(&key, &a).unwrap(), "same");
        assert_eq!(decrypt(&key, &b).unwrap(), "same");
    }

    // The storage format is strictly hex (nonce 24 hex chars + cipher/tag).
    #[test]
    fn storage_format_is_hex() {
        let key = test_key();
        let stored = encrypt(&key, "x").unwrap();
        assert!(stored.len() > 24);
        assert!(stored.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // A different key cannot decrypt the value (tag mismatch).
    #[test]
    fn wrong_key_fails_to_decrypt() {
        let stored = encrypt(&test_key(), "secret").unwrap();
        let other = SecretKey::from_passphrase("another");
        assert!(matches!(
            decrypt(&other, &stored),
            Err(CryptoError::Cipher(_))
        ));
    }

    // Tampering with the ciphertext fails the tag check too.
    #[test]
    fn tampered_ciphertext_fails_to_decrypt() {
        let stored = encrypt(&test_key(), "secret").unwrap();
        let mut bytes = stored.clone().into_bytes();
        let last = bytes.len() - 1;
        bytes[last] = if bytes[last] == b'0' { b'1' } else { b'0' };
        assert!(decrypt(&test_key(), &String::from_utf8(bytes).unwrap()).is_err());
    }

    // Short / odd-length / non-hex storage shapes are rejected up front.
    #[test]
    fn malformed_storage_is_rejected() {
        let key = test_key();
        for bad in [
            "",
            "abcd",
            "00",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
        ] {
            assert!(matches!(decrypt(&key, bad), Err(CryptoError::InvalidHex)));
        }
    }

    // The key derivation depends on the whole passphrase (SHA-256).
    #[test]
    fn passphrases_derive_distinct_keys() {
        let a = SecretKey::from_passphrase("one");
        let b = SecretKey::from_passphrase("two");
        assert_ne!(a.key, b.key);
        assert_eq!(a.key, SecretKey::from_passphrase("one").key);
    }
}
