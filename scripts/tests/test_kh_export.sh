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
expect_in() {
  local label="$1" needle="$2" haystack="$3"
  if printf '%s' "$haystack" | grep -qF -- "$needle"; then
    pass "$label"
  else
    fail "$label  (missing: $needle)"
  fi
}
expect_not_in() {
  local label="$1" needle="$2" haystack="$3"
  if printf '%s' "$haystack" | grep -qF -- "$needle"; then
    fail "$label  (unexpected: $needle)"
  else
    pass "$label"
  fi
}

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

# Markdown wrapper contains all required literal strings.
# Plan 008 Unit 2 changed Roger Rabbit's source title from "Who Framed
# Roger Rabbit (1988)" (filename-based) to "Who Framed Roger Rabbit"
# (IMDb primaryTitle). Plan 009 Unit 3 restores the parens-year alias
# via a "Title with year" body line so KH retrievers can match the
# searchable token even when the IMDb-canonical title omits it.
for needle in "Roger Rabbit" "Who Framed Roger Rabbit" "Who Framed Roger Rabbit (1988)" "who-framed-roger-rabbit-1988" "scdet"; do
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
# Post-plan-008 Unit 2: Mario Galaxy per-movie JSON regenerated with
# resolved IMDb (vote-spread tie-breaker fired on the famous-vs-zero-vote
# tvEpisode tie). The wrapper now carries canonical title + year +
# `## IMDb metadata` body section instead of the old multi_tie caveat.
if grep -qF 'title: "The Super Mario Galaxy Movie"' "$MG_DST_MD" 2>/dev/null; then
  pass "11c.Mario Galaxy wrapper has canonical title (not slug)"
else
  fail "11c.Mario Galaxy wrapper missing canonical title"
fi
if grep -qF "year: 2026" "$MG_DST_MD" 2>/dev/null; then
  pass "11d.Mario Galaxy wrapper has year=2026"
else
  fail "11d.Mario Galaxy wrapper missing year=2026"
fi
if grep -qF 'tconst: "tt28650488"' "$MG_DST_MD" 2>/dev/null; then
  pass "11e.Mario Galaxy wrapper has IMDb tconst"
else
  fail "11e.Mario Galaxy wrapper missing IMDb tconst"
fi
if grep -qF "## IMDb metadata" "$MG_DST_MD" 2>/dev/null; then
  pass "11f.Mario Galaxy wrapper has IMDb metadata section"
else
  fail "11f.Mario Galaxy wrapper missing IMDb metadata section"
fi
# Roger Rabbit also resolves post-plan-008 (vote spread 1110× vs the
# same-name 1988 video game).
RR_DST_MD="$EXPORT/04-derived/per-movie/who-framed-roger-rabbit-1988.md"
if grep -qF 'tconst: "tt0096438"' "$RR_DST_MD" 2>/dev/null; then
  pass "11g.Roger Rabbit wrapper has IMDb tconst"
else
  fail "11g.Roger Rabbit wrapper missing IMDb tconst"
fi
if grep -qF "Robert Zemeckis" "$RR_DST_MD" 2>/dev/null; then
  pass "11h.Roger Rabbit wrapper names Robert Zemeckis as director"
else
  fail "11h.Roger Rabbit wrapper missing director attribution"
fi

# Plan 009 Unit 3: per-movie JSON title/year now overlays the slug-level
# display metadata in manifest.json (was stale slug+null for MG, since
# kb/manifest.jsonl is byte-frozen and predates plan 008's IMDb resolve).
# Raw row provenance must remain intact under slugs[<slug>].rows[].
if python3 -c "
import json, sys
m = json.load(open(sys.argv[1]))
mg = m['slugs']['the-super-mario-galaxy-movie-2026']
assert mg['title'] == 'The Super Mario Galaxy Movie', f'MG header title: {mg[\"title\"]!r}'
assert mg['year'] == 2026, f'MG header year: {mg[\"year\"]!r}'
assert mg['rows'][0]['title'] == 'the-super-mario-galaxy-movie-2026', f'raw row title overwritten: {mg[\"rows\"][0][\"title\"]!r}'
assert mg['rows'][0]['year'] is None, f'raw row year overwritten: {mg[\"rows\"][0][\"year\"]!r}'
" "$MANIFEST_DST"; then
  pass "11i.MG manifest.json display title/year overlay + raw row provenance preserved"
