# Task

Second attempt at the first ingest of the pirata workspace's `kb/` into the
local knowledge-hub MCP server. The first attempt (run from this same agent)
returned `ingest_sync: ok` but only indexed 1 document of the 2 staged.
Root cause confirmed: kh's ingest suffix whitelist excludes `.jsonl`, so the
canonical `kb/manifest.jsonl` was silently skipped. Pirata now ships an
additive kh-compatible export at `kb/kh-export/04-derived/` with the manifest
converted to `manifest.json` and a markdown wrapper added per movie.

This is single-shot. Pirata maintainers will pause if you stop. Treat as
destructive-ish (creates new KB, mutates FS via staging, touches indices).

# Context (verified — cite source if you doubt a claim)

- knowledge-hub `ingest_sync()` has a ZERO-arg signature
  (`/Users/vidigal/projects/knowledge-hub/src/knowledge_hub/mcp_server.py`).
  No `path`, `kb_slug`, glob filter, or license metadata kwargs exist.
- KB discovery happens under `settings.public_bridge_root`. Specifically,
  watched discovery is at `/Users/vidigal/knowledge-base/09-knowledge-base/`
  (verified: `topology.roots.watch_knowledge_base`).
- KBs are discovered under `09-knowledge-base/<slug>/` with at least one
  of these sub-folders: `01-notes`, `02-sources`, `04-derived`, `05-packs`
  (verified at `kb_discovery.py:30-34`).
- `kb_discovery.py` SKIPS symlinked directories during traversal
  (verified: `kb_discovery.py:489` and `:736` both have
  `if child.is_symlink(): continue`). Therefore staging MUST use real
  directories with `cp`, NOT directory symlinks.
- **kh ingest suffix whitelist excludes `.jsonl`** (verified by the failed
  first attempt — staged 2 files, only 1 indexed; the dropped file was
  `manifest.jsonl`). Whitelisted suffixes confirmed via maintainer report:
  `.json`, `.md`, `.txt`, `.yaml`, `.yml`, `.csv`. Therefore pirata now
  ships `kb/manifest.jsonl` AS `manifest.json` under the export.
- No MCP-exposed delete-KB tool. `delete_kb` is internal (`runtime.py`);
  the catalog fires it when a KB disappears from FS.
- `retrieve(query, kb_slugs, mode, explain_retrieval, ...)` — call with
  `kb_slugs=["pirata-kb"]` and `explain_retrieval=True` or smoke results
  will be polluted by the other 32 KBs.
- pirata-kb is NOT registered now (rollback after the first attempt was
  verified). A fresh `list_kbs` should not contain `pirata-kb`.

# pirata-side preparation (already done)

The pirata workspace now contains:

- `scripts/build_kh_export.py` — idempotent builder that regenerates the
  export from `kb/per-movie/*.json` + `kb/manifest.jsonl`.
- `kb/kh-export/04-derived/` — the export directory with kh-compatible
  layout. Contents:
  - `per-movie/who-framed-roger-rabbit-1988.json` — verbatim copy of source
    per-movie JSON.
  - `per-movie/who-framed-roger-rabbit-1988.md` — markdown wrapper with
    YAML frontmatter (slug, title, year, fps, runtime_s, frame_count,
    sheet_count, scdet config) + body that includes the literal strings
    `Roger Rabbit`, `Who Framed Roger Rabbit (1988)`,
    `who-framed-roger-rabbit-1988`, and `scdet`.
  - `manifest.json` — converted from `kb/manifest.jsonl` with shape
    `{source, kind: "frame_manifest", row_count, rows}`. Contains all
    300 frame manifest rows.
  - `README.md` — explainer + license stance (IMDb non-commercial applies
    once Unit 3 ships; for v1, no IMDb data is present).
- The original `kb/manifest.jsonl` is preserved unchanged as pirata's
  canonical append-only ledger; `kb/kh-export/` is regenerated additively.

The Roger Rabbit JSON predates Unit 3 (KB enrichment) so it lacks IMDb
fields (tconst, rating, top_cast, akas, genres, director, plot). Treat
this ingest as PIPELINE TEST, not semantic-recall test.

# Success criteria (these are also your stopping rules)

Use exactly one of these labels in the Final Status of your report.
Each label is a terminal state — do not invent labels or chain states.

