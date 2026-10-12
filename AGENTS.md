# AGENTS.md — OvalBot

Development notes kept up to date by the agent as work progresses.

## Project

Ticket resale bot for Stade Rochelais home games + a simple website (2 pages).
Stack: Rust/Axum (backend) + SvelteKit (frontend) + SQLite. Hosted on a Raspberry Pi (arm64) via Docker Compose. The bot (Phase 4) will be integrated last.

## Structure

- `src/` : Rust backend — Axum server (`main.rs`) + CLI (`bin/create_admin.rs`).
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
