use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use crate::auth::ActiveUser;
use crate::error::AppError;
use crate::notifications;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct SettingsRequest {
    pub email_enabled: bool,
    pub telegram_enabled: bool,
}

/// List the notifications for the user.
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
pub async fn unsubscribe(
    State(state): State<AppState>,
    auth: ActiveUser,
    Path(match_id): Path<i64>,
) -> Result<Response, AppError> {
    notifications::unsubscribe(&state.pool, auth.0.user.id, match_id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Update the notification settings for the user.
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
