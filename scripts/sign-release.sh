#!/usr/bin/env bash
# Sign release metadata with the offline minisign key.
#
# Usage:
#   MINISIGN_SECRET_KEY=/path/to/minisign.key scripts/sign-release.sh [DIST]
#
# The public key is pinned in minisign.pub and in install.sh. The secret key
# must never be committed or downloaded onto a release machine.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="${1:-${ROOT}/dist}"
KEY="${MINISIGN_SECRET_KEY:-}"

die() {
    printf 'sign-release.sh: %s\n' "$*" >&2
    exit 1
}

[ -n "$KEY" ] || die "set MINISIGN_SECRET_KEY to the offline secret key path"
[ -f "$KEY" ] || die "secret key not found: $KEY"
[ -f "${ROOT}/minisign.pub" ] || die "missing pinned public key: ${ROOT}/minisign.pub"
[ -f "${DIST}/SHA256SUMS" ] || die "missing ${DIST}/SHA256SUMS"
[ -f "${DIST}/install.sh" ] || die "missing ${DIST}/install.sh"

command -v minisign >/dev/null 2>&1 || die "minisign is required"
chmod 600 "$KEY"

sign() {
    local file="$1"
    rm -f -- "${file}.minisig"
    minisign -Sm "$file" -s "$KEY" -x "${file}.minisig" >/dev/null
    minisign -Vm "$file" -x "${file}.minisig" -p "${ROOT}/minisign.pub" >/dev/null
    printf 'signed %s\n' "${file}.minisig"
}

sign "${DIST}/SHA256SUMS"
sign "${DIST}/install.sh"
