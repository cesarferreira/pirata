---
title: Validate KH-export and produce KH-replacement handoff
type: fix
status: active
date: 2026-04-26
---

# Plan 009 — Validate KH-export and produce KH-replacement handoff

## Overview

`pirata-kb` is already registered in Knowledge Hub from the 2026-04-26 first ingest (FIRE-v3 prompt; final status `SUCCESS-WITH-CAVEATS`, 6 docs indexed, 3/3 smoke retrieve hits). The KH-side records still carry the old multi_tie shape with no IMDb metadata for Mario Galaxy / Roger Rabbit. Plan 008 Unit 2 then surgical-patched both per-movie JSONs locally (Mario Galaxy → `tt28650488`, Roger Rabbit → `tt0096438`) and re-rendered `kb/kh-export/04-derived/`.

This plan does the maximum safe pirata-side work to (1) verify the locally regenerated export is exact-scope, idempotent, and KH-compatible; (2) patch the smallest durable defect at the builder source if Phase 4 surfaces one; and (3) produce a copy-ready KH replacement handoff block. KH itself stays untouched.

## Problem Frame

`kb/kh-export/04-derived/` is the input contract for the next KH replacement run. KH currently indexes the older multi_tie shape, so a future replacement is needed to surface IMDb genres / rating / directors / top-cast for both movies in retrieve queries. Local state has likely already shifted to the resolved-IMDb shape, but that needs verification on disk and idempotent rebuild before handoff. Any defect must be fixed at the builder level (`scripts/build_kh_export.py`), not by hand-editing generated artifacts.

## Requirements Trace

- R1. Phase 1 read-only state recovery: `pwd`, `git status`, branch, last 5 commits, file inventory, sha256 of `kb/manifest.jsonl` and every file under `kb/kh-export/04-derived/`.
- R2. Phase 2 decision gate: NOOP / FIX / ABORT-SCOPE-DRIFT based on observed state.
- R3. (Conditional) Phase 3 smallest durable fix at source — `scripts/build_kh_export.py` only.
- R4. Phase 4 validation: builder run, idempotency check, narrowest existing tests, JSON parse on every `*.json` under kh-export/04-derived/, exact-scope file list, no `.jsonl`/images/symlinks, MG title/year acceptable, manifest.json preserves raw row provenance.
- R5. Phase 5 KH replacement handoff block — replacement candidate (not first ingest); KH operator runs fresh preflight + needs explicit GO; expected 6 files; `kb/manifest.jsonl` stays pirata-canonical.
- R6. KH boundary hard: no MCP calls, no `workspace_sync`, no `ingest_sync`, no mutation of `~/projects/knowledge-hub` or `~/knowledge-base`.
- R7. Source preservation: `kb/manifest.jsonl` byte-frozen at MD5 `fd359aec527f317fe3c6c6c0e2e7cf81` unless an existing pirata workflow explicitly requires mutation.
- R8. No fabricated IMDb metadata. Slug→title/year deterministic fallback acceptable; everything else null/absent if not locally resolvable.

## Scope Boundaries

- Out: any KH MCP tool, `workspace_sync`, `ingest_sync`, KH staging-directory mutation, KH preflight execution, KH retrieve smokes.
- Out: image assets (frames, contact-sheets) under KH text export.
- Out: rewriting `kb/manifest.jsonl` (byte-frozen ledger).
- Out: any change to `scripts/imdb_kb_enrich.py` or `scripts/imdb_lookup.py` (Unit 3 already shipped per plans 007/008).
- Out: any unrelated dirty-work cleanup or revert.

### Deferred to Separate Tasks

- KH-side replacement run (separate operator session after explicit GO).
- `kb/manifest.jsonl` re-emit decision (item C2 in pending list — independent).

## Context & Research

### Relevant code and patterns

