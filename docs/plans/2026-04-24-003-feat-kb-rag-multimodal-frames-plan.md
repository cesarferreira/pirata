---
title: KB-ready frames + clean contact sheet export for multimodal RAG
type: feat
status: active
date: 2026-04-24
origin: docs/brainstorms/2026-04-24-kb-rag-multimodal-frames-requirements.md
---

# KB-ready frames + clean contact sheet export for multimodal RAG

## Overview

Extend `scripts/contact_sheet.py` with an opt-in `--kb-export <dir>` flag that, in a single pipeline pass, also emits a parallel set of **clean** artifacts (raw frames as JPEG with no overlays + clean tiled sheets without caption strips/header + per-movie JSON manifest + global JSONL line append) into a central knowledge-base directory at `~/claude-code/pirata/kb/`. Wire the sweeper to pass `--kb-export` by default with a `--no-kb` opt-out. Result: every release the sweeper processes also lands a RAG-multimodal-ready dataset, with no manual post-processing.

## Problem Frame

The existing pipeline produces only **labeled** sheets (caption strips with `N 001 · TC HH:MM:SS:FF` below each thumb + header). These captions pollute vision-model embeddings — CLIP/SigLIP/Moondream/Claude-vision will OCR the burned-in text, contaminating retrieval. Raw frames are extracted but deleted by default. There is no per-frame metadata sidecar. There is no global manifest for RAG ingestion. The user wants the sweeper to also produce a clean, structured dataset on every run, ready for any downstream multimodal RAG pipeline (LlamaIndex, Haystack, custom embedder, or local `knowledge-hub` MCP).

See origin: `docs/brainstorms/2026-04-24-kb-rag-multimodal-frames-requirements.md` for the full problem framing, alternatives considered, and decisions resolved.

## Requirements Trace

- **R1.** Sweeper run on a fresh release produces clean frames + clean sheets + per-movie JSON + JSONL line in `~/claude-code/pirata/kb/`, alongside the existing labeled output (which stays unchanged) (see origin: R1).
- **R2.** Frames in `<kb>/frames/<slug>/` have no visible text, no caption, no index marker — pristine thumbnails (see origin: R2).
- **R3.** Frame naming: `<slug>_frame_NNN.jpg` exactly, NNN zero-padded 3 digits matching the contact sheet index (see origin: R3).
- **R4.** Clean contact sheets in `<kb>/contact-sheets-clean/<slug>/` use the same 6×5 grid layout as labeled, minus caption strip + header (see origin: R4).
- **R5.** Per-movie JSON manifest follows the schema in the origin doc, always 300 frame entries on full run (see origin: R5).
- **R6.** Global JSONL is valid line-delimited JSON, atomic per-movie append (see origin: R6).
- **R7.** Re-running on already-exported movie is idempotent (skip unless `--kb-force`) (see origin: R7).
- **R8.** Sweeper `--no-kb` produces current behavior (labeled only, no KB) (see origin: R8).
- **R9.** Disk cost ≤80MB per movie (300 frames × ~80KB JPEG + 10 sheets × ~5MB + manifest) (see origin: R9).

## Scope Boundaries

- KB export is opt-in at `contact_sheet.py` level (flag-driven), default-on at `sheets_sweep.py` level (matches `--autosheets` pattern from `queue.py`)
- Single-host filesystem; no remote KB push, no network I/O
- JPEG q=90 only (no PNG/WebP option flag in v1)
- 640px frame width matches existing thumb (no separate KB resolution flag in v1)
- Slug derivation reuses existing `slugify()` from `contact_sheet.py` — no new naming logic

### Deferred to Separate Tasks

- **Moondream caption per frame**: schema reserves `caption: null`; a future `--kb-caption` flag would populate. Adds ~10min/movie. Out of scope for v1 (see origin: deferred).
- **`mcp__knowledge-hub__ingest_sync` auto-ingest**: hook into sweeper post-step. Decision out of scope; see origin's deferred section.
- **IPTC/XMP embedded metadata via exiftool**: travels-with-file metadata. Sidecar JSON sufficient for v1.
- **Mega-sheet (single 300-thumb fingerprint image)**: out of v1.
- **Cross-rip dedup** (1080p vs 2160p of same movie): manifest's `source_file` is the tiebreaker; second run overwrites. Acceptable for v1.
- **`--kb-prune` utility** for orphaned frames after release deletion: future v2.
- **`--kb-rebuild-manifest` utility** for regenerating `manifest.jsonl` from per-movie JSONs after corruption: future v2.

