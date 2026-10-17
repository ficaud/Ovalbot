use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use crate::common::error::AppError;
use crate::services::auth::ActiveUser;
use crate::services::state::AppState;
use crate::services::tickets::{self, TicketRequestInput};

/// New ticket request received from the frontend.
pub async fn new_ticket_request(
    State(state): State<AppState>,
    auth: ActiveUser,
    Path(match_id): Path<i64>,
    Json(input): Json<TicketRequestInput>,
) -> Result<StatusCode, AppError> {
    // put_request returns the status code 200 if the request was successfully saved.
    tickets::put_request(&state.tickets, auth.0.user.id, match_id, input).await
}

/// Delete a ticket request from the frontend.
pub async fn delete_ticket_request(
    State(state): State<AppState>,
    auth: ActiveUser,
    Path(match_id): Path<i64>,
) -> Result<StatusCode, AppError> {
    tickets::delete_request(&state.tickets, auth.0.user.id, match_id).await
}