- `scripts/build_kh_export.py` (V2, ~532 lines) — idempotent + byte-deterministic per Unit 3 / plan 007 Unit C. Emits `manifest.json` + `README.md` + `per-movie/{slug}.{json,md}`. Renders YAML frontmatter + `## IMDb metadata` body section when `imdb.result == "resolved"`. KH-whitelisted suffixes only: `.json`, `.md`, `.txt`, `.yaml`, `.yml`, `.csv`.
- `scripts/tests/test_kh_export.sh` (~453 lines) — 54/54 PASS as of HEAD `1c7c271`. Covers idempotency, wrapper rendering, YAML quoting, RR title shift, MG resolved tconst.
- `kb/per-movie/the-super-mario-galaxy-movie-2026.json` — resolved (tt28650488 per plan 008 Unit 2).
- `kb/per-movie/who-framed-roger-rabbit-1988.json` — resolved (tt0096438 per plan 008 Unit 2).
- `kb/manifest.jsonl` — byte-frozen at MD5 `fd359aec527f317fe3c6c6c0e2e7cf81`. 600 entries on pre-Unit-3 shape.

### Institutional learnings

- Plan 005 established `kb/manifest.jsonl` as append-only ledger; surgical patching of per-movie JSONs is allowed without re-emit (item C2 deferred).
- Plan 007 Unit C established YAML scalar quoting on commas + `grep -qF --` for needles starting with `-`.
- Plan 008 Unit 2 surgical-patched MG + RR per-movie JSONs and re-ran `build_kh_export.py` — wrappers should already carry full IMDb metadata.
- FIRE-v2 → FIRE-v3 lesson: KH whitelist excludes `.jsonl`. Builder must never emit `.jsonl` under `kb/kh-export/04-derived/`.

### External references

- KH whitelisted suffixes: `.json`, `.md`, `.txt`, `.yaml`, `.yml`, `.csv`.

## Key Technical Decisions

- **Trust per-disk state, not memory.** Phase 1 reads on-disk artifacts before deciding NOOP vs FIX. The session-005 snapshot says "regenerated", but local state must be re-verified.
- **Conservative Phase 3.** Only patch the builder if Phase 4 surfaces a concrete failure. Do not refactor existing rendering paths defensively. Diff cap: ≤ 30 lines.
- **Handoff is text, not action.** Phase 5 handoff is a copy-ready block in the response, not a git commit, not a KH MCP call. KH operator owns mutation.
- **Idempotency anchor.** Run builder twice; second run must produce zero diffs. Same anchor used in plan 007 Unit F + plan 008 review.
- **Status label is the artifact's executive summary.** Final response uses exactly one of the 6 labels — `/lfg` pipeline + downstream prompts depend on it.
- **Cross-repo absolute path is allowed in handoff text only.** The KH operator works from `~/projects/knowledge-hub`, not pirata. The handoff block uses one absolute path (`/Users/vidigal/claude-code/pirata/kb/kh-export/04-derived/`) intentionally — this is the only acceptable use, since repo-relative paths break across the boundary.

## Open Questions

### Resolved during planning

- *Should plan 009 be feat/fix/refactor?* → `fix` (defect-confirmation + KH-replacement readiness; no new feature surface).
- *Should the handoff block be persisted as a doc?* → Inline in response only; no `docs/handoffs/` until a second handoff lands and motivates the directory.
- *Does Phase 1 belong in the plan or in execution?* → In execution (under Unit 1). Plan describes intent; executor runs the bash checks.

### Deferred to implementation

- Whether Phase 3 will fire at all (depends on Phase 1 state).
- Whether any new `test_kh_export.sh` assertion is needed (depends on the specific Phase 4 gap, if any).

## Implementation Units

- [ ] **Unit 1: Read-only recovery and state snapshot**

**Goal:** Capture the current pirata + KH-export state without mutation. Produce a state report the decision gate uses.

**Requirements:** R1, R6, R7

**Dependencies:** none

**Files:**
- Read: `scripts/build_kh_export.py`
- Read: `kb/per-movie/` (inventory)
- Read: `kb/manifest.jsonl` (sha256 only, no parse)
- Read: `kb/kh-export/04-derived/` (recursive inventory + sha256 of every file)

**Approach:**
- Run: `pwd`, `git status --short --untracked-files=all`, `git branch --show-current`, `git log --oneline -5`.
- Inventory: `find kb/kh-export/04-derived -maxdepth 4`; cross-check against the expected 6 files.
- Capture checksums (session-only):
  - `shasum -a 256 kb/manifest.jsonl`
  - `find kb/kh-export/04-derived -maxdepth 4 -type f -print0 | sort -z | xargs -0 shasum -a 256`