else
  fail "11i.MG manifest.json overlay or provenance broken"
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

# ----------------------------------------------------------------- run 5 ---
# Wrapper IMDb-resolved rendering (plan 007 Unit C).
# Drive build_slug_md directly with synthetic per-movie JSON shapes so we
# don't need to touch the live kb/. Hermetic tmpdir for the synthetic JSON.
echo "=== run 5: wrapper IMDb-resolved rendering ==="
SYN_TMP=$(mktemp -d)
trap "rm -rf $SYN_TMP" EXIT

# 5a — resolved: full imdb block surfaces in YAML + body.
RESOLVED_JSON="$SYN_TMP/test-resolved.json"
cat > "$RESOLVED_JSON" <<'EOF'
{
  "slug": "test-resolved",
  "title": "Bacurau",
  "year": 2019,
  "fps": 23.976,
  "runtime_s": 6678.0,
  "source_size_bytes": 12345678,
  "scdet": {"threshold": 8, "floor_s": 4.0, "target": 300},
  "extracted_at": "2026-04-26T16:00:00Z",
  "filename": {"raw_title": "Bacurau.2019.1080p.BluRay.x265", "ptt_title": "Bacurau", "ptt_year": 2019},
  "imdb": {
    "tconst": "tt2762506",
    "primaryTitle": "Bacurau",
    "originalTitle": "Bacurau",
    "year": 2019,
    "genres": ["Drama", "Thriller", "Western"],
    "rating": {"average": 7.3, "votes": 34690},
    "director": [
      {"nconst": "nm9999", "name": "Juliano Dornelles"},
      {"nconst": "nm8888", "name": "Kleber Mendonça Filho"}
    ],
    "plot": null,
    "top_cast": [
      {"nconst": "nm1", "name": "Sônia Braga", "role": null},
      {"nconst": "nm2", "name": "Udo Kier", "role": null}
    ],
    "akas": [{"title": "Bacurau", "language": "pt"}],
    "confidence": 100,
    "multi_tie": false,
    "result": "resolved",
    "lookup_attempted": true
  },
  "frames": [{"idx": 1, "tc": "00:00:01:00", "t_s": 1.0}],
  "sheets": []
}
EOF
out=$(python3 - "$RESOLVED_JSON" <<'PYEOF'
import json, sys
sys.path.insert(0, "scripts")
import build_kh_export
js = json.loads(open(sys.argv[1]).read())
group = {"title": js["title"], "year": js["year"],
         "rows": [{"idx": 1, "tc": "00:00:01:00", "t_s": 1.0}]}
print(build_kh_export.build_slug_md("test-resolved", group, type("P",(object,),{"is_file":lambda self:True,"read_text":lambda self,encoding=None:open(sys.argv[1]).read()})()))
PYEOF
)
expect_in "23a.resolved YAML has tconst"          'tconst: tt2762506'                "$out"
expect_in "23b.resolved YAML has imdb_confidence" 'imdb_confidence: 100'             "$out"
expect_in "23c.resolved YAML has rating_average"  'rating_average: 7.3'              "$out"
expect_in "23d.resolved YAML has rating_votes"    'rating_votes: 34690'              "$out"
expect_in "23e.resolved YAML has genres"          'genres: "Drama, Thriller, Western"' "$out"
expect_in "23f.resolved YAML has directors"       'Juliano Dornelles; Kleber'        "$out"
expect_in "23g.resolved body has IMDb section"    '## IMDb metadata'                 "$out"
expect_in "23h.resolved body has director"        '- Director: Juliano Dornelles'    "$out"
expect_in "23i.resolved body has top cast"        '- Top cast: Sônia Braga, Udo Kier' "$out"
expect_in "23j.resolved body has tconst"          '- IMDb tconst: tt2762506'         "$out"
# Resolved suppresses the title-equals-slug + year-missing caveats.
expect_not_in "23k.resolved no slug-mismatch caveat" 'matches the slug'              "$out"
expect_not_in "23l.resolved no Unit-3-pending caveat" 'predates Unit 3'              "$out"

