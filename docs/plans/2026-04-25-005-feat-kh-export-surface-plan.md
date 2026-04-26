---
title: "feat: KB export surface for knowledge-hub ingest"
type: feat
status: active
date: 2026-04-25
---

# feat: KB export surface for knowledge-hub ingest

## Overview

Add an additive `kb/kh-export/` surface inside the pirata workspace that exposes pirata's existing per-movie metadata + frame manifest in a shape the local `knowledge-hub` MCP ingester actually accepts. Driven by a real failure mode: the first ingest attempt only indexed 1 of 2 staged files because `kb_discovery.py`/the ingester whitelists `.json/.md/.txt/.yaml/.yml/.csv` and silently skips `.jsonl`. This plan keeps `kb/manifest.jsonl` canonical for pirata, generates a `manifest.json` mirror under the export, and wraps each per-movie JSON with a markdown frontmatter+body so kh chunking has a markdown surface to index.

## Problem Frame

A first kh ingest run staged `per-movie/who-framed-roger-rabbit-1988.json` and `manifest.jsonl` under `~/knowledge-base/09-knowledge-base/pirata-kb/04-derived/`. `mcp__knowledge-hub__ingest_sync()` returned ok, but the kh catalog reported only **1 indexed document**. Rollback was executed and `pirata-kb` is no longer in the catalog. Root cause confirmed: kh's ingest suffix whitelist excludes `.jsonl`, so `manifest.jsonl` is dropped. We need a path that:

1. Keeps `kb/manifest.jsonl` unchanged (it's pirata's canonical append-only ledger).
2. Produces a `.json` mirror that kh WILL index.
3. Adds markdown wrappers around per-movie JSONs so kh's markdown-oriented chunker has a real surface to chunk (improves recall vs raw JSON-as-text).
4. Stays inside the pirata workspace (no writes under `~/knowledge-base/`).
5. Is idempotent and re-runnable as new movies land + after Unit 3 (KB enrichment) augments per-movie JSONs with IMDb fields.

## Requirements Trace

- **R1** — Generate `kb/kh-export/04-derived/per-movie/<slug>.json` as a verbatim copy of `kb/per-movie/<slug>.json` for every populated movie.
- **R2** — Generate `kb/kh-export/04-derived/per-movie/<slug>.md` for every populated movie, with YAML frontmatter (`slug`, `title`, `year`, `fps`, `runtime_s`, `source_size_bytes`, `extracted_at`, `frame_count`, `sheet_count`, `scdet`) + a markdown body grounded only in fields actually present in the source JSON. Body MUST contain the literal strings `Roger Rabbit`, `Who Framed Roger Rabbit (1988)`, `who-framed-roger-rabbit-1988`, and `scdet` so kh smoke retrieve hits a known-present token.
- **R3** — Convert `kb/manifest.jsonl` to `kb/kh-export/04-derived/manifest.json` with shape `{source, kind, row_count, rows}` preserving every JSONL row.
- **R4** — Generate `kb/kh-export/04-derived/README.md` explaining: pipeline-test status, image-asset exclusion for v1, `.jsonl` → `.json` rationale, Unit 3 regeneration trigger, and IMDb non-commercial license stance.
- **R5** — Provide `scripts/build_kh_export.py` as the idempotent regenerator. Multiple consecutive runs MUST produce byte-identical output.
- **R6** — Provide `scripts/tests/test_kh_export.sh` as a hermetic smoke test for R1–R5 plus the explicit invariant that `kb/manifest.jsonl` is unchanged after a run.
- **R7** — Refresh the kh ingest paste-now prompt at `docs/prompts/2026-04-25-kh-ingest-FIRE-v2.md` so its `cp` staging commands point at `kb/kh-export/04-derived/` (not raw `kb/per-movie/` + `kb/manifest.jsonl`), and the validation-result block ends with the new caveat that the manifest is now `.json`-converted.
- **R8** — Image assets (`kb/frames/**/*.jpg`, `kb/contact-sheets/**/*.jpg`) MUST NOT be copied or symlinked into `kb/kh-export/`. Verified by validation step in R6.
- **R9** — All work inside `/Users/vidigal/claude-code/pirata`. No writes under `/Users/vidigal/knowledge-base/`. No MCP tool calls. Original `kb/manifest.jsonl` byte-identical before and after.