- Detect:
  - any `.jsonl` under `kb/kh-export/04-derived/`
  - image suffixes (`.jpg`, `.jpeg`, `.png`, `.gif`, `.webp`, `.heic`, `.tif`, `.tiff`)
  - symlinks (`find kb/kh-export/04-derived -type l`)
- Read MG generated artifacts:
  - `kb/kh-export/04-derived/per-movie/the-super-mario-galaxy-movie-2026.json` — observe `title`, `year`
  - `kb/kh-export/04-derived/per-movie/the-super-mario-galaxy-movie-2026.md` — observe YAML frontmatter `title`, `year`, body section
  - `kb/kh-export/04-derived/manifest.json` — observe MG slug entry shape

**Patterns to follow:**
- Plan 007 Unit F + plan 008 Unit 2's read-only state captures.

**Test scenarios:** Test expectation: none — read-only verification, no mutation.

**Verification:**
- State report enumerates: file count under `kh-export/04-derived/`, presence of `.jsonl` / images / symlinks, MG title/year strings observed, `manifest.jsonl` checksum.
- No files modified.

- [ ] **Unit 2: Decision gate**

**Goal:** Apply Phase 2 logic to the Unit 1 state report. Produce one of: NOOP, FIX, ABORT-SCOPE-DRIFT.

**Requirements:** R2

**Dependencies:** Unit 1

**Files:** none (analysis only)

**Approach:**
- **NOOP** if all hold:
  - Exactly 6 files under `kb/kh-export/04-derived/` matching the expected layout.
  - No `.jsonl`, no images, no symlinks.
  - MG `title` is non-slug (e.g., "The Super Mario Galaxy Movie") and `year` is `2026`.
  - `manifest.json` carries raw rows + display metadata without overwriting raw provenance.
- **ABORT-SCOPE-DRIFT** if more or fewer slugs than the two expected (`who-framed-roger-rabbit-1988`, `the-super-mario-galaxy-movie-2026`).
- **FIX** otherwise. Diagnose the smallest delta needed at the builder level.

**Test scenarios:** Test expectation: none — analysis only.

**Verification:**
- Decision is one of {NOOP, FIX, ABORT-SCOPE-DRIFT} with explicit rationale tying back to Unit 1 state report.

- [ ] **Unit 3: Smallest durable fix at source (conditional)**

**Goal:** If Unit 2 routed to FIX, apply the smallest durable fix at the builder level. Skip entirely if Unit 2 routed to NOOP or ABORT-SCOPE-DRIFT.

**Requirements:** R3, R7, R8

**Dependencies:** Unit 2 (only when FIX)

**Files:**
- Modify (conditional): `scripts/build_kh_export.py`
- Modify (conditional): `scripts/tests/test_kh_export.sh` if a behavioral assertion is genuinely missing.

**Approach:**
- Diagnose the precise gap from Unit 1 state report. Likely vectors:
  - `.jsonl` slipping into `kh-export/04-derived/` → tighten the suffix whitelist filter.
  - Image suffix slipping in → harden the inventory pre-emit guard.
  - MG title/year still slug-shaped → ensure builder respects per-movie JSON's `title` / `year` keys when present, with deterministic slug-fallback as last resort. Mark fallback provenance in generated content (e.g., `title_source: "imdb"` vs `title_source: "slug-fallback"`).
  - `manifest.json` overwriting raw rows → switch to additive display-metadata layer that preserves raw row provenance.
- Avoid scope creep: do not refactor rendering paths that already pass tests.
- Add or update a `test_kh_export.sh` assertion only when the new behavior wouldn't have been caught by an existing assertion.

**Patterns to follow:**
- Plan 007 Unit C YAML quoting + needle conventions (`grep -qF --`).
- Plan 008 Unit 1 conservative immutability pattern.

**Test scenarios (conditional, only when FIX fires):**
- Happy path: builder run produces the 6 expected files for the two-movie scope.
- Edge case: per-movie JSON missing `title` or `year` → fallback derives from slug; provenance marked.
- Error path: per-movie JSON with malformed imdb block → builder handles gracefully without overwriting raw manifest row data.
- Integration: second builder run produces byte-identical output (idempotency).

**Verification:**
- `bash scripts/tests/test_kh_export.sh` returns 0; PASS count ≥ 54 (Unit 3 may add, must not subtract).
- Diff to `scripts/build_kh_export.py` ≤ 30 lines.
- No artifact changes outside `kb/kh-export/04-derived/`.

