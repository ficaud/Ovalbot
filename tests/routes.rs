mod common;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::Router;
use common::{insert_session, insert_user, insert_user_with_password, test_state, test_state_with};
use ovalbot::common::models::{Role, Status};
use ovalbot::services::mailer::NoopMailer;
use ovalbot::services::telegram::{create_link_token, NoopTelegram, TelegramConfig};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tower::ServiceExt;

fn app(pool: SqlitePool) -> Router {
    ovalbot::app(test_state(pool))
}

fn app_with_telegram(pool: SqlitePool, config: TelegramConfig) -> Router {
    ovalbot::app(test_state_with(
        pool,
        Arc::new(NoopMailer),
        Arc::new(NoopTelegram),
        config,
    ))
}

async fn call(app: &Router, req: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, headers, bytes.to_vec())
}

fn request(method: &str, uri: &str, cookie: Option<&str>, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(serde_json::to_vec(&value).unwrap())
        }
        None => Body::empty(),
    };
    builder.body(body).unwrap()
}

fn cookie_for(session: &str) -> String {
    format!("session={session}")
}

fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

async fn session_cookie(pool: &SqlitePool, status: Status, role: Role) -> String {
    let id = insert_user(pool, "user@example.com", status, role).await;
    let session = insert_session(pool, id, &ovalbot::common::time::in_days(1)).await;
    cookie_for(&session)
}

// ---- auth ----

