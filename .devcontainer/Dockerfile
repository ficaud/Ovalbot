# SPDX-License-Identifier: GPL-3.0-or-later

# Devcontainer image for OvalBot (Rust/Axum + SvelteKit + SQLite).
#
# The base image is configurable: by default a plain Ubuntu 24.04. Set
# DEVBOX_BASE_IMAGE in .env.devbox (sourced by ./scripts/devbox-up.sh)
# to reuse a personal devbox image. 
#
# With the personal image, its ENTRYPOINT (`onstart`) is inherited (it prepares
# the tool-state volume and checks the SSH agent) and postStartCommand relays the
# host SSH agent. On plain Ubuntu both are simply absent — postStartCommand in
# devcontainer.json is guarded with `command -v` so neither path fails.

ARG DEVBOX_BASE_IMAGE=ubuntu:24.04
FROM ${DEVBOX_BASE_IMAGE}

SHELL ["/bin/bash", "-o", "pipefail", "-c"]
ENV DEBIAN_FRONTEND=noninteractive

USER root

# ------------------------------------------------------------------
# 1. System dependencies
# ------------------------------------------------------------------
# sqlite3: CLI used by doc/SQL_HELPERS.md (the app links SQLite through sqlx).
# libsqlite3-dev / libcurl / OpenSSL headers: native crates of the workspace.
# zsh/git/curl/ca-certificates/openssh-client/socat/build-essential: the portable
# terminal basics the personal image already ships (curl is needed below for
# rustup and the NodeSource script).
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        git \
        openssh-client \
        pkg-config \
        socat \
        sqlite3 \
        zsh \
        libsqlite3-dev \
        libcurl4-openssl-dev \
        libssl-dev \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# ------------------------------------------------------------------
# 2. Rust (stable toolchain + components)
# ------------------------------------------------------------------
ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
ENV PATH="${CARGO_HOME}/bin:${PATH}"

RUN curl --proto '=https' --tlsv1.2 -sSf --retry 5 --retry-delay 5 https://sh.rustup.rs \
      | sh -s -- -y --no-modify-path --default-toolchain stable --profile minimal \
    && rustup component add rustfmt clippy rust-src rust-analyzer llvm-tools-preview \
    && rustc --version \
    && cargo --version

# ------------------------------------------------------------------
# 3. Coverage tooling (CI job `coverage`)
# ------------------------------------------------------------------
RUN curl -L --proto '=https' --tlsv1.2 -sSf --retry 5 --retry-delay 5 \
        https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh \
      | bash \
    && cargo binstall -y cargo-llvm-cov \
    && cargo llvm-cov --version

# ------------------------------------------------------------------
# 4. Node.js 24 (SvelteKit frontend)
# ------------------------------------------------------------------
ENV NODE_MAJOR=24

RUN curl -fsSL --retry 5 --retry-delay 5 --retry-all-errors \
        https://deb.nodesource.com/setup_${NODE_MAJOR}.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/* \
    && node --version \
    && npm --version

# ------------------------------------------------------------------
# 5. Cargo permissions for the runtime user
# ------------------------------------------------------------------
# The personal image already ships a `dev` user; plain Ubuntu does not. Create
# it when missing so this layer and `remoteUser` work with either base image.
RUN id -u dev >/dev/null 2>&1 || useradd -m -s /bin/zsh dev

# The devcontainer CLI may change the `dev` UID on Linux; a dedicated group
# with setgid keeps the toolchain and the registry writable regardless of the
# runtime UID.
RUN groupadd -r rustlang \
    && usermod -aG rustlang dev \
    && chown -R dev:rustlang "${RUSTUP_HOME}" "${CARGO_HOME}" \
    && chmod -R g+rwX "${RUSTUP_HOME}" "${CARGO_HOME}" \
    && chmod g+s "${RUSTUP_HOME}" "${CARGO_HOME}"

# The base image's onstart entrypoint must run as `dev`.
USER dev
