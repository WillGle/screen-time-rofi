#!/usr/bin/env bash
# Screen Time standalone installer and opt-in updater.
#
# The updater is deliberately conservative: it never writes the live database
# directly, it snapshots before replacing anything, it verifies the release,
# and it restores the previous files and service state on any failure.
set -Eeuo pipefail

REPO="WillGle/screen-time-rofi"
PROG="screen-time"
NIXOS_FLAKE=""
VERSION=""
PREFIX="${HOME}/.local"
NO_SERVICE=0
NO_THEME=0
REQUIRE_HYPRLAND=0
FORCE=0
DRY_RUN=0
CLEAN=0
KEEP_BACKUPS=2
ROLLBACK=0
UNINSTALL=0
PURGE=0
YES=0
CHECK_ONLY=0
UPDATE_ONLY=0
JSON=0
WRITE_CHECK_STATE=0
AUTO_UPDATE_MODE=""
NO_AUTO_UPDATE=0
CHANNEL="stable"
ALLOW_UNSIGNED=0
ALLOW_DOWNGRADE=0
ALLOW_SCHEMA_DOWNGRADE=0
ALLOW_MAJOR=0
AUTO=0
TARBALL=""
SHA256=""
ORIGINAL_ARGS=("$@")

# Pinned release signing key(s). The private key is never stored in this
# repository. Multiple entries provide an overlap window during rotation.
TRUSTED_KEYS=(
    "4E5BF83E07DA94ED:RWTtlNoHPvhbTl2HI9jlbVycB8SXCWHtmmXLN8WDApfKOMd/cA0+qnOi"
)

# Test hooks: point the installer at an offline release fixture.
RELEASES_BASE="${SCREEN_TIME_RELEASES_BASE:-https://github.com/${REPO}/releases/download}"
LATEST_TAG_OVERRIDE="${SCREEN_TIME_LATEST_TAG:-}"

usage() {
    cat <<'EOF'
Usage: install.sh [options]

Install, check, update, or remove screen-time for the current user. No root.

Install:
  --version VERSION          install a specific stable release tag
  --prefix PATH              install prefix (default: ~/.local)
  --no-service               install files but not the user unit
  --no-theme                 do not install the Rofi theme
  --require-hyprland         fail when Hyprland is not detected
  --force                    replace a user-modified theme
  --dry-run                  print actions without changing files
  --keep-backups N           verified update backups to retain (default: 2)
  --tarball PATH             install a local release tarball (offline/testing)
  --sha256 HEX               checksum for --tarball
  --allow-unsigned           skip signature checks (manual local testing only)

Updates:
  --check                    report installed/latest versions; no install
  --json                     machine-readable output for --check
  --write-check-state        write updates/last-check.json (auto-check unit)
  --update                   install the latest stable release if newer
  --channel stable           release channel (only stable is supported)
  --allow-downgrade          permit --version older than the installed one
  --allow-schema-downgrade   permit a target binary older than the live schema
  --allow-major              allow auto-apply across a major version

Auto-update (opt-in):
  --auto-update=check        install daily check timer only (default mode)
  --auto-update=apply        install signed auto-apply units
  --no-auto-update           remove check/apply units and update state
  --auto                     internal: unattended unit run; never interactive

Recovery:
  --rollback                 restore the latest verified update backup
  --clean                    remove stale staging and old verified backups
  --uninstall                remove installed files; keep state and backups
  --purge                    with --uninstall, delete state/history too
  --yes                      confirm --purge without an interactive prompt
  -h, --help                 show this help

Check exit codes: 0 up to date, 10 update available, 20 network error,
21 invalid release, 22 signature error.
EOF
}

die() {
    printf 'install.sh: %s\n' "$*" >&2
    if [ "${FAILURE_TRAP_ARMED:-0}" -eq 1 ]; then
        fail_recovery "$*"
    elif [ "${SERVICE_STOPPED:-0}" -eq 1 ]; then
        FIRST_INSTALL=0
        restore_service_state
    fi
    exit 1
}

info() {
    printf '%s\n' "$*"
}

warn() {
    printf 'install.sh: warning: %s\n' "$*" >&2
}

have() {
    command -v "$1" >/dev/null 2>&1
}

json_escape() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

json_optional_string() {
    if [ -n "$1" ]; then
        printf '"%s"' "$(json_escape "$1")"
    else
        printf 'null'
    fi
}

inject_failure() {
    local stage="$1"
    if [ "${SCREEN_TIME_INSTALLER_FAIL_AFTER:-}" = "$stage" ]; then
        ERROR_PHASE="fault-$stage"
        die "injected failure after $stage"
    fi
    if [ "${SCREEN_TIME_INSTALLER_KILL_AFTER:-}" = "$stage" ]; then
        kill -KILL "$$"
    fi
}

notify() {
    local title="$1"
    local body="$2"
    if have notify-send; then
        notify-send --app-name=screen-time "$title" "$body" >/dev/null 2>&1 || true
    fi
    printf '%s: %s\n' "$title" "$body" >&2
}

expand_path() {
    # shellcheck disable=SC2088
    case "$1" in
        "~") printf '%s' "$HOME" ;;
        "~/"*) printf '%s/%s' "$HOME" "${1:2}" ;;
        *) printf '%s' "$1" ;;
    esac
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --nixos)
            [ "$#" -ge 2 ] && [[ "$2" == ?*#?* ]] || die "--nixos requires FLAKE#HOST"
            NIXOS_FLAKE="$2"
            shift 2
            ;;
        --version)
            [ "$#" -ge 2 ] || die "--version requires a value"
            VERSION="$2"
            shift 2
            ;;
        --check) CHECK_ONLY=1; shift ;;
        --update) UPDATE_ONLY=1; shift ;;
        --json) JSON=1; shift ;;
        --write-check-state) WRITE_CHECK_STATE=1; shift ;;
        --auto-update)
            [ "$#" -ge 2 ] || die "--auto-update requires check or apply"
            AUTO_UPDATE_MODE="$2"
            shift 2
            ;;
        --auto-update=*)
            AUTO_UPDATE_MODE="${1#*=}"
            shift
            ;;
        --no-auto-update) NO_AUTO_UPDATE=1; shift ;;
        --channel)
            [ "$#" -ge 2 ] || die "--channel requires a value"
            CHANNEL="$2"
            shift 2
            ;;
        --prefix)
            [ "$#" -ge 2 ] || die "--prefix requires a value"
            PREFIX="$2"
            shift 2
            ;;
        --no-service) NO_SERVICE=1; shift ;;
        --no-theme) NO_THEME=1; shift ;;
        --require-hyprland) REQUIRE_HYPRLAND=1; shift ;;
        --force) FORCE=1; shift ;;
        --dry-run) DRY_RUN=1; shift ;;
        --clean) CLEAN=1; shift ;;
        --keep-backups)
            [ "$#" -ge 2 ] || die "--keep-backups requires a number"
            KEEP_BACKUPS="$2"
            shift 2
            ;;
        --rollback) ROLLBACK=1; shift ;;
        --uninstall) UNINSTALL=1; shift ;;
        --purge) PURGE=1; shift ;;
        --yes) YES=1; shift ;;
        --allow-unsigned) ALLOW_UNSIGNED=1; shift ;;
        --allow-downgrade) ALLOW_DOWNGRADE=1; shift ;;
        --allow-schema-downgrade) ALLOW_SCHEMA_DOWNGRADE=1; shift ;;
        --allow-major) ALLOW_MAJOR=1; shift ;;
        --auto) AUTO=1; shift ;;
        --tarball)
            [ "$#" -ge 2 ] || die "--tarball requires a path"
            TARBALL="$2"
            shift 2
            ;;
        --sha256)
            [ "$#" -ge 2 ] || die "--sha256 requires a value"
            SHA256="$2"
            shift 2
            ;;
        -h|--help) usage; exit 0 ;;
        *) die "unknown option: $1 (try --help)" ;;
    esac
done

if [ "${SCREEN_TIME_AUTO_UPDATE:-0}" = "1" ]; then
    AUTO=1
fi

[ "$(uname -s)" = "Linux" ] || die "screen-time v1 supports Linux only"
[ "${EUID}" -ne 0 ] || die "do not run this as root; screen-time is a per-user install"
[ -n "${HOME:-}" ] || die "HOME is not set"
[ "$CHANNEL" = "stable" ] || die "only --channel stable is supported"

case "$KEEP_BACKUPS" in
    ''|*[!0-9]*) die "--keep-backups must be a non-negative integer" ;;
esac
[ "$KEEP_BACKUPS" -ge 2 ] || die "--keep-backups must be at least 2"

if [ "$PURGE" -eq 1 ] && [ "$UNINSTALL" -ne 1 ]; then
    die "--purge requires --uninstall"
fi
if [ "$CHECK_ONLY" -eq 1 ] && [ "$UPDATE_ONLY" -eq 1 ]; then
    die "--check and --update are separate modes"
fi
if [ "$CHECK_ONLY" -eq 1 ] && { [ "$UNINSTALL" -eq 1 ] || [ "$ROLLBACK" -eq 1 ] || [ "$CLEAN" -eq 1 ] || [ -n "$AUTO_UPDATE_MODE" ]; }; then
    die "--check cannot be combined with install, rollback, clean, or auto-update modes"
fi
if [ "$UPDATE_ONLY" -eq 1 ] && { [ "$UNINSTALL" -eq 1 ] || [ "$ROLLBACK" -eq 1 ] || [ "$CLEAN" -eq 1 ]; }; then
    die "--update cannot be combined with uninstall, rollback, or clean"
fi
if [ -n "$AUTO_UPDATE_MODE" ] && [ "$AUTO_UPDATE_MODE" != "check" ] && [ "$AUTO_UPDATE_MODE" != "apply" ]; then
    die "--auto-update must be check or apply"
