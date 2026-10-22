-- Durable registry of the resale links the bot discovered on the billetterie
-- home page. One row per billetterie card that carries an open resale button
-- (title+date are the stable site keys); the link is then polled directly, so
-- the home page is only hit for discovery. Past-dated rows are removed by the
-- scan loop.
CREATE TABLE resale_links (
    title            TEXT NOT NULL, -- encounter title (stable key half)
    date             TEXT NOT NULL, -- encounter date string (stable key half)
    resale_link      TEXT NOT NULL, -- last known resale product URL, replaced when a new wave appears
    match_id         INTEGER,       -- resolved LNR match id, NULL when outside the LNR calendar
    first_seen_at    TEXT NOT NULL, -- RFC 3339, preserved across updates
    last_seen_at     TEXT NOT NULL, -- RFC 3339, last home refresh that listed this encounter
    last_scanned_at  TEXT,          -- RFC 3339, last direct poll of resale_link
    last_scan_status TEXT,          -- ok | no_seats | queue | http_error
    PRIMARY KEY (title, date)
);

CREATE INDEX idx_resale_links_match ON resale_links(match_id);