## Context & Research

### Relevant Code and Patterns

- `scripts/contact_sheet.py` — pipeline this plan extends. Key existing constants/functions to reuse:
  - `slugify()` (line ~152) — title → filename slug. Already handles unicode/special chars.
  - `fmt_tc_ff()` — broadcast TC. Manifest uses this format.
  - `probe_fps()` — FPS detection. Manifest uses output.
  - `_extract_one()` — produces raw `raw_NNNN.png` files. These are the source of clean frames; today they're deleted in cleanup unless `--keep-raw`.
  - `label_frame()` — composites thumb + caption strip. Clean variant must skip this composition step.
  - `tile_sheets()` — assembles sheets with header. Clean variant needs caller-controlled "skip header + skip caption strip".
  - `results` list (in `main()`) holds `(idx, t, path)` tuples — the source of raw frames before they get labeled and deleted.
- `scripts/sheets_sweep.py` — existing wrapper. Already passes `--threshold 8 --floor 4 --target 300 --cols 6 --rows 5 --width 640 --workers 6 --` to contact_sheet. Just needs `--kb-export ~/claude-code/pirata/kb/` appended (when not `--no-kb`).
- `scripts/queue.py` — pattern reference for `argparse.BooleanOptionalAction` (used for `--autosheets`).
- `.claude/skills/pirata-deck/SKILL.md` — DOCTOR workflow already has CONTRACT check that greps `contact_sheet.py --help` for required flags. New `--kb-export` flag should join that check.

### Institutional Learnings

- **`scenes_raw_t8.txt` cache file in output dir** breaks naive "only PNG" collision detection — already handled in sheets_sweep by checking for `*_sheet_*.png` presence. KB export is in a different directory tree, so this is non-issue here.
- **`sys.path` filter pattern at module top** — required to prevent `scripts/queue.py` from shadowing stdlib `queue`. `contact_sheet.py` already has this; no new code path adds the issue.
- **Pillow JPEG q=90** is the de-facto compression for ML datasets — embeds well in CLIP/SigLIP variants, ~4x smaller than PNG, no measurable quality loss at 640px.
- **POSIX `write()` ≤ PIPE_BUF (4KB) is atomic** for regular files even with concurrent writers. JSONL line-append fits comfortably under 4KB per line. Sweep-level flock further serializes any concurrent KB exports.

### External References

- Origin doc has full external context (LLaVA dataset conventions, LlamaIndex/Haystack manifest patterns, IPTC/XMP analogy). No new external research needed for this plan.

## Key Technical Decisions

- **Capture raw frames pre-cleanup**: `--kb-export` causes the worker to copy/transcode raw frame PNGs (in `frames/raw_NNNN.png` temp dir) into JPEG at the KB path BEFORE the existing cleanup loop deletes them. Avoids duplicating the extract pass. Leveraging existing in-flight artifacts is cheaper than re-extracting.
- **Clean sheet rendering = same `tile_sheets()` with mode flag**: introduce a `clean: bool` parameter to `tile_sheets()` that, when True, skips `label_frame()` composition and skips header rendering. Same tile math, same grid, same paste positions. Reuses existing function; minimal new code.
- **Per-movie JSON written before JSONL append**: order matters — if a crash happens between the two, manifest.jsonl has no orphaned line referencing a missing per-movie JSON. Write per-movie JSON first (atomically via write-then-rename), then append JSONL line. Matches "atomic per-movie append" requirement.
- **Idempotency check via per-movie JSON existence**: `<kb>/per-movie/<slug>.json` exists → skip KB export entirely (logs `kb-skip: <slug> already exported`). `--kb-force` overrides. Cheaper than checking each frame file.
- **JSONL append uses `open('a')` with single write call**: relies on POSIX small-write atomicity. Sweeper-level flock already prevents concurrent sweep runs, so concurrent JSONL writers are not a concern. No additional locking needed.
- **Default KB path is hard-coded to `~/claude-code/pirata/kb/`** in sheets_sweep.py: matches the central-KB choice from origin. User can override via direct `contact_sheet.py --kb-export <other>` invocation.
- **Idempotency check is per-movie, not per-frame**: simplifies bookkeeping. If user wants to re-extract a single frame, they delete the per-movie JSON and re-run.

## Open Questions

### Resolved During Planning

