#!/usr/bin/env bash
set -euo pipefail

REPO="WillGle/screen-time-rofi"
PROG="screen-time"
VERSION=""
PREFIX="${HOME}/.local"
NO_SERVICE=0
NO_THEME=0
REQUIRE_HYPRLAND=0
FORCE=0
DRY_RUN=0
CLEAN=0
KEEP_BACKUPS=1
ROLLBACK=0
UNINSTALL=0
PURGE=0
CHECK_ONLY=0
TARBALL=""
SHA256=""

usage() {
    cat <<'EOF'
Usage: install.sh [options]

Install or update screen-time for the current user. No root required.

Options:
  --version VERSION     install a specific release tag (for example v0.1.0)
  --check               print installed and latest version; make no changes
  --prefix PATH         install prefix (default: ~/.local)
  --no-service          install files but do not install/start the user unit
  --no-theme            do not install the Rofi theme
  --require-hyprland    fail when Hyprland is not detected
  --force               replace a user-modified theme/unit
  --dry-run             print actions without changing files
  --clean               remove stale staging/backups; do not update
  --keep-backups N      previous versions to retain (default: 1)
  --rollback            restore the previous installed version
  --uninstall           remove binary, wrapper, theme, and unit; keep state
  --purge               with --uninstall, also delete state and history
  --tarball PATH        install a local release tarball (testing/offline)
  --sha256 HEX          checksum for --tarball (or use its SHA256SUMS file)
  -h, --help            show this help
EOF
}

die() {
    printf 'install.sh: %s\n' "$*" >&2
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

expand_path() {
    # shellcheck disable=SC2088
    case "$1" in
        "~") printf '%s' "$HOME" ;;
        "~/"*) printf '%s/%s' "$HOME" "${1:2}" ;;
        *) printf '%s' "$1" ;;
    esac
}

TMP_ROOT="${TMPDIR:-/tmp}"
STAGE=""
INSTALL_TEMPS=()
BACKUP=""

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

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            [ "$#" -ge 2 ] || die "--version requires a value"
            VERSION="$2"
            shift 2
            ;;
        --check) CHECK_ONLY=1; shift ;;
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

[ "$(uname -s)" = "Linux" ] || die "screen-time v1 supports Linux only"
[ "${EUID}" -ne 0 ] || die "do not run this as root; screen-time is a per-user install"
[ -n "${HOME:-}" ] || die "HOME is not set"

case "$KEEP_BACKUPS" in
    ''|*[!0-9]*) die "--keep-backups must be a non-negative integer" ;;
esac
[ "$KEEP_BACKUPS" -ge 1 ] || die "--keep-backups must be at least 1"

PREFIX="$(expand_path "$PREFIX")"
PREFIX="${PREFIX%/}"
BIN_DIR="${PREFIX}/bin"
BIN="${BIN_DIR}/${PROG}"
WRAPPER="${BIN_DIR}/rofi-screen-time"
DATA_HOME="${XDG_DATA_HOME:-${HOME}/.local/share}"
THEME_DIR="${DATA_HOME}/screen-time"
THEME="${THEME_DIR}/rofi/screen-time.rasi"
CONFIG_HOME="${XDG_CONFIG_HOME:-${HOME}/.config}"
UNIT="${CONFIG_HOME}/systemd/user/screen-time.service"
STATE_DIR="${XDG_STATE_HOME:-${HOME}/.local/state}/screen-time"
BACKUP_ROOT="${STATE_DIR}/backups/updates"
RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/screen-time"
DB="${STATE_DIR}/screen-time.db"
THEME_SHA_FILE="${STATE_DIR}/theme.sha256"
VERSION_FILE="${STATE_DIR}/installed-version"

if [ "$PURGE" -eq 1 ] && [ "$UNINSTALL" -ne 1 ]; then
    die "--purge requires --uninstall"
fi

