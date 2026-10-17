use std::sync::Arc;
use std::time::Duration;

use ovalbot::app;
use ovalbot::common::db;
use ovalbot::services::calendar::CalendarService;
use ovalbot::services::mailer;
use ovalbot::services::notifications;
use ovalbot::services::state::AppState;
use ovalbot::services::telegram;
use ovalbot::services::tickets::TicketManager;

const SYNC_INTERVAL_SECS: u64 = 20 * 60;

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

    let tickets = Arc::new(TicketManager::default());

    // Initialize the state
    let state = AppState {
        pool,
        mailer: mailer::from_env(),
        telegram: telegram::from_env(),
        telegram_config: telegram::config_from_env(),
        calendar,
        tickets,
    };

    // Register the Telegram webhook (best-effort, no-op when disabled)
    telegram::register_webhook().await;

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
