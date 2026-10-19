//! Maps a scanned billetterie encounter onto the OvalBot calendar match id.
//!
//! The billetterie identifies matches by display title ("UNION BORDEAUX
//! BÈGLES") and a free-form French date ("dimanche 25 octobre à 21h05",
//! "week-end du 28/29 novembre"), while the calendar owns the stable LNR
//! match ids that the rest of OvalBot uses (ticket_requests, notifications).
//! This module is the narrow bridge between the two: pure functions, no
//! persistence — the caller (the bot scan loop) owns logging and, at étape 2,
//! ingestion.

use calendar::{parse_window, Event, MatchStatus, TeamType};
use parser::core::encounter::{Encounter, MatchNature};

/// Reasons an encounter could not be tied to exactly one calendar match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveError {
    /// Encounter nature is not rugby (meals, parking, basketball…).
    NotRugby,
    /// The title does not name a known Top 14 club (e.g. European cup games).
    UnknownOpponent,
    /// The date could not be turned into a day window.
    UnparseableDate,
    /// No home match of that opponent overlaps the date window.
    NoCalendarMatch,
    /// Several home matches of that opponent overlap the date window.
    Ambiguous,
}

/// One-line spelling of a resolution failure, used by the scan-loop debug log.
impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            ResolveError::NotRugby => "encounter is not a rugby match",
            ResolveError::UnknownOpponent => "opponent not recognized",
            ResolveError::UnparseableDate => "date not parseable",
            ResolveError::NoCalendarMatch => "no calendar match in the date window",
            ResolveError::Ambiguous => "several calendar matches in the date window",
        };
        f.write_str(msg)
    }
}

