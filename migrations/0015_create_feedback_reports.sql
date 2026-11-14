-- Bug reports and improvement suggestions submitted by active users from the
-- /feedback page. Deliberately minimal: no status tracking, no attachments —
-- reports are stored and emailed to the admins, then read from the admin page.
CREATE TABLE feedback_reports (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind       TEXT    NOT NULL CHECK (kind IN ('bug', 'suggestion')),
    message    TEXT    NOT NULL,
    created_at TEXT    NOT NULL
);

CREATE INDEX idx_feedback_reports_created_at ON feedback_reports(created_at);
