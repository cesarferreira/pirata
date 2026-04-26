#!/usr/bin/env bash
# Smoke test for scripts/queue.py loose-video wrap helpers.
#
# Hermetic: uses a tmpdir; calls only the helper functions via python -c.
# Does not invoke aria2c.
#
# Covers:
#   1. snapshot_loose_videos finds top-level video files only
#   2. wrap_loose_videos moves each loose video into <stem>/<name>/
#   3. Idempotence: after wrap, no loose videos remain at root
#   4. Collision-safe: pre-existing <stem>/ dir blocks the wrap
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
QUEUE="$REPO_ROOT/scripts/queue.py"
TMP="$(mktemp -d -t pirata-queue-test-XXXXXX)"
PASS=0
FAIL=0

cleanup() { rm -rf "$TMP"; }
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

# Helper: invoke a snippet against scripts/queue.py with sys.path prepended
# so the module imports cleanly (and stdlib `queue` stays accessible because
# queue.py itself does not depend on stdlib `queue`).
pyrun() {
  python3 -c "
import sys
sys.path.insert(0, '$REPO_ROOT/scripts')
import queue as _q
$1
"
}

# Setup: 2 loose videos, 1 non-video, 1 file inside an existing dir.
touch "$TMP/movie.mkv" "$TMP/show.mp4" "$TMP/notes.txt"
mkdir "$TMP/already-a-dir"
touch "$TMP/already-a-dir/inner.mkv"

# Test 1: snapshot finds 2 loose videos (skips notes.txt and dir-internal mkv)
COUNT="$(pyrun "
from pathlib import Path
print(len(_q.snapshot_loose_videos(Path('$TMP'))))
")"
assert "T1 snapshot finds 2 loose videos at root" \
  test "$COUNT" = "2"

# Test 2: wrap moves each loose video into <stem>/<name>/
pyrun "
from pathlib import Path
videos = _q.snapshot_loose_videos(Path('$TMP'))
_q.wrap_loose_videos(videos)
" > /dev/null
assert "T2a movie.mkv landed in movie/" \
  test -f "$TMP/movie/movie.mkv"
assert "T2b show.mp4 landed in show/" \
  test -f "$TMP/show/show.mp4"
assert "T2c root no longer holds movie.mkv" \
  test ! -f "$TMP/movie.mkv"
assert "T2d notes.txt untouched at root" \
  test -f "$TMP/notes.txt"

# Test 3: idempotence — second snapshot returns 0 loose videos
COUNT2="$(pyrun "
from pathlib import Path
print(len(_q.snapshot_loose_videos(Path('$TMP'))))
")"
assert "T3 0 loose videos remain after wrap" \
  test "$COUNT2" = "0"

# Test 4: collision-safe — pre-existing dir blocks wrap, file stays loose
TMP2="$(mktemp -d -t pirata-queue-collision-XXXXXX)"
touch "$TMP2/clash.mkv"
mkdir "$TMP2/clash"  # collision target already present
COLLIDE_OUT="$(pyrun "
from pathlib import Path
videos = _q.snapshot_loose_videos(Path('$TMP2'))
_q.wrap_loose_videos(videos)
" 2>&1)"
assert "T4a collision warned to stderr" \
  bash -c "echo \"$COLLIDE_OUT\" | grep -q 'cannot wrap clash.mkv'"
assert "T4b clash.mkv stayed loose at root" \
  test -f "$TMP2/clash.mkv"
rm -rf "$TMP2"

echo
echo "=== summary ==="
echo "pass: $PASS"
echo "fail: $FAIL"
[ "$FAIL" -eq 0 ]
