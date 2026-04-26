# Memory Deep #004

| Field       | Value                                              |
|-------------|----------------------------------------------------|
| Created     | 2026-04-25 13:37 BRT                               |
| Project     | pirata — personal media download + contact-sheet workspace |
| Session     | Phase 0 real-corpus IMDb ingest fired & verified, Unit 2 (`imdb_lookup.py`) shipped end-to-end with 25/25 smoke + 7/7 fixture, plan doc patched with actuals, then a long DONNA arc producing 4 prompts (validation + ingest v1/v2 PT-BR + v2-eng + v2-eng-codex) culminating in a 4-persona doc-review pass that surfaced FATAL feasibility bugs in v1 (read kh source code), and a final test-fire of v2-eng-codex against Vidigal's kh agent that ABORTED-VALIDATION as designed AND surfaced the real kh staging-convention discovery (`01-notes`/`02-sources`/`04-derived`/`05-packs` under `09-knowledge-base/`) that pirata `kb/` doesn't match. |
| Previous    | MEMORY_DEEP_003.md                                |

---

## ⚡ Continue Trigger (auto-fire on resume)

**When user types `continue`, `continua`, `retoma`, or any variant of "voltei / vamo continuar":**

1. Confirm orientation: read this snapshot, run `git log --oneline -8` to confirm we're at commit `fb4b4a4` or later (NOTHING from session 004 is committed yet).
2. **Surface the open decision blocker** before starting any work: pirata kb/ does NOT match the kh discovery convention. Two paths exist:
   - **Path A (recommended):** symlink-view under `~/knowledge-base/09-knowledge-base/pirata-kb/04-derived/` pointing to pirata's existing `kb/per-movie/` and `kb/manifest.jsonl`. Preserves separation of concerns, reversible, no pirata-side restructure.
   - **Path B:** refactor pirata `kb/` to adopt kh's layout (`kb/04-derived/per-movie/` etc.). Couples pirata to kh's convention.
3. **If user picks A:** compose a `validation-result` GO block from kh agent's preflight intel (already captured in this snapshot under "Technical Notes — kh discovery convention") + paste into `docs/prompts/2026-04-25-kh-ingest-v2-eng-codex.md` slot + re-fire prompt to kh agent.
4. **If user picks B:** restructure pirata `kb/` first (move `per-movie/` → `kb/04-derived/per-movie/`, `manifest.jsonl` → `kb/04-derived/manifest.jsonl`, decide `kb/frames/` and `kb/contact-sheets/` placement), then re-fire validation prompt to get a fresh GO with the new layout, then re-fire ingest.
5. **If neither yet decided:** ask which path; do NOT auto-pick.
6. Once kh ingest succeeds (or fails decisively), **then proceed to Unit 3** (`scripts/contact_sheet.py` `--kb-imdb` flag + `sheets_sweep.py` pass-through + SKILL.md CONTRACT update).
7. **Important warning to surface:** the v2-eng-codex prompt's `# validation-result` slot is at the END of the prompt body. User must scroll to fill it before pasting; if forgotten, kh agent correctly aborts with `ABORT-VALIDATION` (verified 2026-04-25). This is the design, not a bug.

If user qualifies the resume (e.g., "skip ingest decision, do Unit 3 against fixture"), honor that. Trigger is default action, not forced.

---

## Project Context

`pirata` is Vidigal's personal Mac-based media workspace at `~/claude-code/pirata`. Two muscles: (a) a `torrentclaw` MCP for rich movie/TV search with metadata, and (b) a Rust `pirata` CLI scraper for non-TC sources. Downloads go through `aria2c` orchestrated by `scripts/queue.py`. On top: cinema-grade contact-sheet pipeline for human review (`release/contact-sheets/`) and a parallel KB export for RAG-multimodal ingest (`kb/`). Path-agnostic sweeper picks up any new release dir without sheets. **As of session 004:** offline IMDb non-commercial dataset (~12.4 M titles, ~9.3 GB raw TSVs) ingested into a local 15 GB SQLite DB (`imdb/imdb.db`) with FTS5 + B-tree COLLATE NOCASE; Python lookup helper (`scripts/imdb_lookup.py`) live. Unit 3 (KB enrichment in `contact_sheet.py`) is next, gated on the kh ingest decision.

## What Happened This Session

Massive, multi-arc session. Five discrete arcs.

### Arc 1: Continue trigger fired → Phase 0 real-corpus ingest

User typed `continue`. Read MEMORY_DEEP_003.md. Trigger fired Phase 0 acceptance: `python3 -u scripts/imdb_ingest.py --refresh > logs/imdb_ingest.log 2>&1 &`. Pre-flight checks: 96.9 GB free (passed 25 GB gate), all 7 TSVs present, no stale lock, no prior DB.

Started Monitor on log file with grep filter for stage transitions + failure signatures. Streamed events as they fired:

- `title.basics` 12.46 M rows in 29s
- `title.ratings` 1.66 M rows in 2s
- `title.episode` 9.62 M rows in 13s
- `title.crew` 12.46 M rows in 15s
- `name.basics` 15.27 M rows in 27s
- `title.principals` 37.21 M rows kept (filtered from 99 M raw via `category IN ('actor','actress','self')`) in 95s
- 5-min gap (principal name UPDATE-based denorm + supporting indexes)
- `title.akas` 24.29 M rows kept (matches the locked predicate projection, no FR included) in 66s
- B-tree indexes 91s
- FTS5 3 sequential INSERTs: 12.46 M + 175k + 24.29 M = 36.93 M total
- 3.5-min gap (`series_top_cast` window-ranked aggregation + `wal_checkpoint(TRUNCATE)`)
- `integrity_check: ok`, atomic swap, orphan WAL/SHM cleaned
- `state.json` written with SHA-256 source checksums (4s for 7 files)
- "OK refresh complete in 1014.7s (16.9 min)"

