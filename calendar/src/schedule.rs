//! Pure queries over a parsed set of events: keep upcoming games, filter by
//! team, and diff two snapshots to detect scheduling changes. None of these
//! functions touch the network or parse anything again.

use std::collections::HashMap;

use crate::event::MatchStatus;
use crate::team::TeamType;
use crate::Event;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Scheduled {
        id: u16,
        team: TeamType,
        opponent: TeamType,
        kickoff: String,
    },
}

/// Matches that are not finished, sorted by id.
pub fn upcoming(events: &[Event]) -> Vec<Event> {
    let mut result: Vec<Event> = events
        .iter()
        .filter(|event| event.status != MatchStatus::Finished)
        .cloned()
        .collect();
    result.sort_by_key(|event| event.id);
    result
}

/// Matches where the given team plays at home (`is_home = true`) or away (`false`), sorted by id.
pub fn for_team(events: &[Event], team: TeamType, is_home: bool) -> Vec<Event> {
    let mut result: Vec<Event> = events
        .iter()
        .filter(|event| {
            if is_home {
                event.team.team_type == team
            } else {
                event.opponent.team_type == team
            }
        })
        .cloned()
        .collect();
    result.sort_by_key(|event| event.id);
    result
}

/// Changes between two snapshots: matches that moved from Tbd to Scheduled.
pub fn diff(old: &[Event], new: &[Event]) -> Vec<Change> {
    let old_status: HashMap<u16, MatchStatus> =
        old.iter().map(|event| (event.id, event.status)).collect();

    new.iter()
        .filter(|event| event.status == MatchStatus::Scheduled)
        .filter(|event| old_status.get(&event.id) == Some(&MatchStatus::Tbd))
        .map(|event| Change::Scheduled {
            id: event.id,
            team: event.team.team_type,
            opponent: event.opponent.team_type,
            kickoff: event.kickoff.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{diff, for_team, upcoming, Change};
    use crate::event::MatchStatus;
    use crate::team::{Team, TeamType};
    use crate::Event;

    fn team(team_type: TeamType) -> Team {
        Team {
            id: String::new(),
            name: team_type.to_string(),
            team_type,
        }
    }

    fn event(id: u16, team_type: TeamType, opponent: TeamType, status: MatchStatus) -> Event {
        Event {
            id,
            team: team(team_type),
            opponent: team(opponent),
            kickoff: String::new(),
            status,
        }
    }

    /// `upcoming` drops finished matches and sorts the remaining ones by id.
    #[test]
    fn upcoming_excludes_finished_and_sorts() {
        let events = vec![
            event(
                2,
                TeamType::StadeRochelais,
                TeamType::RcToulon,
                MatchStatus::Scheduled,
            ),
            event(
                0,
                TeamType::StadeRochelais,
                TeamType::RcVannes,
                MatchStatus::Finished,
            ),
            event(
                1,
                TeamType::StadeRochelais,
                TeamType::Usap,
                MatchStatus::Tbd,
            ),
        ];
        let result = upcoming(&events);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, 1);
        assert_eq!(result[1].id, 2);
    }

    /// `for_team` returns only home matches when `is_home` is true, and only away
    /// matches when it is false.
    #[test]
    fn for_team_filters_home_and_away() {
        let events = vec![
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
                TeamType::RcVannes,
                TeamType::RcToulon,
                MatchStatus::Scheduled,
            ),
        ];

        let home = for_team(&events, TeamType::StadeRochelais, true);
        assert_eq!(home.len(), 1);
        assert_eq!(home[0].id, 0);

        let away = for_team(&events, TeamType::StadeRochelais, false);
        assert_eq!(away.len(), 1);
        assert_eq!(away[0].id, 1);
    }

    /// `diff` reports a `Change` for a match that moved from `Tbd` to `Scheduled`.
    #[test]
    fn diff_detects_tbd_to_scheduled() {
        let old = vec![
            event(
                0,
                TeamType::StadeRochelais,
                TeamType::RcToulon,
                MatchStatus::Tbd,
            ),
            event(
                1,
                TeamType::StadeRochelais,
                TeamType::Usap,
                MatchStatus::Scheduled,
            ),
        ];
        let new = vec![
            event(
                0,
                TeamType::StadeRochelais,
                TeamType::RcToulon,
                MatchStatus::Scheduled,
            ),
            event(
                1,
                TeamType::StadeRochelais,
                TeamType::Usap,
                MatchStatus::Scheduled,
            ),
        ];
        let changes = diff(&old, &new);
        assert_eq!(changes.len(), 1);
        assert_eq!(
            changes[0],
            Change::Scheduled {
                id: 0,
                team: TeamType::StadeRochelais,
                opponent: TeamType::RcToulon,
                kickoff: String::new(),
            }
        );
    }

    /// `diff` returns nothing when the two snapshots are identical.
    #[test]
    fn diff_ignores_unchanged_statuses() {
        let old = vec![
            event(
                0,
                TeamType::StadeRochelais,
                TeamType::RcToulon,
                MatchStatus::Tbd,
            ),
            event(
                1,
                TeamType::StadeRochelais,
                TeamType::Usap,
                MatchStatus::Scheduled,
            ),
        ];
        let new = old.clone();

        let changes = diff(&old, &new);
        assert!(changes.is_empty());
    }

    /// `diff` ignores newly-added matches that had no previous `Tbd` status.
    #[test]
    fn diff_ignores_new_matches_without_previous_tbd() {
        let old: Vec<Event> = Vec::new();
        let new = vec![event(
            0,
            TeamType::StadeRochelais,
            TeamType::RcToulon,
            MatchStatus::Scheduled,
        )];

        let changes = diff(&old, &new);
        assert!(changes.is_empty());
    }
}
