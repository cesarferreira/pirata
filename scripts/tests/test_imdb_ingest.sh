#!/usr/bin/env bash
# Hermetic smoke test for scripts/imdb_ingest.py.
#
# Strategy: synthesize tiny fixture TSVs via printf with tab separators
# (no real IMDb dump needed), run the ingest in a tmpdir-scoped DB, and
# assert specific facts about the resulting SQLite via sqlite3 CLI.
#
# Asserts (12 scenarios):
#   1. happy path: build succeeds, all expected tables exist
#   2. row counts: title_basics/ratings/episode/crew/akas/names/principals
#   3. \N → NULL: tt003 has startYear IS NULL in title_basics
#   4. cast filter: director/writer NOT in title_principals_top5
#   5. top-5 cap: tconst with 6 actors → only 5 rows stored
#   6. series_top_cast: tt005 entry has nm010 with count=2 (across eps)
#   7. akas filter: DE row dropped; FR isOriginal=1 kept; BR/PT/EN kept
#   8. principal name denorm: nm010 has name="Actor Top" populated
#   9. FTS5 populated: ft_titles has rows from primary/original/aka
#  10. state.json: parses and has schema_version + last_refresh_finished_at
#  11. integrity_check: PRAGMA integrity_check returns 'ok'
#  12. idempotency: second --refresh produces imdb.db.prev with first DB
#  13. precheck: --min-free-gb=99999 fails fast (exit 2)
#  14. sort violation: interleaved tconsts in principals → abort (exit 3)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
INGEST="$REPO_ROOT/scripts/imdb_ingest.py"
TMP="$(mktemp -d -t pirata-imdb-test-XXXXXX)"
SRC="$TMP/src"
DB_DIR="$TMP/db"
DB="$DB_DIR/imdb.db"
STATE="$DB_DIR/state.json"
LOCK="$DB_DIR/.refresh.lock"
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

mkdir -p "$SRC" "$DB_DIR"

# ---------------------------------------------------------------------------
# Fixture generation — 5 titles, 1 series with 2 eps, sample principals/akas.
# ---------------------------------------------------------------------------

# tab-separated; \N is the IMDb NULL sentinel literal
TAB=$'\t'
NL=$'\n'

cat > "$SRC/title.basics.tsv" <<EOF
tconst${TAB}titleType${TAB}primaryTitle${TAB}originalTitle${TAB}isAdult${TAB}startYear${TAB}endYear${TAB}runtimeMinutes${TAB}genres
tt001${TAB}movie${TAB}A Test Movie${TAB}A Test Movie${TAB}0${TAB}2024${TAB}\N${TAB}120${TAB}Drama
tt002${TAB}movie${TAB}Brazilian Title${TAB}Brazilian Title${TAB}0${TAB}2023${TAB}\N${TAB}95${TAB}Drama,Crime
tt003${TAB}short${TAB}A Curta Sem Ano${TAB}A Curta Sem Ano${TAB}0${TAB}\N${TAB}\N${TAB}\N${TAB}Short
tt004${TAB}movie${TAB}Le Titre${TAB}Le Titre${TAB}0${TAB}2022${TAB}\N${TAB}88${TAB}Drama
tt005${TAB}tvSeries${TAB}Test Series${TAB}Test Series${TAB}0${TAB}2020${TAB}2022${TAB}45${TAB}Drama
tt006${TAB}tvEpisode${TAB}Test Series Ep1${TAB}Test Series Ep1${TAB}0${TAB}2020${TAB}\N${TAB}45${TAB}Drama
tt007${TAB}tvEpisode${TAB}Test Series Ep2${TAB}Test Series Ep2${TAB}0${TAB}2020${TAB}\N${TAB}45${TAB}Drama
EOF

cat > "$SRC/title.ratings.tsv" <<EOF
tconst${TAB}averageRating${TAB}numVotes
tt001${TAB}7.5${TAB}1000
tt002${TAB}8.0${TAB}5000
tt003${TAB}6.0${TAB}50
tt005${TAB}9.0${TAB}20000
EOF

