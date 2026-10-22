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

- Documentation: every function (pub and private) carries a rustdoc `///` comment — with `# Arguments` / `# Returns` / `# Errors` / `# Panics` sections when relevant — every struct/enum gets a doc line (fields where it clarifies), and every module opens with a `//!` header. Local variables stay uncommented unless non-obvious. Keep docs factual and short; comments stay in English.
- Unit tests are mandatory for every business function implemented.
- Code and comments are written in English.
- Section separators use plain `//` comments (never `///`, reserved for doc comments that attach to the next item).
- Test functions get a one-line `//` intent comment when their name alone doesn't tell the story.
- Axum 0.8 : path params use `{id}` (not `:id`).
- Cookies : `axum-extra` (feature `cookie`).
- Migrations : `sqlx::migrate!()` embedded at runtime + `sqlx-cli` (sqlite) in dev to create files.
- SQL : runtime-checked `query`/`query_as` for Phase 1.

## Journal

### 2026-10-19 — Documentation campaign (lots 1–4)
- Goal: a newcomer can understand the codebase by reading it — every function/struct/module documented, English only, no behavior change.
- Convention adopted (see Conventions): rustdoc `///` on every fn (incl. private, `# Arguments`/`# Returns`/`# Errors`/`# Panics` when relevant), `///` on structs/enums/fields where it clarifies, `//!` module headers, plain `//` for intent lines and one-liners on tests when the name doesn't suffice. "No superfluous comments" relaxed accordingly.
- Lot 1 foundations: `src/common/{error,models,time,db}.rs` (AppError → HTTP mapping policy, User/Session/UserView field semantics, RFC 3339 helpers incl. the fail-closed `is_past`, pool with FKs + 5-conn cap), `src/lib.rs` (`app` route map), `src/main.rs` (6-step startup order), `bin/create_admin.rs`.
- Lot 2 services: `auth` (3 extractors + password/session helpers), `calendar` (cache/locking, sync diff contract, Team/MatchView), `notifications` (fan-out flow, single-fire, per-channel gating), `waitlist`, `email`, `state`, `mailer` (trait/Resend/Noop, templates, `render` placeholders, best-effort `send_welcome_if_unsent`), `telegram` (client, link-token flow, webhook structs), `tickets` (input bounds, kind→columns flattening, status lifecycle, RemoveOutcome — the TicketKind/TicketStatus/TicketRequestView docs were initially lost to a failed edit and reapplied), `matching` (Display + test helpers), plus all `routes/*` handlers (endpoint one-liners with status/error contracts).
- Lot 3 `calendar` crate: `Event`/`MatchStatus` (LNR stable id!), `Change`, `TeamType`/`Team`, `WebPageFetcher` (`fetch_html`, 404-terminates-pagination), `MatchDate` internals (`month_number`, `normalize`), `LnrParser` (parse_rules, pagination, `build_team`), `EventParser` trait.
- Lot 4 `bot/parser`: misplaced `///` module docs converted to real `//!` headers (`match_manager`, `parse_match`, `parse_seat`, `parsers`, `club`, `club_manager`); all `mod.rs` files got headers; seat `json` deserialization structs documented; trait impl delegates get one-line docs; `Proxy`, `WebClient::new`/`set_proxy_mode` documented. Test-only `//` intent lines partially added (descriptive test names left as-is — full test-comment pass deferred).
- Gotchas fixed along the way: doc lines stranded between `use` statements and after `#[tokio::main]` (needed an attribute-aware survey script — the naive prev-line detector false-positives on `#[derive]`), `//!` after `pub mod` in `lib.rs` → E0753, unused import + stray empty `impl` cleaned up.
- Verification: `cargo fmt --check` green, `cargo clippy --workspace --all-targets -- -D warnings` green, `cargo doc --no-deps` 0 warnings (fixed 3 broken intra-doc links to private items), `cargo test --workspace --jobs 2` all green (17 suites).
- Remaining (noted, not done): `#![warn(missing_docs)]` guard lot + full test-intent pass (both deferred deliberately).

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

### 2026-10-17 — Ticket request modal (frontend) + transmission to the backend
- Goal: let each user describe the tickets they want per match (one option per match), and transmit the parameters to the backend — bot integration and persistence come later.
- Frontend: `BotSettingsModal.svelte` → `git mv` `TicketRequestModal.svelte`, rewritten. Exclusive modes (radio): **Single** (1–10, max price per ticket, bought as they become available), **Duo** (one pair, max price for the pair), **Group** (2–10 places, **pelouse only**, no price — the pelouse rate is automatic). Inactive fields stay `disabled` but keep their values. Prices stored in **cents**, validation mirrored in the UI (`Save` disabled + inline reason), `Remove request` only when a draft exists.
- `frontend/src/lib/types.ts`: `TicketKind`, `TicketRequestDraft` (UI draft, three sub-objects) + `TicketRequestInput` (wire union, tagged on `kind`).
- `frontend/src/lib/api.ts`: `putRequest(draft)` (`PUT /api/requests/{match_id}`), mapping draft → wire union lives here.
- `+page.svelte`: `requests` keyed by match id, robot button turns active when a request exists, `saveRequest` is async — the modal stays open and shows the error if the server refuses.
- Backend: `src/routes/tickets.rs` — `PUT /api/requests/{match_id}` (`ActiveUser`), payload = tagged enum `TicketRequestInput`, bounds validated (mirror of the modal, `MAX_PRICE_CENTS = 100_000`), then `tracing::info!` and `204`. New `AppError::InvalidTicketRequest` → `400 invalid_ticket_request`. `AppState` unchanged.
- **No persistence, no `GET`/`DELETE`, no decision logic, no bot** — the frontend stays the source of truth for the UI (drafts are lost on reload, assumed).
- Tests: 4 unit (`is_valid` per kind) + 3 route tests (`tests/routes.rs`: every kind → 204, out-of-range → 400, anonymous → 401 / waiting → 403). Smoke-tested end to end (`RUST_LOG=ovalbot=info` shows `ticket request received` with `user_id`/`match_id`/params).
- Verified: `cargo test --workspace` green, clippy `-D warnings` + `fmt --check` green, `npm run check` (0 warning) + `npm run build` green.

