use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::common::crypto::encrypt;
use crate::common::error::AppError;
use crate::services::accounts::auth::{ActiveUser, AdminUser};
use crate::services::channels::telegram;
use crate::services::notifications;
use crate::services::state::AppState;

/// Channel-preference payload for `PUT /api/notifications/settings`.
#[derive(Debug, Deserialize)]
pub struct SettingsRequest {
    pub email_enabled: bool,
    pub telegram_enabled: bool,
}

/// List the notifications for the user.
///
/// # Errors
/// `AppError::Sqlx` on query failure.
pub async fn list(
    State(state): State<AppState>,
    auth: ActiveUser,
) -> Result<Json<Vec<i64>>, AppError> {
    let ids = notifications::list_for_user(&state.pool, auth.0.user.id).await?;
    Ok(Json(ids))
}

/// Subscribe to notifications for a match.
pub async fn subscribe(
    State(state): State<AppState>,
    auth: ActiveUser,
    Path(match_id): Path<i64>,
) -> Result<Response, AppError> {
    notifications::subscribe(&state.pool, auth.0.user.id, match_id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Unsubscribe from notifications for a match.
///
/// Idempotent (204 even when nothing was subscribed).
pub async fn unsubscribe(
    State(state): State<AppState>,
    auth: ActiveUser,
    Path(match_id): Path<i64>,
) -> Result<Response, AppError> {
    notifications::unsubscribe(&state.pool, auth.0.user.id, match_id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Update the notification settings for the user.
///
/// # Errors
/// `AppError::Sqlx` on query failure.
pub async fn update_settings(
    State(state): State<AppState>,
    auth: ActiveUser,
    Json(body): Json<SettingsRequest>,
) -> Result<StatusCode, AppError> {
    notifications::update_settings(
        &state.pool,
        auth.0.user.id,
        body.email_enabled,
        body.telegram_enabled,
    )
    .await?;
    Ok(StatusCode::OK)
}

// ============================== BROADCAST CHANNEL (ADMIN) ==============================

/// Current broadcast channel, as returned by `GET /api/admin/notifications/channel`.
#[derive(Debug, Serialize)]
pub struct BroadcastResponse {
    /// The chat id, or `null` when no channel is configured.
    pub chat_id: Option<String>,
    /// Whether a bot token is stored — the token itself is never returned.
    pub has_token: bool,
}

/// Payload of `PUT /api/admin/notifications/channel`. Clearing `chat_id`
/// removes both the channel and its token; omitting `bot_token` keeps the
/// previously stored one.
#[derive(Debug, Deserialize)]
pub struct BroadcastRequest {
    pub chat_id: Option<String>,
    pub bot_token: Option<String>,
}

/// Reads the admin's Telegram broadcast channel (never the secret token).
///
/// # Errors
/// `AppError::Unauthorized`/`AppError::Forbidden` without an admin account,
/// `AppError::Sqlx` on query failure.
pub async fn get_broadcast(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<Json<BroadcastResponse>, AppError> {
    let target = telegram::broadcast_target(&state.pool).await?;
    Ok(Json(BroadcastResponse {
        chat_id: target.as_ref().map(|t| t.chat_id.clone()),
        has_token: target.is_some_and(|t| t.bot_token_cipher.is_some()),
    }))
}

/// Sets or clears the admin's Telegram broadcast channel and its dedicated
/// bot token. The token is encrypted before storage and never echoed back.
///
/// The channel is owned by the admin making the call: only their subscribed
/// alerts are mirrored there. An empty or missing `chat_id` removes it; a
/// token is required on first configuration, but can be left blank afterwards
/// to keep the stored one.
///
/// # Errors
/// `AppError::Unauthorized`/`AppError::Forbidden` without an admin account,
/// `AppError::InvalidTelegramChatId`/`AppError::InvalidTelegramBotToken` on
/// malformed input, `AppError::BroadcastBotUnavailable` when `SECRET_KEY` is
/// missing, `AppError::Sqlx` on query failure.
pub async fn set_broadcast(
    State(state): State<AppState>,
    admin: AdminUser,
    Json(body): Json<BroadcastRequest>,
) -> Result<Json<BroadcastResponse>, AppError> {
    let chat_id = match body.chat_id.as_deref() {
        Some(raw) if !raw.trim().is_empty() => {
            Some(telegram::normalize_chat_id(raw).ok_or(AppError::InvalidTelegramChatId)?)
        }
        _ => None,
    };
    let Some(chat_id) = chat_id else {
        telegram::set_broadcast_target(&state.pool, None, admin.0.user.id, None).await?;
        return Ok(Json(BroadcastResponse {
            chat_id: None,
            has_token: false,
        }));
    };

    let existing = telegram::broadcast_target(&state.pool).await?;
    let cipher = match body.bot_token.as_deref() {
        Some(raw) if !raw.trim().is_empty() => {
            let token =
                telegram::normalize_bot_token(raw).ok_or(AppError::InvalidTelegramBotToken)?;
            let key = state
                .broadcast_bot_key
                .as_ref()
                .ok_or(AppError::BroadcastBotUnavailable)?;
            Some(encrypt(key, &token)?)
        }
        _ => existing.as_ref().and_then(|t| t.bot_token_cipher.clone()),
    };
    if cipher.is_none() {
        return Err(AppError::InvalidTelegramBotToken);
    }
    telegram::set_broadcast_target(
        &state.pool,
        Some(&chat_id),
        admin.0.user.id,
        cipher.as_deref(),
    )
    .await?;
    Ok(Json(BroadcastResponse {
        chat_id: Some(chat_id),
        has_token: true,
    }))
}
