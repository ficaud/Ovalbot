//! Read model over the LNR calendar: an in-memory event cache refreshed by
//! [`CalendarService::sync`], served to the API as team/match views.
//!
//! The `calendar` crate owns parsing/diffing; this service owns the cache and
//! its locking (a `std::sync::RwLock` so reads never block the tokio runtime).

use std::sync::RwLock;

use calendar::parse::{EventParser, LnrParser};
use calendar::{diff, for_team, upcoming, Change, Event, MatchStatus, TeamType};
use serde::Serialize;

/// LNR season scraped by the parser (fixes the pages fetched on the calendar site).
const SEASON: &str = "2026-2027";

/// One team as served by `GET /api/teams`.
#[derive(Debug, Serialize)]
pub struct TeamView {
    /// URL-safe slug used as the `team` path parameter (e.g. `stade-rochelais`).
    pub id: String,
    /// Human-readable display name (e.g. `Stade Rochelais`).
    pub name: String,
}

/// One match as served by `GET /api/teams/{team}/matches`.
#[derive(Debug, Serialize)]
pub struct MatchView {
    /// Stable LNR match id — also the id referenced by ticket requests and
    /// match notification subscriptions.
    pub id: u32,
    /// Home team display name / slug.
    pub team: String,
    pub team_id: String,
    /// Away team display name / slug.
    pub opponent: String,
    pub opponent_id: String,
    /// Free-form French kickoff text copied from the LNR page (may be a range
    /// for TBD fixtures, e.g. "samedi 28 novembre – dimanche 29 novembre").
    pub kickoff: String,
    /// `"scheduled"` / `"not scheduled"` / `"finished"` (raw `MatchStatus`).
    pub status: String,
}

/// In-memory calendar cache: the latest successfully synced `Vec<Event>`,
/// refreshed every 20 minutes by the background loop in `main.rs`.
///
/// All reads use a read lock so the API never waits on the bot scanner, and
/// each response works on an isolated clone.
pub struct CalendarService {
    // events are in memory cache, so that we can read them as we want without blocking the tokio runtime
    // but the writing requires a write lock (only one write at the time).
    events: RwLock<Vec<Event>>,
}

impl CalendarService {
    /// Creates the service with an empty cache (`Default::default`).
    pub fn new() -> Self {
        Self {
            // event list is empty by default
            events: RwLock::new(Vec::new()),
        }
    }

    /// Re-fetches the LNR calendar and updates the cache.
    ///
    /// The blocking fetch/parse work runs on a dedicated thread (`spawn_blocking`)
    /// so the tokio workers stay free; then the old snapshot is diffed against
    /// the new one and only `Tbd → Scheduled`-style atomic values are detected
    /// (that's [`Change`]).
    ///
    /// # Returns
    /// The list of changes between the previous and the new calendar, in
    /// calendar order — fed to `notifications::process` by the caller.
    ///
    /// # Errors
    /// Network/parse failures from the LNR fetch (opaque boxed error); the
    /// cache is left untouched in that case.
    pub async fn sync(&self) -> Result<Vec<Change>, Box<dyn std::error::Error + Send + Sync>> {
        // Event retrival and parsing is blocking so we spawn it on a separate thread.
        let events = tokio::task::spawn_blocking(|| {
            let parser = LnrParser::new(SEASON);
            parser.parse_all()
        })
        .await??;

        // Get the last calendar that has been saved in cache memory
        let old = self.events.read().expect("calendar cache poisoned").clone();

        // Diff the new calendar with the old one
        let changes = diff(&old, &events);
        // Save the new calendar in cache memory
        *self.events.write().expect("calendar cache poisoned") = events;
        Ok(changes)
    }

    /// Every of the 14 known Top 14 teams, in [`TeamType::all`] order —
    /// no database hit, the list is static.
    pub fn list_teams(&self) -> Vec<TeamView> {
        TeamType::all()
            .into_iter()
            .map(|team| TeamView {
                id: team.id().to_string(),
                name: team.to_string(),
            })
            .collect()
    }

    /// Returns a full clone of the in-memory calendar snapshot. The bot scan
    /// loop uses it to resolve encounters to match ids without extra queries.
    pub fn snapshot(&self) -> Vec<Event> {
        self.events.read().expect("calendar cache poisoned").clone()
    }

