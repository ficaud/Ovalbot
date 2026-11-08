#!/usr/bin/env bash
# scripts/devbox-up.sh — start the dev container with the devbox variables.
#
# The devcontainers CLI does not read .env files: it substitutes
# ${localEnv:VAR} from the shell environment. This wrapper exports
# .env.devbox (gitignored) then runs `devcontainer up`, so that personal
# repository URLs are not hard-coded in .devcontainer/devcontainer.json.
#
# Usage: ./scripts/devbox-up.sh [devcontainer up args]
#   e.g. ./scripts/devbox-up.sh --remove-existing-container
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ ! -f .env.devbox ]]; then
  echo "ERREUR : .env.devbox absent — copier le modèle puis l'ajuster :" >&2
  echo "  cp .env.devbox.example .env.devbox" >&2
  exit 1
fi

# Load .env.devbox (shell format: KEY=value, quotes accepted, # comments).
set -a
# shellcheck disable=SC1091
. ./.env.devbox
set +a

: "${DOTFILES_REPO:?DOTFILES_REPO manquant dans .env.devbox}"

exec devcontainer up --workspace-folder . "$@"
