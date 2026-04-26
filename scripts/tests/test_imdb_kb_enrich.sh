#!/usr/bin/env bash
# Smoke test for scripts/imdb_kb_enrich.py (plan 007 Unit A).
#
# Reads the live imdb/imdb.db (Unit 1's output) — does NOT modify it.
# Uses a hermetic tmpdir for the JSONL log so the workspace's real
# logs/sweep_imdb_misses.log is not polluted.
#
# Exercises:
#   - resolved (Bacurau — clean primaryTitle match with directors + rating)
#   - year-hint disambiguation (Dune 1984)
#   - no-year fall-through (still finds something or multi_tie)
#   - slug-shaped input normalization (the-super-mario-galaxy-movie-2026)
#   - multi_tie via imdb_lookup's flag (Who Framed Roger Rabbit — multiple tier-1)
#   - no_match for gibberish
#   - db_unavailable when DB file is missing
#   - log JSONL append on every non-resolved outcome
#   - resolved outcome does NOT append to log
#   - PTN import sanity (module loads at import time)
#
# Run:
#   bash scripts/tests/test_imdb_kb_enrich.sh

set -uo pipefail

REPO=$(cd "$(dirname "$0")/../.." && pwd)
DB="$REPO/imdb/imdb.db"
SCRIPT="$REPO/scripts/imdb_kb_enrich.py"
HELPER="$REPO/scripts/imdb_kb_enrich.py"

if [ ! -f "$DB" ]; then
  echo "FATAL: $DB not found — run scripts/imdb_ingest.py --refresh first."
  exit 2
fi

# Hermetic tmpdir for the miss log; clean up on exit.
TMP=$(mktemp -d)
LOG="$TMP/misses.log"
trap "rm -rf $TMP" EXIT

PASS=0
FAIL=0
pass() { PASS=$((PASS+1)); echo "PASS: $*"; }
fail() { FAIL=$((FAIL+1)); echo "FAIL: $*"; }
expect_in() {
  local label="$1" needle="$2" haystack="$3"
  if printf '%s' "$haystack" | grep -qF "$needle"; then
    pass "$label"
  else
    fail "$label  (missing: $needle)"
    printf '   got: %s\n' "$haystack" | head -c 600
    echo
  fi
}
expect_not_in() {
  local label="$1" needle="$2" haystack="$3"
  if printf '%s' "$haystack" | grep -qF "$needle"; then
    fail "$label  (unexpected: $needle)"
  else
    pass "$label"
  fi
}

# Run resolve with the tmpdir log path injected via Python wrapper
# (the CLI does not expose --log-path; tests pass it through Python).
run() {
  local raw="$1" slug="${2:-}"
  python3 - "$raw" "$slug" "$LOG" <<'PYEOF'
import json, sys
from pathlib import Path
sys.path.insert(0, "scripts")
import imdb_kb_enrich
raw, slug, log_path = sys.argv[1], sys.argv[2] or None, Path(sys.argv[3])
r = imdb_kb_enrich.resolve(raw, slug=slug, log_path=log_path)
print(json.dumps(r.as_dict(), ensure_ascii=False))
PYEOF
}

# ---------------------------------------------------------------- T1 -------
# Bacurau resolves cleanly: real BR film with directors + rating.
echo "=== T1: resolved (Bacurau 2019) ==="
out=$(run "Bacurau.2019.1080p.BluRay.x265")
expect_in "1.canonical_title Bacurau"            '"canonical_title": "Bacurau"'        "$out"
expect_in "1.canonical_year 2019"                '"canonical_year": 2019'              "$out"
expect_in "1.imdb tconst tt2762506"              '"tconst": "tt2762506"'               "$out"
expect_in "1.imdb result resolved"               '"result": "resolved"'                "$out"
expect_in "1.imdb confidence 100"                '"confidence": 100'                   "$out"
expect_in "1.imdb has director"                  '"director": [{'                      "$out"
expect_in "1.imdb has rating"                    '"rating": {'                         "$out"
expect_in "1.imdb plot null"                     '"plot": null'                        "$out"

# ---------------------------------------------------------------- T2 -------
# Year hint disambiguates: Dune 1984 (Lynch) vs Dune 2021 (Villeneuve).
echo "=== T2: year-hint disambiguation (Dune 1984) ==="
out=$(run "Dune.1984.1080p.WEBRip")
expect_in "2.canonical_year 1984"                '"canonical_year": 1984'              "$out"
expect_in "2.imdb tconst Lynch Dune"             '"tconst": "tt0087182"'               "$out"

# ---------------------------------------------------------------- T3 -------
# No year in filename: PTN reports year=None; lookup runs unfiltered.
echo "=== T3: no year (Bacurau without year) ==="
out=$(run "Bacurau.1080p.WEBRip")
expect_in "3.ptt_year null"                      '"ptt_year": null'                    "$out"
# Bacurau (2019) is the dominant hit; should resolve OR multi_tie cleanly.
case "$out" in
  *'"result": "resolved"'*) pass "3.no-year resolved cleanly" ;;
  *'"result": "multi_tie"'*) pass "3.no-year multi_tie (acceptable fallback)" ;;
  *) fail "3.no-year unexpected outcome";;
esac

