#!/usr/bin/env bash
# Installer integration and fault-injection tests in a temp HOME.
#
# Everything runs as the current user with stub systemctl/rofi/notify-send
# commands. No real service, timer, or state directory is touched.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/screen-time-installer-test.XXXXXX")"
PASS=0
FAIL=0

cleanup() {
    if [ "${KEEP_TEST_WORK:-0}" = "1" ]; then
        printf 'test workdir kept at %s\n' "$WORK" >&2
        return 0
    fi
    chmod -R u+rwX "$WORK" 2>/dev/null || true
    rm -rf -- "$WORK"
}
trap cleanup EXIT INT TERM

pass() {
    PASS=$((PASS + 1))
    printf 'ok   %s\n' "$1"
}

fail() {
    FAIL=$((FAIL + 1))
    printf 'FAIL %s\n' "$1" >&2
    printf 'FAIL %s\n' "$1" >>"${FAIL_LOG:-/dev/null}"
}

die() {
    printf 'test-installer.sh: %s\n' "$*" >&2
    exit 1
}

assert_eq() {
    local expected="$1" actual="$2" label="$3"
    if [ "$expected" = "$actual" ]; then
        pass "$label"
    else
        fail "$label (expected '$expected', got '$actual')"
    fi
}

assert_file() {
    if [ -e "$1" ]; then
        pass "$2"
    else
        fail "$2 (missing $1)"
    fi
}

assert_not_file() {
    if [ ! -e "$1" ]; then
        pass "$2"
    else
        fail "$2 (unexpected $1)"
    fi
}

assert_success() {
    local label="$1"
    local output
    shift
    if output="$("$@" 2>&1)"; then
        pass "$label"
    else
        fail "$label (command failed)"
        printf '%s\n' "$output" | tail -n 10 >&2
    fi
}

assert_failure() {
    local label="$1"
    shift
    if "$@"; then
        fail "$label (command unexpectedly succeeded)"
    else
        pass "$label"
    fi
}

assert_mode() {
    local expected="$1" path="$2" label="$3" actual
    actual="$(stat -c '%a' "$path" 2>/dev/null || printf 'missing')"
    assert_eq "$expected" "$actual" "$label"
}

sha() {
    sha256sum "$1" | awk '{print $1}'
}

# ---------------------------------------------------------------------------
# Environment and stubs
# ---------------------------------------------------------------------------

command -v minisign >/dev/null 2>&1 || die "minisign is required for installer tests"
command -v flock >/dev/null 2>&1 || die "flock is required"

mkdir -p "$WORK/bin" "$WORK/home" "$WORK/state" "$WORK/config" "$WORK/data" "$WORK/run" "$WORK/systemctl"
chmod 700 "$WORK/run" "$WORK/state"
export HOME="$WORK/home"
export XDG_STATE_HOME="$WORK/state"
export XDG_CONFIG_HOME="$WORK/config"
export XDG_DATA_HOME="$WORK/data"
export XDG_RUNTIME_DIR="$WORK/run"
export HYPRLAND_INSTANCE_SIGNATURE="installer-test"
export SYSTEMCTL_STATE="$WORK/systemctl"
export PATH="$WORK/bin:$PATH"
export TMPDIR="$WORK/tmp"
mkdir -p "$TMPDIR"

cat >"$WORK/bin/systemctl" <<'EOF'
#!/usr/bin/env bash
state="${SYSTEMCTL_STATE:?}"
[ "${1:-}" = "--user" ] && shift
case "${1:-}" in
    is-active)
        if [ -f "$state/active" ]; then printf 'active\n'; else printf 'inactive\n'; exit 3; fi
        ;;
    start|enable) touch "$state/active" ;;
    stop|disable) rm -f "$state/active" ;;
    show-environment|daemon-reload) exit 0 ;;
    *) exit 0 ;;
esac
EOF
chmod +x "$WORK/bin/systemctl"

cat >"$WORK/bin/rofi" <<'EOF'
#!/usr/bin/env bash
printf 'rofi 2.0.0\n'
EOF
chmod +x "$WORK/bin/rofi"

cat >"$WORK/bin/hyprctl" <<'EOF'
#!/usr/bin/env bash
printf 'Hyprland, 0.45.0\n'
EOF
chmod +x "$WORK/bin/hyprctl"

