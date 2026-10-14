# AGENTS.md — OvalBot

Development notes kept up to date by the agent as work progresses.

## Project

Ticket resale bot for Stade Rochelais home games + a simple website (2 pages).
Stack: Rust/Axum (backend) + SvelteKit (frontend) + SQLite. Hosted on a Raspberry Pi (arm64) via Docker Compose. The bot (Phase 4) will be integrated last.

## Structure

- `src/` : Rust backend — Axum server (`main.rs`) + CLI (`bin/create_admin.rs`).
  - `src/common/` : foundations (`error`, `models`, `time`, `db`).
  - `src/services/` : business + integrations (`auth`, `calendar`, `email`, `mailer`, `notifications`, `state`, `telegram`, `waitlist`).
  - `src/routes/` : HTTP layer (Axum handlers), one file per resource (`admin`, `auth`, `calendar`, `notifications`, `telegram`).
- `migrations/` : SQL migrations (sqlx).
- `frontend/` : SvelteKit (Svelte 5, `adapter-static`).
- `doc/specs/` : specifications (`cdc.md`) + per-phase specs (`phase1.md`).
- `Dockerfile` + `docker-compose.yml` : deployment (arm64 image published on GHCR).

## Commands

- Server : `cargo run --bin ovalbot`
- Create admin : `cargo run --bin create_admin -- --email <email> --password <pwd>`
- Tests : `cargo test`
- Lint : `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
- Frontend : `cd frontend && npm run dev` / `npm run build`
- Devcontainer : `./my-env/up.sh` / `./my-env/down.sh`

## Conventions

- Unit tests are mandatory for every business function implemented.
- Code and comments are written in English.
- No superfluous comments in the code.
- Section separators use plain `//` comments (never `///`, reserved for doc comments that attach to the next item).
- Axum 0.8 : path params use `{id}` (not `:id`).
- Cookies : `axum-extra` (feature `cookie`).
- Migrations : `sqlx::migrate!()` embedded at runtime + `sqlx-cli` (sqlite) in dev to create files.
- SQL : runtime-checked `query`/`query_as` for Phase 1.

## Journal

### 2026-10-07 — Planning & CI/CD
- Moved specs to `doc/specs/` + logo to `doc/assets/`.
- Stack frozen : Axum 0.8.x, sqlx 0.9, argon2 0.6 (Argon2id), Svelte 5, Docker Compose arm64.
- CI/CD : `ci.yml` (lint + test + build, all branches) + `deploy.yml` (`v*` tags → GHCR).
- Language : site, code and comments are English-first.
- Remaining : backend scaffolding (deps, migrations, routes, `create_admin`) + SvelteKit frontend.

### 2026-10-07 — Phase 1 backend scaffolding (done)
- Dependencies added : sqlx 0.9, argon2 0.6 (Argon2id), axum-extra 0.12 (cookie), serde, tower, tower-http, thiserror, time, hex, rand.
- Migrations : `users`, `sessions`.
- Modules : `models`, `error`, `state`, `db`, `auth` (+ `AuthUser`/`AdminUser` extractors), `waitlist`, `email`, `time`, `routes` (auth/admin).
- Binaries : `main` (server) + `create_admin` (CLI).
- Endpoints : register, login, logout, me, version, admin users (list/activate/delete).
- Tests : 9 unit + 1 integration (`sqlx::test`), `cargo test` green, clippy `-D warnings` green.
- Note : argon2 0.6 uses the new `password-hash` 0.6 API (`hash_password` auto-generates salt).
- Remaining : SvelteKit frontend + static serving in Axum.

