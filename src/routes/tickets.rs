use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use crate::common::error::AppError;
use crate::services::accounts::auth::ActiveUser;
use crate::services::bot::matching;
use crate::services::state::AppState;
use crate::services::tickets::{
    self, RemoveOutcome, StatusOutcome, TicketRequestInput, TicketRequestView,
};

/// Store the ticket request parameters for a match (`204`, body ignored).
///
/// The bot only scrapes the Stade Rochelais billetterie (upcoming home
/// games), so the calendar must know the match as one of those before the
/// request is stored — other ids are refused (`400`).
///
/// # Errors
/// `AppError::UnsupportedTicketRequest` when the calendar snapshot does not
/// know `match_id` as an upcoming Stade Rochelais home game; service errors
/// from `upsert` otherwise (`400` invalid values, `409` locked).
pub async fn new_ticket_request(
    State(state): State<AppState>,
    auth: ActiveUser,
    Path(match_id): Path<i64>,
    Json(input): Json<TicketRequestInput>,
) -> Result<StatusCode, AppError> {
    // Fail-closed: while the calendar cache is empty nothing is accepted.
    if !matching::is_bot_covered_match(&state.calendar.snapshot(), match_id) {
        return Err(AppError::UnsupportedTicketRequest);
    }
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

/// Confirm that the user paid for an `accepted` request (`204`).
///
/// Idempotent on an already `purchased` request. Any other status is refused
/// with `409`; an unknown request answers `404`.
///
/// # Errors
/// `AppError::TicketRequestLocked` on an invalid transition,
/// `AppError::NotFound` when the request does not exist, service errors
/// otherwise (`500`).
pub async fn mark_request_purchased(
    State(state): State<AppState>,
    auth: ActiveUser,
    Path(match_id): Path<i64>,
) -> Result<StatusCode, AppError> {
    match tickets::mark_purchased(&state.pool, auth.0.user.id, match_id).await? {
        StatusOutcome::Moved | StatusOutcome::Already => Ok(StatusCode::NO_CONTENT),
        StatusOutcome::Refused => Err(AppError::TicketRequestLocked),
        StatusOutcome::NotFound => Err(AppError::NotFound),
    }
}

/// Reopen an `accepted`/`failed` request for the bot (`204`).
///
/// Releases the seats the request had claimed and returns it to `pending`.
/// Idempotent on an already `pending` request; `purchased` is refused (`409`);
/// an unknown request answers `404`.
///
/// # Errors
/// `AppError::TicketRequestLocked` on an invalid transition,
/// `AppError::NotFound` when the request does not exist, service errors
/// otherwise (`500`).
pub async fn reset_request(
    State(state): State<AppState>,
    auth: ActiveUser,
    Path(match_id): Path<i64>,
) -> Result<StatusCode, AppError> {
    match tickets::reset(&state.pool, auth.0.user.id, match_id).await? {
        StatusOutcome::Moved | StatusOutcome::Already => Ok(StatusCode::NO_CONTENT),
        StatusOutcome::Refused => Err(AppError::TicketRequestLocked),
        StatusOutcome::NotFound => Err(AppError::NotFound),
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
