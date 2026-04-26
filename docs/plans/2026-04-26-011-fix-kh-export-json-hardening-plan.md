---
title: "fix: Harden KH export JSON fallbacks"
type: fix
status: completed
date: 2026-04-26
origin: docs/prompts/2026-04-26-codex-review-plan-010.md
predecessor: docs/plans/2026-04-26-010-fix-kh-export-symmetric-errors-plan.md
---

# fix: Harden KH export JSON fallbacks

## Overview

This follow-up closes two P2 gaps found by the independent review of plan
010's autofix. The KH export builder already handles malformed and non-UTF-8
per-movie JSONs, but valid JSON roots that are not objects still crash on
`.get(...)`, and non-regular `kb/per-movie/*.json` paths still crash during
the copy phase before wrapper/manifest fallback code can run.

---

## Problem Frame

`scripts/build_kh_export.py` is supposed to degrade unreadable or unusable
per-movie JSONs into manifest-derived headers and bare markdown wrappers.
That contract is still incomplete in two places:

- `json.loads(...)` can return a non-dict JSON value such as `123` or `[]`.
  Both overlay sites assume a dict and call `.get(...)`, raising
  `AttributeError` instead of falling back.
- The full builder copies every `kb/per-movie/*.json` before overlay fallback
  runs. A directory, broken symlink, FIFO, socket, or copy race with a `.json`
  suffix can make `shutil.copy2(...)` raise and abort the build.

---

## Requirements Trace

- R1. Non-object JSON roots are treated as degraded per-movie metadata, not as
  fatal builder failures.
- R2. Both overlay sites preserve their existing fallback behavior and warn
  breadcrumbs when JSON is unusable.
- R3. Non-regular `.json` paths are skipped with a warn before `copy2`.
- R4. `per_movie_paths` records only successfully copied JSON files.
- R5. Manifest slugs still produce `manifest.json` entries and markdown
  wrappers even when the matching per-movie JSON is skipped or unusable.
- R6. Existing happy-path export output, schema, and KH ingest surface remain
  unchanged.

---

## Scope Boundaries

- Touch only `scripts/build_kh_export.py` and
  `scripts/tests/test_kh_export.sh`.
- Do not change `manifest.json`, `manifest.jsonl`, markdown wrapper schema, or
  README content.
- Do not introduce a virtualenv, requirements file, or packaging convention.
- Do not run Knowledge Hub staging or replacement ingest as part of this fix.
- Do not extract a shared overlay helper; plan 010 explicitly deferred that
  until a third call-site exists.

---

## Context & Research

### Relevant Code and Patterns

- `scripts/build_kh_export.py` already uses `log("warn", ...)` for degraded
  export paths.
- `build_manifest_json(...)` and `build_slug_md(...)` already have parallel
  unreadable/corrupt JSON fallback behavior from plan 010.
- `scripts/tests/test_kh_export.sh` already has hermetic KB fixtures for
  corrupt JSON and non-UTF-8 bytes; the new cases should extend that style.

### Institutional Learnings

- Plan 005 established `build_kh_export.py` as an idempotent export builder.
- Plan 010 established the symmetric fallback contract and warn breadcrumbs.
- Plan 004/007 document global `pip3` dependencies for IMDb tooling; this plan
  does not change that convention.

---

## Key Technical Decisions

- Validate decoded JSON root type immediately after `json.loads(...)`.
  Non-dict roots are unusable for overlay metadata and should follow the same
  degraded path as malformed JSON.
- In the copy loop, validate/copy first and update `per_movie_paths` only after
  success. Downstream wrapper/manifest logic should read from the copied export
  staging file, not an unverified source path.
- Preserve the existing warning prefix `per-movie JSON unreadable for {slug}`
  where possible so current grep-based tests continue to lock both warn sites.

---

## Implementation Units

- U1. **Validate JSON object roots at overlay sites**

**Goal:** Prevent non-object valid JSON roots from escaping the fallback
contract.

**Requirements:** R1, R2, R6.

**Dependencies:** None.

**Files:**
- Modify: `scripts/build_kh_export.py`
- Test: `scripts/tests/test_kh_export.sh`

**Approach:**
- In `build_manifest_json(...)`, only call `.get(...)` when decoded JSON is a
  dict. Otherwise, leave manifest-derived title/year intact and warn.
- In `build_slug_md(...)`, only keep `has_json=True` when decoded JSON is a
  dict. Otherwise, reset to the manifest-only wrapper path and warn.

**Test scenarios:**
- Error path: per-movie JSON root `123` exits 0, warns at both overlay sites,
  preserves manifest-derived header, and renders a bare wrapper.
- Error path: per-movie JSON root `[]` follows the same fallback path.
- Regression: happy-path object JSON still overlays title/year and renders rich
  wrapper fields.

**Verification:**
- `test_kh_export` passes and includes explicit numeric/list-root coverage.

---

- U2. **Skip non-regular `.json` paths before copy**

**Goal:** Keep non-regular per-movie JSON paths from aborting the build before
fallback logic can run.

**Requirements:** R3, R4, R5, R6.

**Dependencies:** None.

**Files:**
- Modify: `scripts/build_kh_export.py`
- Test: `scripts/tests/test_kh_export.sh`

**Approach:**
- Check `Path.is_file()` before `shutil.copy2(...)`.
- Warn and skip non-regular paths.
- Catch `OSError` from `copy2`, warn, and skip.
- Add to `per_movie_paths` only after a successful copy.

**Test scenarios:**
- Error path: directory named `test-dir.json` exits 0, warns, is not copied, and
  still gets a manifest-only wrapper.
- Error path: broken symlink named `test-broken.json` exits 0, warns, is not
  copied, and still gets a manifest-only wrapper.
- Integration: `manifest.json` still includes all manifest slugs and raw rows.

**Verification:**
- `test_kh_export` passes with copied-count and no-invalid-copy assertions.

---

## System-Wide Impact

- **Interaction graph:** Limited to the KH export builder and its bash tests.
- **Error propagation:** More invalid per-movie JSON states degrade to warnings
  instead of exit code 3.
- **State lifecycle risks:** Lower than before; `per_movie_paths` now points to
  successfully copied staging files only.
- **Unchanged invariants:** Source `kb/manifest.jsonl` remains untouched, output
  schema remains unchanged, and no KH ingest state is mutated.

---

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Warn strings drift and tests stop checking both overlay sites. | Preserve existing distinct suffixes and add explicit grep assertions. |
| Skipped JSON copy hides source corruption. | Emit warn breadcrumbs naming the slug and path class. |
| Full validation blocked by missing IMDb deps. | Install only documented global deps (`rapidfuzz`, `parse-torrent-title`) and avoid packaging changes. |

---

## Documentation / Operational Notes

- After this fix passes tests, regenerate `kb/kh-export/` through the normal
  builder flow only if a downstream KH replacement ingest is planned.
- Do not run KH replacement ingest until the pirata-side test gate is green.

---

## Sources & References

- Review prompt: `docs/prompts/2026-04-26-codex-review-plan-010.md`
- Predecessor plan: `docs/plans/2026-04-26-010-fix-kh-export-symmetric-errors-plan.md`
- Related builder: `scripts/build_kh_export.py`
- Related tests: `scripts/tests/test_kh_export.sh`