### 2026-10-07 — Phase 1 frontend (done)
- Axum now serves the built frontend : `tower-http` `fs` feature + `ServeDir`/`ServeFile` SPA fallback to `index.html` (`STATIC_DIR` env, default `frontend/build`, `ENV STATIC_DIR=/app/static` in Docker). `tokio` gained the `fs` feature.
- SvelteKit scaffolded in `frontend/` : Svelte 5 + `adapter-static` (SPA, `ssr = false`, `fallback: 'index.html'`), Vite 6 with `/api` dev proxy → `:3000`.
- Routes : `/login`, `/register`, `/` (home: active vs waitlist), `/admin` (list/activate/delete). Auth via `/api/me` in root `+layout.ts`; client-side route guards with `redirect()`.
- Shared : `src/lib/api.ts` (typed client), `types.ts`, `format.ts`. Global `app.css` (yellow `#FFD500` / black, mobile-first). Discreet version badge from `/api/version`.
- Verified : `npm run build` + `svelte-check` green; end-to-end smoke test (static serving + full auth flow) OK.

### 2026-10-07 — CI devcontainer build hardened
- `ci.yml` build failed on a transient network error fetching `sudo` from `security.ubuntu.com` (HTTP 80 timeout).
- `.devcontainer/Dockerfile` : `apt-get install` now retries up to 5x (15s pause); added `curl --retry` to the rustup and NodeSource download steps.

### 2026-10-07 — Rust LSP fixed (rust-analyzer)
- Symptom : `gd` in Neovim returned "No results found for `lsp_definitions`"; rust-analyzer started but failed to load the workspace.
- Root cause : `CARGO_HOME=/usr/local/cargo` (root-owned, empty registry) → `cargo metadata` → `Permission denied`; plus `rust-src` missing (minimal profile).
- Fix : `.devcontainer/Dockerfile` adds `rust-src` component + `cargo fetch` prefetch; `.devcontainer/entrypoint.sh` chowns `/usr/local/cargo` to the runtime user.

### 2026-10-08 — Phase 1.5 welcome email (done)
- Spec : `doc/specs/phase1.5.md`. Goal: send a welcome email on every transition to `active` (direct register, auto-promotion, manual activation).
- Provider : **Resend** via HTTP API (`POST https://api.resend.com/emails`), `reqwest` (rustls, no openssl) + `async-trait`.
- New module `src/mailer.rs` : trait `Mailer`, `ResendMailer`, `NoopMailer`, `from_env()`, `send_welcome_if_unsent()` (best-effort + dedup).
- Migration `0003_add_welcome_email_sent_at.sql` : `users.welcome_email_sent_at` (anti-doublon). `User` model updated, all `query_as::<_, User>` SELECTs now include the column.
- `AppState` gains `mailer: Arc<dyn Mailer>`; `promote_next_waiting` now returns `Option<User>` (was `Result<()>`) so the caller can send the email.
- Env vars : `RESEND_ENABLED` (default `false` → Noop), `RESEND_API_KEY`, `RESEND_FROM`. Added to `docker-compose.yml`.
- Tests : 2 new mailer unit tests (dedup + failure leaves marker unset) → `cargo test` 12 green, clippy `-D warnings` + `fmt --check` green.
- Remaining (later) : final email copy (subject/body placeholders), verified domain in Resend.

### 2026-10-10 — Phase 3 match scheduling alerts (done)
- Spec : `doc/specs/phase3.md`. Goal: per-match email alert when a match flips `Tbd → Scheduled` (any of the 14 teams).
- Migration `0004_create_match_notifications.sql` : `(user_id, match_id, created_at)`, PK `(user_id, match_id)`, `ON DELETE CASCADE`.
- `CalendarService::sync()` now returns `Vec<Change>` (captures old snapshot, `calendar::diff(old, new)`). Crate `calendar` gained 2 `diff` edge-case tests.
- `mailer` : `Mailer` trait + `send_match_alert` (`MatchAlert { team, opponent, kickoff }`), `ResendMailer`/`NoopMailer` updated (refactored shared `send()`).
- New module `src/notifications.rs` : `subscribe`/`unsubscribe`/`list_for_user`/`process` (emails subscribers then deletes rows).
- Endpoints : `GET /api/notifications`, `POST`/`DELETE /api/notifications/{match_id}` (extractor `ActiveUser`).
- `main.rs` : background sync loop now calls `notifications::process` on the returned changes.
- Frontend : bell toggle (SVG inline + `title` tooltip) on `Tbd` match cards, card switched `<button>` → `<div>`.
- Verified : `cargo test` 19 green (18 unit + 1 integration), calendar crate 25 green, clippy `-D warnings` + `fmt --check` green, `npm run build` + `npm run check` green.