### 2026-10-17 — Ticket request removal (transmission + log)
- Goal: transmit the removal of a request to the backend, still without persistence.
- Backend: `DELETE /api/requests/{match_id}` in `src/routes/tickets.rs` (`ActiveUser`, no `State`), logs `ticket request removed` with `user_id`/`match_id`, returns `204` — idempotent (a missing request is not an error). Route merged with `PUT` on the same path in `src/lib.rs`.
- Frontend: `deleteRequest(matchId)` in `api.ts`; `+page.svelte`'s `removeRequest` is now async and updates the local map **only after** the server acknowledges (no optimistic removal); `TicketRequestModal.svelte` gains a `removing` state (`Removing…`, buttons disabled) and `saveError` → `serverError`, kept open on failure.
- Tests: 2 route tests (`tests/routes.rs`: anonymous → 401 / waiting → 403, and two consecutive deletes → 204).
- Verified: `cargo test --workspace` green (34 route tests), clippy `-D warnings` + `fmt --check` green, `npm run check` (0 warning) + `npm run build` green; smoke test shows `ticket request removed user_id=1 match_id=42` and `401` when anonymous.

### 2026-10-18 — Ticket request persistence (SQLite, no cache)
- Decision: the database is the single source of truth; the in-memory `TicketManager` (RwLock HashMap) is **removed**. Rationale: the final "accept this offer" decision must be an atomic conditional `UPDATE` in SQL (a cache cannot validate), and SQLite (embedded, indexed, µs queries) makes a mirror unnecessary. Cache can be added later behind the same service API if profiling justifies it.
- Migration `0009_create_ticket_requests.sql`: `(user_id, match_id)` PK with `ON DELETE CASCADE`, flat columns `kind`/`quantity`/`max_price_cents`/`places`, `status TEXT DEFAULT 'pending'` (`pending|accepted|purchased|failed|expired|cancelled` always included — SQLite requires a table rebuild to change a CHECK), `created_at`/`updated_at`, 3 `CHECK`s (note: SQLite passes a CHECK that evaluates to NULL, hence explicit `IS NOT NULL`), indexes on `match_id` and `status`.
- `src/services/tickets.rs`: `TicketKind`/`TicketStatus` (`sqlx::Type`, `rename_all = "lowercase"`), `TicketRequestView` (`FromRow` + `Serialize`, flat shape), `RemoveOutcome { Removed, NotFound, Locked }`, `TicketRequestInput::columns()`.
- Service: `upsert(pool, user_id, match_id, &input)` (atomic `INSERT .. ON CONFLICT(user_id, match_id) DO UPDATE .. WHERE status='pending' RETURNING ..` — one round trip, no duplicate accounts, `created_at` preserved, `None` row → `409`), `remove(pool, ..)` (`DELETE .. WHERE status='pending' RETURNING ..` then a `SELECT` to distinguish `NotFound` from `Locked`), `list_for_user(pool, ..)`. The old `put_request`/`delete_request` wrappers are gone; routes call the service directly.
- HTTP: `PUT`/`DELETE /api/requests/{match_id}` keep their `204` contract (DELETE stays idempotent, now `409 ticket_request_locked` when the row left `pending`); new `GET /api/requests` returns the current user's requests ordered by match id. `AppState` lost its `tickets` field (main + tests reverted).
- Logs: `ticket request created` / `ticket request updated` (distinguished by comparing the returned `created_at` to the bound `now`), `ticket request removed` only when a row was actually deleted.
- Tests: 9 service integration tests (new `tests/tickets.rs`: insert/update-in-place with `created_at` preservation, per-user isolation, kind switch nulls out outgoing columns, locked upsert/remove, scoped sorted list, user-delete cascade, CHECK violation rejection) + 7 new route tests (persistence, second PUT updates in place, survives a new `app()` over the same pool, delete removes from list, list scoped/auth-guarded, locked → `409`).
- Verified: `cargo test --workspace` green (28 lib unit incl. `columns_maps_each_kind`, 9 tickets, 41 routes), `cargo fmt --check` + `clippy --all-targets -D warnings` green, `npm run check` (0 warning). Note: full `cargo test` OOM-killed the linker once on this arm64 box; rerun with `--jobs 2` is enough.
- Remaining (later): frontend sync via `GET /api/requests` (load + prefill modal), bot Phase 4 (`candidates(match_id)` + atomic `claim()`), optional match snapshot columns.