cat >"$WORK/bin/notify-send" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"${NOTIFY_LOG:-/dev/null}"
EOF
chmod +x "$WORK/bin/notify-send"
export NOTIFY_LOG="$WORK/notifications.log"
: >"$NOTIFY_LOG"

BIN="$HOME/.local/bin/screen-time"
UNIT="$XDG_CONFIG_HOME/systemd/user/screen-time.service"
THEME="$XDG_DATA_HOME/screen-time/rofi/screen-time.rasi"
STATE="$XDG_STATE_HOME/screen-time"
BACKUPS="$STATE/backups/updates"
CONFIG="$XDG_CONFIG_HOME/screen-time/config.toml"

# Build once. The installer always installs release-shaped tarballs.
(cd "$ROOT" && cargo build --locked >/dev/null 2>&1)

make_tarball() {
    local out="$1"
    local stage
    stage="$(mktemp -d "$WORK/release.XXXXXX")"
    mkdir -p "$stage/bin" "$stage/share/rofi" "$stage/systemd"
    install -m 755 "$ROOT/target/debug/screen-time" "$stage/bin/screen-time"
    install -m 755 "$ROOT/share/rofi-screen-time" "$stage/bin/rofi-screen-time"
    install -m 644 "$ROOT/share/rofi/screen-time.rasi" "$stage/share/rofi/screen-time.rasi"
    install -m 644 "$ROOT/systemd/screen-time.service" "$stage/systemd/screen-time.service"
    install -m 644 "$ROOT/LICENSE" "$ROOT/README.md" "$stage/"
    mkdir -p "$(dirname "$out")"
    tar -czf "$out" -C "$stage" .
    (cd "$(dirname "$out")" && sha256sum "$(basename "$out")" >SHA256SUMS)
    rm -rf -- "$stage"
}

# Build a signed fixture whose binary reports an older version. Used only to
# exercise the manual-downgrade guard.
make_fake_release() {
    local fixture="$1" tag="$2" version="$3" key="$4"
    local stage dir
    stage="$(mktemp -d "$WORK/fake-release.XXXXXX")"
    dir="${fixture}/${tag}"
    mkdir -p "$stage/bin" "$stage/share/rofi" "$stage/systemd" "$dir"
    cat >"$stage/bin/screen-time" <<EOF
#!/usr/bin/env bash
if [ "\$1" = "--version" ]; then
    printf 'screen-time ${version}\n'
    exit 0
fi
if [ "\$1" = "schema-version" ]; then
    printf '1\n'
    exit 0
fi
exit 0
EOF
    chmod 755 "$stage/bin/screen-time"
    printf '#!/usr/bin/env bash\nexit 0\n' >"$stage/bin/rofi-screen-time"
    chmod 755 "$stage/bin/rofi-screen-time"
    install -m 644 "$ROOT/share/rofi/screen-time.rasi" "$stage/share/rofi/screen-time.rasi"
    install -m 644 "$ROOT/systemd/screen-time.service" "$stage/systemd/screen-time.service"
    install -m 644 "$ROOT/LICENSE" "$ROOT/README.md" "$stage/"
    tar -czf "$dir/screen-time-x86_64-linux.tar.gz" -C "$stage" .
    install -m 755 "$ROOT/install.sh" "$dir/install.sh"
    install -m 755 "$ROOT/uninstall.sh" "$dir/uninstall.sh"
    install -m 644 "$ROOT/minisign.pub" "$dir/minisign.pub"
    (
        cd "$dir"
        sha256sum screen-time-x86_64-linux.tar.gz install.sh uninstall.sh minisign.pub >SHA256SUMS
        rm -f -- SHA256SUMS.minisig install.sh.minisig
        minisign -Sm SHA256SUMS -s "$key" -x SHA256SUMS.minisig >/dev/null
        minisign -Sm install.sh -s "$key" -x install.sh.minisig >/dev/null
    )
    rm -rf -- "$stage"
}

# Replace the installed binary with a wrapper that reports an older version but
# delegates all other commands to the real binary.
make_fake_installed() {
    local version="$1"
    [ -x "$WORK/real-screen-time" ] || cp -- "$BIN" "$WORK/real-screen-time"
    cat >"$BIN" <<EOF
#!/usr/bin/env bash
if [ "\$1" = "--version" ]; then
    printf 'screen-time ${version}\n'
    exit 0
fi
exec "$WORK/real-screen-time" "\$@"
EOF
    chmod 755 "$BIN"
}