cat > "$SRC/title.episode.tsv" <<EOF
tconst${TAB}parentTconst${TAB}seasonNumber${TAB}episodeNumber
tt006${TAB}tt005${TAB}1${TAB}1
tt007${TAB}tt005${TAB}1${TAB}2
EOF

cat > "$SRC/title.crew.tsv" <<EOF
tconst${TAB}directors${TAB}writers
tt001${TAB}nm001${TAB}nm002
tt002${TAB}nm003${TAB}\N
tt005${TAB}nm004${TAB}nm005
EOF

# title.principals: must be sorted by tconst (R2 invariant the script enforces).
# tt001 has 6 actor/actress + a director (filter test) + a writer (filter test).
# Expected: top-5 cast kept (nm010, nm011, nm013, nm014, nm015 — by ordering),
# nm016 dropped (cap), nm001/nm002/nm012 filtered (non-cast categories).
cat > "$SRC/title.principals.tsv" <<EOF
tconst${TAB}ordering${TAB}nconst${TAB}category${TAB}job${TAB}characters
tt001${TAB}1${TAB}nm001${TAB}director${TAB}\N${TAB}\N
tt001${TAB}2${TAB}nm010${TAB}actor${TAB}\N${TAB}["Lead"]
tt001${TAB}3${TAB}nm011${TAB}actress${TAB}\N${TAB}["Co-Lead"]
tt001${TAB}4${TAB}nm012${TAB}writer${TAB}\N${TAB}\N
tt001${TAB}5${TAB}nm013${TAB}actor${TAB}\N${TAB}["Sup1"]
tt001${TAB}6${TAB}nm014${TAB}actor${TAB}\N${TAB}["Sup2"]
tt001${TAB}7${TAB}nm015${TAB}actor${TAB}\N${TAB}["Sup3"]
tt001${TAB}8${TAB}nm016${TAB}actor${TAB}\N${TAB}["Sup4"]
tt002${TAB}1${TAB}nm020${TAB}actor${TAB}\N${TAB}["Hero"]
tt005${TAB}1${TAB}nm004${TAB}director${TAB}\N${TAB}\N
tt006${TAB}1${TAB}nm010${TAB}actor${TAB}\N${TAB}["Eq"]
tt006${TAB}2${TAB}nm011${TAB}actress${TAB}\N${TAB}["Eq2"]
tt006${TAB}3${TAB}nm012${TAB}actor${TAB}\N${TAB}["Sup"]
tt007${TAB}1${TAB}nm010${TAB}actor${TAB}\N${TAB}["Eq"]
tt007${TAB}2${TAB}nm011${TAB}actress${TAB}\N${TAB}["Eq2"]
tt007${TAB}3${TAB}nm013${TAB}actor${TAB}\N${TAB}["Sup3"]
EOF

# title.akas: filter test
#   - tt001: en lang (KEEP); isOriginal=1 (KEEP)
#   - tt002: BR region (KEEP) x2; isOriginal=1 + en (KEEP)
#   - tt003: DE region + de lang (DROP — neither in regions/langs nor isOriginal)
#   - tt004: FR isOriginal=1 (KEEP); FR non-original (DROP)
#   - tt005: en isOriginal=1 (KEEP)
# Expected: 7 rows kept, 2 dropped.
cat > "$SRC/title.akas.tsv" <<EOF
titleId${TAB}ordering${TAB}title${TAB}region${TAB}language${TAB}types${TAB}attributes${TAB}isOriginalTitle
tt001${TAB}1${TAB}A Test Movie${TAB}US${TAB}en${TAB}original${TAB}\N${TAB}1
tt001${TAB}2${TAB}A Test Movie${TAB}\N${TAB}\N${TAB}\N${TAB}\N${TAB}1
tt002${TAB}1${TAB}Brazilian Title${TAB}BR${TAB}pt${TAB}\N${TAB}\N${TAB}0
tt002${TAB}2${TAB}Título Brasileiro${TAB}BR${TAB}pt${TAB}imdbDisplay${TAB}\N${TAB}0
tt002${TAB}3${TAB}Brazilian Title${TAB}US${TAB}en${TAB}\N${TAB}\N${TAB}1
tt003${TAB}1${TAB}A Curta Sem Ano${TAB}DE${TAB}de${TAB}\N${TAB}\N${TAB}0
tt004${TAB}1${TAB}Le Titre${TAB}FR${TAB}fr${TAB}\N${TAB}\N${TAB}1
tt004${TAB}2${TAB}Le Titre${TAB}FR${TAB}fr${TAB}\N${TAB}\N${TAB}0
tt005${TAB}1${TAB}Test Series${TAB}US${TAB}en${TAB}\N${TAB}\N${TAB}1
EOF

