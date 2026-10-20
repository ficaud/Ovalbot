//! Background ticket resale scanner (the bot).
//!
//! Runs a loop that scans the Stade Rochelais billetterie at a fixed
//! interval, resolves every encounter against the calendar snapshot
//! ([`crate::services::matching`]) and diffs the seats on resale between
//! scans for the terminal logs. Every seat of a resolved encounter is also
//! recorded durably in `ticket_offers`
//! ([`crate::services::tickets::record_offers`]): only genuinely new seats
//! trigger the seats alert, fanned out to the pending `ticket_requests`
//! requesters through [`crate::services::notifications::notify_new_seats`]
//! (email and/or Telegram).

use std::collections::HashSet;
use std::time::Duration;

use calendar::Event;
use parser::core::encounter::{Encounter, MatchNature};
use parser::core::seat::Seat;
use parser::interface::club_manager;
use parser::interface::curl::proxy::set_proxy_enabled;
use scanner::app::scan::run_scan;

use crate::services::mailer::{SeatAlert, SeatBrief};
use crate::services::matching;
use crate::services::notifications;
use crate::services::state::AppState;
use crate::services::tickets;

// ============================== SCAN LOOP ==============================

/// Starts the ticket resale scanner loop (the bot). Disabled by default; enable
/// with `BOT_ENABLED=true`, tune the interval with `BOT_SCAN_INTERVAL_SECS`.
/// Proxies are off unless `PROXY_ENABLED=true` (and `PROXIES_FILE` set).
pub fn spawn(state: AppState) {
    if !env_flag("BOT_ENABLED", true) {
        tracing::info!("bot scan disabled (set BOT_ENABLED=true to enable)");
        return;
    }

    set_proxy_enabled(env_flag("PROXY_ENABLED", false));
    let club = club_manager::stade_rochelais();
    let interval_secs = env_u64("BOT_SCAN_INTERVAL_SECS", 60);

    tracing::info!("bot scan enabled: every {interval_secs}s");
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
        // Seat keys seen during the previous scan, to only log what changed.
        let mut previous: HashSet<SeatKey> = HashSet::new();
        loop {
            interval.tick().await;
            let encounters = run_scan(club.clone(), MatchNature::Rugby).await;
            // Resolve every encounter against the calendar snapshot: it is
            // populated by the first sync, started before this loop.
            let events = state.calendar.snapshot();
            // One resolution pass shared by both consumers below.
            let resolutions = resolve_encounters(&encounters, &events);
            let current = log_scanned_encounters(&encounters, &resolutions, &previous);
            notify_fresh_offers(&state, &encounters, &resolutions).await;
            previous = current;
        }
    });
}

// ============================== ENV HELPERS ==============================

/// Reads a boolean flag from the environment (`1`/`true`/`0`/`false`, case-insensitive).
fn env_flag(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| match v.trim().to_ascii_lowercase().as_str() {
            "1" | "true" => true,
            "0" | "false" => false,
            _ => default,
        })
        .unwrap_or(default)
}

/// Reads a non-negative integer from the environment.
fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

// ============================== SEAT DIFF ==============================

/// Uniquely identifies a resale seat across scans. The encounter is identified
/// by its title + date (the only stable identifiers on the ticketing site —
/// `Encounter.id` is a volatile counter), the seat by its site-native id.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct SeatKey {
    title: String,
    date: String,
    pack_id: u64,
    ticket_id: String,
}

/// Builds the diff key of one seat of an encounter.
fn encounter_seat_key(encounter: &Encounter, seat: &Seat) -> SeatKey {
    SeatKey {
        title: encounter.title.clone(),
        date: encounter.date.clone(),
        pack_id: seat.id.pack_id,
        ticket_id: seat.id.ticket_id.clone(),
    }
}

