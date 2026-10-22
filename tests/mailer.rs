mod common;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::Router;
use common::{insert_active_user, RecordingMailer};
use ovalbot::services::alerts::{MatchAlert, SeatAlert, SeatBrief};
use ovalbot::services::mailer::{
    send_welcome_if_unsent, EmailTemplates, MailError, Mailer, ResendMailer,
};
use sqlx::SqlitePool;

struct FailingMailer;

#[async_trait]
impl Mailer for FailingMailer {
    async fn send_welcome(&self, _email: &str) -> Result<(), MailError> {
        Err(MailError::Api("boom".to_string()))
    }

    async fn send_match_alert(&self, _email: &str, _alert: &MatchAlert) -> Result<(), MailError> {
        Err(MailError::Api("boom".to_string()))
    }

    async fn send_seats_alert(&self, _email: &str, _alert: &SeatAlert) -> Result<(), MailError> {
        Err(MailError::Api("boom".to_string()))
    }
}

/// Sends the welcome email only once per user, skipping subsequent calls once
/// the `welcome_email_sent_at` marker is set.
#[sqlx::test]
async fn sends_welcome_once_then_skips(pool: SqlitePool) {
    let id = insert_active_user(&pool, "a@example.com").await;
    let mailer = RecordingMailer::new();

    send_welcome_if_unsent(&mailer, &pool, id, "a@example.com").await;
    send_welcome_if_unsent(&mailer, &pool, id, "a@example.com").await;

    let sent = mailer.welcome.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0], "a@example.com");
}

/// Leaves the `welcome_email_sent_at` marker unset when the mailer fails, so the
/// email can be retried on a later run.
#[sqlx::test]
async fn failed_send_leaves_marker_unset(pool: SqlitePool) {
    let id = insert_active_user(&pool, "a@example.com").await;
    let mailer = FailingMailer;

    send_welcome_if_unsent(&mailer, &pool, id, "a@example.com").await;

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM users WHERE id = ? AND welcome_email_sent_at IS NOT NULL",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
}

#[derive(Clone)]
struct Captured {
    headers: HeaderMap,
    body: Vec<u8>,
}

#[derive(Clone)]
struct MockResend {
    status: StatusCode,
    captured: Arc<Mutex<Option<Captured>>>,
}

async fn capture(State(mock): State<MockResend>, headers: HeaderMap, body: Bytes) -> StatusCode {
    *mock.captured.lock().unwrap() = Some(Captured {
        headers,
        body: body.to_vec(),
    });
    mock.status
}

/// Starts a tiny in-process HTTP server standing in for the Resend API. Returns
/// its base URL and a slot holding the last captured request.
async fn mock_resend(status: u16) -> (String, Arc<Mutex<Option<Captured>>>) {
    let captured = Arc::new(Mutex::new(None));
    let mock = MockResend {
        status: StatusCode::from_u16(status).unwrap(),
        captured: captured.clone(),
    };
    let app = Router::new()
        .route("/emails", post(capture))
        .with_state(mock);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (format!("http://{addr}"), captured)
}

fn mailer(base_url: &str) -> ResendMailer {
    ResendMailer::with_base_url(
        "test-key".to_string(),
        "OvalBot <from@example.com>".to_string(),
        EmailTemplates::defaults(),
        base_url.to_string(),
    )
}

fn captured_body(captured: &Arc<Mutex<Option<Captured>>>) -> serde_json::Value {
    let guard = captured.lock().unwrap();
    let request = guard.as_ref().expect("no request captured");
    serde_json::from_slice(&request.body).unwrap()
}

fn captured_headers(captured: &Arc<Mutex<Option<Captured>>>) -> HeaderMap {
    captured.lock().unwrap().as_ref().unwrap().headers.clone()
}