    /// Replaces the whole cache with the given events.
    ///
    /// Test-only injection point: [`CalendarService::sync`] fetches the live
    /// LNR pages, which integration tests cannot use. `#[doc(hidden)]` keeps
    /// it out of the rendered documentation; it must never be used outside
    /// test code.
    ///
    /// # Arguments
    /// * `events` — the synthetic calendar (old cache is discarded).
    #[doc(hidden)]
    pub fn seed_events(&self, events: Vec<Event>) {
        *self.events.write().expect("calendar cache poisoned") = events;
    }

    /// The team's upcoming home games mapped to [`MatchView`]: finished and
    /// away matches are filtered out (`upcoming` + `for_team`).
    ///
    /// # Arguments
    /// * `team` — the home team to list (usually `TeamType::StadeRochelais`).
    ///
    /// # Returns
    /// Matches in calendar order; empty when nothing is scheduled yet.
    pub fn home_matches(&self, team: TeamType) -> Vec<MatchView> {
        let events = self.events.read().expect("calendar cache poisoned");
        let upcoming = upcoming(&events);
        for_team(&upcoming, team, true)
            .into_iter()
            .map(|event| MatchView {
                id: event.id,
                team: event.team.name,
                team_id: event.team.team_type.id().to_string(),
                opponent: event.opponent.name,
                opponent_id: event.opponent.team_type.id().to_string(),
                kickoff: event.kickoff,
                status: status_str(event.status),
            })
            .collect()
    }
}

impl Default for CalendarService {
    /// Empty cache, same as [`CalendarService::new`].
    fn default() -> Self {
        Self::new()
    }
}

/// The API-facing spelling of a [`MatchStatus`]: `Tbd` is rendered as
/// "not scheduled" so the frontend can show TBD fixtures distinctly.
fn status_str(status: MatchStatus) -> String {
    match status {
        MatchStatus::Tbd => "not scheduled".to_string(),
        MatchStatus::Scheduled => "scheduled".to_string(),
        MatchStatus::Finished => "finished".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calendar::{Team, TeamType};

    fn team(team_type: TeamType) -> Team {
        Team {
            id: team_type.id().to_string(),
            name: team_type.to_string(),
            team_type,
        }
    }

    fn event(id: u32, home: TeamType, away: TeamType, status: MatchStatus) -> Event {
        Event {
            id,
            team: team(home),
            opponent: team(away),
            kickoff: "samedi 10 octobre 14h30".to_string(),
            status,
        }
    }

    /// `list_teams` returns all 14 known teams.
    #[test]
    fn list_teams_returns_the_fourteen_teams() {
        let service = CalendarService::new();
        let teams = service.list_teams();
        assert_eq!(teams.len(), 14);
        assert!(teams.iter().any(|t| t.id == "stade-rochelais"));
    }

    /// `seed_events` replaces the cache: the seeded ids are the ones
    /// `snapshot` then serves (and the API guard validates against).
    #[test]
    fn seed_events_replaces_the_cache() {
        let service = CalendarService::new();
        assert!(service.snapshot().is_empty());

        service.seed_events(vec![event(
            42,
            TeamType::StadeRochelais,
            TeamType::RcToulon,
            MatchStatus::Scheduled,
        )]);
        assert_eq!(
            service.snapshot().iter().map(|e| e.id).collect::<Vec<_>>(),
            [42]
        );

        // A second seed discards the first calendar entirely.
        service.seed_events(vec![]);
        assert!(service.snapshot().is_empty());
    }

    /// `home_matches` returns only the team's home games (excluding finished and
    /// away matches) and maps them to `MatchView`.
    #[test]
    fn home_matches_returns_only_home_games() {
        let service = CalendarService::new();
        *service.events.write().unwrap() = vec![
            event(
                0,
                TeamType::StadeRochelais,
                TeamType::RcToulon,
                MatchStatus::Scheduled,
            ),
            event(
                1,
                TeamType::Usap,
                TeamType::StadeRochelais,
                MatchStatus::Scheduled,
            ),
            event(
                2,
                TeamType::StadeRochelais,
                TeamType::RcVannes,
                MatchStatus::Finished,
            ),
        ];

        let matches = service.home_matches(TeamType::StadeRochelais);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].team, "Stade Rochelais");
        assert_eq!(matches[0].team_id, "stade-rochelais");
        assert_eq!(matches[0].opponent, "RC Toulon");
        assert_eq!(matches[0].opponent_id, "rc-toulon");
    }
}
