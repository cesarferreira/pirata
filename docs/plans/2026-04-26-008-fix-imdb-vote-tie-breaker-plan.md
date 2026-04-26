---
title: "fix: vote-spread tie-breaker for IMDb multi_tie heuristic"
type: fix
status: active
date: 2026-04-26
origin: docs/plans/2026-04-26-007-feat-imdb-kb-enrichment-plan.md
---

# fix: vote-spread tie-breaker for IMDb multi_tie heuristic

## Overview

Calibration patch on top of plan 007 (Unit A — `scripts/imdb_kb_enrich.py`). Adds a Unit-3-layer tie-breaker that demotes `multi_tie` → `resolved` when the top match has overwhelming `numVotes` dominance over the runner-up at the same year. Closes the real-data gap where `scripts/imdb_lookup.py:lookup_by_title` flags famous-vs-obscure same-title pairs as ties (Roger Rabbit movie 233k votes vs same-name 1988 video game 210 votes; Mario Galaxy movie 38k votes vs zero-vote tvEpisodes), even though one entry is clearly dominant.

The fix lands at the consumer side of imdb_lookup (`imdb_kb_enrich.resolve`), not inside imdb_lookup itself — Unit 2's contract is locked per plan 007 boundary. The override is local, narrow, and reversible (single module-level constant for threshold tuning).

## Problem Frame

`scripts/imdb_lookup.py:lookup_by_title` currently flags `multi_tie=True` when:
1. Top-2 candidates are both tier-1 (`fuzz_ratio == 100`, exact-match), AND
2. Their composite scores are within 15 % gap (`CONF_THRESHOLD_PCT`).

The heuristic is purely score-based — it ignores `numVotes` entirely, even though `Match.num_votes` is already in the dataclass. This is correct as a tie-breaker between candidates that genuinely matter: two films named "Dune" from the same era. But IMDb's catalog is full of obscure entries that share exact titles with famous films — same-year video games, tvEpisodes named after the parent series, behind-the-scenes featurettes. They become tier-1 ties with the famous title and trigger multi_tie even when the popularity gap is 100x+.

Real-data evidence collected during plan 007 verification (commit f531c57):

| Title query | year | Top match | Top votes | Runner-up | Runner votes | Ratio | imdb_lookup | Right answer |
|---|---|---|---|---|---|---|---|---|
| Who Framed Roger Rabbit | 1988 | tt0096438 (movie) | 233,248 | tt0295691 (videoGame) | 210 | 1110× | multi_tie | tt0096438 |
| The Super Mario Galaxy Movie | 2026 | tt28650488 (movie) | 38,575 | tt41374083 (tvEpisode) | 0 | ∞ | multi_tie | tt28650488 |
| Bacurau | 2019 | tt2762506 (movie) | 34,690 | (none in tier-1) | — | — | resolved | tt2762506 |

Bacurau resolves cleanly because no other tier-1 candidate exists. Roger Rabbit + Mario Galaxy fail the heuristic gate purely because of catalog noise.

Net effect on the pirata KB pipeline: the canonical title + year still get cleaned up via the PTN fallback path (Unit A), so the headline "Mario Galaxy bug" closes regardless. But the `imdb` block stays bare in the manifest (no tconst, no genres, no rating, no directors), which means `scripts/build_kh_export.py` (Unit C) renders the bare wrapper layout for both releases. The KB catalog is enriched only with title+year, not with the rich metadata Unit 3 was designed to surface in the `## IMDb metadata` section.

## Requirements Trace

- **R1** — `scripts/imdb_kb_enrich.py` exposes a private helper `_apply_vote_tie_breaker(matches: list[Match]) -> list[Match]` that takes the matches list returned by `imdb_lookup.lookup_by_title` and, when warranted, replaces the top match with a `dataclasses.replace(top, multi_tie=False)` clone.
- **R2** — Override fires only when ALL of these hold:
  1. `len(matches) >= 2`
  2. `top.multi_tie == True`
  3. `top.start_year == runner.start_year` (same-year guard; uses Python `==` so `None == None` evaluates True without crashing)
  4. `top.num_votes >= TIE_BREAK_VOTE_RATIO * max(1, runner.num_votes)` (vote-dominance gate; `max(1, …)` avoids div-by-zero / makes 0-vote runners trivially overridden)
