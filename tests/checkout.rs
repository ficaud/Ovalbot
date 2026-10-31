//! Checkout orchestrator integration tests: flags, credentials handling,
//! progressive claims, per-wave notifications and the failure semantics — all
//! against a scripted fake shop (no network, no real billetterie).

mod common;

use std::sync::Arc;

use common::{
    insert_active_user, LoginOutcome, RecordingMailer, RecordingShop, RecordingTelegram,
    SeatOutcome,
};
use ovalbot::common::crypto::SecretKey;
use ovalbot::services::accounts::shop::set_credentials;
use ovalbot::services::bot::checkout::{execute_plans, CartShop, CheckoutTarget, CART_URL};
use ovalbot::services::bot::purchases::PlanCheckout;
use ovalbot::services::calendar::CalendarService;
use ovalbot::services::channels::telegram::TelegramConfig;
use ovalbot::services::state::AppState;
use ovalbot::services::tickets::{
    list_for_user, upsert, TicketKind, TicketRequestInput, TicketStatus,
};
use parser::core::seat::{Seat, SeatComposition, SeatId, SeatInfo};
use sqlx::SqlitePool;

// ============================== FIXTURES ==============================

const MATCH: i64 = 11877;

/// Builds a resale seat with an explicit ticket id and category.
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

/// Records the seats in `ticket_offers` (the scan's `record_offers` step) so
/// `claim_seat` can win them.
async fn offered(pool: &SqlitePool, seats: &[&Seat]) {
    for seat in seats {
        sqlx::query(
            "INSERT INTO ticket_offers (match_id, title, date, pack_id, ticket_id, first_seen_at) \
             VALUES (?, 'AVIRON BAYONNAIS', 'dimanche 8 novembre à 21h05', ?, ?, ?) \
             ON CONFLICT(match_id, pack_id, ticket_id) DO NOTHING",
        )
        .bind(MATCH)
        .bind(seat.id.pack_id as i64)
        .bind(&seat.id.ticket_id)
        .bind(ovalbot::common::time::now())
        .execute(pool)
        .await
        .unwrap();
    }
}

/// A plan for one user, as the matcher would emit it.
fn plan(user_id: i64, kind: TicketKind, total: u32, seats: Vec<Seat>) -> PlanCheckout {
    PlanCheckout {
        user_id,
        match_id: MATCH,
        kind,
        total,
        seats,
    }
}

fn target() -> CheckoutTarget {
    CheckoutTarget {
        match_id: MATCH,
        title: "AVIRON BAYONNAIS".to_string(),
        date: "dimanche 8 novembre à 21h05".to_string(),
        resale_link: "https://billetterie.staderochelais.com/fr/product/1357/revente".to_string(),
    }
}

/// Stores the user's shop credentials (encrypted with the shared test key) and
/// the given channel preferences.
async fn profile(pool: &SqlitePool, user_id: i64, email: bool, telegram: Option<&str>) {
    let key = SecretKey::from_passphrase("test-secret");
    set_credentials(pool, user_id, "shop@example.com", "shop-pw", Some(&key))
        .await
        .unwrap();
    // notify_email = email; notify_telegram = telegram.is_some()
    sqlx::query(
        "UPDATE users SET notify_email = ?, notify_telegram = ?, telegram_id = ? WHERE id = ?",
    )
    .bind(email)
    .bind(telegram.is_some())
    .bind(telegram)
    .bind(user_id)
    .execute(pool)
    .await
    .unwrap();
}

/// State with the checkout enabled and the given dry-run flag, wired to
/// recording mails/Telegram and the scripted fake shop.
fn state(
    pool: SqlitePool,
    mailer: Arc<RecordingMailer>,
    telegram: Arc<RecordingTelegram>,
    shop: Arc<dyn CartShop>,
    dry_run: bool,
) -> AppState {
    AppState {
        pool,
        mailer,
        telegram,
        telegram_config: TelegramConfig::default(),
        calendar: Arc::new(CalendarService::new()),
        shop_key: Some(SecretKey::from_passphrase("test-secret")),
        cart_enabled: true,
        cart_dry_run: dry_run,
        cart_shop: shop,
    }
}

async fn status(pool: &SqlitePool, user_id: i64) -> TicketStatus {
    list_for_user(pool, user_id)
        .await
        .unwrap()
        .into_iter()
        .find(|request| request.match_id == MATCH)
        .expect("request missing")
        .status
}

