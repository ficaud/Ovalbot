use axum::extract::{Path, State};
use axum::Json;

use calendar::TeamType;

use crate::common::error::AppError;
use crate::services::auth::ActiveUser;
use crate::services::calendar::{MatchView, TeamView};
use crate::services::state::AppState;

pub async fn list_teams(State(state): State<AppState>, _auth: ActiveUser) -> Json<Vec<TeamView>> {
    Json(state.calendar.list_teams())
}

pub async fn team_matches(
    State(state): State<AppState>,
    _auth: ActiveUser,
    Path(team_id): Path<String>,
) -> Result<Json<Vec<MatchView>>, AppError> {
    let team = TeamType::from_id(&team_id).ok_or(AppError::NotFound)?;
    Ok(Json(state.calendar.home_matches(team)))
}
