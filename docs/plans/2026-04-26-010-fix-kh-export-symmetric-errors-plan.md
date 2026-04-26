---
title: "fix: KH-export symmetric error handling + cover overlay branches"
type: fix
status: active
date: 2026-04-26
origin: .context/compound-engineering/ce-code-review/20260426-153216-1bc76b28/
predecessor: docs/plans/2026-04-26-009-fix-kh-export-replacement-handoff-plan.md
---

# fix: KH-export symmetric error handling + cover overlay branches

## Overview

Post-review hardening of plan 009 Unit 3 (per-movie JSON overlay into manifest.json + parens-year alias). The ce-code-review run `20260426-153216-1bc76b28` (correctness + testing + maintainability lenses, all 3 reviewers Done) surfaced four convergent low-severity issues against `scripts/build_kh_export.py` and `scripts/tests/test_kh_export.sh`. This plan applies the four convergent fixes; explicitly defers DRY extraction (M-009-1) and the forward-looking missing-JSON branch test (T3).

## Problem Frame

Plan 009 Unit 3 landed two real defects' fixes (manifest.json title/year overlay + parens-year alias literal) in ~38 LOC; tests are 184/184 green. Cross-reviewer synthesis flagged four follow-ups:

1. **Asymmetric failure contract** between sibling overlay sites: `build_manifest_json` (L157-168) wraps `json.loads` in `try/except (json.JSONDecodeError, OSError)` and silently keeps manifest-derived values; `build_slug_md` (L206) calls the same `json.loads` with no guard. A single corrupt per-movie JSON crashes the whole builder before manifest.json is even written, making the silent-fallback path effectively unreachable through normal runs. Reviewers also noted the silent path is observability-blind — no warn breadcrumb if the fallback ever fires (correctness-1, M-009-2).
2. **Untested error path** in `build_manifest_json` overlay try/except — fallback branch is logically reachable but no test exercises it (T1).
3. **Untested suppression branch** of the parens-year alias — current fixtures (MG resolved, RR resolved) only exercise the positive emit path; the `if not title_is_slug and not year_missing` guard at L297-298 has no negative-side assertion (T2).
4. **`2>/dev/null` swallows AssertionError** in test 11i (L188) — bundled 4-inner-assert test loses which inner check failed because stderr is discarded (correctness-2).

## Requirements Trace

- R1. Error contract for corrupt/unreadable per-movie JSON is symmetric across both overlay call-sites — neither raises; both fall back gracefully and log a `warn` breadcrumb.
- R2. The `build_manifest_json` overlay error path is exercised by an automated test that proves manifest.json header falls back to manifest-derived title/year on corrupt JSON, while raw `rows[]` provenance remains intact.
- R3. The "Title with year" alias suppression branch is exercised by an automated test that proves the body line is absent when `title_is_slug` or `year is None`.
- R4. Test 11i preserves the AssertionError diagnostic so a regression points at which of its 4 inner checks fired.
- R5. Idempotency holds (`sha-roll-up 71002f59…` stable across two consecutive builds against live kb/).
- R6. `kb/manifest.jsonl` byte-frozen invariant preserved (sha256=9ea712bb…); `manifest.json` shape unchanged (header carries IMDb-resolved title/year, `slugs[<slug>].rows[]` carries verbatim raw rows).
- R7. All 6 test suites green pre and post: 184/184 (test_imdb_lookup 25, test_imdb_kb_enrich 54, test_kh_export 56→58, test_sweep 15, test_contact_sheet_imdb 26, test_queue_wrap 8 — total goal 186/186 once 2 new tests land).

## Scope Boundaries

- This plan only touches `scripts/build_kh_export.py` and `scripts/tests/test_kh_export.sh`.
- Does NOT change `manifest.json` or `manifest.jsonl` shape, content, or byte-frozen state.
- Does NOT touch IMDb resolution (plan 008 territory) or KH replacement runbook (separate KH-side prompt).

### Deferred to Separate Tasks

- **M-009-1 (DRY extraction)** — a `_overlay_display_title_year(slug, json_path, fallback_title, fallback_year)` helper unifying the duplicated rule at L157-168 and L203-213. Reviewer explicitly marked "Not blocking for plan 009"; rule of three not yet met (only 2 call-sites). Defer to a future plan if a third overlay site appears.
- **T3 (manifest slug without per-movie JSON)** — forward-looking branch coverage for `per_movie_paths.get(slug)` returning `None` or non-file. Live kb/ guarantees coverage; defer until shape evolves to allow this state, or until T1's hermetic fixture is reused for it.
- **A2 (git push)** — opt-in by user, separate concern.
- **Plan 004 Unit 4 (TC-failover)** — separate plan 011 (or later).

## Context & Research

### Relevant Code and Patterns