- **R3** — `TIE_BREAK_VOTE_RATIO = 10` lives as a module-level constant in `scripts/imdb_kb_enrich.py`, alongside `IMDB_CONFIDENCE_PCT` + `AKAS_CAP`. Calibration is a one-line change.
- **R4** — `resolve()` calls `_apply_vote_tie_breaker(matches)` immediately after `lookup_by_title` returns and before the multi_tie check. The downstream `if top.multi_tie:` branch sees the demoted flag.
- **R5** — When the override fires, the resolved record carries the actual top match's tconst and `confidence=100` (tier-1 fuzz_ratio). The `multi_tie=false` field on the imdb block is the existing field — no new schema bump.
- **R6** — Override does NOT fire when `top.start_year != runner.start_year` (genuine year-disambiguation cases like `Dune` 1984 vs 2021 stay multi_tie when no year hint is passed).
- **R7** — Override does NOT fire when `top.num_votes < 10 × max(1, runner.num_votes)` (genuine same-popularity ambiguity stays multi_tie).
- **R8** — `scripts/tests/test_imdb_kb_enrich.sh` extended with at least 4 new test scenarios:
  - **T11** — Real DB: "Who Framed Roger Rabbit" (with PTN year=1988) → resolved to tt0096438 with full enrichment (genres, rating, directors).
  - **T12** — Real DB: "The Super Mario Galaxy Movie" (slug-shape input, PTN year=2026) → resolved to tt28650488 with full enrichment.
  - **T13** — Synthetic Match dataclasses: top.num_votes=100, runner.num_votes=20, same year → ratio < 10× → multi_tie preserved (override does NOT fire).
  - **T14** — Synthetic Match dataclasses: top.num_votes=50000, runner.num_votes=10, different years → year mismatch → multi_tie preserved (override does NOT fire).
- **R9** — `kb/per-movie/the-super-mario-galaxy-movie-2026.json` and `kb/per-movie/who-framed-roger-rabbit-1988.json` regenerated to carry the resolved imdb block. `kb/kh-export/04-derived/per-movie/*.md` regenerated via `scripts/build_kh_export.py`. Both wrappers gain the `## IMDb metadata` section.
- **R10** — All existing tests still pass (160/160 across the 6 suites — `test_imdb_lookup`, `test_imdb_kb_enrich`, `test_kh_export`, `test_sweep`, `test_contact_sheet_imdb`, `test_queue_wrap`). Test count grows to ~164/164.
- **R11** — `docs/plans/2026-04-26-007-feat-imdb-kb-enrichment-plan.md` annotated under "Risks & Dependencies" or the discovered-limitation section to record that the multi_tie calibration was resolved by plan 008.

## Scope Boundaries

- NOT modifying `scripts/imdb_lookup.py`. Unit 2's contract (locked per plan 007 boundary) stays intact. The score-based multi_tie heuristic at the lookup layer continues to flag tier-1 score ties, and downstream consumers (anything beyond `imdb_kb_enrich`) see the original heuristic. Only `imdb_kb_enrich.resolve` applies the vote-spread override.
- NOT introducing a `kind` filter (e.g., prefer titleType=movie over titleType=videoGame). Vote-spread alone is sufficient for the cases at hand. If a real example surfaces where a videoGame outpolls a same-named movie AND the user's input was a movie file, that's a separate calibration patch.
- NOT changing the threshold default during this implementation. The 10× constant is the v1 tune. Re-tuning lives as a one-line follow-up.
- NOT extending `multi_tie` to tier-2 candidates. Plan 007 sketched the extension but deferred to Unit 2's locked behavior; same boundary holds here.
- NOT touching the JSONL miss log policy. When the override fires, the outcome IS resolved — no miss to log. Existing miss-log behavior for genuine multi_tie / no_match / db_unavailable stays exactly as Unit A specified.
- NOT modifying `kb/manifest.jsonl` (byte-frozen per plan 005). The per-movie JSON is what gets the new imdb block; the canonical ledger reads cleanly through `build_kh_export.py`.

## Context & Research

### Relevant Code and Patterns

- **`scripts/imdb_kb_enrich.py:resolve`** — current flow:
  ```
  matches = imdb_lookup.lookup_by_title(...)
  if not matches: return no_match
  top = matches[0]
  if top.multi_tie: return multi_tie    ← override slot
  ...resolved path
  ```
  Insertion point: between `lookup_by_title` and the `if top.multi_tie` branch.
