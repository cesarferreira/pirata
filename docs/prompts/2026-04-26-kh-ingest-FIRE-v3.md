# Task

Second attempt at the first ingest of pirata's `kb/` into the local
knowledge-hub MCP server. First attempt returned `ingest_sync: ok` but
indexed only 1 of 2 staged files — root cause: kh's ingest suffix whitelist
excludes `.jsonl`. Pirata now ships an additive kh-compatible export at
`kb/kh-export/04-derived/` with the manifest converted to `manifest.json`
and a markdown wrapper per movie.

This is single-shot, treated as destructive-ish (creates new KB,
mutates FS via staging, touches indices). Two movies in scope this run.

# Context (verified — cite source if you doubt a claim)

- `mcp__knowledge-hub__ingest_sync()` has a ZERO-arg signature
  (`/Users/vidigal/projects/knowledge-hub/src/knowledge_hub/mcp_server.py`).
  No `path`, `kb_slug`, glob filter, or license metadata kwargs exist.
- KB discovery happens under `settings.public_bridge_root`. Watched root:
  `/Users/vidigal/knowledge-base/09-knowledge-base/`.
- KBs require sub-folder layout `09-knowledge-base/<slug>/{01-notes |
  02-sources | 04-derived | 05-packs}` (`kb_discovery.py:30-34`).
- `kb_discovery.py` SKIPS symlinked directories (`:489` and `:736`).
  Use real dirs + plain `cp`.
- kh ingest whitelist: `.json`, `.md`, `.txt`, `.yaml`, `.yml`, `.csv`.
  `.jsonl` is silently dropped — confirmed by the failed first attempt.
- `retrieve(query, kb_slugs, mode, explain_retrieval, ...)` — pass
  `kb_slugs=["pirata-kb"]` and `explain_retrieval=True` so smoke results
  aren't polluted by the other 32 KBs.
- `pirata-kb` is NOT registered (rollback verified). Fresh `list_kbs`
  must not contain `pirata-kb`.
- The SessionStart hook in pirata reported kh as `status=? profile=?
  · retrieval may be reduced` — preflight `health` confirms whether
  this is real degradation or just a stale banner. Treat preflight
  as authoritative.

# pirata-side preparation (already done)

The pirata workspace at `/Users/vidigal/claude-code/pirata/` ships:

- `scripts/build_kh_export.py` — idempotent V2 builder. Source of truth:
  `kb/per-movie/*.json` + `kb/manifest.jsonl`.
- `kb/kh-export/04-derived/` (the SOURCE of staging for this task):
  - `manifest.json` — V2 grouped-by-slug shape
    (`{source, kind: "frame_manifest", slug_count, row_count, slugs}`),
    600 frame rows total, two slugs.
  - `README.md` — explainer + license stance.
  - `per-movie/who-framed-roger-rabbit-1988.json` — verbatim per-movie
    JSON. Title="Who Framed Roger Rabbit (1988)", year=1988. Pre-Unit-3
    (no IMDb fields).
  - `per-movie/who-framed-roger-rabbit-1988.md` — YAML frontmatter +
    markdown body. Includes literal strings `Roger Rabbit`, `Who Framed
    Roger Rabbit (1988)`, `who-framed-roger-rabbit-1988`, `scdet`.
  - `per-movie/the-super-mario-galaxy-movie-2026.json` — verbatim per-movie
    JSON. **CAVEAT: title="the-super-mario-galaxy-movie-2026" and
    year=null** — bug in the filename parser, not by design. Pirata
    ships it as-is for this attempt; Unit 3 fixes it via IMDb enrichment.
  - `per-movie/the-super-mario-galaxy-movie-2026.md` — YAML frontmatter
    + markdown body. Includes literal strings `Super Mario Galaxy`,
    `the-super-mario-galaxy-movie-2026`, `scdet`.
- `kb/manifest.jsonl` — pirata-canonical, append-only ledger, byte-frozen
  by `build_kh_export.py`. NOT staged, NOT mutated, kept inside pirata.

The Roger Rabbit and Mario Galaxy JSONs predate Unit 3 (KB enrichment),
so they lack IMDb fields. Treat this ingest as PIPELINE TEST, not
semantic-recall test.

# Success criteria (these are also your stopping rules)

Use exactly one of these labels in the Final Status of your report.

