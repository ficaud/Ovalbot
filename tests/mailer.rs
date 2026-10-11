mod common;

use async_trait::async_trait;
use common::{insert_active_user, RecordingMailer};
use ovalbot::mailer::{send_welcome_if_unsent, MailError, Mailer, MatchAlert};
use sqlx::SqlitePool;

struct FailingMailer;

#[async_trait]
impl Mailer for FailingMailer {
    async fn send_welcome(&self, _email: &str) -> Result<(), MailError> {
        Err(MailError::Api("boom".to_string()))
    }

    async fn send_match_alert(&self, _email: &str, _alert: &MatchAlert) -> Result<(), MailError> {
        Err(MailError::Api("boom".to_string()))
    }
}

#[sqlx::test]
async fn sends_welcome_once_then_skips(pool: SqlitePool) {
    let id = insert_active_user(&pool, "a@example.com").await;
    let mailer = RecordingMailer::new();

    send_welcome_if_unsent(&mailer, &pool, id, "a@example.com").await;
    send_welcome_if_unsent(&mailer, &pool, id, "a@example.com").await;

    let sent = mailer.welcome.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0], "a@example.com");
}

#[sqlx::test]
async fn failed_send_leaves_marker_unset(pool: SqlitePool) {
    let id = insert_active_user(&pool, "a@example.com").await;
    let mailer = FailingMailer;

    send_welcome_if_unsent(&mailer, &pool, id, "a@example.com").await;

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM users WHERE id = ? AND welcome_email_sent_at IS NOT NULL",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
}
