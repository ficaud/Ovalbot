mod common;

use common::insert_active_user;
use ovalbot::common::error::AppError;
use ovalbot::services::tickets::{
    list_for_user, pending_requesters, record_offers, remove, upsert, OfferKey, RemoveOutcome,
    TicketKind, TicketRequestInput, TicketRequestView, TicketStatus,
};
use parser::core::seat::{Seat, SeatComposition, SeatId, SeatInfo};
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

fn resale_seat(pack_id: u64, ticket_id: &str) -> Seat {
    Seat {
        id: SeatId {
            pack_id,
            ticket_id: ticket_id.to_string(),
        },
        seat_info: SeatInfo {
            full_name: format!("Pelouse - Rangée 1, Siège {ticket_id}"),
            composition: SeatComposition {
                category: "Pelouse".to_string(),
                bloc: String::new(),
                row: "1".to_string(),
                seat_number: pack_id,
            },
        },
        price: "45.00 €".to_string(),
    }
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

    // A notify subscription cannot carry any payload column.
    let notify_with_payload = sqlx::query(
        "INSERT INTO ticket_requests \
             (user_id, match_id, kind, quantity, status, created_at, updated_at) \
         VALUES (?, ?, 'notify', 3, 'pending', ?, ?)",
    )
    .bind(user)
    .bind(42)
    .bind(&now)
    .bind(&now)
    .execute(&pool)
    .await;
    assert!(notify_with_payload.is_err());

    assert!(stored(&pool, user).await.unwrap().is_empty());
}

// ============================== PENDING REQUESTERS (BOT) ==============================

/// `record_offers` inserts the seat rows and reports exactly this call's
/// inserts; seats already recorded by a previous call come back empty.
#[sqlx::test]
async fn record_offers_reports_only_fresh_seats(pool: SqlitePool) {
    let seats = vec![resale_seat(33727, "49255"), resale_seat(33727, "49256")];

    let fresh = record_offers(
        &pool,
        11864,
        "UNION BORDEAUX BÈGLES",
        "dimanche 25 octobre à 21h05",
        &seats,
    )
    .await
    .unwrap();
    assert_eq!(
        fresh,
        vec![
            OfferKey {
                pack_id: 33727,
                ticket_id: "49255".to_string(),
            },
            OfferKey {
                pack_id: 33727,
                ticket_id: "49256".to_string(),
            },
        ]
    );

    // Same scan on repeat: everything already recorded, nothing fresh.
    let again = record_offers(
        &pool,
        11864,
        "UNION BORDEAUX BÈGLES",
        "dimanche 25 octobre à 21h05",
        &seats,
    )
    .await
    .unwrap();
    assert!(again.is_empty());
}

/// The dedup key includes the match id: the same seat listing on another
/// match is a genuinely fresh offer (seat retrieval is per-product site).
#[sqlx::test]
async fn record_offers_dedups_per_match(pool: SqlitePool) {
    let seats = vec![resale_seat(33727, "49255")];

    record_offers(
        &pool,
        11864,
        "UNION BORDEAUX BÈGLES",
        "dimanche 25 octobre à 21h05",
        &seats,
    )
    .await
    .unwrap();

    let fresh_elsewhere = record_offers(
        &pool,
        11877,
        "AVIRON BAYONNAIS",
        "dimanche 8 novembre à 21h05",
        &seats,
    )
    .await
    .unwrap();
    assert_eq!(fresh_elsewhere.len(), 1);
}

/// An encounter with no seats records and reports nothing.
#[sqlx::test]
async fn record_offers_without_seats_is_a_noop(pool: SqlitePool) {
    let fresh = record_offers(
        &pool,
        11864,
        "UNION BORDEAUX BÈGLES",
        "dimanche 25 octobre à 21h05",
        &[],
    )
    .await
    .unwrap();
    assert!(fresh.is_empty());

    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ticket_offers")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, 0);
}

