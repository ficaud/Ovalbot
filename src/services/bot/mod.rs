//! Background ticket resale scanner (the bot).
//!
//! Two passes per tick:
//! 1. **Discovery** (every `BOT_HOME_REFRESH_SECS`): fetch the billetterie
//!    home page, resolve each card against the calendar snapshot
//!    ([`crate::services::bot::matching`]) and persist every open resale link in
//!    `resale_links` ([`crate::services::bot::resale::upsert_discovered`]). The
//!    home page is only hit for discovery; a waiting-queue page is reported
//!    distinctly and never stops the polling.
//! 2. **Direct poll** (every `BOT_SCAN_INTERVAL_SECS`): fetch each stored
//!    link directly ([`crate::services::bot::resale::links_to_scan`]), record the
//!    seats durably in `ticket_offers`
//!    ([`crate::services::tickets::record_offers`]), alert the pending
//!    `ticket_requests` alerters
//!    ([`crate::services::notifications::notify_new_seats`]) through email
//!    and/or Telegram, and match the listing against the pending **buying**
//!    requests ([`crate::services::bot::purchases::plan_checkouts`]) to feed
//!    the Phase 4B checkout orchestrator.
//!
//! `ticket_offers` is the single "new seats" source of truth: only the seats
//! it actually inserts are logged, so a restart never re-announces old seats.
//!
//! Targeting is set once at startup ([`BotMode`]): `BOT_SPORT=rugby` (default)
//! resolves against the LNR calendar, `BOT_SPORT=basket` restricts discovery
//! and the poll to basketball resales and attaches them to the synthetic
//! `BOT_BASKET_MATCH_ID` (test bed — the LNR calendar has no basket games).

pub mod checkout;
pub mod matching;
pub mod purchases;
pub mod resale;

use std::time::Duration;
use std::time::Instant;

use parser::core::encounter::{Encounter, MatchNature};
use parser::core::seat::Seat;
use parser::interface::club_manager;
use parser::interface::ScanError;
use scanner::app::scan::{discover, fetch_seats};

use crate::services::bot::resale::{ScanStatus, ScanTarget};
use crate::services::channels::alerts::{SeatAlert, SeatBrief};
use crate::services::notifications;
use crate::services::state::AppState;
use crate::services::tickets::{self, OfferKey};

// ============================== SCAN LOOP ==============================

/// Which sport the scan targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sport {
    /// Rugby only, resolved against the LNR calendar (the default).
    Rugby,
    /// Basketball test bed: every retained basket encounter is mapped to a
    /// synthetic match id, because the LNR calendar has no basket games.
    Basket,
}

impl Sport {
    /// The value logged at startup.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Sport::Rugby => "rugby",
            Sport::Basket => "basket",
        }
    }
}

/// Parses `BOT_SPORT` (`""`/`rugby`/`basket`, case-insensitive).
///
/// # Arguments
/// * `value` - The raw environment value
/// # Returns
/// The sport, or `None` for an unknown value
fn parse_sport(value: &str) -> Option<Sport> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "rugby" => Some(Sport::Rugby),
        "basket" => Some(Sport::Basket),
        _ => None,
    }
}

/// Runtime targeting of the scan loop, read once at startup.
#[derive(Debug, Clone)]
pub struct BotMode {
    /// Sport the discovery keeps and the poll serves.
    pub sport: Sport,
    /// `BOT_BASKET_MATCH_ID`: synthetic match id the basket test bed serves
    /// (required in basket mode, the requests are created by SQL).
    pub basket_match_id: Option<i64>,
    /// `BOT_BASKET_OPPONENT`: optional opponent substring (case-insensitive)
    /// to target a single basket match.
    pub basket_opponent: Option<String>,
}

impl BotMode {
    /// Reads the mode from the environment.
    ///
    /// # Errors
    /// Returns a human-readable reason when basket mode is requested without a
    /// valid `BOT_BASKET_MATCH_ID`.
    fn from_env() -> Result<Self, String> {
        let sport = std::env::var("BOT_SPORT").unwrap_or_default();
        let basket_id = std::env::var("BOT_BASKET_MATCH_ID").ok();
        let opponent = std::env::var("BOT_BASKET_OPPONENT").ok();
        Self::from_values(&sport, basket_id.as_deref(), opponent.as_deref())
    }