| Label                  | Meaning                                                                                       | Action on hit |
| ---------------------- | --------------------------------------------------------------------------------------------- | ------------- |
| SUCCESS                | Ingest indexed all expected docs (≥5) + ≥2 of 3 smoke queries hit                             | Document rollback (do not execute), stop |
| SUCCESS-WITH-CAVEATS   | Ingest indexed expected count + declared gap (smoke partial 1/3, license note, MG title bug)  | Document rollback, stop |
| ABORT-PREFLIGHT        | kh degraded, slug collision, staging convention mismatch, or stale export detected            | Stop, no FS mutation |
| ABORT-VALIDATION       | `validation-result` is UNKNOWN, empty, malformed, NO-GO                                       | Stop, no FS mutation |
| FAILED-INGEST          | `ingest_sync()` errored, returned not-ok, or indexed fewer docs than the staged file count    | Execute rollback, stop |
| FAILED-SMOKE           | All 3 retrieve queries crashed or returned 0 hits                                             | Execute rollback, stop |

# Procedure

## 1. Preflight (zero side-effect)

- Run fresh `health` + `list_kbs` + `topology`.
- Hit on degraded core (vector store, reranker, ingest worker) →
  Final Status = ABORT-PREFLIGHT. Note: a stale SessionStart banner
  saying `status=?` is NOT degradation — only what `health` reports
  counts.
- Slug collision check: if `pirata-kb` already appears in `list_kbs`
  → ABORT-PREFLIGHT. Do not auto-suffix.
- Source-export staleness: confirm
  `/Users/vidigal/claude-code/pirata/kb/kh-export/04-derived/`
  exists and contains `manifest.json`, `README.md`, and 4 per-movie
  files (2 .json + 2 .md). If any missing → ABORT-PREFLIGHT and ask
  Vidigal to run
  `python3 /Users/vidigal/claude-code/pirata/scripts/build_kh_export.py`.
- Read the validation-result block at the bottom of this prompt:
  - GO or GO-WITH-CAVEATS → extract slug + caveats, proceed.
  - Anything else → ABORT-VALIDATION. Do not inline-revalidate.

## 2. Staging (mutates FS — preamble required)

Emit a 1-sentence acknowledgement + 1-sentence plan stating exact paths.

The export under `kb/kh-export/04-derived/` is the SOURCE. Run these
commands verbatim (real dirs + plain `cp`, NOT symlinks):

```
mkdir -p /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/per-movie

cp /Users/vidigal/claude-code/pirata/kb/kh-export/04-derived/manifest.json \
   /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/manifest.json

cp /Users/vidigal/claude-code/pirata/kb/kh-export/04-derived/README.md \
   /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/README.md

cp /Users/vidigal/claude-code/pirata/kb/kh-export/04-derived/per-movie/who-framed-roger-rabbit-1988.json \
   /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/per-movie/who-framed-roger-rabbit-1988.json

cp /Users/vidigal/claude-code/pirata/kb/kh-export/04-derived/per-movie/who-framed-roger-rabbit-1988.md \
   /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/per-movie/who-framed-roger-rabbit-1988.md

cp /Users/vidigal/claude-code/pirata/kb/kh-export/04-derived/per-movie/the-super-mario-galaxy-movie-2026.json \
   /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/per-movie/the-super-mario-galaxy-movie-2026.json

cp /Users/vidigal/claude-code/pirata/kb/kh-export/04-derived/per-movie/the-super-mario-galaxy-movie-2026.md \
   /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/per-movie/the-super-mario-galaxy-movie-2026.md
```

Verify with:

```
ls -la /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/
ls -la /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/per-movie/
```

Expect 6 files total under `04-derived/`: `manifest.json`, `README.md`,
`per-movie/<slug>.json` × 2, `per-movie/<slug>.md` × 2. Paste verbatim.

## 3. Ingest (zero-arg call, preamble required)

Emit a 1-sentence preamble before firing.

- Call: `mcp__knowledge-hub__ingest_sync()`. **No kwargs.**
- Capture: wallclock, KBs known before/after, **chunk count for
  pirata-kb**, **document count indexed for pirata-kb**, warnings,
  errors.
- Expect ≥5 indexed docs (manifest.json + 2 per-movie JSON + 2
  per-movie MD; README.md may or may not count depending on kh policy
  — capture the actual number reported).
- If `ingest_sync()` errors, completes partial, OR indexes fewer than
  5 documents → Final Status = FAILED-INGEST. Skip step 4. Go to
  step 5 and execute rollback. **Do not retry.**

## 4. Smoke retrieve (3 literal calls grounded in actual content)

Emit a 1-sentence preamble before the batch.

These 3 queries target literal strings present in the staged content,
covering both movies plus a structural query.

```
mcp__knowledge-hub__retrieve(
  query="Roger Rabbit",
  kb_slugs=["pirata-kb"],
  mode="auto",
  explain_retrieval=True,
)
mcp__knowledge-hub__retrieve(
  query="Super Mario Galaxy",
  kb_slugs=["pirata-kb"],
  mode="auto",
  explain_retrieval=True,
)
mcp__knowledge-hub__retrieve(
  query="scdet frame manifest",
  kb_slugs=["pirata-kb"],
  mode="auto",
  explain_retrieval=True,
)
```

