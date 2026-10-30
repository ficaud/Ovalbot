//! Billetterie credential vault: per-user shop email + AES-encrypted shop
//! password, stored in `users.shop_email` / `users.shop_password_cipher`.
//!
//! The clear password only ever exists in memory: written once (after
//! AES-256-GCM encryption) by [`set_credentials`], read back decrypted by
//! [`load_credentials`] for the Phase 4B checkout flow, never serialized.
//! Without a [`SecretKey`] configured (`AppState::shop_key`), writes are
//! refused (fail-closed) and reads of an existing cipher fail too.

use sqlx::SqlitePool;

use crate::common::crypto::{decrypt, encrypt, SecretKey};
use crate::common::error::AppError;

// ============================== TYPES ==============================
/// An authenticated user's decrypted shop credentials (billetterie account).
#[derive(Debug, Clone)]
pub struct ShopCredentials {
    /// The billetterie email (clear, the user knows it anyway).
    pub email: String,
    /// The decrypted billetterie password.
    pub password: String,
}

// ============================== SERVICE ==============================

/// Stores (or replaces) the user's billetterie credentials: the clear password
/// is encrypted with the server's key before hitting the database.
///
/// # Arguments
/// * `key` — the AES key (`AppState::shop_key`); refusing the call when absent
/// # Errors
/// [`AppError::ShopCredentialsUnavailable`] when no key is configured,
/// [`AppError::Crypto`] when the encryption fails, [`AppError::Sqlx`] on DB
/// errors
pub async fn set_credentials(
    pool: &SqlitePool,
    user_id: i64,
    email: &str,
    password: &str,
    key: Option<&SecretKey>,
) -> Result<(), AppError> {
    let key = key.ok_or(AppError::ShopCredentialsUnavailable)?;
    let cipher = encrypt(key, password)?;

    sqlx::query("UPDATE users SET shop_email = ?, shop_password_cipher = ? WHERE id = ?")
        .bind(email)
        .bind(&cipher)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Clears the user's billetterie credentials (both columns set to `NULL`).
///
/// # Errors
/// `AppError::Sqlx` on DB errors
pub async fn clear_credentials(pool: &SqlitePool, user_id: i64) -> Result<(), AppError> {
    sqlx::query("UPDATE users SET shop_email = NULL, shop_password_cipher = NULL WHERE id = ?")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Loads and decrypts the user's billetterie credentials.
///
/// # Arguments
/// * `key` — the AES key; when absent while a cipher is stored, the load fails
///   closed (that decrypt cannot be done without the server's key)
/// # Returns
/// `None` when nothing is stored (no shop account configured yet)
/// # Errors
/// [`AppError::ShopCredentialsUnavailable`] when a cipher exists but no key is
/// configured, [`AppError::Crypto`] when the decryption fails (wrong /
/// rotated `SECRET_KEY`), [`AppError::Sqlx`] on DB errors
pub async fn load_credentials(
    pool: &SqlitePool,
    user_id: i64,
    key: Option<&SecretKey>,
) -> Result<Option<ShopCredentials>, AppError> {
    let row: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT shop_email, shop_password_cipher FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;

    let Some((email, cipher)) = row else {
        return Ok(None);
    };
    let Some(cipher) = cipher else {
        return Ok(None);
    };

    let key = key.ok_or(AppError::ShopCredentialsUnavailable)?;
    let password = decrypt(key, &cipher)?;
    Ok(email.map(|email| ShopCredentials { email, password }))
}
