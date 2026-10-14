use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

use crate::auth::ActiveUser;
use crate::error::AppError;
use crate::state::AppState;
use crate::telegram::{self, Update};

#[derive(Debug, Serialize)]
pub struct LinkResponse {
    pub url: String,
}

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

pub async fn unlink(State(state): State<AppState>, auth: ActiveUser) -> Result<Response, AppError> {
    telegram::unlink(&state.pool, auth.0.user.id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

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