fi
if [ "$WRITE_CHECK_STATE" -eq 1 ] && [ "$CHECK_ONLY" -ne 1 ]; then
    die "--write-check-state requires --check"
fi
if [ "$JSON" -eq 1 ] && [ "$CHECK_ONLY" -ne 1 ] && [ "$UPDATE_ONLY" -eq 0 ] && [ "$AUTO_UPDATE_MODE" = "" ] && [ "$NO_AUTO_UPDATE" -eq 0 ]; then
    die "--json is only supported with --check or --update"
fi
if [ "$AUTO" -eq 1 ] && [ "$FORCE" -eq 1 ]; then
    die "automatic updates never pass --force"
fi
if [ "$AUTO" -eq 1 ] && [ "$ALLOW_UNSIGNED" -eq 1 ]; then
    die "automatic updates never accept unsigned releases"
fi
if [ "$AUTO" -eq 1 ] && [ "$UNINSTALL" -eq 1 ]; then
    die "automatic updates never uninstall"
fi
if [ "$ALLOW_UNSIGNED" -eq 1 ] && [ "$AUTO" -ne 1 ]; then
    warn "--allow-unsigned disables release verification; use it only for local testing"
fi

PREFIX="$(expand_path "$PREFIX")"
PREFIX="${PREFIX%/}"
BIN_DIR="${PREFIX}/bin"
BIN="${BIN_DIR}/${PROG}"
WRAPPER="${BIN_DIR}/rofi-screen-time"
DATA_HOME="${XDG_DATA_HOME:-${HOME}/.local/share}"
THEME_DIR="${DATA_HOME}/screen-time"
THEME="${THEME_DIR}/rofi/screen-time.rasi"
EXAMPLE_CONFIG="${THEME_DIR}/config.example.toml"
CONFIG_HOME="${XDG_CONFIG_HOME:-${HOME}/.config}"
CONFIG="${CONFIG_HOME}/screen-time/config.toml"
UNIT_DIR="${CONFIG_HOME}/systemd/user"
UNIT="${UNIT_DIR}/screen-time.service"
UPDATE_CHECK_SERVICE="${UNIT_DIR}/screen-time-update-check.service"
UPDATE_CHECK_TIMER="${UNIT_DIR}/screen-time-update-check.timer"
UPDATE_APPLY_SERVICE="${UNIT_DIR}/screen-time-update-apply.service"
UPDATE_APPLY_TIMER="${UNIT_DIR}/screen-time-update-apply.timer"
STATE_DIR="${XDG_STATE_HOME:-${HOME}/.local/state}/screen-time"
BACKUP_ROOT="${STATE_DIR}/backups/updates"
UNIT_BACKUP_ROOT="${STATE_DIR}/backups/units"
UPDATE_DIR="${STATE_DIR}/updates"
LOCK_FILE="${UPDATE_DIR}/update.lock"
LAST_CHECK="${UPDATE_DIR}/last-check.json"
LAST_UPDATE="${UPDATE_DIR}/last-update.json"
IN_PROGRESS="${UPDATE_DIR}/update-in-progress.json"
FAILURE_STATE="${UPDATE_DIR}/failure.json"
INSTALL_COPY="${STATE_DIR}/install.sh"
RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/screen-time"
DB="${STATE_DIR}/screen-time.db"
THEME_SHA_FILE="${STATE_DIR}/theme.sha256"
VERSION_FILE="${STATE_DIR}/installed-version"
TMP_ROOT="${TMPDIR:-/tmp}"
STAGE=""
BACKUP=""
INSTALL_TEMPS=()
WAS_ACTIVE=0
SERVICE_STOPPED=0
SNAPSHOT_DONE=0
FAILURE_TRAP_ARMED=0
FIRST_INSTALL=0
DB_EXISTED=0
CONFIG_HASH_BEFORE=""
CONFIG_HASH_AFTER=""
SIGNATURE_KEY_ID=""
ASSET_SHA=""
INSTALLER_SHA=""
NEW_VERSION=""
OLD_VERSION=""
TAG=""
BASE_URL=""
GENERATION=""
ERROR_PHASE=""

umask 077

cleanup() {
    local file
    for file in "${INSTALL_TEMPS[@]:-}"; do
        [ -n "$file" ] && rm -f -- "$file"
    done
    if [ -n "$STAGE" ] && [ -d "$STAGE" ]; then
        rm -rf -- "$STAGE"
    fi
}
trap cleanup EXIT INT TERM

on_error() {
    local status="$1"
    local line="$2"
    trap - ERR
    set +e
    ERROR_PHASE="${ERROR_PHASE:-command-failed}"
    if [ "${FAILURE_TRAP_ARMED:-0}" -eq 1 ]; then
        fail_recovery "command failed at line $line (exit $status)"
    elif [ "${SERVICE_STOPPED:-0}" -eq 1 ]; then
        FIRST_INSTALL=0
        restore_service_state
    fi
    exit "$status"
}

trap 'on_error $? $LINENO' ERR

have_flock() {
    have flock
}

acquire_lock() {
    have_flock || die "flock is required for safe updates"
    mkdir -p "$UPDATE_DIR"
    chmod 700 "$UPDATE_DIR"
    exec 9>>"$LOCK_FILE"
    chmod 600 "$LOCK_FILE" 2>/dev/null || true
    if ! flock -n 9; then
        if [ "$AUTO" -eq 1 ]; then
            info "another screen-time update holds the lock; skipping"
            exit 0
        fi
        die "another screen-time install/update is running (lock: $LOCK_FILE)"
    fi
}

atomic_install() {
    local src="$1"
    local dest="$2"
    local mode="$3"
    local dir temp
    dir="$(dirname "$dest")"
    mkdir -p "$dir"
    temp="$(mktemp "${dir}/.screen-time-install.XXXXXX")"
    INSTALL_TEMPS+=("$temp")
    install -m "$mode" "$src" "$temp"
    sync "$temp" 2>/dev/null || sync
    mv -f -- "$temp" "$dest"
    sync "$dest" 2>/dev/null || sync
}

atomic_write_text() {
    local dest="$1"
    local mode="$2"
    local text="$3"
    local dir temp
    dir="$(dirname "$dest")"
    mkdir -p "$dir"
    temp="$(mktemp "${dir}/.screen-time-install.XXXXXX")"
    INSTALL_TEMPS+=("$temp")
    printf '%s\n' "$text" > "$temp"
    chmod "$mode" "$temp"
    sync "$temp" 2>/dev/null || sync
    mv -f -- "$temp" "$dest"
    sync "$dest" 2>/dev/null || sync
}

service_active() {
    [ "$NO_SERVICE" -eq 0 ] && have systemctl || return 1
    [ "$(systemctl --user is-active screen-time.service 2>/dev/null || true)" = "active" ]
}

stop_service() {
    if service_active; then
        systemctl --user stop screen-time.service >/dev/null 2>&1 || true
        SERVICE_STOPPED=1
    fi
}

restore_service_state() {
    [ "$NO_SERVICE" -eq 0 ] && have systemctl || return 0
    systemctl --user daemon-reload >/dev/null 2>&1 || true
    if [ "$WAS_ACTIVE" -eq 1 ] || { [ "$FIRST_INSTALL" -eq 1 ] && [ "$WAS_ACTIVE" -eq 0 ]; }; then
        systemctl --user enable --now screen-time.service >/dev/null 2>&1 || true
    elif [ "$SERVICE_STOPPED" -eq 1 ]; then
        systemctl --user stop screen-time.service >/dev/null 2>&1 || true
    fi
}

theme_is_user_modified() {
    [ -f "$THEME" ] || return 1
    [ -f "$THEME_SHA_FILE" ] || return 1
    local recorded current
    recorded="$(cat "$THEME_SHA_FILE" 2>/dev/null || true)"
    current="$(sha256sum "$THEME" | awk '{print $1}')"
    [ -n "$recorded" ] && [ "$recorded" != "$current" ]
}

unit_has_custom_execstart() {
    [ -f "$UNIT" ] || return 1
    local expected actual
    expected="ExecStart=${BIN} daemon"
    actual="$(sed -n 's/^ExecStart=//p' "$UNIT" | head -n1)"
    [ "$actual" != "${BIN} daemon" ] && [ "$actual" != "%h/.local/bin/screen-time daemon" ]
}

backup_user_unit_edit() {
    [ -f "$UNIT" ] || return 0
    mkdir -p "$UNIT_BACKUP_ROOT"
    chmod 700 "$UNIT_BACKUP_ROOT"
    local dest
    dest="${UNIT_BACKUP_ROOT}/screen-time.service.$(date +%Y%m%d-%H%M%S)-$$"
    cp -a -- "$UNIT" "$dest"
    chmod 600 "$dest"
    warn "saved the user-modified unit to $dest"
}

