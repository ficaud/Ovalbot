//! Purchase-matching integration tests: the SQL candidate selection, the
//! progressive fill over claimed seats, and the serving order (oldest first)
//! over a real `ticket_requests` + `users` + `ticket_offers` state.

mod common;

use common::insert_active_user;
use ovalbot::services::bot::purchases::{claim_seat, plan_checkouts};
use ovalbot::services::tickets::{upsert, TicketKind, TicketRequestInput};
use parser::core::seat::{Seat, SeatComposition, SeatId, SeatInfo};
use sqlx::SqlitePool;

// ============================== FIXTURES ==============================

fn single(quantity: u8, max_price_cents: u32) -> TicketRequestInput {
    TicketRequestInput::Single {
        quantity,
        max_price_cents,
    }
}

fn group(places: u8) -> TicketRequestInput {
    TicketRequestInput::Group { places }
}

/// Builds a resale seat with an explicit ticket id, price and category.
fn seat(ticket_id: &str, price: &str, category: &str) -> Seat {
    Seat {
        id: SeatId {
            pack_id: 33727,
            ticket_id: ticket_id.to_string(),
        },
        seat_info: SeatInfo {
            full_name: format!("{category} Place {ticket_id}"),
            composition: SeatComposition {
                category: category.to_string(),
                bloc: "NORD".to_string(),
                row: "U".to_string(),
                seat_number: ticket_id.parse().unwrap_or(0),
            },
        },
        price: price.to_string(),
    }
}

/// Forces the request's `created_at` (single-digit-ordering determinism);
/// upsert stamps `now()` which varies by nanoseconds only.
async fn retime(pool: &SqlitePool, user: i64, match_id: i64, created_at: &str) {
    sqlx::query("UPDATE ticket_requests SET created_at = ? WHERE user_id = ? AND match_id = ?")
        .bind(created_at)
        .bind(user)
        .bind(match_id)
        .execute(pool)
        .await
        .unwrap();
}

/// Grants (or revokes) the user's shop credentials for the matcher.
async fn grant_shop(pool: &SqlitePool, user: i64, email: Option<&str>) {
    sqlx::query("UPDATE users SET shop_email = ? WHERE id = ?")
        .bind(email)
        .bind(user)
        .execute(pool)
        .await
        .unwrap();
}

/// Records an offer in `ticket_offers`, optionally already claimed by a user,
/// mirroring what `record_offers`/`claim_seat` write at scan/purchase time.
async fn offered(pool: &SqlitePool, match_id: i64, ticket_id: &str, claimed_by: Option<i64>) {
    sqlx::query(
        "INSERT INTO ticket_offers (match_id, title, date, pack_id, ticket_id, first_seen_at, claimed_by) \
         VALUES (?, 'Stade Rochelais vs Aviron Bayonnais', 'dimanche 8 novembre 15h00', \
              33727, ?, ?, ?) \
         ON CONFLICT(match_id, pack_id, ticket_id) DO UPDATE SET claimed_by = excluded.claimed_by",
    )
    .bind(match_id)
    .bind(ticket_id)
    .bind(ovalbot::common::time::now())
    .bind(claimed_by)
    .execute(pool)
    .await
    .unwrap();
}

const MATCH: i64 = 11877;

// ============================== MATCHING ==============================

// One credentialed single request within budget is planned with its seat.
#[sqlx::test]
async fn plans_a_credentialed_single_request(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(&pool, user, MATCH, &single(2, 5000)).await.unwrap();
    grant_shop(&pool, user, Some("shop@example.com")).await;

    let plans = plan_checkouts(
        &pool,
        MATCH,
        &[
            seat("s1", "49.00", "CATÉGORIE 3"),
            seat("s2", "60.00", "CATÉGORIE 1"),
        ],
    )
    .await
    .unwrap();

    assert_eq!(plans.len(), 1);
    let plan = &plans[0];
    assert_eq!(plan.user_id, user);
    assert_eq!(plan.match_id, MATCH);
    assert_eq!(plan.kind, TicketKind::Single);
    assert_eq!(plan.seats.len(), 1);
    assert_eq!(plan.seats[0].price, "49.00");
}