/// A notification-only subscription upserts like the other kinds, with every
/// payload column left empty.
#[sqlx::test]
async fn notify_upsert_stores_null_payload_columns(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;

    let view = upsert(&pool, user, 42, &TicketRequestInput::Notify)
        .await
        .unwrap();
    assert_eq!(view.kind, TicketKind::Notify);
    assert_eq!(view.quantity, None);
    assert_eq!(view.max_price_cents, None);
    assert_eq!(view.places, None);
    assert_eq!(view.status, TicketStatus::Pending);
    assert_eq!(stored(&pool, user).await.unwrap().len(), 1);
}

/// A notify subscription turns into a real buying request on the next upsert
/// (one row per user+match, the kind switch replaces the payload columns).
#[sqlx::test]
async fn notify_switches_to_a_buying_kind_in_place(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;

    upsert(&pool, user, 42, &TicketRequestInput::Notify)
        .await
        .unwrap();
    let view = upsert(&pool, user, 42, &duo(9000)).await.unwrap();

    assert_eq!(view.kind, TicketKind::Duo);
    assert_eq!(view.max_price_cents, Some(9000));
}

/// `pending_requesters` lists only the explicit alert subscriptions
/// (`kind = 'notify'`): buying kinds (Single/Duo/Group) are never alert
/// candidates, ordered by user id, with contact info and channel gates.
#[sqlx::test]
async fn pending_requesters_lists_only_notify_subscriptions(pool: SqlitePool) {
    let first = insert_active_user(&pool, "a@example.com").await;
    let second = insert_active_user(&pool, "b@example.com").await;
    let third = insert_active_user(&pool, "c@example.com").await;
    let fourth = insert_active_user(&pool, "d@example.com").await;

    upsert(&pool, second, 11877, &duo(9000)).await.unwrap();
    upsert(&pool, first, 11877, &single(2, 4500)).await.unwrap();
    upsert(&pool, third, 11877, &group(6)).await.unwrap();
    upsert(&pool, fourth, 11877, &TicketRequestInput::Notify)
        .await
        .unwrap();
    upsert(&pool, fourth, 42, &duo(5000)).await.unwrap();
    sqlx::query("UPDATE users SET telegram_id = '123' WHERE id = ?")
        .bind(fourth)
        .execute(&pool)
        .await
        .unwrap();

    let requesters = pending_requesters(&pool, 11877).await.unwrap();
    // Exactly one candidate: the notify subscriber. The three buying kinds on
    // the same match are excluded.
    assert_eq!(requesters.len(), 1);
    assert_eq!(requesters[0].user_id, fourth);

    assert_eq!(requesters[0].email, "d@example.com");
    assert_eq!(requesters[0].telegram_id.as_deref(), Some("123"));
    assert!(requesters[0].notify_email && requesters[0].notify_telegram);

    // A notify-only subscription is an alert candidate with no buying payload.
    assert_eq!(requesters[0].kind, TicketKind::Notify);
    assert_eq!(requesters[0].quantity, None);
    assert_eq!(requesters[0].max_price_cents, None);
    assert_eq!(requesters[0].places, None);

    // A buying request on another match (same user, match 42) never leaks in.
    assert!(pending_requesters(&pool, 42).await.unwrap().is_empty());
}

/// Non-pending notify subscriptions are not alert candidates.
#[sqlx::test]
async fn pending_requesters_skips_non_pending_requests(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(&pool, user, 11877, &TicketRequestInput::Notify)
        .await
        .unwrap();
    sqlx::query("UPDATE ticket_requests SET status = 'accepted'")
        .execute(&pool)
        .await
        .unwrap();

    assert!(pending_requesters(&pool, 11877).await.unwrap().is_empty());
}

/// Match scoping: alert subscriptions on other matches never leak in.
#[sqlx::test]
async fn pending_requesters_is_scoped_to_the_match(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(&pool, user, 42, &TicketRequestInput::Notify)
        .await
        .unwrap();
    upsert(&pool, user, 11877, &TicketRequestInput::Notify)
        .await
        .unwrap();

    let requesters = pending_requesters(&pool, 11877).await.unwrap();
    assert_eq!(requesters.len(), 1);
    assert_eq!(requesters[0].kind, TicketKind::Notify);

    // Nobody subscribed to match 999.
    assert!(pending_requesters(&pool, 999).await.unwrap().is_empty());
}
