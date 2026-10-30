//! Account authN/authZ: password hashing (Argon2id), session tokens, and the
//! three Axum extractors that protect routes.
//!
//! Extractors read the `session` cookie, resolve it to a user via
//! `load_session`/`load_user` and reject with [`AppError::Unauthorized`]
//! (missing/unknown/expired) or [`AppError::Forbidden`] (role/status unfit).

use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum_extra::extract::CookieJar;
use sqlx::SqlitePool;

use crate::common::error::AppError;
use crate::common::models::{Role, Session, Status, User};
use crate::services::state::AppState;

/// Hashes a clear password with Argon2id (default parameters) into the PHC
/// string stored in `users.password_hash`.
///
/// # Errors
/// `AppError::Argon2` if hashing fails.
pub fn hash_password(password: &str) -> Result<String, AppError> {
    Ok(Argon2::default()
        .hash_password(password.as_bytes())?
        .to_string())
}

/// Checks a clear password against a stored PHC string (no side channels
/// beyond Argon2's own timing).
///
/// # Returns
/// `Ok(())` when the password matches; the error carries nothing sensitive.
///
/// # Errors
/// `AppError::Argon2` when the hash is malformed or the password differs.
pub fn verify_password(password: &str, hash: &str) -> Result<(), AppError> {
    Ok(Argon2::default().verify_password(password.as_bytes(), hash)?)
}

/// Generates a new session token: 32 random bytes rendered as 64 lowercase
/// hex chars, used both as the cookie value and the `sessions.id` primary key.
pub fn generate_session_token() -> String {
    let bytes: [u8; 32] = rand::random();
    hex::encode(bytes)
}

/// The successful outcome of [`AuthUser`]'s extraction: the account and the
/// session id that authorized this request (used by logout to delete the row).
pub struct AuthUser {
    pub user: User,
    pub session_id: String,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    /// Reads the `session` cookie, resolves it to a live session and its user.
    ///
    /// # Returns
    /// `AuthUser` with the full `users` row.
    ///
    /// # Errors
    /// `AppError::Unauthorized` when the cookie is missing, unknown, or the
    /// session is expired/deleted (`# Panics`: none).
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_request_parts(parts, state)
            .await
            .map_err(|_| AppError::Unauthorized)?;
        let session_id = jar
            .get("session")
            .map(|c| c.value().to_owned())
            .ok_or(AppError::Unauthorized)?;

        let session = load_session(&state.pool, &session_id).await?;
        if session.is_expired() {
            return Err(AppError::Unauthorized);
        }
        let user = load_user(&state.pool, session.user_id).await?;

        Ok(AuthUser { user, session_id })
    }
}

/// Requires a logged-in user with the `admin` role (admin-only routes).
///
/// Infallible-checked wrapper of [`AuthUser`]; the inner value is the auth it
/// wraps, so handlers get both user and session id.
pub struct AdminUser(pub AuthUser);

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = AppError;

    /// Delegates to [`AuthUser`] then checks `role == admin`.
    ///
    /// # Errors
    /// `AppError::Unauthorized` (delegated) or `AppError::Forbidden` when the
    /// account is not an admin.
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth = AuthUser::from_request_parts(parts, state).await?;
        if auth.user.role != Role::Admin {
            return Err(AppError::Forbidden);
        }
        Ok(AdminUser(auth))
    }
}

/// Requires a logged-in user whose account is `active` (guards user-facing
/// routes such as ticket requests — waitlist accounts cannot act yet).
pub struct ActiveUser(pub AuthUser);

impl FromRequestParts<AppState> for ActiveUser {
    type Rejection = AppError;

    /// Delegates to [`AuthUser`] then checks `status == active`.
    ///
    /// # Errors
    /// `AppError::Unauthorized` (delegated) or `AppError::Forbidden` when the
    /// account still sits on the waitlist.
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth = AuthUser::from_request_parts(parts, state).await?;
        if auth.user.status != Status::Active {
            return Err(AppError::Forbidden);
        }
        Ok(ActiveUser(auth))
    }
}

/// Loads a session row by cookie id. Unknown/missing id maps to
/// `AppError::Unauthorized`; expiry is checked by the caller.
///
/// # Errors
/// `AppError::Sqlx` on query failure.
async fn load_session(pool: &SqlitePool, session_id: &str) -> Result<Session, AppError> {
    sqlx::query_as::<_, Session>(
        "SELECT id, user_id, created_at, expires_at FROM sessions WHERE id = ?",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::Unauthorized)
}

/// Loads the full `users` row for a session's user. A missing row (deleted
/// account still holding a session) maps to `AppError::Unauthorized`.
///
/// # Errors
/// `AppError::Sqlx` on query failure.
async fn load_user(pool: &SqlitePool, user_id: i64) -> Result<User, AppError> {
    sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, status, role, created_at, activated_at, welcome_email_sent_at, telegram_id, notify_email, notify_telegram, shop_email, shop_password_cipher FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::Unauthorized)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A freshly hashed password verifies successfully against its own hash.
    #[test]
    fn hash_and_verify_roundtrip() {
        let password = generate_session_token();
        let hash = hash_password(&password).unwrap();
        assert!(verify_password(&password, &hash).is_ok());
    }

    /// Verifying with a different password fails.
    #[test]
    fn rejects_wrong_password() {
        let password = generate_session_token();
        let wrong = generate_session_token();
        let hash = hash_password(&password).unwrap();
        assert!(verify_password(&wrong, &hash).is_err());
    }

    /// A generated session token is 64 hexadecimal characters long.
    #[test]
    fn session_token_is_64_hex_chars() {
        let token = generate_session_token();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// Two generated session tokens differ.
    #[test]
    fn session_tokens_are_unique() {
        let a = generate_session_token();
        let b = generate_session_token();
        assert_ne!(a, b);
    }
}