async fn claimed(pool: &SqlitePool, user_id: i64) -> i64 {
    ovalbot::services::bot::purchases::claimed_count(pool, MATCH, user_id)
        .await
        .unwrap()
}

// ============================== FLAGS & CREDENTIALS ==============================

// Dry-run: the plan is logged, the shop is never contacted, nothing is
// claimed and nobody is notified — the request keeps waiting.
#[sqlx::test]
async fn dry_run_does_not_touch_the_shop(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(
        &pool,
        user,
        MATCH,
        &TicketRequestInput::Single {
            quantity: 1,
            max_price_cents: 5000,
        },
    )
    .await
    .unwrap();
    profile(&pool, user, true, None).await;
    let s = seat("s1", "45.00", "CATÉGORIE 3");
    offered(&pool, &[&s]).await;

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let shop = Arc::new(RecordingShop::new());
    let state = state(
        pool.clone(),
        mailer.clone(),
        telegram.clone(),
        shop.clone(),
        true,
    );

    execute_plans(
        &state,
        &target(),
        vec![plan(user, TicketKind::Single, 1, vec![s.clone()])],
    )
    .await;

    assert!(shop.logins.lock().unwrap().is_empty());
    assert!(mailer.cart_alerts.lock().unwrap().is_empty());
    assert!(telegram.messages.lock().unwrap().is_empty());
    assert_eq!(claimed(&pool, user).await, 0);
    assert_eq!(status(&pool, user).await, TicketStatus::Pending);
}

// A user without stored credentials cannot be served: no login attempt, no
// notification, the request stays pending.
#[sqlx::test]
async fn no_credentials_skips_the_plan(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(
        &pool,
        user,
        MATCH,
        &TicketRequestInput::Single {
            quantity: 1,
            max_price_cents: 5000,
        },
    )
    .await
    .unwrap();

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let shop = Arc::new(RecordingShop::new());
    let state = state(
        pool.clone(),
        mailer.clone(),
        telegram.clone(),
        shop.clone(),
        false,
    );

    execute_plans(
        &state,
        &target(),
        vec![plan(
            user,
            TicketKind::Single,
            1,
            vec![seat("s1", "45.00", "CATÉGORIE 3")],
        )],
    )
    .await;

    assert!(shop.logins.lock().unwrap().is_empty());
    assert!(mailer.cart_alerts.lock().unwrap().is_empty());
    assert_eq!(status(&pool, user).await, TicketStatus::Pending);
}

// A rotated SECRET_KEY (stored cipher unreadable) skips the plan too.
#[sqlx::test]
async fn unreadable_credentials_skip_the_plan(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(
        &pool,
        user,
        MATCH,
        &TicketRequestInput::Single {
            quantity: 1,
            max_price_cents: 5000,
        },
    )
    .await
    .unwrap();
    let key = SecretKey::from_passphrase("the-key-used-to-store");
    set_credentials(&pool, user, "shop@example.com", "shop-pw", Some(&key))
        .await
        .unwrap();

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let shop = Arc::new(RecordingShop::new());
    let mut state = state(
        pool.clone(),
        mailer.clone(),
        telegram.clone(),
        shop.clone(),
        false,
    );
    state.shop_key = Some(SecretKey::from_passphrase("a-different-key"));

    execute_plans(
        &state,
        &target(),
        vec![plan(
            user,
            TicketKind::Single,
            1,
            vec![seat("s1", "45.00", "CATÉGORIE 3")],
        )],
    )
    .await;

    assert!(shop.logins.lock().unwrap().is_empty());
    assert_eq!(status(&pool, user).await, TicketStatus::Pending);
}

// A dry run tolerates an unreadable vault (missing/rotated SECRET_KEY): it
// only reports the credential state, never touches the shop.
#[sqlx::test]
async fn dry_run_without_vault_stays_quiet(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(
        &pool,
        user,
        MATCH,
        &TicketRequestInput::Single {
            quantity: 1,
            max_price_cents: 5000,
        },
    )
    .await
    .unwrap();
    let key = SecretKey::from_passphrase("the-key-used-to-store");
    set_credentials(&pool, user, "shop@example.com", "shop-pw", Some(&key))
        .await
        .unwrap();

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let shop = Arc::new(RecordingShop::new());
    let mut state = state(
        pool.clone(),
        mailer.clone(),
        telegram.clone(),
        shop.clone(),
        true,
    );
    state.shop_key = None;

    execute_plans(
        &state,
        &target(),
        vec![plan(
            user,
            TicketKind::Single,
            1,
            vec![seat("s1", "45.00", "CATÉGORIE 3")],
        )],
    )
    .await;

    assert!(shop.logins.lock().unwrap().is_empty());
    assert!(mailer.cart_alerts.lock().unwrap().is_empty());
    assert!(telegram.messages.lock().unwrap().is_empty());
    assert_eq!(status(&pool, user).await, TicketStatus::Pending);
}