- [ ] **Unit 4: Validation**

**Goal:** Run all Phase 4 assertions, including idempotency. Produce the tentative final status label.

**Requirements:** R1 (post-fix re-checksum), R4

**Dependencies:** Unit 3 if FIX; otherwise Unit 2

**Files:**
- Run: `python3 scripts/build_kh_export.py`
- Run: `bash scripts/tests/test_kh_export.sh`

**Approach:**
- First builder run: capture sha256 of every file under `kb/kh-export/04-derived/`.
- Second builder run: re-capture sha256. Diff must be empty (idempotency).
- Run `test_kh_export.sh`; expect 54/54 (or higher if Unit 3 added assertions).
- For every `*.json` under `kh-export/04-derived/`, run `python3 -c "import json,sys;json.load(open(sys.argv[1]))"` to confirm parse.
- Confirm `kb/manifest.jsonl` sha256 unchanged from Unit 1 baseline.
- Re-confirm exact 6-file inventory: `manifest.json`, `README.md`, `per-movie/who-framed-roger-rabbit-1988.json`, `per-movie/who-framed-roger-rabbit-1988.md`, `per-movie/the-super-mario-galaxy-movie-2026.json`, `per-movie/the-super-mario-galaxy-movie-2026.md`.
- Re-confirm: no `.jsonl`, no images, no symlinks.
- Confirm MG generated artifacts: `title` non-slug, `year=2026`, markdown body literal "Super Mario Galaxy" present.
- Confirm RR generated artifacts: literals "Roger Rabbit", "Who Framed Roger Rabbit (1988)", and slug `who-framed-roger-rabbit-1988` all present.
- Confirm "scdet" literal somewhere in the export (frame manifest reference — README or per-movie wrappers).
- Confirm `manifest.json` carries raw row provenance + display metadata without overwriting raw rows.

**Patterns to follow:** plan 008 Unit 1 idempotency anchor.

**Test scenarios:**
- Happy path: 9 assertions pass; idempotency holds; tentative label is `NOOP-EXPORT-ALREADY-CORRECT` (no edits) or `SUCCESS-PIRATA-EXPORT-FIXED` (Unit 3 fired) → both proceed to Unit 5 as `READY-FOR-KH-REPLACEMENT`-equivalent.
- Error path: any assertion fails → tentative label `ABORT-PIRATA-VALIDATION`; do not produce KH handoff (skip Unit 5).
- Edge case: idempotency fails on second run → label `ABORT-PIRATA-VALIDATION`; bisect non-deterministic field (timestamp / dict ordering / sort key) and surface the offending field name.

**Verification:**
- All 9 assertions documented as PASS or FAIL with evidence.
- Tentative final status label selected from: `NOOP-EXPORT-ALREADY-CORRECT`, `READY-FOR-KH-REPLACEMENT`, `SUCCESS-PIRATA-EXPORT-FIXED`, `ABORT-PIRATA-VALIDATION`, `ABORT-SCOPE-DRIFT`, `BLOCKED-NEEDS-SOURCE-OR-CREDENTIAL`.

- [ ] **Unit 5: KH replacement handoff block**

**Goal:** Produce a copy-ready KH-side handoff text. Do not execute it.

**Requirements:** R5, R6

**Dependencies:** Unit 4 (status must not be `ABORT-*` or `BLOCKED-*`)

**Files:** none (response only)

**Approach:**
- Author a self-contained block including:
  - "`pirata-kb` already exists" + last-known status `SUCCESS-WITH-CAVEATS`.
  - Replacement / update framing — explicitly not first ingest.
  - KH-side fresh preflight required before any mutation.
  - Explicit GO label gate before deleting or replacing staged files.
  - Pirata export source path: `/Users/vidigal/claude-code/pirata/kb/kh-export/04-derived/` (intentional absolute — KH operator runs from a different repo).
  - Expected 6-file list (literal paths).
  - `kb/manifest.jsonl` stays pirata canonical; never staged into KH.
  - Frames / contact-sheets out of scope.
  - Validation-result block from Unit 4 (label + 9-assertion evidence).
  - File checksum summary (sha256 from Unit 4).
  - Caveats: KH currently indexes the old multi_tie shape; replacement overwrites with full IMDb metadata.
  - Exact next safe step for KH operator: read fresh KH preflight skill, then prompt the user with a replacement runbook labeled "**KH-side only after explicit GO**".