// Progressive fill: quota 2, one seat already claimed → only the remaining
// one is planned; the claimed seat itself drops out of the pool though it is
// still listed.
#[sqlx::test]
async fn single_fills_progressively_over_claims(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(&pool, user, MATCH, &single(2, 5000)).await.unwrap();
    grant_shop(&pool, user, Some("shop@example.com")).await;
    offered(&pool, MATCH, "claimed", Some(user)).await;

    let plans = plan_checkouts(
        &pool,
        MATCH,
        &[
            seat("claimed", "45.00", "CATÉGORIE 3"),
            seat("s1", "45.00", "CATÉGORIE 3"),
            seat("s2", "45.00", "CATÉGORIE 3"),
        ],
    )
    .await
    .unwrap();

    assert_eq!(plans.len(), 1);
    // Restant = 2 - 1 = 1 (the claimed listing line is invisible to the pool).
    assert_eq!(plans[0].seats.len(), 1);
    assert_eq!(plans[0].seats[0].id.ticket_id, "s1");
}

// A fully-claimed request is no longer served (waiting for its status move).
#[sqlx::test]
async fn fully_claimed_requests_are_not_served(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(&pool, user, MATCH, &group(2)).await.unwrap();
    grant_shop(&pool, user, Some("shop@example.com")).await;
    offered(&pool, MATCH, "p1", Some(user)).await;
    offered(&pool, MATCH, "p2", Some(user)).await;

    let plans = plan_checkouts(&pool, MATCH, &[seat("p3", "15.00", "Pelouse")])
        .await
        .unwrap();

    assert!(plans.is_empty());
}

// Claims by OTHER users also drop seats out of the shared pool.
#[sqlx::test]
async fn other_users_claims_shrink_the_pool(pool: SqlitePool) {
    let claimant = insert_active_user(&pool, "other@example.com").await;
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(&pool, user, MATCH, &single(4, 5000)).await.unwrap();
    grant_shop(&pool, user, Some("shop@example.com")).await;
    offered(&pool, MATCH, "claimed-by-other", Some(claimant)).await;

    let plans = plan_checkouts(
        &pool,
        MATCH,
        &[seat("claimed-by-other", "45.00", "CATÉGORIE 3")],
    )
    .await
    .unwrap();

    assert!(plans.is_empty());
}

// The oldest request wins a contested seat; without credentials the older
// request is skipped and the younger (credentialed) one takes the seat.
#[sqlx::test]
async fn oldest_credentialed_request_wins_the_contested_seat(pool: SqlitePool) {
    let older_no_creds = insert_active_user(&pool, "older@example.com").await;
    let younger = insert_active_user(&pool, "younger@example.com").await;
    upsert(&pool, older_no_creds, MATCH, &single(1, 6000))
        .await
        .unwrap();
    upsert(&pool, younger, MATCH, &single(1, 6000))
        .await
        .unwrap();
    retime(&pool, older_no_creds, MATCH, "2026-01-01T00:00:00Z").await;
    retime(&pool, younger, MATCH, "2026-02-01T00:00:00Z").await;
    grant_shop(&pool, older_no_creds, None).await;
    grant_shop(&pool, younger, Some("younger-shop@example.com")).await;

    let plans = plan_checkouts(
        &pool,
        MATCH,
        &[
            seat("s1", "55.00", "CATÉGORIE 2"),
            seat("s2", "55.00", "CATÉGORIE 2"),
        ],
    )
    .await
    .unwrap();

    // Two seats available: the older request is skipped (no credentials), so
    // the younger one is served with its own seat.
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].user_id, younger);
    assert_eq!(plans[0].seats.len(), 1);
}

// One single seat, two credentialed requests: the oldest request takes it.
#[sqlx::test]
async fn contested_seat_goes_to_the_oldest_request(pool: SqlitePool) {
    let older = insert_active_user(&pool, "older@example.com").await;
    let younger = insert_active_user(&pool, "younger@example.com").await;
    for user in [older, younger] {
        upsert(&pool, user, MATCH, &single(1, 6000)).await.unwrap();
        grant_shop(&pool, user, Some("shop@example.com")).await;
    }
    retime(&pool, older, MATCH, "2026-01-01T00:00:00Z").await;
    retime(&pool, younger, MATCH, "2026-02-01T00:00:00Z").await;

    let plans = plan_checkouts(&pool, MATCH, &[seat("s1", "55.00", "CATÉGORIE 2")])
        .await
        .unwrap();

    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].user_id, older);
}

