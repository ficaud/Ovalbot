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