# Installer invocations use local, unsigned tarballs unless a test explicitly
# uses the signed release fixture.
run_install() {
    bash "$ROOT/install.sh" --tarball "$TARBALL" --allow-unsigned "$@"
}

seed_database() {
    # Start the real daemon briefly so it creates the schema and a power row,
    # then add a deterministic session with Python's sqlite3.
    timeout 3 "$BIN" daemon >/dev/null 2>&1 || true
    python3 - "$STATE/screen-time.db" <<'PY'
import sqlite3, sys
path = sys.argv[1]
db = sqlite3.connect(path)
db.execute("INSERT INTO apps(key, first_seen_ts, last_seen_ts) VALUES('Code', 1000, 2000) ON CONFLICT(key) DO NOTHING")
db.execute("INSERT INTO sessions(date, app, start_ts, end_ts, seconds, state, confidence) VALUES('2026-11-20', 'Code', 1000, 1300, 300, 'closed', 'high')")
db.execute("INSERT INTO daily_app(date, app, seconds, sessions, first_ts, last_ts) VALUES('2026-11-20', 'Code', 300, 1, 1000, 1300)")
db.execute("INSERT INTO daily_slots(date, app, slot, seconds) VALUES('2026-11-20', 'Code', 1, 300)")
db.commit()
db.close()
PY
}

doctor_json() {
    "$BIN" doctor --json --no-runtime
}

# ---------------------------------------------------------------------------
# Core install/upgrade/rollback
# ---------------------------------------------------------------------------

TARBALL="$WORK/release/screen-time-test.tar.gz"
make_tarball "$TARBALL"

assert_success "first install" run_install --no-service >/dev/null
assert_file "$BIN" "first install writes the binary"
assert_file "$THEME" "first install writes the theme"
assert_eq "$(cat "$STATE/installed-version")" "0.4.0" "installed version is recorded"

# Install the unit in a second pass (service active).
touch "$SYSTEMCTL_STATE/active"
assert_success "reinstall with service unit" run_install >/dev/null
assert_file "$UNIT" "unit is installed"
assert_file "$SYSTEMCTL_STATE/active" "service is active after a successful update"

seed_database
before="$(doctor_json)"
before_sessions="$(printf '%s' "$before" | jq -r '.fingerprint.session_seconds')"
before_events="$(printf '%s' "$before" | jq -r '.fingerprint.counts.events')"
assert_eq "$before_sessions" "300" "seeded session seconds are visible"

mkdir -p "$(dirname "$CONFIG")"
printf '[theme]\nmode = "nord"\n' >"$CONFIG"
config_sha="$(sha "$CONFIG")"

assert_success "upgrade/reinstall with data" run_install >/dev/null
after="$(doctor_json)"
config_after="$(sha "$CONFIG")"
assert_eq "$config_sha" "$config_after" "config.toml hash is unchanged"
assert_eq "$before_sessions" "$(printf '%s' "$after" | jq -r '.fingerprint.session_seconds')" "session seconds survive an update"
assert_eq "$before_events" "$(printf '%s' "$after" | jq -r '.fingerprint.counts.events')" "event count survives an update"
assert_eq "ok" "$(printf '%s' "$after" | jq -r '.quick_check')" "live database quick_check is ok"
assert_eq "false" "$(printf '%s' "$after" | jq -r '.retention.enabled')" "retention is disabled by default"

backup="$(find "$BACKUPS" -mindepth 1 -maxdepth 1 -type d | sort | tail -n1)"
[ -n "$backup" ] || die "no update backup was created"
assert_file "$backup/manifest.json" "backup manifest exists"
assert_eq "verified" "$(jq -r '.status' "$backup/manifest.json")" "successful backup is marked verified"
assert_eq "300" "$(jq -r '.snapshot.fingerprint.session_seconds' "$backup/manifest.json")" "backup manifest records the fingerprint"
assert_mode 700 "$backup" "backup directory is private"
assert_mode 600 "$backup/manifest.json" "backup manifest is private"
assert_mode 600 "$backup/db/screen-time.db" "backup database is private"
assert_mode 600 "$backup/db/manifest.json" "snapshot manifest is private"

REFERENCE="$WORK/reference-snapshot"
cp -a "$backup/db" "$REFERENCE"

assert_success "restore drill verifies the newest backup" \
    "$ROOT/scripts/restore-drill.sh" "$BACKUPS" --binary "$BIN" >/dev/null