# 5b — bare manifest (no imdb block): wrapper renders historical shape.
BARE_JSON="$SYN_TMP/test-bare.json"
cat > "$BARE_JSON" <<'EOF'
{
  "slug": "test-bare",
  "title": "Test Bare Movie",
  "year": 2020,
  "fps": 24.0,
  "runtime_s": 5400.0,
  "source_size_bytes": 100,
  "scdet": {"threshold": 8, "floor_s": 4.0, "target": 300},
  "extracted_at": "2026-04-26T16:00:00Z",
  "frames": [{"idx": 1, "tc": "00:00:01:00", "t_s": 1.0}],
  "sheets": []
}
EOF
out=$(python3 - "$BARE_JSON" <<'PYEOF'
import json, sys
sys.path.insert(0, "scripts")
import build_kh_export
group = {"title": "Test Bare Movie", "year": 2020,
         "rows": [{"idx": 1, "tc": "00:00:01:00", "t_s": 1.0}]}
print(build_kh_export.build_slug_md("test-bare", group, type("P",(object,),{"is_file":lambda self:True,"read_text":lambda self,encoding=None:open(sys.argv[1]).read()})()))
PYEOF
)
expect_not_in "24a.bare YAML has no tconst"        'tconst:'                          "$out"
expect_not_in "24b.bare YAML has no imdb_confidence" 'imdb_confidence:'               "$out"
expect_not_in "24c.bare body has no IMDb section" '## IMDb metadata'                 "$out"

# 5c — multi_tie: wrapper bare in IMDb section, caveat references reason.
MT_JSON="$SYN_TMP/test-multitie.json"
cat > "$MT_JSON" <<'EOF'
{
  "slug": "test-multitie",
  "title": "Generic Movie",
  "year": 2021,
  "fps": 24.0,
  "runtime_s": 5400.0,
  "source_size_bytes": 100,
  "scdet": {"threshold": 8, "floor_s": 4.0, "target": 300},
  "extracted_at": "2026-04-26T16:00:00Z",
  "imdb": {
    "lookup_attempted": true,
    "result": "multi_tie",
    "candidates_considered": 4,
    "top_score": 100.0,
    "runner_up_score": 100.0,
    "multi_tie": true
  },
  "frames": [{"idx": 1, "tc": "00:00:01:00", "t_s": 1.0}],
  "sheets": []
}
EOF
out=$(python3 - "$MT_JSON" <<'PYEOF'
import json, sys
sys.path.insert(0, "scripts")
import build_kh_export
group = {"title": "Generic Movie", "year": 2021,
         "rows": [{"idx": 1, "tc": "00:00:01:00", "t_s": 1.0}]}
print(build_kh_export.build_slug_md("test-multitie", group, type("P",(object,),{"is_file":lambda self:True,"read_text":lambda self,encoding=None:open(sys.argv[1]).read()})()))
PYEOF
)
expect_not_in "25a.multi_tie no IMDb section"     '## IMDb metadata'                 "$out"
expect_not_in "25b.multi_tie no tconst YAML"      'tconst:'                          "$out"
expect_in     "25c.multi_tie caveat surfaces"     'returned multi_tie'               "$out"

# 5d — db_unavailable: wrapper bare, caveat names reason.
DBNA_JSON="$SYN_TMP/test-dbna.json"
cat > "$DBNA_JSON" <<'EOF'
{
  "slug": "test-dbna",
  "title": "Some Movie",
  "year": 2022,
  "fps": 24.0,
  "runtime_s": 5400.0,
  "source_size_bytes": 100,
  "scdet": {"threshold": 8, "floor_s": 4.0, "target": 300},
  "extracted_at": "2026-04-26T16:00:00Z",
  "imdb": {
    "lookup_attempted": false,
    "result": "db_unavailable",
    "candidates_considered": 0
  },
  "frames": [{"idx": 1, "tc": "00:00:01:00", "t_s": 1.0}],
  "sheets": []
}
EOF
out=$(python3 - "$DBNA_JSON" <<'PYEOF'
import json, sys
sys.path.insert(0, "scripts")
import build_kh_export
group = {"title": "Some Movie", "year": 2022,
         "rows": [{"idx": 1, "tc": "00:00:01:00", "t_s": 1.0}]}
print(build_kh_export.build_slug_md("test-dbna", group, type("P",(object,),{"is_file":lambda self:True,"read_text":lambda self,encoding=None:open(sys.argv[1]).read()})()))
PYEOF
)
expect_not_in "26a.db_unavailable no IMDb section" '## IMDb metadata'                 "$out"
expect_in     "26b.db_unavailable caveat surfaces" 'IMDb DB was unavailable'         "$out"