if [ "$CHECK_ONLY" -eq 1 ]; then
    if [ -x "$BIN" ]; then
        info "installed: $("$BIN" --version 2>/dev/null || printf 'unknown')"
    else
        info "installed: none ($BIN)"
    fi
    if have curl || have wget; then
        latest_url="https://github.com/${REPO}/releases/latest"
        if have curl; then
            latest="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "$latest_url" 2>/dev/null || true)"
        else
            latest="$(wget -qS --spider "$latest_url" 2>&1 | awk '/^  Location:/ {print $2; exit}' || true)"
        fi
        latest_tag="${latest##*/}"
        case "$latest_tag" in
            ""|releases|latest) info "latest:    none published" ;;
            *) info "latest:    $latest_tag" ;;
        esac
    else
        warn "curl and wget are unavailable; cannot check the latest release"
    fi
    exit 0
fi

require_base_tools() {
    have tar || die "tar is required"
    have sha256sum || die "sha256sum is required"
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

fetch() {
    local url="$1"
    local dest="$2"
    if have curl; then
        curl -fsSL --retry 2 --connect-timeout 15 -o "$dest" "$url"
    else
        wget -qO "$dest" "$url"
    fi
}

resolve_latest_tag() {
    local latest_url="https://github.com/${REPO}/releases/latest"
    local effective=""
    if have curl; then
        effective="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "$latest_url" 2>/dev/null || true)"
    else
        effective="$(wget -qS --spider "$latest_url" 2>&1 | awk '/^  Location:/ {print $2; exit}' || true)"
    fi
    [ -n "$effective" ] || die "cannot resolve the latest GitHub release"
    printf '%s' "${effective##*/}"
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
    mv -f -- "$temp" "$dest"
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
    mv -f -- "$temp" "$dest"
}

theme_is_user_modified() {
    [ -f "$THEME" ] || return 1
    [ -f "$THEME_SHA_FILE" ] || return 1
    local recorded current
    recorded="$(cat "$THEME_SHA_FILE" 2>/dev/null || true)"
    current="$(sha256sum "$THEME" | awk '{print $1}')"
    [ -n "$recorded" ] && [ "$recorded" != "$current" ]
}

backup_file() {
    local src="$1"
    local name="$2"
    [ -e "$src" ] || return 0
    cp -a -- "$src" "${BACKUP}/${name}"
}

create_backup() {
    local old_version
    BACKUP="${BACKUP_ROOT}/$(date +%Y%m%d-%H%M%S)-$$"
    mkdir -p "$BACKUP"
    old_version="$(cat "$VERSION_FILE" 2>/dev/null || "$BIN" --version 2>/dev/null | awk '{print $2}' || true)"
    printf '%s\n' "${old_version:-unknown}" > "${BACKUP}/VERSION"
    backup_file "$BIN" "bin-screen-time"
    backup_file "$WRAPPER" "bin-rofi-screen-time"
    backup_file "$THEME" "theme.rasi"
    backup_file "$UNIT" "unit.service"
    if [ -f "$DB" ]; then
        mkdir -p "${BACKUP}/db"
        cp -a -- "$DB" "${BACKUP}/db/screen-time.db"
        [ -f "${DB}-wal" ] && cp -a -- "${DB}-wal" "${BACKUP}/db/screen-time.db-wal"
        [ -f "${DB}-shm" ] && cp -a -- "${DB}-shm" "${BACKUP}/db/screen-time.db-shm"
    fi
    if [ -z "$(find "$BACKUP" -mindepth 1 -maxdepth 1 ! -name VERSION -print -quit)" ]; then
        rm -f -- "${BACKUP}/VERSION"
        rmdir "$BACKUP"
        BACKUP=""
        info "backup: none (first install)"
    else
        info "backup: $BACKUP"
    fi
}

restore_backup() {
    local backup="$1"
    [ -d "$backup" ] || die "backup not found: $backup"
    if [ "$NO_SERVICE" -eq 0 ]; then
        systemctl --user stop screen-time.service >/dev/null 2>&1 || true
    fi
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
        atomic_write_text "$THEME_SHA_FILE" 644 "$(sha256sum "$THEME" | awk '{print $1}')"
    else
        rm -f -- "$THEME_SHA_FILE"
    fi
    if [ -f "${backup}/unit.service" ]; then
        atomic_install "${backup}/unit.service" "$UNIT" 644
    fi
    if [ -d "${backup}/db" ]; then
        rm -f -- "${DB}-wal" "${DB}-shm"
        if [ -f "${backup}/db/screen-time.db" ]; then
            atomic_install "${backup}/db/screen-time.db" "$DB" 600
        fi
        if [ -f "${backup}/db/screen-time.db-wal" ]; then
            atomic_install "${backup}/db/screen-time.db-wal" "${DB}-wal" 600
        fi
        if [ -f "${backup}/db/screen-time.db-shm" ]; then
            atomic_install "${backup}/db/screen-time.db-shm" "${DB}-shm" 600
        fi
    fi
    if [ -f "${backup}/VERSION" ]; then
        cp -- "${backup}/VERSION" "$VERSION_FILE" 2>/dev/null || true
    fi
    if [ "$NO_SERVICE" -eq 0 ]; then
        systemctl --user daemon-reload >/dev/null 2>&1 || true
        systemctl --user enable --now screen-time.service >/dev/null 2>&1 || true
    fi
}

prune_backups() {
    local keep="$1"
    [ -d "$BACKUP_ROOT" ] || return 0
    local count=0 dir
    while IFS= read -r dir; do
        [ -n "$dir" ] || continue
        count=$((count + 1))
        if [ "$count" -gt "$keep" ]; then
            rm -rf -- "$dir"
        fi
    done < <(find "$BACKUP_ROOT" -mindepth 1 -maxdepth 1 -type d -printf '%T@ %p\n' 2>/dev/null \
        | sort -rn | cut -d' ' -f2-)
}

clean_junk() {
    local removed_staging=0 pruned=0
    local dir
    while IFS= read -r dir; do
        [ -n "$dir" ] || continue
        rm -rf -- "$dir"
        removed_staging=$((removed_staging + 1))
    done < <(find "$TMP_ROOT" -mindepth 1 -maxdepth 1 -type d -name 'screen-time-install.*' -mmin +60 2>/dev/null || true)
    if [ -d "$BACKUP_ROOT" ]; then
        local count=0
        while IFS= read -r dir; do
            [ -n "$dir" ] || continue
            count=$((count + 1))
            if [ "$count" -gt "$KEEP_BACKUPS" ]; then
                rm -rf -- "$dir"
                pruned=$((pruned + 1))
            fi
        done < <(find "$BACKUP_ROOT" -mindepth 1 -maxdepth 1 -type d -printf '%T@ %p\n' 2>/dev/null \
            | sort -rn | cut -d' ' -f2-)
    fi
    find "$BIN_DIR" -maxdepth 1 -type f \( -name '.screen-time-install.*' -o -name 'screen-time.new' -o -name 'screen-time.part' \) -delete 2>/dev/null || true
    find "$(dirname "$THEME")" -maxdepth 1 -type f \( -name '.screen-time-install.*' -o -name '*.new' -o -name '*.part' \) -delete 2>/dev/null || true
    find "$(dirname "$UNIT")" -maxdepth 1 -type f -name '.screen-time-install.*' -delete 2>/dev/null || true
    info "removed ${removed_staging} staging dir(s), kept ${KEEP_BACKUPS} rollback backup(s), pruned ${pruned} old backup(s)"
}

do_uninstall() {
    info "uninstalling screen-time"
    if [ "$DRY_RUN" -eq 1 ]; then
        info "dry-run: stop/disable screen-time.service"
        info "dry-run: remove $BIN $WRAPPER $THEME $UNIT"
        [ "$PURGE" -eq 1 ] && info "dry-run: remove state $STATE_DIR"
        return 0
    fi
    if [ "$NO_SERVICE" -eq 0 ] && have systemctl; then
        systemctl --user stop screen-time.service >/dev/null 2>&1 || true
        systemctl --user disable screen-time.service >/dev/null 2>&1 || true
    fi
    rm -f -- "$UNIT"
    if have systemctl; then
        systemctl --user daemon-reload >/dev/null 2>&1 || true
    fi
    rm -f -- "$BIN" "$WRAPPER" "$THEME" "$THEME_SHA_FILE" "$VERSION_FILE"
    rm -f -- "${RUNTIME_DIR}/live.json" "${RUNTIME_DIR}/ui.json"
    rmdir "$RUNTIME_DIR" >/dev/null 2>&1 || true
    rm -rf -- "$STATE_DIR/backups"
    rmdir "$(dirname "$THEME")" "$THEME_DIR" "$BIN_DIR" >/dev/null 2>&1 || true
    rmdir "$STATE_DIR" >/dev/null 2>&1 || true
    if [ "$PURGE" -eq 1 ]; then
        rm -rf -- "$STATE_DIR"
        info "state removed"
    else
        info "state kept at $STATE_DIR (use --purge to remove it)"
    fi
    info "uninstalled"
}

do_rollback() {
    local latest
    latest="$(find "$BACKUP_ROOT" -mindepth 1 -maxdepth 1 -type d -printf '%T@ %p\n' 2>/dev/null | sort -rn | head -n1 | cut -d' ' -f2- || true)"
    [ -n "$latest" ] || die "no rollback backup found under $BACKUP_ROOT"
    info "rolling back from $latest"
    if [ "$DRY_RUN" -eq 1 ]; then
        info "dry-run: restore $latest"
        return 0
    fi
    restore_backup "$latest"
    if [ "$NO_SERVICE" -eq 0 ]; then
        sleep 1
        "$BIN" doctor || warn "doctor reported problems after rollback"
    fi
    rm -rf -- "$latest"
    prune_backups "$KEEP_BACKUPS"
    info "rollback complete"
}

if [ "$CLEAN" -eq 1 ]; then
    if [ "$DRY_RUN" -eq 1 ]; then
        info "dry-run: remove stale staging dirs and old backups"
        exit 0
    fi
    clean_junk
    exit 0
fi

if [ "$UNINSTALL" -eq 1 ]; then
    do_uninstall
    exit 0
fi

if [ "$ROLLBACK" -eq 1 ]; then
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

require_base_tools
require_runtime_tools

[ "$KEEP_BACKUPS" -ge 1 ] || die "keep at least one rollback backup"

if [ -n "$TARBALL" ]; then
    [ -f "$TARBALL" ] || die "tarball not found: $TARBALL"
    STAGE="$(mktemp -d "${TMP_ROOT}/screen-time-install.XXXXXX")"
    cp -- "$TARBALL" "${STAGE}/screen-time.tar.gz"
    archive="${STAGE}/screen-time.tar.gz"
    if [ -n "$SHA256" ]; then
        printf '%s  %s\n' "$SHA256" "$archive" | sha256sum -c - >/dev/null
    elif [ -f "$(dirname "$TARBALL")/SHA256SUMS" ]; then
        expected="$(awk -v name="$(basename "$TARBALL")" '$2 == name {print $1; exit}' "$(dirname "$TARBALL")/SHA256SUMS")"
        [ -n "$expected" ] || die "no checksum for $(basename "$TARBALL") in SHA256SUMS"
        printf '%s  %s\n' "$expected" "$archive" | sha256sum -c - >/dev/null
    else
        die "local tarball install requires --sha256 or an adjacent SHA256SUMS"
    fi
else
    case "$(uname -m)" in
        x86_64|amd64) ARCH="x86_64" ;;
        aarch64|arm64) ARCH="aarch64" ;;
        *) die "unsupported architecture: $(uname -m)" ;;
    esac
    if [ -z "$VERSION" ]; then
        VERSION="$(resolve_latest_tag)"
    fi
    case "$VERSION" in
        v*) TAG="$VERSION" ;;
        *) TAG="v$VERSION" ;;
    esac
    ASSET="screen-time-${ARCH}-linux.tar.gz"
    BASE_URL="https://github.com/${REPO}/releases/download/${TAG}"
    STAGE="$(mktemp -d "${TMP_ROOT}/screen-time-install.XXXXXX")"
    info "downloading $ASSET ($TAG)"
    fetch "${BASE_URL}/${ASSET}" "${STAGE}/${ASSET}"
    fetch "${BASE_URL}/SHA256SUMS" "${STAGE}/SHA256SUMS"
    archive="${STAGE}/${ASSET}"
    expected="$(awk -v name="$ASSET" '$2 == name {print $1; exit}' "${STAGE}/SHA256SUMS")"
    [ -n "$expected" ] || die "checksum for $ASSET is missing from SHA256SUMS"
    printf '%s  %s\n' "$expected" "$archive" | sha256sum -c - >/dev/null
