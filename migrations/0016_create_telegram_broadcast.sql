-- Single-row registry of the admin's Telegram broadcast channel. When set,
-- every alert the owning admin is subscribed to is also mirrored to this chat
-- (in addition to the regular per-user notifications). Deleting the admin
-- removes the row through the foreign key, disabling the broadcast.
CREATE TABLE telegram_broadcast (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    chat_id       TEXT    NOT NULL,
    admin_user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    updated_at    TEXT    NOT NULL
);
