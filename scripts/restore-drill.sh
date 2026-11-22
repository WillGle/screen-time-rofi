#!/usr/bin/env bash
# Verify an update backup without touching the live installation.
#
# A snapshot is copied into a temporary state directory, checked against its
# manifest, opened with `doctor --json --no-runtime`, and compared with
# `check-data`. Use --oldest to exercise the oldest retained generation.
set -euo pipefail

BACKUP_ROOT="${SCREEN_TIME_BACKUP_ROOT:-${XDG_STATE_HOME:-${HOME}/.local/state}/screen-time/backups/updates}"
BIN="${SCREEN_TIME_BIN:-${HOME}/.local/bin/screen-time}"
OLDEST=0

usage() {
    cat <<'EOF'
Usage: restore-drill.sh [BACKUP_ROOT] [--oldest] [--binary PATH]

Restore the newest (default) or oldest verified update backup into a temporary
state directory and verify its manifest, quick_check, fingerprint, and modes.
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --oldest) OLDEST=1; shift ;;
        --binary)
            [ "$#" -ge 2 ] || { printf 'restore-drill.sh: --binary requires a path\n' >&2; exit 1; }
            BIN="$2"
            shift 2
            ;;
        -h|--help) usage; exit 0 ;;
        *) BACKUP_ROOT="$1"; shift ;;
    esac
done

die() {
    printf 'restore-drill.sh: %s\n' "$*" >&2
    exit 1
}

[ -x "$BIN" ] || die "screen-time binary not found: $BIN"
[ -d "$BACKUP_ROOT" ] || die "backup root not found: $BACKUP_ROOT"
have_jq=0
command -v jq >/dev/null 2>&1 && have_jq=1

work="$(mktemp -d "${TMPDIR:-/tmp}/screen-time-restore-drill.XXXXXX")"
trap 'rm -rf -- "$work"' EXIT INT TERM

verified=()
while IFS= read -r dir; do
    [ -n "$dir" ] || continue
    [ -f "${dir}/manifest.json" ] || continue
    [ -f "${dir}/db/screen-time.db" ] || continue
    grep -q '"status": *"verified"' "${dir}/manifest.json" && verified+=("$dir")
done < <(find "$BACKUP_ROOT" -mindepth 1 -maxdepth 1 -type d -printf '%T@ %p\n' | sort -n | cut -d' ' -f2-)

[ "${#verified[@]}" -gt 0 ] || die "no verified backups under $BACKUP_ROOT"
if [ "$OLDEST" -eq 1 ]; then
    backup="${verified[0]}"
else
    backup="${verified[${#verified[@]}-1]}"
fi

[ -f "${backup}/db/screen-time.db" ] || die "backup has no database snapshot: $backup"
[ -f "${backup}/db/manifest.json" ] || die "backup has no snapshot manifest: $backup"
expected="$(sed -n 's/.*"sha256": *"\([^"]*\)".*/\1/p' "${backup}/db/manifest.json" | sed -n '1p')"
actual="$(sha256sum "${backup}/db/screen-time.db" | awk '{print $1}')"
[ -n "$expected" ] || die "snapshot manifest has no sha256"
[ "$expected" = "$actual" ] || die "snapshot checksum mismatch: $backup"

state="${work}/state/screen-time"
mkdir -p "$state"
cp -- "${backup}/db/screen-time.db" "${state}/screen-time.db"
chmod 600 "${state}/screen-time.db"
runtime="${work}/run/screen-time"
mkdir -p "$runtime"

export SCREEN_TIME_STATE_DIR="$state"
export SCREEN_TIME_RUNTIME_DIR="$runtime"
export SCREEN_TIME_IGNORE_PID=1

doctor="${work}/doctor.json"
"$BIN" doctor --json --no-runtime >"$doctor" || die "doctor failed on restored snapshot"
if [ "$have_jq" -eq 1 ]; then
    jq -e '.ok == true and .quick_check == "ok"' "$doctor" >/dev/null \
        || die "doctor JSON is not ok"
else
    grep -q '"ok": true' "$doctor" || die "doctor JSON is not ok"
fi

"$BIN" check-data --against "${backup}/db" --json >"${work}/check.json" \
    || die "restored fingerprint does not match the backup manifest"

mode_dir="$(stat -c '%a' "$backup")"
mode_db="$(stat -c '%a' "${backup}/db/screen-time.db")"
mode_manifest="$(stat -c '%a' "${backup}/db/manifest.json")"
[ "$mode_dir" = "700" ] || die "backup directory mode is $mode_dir, expected 700"
[ "$mode_db" = "600" ] || die "snapshot database mode is $mode_db, expected 600"
[ "$mode_manifest" = "600" ] || die "snapshot manifest mode is $mode_manifest, expected 600"

generation="$(sed -n 's/.*"generation": *"\([^"]*\)".*/\1/p' "${backup}/manifest.json" | sed -n '1p')"
schema="$(sed -n 's/.*"schema_version": *\([0-9]*\).*/\1/p' "${backup}/db/manifest.json" | sed -n '1p')"
seconds="$(sed -n 's/.*"session_seconds": *\([0-9]*\).*/\1/p' "${backup}/db/manifest.json" | sed -n '1p')"
printf 'restore drill ok\n'
printf '  generation: %s\n' "${generation:-unknown}"
printf '  backup:     %s\n' "$backup"
printf '  schema:     %s\n' "${schema:-unknown}"
printf '  seconds:    %s\n' "${seconds:-unknown}"
printf '  quick_check: ok\n'