/// Collects the keys of every seat currently on resale (encounters without
/// seats contribute none).
fn collect_seat_keys(encounters: &[Encounter]) -> HashSet<SeatKey> {
    encounters
        .iter()
        .flat_map(|encounter| {
            encounter
                .seats
                .iter()
                .flatten()
                .map(|seat| encounter_seat_key(encounter, seat))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Logs the results of a bot scan, diffed against the previous one: the info
/// summary is emitted on every scan and the per-seat details are only logged
/// for seats that were not seen in the previous scan (their `event_id` comes
/// from the precomputed `resolutions`, aligned with `encounters`).
///
/// Returns the full set of current seat keys, to be passed back as `previous`
/// on the next iteration.
fn log_scanned_encounters(
    encounters: &[Encounter],
    resolutions: &[Option<u32>],
    previous: &HashSet<SeatKey>,
) -> HashSet<SeatKey> {
    let current = collect_seat_keys(encounters);
    let previous_seen = previous.intersection(&current).count();
    let new_seats = current.len() - previous_seen;

    tracing::info!(
        "bot scan: {} encounter(s), {} seat(s) on resale, {} new since last scan",
        encounters.len(),
        current.len(),
        new_seats
    );

    for (encounter, event_id) in encounters.iter().zip(resolutions.iter().copied()) {
        let Some(seats) = encounter.seats.as_ref().filter(|seats| !seats.is_empty()) else {
            continue;
        };
        for seat in seats {
            let key = encounter_seat_key(encounter, seat);
            if previous.contains(&key) {
                continue;
            }
            tracing::info!(
                event_id = ?event_id,
                title = %encounter.title,
                date = %encounter.date,
                category = %seat.seat_info.composition.category,
                full_name = %seat.seat_info.full_name,
                price = %seat.price,
                pack_id = seat.id.pack_id,
                ticket_id = %seat.id.ticket_id,
                "bot scan: new seat on resale"
            );
        }
    }

    current
}

// ============================== NEW SEATS ALERTS ==============================

/// Resolves every encounter against the calendar snapshot, once per scan so
/// the seat-log pass and the seats-alert pass share the same results.
///
/// Unresolvable encounters (non-rugby, unknown opponent, unparseable date, no
/// or ambiguous calendar match) log at debug and yield `None`, keeping index
/// alignment with `encounters`.
///
/// # Returns
/// One entry per encounter, in order: `Some(match_id)` when the encounter is
/// tied to exactly one calendar match, `None` otherwise.
fn resolve_encounters(encounters: &[Encounter], events: &[Event]) -> Vec<Option<u32>> {
    encounters
        .iter()
        .map(|encounter| match matching::resolve(encounter, events) {
            Ok(match_id) => Some(match_id),
            Err(e) => {
                tracing::debug!(
                    title = %encounter.title,
                    date = %encounter.date,
                    error = %e,
                    "bot scan: encounter not resolved"
                );
                None
            }
        })
        .collect()
}

/// Records a resolved encounter's resale seats in `ticket_offers` and alerts
/// the pending requesters about the seats that only just appeared.
///
/// The durable table is the trigger: a seat already recorded by an earlier
/// scan (or an earlier process lifetime) is silently ignored, so a requester
/// is alerted exactly once per seat. Seat/dB failures warn and continue —
/// one broken encounter cannot skip the others.
async fn notify_fresh_offers(
    state: &AppState,
    encounters: &[Encounter],
    resolutions: &[Option<u32>],
) {
    for (encounter, event_id) in encounters.iter().zip(resolutions.iter().copied()) {
        let Some(match_id) = event_id else {
            continue;
        };
        let Some(seats) = encounter.seats.as_ref().filter(|seats| !seats.is_empty()) else {
            continue;
        };

        let fresh = match tickets::record_offers(
            &state.pool,
            i64::from(match_id),
            &encounter.title,
            &encounter.date,
            seats,
        )
        .await
        {
            Ok(fresh) => fresh,
            Err(e) => {
                tracing::warn!("failed to record ticket offers for match {match_id}: {e}");
                continue;
            }
        };
        if fresh.is_empty() {
            continue;
        }

        // Keep only the seats this call actually inserted: they are the alert's
        // subject; the ones before were already announced (or re-announced).
        let fresh_keys: Vec<(u64, String)> = fresh
            .iter()
            .map(|key| (key.pack_id, key.ticket_id.clone()))
            .collect();
        let alert = SeatAlert {
            opponent: encounter.title.clone(),
            date: encounter.date.clone(),
            seats: seats
                .iter()
                .filter(|seat| fresh_keys.contains(&(seat.id.pack_id, seat.id.ticket_id.clone())))
                .map(|seat| SeatBrief {
                    category: seat.seat_info.composition.category.clone(),
                    price: seat.price.clone(),
                })
                .collect(),
        };
        notifications::notify_new_seats(state, match_id, &alert).await;
    }
}

// ============================== TESTS ==============================

#[cfg(test)]
mod env_helpers_tests {
    use super::*;

    #[test]
    fn env_flag_parses_variants() {
        let name = "OVB_TEST_ENV_FLAG_A";
        std::env::set_var(name, "1");
        assert!(env_flag(name, false));
        std::env::set_var(name, "TRUE");
        assert!(env_flag(name, false));
        std::env::set_var(name, "0");
        assert!(!env_flag(name, true));
        std::env::set_var(name, "false");
        assert!(!env_flag(name, true));
        std::env::set_var(name, " maybe ");
        // Invalid values fall back to the default.
        assert!(env_flag(name, true));
        std::env::remove_var(name);
        assert!(!env_flag(name, false));
    }

    #[test]
    fn env_u64_parses_or_falls_back_to_default() {
        let name = "OVB_TEST_ENV_U64_B";
        std::env::set_var(name, "42");
        assert_eq!(env_u64(name, 7), 42);
        std::env::set_var(name, " 60 ");
        assert_eq!(env_u64(name, 7), 60);
        std::env::set_var(name, "not-a-number");
        assert_eq!(env_u64(name, 7), 7);
        std::env::set_var(name, "-3");
        assert_eq!(env_u64(name, 7), 7);
        std::env::remove_var(name);
        assert_eq!(env_u64(name, 7), 7);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calendar::{Event, MatchStatus, Team, TeamType};
    use parser::core::club::ClubType;
    use parser::core::seat::{Seat, SeatComposition, SeatId, SeatInfo};

    fn seat(pack_id: u64, ticket_id: &str, price: &str) -> Seat {
        Seat {
            id: SeatId {
                pack_id,
                ticket_id: ticket_id.to_string(),
            },
            seat_info: SeatInfo {
                full_name: format!("Pelouse - Rangée 1, Siège {ticket_id}"),
                composition: SeatComposition {
                    category: "Pelouse".to_string(),
                    bloc: String::new(),
                    row: "1".to_string(),
                    seat_number: pack_id,
                },
            },
            price: price.to_string(),
        }
    }

    fn encounter_with_seats(title: &str, date: &str, seats: Vec<Seat>) -> Encounter {
        let mut encounter = Encounter::new(
            ClubType::StadeRochelais,
            title.to_string(),
            date.to_string(),
            MatchNature::Rugby,
            None,
        );
        encounter.set_seats(seats);
        encounter
    }

    // Builds a calendar `Team` whose id/name follow the TeamType conventions.
    fn team(team_type: TeamType) -> Team {
        Team {
            id: team_type.id().to_string(),
            name: team_type.to_string(),
            team_type,
        }
    }

    // Key type: seat keys must be stable across scans.
    #[test]
    fn collect_seat_keys_is_stable_across_identical_scans() {
        let encounters = vec![
            encounter_with_seats(
                "AVIRON BAYONNAIS",
                "dimanche 8 novembre à 21h05",
                vec![seat(1001, "42", "45.00 €"), seat(1001, "43", "45.00 €")],
            ),
            encounter_with_seats(
                "SECTION PALOISE",
                "week-end du 02/03 janvier",
                vec![seat(2002, "7", "20.00 €")],
            ),
        ];

        let first = log_scanned_encounters(&encounters, &[None, None], &HashSet::new());
        assert_eq!(first.len(), 3);

        // Identical scan: nothing new, same state returned.
        let second = log_scanned_encounters(&encounters, &[None, None], &first);
        assert_eq!(second, first);
        assert_eq!(second.difference(&first).count(), 0);
    }

    // Difference: a seat added between two scans is detected as new.
    #[test]
    fn added_seat_is_detected_as_new() {
        let before = vec![encounter_with_seats(
            "AVIRON BAYONNAIS",
            "dimanche 8 novembre à 21h05",
            vec![seat(1001, "42", "45.00 €")],
        )];
        let after = vec![encounter_with_seats(
            "AVIRON BAYONNAIS",
            "dimanche 8 novembre à 21h05",
            vec![seat(1001, "42", "45.00 €"), seat(1001, "43", "49.00 €")],
        )];

        let previous = log_scanned_encounters(&before, &[None], &HashSet::new());
        let current = log_scanned_encounters(&after, &[None], &previous);

        let new_keys: Vec<_> = current.difference(&previous).collect();
        assert_eq!(new_keys.len(), 1);
        assert_eq!(new_keys[0].pack_id, 1001);
        assert_eq!(new_keys[0].ticket_id, "43");
    }

    // Difference granularity: 1 encounter scan (no seats at all) keys —
    // a removed seat that comes back later is detected again.
    #[test]
    fn removed_then_back_seat_is_detected_again() {
        let encounters = vec![encounter_with_seats(
            "AVIRON BAYONNAIS",
            "dimanche 8 novembre à 21h05",
            vec![seat(1001, "42", "45.00 €")],
        )];

        let first = log_scanned_encounters(&encounters, &[None], &HashSet::new());

        // Seat sold: gone from resale.
        let sold_out = vec![encounter_with_seats(
            "AVIRON BAYONNAIS",
            "dimanche 8 novembre à 21h05",
            vec![],
        )];
        let emptied = log_scanned_encounters(&sold_out, &[None], &first);
        assert!(emptied.is_empty());

        // Seat re-listed with the same ids: detected as new again.
        let back = log_scanned_encounters(&encounters, &[None], &emptied);
        assert_eq!(back.difference(&emptied).count(), 1);
        assert_eq!(back, first);
    }

    // Encounters without seats contribute no keys.
    #[test]
    fn encounters_without_seats_contribute_no_keys() {
        let mut no_seats = Encounter::new(
            ClubType::StadeRochelais,
            "MONTPELLIER".to_string(),
            "samedi 6 juin 2026 à 21h05".to_string(),
            MatchNature::Rugby,
            None,
        );
        no_seats.set_seats(vec![]);

        let keys = collect_seat_keys(&[no_seats]);
        assert!(keys.is_empty());
    }

    // The precomputed resolutions only feed the event_id log field: the
    // collected seat keys are identical whether resolved or not.
    #[test]
    fn resolutions_do_not_change_the_collected_keys() {
        let encounters = vec![encounter_with_seats(
            "AVIRON BAYONNAIS",
            "dimanche 8 novembre à 21h05",
            vec![seat(1001, "42", "45.00 €")],
        )];

        assert_eq!(
            log_scanned_encounters(&encounters, &[Some(11877)], &HashSet::new()),
            log_scanned_encounters(&encounters, &[None], &HashSet::new())
        );
    }

    // Resolution happens once per scan: known rugby encounters map to their
    // calendar match id, anything else yields None.
    #[test]
    fn resolve_encounters_maps_or_drops_encounters() {
        let encounters = [
            encounter_with_seats("AVIRON BAYONNAIS", "dimanche 8 novembre à 21h05", vec![]),
            encounter_with_seats("CONNACHT RUGBY", "samedi 16 janvier à 18h30", vec![]),
        ];
        let event = Event {
            id: 11877,
            team: team(TeamType::StadeRochelais),
            opponent: team(TeamType::AvironBayonnais),
            kickoff: "samedi 7 novembre – dimanche 8 novembre 21h05".to_string(),
            status: MatchStatus::Scheduled,
        };

        assert_eq!(
            resolve_encounters(&encounters, &[event]),
            [Some(11877), None]
        );
    }
}
