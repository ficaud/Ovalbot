//! Background ticket resale scanner (the bot).
//!
//! Two passes per tick:
//! 1. **Discovery** (every `BOT_HOME_REFRESH_SECS`): fetch the billetterie
//!    home page, resolve each card against the calendar snapshot
//!    ([`crate::services::matching`]) and persist every open resale link in
//!    `resale_links` ([`crate::services::resale::upsert_discovered`]). The
//!    home page is only hit for discovery; a waiting-queue page is reported
//!    distinctly and never stops the polling.
//! 2. **Direct poll** (every `BOT_SCAN_INTERVAL_SECS`): fetch each stored
//!    link directly ([`crate::services::resale::links_to_scan`]), record the
//!    seats durably in `ticket_offers`
//!    ([`crate::services::tickets::record_offers`]) and alert the pending
//!    `ticket_requests` requesters
//!    ([`crate::services::notifications::notify_new_seats`]) through email
//!    and/or Telegram.
//!
//! `ticket_offers` is the single "new seats" source of truth: only the seats
//! it actually inserts are logged, so a restart never re-announces old seats.

use std::time::Duration;
use std::time::Instant;

use parser::core::encounter::Encounter;
use parser::core::seat::Seat;
use parser::interface::club_manager;
use parser::interface::curl::proxy::set_proxy_enabled;
use parser::interface::ScanError;
use scanner::app::scan::{discover, fetch_seats};

use crate::services::alerts::{SeatAlert, SeatBrief};
use crate::services::matching;
use crate::services::notifications;
use crate::services::resale::{self, ScanStatus, ScanTarget};
use crate::services::state::AppState;
use crate::services::tickets::{self, OfferKey};
// ============================== SCAN LOOP ==============================

