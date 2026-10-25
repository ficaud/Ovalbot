mod common;

use std::sync::Arc;

use calendar::{Change, TeamType};
use common::{insert_active_user, RecordingMailer, RecordingTelegram};
use ovalbot::services::calendar::CalendarService;
use ovalbot::services::channels::alerts::{SeatAlert, SeatBrief};
use ovalbot::services::channels::mailer::Mailer;
use ovalbot::services::channels::telegram::{Telegram, TelegramConfig};
use ovalbot::services::notifications::{
    list_for_user, notify_new_seats, process, subscribe, unsubscribe, update_settings,
};
use ovalbot::services::state::AppState;
use ovalbot::services::tickets::upsert;
use sqlx::SqlitePool;

/// Builds the seats alert fixture (one pelouse seat, one cat 2).
fn seats_alert() -> SeatAlert {
    SeatAlert {
        opponent: "Aviron Bayonnais".to_string(),
        date: "dimanche 8 novembre à 21h05".to_string(),
        link: "https://billetterie.staderochelais.com/fr/product/1357/revente".to_string(),
        seats: vec![
            SeatBrief {
                bloc: "NORD - MACIF BRP".to_string(),
                row: "U".to_string(),
                seat_number: 1,
                price: "45.00".to_string(),
                full_name: "Bloc NORD - MACIF BRP • Rang U • Siège 1".to_string(),
            },
            SeatBrief {
                bloc: "NORD - MACIF BRP".to_string(),
                row: "U".to_string(),
                seat_number: 12,
                price: "99.00".to_string(),
                full_name: "Bloc NORD - MACIF BRP • Rang U • Siège 12".to_string(),
            },
        ],
    }
}

fn state(pool: SqlitePool, mailer: Arc<dyn Mailer>, telegram: Arc<dyn Telegram>) -> AppState {
    AppState {
        pool,
        mailer,
        telegram,
        telegram_config: TelegramConfig::default(),
        calendar: Arc::new(CalendarService::new()),
    }
}

/// Subscribing twice to the same match inserts a single row (`ON CONFLICT DO
/// NOTHING`).
#[sqlx::test]
async fn subscribe_is_idempotent(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;

    subscribe(&pool, user, 5).await.unwrap();
    subscribe(&pool, user, 5).await.unwrap();

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM match_notifications WHERE user_id = ? AND match_id = ?",
    )
    .bind(user)
    .bind(5)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

/// Unsubscribing from a match the user never followed is a no-op.
#[sqlx::test]
async fn unsubscribe_is_a_noop_for_unknown_match(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;

    unsubscribe(&pool, user, 42).await.unwrap();

    let ids = list_for_user(&pool, user).await.unwrap();
    assert!(ids.is_empty());
}

/// Lists the match ids the user is subscribed to, sorted ascending.
#[sqlx::test]
async fn list_returns_subscribed_match_ids(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    subscribe(&pool, user, 3).await.unwrap();
    subscribe(&pool, user, 1).await.unwrap();

    let ids = list_for_user(&pool, user).await.unwrap();
    assert_eq!(ids, vec![1, 3]);
}

/// Emails each subscriber of a newly-scheduled match once, then deletes the
/// `match_notifications` rows for that match while leaving other matches intact.
#[sqlx::test]
async fn process_emails_subscribers_once_and_clears_rows(pool: SqlitePool) {
    let user_a = insert_active_user(&pool, "a@example.com").await;
    let user_b = insert_active_user(&pool, "b@example.com").await;
    subscribe(&pool, user_a, 7).await.unwrap();
    subscribe(&pool, user_b, 7).await.unwrap();
    subscribe(&pool, user_b, 8).await.unwrap();
    sqlx::query("UPDATE users SET telegram_id = ? WHERE id = ?")
        .bind("12345")
        .bind(user_a)
        .execute(&pool)
        .await
        .unwrap();

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let state = state(pool.clone(), mailer.clone(), telegram.clone());

    process(
        &state,
        vec![Change::Scheduled {
            id: 7,
            team: TeamType::StadeRochelais,
            opponent: TeamType::RcToulon,
            kickoff: "samedi 10 octobre 14h30".to_string(),
        }],
    )
    .await;

    let sent = mailer.alerts.lock().unwrap().clone();
    assert_eq!(sent.len(), 2);
    let emails: Vec<&str> = sent.iter().map(|(email, _)| email.as_str()).collect();
    assert!(emails.contains(&"a@example.com"));
    assert!(emails.contains(&"b@example.com"));
    assert_eq!(sent[0].1.team, "Stade Rochelais");
    assert_eq!(sent[0].1.opponent, "RC Toulon");

    let messages = telegram.messages.lock().unwrap().clone();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].0, "12345");
    assert!(messages[0].1.contains("Stade Rochelais vs RC Toulon"));

    let remaining: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM match_notifications WHERE match_id = 7")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(remaining, 0);

    let unrelated: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM match_notifications WHERE match_id = 8")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(unrelated, 1);
}