refuse_nix_managed_path() {
    local path="$1"
    [ -L "$path" ] || return 0
    local target
    target="$(readlink -f -- "$path" 2>/dev/null || true)"
    # On a non-Nix host /nix/store may not exist, so readlink -f can fail.
    # Fall back to the raw link target and still refuse it.
    [ -n "$target" ] || target="$(readlink -- "$path" 2>/dev/null || true)"
    case "$target" in
        /nix/store/*)
            die "$path is managed by Nix/Home Manager. Update the flake input instead of using install.sh."
            ;;
    esac
}

require_base_tools() {
    have tar || die "tar is required"
    have sha256sum || die "sha256sum is required"
    have flock || die "flock is required"
    if ! have curl && ! have wget; then
        die "curl or wget is required"
    fi
}

require_runtime_tools() {
    if [ "$NO_SERVICE" -eq 0 ]; then
        have systemctl || die "systemctl is required (or use --no-service)"
        systemctl --user show-environment >/dev/null 2>&1 \
            || die "the systemd user manager is unavailable (or use --no-service)"
    fi
    have rofi || die "rofi is required for the screen-time UI; install rofi first"
    if [ -z "${HYPRLAND_INSTANCE_SIGNATURE:-}" ] && ! hyprctl version >/dev/null 2>&1; then
        if [ "$REQUIRE_HYPRLAND" -eq 1 ]; then
            die "Hyprland was not detected and --require-hyprland was set"
        fi
        warn "Hyprland was not detected; tracking will only work inside a Hyprland session"
    fi
}

install_dependency_hint() {
    if ! have minisign && [ "$ALLOW_UNSIGNED" -eq 0 ]; then
        die "minisign is required to verify releases; install it or use --allow-unsigned for local testing"
    fi
}

fetch() {
    local url="$1"
    local dest="$2"
    if [ -n "${SCREEN_TIME_FETCH_FAIL_RC:-}" ]; then
        # Test hook for timeout/HTTP/rate-limit classification.
        return "$SCREEN_TIME_FETCH_FAIL_RC"
    fi
    if have curl; then
        curl -fsSL --retry 2 --retry-delay 1 --connect-timeout 15 --max-time 120 \
            -A "screen-time-installer" -o "$dest" "$url"
    else
        wget -q -T 120 -t 3 -U "screen-time-installer" -O "$dest" "$url"
    fi
}

resolve_latest_tag() {
    if [ -n "$LATEST_TAG_OVERRIDE" ]; then
        printf '%s' "$LATEST_TAG_OVERRIDE"
        return 0
    fi
    local api="https://api.github.com/repos/${REPO}/releases/latest"
    local effective="" tag=""
    if have curl; then
        tag="$(curl -fsSL --connect-timeout 15 --max-time 30 \
            -H 'Accept: application/vnd.github+json' \
            -A "screen-time-installer" "$api" 2>/dev/null \
            | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n1 || true)"
        if [ -z "$tag" ]; then
            effective="$(curl -fsSLI -o /dev/null -w '%{url_effective}' \
                "https://github.com/${REPO}/releases/latest" 2>/dev/null || true)"
            tag="${effective##*/}"
        fi
    else
        tag="$(wget -qS --spider "https://github.com/${REPO}/releases/latest" 2>&1 \
            | awk '/^  Location:/ {print $2; exit}' || true)"
        tag="${tag##*/}"
    fi
    case "$tag" in
        ""|releases|latest) die "cannot resolve the latest GitHub release" ;;
    esac
    printf '%s' "$tag"
}

is_stable_tag() {
    [[ "$1" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]
}

version_gt() {
    local left="${1#v}" right="${2#v}"
    [ "$left" != "$right" ] || return 1
    [ "$(printf '%s\n%s\n' "$left" "$right" | sort -V | tail -n1)" = "$left" ]
}

major_of() {
    local value="${1#v}"
    printf '%s' "${value%%.*}"
}

installed_version() {
    if [ -x "$BIN" ]; then
        "$BIN" --version 2>/dev/null | awk '{print $2}' | head -n1
    elif [ -f "$VERSION_FILE" ]; then
        head -n1 "$VERSION_FILE"
    fi
}

trim_v() {
    printf '%s' "${1#v}"
}

trusted_keys() {
    if [ -n "${SCREEN_TIME_TRUSTED_KEYS:-}" ]; then
        printf '%s\n' "$SCREEN_TIME_TRUSTED_KEYS" | tr ';' '\n'
    else
        printf '%s\n' "${TRUSTED_KEYS[@]}"
    fi
}

verify_signature() {
    local file="$1"
    local signature="$2"
    [ -f "$file" ] || die "missing file to verify: $file"
    [ -f "$signature" ] || die "missing signature: $signature"
    if [ "$ALLOW_UNSIGNED" -eq 1 ]; then
        warn "skipping signature verification for $file (--allow-unsigned)"
        SIGNATURE_KEY_ID="unsigned"
        return 0
    fi
    install_dependency_hint
    local entry key_id key
    while IFS= read -r entry; do
        [ -n "$entry" ] || continue
        key_id="${entry%%:*}"
        key="${entry#*:}"
        if minisign -Vm "$file" -x "$signature" -P "$key" >/dev/null 2>&1; then
            SIGNATURE_KEY_ID="$key_id"
            return 0
        fi
    done < <(trusted_keys)
    die "signature verification failed for $file"
}

verify_checksum_line() {
    local sums="$1"
    local name="$2"
    local file="$3"
    local expected
    expected="$(awk -v name="$name" '$2 == name {print $1; exit}' "$sums")"
    [ -n "$expected" ] || die "checksum for $name is missing from SHA256SUMS"
    printf '%s  %s\n' "$expected" "$file" | sha256sum -c - >/dev/null
    printf '%s' "$expected"
}

prepare_release() {
    if [ -n "$TARBALL" ]; then
        [ -f "$TARBALL" ] || die "tarball not found: $TARBALL"
        STAGE="$(mktemp -d "${TMP_ROOT}/screen-time-install.XXXXXX")"
        cp -- "$TARBALL" "${STAGE}/screen-time.tar.gz"
        archive="${STAGE}/screen-time.tar.gz"
        if [ -f "$(dirname "$TARBALL")/SHA256SUMS" ]; then
            expected="$(awk -v name="$(basename "$TARBALL")" '$2 == name {print $1; exit}' "$(dirname "$TARBALL")/SHA256SUMS")"
            [ -n "$expected" ] || die "no checksum for $(basename "$TARBALL") in SHA256SUMS"
            printf '%s  %s\n' "$expected" "$archive" | sha256sum -c - >/dev/null
            ASSET_SHA="$expected"
        elif [ -n "$SHA256" ]; then
            printf '%s  %s\n' "$SHA256" "$archive" | sha256sum -c - >/dev/null
            ASSET_SHA="$SHA256"
        else
            die "local tarball install requires --sha256 or an adjacent SHA256SUMS"
        fi
        if [ -f "$(dirname "$TARBALL")/SHA256SUMS.minisig" ]; then
            verify_signature "$(dirname "$TARBALL")/SHA256SUMS" "$(dirname "$TARBALL")/SHA256SUMS.minisig"
        elif [ "$ALLOW_UNSIGNED" -ne 1 ]; then
            die "local tarball is unsigned; pass --allow-unsigned for offline testing"
        else
            SIGNATURE_KEY_ID="unsigned"
        fi
        TAG="local"
        return 0
    fi

    case "$(uname -m)" in
        x86_64|amd64) ARCH="x86_64" ;;
        aarch64|arm64) ARCH="aarch64" ;;
        *) die "unsupported architecture: $(uname -m)" ;;
    esac
    if [ -z "$VERSION" ]; then
        VERSION="$(resolve_latest_tag)" || die "cannot resolve the latest GitHub release"
    fi
    case "$VERSION" in
        v*) TAG="$VERSION" ;;
        *) TAG="v$VERSION" ;;
    esac
    is_stable_tag "$TAG" || die "release $TAG is not a stable vX.Y.Z tag"
    ASSET="screen-time-${ARCH}-linux.tar.gz"
    BASE_URL="${RELEASES_BASE}/${TAG}"
    STAGE="$(mktemp -d "${TMP_ROOT}/screen-time-install.XXXXXX")"
    info "downloading $ASSET ($TAG)"
    fetch "${BASE_URL}/${ASSET}" "${STAGE}/${ASSET}" || die "cannot download $ASSET"
    fetch "${BASE_URL}/SHA256SUMS" "${STAGE}/SHA256SUMS" || die "cannot download SHA256SUMS"
    archive="${STAGE}/${ASSET}"
    ASSET_SHA="$(verify_checksum_line "${STAGE}/SHA256SUMS" "$ASSET" "$archive")"

    if [ "$ALLOW_UNSIGNED" -eq 1 ]; then
        SIGNATURE_KEY_ID="unsigned"
        if fetch "${BASE_URL}/install.sh" "${STAGE}/install.sh.release" 2>/dev/null; then
            INSTALLER_SHA="$(sha256sum "${STAGE}/install.sh.release" | awk '{print $1}')"
        fi
        return 0
    fi

    fetch "${BASE_URL}/SHA256SUMS.minisig" "${STAGE}/SHA256SUMS.minisig" \
        || die "cannot download SHA256SUMS.minisig"
    fetch "${BASE_URL}/install.sh" "${STAGE}/install.sh.release" || die "cannot download install.sh"
    fetch "${BASE_URL}/install.sh.minisig" "${STAGE}/install.sh.minisig" \
        || die "cannot download install.sh.minisig"
    verify_signature "${STAGE}/SHA256SUMS" "${STAGE}/SHA256SUMS.minisig"
    verify_signature "${STAGE}/install.sh.release" "${STAGE}/install.sh.minisig"
    INSTALLER_SHA="$(verify_checksum_line "${STAGE}/SHA256SUMS" "install.sh" "${STAGE}/install.sh.release")"

    if [ "${SCREEN_TIME_REEXECUTED:-0}" != "1" ]; then
        local self_hash
        self_hash="$(sha256sum "${BASH_SOURCE[0]}" | awk '{print $1}')"
        if [ "$self_hash" != "$INSTALLER_SHA" ]; then
            info "running installer differs from the signed release; re-executing the verified copy"
            set +e
            SCREEN_TIME_REEXECUTED=1 bash "${STAGE}/install.sh.release" "${ORIGINAL_ARGS[@]}"
            local reexec_status=$?
            set -e
            exit "$reexec_status"
        fi
        verify_signature "${BASH_SOURCE[0]}" "${STAGE}/install.sh.minisig"
    fi
}