// Alert subscriptions (notify) and the out-of-scope duo kind never plan.
#[sqlx::test]
async fn notify_and_duo_are_never_planned(pool: SqlitePool) {
    let notifier = insert_active_user(&pool, "notify@example.com").await;
    let duoer = insert_active_user(&pool, "duo@example.com").await;
    upsert(&pool, notifier, MATCH, &TicketRequestInput::Notify)
        .await
        .unwrap();
    upsert(
        &pool,
        duoer,
        MATCH,
        &TicketRequestInput::Duo {
            max_price_cents: 9000,
        },
    )
    .await
    .unwrap();
    for user in [notifier, duoer] {
        grant_shop(&pool, user, Some("shop@example.com")).await;
    }

    let plans = plan_checkouts(&pool, MATCH, &[seat("s1", "45.00", "Pelouse")])
        .await
        .unwrap();

    assert!(plans.is_empty());
}

// Requests that left pending (claimed / closed) are never served again.
#[sqlx::test]
async fn non_pending_requests_are_not_served(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(&pool, user, MATCH, &single(1, 5000)).await.unwrap();
    grant_shop(&pool, user, Some("shop@example.com")).await;
    sqlx::query(
        "UPDATE ticket_requests SET status = 'accepted' WHERE user_id = ? AND match_id = ?",
    )
    .bind(user)
    .bind(MATCH)
    .execute(&pool)
    .await
    .unwrap();

    let plans = plan_checkouts(&pool, MATCH, &[seat("s1", "45.00", "CATÉGORIE 3")])
        .await
        .unwrap();

    assert!(plans.is_empty());
}

// Group = Single + Pelouse filter: every pelouse seat available is planned,
// up to the remaining quota; the pelouse rate applies (no price match).
#[sqlx::test]
async fn group_takes_the_pelouse_available_progressively(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(&pool, user, MATCH, &group(3)).await.unwrap();
    grant_shop(&pool, user, Some("shop@example.com")).await;

    // Wave 1 — two pelouse listed → 2 planned (remaining 3), non-pelouse
    // untouched.
    let plans = plan_checkouts(
        &pool,
        MATCH,
        &[
            seat("p1", "15.00", "Pelouse"),
            seat("c1", "45.00", "CATÉGORIE 2"),
            seat("p2", "15.00", "PELOUSE"),
        ],
    )
    .await
    .unwrap();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].kind, TicketKind::Group);
    assert_eq!(plans[0].seats.len(), 2);
    assert!(plans[0].seats.iter().all(|s| s
        .seat_info
        .composition
        .category
        .eq_ignore_ascii_case("pelouse")));

    // Wave 2 — the orchestrator ran (claim_seat for both planned seats, the
    // offers having been recorded by the scan's record_offers first), then
    // one more pelouse appears: remaining = 1, so only that one is planned.
    offered(&pool, MATCH, "p1", None).await;
    offered(&pool, MATCH, "p2", None).await;
    assert!(claim_seat(&pool, MATCH, 33727, "p1", user).await.unwrap());
    assert!(claim_seat(&pool, MATCH, 33727, "p2", user).await.unwrap());
    offered(&pool, MATCH, "p3", None).await;
    let plans = plan_checkouts(
        &pool,
        MATCH,
        &[
            seat("p1", "15.00", "Pelouse"),
            seat("p3", "15.00", "pelouse"),
        ],
    )
    .await
    .unwrap();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].seats.len(), 1);
    assert_eq!(plans[0].seats[0].id.ticket_id, "p3");

    // Wave 3 — claim it and the order is complete → nothing planned anymore
    // (even with pelouse still listed, incl. the claimed one).
    assert!(claim_seat(&pool, MATCH, 33727, "p3", user).await.unwrap());
    let plans = plan_checkouts(
        &pool,
        MATCH,
        &[
            seat("p3", "15.00", "pelouse"),
            seat("p4", "15.00", "Pelouse"),
        ],
    )
    .await
    .unwrap();
    assert!(plans.is_empty(), "a full order must not be re-served");
}