- **`scripts/imdb_kb_enrich.py:_assemble_imdb_block`** — already maps `top.fuzz_ratio` to `imdb.confidence`. When the override demotes `top.multi_tie`, the assembled block carries `multi_tie=false` automatically; no change needed at this layer.
- **`scripts/imdb_lookup.py:Match`** — frozen dataclass with `multi_tie: bool = False`. Use `dataclasses.replace(top, multi_tie=False)` to produce the demoted clone (frozen dataclasses cannot be mutated in place; `replace` returns a new instance).
- **`scripts/imdb_lookup.py:lookup_by_title`** — sets `multi_tie=True` symmetrically on top-2 (lines around `replace(top, multi_tie=True)` + `replace(runner, multi_tie=True)`). The override only needs to demote the TOP — the runner's flag is irrelevant after the override since `resolve()` only inspects the top.
- **`scripts/tests/test_imdb_kb_enrich.sh`** — existing 34/34 PASS test harness. Pattern: `python3 -` heredoc invocations with `expect_in` / `expect_not_in` assertions. New tests follow the same shape; T13 + T14 use synthetic Match dataclasses to test the helper in isolation without touching the live DB.

### Institutional Learnings

- `docs/solutions/` does not exist in pirata.
- Plan 007's commit message for Unit A explicitly flagged this gap as a Phase 1 calibration follow-up. This plan resolves the follow-up earlier than expected (same day) because the operational impact (bare wrappers for famous titles) was deemed worth fixing now rather than deferring.

### External References

- **Real-data IMDb popularity asymmetry** — observed during plan 007 verification: same-titled obscure entries (videoGames, tvEpisodes) commonly have <100 votes when a famous same-named title has 10k+ votes. The 10× threshold catches the asymmetric cases without firing on genuine same-popularity ties.
- **`Match.num_votes` semantics** — `title_ratings.numVotes` from the IMDb non-commercial bundle. Updates monthly with the IMDb refresh. Zero votes mean no rating data, not zero popularity per se, but it's the best signal available in the local catalog.

## Key Technical Decisions

- **10× vote-ratio threshold (locked, single-line tunable).** Deeper analysis below; the headline rationale is that real catalog noise (videoGames, tvEpisodes named after parents) typically polls 100×+ less than the famous title. 10× is conservative enough to preserve genuine ambiguity (e.g., a famous-vs-also-popular spinoff with 5–8× spread) and aggressive enough to fix the observed cases (Roger Rabbit 1110×, Mario Galaxy ∞).
- **Same-year guard, not "same titleType" filter.** A `start_year != start_year` mismatch suggests deliberate year-disambiguation territory (Dune 1984 vs 2021), and overriding there would auto-pick the wrong year when the user passes no year hint. titleType filtering (e.g., prefer movie over videoGame) is a richer signal but adds complexity for a rare edge case (videoGame more popular than a same-named movie); deferred until a real example surfaces.
- **`max(1, runner.num_votes)` div-by-zero guard, not a special case.** Treating runner-zero-votes as runner-1-vote means the override fires whenever `top.num_votes >= 10`, which matches intuition: a 10-vote film is more popular than a 0-vote tvEpisode tribute. The alternative (special-case zero votes as "no signal, preserve multi_tie") would suppress the override on the most common pattern (zero-vote tvEpisode catalog noise) — exactly what we want to break.
- **Override at the consumer layer, not the producer.** Modifying `imdb_lookup.lookup_by_title` would be the "cleanest" architectural fix but breaches the plan 007 boundary that locks Unit 2's contract. The override-at-consumer approach keeps Unit 2 stable for non-pirata callers (future or hypothetical) and isolates the popularity-aware policy to the pirata-specific KB enrichment context.
- **Helper is private, not exported.** `_apply_vote_tie_breaker` is an internal helper of `imdb_kb_enrich`. Other modules call `resolve()`, which already incorporates the override; they don't need direct access to the helper. Underscore prefix signals internal-only.
- **No new manifest schema fields.** The override demotes `multi_tie` and proceeds through the existing resolved path. The `imdb` block schema is unchanged; only `multi_tie=false` instead of `true` and `result="resolved"` instead of `"multi_tie"`. Downstream consumers (build_kh_export.py wrapper) already handle this correctly.