// ============================== HAPPY PATHS ==============================

// A wave that completes a Single order: claim recorded, request accepted,
// one cart notification per channel.
#[sqlx::test]
async fn completed_wave_claims_accepts_and_notifies(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(
        &pool,
        user,
        MATCH,
        &TicketRequestInput::Single {
            quantity: 1,
            max_price_cents: 5000,
        },
    )
    .await
    .unwrap();
    profile(&pool, user, true, Some("chat-42")).await;
    let s = seat("s1", "45.00", "CATÉGORIE 3");
    offered(&pool, &[&s]).await;

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let shop = Arc::new(RecordingShop::new());
    let state = state(
        pool.clone(),
        mailer.clone(),
        telegram.clone(),
        shop.clone(),
        false,
    );

    execute_plans(
        &state,
        &target(),
        vec![plan(user, TicketKind::Single, 1, vec![s.clone()])],
    )
    .await;

    // The shop was driven exactly once, with the decrypted credentials.
    assert_eq!(
        *shop.logins.lock().unwrap(),
        vec!["shop@example.com".to_string()]
    );
    assert_eq!(*shop.added.lock().unwrap(), vec!["s1".to_string()]);

    // Claim recorded and the order closed as accepted.
    assert_eq!(claimed(&pool, user).await, 1);
    assert_eq!(status(&pool, user).await, TicketStatus::Accepted);

    // One cart alert on each configured channel.
    let mails = mailer.cart_alerts.lock().unwrap();
    assert_eq!(mails.len(), 1);
    assert_eq!(mails[0].0, "a@example.com");
    assert_eq!(mails[0].1.added, 1);
    assert_eq!(mails[0].1.total, 1);
    assert!(!mails[0].1.failed);
    assert_eq!(mails[0].1.cart_link, CART_URL);
    let messages = telegram.messages.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].0, "chat-42");
    assert!(messages[0].1.contains("1/1"));
}

// A progressive wave on a 4-place Group order: 2 seats claimed, request still
// pending, notification carries the 2/4 progress.
#[sqlx::test]
async fn partial_wave_stays_pending_with_progress(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(&pool, user, MATCH, &TicketRequestInput::Group { places: 4 })
        .await
        .unwrap();
    profile(&pool, user, true, None).await;
    let p1 = seat("p1", "15.00", "Pelouse");
    let p2 = seat("p2", "15.00", "Pelouse");
    offered(&pool, &[&p1, &p2]).await;

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let shop = Arc::new(RecordingShop::new());
    let state = state(
        pool.clone(),
        mailer.clone(),
        telegram.clone(),
        shop.clone(),
        false,
    );

    execute_plans(
        &state,
        &target(),
        vec![plan(
            user,
            TicketKind::Group,
            4,
            vec![p1.clone(), p2.clone()],
        )],
    )
    .await;

    assert_eq!(claimed(&pool, user).await, 2);
    assert_eq!(status(&pool, user).await, TicketStatus::Pending);
    let mails = mailer.cart_alerts.lock().unwrap();
    assert_eq!(mails.len(), 1);
    assert_eq!(mails[0].1.added, 2);
    assert_eq!(mails[0].1.total, 4);
}

