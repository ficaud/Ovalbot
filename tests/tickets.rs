mod common;

use common::insert_active_user;
use ovalbot::common::error::AppError;
use ovalbot::services::tickets::{
    list_for_user, remove, upsert, RemoveOutcome, TicketKind, TicketRequestInput,
    TicketRequestView, TicketStatus,
};
use sqlx::SqlitePool;

fn duo(max_price_cents: u32) -> TicketRequestInput {
    TicketRequestInput::Duo { max_price_cents }
}

fn single(quantity: u8, max_price_cents: u32) -> TicketRequestInput {
    TicketRequestInput::Single {
        quantity,
        max_price_cents,
    }
}

fn group(places: u8) -> TicketRequestInput {
    TicketRequestInput::Group { places }
}

async fn stored(pool: &SqlitePool, user: i64) -> Result<Vec<TicketRequestView>, AppError> {
    list_for_user(pool, user).await
}

/// The first upsert inserts the row and returns the stored view.
#[sqlx::test]
async fn upsert_inserts_and_returns_the_view(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;

    let view = upsert(&pool, user, 42, &duo(9000)).await.unwrap();
    assert_eq!(view.match_id, 42);
    assert_eq!(view.kind, TicketKind::Duo);
    assert_eq!(view.quantity, None);
    assert_eq!(view.max_price_cents, Some(9000));
    assert_eq!(view.places, None);
    assert_eq!(view.status, TicketStatus::Pending);

    assert_eq!(stored(&pool, user).await.unwrap().len(), 1);
}

/// A second upsert on the same pair updates the single stored row in place.
#[sqlx::test]
async fn upsert_twice_updates_in_place(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;

    let first = upsert(&pool, user, 42, &duo(9000)).await.unwrap();

    // Rewind the timestamps: the second upsert must refresh `updated_at`
    // while preserving `created_at`.
    sqlx::query("UPDATE ticket_requests SET updated_at = '2000-01-01T00:00:00Z'")
        .execute(&pool)
        .await
        .unwrap();

    let second = upsert(&pool, user, 42, &single(3, 4500)).await.unwrap();

    let rows = stored(&pool, user).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(second.quantity, Some(3));
    assert_eq!(second.max_price_cents, Some(4500));
    assert_eq!(first.created_at, rows[0].created_at);
    assert_ne!(rows[0].updated_at, "2000-01-01T00:00:00Z");
}

/// Two users requesting the same match do not overwrite each other.
#[sqlx::test]
async fn upsert_keeps_users_apart(pool: SqlitePool) {
    let first = insert_active_user(&pool, "a@example.com").await;
    let second = insert_active_user(&pool, "b@example.com").await;

    upsert(&pool, first, 42, &group(4)).await.unwrap();
    upsert(&pool, second, 42, &duo(9000)).await.unwrap();

    assert_eq!(stored(&pool, first).await.unwrap().len(), 1);
    assert_eq!(stored(&pool, second).await.unwrap().len(), 1);
}

/// Switching kind replaces the outgoing columns with empty ones.
#[sqlx::test]
async fn upsert_switches_kind(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;

    upsert(&pool, user, 42, &duo(9000)).await.unwrap();
    let view = upsert(&pool, user, 42, &group(4)).await.unwrap();

    assert_eq!(view.kind, TicketKind::Group);
    assert_eq!(view.quantity, None);
    assert_eq!(view.max_price_cents, None);
    assert_eq!(view.places, Some(4));
}

/// An upsert on a request that left the `pending` state is rejected.
#[sqlx::test]
async fn upsert_locks_a_non_pending_request(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;

    upsert(&pool, user, 42, &duo(9000)).await.unwrap();
    sqlx::query("UPDATE ticket_requests SET status = 'accepted'")
        .execute(&pool)
        .await
        .unwrap();

    assert!(matches!(
        upsert(&pool, user, 42, &duo(9000)).await,
        Err(AppError::TicketRequestLocked)
    ));
}

/// The list contains only the user's own requests, ordered by match id.
#[sqlx::test]
async fn list_for_user_is_scoped_and_sorted(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    let other = insert_active_user(&pool, "b@example.com").await;

    upsert(&pool, user, 9, &duo(9000)).await.unwrap();
    upsert(&pool, user, 7, &single(2, 4500)).await.unwrap();
    upsert(&pool, other, 8, &group(3)).await.unwrap();

    let views = stored(&pool, user).await.unwrap();
    let ids: Vec<i64> = views.iter().map(|view| view.match_id).collect();
    assert_eq!(ids, vec![7, 9]);

    assert_eq!(stored(&pool, other).await.unwrap().len(), 1);
}

/// Removing reports every outcome and keeps the row when it is locked.
#[sqlx::test]
async fn remove_reports_each_outcome(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;

    // Absent row: not an error.
    assert!(matches!(
        remove(&pool, user, 42).await,
        Ok(RemoveOutcome::NotFound)
    ));

    // Pending row: removed.
    upsert(&pool, user, 42, &duo(9000)).await.unwrap();
    assert!(matches!(
        remove(&pool, user, 42).await,
        Ok(RemoveOutcome::Removed)
    ));
    assert!(stored(&pool, user).await.unwrap().is_empty());

    // Removing again stays idempotent.
    assert!(matches!(
        remove(&pool, user, 42).await,
        Ok(RemoveOutcome::NotFound)
    ));

    // A locked row is protected.
    upsert(&pool, user, 42, &duo(9000)).await.unwrap();
    sqlx::query("UPDATE ticket_requests SET status = 'accepted'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        remove(&pool, user, 42).await,
        Ok(RemoveOutcome::Locked)
    ));
    assert_eq!(stored(&pool, user).await.unwrap().len(), 1);
}

/// Deleting a user removes their requests through the foreign key cascade.
#[sqlx::test]
async fn deleting_user_cascades_their_requests(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(&pool, user, 42, &duo(9000)).await.unwrap();

    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ticket_requests")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

/// The table CHECK constraints reject incoherent rows coming from elsewhere.
#[sqlx::test]
async fn check_constraint_rejects_incoherent_rows(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    let now = ovalbot::common::time::now();

    // A duo cannot carry a quantity.
    let wrong_kind = sqlx::query(
        "INSERT INTO ticket_requests \
             (user_id, match_id, kind, quantity, status, created_at, updated_at) \
         VALUES (?, ?, 'duo', 3, 'pending', ?, ?)",
    )
    .bind(user)
    .bind(42)
    .bind(&now)
    .bind(&now)
    .execute(&pool)
    .await;
    assert!(wrong_kind.is_err());

    // A duo price above the upper bound is rejected.
    let too_expensive = sqlx::query(
        "INSERT INTO ticket_requests \
             (user_id, match_id, kind, max_price_cents, status, created_at, updated_at) \
         VALUES (?, ?, 'duo', 100001, 'pending', ?, ?)",
    )
    .bind(user)
    .bind(42)
    .bind(&now)
    .bind(&now)
    .execute(&pool)
    .await;
    assert!(too_expensive.is_err());

    assert!(stored(&pool, user).await.unwrap().is_empty());
}
