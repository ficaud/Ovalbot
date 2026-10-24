use std::sync::Arc;
use std::time::Duration;

use ovalbot::app;
use ovalbot::common::db;
use ovalbot::services::calendar::CalendarService;
use ovalbot::services::channels::mailer;
use ovalbot::services::channels::telegram;
use ovalbot::services::notifications;
use ovalbot::services::state::AppState;

const SYNC_INTERVAL_SECS: u64 = 20 * 60;

/// Server entry point. Startup order (each step logs its own failures):
///
/// 1. tracing (JSON-ish default filter `ovalbot=info,tower_http=info`,
///    overridable via `RUST_LOG`),
/// 2. SQLite connect + migrations (`DATABASE_URL`, default `sqlite://ovalbot.db`),
/// 3. calendar service + a first LNR sync (best-effort — the bot loop needs
///    the snapshot, so it is started before any HTTP traffic),
/// 4. `AppState` (pool, mailer, telegram), Telegram webhook registration,
/// 5. background tasks: ticket resale scanner (bot) and the periodic calendar
///    sync feeding match notifications,
/// 6. Axum listener on `0.0.0.0:3000` serving [`ovalbot::app`].
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the mailing resend service
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