- `scripts/build_kh_export.py:80` — `log(level: str, msg: str) -> None` helper; existing call-sites at L488, L499, L532 use `log("warn", ...)` for I/O-degradation breadcrumbs. Pattern to follow for the new fallback paths.
- `scripts/build_kh_export.py:137-168` — `build_manifest_json`; already has the symmetric try/except shape needed for the breadcrumb retrofit.
- `scripts/build_kh_export.py:188-213` — `build_slug_md`; sets `has_json = per_movie_json is not None and per_movie_json.is_file()` then unconditionally calls `json.loads`. The fallback semantic of "treat as if no JSON" already exists when `has_json=False` — the fix flips `has_json=False` and clears `json_data` on parse failure.
- `scripts/tests/test_kh_export.sh:52` — `expect_not_in()` helper for absence assertions (already used at L336-337, L364-366, L401-402, L435-436).
- `scripts/tests/test_kh_export.sh:268-...` — run 5 ("wrapper IMDb-resolved rendering") uses `SYN_TMP=$(mktemp -d)` for hermetic per-movie JSON fixtures; same scaffolding pattern fits T1 and T2.
- `scripts/tests/test_kh_export.sh:178-193` — test 11i; the `2>/dev/null` is on the `python3 -c "…"` invocation, between the closing `"` and `; then`.

### Institutional Learnings

- Plan 009 Unit 3 lineage comment (test_kh_export.sh:178-181) names the load-bearing invariant ("overlay applies to slug header but never mutates rows[]"); preserve it verbatim — only the redirect changes.
- Plan 008 Unit 1 review precedent: sibling fixes for low-severity findings landed in a follow-up commit. Same shape applies here.

### External References

None — fix is purely internal to the export builder.

## Key Technical Decisions

- **Reuse `log("warn", …)` rather than introduce a new diagnostic channel** — already-established pattern at L488/L499/L532; no new dependency, no new shape for callers to learn.
- **`build_slug_md` fallback flips `has_json=False`, not "render with empty json_data"** — the rest of the function already gracefully handles `has_json=False` (lines 217+ all gate on `has_json`). Flipping the flag reuses the existing degraded path; setting `json_data={}` while leaving `has_json=True` would silently emit IMDb-block None-defaults and pollute the wrapper.
- **Bundle the two new tests under run 5 (hermetic SYN_TMP)** rather than creating a new run 6 — both tests want a hermetic per-movie JSON fixture; run 5 already builds that scaffolding. T1 fixture is "corrupt JSON"; T2 fixture is "title==slug, year=None". Both are 1 LOC additions to the existing tmpdir.
- **Drop `2>/dev/null` rather than redirect-and-capture** — the AssertionError messages already use f-string interpolation (`f'MG header title: {mg["title"]!r}'`). Letting them flow to stderr surfaces them in CI output without needing a tempfile dance. Diagnostic value > silence.
- **One commit, not two** — the four fixes are tightly coupled (symmetric error handling + tests proving the symmetry holds + diagnostic preserving testability). Review precedent (plan 008 follow-up) bundled similar low-severity convergent fixes.

## Open Questions

### Resolved During Planning

- *Should the warn breadcrumb fire from `build_manifest_json` even when `build_slug_md` will raise on the same file?* — Resolved: yes, both paths log independently. The warn carries no information about ordering and adding it to both means once `build_slug_md` is hardened, both functions emit the same breadcrumb (signal, not noise; corrupt JSON is rare enough that two log lines is acceptable).
- *Should the suppression test be against title==slug, year==None, or both?* — Resolved: synthetic fixture with `title=slug` AND `year=null` both flips. The `if not title_is_slug and not year_missing` guard fails as soon as either condition is true; one test covers both flags.
- *Should test 11i's `2>/dev/null` be removed or replaced with a captured tempfile?* — Resolved: removed. Tempfile dance adds 3 LOC for no benefit; AssertionError stderr is desired output, not noise.

### Deferred to Implementation