**Test scenarios:** Test expectation: none — text artifact, not code.

**Verification:**
- Block contains all 6 file paths.
- Block carries the GO-gating language.
- Block does NOT include `mcp__knowledge-hub__*` tool calls as something to run immediately. Any sample runbook is labeled "KH-side only after explicit GO".
- No KH MCP tool was called during Unit 5.

## System-Wide Impact

- **Interaction graph:** None on the pirata side beyond `scripts/build_kh_export.py` (only if FIX). KH side: replacement handoff is downstream — not executed here.
- **Error propagation:** Phase 4 `ABORT-PIRATA-VALIDATION` must hard-stop before KH handoff. Phase 5 must not auto-fall-through if any assertion failed.
- **State lifecycle risks:** `kb/manifest.jsonl` byte-frozen — any builder change must not touch it. KH staging directory must remain untouched.
- **API surface parity:** None. Builder change (if any) is internal.
- **Integration coverage:** `test_kh_export.sh` 54-assertion safety net + builder-twice idempotency anchor.
- **Unchanged invariants:**
  - `kb/manifest.jsonl` MD5 stays at `fd359aec527f317fe3c6c6c0e2e7cf81`.
  - `scripts/imdb_lookup.py`, `scripts/imdb_kb_enrich.py` not touched.
  - All 6 test suites stay green (182/182 baseline or higher if Unit 3 adds assertions).
  - No new files outside `kb/kh-export/04-derived/`, `scripts/build_kh_export.py`, `scripts/tests/test_kh_export.sh`, and the new plan file.

## Risks & Dependencies

| Risk | Mitigation |
|---|---|
| Phase 3 fix introduces regression in `test_kh_export.sh`. | Run full suite after every edit; keep diff ≤ 30 lines; abort to `ABORT-PIRATA-VALIDATION` rather than ship a broken builder. |
| Unit 1 reveals scope drift (extra slugs / removed slug). | `ABORT-SCOPE-DRIFT` immediately; do not guess inclusion. Surface unexpected files in response. |
| Idempotency check fails on second builder run. | `ABORT-PIRATA-VALIDATION`; bisect the non-deterministic field (timestamp, dict ordering, sort key); name it in the abort message. |
| `kb/manifest.jsonl` checksum drifts during Unit 1/4. | Hard stop. Investigate which workflow touched it before continuing. Item C2 of pending list explicitly tracks this. |
| Operator confuses pirata-side handoff with KH-side execution. | Explicit "KH-side only after explicit GO" labeling on every command in the runbook. Validation-result block names pirata as the producer. |
| Unit 3 over-engineers a fix for a symptom that's actually upstream (per-movie JSON shape). | Diagnose Unit 1 state delta first; if upstream, document and pivot to a per-movie surgical patch path explicitly outside this plan's scope. Do not chase the cause inside `build_kh_export.py`. |

## Documentation / Operational Notes

- Final status label drives the next planning step:
  - `NOOP-EXPORT-ALREADY-CORRECT` → present handoff and stop.
  - `READY-FOR-KH-REPLACEMENT` / `SUCCESS-PIRATA-EXPORT-FIXED` → present handoff and stop.
  - `ABORT-*` → surface root cause; no handoff produced; pending list updated.
- Pending list item A1 framing pivots from "first re-stage" to "KH replacement run (after explicit GO)" once Unit 5 fires.

## Sources & References

- Plan 005 — kh-export surface: `docs/plans/2026-04-25-005-feat-kh-export-surface-plan.md`
- Plan 007 — IMDb KB enrichment: `docs/plans/2026-04-26-007-feat-imdb-kb-enrichment-plan.md`
- Plan 008 — vote-spread tie-breaker: `docs/plans/2026-04-26-008-fix-imdb-vote-tie-breaker-plan.md`
- KH ingest prompt (FIRE-v3): `docs/prompts/2026-04-26-kh-ingest-FIRE-v3.md`
- Memory snapshot: `MEMORY_DEEP_005.md`
- Builder: `scripts/build_kh_export.py`
- Tests: `scripts/tests/test_kh_export.sh`