### Threshold sensitivity (ULTRATHINK)

| Case | Vote ratio | 5× | **10× (chosen)** | 20× | 100× |
|---|---|---|---|---|---|
| Roger Rabbit movie vs videoGame (same year) | 1110× | override | **override** | override | override |
| Mario Galaxy movie vs zero-vote tvEpisodes (same year) | ∞ | override | **override** | override | override |
| Hypothetical 50k vs 30k same-fame, same year | 1.67× | preserve | **preserve** | preserve | preserve |
| Hypothetical 200k vs 25k popular spinoff, same year | 8× | override | **preserve** | preserve | preserve |
| Hypothetical 100k vs 8k mid-popularity, same year | 12.5× | override | **override** | preserve | preserve |
| Dune 1984 vs Dune 2021 (different years) | 7.5× (cross-year) | preserve (year guard) | **preserve (year guard)** | preserve | preserve |

5× is too aggressive (overrides 200k-vs-25k borderline). 100× is too conservative (misses 100k-vs-8k cases that should resolve). 20× is reasonable but rejects 12.5× cases that intuitively should resolve. 10× matches the gut-feel "an order of magnitude more popular" boundary and lands all observed real cases.

Re-tune downward to 5× if the calibration fixture shows too many false-multi_tie cases for valid resolutions; re-tune upward to 20× if false-resolves surface (e.g., a 12× ratio that picked the wrong tconst). One-line change, single test impact.

## Open Questions

### Resolved During Planning

- **Should the helper return a new list or mutate in place?** Return a new list. `Match` is a frozen dataclass; in-place mutation isn't possible. Building a new list with `[replaced_top] + matches[1:]` keeps the function pure and makes the test signal cleaner (input list unchanged after the call).
- **Should `imdb.confidence` reflect the override?** No. Confidence is `top.fuzz_ratio` (100 for tier-1, score for tier-2). The override doesn't change fuzz_ratio; it changes `multi_tie`. So `confidence=100` for tier-1 overridden cases is correct.
- **Should we add a debug field to the imdb block recording that the override fired?** No for v1. The wrapper rendering decision (resolved vs bare) is what matters for retrieval. Adding an audit field bloats the manifest. If calibration becomes contentious, add `imdb.tie_break_applied: bool` later.
- **Should the override run before or after `lookup_by_tconst`?** Before. The override changes which tconst gets enriched (the top one, with multi_tie cleared). `lookup_by_tconst` then runs once on the chosen tconst. Running the override after `lookup_by_tconst` would mean we'd already enriched the wrong tconst before realizing the right one.

### Deferred to Implementation

- **Exact dataclass-import idiom.** The helper needs `from dataclasses import replace`. Whether to import at module top vs inside the helper function is a style choice (PEP 8 says module top); decide during implementation.
- **Whether T13/T14's synthetic Match construction needs a small factory helper.** If the synthetic Match dataclass has many required fields, a `_make_match(...)` test helper might be cleaner. Decide while writing the test based on field count.

## Implementation Units

