use std::sync::RwLock;

use calendar::parse::{EventParser, LnrParser};
use calendar::{diff, for_team, upcoming, Change, Event, MatchStatus, TeamType};
use serde::Serialize;

const SEASON: &str = "2026-2027";

#[derive(Debug, Serialize)]
pub struct TeamView {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct MatchView {
    pub id: u32,
    pub team: String,
    pub team_id: String,
    pub opponent: String,
    pub opponent_id: String,
    pub kickoff: String,
    pub status: String,
}

pub struct CalendarService {
    // events are in memory cache, so that we can read them as we want without blocking the tokio runtime
    // but the writing requires a write lock (only one write at the time).
    events: RwLock<Vec<Event>>,
}

impl CalendarService {
    pub fn new() -> Self {
        Self {
            // event list is empty by default
            events: RwLock::new(Vec::new()),
        }
    }

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

    pub fn list_teams(&self) -> Vec<TeamView> {
        TeamType::all()
            .into_iter()
            .map(|team| TeamView {
                id: team.id().to_string(),
                name: team.to_string(),
            })
            .collect()
    }

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
    fn default() -> Self {
        Self::new()
    }
}

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