# 5e — determinism: two consecutive renders of the resolved fixture must
# be byte-identical (regression guard for the new branches).
out1=$(python3 - "$RESOLVED_JSON" <<'PYEOF'
import json, sys
sys.path.insert(0, "scripts")
import build_kh_export
group = {"title": "Bacurau", "year": 2019,
         "rows": [{"idx": 1, "tc": "00:00:01:00", "t_s": 1.0}]}
print(build_kh_export.build_slug_md("test-resolved", group, type("P",(object,),{"is_file":lambda self:True,"read_text":lambda self,encoding=None:open(sys.argv[1]).read()})()))
PYEOF
)
out2=$(python3 - "$RESOLVED_JSON" <<'PYEOF'
import json, sys
sys.path.insert(0, "scripts")
import build_kh_export
group = {"title": "Bacurau", "year": 2019,
         "rows": [{"idx": 1, "tc": "00:00:01:00", "t_s": 1.0}]}
print(build_kh_export.build_slug_md("test-resolved", group, type("P",(object,),{"is_file":lambda self:True,"read_text":lambda self,encoding=None:open(sys.argv[1]).read()})()))
PYEOF
)
if [ "$out1" = "$out2" ]; then
  pass "27.determinism: two renders byte-identical"
else
  fail "27.determinism: render drift detected"
fi

# 5f — Plan 010 Unit 3: 'Title with year' alias suppression branch.
# When title==slug OR year==null, the parens-year body line must be absent
# (else KH retrieval would see literal '<slug> (None)' or '<slug> (<slug>)'
# polluting the index). The L297-298 guard `not title_is_slug and not
# year_missing` is exercised here — fixtures only-positive-side until now.
SUPPRESS_BOTH_JSON="$SYN_TMP/test-suppress-both.json"
cat > "$SUPPRESS_BOTH_JSON" <<'EOF'
{
  "slug": "test-suppress-both",
  "title": "test-suppress-both",
  "year": null,
  "fps": 24.0,
  "runtime_s": 5400.0,
  "scdet": {"threshold": 8, "floor_s": 4.0, "target": 300},
  "extracted_at": "2026-04-26T16:00:00Z",
  "frames": [{"idx": 1, "tc": "00:00:01:00", "t_s": 1.0}],
  "sheets": []
}
EOF
out=$(python3 - "$SUPPRESS_BOTH_JSON" <<'PYEOF'
import json, sys
sys.path.insert(0, "scripts")
import build_kh_export
group = {"title": "test-suppress-both", "year": None,
         "rows": [{"idx": 1, "tc": "00:00:01:00", "t_s": 1.0}]}
print(build_kh_export.build_slug_md("test-suppress-both", group, type("P",(object,),{"is_file":lambda self:True,"read_text":lambda self,encoding=None:open(sys.argv[1]).read()})()))
PYEOF
)
expect_not_in "28a.suppress-both: no Title with year alias"     'Title with year:' "$out"
expect_in     "28b.suppress-both: slug literal still surfaces"  'test-suppress-both' "$out"

