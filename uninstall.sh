#!/usr/bin/env bash
# Remove the per-user screen-time installation. Data and backups are kept
# unless --purge is given, and --purge always requires confirmation.
set -euo pipefail

PREFIX="${HOME}/.local"
PURGE=0
DRY_RUN=0
NO_SERVICE=0
YES=0

usage() {
    cat <<'EOF'
Usage: uninstall.sh [options]

Remove the per-user screen-time files. State, database, config, and backups
are kept unless --purge is given.

Options:
  --prefix PATH   install prefix used by install.sh (default: ~/.local)
  --purge         also delete ~/.local/state/screen-time (database/history)
  --yes           confirm --purge without an interactive prompt
  --dry-run       print actions without deleting anything
  --no-service    do not call systemctl --user
  -h, --help      show this help
EOF
}

die() {
    printf 'uninstall.sh: %s\n' "$*" >&2
    exit 1
}

info() {
    printf '%s\n' "$*"
}

have() {
    command -v "$1" >/dev/null 2>&1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --prefix)
            [ "$#" -ge 2 ] || die "--prefix requires a value"
            PREFIX="$2"
            shift 2
            ;;
        --purge) PURGE=1; shift ;;
        --yes) YES=1; shift ;;
        --dry-run) DRY_RUN=1; shift ;;
        --no-service) NO_SERVICE=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) die "unknown option: $1 (try --help)" ;;
    esac
done

[ "$(uname -s)" = "Linux" ] || die "screen-time v1 supports Linux only"
[ "${EUID}" -ne 0 ] || die "do not run this as root"
[ -n "${HOME:-}" ] || die "HOME is not set"

# shellcheck disable=SC2088
case "$PREFIX" in
    "~") PREFIX="$HOME" ;;
    "~/"*) PREFIX="${HOME}/${PREFIX:2}" ;;
esac
PREFIX="${PREFIX%/}"

BIN_DIR="${PREFIX}/bin"
BIN="${BIN_DIR}/screen-time"
WRAPPER="${BIN_DIR}/rofi-screen-time"
DATA_HOME="${XDG_DATA_HOME:-${HOME}/.local/share}"
THEME_DIR="${DATA_HOME}/screen-time"
THEME="${THEME_DIR}/rofi/screen-time.rasi"
CONFIG_HOME="${XDG_CONFIG_HOME:-${HOME}/.config}"
UNIT_DIR="${CONFIG_HOME}/systemd/user"
UNIT="${UNIT_DIR}/screen-time.service"
STATE_DIR="${XDG_STATE_HOME:-${HOME}/.local/state}/screen-time"
UPDATE_DIR="${STATE_DIR}/updates"
LOCK_FILE="${UPDATE_DIR}/update.lock"
RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/screen-time"

if [ "$DRY_RUN" -eq 1 ]; then
    info "dry-run: stop/disable screen-time.service and update timers"
    info "dry-run: remove $BIN $WRAPPER $THEME $UNIT"
    info "dry-run: remove update units under $UNIT_DIR"
    info "dry-run: keep $STATE_DIR/backups unless --purge"
    [ "$PURGE" -eq 1 ] && info "dry-run: remove state $STATE_DIR"
    exit 0
fi

if [ "$PURGE" -eq 1 ] && [ "$YES" -ne 1 ]; then
    if [ ! -t 0 ]; then
        die "--purge in non-interactive mode requires --yes"
    fi
    read -r -p "This deletes the database and all backups. Type 'purge' to continue: " answer
    [ "$answer" = "purge" ] || die "purge cancelled"
fi

if have flock && [ -d "$UPDATE_DIR" ]; then
    exec 9>>"$LOCK_FILE"
    flock -n 9 || die "another screen-time update is running (lock: $LOCK_FILE)"
fi

if [ "$NO_SERVICE" -eq 0 ] && have systemctl; then
    systemctl --user stop screen-time.service >/dev/null 2>&1 || true
    systemctl --user disable screen-time.service >/dev/null 2>&1 || true
    systemctl --user disable --now screen-time-update-check.timer >/dev/null 2>&1 || true
    systemctl --user disable --now screen-time-update-apply.timer >/dev/null 2>&1 || true
fi

rm -f -- "$UNIT" \
    "${UNIT_DIR}/screen-time-update-check.service" \
    "${UNIT_DIR}/screen-time-update-check.timer" \
    "${UNIT_DIR}/screen-time-update-apply.service" \
    "${UNIT_DIR}/screen-time-update-apply.timer"
if [ "$NO_SERVICE" -eq 0 ] && have systemctl; then
    systemctl --user daemon-reload >/dev/null 2>&1 || true
fi
rm -f -- "$BIN" "$WRAPPER" "$THEME" "$THEME_DIR/installed-version" \
    "$THEME_DIR/theme.sha256" "$STATE_DIR/theme.sha256" "$STATE_DIR/installed-version" \
    "$STATE_DIR/install.sh"
rm -f -- "${RUNTIME_DIR}/live.json" "${RUNTIME_DIR}/ui.json" "${RUNTIME_DIR}/rofi-theme.rasi"
rmdir "$RUNTIME_DIR" >/dev/null 2>&1 || true
rm -f -- "${UPDATE_DIR}/last-check.json" "${UPDATE_DIR}/last-update.json" \
    "${UPDATE_DIR}/update-in-progress.json" "${UPDATE_DIR}/failure.json" "$LOCK_FILE"
rmdir "$UPDATE_DIR" >/dev/null 2>&1 || true
rmdir "$THEME_DIR/rofi" "$THEME_DIR" "$BIN_DIR" >/dev/null 2>&1 || true

if [ "$PURGE" -eq 1 ]; then
    rm -rf -- "$STATE_DIR"
    info "removed state and history at $STATE_DIR"
else
    info "kept state, database, config, and backups at $STATE_DIR"
fi

rmdir "$UNIT_DIR" >/dev/null 2>&1 || true
info "uninstalled screen-time"