extract_release() {
    mkdir -p "${STAGE}/extract"
    tar -xzf "$archive" -C "${STAGE}/extract"
    for required in bin/screen-time bin/rofi-screen-time systemd/screen-time.service; do
        [ -f "${STAGE}/extract/${required}" ] || die "release is missing $required"
    done
    if [ "$NO_THEME" -eq 0 ]; then
        [ -f "${STAGE}/extract/share/rofi/screen-time.rasi" ] || die "release is missing share/rofi/screen-time.rasi"
    fi
    STAGED_BIN="${STAGE}/extract/bin/screen-time"
}

check_disk_space() {
    local probe="$STATE_DIR"
    [ -d "$probe" ] || probe="$HOME"
    local available_kb db_bytes wal_bytes release_bytes need_kb
    available_kb="$(df -Pk "$probe" | awk 'NR==2 {print $4}')"
    db_bytes="$(stat -c %s "$DB" 2>/dev/null || printf '0')"
    wal_bytes="$(stat -c %s "${DB}-wal" 2>/dev/null || printf '0')"
    release_bytes="$(stat -c %s "$archive" 2>/dev/null || printf '0')"
    # Live database + WAL + two verified backups + release + staging headroom.
    need_kb=$(( (db_bytes + wal_bytes) * 3 / 1024 + release_bytes / 1024 + 10240 ))
    if [ "$available_kb" -lt "$need_kb" ]; then
        die "not enough free space: need about ${need_kb} KiB, have ${available_kb} KiB"
    fi
}

write_in_progress() {
    local phase="$1"
    mkdir -p "$UPDATE_DIR"
    chmod 700 "$UPDATE_DIR"
    local temp="${UPDATE_DIR}/.update-in-progress.$$"
    cat >"$temp" <<EOF
{
  "version": 1,
  "phase": "$phase",
  "timestamp": "$(date -Is)",
  "previous_version": "$(json_escape "$OLD_VERSION")",
  "target_version": "$(json_escape "$NEW_VERSION")",
  "release_tag": "$(json_escape "$TAG")",
  "backup": "$(json_escape "$BACKUP")",
  "service_was_active": $([ "$WAS_ACTIVE" -eq 1 ] && printf true || printf false)
}
EOF
    chmod 600 "$temp"
    sync "$temp" 2>/dev/null || sync
    mv -f -- "$temp" "$IN_PROGRESS"
    sync "$UPDATE_DIR" 2>/dev/null || sync
}

write_last_update() {
    local status="$1"
    local phase="${2:-}"
    local error
    error="$(json_escape "${3:-}")"
    mkdir -p "$UPDATE_DIR"
    local temp="${UPDATE_DIR}/.last-update.$$"
    cat >"$temp" <<EOF
{
  "version": 1,
  "status": "$status",
  "phase": "$phase",
  "error": "$error",
  "timestamp": "$(date -Is)",
  "previous_version": "${OLD_VERSION}",
  "current_version": "${NEW_VERSION}",
  "release_tag": "${TAG}",
  "backup": "${BACKUP}",
  "signature_key_id": "${SIGNATURE_KEY_ID}",
  "service_was_active": $([ "$WAS_ACTIVE" -eq 1 ] && printf true || printf false)
}
EOF
    chmod 600 "$temp"
    sync "$temp" 2>/dev/null || sync
    mv -f -- "$temp" "$LAST_UPDATE"
    sync "$UPDATE_DIR" 2>/dev/null || sync
}

write_backup_manifest() {
    local status="$1"
    [ -d "$BACKUP" ] || return 0
    local temp="${BACKUP}/.manifest.$$"
    {
        printf '{\n'
        printf '  "version": 1,\n'
        printf '  "generation": "%s",\n' "$GENERATION"
        printf '  "timestamp": "%s",\n' "$(date -Is)"
        printf '  "previous_version": "%s",\n' "$OLD_VERSION"
        printf '  "current_version": "%s",\n' "$NEW_VERSION"
        printf '  "release_tag": "%s",\n' "$TAG"
        printf '  "asset_sha256": "%s",\n' "$ASSET_SHA"
        printf '  "installer_sha256": "%s",\n' "$INSTALLER_SHA"
        printf '  "signature_key_id": "%s",\n' "$SIGNATURE_KEY_ID"
        printf '  "service_was_active": %s,\n' "$([ "$WAS_ACTIVE" -eq 1 ] && printf true || printf false)"
        printf '  "status": "%s",\n' "$status"
        if [ -f "${BACKUP}/db/manifest.json" ]; then
            printf '  "snapshot_manifest": "db/manifest.json",\n'
            printf '  "snapshot": '
            cat "${BACKUP}/db/manifest.json"
            printf ',\n'
        else
            printf '  "snapshot_manifest": null,\n'
        fi
        printf '  "files": ['
        local first=1 file name
        for file in "$BACKUP"/bin-screen-time "$BACKUP"/bin-rofi-screen-time "$BACKUP"/theme.rasi "$BACKUP"/unit.service; do
            [ -f "$file" ] || continue
            name="$(basename "$file")"
            if [ "$first" -eq 1 ]; then first=0; else printf ', '; fi
            printf '{"name": "%s", "sha256": "%s"}' "$name" "$(sha256sum "$file" | awk '{print $1}')"
        done
        printf ']\n'
        printf '}\n'
    } >"$temp"
    chmod 600 "$temp"
    sync "$temp" 2>/dev/null || sync
    mv -f -- "$temp" "${BACKUP}/manifest.json"
    sync "$BACKUP" 2>/dev/null || sync
}

create_backup() {
    GENERATION="$(date +%Y%m%d-%H%M%S)-$$"
    BACKUP="${BACKUP_ROOT}/${GENERATION}"
    mkdir -p "$BACKUP" || die "cannot create backup directory $BACKUP"
    chmod 700 "$BACKUP_ROOT" "$BACKUP" || die "cannot set permissions on $BACKUP"
    OLD_VERSION="$(installed_version || true)"
    OLD_VERSION="${OLD_VERSION:-none}"
    printf '%s\n' "$OLD_VERSION" > "${BACKUP}/VERSION"
    chmod 600 "${BACKUP}/VERSION"
    backup_file "$BIN" "bin-screen-time"
    backup_file "$WRAPPER" "bin-rofi-screen-time"
    backup_file "$THEME" "theme.rasi"
    backup_file "$UNIT" "unit.service"
    if [ -f "$DB" ]; then
        DB_EXISTED=1
        SCREEN_TIME_STATE_DIR="$STATE_DIR" "$STAGED_BIN" checkpoint >/dev/null 2>&1 \
            || warn "WAL checkpoint failed; continuing with a consistent VACUUM INTO snapshot"
        SCREEN_TIME_STATE_DIR="$STATE_DIR" "$STAGED_BIN" snapshot --to "${BACKUP}/db" --json >/dev/null \
            || die "cannot create a database snapshot; the live database is unchanged"
    fi
    write_backup_manifest "pending"
    sync
    info "backup: $BACKUP"
}

backup_file() {
    local src="$1"
    local name="$2"
    [ -e "$src" ] || return 0
    cp -a -- "$src" "${BACKUP}/${name}"
    chmod 600 "${BACKUP}/${name}"
}

restore_backup_files() {
    local backup="$1"
    [ -d "$backup" ] || return 1
    stop_service
    if [ -f "${backup}/bin-screen-time" ]; then
        atomic_install "${backup}/bin-screen-time" "$BIN" 755
    else
        rm -f -- "$BIN"
    fi
    if [ -f "${backup}/bin-rofi-screen-time" ]; then
        atomic_install "${backup}/bin-rofi-screen-time" "$WRAPPER" 755
    else
        rm -f -- "$WRAPPER"
    fi
    if [ -f "${backup}/theme.rasi" ]; then
        atomic_install "${backup}/theme.rasi" "$THEME" 644
        atomic_write_text "$THEME_SHA_FILE" 600 "$(sha256sum "$THEME" | awk '{print $1}')"
    else
        rm -f -- "$THEME_SHA_FILE"
    fi
    if [ -f "${backup}/unit.service" ]; then
        atomic_install "${backup}/unit.service" "$UNIT" 644
    elif [ -f "$UNIT" ]; then
        rm -f -- "$UNIT"
    fi
    if [ -d "${backup}/db" ] && [ -f "${backup}/db/screen-time.db" ]; then
        rm -f -- "${DB}-wal" "${DB}-shm"
        atomic_install "${backup}/db/screen-time.db" "$DB" 600
        # Pre-0.4.0 backups used a file copy and may need their WAL/SHM.
        if [ -f "${backup}/db/screen-time.db-wal" ]; then
            atomic_install "${backup}/db/screen-time.db-wal" "${DB}-wal" 600
        fi
        if [ -f "${backup}/db/screen-time.db-shm" ]; then
            atomic_install "${backup}/db/screen-time.db-shm" "${DB}-shm" 600
        fi
    fi
    if [ -f "${backup}/VERSION" ]; then
        atomic_write_text "$VERSION_FILE" 600 "$(cat "${backup}/VERSION")"
    elif [ -n "$OLD_VERSION" ] && [ "$OLD_VERSION" != "none" ]; then
        atomic_write_text "$VERSION_FILE" 600 "$OLD_VERSION"
    fi
    return 0
}