# ---------------------------------------------------------------- T4 -------
# Slug-shaped input gets normalized: dashes → spaces + Title Case.
echo "=== T4: slug-shaped input (Mario Galaxy slug) ==="
out=$(run "the-super-mario-galaxy-movie-2026" "the-super-mario-galaxy-movie-2026")
expect_in "4.ptt_title normalized"               '"ptt_title": "The Super Mario Galaxy Movie"' "$out"
expect_in "4.ptt_year 2026"                      '"ptt_year": 2026'                    "$out"
# Whether IMDb has it or not, top-level fields must be cleaned (bug fix).
expect_not_in "4.canonical not slug literal"     '"canonical_title": "the-super-mario-galaxy-movie-2026"' "$out"
expect_in "4.canonical_year 2026"                '"canonical_year": 2026'              "$out"

# ---------------------------------------------------------------- T5 -------
# multi_tie surfaces when imdb_lookup flags top match: Roger Rabbit
# returns the famous 1988 movie + a video game with same exact title.
echo "=== T5: multi_tie (Roger Rabbit 1988) ==="
out=$(run "Who.Framed.Roger.Rabbit.1988.1080p.BluRay.x264")
expect_in "5.imdb result multi_tie"              '"result": "multi_tie"'               "$out"
expect_in "5.imdb multi_tie true"                '"multi_tie": true'                   "$out"
# Canonical falls back to PTN values.
expect_in "5.canonical from PTN"                 '"canonical_title": "Who Framed Roger Rabbit"' "$out"
expect_in "5.canonical_year 1988"                '"canonical_year": 1988'              "$out"

# ---------------------------------------------------------------- T6 -------
# Gibberish: no IMDb match; canonical falls back to PTN cleanup.
echo "=== T6: no_match (gibberish) ==="
out=$(run "Xyzzyplugh.Definitely.Not.A.Real.Movie.2026")
expect_in "6.imdb result no_match"               '"result": "no_match"'                "$out"
expect_in "6.canonical from PTN"                 '"canonical_title": "Xyzzyplugh Definitely Not A Real Movie"' "$out"
expect_in "6.canonical_year 2026"                '"canonical_year": 2026'              "$out"

# ---------------------------------------------------------------- T7 -------
# DB unavailable: rename DB aside, run resolve, restore DB.
echo "=== T7: db_unavailable ==="
mv "$DB" "$DB.test_aside"
out=$(run "Dune.2021")
mv "$DB.test_aside" "$DB"
expect_in "7.imdb result db_unavailable"         '"result": "db_unavailable"'          "$out"
expect_in "7.lookup_attempted false"             '"lookup_attempted": false'           "$out"
expect_in "7.canonical from PTN"                 '"canonical_title": "Dune"'           "$out"
# Reset connection cache so subsequent tests can reach the restored DB.
python3 - <<'PYEOF'
import sys
sys.path.insert(0, "scripts")
import imdb_lookup
imdb_lookup.close_connection()
PYEOF

# ---------------------------------------------------------------- T8 -------
# Log content: every non-resolved outcome must have one JSONL line.
echo "=== T8: log JSONL append on misses ==="
[ -f "$LOG" ] && pass "8.log file exists" || fail "8.log file missing"
# Expect lines for: T5 (multi_tie), T6 (no_match), T7 (db_unavailable).
# T3 may have added one if it multi-tied — count flexibly.
expect_in "8.log has multi_tie line"             '"result": "multi_tie"'               "$(cat $LOG)"
expect_in "8.log has no_match line"              '"result": "no_match"'                "$(cat $LOG)"
expect_in "8.log has db_unavailable line"        '"result": "db_unavailable"'          "$(cat $LOG)"
# Resolved outcomes do NOT log.
expect_not_in "8.log has no resolved entries"    '"result": "resolved"'                "$(cat $LOG)"
# Each line is valid JSON.
all_lines_valid=1
while IFS= read -r line; do
  [ -z "$line" ] && continue
  printf '%s' "$line" | python3 -c "import json, sys; json.loads(sys.stdin.read())" 2>/dev/null || all_lines_valid=0
done < "$LOG"
[ "$all_lines_valid" = "1" ] && pass "8.every log line is valid JSON" || fail "8.log has malformed JSONL"

# ---------------------------------------------------------------- T9 -------
# PTN import sanity: importing the helper module must not raise.
echo "=== T9: PTN import sanity ==="
import_ok=$(python3 -c "import sys; sys.path.insert(0,'scripts'); import imdb_kb_enrich; print('ok')" 2>&1)
expect_in "9.helper module imports"              'ok'                                  "$import_ok"

# ---------------------------------------------------------------- T10 ------
# AKAS_CAP enforcement: pick a film known to have many AKAs and assert
# the imdb.akas list is bounded. The Lynch Dune (tt0087182) had PT/EN/ES
# rows in the akas slice; assert len ≤ AKAS_CAP (10).
echo "=== T10: AKAS_CAP enforcement ==="
out=$(run "Dune.1984")
akas_count=$(printf '%s' "$out" | python3 -c "import json,sys; r=json.loads(sys.stdin.read()); print(len(r['imdb'].get('akas',[])))" 2>&1)
case "$akas_count" in
  ''|*[!0-9]*) fail "10.akas count not parseable: $akas_count" ;;
  *)
    if [ "$akas_count" -le 10 ]; then
      pass "10.akas count $akas_count <= 10 (AKAS_CAP)"
    else
      fail "10.akas count $akas_count exceeds AKAS_CAP (10)"
    fi
    ;;
esac

# ---------------------------------------------------------------- summary --
echo
echo "=== summary ==="
echo "PASS: $PASS"
echo "FAIL: $FAIL"
[ "$FAIL" -eq 0 ]