/// Sends nothing when a scheduled match has no subscribers.
#[sqlx::test]
async fn process_without_subscribers_sends_nothing(pool: SqlitePool) {
    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let state = state(pool.clone(), mailer.clone(), telegram.clone());

    process(
        &state,
        vec![Change::Scheduled {
            id: 9,
            team: TeamType::StadeRochelais,
            opponent: TeamType::RcToulon,
            kickoff: "samedi 10 octobre 14h30".to_string(),
        }],
    )
    .await;

    assert!(mailer.alerts.lock().unwrap().is_empty());
    assert!(telegram.messages.lock().unwrap().is_empty());
}

/// Honors per-channel preferences: email can be muted, Telegram can be muted, or
/// both, independently of the other subscribers.
#[sqlx::test]
async fn process_respects_channel_preferences(pool: SqlitePool) {
    let email_only = insert_active_user(&pool, "email-only@example.com").await;
    let telegram_only = insert_active_user(&pool, "telegram-only@example.com").await;
    subscribe(&pool, email_only, 11).await.unwrap();
    subscribe(&pool, telegram_only, 11).await.unwrap();
    sqlx::query("UPDATE users SET notify_telegram = 0, telegram_id = '1' WHERE id = ?")
        .bind(email_only)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET notify_email = 0, telegram_id = '2' WHERE id = ?")
        .bind(telegram_only)
        .execute(&pool)
        .await
        .unwrap();

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let state = state(pool.clone(), mailer.clone(), telegram.clone());

    process(
        &state,
        vec![Change::Scheduled {
            id: 11,
            team: TeamType::StadeRochelais,
            opponent: TeamType::RcToulon,
            kickoff: "samedi 10 octobre 14h30".to_string(),
        }],
    )
    .await;

    let emails = mailer.alerts.lock().unwrap().clone();
    assert_eq!(emails.len(), 1);
    assert_eq!(emails[0].0, "email-only@example.com");

    let messages = telegram.messages.lock().unwrap().clone();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].0, "2");
}

/// Sends nothing when a subscriber muted both channels.
#[sqlx::test]
async fn process_sends_nothing_when_all_channels_muted(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    subscribe(&pool, user, 12).await.unwrap();
    sqlx::query(
        "UPDATE users SET notify_email = 0, notify_telegram = 0, telegram_id = '3' WHERE id = ?",
    )
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let state = state(pool.clone(), mailer.clone(), telegram.clone());

    process(
        &state,
        vec![Change::Scheduled {
            id: 12,
            team: TeamType::StadeRochelais,
            opponent: TeamType::RcToulon,
            kickoff: "samedi 10 octobre 14h30".to_string(),
        }],
    )
    .await;

    assert!(mailer.alerts.lock().unwrap().is_empty());
    assert!(telegram.messages.lock().unwrap().is_empty());
}

/// `update_settings` persists both channel flags for the user.
#[sqlx::test]
async fn update_settings_persists_preferences(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;

    update_settings(&pool, user, false, true).await.unwrap();

    let (email_enabled, telegram_enabled): (bool, bool) =
        sqlx::query_as("SELECT notify_email, notify_telegram FROM users WHERE id = ?")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!email_enabled);
    assert!(telegram_enabled);
}

// ============================== SEATS ALERTS (BOT) ==============================