assert_success "restore drill verifies the oldest backup" \
    "$ROOT/scripts/restore-drill.sh" "$BACKUPS" --oldest --binary "$BIN" >/dev/null

assert_success "rollback data check" "$BIN" check-data --against "$REFERENCE" --json >/dev/null
if KEEP_TEST_WORK=1 bash "$ROOT/install.sh" --rollback >"$WORK/rollback.out" 2>"$WORK/rollback.err"; then
    pass "rollback keeps backup"
else
    fail "rollback keeps backup (command failed)"
    tail -n 20 "$WORK/rollback.out" >&2
    tail -n 20 "$WORK/rollback.err" >&2
fi
assert_file "$backup/manifest.json" "rollback does not delete its backup"
assert_eq "300" "$("$BIN" check-data --against "$REFERENCE" --json | jq -r '.actual_fingerprint.session_seconds')" "rollback preserves session seconds"

# A rollback that fails its post-restore doctor keeps the backup, stops the
# service, and records a recovery status.
assert_failure "rollback doctor failure is reported" \
    env SCREEN_TIME_INSTALLER_FAIL_ROLLBACK_DOCTOR=1 bash "$ROOT/install.sh" --rollback >/dev/null 2>&1
assert_file "$backup/manifest.json" "failed rollback keeps the backup"
assert_not_file "$SYSTEMCTL_STATE/active" "failed rollback leaves the service stopped"
assert_eq "rollback-failed" "$(jq -r '.status' "$STATE/updates/last-update.json")" "failed rollback writes last-update state"
assert_eq "doctor" "$(jq -r '.phase' "$STATE/updates/last-update.json")" "failed rollback records the doctor phase"
assert_success "rollback recovers after a failed doctor" bash "$ROOT/install.sh" --rollback >/dev/null
assert_file "$SYSTEMCTL_STATE/active" "successful rollback restores the active service"

# Inactive service stays inactive.
rm -f "$SYSTEMCTL_STATE/active"
assert_success "update while service inactive" run_install >/dev/null
assert_not_file "$SYSTEMCTL_STATE/active" "inactive service stays inactive"
touch "$SYSTEMCTL_STATE/active"
assert_success "update while service active" run_install >/dev/null
assert_file "$SYSTEMCTL_STATE/active" "active service stays active"

# ---------------------------------------------------------------------------
# Protected user files and failure injection
# ---------------------------------------------------------------------------

printf '\n// user edit\n' >>"$THEME"
assert_failure "modified theme refuses update" run_install >/dev/null 2>&1
assert_success "modified theme accepts --force" run_install --force >/dev/null

sed -i 's|^ExecStart=.*|ExecStart=/bin/true|' "$UNIT"
assert_failure "custom unit ExecStart refuses update" run_install >/dev/null 2>&1
if find "$STATE/backups/units" -type f -print -quit 2>/dev/null | grep -q .; then
    pass "custom unit edit is backed up"
else
    fail "custom unit edit is backed up"
fi
# Restore a managed unit for the remaining tests.
sed -i "s|^ExecStart=.*|ExecStart=${BIN} daemon|" "$UNIT"
assert_success "reinstall after restoring unit" run_install --force >/dev/null

for stage in binary wrapper theme unit version data-check doctor; do
    assert_failure "fault after $stage rolls back" env SCREEN_TIME_INSTALLER_FAIL_AFTER="$stage" bash "$ROOT/install.sh" --tarball "$TARBALL" --allow-unsigned >/dev/null 2>&1
    assert_eq "0.4.0" "$("$BIN" --version | awk '{print $2}')" "binary restored after fault at $stage"
    assert_eq "300" "$("$BIN" check-data --against "$REFERENCE" --json | jq -r '.actual_fingerprint.session_seconds')" "data restored after fault at $stage"
    assert_file "$SYSTEMCTL_STATE/active" "service restored after fault at $stage"
done

assert_failure "fault after snapshot" env SCREEN_TIME_INSTALLER_FAIL_AFTER=snapshot bash "$ROOT/install.sh" --tarball "$TARBALL" --allow-unsigned >/dev/null 2>&1
assert_eq "0.4.0" "$("$BIN" --version | awk '{print $2}')" "binary restored after snapshot fault"