#[sqlx::test]
async fn version_returns_package_version(pool: SqlitePool) {
    let (status, _, body) = call(&app(pool), request("GET", "/api/version", None, None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["version"], env!("CARGO_PKG_VERSION"));
}

#[sqlx::test]
async fn register_creates_active_user(pool: SqlitePool) {
    let (status, _, body) = call(
        &app(pool.clone()),
        request(
            "POST",
            "/api/register",
            None,
            Some(json!({ "email": "a@example.com", "password": "password123" })),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(json(&body)["status"], "active");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE email = ?")
        .bind("a@example.com")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test]
async fn register_rejects_invalid_email(pool: SqlitePool) {
    let (status, _, body) = call(
        &app(pool),
        request(
            "POST",
            "/api/register",
            None,
            Some(json!({ "email": "nope", "password": "password123" })),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["error"], "invalid_email");
}

#[sqlx::test]
async fn register_rejects_short_password(pool: SqlitePool) {
    let (status, _, body) = call(
        &app(pool),
        request(
            "POST",
            "/api/register",
            None,
            Some(json!({ "email": "a@example.com", "password": "short" })),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&body)["error"], "invalid_password");
}

#[sqlx::test]
async fn register_rejects_duplicate_email(pool: SqlitePool) {
    common::insert_active_user(&pool, "a@example.com").await;
    let (status, _, body) = call(
        &app(pool),
        request(
            "POST",
            "/api/register",
            None,
            Some(json!({ "email": "a@example.com", "password": "password123" })),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(json(&body)["error"], "email_taken");
}

#[sqlx::test]
async fn register_waitlists_after_limit(pool: SqlitePool) {
    for i in 0..ovalbot::services::waitlist::ACTIVE_LIMIT {
        common::insert_active_user(&pool, &format!("a{i}@example.com")).await;
    }

    let (status, _, body) = call(
        &app(pool),
        request(
            "POST",
            "/api/register",
            None,
            Some(json!({ "email": "late@example.com", "password": "password123" })),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(json(&body)["status"], "waiting");
}

#[sqlx::test]
async fn login_sets_session_cookie(pool: SqlitePool) {
    insert_user_with_password(&pool, "a@example.com", "password123").await;

    let (status, headers, _) = call(
        &app(pool),
        request(
            "POST",
            "/api/login",
            None,
            Some(json!({ "email": "a@example.com", "password": "password123" })),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let set_cookie = headers.get(header::SET_COOKIE).unwrap().to_str().unwrap();
    assert!(set_cookie.starts_with("session="));
}

#[sqlx::test]
async fn login_rejects_bad_credentials(pool: SqlitePool) {
    insert_user_with_password(&pool, "a@example.com", "password123").await;

    let (status, _, body) = call(
        &app(pool),
        request(
            "POST",
            "/api/login",
            None,
            Some(json!({ "email": "a@example.com", "password": "wrongpass" })),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json(&body)["error"], "invalid_credentials");
}

#[sqlx::test]
async fn me_requires_session(pool: SqlitePool) {
    let (status, _, _) = call(&app(pool), request("GET", "/api/me", None, None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn me_returns_user_with_valid_session(pool: SqlitePool) {
    let cookie = session_cookie(&pool, Status::Active, Role::User).await;

    let (status, _, body) = call(&app(pool), request("GET", "/api/me", Some(&cookie), None)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["email"], "user@example.com");
    assert_eq!(json(&body)["telegram_linked"], false);
}

#[sqlx::test]
async fn me_rejects_unknown_session(pool: SqlitePool) {
    let cookie = cookie_for("deadbeef");
    let (status, _, _) = call(&app(pool), request("GET", "/api/me", Some(&cookie), None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn me_rejects_expired_session(pool: SqlitePool) {
    let id = insert_user(&pool, "a@example.com", Status::Active, Role::User).await;
    let session = insert_session(&pool, id, "2000-01-01T00:00:00Z").await;

    let (status, _, _) = call(
        &app(pool),
        request("GET", "/api/me", Some(&cookie_for(&session)), None),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn me_rejects_when_user_deleted(pool: SqlitePool) {
    let id = insert_user(&pool, "a@example.com", Status::Active, Role::User).await;
    let session = insert_session(&pool, id, &ovalbot::common::time::in_days(1)).await;

    // Orphan the session: sqlx enables foreign keys by default, so disable them on
    // a dedicated connection to delete the user while the session remains.
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(id)
        .execute(&mut *conn)
        .await
        .unwrap();
    drop(conn);

    let (status, _, _) = call(
        &app(pool),
        request("GET", "/api/me", Some(&cookie_for(&session)), None),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn logout_invalidates_session(pool: SqlitePool) {
    let cookie = session_cookie(&pool, Status::Active, Role::User).await;

    let (status, _, _) = call(
        &app(pool.clone()),
        request("POST", "/api/logout", Some(&cookie), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(remaining, 0);

    let (status, _, _) = call(&app(pool), request("GET", "/api/me", Some(&cookie), None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ---- admin ----

#[sqlx::test]
async fn admin_routes_reject_regular_user(pool: SqlitePool) {
    let cookie = session_cookie(&pool, Status::Active, Role::User).await;
    let (status, _, _) = call(
        &app(pool),
        request("GET", "/api/admin/users", Some(&cookie), None),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn admin_lists_users(pool: SqlitePool) {
    let cookie = session_cookie(&pool, Status::Active, Role::Admin).await;
    insert_user(&pool, "b@example.com", Status::Active, Role::User).await;

    let (status, _, body) = call(
        &app(pool),
        request("GET", "/api/admin/users", Some(&cookie), None),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body).as_array().unwrap().len(), 2);
}

#[sqlx::test]
async fn admin_activates_user(pool: SqlitePool) {
    let cookie = session_cookie(&pool, Status::Active, Role::Admin).await;
    let waiting = insert_user(&pool, "w@example.com", Status::Waiting, Role::User).await;

    let (status, _, _) = call(
        &app(pool.clone()),
        request(
            "POST",
            &format!("/api/admin/users/{waiting}/activate"),
            Some(&cookie),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let status: String = sqlx::query_scalar("SELECT status FROM users WHERE id = ?")
        .bind(waiting)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "active");

    let (missing, _, _) = call(
        &app(pool),
        request(
            "POST",
            "/api/admin/users/99999/activate",
            Some(&cookie),
            None,
        ),
    )
    .await;
    assert_eq!(missing, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn admin_deletes_user(pool: SqlitePool) {
    let cookie = session_cookie(&pool, Status::Active, Role::Admin).await;
    let target = insert_user(&pool, "t@example.com", Status::Active, Role::User).await;

    let (status, _, _) = call(
        &app(pool.clone()),
        request(
            "DELETE",
            &format!("/api/admin/users/{target}"),
            Some(&cookie),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE id = ?")
        .bind(target)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);

    let other_admin = insert_user(&pool, "admin2@example.com", Status::Active, Role::Admin).await;
    let (forbidden, _, _) = call(
        &app(pool),
        request(
            "DELETE",
            &format!("/api/admin/users/{other_admin}"),
            Some(&cookie),
            None,
        ),
    )
    .await;
    assert_eq!(forbidden, StatusCode::FORBIDDEN);
}

// ---- calendar ----

#[sqlx::test]
async fn teams_requires_active(pool: SqlitePool) {
    let cookie = session_cookie(&pool, Status::Waiting, Role::User).await;
    let (status, _, _) = call(
        &app(pool),
        request("GET", "/api/teams", Some(&cookie), None),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn teams_lists_the_fourteen_clubs(pool: SqlitePool) {
    let cookie = session_cookie(&pool, Status::Active, Role::User).await;
    let (status, _, body) = call(
        &app(pool),
        request("GET", "/api/teams", Some(&cookie), None),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let teams = json(&body);
    assert_eq!(teams.as_array().unwrap().len(), 14);
    assert!(teams
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["id"] == "stade-rochelais"));
}

#[sqlx::test]
async fn team_matches_returns_empty_then_404(pool: SqlitePool) {
    let cookie = session_cookie(&pool, Status::Active, Role::User).await;

    let (status, _, body) = call(
        &app(pool.clone()),
        request(
            "GET",
            "/api/teams/stade-rochelais/matches",
            Some(&cookie),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body).as_array().unwrap().len(), 0);

    let (missing, _, _) = call(
        &app(pool),
        request("GET", "/api/teams/unknown/matches", Some(&cookie), None),
    )
    .await;
    assert_eq!(missing, StatusCode::NOT_FOUND);
}

// ---- notifications ----

#[sqlx::test]
async fn notifications_require_active(pool: SqlitePool) {
    let cookie = session_cookie(&pool, Status::Waiting, Role::User).await;
    let (status, _, _) = call(
        &app(pool),
        request("GET", "/api/notifications", Some(&cookie), None),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn subscribe_list_unsubscribe_roundtrip(pool: SqlitePool) {
    let cookie = session_cookie(&pool, Status::Active, Role::User).await;
    let app = app(pool);

    let (subscribed, _, _) = call(
        &app,
        request("POST", "/api/notifications/7", Some(&cookie), None),
    )
    .await;
    assert_eq!(subscribed, StatusCode::NO_CONTENT);

    let (status, _, body) = call(
        &app,
        request("GET", "/api/notifications", Some(&cookie), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body), json!([7]));

    let (unsubscribed, _, _) = call(
        &app,
        request("DELETE", "/api/notifications/7", Some(&cookie), None),
    )
    .await;
    assert_eq!(unsubscribed, StatusCode::NO_CONTENT);

    let (_, _, body) = call(
        &app,
        request("GET", "/api/notifications", Some(&cookie), None),
    )
    .await;
    assert_eq!(json(&body), json!([]));
}

#[sqlx::test]
async fn update_notification_settings_persists(pool: SqlitePool) {
    let cookie = session_cookie(&pool, Status::Active, Role::User).await;
    let app = app(pool);

    let (status, _, _) = call(
        &app,
        request(
            "PUT",
            "/api/notifications/settings",
            Some(&cookie),
            Some(json!({ "email_enabled": false, "telegram_enabled": true })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, _, body) = call(&app, request("GET", "/api/me", Some(&cookie), None)).await;
    assert_eq!(json(&body)["notify_email"], false);
    assert_eq!(json(&body)["notify_telegram"], true);
}

// ---- telegram ----

#[sqlx::test]
async fn telegram_link_unavailable_when_disabled(pool: SqlitePool) {
    let cookie = session_cookie(&pool, Status::Active, Role::User).await;
    let (status, _, _) = call(
        &app(pool),
        request("POST", "/api/telegram/link", Some(&cookie), None),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

#[sqlx::test]
async fn telegram_link_returns_deep_link(pool: SqlitePool) {
    let cookie = session_cookie(&pool, Status::Active, Role::User).await;
    let config = TelegramConfig {
        bot_username: Some("OvalBotBot".to_string()),
        webhook_secret: Some("secret".to_string()),
    };

    let (status, _, body) = call(
        &app_with_telegram(pool, config),
        request("POST", "/api/telegram/link", Some(&cookie), None),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let url = json(&body)["url"].as_str().unwrap().to_string();
    assert!(url.starts_with("https://t.me/OvalBotBot?start="));
}

#[sqlx::test]
async fn telegram_unlink_clears_chat(pool: SqlitePool) {
    let id = insert_user(&pool, "a@example.com", Status::Active, Role::User).await;
    let session = insert_session(&pool, id, &ovalbot::common::time::in_days(1)).await;
    sqlx::query("UPDATE users SET telegram_id = '123' WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();

    let (status, _, _) = call(
        &app(pool.clone()),
        request(
            "DELETE",
            "/api/telegram/link",
            Some(&cookie_for(&session)),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let telegram_id: Option<String> =
        sqlx::query_scalar("SELECT telegram_id FROM users WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(telegram_id.is_none());
}

fn webhook_request(secret: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/telegram/webhook")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-telegram-bot-api-secret-token", secret)
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

#[sqlx::test]
async fn telegram_webhook_rejects_bad_secret(pool: SqlitePool) {
    let config = TelegramConfig {
        bot_username: Some("OvalBotBot".to_string()),
        webhook_secret: Some("secret".to_string()),
    };

    let (status, _, _) = call(
        &app_with_telegram(pool, config),
        webhook_request(
            "wrong",
            json!({ "message": { "chat": { "id": 1 }, "text": "/start abc" } }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn telegram_webhook_links_chat(pool: SqlitePool) {
    let id = insert_user(&pool, "a@example.com", Status::Active, Role::User).await;
    let token = create_link_token(&pool, id).await.unwrap();
    let config = TelegramConfig {
        bot_username: Some("OvalBotBot".to_string()),
        webhook_secret: Some("secret".to_string()),
    };

    let (status, _, _) = call(
        &app_with_telegram(pool.clone(), config),
        webhook_request(
            "secret",
            json!({ "message": { "chat": { "id": 12345 }, "text": format!("/start {token}") } }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let telegram_id: Option<String> =
        sqlx::query_scalar("SELECT telegram_id FROM users WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(telegram_id.as_deref(), Some("12345"));
}