| Label                  | Meaning                                                                                       | Action on hit |
| ---------------------- | --------------------------------------------------------------------------------------------- | ------------- |
| SUCCESS                | Ingest indexed all expected docs (≥3) + ≥2 of 3 smoke queries hit                             | Document rollback (do not execute), stop |
| SUCCESS-WITH-CAVEATS   | Ingest indexed expected count + declared gap (smoke partial 1/3, license note, etc.)          | Document rollback, stop |
| ABORT-PREFLIGHT        | kh degraded, slug collision, staging convention mismatch, or stale export detected            | Stop, no FS mutation |
| ABORT-VALIDATION       | `validation-result` is UNKNOWN, empty, malformed, NO-GO                                       | Stop, no FS mutation |
| FAILED-INGEST          | `ingest_sync()` errored, returned not-ok, or indexed fewer docs than the staged file count    | Execute rollback, stop |
| FAILED-SMOKE           | All 3 retrieve queries crashed or returned 0 hits                                             | Execute rollback, stop |

# Procedure

## 1. Preflight (zero side-effect)

- Run fresh `health` + `list_kbs` + `topology`.
- Hit on degraded core (vector store, reranker, ingest worker) →
  Final Status = ABORT-PREFLIGHT.
- Slug collision check: if `pirata-kb` already appears in `list_kbs`
  → ABORT-PREFLIGHT. Do not auto-suffix.
- Staging convention check: confirm `kb_discovery.py` accepts a real
  directory at `09-knowledge-base/pirata-kb/04-derived/` populated with
  copied (non-symlinked) files. If the convention requires a top-level
  manifest the validation-result does not cover → ABORT-PREFLIGHT and
  propose the minimal manifest.
- Source-export staleness check: confirm
  `/Users/vidigal/claude-code/pirata/kb/kh-export/04-derived/` exists and
  contains `manifest.json`, `README.md`, and at least one `per-movie/*.json`.
  If any of these is missing or the directory is absent →
  ABORT-PREFLIGHT and ask Vidigal to run
  `python3 /Users/vidigal/claude-code/pirata/scripts/build_kh_export.py`
  before re-firing this prompt.
- Read the validation-result block at the bottom of this prompt:
  - Contains GO or GO-WITH-CAVEATS → extract slug + caveats, proceed.
  - Anything else (NO-GO, UNKNOWN, empty, malformed, contradictory)
    → ABORT-VALIDATION. Do not inline-revalidate.

## 2. Staging (mutates FS — preamble required)

Before running the staging commands, emit a 1-sentence acknowledgement +
1-sentence plan stating the exact paths you will create.

The export under `kb/kh-export/04-derived/` is the SOURCE of staging. Copy
its full contents to the kh canonical root. Run these commands verbatim
(real directories + plain `cp`, NOT symlinks):

```
mkdir -p /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/per-movie
cp /Users/vidigal/claude-code/pirata/kb/kh-export/04-derived/per-movie/who-framed-roger-rabbit-1988.json \
   /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/per-movie/who-framed-roger-rabbit-1988.json
cp /Users/vidigal/claude-code/pirata/kb/kh-export/04-derived/per-movie/who-framed-roger-rabbit-1988.md \
   /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/per-movie/who-framed-roger-rabbit-1988.md
cp /Users/vidigal/claude-code/pirata/kb/kh-export/04-derived/manifest.json \
   /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/manifest.json
cp /Users/vidigal/claude-code/pirata/kb/kh-export/04-derived/README.md \
   /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/README.md
```

Verify with:

```
ls -la /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/
ls -la /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/per-movie/
```

Expect 4 files total under `04-derived/`: `manifest.json`, `README.md`,
`per-movie/<slug>.json`, `per-movie/<slug>.md`. Paste verbatim outputs.

## 3. Ingest (zero-arg call, preamble required)

Emit a 1-sentence preamble before firing.

- Call: `mcp__knowledge-hub__ingest_sync()`. **No kwargs.**
- Capture: wallclock, KBs known before/after, **chunk count for the new
  pirata-kb KB**, **document count indexed for pirata-kb**, warnings,
  errors.
- The first attempt returned `status=ok` but only indexed 1 of 2 staged
  files (the `.jsonl` was filtered). On this attempt expect ≥3 indexed
  docs (per-movie JSON, per-movie MD, manifest.json; README.md may or
  may not count depending on kh policy — capture the actual number
  reported).