fail_recovery() {
    local reason="$1"
    [ "${FAILURE_TRAP_ARMED:-0}" -eq 1 ] || return 0
    FAILURE_TRAP_ARMED=0
    set +e
    ERROR_PHASE="${ERROR_PHASE:-${CURRENT_PHASE:-unknown}}"
    warn "update failed during ${ERROR_PHASE}: $reason"
    warn "restoring the previous version, database snapshot, and service state"
    local recovery_path=""
    if [ -n "$BACKUP" ] && [ -d "$BACKUP" ]; then
        stop_service
        restore_backup_files "$BACKUP"
        restore_service_state
        local doctor_status="not-run"
        if [ -x "$BIN" ]; then
            if SCREEN_TIME_STATE_DIR="$STATE_DIR" "$BIN" doctor --json --no-runtime >/dev/null 2>&1; then
                doctor_status="ok"
            else
                doctor_status="failed"
            fi
        fi
        recovery_path="${BACKUP}/recovery.json"
        cat >"$recovery_path" <<EOF
{
  "version": 1,
  "status": "failed",
  "phase": "$ERROR_PHASE",
  "error": "$(json_escape "$reason")",
  "timestamp": "$(date -Is)",
  "previous_version": "$OLD_VERSION",
  "target_version": "$NEW_VERSION",
  "release_tag": "$TAG",
  "backup": "$BACKUP",
  "service_was_active": $([ "$WAS_ACTIVE" -eq 1 ] && printf true || printf false),
  "doctor": "$doctor_status"
}
EOF
        chmod 600 "$recovery_path"
        write_backup_manifest "failed"
    fi
    write_last_update "failed" "$ERROR_PHASE" "$reason"
    rm -f -- "$IN_PROGRESS"
    if [ "$AUTO" -eq 1 ]; then
        record_failure "$TAG" "$ERROR_PHASE"
        notify "Screen Time update failed" "Restored $OLD_VERSION; backup kept at $BACKUP"
    fi
    if [ -n "$recovery_path" ]; then
        printf 'install.sh: recovery manifest: %s\n' "$recovery_path" >&2
    fi
    printf 'install.sh: previous version restored; backup kept for diagnosis\n' >&2
}

record_failure() {
    local tag="$1"
    local phase="$2"
    local attempts=1 next=0 now
    now="$(date +%s)"
    if [ -f "$FAILURE_STATE" ]; then
        attempts="$(sed -n 's/.*"attempts": *\([0-9]*\).*/\1/p' "$FAILURE_STATE" | head -n1)"
        attempts="${attempts:-0}"
        attempts=$((attempts + 1))
    fi
    local delay=$((3600 * (1 << (attempts - 1))))
    [ "$delay" -gt 86400 ] && delay=86400
    next=$((now + delay))
    cat >"$FAILURE_STATE" <<EOF
{
  "version": 1,
  "tag": "$tag",
  "phase": "$phase",
  "attempts": $attempts,
  "next_attempt_epoch": $next,
  "timestamp": "$(date -Is)"
}
EOF
    chmod 600 "$FAILURE_STATE"
}

recover_interrupted() {
    [ -f "$IN_PROGRESS" ] || return 0
    warn "found an interrupted update at $IN_PROGRESS"
    local backup service_was_active
    backup="$(sed -n 's/.*"backup": *"\([^"]*\)".*/\1/p' "$IN_PROGRESS" | head -n1)"
    service_was_active="$(sed -n 's/.*"service_was_active": *\(true\|false\).*/\1/p' "$IN_PROGRESS" | head -n1)"
    if [ -n "$service_was_active" ]; then
        [ "$service_was_active" = "true" ] && WAS_ACTIVE=1 || WAS_ACTIVE=0
    fi
    if [ -n "$backup" ] && [ -d "$backup" ] && [ -f "$backup/manifest.json" ]; then
        BACKUP="$backup"
        restore_backup_files "$backup"
        restore_service_state
        write_last_update "failed" "interrupted" "previous update was interrupted"
        warn "restored the previous installation from $backup"
    else
        restore_service_state
    fi
    rm -f -- "$IN_PROGRESS"
}

# ---------------------------------------------------------------------------
# Read-only check mode
# ---------------------------------------------------------------------------

check_release_metadata() {
    local tag="$1"
    local stage fetch_status=0
    stage="$(mktemp -d "${TMP_ROOT}/screen-time-check.XXXXXX")"
    fetch "${RELEASES_BASE}/${tag}/SHA256SUMS" "${stage}/SHA256SUMS" 2>/dev/null \
        || fetch_status=$?
    if [ "$fetch_status" -ne 0 ]; then
        rm -rf -- "$stage"
        # curl 22 and wget 8 are HTTP errors (for example a 404).
        if [ "$fetch_status" -eq 22 ] || [ "$fetch_status" -eq 8 ]; then
            return 21
        fi
        return 20
    fi
    local asset
    case "$(uname -m)" in
        x86_64|amd64) asset="screen-time-x86_64-linux.tar.gz" ;;
        aarch64|arm64) asset="screen-time-aarch64-linux.tar.gz" ;;
        *) rm -rf -- "$stage"; return 21 ;;
    esac
    if ! awk -v name="$asset" '$2 == name {found=1} END {exit !found}' "${stage}/SHA256SUMS"; then
        rm -rf -- "$stage"
        return 21
    fi
    if [ "$ALLOW_UNSIGNED" -eq 1 ]; then
        SIGNATURE_KEY_ID="unsigned"
        rm -rf -- "$stage"
        return 0
    fi
    if ! fetch "${RELEASES_BASE}/${tag}/SHA256SUMS.minisig" "${stage}/SHA256SUMS.minisig" 2>/dev/null; then
        rm -rf -- "$stage"
        return 22
    fi
    if ! have minisign; then
        rm -rf -- "$stage"
        return 22
    fi
    local entry key verified=1
    while IFS= read -r entry; do
        [ -n "$entry" ] || continue
        key="${entry#*:}"
        if minisign -Vm "${stage}/SHA256SUMS" -x "${stage}/SHA256SUMS.minisig" -P "$key" >/dev/null 2>&1; then
            verified=0
            SIGNATURE_KEY_ID="${entry%%:*}"
            break
        fi
    done < <(trusted_keys)
    if [ "$verified" -ne 0 ]; then
        rm -rf -- "$stage"
        return 22
    fi
    rm -rf -- "$stage"
    return 0
}

is_session_locked() {
    [ -n "${XDG_SESSION_ID:-}" ] || return 1
    have loginctl || return 1
    local locked
    locked="$(loginctl show-session "$XDG_SESSION_ID" -p LockedHint --value 2>/dev/null || true)"
    [ "$locked" = "yes" ]
}

is_on_battery() {
    local supply status
    for supply in /sys/class/power_supply/*; do
        [ -r "$supply/type" ] || continue
        [ "$(cat "$supply/type" 2>/dev/null)" = "Battery" ] || continue
        status="$(cat "$supply/status" 2>/dev/null || true)"
        if [ "$status" = "Discharging" ]; then
            return 0
        fi
    done
    return 1
}

check_backoff() {
    [ -f "$FAILURE_STATE" ] || return 0
    local tag next now
    tag="$(sed -n 's/.*"tag": *"\([^"]*\)".*/\1/p' "$FAILURE_STATE" | head -n1)"
    next="$(sed -n 's/.*"next_attempt_epoch": *\([0-9]*\).*/\1/p' "$FAILURE_STATE" | head -n1)"
    now="$(date +%s)"
    if [ "$tag" = "$TAG" ] && [ -n "$next" ] && [ "$now" -lt "$next" ]; then
        info "previous attempt for $TAG failed; next retry after $(date -d "@$next" -Is 2>/dev/null || printf '%s' "$next")"
        return 1
    fi
    return 0
}

do_check() {
    local installed latest="" status=0 error="" available=false release_url=""
    installed="$(installed_version || true)"
    installed="${installed:-none}"

    if [ "$ALLOW_UNSIGNED" -ne 1 ] && ! have minisign; then
        status=22
        error="minisign is required to verify release signatures; install minisign or use --allow-unsigned for local testing"
    elif ! latest="$(resolve_latest_tag 2>/dev/null)"; then
        status=20
        error="cannot resolve the latest release"
    elif ! is_stable_tag "$latest"; then
        status=21
        error="latest release is not a stable tag: $latest"
    else
        local verify_status=0
        check_release_metadata "$latest" || verify_status=$?
        if [ "$verify_status" -eq 22 ]; then
            status=22
            error="release signature verification failed"
        elif [ "$verify_status" -eq 21 ]; then
            status=21
            error="invalid release metadata or missing architecture asset"
        elif [ "$verify_status" -ne 0 ]; then
            status=20
            error="cannot fetch release metadata"
        else
            release_url="https://github.com/${REPO}/releases/tag/${latest}"
            if [ "$installed" = "none" ] || version_gt "$latest" "$installed"; then
                available=true
                status=10
            fi
        fi
    fi

    if [ "$JSON" -eq 1 ]; then
        cat <<EOF
{
  "version": 1,
  "installed_version": "$installed",
  "latest_version": $(json_optional_string "$latest"),
  "release_url": $(json_optional_string "$release_url"),
  "channel": "stable",
  "update_available": $available,
  "signature_key_id": $(json_optional_string "$SIGNATURE_KEY_ID"),
  "checked_at": "$(date -Is)",
  "error": $( [ -n "$error" ] && printf '"%s"' "$error" || printf 'null' )
}
EOF
    else
        info "installed: $installed"
        if [ -n "$latest" ]; then info "latest:    $latest"; else info "latest:    none"; fi
        if [ "$available" = true ]; then info "update:    available"; else info "update:    up to date"; fi
        [ -n "$error" ] && warn "$error"
    fi

    if [ "$WRITE_CHECK_STATE" -eq 1 ]; then
        mkdir -p "$UPDATE_DIR"
        chmod 700 "$UPDATE_DIR"
        local temp="${UPDATE_DIR}/.last-check.$$"
        cat >"$temp" <<EOF
{
  "version": 1,
  "installed_version": "$installed",
  "latest_version": $(json_optional_string "$latest"),
  "release_url": $(json_optional_string "$release_url"),
  "channel": "stable",
  "update_available": $available,
  "signature_key_id": $(json_optional_string "$SIGNATURE_KEY_ID"),
  "checked_at": "$(date -Is)",
  "error": $( [ -n "$error" ] && printf '"%s"' "$error" || printf 'null' )
}
EOF
        chmod 600 "$temp"
        sync "$temp" 2>/dev/null || sync
        mv -f -- "$temp" "$LAST_CHECK"
        sync "$UPDATE_DIR" 2>/dev/null || sync
    fi

    if [ "$AUTO" -eq 1 ]; then
        if [ "$available" = true ]; then
            notify "Screen Time update available" "$installed -> ${latest:-unknown}; run bash install.sh --update"
        fi
        exit 0
    fi
    exit "$status"
}

