# SPDX-License-Identifier: GPL-3.0-or-later

# Devcontainer image for OvalBot (Rust/Axum + SvelteKit + SQLite).
#
# Derived from the published devbox image, which provides the portable terminal
# environment (zsh, tmux, Neovim, LazyGit, OpenCode, ripgrep, fd, fzf, socat,
# openssh-client, build-essential) and the `dev` user. This layer only adds the
# project toolchain: Rust (+ rustfmt/clippy/rust-analyzer/llvm-tools for
# coverage), Node 24, the SQLite CLI and the native dependencies required by
# the parser crates (libcurl, OpenSSL).
#
# The base image's ENTRYPOINT (`onstart`) is inherited: it bootstraps the
# dotfiles and the Neovim configuration at container start. The host SSH agent
# is exposed by `devbox-agent-relay` from the devcontainer postStartCommand.

FROM ghcr.io/busymoood/dotfiles-docker:main

SHELL ["/bin/bash", "-o", "pipefail", "-c"]
ENV DEBIAN_FRONTEND=noninteractive

USER root

# ------------------------------------------------------------------
# 1. System dependencies
# ------------------------------------------------------------------
# sqlite3: CLI used by doc/SQL_HELPERS.md (the app links SQLite through sqlx).
# libsqlite3-dev / libcurl / OpenSSL headers: native crates of the workspace.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        pkg-config \
        sqlite3 \
        libsqlite3-dev \
        libcurl4-openssl-dev \
        libssl-dev \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# ------------------------------------------------------------------
# 2. Rust (pinned toolchain + components)
# ------------------------------------------------------------------
# The version must stay in sync with the repo's `rust-toolchain.toml`: installing
# it here (instead of the floating `stable` channel) means a `cargo` call inside
# the container never has to download the pinned toolchain, which matters in CI
# where the container is recreated on every run.
ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
ENV PATH="${CARGO_HOME}/bin:${PATH}"

RUN curl --proto '=https' --tlsv1.2 -sSf --retry 5 --retry-delay 5 https://sh.rustup.rs \
      | sh -s -- -y --no-modify-path --default-toolchain 1.100.0 --profile minimal \
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
