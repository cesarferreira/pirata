# Prompt — primeiro ingest da pirata kb/ no knowledge-hub (v2-eng-codex)

**Para colar em:** agente baseado em **OpenAI GPT-5.5-Codex** com `reasoning_effort=xhigh`
**Pré-requisito recomendado:** rodar antes o prompt de validação (cole o relatório no slot final). Se pular, o prompt aborta com `ABORT-VALIDATION`.

**Por que esta versão existe (delta vs v2-eng):**
- Markdown sectional headers em vez de XML tags (Codex prompting guide)
- Outcome-first framing (xhigh figures out HOW; we state WHAT)
- Static prefix / dynamic suffix para prompt caching
- Drop `ULTRATHINK` (xhigh já é máximo) e drop date stamps
- Stopping criteria EXPLÍCITAS (Codex 5.5 default = persist end-to-end; ops destrutivas precisam stop hard)
- Preamble cadence baked in: 1-sentence ack + 1-sentence plan antes de tool calls; update a cada 1-3 steps
- Status enum apresentada como **success criteria + stop conditions**, não como label decorativa

**Histórico:**
- v1: PT-BR, kwargs fictícios em `ingest_sync()`. NÃO USAR.
- v2: PT-BR, pós-review por 4 personas, source code do kh validado.
- v2-eng: tradução EN da v2.
- v2-eng-codex (este): rewrite estrutural pra GPT-5.5-Codex / xhigh.

