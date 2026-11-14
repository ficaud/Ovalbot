use axum::extract::{Path, State};
use axum::Json;

use calendar::TeamType;

use crate::common::error::AppError;
use crate::services::accounts::auth::ActiveUser;
use crate::services::calendar::{MatchView, MatchViewStats, TeamView};
use crate::services::state::AppState;
use crate::services::tickets;

/// `GET /api/teams` — the 14 Top 14 teams (id slug + display name) for the
/// team pickers.
///
/// # Errors
/// None on the fast path; the extractor rejects anonymous users upfront.
pub async fn list_teams(State(state): State<AppState>, _auth: ActiveUser) -> Json<Vec<TeamView>> {
    Json(state.calendar.list_teams())
}

/// `GET /api/teams/{team}/matches` — the team's upcoming home games (the site's
/// core tiles: flag a resale or a want for any of these), enriched with the
/// bot's resale statistics per match (`stats` is `None` when the bot has never
/// seen the match's resale).
///
/// # Errors
/// `AppError::NotFound` when the slug is not a known team id; `AppError::Sqlx`
/// when the statistics query fails.
pub async fn team_matches(
    State(state): State<AppState>,
    _auth: ActiveUser,
    Path(team_id): Path<String>,
) -> Result<Json<Vec<MatchView>>, AppError> {
    let team = TeamType::from_id(&team_id).ok_or(AppError::NotFound)?;
    let mut matches = state.calendar.home_matches(team);

    // Zip the calendar with the durable seat counts: the scan only records
    // offers for encounters resolved to an LNR match id, so the keys align.
    let seen = tickets::seats_seen_counts(&state.pool).await?;
    for view in &mut matches {
        if let Some(count) = seen.get(&(view.id as i64)) {
            view.stats = Some(MatchViewStats { seats_seen: *count });
        }
    }
    Ok(Json(matches))
}
