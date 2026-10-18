CREATE TABLE ticket_requests (
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
    CHECK (kind IN ('single', 'duo', 'group')),
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
    )
);

CREATE INDEX idx_ticket_requests_match ON ticket_requests(match_id);
CREATE INDEX idx_ticket_requests_status ON ticket_requests(status);