# ---------------------------------------------------------------------------
# Auto-update units
# ---------------------------------------------------------------------------

refuse_nix_update_units() {
    local path
    for path in "$BIN" "$WRAPPER" "$UNIT" "$THEME"; do
        if [ -L "$path" ]; then
            local target
            target="$(readlink -f -- "$path" 2>/dev/null || true)"
            [ -n "$target" ] || target="$(readlink -- "$path" 2>/dev/null || true)"
            case "$target" in
                /nix/store/*)
                    die "refusing to create update units: $path is Nix/Home Manager-owned"
                    ;;
            esac
        fi
    done
}

write_unit_file() {
    local path="$1"
    local content="$2"
    atomic_write_text "$path" 644 "$content"
}

install_auto_update_units() {
    local mode="$1"
    [ "$NO_SERVICE" -eq 0 ] || die "--auto-update requires the systemd user service"
    refuse_nix_update_units
    mkdir -p "$UNIT_DIR"
    atomic_install "${BASH_SOURCE[0]}" "$INSTALL_COPY" 755
    local install_bin="${INSTALL_COPY}"
    local service_exec
    if [ ! -x "$install_bin" ]; then
        die "cannot install update units: installer copy is missing"
    fi
    if [ "$mode" = "check" ]; then
        service_exec="$(printf '%q' "$install_bin") --check --json --write-check-state --auto"
        write_unit_file "$UPDATE_CHECK_SERVICE" "[Unit]
Description=Screen Time update check
Wants=network-online.target
After=network-online.target

[Service]
Type=oneshot
Environment=SCREEN_TIME_AUTO_UPDATE=1
ExecStart=/usr/bin/env bash ${service_exec}
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=${STATE_DIR} ${CONFIG_HOME}/systemd/user
UMask=0077
"
        write_unit_file "$UPDATE_CHECK_TIMER" "[Unit]
Description=Daily Screen Time update check

[Timer]
OnCalendar=daily
Persistent=true
RandomizedDelaySec=1h
Unit=screen-time-update-check.service

[Install]
WantedBy=timers.target
"
        rm -f -- "$UPDATE_APPLY_SERVICE" "$UPDATE_APPLY_TIMER"
        if have systemctl; then
            systemctl --user daemon-reload >/dev/null 2>&1 || true
            systemctl --user disable --now screen-time-update-apply.timer >/dev/null 2>&1 || true
            systemctl --user enable --now screen-time-update-check.timer >/dev/null 2>&1 || true
        fi
        info "auto-update: check-only timer installed (screen-time-update-check.timer)"
    else
        service_exec="$(printf '%q' "$install_bin") --update --auto --channel stable --keep-backups 2"
        write_unit_file "$UPDATE_APPLY_SERVICE" "[Unit]
Description=Screen Time automatic update (signed stable releases)
Wants=network-online.target
After=network-online.target

[Service]
Type=oneshot
Environment=SCREEN_TIME_AUTO_UPDATE=1
ExecStart=/usr/bin/env bash ${service_exec}
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=${PREFIX} ${DATA_HOME} ${CONFIG_HOME}/systemd/user ${STATE_DIR}
UMask=0077
"
        write_unit_file "$UPDATE_APPLY_TIMER" "[Unit]
Description=Daily Screen Time automatic update

[Timer]
OnCalendar=daily
Persistent=true
RandomizedDelaySec=1h
Unit=screen-time-update-apply.service

[Install]
WantedBy=timers.target
"
        rm -f -- "$UPDATE_CHECK_SERVICE" "$UPDATE_CHECK_TIMER"
        if have systemctl; then
            systemctl --user daemon-reload >/dev/null 2>&1 || true
            systemctl --user disable --now screen-time-update-check.timer >/dev/null 2>&1 || true
            systemctl --user enable --now screen-time-update-apply.timer >/dev/null 2>&1 || true
        fi
        info "auto-update: signed auto-apply timer installed (screen-time-update-apply.timer)"
    fi
}

remove_auto_update_units() {
    if have systemctl; then
        systemctl --user disable --now screen-time-update-check.timer >/dev/null 2>&1 || true
        systemctl --user disable --now screen-time-update-apply.timer >/dev/null 2>&1 || true
    fi
    rm -f -- "$UPDATE_CHECK_SERVICE" "$UPDATE_CHECK_TIMER" "$UPDATE_APPLY_SERVICE" "$UPDATE_APPLY_TIMER"
    if have systemctl; then
        systemctl --user daemon-reload >/dev/null 2>&1 || true
    fi
    rm -f -- "$LAST_CHECK" "$FAILURE_STATE" "$IN_PROGRESS" "$INSTALL_COPY"
    info "auto-update: units removed"
}

# ---------------------------------------------------------------------------
# Pruning and cleanup
# ---------------------------------------------------------------------------

verified_backup_dirs() {
    [ -d "$BACKUP_ROOT" ] || return 0
    find "$BACKUP_ROOT" -mindepth 1 -maxdepth 1 -type d -printf '%T@ %p\n' 2>/dev/null \
        | sort -rn \
        | cut -d' ' -f2- \
        | while IFS= read -r dir; do
            [ -f "${dir}/manifest.json" ] || continue
            [ -f "${dir}/db/screen-time.db" ] || continue
            grep -q '"status": *"verified"' "${dir}/manifest.json" && printf '%s\n' "$dir"
        done
}

prune_backups() {
    local keep="$1"
    if [ "$keep" -lt 2 ]; then
        keep=2
    fi
    [ -d "$BACKUP_ROOT" ] || return 0
    local total=0
    while IFS= read -r dir; do
        [ -n "$dir" ] || continue
        total=$((total + 1))
    done < <(verified_backup_dirs)
    local index=0
    while IFS= read -r dir; do
        [ -n "$dir" ] || continue
        index=$((index + 1))
        if [ "$index" -gt "$keep" ] && [ "$total" -gt 2 ]; then
            rm -rf -- "$dir"
            total=$((total - 1))
            info "pruned old verified backup: $dir"
        fi
    done < <(verified_backup_dirs)
}

clean_junk() {
    local removed_staging=0 dir
    while IFS= read -r dir; do
        [ -n "$dir" ] || continue
        rm -rf -- "$dir"
        removed_staging=$((removed_staging + 1))
    done < <(find "$TMP_ROOT" -mindepth 1 -maxdepth 1 -type d -name 'screen-time-install.*' -mmin +60 2>/dev/null || true)
    find "$BIN_DIR" -maxdepth 1 -type f \( -name '.screen-time-install.*' -o -name 'screen-time.new' -o -name 'screen-time.part' \) -delete 2>/dev/null || true
    find "$(dirname "$THEME")" -maxdepth 1 -type f \( -name '.screen-time-install.*' -o -name '*.new' -o -name '*.part' \) -delete 2>/dev/null || true
    find "$UNIT_DIR" -maxdepth 1 -type f -name '.screen-time-install.*' -delete 2>/dev/null || true
    prune_backups "$KEEP_BACKUPS"
    info "removed ${removed_staging} stale staging dir(s); kept at least 2 verified backups"
}

# ---------------------------------------------------------------------------
# Rollback / uninstall
# ---------------------------------------------------------------------------

latest_verified_backup() {
    # sed reads the whole stream; head can close the pipe early and make
    # pipefail report a spurious error.
    verified_backup_dirs | sed -n '1p'
}

backup_schema_version() {
    local backup="$1"
    sed -n 's/.*"schema_version": *\([0-9]*\).*/\1/p' "${backup}/db/manifest.json" 2>/dev/null | head -n1
}

backup_service_was_active() {
    local backup="$1"
    local value
    value="$(sed -n 's/.*"service_was_active": *\(true\|false\).*/\1/p' "${backup}/manifest.json" 2>/dev/null | head -n1)"
    [ "$value" = "true" ]
}

