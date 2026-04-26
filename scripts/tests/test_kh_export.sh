#!/usr/bin/env bash
# Smoke test for scripts/build_kh_export.py.
#
# Builds the kh-export against the live kb/ and validates every requirement
# from docs/plans/2026-04-25-005-feat-kh-export-surface-plan.md (R1-R8) plus
# idempotency. Read-only with respect to kb/per-movie/ and kb/manifest.jsonl;
# the only mutated path is kb/kh-export/.
#
# Run:
#   bash scripts/tests/test_kh_export.sh

set -uo pipefail

REPO=$(cd "$(dirname "$0")/../.." && pwd)
KB="$REPO/kb"
EXPORT="$REPO/kb/kh-export"
SCRIPT="$REPO/scripts/build_kh_export.py"

PER_MOVIE_SRC="$KB/per-movie/who-framed-roger-rabbit-1988.json"
PER_MOVIE_DST_JSON="$EXPORT/04-derived/per-movie/who-framed-roger-rabbit-1988.json"
PER_MOVIE_DST_MD="$EXPORT/04-derived/per-movie/who-framed-roger-rabbit-1988.md"
MG_DST_MD="$EXPORT/04-derived/per-movie/the-super-mario-galaxy-movie-2026.md"
MANIFEST_SRC="$KB/manifest.jsonl"
MANIFEST_DST="$EXPORT/04-derived/manifest.json"
README_DST="$EXPORT/04-derived/README.md"

if [ ! -f "$SCRIPT" ]; then
  echo "FATAL: $SCRIPT not found"
  exit 2
fi
if [ ! -f "$PER_MOVIE_SRC" ]; then
  echo "FATAL: source per-movie JSON missing at $PER_MOVIE_SRC"
  exit 2
fi
if [ ! -f "$MANIFEST_SRC" ]; then
  echo "FATAL: source manifest.jsonl missing at $MANIFEST_SRC"
  exit 2
fi

PASS=0
FAIL=0
pass() { PASS=$((PASS+1)); echo "PASS: $*"; }
fail() { FAIL=$((FAIL+1)); echo "FAIL: $*"; }

# Capture pre-run checksum of the canonical jsonl (must remain unchanged).
ORIG_JSONL_SHA=$(shasum -a 256 "$MANIFEST_SRC" | awk '{print $1}')

# ----------------------------------------------------------------- run 1 ---
echo "=== run 1: build against live kb/ ==="
python3 "$SCRIPT" > "$REPO/.kh_export_run1.log" 2>&1
rc=$?
if [ "$rc" -eq 0 ]; then pass "01.run 1 exits 0"; else fail "01.run 1 exit was $rc (see .kh_export_run1.log)"; fi

# Layout assertions
[ -d "$EXPORT/04-derived" ] && pass "02.04-derived/ exists" || fail "02.04-derived/ missing"
[ -d "$EXPORT/04-derived/per-movie" ] && pass "03.per-movie/ exists" || fail "03.per-movie/ missing"
[ -f "$PER_MOVIE_DST_JSON" ] && pass "04.per-movie JSON exported" || fail "04.per-movie JSON missing"
[ -f "$PER_MOVIE_DST_MD" ] && pass "05.per-movie MD exported" || fail "05.per-movie MD missing"
[ -f "$MANIFEST_DST" ] && pass "06.manifest.json exported" || fail "06.manifest.json missing"
[ -f "$README_DST" ] && pass "07.README.md exported" || fail "07.README.md missing"

# JSON copy must be byte-identical to source
if cmp -s "$PER_MOVIE_SRC" "$PER_MOVIE_DST_JSON"; then
  pass "08.per-movie JSON is byte-identical to source"
else
  fail "08.per-movie JSON differs from source"
fi

# JSON parses
if python3 -c "import json,sys; json.load(open(sys.argv[1]))" "$PER_MOVIE_DST_JSON" 2>/dev/null; then
  pass "09.per-movie JSON parses"
else
  fail "09.per-movie JSON does not parse"
fi

# Markdown wrapper contains all 4 required literal strings
for needle in "Roger Rabbit" "Who Framed Roger Rabbit (1988)" "who-framed-roger-rabbit-1988" "scdet"; do
  if grep -qF "$needle" "$PER_MOVIE_DST_MD"; then
    pass "10.MD contains literal: $needle"
  else
    fail "10.MD missing literal: $needle"
  fi
done

# manifest.json parses, has expected slug-grouped schema and row count
EXPECTED_ROWS=$(grep -c . "$MANIFEST_SRC")
if python3 -c "
import json, sys
m = json.load(open(sys.argv[1]))
assert m['source'] == 'kb/manifest.jsonl', f'source mismatch: {m[\"source\"]}'
assert m['kind'] == 'frame_manifest', f'kind mismatch: {m[\"kind\"]}'
assert isinstance(m['slugs'], dict), 'slugs is not a dict'
assert m['slug_count'] == len(m['slugs']), f'slug_count {m[\"slug_count\"]} != len(slugs) {len(m[\"slugs\"])}'
total = sum(len(s['rows']) for s in m['slugs'].values())
assert m['row_count'] == total, f'row_count {m[\"row_count\"]} != sum(len(rows)) {total}'
assert m['row_count'] == int(sys.argv[2]), f'row_count {m[\"row_count\"]} != expected {sys.argv[2]}'
required = {'title','year','frame_count','first_tc','last_tc','first_t_s','last_t_s','rows'}
for slug, s in m['slugs'].items():
    missing = required - set(s.keys())
    assert not missing, f'slug {slug} missing fields: {missing}'
    assert s['frame_count'] == len(s['rows']), f'slug {slug} frame_count != len(rows)'