    /// Builds a mode from raw values (unit-testable, no environment access).
    ///
    /// An unknown `BOT_SPORT` falls back to rugby with a warning; basket mode
    /// refuses to start without a parseable `BOT_BASKET_MATCH_ID`.
    ///
    /// # Arguments
    /// * `sport` - Raw `BOT_SPORT` value
    /// * `basket_match_id` - Raw `BOT_BASKET_MATCH_ID` value
    /// * `basket_opponent` - Raw `BOT_BASKET_OPPONENT` value
    /// # Errors
    /// Returns a reason when basket mode lacks a valid match id.
    fn from_values(
        sport: &str,
        basket_match_id: Option<&str>,
        basket_opponent: Option<&str>,
    ) -> Result<Self, String> {
        let sport = match parse_sport(sport) {
            Some(sport) => sport,
            None => {
                tracing::warn!(
                    value = %sport,
                    "unknown BOT_SPORT, defaulting to rugby (expected 'rugby' or 'basket')"
                );
                Sport::Rugby
            }
        };
        let basket_match_id = match basket_match_id.map(str::trim).filter(|v| !v.is_empty()) {
            Some(raw) => Some(
                raw.parse::<i64>()
                    .map_err(|_| format!("BOT_BASKET_MATCH_ID is not an integer: {raw}"))?,
            ),
            None => None,
        };
        if sport == Sport::Basket && basket_match_id.is_none() {
            return Err(
                "BOT_SPORT=basket requires BOT_BASKET_MATCH_ID (the synthetic match id \
                 the test requests are created with)"
                    .to_string(),
            );
        }
        let basket_opponent = basket_opponent
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_lowercase);
        Ok(Self {
            sport,
            basket_match_id,
            basket_opponent,
        })
    }

    /// The `match_id` the direct poll is restricted to (`None` = poll all
    /// stored links, the rugby behavior).
    fn poll_match_id(&self) -> Option<i64> {
        match self.sport {
            Sport::Rugby => None,
            Sport::Basket => self.basket_match_id,
        }
    }
}

/// Keeps the encounters the mode serves, in order.
///
/// Rugby mode keeps everything (the resolver, not the discovery, decides what
/// maps to a calendar match — promotional badges must not drop rugby cards).
/// Basket mode keeps basketball cards only, optionally restricted to one
/// opponent.
///
/// # Arguments
/// * `encounters` - The discovery result, filtered in place
/// * `mode` - The runtime targeting
fn retain_for_mode(encounters: &mut Vec<Encounter>, mode: &BotMode) {
    if mode.sport == Sport::Rugby {
        return;
    }
    encounters.retain(|encounter| {
        encounter.nature == MatchNature::Basketball
            && mode
                .basket_opponent
                .as_ref()
                .is_none_or(|needle| encounter.title.to_lowercase().contains(needle))
    });
}

/// Starts the ticket resale scanner loop (the bot). Disabled by default; enable
/// with `BOT_ENABLED=true`. Two cadences: `BOT_SCAN_INTERVAL_SECS` drives the
/// whole loop (direct poll of the stored links, default 60 s), and
/// `BOT_HOME_REFRESH_SECS` the home-page discovery (default 60 s).
///
/// `BOT_SPORT=basket` switches the loop to the basketball test bed (see
/// [`BotMode`]): only basket resales are discovered/polled and they are mapped
/// to `BOT_BASKET_MATCH_ID`.
pub fn spawn(state: AppState) {
    if !env_flag("BOT_ENABLED", true) {
        tracing::info!("bot scan disabled (set BOT_ENABLED=true to enable)");
        return;
    }

    let mode = match BotMode::from_env() {
        Ok(mode) => mode,
        Err(reason) => {
            tracing::error!("bot scan disabled: {reason}");
            return;
        }
    };
    if let (Sport::Basket, Some(id)) = (mode.sport, mode.basket_match_id) {
        if id < 900_000 {
            tracing::warn!(
                basket_match_id = id,
                "BOT_BASKET_MATCH_ID looks like an LNR id (basket test ids are >= 900000)"
            );
        }
    }

    let club = club_manager::stade_rochelais();
    let poll_secs = env_u64("BOT_SCAN_INTERVAL_SECS", 60);
    let home_secs = env_u64("BOT_HOME_REFRESH_SECS", 60);

    tracing::info!(
        sport = mode.sport.as_str(),
        basket_match_id = ?mode.basket_match_id,
        basket_opponent = ?mode.basket_opponent,
        "bot scan enabled: resale poll every {poll_secs}s, home discovery every {home_secs}s"
    );
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(poll_secs));
        // Last successful home discovery, to pace the next one. Empty at
        // startup so the first tick discovers immediately (cold start).
        let mut last_refresh: Option<Instant> = None;
        loop {
            interval.tick().await;
            discover_home(&state, &club, home_secs, &mut last_refresh, &mode).await;
            poll_links(&state, &mode).await;
        }
    });
}

