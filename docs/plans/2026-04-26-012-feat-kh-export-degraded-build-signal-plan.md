---
title: "feat: KH export degraded build signal"
type: feat
status: completed
date: 2026-04-26
origin: .context/compound-engineering/ce-code-review/20260426-154856-e8a6ec55/
predecessor: docs/plans/2026-04-26-011-fix-kh-export-json-hardening-plan.md
---

# feat: KH export degraded build signal

## Overview

Add an aggregate degraded-build signal to `scripts/build_kh_export.py`.
Plan 011 hardened individual fallback paths, but a build with degraded slugs
still exits like a clean build. Operators need a one-line summary and a
distinct exit code so cron/CI can tell clean exports from exports that
published with per-slug metadata fallback.

---

## Problem Frame

The builder currently emits per-call-site warn breadcrumbs for corrupt,
unreadable, non-object, non-regular, partial-copy, and missing per-movie JSON
states. Those warnings are useful during investigation, but they are not enough
for automation: a caller can redirect stderr and treat exit code 0 as clean even
when one or more slugs degraded.

---

## Requirements Trace

- R1. Count degraded slugs once per slug, even when multiple fallback paths fire
  for the same slug.
- R2. Emit a final summary log line with total manifest slug count and degraded
  slug count.
- R3. Return exit code 4 when the export succeeds but at least one slug
  degraded.
- R4. Preserve existing exit codes 0, 1, 2, and 3.
- R5. Preserve happy-path export tree bytes and manifest/wrapper schemas.
- R6. Keep degraded exports publishable; this is an observability signal, not a
  circuit breaker.

---

## Scope Boundaries

- Do not implement rel-002 circuit breaker before atomic swap.
- Do not implement rel-003 SIGPIPE/BrokenPipe handling for `log()`.
- Do not add `manifest.json` metadata or `meta.warnings[]`.
- Do not run Knowledge Hub replacement ingest.
- Do not change generated file schemas or happy-path export bytes.

---

## Context & Research

### Relevant Code and Patterns

- `scripts/build_kh_export.py` uses exit codes 0-3 and the `log(...)` helper.
- Plan 010/011 warnings already identify all per-slug fallback paths.
- `scripts/tests/test_kh_export.sh` has hermetic degradation fixtures that can
  assert exit code 4 without touching live `kb/`.

### Institutional Learnings

- Plan 011 intentionally kept atomic publish behavior for degraded exports.
- Prior KH ingest work should remain downstream and explicit; this plan is
  pirata-side only.

---

## Key Technical Decisions

- Use a `set[str]` of degraded slugs inside `build()` to deduplicate multiple
  warn paths for the same slug.
- Thread that set into the two internal helpers that detect per-movie JSON
  overlay degradation.
- Keep exit code 4 below the `main()` catch-all path; unexpected exceptions
  still return 3.

---

## Implementation Units

- U1. **Track degraded slugs**

**Goal:** Count each degraded slug once across copy, wrapper, and manifest
fallbacks.

**Requirements:** R1, R5, R6.

**Dependencies:** None.

**Files:**
- Modify: `scripts/build_kh_export.py`
- Test: `scripts/tests/test_kh_export.sh`

**Approach:**
- Add a `degraded_slugs` set in `build()`.
- Add copy-phase degraded slugs when non-regular paths or copy failures are
  skipped.
- Add helper-level degraded slugs for non-object, corrupt, non-UTF-8, and
  missing per-movie JSON fallback paths.

**Test scenarios:**
- Error path: corrupt JSON hits both helper fallback sites but contributes one
  degraded slug.
- Error path: non-regular or partial-copy failure contributes one degraded slug.

**Verification:**
- Degraded fixture builds emit `degraded=1` for one slug and `degraded=2` for
  two distinct degraded slugs.

---

- U2. **Expose summary log and exit code 4**

**Goal:** Make degraded-but-published builds distinguishable by logs and process
status.

**Requirements:** R2, R3, R4, R5, R6.

**Dependencies:** U1.

**Files:**
- Modify: `scripts/build_kh_export.py`
- Test: `scripts/tests/test_kh_export.sh`

**Approach:**
- Update the docstring exit-code block to include exit code 4.
- Emit `log("info", f"export complete: slugs={N} degraded={K}")` near the end
  of `build()`.
- Return 4 after successful publish when `K > 0`, while preserving code 2 for
  the no-source case.

**Test scenarios:**
- Happy path: live `kb/` build exits 0 and logs `degraded=0`.
- Degraded path: existing hermetic fallback fixtures exit 4 and log the expected
  degraded count.

**Verification:**
- `test_kh_export` passes with at least one new assertion for exit code 4 and
  summary log.

---

## System-Wide Impact

- **Interaction graph:** Limited to `build_kh_export.py` and its tests.
- **Error propagation:** Expected degraded builds move from exit 0 to exit 4;
  unexpected exceptions still exit 3.
- **State lifecycle risks:** Degraded exports are still atomically published;
  operators decide how to react to exit 4.
- **Unchanged invariants:** Happy-path export tree checksum remains stable.

---

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Existing automation assumes any nonzero exit means no export was published. | Document exit code 4 as succeeded-but-degraded and keep atomic publish behavior unchanged. |
| Double-counting a slug with multiple fallbacks overstates degradation. | Track slugs in a set. |
| Summary log affects deterministic output checks. | Existing checksum covers export tree only; no generated files change. |

---

## Sources & References

- Predecessor plan: `docs/plans/2026-04-26-011-fix-kh-export-json-hardening-plan.md`
- Builder: `scripts/build_kh_export.py`
- Tests: `scripts/tests/test_kh_export.sh`

---

## Completion Notes

Implemented:
- `build()` now tracks degraded slugs in a `set[str]`.
- Degraded-but-published exports now return exit code 4.
- The builder emits `export complete: slugs=N degraded=K`.
- Existing degraded fixtures now assert exit code 4 and summary counts.

Validation:
- `python3 -m py_compile scripts/build_kh_export.py`
- `bash -n scripts/tests/test_kh_export.sh`
- `git diff --check -- docs/plans/2026-04-26-012-feat-kh-export-degraded-build-signal-plan.md scripts/build_kh_export.py scripts/tests/test_kh_export.sh`
- `bash scripts/tests/test_imdb_lookup.sh`
- `bash scripts/tests/test_imdb_kb_enrich.sh`
- `bash scripts/tests/test_kh_export.sh`
- `bash scripts/tests/test_sweep.sh`
- `bash scripts/tests/test_contact_sheet_imdb.sh`
- `bash scripts/tests/test_queue_wrap.sh`
- `bash scripts/tests/test_imdb_ingest.sh`
- `bash scripts/tests/test_kb_export.sh`