/// Sends the welcome email to Resend with the expected auth header and payload.
#[tokio::test]
async fn send_welcome_posts_expected_request() {
    let (base_url, captured) = mock_resend(200).await;

    mailer(&base_url)
        .send_welcome("a@example.com")
        .await
        .unwrap();

    let headers = captured_headers(&captured);
    assert_eq!(headers.get("authorization").unwrap(), "Bearer test-key");
    let body = captured_body(&captured);
    assert_eq!(body["from"], "OvalBot <from@example.com>");
    assert_eq!(body["to"], "a@example.com");
    assert_eq!(body["subject"], "Welcome to OvalBot");
}

/// Sends the match alert with a subject built from the teams.
#[tokio::test]
async fn send_match_alert_posts_expected_request() {
    let (base_url, captured) = mock_resend(200).await;

    let alert = MatchAlert {
        team: "Stade Rochelais".to_string(),
        opponent: "RC Toulon".to_string(),
        kickoff: "samedi 14h30".to_string(),
    };
    mailer(&base_url)
        .send_match_alert("b@example.com", &alert)
        .await
        .unwrap();

    let body = captured_body(&captured);
    assert_eq!(body["to"], "b@example.com");
    assert_eq!(
        body["subject"],
        "Match scheduled: Stade Rochelais vs RC Toulon"
    );
    assert!(body["html"]
        .as_str()
        .unwrap()
        .contains("Stade Rochelais vs RC Toulon"));
}

/// A non-success response is reported as `MailError::Api`.
#[tokio::test]
async fn send_returns_api_error_on_failure() {
    let (base_url, _) = mock_resend(500).await;

    let err = mailer(&base_url)
        .send_welcome("a@example.com")
        .await
        .unwrap_err();
    assert!(matches!(err, MailError::Api(_)));
}

/// Sends the seats alert with the French subject, the seats rendered as styled
/// rows in the HTML and the resale link in the plain text + CTA.
#[tokio::test]
async fn send_seats_alert_posts_expected_request() {
    let (base_url, captured) = mock_resend(200).await;

    let alert = SeatAlert {
        opponent: "RC Toulon".to_string(),
        date: "dimanche 20 décembre à 15h15".to_string(),
        link: "https://billetterie.example.com/revente".to_string(),
        seats: vec![SeatBrief {
            bloc: "NORD - MACIF BRP".to_string(),
            row: "U".to_string(),
            seat_number: 2,
            price: "49.00".to_string(),
        }],
    };
    mailer(&base_url)
        .send_seats_alert("c@example.com", &alert)
        .await
        .unwrap();

    let body = captured_body(&captured);
    assert_eq!(body["to"], "c@example.com");
    assert_eq!(
        body["subject"],
        "Nouvelles places en revente : Stade Rochelais vs RC Toulon"
    );
    let text = body["text"].as_str().unwrap();
    assert!(text.contains("NORD - MACIF BRP - U - 2 for 49.00 €"));
    assert!(text.contains("https://billetterie.example.com/revente"));
    let html = body["html"].as_str().unwrap();
    assert!(html.contains("href=\"https://billetterie.example.com/revente\""));
    assert!(html.contains("🎟️ NORD - MACIF BRP"));
    assert!(html.contains("Rang U · Siège 2"));
    assert!(html.contains("49.00 €"));
}

/// An alert with no link renders the CTA with a `#` fallback instead of an
/// empty `href`.
#[tokio::test]
async fn send_seats_alert_falls_back_to_hash_without_link() {
    let (base_url, captured) = mock_resend(200).await;

    let alert = SeatAlert {
        opponent: "RC Toulon".to_string(),
        date: "dimanche 20 décembre à 15h15".to_string(),
        link: String::new(),
        seats: vec![SeatBrief {
            bloc: "NORD - MACIF BRP".to_string(),
            row: "U".to_string(),
            seat_number: 2,
            price: "49.00".to_string(),
        }],
    };
    mailer(&base_url)
        .send_seats_alert("c@example.com", &alert)
        .await
        .unwrap();

    let body = captured_body(&captured);
    let html = body["html"].as_str().unwrap();
    assert!(html.contains("href=\"#\""));
}
