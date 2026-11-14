mod common;

use common::{insert_active_user, insert_user};
use ovalbot::common::error::AppError;
use ovalbot::common::models::Role;
use ovalbot::services::feedback::{admin_emails, list_all, submit, FeedbackInput, FeedbackKind};
use sqlx::SqlitePool;

fn bug(message: &str) -> FeedbackInput {
    FeedbackInput::Bug {
        message: message.to_string(),
    }
}

fn suggestion(message: &str) -> FeedbackInput {
    FeedbackInput::Suggestion {
        message: message.to_string(),
    }
}

/// The submitted report is stored and returned with the author's email.
#[sqlx::test]
async fn submit_stores_and_returns_the_view(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;

    let view = submit(&pool, user, &bug("The Telegram link button does nothing"))
        .await
        .unwrap();
    assert_eq!(view.kind, FeedbackKind::Bug);
    assert_eq!(view.message, "The Telegram link button does nothing");
    assert_eq!(view.reporter_email, "a@example.com");

    let all = list_all(&pool).await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, view.id);
    assert_eq!(all[0].kind, FeedbackKind::Bug);
}

/// The suggestion kind is stored as-is, distinct from the bug kind.
#[sqlx::test]
async fn submit_stores_a_suggestion(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;

    let view = submit(&pool, user, &suggestion("Add a dark mode"))
        .await
        .unwrap();
    assert_eq!(view.kind, FeedbackKind::Suggestion);
    assert_eq!(view.message, "Add a dark mode");
}

/// Blank and oversized messages are refused before touching the DB.
#[sqlx::test]
async fn submit_rejects_blank_and_oversized_messages(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;

    let blank = submit(&pool, user, &bug("   ")).await;
    assert!(matches!(blank, Err(AppError::InvalidFeedback)));

    let oversized = submit(
        &pool,
        user,
        &FeedbackInput::Bug {
            message: "x".repeat(4_001),
        },
    )
    .await;
    assert!(matches!(oversized, Err(AppError::InvalidFeedback)));

    assert_eq!(list_all(&pool).await.unwrap().len(), 0);
}

/// Two reports keep their own author email, and the newest comes first.
#[sqlx::test]
async fn list_all_orders_newest_first(pool: SqlitePool) {
    let first = insert_active_user(&pool, "a@example.com").await;
    let second = insert_active_user(&pool, "b@example.com").await;

    submit(&pool, first, &bug("older report")).await.unwrap();
    // Rewind the clock so the ordering is observable despite same-second
    // timestamps.
    sqlx::query("UPDATE feedback_reports SET created_at = '2000-01-01T00:00:00Z'")
        .execute(&pool)
        .await
        .unwrap();
    submit(&pool, second, &suggestion("newer suggestion"))
        .await
        .unwrap();

    let all = list_all(&pool).await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].message, "newer suggestion");
    assert_eq!(all[0].reporter_email, "b@example.com");
    assert_eq!(all[1].message, "older report");
    assert_eq!(all[1].reporter_email, "a@example.com");
}

/// Deleting a user cascades to their reports (FK `ON DELETE CASCADE`).
#[sqlx::test]
async fn deleting_the_user_cascades(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    submit(&pool, user, &bug("report")).await.unwrap();

    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(list_all(&pool).await.unwrap().len(), 0);
}

/// Only admin accounts are recipients of the report notification.
#[sqlx::test]
async fn admin_emails_lists_only_admins(pool: SqlitePool) {
    insert_active_user(&pool, "user@example.com").await;
    insert_user(
        &pool,
        "admin@example.com",
        ovalbot::common::models::Status::Active,
        Role::Admin,
    )
    .await;

    let emails = admin_emails(&pool).await.unwrap();
    assert_eq!(emails, vec!["admin@example.com".to_string()]);
}
