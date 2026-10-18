use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

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
    #[error("ticket request can no longer be modified")]
    TicketRequestLocked,
    #[error("telegram unavailable")]
    TelegramUnavailable,
}

impl IntoResponse for AppError {
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
            AppError::TelegramUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "telegram_unavailable",
                "telegram notifications are not available",
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
            AppError::TicketRequestLocked => (
                StatusCode::CONFLICT,
                "ticket_request_locked",
                "ticket request can no longer be modified",
            ),
            AppError::Sqlx(_) | AppError::Migrate(_) | AppError::Argon2(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal error",
            ),
        };
        let body = Json(json!({ "error": code, "message": message }));
        (status, body).into_response()
    }
}