- [ ] **Unit 1: `scripts/imdb_kb_enrich.py` — vote-spread tie-breaker helper + integration**

  **Goal:** Add `_apply_vote_tie_breaker` and call it from `resolve()` immediately after `lookup_by_title`. Demotes top.multi_tie to False when vote-dominance + same-year guards both hold.

  **Requirements:** R1, R2, R3, R4, R5, R6, R7.

  **Dependencies:** plan 007 Unit A (already shipped at commit `1b610a9`).

  **Files:**
  - Modify: `scripts/imdb_kb_enrich.py`

  **Approach:**
  - Add module-level constant `TIE_BREAK_VOTE_RATIO = 10` near the existing `IMDB_CONFIDENCE_PCT = 15` and `AKAS_CAP = 10` constants.
  - Add `from dataclasses import replace` to the imports block (the existing block already does `from dataclasses import asdict, dataclass`).
  - Define `_apply_vote_tie_breaker(matches: list[Match]) -> list[Match]`:
    - Guard: if `len(matches) < 2`, return matches unchanged.
    - Guard: if not `matches[0].multi_tie`, return matches unchanged.
    - Same-year guard: if `matches[0].start_year != matches[1].start_year`, return matches unchanged.
    - Vote-dominance: if `matches[0].num_votes < TIE_BREAK_VOTE_RATIO * max(1, matches[1].num_votes)`, return matches unchanged.
    - Override fires: build `new_top = replace(matches[0], multi_tie=False)`. Return `[new_top] + matches[1:]`. The runner's `multi_tie=True` is left intact since `resolve()` only inspects the top.
  - In `resolve()`, after the `try/except IMDbDBUnavailable` block that produces `matches`, insert `matches = _apply_vote_tie_breaker(matches)` before the `if not matches` and `if top.multi_tie` branches.
  - Document the override inline with a docstring on the helper explaining the rationale + same-year guard + threshold tuning location.

  **Patterns to follow:**
  - Existing private helpers in `scripts/imdb_kb_enrich.py` use leading-underscore names (`_parse_filename`, `_get_directors`, `_assemble_imdb_block`, `_log_miss`).
  - The `dataclasses.replace` pattern is already used in `scripts/imdb_lookup.py:lookup_by_title` to mutate the multi_tie flag (look for `replace(top, multi_tie=True)` near the multi_tie block).

  **Test scenarios:**
  - **Happy path: real DB, Roger Rabbit 1988 with PTN year=1988** — input `Who.Framed.Roger.Rabbit.1988.1080p.BluRay.x264` → `imdb.result == "resolved"`, `imdb.tconst == "tt0096438"`, `imdb.confidence == 100`, `imdb.director` contains "Robert Zemeckis" (or whatever the live DB returns; assert non-empty director list + the famous tconst). Logged in `test_imdb_kb_enrich.sh:T11`.
  - **Happy path: real DB, Mario Galaxy slug input** — input `the-super-mario-galaxy-movie-2026` → `imdb.result == "resolved"`, `imdb.tconst == "tt28650488"`, `imdb.confidence == 100`, `imdb.rating.average` is non-null, `imdb.rating.votes >= 10000`. Logged in `test_imdb_kb_enrich.sh:T12`.
  - **Edge case: synthetic Match dataclasses, top.num_votes=100, runner.num_votes=20, same year** — ratio 5× < 10× → `_apply_vote_tie_breaker` returns matches unchanged, top.multi_tie stays True. Logged in `test_imdb_kb_enrich.sh:T13`.
  - **Edge case: synthetic Match dataclasses, top.num_votes=50000, runner.num_votes=10, different years** — year mismatch → helper returns matches unchanged. Logged in `test_imdb_kb_enrich.sh:T14`.
  - **Edge case: synthetic Match dataclasses, top.num_votes=100, runner.num_votes=0, same year** — `max(1, 0) = 1`, ratio 100× >= 10× → override fires, top.multi_tie becomes False (cover the zero-runner path explicitly).
  - **Edge case: single-match list** — pass `[top]` with `top.multi_tie=True` → helper returns unchanged (len < 2 guard).
  - **Edge case: empty match list** — pass `[]` → helper returns unchanged.

  **Verification:**
  - `bash scripts/tests/test_imdb_kb_enrich.sh` exits 0 with PASS count >= 38 (currently 34; gains at least T11, T12, T13, T14).
  - All other test suites unchanged in PASS count.
  - `scripts/imdb_kb_enrich.py` syntax passes `python3 -m py_compile`.

