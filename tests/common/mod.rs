#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use calendar::{Event, MatchStatus, Team, TeamType};
use ovalbot::common::crypto::SecretKey;
use ovalbot::common::models::{Role, Status};
use ovalbot::services::accounts::auth::generate_session_token;
use ovalbot::services::bot::checkout::{CartSession, CartShop, RealCartShop};
use ovalbot::services::calendar::CalendarService;
use ovalbot::services::channels::alerts::{CartAlert, MatchAlert, SeatAlert};
use ovalbot::services::channels::mailer::{MailError, Mailer, NoopMailer};
use ovalbot::services::channels::telegram::{
    NoopTelegram, Telegram, TelegramConfig, TelegramError,
};
use ovalbot::services::state::AppState;
use parser::core::seat::Seat;
use parser::interface::curl::checkout::CheckoutError;
use sqlx::SqlitePool;
use std::collections::VecDeque;

pub struct RecordingMailer {
    pub welcome: Mutex<Vec<String>>,
    pub alerts: Mutex<Vec<(String, MatchAlert)>>,
    pub seats_alerts: Mutex<Vec<(String, SeatAlert)>>,
    pub cart_alerts: Mutex<Vec<(String, CartAlert)>>,
}

impl RecordingMailer {
    pub fn new() -> Self {
        Self {
            welcome: Mutex::new(Vec::new()),
            alerts: Mutex::new(Vec::new()),
            seats_alerts: Mutex::new(Vec::new()),
            cart_alerts: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl Mailer for RecordingMailer {
    async fn send_welcome(&self, email: &str) -> Result<(), MailError> {
        self.welcome.lock().unwrap().push(email.to_string());
        Ok(())
    }

    async fn send_match_alert(&self, email: &str, alert: &MatchAlert) -> Result<(), MailError> {
        self.alerts
            .lock()
            .unwrap()
            .push((email.to_string(), alert.clone()));
        Ok(())
    }

    async fn send_seats_alert(&self, email: &str, alert: &SeatAlert) -> Result<(), MailError> {
        self.seats_alerts
            .lock()
            .unwrap()
            .push((email.to_string(), alert.clone()));
        Ok(())
    }

    async fn send_cart_alert(&self, email: &str, alert: &CartAlert) -> Result<(), MailError> {
        self.cart_alerts
            .lock()
            .unwrap()
            .push((email.to_string(), alert.clone()));
        Ok(())
    }
}

pub struct RecordingTelegram {
    pub messages: Mutex<Vec<(String, String)>>,
}

impl RecordingTelegram {
    pub fn new() -> Self {
        Self {
            messages: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl Telegram for RecordingTelegram {
    async fn send_message(&self, chat_id: &str, text: &str) -> Result<(), TelegramError> {
        self.messages
            .lock()
            .unwrap()
            .push((chat_id.to_string(), text.to_string()));
        Ok(())
    }

    async fn send_message_html(&self, chat_id: &str, html: &str) -> Result<(), TelegramError> {
        self.messages
            .lock()
            .unwrap()
            .push((chat_id.to_string(), html.to_string()));
        Ok(())
    }
}

/// What the fake shop does with the next `add_seat` call.
pub enum SeatOutcome {
    /// The add-to-cart succeeds (the seat lands in the fake cart).
    Added,
    /// The seat disappeared from the resale page (retry-later churn).
    SeatGone,
    /// The billetterie served its waiting queue.
    WaitingQueue,
    /// A terminal error (expired form / invalid session).
    HardFail,
}

/// Fake [`CartShop`]: records logins and added seats, replays a scripted
/// per-seat outcome queue, and can refuse logins outright.
pub struct RecordingShop {
    /// Emails of every `login` attempt (order = call order).
    pub logins: Mutex<Vec<String>>,
    /// When `true`, every login answers `CheckoutError::Login`.
    pub login_fails: Mutex<bool>,
    /// Per-seat outcomes consumed in order (defaults to [`SeatOutcome::Added`]
    /// once the queue is empty).
    pub outcomes: Mutex<VecDeque<SeatOutcome>>,
    /// Ticket ids of the seats the fake cart accepted.
    pub added: Arc<Mutex<Vec<String>>>,
}

impl RecordingShop {
    /// A shop whose logins succeed and whose adds all succeed.
    pub fn new() -> Self {
        Self {
            logins: Mutex::new(Vec::new()),
            login_fails: Mutex::new(false),
            outcomes: Mutex::new(VecDeque::new()),
            added: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Scripts the per-seat outcomes, in add-call order.
    pub fn with_outcomes(self, outcomes: Vec<SeatOutcome>) -> Self {
        *self.outcomes.lock().unwrap() = outcomes.into();
        self
    }

    /// Makes every login fail.
    pub fn refusing(self) -> Self {
        *self.login_fails.lock().unwrap() = true;
        self
    }
}

struct RecordingSession {
    outcomes: VecDeque<SeatOutcome>,
    added: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl CartShop for RecordingShop {
    async fn login(
        &self,
        email: &str,
        _password: &str,
    ) -> Result<Box<dyn CartSession>, CheckoutError> {
        self.logins.lock().unwrap().push(email.to_string());
        if *self.login_fails.lock().unwrap() {
            return Err(CheckoutError::Login("fake refusal".to_string()));
        }
        let outcomes = std::mem::take(&mut *self.outcomes.lock().unwrap());
        Ok(Box::new(RecordingSession {
            outcomes,
            added: Arc::clone(&self.added),
        }))
    }
}

#[async_trait]
impl CartSession for RecordingSession {
    async fn add_seat(&mut self, _resale_url: &str, seat: &Seat) -> Result<(), CheckoutError> {
        match self.outcomes.pop_front().unwrap_or(SeatOutcome::Added) {
            SeatOutcome::Added => {
                self.added.lock().unwrap().push(seat.id.ticket_id.clone());
                Ok(())
            }
            SeatOutcome::SeatGone => Err(CheckoutError::SeatGone),
            SeatOutcome::WaitingQueue => Err(CheckoutError::WaitingQueue),
            SeatOutcome::HardFail => Err(CheckoutError::ExpiredForm),
        }
    }
}

pub async fn insert_active_user(pool: &SqlitePool, email: &str) -> i64 {
    insert_user(pool, email, Status::Active, Role::User).await
}

pub async fn insert_user(pool: &SqlitePool, email: &str, status: Status, role: Role) -> i64 {
    let now = ovalbot::common::time::now();
    let activated_at = (status == Status::Active).then(|| now.clone());
    sqlx::query(
        "INSERT INTO users (email, password_hash, status, role, created_at, activated_at) \
         VALUES (?, 'x', ?, ?, ?, ?)",
    )
    .bind(email)
    .bind(status.as_str())
    .bind(role.as_str())
    .bind(&now)
    .bind(&activated_at)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_rowid()
}

pub async fn insert_user_with_password(pool: &SqlitePool, email: &str, password: &str) -> i64 {
    let id = insert_active_user(pool, email).await;
    let hash = ovalbot::services::accounts::auth::hash_password(password).unwrap();
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(&hash)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    id
}

pub async fn insert_session(pool: &SqlitePool, user_id: i64, expires_at: &str) -> String {
    let id = generate_session_token();
    let now = ovalbot::common::time::now();
    sqlx::query("INSERT INTO sessions (id, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(user_id)
        .bind(&now)
        .bind(expires_at)
        .execute(pool)
        .await
        .unwrap();
    id
}

pub fn test_state(pool: SqlitePool) -> AppState {
    test_state_with(
        pool,
        Arc::new(NoopMailer),
        Arc::new(NoopTelegram),
        TelegramConfig::default(),
    )
}

/// A calendar `Team` whose id/name follow the `TeamType` conventions.
pub fn calendar_team(team_type: TeamType) -> Team {
    Team {
        id: team_type.id().to_string(),
        name: team_type.to_string(),
        team_type,
    }
}

/// A home fixture of `home` against `away` with the given LNR id and
/// scheduling state, for calendar seeding (`CalendarService::seed_events`).
///
/// # Arguments
/// * `match_id` — the stable LNR id the API paths use.
pub fn home_event(home: TeamType, away: TeamType, match_id: u32, status: MatchStatus) -> Event {
    Event {
        id: match_id,
        team: calendar_team(home),
        opponent: calendar_team(away),
        kickoff: "samedi 10 octobre 14h30".to_string(),
        status,
    }
}

#[allow(dead_code)]
pub fn test_state_without_shop_key(pool: SqlitePool) -> AppState {
    build_state(
        pool,
        Arc::new(NoopMailer),
        Arc::new(NoopTelegram),
        TelegramConfig::default(),
        None,
    )
}

/// [`AppState`] variant carrying a shop-credentials vault key (derived from a
/// fixed test passphrase).
#[allow(dead_code)]
pub fn test_state_with_shop_key(pool: SqlitePool) -> AppState {
    build_state(
        pool,
        Arc::new(NoopMailer),
        Arc::new(NoopTelegram),
        TelegramConfig::default(),
        Some(SecretKey::from_passphrase("test-secret")),
    )
}

pub fn build_state(
    pool: SqlitePool,
    mailer: Arc<dyn Mailer>,
    telegram: Arc<dyn Telegram>,
    telegram_config: TelegramConfig,
    shop_key: Option<SecretKey>,
) -> AppState {
    // The checkout stays disabled by default for the test state builders; the
    // checkout tests assemble their own state with the flags they need.
    AppState {
        pool,
        mailer,
        telegram,
        telegram_config,
        calendar: Arc::new(CalendarService::new()),
        shop_key,
        cart_enabled: false,
        cart_dry_run: true,
        cart_shop: Arc::new(RealCartShop),
    }
}

pub fn test_state_with(
    pool: SqlitePool,
    mailer: Arc<dyn Mailer>,
    telegram: Arc<dyn Telegram>,
    telegram_config: TelegramConfig,
) -> AppState {
    build_state(
        pool,
        mailer,
        telegram,
        telegram_config,
        Some(SecretKey::from_passphrase("test-secret")),
    )
}
