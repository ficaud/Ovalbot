use axum::extract::State;
use axum::extract::{ConnectInfo, Extension};
use axum::http::header::HeaderMap;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use time::Duration;

use crate::common::error::AppError;
use crate::common::models::{Status, User, UserView};
use crate::services::accounts::auth::{
    generate_session_token, hash_password, verify_password, AuthUser,
};
use crate::services::accounts::email::Email;
use crate::services::accounts::waitlist::status_for;
use crate::services::channels::mailer::send_welcome_if_unsent;
use crate::services::rate_limit::Policy;
use crate::services::state::AppState;

const SESSION_DAYS: i64 = 30;

/// Client key feeding the rate limiter: the socket peer IP by default, the
/// first `X-Forwarded-For` entry when the app runs behind a trusted reverse
/// proxy (`PROXY_TRUSTED=true`), and `"unknown"` when no connect info exists
/// (tests drive the router without a listener).
///
/// # Arguments
/// * `headers` — request headers (`X-Forwarded-For` source).
/// * `connect_info` — socket peer address, when the server provides it.
/// * `proxy_trusted` — whether `X-Forwarded-For` may be trusted.
///
/// # Returns
/// The key the rate limiter buckets events under.
fn client_key(
    headers: &HeaderMap,
    connect_info: Option<Extension<ConnectInfo<SocketAddr>>>,
    proxy_trusted: bool,
) -> String {
    if proxy_trusted {
        if let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
            let first = forwarded.split(',').next().unwrap_or_default().trim();
            if !first.is_empty() {
                return first.to_owned();
            }
        }
    }
    connect_info
        .map(|Extension(info)| info.0.ip().to_string())
        .unwrap_or_else(|| "unknown".to_owned())
}

/// Registration payload (`POST /api/register` body).
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
}

/// Login payload (`POST /api/login` body).
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// Registration answer: how the account landed (`active` admission or
/// `waiting` on the waitlist), echoed to the frontend.
#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub email: String,
    pub status: Status,
}

/// `GET /api/version` payload (crate version, used by the site footer badge).
#[derive(Debug, Serialize)]
pub struct VersionResponse {
    pub version: &'static str,
}

/// `POST /api/register` — validates email/password, hashes the password, then
/// admits or queues by the active count ([`status_for`]). Newly active users
/// get their welcome email right away (best-effort).
///
/// # Returns
/// `201` with the [`RegisterResponse`].
///
/// # Errors
/// `AppError::TooManyAttempts` (429, more than 3 attempts per hour from the
/// same client), `AppError::InvalidEmail` (400), `AppError::InvalidPassword`
/// (< 8 chars), `AppError::EmailTaken` (409, unique violation),
/// `AppError::Sqlx`/`Argon2`.
pub async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    connect_info: Option<Extension<ConnectInfo<SocketAddr>>>,
    Json(body): Json<RegisterRequest>,
) -> Result<Response, AppError> {
    // Every attempt counts (creating accounts is the abuse itself).
    let key = client_key(&headers, connect_info, state.proxy_trusted);
    state
        .rate_limiter
        .admit(&key, Policy::REGISTER)
        .map_err(AppError::TooManyAttempts)?;

    let email = Email::parse(&body.email)?;
    if body.password.len() < 8 {
        return Err(AppError::InvalidPassword);
    }

    let active_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE status = 'active'")
            .fetch_one(&state.pool)
            .await?;

    let status = status_for(active_count);
    let now = crate::common::time::now();
    let activated_at = (status == Status::Active).then(|| now.clone());

    let result = sqlx::query(
        "INSERT INTO users (email, password_hash, status, role, created_at, activated_at) \
         VALUES (?, ?, ?, 'user', ?, ?)",
    )
    .bind(email.as_str())
    .bind(hash_password(&body.password)?)
    .bind(status.as_str())
    .bind(&now)
    .bind(&activated_at)
    .execute(&state.pool)
    .await;

    match result {
        Ok(res) => {
            if status == Status::Active {
                send_welcome_if_unsent(
                    state.mailer.as_ref(),
                    &state.pool,
                    res.last_insert_rowid(),
                    email.as_str(),
                )
                .await;
            }
        }
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return Err(AppError::EmailTaken);
        }
        Err(e) => return Err(e.into()),
    }

    let body = Json(RegisterResponse {
        email: email.to_string(),
        status,
    });
    Ok((StatusCode::CREATED, body).into_response())
}