- **Q: Where in the contact_sheet.py pipeline does the KB export happen?**  
  A: After `tile_sheets()` produces labeled sheets (so labeled output stays first-class), but BEFORE the existing cleanup loop deletes raw frames. Insert between current `tile_sheets(...)` call and the `if not args.keep_raw: ... unlink ...` block. Reuses the in-memory `labeled` list and the on-disk `frames/raw_NNNN.png` files.

- **Q: How does `tile_sheets()` skip captions cleanly?**  
  A: Add a `clean: bool` parameter (default False). When True, the function pastes raw frames (from the `labeled` tuple's idx — re-open from `<frames_dir>/raw_NNNN.png`) instead of the labeled composite, and skips the header band entirely. This avoids duplicating the entire tiling logic.

- **Q: Order of writes for per-movie JSON + JSONL append?**  
  A: 1) Write frames as JPEG. 2) Write clean sheets as JPEG. 3) Write per-movie JSON via temp+rename for atomicity. 4) Append single line to global JSONL. Order is "heavy → light" so a crash leaves either nothing or all-but-JSONL (recoverable later via `--kb-rebuild-manifest`, deferred).

- **Q: How to construct manifest path-references? Absolute vs relative?**  
  A: Relative to `<kb>/` root in the JSONL. So `frames/who-framed-roger-rabbit-1988/...jpg`, not absolute. Makes the kb/ directory portable across machines (rsync, external drive, backup restore).

- **Q: How does sheets_sweep.py wire `--kb-export`?**  
  A: Hard-coded default: `~/claude-code/pirata/kb` (resolved). Add `--no-kb` flag to disable. Mirrors the `--autosheets` pattern in queue.py.

- **Q: How does DOCTOR detect KB drift?**  
  A: Existing `CONTRACT` check that greps `contact_sheet.py --help` adds `--kb-export` to its watched flag list. Same mechanism as before, one new entry.

### Deferred to Implementation

- **Exact JPEG encoder parameters**: Pillow's `quality=90, optimize=True, progressive=False` is a safe default but the implementer can tune (e.g., `subsampling=0` for max quality vs `subsampling=2` for max compression). Implementer chooses based on quick visual A/B.
- **Exact `clean=True` paste source**: easiest is re-open `raw_NNNN.png` from disk; alternative is keep a parallel "raw composite" list in memory. Pick based on memory pressure vs simplicity at implementation time.
- **Exact JSONL line shape**: schema is in origin doc, but field ordering / whitespace / newline-at-EOF conventions are implementation-time micro-decisions. Implementer picks; should match the origin schema's keys exactly.
- **Exact idempotency check timing**: do it at top of `main()` before scdet so we don't burn 530s on scene detection unnecessarily, OR at the start of the KB-export phase (after tile_sheets succeeds). Doing it early is faster for re-runs but means scdet cache stays fresh either way (it's already cached in `<labeled-out>/scenes_raw_t8.txt`). Implementer picks; recommended early.

## Output Structure

```
~/claude-code/pirata/kb/                                  # NEW: central KB root
├── frames/                                                # NEW: raw clean frames per movie
│   └── <slug>/
│       └── <slug>_frame_NNN.jpg                          # 300 per movie, 640px JPEG q=90
├── contact-sheets-clean/                                  # NEW: clean tiled sheets per movie
│   └── <slug>/
│       └── <slug>_sheet_NN.jpg                           # 10 per movie, no captions/header
├── per-movie/                                             # NEW: per-movie JSON manifests
│   └── <slug>.json                                        # 1 file per movie, ~5KB
└── manifest.jsonl                                         # NEW: global, append-only

scripts/
  contact_sheet.py                  [MODIFIED — add --kb-export, clean tile mode, manifest writers]
  sheets_sweep.py                   [MODIFIED — add --no-kb, default-pass --kb-export]
  tests/
    test_kb_export.sh               [NEW — smoke test for KB artifacts]

.claude/skills/pirata-deck/
  SKILL.md                          [MODIFIED — DOCTOR CONTRACT row adds --kb-export]
  references/menu-style.md          [MODIFIED — DOCTOR panel row for KB DIR (existence check)]
```

## Implementation Units

- [ ] **Unit 1: `contact_sheet.py` — add `--kb-export` flag + clean artifact emission**

**Goal:** When `--kb-export <dir>` is set, after the existing labeled pipeline completes, also emit clean frames (JPEG), clean sheets (no captions/header), per-movie JSON manifest, and append a global JSONL line — all under the kb root. Skip if per-movie JSON already exists unless `--kb-force` is set.

