<div align="center">

<img src="frontend/static/logo.png" width="150" alt="Ovalbot logo">

<br/>
<br/>
<br/>

# OvalBot

[![CI](https://github.com/ficaud/Ovalbot/actions/workflows/ci.yml/badge.svg)](https://github.com/ficaud/Ovalbot/actions/workflows/ci.yml)
[![Deploy](https://github.com/ficaud/Ovalbot/actions/workflows/deploy.yml/badge.svg)](https://github.com/ficaud/Ovalbot/actions/workflows/deploy.yml)
[![codecov](https://codecov.io/gh/ficaud/Ovalbot/branch/main/graph/badge.svg)](https://codecov.io/gh/ficaud/Ovalbot)

</br>
</div>

OvalBot notify you when there is a TOP14 match that gets a date or for tickets put on resale in the official team's website. Registration is open, gated by a 10-person waitlist during this early phase.

- Stack: **Rust / Axum** (backend) + **SvelteKit** (frontend) + **SQLite**.
- Hosted on a VPS

## Development

The recommended environment is the devbox dev container described below; the backend and
frontend commands also work on the host with the same toolchain. The Rust version is pinned in
`rust-toolchain.toml` (`1.100.0`): rustup installs it on the first `cargo` command, both on the
host and in the container.

### Backend

```bash
cargo run --bin ovalbot          # start the API + static frontend (port 3000)
RUST_LOG=ovalbot=debug cargo run --bin ovalbot          # start the API + static frontend (port 3000) with backend debug logs
cargo run --bin create_admin -- --email <email> --password <pwd>
cargo test --workspace          # unit + integration tests (both crates)
cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings
```

`cargo run` starts the resale scan loop by default (`BOT_ENABLED` defaults to `true` in the
code). Set `BOT_ENABLED=false` for a UI-only server, and see `.env.example` for the bot and
cart variables.

The backend serves the built frontend from `frontend/build` (override with the
`STATIC_DIR` environment variable). Email HTML is loaded from `templates/`
(override with `TEMPLATE_DIR`); see `welcome.html` and `match_alert.html`.

Log timestamps are rendered in **Paris time** (`Europe/Paris`, DST-aware,
RFC 3339 with offset), independent of the container's system timezone.

### Frontend

```bash
cd frontend
npm ci                          # install exact dependencies from package-lock.json
npm run dev                      # dev server on port 5173 (proxies /api -> :3000)
npm run build                    # static build into frontend/build
npm run check                    # svelte-check (types)
```

### Dev container (devbox)

The recommended development environment is the `devbox` dev container: the published
multi-arch image [`ghcr.io/busymoood/dotfiles-docker:v0.6.0`](https://github.com/busymoood/dotfiles-docker)
(Ubuntu 24.04, zsh, tmux, Neovim/LazyVim, lazygit, opencode, ripgrep/fd/fzf), extended by
`.devcontainer/Dockerfile` with the project toolchain:

- Rust 1.100.0 — the version pinned by `rust-toolchain.toml` and installed by the image
  (`rustfmt`, `clippy`, `rust-analyzer`, `llvm-tools-preview`) — and `cargo-llvm-cov`;
- Node.js 24;
- the SQLite CLI (`sqlite3`) and the native headers of the workspace (`libsqlite3-dev`,
  `libcurl4-openssl-dev`, `libssl-dev`, `pkg-config`).

The repository is mounted at `/workspace` and the container runs as the non-root user `dev`.
Tool state (Neovim plugins, OpenCode auth) persists in the `devbox-home` volume; no private
key lives in the container — the host SSH agent is relayed by `devbox-agent-relay`.

#### Prerequisites

- Docker (Docker Desktop, or Docker Engine + the Compose plugin).
- The devcontainers CLI: `npm install -g @devcontainers/cli`.
- An unlocked SSH agent (Bitwarden, `ssh -A`). On Docker Desktop the
  host agent is exposed by a small `socat` relay on port 4242, installed once as a login
  service — not run per session.

#### Start

```bash
cp .env.devbox.example .env.devbox   # DOTFILES_REPO / NVIM_CONFIG_REPO
./scripts/devbox-up.sh               # exports .env.devbox, then `devcontainer up`
devcontainer exec --workspace-folder . zsh
```

`devcontainer up` reuses an existing container; after a configuration or image change:

```bash
./scripts/devbox-up.sh --remove-existing-container
```

Ports `3000` (Axum / built SvelteKit app) and `5173` (Vite) are published on `127.0.0.1`
(`appPort`).

`.env` remains the production Compose file; `.env.devbox` is dev-only. The current
`DOTFILES_REPO` placeholder points to the image repository, which has no zsh/tmux configs:
nothing is linked until a real dotfiles repository is provided.

#### SSH agent

`devbox-agent-relay` exposes the host agent: a direct socket mount on native Docker, a TCP
relay on Docker Desktop. If Bitwarden was locked at startup, or after a container restart:

```bash
devcontainer exec --workspace-folder . bash -lc \
  'devbox-agent-relay start; onstart --no-shell'
devcontainer exec --workspace-folder . bash -lc \
  'devbox-agent-relay doctor'
```

`postStartCommand` is not replayed on an already running container; the commands above also
re-run the dotfiles/Neovim bootstrap.

See [`doc/devcontainer.md`](doc/devcontainer.md) for the full command reference.

#### Update

```bash
docker pull ghcr.io/busymoood/dotfiles-docker:v0.6.0
./scripts/devbox-up.sh --remove-existing-container --build-no-cache
```

The Neovim/LazyVim configuration and the dotfiles are pulled at each start; no image rebuild
is needed for them.

## Configuration

The backend reads these environment variables:

| Variable | Default | Description |
|---|---|---|
| `DATABASE_URL` | `sqlite://ovalbot.db` | SQLite connection string. |
| `STATIC_DIR` | `frontend/build` | Directory holding the built SvelteKit frontend. |
| `TEMPLATE_DIR` | `templates` | Directory holding the HTML email templates. |
| `SECRET_KEY` | *(empty)* | Server secret for the per-user shop-credentials vault (AES-256-GCM key derived with HKDF-SHA256, domain-separated). Required to save billetterie credentials (fail-closed when absent). Changing it makes previously stored passwords unreadable — users simply re-save them. |
| `RESEND_ENABLED` | `false` | Set `true` to send emails via Resend (welcome + match alerts). |
| `RESEND_API_KEY` | *(empty)* | Resend API key, required when `RESEND_ENABLED=true` (See more on [resend](https://resend.com/)) |
| `RESEND_FROM` | `OvalBot <onboarding@resend.dev>` | Sender address for outgoing emails. |
| `TELEGRAM_ENABLED` | `false` | Set `true` to enable Telegram notifications. |
| `TELEGRAM_BOT_TOKEN` | *(empty)* | Telegram bot token from @BotFather, required when `TELEGRAM_ENABLED=true`. |
| `TELEGRAM_BOT_USERNAME` | *(empty)* | Bot username (without `@`), used to build the account-linking deep link. |
| `TELEGRAM_WEBHOOK_URL` | *(empty)* | Public HTTPS URL of this app; registered via `setWebhook` at startup. |
| `TELEGRAM_WEBHOOK_SECRET` | *(empty)* | Secret token Telegram sends in `X-Telegram-Bot-Api-Secret-Token`, verified on every update. |
| `BOT_ENABLED` | `false` | Set `true` to run the ticket resale scanner loop. |
| `BOT_SCAN_INTERVAL_SECS` | `60` | Seconds between two loop ticks: the direct poll of the stored resale links. |
| `BOT_HOME_REFRESH_SECS` | `60` | Seconds between two billetterie home-page fetches (link discovery). Each tick also re-polls the links learned earlier, so a queued home page never stops the polling. |
| `CART_ENABLED` | `false` | Set `true` to let the bot add matching resale seats to users' shop carts (Phase 4B checkout). Requires `SECRET_KEY` and per-user shop credentials. |
| `CART_DRY_RUN` | `true` | When `true`, the checkout only logs what it *would* add (plus `credentials=present\|missing\|unreadable`): no login, no add-to-cart, no claim, no notification. Set `false` to buy for real. |
| `CHECKOUT_DUMP_DIR` | *(empty)* | When set, a failed add dumps the resale page and/or the AJAX response to this directory (diagnostic only, nothing is written by default — see below). |
| `QUIET_HOURS_START` | 23*60 | Start of the quiet hours window (when there is not purcahse attempt from the bot) |
| `QUIET_HOURS_END` | 8*60 | End of the quiet hours window (when there is not purcahse attempt from the bot) |

With `CART_ENABLED=true`, a request is only closed as `failed` on a refused shop
login. Seat-gone, waiting queue, network, expired-form and missing add-to-cart
context errors all leave it `pending` for the next wave.

See `.env.example` for a ready-to-copy template.

### Diagnose a checkout failure (dump mode)

How to run the bot with debug:

```bash
RUST_LOG=ovalbot=debug,parser=debug \
  cargo run --bin ovalbot
```

`RUST_LOG=ovalbot=debug` shows the checkout decisions (`checkout: pack skipped`,
`checkout wave added nothing`, …); add `parser=debug` to also see the parser's
own `[CHECKOUT]` traces.

**Quitting the live log view.** The `docker compose run … ovalbot` command above
attaches to the container's output:

- `Ctrl+P` then `Ctrl+Q` detaches — the bot keeps running in the background
  (find it with `docker ps`, then `docker logs -f <container>` to bring the
  stream back). Add `--detach-keys="ctrl-x"` to the `run` command if
  `Ctrl+P`/`Ctrl+Q` conflicts with your terminal.
- `Ctrl+C` stops the container (and `--rm` removes it) — the bot stops too.

If you run the service normally instead (`docker compose up -d` followed by
`docker compose logs -f ovalbot`), `Ctrl+C` only closes the log stream and the
bot keeps running. From source (`cargo run`), `Ctrl+C` stops the bot.

## Testing

### Local dev with hot reload (two terminals)

Terminal 1 — backend on `:3000`:

```bash
cargo run --bin ovalbot
```

Terminal 2 — frontend on `:5173` (proxies `/api` -> `:3000`):

```bash
cd frontend && npm run dev
```

Open http://localhost:5173 (ports 3000 and 5173 are published on `127.0.0.1` by the
dev container).

### Create an admin account (to test the admin panel)

```bash
DATABASE_URL="sqlite:///tmp/test.db" cargo run --bin create_admin -- --email admin@example.com --password password123
```

## Database SQL commands

Some SQL commands that you can run on the local database for testing purposes or to get information :
- [SQL_HELPERS.md](SQL_HELPERS.md)

## Deployment

Images are published for both `linux/arm64` and `linux/amd64` — Docker pulls the
matching architecture automatically, so the same tag runs on a Raspberry Pi as
well as on x86_64 servers.

### 1. Prerequisites

- Any arm64 distro or an x86_64 Linux host.
- Docker Engine + the Compose plugin.

```bash
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER          # log out/in afterwards
docker --version && docker compose version
```

### 2. Configure

Get the docker-compose.yml file from the OvalBot GitHub project as an example.

Copy the example env file and adjust the values:

```bash
cp .env.example .env
```

The SQLite database is stored in `./data` next to `docker-compose.yml`. Create it
writable by the container user (uid `10001`) before the first run:

```bash
mkdir -p data && sudo chown -R 10001:10001 data
```

### 3. Run

```bash
docker compose up -d                 # pulls the image and starts detached
docker compose logs -f ovalbot       # follow the logs
```

The service restarts automatically (`restart: unless-stopped`) and persists its
SQLite database in the `./data` directory next to `docker-compose.yml`.

### 4. Create the first admin account

```bash
docker compose exec ovalbot create_admin --email admin@example.com --password '<pwd>'
```

This runs the `create_admin` CLI inside the container, against the same
`DATABASE_URL` and `./data` directory as the server.

### 5. Update to a new version

```bash
docker compose pull && docker compose up -d
```