- [ ] **Unit 2: Regenerate Mario Galaxy + Roger Rabbit per-movie JSONs + re-export + plan 007 annotation**

  **Goal:** Land the data win the override unlocks: Mario Galaxy + Roger Rabbit per-movie JSONs gain the resolved imdb block; `kb/kh-export/04-derived/per-movie/*.md` wrappers surface the `## IMDb metadata` section. Plan 007 annotation records that the deferred calibration follow-up is now resolved by plan 008.

  **Requirements:** R9, R10, R11.

  **Dependencies:** Unit 1 (override must exist before regeneration produces the right shape).

  **Files:**
  - Modify: `kb/per-movie/the-super-mario-galaxy-movie-2026.json`
  - Modify: `kb/per-movie/who-framed-roger-rabbit-1988.json`
  - Modify: `kb/kh-export/04-derived/per-movie/the-super-mario-galaxy-movie-2026.json`
  - Modify: `kb/kh-export/04-derived/per-movie/the-super-mario-galaxy-movie-2026.md`
  - Modify: `kb/kh-export/04-derived/per-movie/who-framed-roger-rabbit-1988.json`
  - Modify: `kb/kh-export/04-derived/per-movie/who-framed-roger-rabbit-1988.md`
  - Modify: `kb/kh-export/04-derived/manifest.json` (slug-grouped manifest gets re-rendered by build_kh_export)
  - Modify: `docs/plans/2026-04-26-007-feat-imdb-kb-enrichment-plan.md`

  **Approach:**
  - Surgical-patch script (one-shot Python invocation, mirrors plan 007 Unit G's surgical-patch approach): for each of the two slugs, read the existing per-movie JSON, run `imdb_kb_enrich.resolve(<existing title>, slug=<slug>)`, build the patched dict (top-level title/year canonical + filename block + imdb block), atomic-write back via temp+rename. Frames + sheets + scdet config carry over verbatim.
  - Re-run `python3 scripts/build_kh_export.py` to regenerate `kb/kh-export/04-derived/`. The Mario Galaxy + Roger Rabbit wrappers gain the `## IMDb metadata` section automatically (Unit C of plan 007 already wired the rendering).
  - Test update in `scripts/tests/test_kh_export.sh`: the existing T11c/T11d/T11e suite for Mario Galaxy needs to acknowledge the new resolved state. Add T11f checking `## IMDb metadata` section is now present in the wrapper. T11e (multi_tie caveat) likely needs to flip to assert ABSENCE post-override.
  - Plan 007 annotation: add a one-line note under the "Risks & Dependencies" `multi_tie heuristic` row (or wherever the calibration follow-up was documented) pointing at plan 008's resolution.

  **Patterns to follow:**
  - Plan 007 Unit G's surgical patch (commit `f531c57`) is the template — read existing JSON, run resolve, patch in place, atomic write. Same dict ordering: slug, title, year, fps, runtime_s, source_file, source_size_bytes, scdet, extracted_at, filename, imdb, frames, sheets.
  - `kb/manifest.jsonl` stays untouched (byte-frozen per plan 005). The patch only modifies `kb/per-movie/*.json`.

  **Test scenarios:**
  - **Happy path: Mario Galaxy wrapper has IMDb section** — `kb/kh-export/04-derived/per-movie/the-super-mario-galaxy-movie-2026.md` contains `## IMDb metadata`, frontmatter has `tconst: tt28650488`. Updated `test_kh_export.sh:T11f`.
  - **Happy path: Roger Rabbit wrapper has IMDb section** — same shape for `who-framed-roger-rabbit-1988.md`, tconst `tt0096438`. New `test_kh_export.sh:T11g`.
  - **Edge case: T11e multi_tie caveat must NOT appear in Mario Galaxy wrapper post-override** — assert absence (was a positive assertion before; flip to expect_not_in).

  **Verification:**
  - `bash scripts/tests/test_kh_export.sh` exits 0 with PASS count adjusted appropriately (gain T11f, T11g; T11e flips polarity).
  - Manual inspection: both wrappers carry director, rating, genres in YAML + body section.
  - Plan 007 annotation visible under `git diff`.

## System-Wide Impact

- **Interaction graph:** `scripts/contact_sheet.py:export_kb` (when `--kb-imdb` on) → `imdb_kb_enrich.resolve` → `_apply_vote_tie_breaker` → `imdb_lookup.lookup_by_title`. The new helper is a single insertion in the chain; no callbacks, no event handlers. The override changes ONLY which `imdb.result` value lands in the per-movie JSON; downstream `build_kh_export.py` rendering already branches on `imdb.result == "resolved"` correctly (Unit C of plan 007).
- **Error propagation:** Override never raises. If matches list is malformed, the helper guards return matches unchanged (input passes through). If `Match.num_votes` is somehow None (shouldn't be — int field with default 0), `max(1, None)` would TypeError; defensive code path would be `max(1, top.num_votes or 0)`. v1 trusts the dataclass shape.
- **State lifecycle risks:** No persistent state. The helper is pure (input list → output list, no side effects). The miss-log writer in `_log_miss` is unaffected — when override fires, the result is resolved, so no log line writes. Existing miss-log behavior for non-overridden multi_tie / no_match / db_unavailable is unchanged.
- **API surface parity:** The `--kb-imdb` flag in `contact_sheet.py` and the `--kb-imdb` pass-through in `sheets_sweep.py` (Unit D of plan 007) are unaffected. SKILL.md DOCTOR contract row (Unit E) is unaffected. The override is invisible to all flag consumers.
- **Integration coverage:** Unit 1's tests exercise both the helper-in-isolation (synthetic Match dataclasses, T13 + T14) and the helper-via-resolve-via-real-DB integration (T11 + T12). Unit 2's regeneration step is the end-to-end smoke that proves the override produces the right wrapper output.
- **Unchanged invariants:** `kb/manifest.jsonl` byte-frozen ledger contract holds. `imdb_lookup.py` Unit 2 contract holds. `Match` and `Title` dataclass shapes hold. The `imdb` block schema in per-movie JSONs holds. JSONL miss-log JSONL line schema holds.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| 10× threshold proves too aggressive (false-resolves on borderline cases like 12× when one of them is the "wrong" tconst per user intent) | One-line tune at the constant. Re-running this plan's verification with the new threshold catches the regression in T11 / T12. The PT-BR fixture (still 7/7 from plan 007) is the calibration corpus. |
| 10× threshold proves too conservative (still flags multi_tie on cases where user wants resolved, e.g., 8× spread on a famous-vs-also-popular title) | Same one-line tune; lower to 5×. The opposite tune. |
| `Match.num_votes` semantics drift in a future imdb_lookup change (e.g., switches from numVotes to a derived popularity score) | Override depends only on `Match.num_votes` being numeric and ordered. Unit 2's contract locks the field name; if Unit 2 ever changes, this plan's test breaks loudly and signals re-calibration is needed. |
| Same-year override misses cross-year edge case where the famous title moved year (e.g., a re-release with a new tconst at a different year) | Acceptable. Cross-year ambiguity is rare in IMDb data; when it does occur, multi_tie flagging is the right answer (genuine ambiguity). The user can pass `year=` hint to disambiguate. |
| Helper accidentally fires on a resolved match where someone manually tampered with `multi_tie=True` outside imdb_lookup's heuristic | The helper's guard `if not matches[0].multi_tie: return matches` short-circuits. Only fires when imdb_lookup's heuristic already flagged the tie, which is the correct invariant. |
| videoGame outpolls a same-titled movie (the M2 edge case from Unit A's discussion) | Accepted as Phase-2 follow-up. Not blocking. If it surfaces with a real example, add a `kind` filter at the helper layer (Unit-3-layer) without modifying imdb_lookup. |

## Documentation / Operational Notes

- **No README / runbook updates needed.** The override is internal calibration. Users invoking `--kb-imdb` continue to get the same surface; they just see more `result=resolved` outcomes for the existing famous-title corpus.
- **Re-staging to knowledge-hub** — operator-driven step. Same FIRE-v3 prompt at `docs/prompts/2026-04-26-kh-ingest-FIRE-v3.md` covers the Codex-side re-stage + ingest. After plan 008 lands, the re-stage will index richer wrappers (with `## IMDb metadata` sections), making retrieval more useful for the existing pirata-kb slugs.
- **Post-tune verification protocol** — if the threshold ever changes, re-run `bash scripts/tests/test_imdb_kb_enrich.sh` and `bash scripts/tests/test_kh_export.sh`. T11 + T12 are the canary scenarios; T13 + T14 verify the guards still hold.

## Sources & References

- **Origin document:** [docs/plans/2026-04-26-007-feat-imdb-kb-enrichment-plan.md](2026-04-26-007-feat-imdb-kb-enrichment-plan.md) (the calibration follow-up referenced under Risks & Dependencies).
- **Unit A ship commit:** `1b610a9` — `feat(imdb): Unit A — imdb_kb_enrich.py resolution helper for KB manifests`.
- **Unit G surgical-patch precedent:** `f531c57` — `data(imdb): Unit G — Mario Galaxy regression closes via Unit 3 enrichment`.
- **`imdb_lookup.py:lookup_by_title` multi_tie heuristic:** locked in commit `8e31539` (Unit 2 of plan 004); see the `both_tier1` + `within_pct` block.
- **Real-data evidence:** captured in plan 007's commit-message of Unit A (`1b610a9` body: "Discovered limitation (deferred to Phase 1 calibration, not blocking)") and Unit G (`f531c57` body: "The IMDb tconst doesn't anchor because lookup_by_title returns 14 candidates...").