- If `ingest_sync()` errors, completes partial, OR indexes fewer than
  3 documents → Final Status = FAILED-INGEST. Skip step 4. Go to step 5
  and execute rollback. **Do not retry.**

## 4. Smoke retrieve (3 literal calls grounded in actual content)

Emit a 1-sentence preamble before the batch.

These 3 queries target literal strings present in BOTH the per-movie JSON
copy AND the markdown wrapper, so kh has at least 2 chunks per token to
land on. Run literally:

```
mcp__knowledge-hub__retrieve(
  query="Roger Rabbit",
  kb_slugs=["pirata-kb"],
  mode="auto",
  explain_retrieval=True,
)
mcp__knowledge-hub__retrieve(
  query="Who Framed Roger Rabbit 1988",
  kb_slugs=["pirata-kb"],
  mode="auto",
  explain_retrieval=True,
)
mcp__knowledge-hub__retrieve(
  query="who-framed-roger-rabbit-1988",
  kb_slugs=["pirata-kb"],
  mode="auto",
  explain_retrieval=True,
)
```

For each: hit_count, top-3 (path + score), wallclock, and
`degraded_components` from the `explain` payload.

If ≥1 query crashed OR all 3 returned hit_count = 0 → Final Status =
FAILED-SMOKE. Go to step 5 and execute rollback.

Note on `degraded_components`: pirata-kb has no CAG pack
(`has_cag_pack=false` is expected on first ingest). A degraded component
on a CAG-related layer is EXPECTED for a new KB and does not by itself
downgrade SUCCESS.

## 5. Rollback (document always; execute only on FAILED-*)

Real procedure (no invented commands):

1. `rm -rf /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb`
   (real-dir removal — step 2 used `cp`, not symlinks).
2. `mcp__knowledge-hub__ingest_sync()` — catalog reconciles, fires the
   internal `delete_kb`.
3. `mcp__knowledge-hub__list_kbs()` — verify `pirata-kb` is gone.

In FAILED-INGEST and FAILED-SMOKE: execute these 3 steps and paste
evidence (commands + outputs) in the report.
In SUCCESS / SUCCESS-WITH-CAVEATS: only document the procedure;
do not execute.

# Constraints

- Do not call `workspace_sync`, `delete_kb` (internal), or invent kwargs
  on `ingest_sync()`. If you need a missing capability, ABORT-PREFLIGHT
  and surface it in "Next actions".
- Do not write inside `/Users/vidigal/claude-code/pirata/kb/`. Read-only.
  Pirata owns that path.
- Do not mutate the source export at
  `/Users/vidigal/claude-code/pirata/kb/kh-export/`. Read-only.
- Do not symlink directories under
  `/Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/` —
  `kb_discovery.py` skips them. Use real dirs + `cp` (already specified
  in step 2).
- Do not auto-retry on FAILED-*. One shot, then report.
- Do not infer intent from a malformed validation-result.
  ABORT-VALIDATION is the correct response.
- If the source export is stale (older than the per-movie JSONs in
  `kb/per-movie/`), ABORT-PREFLIGHT and ask Vidigal to re-run
  `scripts/build_kh_export.py`.
- No emojis. No filler. No "Got it / Aha / Good catch" preamble tics.
  Pragmatic, peer-to-peer tone.

# Stopping criteria (hard)

- Any ABORT-* → stop after the report; do not retry, do not refire,
  do not stage, do not ingest.
- Any FAILED-* → execute rollback (step 5), then stop.
- SUCCESS / SUCCESS-WITH-CAVEATS → write the report and stop. Do not
  pre-warm cache, do not run extra queries, do not propose follow-up
  commands beyond the "Next actions" bullets.

# Preamble cadence

- Before each tool batch: 1-sentence acknowledgement + 1-sentence plan.
  Combined ≤ 2 sentences.
- Update cadence: every 1–3 execution steps. Hard floor: at least one
  preamble per 6 steps or per 10 tool calls.
- Preambles are status, not narration. State what you are about to
  check or do; skip what you already did.

# Output contract (the report)

Deliver one markdown document with these sections, in order:

1. **Preflight** — health/list_kbs/topology output (verbatim excerpts),
   validation-result decision, slug-collision result, staging-convention
   result, source-export staleness check.
2. **Staging** — exact commands run, ls/stat verification (verbatim).
3. **Ingest** — exact command, metrics (wallclock, KB delta, chunk count,
   **document count indexed for pirata-kb**), warnings, errors.
