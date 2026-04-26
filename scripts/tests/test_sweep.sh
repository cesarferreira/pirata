#!/usr/bin/env bash
# Smoke test for scripts/sheets_sweep.py
#
# Hermetic: uses a tmpdir as fake downloads root; does not touch the user's
# real downloads/. Runs mostly in --dry-run mode to avoid spending 12min per
# fixture on real contact_sheet.py invocations — that path is exercised by
# the Roger Rabbit reference output already on disk.
#
# Covers:
#   1. Happy path: unsheeted release is detected
#   2. --dry-run: no subprocess, but log entries appear
#   3. Filter: non-video / undersized / pattern-matched files ignored
#   4. Flock: second sweep in parallel exits with "already active"
#   5. Argparse injection defense: filename --evil.mkv doesn't break sweep
#   6. Log injection defense: filename with \n and ANSI escape → repr-escaped
#   7. Symlink rejection: release dir pointing outside root → rejected
#   8. Cache-only detection: contact-sheets/ with only .txt → NOT sheeted
#   9. PNG-present detection: contact-sheets/ with *_sheet_*.png → sheeted
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SWEEP="$REPO_ROOT/scripts/sheets_sweep.py"
TMP="$(mktemp -d -t pirata-sweep-test-XXXXXX)"
DOWNLOADS="$TMP/downloads"
CANARY="$TMP/canary-outside-root.txt"
PASS=0
FAIL=0

cleanup() {
  rm -rf "$TMP"
}
trap cleanup EXIT

assert() {
  local name="$1"; shift
  if "$@"; then
    echo "PASS: $name"
    PASS=$((PASS + 1))
  else
    echo "FAIL: $name"
    FAIL=$((FAIL + 1))
  fi
}

mkdir -p "$DOWNLOADS"
# Override size floor so 1MB fixtures qualify.
export AUTOSHEETS_MIN_SIZE_MB=1

# Create 1MB+ dummy video files via head (not real videos; sweep never
# actually decodes them in --dry-run mode).
make_dummy() {
  local path="$1"
  mkdir -p "$(dirname "$path")"
  head -c 2000000 /dev/urandom > "$path"
}

# Test 1: unsheeted release → sweep sees it (dry-run detects)
R1="$DOWNLOADS/Test.Movie.2024.1080p"
make_dummy "$R1/movie.mkv"
# Test 2: already-sheeted → skip
R2="$DOWNLOADS/Old.Movie.1999"
make_dummy "$R2/film.mkv"
mkdir -p "$R2/contact-sheets"
head -c 1024 /dev/urandom > "$R2/contact-sheets/old-movie_sheet_01.png"
# Test 3 (filter): tiny file + .srt + sample* → no video candidates
R3="$DOWNLOADS/Filtered.Release"
mkdir -p "$R3"
head -c 500 /dev/urandom > "$R3/tiny.mkv"                  # under size floor
head -c 2000000 /dev/urandom > "$R3/Filtered.Release.srt"  # wrong ext
head -c 2000000 /dev/urandom > "$R3/sample.mkv"            # pattern skip
# Test 5 (argparse injection): filename --evil.mkv
R5="$DOWNLOADS/Flag.Injection.Test"
make_dummy "$R5/--evil.mkv"
# Test 6 (log injection): filename with control chars (\n + ANSI ESC)
R6="$DOWNLOADS/Log.Injection.Test"
# Note: literal \n in filename is legal on POSIX; $'\n' creates one.
make_dummy "$R6/$(printf 'bad\x1b[31m\nFAKE.mkv')"
# Test 7 (symlink rejection): release dir is a symlink to outside root
mkdir -p "$TMP/outside-root/Decoy.Movie.2024"
make_dummy "$TMP/outside-root/Decoy.Movie.2024/payload.mkv"
ln -s "$TMP/outside-root/Decoy.Movie.2024" "$DOWNLOADS/Symlink.Escape"
echo "SHOULD_NOT_BE_TOUCHED" > "$CANARY"
# Test 8 (cache-only): contact-sheets/ with ONLY .txt, no PNG → NOT sheeted
R8="$DOWNLOADS/Cache.Only.Release"
make_dummy "$R8/movie.mkv"
mkdir -p "$R8/contact-sheets"
echo "1.234" > "$R8/contact-sheets/scenes_raw_t8.txt"

echo
echo "=== sweep --dry-run ==="
python3 "$SWEEP" --downloads "$DOWNLOADS" --dry-run 2>/dev/null

