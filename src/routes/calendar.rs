use axum::extract::{Path, State};
use axum::Json;

use calendar::TeamType;

use crate::common::error::AppError;
use crate::services::accounts::auth::ActiveUser;
use crate::services::calendar::{MatchView, TeamView};
use crate::services::state::AppState;

/// `GET /api/teams` — the 14 Top 14 teams (id slug + display name) for the
/// team pickers.
///
/// # Errors
/// None on the fast path; the extractor rejects anonymous users upfront.
pub async fn list_teams(State(state): State<AppState>, _auth: ActiveUser) -> Json<Vec<TeamView>> {
    Json(state.calendar.list_teams())
}

/// `GET /api/teams/{team}/matches` — the team's upcoming home games (the site's
/// core tiles: flag a resale or a want for any of these).
///
/// # Errors
/// `AppError::NotFound` when the slug is not a known team id.
pub async fn team_matches(
    State(state): State<AppState>,
    _auth: ActiveUser,
    Path(team_id): Path<String>,
) -> Result<Json<Vec<MatchView>>, AppError> {
    let team = TeamType::from_id(&team_id).ok_or(AppError::NotFound)?;
    Ok(Json(state.calendar.home_matches(team)))
}