4. **Smoke retrieve** — table: query | hit_count | top-3 (path + score) |
   wallclock | degraded_components.
5. **Final Status** — one of the 6 canonical labels + 1–2 sentences of
   justification linked to evidence in 1–4.
6. **Rollback** — documented procedure; if executed, paste evidence
   (commands + outputs).
7. **Next actions for the pirata side** — up to 5 plain-text bullets.
   Each bullet = 1 concrete action (e.g., "run Unit 3 then re-run
   build_kh_export.py", "open kh issue: license metadata field",
   "add support for `.jsonl` in kh ingest whitelist if appropriate").
   No formatted acceptance criteria.

# Verification before you ship the report

- Final Status is exactly one of the 6 canonical labels.
- Step 2 + 3 commands are pasted verbatim and reproducible.
- Step 4 retrieve calls were issued literally (with `kb_slugs=["pirata-kb"]`
  and `explain_retrieval=True`); results pasted verbatim.
- Step 3 includes the actual document count indexed for pirata-kb.
- On FAILED-*, rollback was executed and `list_kbs` evidence shows
  `pirata-kb` is gone.

---

# validation-result

Decision: GO-WITH-CAVEATS

Slug: pirata-kb

Source export path:
`/Users/vidigal/claude-code/pirata/kb/kh-export/04-derived/`

Staging method: real directories + plain `cp` (NOT symlinks). The pirata
side now ships an export under `kb/kh-export/` so kh sees only files with
whitelisted suffixes (`.json`, `.md`). The original `kb/manifest.jsonl`
remains pirata-canonical and is NOT staged; `manifest.json` (converted)
goes in its place.

Required sub-layout: `04-derived/` (per kh discovery convention; one of
`{01-notes, 02-sources, 04-derived, 05-packs}` is required, and
`04-derived` is the semantic fit for auto-generated derivative content).

Files to copy (4 files total, all whitelisted suffixes):
- `per-movie/who-framed-roger-rabbit-1988.json`
- `per-movie/who-framed-roger-rabbit-1988.md`
- `manifest.json`
- `README.md`

Exact staging commands: see step 2 above. The 4 `cp` commands are the
authoritative form.

Exclusions:
- `kb/frames/**/*.jpg` — image embedding for retrieve is unverified and
  out of scope for v1. Defer multimodal indexing to Phase 2.
- `kb/contact-sheets/**/*.jpg` — same reasoning.
- The original `kb/manifest.jsonl` — pirata-canonical, kept unchanged in
  pirata, not staged. Its content is exposed to kh via the converted
  `manifest.json`.

License stance: IMDb non-commercial license will apply once Unit 3
enriches manifests with IMDb fields. The current Roger Rabbit JSON has
none, so license is moot for v1. kh has no license metadata field
convention (verified). The constraint is documented in the staged
`README.md` rather than encoded in metadata.

Caveats (these define GO-WITH-CAVEATS, not clean GO):

1. The Roger Rabbit JSON predates Unit 3 KB-enrichment, so it has NO
   IMDb fields (tconst, rating, top_cast, akas, genres, director, plot).
   Treat this ingest as PIPELINE TEST, not semantic-recall test.
2. The markdown wrapper at `<slug>.md` is auto-generated by
   `scripts/build_kh_export.py` and regenerated on every build.
   Manual edits to wrapper files would be silently overwritten.
3. Re-staging will be needed after Unit 3 ships (enriched manifests
   replace bare ones). The pirata side must re-run
   `scripts/build_kh_export.py` and then re-fire this ingest prompt.
4. `degraded_components` on retrieve calls is expected because pirata-kb
   has no CAG pack — that's normal for a freshly registered KB without
   a curated pack. Does not by itself downgrade SUCCESS.
5. License stance is documented inside the staged `README.md` because
   kh has no license metadata field convention. If kh ever ships such a
   field, this caveat becomes a structured tag.
6. The first attempt at this ingest staged `kb/manifest.jsonl` directly
   and only got 1 of 2 docs indexed (kh ingest whitelist excludes
   `.jsonl`). This second attempt addresses that root cause by staging
   the converted `manifest.json` instead. Expected document count
   indexed: ≥3 (per-movie JSON, per-movie MD, manifest.json; README.md
   may or may not be counted by kh policy — capture the actual number
   in the report).

Smoke queries: use exactly the 3 queries already specified in step 4
of this prompt. Do not substitute.