**Sources consultadas pra esta versão:**
- [GPT-5 prompting guide — OpenAI Cookbook](https://cookbook.openai.com/examples/gpt-5/gpt-5_prompting_guide)
- [GPT-5.2 Prompting Guide — Cookbook](https://cookbook.openai.com/examples/gpt-5/gpt-5-2_prompting_guide)
- [Codex Prompting Guide — Cookbook](https://developers.openai.com/cookbook/examples/gpt-5/codex_prompting_guide)
- [Using GPT-5.5 — OpenAI dev docs](https://developers.openai.com/api/docs/guides/latest-model)
- [GPT-5.5 prompting guide — Simon Willison, Apr 2026](https://simonwillison.net/2026/Apr/25/gpt-5-5-prompting-guide/)
- [Introducing GPT-5.5 — OpenAI](https://openai.com/index/introducing-gpt-5-5/)

---

```
# Task

First ingest of the pirata workspace's `kb/` folder into the local
knowledge-hub MCP server. Single-shot operation; pirata maintainers
will pause if you stop. Treat as destructive-ish (creates new KB,
mutates FS via staging, touches indices).

# Context (verified — cite source if you doubt a claim)

- knowledge-hub `ingest_sync()` has a ZERO-arg signature
  (`/Users/vidigal/projects/knowledge-hub/src/knowledge_hub/mcp_server.py`).
  No `path`, `kb_slug`, glob filter, or license metadata kwargs exist.
- KB discovery happens under `settings.public_bridge_root`
  (= `/Users/vidigal/knowledge-base/`). To ingest a directory outside
  that root, stage it there first via symlink or copy; convention is
  defined in
  `/Users/vidigal/projects/knowledge-hub/src/knowledge_hub/kb_discovery.py`.
- No MCP-exposed delete-KB tool. `delete_kb` is internal
  (`runtime.py`); the catalog fires it when a KB disappears from FS.
- `retrieve(query, kb_slugs, mode, explain_retrieval, ...)` — call
  with `kb_slugs=["<slug>"]` and `explain_retrieval=True` or smoke
  results will be polluted by the other 32 KBs.
- pirata kb/ is NOT registered today (verified in
  `/Users/vidigal/claude-code/pirata/MEMORY_DEEP_003.md`).
- pirata `kb/` contents:
  - `kb/per-movie/who-framed-roger-rabbit-1988.json` (only populated
    movie). Predates Unit 3 KB-enrichment, so it lacks IMDb fields
    (tconst, rating, top_cast, akas, genres). Treat this ingest as
    PIPELINE TEST, not semantic-recall test.
  - `kb/frames/who-framed-roger-rabbit-1988/*.jpg`
  - `kb/contact-sheets/who-framed-roger-rabbit-1988__sheet_*.jpg`
  - `kb/manifest.jsonl` (~1 line)
- pirata SessionStart reported: `[knowledge-hub] status=? profile=?
  · retrieval may be reduced`. Verify fresh state in preflight.

# Success criteria (these are also your stopping rules)

Use exactly one of these labels in the Final Status of your report.
Each label is a terminal state — do not invent labels or chain
states.

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
- Slug collision check: if the slug you intend to use already
  appears in `list_kbs` → ABORT-PREFLIGHT. Do not auto-suffix.
- Staging-convention check: confirm `kb_discovery.py` will discover
  the pirata `kb/` layout (per-movie JSONs + manifest.jsonl + asset
  dirs) without a top-level manifest the pirata side does not
  provide. If it requires a manifest pirata lacks → ABORT-PREFLIGHT
  and propose the minimal manifest in your report.
- Read the validation-result block (at the bottom of this prompt):
  - Contains GO or GO-WITH-CAVEATS → extract slug + exclusions, proceed.
  - Anything else (NO-GO, UNKNOWN, empty, malformed, contradictory)
    → ABORT-VALIDATION. Do not inline-revalidate. Ask Vidigal to
    paste a valid result and re-fire this prompt.

## 2. Staging (mutates FS — preamble required)

Before running the staging command, emit a 1-sentence acknowledgement
+ 1-sentence plan stating the exact path you'll create.

- Default action: symlink.
  `ln -s /Users/vidigal/claude-code/pirata/kb /Users/vidigal/knowledge-base/<slug>`
- If `kb_discovery.py` requires a real directory (not a symlink),
  use `cp -R` and note the trade-off (re-ingest needs re-copy) in
  the report.
- Verify with `ls -la /Users/vidigal/knowledge-base/<slug>` and
  `stat`. Paste outputs verbatim in the report.

## 3. Ingest (zero-arg call, preamble required)

Emit a 1-sentence preamble before firing.

- Call: `mcp__knowledge-hub__ingest_sync()`. **No kwargs.**
- Capture: wallclock, KBs known before/after, chunk count for the
  new KB, warnings, errors.
- On error or partial completion → Final Status = FAILED-INGEST.
  Skip step 4. Go to step 5 and execute rollback. **Do not retry.**

## 4. Smoke retrieve (3 literal calls)

Emit a 1-sentence preamble before the batch.

Run these three calls verbatim. Use `<slug>` from step 1d. Paste
literal results.

```
mcp__knowledge-hub__retrieve(
  query="Roger Rabbit",
  kb_slugs=["<slug>"],
  mode="auto",
  explain_retrieval=True,
)
mcp__knowledge-hub__retrieve(
  query="1988 noir cartoon detective",
  kb_slugs=["<slug>"],
  mode="auto",
  explain_retrieval=True,
)
mcp__knowledge-hub__retrieve(
  query="Bob Hoskins",
  kb_slugs=["<slug>"],
  mode="auto",
  explain_retrieval=True,
)
```

For each: hit_count, top-3 (path + score), wallclock,
`degraded_components` from the `explain` payload.

If ≥1 query crashed OR all 3 returned hit_count = 0 → Final Status =
FAILED-SMOKE. Go to step 5 and execute rollback.

## 5. Rollback (document always; execute only on FAILED-*)

Real procedure (no invented commands):

1. `rm /Users/vidigal/knowledge-base/<slug>` (unlink the symlink) —
   or `rm -rf` if step 2 used `cp -R`.
2. `mcp__knowledge-hub__ingest_sync()` — catalog reconciles, fires
   the internal `delete_kb`.
3. `mcp__knowledge-hub__list_kbs()` — verify `<slug>` is gone.

In FAILED-INGEST and FAILED-SMOKE: execute these 3 steps and paste
evidence (commands + outputs) in the report.
In SUCCESS / SUCCESS-WITH-CAVEATS: only document the procedure;
do not execute.

# Constraints

- Do not call `workspace_sync`, `delete_kb` (internal), or invent
  kwargs on `ingest_sync()`. If you need a missing capability,
  ABORT-PREFLIGHT and surface it in "Next actions".
- Do not write inside `/Users/vidigal/claude-code/pirata/kb/`. Read-
  only. Pirata owns that path.
- Do not auto-retry on FAILED-*. One shot, then report.
- Do not infer intent from a malformed validation-result.
  ABORT-VALIDATION is the correct response.
- No emojis. No filler. No "Got it / Aha / Good catch" preamble
  tics. Pragmatic, peer-to-peer tone.

# Stopping criteria (hard)

- Any ABORT-* → stop after the report; do not retry, do not refire,
  do not stage, do not ingest.
- Any FAILED-* → execute rollback (step 5), then stop.
- SUCCESS / SUCCESS-WITH-CAVEATS → write the report and stop. Do
  not pre-warm cache, do not run extra queries, do not propose
  follow-up commands.

# Preamble cadence

- Before each tool batch: 1-sentence acknowledgement + 1-sentence
  plan. Combined ≤ 2 sentences.
- Update cadence: every 1–3 execution steps. Hard floor: at least
  one preamble per 6 steps or per 10 tool calls.
- Preambles are status, not narration. State what you are about to
  check or do; skip what you already did.

# Output contract (the report)

Deliver one markdown document with these sections, in order:

1. **Preflight** — health/list_kbs/topology output (verbatim
   excerpts), validation-result decision, slug-collision result,
   staging-convention result.
2. **Staging** — exact command, ls/stat verification (verbatim).
3. **Ingest** — exact command, metrics (wallclock, KB delta, chunk
   count), warnings, errors.
4. **Smoke retrieve** — table: query | hit_count | top-3 (path +
   score) | wallclock | degraded_components.
5. **Final Status** — one of the 6 canonical labels +
   1–2 sentences of justification linked to evidence in 1–4.
6. **Rollback** — documented procedure; if executed, paste evidence
   (commands + outputs).
7. **Next actions for the pirata side** — up to 5 plain-text
   bullets. Each bullet = 1 concrete action (e.g., "run Unit 3 then
   re-stage", "add markdown wrapper under kb/per-movie/", "open
   kh issue: license metadata field"). No formatted acceptance
   criteria.

# Verification before you ship the report

- Final Status is exactly one of the 6 canonical labels.
- Step 2 + 3 commands are pasted verbatim and reproducible.
- Step 4 retrieve calls were issued literally (with `kb_slugs` and
  `explain_retrieval=True`); results pasted verbatim.
- On FAILED-*, rollback was executed and `list_kbs` evidence shows
  `<slug>` gone.

---

# validation-result

[VIDIGAL: paste the GO / GO-WITH-CAVEATS / NO-GO report from the
prior validation prompt here. If you have not run validation,
write UNKNOWN. Do not leave empty.]

```