fi

mkdir -p "${STAGE}/extract"
tar -xzf "$archive" -C "${STAGE}/extract"
for required in bin/screen-time bin/rofi-screen-time systemd/screen-time.service; do
    [ -f "${STAGE}/extract/${required}" ] || die "release is missing $required"
done
if [ "$NO_THEME" -eq 0 ]; then
    [ -f "${STAGE}/extract/share/rofi/screen-time.rasi" ] || die "release is missing share/rofi/screen-time.rasi"
fi

if [ "$NO_THEME" -eq 0 ] && theme_is_user_modified && [ "$FORCE" -ne 1 ]; then
    die "the installed theme differs from the last installed version; back it up and rerun with --force"
fi

if [ "$NO_SERVICE" -eq 0 ] && have systemctl; then
    systemctl --user stop screen-time.service >/dev/null 2>&1 || true
fi

create_backup

atomic_install "${STAGE}/extract/bin/screen-time" "$BIN" 755
atomic_install "${STAGE}/extract/bin/rofi-screen-time" "$WRAPPER" 755
if [ "$NO_THEME" -eq 0 ]; then
    atomic_install "${STAGE}/extract/share/rofi/screen-time.rasi" "$THEME" 644
fi
if [ "$NO_SERVICE" -eq 0 ]; then
    sed "s|^ExecStart=.*|ExecStart=${BIN} daemon|" \
        "${STAGE}/extract/systemd/screen-time.service" > "${STAGE}/screen-time.service"
    atomic_install "${STAGE}/screen-time.service" "$UNIT" 644
    systemctl --user daemon-reload
    systemctl --user enable --now screen-time.service
    sleep 1