/// Emails (and telegraphically for the linked ones) every notify subscriber
/// of the match, with the alert content intact; unlinked Telegram users only
/// get the email.
#[sqlx::test]
async fn notify_new_seats_alerts_pending_requesters_per_channel(pool: SqlitePool) {
    let linked = insert_active_user(&pool, "linked@example.com").await;
    let unlinked = insert_active_user(&pool, "unlinked@example.com").await;
    upsert(
        &pool,
        linked,
        11877,
        &ovalbot::services::tickets::TicketRequestInput::Notify,
    )
    .await
    .unwrap();
    upsert(
        &pool,
        unlinked,
        11877,
        &ovalbot::services::tickets::TicketRequestInput::Notify,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE users SET telegram_id = '123' WHERE id = ?")
        .bind(linked)
        .execute(&pool)
        .await
        .unwrap();

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let state = state(pool.clone(), mailer.clone(), telegram.clone());

    notify_new_seats(&state, 11877, &seats_alert()).await;

    let alert = seats_alert();
    let sent = mailer.seats_alerts.lock().unwrap().clone();
    assert_eq!(
        sent.iter()
            .map(|(email, _)| email.as_str())
            .collect::<Vec<_>>(),
        vec!["linked@example.com", "unlinked@example.com"]
    );
    assert_eq!(sent[0].1.opponent, alert.opponent);
    assert_eq!(sent[0].1.date, alert.date);
    assert_eq!(sent[0].1.seats_listing(), alert.seats_listing());

    let messages = telegram.messages.lock().unwrap().clone();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].0, "123");
    // The Telegram message follows the house design: header block, clickable
    // CTA, and one bullet per seat keeping the site's own seat text.
    assert!(messages[0].1.contains("🟢 Nouvelles places :"));
    assert!(messages[0].1.contains(
        "🔗 <a href=\"https://billetterie.staderochelais.com/fr/product/1357/revente\">Accéder à la revente</a>"
    ));
    assert!(messages[0]
        .1
        .contains("  • Bloc NORD - MACIF BRP • Rang U • Siège 1 — 45.00€"));
    assert!(messages[0].1.contains("99.00€"));
    assert!(messages[0].1.starts_with("🏉 Stade Rochelais"));
}

/// The same per-channel preferences gate the seats alerts: an email-muted
/// user only gets Telegram, a Telegram-muted user only the email.
#[sqlx::test]
async fn notify_new_seats_respects_channel_preferences(pool: SqlitePool) {
    let email_only = insert_active_user(&pool, "email-only@example.com").await;
    let telegram_only = insert_active_user(&pool, "telegram-only@example.com").await;
    upsert(
        &pool,
        email_only,
        11877,
        &ovalbot::services::tickets::TicketRequestInput::Notify,
    )
    .await
    .unwrap();
    upsert(
        &pool,
        telegram_only,
        11877,
        &ovalbot::services::tickets::TicketRequestInput::Notify,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE users SET notify_telegram = 0, telegram_id = '2' WHERE id = ?")
        .bind(email_only)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET notify_email = 0, telegram_id = '1' WHERE id = ?")
        .bind(telegram_only)
        .execute(&pool)
        .await
        .unwrap();

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let state = state(pool.clone(), mailer.clone(), telegram.clone());

    notify_new_seats(&state, 11877, &seats_alert()).await;

    let emails = mailer.seats_alerts.lock().unwrap().clone();
    assert_eq!(emails.len(), 1);
    assert_eq!(emails[0].0, "email-only@example.com");

    let messages = telegram.messages.lock().unwrap().clone();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].0, "1");
}

/// Buying-kind requests (Single/Duo/Group) are not alert candidates: a match
/// with buying requests but no `notify` subscription triggers no send at all.
#[sqlx::test]
async fn notify_new_seats_ignores_buying_kinds(pool: SqlitePool) {
    let user = insert_active_user(&pool, "buyer@example.com").await;
    upsert(
        &pool,
        user,
        11877,
        &ovalbot::services::tickets::TicketRequestInput::Single {
            quantity: 2,
            max_price_cents: 5000,
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE users SET telegram_id = '123' WHERE id = ?")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let state = state(pool.clone(), mailer.clone(), telegram.clone());

    notify_new_seats(&state, 11877, &seats_alert()).await;

    assert!(mailer.seats_alerts.lock().unwrap().is_empty());
    assert!(telegram.messages.lock().unwrap().is_empty());
}

/// A match with no subscriber at all triggers no send.
#[sqlx::test]
async fn notify_new_seats_sends_nothing_without_pending_requesters(pool: SqlitePool) {
    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let state = state(pool.clone(), mailer.clone(), telegram.clone());

    notify_new_seats(&state, 11877, &seats_alert()).await;

    assert!(mailer.seats_alerts.lock().unwrap().is_empty());
    assert!(telegram.messages.lock().unwrap().is_empty());
}
