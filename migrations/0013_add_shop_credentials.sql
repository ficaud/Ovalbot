-- Per-user shop (billetterie) credentials for the automatic add-to-cart flow.
-- The password is stored encrypted at rest (AES-256-GCM, nonce prepended,
-- hex-encoded) under the server's `SECRET_KEY`-derived key — never clear.
ALTER TABLE users ADD COLUMN shop_email TEXT;
ALTER TABLE users ADD COLUMN shop_password_cipher TEXT;
