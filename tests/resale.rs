//! Integration tests for the `resale_links` service: discovery upsert,
//! polling-target selection, scan bookkeeping and retirement.

use ovalbot::services::resale::{
    delete_one, links_to_scan, mark_scanned, upsert_discovered, DiscoveryCounts, ScanStatus,
    ScanTarget,
};
use parser::core::club::ClubType;
use parser::core::encounter::{Encounter, MatchNature};
use sqlx::SqlitePool;

/// Builds an encounter with (or without) an open resale link.
fn linked(title: &str, date: &str, link: &str) -> Encounter {
    Encounter::new(
        ClubType::StadeRochelais,
        title.to_string(),
        date.to_string(),
        MatchNature::Rugby,
        Some(link.to_string()),
    )
}

fn bare(title: &str, date: &str) -> Encounter {
    Encounter::new(
        ClubType::StadeRochelais,
        title.to_string(),
        date.to_string(),
        MatchNature::Rugby,
        None,
    )
}

/// Every row of the table, as raw columns (display-order check).
async fn rows(pool: &SqlitePool) -> Vec<ScanTarget> {
    links_to_scan(pool).await.unwrap()
}

/// The first upsert inserts the row: link, resolved match id, timestamps.
#[sqlx::test]
async fn upsert_inserts_discovered_links(pool: SqlitePool) {
    let ubb = linked(
        "UNION BORDEAUX BÈGLES",
        "dimanche 25 octobre à 21h05",
        "https://billetterie.staderochelais.com/fr/product/1357/revente_ubb",
    );
    let counts = upsert_discovered(
        &pool,
        &[bare("ÉVREUX", "vendredi 23 octobre à 20h00"), ubb],
        &[None, Some(11864)],
    )
    .await
    .unwrap();

    // Links only: the link-less card is not stored.
    assert_eq!(counts.with_link, 1);
    assert_eq!(counts.inserted, 1);
    assert_eq!(counts.updated, 0);

    let stored = rows(&pool).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].title, "UNION BORDEAUX BÈGLES");
    assert_eq!(
        stored[0].resale_link,
        "https://billetterie.staderochelais.com/fr/product/1357/revente_ubb"
    );
    assert_eq!(stored[0].match_id, Some(11864));
}

/// A second discovery pass of the same card preserves `first_seen_at`,
/// bumps `last_seen_at` and counts as an update, not an insert.
#[sqlx::test]
async fn upsert_twice_updates_in_place(pool: SqlitePool) {
    let ubb = linked(
        "UNION BORDEAUX BÈGLES",
        "dimanche 25 octobre à 21h05",
        "https://x/1357",
    );
    upsert_discovered(&pool, std::slice::from_ref(&ubb), &[Some(11864)])
        .await
        .unwrap();

    // Rewind the timestamps: the second pass must refresh `last_seen_at`
    // while preserving `first_seen_at`, even when nothing else changed.
    sqlx::query(
        "UPDATE resale_links SET first_seen_at = '2000-01-01T00:00:00Z', \
              last_seen_at = '2000-01-01T00:00:00Z', last_scanned_at = '2000-01-01T00:00:00Z'",
    )
    .execute(&pool)
    .await
    .unwrap();

    let counts = upsert_discovered(&pool, std::slice::from_ref(&ubb), &[Some(11864)])
        .await
        .unwrap();
    assert_eq!(
        counts,
        DiscoveryCounts {
            with_link: 1,
            inserted: 0,
            updated: 1
        }
    );

    let targets = links_to_scan(&pool).await.unwrap();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].match_id, Some(11864));
    // first_seen_at preserved, last_seen_at refreshed, scan bookkeeping kept.
    let (first_seen, last_seen, last_scanned): (String, String, String) = sqlx::query_as(
        "SELECT first_seen_at, last_seen_at, last_scanned_at FROM resale_links \
         WHERE title = 'UNION BORDEAUX BÈGLES'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(first_seen, "2000-01-01T00:00:00Z");
    assert_ne!(last_seen, "2000-01-01T00:00:00Z");
    assert_eq!(last_scanned, "2000-01-01T00:00:00Z");
}

