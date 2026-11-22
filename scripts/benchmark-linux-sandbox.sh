#!/usr/bin/env bash
# Isolate the X server, sample processes, and ActivityWatch loopback API.
set -euo pipefail

if [ "$#" -lt 2 ]; then
    printf 'Usage: %s INPUT_DIRECTORY BENCHMARK_OPTIONS...\n' "$0" >&2
    exit 2
fi
inputs=$(realpath "$1")
shift
repo=$(realpath "$(dirname "${BASH_SOURCE[0]}")/..")
mkdir -p "$repo/target" "$inputs/results"

# The outside directory is read-only except for explicit result files. Inside
# the sandbox, Xvfb creates a fresh socket unrelated to the desktop session.
exec bwrap --die-with-parent --unshare-user --unshare-pid --unshare-net \
    --proc /proc --dev /dev --tmpfs /tmp --dir /tmp/.X11-unix \
    --ro-bind /nix/store /nix/store --ro-bind /sys /sys \
    --ro-bind /etc/passwd /etc/passwd --ro-bind /etc/group /etc/group \
    --ro-bind /run/current-system/sw /run/current-system/sw \
    --symlink "$(realpath "$(command -v bash)")" /bin/sh \
    --symlink "$(realpath "$(command -v zsh)")" /bin/zsh \
    --symlink "$(realpath "$(command -v env)")" /usr/bin/env \
    --ro-bind "$repo" "$repo" --bind "$repo/target" "$repo/target" \
    --ro-bind "$inputs" "$inputs" --bind "$inputs/results" "$inputs/results" \
    --chdir "$repo" -- \
    "$(command -v python3)" scripts/benchmark-linux.py "$@"
