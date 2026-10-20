-- Durable record of the resale seats the bot has already seen per match.
-- Inserted with `ON CONFLICT DO NOTHING` at every scan: only genuinely new
-- seats come back (INSERT .. RETURNING), which is the notification trigger.
-- Minimal by design — the Phase 4 proposal flow will extend it if needed.
CREATE TABLE ticket_offers (
    match_id      INTEGER NOT NULL,
    title         TEXT    NOT NULL,
    date          TEXT    NOT NULL,
    pack_id       INTEGER NOT NULL,
    ticket_id     TEXT    NOT NULL,
    first_seen_at TEXT    NOT NULL,
    PRIMARY KEY (match_id, pack_id, ticket_id)
);

CREATE INDEX idx_ticket_offers_match ON ticket_offers(match_id);