# 5g — only year is missing (title resolves to a real value via overlay).
# year_missing alone must trip the suppression guard.
SUPPRESS_YEAR_JSON="$SYN_TMP/test-suppress-year.json"
cat > "$SUPPRESS_YEAR_JSON" <<'EOF'
{
  "slug": "test-suppress-year",
  "title": "Some Movie",
  "year": null,
  "fps": 24.0,
  "runtime_s": 5400.0,
  "scdet": {"threshold": 8, "floor_s": 4.0, "target": 300},
  "extracted_at": "2026-04-26T16:00:00Z",
  "frames": [{"idx": 1, "tc": "00:00:01:00", "t_s": 1.0}],
  "sheets": []
}
EOF
out=$(python3 - "$SUPPRESS_YEAR_JSON" <<'PYEOF'
import json, sys
sys.path.insert(0, "scripts")
import build_kh_export
group = {"title": "test-suppress-year", "year": None,
         "rows": [{"idx": 1, "tc": "00:00:01:00", "t_s": 1.0}]}
print(build_kh_export.build_slug_md("test-suppress-year", group, type("P",(object,),{"is_file":lambda self:True,"read_text":lambda self,encoding=None:open(sys.argv[1]).read()})()))
PYEOF
)
expect_not_in "28c.suppress-year-only: no Title with year alias" 'Title with year:' "$out"
expect_in     "28d.suppress-year-only: title overlaid from JSON" '- Title: Some Movie' "$out"

# 5h — only title equals slug (year resolves). title_is_slug alone must trip.
SUPPRESS_TITLE_JSON="$SYN_TMP/test-suppress-title.json"
cat > "$SUPPRESS_TITLE_JSON" <<'EOF'
{
  "slug": "test-suppress-title",
  "title": "test-suppress-title",
  "year": 2020,
  "fps": 24.0,
  "runtime_s": 5400.0,
  "scdet": {"threshold": 8, "floor_s": 4.0, "target": 300},
  "extracted_at": "2026-04-26T16:00:00Z",
  "frames": [{"idx": 1, "tc": "00:00:01:00", "t_s": 1.0}],
  "sheets": []
}
EOF
out=$(python3 - "$SUPPRESS_TITLE_JSON" <<'PYEOF'
import json, sys
sys.path.insert(0, "scripts")
import build_kh_export
group = {"title": "test-suppress-title", "year": 2020,
         "rows": [{"idx": 1, "tc": "00:00:01:00", "t_s": 1.0}]}
print(build_kh_export.build_slug_md("test-suppress-title", group, type("P",(object,),{"is_file":lambda self:True,"read_text":lambda self,encoding=None:open(sys.argv[1]).read()})()))
PYEOF
)
expect_not_in "28e.suppress-title-only: no Title with year alias" 'Title with year:' "$out"
expect_in     "28f.suppress-title-only: year overlaid from JSON"  '- Year: 2020' "$out"

# ----------------------------------------------------------------- run 6 ---
# Plan 010 Unit 2: hermetic kb/-shape tmpdir with corrupt per-movie JSON.
# Proves the symmetric fallback in build_manifest_json (silent overlay
# error path) and build_slug_md (Unit 1 hardening) both degrade gracefully
# instead of crashing the builder. Pre-Unit-1, build_slug_md raised on the
# corrupt JSON before manifest.json was ever written, making this test
# physically unreachable. Now both functions emit a warn breadcrumb and
# the builder completes, manifest.json header falls back to manifest-derived
# slug-shaped title with year=null, and raw rows[] ship verbatim.
echo "=== run 6: corrupt per-movie JSON fallback (plan 010) ==="
KB6=$(mktemp -d -t khexport-kb6.XXXXXX)
trap "rm -rf $SYN_TMP $KB6" EXIT  # combined cleanup with run-5 SYN_TMP
mkdir -p "$KB6/per-movie"
echo '{"slug":"test-corrupt","idx":1,"tc":"00:00:01:00","t_s":1.0}' > "$KB6/manifest.jsonl"
echo '{ not json' > "$KB6/per-movie/test-corrupt.json"
OUT6="$KB6/out"
RUN6_LOG=$(python3 "$SCRIPT" --kb "$KB6" --out "$OUT6" 2>&1)
rc=$?
if [ "$rc" -eq 0 ]; then pass "29.corrupt JSON: builder exits 0 (graceful)"; else fail "29.corrupt JSON: builder exit was $rc (log: $RUN6_LOG)"; fi
# Warn breadcrumb fires from BOTH overlay sites — split into two distinct
# assertions so a future regression dropping one of the two log paths
# (manifest.json header fallback vs bare-wrapper fallback) is caught
# directly. Single-grep would silently pass if either message survived.
if printf '%s' "$RUN6_LOG" | grep -qF 'manifest.json header falls back to manifest-derived title/year'; then
  pass "30a.corrupt JSON: build_manifest_json warn breadcrumb emitted"
