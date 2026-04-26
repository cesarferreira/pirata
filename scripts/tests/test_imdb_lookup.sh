#!/usr/bin/env bash
# Smoke test for scripts/imdb_lookup.py (Phase 1, Unit 2).
#
# Reads the live imdb/imdb.db (Unit 1's output) — does NOT modify it.
# Exercises tier-1 exact, tier-2 fuzzy, year+kind filters, multi-tie,
# zero-match, lookup_by_tconst, lookup_episodes, DB-missing error path,
# and the seeded PT-BR fixture (drives R5 calibration).
#
# Run:
#   bash scripts/tests/test_imdb_lookup.sh

set -uo pipefail

REPO=$(cd "$(dirname "$0")/../.." && pwd)
DB="$REPO/imdb/imdb.db"
SCRIPT="$REPO/scripts/imdb_lookup.py"
FIXTURE="$REPO/scripts/tests/fixtures/imdb_pt_br_20.txt"

if [ ! -f "$DB" ]; then
  echo "FATAL: $DB not found — run scripts/imdb_ingest.py --refresh first."
  exit 2
fi

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

run() { python3 "$SCRIPT" "$@" 2>&1; }

# ---------------------------------------------------------------- tier 1 ----
out=$(run "Dune" --year 2021 --kind movie --limit 1)
expect_in "1.tier1 Dune 2021 movie → tt1160419 (aka_regional)" '"tconst": "tt1160419"' "$out"
expect_in "1.tier1 fuzz_ratio=100"                              '"fuzz_ratio": 100'      "$out"

out=$(run "Cidade de Deus" --year 2002 --kind movie --limit 1)
expect_in "2.tier1 Cidade de Deus 2002 → tt0317248 (originalTitle)" '"tconst": "tt0317248"' "$out"
expect_in "2.tier1 field=original"                                  '"field": "original"'   "$out"

out=$(run "Bacurau" --year 2019 --kind movie --limit 1)
expect_in "3.tier1 Bacurau 2019 → tt2762506 (primaryTitle)"  '"tconst": "tt2762506"' "$out"
expect_in "3.tier1 field=primary"                            '"field": "primary"'    "$out"

# ---------------------------------------------------------------- tier 2 ----
out=$(run "Oppenheime" --year 2023 --kind movie --limit 1)
expect_in "4.tier2 typo Oppenheime → tt15398776 (Oppenheimer)" '"tconst": "tt15398776"' "$out"
# fuzz_ratio < 100 → tier 2
expect_not_in "4.tier2 fuzz_ratio < 100"                       '"fuzz_ratio": 100'      "$out"

# ---------------------------------------------------- year + kind filtering ---
out=$(run "Dune" --year 1984 --kind movie --limit 1)
expect_in "5.year filter Dune 1984 → tt0087182 (Lynch)" '"tconst": "tt0087182"' "$out"

# Without --kind, primaryTitle "Dune" tvEpisodes (2021) outrank tt1160419 (which
# matches via aka_regional, score 150). tt1160419 still appears with --kind movie.
out=$(run "Dune" --year 2021 --kind movie --limit 5)
expect_in "6.year+kind movie 2021 contains tt1160419" '"tconst": "tt1160419"' "$out"

# ------------------------------------------------------------- zero match ---
out=$(run "aslkdjfaslkdjf" --limit 1)
if [ -z "$out" ]; then pass "7.zero-match returns empty"; else fail "7.zero-match returned: $out"; fi

# ----------------------------------------------------------------- multi-tie ---
out=$(run "Dune" --kind movie --limit 5)
top=$(printf '%s' "$out" | head -n1)
expect_in "8.multi-tie Dune (no year) → top1 multi_tie=true" '"multi_tie": true' "$top"

# ----------------------------------------------------------- lookup_by_tconst ---
out=$(run --tconst tt0317248)
expect_in "9.tconst movie primary"   '"primary_title": "City of God"' "$out"
expect_in "9.tconst movie original"  '"original_title": "Cidade de Deus"' "$out"
expect_in "9.tconst movie genres"    '"genres":'                     "$out"
expect_in "9.tconst movie top_cast"  '"top_cast":'                   "$out"
expect_in "9.tconst movie rating"    '"average_rating": 8.6'         "$out"

