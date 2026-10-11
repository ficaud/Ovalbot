mod common;

use std::sync::Arc;

use calendar::{Change, TeamType};
use common::{insert_active_user, RecordingMailer};
use ovalbot::calendar::CalendarService;
use ovalbot::mailer::Mailer;
use ovalbot::notifications::{list_for_user, process, subscribe, unsubscribe};
use ovalbot::state::AppState;
use sqlx::SqlitePool;

fn state(pool: SqlitePool, mailer: Arc<dyn Mailer>) -> AppState {
    AppState {
        pool,
        mailer,
        calendar: Arc::new(CalendarService::new()),
    }
}

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

#[sqlx::test]
async fn unsubscribe_is_a_noop_for_unknown_match(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;

    unsubscribe(&pool, user, 42).await.unwrap();

    let ids = list_for_user(&pool, user).await.unwrap();
    assert!(ids.is_empty());
}

#[sqlx::test]
async fn list_returns_subscribed_match_ids(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    subscribe(&pool, user, 3).await.unwrap();
    subscribe(&pool, user, 1).await.unwrap();

    let ids = list_for_user(&pool, user).await.unwrap();
    assert_eq!(ids, vec![1, 3]);
}

#[sqlx::test]
async fn process_emails_subscribers_once_and_clears_rows(pool: SqlitePool) {
    let user_a = insert_active_user(&pool, "a@example.com").await;
    let user_b = insert_active_user(&pool, "b@example.com").await;
    subscribe(&pool, user_a, 7).await.unwrap();
    subscribe(&pool, user_b, 7).await.unwrap();
    subscribe(&pool, user_b, 8).await.unwrap();

    let mailer = Arc::new(RecordingMailer::new());
    let state = state(pool.clone(), mailer.clone());

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

#[sqlx::test]
async fn process_without_subscribers_sends_nothing(pool: SqlitePool) {
    let mailer = Arc::new(RecordingMailer::new());
    let state = state(pool.clone(), mailer.clone());

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
}