/// Starts the ticket resale scanner loop (the bot). Disabled by default; enable
/// with `BOT_ENABLED=true`. Two cadences: `BOT_SCAN_INTERVAL_SECS` drives the
/// whole loop (direct poll of the stored links, default 60 s), and
/// `BOT_HOME_REFRESH_SECS` the home-page discovery (default 60 s).
/// Proxies are off unless `PROXY_ENABLED=true` (and `PROXIES_FILE` set).
pub fn spawn(state: AppState) {
    if !env_flag("BOT_ENABLED", true) {
        tracing::info!("bot scan disabled (set BOT_ENABLED=true to enable)");
        return;
    }

    set_proxy_enabled(env_flag("PROXY_ENABLED", false));
    let club = club_manager::stade_rochelais();
    let poll_secs = env_u64("BOT_SCAN_INTERVAL_SECS", 60);
    let home_secs = env_u64("BOT_HOME_REFRESH_SECS", 60);

    tracing::info!(
        "bot scan enabled: resale poll every {poll_secs}s, home discovery every {home_secs}s"
    );
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(poll_secs));
        // Last successful home discovery, to pace the next one. Empty at
        // startup so the first tick discovers immediately (cold start).
        let mut last_refresh: Option<Instant> = None;
        loop {
            interval.tick().await;
            discover_home(&state, &club, home_secs, &mut last_refresh).await;
            poll_links(&state).await;
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

// ============================== DISCOVERY PASS ==============================

/// Runs the home-discovery pass when its cadence allows it: fetch + parse the
/// match list, resolve every card against the calendar snapshot, then persist
/// the links in `resale_links`.
///
/// A queued or failed home page never aborts the loop: the caller just logs a
/// warning and the stored links keep polling.
///
/// # Arguments
/// * `state` - shared app dependencies (pool + calendar snapshot)
/// * `club` - the club to discover
/// * `home_secs` - the discovery cadence
/// * `last_refresh` - pacing state, updated on the first success
async fn discover_home(
    state: &AppState,
    club: &parser::core::club::Club,
    home_secs: u64,
    last_refresh: &mut Option<Instant>,
) {
    // The first tick always discovers (cold start; the table may be empty).
    if !last_refresh.is_none_or(|at| at.elapsed().as_secs() >= home_secs) {
        return;
    }

    match discover(club.clone()).await {
        Ok(encounters) => {
            // Resolve every encounter against the calendar snapshot: it is
            // populated by the first sync, started before this loop.
            let events = state.calendar.snapshot();
            let match_ids = resolve_encounters(&encounters, &events);
            match resale::upsert_discovered(&state.pool, &encounters, &match_ids).await {
                Ok(counts) => tracing::info!(
                    cards = encounters.len(),
                    links = counts.with_link,
                    new = counts.inserted,
                    updated = counts.updated,
                    "bot home refresh"
                ),
                Err(e) => tracing::warn!("home refresh: failed to persist resale links: {e}"),
            }
            if encounters.is_empty() {
                // With a live site this never happens: dozens of cards are
                // always listed. Debug-worthy, so the caller sees it.
                tracing::warn!("bot home refresh: 0 card parsed — queue page or layout change?");
            } else {
                *last_refresh = Some(Instant::now());
            }
        }
        Err(ScanError::WaitingQueue) => {
            tracing::warn!("bot home refresh blocked by waiting queue; stored links keep polling");
        }
        Err(ScanError::Fetch(e)) => {
            tracing::warn!("bot home refresh failed: {e}");
        }
    }
}

/// Resolves every encounter against the calendar snapshot.
///
/// Unresolvable encounters (non-match cards, unknown opponents, unparseable
/// date, no or ambiguous calendar match) log at debug and yield `None`,
/// keeping index alignment with `encounters`.
///
/// # Returns
/// One entry per encounter, in order: `Some(match_id)` when the encounter is
/// tied to exactly one calendar match, `None` otherwise.
fn resolve_encounters(encounters: &[Encounter], events: &[calendar::Event]) -> Vec<Option<u32>> {
    encounters
        .iter()
        .map(|encounter| match matching::resolve(encounter, events) {
            Ok(match_id) => Some(match_id),
            Err(e) => {
                tracing::debug!(
                    title = %encounter.title,
                    date = %encounter.date,
                    error = %e,
                    "encounter not resolved"
                );
                None
            }
        })
        .collect()
}

// ============================== DIRECT POLL PASS ==============================

/// Polls every stored resale link directly: fetches the product page, records
/// the seats and alerts the requesters. The DB drives the loop, so a queued
/// home page changes nothing here.
///
/// Fetch/dB failures warn and continue — one broken link cannot skip the
/// others.
async fn poll_links(state: &AppState) {
    let targets = match resale::links_to_scan(&state.pool).await {
        Ok(targets) => targets,
        Err(e) => {
            tracing::warn!("resale poll: failed to read resale_links: {e}");
            return;
        }
    };
    if targets.is_empty() {
        tracing::debug!("resale poll: no stored link yet (waiting for the first discovery)");
        return;
    }
    for target in &targets {
        poll_one(state, target).await;
    }
}

/// Polls one stored resale link: retirement of past-dated matches first, then
/// the seat fetch, the durable recording and the alert.
async fn poll_one(state: &AppState, target: &ScanTarget) {
    let encounter = target.to_encounter();

    // A link whose match is over is removed, not polled: the product page
    // could be recycled by a later wave.
    if encounter.date_passed() {
        match resale::delete_one(&state.pool, &target.title, &target.date).await {
            Ok(removed) if removed => tracing::info!(
                title = %target.title,
                date = %target.date,
                link = %target.resale_link,
                "past-dated resale link removed"
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!("failed to remove past-dated resale link: {e}"),
        }
        return;
    }

    let seats = match fetch_seats(encounter.clone()).await {
        Ok(seats) => seats,
        Err(ScanError::WaitingQueue) => {
            mark_and_continue(state, target, ScanStatus::Queue).await;
            return;
        }
        Err(ScanError::Fetch(e)) => {
            tracing::warn!(link = %target.resale_link, "resale poll fetch failed: {e}");
            mark_and_continue(state, target, ScanStatus::HttpError).await;
            return;
        }
    };

    let status = if seats.is_empty() {
        ScanStatus::NoSeats
    } else {
        ScanStatus::Ok
    };

    let Some(match_id) = target.match_id else {
        // Cards outside the calendar (basket games, other events) keep their
        // link and get polled, but the resolver has no match id for them:
        // nothing to record, nothing to alert. The next discovery pass
        // re-resolves them anyway.
        tracing::debug!(
            title = %target.title,
            date = %target.date,
            "resale poll: encounter not resolved (no calendar match)"
        );
        mark_and_continue(state, target, status).await;
        return;
    };
    let Ok(event_id) = u32::try_from(match_id) else {
        tracing::warn!(match_id, "resale poll: impossible match id");
        mark_and_continue(state, target, status).await;
        return;
    };

    let fresh = match tickets::record_offers(
        &state.pool,
        i64::from(event_id),
        &target.title,
        &target.date,
        &seats,
    )
    .await
    {
        Ok(fresh) => fresh,
        Err(e) => {
            tracing::warn!("failed to record ticket offers for match {event_id}: {e}");
            mark_and_continue(state, target, status).await;
            return;
        }
    };

    // The seats this call inserted are at once the alert's and the log's
    // subject: whatever was known before is re-announced by nobody, ever.
    let fresh_seats = seats_matching(&seats, &fresh);
    for seat in &fresh_seats {
        tracing::info!(
            event_id,
            title = %target.title,
            date = %target.date,
            category = %seat.seat_info.composition.category,
            full_name = %seat.seat_info.full_name,
            price = %seat.price,
            pack_id = seat.id.pack_id,
            ticket_id = %seat.id.ticket_id,
            "resale poll: new seat on resale"
        );
    }
    tracing::info!(
        event_id,
        title = %target.title,
        date = %target.date,
        seats = seats.len(),
        new = fresh_seats.len(),
        "bot resale poll"
    );

    if !fresh_seats.is_empty() {
        let alert = SeatAlert {
            opponent: target.title.clone(),
            date: target.date.clone(),
            link: target.resale_link.clone(),
            seats: fresh_seats
                .iter()
                .map(|seat| SeatBrief {
                    bloc: seat.seat_info.composition.bloc.clone(),
                    row: seat.seat_info.composition.row.clone(),
                    seat_number: seat.seat_info.composition.seat_number,
                    price: seat.price.clone(),
                })
                .collect(),
        };
        notifications::notify_new_seats(state, event_id, &alert).await;
    }

    mark_and_continue(state, target, status).await;
}

/// Keeps the `fresh_keys`-matching seats in their parsed order.
///
/// # Arguments
/// * `seats` - the seats currently parsed from the resale page
/// * `fresh` - the offer keys inserted by this call (`record_offers`)
/// # Returns
/// The subset of `seats` whose `(pack_id, ticket_id)` is in `fresh`.
fn seats_matching<'a>(seats: &'a [Seat], fresh: &[OfferKey]) -> Vec<&'a Seat> {
    let keys: Vec<(u64, String)> = fresh
        .iter()
        .map(|key| (key.pack_id, key.ticket_id.clone()))
        .collect();
    seats
        .iter()
        .filter(|s| keys.contains(&(s.id.pack_id, s.id.ticket_id.clone())))
        .collect()
}

