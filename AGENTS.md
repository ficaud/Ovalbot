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

- Server : `cargo run`
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

## Journal

### 2026-10-07 — Planning & CI/CD
- Moved specs to `doc/specs/` + logo to `doc/assets/`.
- Stack frozen : Axum 0.8.x, sqlx 0.9, argon2 0.6 (Argon2id), Svelte 5, Docker Compose arm64.
- CI/CD : `ci.yml` (lint + test + build, all branches) + `deploy.yml` (`v*` tags → GHCR).
- Language : site, code and comments are English-first.
- Remaining : backend scaffolding (deps, migrations, routes, `create_admin`) + SvelteKit frontend.
