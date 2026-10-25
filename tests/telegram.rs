mod common;

use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::Router;
use common::insert_active_user;
use ovalbot::services::channels::telegram::{
    consume_link_token, create_link_token, unlink, Telegram, TelegramClient, TelegramError,
};
use sqlx::SqlitePool;

async fn telegram_id(pool: &SqlitePool, user_id: i64) -> Option<String> {
    sqlx::query_scalar("SELECT telegram_id FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// A freshly created token links the chat to the user when consumed.
#[sqlx::test]
async fn consume_links_chat_to_user(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;

    let token = create_link_token(&pool, user).await.unwrap();
    let linked = consume_link_token(&pool, &token, "12345").await.unwrap();

    assert!(linked);
    assert_eq!(telegram_id(&pool, user).await.as_deref(), Some("12345"));
}

/// A token can only be used once.
#[sqlx::test]
async fn token_is_single_use(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    let token = create_link_token(&pool, user).await.unwrap();

    assert!(consume_link_token(&pool, &token, "12345").await.unwrap());
    assert!(!consume_link_token(&pool, &token, "99999").await.unwrap());
    assert_eq!(telegram_id(&pool, user).await.as_deref(), Some("12345"));
}

/// An unknown token is rejected.
#[sqlx::test]
async fn unknown_token_is_rejected(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;

    assert!(!consume_link_token(&pool, "deadbeef", "12345")
        .await
        .unwrap());
    assert!(telegram_id(&pool, user).await.is_none());
}

/// An expired token is rejected and does not link the chat.
#[sqlx::test]
async fn expired_token_is_rejected(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    sqlx::query(
        "INSERT INTO telegram_link_tokens (token, user_id, created_at, expires_at) \
         VALUES ('expired', ?, '2000-01-01T00:00:00Z', '2000-01-01T00:00:00Z')",
    )
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();

    assert!(!consume_link_token(&pool, "expired", "12345").await.unwrap());
    assert!(telegram_id(&pool, user).await.is_none());
}

/// A chat already linked to another user cannot be linked again.
#[sqlx::test]
async fn chat_already_linked_is_rejected(pool: SqlitePool) {
    let user_a = insert_active_user(&pool, "a@example.com").await;
    let user_b = insert_active_user(&pool, "b@example.com").await;

    let token_a = create_link_token(&pool, user_a).await.unwrap();
    assert!(consume_link_token(&pool, &token_a, "12345").await.unwrap());

    let token_b = create_link_token(&pool, user_b).await.unwrap();
    assert!(!consume_link_token(&pool, &token_b, "12345").await.unwrap());
    assert!(telegram_id(&pool, user_b).await.is_none());
}

/// Unlinking clears the previously stored chat.
#[sqlx::test]
async fn unlink_clears_telegram_id(pool: SqlitePool) {
    let user = insert_active_user(&pool, "a@example.com").await;
    let token = create_link_token(&pool, user).await.unwrap();
    consume_link_token(&pool, &token, "12345").await.unwrap();

    unlink(&pool, user).await.unwrap();

    assert!(telegram_id(&pool, user).await.is_none());
}

// ============================== TRANSPORT (Bot API) ==========================

/// One captured request: path and raw JSON body.
type CapturedRequest = (String, Vec<u8>);
/// Slot shared with the mock server, holding the last captured request.
type CapturedSlot = Arc<Mutex<Option<CapturedRequest>>>;

#[derive(Clone)]
struct MockBotApi {
    status: StatusCode,
    captured: CapturedSlot,
}

async fn capture(
    State(mock): State<MockBotApi>,
    axum::extract::Path(path): axum::extract::Path<String>,
    body: Bytes,
) -> StatusCode {
    *mock.captured.lock().unwrap() = Some((path, body.to_vec()));
    mock.status
}

/// Starts a tiny in-process HTTP server standing in for the Bot API. Returns
/// its base URL and a slot holding the last captured request (path + body).
async fn mock_bot_api(status: u16) -> (String, CapturedSlot) {
    let captured = Arc::new(Mutex::new(None));
    let mock = MockBotApi {
        status: StatusCode::from_u16(status).unwrap(),
        captured: captured.clone(),
    };
    let app = Router::new()
        .route("/{*path}", post(capture))
        .with_state(mock);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (format!("http://{addr}"), captured)
}

fn client(base_url: &str) -> TelegramClient {
    TelegramClient::with_base_url("test-token".to_string(), base_url.to_string())
}

fn captured_request(captured: &CapturedSlot) -> (String, serde_json::Value) {
    let guard = captured.lock().unwrap();
    let (path, body) = guard.as_ref().expect("no request captured");
    (
        path.clone(),
        serde_json::from_slice(body).expect("valid JSON body"),
    )
}

/// Plain-text sends hit `bot{token}/sendMessage` with no `parse_mode` key.
#[tokio::test]
async fn send_message_posts_plain_text_without_parse_mode() {
    let (base_url, captured) = mock_bot_api(200).await;

    client(&base_url)
        .send_message("123", "Match scheduled: X vs Y")
        .await
        .unwrap();

    let (path, body) = captured_request(&captured);
    // `/{*path}` captures the path without its leading slash.
    assert_eq!(path, "bottest-token/sendMessage");
    assert_eq!(body["chat_id"], "123");
    assert_eq!(body["text"], "Match scheduled: X vs Y");
    assert!(body.get("parse_mode").is_none());
}

/// HTML sends post the fragment and switch the Bot API's `parse_mode` on.
#[tokio::test]
async fn send_message_html_posts_the_fragment_with_parse_mode() {
    let (base_url, captured) = mock_bot_api(200).await;

    client(&base_url)
        .send_message_html("123", "🔗 <a href=\"https://x\">Accéder</a>")
        .await
        .unwrap();

    let (path, body) = captured_request(&captured);
    // `/{*path}` captures the path without its leading slash.
    assert_eq!(path, "bottest-token/sendMessage");
    assert_eq!(body["parse_mode"], "HTML");
    assert!(body["text"].as_str().unwrap().contains("Accéder"));
}

/// A Bot API error response surfaces as `TelegramError::Api`.
#[tokio::test]
async fn non_2xx_response_is_reported_as_api_error() {
    let (base_url, _captured) = mock_bot_api(400).await;

    let err = client(&base_url)
        .send_message("123", "hello")
        .await
        .unwrap_err();

    assert!(matches!(err, TelegramError::Api(_)));
}