- Exact `log("warn", …)` message wording — implementer chooses concise consistent phrasing (suggested: `f"per-movie JSON unreadable for {slug}; falling back to manifest-derived title/year"`).
- Whether T1's corrupt-JSON fixture uses `'{ not json'` or some other malformed payload — any string that fails `json.loads` works; reviewer suggested `'{ not json'`.

## Implementation Units

- [ ] **Unit 1: Symmetric overlay error handling + warn breadcrumbs**

**Goal:** Make `build_slug_md` graceful on corrupt per-movie JSON to match `build_manifest_json`'s contract; add `log("warn", …)` to both fallback paths so corruption is observable.

**Requirements:** R1, R6.

**Dependencies:** None.

**Files:**
- Modify: `scripts/build_kh_export.py`

**Approach:**
- In `build_manifest_json` (L157-168): inside the existing `except (json.JSONDecodeError, OSError):` clause, replace bare `pass` with a `log("warn", …)` call naming the slug. Keep the silent-fallback semantic (no raise).
- In `build_slug_md` (L205-213): wrap the `json.loads(per_movie_json.read_text(...))` block in `try/except (json.JSONDecodeError, OSError):`. On exception, set `has_json = False` and `json_data = {}`; emit the same `log("warn", …)`. Downstream code already handles `has_json=False` correctly (no IMDb block, no fps/runtime/scdet body sections), so the wrapper renders in bare-manifest mode.
- The "j_title/j_year overlay" inside the try-block stays untouched — only the read+parse is now guarded.

**Patterns to follow:**
- `log("warn", f"…")` call shape at `scripts/build_kh_export.py:488`, `:499`, `:532`.
- Existing try/except shape at `scripts/build_kh_export.py:159-168`.

**Test scenarios:** *(behavior is exercised by Unit 2 and Unit 3 below; this unit is a code-shape change with no standalone test)*
- Test expectation: validated by Unit 2 (build_manifest_json fallback path) and indirectly by run 1 against live kb/ (no regression on happy path).

**Verification:**
- `build_slug_md` no longer raises when per-movie JSON is corrupt; instead returns a bare wrapper.
- `build_manifest_json` and `build_slug_md` both emit `log("warn", …)` when their respective `json.loads` fails.
- Live kb/ run (run 1) emits zero warn lines (no corruption present).
- Idempotency unchanged (run 2 sha-roll-up 71002f59…).

---

- [ ] **Unit 2: Test for `build_manifest_json` overlay error path**

**Goal:** Prove the silent-fallback path is reachable and behaves correctly when per-movie JSON is corrupt.

**Requirements:** R2.

**Dependencies:** Unit 1.

**Files:**
- Modify: `scripts/tests/test_kh_export.sh`

**Approach:**
- Extend run 5 (or add a sibling sub-run inside run 5's hermetic SYN_TMP block) with a new fixture: a slug whose per-movie JSON contains `{ not json` (deliberately malformed).
- Run the builder against the synthetic kb/ tmpdir.
- Assert `manifest.json`'s `slugs[<slug>].title` and `.year` fall back to the manifest.jsonl-derived values (the slug-shaped title and `None` year, since the synthetic fixture mimics the pre-008 shape).
- Assert `slugs[<slug>].rows[]` still ships verbatim from manifest.jsonl (provenance intact through the fallback).

**Patterns to follow:**
- `scripts/tests/test_kh_export.sh:268-...` (run 5 hermetic mktemp scaffolding).
- `scripts/tests/test_kh_export.sh:178-193` (test 11i — same `python3 -c "…"` shape for multi-assertion bundle).

**Test scenarios:**
- Happy path: corrupt per-movie JSON → manifest.json header keeps manifest-derived (slug-shaped) title; year stays `None` from manifest source.
- Integration: raw `rows[]` for the corrupt slug is byte-identical to the rows present in the synthetic manifest.jsonl input.
- Edge: builder exits 0 (no raise from either function); stderr contains the warn breadcrumb naming the slug.

**Verification:**
- New test (e.g., `27.manifest.json fallback on corrupt per-movie JSON`) PASSes.
- Total `test_kh_export` suite count goes 56 → 57.

---

- [ ] **Unit 3: Test for "Title with year" suppression branch**

**Goal:** Prove the parens-year alias body line is absent when title equals slug or year is None.

**Requirements:** R3.

**Dependencies:** None (independent of Unit 1; could in principle land before).

**Files:**
- Modify: `scripts/tests/test_kh_export.sh`

**Approach:**
- Inside run 5's hermetic block, add a fixture with `title == slug` and `year is None` (mimicking the pre-008 bare-MG shape).
- Render the wrapper for that slug.
- `expect_not_in '<run>.suppress no Title with year alias' 'Title with year:' "$out"`
- Optionally: also `expect_in` the slug literal once, to prove the slug still surfaces normally.

**Patterns to follow:**
- `scripts/tests/test_kh_export.sh:336-337`, `:364-366`, `:401-402`, `:435-436` — `expect_not_in` for negative assertions on rendered wrappers.
- Existing run 5 fixture shape (synthetic per-movie JSON path).

**Test scenarios:**
- Happy path: title==slug, year=null → `'Title with year:'` literal absent from rendered wrapper.
- Edge: title!=slug but year=null → suppression still fires (year_missing alone trips the guard).
- Edge: title==slug but year=2020 → suppression still fires (title_is_slug alone trips the guard).

**Verification:**
- New test(s) (e.g., `28a/28b/28c`) PASS.
- Total `test_kh_export` suite count goes 57 → 58 (or more, depending on how many sub-asserts land).

---

- [ ] **Unit 4: Drop `2>/dev/null` from test 11i**

**Goal:** Restore AssertionError stderr visibility so test 11i diagnoses which of its 4 inner asserts failed.

**Requirements:** R4.

**Dependencies:** None.

**Files:**
- Modify: `scripts/tests/test_kh_export.sh` (line 188 only).

**Approach:**
- Remove the `2>/dev/null` between the closing `"` of the python heredoc-style `-c` argument and the `; then`.
- Preserve `>/dev/null` if present on stdout — only stderr is being unsuppressed (test 11i's python script has no print statements, so this is a no-op for happy-path output).
- Lineage comment at L178-181 stays untouched.

**Patterns to follow:**
- Other `python3 -c "…"; then ... else ... fi` blocks in the same file generally don't redirect stderr (e.g., the synthetic-fixture python invocations in run 5 let stderr through). Bringing 11i in line.

**Test scenarios:** *(no new test; this unit improves diagnostic surface of an existing test)*
- Test expectation: existing 11i passes unchanged on green; on a deliberate-break smoke run (e.g., temporarily corrupt manifest.json), the AssertionError message now appears in CI output naming which inner check fired.

**Verification:**
- 11i still PASSes against live kb/.
- Manual smoke (optional): break one of the 4 inner asserts in a scratch branch; confirm the failure message names the broken check rather than the generic `fail "11i.MG manifest.json overlay or provenance broken"`.

## System-Wide Impact

- **Interaction graph:** No new entry points; both modified functions are internal to the export builder. Zero impact on aria2c, contact-sheet sweeper, IMDb resolver, or `/pirata` skill.
- **Error propagation:** The fix narrows propagation — corrupt per-movie JSON now degrades silently with a warn instead of raising upward. Builder exit code is unchanged on corruption (was: nonzero from raise; becomes: zero with warn). This matches the precedent at L488/L499/L532 (missing manifest.jsonl, missing per-movie/, total-empty source).
- **State lifecycle risks:** None — the fallback path produces a bare wrapper / manifest-derived header that already exists as a valid state.
- **API surface parity:** None changed.
- **Integration coverage:** Hermetic tests (Units 2-3) cover branches that live kb/ cannot reach without state corruption; unit-shaped, no integration mocking needed.
- **Unchanged invariants:**
  - `kb/manifest.jsonl` byte-frozen (sha256=9ea712bb…). Untouched.
  - `manifest.json` shape: header carries IMDb-resolved title/year; `slugs[<slug>].rows[]` carries verbatim manifest.jsonl rows. Untouched.
  - Builder idempotency (sha-roll-up 71002f59…). Run 2 must produce byte-identical output.
  - Run 1 happy-path output (per-movie wrappers, README, manifest.json) byte-identical pre and post — Unit 1 only changes behavior on corruption (currently impossible against live kb/).

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| `log("warn", …)` in the corruption path could flood logs in a future bulk-corruption scenario | Acceptable: bulk corruption is itself a Sev1 event, and per-slug warn is the right granularity. No throttling needed. |
| Run 1 against live kb/ accidentally regressed by Unit 1 (e.g., `has_json=False` flip leaks to a non-corruption case) | Unit 1's flip is gated only on `except (json.JSONDecodeError, OSError)`; run 1 must still emit zero warns and produce byte-identical output. Pre-flight: run full test suite before commit. |
| New tests in run 5 break the hermetic SYN_TMP cleanup (leftover dirs in `/tmp`) | run 5 already uses `mktemp -d` and trap-based cleanup. Reuse the same scaffolding; no new cleanup logic. |
| Unit 4's `2>/dev/null` removal makes happy-path CI output noisier | The python script in 11i is silent on success (no print statements). Removing the redirect is a no-op for green runs. |

## Documentation / Operational Notes

- `kb/kh-export/04-derived/README.md` (rendered from `scripts/build_kh_export.py:415-422`): no change. Plan 009 already replaced the transition-state caveat with current-behavior text. The new error-handling symmetry is internal — no user-visible doc impact.
- KH-side replacement runbook (handoff block in plan 009): unchanged. The export shape is byte-identical post Unit 1 (no corruption on live kb/), so the staged GO is unaffected by this plan.

## Sources & References

- **Origin:** `.context/compound-engineering/ce-code-review/20260426-153216-1bc76b28/{correctness,testing,maintainability}.json` (run 20260426-153216-1bc76b28).
- **Predecessor plan:** `docs/plans/2026-04-26-009-fix-kh-export-replacement-handoff-plan.md`.
- **Sibling plan precedent:** `docs/plans/2026-04-26-008-fix-imdb-vote-tie-breaker-plan.md` (post-review tightening commit `1c7c271`).
- Code: `scripts/build_kh_export.py` (build_manifest_json, build_slug_md, log helper), `scripts/tests/test_kh_export.sh` (run 5 hermetic SYN_TMP, expect_not_in helper, test 11i).