" "$MANIFEST_DST" "$EXPECTED_ROWS" 2>/dev/null; then
  pass "11.manifest.json schema + row_count=$EXPECTED_ROWS"
else
  fail "11.manifest.json schema/row_count mismatch (expected $EXPECTED_ROWS)"
fi

# Mario Galaxy wrapper exists and surfaces the metadata gaps
if [ -f "$MG_DST_MD" ]; then
  pass "11a.Mario Galaxy wrapper exported"
else
  fail "11a.Mario Galaxy wrapper missing at $MG_DST_MD"
fi
if grep -qF "the-super-mario-galaxy-movie-2026" "$MG_DST_MD" 2>/dev/null; then
  pass "11b.Mario Galaxy wrapper contains slug literal"
else
  fail "11b.Mario Galaxy wrapper missing slug literal"
fi
if grep -qF "matches the slug" "$MG_DST_MD" 2>/dev/null; then
  pass "11c.Mario Galaxy wrapper notes title-equals-slug caveat"
else
  fail "11c.Mario Galaxy wrapper missing title-equals-slug caveat"
fi
if grep -qF "Year is null" "$MG_DST_MD" 2>/dev/null; then
  pass "11d.Mario Galaxy wrapper notes null-year caveat"
else
  fail "11d.Mario Galaxy wrapper missing null-year caveat"
fi

# README is non-empty
if [ -s "$README_DST" ]; then pass "12.README is non-empty"; else fail "12.README is empty"; fi

# No JPGs anywhere in kh-export
JPG_COUNT=$(find "$EXPORT" -type f \( -iname '*.jpg' -o -iname '*.jpeg' \) 2>/dev/null | wc -l | tr -d ' ')
if [ "$JPG_COUNT" = "0" ]; then
  pass "13.no JPGs in kh-export"
else
  fail "13.found $JPG_COUNT JPG(s) in kh-export"
fi

# Original kb/manifest.jsonl unchanged
NEW_JSONL_SHA=$(shasum -a 256 "$MANIFEST_SRC" | awk '{print $1}')
if [ "$ORIG_JSONL_SHA" = "$NEW_JSONL_SHA" ]; then
  pass "14.original kb/manifest.jsonl unchanged"
else
  fail "14.original kb/manifest.jsonl was mutated (sha drift)"
fi

# Tmp dir was cleaned up (no kb/kh-export.tmp/ left over)
if [ ! -d "$KB/kh-export.tmp" ]; then
  pass "15.kb/kh-export.tmp cleaned up"
else
  fail "15.kb/kh-export.tmp still present after run"
fi

# ----------------------------------------------------------------- run 2 ---
echo "=== run 2: idempotency check ==="
SHA_AFTER_RUN1=$(find "$EXPORT" -type f | sort | xargs shasum -a 256 | shasum -a 256 | awk '{print $1}')
python3 "$SCRIPT" > "$REPO/.kh_export_run2.log" 2>&1
rc=$?
if [ "$rc" -eq 0 ]; then pass "16.run 2 exits 0"; else fail "16.run 2 exit was $rc"; fi
SHA_AFTER_RUN2=$(find "$EXPORT" -type f | sort | xargs shasum -a 256 | shasum -a 256 | awk '{print $1}')
if [ "$SHA_AFTER_RUN1" = "$SHA_AFTER_RUN2" ]; then
  pass "17.idempotent: tree checksum stable across runs"
else
  fail "17.tree drift between run 1 and run 2 ($SHA_AFTER_RUN1 vs $SHA_AFTER_RUN2)"
fi

# ----------------------------------------------------------------- run 3 ---
# Stale tmp recovery: pre-create kb/kh-export.tmp/ and verify cleanup.
echo "=== run 3: stale tmp recovery ==="
mkdir -p "$KB/kh-export.tmp/garbage"
echo "leftover" > "$KB/kh-export.tmp/garbage/junk.txt"
python3 "$SCRIPT" > "$REPO/.kh_export_run3.log" 2>&1
rc=$?
if [ "$rc" -eq 0 ]; then pass "18.run 3 (stale tmp) exits 0"; else fail "18.run 3 exit was $rc"; fi
if [ ! -d "$KB/kh-export.tmp" ]; then
  pass "19.stale tmp removed"
else
  fail "19.stale tmp still present"
fi
if [ ! -f "$EXPORT/04-derived/garbage/junk.txt" ]; then
  pass "20.no junk leaked from stale tmp"
else
  fail "20.junk from stale tmp leaked into final export"
fi

# ----------------------------------------------------------------- run 4 ---
# Custom --out smoke (writes to a tmp scratch dir; verifies CLI plumbing).
echo "=== run 4: custom --out ==="
SCRATCH=$(mktemp -d -t khexport-scratch.XXXXXX)
trap 'rm -rf "$SCRATCH"' EXIT
python3 "$SCRIPT" --out "$SCRATCH/x" > "$REPO/.kh_export_run4.log" 2>&1
rc=$?
if [ "$rc" -eq 0 ]; then pass "21.run 4 (custom --out) exits 0"; else fail "21.run 4 exit was $rc"; fi
if [ -f "$SCRATCH/x/04-derived/manifest.json" ]; then
  pass "22.custom --out produced manifest.json"
else
  fail "22.custom --out missing manifest.json"
fi

# ----------------------------------------------------------------- summary ---
rm -f "$REPO/.kh_export_run1.log" "$REPO/.kh_export_run2.log" "$REPO/.kh_export_run3.log" "$REPO/.kh_export_run4.log"

echo
echo "=== summary ==="
echo "PASS: $PASS"
echo "FAIL: $FAIL"
[ "$FAIL" -eq 0 ] && exit 0 || exit 1
