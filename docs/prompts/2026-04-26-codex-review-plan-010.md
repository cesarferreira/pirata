# Codex prompt — independent review of plan 010 + autofix

**Created:** 2026-04-26
**Audience:** OpenAI Codex CLI (separate session, no shared context with the originating Claude session)
**Goal:** Second-opinion diff review of commits `2a75b63..ff10929` to surface defects Claude's 8-reviewer autofix pass missed.

**Why a second pass:** The Claude-side autofix run (`20260426-154856-e8a6ec55`) caught one P2 defect (`UnicodeDecodeError` missing from the except tuple) that Claude's own plan and initial review missed. Independent review is the cheapest insurance against equivalent class-of-error misses still in the diff.

---

## Prompt (copy-paste below this line)

```
Independent code review of two fresh commits in /Users/vidigal/claude-code/pirata.

Repo: /Users/vidigal/claude-code/pirata (main branch, local, pure Python + bash).
Commits to review: 2a75b63..ff10929 (range, both included).
  - 2a75b63 fix(kh-export): plan 010 — symmetric overlay error handling + branch coverage
  - ff10929 fix(kh-export): plan 010 autofix — UnicodeDecodeError + warn-grep split

Plan:
  docs/plans/2026-04-26-010-fix-kh-export-symmetric-errors-plan.md

Origin (the run that motivated plan 010):
  .context/compound-engineering/ce-code-review/20260426-153216-1bc76b28/
  (correctness.json + testing.json + maintainability.json from the 3-reviewer Claude-side
  pass on plan 009 Unit 3; the 4 convergent findings here became plan 010's units.)

Self-review (Claude's 8-reviewer pass on plan 010 itself):
  .context/compound-engineering/ce-code-review/20260426-154856-e8a6ec55/
  - findings.md (synthesized report)
  - metadata.json (run summary)
  - correctness.json, testing.json, maintainability.json, project-standards.json,
    agent-native.json, learnings.json, reliability.json, kieran-python.json
  Two findings landed as safe_auto (committed as ff10929):
    1. kieran-python-001 (P2/medium, conf 0.86) — UnicodeDecodeError missing from both
       overlay try/except tuples in build_kh_export.py L167 and L219; Path.read_text(
       encoding="utf-8") raises it on non-UTF-8 bytes; UnicodeDecodeError is a ValueError
       subclass and would have escaped (json.JSONDecodeError, OSError) and crashed the
       builder via main()'s catch-all, contradicting the stated symmetric-fallback contract.
    2. T4 / T-010-1 (P3, 3-reviewer convergence) — run-6 warn-breadcrumb assertion grepped
       a shared prefix; a regression silently dropping one of the two warn paths
       (build_manifest_json header-fallback vs build_slug_md bare-wrapper-fallback) would
       still pass. Split into 30a + 30b for distinct-suffix matching.
  11 advisory residuals deferred (all owner=human):
    - rel-001 (P3): no aggregate counter / non-zero exit on partial corruption
    - rel-002 (P3): no circuit-breaker before atomic swap of tmp/ → out/
    - rel-003 (info): log() has no SIGPIPE / BrokenPipeError guard
    - T5 (P3): no_match caveat branch (build_slug_md L384-386) has no fixture
    - M-010-1..7 (P3/info): duplication grew 5→12 LOC per site (still under rule-of-three);
      fixture DRY pressure in run 5; pre-existing trap-overwrite asymmetry across runs 4/5
    - AN-1 (info): optional manifest.json meta.warnings[] for agent consumers that
      can't scan stderr

Files touched in the 2-commit range:
  scripts/build_kh_export.py
    - build_manifest_json L137-180: try/except (json.JSONDecodeError, OSError,
      UnicodeDecodeError) around json.loads; on failure, log("warn", ...) and keep
      manifest-derived title/year. New `per_movie_paths` kwarg threads in the per-slug
      JSON path map (was already added by plan 009).
    - build_slug_md L188-227: NEW try/except around json.loads of per_movie_json with
      same exception tuple; on failure, has_json=False AND json_data={} so all
      downstream `if has_json:` and `json_data.get(...)` branches behave like the
      no-JSON path; log("warn", ...) emitted.
    - L297-298: parens-year alias body line `Title with year: <T> (<Y>)` is the
      target of plan 010 Unit 3's suppression-branch tests; only emitted when
      `not title_is_slug and not year_missing`.
  scripts/tests/test_kh_export.sh
    - L188 (test 11i): dropped `2>/dev/null` so AssertionError stderr surfaces which
      of the 4 inner asserts failed (correctness-2 from plan 009 review).
    - L463-562 (run 5 sub-tests 28a-f): three near-identical heredoc fixtures + python
      -c PYEOF blocks driving build_slug_md directly with synthetic per-movie JSONs
      that trip the L297-298 guard via three flag combinations (both flags True;
      year-only missing with title overlaid; title-only equals slug with year overlaid).
    - L564-619 (NEW run 6, tests 29-35): hermetic kb/-shape tmpdir invokes the full
      builder via `--kb $KB6 --out $OUT6` against (a) a corrupt JSON fixture
      (`{ not json`) covering JSONDecodeError, then (b) a non-UTF-8 byte fixture
      (printf '\xff\xfe garbage \xff\xfe') covering UnicodeDecodeError. Asserts:
      builder exits 0; warn breadcrumbs from BOTH overlay sites; manifest.json header
      falls back to manifest-derived; raw rows[] preserved verbatim; bare wrapper
      still rendered without IMDb section.
    - Trap chain at L565: combined `trap "rm -rf $SYN_TMP $KB6 $KB7" EXIT` (runs 4
      and 5 still use single-tmpdir traps; pre-existing asymmetry).

Tests: 184/184 → 198/198 across 6 suites:
  test_imdb_lookup        25/25
  test_imdb_kb_enrich     54/54
  test_kh_export          70/70  (was 56; +6 suppression sub-tests + +5 corrupt-JSON
                                  sub-tests + +3 from autofix split/non-UTF-8 fixture)
  test_sweep              15/15
  test_contact_sheet_imdb 26/26
  test_queue_wrap          8/8

Idempotency: sha-roll-up of kb/kh-export tree against live kb/ remains
71002f59a67b48cc97fafd02f771ae87e601df1120b336e830cd31526998c2e9 (matches plan 009's
stable value; happy path produces byte-identical output to pre-plan-010).

What I want from you:

1. Independent diff review of 2a75b63..ff10929. Don't trust the synthesis above —
   re-read both commits cold (`git show 2a75b63`, `git show ff10929`) and form your
   own opinion. Read the post-fix files end-to-end, not just the diff hunks.

2. Specifically hunt for things Claude's 8-reviewer pass missed:
   a. Other exception families Path.read_text or json.loads can raise that escape
      the new (json.JSONDecodeError, OSError, UnicodeDecodeError) tuple. Edge cases:
      file is a directory, file is a broken symlink, file is a FIFO/socket, JSON is
      a valid string-encoded number where slug expects an object, json.loads on a
      huge file blowing recursion limits.
   b. Race conditions or TOCTOU between `per_movie_json.is_file()` and
      `per_movie_json.read_text(...)` in build_slug_md, or between
      `json_path.is_file()` and `json_path.read_text(...)` in build_manifest_json.
      The pirata pipeline writes per-movie JSONs from contact_sheet.py before
      build_kh_export runs; in principle a concurrent run of contact_sheet.py while
      build_kh_export is mid-flight could race.
   c. Edge cases in the parens-year suppression guard (build_kh_export.py L297-298)
      the 6 sub-fixtures don't cover. Specifically: per-movie JSON title field that
      is whitespace-only ("   "), title that is None vs missing entirely, title that
      starts with the slug as a substring, year = 0 (falsy non-None integer).
   d. Test scaffolding bugs: trap chain at run 6's L565 combines $SYN_TMP $KB6 $KB7,
      but if test 34's mktemp succeeds and then tests 34/35 fail before completion,
      $KB7 still cleans up — verify. mktemp -d -t flag portability between BSD
      (macOS) and GNU (Linux/CI); pirata is mac-only per CLAUDE.md but is the test
      portable to a future CI? printf with raw \xff bytes — does it produce the
      expected non-UTF-8 byte sequence on every shell pirata might run under?
   e. Determinism: the new log("warn", ...) call writes to stderr with an ISO
      timestamp prefix (see scripts/build_kh_export.py:80-81). Does the warn line
      appearance affect the manifest.json byte-equality invariant? The sha-roll-up
      71002f59 was measured against the kb/kh-export tree, not stderr — but if any
      future check captures stderr into the determinism guard, the timestamp would
      break it. Worth flagging.
   f. The `has_json = False; json_data = {}` reset on exception in build_slug_md
      (plan 010 Unit 1) — does any downstream branch in build_slug_md after L227
      assume `has_json is True implies json_data is truthy and well-formed`? Walk
      L228-end and verify every `if has_json:` site is correctly gated on the new
      contract, not just the obvious ones.
   g. Symmetry contract: the warn message strings differ deliberately between the
      two functions ("manifest.json header falls back to manifest-derived title/year"
      vs "falling back to bare-wrapper rendering"). The autofix split test 30 into
      30a/30b to detect a future regression that drops one warn site. Does the
      shared prefix `per-movie JSON unreadable for {slug}` itself need to be locked
      so a future maintainer doesn't drift one of them?

3. If you find anything actionable, classify it as P0 / P1 / P2 / P3 with confidence
   (0.0-1.0) and an autofix-safety call (safe_auto / gated_auto / manual / advisory).
   Diff-only fixes — don't refactor surrounding code, don't propose extracting the
   overlay helper (that deferral is documented in plan 010 as M-009-1, awaits 3rd
   call-site).

4. If you confirm the work is solid, say so plainly and exit. No padding, no
   recommendations beyond what you found, no advisory restatement of items already
   in the residual list.

Operational notes:
- Repo is small (~25 Python+shell files in scripts/). Read the actual code; don't
  speculate about pirata's broader architecture (torrentclaw MCP, aria2c queue,
  contact-sheet sweeper) unless directly relevant to a finding.
- Tests run via `bash scripts/tests/test_kh_export.sh` (and the other 5 suites);
  prerequisite is contact_sheet.py outputs in kb/per-movie/ which already exist
  on this checkout. Don't run tests if you only need to read the code; do run
  test_kh_export.sh if a finding needs reproduction.
- Pirata is mac-only per CLAUDE.md ("Personal Mac-based media workspace"); BSD
  mktemp / shasum patterns are intentional, not portability bugs.
- docs/plans/, docs/brainstorms/, docs/solutions/, .context/ are protected
  pipeline artifacts — never propose deleting or gitignoring them.
```