**Phase 0 acceptance verification (the snapshot 003 trigger goal):**
- ✅ Exit 0
- ❌ Wallclock 16.9 min (plan target was <10 min — over by ~70%)
- ✅ `integrity_check` ok (script-side, pre-swap)
- ✅ All 10 tables non-zero (8 user tables + ft_titles + ingest_meta)
- ✅ `ft_titles` populated (36.93 M)
- ✅ Atomic swap clean (orphan WAL/SHM cleaned post-swap)
- ✅ `state.json` schema v1 + 7 SHA-256 checksums
- ❌ DB on disk **15 GB** (plan estimate was 500 MB–1 GB — off by 15–30×)
- ✅ B-tree, FTS5, and `series_top_cast` query paths all work end-to-end (smoke queries on Cidade de Deus, Roger Rabbit, etc.)

### Arc 2: Plan doc + script patches for Phase 0 actuals

Two material plan deviations surfaced and patched:

1. **DB size 15 GB vs 1 GB plan** — driver is FTS5 `prefix='2 3'` storing each token 3× (full + 2-char + 3-char prefix indexes) over 36.93M entries, plus name-denorm into `title_principals_top5` (37M rows × ~20 bytes = ~500 MB just for names), plus B-tree COLLATE NOCASE indexes. Not a bug — calibrated against a narrower akas slice that got corrected mid-brainstorm without recomputing disk.

2. **Refresh budget needs raising** — with 15 GB live DB, peak refresh = ~15 GB old + ~15 GB new + WAL ≈ 32–35 GB. Pre-existing `--min-free-gb 25` would let a refresh start but could fail mid-flight. Raised to **35 GB**.

Patches applied:
- `scripts/imdb_ingest.py`: `DISK_FREE_FLOOR_GB = 25` → `35` with inline comment justifying. Docstring `<25 GB → abort` → `<35 GB → abort`.
- `docs/plans/2026-04-24-004-feat-imdb-local-pirata-coupling-plan.md`: Requirements Trace + Approach + Verification + 5 references updated, Unit 1 marked `[x]` with measured wallclock + DB size, "Bulk ingest implementation strategy" Deferred-to-Implementation entry resolved with actuals.

Re-ran Unit 1 smoke test after patches: **46/46 PASS** (no regression — smoke uses `--min-free-gb 1` overrides so default change is harmless).

### Arc 3: Unit 2 — `scripts/imdb_lookup.py` end-to-end

Wrote 437-line Python module:
- Module-level connection cache (read-only via `file:.../imdb.db?mode=ro`).
- `IMDbDBUnavailable(RuntimeError)` exception type.
- Frozen dataclasses: `Match`, `Title`, `Episode`.
- Public API: `lookup_by_title(query, year, kind)`, `lookup_by_tconst(tconst)`, `lookup_episodes(parent_tconst, season)`.
- Tier 1 (exact, B-tree COLLATE NOCASE): 3 queries against `title_basics(primaryTitle)`, `title_basics(originalTitle != primaryTitle)`, and `title_akas(title) JOIN title_basics`. fuzz_ratio = 100.0 explicitly.
- Tier 2 (fuzzy, FTS5 + RapidFuzz): `_build_fts_query` strips FTS5 operators + appends `*` for prefix-match on last token. SELECT joins ft_titles candidates with title_basics via subquery (avoids per-row roundtrips).
- `_classify_field` for aka isOriginalTitle subclassification post-FTS5.
- Composite score = fuzz_ratio × field_multiplier (locked: primary=3.0, original=2.0, aka_original=1.8, aka_regional=1.5).
- CLI with `--year`, `--kind`, `--tconst`, `--episodes`, `--season`, `--db`, `--limit`.

**Bug discovered during initial smoke:** `fuzz.token_set_ratio` returns 100 when query tokens are a SUBSET of candidate tokens (e.g., "Dune" → "Dune World" = 100, "Cidade de Deus" → "Macau, Cidade do Santo Nome de Deus" = 100). Plan had locked `token_set_ratio` but smoke proved it broken — poisoned the tier 1 / tier 2 boundary completely.

**Two fixes applied:**
1. Switched tier 2 scorer to `fuzz.WRatio` (length-aware blend) capped at 99.0.
2. Sort key changed from `(-score, -num_votes)` to `(0 if fuzz_ratio==100 else 1, -score, -num_votes)` — enforces strict tier separation: Tier 1 always ranks above Tier 2 regardless of score.

Re-ran smoke after fixes:
- "Cidade de Deus" no year → tt27556897 (2023) + tt38066322 multi_tie at top (correct disambig behavior); tt0317248 ranks below by formula (multi_tie is the escape hatch via R11a/R11b text-fallback prompt).
- "Cidade de Deus" --year 2002 → tt0317248 ✓
- "Dune" no year → tt0087182 (1984, 192k votes) + tt0142032 (2000) multi_tie (correct).
- "Dune" --year 2021 --kind movie → tt1160419 (Dune: Part One) ✓
- Zero-match → empty list.

### Arc 4: Unit 2 test infra + fixture

Wrote `scripts/tests/test_imdb_lookup.sh` (15 test groups) and `scripts/tests/fixtures/imdb_pt_br_20.txt` (7 of 20 seeded with canonical tconsts).