### 2026-10-12 — Fix wrong match in scheduling alerts (unstable match id)
- Bug : a user received an alert for « Montpellier vs LOU » instead of « Stade Rochelais vs Bayonne ». Root cause: `Event.id` was **positional** (`(journée-1)*7 + index`) and `calendar::diff` matched on id only, so after LNR reordered the journée the same id pointed to another fixture → wrong email (and wrong subscription lookup).
- Fix : `Event.id` is now the **LNR match id** (segment of the `feuille-de-match/.../{id}-...` link, present on every row incl. `Tbd`). Type widened `u16 → u32` (`Event`, `Change::Scheduled`, `MatchView`, `process_one`). Positional id kept only as fallback when the link is absent (offline fixtures).
- `calendar::diff` now also requires the previous and current events to be the **same match** (same `team.id`/`opponent.id` slugs) before emitting `Tbd → Scheduled`.
- Migration `0005_clear_match_notifications.sql` : wipes stale positional subscriptions (test phase, no corrective email sent).
- Fixtures `lnr.html`/`lnr_played.html` gained `feuille-de-match` links; new tests: id extraction, positional fallback, diff id-reassignment guard.
- Verified : `cargo test --workspace` green (51 passed, 1 ignored), clippy `-D warnings` + `fmt --check` green.

### 2026-10-14 — Telegram notifications (done)
- Goal: match alerts delivered via Telegram **in addition to** email. Account linking uses a **bot deep-link + webhook**, never manual ID entry.
- Migration `0006_add_telegram_id.sql` (`users.telegram_id TEXT`) + `0007_create_telegram_link_tokens.sql` (partial unique index on `telegram_id`, `telegram_link_tokens` table with `ON DELETE CASCADE`).
- New module `src/telegram.rs` : trait `Telegram` (+ `NoopTelegram`), `TelegramClient` (reqwest POST `sendMessage`), `from_env()`/`config_from_env()`/`register_webhook()` (startup `setWebhook`), `Update::start_token()`, constant-time `secret_matches()`, `create_link_token`/`consume_link_token`/`unlink`, `deep_link`. Link tokens are single-use, 15 min TTL.
- `models` : `User.telegram_id`, `UserView.telegram_linked` (raw chat id never exposed). All `query_as::<_, User>` SELECTs updated.
- `AppState` gains `telegram: Arc<dyn Telegram>` + `telegram_config: TelegramConfig`.
- Endpoints (`src/routes/telegram.rs`) : `POST /api/telegram/link` (`ActiveUser`, returns deep link), `DELETE /api/telegram/link` (unlink), `POST /api/telegram/webhook` (public, verifies `X-Telegram-Bot-Api-Secret-Token`).
- `notifications::process_one` now dispatches email **and** Telegram per subscriber.
- Registration form: removed the manual Telegram ID field (linking happens post-login from the home page).
- Env vars : `TELEGRAM_ENABLED`, `TELEGRAM_BOT_TOKEN`, `TELEGRAM_BOT_USERNAME`, `TELEGRAM_WEBHOOK_URL`, `TELEGRAM_WEBHOOK_SECRET` (added to README, `.env.example`, `docker-compose.yml`).
- Verified : `cargo test --workspace` green (6 new telegram tests), clippy `-D warnings` + `fmt --check` green, `npm run check` + `npm run build` green.

