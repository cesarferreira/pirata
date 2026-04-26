# Prompt — primeiro ingest da pirata kb/ no knowledge-hub (v2-eng)

**Criado:** 2026-04-25
**Para colar em:** agente que mantém o `knowledge-hub` MCP no Dante
**Idioma:** EN (modelo-facing — perf maior em raciocínio técnico denso)
**Pré-requisito recomendado:** rodar antes o prompt de validação (cole o relatório no slot `<validation-result>`). Se pular, este prompt aborta cedo com `ABORT-VALIDATION`.

**Histórico:**
- v1 em `/tmp/donna_ingest_prompt_v1.md` — assumia kwargs em `ingest_sync()` que não existem. NÃO USAR.
- v2 PT-BR em `2026-04-25-kh-ingest-v2.md` — pós-review por 4 personas com leitura do source code do kh.
- v2-eng (este) — tradução literal da v2, mesma semântica, mesmas safety rails. Use quando quiser que o agente raciocine em inglês.

**Status canônicos** (definidos em `<canonical-statuses>`): `SUCCESS`, `SUCCESS-WITH-CAVEATS`, `ABORT-PREFLIGHT`, `ABORT-VALIDATION`, `FAILED-INGEST`, `FAILED-SMOKE`.

---

```
<role>
You are the agent maintaining the knowledge-hub MCP server on Dante.
ULTRATHINK before any step that mutates FS or kh state. Destructive
operations deserve a second thought.
</role>

<context>
Vidigal wants to perform the first ingest of the pirata `kb/` folder
into the kh. This request comes from the Claude Code session running
in the pirata workspace at `/Users/vidigal/claude-code/pirata`.

Verified facts (cite the source if you doubt them):
- `ingest_sync()` has a ZERO-arg signature
  (`/Users/vidigal/projects/knowledge-hub/src/knowledge_hub/mcp_server.py`).
  It does NOT accept `path`, `kb_slug`, glob filters, or license
  metadata.
- KB discovery happens under `settings.public_bridge_root`, currently
  `/Users/vidigal/knowledge-base/`. To ingest a directory outside
  that root, it must be STAGED there first (symlink or copy)
  following the convention defined in
  `/Users/vidigal/projects/knowledge-hub/src/knowledge_hub/kb_discovery.py`.
- There is NO MCP-exposed delete-KB tool. `delete_kb` is internal
  (`runtime.py`) and is fired by the catalog when a KB disappears
  from the FS.
- `retrieve` takes `(query, kb_slugs, mode, explain_retrieval, ...)`.
  Without `kb_slugs` the smoke test will collide with the other 32
  KBs.
- pirata kb/ is NOT registered today (verified in
  `MEMORY_DEEP_003.md`). All watched roots live under
  `~/knowledge-base/`.
- The pirata SessionStart for this run reported degradation:
  `[knowledge-hub] status=? profile=? · retrieval may be reduced`.

Pirata kb/ state:
- 1 movie populated: Who Framed Roger Rabbit 1988
  (`kb/per-movie/who-framed-roger-rabbit-1988.json` + frames JPG +
  contact-sheets JPG + ~1 line in `kb/manifest.jsonl`).
- The JSON predates Unit 3 (KB enrichment), so it does NOT contain
  the IMDb fields (tconst, rating, top_cast, akas, genres). The
  value of this ingest is PIPELINE TEST, not semantic recall —
  acknowledge this in your decision.

Full project plan:
`/Users/vidigal/claude-code/pirata/docs/plans/2026-04-24-004-feat-imdb-local-pirata-coupling-plan.md`
(Unit 6 op-step lists this ingest as post-Unit-3; we are testing the
path early on purpose.)
</context>

<validation-result>
[VIDIGAL: paste here the GO/NO-GO + parametrization report (suggested
slug, exclusions, license stance) produced by the kh agent in the
prior validation round. If you have not run validation yet, write
UNKNOWN. If NO-GO, paste the reason. Do not leave this empty.]
</validation-result>

<canonical-statuses>
Use exactly one of these 6 labels in the report's Final Status:
- SUCCESS              — ingest ok + ≥2 of 3 smoke queries hit
- SUCCESS-WITH-CAVEATS — ingest ok with declared gap (license,
                         suboptimal chunking, partial smoke 1/3)
- ABORT-PREFLIGHT      — kh degraded, slug collision, or
                         non-deterministic staging convention
- ABORT-VALIDATION     — `<validation-result>` is UNKNOWN, empty,
                         malformed, contradictory, or NO-GO
- FAILED-INGEST        — `ingest_sync()` errored or completed partial
- FAILED-SMOKE         — all 3 queries returned 0 hits or crashed
                         after an apparently successful ingest
</canonical-statuses>

<task>
Execute in order. ULTRATHINK before any step that mutates state.

1. **Preflight (zero side-effect)**
   a. Run fresh `health` + `list_kbs` + `topology`. If any core
      component (vector store, reranker, ingest worker) is degraded
      → status=ABORT-PREFLIGHT, stop, and report.
   b. Confirm NO existing KB collides with the slug you intend to
      use (e.g., `pirata-kb` or whatever `<validation-result>`
      dictated). On collision → ABORT-PREFLIGHT.
   c. Confirm the staging convention in `kb_discovery.py` is
      deterministic for the pirata kb/ layout (per-movie JSONs +
      manifest.jsonl + asset dirs). If the convention requires a
      top-level manifest the pirata side does not provide →
      ABORT-PREFLIGHT and propose the minimal manifest in the report.
   d. Read `<validation-result>`:
      - GO or GO-WITH-CAVEATS → extract slug + exclusions and proceed.
      - NO-GO, UNKNOWN, empty, malformed, or contradictory
        → status=ABORT-VALIDATION. Do NOT inline-revalidate. Ask
        Vidigal to paste a valid result and re-fire.

2. **Staging (mutates FS — announce and execute in this turn)**
   - Create `~/knowledge-base/<slug>/` as a SYMLINK pointing to
     `/Users/vidigal/claude-code/pirata/kb/`. Exact command:
     `ln -s /Users/vidigal/claude-code/pirata/kb /Users/vidigal/knowledge-base/<slug>`
   - If the kh convention requires a copy instead of a symlink
     (to be confirmed in 1c), use `cp -R` and note the trade-off in
     the report (future re-ingest needs to re-copy).
   - Paste the exact command you ran + ls/stat verification.

3. **Ingest** (zero-arg call)
   - Fire `mcp__knowledge-hub__ingest_sync()`. NO kwargs.
   - Capture: wallclock, KB count before/after discovery, chunk
     count generated for the new KB, warnings, errors.
   - On error or partial completion → status=FAILED-INGEST. Do NOT
     auto-retry. Proceed to step 5 (rollback dry-run + report).

4. **Smoke retrieve** (literal scoped queries)
   Run these 3 calls verbatim and paste literal results:
   a. `mcp__knowledge-hub__retrieve(query="Roger Rabbit", kb_slugs=["<slug>"], mode="auto", explain_retrieval=True)`
   b. `mcp__knowledge-hub__retrieve(query="1988 noir cartoon detective", kb_slugs=["<slug>"], mode="auto", explain_retrieval=True)`
   c. `mcp__knowledge-hub__retrieve(query="Bob Hoskins", kb_slugs=["<slug>"], mode="auto", explain_retrieval=True)`
   For each: hit_count, top-3 (path + score), wallclock, and
   `degraded_components` from `explain`. If ≥1 query crashed OR all
   3 returned hit_count=0 → status=FAILED-SMOKE + execute rollback.

5. **Rollback procedure** (document; execute ONLY in FAILED-*)
   Real procedure (not invented):
   - `rm /Users/vidigal/knowledge-base/<slug>` (unlink the symlink)
     or `rm -rf` if it was a copy.
   - `mcp__knowledge-hub__ingest_sync()` so the catalog reconciles
     and fires the internal `delete_kb`.
   - Verify: `mcp__knowledge-hub__list_kbs()` no longer lists `<slug>`.
   In any FAILED-*, execute these 3 steps and paste evidence.
   In SUCCESS / SUCCESS-WITH-CAVEATS, only DOCUMENT — do not execute.
</task>

<deliverables>
A. **Technical report** in markdown English:
   1. Preflight findings (health/list_kbs/topology output + decision
      on `<validation-result>` + slug-collision check + staging
      convention check)
   2. Staging executed: exact command + ls/stat verification
   3. Ingest executed: command + metrics + warnings/errors
   4. Smoke retrieve: table of query × hit_count × top-3 × wallclock
      × degraded_components
   5. Final status (one of the 6 canonical labels) + 1-2 sentences
      of justification linked to evidence in sections 1-4
   6. Rollback: documented procedure + (if executed) evidence

B. **Next actions for the pirata-side** (up to 5 plain-text bullets):
   - Each bullet = 1 concrete action (e.g., "run Unit 3 before
     re-ingest", "add markdown wrapper under kb/per-movie/", "open
     issue on kh for license metadata field")
   - No formatted acceptance criteria — this is v1 of the ingest,
     plain text suffices
</deliverables>

<constraints>
- Do NOT call `workspace_sync`, `delete_kb` (internal), or invent
  kwargs on `ingest_sync()`. If you think you need a missing
  capability, ABORT and propose it as a gap in "Next actions".
- Do NOT write anything inside `/Users/vidigal/claude-code/pirata/kb/`.
  Read-only. The pirata side owns that path.
- Do NOT auto-retry on any FAILED-*. Report and stop.
- Do NOT infer intent from a malformed `<validation-result>`.
  ABORT-VALIDATION is the correct response.
- No emojis. No filler. Technical, peer-to-peer tone.
</constraints>

<verify-before-finalizing>
- [ ] Final status is one of the 6 canonical labels
- [ ] Exact staging + `ingest_sync()` commands are pasted and
      reproducible
- [ ] The 3 smoke retrieve queries were called LITERALLY (with
      `kb_slugs` and `explain_retrieval=True`) and literal results
      pasted
- [ ] In FAILED-*, rollback was executed and the evidence
      (`list_kbs` no longer lists the slug) is pasted
</verify-before-finalizing>
```