// Two waves fill the order: the second one reaches 4/4 and closes it.
#[sqlx::test]
async fn progressive_waves_complete_the_order(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(&pool, user, MATCH, &TicketRequestInput::Group { places: 4 })
        .await
        .unwrap();
    profile(&pool, user, true, None).await;
    let (p1, p2) = (
        seat("p1", "15.00", "Pelouse"),
        seat("p2", "15.00", "Pelouse"),
    );
    let (p3, p4) = (
        seat("p3", "15.00", "Pelouse"),
        seat("p4", "15.00", "Pelouse"),
    );
    offered(&pool, &[&p1, &p2, &p3, &p4]).await;

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let shop = Arc::new(RecordingShop::new());
    let state = state(
        pool.clone(),
        mailer.clone(),
        telegram.clone(),
        shop.clone(),
        false,
    );

    execute_plans(
        &state,
        &target(),
        vec![plan(
            user,
            TicketKind::Group,
            4,
            vec![p1.clone(), p2.clone()],
        )],
    )
    .await;
    assert_eq!(status(&pool, user).await, TicketStatus::Pending);

    execute_plans(
        &state,
        &target(),
        vec![plan(
            user,
            TicketKind::Group,
            4,
            vec![p3.clone(), p4.clone()],
        )],
    )
    .await;
    assert_eq!(claimed(&pool, user).await, 4);
    assert_eq!(status(&pool, user).await, TicketStatus::Accepted);

    // One notification per wave.
    let mails = mailer.cart_alerts.lock().unwrap();
    assert_eq!(mails.len(), 2);
    assert_eq!((mails[0].1.added, mails[0].1.total), (2, 4));
    assert_eq!((mails[1].1.added, mails[1].1.total), (2, 4));
}

// ============================== FAILURES ==============================

// SeatGone is resale churn: the seat is skipped, the others are added and the
// request stays pending for the next wave.
#[sqlx::test]
async fn seat_gone_is_tolerated(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(
        &pool,
        user,
        MATCH,
        &TicketRequestInput::Single {
            quantity: 2,
            max_price_cents: 5000,
        },
    )
    .await
    .unwrap();
    profile(&pool, user, true, None).await;
    let s1 = seat("s1", "45.00", "CATÉGORIE 3");
    let s2 = seat("s2", "45.00", "CATÉGORIE 3");
    offered(&pool, &[&s1, &s2]).await;

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let shop = Arc::new(
        RecordingShop::new().with_outcomes(vec![SeatOutcome::Added, SeatOutcome::SeatGone]),
    );
    let state = state(
        pool.clone(),
        mailer.clone(),
        telegram.clone(),
        shop.clone(),
        false,
    );

    execute_plans(
        &state,
        &target(),
        vec![plan(
            user,
            TicketKind::Single,
            2,
            vec![s1.clone(), s2.clone()],
        )],
    )
    .await;

    assert_eq!(claimed(&pool, user).await, 1);
    assert_eq!(status(&pool, user).await, TicketStatus::Pending);
    let mails = mailer.cart_alerts.lock().unwrap();
    assert_eq!(mails.len(), 1);
    assert_eq!(mails[0].1.added, 1);
    assert!(!mails[0].1.failed);
}

// A terminal add error closes the request as failed, notifying what is already
// in the cart.
#[sqlx::test]
async fn hard_add_failure_marks_failed(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(
        &pool,
        user,
        MATCH,
        &TicketRequestInput::Single {
            quantity: 2,
            max_price_cents: 5000,
        },
    )
    .await
    .unwrap();
    profile(&pool, user, true, None).await;
    let s1 = seat("s1", "45.00", "CATÉGORIE 3");
    let s2 = seat("s2", "45.00", "CATÉGORIE 3");
    offered(&pool, &[&s1, &s2]).await;

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let shop = Arc::new(
        RecordingShop::new().with_outcomes(vec![SeatOutcome::Added, SeatOutcome::MissingContext]),
    );
    let state = state(
        pool.clone(),
        mailer.clone(),
        telegram.clone(),
        shop.clone(),
        false,
    );

    execute_plans(
        &state,
        &target(),
        vec![plan(
            user,
            TicketKind::Single,
            2,
            vec![s1.clone(), s2.clone()],
        )],
    )
    .await;

    assert_eq!(claimed(&pool, user).await, 1);
    assert_eq!(status(&pool, user).await, TicketStatus::Failed);
    let mails = mailer.cart_alerts.lock().unwrap();
    assert_eq!(mails.len(), 1);
    assert!(mails[0].1.failed);
    assert_eq!(mails[0].1.added, 1);
}

