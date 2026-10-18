use std::sync::Arc;
use std::time::Duration;

use ovalbot::app;
use ovalbot::common::db;
use ovalbot::services::calendar::CalendarService;
use ovalbot::services::mailer;
use ovalbot::services::notifications;
use ovalbot::services::state::AppState;
use ovalbot::services::telegram;

use parser::core::encounter::{Encounter, MatchNature};
use parser::interface::club_manager;
use parser::interface::curl::proxy::set_proxy_enabled;
use scanner::app::scan::run_scan;
use std::collections::HashSet;

const SYNC_INTERVAL_SECS: u64 = 20 * 60;

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
                .map(|seat| SeatKey {
                    title: encounter.title.clone(),
                    date: encounter.date.clone(),
                    pack_id: seat.id.pack_id,
                    ticket_id: seat.id.ticket_id.clone(),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Logs the results of a bot scan, diffed against the previous one: the info
/// summary is emitted on every scan, but the per-seat details are only logged
/// for seats that were not seen in the previous scan. Étape 1 only: no
/// persistence, no notification — the goal is to validate the scan pipeline
/// in the terminal.
///
/// Returns the full set of current seat keys, to be passed back as `previous`
/// on the next iteration.
fn log_scanned_encounters(
    encounters: &[Encounter],
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

    for encounter in encounters {
        let Some(seats) = encounter.seats.as_ref().filter(|seats| !seats.is_empty()) else {
            continue;
        };
        for seat in seats {
            let key = SeatKey {
                title: encounter.title.clone(),
                date: encounter.date.clone(),
                pack_id: seat.id.pack_id,
                ticket_id: seat.id.ticket_id.clone(),
            };
            if previous.contains(&key) {
                continue;
            }
            tracing::debug!(
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

/// Starts the ticket resale scanner loop (the bot). Disabled by default; enable
/// with `BOT_ENABLED=true`, tune the interval with `BOT_SCAN_INTERVAL_SECS`.
/// Proxies are off unless `PROXY_ENABLED=true` (and `PROXIES_FILE` set).
fn spawn_bot_scan_loop() {
    if !env_flag("BOT_ENABLED", true) {
        tracing::info!("bot scan disabled (set BOT_ENABLED=true to enable)");
        return;
    }

    set_proxy_enabled(env_flag("PROXY_ENABLED", false));
    let club = club_manager::stade_rochelais();
    let interval_secs = env_u64("BOT_SCAN_INTERVAL_SECS", 60);

    tracing::info!("bot scan enabled: every {interval_secs}s");
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        // Seat keys seen during the previous scan, to only log what changed.
        let mut previous: HashSet<SeatKey> = HashSet::new();
        loop {
            interval.tick().await;
            let encounters = run_scan(club.clone(), MatchNature::Rugby).await;
            previous = log_scanned_encounters(&encounters, &previous);
        }
    });
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initalize the mailing resend service
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ovalbot=info,tower_http=info".into()),
        )
        .init();

    // Connect to the database
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://ovalbot.db".to_string());
    let pool = db::connect(&database_url).await?;
    db::migrate(&pool).await?;

    // Initialize the calendar service
    let calendar = Arc::new(CalendarService::new());

    // First sync the calendar
    if let Err(e) = calendar.sync().await {
        tracing::warn!("initial calendar sync failed: {e}");
    }

    // Initialize the state
    let state = AppState {
        pool,
        mailer: mailer::from_env(),
        telegram: telegram::from_env(),
        telegram_config: telegram::config_from_env(),
        calendar,
    };

    // Register the Telegram webhook (best-effort, no-op when disabled)
    telegram::register_webhook().await;

    // Start the ticket resale scanner (bot) if enabled
    spawn_bot_scan_loop();

    // Start the calendar sync service
    let sync_state = state.clone();
    tokio::spawn(async move {
        // Sync the calendar every 20 minutes
        let mut interval = tokio::time::interval(Duration::from_secs(SYNC_INTERVAL_SECS));
        loop {
            interval.tick().await;
            match sync_state.calendar.sync().await {
                // If there is no error, we can continue and notify the subscribers
                Ok(changes) => notifications::process(&sync_state, changes).await,
                Err(e) => tracing::warn!("calendar sync failed: {e}"),
            }
        }
    });

    // Start the HTTP server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    tracing::info!("listening on {}", listener.local_addr()?);
    // serve the app
    axum::serve(listener, app(state)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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

        let first = log_scanned_encounters(&encounters, &HashSet::new());
        assert_eq!(first.len(), 3);

        // Identical scan: nothing new, same state returned.
        let second = log_scanned_encounters(&encounters, &first);
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

        let previous = log_scanned_encounters(&before, &HashSet::new());
        let current = log_scanned_encounters(&after, &previous);

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

        let first = log_scanned_encounters(&encounters, &HashSet::new());

        // Seat sold: gone from resale.
        let sold_out = vec![encounter_with_seats(
            "AVIRON BAYONNAIS",
            "dimanche 8 novembre à 21h05",
            vec![],
        )];
        let emptied = log_scanned_encounters(&sold_out, &first);
        assert!(emptied.is_empty());

        // Seat re-listed with the same ids: detected as new again.
        let back = log_scanned_encounters(&encounters, &emptied);
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
}
