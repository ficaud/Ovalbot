#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Entrypoint — Adjusts the container user UID/GID to match the workspace
# volume owner, so that cargo/npm/git work regardless of the host user's UID.
#
# Personal developer tooling (nvim, zsh, gitconfig, ...) is intentionally NOT
# handled here; it lives in the `ovalbot-devenv` overlay image built on top of
# this one.

set -eu

USERNAME="${USERNAME:-devuser}"
USER_UID="${USER_UID:-1000}"
USER_GID="${USER_GID:-1000}"
WORKSPACE="${WORKSPACE:-/workspaces/OvalBot}"

# ---- Detect the target UID/GID for the runtime user -------------------
# On macOS Docker Desktop the workspace mount reports root:root, so the real
# host UID/GID must be supplied explicitly via OVALBOT_HOST_UID/GID (set by the
# personal overlay). On Linux we fall back to the workspace owner.
if [ -n "${OVALBOT_HOST_UID:-}" ] && [ -n "${OVALBOT_HOST_GID:-}" ]; then
    MOUNT_UID="$OVALBOT_HOST_UID"
    MOUNT_GID="$OVALBOT_HOST_GID"
elif [ -d "$WORKSPACE" ]; then
    MOUNT_UID="$(stat -c '%u' "$WORKSPACE")"
    MOUNT_GID="$(stat -c '%g' "$WORKSPACE")"
else
    MOUNT_UID="$USER_UID"
    MOUNT_GID="$USER_GID"
fi

# ---- (Re)create the user with the detected IDs -------------------------
if [ "$MOUNT_GID" -ne "$(id -g "$USERNAME" 2>/dev/null || echo 0)" ]; then
    getent group "$USERNAME" >/dev/null 2>&1 || groupadd "$USERNAME" 2>/dev/null || true
    groupadd --gid "$MOUNT_GID" "$USERNAME" 2>/dev/null || \
        groupmod -o -g "$MOUNT_GID" "$USERNAME" 2>/dev/null || true
fi

if [ "$MOUNT_UID" -ne "$(id -u "$USERNAME" 2>/dev/null || echo 0)" ]; then
    usermod -o -u "$MOUNT_UID" -g "$MOUNT_GID" "$USERNAME" 2>/dev/null || true
fi

# Make sure home directory has correct ownership
chown -R "$MOUNT_UID:$MOUNT_GID" "/home/$USERNAME" 2>/dev/null || true

# Make the cargo registry writable by the runtime user. It is populated
# (root-owned) by the image build's `cargo fetch`; without this, cargo and
# rust-analyzer fail with "Permission denied" when fetching dependencies.
chown -R "$MOUNT_UID:$MOUNT_GID" /usr/local/cargo 2>/dev/null || true

# ---- Ensure the effective user has passwordless sudo -------------------
EFFECTIVE_USER="$(getent passwd "$MOUNT_UID" | cut -d: -f1)"
if [ -n "$EFFECTIVE_USER" ] && [ "$EFFECTIVE_USER" != "$USERNAME" ]; then
    echo "$EFFECTIVE_USER ALL=(ALL) NOPASSWD:ALL" > "/etc/sudoers.d/$EFFECTIVE_USER"
    chmod 440 "/etc/sudoers.d/$EFFECTIVE_USER"
fi

if ! grep -qs "^${USERNAME}" /etc/sudoers.d/"${USERNAME}" 2>/dev/null; then
    echo "$USERNAME ALL=(ALL) NOPASSWD:ALL" > "/etc/sudoers.d/$USERNAME"
    chmod 440 "/etc/sudoers.d/$USERNAME"
fi

# ---- Drop privileges and execute the requested command -----------------
if [ "$#" -eq 0 ]; then
    set -- sleep infinity
fi

exec sudo -E -H -u "$USERNAME" "$@"