// An expired form mid-wave postpones the rest: what was added is reported and
// the request keeps waiting (no terminal failure).
#[sqlx::test]
async fn expired_form_keeps_request_pending(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(
        &pool,
        user,
        MATCH,
        &TicketRequestInput::Single {
            quantity: 2,
            max_price_cents: 5000,
        },
    )
    .await
    .unwrap();
    profile(&pool, user, true, None).await;
    let s1 = seat("s1", "45.00", "CATÉGORIE 3");
    let s2 = seat("s2", "45.00", "CATÉGORIE 3");
    offered(&pool, &[&s1, &s2]).await;

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let shop = Arc::new(
        RecordingShop::new().with_outcomes(vec![SeatOutcome::Added, SeatOutcome::ExpiredForm]),
    );
    let state = state(
        pool.clone(),
        mailer.clone(),
        telegram.clone(),
        shop.clone(),
        false,
    );

    execute_plans(
        &state,
        &target(),
        vec![plan(
            user,
            TicketKind::Single,
            2,
            vec![s1.clone(), s2.clone()],
        )],
    )
    .await;

    assert_eq!(claimed(&pool, user).await, 1);
    assert_eq!(status(&pool, user).await, TicketStatus::Pending);
    let mails = mailer.cart_alerts.lock().unwrap();
    assert_eq!(mails.len(), 1);
    assert_eq!(mails[0].1.added, 1);
    assert!(!mails[0].1.failed);
}

// A transport error mid-wave is retried later too: same silent postponement.
#[sqlx::test]
async fn network_error_keeps_request_pending(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(
        &pool,
        user,
        MATCH,
        &TicketRequestInput::Single {
            quantity: 2,
            max_price_cents: 5000,
        },
    )
    .await
    .unwrap();
    profile(&pool, user, true, None).await;
    let s1 = seat("s1", "45.00", "CATÉGORIE 3");
    let s2 = seat("s2", "45.00", "CATÉGORIE 3");
    offered(&pool, &[&s1, &s2]).await;

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let shop = Arc::new(
        RecordingShop::new().with_outcomes(vec![SeatOutcome::Added, SeatOutcome::Network]),
    );
    let state = state(
        pool.clone(),
        mailer.clone(),
        telegram.clone(),
        shop.clone(),
        false,
    );

    execute_plans(
        &state,
        &target(),
        vec![plan(
            user,
            TicketKind::Single,
            2,
            vec![s1.clone(), s2.clone()],
        )],
    )
    .await;

    assert_eq!(claimed(&pool, user).await, 1);
    assert_eq!(status(&pool, user).await, TicketStatus::Pending);
    let mails = mailer.cart_alerts.lock().unwrap();
    assert_eq!(mails.len(), 1);
    assert_eq!(mails[0].1.added, 1);
    assert!(!mails[0].1.failed);
}

// A refused login is terminal: the request fails and the user is told.
#[sqlx::test]
async fn login_failure_marks_failed(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(
        &pool,
        user,
        MATCH,
        &TicketRequestInput::Single {
            quantity: 1,
            max_price_cents: 5000,
        },
    )
    .await
    .unwrap();
    profile(&pool, user, true, Some("chat-7")).await;
    let s1 = seat("s1", "45.00", "CATÉGORIE 3");
    offered(&pool, &[&s1]).await;

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let shop = Arc::new(RecordingShop::new().refusing());
    let state = state(
        pool.clone(),
        mailer.clone(),
        telegram.clone(),
        shop.clone(),
        false,
    );

    execute_plans(
        &state,
        &target(),
        vec![plan(user, TicketKind::Single, 1, vec![s1.clone()])],
    )
    .await;

    assert_eq!(
        *shop.logins.lock().unwrap(),
        vec!["shop@example.com".to_string()]
    );
    assert_eq!(status(&pool, user).await, TicketStatus::Failed);
    let mails = mailer.cart_alerts.lock().unwrap();
    assert_eq!(mails.len(), 1);
    assert!(mails[0].1.failed);
    assert_eq!(mails[0].1.added, 0);
    assert_eq!(telegram.messages.lock().unwrap().len(), 1);
}

