use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum_extra::extract::CookieJar;
use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::{Role, Session, Status, User};
use crate::state::AppState;

pub fn hash_password(password: &str) -> Result<String, AppError> {
    Ok(Argon2::default()
        .hash_password(password.as_bytes())?
        .to_string())
}

pub fn verify_password(password: &str, hash: &str) -> Result<(), AppError> {
    Ok(Argon2::default().verify_password(password.as_bytes(), hash)?)
}

pub fn generate_session_token() -> String {
    let bytes: [u8; 32] = rand::random();
    hex::encode(bytes)
}

pub struct AuthUser {
    pub user: User,
    pub session_id: String,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

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

pub struct AdminUser(pub AuthUser);

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = AppError;

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

pub struct ActiveUser(pub AuthUser);

impl FromRequestParts<AppState> for ActiveUser {
    type Rejection = AppError;

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

async fn load_session(pool: &SqlitePool, session_id: &str) -> Result<Session, AppError> {
    sqlx::query_as::<_, Session>(
        "SELECT id, user_id, created_at, expires_at FROM sessions WHERE id = ?",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::Unauthorized)
}

async fn load_user(pool: &SqlitePool, user_id: i64) -> Result<User, AppError> {
    sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, status, role, created_at, activated_at, welcome_email_sent_at FROM users WHERE id = ?",
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