cat > "$SRC/name.basics.tsv" <<EOF
nconst${TAB}primaryName${TAB}birthYear${TAB}deathYear${TAB}primaryProfession${TAB}knownForTitles
nm001${TAB}Director One${TAB}1970${TAB}\N${TAB}director${TAB}tt001
nm002${TAB}Writer One${TAB}1975${TAB}\N${TAB}writer${TAB}tt001
nm003${TAB}Director Two${TAB}1968${TAB}\N${TAB}director${TAB}tt002
nm004${TAB}Director Series${TAB}1972${TAB}\N${TAB}director${TAB}tt005
nm005${TAB}Writer Series${TAB}1980${TAB}\N${TAB}writer${TAB}tt005
nm010${TAB}Actor Top${TAB}1985${TAB}\N${TAB}actor${TAB}tt001,tt006,tt007
nm011${TAB}Actress Lead${TAB}1988${TAB}\N${TAB}actress${TAB}tt001,tt006,tt007
nm012${TAB}Actor Sup${TAB}1990${TAB}\N${TAB}actor${TAB}tt001,tt006
nm013${TAB}Actor Two${TAB}1992${TAB}\N${TAB}actor${TAB}tt001,tt007
nm014${TAB}Actor Three${TAB}1994${TAB}\N${TAB}actor${TAB}tt001
nm015${TAB}Actor Four${TAB}1996${TAB}\N${TAB}actor${TAB}tt001
nm016${TAB}Actor Five${TAB}1998${TAB}\N${TAB}actor${TAB}tt001
nm020${TAB}Actor Brazilian${TAB}1990${TAB}\N${TAB}actor${TAB}tt002
EOF

# ---------------------------------------------------------------------------
# Run #1: happy path
# ---------------------------------------------------------------------------

echo "=== run 1: ingest fixture TSVs into $DB ==="
python3 "$INGEST" --refresh \
  --source "$SRC" \
  --db "$DB" \
  --state "$STATE" \
  --lock "$LOCK" \
  --min-free-gb 1 > "$TMP/run1.log" 2>&1
RC1=$?
echo "rc=$RC1; log tail:"
tail -5 "$TMP/run1.log"

assert "1. ingest exits 0" test "$RC1" -eq 0
assert "2. db file exists" test -f "$DB"
assert "3. state.json exists" test -f "$STATE"

# Helper for sqlite3 queries
sql() { sqlite3 -batch "$DB" "$1"; }

# 4. All expected tables exist
TABLES_EXPECTED="title_basics title_ratings title_episode title_crew title_principals_top5 title_akas name_basics series_top_cast ingest_meta"
TABLES_ACTUAL="$(sql "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name" | tr '\n' ' ')"
for t in $TABLES_EXPECTED; do
  assert "4.$t table exists" grep -q "$t" <<< "$TABLES_ACTUAL"
done
# FTS5 virtual table appears as table too (with auxiliary _data, _idx, etc.)
assert "4.ft_titles virtual table exists" sql "SELECT 1 FROM ft_titles LIMIT 0" >/dev/null 2>&1