// Non-pelouse seats are invisible to a group request.
#[sqlx::test]
async fn group_ignores_non_pelouse_seats(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(&pool, user, MATCH, &group(2)).await.unwrap();
    grant_shop(&pool, user, Some("shop@example.com")).await;

    let plans = plan_checkouts(
        &pool,
        MATCH,
        &[
            seat("c1", "45.00", "CATÉGORIE 1"),
            seat("c2", "55.00", "CATÉGORIE 3"),
        ],
    )
    .await
    .unwrap();

    assert!(plans.is_empty());
}

// A group request without credentials is skipped even with pelouse listed.
#[sqlx::test]
async fn group_without_credentials_is_skipped(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(&pool, user, MATCH, &group(2)).await.unwrap();

    let plans = plan_checkouts(
        &pool,
        MATCH,
        &[
            seat("p1", "15.00", "Pelouse"),
            seat("p2", "15.00", "Pelouse"),
        ],
    )
    .await
    .unwrap();

    assert!(plans.is_empty());
}

// Two competing groups on pelouse seats: the oldest takes the first wave,
// the younger group is served too when seats remain (progressive fill).
#[sqlx::test]
async fn contested_pelouse_seats_give_the_oldest_group(pool: SqlitePool) {
    let older = insert_active_user(&pool, "older@example.com").await;
    let younger = insert_active_user(&pool, "younger@example.com").await;
    for user in [older, younger] {
        upsert(&pool, user, MATCH, &group(2)).await.unwrap();
        grant_shop(&pool, user, Some("shop@example.com")).await;
    }
    retime(&pool, older, MATCH, "2026-01-01T00:00:00Z").await;
    retime(&pool, younger, MATCH, "2026-02-01T00:00:00Z").await;

    // Two pelouse listed: the older group takes exactly its 2, the younger
    // gets nothing this pass (but stays pending for the next wave).
    let one_wave = plan_checkouts(
        &pool,
        MATCH,
        &[
            seat("p1", "15.00", "Pelouse"),
            seat("p2", "15.00", "Pelouse"),
        ],
    )
    .await
    .unwrap();
    assert_eq!(one_wave.len(), 1);
    assert_eq!(one_wave[0].user_id, older);

    // With their prior claims simulated, a next wave a seat appears: the
    // older request is at quota (2/2) so the younger one takes the new seat.
    offered(&pool, MATCH, "p1", Some(older)).await;
    offered(&pool, MATCH, "p2", Some(older)).await;
    let next_wave = plan_checkouts(&pool, MATCH, &[seat("p3", "15.00", "Pelouse")])
        .await
        .unwrap();
    assert_eq!(next_wave.len(), 1);
    assert_eq!(next_wave[0].user_id, younger);
}

// claim_seat is a winner-takes-all: the second concurrent claim loses.
#[sqlx::test]
async fn claim_seat_is_atomic(pool: SqlitePool) {
    let first = insert_active_user(&pool, "first@example.com").await;
    let second = insert_active_user(&pool, "second@example.com").await;
    offered(&pool, MATCH, "p1", None).await;

    assert!(claim_seat(&pool, MATCH, 33727, "p1", first).await.unwrap());
    assert!(!claim_seat(&pool, MATCH, 33727, "p1", second).await.unwrap());
    // A repeated call for the same user loses too (already claimed).
    assert!(!claim_seat(&pool, MATCH, 33727, "p1", first).await.unwrap());

    // Unknown seat: never claimable.
    assert!(!claim_seat(&pool, MATCH, 33727, "ghost", first)
        .await
        .unwrap());
}

// No pending request for the match → no plans, no error (quiet pass).
#[sqlx::test]
async fn no_candidates_is_quiet(pool: SqlitePool) {
    let plans = plan_checkouts(&pool, MATCH, &[seat("p1", "15.00", "Pelouse")])
        .await
        .unwrap();
    assert!(plans.is_empty());
}
