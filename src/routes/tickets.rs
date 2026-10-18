use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use crate::common::error::AppError;
use crate::services::auth::ActiveUser;
use crate::services::state::AppState;
use crate::services::tickets::{self, RemoveOutcome, TicketRequestInput, TicketRequestView};

/// Store the ticket request parameters for a match (`204`, body ignored).
pub async fn new_ticket_request(
    State(state): State<AppState>,
    auth: ActiveUser,
    Path(match_id): Path<i64>,
    Json(input): Json<TicketRequestInput>,
) -> Result<StatusCode, AppError> {
    tickets::upsert(&state.pool, auth.0.user.id, match_id, &input).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Remove the ticket request for a match (`204`, idempotent; `409` when locked).
pub async fn delete_ticket_request(
    State(state): State<AppState>,
    auth: ActiveUser,
    Path(match_id): Path<i64>,
) -> Result<StatusCode, AppError> {
    match tickets::remove(&state.pool, auth.0.user.id, match_id).await? {
        RemoveOutcome::Removed | RemoveOutcome::NotFound => Ok(StatusCode::NO_CONTENT),
        RemoveOutcome::Locked => Err(AppError::TicketRequestLocked),
    }
}

/// List the ticket requests of the current user.
pub async fn list_requests(
    State(state): State<AppState>,
    auth: ActiveUser,
) -> Result<Json<Vec<TicketRequestView>>, AppError> {
    Ok(Json(
        tickets::list_for_user(&state.pool, auth.0.user.id).await?,
    ))
}