# 5. Row counts match expected fixture counts
assert "5.title_basics has 7 rows"   test "$(sql 'SELECT count(*) FROM title_basics')" = "7"
assert "5.title_ratings has 4 rows"  test "$(sql 'SELECT count(*) FROM title_ratings')" = "4"
assert "5.title_episode has 2 rows"  test "$(sql 'SELECT count(*) FROM title_episode')" = "2"
assert "5.title_crew has 3 rows"     test "$(sql 'SELECT count(*) FROM title_crew')" = "3"
assert "5.name_basics has 13 rows"   test "$(sql 'SELECT count(*) FROM name_basics')" = "13"

# 6. \N → NULL: tt003 has startYear IS NULL
assert "6.\N becomes NULL in title_basics.startYear" \
  test "$(sql "SELECT startYear FROM title_basics WHERE tconst='tt003'")" = ""

# 7. Cast filter: tt001 director (nm001) and writer (nm012) NOT in principals_top5
assert "7.cast filter drops directors/writers" \
  test "$(sql "SELECT count(*) FROM title_principals_top5 WHERE tconst='tt001' AND nconst IN ('nm001','nm012')")" = "0"

# 8. Top-5 cap: tt001 has 6 actors in fixture, only 5 should be stored
assert "8.top-5 cap on tt001" \
  test "$(sql "SELECT count(*) FROM title_principals_top5 WHERE tconst='tt001'")" = "5"

# 9. Top-5 contains the right 5 (by ordering: nm010, nm011, nm013, nm014, nm015 — nm016 dropped)
TOP5="$(sql "SELECT nconst FROM title_principals_top5 WHERE tconst='tt001' ORDER BY ordering")"
assert "9.top-5 keeps lowest-ordering cast" \
  diff <(echo "nm010 nm011 nm013 nm014 nm015" | tr ' ' '\n') <(echo "$TOP5") >/dev/null

# 10. Principal name denorm: nm010 has primaryName='Actor Top' populated
assert "10.principal name denormalized" \
  test "$(sql "SELECT name FROM title_principals_top5 WHERE tconst='tt001' AND nconst='nm010'")" = "Actor Top"

# 11. Akas filter: 7 kept, 2 dropped (DE + FR-non-original)
assert "11.akas filter kept 7 rows" \
  test "$(sql "SELECT count(*) FROM title_akas")" = "7"
assert "11.akas DE row dropped" \
  test "$(sql "SELECT count(*) FROM title_akas WHERE region='DE'")" = "0"
assert "11.akas FR isOriginal=1 kept, FR non-original dropped" \
  test "$(sql "SELECT count(*) FROM title_akas WHERE region='FR'")" = "1"

# 12. series_top_cast: tt005 entry exists with nm010 count=2 (in both eps)
assert "12.series_top_cast has tt005 entry" \
  test "$(sql "SELECT count(*) FROM series_top_cast WHERE parent_tconst='tt005'")" = "1"
TOP_JSON="$(sql "SELECT top_5_nconsts FROM series_top_cast WHERE parent_tconst='tt005'")"
assert "12.series_top_cast nm010 count=2" \
  python3 -c "
import json, sys
data = json.loads('''$TOP_JSON''')
nm010 = next((d for d in data if d['nconst']=='nm010'), None)
sys.exit(0 if nm010 and nm010['count']==2 else 1)
"

# 13. FTS5 populated from all 3 sources
assert "13.fts5 has 'primary' source rows" \
  test "$(sql "SELECT count(*) FROM ft_titles WHERE title_source='primary'")" = "7"
assert "13.fts5 has 'aka' source rows (filtered)" \
  test "$(sql "SELECT count(*) FROM ft_titles WHERE title_source='aka'")" = "7"
# 'original' rows: only when originalTitle != primaryTitle. Our fixture has them equal.
assert "13.fts5 'original' row count matches non-equal originals" \
  test "$(sql "SELECT count(*) FROM ft_titles WHERE title_source='original'")" = "0"