fi

mkdir -p "$STATE_DIR"
new_version="$("$BIN" --version 2>/dev/null | awk '{print $2}' || true)"
atomic_write_text "$VERSION_FILE" 644 "${new_version:-unknown}"
if [ "$NO_THEME" -eq 0 ] && [ -f "$THEME" ]; then
    atomic_write_text "$THEME_SHA_FILE" 644 "$(sha256sum "$THEME" | awk '{print $1}')"
fi

if [ "$NO_SERVICE" -eq 0 ]; then
    if ! "$BIN" doctor; then
        warn "doctor failed; restoring the previous version"
        if [ -n "$BACKUP" ] && { [ -f "${BACKUP}/VERSION" ] || [ -f "${BACKUP}/bin-screen-time" ]; }; then
            restore_backup "$BACKUP"
        else
            if have systemctl; then
                systemctl --user stop screen-time.service >/dev/null 2>&1 || true
                systemctl --user disable screen-time.service >/dev/null 2>&1 || true
            fi
            rm -f -- "$BIN" "$WRAPPER" "$UNIT" "$THEME" "$THEME_SHA_FILE" "$VERSION_FILE"
        fi
        exit 1
    fi
fi

prune_backups "$KEEP_BACKUPS"

case ":${PATH}:" in
    *":${BIN_DIR}:"*) ;;
    *) warn "${BIN_DIR} is not in PATH; add it to launch rofi-screen-time" ;;
esac

info "installed ${new_version:-screen-time} from ${TAG:-local tarball}"
info "open the UI with: rofi-screen-time"
if [ "$NO_SERVICE" -eq 0 ]; then
    info "logs: journalctl --user -u screen-time"
fi
info "cleanup: 1 staging dir removed, 1 rollback backup kept, pruned older backups"