First run: 23/25 PASS, 2 FAIL — but the failures were **over-specified test assumptions**, not implementation bugs:
- Test 6 expected tt1160419 in no-kind Dune 2021 top-5 — but ~10 tvEpisodes named "Dune" (2021) outrank by primary multiplier. Realistic Phase-1 use always passes `--kind movie`. Tightened test to require `--kind movie`.
- Test 14 fed `Dune: Part Two * (2024)` and expected tt15239678 — but the trailing "2024" AND-token kicked tt15239678 out (its primaryTitle has no "2024"). Caller (PTT) extracts year separately so `lookup_by_title` should never see a 4-digit year in the title arg. Reworded test to use punctuation only.

Re-ran: **25/25 PASS + fixture 7/7**. p99 latency = 4.0 ms (1000-query probe vs target <50 ms).

Plan doc patched: Unit 2 marked `[x]` with shipped 2026-04-25 + smoke results + deviations noted. Three additional plan locations updated for the WRatio + tier-separation deviations:
- "Disambiguation composite score formula" Key Decision (locked formula updated)
- Unit 2 Approach (token_set_ratio note replaced with WRatio rationale)
- Risks table row about RapidFuzz wrong-tconst poisoning (updated with actual fix detail + remaining "same-primary-title clash" gap surfaced via fixture)

### Arc 5: DONNA prompt arc → 4 prompts produced + 4-persona review

Long sub-arc. User asked DONNA-mode questions in sequence, escalating each turn:

**Step 1 (validation prompt PT-BR):** User asked "está ingerido no kh? se sim, escreva prompt pra agente do kh com contexto + pedido de testes/melhorias". Honest answer: NO, nothing ingested yet. Wrote forward-looking validation prompt with `<resultado-validacao>` slot, 9 technical questions (schema fit, wrapper md, multimodal, ingest semantics, license flag, registro vs watched root, etc.), two structured deliverables (relatório + prompt-resposta autocontido).

**Step 2 (ingest prompt v1 PT-BR):** User asked "podemos otimizar prompt pra ele fazer ingest". I led with both-sides reasoning per `feedback_show_reasoning.md` (against: kb/ has 1 movie pre-Unit-3, premature; for: tests pipeline early). Recommended two-step (validation first, then ingest) but produced conditional ingest prompt with `<validation-result>` slot.

v1 had 5-step procedure (preflight, plano, execução, smoke, rollback), 4-status enum (SUCCESS / SUCCESS-COM-RESSALVAS / ABORT / FAILED), 7 restrições, 8-checkbox final verify. Saved to `/tmp/donna_ingest_prompt_v1.md`.

**Step 3 (`/ce-doc-review` on v1):** User asked to review v1 with `/ce-doc-review` subagent. Spawned 4 personas in parallel:
- adversarial — surfaced 8 findings incl. "aprovação implícita" auto-authorization, smoke retrieve crash unhandled, empty/malformed `<validation-result>` not handled, status enum incomplete
- coherence — surfaced 5 findings incl. status enum drift across sections (8 distinct names), dangling "9 perguntas" reference, mixed EN/PT in tags
- **feasibility — read the actual kh source code at `/Users/vidigal/projects/knowledge-hub/`** and surfaced 6 catastrophic findings:
  - `ingest_sync()` is **ZERO-arg** (no path, kb_slug, glob filter, license)
  - Discovery only under `settings.public_bridge_root` (= `/Users/vidigal/knowledge-base/`)
  - No MCP-exposed delete-KB tool (`delete_kb` is internal, fired by catalog when KB disappears from FS)
  - `retrieve` requires `kb_slugs=[...]` + `explain_retrieval=True`
  - Missing license metadata field convention
  - Validation re-run dangling reference
- scope-guardian — surfaced 6 findings incl. inline UNKNOWN re-validation contradicts two-step design, 8-checkbox verify is redundant, prompt-resposta autocontido is over-engineering for v1

Without the feasibility review, v1 would have been pasted and the kh agent would have crashed at runtime calling fictional kwargs.

**Step 4 (v2 PT-BR):** Synthesized findings, applied fixes:
- Rewrote step 2 entirely: staging by symlink under `~/knowledge-base/<slug>/` (real procedure)
- Rewrote step 4 with literal `mcp__knowledge-hub__retrieve(query=..., kb_slugs=[...], mode='auto', explain_retrieval=True)` calls
- Rewrote step 5 with real rollback (rm staging + reconcile via ingest_sync + verify list_kbs)
- Removed "license metadata" field (unsupported); license becomes "Próximas ações" gap item
- Killed inline UNKNOWN re-validation (UNKNOWN/empty/malformed/contradictory → ABORT-VALIDATION)
- Renamed `<validation-result>` → `<resultado-validacao>` for tag consistency
- Defined canonical 6-status enum in own block: `SUCCESS / SUCCESS-COM-RESSALVAS / ABORT-PREFLIGHT / ABORT-VALIDATION / FAILED-INGEST / FAILED-SMOKE`
- Added slug-collision check in step 1b
- Trimmed `<verifique-antes-de-finalizar>` 8 → 4 items
- Replaced "prompt-resposta autocontido" deliverable with simple 5-bullet list
- Trimmed restrições 7 → 5

Saved at `/Users/vidigal/claude-code/pirata/docs/prompts/2026-04-25-kh-ingest-v2.md`.

