-- Add the `notify` kind to `ticket_requests`: a notification-only subscription
-- (the user is alerted about resale seats but the bot must not buy for them).
-- The `kind` CHECK constraints can only be changed via a table rebuild.
CREATE TABLE ticket_requests_new (
    user_id         INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    match_id        INTEGER NOT NULL,
    kind            TEXT    NOT NULL,
    quantity        INTEGER,
    max_price_cents INTEGER,
    places          INTEGER,
    status          TEXT    NOT NULL DEFAULT 'pending',
    created_at      TEXT    NOT NULL,
    updated_at      TEXT    NOT NULL,
    PRIMARY KEY (user_id, match_id),
    CHECK (kind IN ('single', 'duo', 'group', 'notify')),
    CHECK (status IN ('pending', 'accepted', 'purchased', 'failed', 'expired', 'cancelled')),
    CHECK (
        (kind = 'single' AND quantity IS NOT NULL AND quantity BETWEEN 1 AND 10
            AND max_price_cents IS NOT NULL AND max_price_cents BETWEEN 1 AND 100000
            AND places IS NULL)
        OR (kind = 'duo' AND quantity IS NULL
            AND max_price_cents IS NOT NULL AND max_price_cents BETWEEN 1 AND 100000
            AND places IS NULL)
        OR (kind = 'group' AND quantity IS NULL AND max_price_cents IS NULL
            AND places IS NOT NULL AND places BETWEEN 2 AND 10)
        OR (kind = 'notify' AND quantity IS NULL AND max_price_cents IS NULL
            AND places IS NULL)
    )
);

INSERT INTO ticket_requests_new (
    user_id, match_id, kind, quantity, max_price_cents, places, status, created_at, updated_at
)
SELECT user_id, match_id, kind, quantity, max_price_cents, places, status, created_at, updated_at
FROM ticket_requests;

DROP TABLE ticket_requests;
ALTER TABLE ticket_requests_new RENAME TO ticket_requests;

CREATE INDEX idx_ticket_requests_match ON ticket_requests(match_id);
CREATE INDEX idx_ticket_requests_status ON ticket_requests(status);
