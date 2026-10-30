//! Shop-credential vault tests: the crypto round trip wired through the
//! service (per-user storage, replacement, clearing, wrong-key failures) and
//! the two HTTP routes.

mod common;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use common::{insert_active_user, insert_session, test_state_without_shop_key};
use ovalbot::common::crypto::SecretKey;
use ovalbot::services::accounts::shop::{clear_credentials, load_credentials, set_credentials};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tower::ServiceExt;

// ============================== LOCAL HELPERS ==============================

fn app(pool: SqlitePool) -> Router {
    ovalbot::app(common::test_state(pool))
}

async fn call(app: &Router, req: Request<Body>) -> (StatusCode, Value) {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
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

// ============================== SERVICE ==============================

// A set + load round trip gives back the exact email and clear password.
#[sqlx::test]
async fn set_then_load_roundtrips_the_credentials(pool: SqlitePool) {
    let user_id = insert_active_user(&pool, "a@example.com").await;
    let key = SecretKey::from_passphrase("test-secret");

    set_credentials(
        &pool,
        user_id,
        "shop@example.com",
        "mot de passe",
        Some(&key),
    )
    .await
    .unwrap();

    let creds = load_credentials(&pool, user_id, Some(&key))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(creds.email, "shop@example.com");
    assert_eq!(creds.password, "mot de passe");
}

// The stored password is never clear in the database.
#[sqlx::test]
async fn stored_password_is_encrypted_at_rest(pool: SqlitePool) {
    let user_id = insert_active_user(&pool, "a@example.com").await;
    let key = SecretKey::from_passphrase("secret-secret");

    set_credentials(
        &pool,
        user_id,
        "shop@example.com",
        "plainpassword",
        Some(&key),
    )
    .await
    .unwrap();

    let (cipher,): (String,) =
        sqlx::query_as("SELECT shop_password_cipher FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_ne!(cipher, "plainpassword");
    assert!(!cipher.contains("plain"));
    assert!(cipher.chars().all(|c| c.is_ascii_hexdigit()));
}

// Storing twice replaces both values in place (no duplicate rows possible).
#[sqlx::test]
async fn second_set_replaces_the_credentials(pool: SqlitePool) {
    let user_id = insert_active_user(&pool, "a@example.com").await;
    let key = SecretKey::from_passphrase("test-secret");

    set_credentials(&pool, user_id, "old@example.com", "old", Some(&key))
        .await
        .unwrap();
    set_credentials(&pool, user_id, "new@example.com", "new", Some(&key))
        .await
        .unwrap();

    let creds = load_credentials(&pool, user_id, Some(&key))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(creds.email, "new@example.com");
    assert_eq!(creds.password, "new");
}

// Clearing makes the load return None (idempotent clearing covered upstream).
#[sqlx::test]
async fn clearing_empties_the_vault(pool: SqlitePool) {
    let user_id = insert_active_user(&pool, "a@example.com").await;
    let key = SecretKey::from_passphrase("test-secret");

    set_credentials(&pool, user_id, "shop@example.com", "pw", Some(&key))
        .await
        .unwrap();
    clear_credentials(&pool, user_id).await.unwrap();

    assert!(load_credentials(&pool, user_id, Some(&key))
        .await
        .unwrap()
        .is_none());
}

// A rotated SECRET_KEY cannot decrypt stored credentials (fail-closed).
#[sqlx::test]
async fn load_fails_with_a_rotated_key(pool: SqlitePool) {
    let user_id = insert_active_user(&pool, "a@example.com").await;
    set_credentials(
        &pool,
        user_id,
        "shop@example.com",
        "pw",
        Some(&SecretKey::from_passphrase("one")),
    )
    .await
    .unwrap();

    let result = load_credentials(&pool, user_id, Some(&SecretKey::from_passphrase("two"))).await;
    assert!(matches!(
        result,
        Err(ovalbot::common::error::AppError::Crypto(_))
    ));
}

// Writing is refused without a key (fail-closed, nothing half-stored).
#[sqlx::test]
async fn set_without_key_is_refused(pool: SqlitePool) {
    let user_id = insert_active_user(&pool, "a@example.com").await;
    let result = set_credentials(&pool, user_id, "shop@example.com", "pw", None).await;
    assert!(matches!(
        result,
        Err(ovalbot::common::error::AppError::ShopCredentialsUnavailable)
    ));

    // Nothing was stored.
    let (cipher,): (Option<String>,) =
        sqlx::query_as("SELECT shop_password_cipher FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(cipher.is_none());
}

// Reading an existing cipher without the configured key is refused too.
#[sqlx::test]
async fn load_without_key_is_refused(pool: SqlitePool) {
    let user_id = insert_active_user(&pool, "a@example.com").await;
    set_credentials(
        &pool,
        user_id,
        "shop@example.com",
        "pw",
        Some(&SecretKey::from_passphrase("test-secret")),
    )
    .await
    .unwrap();

    let result = load_credentials(&pool, user_id, None).await;
    assert!(matches!(
        result,
        Err(ovalbot::common::error::AppError::ShopCredentialsUnavailable)
    ));
}

// ============================== ROUTES ==============================

#[sqlx::test]
async fn put_credentials_stores_and_me_exposes_the_shop_email(pool: SqlitePool) {
    let id = insert_active_user(&pool, "a@example.com").await;
    let cookie = format!(
        "session={}",
        insert_session(&pool, id, &ovalbot::common::time::in_days(1)).await
    );

    let (status, body) = call(
        &app(pool.clone()),
        request(
            "PUT",
            "/api/shop/credentials",
            Some(&cookie),
            Some(json!({ "email": "shop@example.com", "password": "shop-password" })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null, "no body on 204");

    // /api/me now carries the shop email (and the password in NO form).
    let (status, body) = call(
        &app(pool.clone()),
        request("GET", "/api/me", Some(&cookie), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["shop_email"], "shop@example.com");
    assert!(body.get("shop_password_cipher").is_none());
    assert!(body.get("password").is_none());

    // The service round trip decrypts back end-to-end.
    let key = SecretKey::from_passphrase("test-secret");
    let creds = load_credentials(&pool, id, Some(&key))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(creds.email, "shop@example.com");
    assert_eq!(creds.password, "shop-password");
}

#[sqlx::test]
async fn put_credentials_rejects_invalid_input(pool: SqlitePool) {
    let id = insert_active_user(&pool, "a@example.com").await;
    let cookie = format!(
        "session={}",
        insert_session(&pool, id, &ovalbot::common::time::in_days(1)).await
    );

    for payload in [
        json!({ "email": "", "password": "pw" }),
        json!({ "email": "not-an-email", "password": "pw" }),
        json!({ "email": "shop@example.com", "password": "" }),
    ] {
        let (status, _) = call(
            &app(pool.clone()),
            request("PUT", "/api/shop/credentials", Some(&cookie), Some(payload)),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}

#[sqlx::test]
async fn put_credentials_requires_the_vault_key(pool: SqlitePool) {
    let app = ovalbot::app(test_state_without_shop_key(pool.clone()));
    let id = insert_active_user(&pool, "a@example.com").await;
    let cookie = format!(
        "session={}",
        insert_session(&pool, id, &ovalbot::common::time::in_days(1)).await
    );

    let (status, body) = call(
        &app,
        request(
            "PUT",
            "/api/shop/credentials",
            Some(&cookie),
            Some(json!({ "email": "shop@example.com", "password": "shop-password" })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"], "shop_credentials_unavailable");
}

#[sqlx::test]
async fn shop_routes_are_guarded(pool: SqlitePool) {
    // Anonymous: 401 on both routes.
    let (status, _) = call(
        &app(pool.clone()),
        request(
            "PUT",
            "/api/shop/credentials",
            None,
            Some(json!({ "email": "shop@example.com", "password": "pw" })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(
        &app(pool.clone()),
        request("DELETE", "/api/shop/credentials", None, None),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Waiting account: 403 (ActiveUser).
    let user_id = common::insert_user(
        &pool,
        "waiting@example.com",
        ovalbot::common::models::Status::Waiting,
        ovalbot::common::models::Role::User,
    )
    .await;
    let cookie = format!(
        "session={}",
        insert_session(&pool, user_id, &ovalbot::common::time::in_days(1)).await
    );
    let (status, _) = call(
        &app(pool.clone()),
        request(
            "PUT",
            "/api/shop/credentials",
            Some(&cookie),
            Some(json!({ "email": "shop@example.com", "password": "pw" })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn delete_credentials_clears_the_vault(pool: SqlitePool) {
    let id = insert_active_user(&pool, "a@example.com").await;
    let key = SecretKey::from_passphrase("test-secret");
    let cookie = format!(
        "session={}",
        insert_session(&pool, id, &ovalbot::common::time::in_days(1)).await
    );
    set_credentials(&pool, id, "shop@example.com", "pw", Some(&key))
        .await
        .unwrap();

    let (status, _) = call(
        &app(pool.clone()),
        request("DELETE", "/api/shop/credentials", Some(&cookie), None),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Idempotent: a second delete still answers 204.
    let (status, _) = call(
        &app(pool.clone()),
        request("DELETE", "/api/shop/credentials", Some(&cookie), None),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Nothing left, server-side.
    assert!(load_credentials(&pool, id, Some(&key))
        .await
        .unwrap()
        .is_none());
    let (status, body) = call(
        &app(pool.clone()),
        request("GET", "/api/me", Some(&cookie), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["shop_email"], Value::Null);
}