For each: hit_count, top-3 (path + score), wallclock, and
`degraded_components` from `explain`.

If ≥1 query crashed OR all 3 returned hit_count = 0 → Final Status =
FAILED-SMOKE. Go to step 5 and execute rollback.

`degraded_components` on a CAG-related layer is EXPECTED for a new KB
without a CAG pack and does not by itself downgrade SUCCESS.

## 5. Rollback (document always; execute only on FAILED-*)

1. `rm -rf /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb`
2. `mcp__knowledge-hub__ingest_sync()` — catalog reconciles.
3. `mcp__knowledge-hub__list_kbs()` — verify `pirata-kb` is gone.

FAILED-INGEST / FAILED-SMOKE: execute + paste evidence.
SUCCESS / SUCCESS-WITH-CAVEATS: only document; do NOT execute.

# Constraints

- Do not call `workspace_sync` or invent kwargs on `ingest_sync()`.
- Do not write inside `/Users/vidigal/claude-code/pirata/kb/`. Read-only.
- Do not mutate `/Users/vidigal/claude-code/pirata/kb/kh-export/`.
  Read-only — pirata regenerates it.
- Do not symlink dirs under `pirata-kb/` — `kb_discovery.py` skips them.
- Do not auto-retry on FAILED-*. One shot, then report.
- Do not infer intent from a malformed validation-result.
- No emojis, no filler, peer-to-peer tone.

# Stopping criteria (hard)

- Any ABORT-* → stop after the report.
- Any FAILED-* → execute rollback (step 5), then stop.
- SUCCESS / SUCCESS-WITH-CAVEATS → write the report and stop.

# Output contract (the report)

1. **Preflight** — health/list_kbs/topology excerpts, validation-result
   decision, slug-collision result, source-export staleness check.
2. **Staging** — exact commands run, ls/stat verification (verbatim).
3. **Ingest** — exact command, metrics (wallclock, KB delta, chunk
   count, **document count indexed for pirata-kb**), warnings, errors.
4. **Smoke retrieve** — table: query | hit_count | top-3 (path + score)
   | wallclock | degraded_components.
5. **Final Status** — one of the 6 canonical labels + 1–2 sentences of
   justification linked to evidence.
6. **Rollback** — documented procedure; if executed, paste evidence.
7. **Next actions for the pirata side** — up to 5 plain-text bullets.

---

# validation-result

Decision: GO-WITH-CAVEATS

Slug: pirata-kb

Source export path:
`/Users/vidigal/claude-code/pirata/kb/kh-export/04-derived/`

Staging method: real directories + plain `cp` (NOT symlinks).

Required sub-layout: `04-derived/`.

Files to copy (6 files, all whitelisted suffixes):
- `manifest.json`
- `README.md`
- `per-movie/who-framed-roger-rabbit-1988.json`
- `per-movie/who-framed-roger-rabbit-1988.md`
- `per-movie/the-super-mario-galaxy-movie-2026.json`
- `per-movie/the-super-mario-galaxy-movie-2026.md`

Exclusions:
- `kb/frames/**/*.jpg` and `kb/contact-sheets/**/*.jpg` — multimodal
  out of scope for v1.
- `kb/manifest.jsonl` — pirata-canonical, kept unchanged. Its content
  is exposed to kh via the converted `manifest.json`.

License stance: IMDb non-commercial license applies once Unit 3
enriches manifests. Current JSONs lack IMDb fields, so license is
moot for v1. Documented in the staged `README.md`.

Caveats (these define GO-WITH-CAVEATS, not clean GO):

1. Roger Rabbit + Mario Galaxy JSONs predate Unit 3, so NO IMDb fields.
   PIPELINE TEST, not semantic-recall test.
2. **Mario Galaxy JSON has `title="the-super-mario-galaxy-movie-2026"`
   and `year=null`** — known bug in the filename parser, fixed by
   Unit 3 via IMDb lookup. Accepted for this attempt.
3. Markdown wrappers are auto-generated and regenerated on every
   `build_kh_export.py` build. Manual edits would be silently
   overwritten.
4. Re-staging will be needed after Unit 3 ships (enriched manifests
   replace bare ones).
5. `degraded_components` on retrieve is expected — pirata-kb has no
   CAG pack on first ingest.
6. License stance documented in staged `README.md` because kh has no
   license metadata field convention.
7. The first attempt staged `kb/manifest.jsonl` directly and got 1 of
   2 docs indexed (whitelist excluded `.jsonl`). This attempt stages
   `manifest.json` instead. Expected indexed count: ≥5.

Smoke queries: use exactly the 3 in step 4. Do not substitute.