/// A new resale wave opens a new product: the stored link is replaced but
/// the row keeps its place — no duplicate, no history to clean up.
#[sqlx::test]
async fn upsert_replaces_a_changed_link(pool: SqlitePool) {
    let first = linked(
        "UNION BORDEAUX BÈGLES",
        "dimanche 25 octobre à 21h05",
        "https://x/1357",
    );
    upsert_discovered(&pool, std::slice::from_ref(&first), &[Some(11864)])
        .await
        .unwrap();

    let second = linked(
        "UNION BORDEAUX BÈGLES",
        "dimanche 25 octobre à 21h05",
        "https://x/1360/revente_j8",
    );
    let counts = upsert_discovered(&pool, std::slice::from_ref(&second), &[Some(11864)])
        .await
        .unwrap();

    assert_eq!(counts.inserted, 0);
    assert_eq!(counts.updated, 1);
    let targets = links_to_scan(&pool).await.unwrap();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].resale_link, "https://x/1360/revente_j8");
}

/// A card whose encounter left the calendar keeps the old match id until the
/// next pass re-resolves it; `match_id` is refreshed on every upsert of the
/// same (title, date).
#[sqlx::test]
async fn upsert_refreshes_the_match_id(pool: SqlitePool) {
    let card = linked(
        "AVIRON BAYONNAIS",
        "dimanche 8 novembre à 21h05",
        "https://x/1358",
    );
    upsert_discovered(&pool, std::slice::from_ref(&card), &[None])
        .await
        .unwrap();
    assert_eq!(links_to_scan(&pool).await.unwrap()[0].match_id, None);

    upsert_discovered(&pool, std::slice::from_ref(&card), &[Some(11877)])
        .await
        .unwrap();
    assert_eq!(links_to_scan(&pool).await.unwrap()[0].match_id, Some(11877));
}

/// The polling targets come back in a deterministic order, and rows lacking
/// a match id are included (they are polled but never recorded).
#[sqlx::test]
async fn links_to_scan_lists_every_row_in_order(pool: SqlitePool) {
    let second = linked(
        "USA PERPIGNAN",
        "week-end du 28/29 novembre",
        "https://x/1173",
    );
    let first = linked(
        "AVIRON BAYONNAIS",
        "dimanche 8 novembre à 21h05",
        "https://x/1171",
    );
    // Same pass, first_seen_at in list order: first then title breaks ties.
    upsert_discovered(&pool, &[first, second], &[Some(11877), None])
        .await
        .unwrap();

    let targets = links_to_scan(&pool).await.unwrap();
    let titles: Vec<&str> = targets.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(titles, ["AVIRON BAYONNAIS", "USA PERPIGNAN"]);
    assert_eq!(targets[1].match_id, None);
}

/// `mark_scanned` writes the status and the scan timestamp of the row only.
#[sqlx::test]
async fn mark_scanned_records_the_outcome(pool: SqlitePool) {
    let a = linked("A", "dimanche 8 novembre à 21h05", "https://x/a");
    let b = linked("B", "dimanche 8 novembre à 21h05", "https://x/b");
    upsert_discovered(&pool, &[a, b], &[None, None])
        .await
        .unwrap();

    mark_scanned(&pool, "A", "dimanche 8 novembre à 21h05", ScanStatus::Queue)
        .await
        .unwrap();

    let rows: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT title, last_scan_status FROM resale_links ORDER BY title")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        rows,
        vec![
            ("A".to_string(), Some("queue".to_string())),
            ("B".to_string(), None)
        ]
    );
}

/// A past-dated match's link is removed; a live one stays, and deleting a
/// missing row reports `false` (the poll pass only logs "removed" on `true`).
#[sqlx::test]
async fn delete_one_removes_only_the_target_row(pool: SqlitePool) {
    let past = linked(
        "STADE TOULOUSAIN",
        "dimanche 6 septembre 2025 à 21h05",
        "https://x/1308",
    );
    let live = linked(
        "AVIRON BAYONNAIS",
        "dimanche 8 novembre 2099 à 21h05",
        "https://x/1171",
    );
    upsert_discovered(&pool, &[past, live], &[Some(11840), Some(11877)])
        .await
        .unwrap();

    // The poll retires by date through `delete_one` with the parsed encounter.
    let targets = links_to_scan(&pool).await.unwrap();
    for target in targets {
        let encounter = target.to_encounter();
        if encounter.date_passed() {
            assert!(delete_one(&pool, &target.title, &target.date)
                .await
                .unwrap());
        }
    }

    let remaining = links_to_scan(&pool).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].title, "AVIRON BAYONNAIS");

    // Idempotent: deleting a gone row reports false.
    assert!(!delete_one(
        &pool,
        "STADE TOULOUSAIN",
        "dimanche 6 septembre 2025 à 21h05"
    )
    .await
    .unwrap());
}
