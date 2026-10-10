<div align="center">

<img src="frontend/static/logo.png" width="150" alt="Ovalbot logo">

<br/>
<br/>
<br/>

# OvalBot

</br>
</div>

Ticket resale helper for Stade Rochelais home games, plus a small website
(registration + waitlist in Phase 1, match calendar and alerts later).

Stack: **Rust / Axum** (backend) + **SvelteKit** (frontend) + **SQLite**.
Hosted on a Raspberry Pi (arm64) via Docker Compose.

## Structure

- `src/` — Rust backend (Axum server + `create_admin` CLI).
- `migrations/` — SQL migrations (sqlx, embedded at runtime).
- `frontend/` — SvelteKit app (Svelte 5, `adapter-static`).
- `doc/specs/` — specifications (`cdc.md` + per-phase specs).

## Development

### Backend

```bash
cargo run --bin ovalbot          # start the API + static frontend (port 3000)
cargo run --bin create_admin -- --email <email> --password <pwd>
cargo test                       # unit + integration tests
cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings
```

The backend serves the built frontend from `frontend/build` (override with the
`STATIC_DIR` environment variable).

### Frontend

```bash
cd frontend
npm install                      # first time only (or npm ci if package-lock.json is committed)
npm run dev                      # dev server on port 5173 (proxies /api -> :3000)
npm run build                    # static build into frontend/build
npm run check                    # svelte-check (types)
```

### Devcontainer

Open a shell with `devcontainer exec --workspace-folder . zsh` or
`./my-env/up.sh`, then:

```bash
./my-env/up.sh                   # build images + start container + link configs
./my-env/up.sh --no-cache        # rebuild images
./my-env/down.sh                 # stop the container
```

To start the container without rebuilding the images (uses the existing
`ovalbot-personal` image):

```bash
export OVALBOT_HOST_UID="$(id -u)" OVALBOT_HOST_GID="$(id -g)"
devcontainer up --workspace-folder . --override-config my-env/devcontainer.override.json
devcontainer exec --workspace-folder . /workspaces/OvalBot/my-env/link.sh
devcontainer exec --workspace-folder . zsh
```

## Testing

### Option A — simulated production (backend serves the built frontend)

```bash
DATABASE_URL="sqlite:///tmp/test.db" cargo run --bin ovalbot
```

### Option B — dev with hot reload (two terminals)

Terminal 1 — backend on `:3000`:

```bash
cargo run --bin ovalbot
```

Terminal 2 — frontend on `:5173` (proxies `/api` -> `:3000`):

```bash
cd frontend && npm run dev
```

Open http://localhost:5173 (ports 5173 and 3000 are forwarded in the devcontainer).

### Create an admin account (to test the admin panel)

```bash
DATABASE_URL="sqlite:///tmp/test.db" cargo run --bin create_admin -- --email admin@example.com --password password123
```

## Rust crate Calendar testing

To manually fetch the calendar and test the parsing you can run the following test manually (this will execute the test for LNR parsing in `src/parse/lnr.rs`): 

```bash
cargo test capture_lnr_fixture -- --ignored
```

## Deployment (Raspberry Pi, arm64)

`deploy.yml` builds a `linux/arm64` image and publishes it to GHCR on every `v*`
tag (`ghcr.io/ficaud/ovalbot:<version>`, plus `<major>.<minor>` and `latest`).
The image contains the compiled backend, the built frontend, and the
`create_admin` CLI. The Pi only needs to pull and run it.

### 1. Prerequisites

- 64-bit OS (Raspberry Pi OS 64-bit / any arm64 distro).
- Docker Engine + the Compose plugin.

```bash
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER          # log out/in afterwards
docker --version && docker compose version
```

### 2. Configure

Copy the example env file and adjust the values:

```bash
cp .env.example .env
```

- `OVALBOT_VERSION`: tag to deploy (pin it, e.g. `v1.0.0`, for reproducibility).
- `PORT`: host port exposed by the container (default `3000`).

### 3. Run

```bash
docker compose up -d                 # pulls the image and starts detached
docker compose logs -f ovalbot       # follow the logs
```

The service restarts automatically (`restart: unless-stopped`) and persists its
SQLite database in the `ovalbot-data` volume.

### 4. Create the first admin account

```bash
docker compose exec ovalbot create_admin --email admin@example.com --password <pwd>
```

This runs the `create_admin` CLI inside the container, against the same
`DATABASE_URL` and data volume as the server.

### 5. Update to a new version

```bash
docker compose pull && docker compose up -d
```