# series (tvSeries titleType): top_cast comes from series_top_cast
# Stable choice: pick a series with episodes from title_episode
SERIES=$(sqlite3 "$DB" "
  SELECT b.tconst FROM title_basics b
  JOIN series_top_cast c ON c.parent_tconst = b.tconst
  JOIN title_ratings r ON r.tconst = b.tconst
  WHERE b.titleType IN ('tvSeries','tvMiniSeries') AND r.numVotes > 100000
  ORDER BY r.numVotes DESC LIMIT 1
")
out=$(run --tconst "$SERIES")
expect_in "10.tconst series titleType"  '"title_type": "tv'   "$out"
expect_in "10.tconst series top_cast"   '"top_cast":'         "$out"

# unknown tconst → exit 1
out=$(run --tconst tt99999999 2>&1) ; rc=$?
[ "$rc" -eq 1 ] && pass "11.unknown tconst exit 1" || fail "11.unknown tconst exit was $rc"

# ----------------------------------------------------------- lookup_episodes ---
out=$(run --episodes "$SERIES" --season 1 | head -3)
[ -n "$out" ] && pass "12.episodes season 1 non-empty" || fail "12.episodes season 1 empty for $SERIES"

# ------------------------------------------------------------ DB unavailable ---
out=$(run "Dune" --db /tmp/nx_imdb_test.db 2>&1) ; rc=$?
expect_in "13.DB missing → IMDbDBUnavailable" 'IMDbDBUnavailable' "$out"
[ "$rc" -ne 0 ] && pass "13.DB missing exit non-zero (was $rc)" || fail "13.DB missing exit was 0"

# ------------------------------------------------------ FTS5 operator stripping ---
# Caller (Unit 3 PTT extractor) strips year before lookup; punctuation may remain.
# _build_fts_query must strip FTS5 operators without crashing AND still resolve
# the canonical match. Year-as-token in the query string is out of scope (PTT
# pulls year into the year arg).
out=$(run 'Dune: Part Two!' --year 2024 --kind movie --limit 1 2>&1)
expect_in "14.fts5 operators stripped → tt15239678" '"tconst": "tt15239678"' "$out"

# ------------------------------------------------------------------ fixture ---
echo "=== PT-BR fixture validation (calibration; failures = signals) ==="
PYTHONPATH="$REPO" python3 -u <<PY
import sys
from pathlib import Path
sys.path.insert(0, "$REPO")
from scripts.imdb_lookup import lookup_by_title

fix = Path("$FIXTURE").read_text().splitlines()
ok = 0
miss = []
total = 0
for raw in fix:
    line = raw.split("#", 1)[0].rstrip()
    if not line.strip():
        continue
    parts = line.split("\t")
    if len(parts) < 4:
        continue
    title, year_s, kind_s, expected = parts[0].strip(), parts[1].strip(), parts[2].strip(), parts[3].strip()
    year = int(year_s) if year_s and year_s != "-" else None
    kind = kind_s if kind_s and kind_s != "-" else None
    total += 1
    try:
        m = lookup_by_title(title, year=year, kind=kind)
    except Exception as e:
        miss.append(f"{title} ({year}|{kind}) expected={expected} ERROR={e}")
        continue
    if not m:
        miss.append(f"{title} ({year}|{kind}) expected={expected} got=EMPTY")
        continue
    top = m[0]
    if top.tconst == expected and top.fuzz_ratio >= 80:
        ok += 1
    else:
        miss.append(f"{title} ({year}|{kind}) expected={expected} got={top.tconst} field={top.field} fuzz={top.fuzz_ratio:.1f} score={top.score:.1f} multi_tie={top.multi_tie}")

print(f"FIXTURE: {ok}/{total} ok, {len(miss)} miss")
for m in miss:
    print(f"  MISS: {m}")
# Calibration phase: don't hard-fail unless <50% pass.
if total == 0:
    sys.exit(2)  # no fixture entries
threshold = total * 0.5
sys.exit(0 if ok >= threshold else 3)
PY
fix_rc=$?
case "$fix_rc" in
  0) pass "15.fixture (>=50% ok at calibration phase)" ;;
  2) fail "15.fixture has zero entries" ;;
  3) fail "15.fixture <50% pass — formula needs revision" ;;
  *) fail "15.fixture runner exited $fix_rc" ;;
esac

# ---------------------------------------------------------------- summary ---
echo
echo "=== summary ==="
echo "PASS: $PASS"
echo "FAIL: $FAIL"
[ "$FAIL" -eq 0 ] && exit 0 || exit 1
