# SPDX-License-Identifier: GPL-3.0-or-later

# Devcontainer minimal for OvalBot (Rust/Axum backend + SvelteKit frontend).
# Includes only what is required to build and run the project:
#   Rust (rustup/cargo), Node.js LTS, SQLite (lib + CLI), build essentials.
#
# Personal developer tools (nvim, lazygit, zsh, Oh My Zsh, ...) are
# intentionally NOT installed here; they live in the `ovalbot-devenv` overlay
# repository (cloned at `my-env/`) and are baked into a personal image built
# on top of this one.

FROM ubuntu:26.04

LABEL description="OvalBot dev environment (Rust + SvelteKit + SQLite)"

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

ENV DEBIAN_FRONTEND=noninteractive
ENV TZ=UTC

# ------------------------------------------------------------------
# 1. System dependencies
# ------------------------------------------------------------------
RUN apt-get update \
    && packages="build-essential pkg-config git wget curl ca-certificates libsqlite3-dev sqlite3 sudo" \
    && ok=0 \
    && for i in 1 2 3 4 5; do \
        apt-get install -y --no-install-recommends $packages \
            && { ok=1; break; } \
            || sleep 15; \
    done \
    && test "$ok" -eq 1 \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# ------------------------------------------------------------------
# 2. Rust (rustup, stable toolchain + cargo)
# ------------------------------------------------------------------
ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
ENV PATH="${CARGO_HOME}/bin:${PATH}"

RUN curl --proto '=https' --tlsv1.2 -sSf --retry 5 --retry-delay 5 https://sh.rustup.rs | sh -s -- \
    -y --no-modify-path --default-toolchain stable --profile minimal \
    && rustup component add rustfmt clippy rust-src \
    && cargo --version \
    && rustc --version

# ------------------------------------------------------------------
# 2b. Pre-fetch Rust dependencies (so cargo/rust-analyzer run offline)
# ------------------------------------------------------------------
# The registry under CARGO_HOME is root-owned at this stage; the entrypoint
# chowns it to the runtime user so cargo can write at runtime.
WORKDIR /tmp/prefetch
COPY Cargo.toml Cargo.lock ./
COPY calendar ./calendar
RUN mkdir -p src && echo "fn main() {}" > src/main.rs \
    && cargo fetch \
    && rm -rf src

# ------------------------------------------------------------------
# 3. Node.js LTS (NodeSource) for the SvelteKit frontend
# ------------------------------------------------------------------
ENV NODE_MAJOR=24

RUN curl -fsSL --retry 5 --retry-delay 5 --retry-all-errors https://deb.nodesource.com/setup_${NODE_MAJOR}.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/* \
    && node --version \
    && npm --version

# ------------------------------------------------------------------
# 4. Non-root user
# ------------------------------------------------------------------
ARG USERNAME=devuser
ARG USER_UID=1000
ARG USER_GID=1000

# The base image (ubuntu:26.04) ships a user/group named "ubuntu" with
# UID/GID 1000. Remove it first so "devuser" is the unique UID/GID 1000 owner.
RUN userdel ubuntu 2>/dev/null || true; \
    groupdel ubuntu 2>/dev/null || true; \
    groupadd --gid "${USER_GID}" "${USERNAME}" 2>/dev/null || true; \
    useradd --uid "${USER_UID}" --gid "${USER_GID}" -m "${USERNAME}"; \
    echo "${USERNAME} ALL=(root) NOPASSWD:ALL" > /etc/sudoers.d/"${USERNAME}" && \
    chmod 0440 /etc/sudoers.d/"${USERNAME}"

# Entrypoint — adjusts UID/GID at runtime for volume-mounted workspaces
# (required on macOS Docker Desktop). Drops privileges to USERNAME via
# sudo -E before exec.
COPY .devcontainer/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

USER root
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
WORKDIR /workspaces/OvalBot