verify_backup_snapshot() {
    local backup="$1"
    [ -f "${backup}/db/manifest.json" ] || return 0
    [ -f "${backup}/db/screen-time.db" ] || die "backup database is missing: $backup"
    local expected actual
    expected="$(sed -n 's/.*"sha256": *"\([^"]*\)".*/\1/p' "${backup}/db/manifest.json" | head -n1)"
    [ -n "$expected" ] || die "backup manifest has no database hash: $backup"
    actual="$(sha256sum "${backup}/db/screen-time.db" | awk '{print $1}')"
    [ "$expected" = "$actual" ] || die "backup database checksum mismatch: $backup"
}

do_rollback() {
    local backup
    recover_interrupted
    backup="$(latest_verified_backup)"
    [ -n "$backup" ] || die "no verified rollback backup found under $BACKUP_ROOT"
    info "rolling back from $backup"
    verify_backup_snapshot "$backup"
    BACKUP="$backup"
    OLD_VERSION="$(installed_version || true)"
    OLD_VERSION="${OLD_VERSION:-none}"
    NEW_VERSION="$(sed -n 's/.*"previous_version": *"\([^"]*\)".*/\1/p' "${backup}/manifest.json" | head -n1)"
    NEW_VERSION="${NEW_VERSION:-unknown}"
    TAG="$(sed -n 's/.*"release_tag": *"\([^"]*\)".*/\1/p' "${backup}/manifest.json" | head -n1)"
    WAS_ACTIVE=0
    backup_service_was_active "$backup" && WAS_ACTIVE=1
    local backup_schema target_schema
    backup_schema="$(backup_schema_version "$backup")"
    backup_schema="${backup_schema:-0}"
    target_schema="${backup_schema}"
    if [ -x "${backup}/bin-screen-time" ]; then
        target_schema="$("${backup}/bin-screen-time" schema-version 2>/dev/null || printf '%s' "$backup_schema")"
    fi
    if [ "$backup_schema" -gt "$target_schema" ] 2>/dev/null && [ "$ALLOW_SCHEMA_DOWNGRADE" -ne 1 ]; then
        die "backup schema v${backup_schema} is newer than the backup binary supports (v${target_schema}); pass --allow-schema-downgrade to override"
    fi
    if [ "$DRY_RUN" -eq 1 ]; then
        info "dry-run: restore $backup"
        return 0
    fi
    # Keep the current binary as a verifier: the restored binary may be older
    # and may predate `check-data` or JSON doctor output.
    local checker="${UPDATE_DIR}/.rollback-checker-$$"
    local checker_has_json=0
    if [ -x "$BIN" ] && cp -- "$BIN" "$checker" 2>/dev/null; then
        chmod 700 "$checker"
        INSTALL_TEMPS+=("$checker")
        if "$checker" schema-version >/dev/null 2>&1; then
            checker_has_json=1
        fi
    fi
    stop_service
    restore_backup_files "$backup"
    if [ -f "${backup}/db/screen-time.db" ] && [ "$checker_has_json" -eq 1 ]; then
        if ! SCREEN_TIME_STATE_DIR="$STATE_DIR" "$checker" check-data --against "${backup}/db" --json >/dev/null; then
            warn "data check failed after rollback; keeping the backup and stopping the service"
            if have systemctl; then
                systemctl --user stop screen-time.service >/dev/null 2>&1 || true
            fi
            SERVICE_STOPPED=0
            write_last_update "rollback-failed" "data-check" "restored data does not match the backup manifest"
            die "rollback data check failed; backup kept at $backup"
        fi
    fi
    restore_service_state
    if [ "$NO_SERVICE" -eq 0 ] && [ "$checker_has_json" -eq 1 ]; then
        sleep 1
        if [ "${SCREEN_TIME_INSTALLER_FAIL_ROLLBACK_DOCTOR:-0}" = "1" ] \
            || ! SCREEN_TIME_STATE_DIR="$STATE_DIR" "$checker" doctor --json --no-runtime >/dev/null; then
            warn "doctor failed after rollback; keeping the backup and stopping the service"
            if have systemctl; then
                systemctl --user stop screen-time.service >/dev/null 2>&1 || true
            fi
            SERVICE_STOPPED=0
            write_last_update "rollback-failed" "doctor" "doctor reported problems after rollback"
            die "rollback doctor failed; backup kept at $backup"
        fi
    fi
    write_last_update "rolled-back" "complete" ""
    info "rollback complete; backup kept at $backup"
}

confirm_purge() {
    [ "$YES" -eq 1 ] && return 0
    if [ ! -t 0 ]; then
        die "--purge in non-interactive mode requires --yes"
    fi
    local answer=""
    read -r -p "This deletes the database and all backups. Type 'purge' to continue: " answer
    [ "$answer" = "purge" ] || die "purge cancelled"
}

do_uninstall() {
    info "uninstalling screen-time"
    if [ "$DRY_RUN" -eq 1 ]; then
        info "dry-run: stop/disable screen-time.service and update timers"
        info "dry-run: remove $BIN $WRAPPER $THEME $UNIT"
        info "dry-run: remove update units under $UNIT_DIR"
        [ "$PURGE" -eq 1 ] && info "dry-run: remove state $STATE_DIR"
        return 0
    fi
    if [ "$PURGE" -eq 1 ]; then
        confirm_purge
    fi
    if [ "$NO_SERVICE" -eq 0 ] && have systemctl; then
        systemctl --user stop screen-time.service >/dev/null 2>&1 || true
        systemctl --user disable screen-time.service >/dev/null 2>&1 || true
        systemctl --user disable --now screen-time-update-check.timer >/dev/null 2>&1 || true
        systemctl --user disable --now screen-time-update-apply.timer >/dev/null 2>&1 || true
    fi
    rm -f -- "$UNIT" "$UPDATE_CHECK_SERVICE" "$UPDATE_CHECK_TIMER" \
        "$UPDATE_APPLY_SERVICE" "$UPDATE_APPLY_TIMER"
    if have systemctl; then
        systemctl --user daemon-reload >/dev/null 2>&1 || true
    fi
    rm -f -- "$BIN" "$WRAPPER" "$THEME" "$EXAMPLE_CONFIG" "$THEME_SHA_FILE" "$VERSION_FILE" "$INSTALL_COPY"
    rm -f -- "${RUNTIME_DIR}/live.json" "${RUNTIME_DIR}/ui.json" "${RUNTIME_DIR}/rofi-theme.rasi"
    rmdir "$RUNTIME_DIR" >/dev/null 2>&1 || true
    rm -f -- "$LAST_CHECK" "$FAILURE_STATE" "$IN_PROGRESS"
    rmdir "$(dirname "$THEME")" "$THEME_DIR" "$BIN_DIR" >/dev/null 2>&1 || true
    if [ "$PURGE" -eq 1 ]; then
        rm -rf -- "$STATE_DIR"
        info "state and history removed"
    else
        info "state kept at $STATE_DIR (use --purge to remove it)"
    fi
    rmdir "$UNIT_DIR" >/dev/null 2>&1 || true
    info "uninstalled"
}

# ---------------------------------------------------------------------------
# Install / update
# ---------------------------------------------------------------------------

write_updater_copy() {
    [ -f "${BASH_SOURCE[0]}" ] || return 0
    atomic_install "${BASH_SOURCE[0]}" "$INSTALL_COPY" 755
}

do_install() {
    require_base_tools
    recover_interrupted
    prepare_release
    extract_release

    NEW_VERSION="$("$STAGED_BIN" --version 2>/dev/null | awk '{print $2}' | head -n1 || true)"
    NEW_VERSION="${NEW_VERSION:-unknown}"
    OLD_VERSION="$(installed_version || true)"
    OLD_VERSION="${OLD_VERSION:-none}"
    if [ "$OLD_VERSION" = "none" ] || [ ! -x "$BIN" ]; then
        FIRST_INSTALL=1
    fi

    if [ "$UPDATE_ONLY" -eq 1 ]; then
        if [ "$NEW_VERSION" = "$OLD_VERSION" ]; then
            info "screen-time is up to date ($OLD_VERSION)"
            write_last_update "up-to-date" "complete" ""
            exit 0
        fi
        if [ "$ALLOW_DOWNGRADE" -ne 1 ] && version_gt "$OLD_VERSION" "$NEW_VERSION"; then
            info "installed $OLD_VERSION is newer than $NEW_VERSION; no automatic downgrade"
            write_last_update "up-to-date" "complete" ""
            exit 0
        fi
        if [ "$ALLOW_DOWNGRADE" -ne 1 ] && [ "$TAG" != "local" ] && ! version_gt "$TAG" "$OLD_VERSION"; then
            info "screen-time is up to date ($OLD_VERSION)"
            write_last_update "up-to-date" "complete" ""
            exit 0
        fi
    fi

    if [ "$AUTO" -eq 1 ]; then
        if is_session_locked; then
            info "user session is locked; skipping update"
            exit 0
        fi
        if [ "${SCREEN_TIME_ALLOW_BATTERY:-0}" != "1" ] && is_on_battery; then
            info "running on battery; skipping automatic update"
            exit 0
        fi
        if ! check_backoff; then
            exit 0
        fi
        if [ "$ALLOW_MAJOR" -ne 1 ] && [ "$OLD_VERSION" != "none" ] \
            && [ "$(major_of "$NEW_VERSION")" != "$(major_of "$OLD_VERSION")" ]; then
            notify "Screen Time major update held" "$OLD_VERSION -> $NEW_VERSION requires manual approval"
            info "major version update held for manual approval"
            exit 0
        fi
    fi

    local is_downgrade=0
    if [ "$OLD_VERSION" != "none" ] && version_gt "$OLD_VERSION" "$NEW_VERSION"; then
        is_downgrade=1
    fi
    if [ "$is_downgrade" -eq 1 ] && [ "$ALLOW_DOWNGRADE" -ne 1 ]; then
        die "installed $OLD_VERSION is newer than $NEW_VERSION; pass --allow-downgrade to override"
    fi
    if [ "$is_downgrade" -eq 1 ] || [ "$ALLOW_DOWNGRADE" -eq 1 ]; then
        local target_schema live_schema
        target_schema="$("$STAGED_BIN" schema-version 2>/dev/null || printf '1')"
        live_schema="$("$BIN" schema-version 2>/dev/null || printf '1')"
        if [ "$live_schema" -gt "$target_schema" ] && [ "$ALLOW_SCHEMA_DOWNGRADE" -ne 1 ]; then
            die "live schema v${live_schema} is newer than $NEW_VERSION supports (v${target_schema}); pass --allow-schema-downgrade to override"
        fi
    fi

    if [ "$NO_THEME" -eq 0 ] && theme_is_user_modified && [ "$FORCE" -ne 1 ]; then
        die "the installed theme differs from the last installed version; back it up and rerun with --force"
    fi
    if [ "$NO_SERVICE" -eq 0 ] && unit_has_custom_execstart; then
        backup_user_unit_edit
        die "the installed unit has a custom ExecStart; update it manually or remove it before reinstalling"
    fi

    check_disk_space
    if [ -f "$CONFIG" ]; then
        CONFIG_HASH_BEFORE="$(sha256sum "$CONFIG" | awk '{print $1}')"
    fi

    if [ "$DRY_RUN" -eq 1 ]; then
        info "dry-run: install $NEW_VERSION (previous: $OLD_VERSION)"
        info "dry-run: binary $BIN"
        info "dry-run: wrapper $WRAPPER"
        [ "$NO_THEME" -eq 0 ] && info "dry-run: theme $THEME"
        [ "$NO_SERVICE" -eq 0 ] && info "dry-run: unit $UNIT"
        info "dry-run: backup $BACKUP_ROOT/<generation>"
        return 0
    fi

    mkdir -p "$STATE_DIR"
    chmod 700 "$STATE_DIR"

    printf 'install.sh: starting update %s -> %s\n' "$OLD_VERSION" "$NEW_VERSION" >&2
    WAS_ACTIVE=0
    service_active && WAS_ACTIVE=1
    CURRENT_PHASE="stopping"
    write_in_progress "$CURRENT_PHASE"
    stop_service

    CURRENT_PHASE="snapshot"
    create_backup
    SNAPSHOT_DONE=1
    CURRENT_PHASE="installing"
    write_in_progress "$CURRENT_PHASE"
    FAILURE_TRAP_ARMED=1
    inject_failure "snapshot"

    CURRENT_PHASE="installing"
    atomic_install "$STAGED_BIN" "$BIN" 755
    inject_failure "binary"
    atomic_install "${STAGE}/extract/bin/rofi-screen-time" "$WRAPPER" 755
    inject_failure "wrapper"
    if [ "$NO_THEME" -eq 0 ]; then
        atomic_install "${STAGE}/extract/share/rofi/screen-time.rasi" "$THEME" 644
        inject_failure "theme"
    fi
    if [ -f "${STAGE}/extract/share/screen-time/config.example.toml" ]; then
        atomic_install "${STAGE}/extract/share/screen-time/config.example.toml" "$EXAMPLE_CONFIG" 644
    fi
    if [ "$NO_SERVICE" -eq 0 ]; then
        sed "s|^ExecStart=.*|ExecStart=${BIN} daemon|" \
            "${STAGE}/extract/systemd/screen-time.service" > "${STAGE}/screen-time.service"
        atomic_install "${STAGE}/screen-time.service" "$UNIT" 644
        inject_failure "unit"
    fi
    atomic_write_text "$VERSION_FILE" 600 "$NEW_VERSION"
    if [ "$NO_THEME" -eq 0 ] && [ -f "$THEME" ]; then
        atomic_write_text "$THEME_SHA_FILE" 600 "$(sha256sum "$THEME" | awk '{print $1}')"
    fi
    inject_failure "version"
    sync

    CURRENT_PHASE="doctor"
    write_in_progress "$CURRENT_PHASE"
    if [ "$NO_SERVICE" -eq 0 ]; then
        systemctl --user daemon-reload
    fi

    # Keep the service stopped for the data checks. Starting the daemon writes
    # a boot event and a power row, which would make a strict pre/post
    # fingerprint comparison meaningless.
    if [ "$SNAPSHOT_DONE" -eq 1 ] && [ "$DB_EXISTED" -eq 1 ] && [ -d "${BACKUP}/db" ]; then
        CURRENT_PHASE="data-check"
        if ! SCREEN_TIME_STATE_DIR="$STATE_DIR" "$BIN" check-data --against "${BACKUP}/db" --json >/dev/null; then
            ERROR_PHASE="data-check"
            die "post-update data fingerprint does not match the pre-update snapshot"
        fi
        inject_failure "data-check"
    fi
    if ! SCREEN_TIME_STATE_DIR="$STATE_DIR" "$BIN" doctor --json --no-runtime >/dev/null; then
        ERROR_PHASE="doctor"
        die "post-update doctor failed"
    fi
    inject_failure "doctor"
    if [ -f "$CONFIG" ]; then
        CONFIG_HASH_AFTER="$(sha256sum "$CONFIG" | awk '{print $1}')"
        if [ "$CONFIG_HASH_BEFORE" != "$CONFIG_HASH_AFTER" ]; then
            ERROR_PHASE="config-check"
            die "config.toml changed during the update"
        fi
    fi
    if [ "$NO_SERVICE" -eq 0 ]; then
        if [ "$WAS_ACTIVE" -eq 1 ] || [ "$FIRST_INSTALL" -eq 1 ]; then
            systemctl --user enable --now screen-time.service
            local active=0
            for _ in 1 2 3 4 5; do
                if [ "$(systemctl --user is-active screen-time.service 2>/dev/null || true)" = "active" ]; then
                    active=1
                    break
                fi
                sleep 1
            done
            if [ "$active" -ne 1 ]; then
                ERROR_PHASE="service-start"
                die "the new service did not become active"
            fi
            # The daemon may migrate app keys on first start. Verify the
            # post-migration database and runtime health while the failure
            # trap is still armed.
            if ! SCREEN_TIME_STATE_DIR="$STATE_DIR" "$BIN" doctor --json --no-runtime >/dev/null; then
                ERROR_PHASE="post-start-doctor"
                die "doctor failed after starting the new service"
            fi
        fi
    fi

    FAILURE_TRAP_ARMED=0
    write_backup_manifest "verified"
    if [ -n "$BACKUP" ]; then
        info "verified backup: $BACKUP"
    fi
    rm -f -- "$IN_PROGRESS" "$FAILURE_STATE"
    write_last_update "success" "complete" ""
    CURRENT_PHASE="complete"

    if [ "$NO_SERVICE" -eq 0 ] && [ "$WAS_ACTIVE" -eq 0 ] && [ "$FIRST_INSTALL" -eq 0 ]; then
        info "service was inactive before the update; leaving it inactive"
    fi

    write_updater_copy
    if [ -n "$AUTO_UPDATE_MODE" ]; then
        install_auto_update_units "$AUTO_UPDATE_MODE"
    fi
    prune_backups "$KEEP_BACKUPS"

    case ":${PATH}:" in
        *":${BIN_DIR}:"*) ;;
        *) warn "${BIN_DIR} is not in PATH; add it to launch rofi-screen-time" ;;
    esac

    if [ "$AUTO" -eq 1 ]; then
        notify "Screen Time updated" "$OLD_VERSION -> $NEW_VERSION; backup $BACKUP"
    fi
    info "installed ${NEW_VERSION} from ${TAG:-local tarball}"
    info "open the UI with: rofi-screen-time"
    if [ "$NO_SERVICE" -eq 0 ]; then
        info "logs: journalctl --user -u screen-time"
    fi
}

# ---------------------------------------------------------------------------
# Dispatch
# ---------------------------------------------------------------------------

if [ -n "$NIXOS_FLAKE" ]; then
    if (( CHECK_ONLY || UPDATE_ONLY || CLEAN || UNINSTALL || PURGE || ROLLBACK || FORCE || NO_SERVICE || NO_THEME || REQUIRE_HYPRLAND || NO_AUTO_UPDATE )) ||
        [ -n "$AUTO_UPDATE_MODE$VERSION$TARBALL$SHA256" ] || [ "$PREFIX" != "$HOME/.local" ] || [ "$KEEP_BACKUPS" != 2 ]; then
        die "--nixos accepts only --dry-run alongside it"
    fi
    checkout="$(dirname "$(realpath -- "${BASH_SOURCE[0]}")")"
    [ -f "$checkout/flake.nix" ] || die "--nixos requires install.sh from the screen-time-rofi checkout"
    have nixos-rebuild || die "nixos-rebuild is required for --nixos"
    rebuild=(sudo nixos-rebuild switch --flake "$NIXOS_FLAKE" --override-input screen-time-rofi "$checkout" --no-write-lock-file)
    if (( DRY_RUN )); then
        printf 'dry-run:'
        printf ' %q' "${rebuild[@]}"
        printf '\n'
        exit 0
    fi
    exec "${rebuild[@]}"
fi

refuse_nix_managed_path "$BIN"
refuse_nix_managed_path "$WRAPPER"
refuse_nix_managed_path "$UNIT"
refuse_nix_managed_path "$THEME"

if [ "$NO_AUTO_UPDATE" -eq 1 ]; then
    [ "$DRY_RUN" -eq 1 ] && { info "dry-run: remove update units"; exit 0; }
    acquire_lock
    remove_auto_update_units
    exit 0
fi

if [ "$CHECK_ONLY" -eq 1 ]; then
    [ "$DRY_RUN" -eq 1 ] && { info "dry-run: check the latest release"; exit 0; }
    # A plain check performs no installation writes. Only take the lock when
    # update state already exists or when the auto-check unit will write it.
    if [ "$WRITE_CHECK_STATE" -eq 1 ] || [ -d "$UPDATE_DIR" ]; then
        acquire_lock
    fi
    do_check
fi

if [ "$CLEAN" -eq 1 ]; then
    [ "$DRY_RUN" -eq 1 ] && { info "dry-run: remove stale staging dirs and old backups"; exit 0; }
    acquire_lock
    clean_junk
    exit 0
fi

if [ "$UNINSTALL" -eq 1 ]; then
    [ "$DRY_RUN" -eq 1 ] && { do_uninstall; exit 0; }
    acquire_lock
    do_uninstall
    exit 0
fi

if [ "$ROLLBACK" -eq 1 ]; then
    [ "$DRY_RUN" -eq 1 ] && { acquire_lock; do_rollback; exit 0; }
    acquire_lock
    do_rollback
    exit 0
fi

if [ "$DRY_RUN" -eq 1 ]; then
    info "dry-run: install prefix $PREFIX"
    info "dry-run: binary $BIN"
    info "dry-run: wrapper $WRAPPER"
    [ "$NO_THEME" -eq 0 ] && info "dry-run: theme $THEME"
    [ "$NO_SERVICE" -eq 0 ] && info "dry-run: unit $UNIT"
    info "dry-run: state $STATE_DIR"
    exit 0
fi

require_runtime_tools
install_dependency_hint
acquire_lock
do_install
