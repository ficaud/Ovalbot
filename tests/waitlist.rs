use ovalbot::common::models::{Status, User};
use ovalbot::services::accounts::waitlist::promote_next_waiting;
use sqlx::SqlitePool;

/// Promotes the oldest waiting user to `Active` and sets `activated_at`.
#[sqlx::test]
async fn promote_activates_oldest_waiting(pool: SqlitePool) {
    let now = ovalbot::common::time::now();
    for i in 0..10 {
        sqlx::query(
            "INSERT INTO users (email, password_hash, status, role, created_at, activated_at) \
             VALUES (?, 'x', 'active', 'user', ?, ?)",
        )
        .bind(format!("active{i}@example.com"))
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO users (email, password_hash, status, role, created_at, activated_at) \
         VALUES (?, 'x', 'waiting', 'user', ?, NULL)",
    )
    .bind("waiting@example.com")
    .bind(&now)
    .execute(&pool)
    .await
    .unwrap();

    promote_next_waiting(&pool).await.unwrap();

    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, status, role, created_at, activated_at, welcome_email_sent_at, telegram_id, notify_email, notify_telegram, shop_email, shop_password_cipher \
         FROM users WHERE email = ?",
    )
    .bind("waiting@example.com")
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(user.status, Status::Active);
    assert!(user.activated_at.is_some());
}