// A waiting queue at login is site churn: the wave gives up quietly, nothing is
// claimed, nobody is notified, and the request stays pending.
#[sqlx::test]
async fn login_waiting_queue_keeps_request_pending(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(
        &pool,
        user,
        MATCH,
        &TicketRequestInput::Single {
            quantity: 1,
            max_price_cents: 5000,
        },
    )
    .await
    .unwrap();
    profile(&pool, user, true, Some("chat-7")).await;
    let s1 = seat("s1", "45.00", "CATÉGORIE 3");
    offered(&pool, &[&s1]).await;

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let shop = Arc::new(RecordingShop::new().unavailable(LoginOutcome::Queue));
    let state = state(
        pool.clone(),
        mailer.clone(),
        telegram.clone(),
        shop.clone(),
        false,
    );

    execute_plans(
        &state,
        &target(),
        vec![plan(user, TicketKind::Single, 1, vec![s1.clone()])],
    )
    .await;

    assert_eq!(shop.logins.lock().unwrap().len(), 1);
    assert_eq!(claimed(&pool, user).await, 0);
    assert_eq!(status(&pool, user).await, TicketStatus::Pending);
    assert!(mailer.cart_alerts.lock().unwrap().is_empty());
    assert!(telegram.messages.lock().unwrap().is_empty());
}

// A network error at login is retried later too: same pending, same silence.
#[sqlx::test]
async fn login_network_error_keeps_request_pending(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(
        &pool,
        user,
        MATCH,
        &TicketRequestInput::Single {
            quantity: 1,
            max_price_cents: 5000,
        },
    )
    .await
    .unwrap();
    profile(&pool, user, true, Some("chat-7")).await;
    let s1 = seat("s1", "45.00", "CATÉGORIE 3");
    offered(&pool, &[&s1]).await;

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let shop = Arc::new(RecordingShop::new().unavailable(LoginOutcome::Network));
    let state = state(
        pool.clone(),
        mailer.clone(),
        telegram.clone(),
        shop.clone(),
        false,
    );

    execute_plans(
        &state,
        &target(),
        vec![plan(user, TicketKind::Single, 1, vec![s1.clone()])],
    )
    .await;

    assert_eq!(shop.logins.lock().unwrap().len(), 1);
    assert_eq!(claimed(&pool, user).await, 0);
    assert_eq!(status(&pool, user).await, TicketStatus::Pending);
    assert!(mailer.cart_alerts.lock().unwrap().is_empty());
    assert!(telegram.messages.lock().unwrap().is_empty());
}

// A wave where every seat vanished sends nothing (nothing to pay).
#[sqlx::test]
async fn all_seats_gone_sends_no_notification(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(
        &pool,
        user,
        MATCH,
        &TicketRequestInput::Single {
            quantity: 1,
            max_price_cents: 5000,
        },
    )
    .await
    .unwrap();
    profile(&pool, user, true, Some("chat-7")).await;
    let s1 = seat("s1", "45.00", "CATÉGORIE 3");
    offered(&pool, &[&s1]).await;

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let shop = Arc::new(RecordingShop::new().with_outcomes(vec![SeatOutcome::SeatGone]));
    let state = state(
        pool.clone(),
        mailer.clone(),
        telegram.clone(),
        shop.clone(),
        false,
    );

    execute_plans(
        &state,
        &target(),
        vec![plan(user, TicketKind::Single, 1, vec![s1.clone()])],
    )
    .await;

    assert_eq!(status(&pool, user).await, TicketStatus::Pending);
    assert!(mailer.cart_alerts.lock().unwrap().is_empty());
    assert!(telegram.messages.lock().unwrap().is_empty());
}

// Both channel preferences muted: the cart is filled but nothing is sent.
#[sqlx::test]
async fn muted_channels_receive_nothing(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    upsert(
        &pool,
        user,
        MATCH,
        &TicketRequestInput::Single {
            quantity: 1,
            max_price_cents: 5000,
        },
    )
    .await
    .unwrap();
    profile(&pool, user, false, None).await;
    let s1 = seat("s1", "45.00", "CATÉGORIE 3");
    offered(&pool, &[&s1]).await;

    let mailer = Arc::new(RecordingMailer::new());
    let telegram = Arc::new(RecordingTelegram::new());
    let shop = Arc::new(RecordingShop::new());
    let state = state(
        pool.clone(),
        mailer.clone(),
        telegram.clone(),
        shop.clone(),
        false,
    );

    execute_plans(
        &state,
        &target(),
        vec![plan(user, TicketKind::Single, 1, vec![s1.clone()])],
    )
    .await;

    assert_eq!(claimed(&pool, user).await, 1);
    assert!(mailer.cart_alerts.lock().unwrap().is_empty());
    assert!(telegram.messages.lock().unwrap().is_empty());
}
