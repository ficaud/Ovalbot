use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::auth::ActiveUser;
use crate::error::AppError;
use crate::notifications;
use crate::state::AppState;

pub async fn list(
    State(state): State<AppState>,
    auth: ActiveUser,
) -> Result<Json<Vec<i64>>, AppError> {
    let ids = notifications::list_for_user(&state.pool, auth.0.user.id).await?;
    Ok(Json(ids))
}

pub async fn subscribe(
    State(state): State<AppState>,
    auth: ActiveUser,
    Path(match_id): Path<i64>,
) -> Result<Response, AppError> {
    notifications::subscribe(&state.pool, auth.0.user.id, match_id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn unsubscribe(
    State(state): State<AppState>,
    auth: ActiveUser,
    Path(match_id): Path<i64>,
) -> Result<Response, AppError> {
    notifications::unsubscribe(&state.pool, auth.0.user.id, match_id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