/// Marks the poll outcome in `resale_links`; a failure here only logs — the
/// next tick overwrites the status anyway.
async fn mark_and_continue(state: &AppState, target: &ScanTarget, status: ScanStatus) {
    if let Err(e) = resale::mark_scanned(&state.pool, &target.title, &target.date, status).await {
        tracing::warn!("failed to mark resale link scanned: {e}");
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
    use parser::core::encounter::MatchNature;
    use parser::core::seat::{SeatComposition, SeatId, SeatInfo};

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

    // Builds a calendar `Team` whose id/name follow the TeamType conventions.
    fn team(team_type: TeamType) -> Team {
        Team {
            id: team_type.id().to_string(),
            name: team_type.to_string(),
            team_type,
        }
    }

    // The fresh-seat filter keeps only the seats whose ids were inserted by
    // this call, in parsed order.
    #[test]
    fn seats_matching_keeps_only_fresh_seats() {
        let seats = vec![seat(1001, "42", "45.00"), seat(1001, "43", "49.00")];
        let fresh = vec![OfferKey {
            pack_id: 1001,
            ticket_id: "43".to_string(),
        }];
        let kept = seats_matching(&seats, &fresh);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id.ticket_id, "43");

        let all_fresh = vec![
            OfferKey {
                pack_id: 1001,
                ticket_id: "42".to_string(),
            },
            OfferKey {
                pack_id: 1001,
                ticket_id: "43".to_string(),
            },
        ];
        assert_eq!(seats_matching(&seats, &all_fresh).len(), 2);
        assert!(seats_matching(&seats, &[]).is_empty());
    }

    // Resolution happens once per discovery pass: known encounters map to
    // their calendar match id, anything else yields None.
    #[test]
    fn resolve_encounters_maps_or_drops_encounters() {
        let encounter_with = |title: &str, date: &str| {
            Encounter::new(
                ClubType::StadeRochelais,
                title.to_string(),
                date.to_string(),
                MatchNature::Rugby,
                None,
            )
        };
        let encounters = [
            encounter_with("AVIRON BAYONNAIS", "dimanche 8 novembre à 21h05"),
            encounter_with("CONNACHT RUGBY", "samedi 16 janvier à 18h30"),
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
