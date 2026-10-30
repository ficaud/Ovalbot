use std::sync::Arc;
use std::time::Duration;

use jiff::tz::TimeZone;
use jiff::Timestamp;
use ovalbot::app;
use ovalbot::common::crypto::SecretKey;
use ovalbot::common::db;
use ovalbot::services::calendar::CalendarService;
use ovalbot::services::channels::mailer;
use ovalbot::services::channels::telegram;
use ovalbot::services::notifications;
use ovalbot::services::state::AppState;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime;

const SYNC_INTERVAL_SECS: u64 = 20 * 60;

/// RFC 3339 log timestamp in Europe/Paris, with millisecond precision and
/// an explicit UTC offset (`2026-01-15T13:00:00.000+01:00`).
const PARIS_LOG_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3f%:z";

/// Log timestamp formatter rendering [`Timestamp`]s in `Europe/Paris`.
struct ParisTime(TimeZone);

/// Formats a moment in Paris time, DST-aware, as RFC 3339 with offset
/// (`2026-07-15T14:30:00.000+02:00`); the caller decides the time zone
/// (the server init falls back to UTC when the IANA id cannot be resolved).
///
/// # Arguments
/// * `ts` — the moment to format (usually `Timestamp::now()`).
/// * `tz` — the target time zone (see [`ParisTime`]).
///
/// # Returns
/// The RFC 3339 string written to the log line.
fn format_paris(ts: Timestamp, tz: &TimeZone) -> String {
    // The timestamp format is fixed and jiff-representable, so formatting
    // cannot fail; `Display` would emit an empty string on the improbable
    // internal error, which keeps the writer contract (infallible) anyway.
    ts.to_zoned(tz.clone())
        .strftime(PARIS_LOG_FORMAT)
        .to_string()
}

/// Formats the current wall-clock time in Paris time for each log event.
impl FormatTime for ParisTime {
    fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", format_paris(Timestamp::now(), &self.0))
    }
}

/// Server entry point. Startup order (each step logs its own failures):
///
/// 1. tracing (JSON-ish default filter `ovalbot=info,tower_http=info`,
///    overridable via `RUST_LOG`, timestamps in Paris time),
/// 2. SQLite connect + migrations (`DATABASE_URL`, default `sqlite://ovalbot.db`),
/// 3. calendar service + a first LNR sync (best-effort — the bot loop needs
///    the snapshot, so it is started before any HTTP traffic),
/// 4. `AppState` (pool, mailer, telegram), Telegram webhook registration,
/// 5. background tasks: ticket resale scanner (bot) and the periodic calendar
///    sync feeding match notifications,
/// 6. Axum listener on `0.0.0.0:3000` serving [`ovalbot::app`].
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the mailing resend service (log timestamps in Paris time;
    // DST handled by the tz database, no reliance on the system zoneinfo)
    let tz = TimeZone::get("Europe/Paris").unwrap_or(TimeZone::UTC);
    tracing_subscriber::fmt()
        .with_timer(ParisTime(tz))
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
    let shop_key = SecretKey::from_env();
    if shop_key.is_none() {
        tracing::warn!(
            "SECRET_KEY not set: the shop-credentials vault is refused (set SECRET_KEY to enable)"
        );
    }
    let state = AppState {
        pool,
        mailer: mailer::from_env(),
        telegram: telegram::from_env(),
        telegram_config: telegram::config_from_env(),
        calendar,
        shop_key,
    };

    // Register the Telegram webhook (best-effort, no-op when disabled)
    telegram::register_webhook().await;

    // Start the ticket resale scanner (bot) if enabled
    ovalbot::services::bot::spawn(state.clone());

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

    /// Winter subtests the CET offset (`+01:00`) around the DST transitions.
    #[test]
    fn winter_timestamps_render_in_cet() {
        let tz = TimeZone::get("Europe/Paris").unwrap();
        let ts: Timestamp = "2026-01-15T11:30:45.5Z".parse().unwrap();
        assert_eq!(format_paris(ts, &tz), "2026-01-15T12:30:45.500+01:00");
    }

    /// Summer subtests the CEST offset (`+02:00`) around the DST transitions.
    #[test]
    fn summer_timestamps_render_in_cest() {
        let tz = TimeZone::get("Europe/Paris").unwrap();
        let ts: Timestamp = "2026-07-15T11:30:45.5Z".parse().unwrap();
        assert_eq!(format_paris(ts, &tz), "2026-07-15T13:30:45.500+02:00");
    }

    /// An unresolvable time zone id degrades to UTC instead of panicking.
    #[test]
    fn fixed_offset_fallback_still_formats() {
        let fixed = TimeZone::fixed(jiff::tz::Offset::UTC);
        let ts: Timestamp = "2026-01-15T11:30:45.5Z".parse().unwrap();
        assert_eq!(format_paris(ts, &fixed), "2026-01-15T11:30:45.500+00:00");
    }
}