/// Resolves an encounter to the calendar match id (LNR id) of the home game
/// against the club named by the title, when the kickoff falls inside the
/// encounter date window and the assignment is unambiguous.
///
/// Only the title names the club — billetterie events are Stade Rochelais
/// home games, so the title carries the opponent. Several overlapping
/// candidates produce an error rather than a guess.
pub fn resolve(encounter: &Encounter, events: &[Event]) -> Result<u32, ResolveError> {
    if encounter.nature != MatchNature::Rugby {
        return Err(ResolveError::NotRugby);
    }
    let opponent = TeamType::from_name(&encounter.title).ok_or(ResolveError::UnknownOpponent)?;
    let window = parse_window(&encounter.date).ok_or(ResolveError::UnparseableDate)?;

    let candidates: Vec<u32> = events
        .iter()
        .filter(|event| {
            event.team.team_type == TeamType::StadeRochelais
                && event.opponent.team_type == opponent
                && event.status != MatchStatus::Finished
                && parse_window(&event.kickoff)
                    .map(|kickoff| kickoff.overlaps(&window))
                    .unwrap_or(false)
        })
        .map(|event| event.id)
        .collect();

    match candidates[..] {
        [match_id] => Ok(match_id),
        [] => Err(ResolveError::NoCalendarMatch),
        _ => Err(ResolveError::Ambiguous),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calendar::Team;
    use parser::core::club::ClubType;

    // Builds a calendar `Team` whose id/name follow the TeamType conventions.
    fn team(team_type: TeamType) -> Team {
        Team {
            id: team_type.id().to_string(),
            name: team_type.to_string(),
            team_type,
        }
    }

    // Builds a calendar event: home = `home`, away = `away`.
    fn event(id: u32, home: TeamType, away: TeamType, kickoff: &str, status: MatchStatus) -> Event {
        Event {
            id,
            team: team(home),
            opponent: team(away),
            kickoff: kickoff.to_string(),
            status,
        }
    }

    // Builds a billetterie encounter with the given title/date/nature.
    fn encounter(title: &str, date: &str, nature: MatchNature) -> Encounter {
        Encounter::new(
            ClubType::StadeRochelais,
            title.to_string(),
            date.to_string(),
            nature,
            None,
        )
    }

    /// The five rugby titles of the billetterie page resolve to the right
    /// calendar matches, across every date shape the billetterie uses.
    #[test]
    fn resolves_each_fixture_encounter() {
        let events = [
            event(
                101,
                TeamType::StadeRochelais,
                TeamType::UnionBordeauxBegles,
                "dimanche 25 octobre 21h05",
                MatchStatus::Scheduled,
            ),
            event(
                102,
                TeamType::StadeRochelais,
                TeamType::AvironBayonnais,
                "samedi 7 novembre – dimanche 8 novembre 21h05",
                MatchStatus::Scheduled,
            ),
            event(
                103,
                TeamType::StadeRochelais,
                TeamType::Usap,
                "samedi 28 novembre – dimanche 29 novembre",
                MatchStatus::Tbd,
            ),
            event(
                104,
                TeamType::StadeRochelais,
                TeamType::CastresOlympique,
                "dimanche 20 décembre 15h15",
                MatchStatus::Tbd,
            ),
            event(
                105,
                TeamType::StadeRochelais,
                TeamType::SectionPalois,
                "samedi 2 janvier – dimanche 3 janvier",
                MatchStatus::Tbd,
            ),
        ];

        assert_eq!(
            resolve(
                &encounter(
                    "UNION BORDEAUX BÈGLES",
                    "dimanche 25 octobre à 21h05",
                    MatchNature::Rugby
                ),
                &events
            ),
            Ok(101)
        );
        assert_eq!(
            resolve(
                &encounter(
                    "AVIRON BAYONNAIS",
                    "dimanche 8 novembre à 21h05",
                    MatchNature::Rugby
                ),
                &events
            ),
            Ok(102)
        );
        assert_eq!(
            resolve(
                &encounter(
                    "USA PERPIGNAN",
                    "week-end du 28/29 novembre",
                    MatchNature::Rugby
                ),
                &events
            ),
            Ok(103)
        );
        assert_eq!(
            resolve(
                &encounter(
                    "CASTRES OLYMPIQUE",
                    "week-end du 19/20 décembre",
                    MatchNature::Rugby
                ),
                &events
            ),
            Ok(104)
        );
        assert_eq!(
            resolve(
                &encounter(
                    "SECTION PALOISE",
                    "week-end du 02/03 janvier",
                    MatchNature::Rugby
                ),
                &events
            ),
            Ok(105)
        );
    }

    /// Non-rugby events (meals, parking…) are never resolved, even when their
    /// title aliases the club ("BORDEAUX🍴REPAS D'AVANT-MATCH") and their date
    /// falls inside the real match's window.
    #[test]
    fn rejects_non_rugby_natures() {
        let events = [event(
            101,
            TeamType::StadeRochelais,
            TeamType::UnionBordeauxBegles,
            "dimanche 25 octobre 21h05",
            MatchStatus::Scheduled,
        )];
        assert_eq!(
            resolve(
                &encounter(
                    "BORDEAUX🍴REPAS D'AVANT-MATCH",
                    "dimanche 25 octobre dès 19h00",
                    MatchNature::Other
                ),
                &events
            ),
            Err(ResolveError::NotRugby)
        );
    }

    /// European cup opponents (e.g. Connacht) are not in the Top 14 calendar —
    /// the alias lookup fails and the encounter is reported as unknown.
    #[test]
    fn rejects_unknown_opponents() {
        let encounter = encounter(
            "CONNACHT RUGBY",
            "samedi 16 janvier à 18h30",
            MatchNature::Rugby,
        );
        assert_eq!(
            resolve(
                &encounter,
                &[event(
                    106,
                    TeamType::StadeRochelais,
                    TeamType::RcToulon,
                    "samedi 16 janvier 18h30",
                    MatchStatus::Scheduled
                )]
            ),
            Err(ResolveError::UnknownOpponent)
        );
    }

    /// An unparseable date cannot be matched, whatever the opponent.
    #[test]
    fn rejects_unparseable_dates() {
        let encounter = encounter("CASTRES OLYMPIQUE", "", MatchNature::Rugby);
        assert_eq!(
            resolve(
                &encounter,
                &[event(
                    104,
                    TeamType::StadeRochelais,
                    TeamType::CastresOlympique,
                    "dimanche 20 décembre 15h15",
                    MatchStatus::Scheduled
                )]
            ),
            Err(ResolveError::UnparseableDate)
        );
    }

    /// Away matches never match (the billetterie only lists home games),
    /// other opponents never match, finished matches never match, and a date
    /// window outside the kickoff finds nothing.
    #[test]
    fn rejects_non_matching_events() {
        let encounter = encounter(
            "UNION BORDEAUX BÈGLES",
            "dimanche 25 octobre à 21h05",
            MatchNature::Rugby,
        );

        let away = event(
            999,
            TeamType::UnionBordeauxBegles,
            TeamType::StadeRochelais,
            "dimanche 25 octobre 21h05",
            MatchStatus::Scheduled,
        );
        assert_eq!(
            resolve(&encounter, &[away]),
            Err(ResolveError::NoCalendarMatch)
        );

        let other_opponent = event(
            998,
            TeamType::StadeRochelais,
            TeamType::RcVannes,
            "dimanche 25 octobre 21h05",
            MatchStatus::Scheduled,
        );
        assert_eq!(
            resolve(&encounter, &[other_opponent]),
            Err(ResolveError::NoCalendarMatch)
        );

        let finished = event(
            997,
            TeamType::StadeRochelais,
            TeamType::UnionBordeauxBegles,
            "dimanche 25 octobre 21h05",
            MatchStatus::Finished,
        );
        assert_eq!(
            resolve(&encounter, &[finished]),
            Err(ResolveError::NoCalendarMatch)
        );

        let wrong_dates = event(
            996,
            TeamType::StadeRochelais,
            TeamType::UnionBordeauxBegles,
            "dimanche 1 novembre 21h05",
            MatchStatus::Scheduled,
        );
        assert_eq!(
            resolve(&encounter, &[wrong_dates]),
            Err(ResolveError::NoCalendarMatch)
        );
    }

    /// Two home matches of the same opponent overlapping the window is a hard
    /// stop: the assignment stays ambiguous rather than guessed.
    #[test]
    fn refuses_ambiguous_matches() {
        let encounter = encounter(
            "UNION BORDEAUX BÈGLES",
            "week-end du 24/25 octobre",
            MatchNature::Rugby,
        );
        let events = [
            event(
                101,
                TeamType::StadeRochelais,
                TeamType::UnionBordeauxBegles,
                "samedi 24 octobre 21h05",
                MatchStatus::Scheduled,
            ),
            event(
                107,
                TeamType::StadeRochelais,
                TeamType::UnionBordeauxBegles,
                "dimanche 25 octobre 21h05",
                MatchStatus::Scheduled,
            ),
        ];
        assert_eq!(resolve(&encounter, &events), Err(ResolveError::Ambiguous));
    }
}
