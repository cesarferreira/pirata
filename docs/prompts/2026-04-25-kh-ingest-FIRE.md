# Task

First ingest of the pirata workspace's `kb/` folder into the local
knowledge-hub MCP server. Single-shot operation; pirata maintainers
will pause if you stop. Treat as destructive-ish (creates new KB,
mutates FS via staging, touches indices).

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
- **`kb_discovery.py` SKIPS symlinked directories** during traversal
  (verified: `kb_discovery.py:489` and `:736` both have
  `if child.is_symlink(): continue`). Therefore staging MUST use real
  directories with `cp -R`, NOT directory symlinks. File-level symlinks
  are not used here either — copy the JSON + manifest.jsonl plain.
- No MCP-exposed delete-KB tool. `delete_kb` is internal (`runtime.py`);
  the catalog fires it when a KB disappears from FS.
- `retrieve(query, kb_slugs, mode, explain_retrieval, ...)` — call with
  `kb_slugs=["pirata-kb"]` and `explain_retrieval=True` or smoke results
  will be polluted by the other 32 KBs.
- pirata kb/ is NOT registered (verified via `list_kbs`: slug `pirata-kb`
  is absent from current catalog).
- pirata kb/ contents:
  - `kb/per-movie/who-framed-roger-rabbit-1988.json` (only populated
    movie). The JSON predates Unit 3 KB-enrichment, so it lacks IMDb
    fields (tconst, rating, top_cast, akas, genres, director, plot).
    Top-level fields actually present: `slug`, `title`, `year`, `fps`,
    `runtime_s`, `source_file`, `source_size_bytes`, `scdet` (config
    block), `extracted_at`, `frames` (300 entries with idx/file/tc/t_s),
    `sheets` (3 entries). Treat this ingest as PIPELINE TEST, not
    semantic-recall test.
  - `kb/frames/who-framed-roger-rabbit-1988/*.jpg` — excluded.
  - `kb/contact-sheets/who-framed-roger-rabbit-1988__sheet_*.jpg` — excluded.
  - `kb/manifest.jsonl` (~1 line, JSON-lines) — included.
- pirata SessionStart banner reported degraded knowledge-hub status, but
  fresh `health` returns `status=ok, hub_profile=local_full_power_plus`.
  The banner is stale; trust fresh `health`.

# Success criteria (these are also your stopping rules)

Use exactly one of these labels in the Final Status of your report.
Each label is a terminal state — do not invent labels or chain states.

| Label                  | Meaning                                                          | Action on hit |
| ---------------------- | ---------------------------------------------------------------- | ------------- |
| SUCCESS                | Ingest ok + ≥2 of 3 smoke queries hit                            | Document rollback (do not execute), stop |
| SUCCESS-WITH-CAVEATS   | Ingest ok + declared gap (license, suboptimal chunking, smoke 1/3) | Document rollback, stop |
| ABORT-PREFLIGHT        | kh degraded, slug collision, or non-deterministic staging        | Stop, no FS mutation |
| ABORT-VALIDATION       | `validation-result` is UNKNOWN, empty, malformed, NO-GO          | Stop, no FS mutation |
| FAILED-INGEST          | `ingest_sync()` errored or completed partial                     | Execute rollback, stop |
| FAILED-SMOKE           | All 3 retrieve queries crashed or returned 0 hits                | Execute rollback, stop |

# Procedure

## 1. Preflight (zero side-effect)

- Run fresh `health` + `list_kbs` + `topology`.
- Hit on degraded core (vector store, reranker, ingest worker) →
  Final Status = ABORT-PREFLIGHT.
- Slug collision check: if `pirata-kb` already appears in `list_kbs`
  → ABORT-PREFLIGHT. Do not auto-suffix.
- Staging convention check: confirm `kb_discovery.py` accepts a real
  directory at `09-knowledge-base/pirata-kb/04-derived/` populated
  with copied (non-symlinked) files. If the convention requires a
  top-level manifest the validation-result does not cover →
  ABORT-PREFLIGHT and propose the minimal manifest.
- Read the validation-result block at the bottom of this prompt:
  - Contains GO or GO-WITH-CAVEATS → extract slug + caveats, proceed.
  - Anything else (NO-GO, UNKNOWN, empty, malformed, contradictory)
    → ABORT-VALIDATION. Do not inline-revalidate.

## 2. Staging (mutates FS — preamble required)

Before running the staging commands, emit a 1-sentence acknowledgement
+ 1-sentence plan stating the exact paths you will create.

Run these commands verbatim (real directories + plain `cp`, NOT symlinks):

```
mkdir -p /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/per-movie
cp /Users/vidigal/claude-code/pirata/kb/per-movie/who-framed-roger-rabbit-1988.json \
   /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/per-movie/who-framed-roger-rabbit-1988.json
cp /Users/vidigal/claude-code/pirata/kb/manifest.jsonl \
   /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/manifest.jsonl
```

Verify with:

```
ls -la /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/
ls -la /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/per-movie/
stat /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/manifest.jsonl
```

Paste the verbatim outputs of `ls -la` and `stat` in the report.

## 3. Ingest (zero-arg call, preamble required)

Emit a 1-sentence preamble before firing.

- Call: `mcp__knowledge-hub__ingest_sync()`. **No kwargs.**
- Capture: wallclock, KBs known before/after, chunk count for the new
  `pirata-kb` KB, warnings, errors.
- On error or partial completion → Final Status = FAILED-INGEST.
  Skip step 4. Go to step 5 and execute rollback. Do not retry.

## 4. Smoke retrieve (3 literal calls grounded in actual JSON content)

