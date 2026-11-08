#!/usr/bin/env bash
# scripts/devbox-up.sh — start the dev container with the optional devbox image.
#
# The devcontainers CLI does not read .env files for Dockerfile-based dev
# containers: it substitutes ${localEnv:VAR} from the shell environment. This
# wrapper sources .env.devbox (gitignored) when present and exports
# DEVBOX_BASE_IMAGE, so a personal image reference stays out of the repo. With
# no .env.devbox, the pinned Ubuntu 24.04 default from the Dockerfile is used.
#
# Usage: ./scripts/devbox-up.sh [devcontainer up args]
#   e.g. ./scripts/devbox-up.sh --remove-existing-container
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ -f .env.devbox ]]; then
  set -a
  # shellcheck disable=SC1091
  . ./.env.devbox
  set +a
fi

: "${DEVBOX_BASE_IMAGE:=ubuntu:24.04}"
export DEVBOX_BASE_IMAGE

exec devcontainer up --workspace-folder . "$@"