**Requirements:** R1, R2, R3, R4, R5, R6, R7, R9

**Dependencies:** none (pure extension of existing pipeline)

**Files:**
- Modify: `scripts/contact_sheet.py`

**Approach:**
- New argparse flags: `--kb-export <dir>` (Path, default None) and `--kb-force` (store_true, default False).
- Early idempotency check (after argparse, before scdet): if `args.kb_export` set AND `<kb>/per-movie/<slug>.json` exists AND `--kb-force` not set, log `kb-skip: <slug> already exported` and return early — but only if labeled output is also already complete (don't break the case where user wants labeled-only first, KB-export second). Pragmatic check: if labeled `<out>/<slug>_sheet_*.png` exist AND per-movie JSON exists → skip whole pipeline. If only per-movie JSON exists, proceed with labeled (existing logic) and skip only the KB-export phase.
- Extend `tile_sheets()` signature with `clean: bool = False` parameter. When True:
  - For each thumb: open `raw_NNNN.png` from frames_dir directly (skip `label_frame()` composite).
  - Skip header strip rendering; sheet height = `rows * tw` (no header_h).
  - Sheet output filename: same base name; saved as JPEG q=90 (vs PNG for labeled) into the kb/contact-sheets-clean/<slug>/ dir.
- After labeled `tile_sheets()` completes (existing flow), if `--kb-export` set:
  1. **Save clean frames**: iterate `results` (idx, t, raw_path) and copy/transcode each `raw_path` to `<kb>/frames/<slug>/<slug>_frame_<idx:03d>.jpg` via Pillow open + save quality=90.
  2. **Render clean sheets**: call `tile_sheets(labeled, cols, rows, <kb>/contact-sheets-clean/<slug>/, title, slug, header_font_size, clean=True)`. Re-uses tile math, skips overlays.
  3. **Write per-movie JSON**: build dict per origin schema (slug, title, year, fps, runtime_s, source_file path relative to repo or absolute, source_size_bytes from os.stat, scdet params, extracted_at ISO8601, frames list, sheets list). Write atomically via temp file + os.rename to `<kb>/per-movie/<slug>.json`.
  4. **Append JSONL line**: build minimal-key dict (slug, idx, file relative to kb/, tc, t_s, title, year) per frame, write all 300 lines as a single `'\n'.join(...) + '\n'` write to `<kb>/manifest.jsonl` in append mode (single `open('a')` + single `write()` call). Per-movie batch is well within POSIX small-write atomicity for our case.
  5. **Title/year extraction**: `--title` arg is already passed from sweeper as the human title (e.g., "Who Framed Roger Rabbit (1988)"). Parse year from trailing `(YYYY)` regex; if absent, set to None in manifest.
- The cleanup block (`if not args.keep_raw: ... unlink ...`) runs AFTER KB export finishes — raw frames are still on disk during step 1.
- Print summary at end: `[kb] exported <N> frames + <M> sheets to <kb_path>`.

**Patterns to follow:**
- Existing argparse structure in `contact_sheet.py main()` for flag definitions.
- Existing `tile_sheets()` for grid math (just extend with clean flag).
- Existing `slugify()` for filename derivation.
- `queue.py`'s `argparse.BooleanOptionalAction` pattern for `--kb-force` if BoolFlag style preferred (or `store_true` is fine — `--no-kb-force` not needed since v1).

**Test scenarios:**
- Happy path: `contact_sheet.py <mkv> --out <out> --title "Test (2024)" --kb-export <kb>` → labeled sheets in `<out>/`, clean frames in `<kb>/frames/test-2024/`, clean sheets in `<kb>/contact-sheets-clean/test-2024/`, per-movie JSON in `<kb>/per-movie/test-2024.json`, JSONL line appended to `<kb>/manifest.jsonl`.
- Happy path: re-run with `<kb>/per-movie/test-2024.json` already present → kb-skip log, no overwrite, JSONL not duplicated.
- Happy path: re-run with `--kb-force` → re-emits all KB artifacts, overwrites JSON, JSONL grows by 300 lines (acceptable; user invoked force; future `--kb-rebuild-manifest` would dedupe).
- Edge case: `--title "Title without year"` → year is None in manifest, slug remains `title-without-year`.
- Edge case: `--title "Filme com Acentos (2024)"` → unicode preserved in title field; slug normalized via existing slugify.
- Edge case: source file path contains spaces/brackets → JSON serializes correctly (Python json module handles).
- Edge case: kb dir doesn't exist → worker creates `<kb>/{frames,contact-sheets-clean,per-movie}/<slug>/` recursively.
- Error path: `<kb>` is read-only → log `[kb] export failed: permission denied`, continue (don't fail labeled output).
- Error path: per-movie JSON write fails (disk full) → log error, leave any partially-written frames in place (cleanup is user's job; idempotency check on re-run will see no JSON and retry).
- Error path: JSONL append fails → log warning, per-movie JSON still exists, manifest.jsonl is now slightly inconsistent (recovery deferred to v2 utility).
- Integration: full pipeline with `--kb-export` and labeled `--out` set to different paths → both produce their outputs, no cross-contamination, both succeed independently.
- Integration: clean sheet content has zero burned-in text (verifiable via OCR check or pixel-diff vs labeled equivalent showing ~5-7MB caption-strip-shaped diff region).
- Integration: frame file `<slug>_frame_001.jpg` has the same visual content as labeled `<slug>_sheet_01.png`'s top-left thumb but no caption strip below.

**Verification:**
- Unit 1 is complete when: (a) running contact_sheet.py with `--kb-export <kb>` produces all 4 KB artifact classes (frames JPEG, clean sheets JPEG, per-movie JSON, JSONL line per frame); (b) re-running without `--kb-force` is a no-op for KB (labeled may or may not regenerate per existing semantics); (c) re-running with `--kb-force` re-emits everything; (d) labeled `--out` artifacts are byte-identical to a run without `--kb-export` (proves no regression in human-facing output); (e) clean sheet thumbs visually match labeled thumbs minus caption strip + header (manual A/B); (f) per-movie JSON validates against the origin schema (jq round-trip succeeds, all expected keys present); (g) manifest.jsonl is line-delimited, each line is valid JSON, line count after run = prior count + 300.

---

- [ ] **Unit 2: `sheets_sweep.py` — wire `--kb-export` default + `--no-kb` opt-out**

**Goal:** Sweep passes `--kb-export ~/claude-code/pirata/kb/` to `contact_sheet.py` by default. Adds `--no-kb` flag to disable.

**Requirements:** R8 (opt-out)

**Dependencies:** Unit 1 (sweep invokes the new flag)

**Files:**
- Modify: `scripts/sheets_sweep.py`

**Approach:**
- New argparse flag: `--kb` / `--no-kb` via `argparse.BooleanOptionalAction` (default True).
- Resolve KB path: `KB_ROOT = REPO_ROOT / "kb"` constant at module top, OR `Path.home() / "claude-code" / "pirata" / "kb"` if user wants central (matches origin's "Central" choice). Use `REPO_ROOT / "kb"` since pirata IS already at `~/claude-code/pirata/`, so they're equivalent on the user's machine. Future-proof: hardcoding to repo-relative keeps the path portable if user clones the repo elsewhere.
- In `run_contact_sheet()`, when `--kb` is on, append `--kb-export <KB_ROOT>` to argv (BEFORE the `--` terminator).
- No other behavior changes; existing `--kb-force` semantics flow through `contact_sheet.py` if user wants (sweep doesn't currently expose force, deferred — user can always invoke contact_sheet.py directly).
- Log line on sweep start: include `kb=<on|off>` in the existing `log("start", ...)` detail.

**Patterns to follow:**
- `queue.py` `argparse.BooleanOptionalAction` for `--autosheets`.
- Existing `run_contact_sheet()` argv construction.

**Test scenarios:**
- Happy path: `sheets_sweep.py` (default) → contact_sheet.py invocation includes `--kb-export <kb_root>` in argv.
- Happy path: `sheets_sweep.py --no-kb` → contact_sheet.py argv excludes `--kb-export`.
- Edge case: `--kb` and `--no-kb` mutual exclusion via `BooleanOptionalAction` is automatic; user passing both is rejected by argparse.
- Integration: full sweep with default flags against fixture release → KB artifacts appear in `<kb>/` after run.
- Integration: full sweep with `--no-kb` → `<kb>/` unchanged from prior state (no new files for the swept release).

**Verification:**
- Unit 2 is complete when: (a) default sweep produces KB artifacts; (b) `--no-kb` produces no KB artifacts; (c) all existing sweep behavior (flock, --skip, --dry-run, --force) is preserved; (d) the start-of-sweep log line reflects the `kb=<on|off>` state.

---

- [ ] **Unit 3: Skill DOCTOR + STATUS panel updates**

**Goal:** `/pirata 10` (DOCTOR) verifies KB dir exists and the new `--kb-export` flag is in `contact_sheet.py --help`. `/pirata 9` (STATUS) optionally surfaces KB size for visibility.

**Requirements:** R7 (visibility into KB state)

**Dependencies:** Units 1, 2

**Files:**
- Modify: `.claude/skills/pirata-deck/SKILL.md`
- Modify: `.claude/skills/pirata-deck/references/menu-style.md`

**Approach:**

SKILL.md Workflow 9 (STATUS) — add 1 row:
- **KB SIZE**: count of `~/claude-code/pirata/kb/per-movie/*.json` files = movies in KB; total disk via `du -sh ~/claude-code/pirata/kb` for size badge. Format: `<N> movies · <size>` (e.g., `12 movies · 850MB`).

SKILL.md Workflow 10 (DOCTOR) — extend existing CONTRACT row to also grep for `--kb-export` in `contact_sheet.py --help`. Add 1 new row:
- **KB DIR**: `~/claude-code/pirata/kb/` exists and is writable → `[OK]`/`[FAIL]`.

menu-style.md — add panel rows within existing 55-char grid:

```
STATUS additions (existing grid, after SHEETED row):
│ KB SIZE    │ 12 movies · 850MB                      │

DOCTOR additions:
│ KB DIR     │ ~/claude-code/pirata/kb/ writable [OK] │
```

The CONTRACT row text doesn't need to change visibly — the underlying check just adds `--kb-export` to its grep list.

**Patterns to follow:**
- Existing TR-100 panel templates; 55-char fixed width; 12-char label col, 40-char data col, `[STATE]` badges right-aligned.

**Test scenarios:**

*Test expectation: none — this unit updates skill prose/templates, not behavioral code. Verification is visual via `/pirata 9` and `/pirata 10` after Units 1-2 ship and a sweep has run.*

**Verification:**
- Unit 3 is complete when: (a) `/pirata 9` STATUS renders KB SIZE row showing correct movie count + size on a workspace with at least one swept movie; (b) `/pirata 10` DOCTOR shows `[OK]` on KB DIR row when `~/claude-code/pirata/kb/` is writable, `[FAIL]` when missing/read-only; (c) DOCTOR CONTRACT check `[FAIL]`s when `--kb-export` is removed from `contact_sheet.py` (simulated drift); (d) all panel rows remain 55 chars wide.

---

- [ ] **Unit 4: Smoke test `test_kb_export.sh`**

**Goal:** Hermetic test asserting all 4 KB artifact classes appear correctly for a fixture run, including idempotency, force, opt-out, and basic schema validation.

**Requirements:** R5, R6, R7, R8 (operational invariants)

**Dependencies:** Units 1, 2

**Files:**
- Create: `scripts/tests/test_kb_export.sh`

**Approach:**
- Hermetic fixture: `ffmpeg -f lavfi -i 'mandelbrot=duration=15:size=640x360:rate=24'` to create a small mkv with sufficient visual variation that scdet finds ≥10 scenes (more than testsrc, which is too uniform).
- Test fixtures live in `<TMPDIR>/pirata-kb-test-XXXXXX/`. Set `AUTOSHEETS_MIN_SIZE_MB=1` to bypass production size floor.
- Run `contact_sheet.py` directly with `--kb-export <fixture_kb>` and `--threshold` low enough (e.g., 1) that the small fixture produces frames.
- Tests:
  1. **Happy path**: KB artifacts present — `<kb>/frames/<slug>/<slug>_frame_*.jpg` count > 0; `<kb>/contact-sheets-clean/<slug>/<slug>_sheet_*.jpg` count ≥ 1; `<kb>/per-movie/<slug>.json` valid JSON; `<kb>/manifest.jsonl` line count = frame count.
  2. **Per-movie JSON schema**: top-level keys (slug, title, year, fps, runtime_s, source_file, scdet, extracted_at, frames, sheets) all present via `jq -e`. `frames[]` array length matches `<kb>/frames/<slug>/` file count.
  3. **JSONL validity**: each line is valid JSON via `while read line; do echo "$line" | jq -e . > /dev/null; done < manifest.jsonl`.
  4. **Idempotency**: re-run without `--kb-force` → no change to file mtimes; manifest.jsonl line count unchanged.
  5. **Force**: re-run with `--kb-force` → file mtimes update; JSONL line count grows (current v1 behavior; future `--kb-rebuild-manifest` deferred).
  6. **Sweeper opt-out**: invoke `sheets_sweep.py --downloads <fake_root> --no-kb --dry-run` → no KB artifact creation (since dry-run anyway, but verify --no-kb is plumbed through).
  7. **Clean sheet has no caption strip**: check pixel content of bottom 50px of a clean sheet — should be padding-color (sheet bg ~10,10,10), not the labeled `(0,0,0,200)` caption box. Approximation: read the bottom-strip region via Python Pillow and assert mean luminance is below threshold X (= dark sheet bg) without the caption-strip step's white text contributing.
  8. **Frame has no overlay**: open `<kb>/frames/<slug>/<slug>_frame_001.jpg` via Pillow, assert no `(255,255,255)` pixels in bottom-left corner where caption would have been (allowing for natural bright video content elsewhere).
  9. **Argparse flag injection still defended**: existing test_sweep.sh's T5 covered this; reuse the canary.
  10. **Title with parentheses preserves year**: `--title "Test Movie (2024)"` → JSON `year=2024`, slug `test-movie-2024`.
- Teardown: `rm -rf` fixture tmpdir; restore `<kb>/manifest.jsonl` to pre-test state if test polluted it (write fixture KB to a separate root, don't touch real `~/claude-code/pirata/kb/`).

**Patterns to follow:**
- Existing `scripts/tests/test_sweep.sh` for bash structure (`set -euo pipefail`, assert helper, PASS/FAIL counter, trap cleanup).
- Use a separate KB root under `$TMPDIR` so the test never writes to the user's real KB.

**Test scenarios:**

*This unit IS the test suite. Its assertions ARE the scenarios above.*

**Verification:**
- Unit 4 is complete when: (a) `bash scripts/tests/test_kb_export.sh` exits 0 with all assertions passing; (b) test is hermetic — no writes to `~/claude-code/pirata/kb/`, no leftover files in workspace after teardown; (c) test runs in <60s on the user's hardware (Pillow + ffmpeg costs are the floor; should be fast given small fixture).

## System-Wide Impact

- **Interaction graph:** Sweep → contact_sheet.py (NEW arg) → emits artifacts to two roots (existing labeled `<out>/`, new clean `<kb>/`). `/pirata 9` STATUS reads `<kb>/per-movie/*.json` count and `du -sh <kb>`. `/pirata 10` DOCTOR checks `<kb>/` writability and `--kb-export` flag presence in `contact_sheet.py --help`.
- **Error propagation:** KB export errors are isolated — labeled output is written FIRST and unaffected. KB export failures log warnings/errors but do not fail the labeled run. Per-movie JSON write uses temp+rename for atomicity. JSONL append uses POSIX small-write atomicity (sweep flock further serializes concurrent runs).
- **State lifecycle risks:**
  - `<kb>/manifest.jsonl` grows unbounded (rotation deferred to v2).
  - Orphaned KB frames after release deletion (deferred to `--kb-prune` v2).
  - `--kb-force` re-runs duplicate JSONL entries (deferred to `--kb-rebuild-manifest` v2). Acceptable v1 — duplicates are detectable downstream by `slug + idx` key.
- **API surface parity:**
  - `contact_sheet.py` adds 2 flags (`--kb-export`, `--kb-force`); existing flags unchanged.
  - `sheets_sweep.py` adds `--kb`/`--no-kb`; existing flags unchanged.
  - `queue.py` unchanged (it doesn't directly invoke contact_sheet.py; the flow is queue → sweep → contact_sheet, so KB export reaches users via sweep).
- **Integration coverage:**
  - Unit 4 smoke test covers contact_sheet.py + sweep flag plumbing end-to-end.
  - Existing test_sweep.sh assertions still apply (filter, flock, security defenses) — no regression expected since KB code is purely additive.
- **Unchanged invariants:**
  - Labeled `<release>/contact-sheets/` output is byte-identical to a non-KB run (verified by Unit 1 test (d)).
  - `contact_sheet.py` exit codes unchanged on success (still 0).
  - No new dependencies — uses existing Pillow + stdlib (json, pathlib, shutil, datetime).

## Risks & Dependencies

| Risk | Severity | Mitigation |
|---|---|---|
| Disk grows fast: ~75MB labeled + ~30MB KB per movie = ~105MB/movie. Steady downloads × ~100 movies = ~10GB | MEDIUM | JPEG q=90 already minimizes KB; user disk monitored via `/pirata 10`. Future `--kb-prune` deferred. |
| `--kb-force` rerun duplicates JSONL entries | LOW | Documented v1 behavior; manifest is append-only, dupe-detectable by `slug + idx` key downstream. Future `--kb-rebuild-manifest` reconciles. |
| Manifest JSONL corruption (mid-write crash) | LOW | POSIX single-write atomicity for ≤4KB; sweeper-level flock serializes concurrent runs. Per-movie batch writes are typically <16KB but well under pipe-buf even for large batches; OS handles. |
| Per-movie JSON write fails mid-run (disk full) | LOW | Temp+rename: either old file remains untouched (rename atomic) or new file replaces fully. Partial frames remain on disk (orphaned but harmless; idempotency re-tries on next run). |
| `tile_sheets()` `clean=True` mode bug introduces visual regression in labeled path | LOW | Default `clean=False` preserves existing behavior. Clean path is opt-in. Unit 1 test (d) explicitly verifies labeled output is byte-identical. |
| User's `~/.config/pirata/config.toml` doesn't define `download_dir` and KB defaults silently to a wrong location | LOW | KB path is hard-coded to repo's `<repo>/kb/` (Path(__file__).parent.parent), independent of pirata config. No config dependency for KB path. |
| `<kb>/per-movie/<slug>.json` exists but `<kb>/frames/<slug>/` is empty/corrupt (partial-state) | LOW | Idempotency check trusts JSON presence as "done" signal. If user wants to repair, delete the per-movie JSON and re-run. Documented in skill help. |
| Slug collision across rips (e.g., 1080p + 2160p of same movie) | LOW | Documented v1 limitation. Second run overwrites; manifest's `source_file` distinguishes. No data loss; just last-rip-wins. |
| JPEG q=90 introduces compression artifacts that degrade RAG retrieval quality | LOW | q=90 is industry-standard for ML datasets; CLIP/SigLIP/Moondream are robust. If retrieval quality issues surface, bump to q=95 (1 line change) or switch to PNG (would 4x disk cost). |
| Future refactor of `tile_sheets()` breaks the clean=True branch silently | MEDIUM | Unit 4 smoke test asserts clean sheet has no caption strip pixel content. Catches the regression class directly. |

## Documentation / Operational Notes

- Update `.claude/skills/pirata-deck/SKILL.md` Helper Scripts section to mention `--kb-export` and the kb/ structure.
- Add one-line note to skill help/FAQ explaining: "downloads/X/contact-sheets/ has labeled sheets for human review; ~/claude-code/pirata/kb/ has clean RAG-ready frames + manifests for embedding pipelines."
- No external docs / runbooks affected.
- Operational caveat for users: deleting a release dir does NOT delete its KB entries. KB lifecycle is separate. Future `--kb-prune` will reconcile (deferred).
- `manifest.jsonl` is the durable index; per-movie JSONs are the human-readable detail. Both are written; redundancy is intentional and cheap (~5KB extra/movie).

## Sources & References

- **Origin document:** [docs/brainstorms/2026-04-24-kb-rag-multimodal-frames-requirements.md](docs/brainstorms/2026-04-24-kb-rag-multimodal-frames-requirements.md)
- Related code:
  - `scripts/contact_sheet.py` (the file being extended)
  - `scripts/sheets_sweep.py` (the wrapper to update)
  - `scripts/queue.py` (BooleanOptionalAction pattern reference)
  - `.claude/skills/pirata-deck/SKILL.md` and `references/menu-style.md` (DOCTOR/STATUS panel surfaces)
- External docs: see origin (LLaVA conventions, LlamaIndex/Haystack JSONL patterns, IPTC/XMP cross-domain analogy). No new external research for this plan.
- **Approved reference output:** `~/claude-code/pirata/downloads/Who Framed Roger Rabbit (1988) [...]/contact-sheets/who-framed-roger-rabbit-1988_sheet_{01..10}.png` (labeled, exists). Equivalent KB-side after rollout: `~/claude-code/pirata/kb/contact-sheets-clean/who-framed-roger-rabbit-1988/who-framed-roger-rabbit-1988_sheet_{01..10}.jpg` + 300 frame JPEGs + per-movie JSON + JSONL line.
