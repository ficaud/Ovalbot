-- Token of the dedicated bot used to publish the admin's broadcasts to the
-- configured Telegram channel. Stored encrypted with a SECRET_KEY-derived key
-- (same shape as the shop password vault) and never returned by the API.
-- Existing rows from 0016 have no token yet: the broadcast stays disabled
-- until the admin provides one.
ALTER TABLE telegram_broadcast ADD COLUMN bot_token_cipher TEXT;