# Kill after the backup, then after the atomic binary replacement. The next
# run must recover the previous installation both times.
for stage in snapshot binary; do
    set +e
    env SCREEN_TIME_INSTALLER_KILL_AFTER="$stage" bash "$ROOT/install.sh" --tarball "$TARBALL" --allow-unsigned >/dev/null 2>&1
    kill_status=$?
    set -e
    assert_eq "137" "$kill_status" "simulated kill after $stage"
    assert_file "$STATE/updates/update-in-progress.json" "interrupted update ($stage) leaves in-progress state"
    assert_success "next run recovers interrupted update ($stage)" run_install >/dev/null
    assert_not_file "$STATE/updates/update-in-progress.json" "recovery ($stage) clears in-progress state"
    assert_eq "300" "$("$BIN" check-data --against "$REFERENCE" --json | jq -r '.actual_fingerprint.session_seconds')" "data intact after interrupted recovery ($stage)"
done

# Read-only backup directory aborts without replacing live files.
chmod 500 "$BACKUPS"
assert_mode 500 "$BACKUPS" "backup dir was made read-only"
if run_install --no-service; then
    fail "read-only backup dir aborts (command unexpectedly succeeded)"
    stat -c '%a %n' "$BACKUPS" "$BACKUPS"/* 2>&1 | tail -5 >&2 || true
else
    pass "read-only backup dir aborts"
fi
chmod 700 "$BACKUPS"
assert_eq "0.4.0" "$("$BIN" --version | awk '{print $2}')" "binary intact after read-only backup abort"
assert_file "$SYSTEMCTL_STATE/active" "service active after read-only backup abort"

# Read-only install target aborts after the snapshot and restores everything.
chmod 500 "$(dirname "$BIN")"
assert_failure "read-only binary dir aborts" run_install >/dev/null 2>&1
chmod 700 "$(dirname "$BIN")"
assert_eq "0.4.0" "$("$BIN" --version | awk '{print $2}')" "binary restored after read-only install target"
assert_eq "300" "$("$BIN" check-data --against "$REFERENCE" --json | jq -r '.actual_fingerprint.session_seconds')" "data restored after read-only install target"
assert_file "$SYSTEMCTL_STATE/active" "service restored after read-only install target"

# Concurrent update lock.
mkdir -p "$STATE/updates"
( flock 9; sleep 3 ) 9>>"$STATE/updates/update.lock" &
lock_pid=$!
sleep 0.3
assert_failure "concurrent update is refused" run_install >/dev/null 2>&1
wait "$lock_pid" || true

# ---------------------------------------------------------------------------
# Signed release fixture, check mode, and tamper refusal
# ---------------------------------------------------------------------------

make_release_fixture() {
    local dir="$1" key="$2"
    mkdir -p "$dir/v0.4.0"
    cp "$TARBALL" "$dir/v0.4.0/screen-time-x86_64-linux.tar.gz"
    install -m 755 "$ROOT/install.sh" "$dir/v0.4.0/install.sh"
    install -m 755 "$ROOT/uninstall.sh" "$dir/v0.4.0/uninstall.sh"
    install -m 644 "$ROOT/minisign.pub" "$dir/v0.4.0/minisign.pub"
    (
        cd "$dir/v0.4.0"
        sha256sum screen-time-x86_64-linux.tar.gz install.sh uninstall.sh minisign.pub >SHA256SUMS
        rm -f -- SHA256SUMS.minisig install.sh.minisig
        minisign -Sm SHA256SUMS -s "$key" -x SHA256SUMS.minisig >/dev/null
        minisign -Sm install.sh -s "$key" -x install.sh.minisig >/dev/null
    )
}

KEY1="$WORK/key1.key"
KEY2="$WORK/key2.key"
minisign -G -W -p "$WORK/key1.pub" -s "$KEY1" >/dev/null
minisign -G -W -p "$WORK/key2.pub" -s "$KEY2" >/dev/null
PUB1="$(tail -n1 "$WORK/key1.pub")"
PUB2="$(tail -n1 "$WORK/key2.pub")"
FIXTURE="$WORK/fixture"

make_release_fixture "$FIXTURE" "$KEY1"
export SCREEN_TIME_TRUSTED_KEYS="test-key-1:$PUB1"
export SCREEN_TIME_RELEASES_BASE="file://$FIXTURE"
export SCREEN_TIME_LATEST_TAG="v0.4.0"

set +e
check_output="$(bash "$ROOT/install.sh" --check --json)"
check_status=$?
set -e
assert_eq "0" "$check_status" "check exits 0 when installed is current"
assert_eq "test-key-1" "$(printf '%s' "$check_output" | jq -r '.signature_key_id')" "check verifies the signed release"
assert_eq "stable" "$(printf '%s' "$check_output" | jq -r '.channel')" "check reports the stable channel"

mkdir -p "$FIXTURE/v0.5.0"
printf 'dummy  not-the-expected-asset\n' >"$FIXTURE/v0.5.0/SHA256SUMS"
set +e
SCREEN_TIME_LATEST_TAG=v0.5.0 bash "$ROOT/install.sh" --check --json >"$WORK/invalid.json"
invalid_status=$?
SCREEN_TIME_RELEASES_BASE="file://$WORK/missing" SCREEN_TIME_LATEST_TAG=v0.4.0 \
    bash "$ROOT/install.sh" --check --json >"$WORK/network.json"
network_status=$?
set -e
assert_eq "21" "$invalid_status" "missing architecture asset exits 21"
assert_eq "false" "$(jq -r '.update_available' "$WORK/invalid.json")" "invalid release reports no update"
assert_eq "false" "$(jq -r '.error == null' "$WORK/invalid.json")" "invalid release records an error"
assert_eq "20" "$network_status" "unreachable release exits 20"

set +e
SCREEN_TIME_FETCH_FAIL_RC=28 SCREEN_TIME_LATEST_TAG=v0.4.0 \
    bash "$ROOT/install.sh" --check --json >"$WORK/timeout.json"
timeout_status=$?
SCREEN_TIME_FETCH_FAIL_RC=22 SCREEN_TIME_LATEST_TAG=v0.4.0 \
    bash "$ROOT/install.sh" --check --json >"$WORK/rate-limit.json"
rate_limit_status=$?
set -e
assert_eq "20" "$timeout_status" "timeout is classified as a network error"
assert_eq "21" "$rate_limit_status" "HTTP/rate-limit failure is classified as an invalid release"

assert_success "signed release install" bash "$ROOT/install.sh" --version v0.4.0 >/dev/null
assert_eq "300" "$("$BIN" check-data --against "$REFERENCE" --json | jq -r '.actual_fingerprint.session_seconds')" "signed release install preserves data"
verified_count=0
while IFS= read -r dir; do
    [ -f "$dir/manifest.json" ] || continue
    [ -f "$dir/db/screen-time.db" ] || continue
    grep -q '"status": *"verified"' "$dir/manifest.json" && verified_count=$((verified_count + 1))
done < <(find "$BACKUPS" -mindepth 1 -maxdepth 1 -type d)
assert_eq "true" "$([ "$verified_count" -ge 2 ] && printf true || printf false)" "at least two verified data backups are retained"

# A manual --version older than the installed release is refused before any
# backup or file replacement unless --allow-downgrade is passed.
make_fake_release "$FIXTURE" v0.3.0 0.3.0 "$KEY1"
backup_count_before="$(find "$BACKUPS" -mindepth 1 -maxdepth 1 -type d | wc -l)"
assert_failure "manual downgrade is refused" \
    env SCREEN_TIME_RELEASES_BASE="file://$FIXTURE" bash "$ROOT/install.sh" --version v0.3.0 >/dev/null 2>&1
assert_eq "0.4.0" "$("$BIN" --version | awk '{print $2}')" "downgrade refusal leaves the installed version"
assert_eq "$backup_count_before" "$(find "$BACKUPS" -mindepth 1 -maxdepth 1 -type d | wc -l)" "downgrade refusal creates no backup"

mkdir -p "$WORK/tampered"
cp -a "$FIXTURE/." "$WORK/tampered/"
printf 'tampered' >>"$WORK/tampered/v0.4.0/screen-time-x86_64-linux.tar.gz"
assert_failure "tampered asset is refused" env SCREEN_TIME_RELEASES_BASE="file://$WORK/tampered" bash "$ROOT/install.sh" --version v0.4.0 >/dev/null 2>&1

mkdir -p "$WORK/wrongkey"
cp -a "$FIXTURE/." "$WORK/wrongkey/"
(cd "$WORK/wrongkey/v0.4.0" && rm -f SHA256SUMS.minisig install.sh.minisig \
    && minisign -Sm SHA256SUMS -s "$KEY2" -x SHA256SUMS.minisig >/dev/null \
    && minisign -Sm install.sh -s "$KEY2" -x install.sh.minisig >/dev/null)
assert_failure "wrong signing key is refused" env SCREEN_TIME_RELEASES_BASE="file://$WORK/wrongkey" bash "$ROOT/install.sh" --version v0.4.0 >/dev/null 2>&1

# Key rotation overlap: both keys trusted, release signed by the new key.
mkdir -p "$WORK/rotated"
cp -a "$FIXTURE/." "$WORK/rotated/"
(cd "$WORK/rotated/v0.4.0" && rm -f SHA256SUMS.minisig install.sh.minisig \
    && minisign -Sm SHA256SUMS -s "$KEY2" -x SHA256SUMS.minisig >/dev/null \
    && minisign -Sm install.sh -s "$KEY2" -x install.sh.minisig >/dev/null)
set +e
rotated_output="$(SCREEN_TIME_TRUSTED_KEYS="old:$PUB1;new:$PUB2" \
    SCREEN_TIME_RELEASES_BASE="file://$WORK/rotated" \
    bash "$ROOT/install.sh" --check --json)"
rotated_status=$?
set -e
assert_eq "0" "$rotated_status" "rotation overlap accepts a new trusted key"
assert_eq "new" "$(printf '%s' "$rotated_output" | jq -r '.signature_key_id')" "rotation reports the matching new key id"

# A key removed from the trusted set (revoked) cannot verify its release.
set +e
SCREEN_TIME_TRUSTED_KEYS="new:$PUB2" SCREEN_TIME_RELEASES_BASE="file://$FIXTURE" \
    bash "$ROOT/install.sh" --check --json >"$WORK/revoked.json"
revoked_status=$?
set -e
assert_eq "22" "$revoked_status" "revoked/removed key is refused"
assert_eq "false" "$(jq -r '.error == null' "$WORK/revoked.json")" "revoked key records a signature error"

mkdir -p "$WORK/unsigned"
cp -a "$FIXTURE/." "$WORK/unsigned/"
rm -f "$WORK/unsigned/v0.4.0/"*.minisig
assert_failure "unsigned release is refused" env SCREEN_TIME_RELEASES_BASE="file://$WORK/unsigned" bash "$ROOT/install.sh" --version v0.4.0 >/dev/null 2>&1

mkdir -p "$WORK/tampered-installer"
cp -a "$FIXTURE/." "$WORK/tampered-installer/"
printf '\n# tampered installer\n' >>"$WORK/tampered-installer/v0.4.0/install.sh"
assert_failure "tampered installer is refused" env SCREEN_TIME_RELEASES_BASE="file://$WORK/tampered-installer" bash "$ROOT/install.sh" --version v0.4.0 >/dev/null 2>&1

# Auto-update notification and rollback behavior. The fake old binary makes
# the signed v0.4.0 fixture look newer without rebuilding the project.
: >"$NOTIFY_LOG"
make_fake_installed 0.3.0
assert_success "auto-apply update succeeds" \
    env SCREEN_TIME_AUTO_UPDATE=1 SCREEN_TIME_ALLOW_BATTERY=1 \
    bash "$ROOT/install.sh" --update --auto --channel stable --keep-backups 2 >/dev/null
assert_eq "0.4.0" "$("$BIN" --version | awk '{print $2}')" "auto-apply installs the signed release"
if grep -q 'Screen Time updated' "$NOTIFY_LOG"; then
    pass "auto-apply sends a success notification"
else
    fail "auto-apply sends a success notification"
fi

make_fake_installed 0.3.0
assert_failure "auto-apply failure rolls back" \
    env SCREEN_TIME_AUTO_UPDATE=1 SCREEN_TIME_ALLOW_BATTERY=1 \
    SCREEN_TIME_INSTALLER_FAIL_AFTER=binary \
    bash "$ROOT/install.sh" --update --auto --channel stable --keep-backups 2 >/dev/null 2>&1
assert_eq "0.3.0" "$("$BIN" --version | awk '{print $2}')" "auto-apply failure restores the previous binary"
if grep -q 'Screen Time update failed' "$NOTIFY_LOG"; then
    pass "auto-apply sends a failure notification"
else
    fail "auto-apply sends a failure notification"
fi
assert_file "$SYSTEMCTL_STATE/active" "auto-apply failure restores the active service"
assert_file "$STATE/updates/failure.json" "auto-apply failure records backoff state"

# The backup from the failed auto-apply is retained and marked failed.
failed_backup=""
while IFS= read -r dir; do
    if grep -q '"status": *"failed"' "$dir/manifest.json" 2>/dev/null; then
        failed_backup="$dir"
        break
    fi
done < <(find "$BACKUPS" -mindepth 1 -maxdepth 1 -type d -printf '%T@ %p\n' | sort -rn | cut -d' ' -f2-)
if [ -n "$failed_backup" ]; then
    pass "failed auto-apply keeps its backup"
else
    fail "failed auto-apply keeps its backup"
fi

assert_success "restore real binary after notification tests" run_install >/dev/null
assert_eq "0.4.0" "$("$BIN" --version | awk '{print $2}')" "real binary restored after notification tests"

unset SCREEN_TIME_TRUSTED_KEYS SCREEN_TIME_RELEASES_BASE SCREEN_TIME_LATEST_TAG

# ---------------------------------------------------------------------------
# Auto-update unit lifecycle and uninstall
# ---------------------------------------------------------------------------

assert_success "install check timer" run_install --auto-update=check >/dev/null
assert_file "$XDG_CONFIG_HOME/systemd/user/screen-time-update-check.service" "check service unit installed"
assert_file "$XDG_CONFIG_HOME/systemd/user/screen-time-update-check.timer" "check timer installed"
assert_not_file "$XDG_CONFIG_HOME/systemd/user/screen-time-update-apply.timer" "apply timer absent in check mode"
assert_success "remove auto-update units" bash "$ROOT/install.sh" --no-auto-update >/dev/null
assert_not_file "$XDG_CONFIG_HOME/systemd/user/screen-time-update-check.timer" "check timer removed"
assert_success "install apply timer" run_install --auto-update=apply >/dev/null
assert_file "$XDG_CONFIG_HOME/systemd/user/screen-time-update-apply.timer" "apply timer installed"
assert_not_file "$XDG_CONFIG_HOME/systemd/user/screen-time-update-check.timer" "check timer removed in apply mode"
assert_eq "daily" "$(sed -n 's/^OnCalendar=//p' "$XDG_CONFIG_HOME/systemd/user/screen-time-update-apply.timer")" "timer runs daily"
assert_eq "true" "$(sed -n 's/^Persistent=//p' "$XDG_CONFIG_HOME/systemd/user/screen-time-update-apply.timer")" "timer is persistent"
assert_eq "1h" "$(sed -n 's/^RandomizedDelaySec=//p' "$XDG_CONFIG_HOME/systemd/user/screen-time-update-apply.timer")" "timer has a randomized delay"
if command -v systemd-analyze >/dev/null 2>&1; then
    assert_success "systemd unit syntax is valid" systemd-analyze verify \
        "$UNIT" \
        "$XDG_CONFIG_HOME/systemd/user/screen-time-update-apply.service" \
        "$XDG_CONFIG_HOME/systemd/user/screen-time-update-apply.timer"
fi

assert_failure "purge without confirmation is refused" bash "$ROOT/install.sh" --uninstall --purge >/dev/null 2>&1
assert_success "uninstall keeps state" bash "$ROOT/install.sh" --uninstall >/dev/null
assert_file "$STATE/screen-time.db" "uninstall keeps the database"
assert_file "$BACKUPS" "uninstall keeps backups and history"
assert_not_file "$BIN" "uninstall removes the binary"
assert_not_file "$XDG_CONFIG_HOME/systemd/user/screen-time-update-apply.timer" "uninstall removes update timers"

assert_success "purge with --yes" bash "$ROOT/install.sh" --uninstall --purge --yes >/dev/null
assert_not_file "$STATE" "purge removes state and history"

# Nix-managed path refusal in a separate HOME.
NIXHOME="$WORK/nixhome"
mkdir -p "$NIXHOME/.local/bin" "$NIXHOME/.local/state/screen-time"
ln -s /nix/store/fake-screen-time "$NIXHOME/.local/bin/screen-time"
if HOME="$NIXHOME" XDG_STATE_HOME="$NIXHOME/.local/state" XDG_CONFIG_HOME="$NIXHOME/.config" \
    XDG_DATA_HOME="$NIXHOME/.local/share" XDG_RUNTIME_DIR="$WORK/run" \
    bash "$ROOT/install.sh" --tarball "$TARBALL" --allow-unsigned --dry-run >/dev/null 2>&1; then
    fail "Nix-managed path is refused (dry-run)"
else
    pass "Nix-managed path is refused (dry-run)"
fi

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