// ============================== ENV HELPERS ==============================

/// Reads a boolean flag from the environment (`1`/`true`/`0`/`false`, case-insensitive).
///
/// Public because the binary also parses the checkout flags (`CART_ENABLED`,
/// `CART_DRY_RUN`) when it builds the app state.
pub fn env_flag(name: &str, default: bool) -> bool {
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
/// match list, keep the encounters the mode serves, resolve every card against
/// the calendar snapshot (rugby) or the configured synthetic id (basket), then
/// persist the links in `resale_links`.
///
/// A queued or failed home page never aborts the loop: the caller just logs a
/// warning and the stored links keep polling.
///
/// # Arguments
/// * `state` - shared app dependencies (pool + calendar snapshot)
/// * `club` - the club to discover
/// * `home_secs` - the discovery cadence
/// * `last_refresh` - pacing state, updated on the first success
/// * `mode` - the runtime targeting (`BOT_SPORT`)
async fn discover_home(
    state: &AppState,
    club: &parser::core::club::Club,
    home_secs: u64,
    last_refresh: &mut Option<Instant>,
    mode: &BotMode,
) {
    // The first tick always discovers (cold start; the table may be empty).
    if !last_refresh.is_none_or(|at| at.elapsed().as_secs() >= home_secs) {
        return;
    }

    match discover(club.clone()).await {
        Ok(mut encounters) => {
            retain_for_mode(&mut encounters, mode);
            // Resolve every encounter against the calendar snapshot (rugby) or
            // the configured synthetic id (basket).
            let events = state.calendar.snapshot();
            let match_ids = resolve_encounters(&encounters, &events, mode);
            match resale::upsert_discovered(&state.pool, &encounters, &match_ids).await {
                Ok(counts) => tracing::info!(
                    sport = mode.sport.as_str(),
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

/// Resolves every encounter to the `match_id` the rest of the bot works with.
///
/// Rugby mode: the LNR calendar lookup; unresolvable encounters (non-match
/// cards, unknown opponents, unparseable date, no or ambiguous calendar match)
/// log at debug and yield `None`, keeping index alignment with `encounters`.
/// Basket mode: every retained encounter maps to the configured synthetic id.
///
/// # Arguments
/// * `encounters` - The (mode-filtered) discovery result
/// * `events` - The rugby calendar snapshot
/// * `mode` - The runtime targeting
/// # Returns
/// One entry per encounter, in order: `Some(match_id)` when the encounter is
/// served, `None` otherwise.
fn resolve_encounters(
    encounters: &[Encounter],
    events: &[calendar::Event],
    mode: &BotMode,
) -> Vec<Option<u32>> {
    encounters
        .iter()
        .map(|encounter| match mode.sport {
            Sport::Basket => mode.basket_match_id.and_then(|id| u32::try_from(id).ok()),
            Sport::Rugby => match matching::resolve(encounter, events) {
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
            },
        })
        .collect()
}

// ============================== DIRECT POLL PASS ==============================

/// Polls the stored resale links the mode serves: fetches the product page,
/// records the seats and alerts the requesters. The DB drives the loop, so a
/// queued home page changes nothing here.
///
/// In basket mode only the links resolved to the configured synthetic id are
/// polled (the rugby links stay stored but are left untouched, so a test bed
/// never buys a rugby seat). Fetch/dB failures warn and continue — one broken
/// link cannot skip the others.
async fn poll_links(state: &AppState, mode: &BotMode) {
    let targets = match resale::links_to_scan_for(&state.pool, mode.poll_match_id()).await {
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
                    full_name: seat.seat_info.full_name.clone(),
                })
                .collect(),
        };
        notifications::notify_new_seats(state, event_id, &alert).await;

        // Purchase matching (Phase 4B): decide which pending buying requests
        // the current listing serves, then let the checkout orchestrator turn
        // the plans into real add-to-cart calls (behind `CART_ENABLED`). Every
        // skip/plan reason is logged inside; a matching/checkout failure never
        // stops the poll.
        match purchases::plan_checkouts_with_fresh(&state.pool, i64::from(event_id), &seats, &fresh)
            .await
        {
            Ok(plans) if !plans.is_empty() => {
                let checkout_target = checkout::CheckoutTarget {
                    match_id: i64::from(event_id),
                    title: target.title.clone(),
                    date: target.date.clone(),
                    resale_link: target.resale_link.clone(),
                };
                if state.cart_enabled {
                    checkout::execute_plans(state, &checkout_target, plans).await;
                } else {
                    tracing::info!(
                        event_id,
                        plans = plans.len(),
                        "checkout disabled (CART_ENABLED), purchase plans logged only"
                    );
                }
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("purchase matching failed for match {event_id}: {e}"),
        }
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

    /// A rugby-mode targeting (the default).
    fn rugby_mode() -> BotMode {
        BotMode {
            sport: Sport::Rugby,
            basket_match_id: None,
            basket_opponent: None,
        }
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
            resolve_encounters(&encounters, &[event], &rugby_mode()),
            [Some(11877), None]
        );
    }

    // Basket mode bypasses the calendar: every retained encounter maps to the
    // configured synthetic id.
    #[test]
    fn basket_mode_maps_every_encounter_to_the_configured_id() {
        let encounters = [
            Encounter::new(
                ClubType::StadeRochelais,
                "DENAIN".to_string(),
                "vendredi 6 novembre à 20h00".to_string(),
                MatchNature::Basketball,
                None,
            ),
            Encounter::new(
                ClubType::StadeRochelais,
                "ÉVREUX".to_string(),
                "samedi 14 novembre à 20h00".to_string(),
                MatchNature::Basketball,
                None,
            ),
        ];
        let mode = BotMode {
            sport: Sport::Basket,
            basket_match_id: Some(900_001),
            basket_opponent: None,
        };
        assert_eq!(
            resolve_encounters(&encounters, &[], &mode),
            [Some(900_001), Some(900_001)]
        );
    }

    // Rugby mode keeps every card (promotional badges must not drop rugby
    // cards); basket mode keeps basketball cards only, optionally by opponent.
    #[test]
    fn retain_for_mode_filters_the_discovery() {
        let encounter = |title: &str, nature: MatchNature| {
            Encounter::new(
                ClubType::StadeRochelais,
                title.to_string(),
                "samedi 8 novembre à 20h00".to_string(),
                nature,
                None,
            )
        };
        let mut encounters = vec![
            encounter("AVIRON BAYONNAIS", MatchNature::Rugby),
            encounter("DENAIN", MatchNature::Basketball),
            encounter("BORDEAUX REPAS", MatchNature::Other),
        ];

        retain_for_mode(&mut encounters, &rugby_mode());
        assert_eq!(encounters.len(), 3, "rugby keeps everything");

        let basket_mode = BotMode {
            sport: Sport::Basket,
            basket_match_id: Some(900_001),
            basket_opponent: None,
        };
        let mut encounters = vec![
            encounter("AVIRON BAYONNAIS", MatchNature::Rugby),
            encounter("DENAIN", MatchNature::Basketball),
            encounter("ÉVREUX", MatchNature::Basketball),
        ];
        retain_for_mode(&mut encounters, &basket_mode);
        assert_eq!(encounters.len(), 2);
        assert!(encounters
            .iter()
            .all(|e| e.nature == MatchNature::Basketball));

        let filtered = BotMode {
            basket_opponent: Some("denain".to_string()),
            ..basket_mode
        };
        retain_for_mode(&mut encounters, &filtered);
        assert_eq!(encounters.len(), 1);
        assert_eq!(encounters[0].title, "DENAIN");
    }

    // BOT_SPORT parsing: known values map, anything else is rejected so the
    // caller can fall back to rugby with a warning.
    #[test]
    fn parse_sport_accepts_known_values() {
        assert_eq!(parse_sport(""), Some(Sport::Rugby));
        assert_eq!(parse_sport(" rugby "), Some(Sport::Rugby));
        assert_eq!(parse_sport("BASKET"), Some(Sport::Basket));
        assert_eq!(parse_sport("foot"), None);
    }

    // Basket mode refuses to start without a synthetic match id; rugby mode
    // ignores it. The opponent is trimmed and lowercased.
    #[test]
    fn basket_mode_requires_a_match_id() {
        assert!(BotMode::from_values("basket", None, None).is_err());
        assert!(BotMode::from_values("basket", Some("abc"), None).is_err());

        let mode = BotMode::from_values("basket", Some(" 900001 "), Some(" Denain ")).unwrap();
        assert_eq!(mode.sport, Sport::Basket);
        assert_eq!(mode.basket_match_id, Some(900_001));
        assert_eq!(mode.basket_opponent.as_deref(), Some("denain"));

        let rugby = BotMode::from_values("rugby", None, None).unwrap();
        assert_eq!(rugby.sport, Sport::Rugby);
        assert_eq!(rugby.poll_match_id(), None);
        assert_eq!(mode.poll_match_id(), Some(900_001));
    }
}
