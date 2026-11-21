#!/usr/bin/env bash
# Build a GitHub-release-shaped tarball for the host architecture.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "${ROOT}/Cargo.toml" | head -n1)}"
case "${ARCH:-$(uname -m)}" in
    x86_64|amd64) ARCH=x86_64 ;;
    aarch64|arm64) ARCH=aarch64 ;;
    *) printf 'unsupported architecture: %s\n' "${ARCH:-$(uname -m)}" >&2; exit 1 ;;
esac

cd "$ROOT"
cargo build --release --locked

STAGE="$(mktemp -d "${TMPDIR:-/tmp}/screen-time-release.XXXXXX")"
trap 'rm -rf -- "$STAGE"' EXIT INT TERM

mkdir -p "$STAGE/bin" "$STAGE/share/rofi" "$STAGE/systemd" "$ROOT/dist"
install -m 755 target/release/screen-time "$STAGE/bin/screen-time"
install -m 755 share/rofi-screen-time "$STAGE/bin/rofi-screen-time"
install -m 644 share/rofi/screen-time.rasi "$STAGE/share/rofi/screen-time.rasi"
install -m 644 systemd/screen-time.service "$STAGE/systemd/screen-time.service"
install -m 644 LICENSE README.md "$STAGE/"

ASSET="screen-time-${ARCH}-linux.tar.gz"
tar -czf "$ROOT/dist/$ASSET" -C "$STAGE" .
(
    cd "$ROOT/dist"
    if [ -f SHA256SUMS ]; then
        grep -v "  ${ASSET}$" SHA256SUMS > SHA256SUMS.tmp || true
        mv SHA256SUMS.tmp SHA256SUMS
    fi
    sha256sum "$ASSET" >> SHA256SUMS
)
printf '%s\n' "$ROOT/dist/$ASSET"