### 2026-10-18 — TELESCRAP bot import + integration étape 1 (scan → logs)
- Goal: integrate the imported TELESCRAP repo (`bot/`) as OvalBot crates, validate the scan pipeline operationally by logging found seats in the terminal. **No ticket_requests linkage yet** (no matching, no notifications, no migration).
- Decisions: (1) no git history — imported in one commit (`bot/.git` dropped); (2) keep curl + openssl transport (no reqwest port); (3) `filter` crate dropped — OvalBot will decide via SQL matching against `ticket_requests`; (4) SR only (`ClubType::Ubb` removed); (5) removed redb persistence + resale-link cache — scan is stateless (the match list page carries resale links; enrichment/cache can return later if needed); (6) aggressive mode (login / add-to-cart / form tokens / Session cookie jar) removed entirely — Phase 4B will re-implement from scratch; `SeatAction` → `SeatId { pack_id, ticket_id }`, `PageContext` reduced to `category_names`, `FetchHtml` reduced to `get_html`.
- Structure: `bot/parser` + `bot/scanner` moved up from `bot/crates/*`, single crate dirs (no more workspace-in-bot); root `members = ["calendar", "bot/parser", "bot/scanner"]`.
- `parser` (curl GET + proxy pool, scraper, chrono): `match_manager::get_seats_from_matches(club, nature)` = one blocking call → match-list parse → seat pages; `html_extract::FetchHtml` (get_html only); `proxy` module kept (`PROXY_ENABLED` default flipped to **false** — direct connections unless `PROXY_ENABLED=true`); preview parser kept but unused.
- `scanner` = thin async wrapper: `app::scan::run_scan(club, nature) -> Vec<Encounter>` (`spawn_blocking`, no config struct / diff / notifier / app_state / scan loop).
- OvalBot integration: `spawn_bot_scan_loop()` in `main.rs` — `BOT_ENABLED` (default `false`), `BOT_SCAN_INTERVAL_SECS` (default 300), logs per scan: info summary `bot scan: N encounter(s), M with resale seats` + debug lines per encounter/seat (title, date, category, full_name, price, pack_id, ticket_id). `tracing-subscriber` gained the `tracing-log` feature (parser's `log::` messages appear in the terminal).
- Diff added (same day): scans are now deduplicated — `SeatKey { title, date, pack_id, ticket_id }` (encounter keyed by title+date only, `Encounter.id` is volatile; seats keyed by site-native ids), `collect_seat_keys` + `log_scanned_encounters(encounters, previous) -> current` in `main.rs`. Info summary every scan (`bot scan: N encounter(s), M seat(s) on resale, K new since last scan`), per-seat debug lines only for seats absent from the previous scan (`bot scan: new seat on resale`); loop keeps the previous set in memory (reset on restart → first scan logs everything once). In-memory only — durable dedup arrives with the `ticket_offers` table at étape 2. 4 bin unit tests (stability, added seat, removed-then-back seat, no-seats).
- Local secrets kept on disk, never committed: `.gitignore` adds `/bot/.env`, `/bot/parser/.env` (real shop credentials, reused in Phase 4B), `/bot/.private/`, `/bot/config_scan.json`, `/bot/proxies.txt` (`matchs.db` already covered by `*.db`); deleted `bot/parser/temp/` (postman dumps), stray `.DS_Store`, crate READMEs + nested Cargo.locks.
- Docker: builder stage gained `libcurl4-openssl-dev libssl-dev` + `COPY bot ./bot`; runtime image gained `libcurl4 libssl3`. Devcontainer installs `libcurl4-openssl-dev` and its dependency prefetch (`cargo fetch` step) now `COPY bot ./bot` too — without it, `cargo fetch` failed on CI with "failed to load manifest for workspace member `/tmp/prefetch/bot/parser`" since the root workspace now lists the bot crates.
- Smoke test (real scan, no login): `BOT_ENABLED=true RUST_LOG=ovalbot=debug,parser=info` → `bot scan: 5 encounter(s), 0 with resale seats` (UBB 25/10, Aviron Bayonnais 8/11, USAP 28-29/11, Castres 19-20/12, Pau 02-03/01); proxy-off INFO log bridged correctly. Seat-level parsing not yet exercised against a real open resale (none live today) — first open resale should be watched in logs.
- Verified: `cargo test --workspace` green (parser 29 inline tests kept), `cargo fmt --check` + `cargo clippy --all-targets -D warnings` green (imported code linted up to OvalBot's gate incl. let-chains refactors), smoke OK.
- Remaining (étape 2): map `Encounter` → OvalBot `match_id` (title opponent + date ±12h vs calendar snapshot), `ticket_offers` migration, `proposals::ingest`, per-user matching vs `ticket_requests` → mailer/Telegram notification. Later: Phase 4B cart flow from scratch, UBB support, optional in-memory resale-link cache, CI deploy image rebuild with curl deps.

### 2026-10-18 — Frontend reload of ticket requests

- Goal: after a reload, the user still sees the matches they sent ticket requests for (robot button active, modal prefilled, removal possible).
- `types.ts`: `TicketRequestStatus` (6 unions) + `TicketRequestView` (wire shape of `GET /api/requests`, snake_case like `UserView`/`MatchView`).
- `api.ts`: `listRequests()` + `toTicketRequestDraft(view)` (inverse of `toTicketRequestDraft`'s input mapping; inactive sub-objects stay null/default, the modal already tolerates that).
- `+page.svelte`: `statuses` map keyed by match id; `onMount` loads teams/notifications/requests with `Promise.all` (`listRequests().catch(() => [])` so an expired session doesn't block the page); `saveRequest`/`removeRequest` keep `statuses` in sync; the robot `open()` call passes `statuses[match.id]`.
- `TicketRequestModal.svelte`: `open(selected, draft, existingStatus?)` + `locked` derived from `status !== 'pending'`. When locked (accepted/purchased/…): inline alert “This request is now “ACCEPTED” and can no longer be modified.”, whole `<fieldset disabled>` (radios + inputs) and both `Save`/`Remove` buttons disabled — avoids a bare 409.
- Cleanup: removed a duplicate `let calendar = Arc::new(...)` in `main.rs` (shadowing leftover from the `TicketManager` removal).
- Smoke test (real server): login as admin → `PUT /api/requests/42` → `GET /api/requests` returns the exact wire shape (`match_id`, `kind`, `quantity`, `max_price_cents`, `places`, `status`, timestamps); log shows `ticket request created user_id=1 match_id=42`; bundle checked (`api/requests` present in the built JS).
- Verified: `npm run check` (0 warning) + `npm run build` green, `cargo fmt --check` + `clippy --all-targets -D warnings` green.
- Remaining (later): cross-team “my requests” summary, bot Phase 4.

### 2026-10-18 — Bot coverage push + preview removal
- Goal: raise `bot/` test coverage (parser was ~40% module-wide with key files at 0%), then drop the Pacifa3d seat-preview retrieval entirely (OvalBot will never use the visual preview).
- Fixtures committed under `bot/parser/src/app/clubs/larochelle/fixtures/`, captured live on 2026-10-18: `match_list.html` (97 KB billetterie home: 31 cards, 5 rugby) and `seat_page.html` (70 KB real open basketball resale page — ÉVREUX product 1368, 3 packs with real seat JSONs; rugby resale for UBB existed too but with no listable seats). Frozen values asserted in tests.
- New tests (parser 28 → 54): `parse_match` (fixture assertions: counts, rugby titles/dates, resale links absolutised, badge-over-title priority, no-href resale `<button>` ignored, dedup, titleless cards skipped), `parse_seat` (fixture: pack/ticket ids, price from `amount_by_ticket`, category via `drupal-settings-json` attributes, composition parsing; malformed `data-json` skipped; unknown category id → empty name; `get_seat_composition` unit), `match_manager` (fake `FetchHtml`: nature filtering, fetch-error → empty/seats-untouched, resale seats populated, no-link → empty seats), `web.rs` (std `TcpListener` canned-HTTP mock: body round-trip, connection refused; `set_proxy_mode`), `club.rs`/`club_manager.rs` trivial, `parse_seat_preview` pure parts (`translate_access`/`resolve_row_id` extracted) — preview parts removed after (see below). `src/main.rs`: `env_flag`/`env_u64` tests (unique `OVB_TEST_*` names).
- Gotchas baked into tests: `<script>` text is a raw element — HTML entities inside `drupal-settings-json` are NOT decoded (synthetic scripts must use real quotes); `MatchNature::from_title` requires the exact prefix "stade rochelais"; PACIFA3D row-id prefixing applies only to 3-letter MACIF groups (FKJ/BRP/DMN), not 2-letter ones (AS, GH…).
- Preview removal (user decision): deleted `parse_seat_preview.rs`, `ParseSeatPreview` trait, `get_seat_preview` (no remaining callers since the aggressive-mode import), `SeatInfo.preview_url` field (+ its 2 `None` construction sites). `SeatComposition` kept (used by seat logs).
- Coverage: TOTAL 71.71% → 84.80% (`parse_match` 100%, `parse_seat` 99.2%, `web.rs` 92.2%, `match_manager` 88.2%, `club.rs`/`club_manager` 100%). Left uncovered by design: network/env paths (`proxy_api` env-driven fetch, `proxy_core` API fetch, `scan.rs` wrapper, `create_admin`, `from_env` fns).
- Verified: `cargo test --workspace --jobs 2` green (17 suites), `cargo fmt --check` + `cargo clippy --workspace --all-targets -D warnings` green, no `preview` reference left in the workspace.

### 2026-10-18 — Encounter → match_id resolver (bot ↔ calendar bridge)
- Goal: tie scanned billetterie encounters to the OvalBot calendar `match_id` (stable LNR id shared with `ticket_requests`/match notifications). Identification only — no persistence, no notification (étape 2 next).
- Key insight: a **year-less `(month, day)` window** comparison suffices — each month occurs once per LNR season, so no year inference anywhere. Windows replace the earlier "±12h" idea (billetterie dates are often weekend ranges: "week-end du 28/29 novembre").
- `calendar/src/match_date.rs` gains `DateWindow { from, to }` (inclusive `(month, day)` pairs, `overlaps()`) and a free `parse_window(date)` that tolerates BOTH sides' formats with one generic token-scanner: LNR kickoffs ("samedi 10 octobre 14h30", "samedi 28 novembre – dimanche 29 novembre"), billetterie dates ("dimanche 25 octobre à 21h05" — times never parse as days), slash pairs ("week-end du 28/29 novembre"/"02/03 janvier"). Exported via `calendar::`.
- New `src/services/matching.rs`: `resolve(encounter, events) -> Result<u32, ResolveError>` — rules in order: `nature == Rugby` (guards the confirmed trap "BORDEAUX🍴REPAS D'AVANT-MATCH", same window as UBB and aliasing to it), `TeamType::from_name(title)` (reused as-is, covers all 5 fixture rugby titles; European cup opponents are outside the LNR calendar → `UnknownOpponent`, logged and dropped), date window from `Encounter.date`, then home (`team == StadeRochelais`) + `opponent` + `status != Finished` + kickoff-window overlap; **exactly one** candidate → `Ok(id)`, 0 → `NoCalendarMatch`, 2+ → `Ambiguous` (never a guess). `ResolveError` variants + Display for logs.
- Wiring: `CalendarService::snapshot()` (full `Vec<Event>` clone of the RwLock); `spawn_bot_scan_loop(calendar)` now takes the calendar Arc (`main()` uses `state.calendar.clone()`); per scan it snapshots and `log_resolved_encounters` logs `bot scan: encounter mapped {title, date, match_id}` at info, `encounter not resolved {title, date, error}` at debug for unknown/unmatched. The existing seat-diff log fn is untouched.
- Note: live accuracy — `run_scan` filters by `MatchNature::Rugby`, so REPAS/PARKING/BASKET cards never reach the resolver anyway (the `NotRugby` guard protects direct callers); cards with a future-opening "MISE EN VENTE" badge are parsed as `Other` and thus excluded from the scan until their resale opens (cup games like Connacht would only appear if their badge turns to RUGBY).
- Tests: 6 `parse_window`/overlap cases (`calendar` crate → 34 lib tests) + 6 `resolve` tests (each of the 5 fixture rugby titles vs hand-written LNR events incl. Tbd kickoffs and range kickoffs; the repas card with a matching title-alias and window still rejected via nature guard; Connacht → unknown opponent; empty date; away/other-opponent/finished/mismatched-window; same-opponent double overlap → Ambiguous). `scanner` untouched, no new deps, no migrations.
- Smoke (real, 2026-10-18): all 5 rugby encounters mapped to real LNR ids — UBB **11864** (25/10, `Scheduled`), Bayonne **11877** (8/11), USAP **11885** (28-29/11, `Tbd` vs weekend-range window: Tbd matches resolve too), Castres **11899** (19-20/12, `Tbd`), Pau **11913** (02-03/01, `Tbd`) — mapping visible ~2s after scan start. Bonus: the UBB resale actually had 1 live seat (Catégorie 3, MACIF BRP Nord, Rang Y · Siège 10, 49.00 €, pack 33727/ticket 49255) — seat parsing exercised against a real open resale end-to-end for the first time.
- Verified: `cargo test --workspace --jobs 2` green (17 suites), `cargo fmt --check` + `cargo clippy --workspace --all-targets -D warnings` green.
- Remaining (étape 2 next): persist resolved offers in `ticket_offers` (match_id included at ingest), then users matching vs `ticket_requests` (kind/quantity/price) and mail/Telegram notifications.

### 2026-10-18 — Scan logging merged, event_id in seat logs
- `log_resolved_encounters` and `log_scanned_encounters` merged into a single `log_scanned_encounters(encounters, events, previous)` — resolution runs once per encounter (was twice, split across the two fns). Per encounter: `Ok` → info `bot scan: encounter mapped {title, date, match_id}`, `Err` → debug `bot scan: encounter not resolved {…, error}` (was emptied to `_ => ()` by mistake mid-edit, which silently dropped mapped lines).
- Per-seat debug lines (only for new seats, unchanged trigger) now carry `event_id = ?Option<u32>` — `Some(LNR id)` when the encounter mapped, `None` otherwise (e.g. cup resales outside the calendar). Replaces the author's in-progress `math_id = %event.id.` line (invalid: slice `.id` + `%Event` has no `Display`).
- Tests: the 3 diff-suite call sites updated to the new 3-arg signature (empty `&[]` snapshot — seat-key collection is orthogonal to the calendar); new guard test `events_do_not_change_the_collected_keys` — a Bayonne encounter scanned with a matching calendar `Event` (11877, range kickoff) collects the exact same keys as with no events, proving resolution only feeds logs. Bin suite now 5 tests.
- Live logging redirects: `/home/devuser/…/shell/*.out` stream files or a foreground run instead of `nohup` (0-byte redirect observed this time); server relaunched (PID 174080), `/api/version` OK.
- Verified: `cargo test --workspace --jobs 2` green (17 suites), `cargo fmt --check` + `cargo clippy --workspace --all-targets -D warnings` green, foreground smoke shows both `encounter mapped` lines and the summary on the live billetterie data.

### 2026-10-20 — Demandes ↔ seats-alert bridge (durable) + email/Telegram delivery
- Two user-validated stages shipped together: (1) a logs-only verification of the match_id ↔ user_id link, then (2) conversion to real notifications — trigger = resale seats that are **new**, channel gates = the existing `notify_email`/`notify_telegram` flags, durable anti-spam = the `ticket_offers` table (the in-memory `SeatKey` diff now only feeds the terminal logs).
- Stage (1) delivered `tickets::pending_requesters` (pending requesters of one match) wired as scan-loop log lines, then got replaced by stage (2). Fix along the way: the working tree carried pre-existing uncommitted edits (the `env_u64` test rewritten to `assert_eq!(…, 7)` vs HEAD's `60`, plus a 2-line comment in `matching.rs`) — test restored in line with the code's `trim()` (`" 60 "` parses to 60); the comment kept.
- Stage (2) (the real notifications):
  - Migration `0010_create_ticket_offers.sql`: `(match_id, title, date, pack_id, ticket_id, first_seen_at)`, PK `(match_id, pack_id, ticket_id)` + match index. Deliberately minimal: the notification-dedup key; Phase 4 may extend it.
  - `tickets::record_offers(pool, match_id, title, date, &[Seat]) -> Vec<OfferKey>`: per-seat `INSERT ... ON CONFLICT DO NOTHING RETURNING` — returns only the seats it actually inserted. A seat already recorded (including from a previous process lifetime) never fires again. `PendingRequester` widened at the same time (JOIN users: `email`, `telegram_id`, `notify_email`, `notify_telegram`).
  - Mailer: new `SeatAlert {opponent, date, seats: Vec<SeatBrief {category, price}>}` + shared `seats_listing()` (email/Telegram); trait method `send_seats_alert` (+ Resend, Noop, `FailingMailer`/`RecordingMailer`); `seats_alert.html` template (subject `Seats on resale: Stade Rochelais vs {opponent}`); `EmailTemplates`/`load_templates` now hold 3 templates with built-in fallbacks.
  - `notifications::notify_new_seats(state, match_id, &SeatAlert)`: fan-out mirroring `process_one` (recipients via `pending_requesters`, per-channel gating, best-effort `warn` + continue, single info line with `emailed`/`messaged` counts, short Telegram text with the listing). Sends nothing when nobody holds a pending request for the match.
  - `bot.rs`: `log_pending_requesters` removed → `notify_fresh_offers(state, encounters, resolutions)`: resolved encounter + seats → `record_offers` → (non-empty return) → `SeatAlert` of just the fresh seats → `notify_new_seats`. `new_seat_keys` removed too (the table is now the trigger; the previous `SeatKey` diff serves the terminal logs only). `spawn(AppState)` keeps the whole `state` inside the task (pool + mailer + telegram reachable).
- Tests: `record_offers` (first call fresh, identical re-scan empty, dedup key per match, no-seats no-op), `pending_requesters` with contact/flags (3 widened tests), `notify_new_seats` (per-channel via `RecordingMailer`/`RecordingTelegram`: linked vs unlinked, preferences, no requesters → nothing), `send_seats_alert` transport + body (axum mock). Counts: lib 42 (−1: the `new_seat_keys` test left with its function), tickets 15, notifications 11, mailer 6.
- Smoke (live): `create_admin` on `/tmp/ovalbot-smoke.db`, server `RUST_LOG=ovalbot=info,ovalbot::services::bot=debug BOT_ENABLED=true`, login + 5× `PUT /api/requests/{11864,11877,11885,11899,11913}` (all 204, `GET /api/requests` lists them) → two scans `5 encounter(s), 0 seat(s), 0 new`, zero warns, zero `encounter not resolved` lines. The alert trigger could not be demonstrated against real data: **no seats on resale today** (the UBB resale that had 1 live seat on 10-18 no longer lists any). Watch `ticket offers recorded` / `seats alert processed` on the first open resale.
- Gotchas: `RecordingMailer` (tests/common) must implement every `Mailer` trait method or the route integrations stop compiling; per-user assert ordering `[email-only, telegram-only]` comes from `ORDER BY user_id`, not from the channels.
- Verified: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace --jobs 2` all green (18 suites).
- Remaining (next stage): per-kind eligibility matcher (Single/Duo/Group + price-in-cents parsing + pelouse category) slotted between `record_offers` and `notify_new_seats`; richer email body (billetterie link); real end-to-end Telegram check (only Noop today).

### 2026-10-22 — Durable resale links (`resale_links`) + direct poll, `SeatKey` diff retired
- Goal: persist the bot's billetterie links so the home page is only hit for **discovery** and the open-resale product pages are polled **directly** each tick — plus one single "new seats" source of truth (`ticket_offers`), killing the in-memory `SeatKey` diff (the old `previous` HashSet reset on restart and re-logged stale seats as "new").
- Live facts driving the design (2026-10-21/22): the Wetix waiting queue (`307` → `static-staderochelais.mutu.fa.wetix-agency.fr`) is intermittent and hits home **and** product pages; a stored resale link stays pollable even when the home card stops advertising it (UBB `1357` still `200` with "Aucun" after its resale closed); the home page is the only place resale links ever appear (`/fr/cse-rugby` is login-gated, `/fr/billetterie-secondaire` is not a listing, main product pages do not link to their resale product).
- Migration `0011_create_resale_links.sql`: `(title, date)` PK (stable site keys, cf. the old TELESCRAP `stable_id`), `resale_link`, nullable `match_id`, `first_seen_at` (preserved) / `last_seen_at` / `last_scanned_at` / `last_scan_status` (`ok|no_seats|queue|http_error` — no CHECK, deliberate), index on `match_id`.
- New `src/services/resale.rs`: `ScanTarget::to_encounter` (rebuilds a parser `Encounter` for the seat fetch + `date_passed()`), `upsert_discovered` (one `INSERT .. ON CONFLICT UPDATE RETURNING (first_seen_at = ?)` per link, distinguishes insert/update; `excluded` is NOT usable in SQLite `RETURNING`), `links_to_scan` (deterministic order), `mark_scanned`, `delete_one`; `DiscoveryCounts` (`with_link/inserted/updated`) feeds the log line. Links-only by contract: encounters without a link are never stored; rows absent from a discovery pass are left untouched (a link learned earlier keeps polling).
- Parser split: `controller::html_extract` gains `Fetched` + `FetchHtml::get_html_tracked` (default impl = plain body, the real `WebClient` returns curl's `effective_url`); `match_manager` gains `ScanError {Fetch, WaitingQueue}` + `is_waiting_queue` (host `wetix-agency.fr` or the bare "Move to waiting queue" body), `get_home_encounters` (discovery, no seat fetch — the old `content.unwrap_or_default()` silent failure became a real error), `get_seats_for_encounter` (direct poll, injectable `get_seats_with` for tests); `get_seats_from_matches`/`scanner::run_scan` deleted → `scanner::{discover, fetch_seats}`.
- `services/bot.rs` rewritten as two passes: `discover_home` (paced by `BOT_HOME_REFRESH_SECS`, default 60; first tick always; queued/failed home → warn, never aborts; 0 parsed cards warns and forces a retry next tick) then `poll_links` (every `BOT_SCAN_INTERVAL_SECS`, default 60): per target → `date_passed()` → `delete_one` + log, else fetch seats → `record_offers` → per-fresh-seat info lines + poll summary → `notify_new_seats` → `mark_scanned`. Unresolved links (basket!) are polled but never recorded/alerted (debug log).
- `SeatKey`/`collect_seat_keys`/`log_scanned_encounters`/`previous` gone: logged seats == `record_offers`-inserted seats, shared with the alert via `seats_matching` (unit-tested). Restart no longer produces the false "new seat" storm (verified live: second start logs `new=0 updated=2`).
- Tests: 7 `tests/resale.rs` integration (insert/update-in-place with timestamp preservation, link replacement on a new wave, match-id refresh, deterministic order incl. `match_id IS NULL`, `mark_scanned`, retirement by past date), parser `is_waiting_queue`/queue via Stub (`get_seats_with`), `WebClient::get_html_tracked` effective URL. Gotcha: error enums can't `assert_eq!` against `Result<Vec<Encounter>, _>` (no `PartialEq` on `Encounter`) → `matches!`/panic patterns; closed-port network tests race with other tests' ephemeral listeners → stub-based tests preferred.
- Smoke (live, two runs): `bot home refresh cards=31 links=2 new=2 updated=0` (ÉVREUX j6 23/10 + DENAIN j8 06/11, both basketball) then `new=0 updated=2` on restart; poll marks `no_seats` (closed product) / `ok`; unresolved encounters drop to debug. README gains the `resale_links` probe queries + the `BOT_HOME_REFRESH_SECS` row; `BOT_SCAN_INTERVAL_SECS` default aligned to 60 everywhere (README/compose kept `300` before — the 2026-10-19 mismatch is reconciled). Side fix in `notify_new_seats`: the `emailed`/`messaged` counters were declared but never incremented (clippy `unused_mut` would have failed the gate).
- Verified: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace --jobs 2` all green (18 suites, 61 parser + 7 resale).
- Remaining: per-kind eligibility matcher between `record_offers` and `notify_new_seats`; dead-link (non-product page) detection beyond the queue; per-kind observability for basket links if they become interesting.

### 2026-10-22 — Alert payloads extracted out of the mailer (`services::alerts`)

- Motivation: `MatchAlert`/`SeatAlert`/`SeatBrief` + the shared `seats_listing()` formatting lived inside the **email transport** (`mailer.rs`) while being consumed by both channels — `notifications.rs` (Telegram text) and `bot.rs` (alert building) imported alert data types from a transport. Now that Telegram consumes the same listing, the coupling had to go.
- New `src/services/alerts.rs`: channel-neutral payloads (`MatchAlert`, `SeatBrief`, `SeatAlert`) + `seats_listing()` — pure data + display, no transport, no persistence. `mailer.rs` keeps only the `Mailer` trait, Resend/Noop and the templates, and imports the types from `alerts`; no `pub use` compatibility shim (same "clean break" policy as the 2026-10-14 layering).
- Route/behavior contracts untouched: `Mailer` trait signatures, `notify_new_seats`, `bot::poll_one` unchanged — a pure move plus fixture hygiene.
- Fixture fixes riding along: `tests/mailer.rs` still asserted the pre-`bloc/row/seat_number` listing format ("Pelouse · 49.00 €") → updated to the current one ("NORD - MACIF BRP - Rang U - 2 for 49.00 €"); `tests/notifications.rs` fixtures carried prices already suffixed with `€` ("45.00 €") while `seats_listing` appends `€` itself (parser raw prices are unsuffixed, e.g. "44.00" per `match_manager` tests) → fixtures normalized to raw prices, format aligned to its own doc ("for {} €" with a space). The notifications assertion `contains("45.00 €")` keeps passing via the listing output.
- Gotcha: `#[cfg(test)]` in `mailer.rs` uses `use super::*` — `SeatAlert` came through but `SeatBrief` needed its own import; `seat_number` is `u64` (Copy) → clippy caught a needless `.clone()` in `bot.rs`.
- Updated files: `src/services/{mod,mailer,notifications,bot}.rs`, new `src/services/alerts.rs`, `tests/{mailer,notifications}.rs`, `tests/common/mod.rs` (`RecordingMailer` imports).
- Verified: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace --jobs 2` all green (18 suites, 61 parser + 6 mailer + 41 routes + 7 resale).

### 2026-10-22 — Seats-alert email redesigned (new template, real resale-link CTA, French copy)
- Goal: replace the plain `seats_alert.html` with the user-provided "Match Day" design (dark slate + yellow `#FFD500` gradient banner, "Alerte Revente" pill, CTA button) — validated choices: one styled row per seat (no per-category aggregation), the CTA points at the real billetterie resale link, and subject/plain-text/Telegram copy all switch to French (body rows follow the site's own field naming).
- `templates/seats_alert.html` fully replaced: `{{opponent}}` (Affiche box), `{{date}}` (📅 box), `{{seats_html}}` (inside the seats card — one generated `<tr>` pair per seat), `{{link}}` (CTA `href`). Footer reworded ("Envoyé par OvalBot…") to avoid a hard-coded year.
- `alerts.rs`: `SeatAlert` gains `link: String` (the `resale_links.resale_link` value; empty when unknown) + `seats_html()` → `seat_html_row(per seat)` rendering `🎟️ {bloc} | {price} €` bold row + gray `Rang {row} · Siège {n}` detail row; empty parts dropped (pelouse has no bloc → generic "Place" label, packs with no position lose the detail row). 2 unit tests (row pairs + missing-parts tolerance).
- `mailer.rs`: subjects/text French (`Nouvelles places en revente : Stade Rochelais vs {opponent}`), render vars `opponent`/`date`/`seats_html`/`link` (empty link → `#` fallback, tested); `DEFAULT_SEATS_ALERT_HTML` (fallback) rewritten in French around `{{seats_html}}`/`{{link}}`; doc comments updated.
- `notifications.rs`: Telegram seats message now French ("Places de rugby en revente : … / Nouvelles places (N) : …") and appends `Réserver : {link}` when non-empty — test assertions (`Aviron Bayonnais`, `45.00 €`) unaffected.
- `bot.rs`: `link: target.resale_link.clone()` at the `SeatAlert` construction — the link was already persisted, it just never reached the channels before.
- Fix riding along: `seats_listing` format string said `for {}€` while its own doc (and the 2026-10-22 extraction journal) claimed `for {} €` — the space was never actually applied; now aligned.
- Tests: `tests/mailer.rs` transport test rewritten (French subject, listing + link in plain text, styled rows + `href` in the HTML) + new `send_seats_alert_falls_back_to_hash_without_link`; fixtures normalized to parser-style raw values (`row: "U"`), `SeatAlert` literals gain `link` (notifications fixture, mailer unit test). Preview of the rendered email generated at `/tmp/seats_alert_preview.html` for user sign-off.
- Verified: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace --jobs 2` all green (18 suites, 41 lib + 7 mailer + 41 routes + 61 parser).

### 2026-10-22 — Bot settings gated to Stade Rochelais (UI grey-out + backend guard)

- Goal: the tickets-bot request modal is only meaningful for matches the bot can actually service — the scan covers the Stade Rochelais billetterie (upcoming home games) only. Greying encourages selection only; a backend guard prevents circumvention. User-validated choices: backend guard wanted, existing (orphaned) requests stay reachable for removal, English UI copy.
- Frontend `+page.svelte`: `BOT_SUPPORTED_TEAM_IDS = ['stade-rochelais']` + `isBotSupported(match)` / `isRequestable(match)` (extension note in comment); the "Ticket request" button is `disabled` unless the card is an SR home game **or** a request already exists for it (`requests`/`statuses` map), with a dynamic `title` → "The bot only covers Stade Rochelais home games for now." when greyed. `.bell:disabled` now `cursor: not-allowed` (rule already existed). Modal untouched — removal of orphaned requests flows through the normal path.
- Backend: new `matching::is_bot_covered_match(events, match_id)` — `id` in the snapshot, `team == StadeRochelais`, `status != Finished`, `i64`→`u32` range check; fail-closed (an empty cache accepts nothing). `PUT /api/requests/{match_id}` (`routes/tickets.rs`) guards before `upsert` via `calendar.snapshot()` → new `AppError::UnsupportedTicketRequest` → `400 unsupported_ticket_request`. `DELETE`/`GET` untouched (idempotent cleanup always available).
- `CalendarService::seed_events` (`#[doc(hidden)]`, test-only): `sync` is network-bound, so integration tests inject a synthetic calendar. Gotcha: `#[cfg(test)]` items are invisible from `tests/` — it must be a public (hidden) method. Unit test `seed_events_replaces_the_cache`.
- Tests: `tests/common` gains `calendar_team`/`home_event(home, away, id, status)` builders; `tests/routes.rs` gains `app_with_calendar(pool, events)` and every PUT-flow test now seeds SR home fixtures (ids 7/8/9 and 42/9000 keep their roles). New guard tests: other club's home + SR away → `400`, unknown id with a non-empty calendar → `400`, empty calendar → `400` (router built with plain `app()`, all bodies legal but refused by coverage). Auth-guard tests unaffected (extractors run before the handler body, no seed needed).
- Verified: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace --jobs 2` all green (18 suites, lib 44 incl. 2 new matching tests + seed test, routes 44 = 41 + 3 guard tests); `npm run check` (0 warning) + `npm run build` green.
- Remaining/later: when a second club parser lands, extend `BOT_SUPPORTED_TEAM_IDS` **and** `is_bot_covered_match` together (two places, deliberately — no config plumbing for a single client).

### 2026-10-21 — Fix: resales missed when a match card wears a promotional badge
- **Bug**: the bot logged `4 encounter(s), 0 seat(s)` while the UBB resale (`/fr/product/1357/revente_j7_24_25_oct_stade_rochelais_ubb`) was live with 3 seats. Root cause: the card's badge changed from `RUGBY` to **"DERNIÈRE MISE EN VENTE À 18H00"** (resale deadline), and `parse_match` let badges override the title unconditionally → `from_badge` mapped it to `Other` → the `nature == Rugby` scan filter dropped the card. `from_title` could not recover (requires a "stade rochelais" prefix; opponent-only titles fail). Triggers at the end of every match's selling window.
- **Fix (Option B — nature stops being a decision criterion)**:
  - `MatchNature::from_badge` now returns `Option<MatchNature>` — sport-naming badges ("RUGBY"/"BASKET") classify, promotional badges → `None`; `parse_match` falls back on `from_title` in that case.
  - `get_seats_from_matches(club)` (and `scanner::run_scan(club)`) dropped the nature parameter and filter: seat pages are fetched **only** for cards with an open `<a class="btn-resale">` (≤ 3 in practice), whatever their nature.
  - `matching::resolve` is now the sole arbiter: the `nature == Rugby` guard / `ResolveError::NotRugby` was replaced by `NotAMatch` + `is_not_a_match(title)` (marker fragments: repas, parking, xperience, visite, séjour, hospitalité, réception…). The old guard is still needed as an anti-alias — "BORDEAUX🍴REPAS D'AVANT-MATCH" resolved to UBB before (latent bug), now rejected up front.
- **Fixtures** (captured live 2026-10-21): `match_list_promo_badge.html` (UBB card with deadline badge + open resale, Bayonne/ÉVREUX buttons disabled) and `seat_page_resale_open.html` (UBB resale: 3 seats — 1×MACIF GH 44.00 € pack 33850/ticket 49432 + 2 duo tickets pack 33853). Gotcha: `parse_seat` yields **one seat per ticket** (3 from 2 packs) and the ticket map is a `HashMap` → seat order is random; assert order-insensitively.
- **Tests**: `from_badge_classifies_sports_and_ignores_promos` (encounter.rs), `promo_badge_falls_back_to_title_inference` + `fixture_promo_badge_ubb_keeps_resale_link` (parse_match), `get_matches_returns_every_card` (31) replacing the nature-filter suite, `open_resale_links_are_scanned_regardless_of_nature`. `matching.rs` gained `rejects_non_match_cards` (repas → `NotAMatch`) and `resolves_promo_badge_card_despite_other_nature` (UBB as `Other` → `Ok(101)`). OvalBot lib 43 tests, parser lib 58.
- **Smoke (live, 18:54)**: `bot scan: 31 encounter(s), 5 seat(s) on resale, 5 new` — UBB mapped `event_id=Some(11864)` with its 3 seats logged at info, **Bayonne resale reopened mid-smoke and was caught too** (`Some(11877)`, 1 seat), Denain (basket) scanned then dropped (`event_id=None`); repas/xperience cards rejected via markers; next scan `0 new` (anti-doublon). The missed-ticket bug is fixed end-to-end.
- Durable note: the UBB resale deadline was 18:00 that day — scans after 18:54 may find it closed; the structural fix holds for every future deadline badge.
- Verified: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace --jobs 2` all green.

### 2026-10-19 — Bot scan logic moved out of `main.rs` (`src/services/bot.rs`)
- Goal: keep `main.rs` down to pure wiring (server + DB + spawns), consistent with the layered `src/` convention — the bot loop, seat diff and env helpers were business logic living in the binary.
- New `src/services/bot.rs`: `pub fn spawn(calendar: Arc<CalendarService>)` (the old `spawn_bot_scan_loop`, no API change — `main()` now calls `services::bot::spawn(state.calendar.clone())`) + private `env_flag`/`env_u64`, `SeatKey`, `collect_seat_keys`, `log_scanned_encounters`. Helpers stay module-private (safer than a speculative `common/env.rs` — only the bot parses `1`/`true` flags today; promote later if a second consumer appears).
- Code moved verbatim, zero behavior change (per-seat `new seat on resale` line is currently `info`, only fires for seats absent from the previous scan; resolution `Ok` arm stays silent, `event_id` rides on the seat lines).
- Tests moved with the code from the bin suite to the lib: `services::bot::env_helpers_tests` (2) + `services::bot::tests` (5 seat-diff suite, same names) — bin suite is now empty, lib unit tests 28 → 35.
- Tracing target changes from `ovalbot` to `ovalbot::services::bot` — the default `ovalbot=info` filter still matches (verified).
- Pre-existing mismatch left untouched (to reconcile separately): code defaults `BOT_ENABLED=true` + `BOT_SCAN_INTERVAL_SECS=60` vs README `false`/`300`.
- Verified: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace --jobs 2` all green; smoke (`DATABASE_URL=sqlite:///tmp/ovalbot-smoke.db RUST_LOG=ovalbot=info`) shows `bot scan enabled: every 60s`, `listening`, then `bot scan: 5 encounter(s), 0 seat(s) on resale, 0 new since last scan` on the live billetterie. Note: `/tmp/opencode` is root-owned on this box — disposable DBs go to `/tmp` directly.