**Step 5 (v2-eng):** User asked English version. Translated literally — same semantic, same safety rails, tags renamed (`<papel>` → `<role>`, `<resultado-validacao>` → `<validation-result>`, etc.), `SUCCESS-COM-RESSALVAS` → `SUCCESS-WITH-CAVEATS`. Saved to `2026-04-25-kh-ingest-v2-eng.md`.

**Step 6 (v2-eng-codex for GPT-5.5/xhigh):** User asked Codex-optimized version with research via firecrawl + WebSearch. Pulled 6 authoritative sources (OpenAI Cookbook GPT-5/5.1/5.2 prompting guides, GPT-5.5 "Using" doc, Codex Prompting Guide cookbook, Simon Willison's GPT-5.5 prompting guide dated 2026-04-25, NVIDIA blog, plus local `gpt-5-4-prompting` skill SKILL.md). Synthesized rules:

1. Markdown sectional headers, NOT XML tags (Codex prompting guide explicitly)
2. Outcome-first framing (xhigh figures HOW; we state WHAT)
3. Drop the date (model knows UTC)
4. Static prefix / dynamic suffix for prompt caching (validation-result slot moved to END)
5. Drop ULTRATHINK (xhigh is already maximum reasoning)
6. xhigh is dangerous with weak stopping criteria → tightening is the defense
7. Preamble cadence: 1-sentence ack + 1-sentence plan before tool batches; refresh every 1-3 steps; floor 6 steps / 10 tool calls
8. Codex 5.5 default = persist end-to-end → must explicitly say "do not pre-warm cache, do not run extra queries" in SUCCESS branch
9. Treat as new model family — don't transfer Claude-XML literal

Restructured prompt accordingly: status enum became table with "Action on hit" column per row, dedicated "Stopping criteria (hard)" and "Preamble cadence" sections, validation-result slot at the END of prompt body. Saved to `2026-04-25-kh-ingest-v2-eng-codex.md`.

**Step 7 (test fire on Vidigal's kh agent):** User pasted v2-eng-codex without filling the validation-result slot. The kh agent did EXACTLY what the prompt designed:
- Ran preflight (`health` + `list_kbs` + `topology`) and pasted real outputs
- Decided ABORT-VALIDATION because slot was empty
- Did NOT mutate FS, did NOT run ingest, did NOT run smoke
- Documented (but did not execute) the rollback procedure
- Final Status = ABORT-VALIDATION ✓

**Critical real intel surfaced in the kh agent's preflight:**
- `bridge_root` = `/Users/vidigal/knowledge-base/`
- `canonical_workspace_root` = `/Users/vidigal/knowledge-base/`
- `runtime_root` = `/Users/vidigal/.knowledge-hub/`
- `knowledge_base_root` = `/Users/vidigal/knowledge-base/09-knowledge-base/` ← **THIS IS THE DISCOVERY ROOT**
- `hub_profile` = `local_full_power_plus`, `status` = `ok` (so the SessionStart degradation banner was stale/misleading)
- KBs need sub-layout `01-notes` / `02-sources` / `04-derived` / `05-packs` to be discoverable
- Existing slugs sample: `actor-direction`, `agent-orchestration`, `approved`, `benchmarks-and-evals`, `workflow-orchestration` — all CAG-pack-flagged (per-KB metadata)
- pirata `kb/` layout (`per-movie/`, `frames/`, `contact-sheets/`, `manifest.jsonl`) does NOT match the sub-layout convention → would also fail ABORT-PREFLIGHT even if validation slot were filled

User reaction: "ta dando algum pau" (it's failing). Corrected: not failing — both safety rails functioned correctly AND surfaced a real architectural blocker (staging convention mismatch). Surfaced two paths forward (A: symlink view; B: refactor pirata layout), recommended A.

Session ended at decision point: which path? Awaiting user response. Pending: compose validation-result block + save kh staging-convention memo.

## Decisions Made

- **Decision:** Phase 0 pre-flight floor raised 25 → 35 GB — **Why:** measured live DB at 15 GB; peak refresh = old + new + WAL ≈ 32-35 GB. 25 GB would let refresh start but fail mid-flight. Patched script + plan in same change.
- **Decision:** Tier 2 scorer is `fuzz.WRatio` capped at 99, NOT `fuzz.token_set_ratio` — **Why:** smoke proved token_set_ratio returns 100 on token-subset overlap, breaking tier 1 / tier 2 boundary. Documented as Unit 2 implementation deviation in plan.
- **Decision:** Sort key uses `(0 if fuzz_ratio == 100 else 1, -score, -num_votes)` — **Why:** enforces strict tier separation. Plan said "3-tier ranking" but Unit 2 implementation didn't initially enforce it; added.
- **Decision:** Multi-tie threshold uses `fuzz_ratio == 100` as Tier 1 marker — **Why:** clean predicate matching the new sort definition; only Tier 1 (exact match) reaches 100, Tier 2 caps at 99.
- **Decision:** Patches uncommitted on main; user controls commit timing — **Why:** CLAUDE.md safety rule "NEVER commit unless explicitly asked". Suggested boundaries: (a) `fix(imdb): raise pre-flight 25→35 GB after Phase 0 actuals`, (b) `feat(imdb): Unit 2 — FTS5+RapidFuzz lookup with tier-separated WRatio`.
- **Decision:** Path A (symlink view under `~/knowledge-base/09-knowledge-base/pirata-kb/04-derived/`) is the recommended path for kh staging — **Why:** preserves separation of concerns, reversible, no pirata-side restructure, pirata `kb/` stays canonical for pirata's own use; the kh "sees" via 04-derived view. Path B (refactor pirata `kb/` to adopt kh's `01-notes/02-sources/04-derived/05-packs` layout) couples pirata to kh's convention. Decision still pending user pick.
- **Decision:** v1 ingest prompt is invalid — **Why:** assumed kwargs in `ingest_sync()` (`path`, `kb_slug`, `INCLUI`, `EXCLUI`, license) that don't exist; feasibility reviewer read `/Users/vidigal/projects/knowledge-hub/src/knowledge_hub/mcp_server.py` and confirmed zero-arg signature. Saved to `/tmp/donna_ingest_prompt_v1.md` with NÃO USAR header. v2 + v2-eng + v2-eng-codex are the live versions.
- **Decision:** v2-eng-codex uses markdown sectional headers, not XML tags — **Why:** Codex Prompting Guide cookbook explicitly favors markdown organization; XML is Claude-native and forces Codex to re-map. Drops `ULTRATHINK` (xhigh is already maximum). Drops timestamps (model knows UTC).
- **Decision:** Static prefix / dynamic suffix in v2-eng-codex — **Why:** prompt caching wins. Verified facts + status enum + procedure stay at top (cache-friendly); `# validation-result` slot moved to BOTTOM of prompt body so the dynamic part doesn't invalidate the static prefix. Per OpenAI's Using GPT-5.5 doc.
- **Decision:** kh degradation banner from SessionStart is misleading — **Why:** kh agent's runtime preflight returned `hub_profile=local_full_power_plus, status=ok`. The `[knowledge-hub] status=? profile=?` banner is from SessionStart hook timing, not actual runtime degradation. Don't rely on it.

## Current State

**Working / done:**
- Phase 0 verified end-to-end: 15 GB SQLite at `imdb/imdb.db`, integrity ok, 10 tables populated, 36.93M ft_titles, B-tree + FTS5 + series_top_cast all queryable.
- Unit 2 shipped: `scripts/imdb_lookup.py` (437 lines), `scripts/tests/test_imdb_lookup.sh` (15 test groups, 25/25 PASS), `scripts/tests/fixtures/imdb_pt_br_20.txt` (7 seeded).
- Latency p99 = 4.0 ms (vs target <50 ms).
- Plan doc fully updated: Phase 0 actuals, 35 GB floor, Unit 1 + Unit 2 marked `[x]`, deviations documented (WRatio, tier separation), Risks table row updated.
- 3 prompt versions ready in `docs/prompts/`: v2 PT-BR, v2-eng, v2-eng-codex.
- `/tmp/donna_ingest_prompt_v1.md` exists but **DO NOT USE** (assumed bogus kwargs).
- kh agent test-fire returned ABORT-VALIDATION as designed; preflight intel captured.

**Pending decision (BLOCKING for kh ingest):**
- Path A vs Path B for staging pirata `kb/` to match kh discovery convention.
- Awaiting user pick.

**Untouched working tree (pre-existing carry-over from prior sessions, not owned by 004):**
- `M .gitignore` (no — was committed in 003; nothing modified)
- Untracked: `.claude/commands/`, `CLAUDE.md`, `MEMORY_DEEP_001.md`, `MEMORY_DEEP_002.md`, `MEMORY_DEEP_003.md`, prior brainstorms + plans, `kb/` (excluding new files), `logs/` (with new `imdb_ingest.log`).

**Disk:** 81 GB free post-ingest (was 96.9 pre). DB occupies 15 GB. WAL/SHM cleaned. State of the volume is healthy.

## Done (Cumulative)

- [x] `/pirata` skill spec'd (TR-100 monochrome, 12-branch menu)
- [x] Memory feedback saved: ANSI escapes don't render in Claude code fences
- [x] `scripts/queue.py` — aria2c wrapper
- [x] `scripts/contact_sheet.py` — full pipeline (scdet → extract → label → tile)
- [x] Caption strip below thumb design
- [x] LLM-readable label fonts (auto-scaled with thumb width)
- [x] fps auto-detection via `probe_fps()`
- [x] Slug-prefixed sheet filenames
- [x] scdet result caching for fast re-runs
- [x] `scripts/sheets_sweep.py` — opportunistic sweeper, path-agnostic
- [x] sweep-level flock + security defenses (resolve+is_relative_to, --terminator, repr-sanitize, killpg)
- [x] `scripts/queue.py` `--autosheets`/`--no-autosheets` integration
- [x] `/pirata` skill panel rows: STATUS / DOCTOR / SHORTLIST
- [x] `scripts/tests/test_sweep.sh` — 12 assertions
- [x] `contact_sheet.py --kb-export` + `--kb-force` flags
- [x] `tile_sheets()` (clean= mode removed in session 002)
- [x] `export_kb()` — frames JPEG + sheets JPEG + per-movie JSON + JSONL append
- [x] `sheets_sweep.py --kb`/`--no-kb` integration
- [x] `scripts/tests/test_kb_export.sh` — 18 assertions
- [x] First real-world KB export run validated on Roger Rabbit (session 002)
- [x] KB sheet refactor: clean re-tile → labeled JPEG (~80% lighter) (session 002)
- [x] Dir rename: `kb/contact-sheets-clean/` → `kb/contact-sheets/` (session 002)
- [x] Manifest.jsonl deduplicated post --kb-force via per-movie JSON reconstruction (session 002)
- [x] **Brainstorm doc for IMDb × /pirata coupling — 3 passes, 6 reviewers each, scope landed at Phase 0+1 + deferred Phase 2** (session 003)
- [x] **Verified TC `search_content` does NOT accept `imdb_id` via MCP introspection** (session 003)
- [x] **Verified knowledge-hub `kb/` consumer story (initial pass): searchable-not-filterable, pirata kb/ not registered** (session 003)
- [x] **Plan doc with 6 implementation units, locked Key Decisions, headless ce-doc-review pass, knowledge-hub follow-up** (session 003)
- [x] **Saved `feedback_show_reasoning.md` auto-memory** (session 003)
- [x] **Unit 1: `scripts/imdb_ingest.py` + `scripts/tests/test_imdb_ingest.sh` shipped, 46/46 PASS** (session 003)
- [x] **`.gitignore` extended for IMDb + runtime log artifacts** (session 003)
- [x] **Phase 0 real-corpus ingest fired & verified: 15 GB SQLite, 16.9 min wallclock, integrity ok, 10 tables populated, ft_titles 36.93M** (session 004)
- [x] **Plan + script patched: pre-flight floor 25 → 35 GB after measuring actuals; smoke 46/46 PASS post-patch** (session 004)
- [x] **Unit 2: `scripts/imdb_lookup.py` shipped (437 lines, FTS5 + B-tree + RapidFuzz WRatio with tier separation, p99=4ms, 25/25 smoke + 7/7 fixture)** (session 004)
- [x] **Plan deviations from Unit 2 documented inline** (WRatio not token_set_ratio, tier-1-always-above-tier-2 sort) (session 004)
- [x] **DONNA prompt arc: 4 prompts produced (validation, ingest v2 PT-BR, v2-eng, v2-eng-codex)** (session 004)
- [x] **4-persona ce-doc-review on v1 ingest prompt revealed FATAL feasibility bugs (kwargs that don't exist on `ingest_sync()`); v1 marked DO NOT USE** (session 004)
- [x] **kh staging convention discovered: discovery under `~/knowledge-base/09-knowledge-base/` with sub-layout `01-notes/02-sources/04-derived/05-packs`** (session 004)
- [x] **kh agent test-fire of v2-eng-codex returned ABORT-VALIDATION as designed; verified single-shot safety rails work** (session 004)

## Pending (By Priority)

### P1 — Urgent / Blocking

- [ ] **Decide kh staging path: A (symlink view) vs B (restructure pirata `kb/`)** — blocks Unit 6 op-step entirely. Default recommendation: A.
- [ ] After path picked: compose `<resultado-validacao>` / `<validation-result>` GO block from kh agent's preflight intel + paste into v2 (PT or eng or eng-codex per agent choice) + re-fire ingest prompt to kh agent.
- [ ] Save kh staging-convention memo (real intel from session 004 preflight) somewhere durable — `docs/research/2026-04-25-kh-staging-conventions.md` or similar — so next session doesn't re-discover.
- [ ] Commit decision (uncommitted changes from session 004): suggested boundaries are 3 commits: (a) `fix(imdb): raise pre-flight 25→35 GB after Phase 0 actuals` (script + plan changes), (b) `feat(imdb): Unit 2 — FTS5+RapidFuzz lookup with tier-separated WRatio` (lookup + tests + fixture + plan), (c) `docs(prompts): kh-ingest prompts v2 PT/eng/eng-codex`.

### P2 — Important

- [ ] **Unit 3: KB enrichment in `scripts/contact_sheet.py`** — manifest builder hook + sweep pass-through `--kb-imdb` flag + SKILL.md CONTRACT update (lockstep coupled in Unit 3 per pass-3 plan). Gated on Unit 2 (done) and the kh ingest decision (P1 above).
- [ ] **Unit 4: `/pirata` skill TC-failover wiring + event log** — SKILL.md workflow update + `scripts/skill_log.py` thin event-log writer + RESOLVED / TC STATUS row rendering.
- [ ] **Unit 5: TR-100 panel templates** — STATUS / DOCTOR / SHORTLIST updates in `menu-style.md`. Char-count verification.
- [ ] **Unit 6: Operations** — `imdb/state.json.example`, `imdb/README.md`, `scripts/pirata_evaluate.py` (Phase 2 gate evaluator with locked N_min=50 + both AND-clauses).
- [ ] 13 more PT-BR fixture entries (user pick) in `scripts/tests/fixtures/imdb_pt_br_20.txt`.

### P3 — Nice to Have

- [ ] (Carry-forward) Liberar disco em geral; Roger Rabbit migration `contact/` → `contact-sheets/`.
- [ ] (Carry-forward) `--kb-prune`, `--kb-rebuild-manifest`, launchd plist for auto-sweep, IPTC/XMP via exiftool, mega-sheet "movie fingerprint", `--kb-export` flag in `queue.py`, `/pirata` UPDATE for RAG-query workflow, cross-rip dedup, `cols/rows` default decision.
- [ ] After kh ingest succeeds, decide whether the locked composite-score formula needs popularity-aware bonus (Cidade de Deus / Duna no-year ambiguity is the calibration signal; current behavior of multi_tie + year-disambig is intentional but UX-tight on long-tail same-primary collisions).

## Technical Notes

**Stack additions for IMDb work (cumulative):**
- macOS Python `sqlite3` stdlib v3.43.2 (FTS5 compiled in — verified).
- `rapidfuzz` 3.14.3 (global pip install, matches Pillow convention; verified at session 004 start).
- `parse-torrent-title` (PTT) — NOT yet installed; needed for Unit 3.
- IMDb dump location: `/Users/vidigal/claude-code/pirata/imdb/unnoficial/` (~9.3 GB, 7 TSVs).
- DB output: `/Users/vidigal/claude-code/pirata/imdb/imdb.db` (15 GB, gitignored).
- State: `/Users/vidigal/claude-code/pirata/imdb/state.json` (gitignored; v1 schema: `last_refresh_started_at`, `last_refresh_finished_at`, `schema_version`, `source_checksums` (7 SHA-256), `source_dir`).

**FTS5 schema (locked, in-DB):**
```sql
CREATE VIRTUAL TABLE ft_titles USING fts5(
    title,
    title_source UNINDEXED,
    tconst       UNINDEXED,
    tokenize = 'unicode61 remove_diacritics 2',
    prefix = '2 3'
);
```

**B-tree COLLATE NOCASE indexes:** `idx_basics_primary_lower`, `idx_basics_original_lower`, `idx_akas_title_lower`, `idx_basics_titletype`, `idx_episode_parent`, `idx_ratings_votes`, `idx_principals_tconst`, `idx_akas_tconst`, `idx_names_primary_lower`.

**Tier separation in `imdb_lookup.py` (locked, post-Unit-2-fix):**
- Tier 1 = exact case-insensitive match → `fuzz_ratio = 100.0` explicitly
- Tier 2 = FTS5 + RapidFuzz `WRatio` capped at 99.0 → `fuzz_ratio < 100`
- Sort: `(0 if fuzz_ratio == 100 else 1, -score, -num_votes)` enforces Tier 1 always above Tier 2 regardless of composite score
- Multi-tie predicate: top 2 both have `fuzz_ratio == 100` AND score within 15% of each other

**Composite score (locked, in-`imdb_lookup.py`):** `score = fuzz_ratio × FIELD_MULTIPLIERS[field]` with field multipliers `primary=3.0`, `original=2.0`, `aka_original=1.8`, `aka_regional=1.5`. `numVotes` desc breaks ties when scores within 0.5.

**knowledge-hub real ops surface (verified 2026-04-25 via `health` + `list_kbs` + `topology` + `mcp_server.py` source read):**
- `bridge_root` = `canonical_workspace_root` = `shared_bridge_root` = `/Users/vidigal/knowledge-base/`
- `runtime_root` = `/Users/vidigal/.knowledge-hub/`
- `knowledge_base_root` = **`/Users/vidigal/knowledge-base/09-knowledge-base/`** ← discovery actually happens here
- `hub_profile` = `local_full_power_plus`, `status` = `ok` (don't trust the SessionStart degradation banner — it was stale)
- `host_role` = `dante-m4-primary`
- KB sub-layout discovery requires one of: `01-notes`, `02-sources`, `04-derived`, `05-packs` per KB
- Existing slugs sample: `actor-direction`, `agent-orchestration`, `approved`, `benchmarks-and-evals`, `workflow-orchestration` — all flagged `has_cag_pack=true` (CAG packs are opt-in)
- Infra: Qdrant at `http://localhost:6333`, dense family `bge_m3_1024`, retrieval profile `local_full_power_plus`, agentic_retrieval `planner+multi_step_local`
- `ingest_sync()` is **zero-arg** — reconciles whatever is under `public_bridge_root`. NOT a per-call ingest of an arbitrary path.
- No MCP-exposed `delete_kb`. Internal `delete_kb` (`runtime.py`) fires when catalog detects KB has disappeared from FS.
- `retrieve(query, kb_slugs=[...], mode='auto', explain_retrieval=True)` is the real signature; without `kb_slugs` smoke results pollute across all KBs.

**Codex 5.5 / xhigh prompting principles (from sources researched 2026-04-25):**
- Markdown sectional headers > XML tags
- Outcome-first framing; avoid step-by-step prescription
- Drop date stamps (model knows UTC)
- Static prefix / dynamic suffix for prompt caching
- Drop ULTRATHINK keyword (xhigh is already max reasoning)
- Preamble: 1-sent ack + 1-sent plan before tool batches; refresh every 1-3 steps; floor 6 steps / 10 tool calls
- Codex 5.5 default = persist end-to-end → must explicitly say "do not run extra queries" in SUCCESS branch for one-shot ops
- xhigh + weak stopping criteria = overthinking + regressions → tightening is the defense
- Treat as new model family — don't transfer Claude-XML literal

## Key Files

**This session (uncommitted on main):**
- `scripts/imdb_lookup.py` — 437 lines. FTS5+B-tree query layer, RapidFuzz WRatio tier-separated. Public API: `lookup_by_title`, `lookup_by_tconst`, `lookup_episodes`. Module-level connection cache.
- `scripts/tests/test_imdb_lookup.sh` — 15 test groups + fixture validation. 25/25 PASS, fixture 7/7.
- `scripts/tests/fixtures/imdb_pt_br_20.txt` — 7 seeded entries (Cidade de Deus 2002, Tropa de Elite 2007, Bacurau 2019, Oppenheimer 2023, Interestelar 2014, Ainda Estou Aqui 2024, Dune: Part Two 2024). Format: `title\tyear\tkind\texpected_tconst`. 13 more pending user.
- `docs/prompts/2026-04-25-kh-ingest-v2.md` — 9.2 KB. PT-BR ingest prompt for kh agent. Post-review-by-4-personas. Real kh API. Use for Claude-based kh agents.
- `docs/prompts/2026-04-25-kh-ingest-v2-eng.md` — 9.3 KB. EN translation of v2 PT-BR. Same semantic.
- `docs/prompts/2026-04-25-kh-ingest-v2-eng-codex.md` — 10.9 KB. Markdown-sectional rewrite for GPT-5.5-Codex with `reasoning_effort=xhigh`. Static prefix / dynamic suffix; no XML; no ULTRATHINK; explicit stopping criteria; preamble cadence baked in. Use for OpenAI Codex agents.
- `imdb/imdb.db` — 15 GB SQLite (gitignored). 10 populated tables, FTS5 36.93M entries.
- `imdb/state.json` — schema v1 + 7 SHA-256 source checksums (gitignored).
- `logs/imdb_ingest.log` — Phase 0 ingest log (gitignored).

**Modified this session (uncommitted):**
- `scripts/imdb_ingest.py` — `DISK_FREE_FLOOR_GB = 35` (was 25), docstring `<35 GB → abort` updated.
- `docs/plans/2026-04-24-004-feat-imdb-local-pirata-coupling-plan.md` — Phase 0 actuals (16.9 min, 15 GB), 35 GB floor, Unit 1 + Unit 2 marked `[x]`, deviations documented for WRatio + tier separation, Risks table row updated.

**Carried over from 003 (unchanged):**
- `scripts/imdb_ingest.py` core logic (only floor + docstring touched)
- `scripts/tests/test_imdb_ingest.sh` (re-ran after patch, 46/46 PASS)
- `docs/brainstorms/2026-04-24-imdb-local-pirata-coupling-requirements.md`
- `scripts/contact_sheet.py`, `scripts/sheets_sweep.py`, `scripts/queue.py`
- `scripts/tests/test_sweep.sh`, `scripts/tests/test_kb_export.sh`
- `.claude/skills/pirata-deck/SKILL.md`, `.claude/skills/pirata-deck/references/menu-style.md`
- `kb/per-movie/who-framed-roger-rabbit-1988.json` + supporting `kb/frames/`, `kb/contact-sheets/`, `kb/manifest.jsonl`
- `imdb/unnoficial/*.tsv` (7 files, 9.3 GB raw)
- Auto-memory: `feedback_show_reasoning.md`, `feedback_ansi_in_code_fence.md` (no changes)

**Volatile / disposable:**
- `/tmp/donna_ingest_prompt_v1.md` — buggy v1 of ingest prompt with assumed kwargs that don't exist. **DO NOT USE.** Reference only for diff history.

## Warnings & Gotchas

- **kh staging convention is real and pirata `kb/` doesn't match.** Discovery under `~/knowledge-base/09-knowledge-base/` requires sub-layout `01-notes` / `02-sources` / `04-derived` / `05-packs`. pirata's flat `per-movie/`, `frames/`, `contact-sheets/`, `manifest.jsonl` won't be discovered as-is. Decide Path A (symlink view) or Path B (refactor) before re-firing the ingest prompt.
- **`ingest_sync()` is zero-arg.** Don't pass kwargs. v1 ingest prompt was wrong about this. v2/v2-eng/v2-eng-codex are correct.
- **`retrieve` needs `kb_slugs`** to avoid colliding with the other 32 KBs on Vidigal's machine. v2/v2-eng/v2-eng-codex specify this literally.
- **No MCP-exposed delete-KB.** Rollback procedure is `rm` of the staged path + `ingest_sync()` to reconcile + verify via `list_kbs`. Documented in v2.
- **SessionStart kh degradation banner is misleading** — `[knowledge-hub] status=? profile=?` from the hook is independent of actual runtime state. kh agent's `health` returned `status=ok, hub_profile=local_full_power_plus`. Ignore the banner; rely on fresh `health` calls.
- **v1 ingest prompt at `/tmp/donna_ingest_prompt_v1.md` is BUGGY** — kept for diff/audit only, NOT for use.
- **Locked composite-score formula has gap on no-year same-primary-title clashes.** "Cidade de Deus" no year → tt27556897 (2023, 0 votes) outranks tt0317248 (2002, 869k votes). multi_tie + year-disambig is the locked escape hatch (R11a/R11b). Surfaces as a fixture-validation calibration signal; not a regression to fix this phase. Document only; revisit if Phase 1 enrichment-rate baseline doesn't hit target.
- **Pre-flight disk gate now 35 GB.** Was 25 GB before session 004. Future refreshes need ≥35 GB free or `--min-free-gb` override (risky).
- **DB is 15 GB, plan estimate was 1 GB.** 15× off but not a bug — driver is FTS5 prefix tokenization + name denorm + index size. Disk planning for any future scaling needs to use 15 GB as baseline, not 1 GB.
- **patches uncommitted on main.** All session-004 work is local. Suggested 3-commit boundaries listed under P1. Do not auto-commit; user controls commit timing.
- **Token_set_ratio is a footgun** for fuzzy title matching when query tokens are subset of candidate. Documented as plan deviation. Use `WRatio` instead (length-aware blend).
- **13 fixture entries pending** in `scripts/tests/fixtures/imdb_pt_br_20.txt` — user picks. Test passes ≥50% so doesn't block but calibration is incomplete.
- **`feedback_show_reasoning.md` is alive and well** — used during DONNA arc to lead with both-sides reasoning before recommending. Pattern continues to apply.
- **NEVER auto-commit pre-existing untracked files** like `.claude/commands/`, prior `MEMORY_DEEP_*.md`, prior brainstorms — they're carry-forward from earlier sessions and the user has reasons not to have committed them.
- **Don't run `/loop` or autonomous-loop on the continue trigger** — the trigger is conditional on user typing the resume word. Auto-trigger means "when user types continue, fire the action without re-asking", NOT "fire the action periodically".
