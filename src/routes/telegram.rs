use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

use crate::common::error::AppError;
use crate::services::accounts::auth::ActiveUser;
use crate::services::channels::telegram::{self, Update};
use crate::services::state::AppState;

/// `POST /api/telegram/link` — answer payload: the `t.me` deep link.
#[derive(Debug, Serialize)]
pub struct LinkResponse {
    /// Opens the bot chat pre-filled with `/start <token>` (one-shot link).
    pub url: String,
}

/// `POST /api/telegram/link` — mints a single-use 15-minute link token and
/// returns a Telegram deep link; the user pastes `/start <token>` from the bot.
///
/// # Returns
/// The [`LinkResponse`].
///
/// # Errors
/// `AppError::TelegramUnavailable` when the bot is not configured
/// (username/secret missing), DB errors otherwise.
pub async fn link(
    State(state): State<AppState>,
    auth: ActiveUser,
) -> Result<Json<LinkResponse>, AppError> {
    let username = state
        .telegram_config
        .bot_username
        .as_deref()
        .ok_or(AppError::TelegramUnavailable)?;
    let token = telegram::create_link_token(&state.pool, auth.0.user.id).await?;
    let url = telegram::deep_link(username, &token);
    Ok(Json(LinkResponse { url }))
}

/// `DELETE /api/telegram/link` — unbinds the current user's chat
/// (clears `users.telegram_id`).
///
/// # Errors
/// `AppError::Sqlx` on query failure.
pub async fn unlink(State(state): State<AppState>, auth: ActiveUser) -> Result<Response, AppError> {
    telegram::unlink(&state.pool, auth.0.user.id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `POST /api/telegram/webhook` — the PUBLIC callback Telegram pushes every
/// bot update to. Authenticated by the constant-time comparison of the
/// `X-Telegram-Bot-Api-Secret-Token` header against the configured secret.
///
/// Only `/start <token>` commands matter: a valid token binds the sending
/// chat to the token's user (`consume_link_token`) and the bot replies
/// in-chat (success/failure text). Any other update is acknowledged 200.
///
/// # Errors
/// `AppError::Unauthorized` on a bad/missing secret header,
/// `AppError::TelegramUnavailable` when no webhook secret is configured.
pub async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(update): Json<Update>,
) -> Result<StatusCode, AppError> {
    let secret = state
        .telegram_config
        .webhook_secret
        .as_deref()
        .ok_or(AppError::TelegramUnavailable)?;
    let provided = headers
        .get("x-telegram-bot-api-secret-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if !telegram::secret_matches(secret, provided) {
        return Err(AppError::Unauthorized);
    }

    if let Some((chat_id, token)) = update.start_token() {
        match telegram::consume_link_token(&state.pool, &token, &chat_id).await {
            Ok(true) => {
                let _ = state
                    .telegram
                    .send_message(&chat_id, "Notifications are now enabled.")
                    .await;
            }
            Ok(false) => {
                let _ = state
                    .telegram
                    .send_message(&chat_id, "This link is invalid or has expired.")
                    .await;
            }
            Err(e) => tracing::warn!("failed to consume telegram link token: {e}"),
        }
    }

    Ok(StatusCode::OK)
}