## Scope Boundaries

- NOT modifying knowledge-hub source code.
- NOT running `mcp__knowledge-hub__ingest_sync()` or `mcp__knowledge-hub__retrieve` in this plan — that's Codex's job after the export is produced.
- NOT adding image indexing, `.jsonl` support upstream, or license metadata field on the kh side.
- NOT integrating the build script into `sheets_sweep.py` or `contact_sheet.py --kb-export` — that's Unit 3's job once enrichment lands. The build script is run manually for v1.
- NOT writing tests against the live kh corpus or ingest pipeline.

### Deferred to Separate Tasks

- Auto-trigger of `build_kh_export.py` from `sheets_sweep.py` after every sweep — deferred to Unit 3 of the IMDb × pirata coupling plan (`docs/plans/2026-04-24-004-feat-imdb-local-pirata-coupling-plan.md`).
- Markdown wrapper expansion to include IMDb-enriched fields (tconst, rating, top_cast, akas, genres) — deferred to post-Unit-3 regeneration. The wrapper template MUST tolerate optional fields gracefully so post-Unit-3 changes are additive.
- License metadata field convention in kh upstream — out of scope; documented as gap in `kb/kh-export/04-derived/README.md`.

## Context & Research

### Relevant Code and Patterns

- `scripts/contact_sheet.py:21-22` — sys.path guard pattern (drop script's own dir to prevent `queue` shadowing). New script must use same guard.
- `scripts/contact_sheet.py:327-330` — atomic write pattern (`tmp.write(...) ; tmp.replace(target)`). `build_kh_export.py` must mirror this for the directory-level rebuild (build to `kb/kh-export.tmp/` then `os.replace` over `kb/kh-export/`).
- `scripts/sheets_sweep.py:52-58` — `now_iso()` and `sanitize()` helpers. Reuse if logging is added.
- `scripts/imdb_ingest.py` (recent, large) — flow guard pattern, ISO timestamps, exit codes 0/1/2/3 per failure class. New script can be smaller (no FS race like the WAL-safe atomic refresh) but should adopt the same exit-code convention: 0 ok / 1 config / 2 missing input / 3 build failure.
- `scripts/tests/test_imdb_ingest.sh`, `scripts/tests/test_sweep.sh`, `scripts/tests/test_kb_export.sh` — bash + sqlite3 + python3 hermetic test pattern with `pass()` / `fail()` counter and final summary. New test mirrors this exactly.

### Institutional Learnings

- This session previously discovered (via the kh agent's preflight + reading `/Users/vidigal/projects/knowledge-hub/src/knowledge_hub/kb_discovery.py`) that:
  - `kb_discovery.py:489` and `:736` SKIP symlinked directories (`if child.is_symlink(): continue`). Therefore the export must produce real directories + real files, not symlinks. **Confirmed by the FAILED-INGEST run**: even with copies (no symlinks), only 1 of 2 files indexed — root cause was `.jsonl` filtering, not symlinks.
  - kh's ingest suffix whitelist excludes `.jsonl` (newly verified via the failed run's "1 indexed document" output).
  - kh has no license metadata field; license constraints must live in the export's README.

### External References

None needed. The work is local file generation with no external API or framework dependency beyond Python stdlib.

## Key Technical Decisions

- **`kb/kh-export/` lives inside pirata, NOT `~/knowledge-base/`** — Codex still does the cp staging from `kb/kh-export/04-derived/` → `~/knowledge-base/09-knowledge-base/pirata-kb/04-derived/` in its own turn. Keeps separation of concerns, keeps pirata canonical, keeps work additive. The constraint "Do not write under `/Users/vidigal/knowledge-base`" is satisfied.
- **Atomic rebuild via `kb/kh-export.tmp/` + `os.replace`** — mirrors `scripts/contact_sheet.py:327-330`. Ensures partial-write crashes don't leave a half-rebuilt export. For 1 movie this is overkill; for N movies it matters. Adopt now to amortize.
- **Wrapper markdown is generated, not authored** — every per-movie wrapper is regenerated on every build. Manual edits would be silently overwritten. README explains this.
- **manifest.json schema is `{source, kind, row_count, rows}` per spec** — `source` is the literal string `kb/manifest.jsonl`, `kind` is `frame_manifest`, `row_count` is `len(rows)`, `rows` is the array of every parsed JSONL line in order. Sort-stable. Indented with 2 spaces (kh chunking is markdown-oriented but JSON is also accepted; readable indentation aids debugging).
- **Markdown wrapper body grounding** — strictly fields present in the source JSON today. No invented metadata. The 4 literal-string requirements (R2) are satisfied because `slug`, `title`, and `scdet` already appear in the source JSON; the wrapper just surfaces them in markdown.
- **Validation lives in `scripts/tests/test_kh_export.sh`, NOT inline in the build script** — keeps build script simple. Tests cover JSON parseability, markdown literal-string presence, no-JPG invariant, jsonl-unchanged invariant, and idempotency (run twice, byte-identical output).
- **Exit codes** — adopt `imdb_ingest.py` convention: 0 ok / 1 config / 2 missing input / 3 build failure. Helps the future sweeper integration distinguish failure classes.
- **YAML frontmatter is hand-formatted, not via `yaml` lib** — Pillow is the only third-party Python dep in pirata; adding `pyyaml` for one wrapper is over-spec. The fields are simple scalars and a small dict; manual `key: value` formatting is fine. Source JSON values are passed through `json.dumps(..., ensure_ascii=False)` for any string that could contain `:` or `'` to avoid YAML parse hazards.
- **No git operations in the build script** — no auto-commit, no auto-stage. The export under `kb/kh-export/` is generated; whether it's committed is a separate decision.

## Open Questions

### Resolved During Planning

- **Should `kb/kh-export/` be committed to git or gitignored?** — committed. The export is small (1 JSON copy + 1 markdown + manifest.json + README ≈ tens of KB), deterministic from source, and useful as a review surface during PR. If the manifest grows to hundreds of MB, revisit. Not in `.gitignore`.
- **Should the build script support a single-movie filter (`--movie <slug>`)?** — no. v1 always rebuilds the whole export. Atomic replace makes single-movie filtering harder without leaving stale state. Defer until needed.
- **Should the wrapper markdown include the frames array?** — no. 300 frame entries × 7 fields each = ~2100 lines of repetitive JSON-in-markdown noise. Keep wrapper concise (frontmatter + small body). The full per-frame data lives in the JSON sibling, which kh ingests separately.

### Deferred to Implementation

- Exact YAML frontmatter quoting strategy for fields that contain colons or apostrophes — empirically validated against the Roger Rabbit source JSON (no quoting hazards present). If a future per-movie JSON contains tricky strings, the test will catch parse failure.
- Whether `manifest.json` should pretty-print with `indent=2` or compact (`separators=(",", ":")`) — implement with `indent=2` for readability and revisit only if size becomes an issue.

## Output Structure

    kb/kh-export/                          (new, gitignored=NO)
    └── 04-derived/
        ├── per-movie/
        │   ├── who-framed-roger-rabbit-1988.json   (verbatim copy)
        │   └── who-framed-roger-rabbit-1988.md     (markdown wrapper)
        ├── manifest.json                  (converted from kb/manifest.jsonl)
        └── README.md                      (explainer + caveats)

    scripts/build_kh_export.py             (new)
    scripts/tests/test_kh_export.sh        (new)
    docs/prompts/2026-04-25-kh-ingest-FIRE-v2.md   (new — supersedes FIRE.md)

## Implementation Units

- [ ] **Unit 1: `scripts/build_kh_export.py` — idempotent kh-export builder**

  **Goal:** A standalone Python script that regenerates `kb/kh-export/04-derived/` from `kb/per-movie/*.json` + `kb/manifest.jsonl`, atomically and idempotently.

  **Requirements:** R1, R2, R3, R4, R5, R8, R9.

  **Dependencies:** None (Python stdlib only; no third-party packages).

  **Files:**
  - Create: `scripts/build_kh_export.py`

  **Approach:**
  - sys.path guard at top (mirror `scripts/contact_sheet.py:21-22`).
  - `argparse` with `--kb` (default `kb/`) and `--out` (default `kb/kh-export/`).
  - Build into `<out>.tmp/` first; on success, `shutil.rmtree(<out>)` + `<tmp>.replace(<out>)`. Atomic at the directory level.
  - Layer 1: ensure `<tmp>/04-derived/per-movie/` exists.
  - Layer 2: iterate `sorted(kb/per-movie/*.json)` (sorted for determinism). For each:
    - Verbatim copy to `<tmp>/04-derived/per-movie/<slug>.json` via `shutil.copy2`.
    - Generate `<tmp>/04-derived/per-movie/<slug>.md` with `build_movie_md(json_path)`. The function reads the source JSON, extracts the 9 frontmatter fields (slug, title, year, fps, runtime_s, source_size_bytes, extracted_at, frame_count = `len(data.get("frames", []))`, sheet_count = `len(data.get("sheets", []))`, plus the `scdet` dict's three sub-fields). Frontmatter is `---\n` + manual `key: value\n` lines + `---\n`. Body is a fixed template that includes the 4 required literal strings via the `slug`, `title`, and `scdet` fields.
  - Layer 3: parse `kb/manifest.jsonl` line-by-line (skip blank lines), build `{"source": "kb/manifest.jsonl", "kind": "frame_manifest", "row_count": len(rows), "rows": rows}`, write to `<tmp>/04-derived/manifest.json` with `indent=2` + trailing newline.
  - Layer 4: write `<tmp>/04-derived/README.md` from a fixed string constant in the script (no template engine).
  - Atomic swap: if `<out>` exists, `shutil.rmtree`. Then `tmp.rename(out)`.
  - Exit codes: 0 ok / 1 config error / 2 missing `kb/per-movie/` (no movies to export — still proceed but warn) / 3 build failure (exception caught + logged).
  - No logging library; print to stderr with ISO timestamp prefix on warnings/errors.

  **Patterns to follow:**
  - `scripts/contact_sheet.py:21-22` — sys.path guard.
  - `scripts/contact_sheet.py:327-330` — atomic write (adapt to dir-level atomic rebuild).
  - `scripts/imdb_ingest.py` — exit-code convention (0/1/2/3) and ISO timestamp logging style.
  - `scripts/sheets_sweep.py:52-58` — `now_iso()` + `sanitize()` if logging filenames.

  **Test scenarios:** (covered by Unit 2)

  **Verification:**
  - `python3 scripts/build_kh_export.py` exits 0 against the live `kb/`.
  - `kb/kh-export/04-derived/per-movie/who-framed-roger-rabbit-1988.json` exists and is byte-identical to `kb/per-movie/who-framed-roger-rabbit-1988.json`.
  - `kb/kh-export/04-derived/per-movie/who-framed-roger-rabbit-1988.md` parses as YAML frontmatter + markdown body.
  - `kb/kh-export/04-derived/manifest.json` parses as JSON and `row_count` matches the line count of `kb/manifest.jsonl`.
  - `kb/kh-export/04-derived/README.md` exists and is non-empty.
  - `kb/manifest.jsonl` is byte-identical before and after the build.

- [ ] **Unit 2: `scripts/tests/test_kh_export.sh` — hermetic smoke test**

  **Goal:** A bash test that runs `build_kh_export.py` against the live `kb/`, validates every output requirement (R1–R8), and confirms idempotency.

  **Requirements:** R6.

  **Dependencies:** Unit 1.

  **Files:**
  - Create: `scripts/tests/test_kh_export.sh`

  **Approach:**
  - Pre-test: capture `shasum -a 256 kb/manifest.jsonl` for the unchanged-invariant check.
  - Run `python3 scripts/build_kh_export.py`, assert exit 0.
  - Assert `kb/kh-export/04-derived/per-movie/who-framed-roger-rabbit-1988.json` exists, parses (`python3 -c "import json; json.load(open('...'))"`), and is byte-identical to the source via `cmp` or `diff`.
  - Assert `.md` wrapper exists and contains all 4 literal strings via `grep -q` (one assert per string for granular failure messages): `Roger Rabbit`, `Who Framed Roger Rabbit (1988)`, `who-framed-roger-rabbit-1988`, `scdet`.
  - Assert `manifest.json` exists, parses, has `row_count` equal to `wc -l < kb/manifest.jsonl` (after stripping blank lines if any).
  - Assert `README.md` exists and is non-empty.
  - Assert no JPGs in `kb/kh-export/`: `[ "$(find kb/kh-export -name '*.jpg' | wc -l | tr -d ' ')" = "0" ]`.
  - Assert original `kb/manifest.jsonl` checksum unchanged.
  - Run `python3 scripts/build_kh_export.py` a SECOND time. Capture full-tree checksum (`find kb/kh-export -type f | sort | xargs shasum -a 256 | shasum -a 256`). Run a THIRD time, compare checksums — must be byte-identical for idempotency.
  - Final `pass: N / fail: M` summary; exit 0 only if `fail = 0`.

  **Patterns to follow:**
  - `scripts/tests/test_imdb_ingest.sh` (entire structure: hermetic, `pass()`/`fail()` counters, final summary, mktemp tmpdir if isolation needed).
  - `scripts/tests/test_kb_export.sh` (existing test for the `--kb-export` flag — reuse the assertion idiom).
  - `scripts/tests/test_sweep.sh` — `set -uo pipefail` discipline.

  **Test scenarios:**
  - Happy path: build runs against live `kb/`, all 12-15 assertions pass, exit 0.
  - Edge case (idempotency): two consecutive builds produce byte-identical output; full-tree checksum match.
  - Edge case (input invariant): `kb/manifest.jsonl` checksum identical before/after.
  - Error path (no JPG leakage): JPG count in `kb/kh-export` is 0.
  - Error path (missing source — out of scope for v1): the test does NOT exercise the missing-`kb/per-movie/` case because the live workspace always has it. Document as a known coverage gap.

  **Verification:**
  - `bash scripts/tests/test_kh_export.sh` exits 0 with `FAIL: 0`.
  - All assertion labels (`PASS: N.<description>`) are unique and grep-able.

- [ ] **Unit 3: `docs/prompts/2026-04-25-kh-ingest-FIRE-v2.md` — refreshed paste-now prompt**

  **Goal:** A new paste-now prompt for Codex that supersedes `docs/prompts/2026-04-25-kh-ingest-FIRE.md`. Staging commands point at `kb/kh-export/04-derived/` (not raw `kb/per-movie/` + `kb/manifest.jsonl`); validation-result reflects the post-export reality (no `.jsonl`, all files are kh-supported suffixes); caveats list updated.

  **Requirements:** R7.

  **Dependencies:** Unit 1 (the export must exist before Codex stages it).

  **Files:**
  - Create: `docs/prompts/2026-04-25-kh-ingest-FIRE-v2.md`

  **Approach:**
  - Copy the structure of `docs/prompts/2026-04-25-kh-ingest-FIRE.md` (markdown sectional, Codex 5.5 / xhigh style).
  - Update `# Context` to:
    - Add the failed-first-attempt finding: kh ingest suffix whitelist excludes `.jsonl`; pirata workspace now produces a kh-compatible export under `kb/kh-export/` to address this.
    - Reference `scripts/build_kh_export.py` as the export generator.
    - Replace mention of `kb/manifest.jsonl` with `kb/kh-export/04-derived/manifest.json`.
  - Update `# Procedure / 2. Staging` to use these literal commands (replacing the old per-movie + jsonl commands):
    - `cp -R /Users/vidigal/claude-code/pirata/kb/kh-export/04-derived/. /Users/vidigal/knowledge-base/09-knowledge-base/pirata-kb/04-derived/`
    - Or equivalently 4 explicit `mkdir` + `cp` lines for the .json, .md, manifest.json, README.md.
    - Use the explicit form for clarity; the agent's preamble + ls/stat verification is already specified.
  - Update `# Procedure / 4. Smoke retrieve` queries to reflect that kh now sees both the per-movie wrapper markdown and the manifest. Keep the 3-query shape: `Roger Rabbit`, `Who Framed Roger Rabbit 1988`, `who-framed-roger-rabbit-1988`. All three are present in both the wrapper markdown and the source JSON copy. (No need to change queries — they already ground in present content.)
  - Update `# validation-result` block:
    - Decision: GO-WITH-CAVEATS.
    - Slug: `pirata-kb`.
    - Staging method: real directories + plain `cp`. Source path: `/Users/vidigal/claude-code/pirata/kb/kh-export/04-derived/`.
    - Files to copy: `per-movie/who-framed-roger-rabbit-1988.json`, `per-movie/who-framed-roger-rabbit-1988.md`, `manifest.json`, `README.md` (4 files).
    - Exclusions: still no JPG frames, no contact-sheets.
    - Caveats: (1) pre-Unit-3 enrichment, (2) markdown wrappers are auto-generated, (3) re-run `build_kh_export.py` after Unit 3, (4) `degraded_components` expected for new KB without CAG pack, (5) license stance documented in `README.md` not in metadata, (6) the original `kb/manifest.jsonl` remains pirata-canonical and is NOT staged.
  - Update `# Constraints` to add: "If the on-disk export is stale (build_kh_export.py has not been re-run since per-movie JSONs changed), ABORT-PREFLIGHT and ask Vidigal to re-run the export."

  **Patterns to follow:**
  - `docs/prompts/2026-04-25-kh-ingest-FIRE.md` (current, will be superseded — copy structure not content).
  - `docs/prompts/2026-04-25-kh-ingest-v2-eng-codex.md` (the template — markdown sectional, GPT-5.5/xhigh style).

  **Test expectation: none** — this is a documentation/prompt artifact. The "test" is the actual fire run, which is out of scope for this plan (Codex's job).

  **Verification:**
  - `wc -l docs/prompts/2026-04-25-kh-ingest-FIRE-v2.md` returns a non-trivial count (>200 lines).
  - `grep -c "kb/kh-export/04-derived" docs/prompts/2026-04-25-kh-ingest-FIRE-v2.md` ≥ 3 (in context, staging commands, validation-result).
  - `grep -c "manifest.jsonl" docs/prompts/2026-04-25-kh-ingest-FIRE-v2.md` is 0 in the staging commands (the `.jsonl` reference only appears in caveat #6 noting it's pirata-canonical).
  - `grep -q "GO-WITH-CAVEATS" docs/prompts/2026-04-25-kh-ingest-FIRE-v2.md` — validation-result decision is set.

## System-Wide Impact

- **Interaction graph:** `build_kh_export.py` reads `kb/per-movie/*.json` + `kb/manifest.jsonl` (read-only). Writes only under `kb/kh-export/`. No interaction with `scripts/contact_sheet.py`, `scripts/sheets_sweep.py`, or `scripts/queue.py`.
- **Error propagation:** non-zero exit codes (1/2/3) are the contract; future sweeper integration in Unit 3 of the IMDb plan will read these. README documents the convention.
- **State lifecycle risks:** atomic dir-level rebuild via `<out>.tmp` + `os.replace` prevents half-rebuilt state on Ctrl-C. If a previous `.tmp` dir is left over from a crashed run, the next build pre-cleans it before starting.
- **API surface parity:** none — this is a new artifact, no existing kh consumer.
- **Integration coverage:** none beyond Unit 2's smoke test. The actual ingest run (Codex) is the integration test of the whole chain.
- **Unchanged invariants:** `kb/per-movie/*.json` and `kb/manifest.jsonl` remain canonical sources for pirata's own use. `scripts/contact_sheet.py --kb-export` continues to write into `kb/per-movie/` and `kb/manifest.jsonl` as before. No changes to `.claude/skills/pirata-deck/SKILL.md` or the TR-100 panels.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| YAML frontmatter parse hazard if a per-movie JSON contains a string with unescaped `:` or `'` | Validation step in Unit 2 attempts to parse the wrapper markdown; failure surfaces immediately. For Roger Rabbit (only populated movie), no hazards present. Future movies caught by re-running the test. |
| `kb/kh-export/` accidentally committed with stale content if developer forgets to re-run `build_kh_export.py` after editing source JSONs | README documents the rebuild contract. Future Unit 3 sweeper integration auto-rebuilds. For v1, manual discipline. Test runs idempotency check. |
| kh chunker still produces poor recall on JSON-as-text even with markdown wrappers | This is the pipeline test. If smoke retrieve queries return 0 hits despite the wrapper, FAILED-SMOKE → rollback → learn → iterate (e.g., expand wrapper body, add full-text concatenation). The plan's job is to give kh a fair shot, not to guarantee recall. |
| `manifest.json` becomes large if `manifest.jsonl` grows to thousands of frames across many movies | At >10MB, kh chunking may degrade. Defer until empirically observed; document as future concern in README. |
| Atomic dir-level swap (`shutil.rmtree(out)` + `tmp.rename(out)`) has a microsecond window where `kb/kh-export/` doesn't exist | Acceptable: no external consumer reads `kb/kh-export/` between rebuilds. If a parallel `cp -R` from Codex races with `build_kh_export.py`, that's an operational discipline concern, not a code concern. |

## Documentation / Operational Notes

- README in `kb/kh-export/04-derived/` is the user-facing doc. It's regenerated on every build, so its content lives in `scripts/build_kh_export.py` as a string constant.
- No CLAUDE.md update needed; this is internal pipeline tooling.
- No SKILL.md update needed; the `/pirata` skill doesn't surface kh-export status (TR-100 panels would need a new row, deferred to Unit 5 of the IMDb plan).
- Memory snapshot (`MEMORY_DEEP_004.md` or successor) should reference this plan when summarizing the FAILED-INGEST → recovery → re-fire arc.

## Sources & References

- Failed first attempt context: provided in this LFG invocation; root cause confirmed via the kh agent's preflight output (1-doc indexed).
- kh source files: `/Users/vidigal/projects/knowledge-hub/src/knowledge_hub/mcp_server.py` (zero-arg `ingest_sync`), `/Users/vidigal/projects/knowledge-hub/src/knowledge_hub/kb_discovery.py` (sub-layout requirement, symlink skip).
- Prior prompt: `docs/prompts/2026-04-25-kh-ingest-FIRE.md` (will be superseded by FIRE-v2.md in Unit 3).
- Prior plan (parent): `docs/plans/2026-04-24-004-feat-imdb-local-pirata-coupling-plan.md` (this work supports Unit 6 op-step of that plan).
- Related session memory: `MEMORY_DEEP_004.md`.