### 2026-10-14 — Notification settings modal (per-channel prefs)
- Goal: let users pick which channel receives match alerts (email and/or Telegram), from a modal.
- Migration `0008_add_notification_prefs.sql` : `users.notify_email` / `users.notify_telegram` (`INTEGER NOT NULL DEFAULT 1`).
- `models` : `User` + `UserView` gain both flags ; all `query_as::<_, User>` SELECTs updated.
- Endpoint `PUT /api/notifications/settings` (`ActiveUser`, `src/routes/notifications.rs`) → `notifications::update_settings` (updates both booleans). Initial values come from `/api/me`.
- `notifications::process_one` now fetches `notify_email`/`notify_telegram` per subscriber and gates each channel (both can be muted → user receives nothing). Welcome email unchanged.
- Frontend `+page.svelte` : Telegram card replaced by a **Notification settings** button opening a native `<dialog>` modal (connect/disconnect Telegram + email/Telegram toggles, explicit **Save**, no autosave). `api.ts` gains `updateNotificationSettings`, `types.ts` gains the flags. `invalidateAll()` refreshes `data.user` after save/connect.
- Verified : `cargo test --workspace` green (3 new notification tests), clippy `-D warnings` + `fmt --check` green, `npm run check` (0 warning) + `npm run build` green.

### 2026-10-14 — Layered `src/` reorg (`common/` + `services/`)
- Goal: separate the foundations and the business/integration layer from the HTTP layer, and remove the `telegram.rs` vs `routes/telegram.rs` naming ambiguity.
- `git mv` foundations to `src/common/` : `db`, `error`, `models`, `time`.
- `git mv` services to `src/services/` : `auth`, `calendar`, `email`, `mailer`, `notifications`, `state`, `telegram`, `waitlist`.
- `src/routes/` untouched. `lib.rs` now declares only `pub mod common; pub mod routes; pub mod services;`.
- All internal `crate::X` and public `ovalbot::X` paths rewritten (`main.rs`, `bin/create_admin.rs`, `tests/*`). Clean break — no compatibility `pub use`.
- No change to `Cargo.toml`, Docker, CI or migrations (`Dockerfile` copies `src ./src` wholesale).
- Verified : `cargo test --workspace` green, clippy `-D warnings` + `fmt --check` green.

### 2026-10-14 — Coverage: routes + mailer transport
- Goal: raise coverage on `src/services/auth.rs`, `src/services/mailer.rs` and the whole `src/routes/` layer.
- `ResendMailer` gained an injectable `base_url` (`with_base_url`), default `https://api.resend.com`.
- `tests/routes.rs` (29 tests): full HTTP layer via `ovalbot::app(state)` + `tower::ServiceExt::oneshot` — register/login/logout/me, admin (list/activate/delete), calendar (teams/matches), notifications (subscribe/settings), telegram (link/unlink/webhook). Covers `AuthUser`/`AdminUser`/`ActiveUser` + `load_session`/`load_user` (missing/expired/unknown session, deleted user).
- `tests/mailer.rs`: transport tests against a tiny in-process **axum** mock (binds `127.0.0.1:0`, captures the last request) — `send_welcome`/`send_match_alert` POST `/emails` (auth header + JSON body), non-2xx → `MailError::Api`. No new dependency. `src/services/mailer.rs` unit: `NoopMailer` + default `base_url`.
- Note: `wiremock` was tried first but its heavy dep tree made the `mailer` test binary OOM the linker on the memory-constrained arm64 dev box → replaced by the axum mock.
- `tests/common/mod.rs`: shared `test_state`/`test_state_with`, `insert_user`, `insert_session`, `insert_user_with_password`.
- Note: sqlx enables SQLite foreign keys by default in `#[sqlx::test]`; the orphaned-session test toggles `PRAGMA foreign_keys = OFF` on a dedicated connection.
- Not covered: `mailer::from_env` (env vars, parallel-test race) — left out.
- Verified : `cargo test --workspace` green, clippy `-D warnings` + `fmt --check` green.

### 2026-10-14 — "Bot settings" modal (UI only)
- `frontend/src/routes/+page.svelte` : a robot button (inline SVG) added next to the bell on **every** match card, opening a native `<dialog>` "Bot settings".
- Modal content: checkboxes "Search for duo tickets" / "Search for single tickets" + number input "Search group tickets". Local `$state` only.
- **No backend/API/persistence, nothing triggered** — `Save` just closes (placeholder for Phase 4).
- Unrelated to the admin card's "Bot settings" link (left unchanged).
- Verified : `npm run check` (0 warning) + `npm run build` green.
