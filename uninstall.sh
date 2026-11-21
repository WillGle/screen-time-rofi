#!/usr/bin/env bash
set -euo pipefail

PREFIX="${HOME}/.local"
PURGE=0
DRY_RUN=0
NO_SERVICE=0

usage() {
    cat <<'EOF'
Usage: uninstall.sh [options]

Remove the per-user screen-time files. State and history are kept unless
--purge is given.

Options:
  --prefix PATH   install prefix used by install.sh (default: ~/.local)
  --purge         also delete ~/.local/state/screen-time (database and history)
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

while [ "$#" -gt 0 ]; do
    case "$1" in
        --prefix)
            [ "$#" -ge 2 ] || die "--prefix requires a value"
            PREFIX="$2"
            shift 2
            ;;
        --purge) PURGE=1; shift ;;
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
UNIT="${CONFIG_HOME}/systemd/user/screen-time.service"
STATE_DIR="${XDG_STATE_HOME:-${HOME}/.local/state}/screen-time"
RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/screen-time"

if [ "$DRY_RUN" -eq 1 ]; then
    info "dry-run: stop/disable screen-time.service"
    info "dry-run: remove $BIN $WRAPPER $THEME $UNIT"
    info "dry-run: remove update backups under $STATE_DIR/backups"
    [ "$PURGE" -eq 1 ] && info "dry-run: remove state $STATE_DIR"
    exit 0
fi

if [ "$NO_SERVICE" -eq 0 ] && command -v systemctl >/dev/null 2>&1; then
    systemctl --user stop screen-time.service >/dev/null 2>&1 || true
    systemctl --user disable screen-time.service >/dev/null 2>&1 || true
fi

rm -f -- "$UNIT"
if [ "$NO_SERVICE" -eq 0 ] && command -v systemctl >/dev/null 2>&1; then
    systemctl --user daemon-reload >/dev/null 2>&1 || true
fi
rm -f -- "$BIN" "$WRAPPER" "$THEME" "$THEME_DIR/installed-version" \
    "$THEME_DIR/theme.sha256" "$STATE_DIR/theme.sha256" "$STATE_DIR/installed-version"
rm -f -- "${RUNTIME_DIR}/live.json" "${RUNTIME_DIR}/ui.json"
rmdir "$RUNTIME_DIR" >/dev/null 2>&1 || true
rm -rf -- "$STATE_DIR/backups"
rmdir "$THEME_DIR/rofi" "$THEME_DIR" "$BIN_DIR" >/dev/null 2>&1 || true
rmdir "$STATE_DIR" >/dev/null 2>&1 || true

if [ "$PURGE" -eq 1 ]; then
    rm -rf -- "$STATE_DIR"
    info "removed state and history at $STATE_DIR"
else
    info "kept state and history at $STATE_DIR (use --purge to remove it)"
fi

info "uninstalled screen-time"
