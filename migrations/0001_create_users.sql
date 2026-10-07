CREATE TABLE users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    status TEXT NOT NULL,
    role TEXT NOT NULL,
    created_at TEXT NOT NULL,
    activated_at TEXT
);