Emit a 1-sentence preamble before the batch.

The Roger Rabbit JSON contains the strings `"Who Framed Roger Rabbit
(1988)"`, `"who-framed-roger-rabbit-1988"`, and `"scdet"` (among others).
The 3 queries below target tokens that ARE in the indexed content. Run
literally:

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

Note on degraded_components expectation: pirata-kb has no CAG pack
(`has_cag_pack=false` will be set on first ingest), so a degraded
component on rerank or CAG-related layer is EXPECTED for a new KB
and does not by itself downgrade SUCCESS.

## 5. Rollback (document always; execute only on FAILED-*)

Real procedure (no invented commands):

1. `rm -rf /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb`
   (real-dir removal — step 2 used `cp -R`, not symlinks).
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
- Do not symlink directories under
  `/Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/` —
  `kb_discovery.py` skips them. Use real dirs + `cp` (already specified
  in step 2).
- Do not auto-retry on FAILED-*. One shot, then report.
- Do not infer intent from a malformed validation-result. ABORT-VALIDATION
  is the correct response.
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
   result.
2. **Staging** — exact commands run, ls/stat verification (verbatim).
3. **Ingest** — exact command, metrics (wallclock, KB delta, chunk count),
   warnings, errors.
4. **Smoke retrieve** — table: query | hit_count | top-3 (path + score) |
   wallclock | degraded_components.
5. **Final Status** — one of the 6 canonical labels + 1–2 sentences of
   justification linked to evidence in 1–4.
6. **Rollback** — documented procedure; if executed, paste evidence
   (commands + outputs).
7. **Next actions for the pirata side** — up to 5 plain-text bullets.
   Each bullet = 1 concrete action (e.g., "run Unit 3 then re-stage with
   fresh cp", "add markdown wrapper under kb/per-movie/", "open kh
   issue: license metadata field"). No formatted acceptance criteria.

# Verification before you ship the report

- Final Status is exactly one of the 6 canonical labels.
- Step 2 + 3 commands are pasted verbatim and reproducible.
- Step 4 retrieve calls were issued literally (with `kb_slugs=["pirata-kb"]`
  and `explain_retrieval=True`); results pasted verbatim.
- On FAILED-*, rollback was executed and `list_kbs` evidence shows
  `pirata-kb` is gone.

---

# validation-result

Decision: GO-WITH-CAVEATS

Slug: pirata-kb

Staging method: real directories + `cp` (NOT symlinks). Driven by
`kb_discovery.py:489` and `:736` which both contain
`if child.is_symlink(): continue`, so symlinked directories are
silently skipped during discovery. Plain `cp` on the 1 JSON file
(~68 KB) and 1 manifest.jsonl (~70 KB) is trivially cheap; re-stage
on Unit 3 cycles is also cheap.

Stage path: `/Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/`

Required sub-layout: `04-derived/` (per kh discovery convention; one of
`{01-notes, 02-sources, 04-derived, 05-packs}` is required, and
`04-derived` is the semantic fit for auto-generated per-movie manifests
extracted from torrent files).

Exact staging commands (these are also embedded in step 2 above; they
match by design):

```
mkdir -p /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/per-movie
cp /Users/vidigal/claude-code/pirata/kb/per-movie/who-framed-roger-rabbit-1988.json \
   /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/per-movie/who-framed-roger-rabbit-1988.json
cp /Users/vidigal/claude-code/pirata/kb/manifest.jsonl \
   /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/manifest.jsonl
```

Exclusions:
- `kb/frames/**/*.jpg` — do NOT copy. kh `visual_memory_status` is
  `cross-modal+manifest`, but image embedding for retrieve is
  unverified and not required for v1 pipeline test. Defer multimodal
  indexing to Phase 2.
- `kb/contact-sheets/**/*.jpg` — same reasoning.

License stance: IMDb non-commercial license will apply once Unit 3
enriches manifests with IMDb fields. The current Roger Rabbit JSON has
none, so license is moot for v1. kh has no license metadata field
convention (verified). Document the constraint in pirata's `kb/README.md`
preamble as a follow-up; do NOT block ingest on it.

Caveats (these define GO-WITH-CAVEATS, not clean GO):

1. Roger Rabbit JSON predates Unit 3 KB-enrichment, so it has NO IMDb
   fields (tconst, rating, top_cast, akas, genres, director, plot).
   Treat this ingest as PIPELINE TEST, not semantic-recall test.
2. JSON files don't fit kh's markdown-oriented chunking ideally. A
   markdown wrapper (`kb/per-movie/<slug>.md` with YAML frontmatter +
   body) would improve chunk quality. Deferred to a follow-up
   iteration; v1 ingest accepts raw JSON.
3. Re-ingest will be needed after Unit 3 ships (enriched manifests
   replace bare ones). Confirm idempotency at that time.
4. `degraded_components` on retrieve calls is expected because pirata-kb
   has no CAG pack — that's normal for a freshly registered KB without
   a curated pack. Does not by itself downgrade SUCCESS.
5. Smoke queries were chosen to ground in actual JSON content. The
   strings `"Roger Rabbit"`, `"Who Framed Roger Rabbit (1988)"`, and
   `"who-framed-roger-rabbit-1988"` all appear literally in the JSON.
   If kh tokenizes JSON as text, all 3 should hit. If kh skips JSON
   files entirely (extension filter), all 3 will return 0 → FAILED-SMOKE
   is the correct outcome and will trigger rollback. That is the
   intended pipeline test.

Smoke queries: use exactly the 3 queries already specified in step 4
of this prompt. Do not substitute.