else
  fail "30a.corrupt JSON: missing build_manifest_json warn breadcrumb"
fi
if printf '%s' "$RUN6_LOG" | grep -qF 'falling back to bare-wrapper rendering'; then
  pass "30b.corrupt JSON: build_slug_md warn breadcrumb emitted"
else
  fail "30b.corrupt JSON: missing build_slug_md warn breadcrumb"
fi
# manifest.json header falls back to manifest-derived slug-shaped title +
# year=null; raw rows[] preserved verbatim from manifest.jsonl input.
if python3 -c "
import json, sys
m = json.load(open(sys.argv[1]))
s = m['slugs']['test-corrupt']
assert s['title'] == 'test-corrupt', f'header title: {s[\"title\"]!r} (expected manifest-derived slug fallback)'
assert s['year'] is None, f'header year: {s[\"year\"]!r} (expected None)'
assert s['rows'][0]['slug'] == 'test-corrupt', f'row slug: {s[\"rows\"][0][\"slug\"]!r}'
assert s['rows'][0]['idx'] == 1, f'row idx: {s[\"rows\"][0][\"idx\"]!r}'
" "$OUT6/04-derived/manifest.json"; then
  pass "31.corrupt JSON: manifest.json header falls back, rows[] preserved"
else
  fail "31.corrupt JSON: header overlay or row provenance broken"
fi
# build_slug_md fallback: wrapper file gets written (no crash), in bare-
# manifest mode (no IMDb section, no fps/runtime body fields).
if [ -f "$OUT6/04-derived/per-movie/test-corrupt.md" ]; then
  pass "32.corrupt JSON: bare wrapper still produced"
else
  fail "32.corrupt JSON: wrapper missing (build_slug_md may have crashed)"
fi
if grep -qF "## IMDb metadata" "$OUT6/04-derived/per-movie/test-corrupt.md" 2>/dev/null; then
  fail "33.corrupt JSON: wrapper unexpectedly emitted IMDb section"
else
  pass "33.corrupt JSON: wrapper rendered in bare-manifest mode"
fi

# 34 — Non-UTF-8 byte corruption: Path.read_text(encoding="utf-8") raises
# UnicodeDecodeError (ValueError subclass), distinct from JSONDecodeError
# and OSError. Pre-fix, this escaped the except clause and crashed the
# builder via main()'s catch-all. Plan-010-followup (kieran-python-001)
# extended both except tuples to cover it.
KB7=$(mktemp -d -t khexport-kb7.XXXXXX)
trap "rm -rf $SYN_TMP $KB6 $KB7" EXIT  # combined cleanup with run-5 SYN_TMP and run-6 KB6
mkdir -p "$KB7/per-movie"
echo '{"slug":"test-utf8","idx":1,"tc":"00:00:01:00","t_s":1.0}' > "$KB7/manifest.jsonl"
# Write raw non-UTF-8 bytes (0xff is invalid as a UTF-8 lead byte).
printf '\xff\xfe garbage bytes \xff\xfe' > "$KB7/per-movie/test-utf8.json"
OUT7="$KB7/out"
RUN7_LOG=$(python3 "$SCRIPT" --kb "$KB7" --out "$OUT7" 2>&1)
rc=$?
if [ "$rc" -eq 0 ]; then pass "34.non-UTF-8 bytes: builder exits 0 (graceful)"; else fail "34.non-UTF-8 bytes: builder exit was $rc (log: $RUN7_LOG)"; fi
if printf '%s' "$RUN7_LOG" | grep -qF 'per-movie JSON unreadable for test-utf8'; then
  pass "35.non-UTF-8 bytes: warn breadcrumb emitted (UnicodeDecodeError path)"
else
  fail "35.non-UTF-8 bytes: no warn breadcrumb (encoding error escaped except)"
fi

# ----------------------------------------------------------------- summary ---
rm -f "$REPO/.kh_export_run1.log" "$REPO/.kh_export_run2.log" "$REPO/.kh_export_run3.log" "$REPO/.kh_export_run4.log"

echo
echo "=== summary ==="
echo "PASS: $PASS"
echo "FAIL: $FAIL"
[ "$FAIL" -eq 0 ] && exit 0 || exit 1