# 14. state.json fields valid
assert "14.state.json has schema_version" \
  python3 -c "
import json
with open('$STATE') as f: s = json.load(f)
assert s['schema_version'] == 1
assert 'last_refresh_finished_at' in s
assert 'source_checksums' in s
assert len(s['source_checksums']) == 7
"

# 15. PRAGMA integrity_check
assert "15.integrity_check returns ok" \
  test "$(sql 'PRAGMA integrity_check')" = "ok"

# 16. B-tree indexes for tier-1 exact match exist
INDEXES="$(sql "SELECT name FROM sqlite_master WHERE type='index' AND name LIKE 'idx_%' ORDER BY name" | tr '\n' ' ')"
for idx in idx_basics_primary_lower idx_basics_original_lower idx_akas_title_lower idx_episode_parent idx_ratings_votes idx_principals_tconst; do
  assert "16.$idx index exists" grep -q "$idx" <<< "$INDEXES"
done

# ---------------------------------------------------------------------------
# Run #2: idempotency — second --refresh should produce imdb.db.prev
# ---------------------------------------------------------------------------

echo "=== run 2: re-ingest (idempotency) ==="
python3 "$INGEST" --refresh \
  --source "$SRC" \
  --db "$DB" \
  --state "$STATE" \
  --lock "$LOCK" \
  --min-free-gb 1 > "$TMP/run2.log" 2>&1
RC2=$?
assert "17.run 2 exits 0" test "$RC2" -eq 0
assert "18.imdb.db.prev exists after run 2" test -f "$DB.prev"
# Re-verify integrity on the new live DB
assert "19.integrity_check ok after run 2" test "$(sql 'PRAGMA integrity_check')" = "ok"

# ---------------------------------------------------------------------------
# Run #3: disk pre-flight — --min-free-gb=99999 should abort with exit 2
# ---------------------------------------------------------------------------

echo "=== run 3: disk pre-flight failure ==="
DB3="$TMP/db3/imdb.db"
mkdir -p "$(dirname "$DB3")"
set +e
python3 "$INGEST" --refresh \
  --source "$SRC" \
  --db "$DB3" \
  --min-free-gb 99999 > "$TMP/run3.log" 2>&1
RC3=$?
set -e
assert "20.disk precheck fails with exit 2" test "$RC3" -eq 2
assert "21.disk precheck did not create db" test ! -f "$DB3"

# ---------------------------------------------------------------------------
# Run #4: principals sort violation — interleaved tconsts → exit 3
# ---------------------------------------------------------------------------

echo "=== run 4: principals sort violation ==="
SRC4="$TMP/src4"
cp -R "$SRC" "$SRC4"
# Swap principals to interleaved order: tt001, tt002, tt001 (back) — should abort
cat > "$SRC4/title.principals.tsv" <<EOF
tconst${TAB}ordering${TAB}nconst${TAB}category${TAB}job${TAB}characters
tt001${TAB}1${TAB}nm010${TAB}actor${TAB}\N${TAB}["Lead"]
tt002${TAB}1${TAB}nm020${TAB}actor${TAB}\N${TAB}["Hero"]
tt001${TAB}2${TAB}nm011${TAB}actress${TAB}\N${TAB}["Co"]
EOF
DB4="$TMP/db4/imdb.db"
mkdir -p "$(dirname "$DB4")"
set +e
python3 "$INGEST" --refresh \
  --source "$SRC4" \
  --db "$DB4" \
  --min-free-gb 1 > "$TMP/run4.log" 2>&1
RC4=$?
set -e
assert "22.sort violation exits 3" test "$RC4" -eq 3
assert "23.sort violation log mentions 'sort assumption violated'" \
  grep -q "sort assumption violated" "$TMP/run4.log"

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------

echo ""
echo "=== summary ==="
echo "PASS: $PASS"
echo "FAIL: $FAIL"
exit $FAIL