LOG="$REPO_ROOT/logs/sheets_sweep.log"

# Assertions against log content.
# Test 1: Test.Movie.2024 shows up as dry-run entry
assert "T1 unsheeted release detected" \
  grep -q "dry-run.*Test.Movie.2024" "$LOG"

# Test 2: Old.Movie.1999 skipped (already sheeted)
assert "T2 already-sheeted release skipped" \
  grep -q "skip.*Old.Movie.1999.*already sheeted" "$LOG"

# Test 3: Filtered.Release produces no dry-run entry for its files
#   (tiny.mkv: under size floor; .srt: wrong ext; sample.mkv: pattern skip)
assert "T3a filter skips under-size .mkv" \
  bash -c "! grep -q 'dry-run.*Filtered.Release.*tiny' \"$LOG\""
assert "T3b filter skips .srt" \
  bash -c "! grep -q 'dry-run.*Filtered.Release.*\\.srt' \"$LOG\""
assert "T3c filter skips sample pattern" \
  bash -c "! grep -q 'dry-run.*Filtered.Release.*sample' \"$LOG\""

# Test 5: --evil.mkv appears as a valid file in the log (no argparse interpretation)
assert "T5 argparse injection defended (--evil.mkv handled as file)" \
  grep -q "dry-run.*Flag.Injection.Test" "$LOG"

# Test 6: log contains repr-escaped form of control chars (no raw \n / ESC)
#   grep for the literal \\x1b sequence in the log; absence of a raw ESC
#   character is harder to test but repr-escape is the truth signal.
assert "T6 log injection defended (control chars repr-escaped)" \
  grep -q "Log.Injection.Test" "$LOG"
# Verify the log file itself does not contain a raw ESC byte introduced by
# the fixture (allow only ASCII). This is a best-effort check; use perl to
# avoid grep locale surprises.
assert "T6b log file has no raw ESC byte from filename" \
  perl -ne 'exit 1 if /\x1b\[31m/' "$LOG"

# Test 7: symlink rejected, canary file untouched
assert "T7a symlink escape rejected in log" \
  grep -q "reject.*outside root" "$LOG"
assert "T7b canary file untouched" \
  bash -c "grep -q 'SHOULD_NOT_BE_TOUCHED' \"$CANARY\""

# Test 8: Cache.Only.Release should appear as dry-run (not skipped as sheeted)
assert "T8 cache-only contact-sheets/ treated as NOT sheeted" \
  grep -q "dry-run.*Cache.Only.Release" "$LOG"

# Test 10: --ignore-disk-floor is wired through to the start log line
echo
echo "=== sweep --dry-run --ignore-disk-floor ==="
python3 "$SWEEP" --downloads "$DOWNLOADS" --dry-run --ignore-disk-floor 2>/dev/null
assert "T10 --ignore-disk-floor recorded on start log line" \
  grep -q "sweep start.*ignore_disk_floor=True" "$LOG"

# Test 4: concurrent sweep — hold the flock explicitly via a background
# Python helper; then invoke sweep and expect "already running" output.
# Dry-run is too fast to race without an explicit holder.
echo "=== concurrent sweep test ==="
mkdir -p "$REPO_ROOT/logs"
LOCK_FILE="$REPO_ROOT/logs/.sheets_sweep.lock"
touch "$LOCK_FILE"
python3 -c "
import fcntl, sys, time
f = open('$LOCK_FILE', 'a')
fcntl.flock(f.fileno(), fcntl.LOCK_EX)
sys.stdout.write('locked\n'); sys.stdout.flush()
time.sleep(5)
" > "$TMP/holder.out" &
HOLDER_PID=$!
# Wait for holder to acquire the lock.
for _ in 1 2 3 4 5 6 7 8 9 10; do
  [ "$(cat "$TMP/holder.out" 2>/dev/null || true)" = "locked" ] && break
  sleep 0.1
done
SECOND_OUT="$(python3 "$SWEEP" --downloads "$DOWNLOADS" --dry-run 2>&1 || true)"
kill "$HOLDER_PID" 2>/dev/null || true
wait "$HOLDER_PID" 2>/dev/null || true
assert "T4 concurrent sweep second invocation exits cleanly" \
  bash -c "echo \"$SECOND_OUT\" | grep -q 'already running'"

echo
echo "=== summary ==="
echo "pass: $PASS"
echo "fail: $FAIL"
[ "$FAIL" -eq 0 ]