/// `POST /api/login` — authenticates by email + password and opens a session:
/// a 64-hex token stored in `sessions` (30-day expiry) and set as the
/// HttpOnly `session` cookie.
///
/// # Returns
/// 200 with the current [`UserView`]-style view of the user plus the session
/// cookie (the frontend's root `+layout.ts` guards routes on it).
///
/// # Errors
/// `AppError::TooManyAttempts` (429, more than 5 refused credentials per 15
/// minutes from the same client — a success resets the count),
/// `AppError::InvalidCredentials` when the email or password does not match
/// (deliberately indistinguishable), `AppError::Sqlx`/`Argon2` internally.
pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<Extension<ConnectInfo<SocketAddr>>>,
    Json(body): Json<LoginRequest>,
) -> Result<Response, AppError> {
    // Refused credentials only: the gate is read-only, the event is recorded
    // on the failure paths below (a success clears the count instead).
    let key = client_key(&headers, connect_info, state.proxy_trusted);
    state
        .rate_limiter
        .check(&key, Policy::LOGIN)
        .map_err(AppError::TooManyAttempts)?;

    // An email that fails validation is not a credential attempt: the event
    // is not recorded (the 400 answers immediately, no DB lookup).
    let email = Email::parse(&body.email)?;
    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, status, role, created_at, activated_at, welcome_email_sent_at, telegram_id, notify_email, notify_telegram, shop_email, shop_password_cipher FROM users WHERE email = ?",
    )
    .bind(email.as_str())
    .fetch_optional(&state.pool)
    .await?
    .filter(|user| verify_password(&body.password, &user.password_hash).is_ok());

    let user = match user {
        Some(user) => user,
        None => {
            state.rate_limiter.record(&key, Policy::LOGIN);
            return Err(AppError::InvalidCredentials);
        }
    };
    state.rate_limiter.clear(&key, Policy::LOGIN);

    let session_id = generate_session_token();
    let now = crate::common::time::now();
    let expires_at = crate::common::time::in_days(SESSION_DAYS);
    sqlx::query("INSERT INTO sessions (id, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)")
        .bind(&session_id)
        .bind(user.id)
        .bind(&now)
        .bind(&expires_at)
        .execute(&state.pool)
        .await?;

    let cookie = Cookie::build(("session", session_id))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(Duration::days(SESSION_DAYS))
        .build();

    let jar = jar.add(cookie);
    Ok((jar, Json(UserView::from(user))).into_response())
}

/// `POST /api/logout` — deletes the session row server-side and clears the
/// cookie; requires a logged-in user (the [`AuthUser`] extractor carries both
/// the user and the session id to delete).
pub async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
    auth: AuthUser,
) -> Result<Response, AppError> {
    sqlx::query("DELETE FROM sessions WHERE id = ?")
        .bind(&auth.session_id)
        .execute(&state.pool)
        .await?;

    let jar = jar.remove(Cookie::from("session"));
    Ok((jar, StatusCode::OK).into_response())
}

/// `GET /api/me` — the current account, as seen by the frontend (the root
/// `+layout.ts` builds its route guards on this including the flags).
pub async fn me(auth: AuthUser) -> Json<UserView> {
    Json(UserView::from(auth.user))
}

/// `GET /api/version` — crate version, shown as the discreet site footer badge.
pub async fn version() -> Json<VersionResponse> {
    Json(VersionResponse {
        version: env!("CARGO_PKG_VERSION"),
    })
}
