mod common;

use common::insert_active_user;
use ovalbot::services::channels::telegram::{consume_link_token, create_link_token, unlink};
use sqlx::SqlitePool;

async fn telegram_id(pool: &SqlitePool, user_id: i64) -> Option<String> {
    sqlx::query_scalar("SELECT telegram_id FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// A freshly created token links the chat to the user when consumed.
#[sqlx::test]
async fn consume_links_chat_to_user(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;

    let token = create_link_token(&pool, user).await.unwrap();
    let linked = consume_link_token(&pool, &token, "12345").await.unwrap();

    assert!(linked);
    assert_eq!(telegram_id(&pool, user).await.as_deref(), Some("12345"));
}

/// A token can only be used once.
#[sqlx::test]
async fn token_is_single_use(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    let token = create_link_token(&pool, user).await.unwrap();

    assert!(consume_link_token(&pool, &token, "12345").await.unwrap());
    assert!(!consume_link_token(&pool, &token, "99999").await.unwrap());
    assert_eq!(telegram_id(&pool, user).await.as_deref(), Some("12345"));
}

/// An unknown token is rejected.
#[sqlx::test]
async fn unknown_token_is_rejected(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;

    assert!(!consume_link_token(&pool, "deadbeef", "12345")
        .await
        .unwrap());
    assert!(telegram_id(&pool, user).await.is_none());
}

/// An expired token is rejected and does not link the chat.
#[sqlx::test]
async fn expired_token_is_rejected(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    sqlx::query(
        "INSERT INTO telegram_link_tokens (token, user_id, created_at, expires_at) \
         VALUES ('expired', ?, '2000-01-01T00:00:00Z', '2000-01-01T00:00:00Z')",
    )
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();

    assert!(!consume_link_token(&pool, "expired", "12345").await.unwrap());
    assert!(telegram_id(&pool, user).await.is_none());
}

/// A chat already linked to another user cannot be linked again.
#[sqlx::test]
async fn chat_already_linked_is_rejected(pool: SqlitePool) {
    let user_a = insert_active_user(&pool, "a@example.com").await;
    let user_b = insert_active_user(&pool, "b@example.com").await;

    let token_a = create_link_token(&pool, user_a).await.unwrap();
    assert!(consume_link_token(&pool, &token_a, "12345").await.unwrap());

    let token_b = create_link_token(&pool, user_b).await.unwrap();
    assert!(!consume_link_token(&pool, &token_b, "12345").await.unwrap());
    assert!(telegram_id(&pool, user_b).await.is_none());
}

/// Unlinking clears the previously stored chat.
#[sqlx::test]
async fn unlink_clears_telegram_id(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    let token = create_link_token(&pool, user).await.unwrap();
    consume_link_token(&pool, &token, "12345").await.unwrap();

    unlink(&pool, user).await.unwrap();

    assert!(telegram_id(&pool, user).await.is_none());
}
