use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::{Deserialize, Serialize};
use time::Duration;

use crate::common::error::AppError;
use crate::common::models::{Status, User, UserView};
use crate::services::auth::{generate_session_token, hash_password, verify_password, AuthUser};
use crate::services::email::Email;
use crate::services::mailer::send_welcome_if_unsent;
use crate::services::state::AppState;
use crate::services::waitlist::status_for;

const SESSION_DAYS: i64 = 30;

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub email: String,
    pub status: Status,
}

#[derive(Debug, Serialize)]
pub struct VersionResponse {
    pub version: &'static str,
}

pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> Result<Response, AppError> {
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

pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<LoginRequest>,
) -> Result<Response, AppError> {
    let email = Email::parse(&body.email)?;
    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, status, role, created_at, activated_at, welcome_email_sent_at, telegram_id, notify_email, notify_telegram FROM users WHERE email = ?",
    )
    .bind(email.as_str())
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::InvalidCredentials)?;

    verify_password(&body.password, &user.password_hash)
        .map_err(|_| AppError::InvalidCredentials)?;

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

pub async fn me(auth: AuthUser) -> Json<UserView> {
    Json(UserView::from(auth.user))
}

pub async fn version() -> Json<VersionResponse> {
    Json(VersionResponse {
        version: env!("CARGO_PKG_VERSION"),
    })
}
