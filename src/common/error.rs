use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// Domain errors for every backend operation, mapped in [`IntoResponse`] to a
/// single JSON error shape `{ "error": code, "message": text }`.
///
/// Third-party errors wrapped with `#[from]` (`Sqlx`, `Migrate`, `Argon2`) are
/// internal: they always collapse into a generic `internal` 500 so their
/// details never leak to the client. Every other variant is user-facing and
/// carries its own status code and stable error code.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database error")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("password hash error")]
    Argon2(#[from] argon2::password_hash::Error),
    #[error("invalid email")]
    InvalidEmail,
    #[error("invalid password")]
    InvalidPassword,
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("email already registered")]
    EmailTaken,
    #[error("not authenticated")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("invalid ticket request")]
    InvalidTicketRequest,
    #[error("ticket request for a match the bot does not cover")]
    UnsupportedTicketRequest,
    #[error("ticket request can no longer be modified")]
    TicketRequestLocked,
    #[error("invalid shop credentials")]
    InvalidShopCredentials,
    #[error("invalid feedback report")]
    InvalidFeedback,
    #[error("shop credentials storage unavailable")]
    ShopCredentialsUnavailable,
    #[error("crypto error")]
    Crypto(#[from] crate::common::crypto::CryptoError),
    #[error("too many attempts")]
    TooManyAttempts(u64),
    #[error("telegram unavailable")]
    TelegramUnavailable,
    #[error("invalid telegram chat id")]
    InvalidTelegramChatId,
    #[error("invalid telegram bot token")]
    InvalidTelegramBotToken,
    #[error("broadcast bot token storage unavailable")]
    BroadcastBotUnavailable,
}

impl IntoResponse for AppError {
    /// Builds the JSON error response `{ error, message }` with the status the
    /// variant maps to; `# Panics`: none.
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            AppError::InvalidCredentials => (
                StatusCode::UNAUTHORIZED,
                "invalid_credentials",
                "invalid credentials",
            ),
            AppError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "not authenticated",
            ),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden", "forbidden"),
            AppError::NotFound => (StatusCode::NOT_FOUND, "not_found", "not found"),
            AppError::TooManyAttempts(_) => (
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_attempts",
                "too many attempts, try again later",
            ),
            AppError::TelegramUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "telegram_unavailable",
                "telegram notifications are not available",
            ),
            AppError::InvalidTelegramChatId => (
                StatusCode::BAD_REQUEST,
                "invalid_telegram_chat_id",
                "invalid telegram chat id",
            ),
            AppError::InvalidTelegramBotToken => (
                StatusCode::BAD_REQUEST,
                "invalid_telegram_bot_token",
                "invalid telegram bot token",
            ),
            AppError::BroadcastBotUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "broadcast_bot_unavailable",
                "broadcast bot token cannot be stored without SECRET_KEY",
            ),
            AppError::EmailTaken => (
                StatusCode::CONFLICT,
                "email_taken",
                "email already registered",
            ),
            AppError::InvalidEmail => (StatusCode::BAD_REQUEST, "invalid_email", "invalid email"),
            AppError::InvalidPassword => (
                StatusCode::BAD_REQUEST,
                "invalid_password",
                "invalid password",
            ),
            AppError::InvalidTicketRequest => (
                StatusCode::BAD_REQUEST,
                "invalid_ticket_request",
                "invalid ticket request",
            ),
            AppError::UnsupportedTicketRequest => (
                StatusCode::BAD_REQUEST,
                "unsupported_ticket_request",
                "the bot only covers Stade Rochelais home games",
            ),
            AppError::TicketRequestLocked => (
                StatusCode::CONFLICT,
                "ticket_request_locked",
                "ticket request can no longer be modified",
            ),
            AppError::InvalidShopCredentials => (
                StatusCode::BAD_REQUEST,
                "invalid_shop_credentials",
                "invalid shop credentials",
            ),
            AppError::InvalidFeedback => (
                StatusCode::BAD_REQUEST,
                "invalid_feedback",
                "invalid feedback report",
            ),
            AppError::ShopCredentialsUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "shop_credentials_unavailable",
                "shop credentials storage is not available (set SECRET_KEY)",
            ),
            AppError::Sqlx(_)
            | AppError::Migrate(_)
            | AppError::Argon2(_)
            | AppError::Crypto(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal error",
            ),
        };
        let body = Json(json!({ "error": code, "message": message }));
        let mut response = (status, body).into_response();
        // The rate limiter's variant carries its own wait hint (seconds).
        if let AppError::TooManyAttempts(retry_after) = self {
            if let Ok(value) = retry_after.to_string().parse() {
                response
                    .headers_mut()
                    .insert(axum::http::header::RETRY_AFTER, value);
            }
        }
        response
    }
}
