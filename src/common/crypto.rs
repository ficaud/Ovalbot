//! Symmetric encryption helpers for data at rest that must stay reversible
//! (the user's shop password, needed later in clear to log into the shop, and
//! the admin's broadcast-bot token, needed in clear to send Telegram messages).
//!
//! Everything here is keyed by [`SecretKey`], derived from the server's
//! `SECRET_KEY` secret with HKDF-SHA256 (extract-then-expand, fixed salt plus
//! a domain-separation label) into a 32-byte AES-256 key. The storage format
//! is `hex(nonce) || hex(ciphertext+tag)` — a fresh random 12-byte nonce per
//! encryption, GCM's tag rides along the ciphertext.
//!
//! `SECRET_KEY` is key material (README: `openssl rand -base64 48`), not a
//! user password, so no stretched password hash is involved; HKDF is the KDF
//! recommended for exactly this job. The derivation is deliberately versioned:
//! changing the salt or label yields a different key, which makes previously
//! stored ciphers unreadable (they must be re-saved by their owner).

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hkdf::Hkdf;
use sha2::Sha256;

// ============================== KEY DERIVATION ==============================

/// Non-secret HKDF salt: extracts a pseudorandom key from the server secret
/// before expansion. Fixed on purpose (a random salt could not be persisted
/// alongside the ciphers without also storing it).
const KDF_SALT: &[u8] = b"ovalbot.shop-credentials.salt.v1";

/// HKDF `info` label: binds the output to the shop-credentials vault, so the
/// same `SECRET_KEY` can safely key another feature without collision.
const KDF_INFO: &[u8] = b"ovalbot.shop-credentials.aes-256-gcm.v1";

/// HKDF salt for the admin's Telegram broadcast-bot token vault. A different
/// salt *and* info than the shop vault: a cipher from one feature cannot be
/// decrypted with the key of the other.
const BROADCAST_KDF_SALT: &[u8] = b"ovalbot.broadcast-bot-token.salt.v1";

/// HKDF `info` label for the broadcast-bot token vault.
const BROADCAST_KDF_INFO: &[u8] = b"ovalbot.broadcast-bot-token.aes-256-gcm.v1";

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

/// An AES-256 key derived from the server's `SECRET_KEY` secret for one
/// specific feature. Holding no key means that feature's secret storage is
/// refused (fail-closed).
#[derive(Debug, Clone, Copy)]
pub struct SecretKey {
    /// The 32-byte HKDF-SHA256 output.
    key: [u8; 32],
}

impl SecretKey {
    /// Derives the shop-credentials AES-256 key from `SECRET_KEY`.
    ///
    /// HKDF-SHA256 (extract with `KDF_SALT`, expand with `KDF_INFO`) rather
    /// than a bare hash: SHA-256 on its own is not a key-derivation function,
    /// and flagging it is exactly what security scanners do for a secret that
    /// looks like a password.
    pub fn from_passphrase(secret: &str) -> Self {
        Self::derive(KDF_SALT, KDF_INFO, secret)
    }

    /// Derives the broadcast-bot-token AES-256 key from `SECRET_KEY`, in a
    /// separate HKDF domain from the shop vault.
    pub fn for_broadcast_bot(secret: &str) -> Self {
        Self::derive(BROADCAST_KDF_SALT, BROADCAST_KDF_INFO, secret)
    }

    /// Extracts then expands the 32-byte AES key for the given domain labels.
    fn derive(salt: &[u8], info: &[u8], secret: &str) -> Self {
        let hkdf = Hkdf::<Sha256>::new(Some(salt), secret.as_bytes());
        let mut key = [0u8; 32];
        hkdf.expand(info, &mut key)
            .expect("HKDF-SHA256 always produces 32 bytes");
        Self { key }
    }

    /// Reads a non-blank `SECRET_KEY` from the environment (fail-closed: an
    /// absent or blank value yields no key at all).
    fn env_secret() -> Option<String> {
        std::env::var("SECRET_KEY")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    }

    /// Reads `SECRET_KEY` and derives the shop-credentials key, when configured.
    pub fn from_env() -> Option<Self> {
        Self::env_secret().as_deref().map(Self::from_passphrase)
    }

    /// Reads `SECRET_KEY` and derives the broadcast-bot-token key, when set.
    pub fn from_env_for_broadcast() -> Option<Self> {
        Self::env_secret().as_deref().map(Self::for_broadcast_bot)
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
    let cipher = Aes256Gcm::new((&key.key).into());
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

    let cipher = Aes256Gcm::new((&key.key).into());
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

    // The key derivation depends on the whole secret (HKDF-SHA256).
    #[test]
    fn passphrases_derive_distinct_keys() {
        let a = SecretKey::from_passphrase("one");
        let b = SecretKey::from_passphrase("two");
        assert_ne!(a.key, b.key);
        assert_eq!(a.key, SecretKey::from_passphrase("one").key);
    }

    // Regression guard: the key must not be a bare SHA-256 of the secret (the
    // insecure derivation security scanners flag for password-like input).
    #[test]
    fn derivation_is_not_a_bare_sha256_of_the_secret() {
        use sha2::Digest;

        let secret = "openssl-rand-base64-48";
        assert_ne!(
            SecretKey::from_passphrase(secret).key,
            <[u8; 32]>::from(sha2::Sha256::digest(secret.as_bytes()))
        );
    }

    // HKDF is deterministic: a fixed salt/info pair always yields the same key
    // for the same secret (otherwise stored ciphers would never decrypt).
    #[test]
    fn derivation_is_deterministic() {
        let first = SecretKey::from_passphrase("stable-secret");
        let second = SecretKey::from_passphrase("stable-secret");
        assert_eq!(first.key, second.key);
    }

    // The broadcast-bot vault has its own key domain: a token cipher cannot be
    // decrypted with the shop key, and vice versa.
    #[test]
    fn broadcast_and_shop_keys_are_domain_separated() {
        let shop = SecretKey::from_passphrase("same-secret");
        let broadcast = SecretKey::for_broadcast_bot("same-secret");
        assert_ne!(shop.key, broadcast.key);

        let stored = encrypt(&broadcast, "123456:token").unwrap();
        assert!(decrypt(&shop, &stored).is_err());
        assert_eq!(decrypt(&broadcast, &stored).unwrap(), "123456:token");
        assert_eq!(
            SecretKey::for_broadcast_bot("same-secret").key,
            broadcast.key
        );
    }
}
