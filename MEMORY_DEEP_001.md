# Memory Deep #001

| Field       | Value                                              |
|-------------|----------------------------------------------------|
| Created     | 2026-04-24 19:47 BRT                               |
| Project     | pirata — personal media download + contact-sheet workspace |
| Session     | Built end-to-end pipeline: torrent search/queue → contact sheet generation → opportunistic sweeper post-download → RAG-multimodal KB export. 8 commits to main. |
| Previous    | none                                               |

---

## Project Context

`pirata` is Vidigal's personal Mac-based media workspace at `~/claude-code/pirata`. Two muscles: (a) a `torrentclaw` MCP for rich movie/TV search with metadata, and (b) a Rust `pirata` CLI scraper for non-TC sources (anime, music, software). Downloads go through `aria2c` orchestrated by `scripts/queue.py`. The session built a cinema-grade contact-sheet pipeline on top of that, plus opportunistic automation and RAG-ready knowledge-base export.

## What Happened This Session

This was a long session in three acts: (1) build the manual contact-sheet pipeline, (2) automate it post-download via opportunistic sweeper, (3) emit a parallel RAG-ready KB dataset.

### Act 1: contact_sheet.py from scratch + iteration loop

**Started** with the `/pirata` skill rendering its TR-100 menu. Initial spec had ANSI 24-bit toxic-green escapes around the figlet hero rows. **Failed** — the Claude Code code fence does NOT interpret `\x1b[...]m` escapes; they leaked as literal text, breaking the visual. Pivoted skill to monochrome (matches `/annas` style). Updated `.claude/skills/pirata-deck/SKILL.md` and `references/menu-style.md` to remove all color policy. Saved feedback memory at `~/.claude/projects/-Users-vidigal-claude-code-pirata/memory/feedback_ansi_in_code_fence.md` + `MEMORY.md` index.

**Then**: searched for "Who Framed Roger Rabbit" via the `torrentclaw` MCP. Got 23 torrents in 2 dedup'd entries. Picked the YTS 2160p BluRay x265 4.7GB rip (56 seeders, score 85). Enqueued via `scripts/queue.py` → aria2c PID 75784 → completed.

**Brainstormed** ffmpeg-based contact-sheet strategies. Settled on **Plan B Editorial**: ffprobe scdet for real scene timestamps, min-4s floor, target 300 frames, 10×10 grid, Pillow labels.

**Built** `scripts/contact_sheet.py` from scratch. Pipeline: ffprobe scdet (scaled-down) → cache timestamps → floor + cap → ffmpeg per-frame seek (parallel) → Pillow label + tile.

**Hit four bugs in succession** during iteration:

1. ffprobe CSV output emits `,` per non-scene frame → my parser bombed splitting on whitespace. Fixed: skip empty/comma-only lines.
2. `multiprocessing.Pool` failed with `ImportError: cannot import name 'Empty' from 'queue'` because `scripts/queue.py` shadows stdlib `queue` when Python adds `scripts/` to `sys.path`. Switched to `concurrent.futures.ThreadPoolExecutor`.
3. ThreadPoolExecutor ALSO imports queue lazily → same shadowing. Added explicit `sys.path[:] = [p for p in sys.path if os.path.abspath(p) != os.path.dirname(os.path.abspath(__file__))]` at the top of `contact_sheet.py`. Same fix later applied to `sheets_sweep.py`.
4. Lost the 530s scdet output on first failure. Added scdet result caching to `<out>/scenes_raw_t<N>.txt` to avoid re-running. Re-runs trigger only frame extract + tile (~2min).

**First successful run**: 10 sheets, 6×5 @ 640px, ANSI-overlay caption + index. Vidigal asked for 10 sheets instead of 3 → reconfigured. Then asked for `001` top-right + `00:07:18:06` bottom-left split → split badges. Then said the diagonal layout was confusing → redesigned to **caption strip below each thumb** (no overlay on frame). Final format: `N 001  ·  TC 00:07:18:06`. Slug-prefixed filenames: `who-framed-roger-rabbit-1988_sheet_{01..10}.png`.

**LLM-readability fix**: bumped fonts to be proportional to thumb width (`max(22, width // 20)` for caption, `max(24, width // 14)` for header). Reasoning: vision LLMs (Claude/GPT-4V/Gemini) downsample to ~1568px; original 14pt → ~6px after scale, sub-OCR threshold. New 32pt → ~13px post-resize, readable. Auto-detected fps via `probe_fps()` (24000/1001 → 23.976 for Roger Rabbit).

### Act 2: auto-sheet pipeline post-download

**Brainstormed** automation. Considered aria2c `--on-download-complete` hook vs sweeper vs daemon. ce-plan produced a 5-unit hook plan (bash shim → Python worker with flock → integration with queue.py + skill panels + smoke test). Pivoted to **deepening pass** which surfaced 25 findings from 3 specialists (pattern-recognition, architecture-strategist, security-sentinel), all accepted and integrated.

Then ran **ce-doc-review** (6 personas) on the deepened plan. The reviews were brutal but accurate:

- **Adversarial #5 (HIGH)**: pirata's Rust CLI bypasses `queue.py` entirely; aria2c-hook architecture has a 50% blind spot.
- **Adversarial #8 (HIGH)**: `contact_sheet.py` writes `scenes_raw_t8.txt` cache into output dir → "only PNG" collision heuristic broken on second run.
- **Feasibility F1 (HIGH)**: macOS bash 3.2 doesn't support `${VAR,,}` lowercase expansion.
- **Feasibility F2 (HIGH)**: `is_relative_to(downloads_root)` requires `downloads_root.resolve()` too.
- **Product-lens #1, #7**: a sweeper alternative captures 80% of value at 20% of complexity.

User chose to **pivot architecture** to the sweeper. Re-wrote `docs/plans/2026-04-24-002-feat-auto-contact-sheets-plan.md` from scratch (event-driven → opportunistic).

**Sweeper architecture executed in 4 commits**:
- `b56ec67` — `scripts/sheets_sweep.py`: walks `downloads/`, finds release dirs without `contact-sheets/*_sheet_*.png`, invokes `contact_sheet.py` serially. fcntl.flock LOCK_NB on `logs/.sheets_sweep.lock`. resolve+is_relative_to security gate. argparse `--` terminator. repr() log sanitization. Output dir is `contact-sheets/` (not `contact/` — torrent payloads sometimes ship `contact/` subdirs; would break re-seeding).
- `deb6f21` — `queue.py --autosheets/--no-autosheets` (default on). When `--wait` + autosheets → runs sweep synchronously after aria2c completes.
- `f9e5013` — `/pirata 9` STATUS adds LAST SWEEP + SHEETED rows; `/pirata 10` DOCTOR adds SWEEP + DL DIR + CONTRACT rows (greps `contact_sheet.py --help` for required flags).
- `e00c1bb` — `scripts/tests/test_sweep.sh` 12 hermetic assertions including malicious-fixture coverage. Disk check skip on dry-run.

### Act 3: KB RAG-multimodal export

User asked for a parallel RAG-multimodal-ready dataset: clean frames (no overlay), clean contact sheets (no captions), naming `<slug>_frame_NNN`, central kb dir, manifest format.

**Brainstormed** in 4 axes (location, format, naming, manifest, clean-sheet layout). All decisions resolved via 4 AskUserQuestion rounds:
- Location: **Central** `~/claude-code/pirata/kb/`
- Format: **JPEG q=90** (4x smaller than PNG, embedding quality indistinguishable)
- Manifest: **Both** per-movie JSON + global JSONL
- Clean sheets: **Same 10×(6×5) as labeled, minus captions+header**

Wrote `docs/brainstorms/2026-04-24-kb-rag-multimodal-frames-requirements.md` then `docs/plans/2026-04-24-003-feat-kb-rag-multimodal-frames-plan.md`.

**Executed in 4 commits**:
- `8a1ffee` — `contact_sheet.py --kb-export <dir>` + `--kb-force`. Extended `tile_sheets()` with `clean: bool` mode (skip header band, skip label_frame, save as JPEG). Added `export_kb()` orchestrator: writes 300 clean frames + 10 clean sheets + per-movie JSON via temp+rename + appends 300 lines to global JSONL. Idempotency check: `<kb>/per-movie/<slug>.json` existence. New helpers: `probe_duration()`, `parse_year_from_title()`. KB phase runs BEFORE existing raw-frame cleanup — no extra extract pass.
- `1e38677` — `sheets_sweep.py --kb/--no-kb` BooleanOptionalAction (default on). Resolves `kb_root = REPO_ROOT/kb`, passes to `run_contact_sheet()` which appends `--kb-export <kb>` to argv (before `--`). Start log line shows `kb=on/off`.
- `3d5c334` — `/pirata 9` STATUS adds KB SIZE row (count of `kb/per-movie/*.json` + `du -sh`). `/pirata 10` DOCTOR adds KB DIR + extends CONTRACT grep to include `--kb-export`.
- `c4cb468` — `scripts/tests/test_kb_export.sh` 18 hermetic assertions. Fixture: 7-color concat lavfi (real scene cuts at boundaries, no real video needed).

**Smoke test fixture choice was deliberate**: tried mandelbrot (`duration` option doesn't exist on mandelbrot, used `-t`), tried still mandelbrot with threshold 1 (zero scenes — too uniform), settled on concat of 7 colored 4-second blocks. ffmpeg `concat=n=7:v=1:a=0[v]` + `-map "[v]"`. scdet finds 6 hard cuts at color boundaries. Total fixture runtime ~5s on M-series.

**Test scenarios shifted during smoke-test build**: T8 originally checked stddev of bottom 8px of clean sheet (false positive on uniform-color fixture — stddev 0 looks like caption band). Replaced with height-comparison: clean sheet height < labeled sheet height by ≥40px (proves header band absent). 18/18 PASS.

## Decisions Made

- **Decision:** Skill output is monochrome (no ANSI escapes inside code fences) — **Why:** Claude Code code fence doesn't interpret `\x1b[...]m`; escapes leak as literal text. Verified empirically on first attempt at toxic-green figlet.
- **Decision:** sys.path filter at module top of every Python script in `scripts/` — **Why:** `scripts/queue.py` shadows stdlib `queue`; `concurrent.futures.thread`, `multiprocessing.queues` import lazily. Filter runs before any import that touches queue.
- **Decision:** scdet results cached to `<out>/scenes_raw_t<N>.txt` per-threshold — **Why:** scdet on 104min 2160p x265 takes ~9min. Re-runs would be punishing. Cache is keyed by threshold so different settings don't collide.
- **Decision:** ThreadPoolExecutor over multiprocessing — **Why:** subprocess-bound ffmpeg releases the GIL during decode; threads avoid pickle + module-shadowing issues; simpler shutdown.
- **Decision:** Caption strip BELOW each thumb (not overlay on frame) — **Why:** user feedback that diagonal `001` top-right + TC bottom-left was visually confusing. Caption strip groups all metadata in one place outside the frame; doesn't pollute the visual.
- **Decision:** Output dir `contact-sheets/` (not `contact/`) — **Why:** torrent payloads frequently ship `contact/` subdirs (scene-release info). Writing into `contact/` co-mingles with torrent files, breaks re-seeding. `contact-sheets/` is not a known scene convention.
- **Decision:** Sweeper architecture, NOT aria2c hook — **Why:** Adversarial review found Rust pirata CLI bypasses `queue.py`; hook architecture had 50% blind spot. Sweeper is path-agnostic — only cares about filesystem state.
- **Decision:** flock at sweep level (cap=1) instead of per-worker — **Why:** scdet is single-threaded CPU-bound; concurrent runs are slower (cache contention + thermal). Serial keeps machine cool, throughput equal-or-better.
- **Decision:** "Sheeted" detection = `contact-sheets/` exists AND contains ≥1 `*_sheet_*.png` — **Why:** Adversarial #8 catch: `contact_sheet.py` leaves `scenes_raw_t8.txt` cache file in the dir; "only PNG" check would break on second run. Checking for `*_sheet_*.png` ignores cache and partial-failure dirs.
- **Decision:** KB output dir is central `~/claude-code/pirata/kb/`, not co-located in release — **Why:** RAG crawlers want one root path; KB survives release deletion; clean separation between human review (release/contact-sheets/) and machine ingest (kb/).
- **Decision:** JPEG q=90 for KB frames — **Why:** ~80KB vs PNG ~300KB. CLIP/SigLIP/Moondream embeddings indistinguishable. User disk at 92% used; 100 movies = 2.4GB JPEG vs 9GB PNG.
- **Decision:** Both per-movie JSON + global JSONL manifests — **Why:** Per-movie human-inspectable; JSONL standard for streaming RAG ingest (LlamaIndex/Haystack). Cost is ~5KB extra per movie.
- **Decision:** KB export idempotency keyed by per-movie JSON existence — **Why:** Cheap check; skipping early avoids the 530s scdet on re-runs. `--kb-force` overrides.
- **Decision:** `--` terminator in subprocess argv — **Why:** Security review found that filenames like `--evil.mkv` (legal on macOS) would be parsed as argparse flags, causing DoS or worse. `--` terminator forces positional interpretation.
- **Decision:** `repr()` on filename-derived log content — **Why:** Filenames with `\n`/ANSI escapes/control chars would inject fake log entries or hijack `tail -f` terminal rendering. `repr()` escapes them visibly.
- **Decision:** `Path.resolve(strict=True)` + `is_relative_to(downloads_root.resolve())` — **Why:** Symlink path-traversal defense. Both sides MUST resolve (Feasibility F2 — `/tmp → /private/tmp` on macOS would silently reject everything).
- **Decision:** `subprocess.Popen(start_new_session=True)` + SIGTERM/SIGINT → killpg — **Why:** Kill propagates to ffmpeg subprocess tree. Without it, kill -9 on worker leaves orphan ffmpeg holding 6 CPU threads + 2GB RAM, defeating cap=1.
- **Decision:** Skip disk-free check in `--dry-run` mode — **Why:** dry-run writes nothing; gating it on disk state is a false-negative for "would this work". Real run still gates.

## Current State

**Working end-to-end**: 
- `queue.py --wait <magnet>` → aria2c → sweep → contact sheets (labeled in release/contact-sheets/) + KB export (clean frames + sheets + manifests in `~/claude-code/pirata/kb/`).
- `python3 scripts/sheets_sweep.py` (manual) is path-agnostic — works for aria2c, Rust CLI, manual drops, AirDrop, Drive sync.
- `python3 scripts/sheets_sweep.py --no-kb` skips KB export.
- `python3 scripts/contact_sheet.py <mkv> --out <dir> --kb-export <kb>` direct invocation also works.
- Two smoke tests run hermetically: `scripts/tests/test_sweep.sh` (12/12) and `scripts/tests/test_kb_export.sh` (18/18).

**Known state**:
- One existing release in `downloads/` (Roger Rabbit) has its sheets in `contact/` (pre-sweeper-pivot path), not `contact-sheets/`. Sweep would regenerate to new path on next real run; user can also `mv contact contact-sheets`.
- User disk at **92% used** — real-run sweep gates at <10% free, will skip with warning. Need to free disk before processing many movies.
- KB dir not yet created on disk (only created on first actual KB export).

**Not started**:
- Moondream caption pass per frame (manifest schema reserves `caption: null`).
- knowledge-hub MCP auto-ingest hook.
- IPTC/XMP embedded metadata via exiftool.
- `--kb-prune` cleanup utility.
- launchd schedule for periodic auto-sweep.

## Done (Cumulative)

- [x] `/pirata` skill spec'd (TR-100 monochrome, 12-branch menu)
- [x] Memory feedback saved: ANSI escapes don't render in Claude code fences
- [x] `scripts/queue.py` (existed pre-session; now tracked in git) — aria2c wrapper
- [x] `scripts/contact_sheet.py` — full pipeline (scdet → extract → label → tile)
- [x] Caption strip below thumb design (vs initial diagonal badges)
- [x] LLM-readable label fonts (auto-scaled with thumb width)
- [x] fps auto-detection via `probe_fps()`
- [x] Slug-prefixed sheet filenames (e.g., `who-framed-roger-rabbit-1988_sheet_NN.png`)
- [x] scdet result caching for fast re-runs
- [x] `scripts/sheets_sweep.py` — opportunistic sweeper, path-agnostic
- [x] sweep-level flock + security defenses (resolve+is_relative_to, --terminator, repr-sanitize, killpg)
- [x] `scripts/queue.py` `--autosheets`/`--no-autosheets` integration
- [x] `/pirata` skill panel rows: STATUS (LAST SWEEP, SHEETED, KB SIZE), DOCTOR (SWEEP, DL DIR, CONTRACT, KB DIR)
- [x] `scripts/tests/test_sweep.sh` — 12 assertions
- [x] `contact_sheet.py --kb-export` + `--kb-force` flags
- [x] `tile_sheets()` `clean=True` mode (no header, no caption strip, JPEG output)
- [x] `export_kb()` — frames JPEG + clean sheets JPEG + per-movie JSON + JSONL append
- [x] `sheets_sweep.py --kb`/`--no-kb` integration
- [x] `scripts/tests/test_kb_export.sh` — 18 assertions
- [x] Comprehensive docs: 1 brainstorm + 3 plans in `docs/`
- [x] 8 atomic git commits on main with conventional messages

## Pending (By Priority)

### P1 — Urgent / Blocking

- [ ] Free disk space — sweep won't run real (10%-free guard) until below 90% usage. Currently 92%.
- [ ] (Optional) Migrate Roger Rabbit's existing `contact/` to `contact-sheets/` if user wants new path consistency. `mv` is safe.

### P2 — Important

- [ ] First real-world KB export run on a fresh download. Validates the production path end-to-end (currently only validated against synthetic fixture).
- [ ] Decide on RAG ingestion target (LlamaIndex / Haystack / knowledge-hub MCP / custom embedder). Format is agnostic, but downstream tooling is open.
- [ ] Consider Moondream caption pass per frame as opt-in flag (`--kb-caption`). Schema reserves `caption: null`. Adds ~10min/movie. User has Moondream skill + Apple Silicon hardware.

### P3 — Nice to Have

- [ ] `scripts/sheets_sweep.py --kb-prune` utility for orphaned KB entries after release deletion.
- [ ] `--kb-rebuild-manifest` utility to regenerate `manifest.jsonl` from per-movie JSONs (handy after `--kb-force` re-runs duplicate JSONL lines).
- [ ] launchd `.plist` for periodic auto-sweep (deferred — manual + queue.py-wait integration covers most cases).
- [ ] IPTC/XMP metadata embedding via exiftool for travels-with-file metadata.
- [ ] Mega-sheet (single 300-thumb image) as "movie fingerprint" for similar-film retrieval.
- [ ] `--kb-export` flag in `queue.py` to expose KB controls higher up the stack (today only via `sheets_sweep.py --kb`).
- [ ] /pirata skill UPDATE flow if user wants to add RAG-query workflow on top of KB.
- [ ] Cross-rip dedup (1080p vs 2160p of same movie collide on slug; manifest's `source_file` is the tiebreaker but no automatic resolution).

## Technical Notes

**Stack**:
- Python 3.14.4 via pyenv
- ffmpeg-full 8.1 at `/opt/homebrew/opt/ffmpeg-full/bin/{ffmpeg,ffprobe}` (NOT default PATH)
- Pillow 10.4.0
- aria2c via Homebrew
- macOS Darwin 24.6.0, Apple Silicon, /bin/bash is 3.2.57 (frozen)
- 16 CPUs, ~36GB RAM (estimated from `sysctl hw.ncpu`)

**Config**:
- `~/.config/pirata/config.toml` — aria2.download_dir = `/Users/vidigal/claude-code/pirata/downloads`
- No env vars currently required (all hard-coded constants in scripts)
- `AUTOSHEETS_MIN_SIZE_MB` env override exists for tests (default 300MB)

**Key sizes** (per movie, post-pipeline):
- Labeled output (release/contact-sheets/): ~75MB (10 PNG sheets w/ captions)
- KB output: ~30MB (300 JPEG frames @ ~80KB + 10 clean JPEG sheets @ ~5MB + ~5KB JSON + ~50KB JSONL append)
- contact_sheet.py cache: ~10KB (`scenes_raw_t8.txt`)
- Total per movie: ~105MB

**Pipeline timing** (104min x265 2160p source):
- scdet: ~530s first run, 0s cached
- Frame extract (300 frames × 6 threads): ~108s
- Pillow label + tile: ~5s
- KB export (frames + clean sheets + manifests): ~10s
- **End-to-end fresh: ~10min, cached: ~2min**

**Architecture**:
```
queue.py [--wait] <magnet>
  ↓
aria2c → downloads/<release>/
  ↓ (if --wait + --autosheets)
sheets_sweep.py
  ↓ (per qualifying release)
contact_sheet.py [--kb-export <kb>]
  ↓
release/contact-sheets/<slug>_sheet_NN.png   # labeled, human review
kb/frames/<slug>/<slug>_frame_NNN.jpg        # clean, RAG ingest
kb/contact-sheets-clean/<slug>/<slug>_sheet_NN.jpg
kb/per-movie/<slug>.json
kb/manifest.jsonl  (append)
```

## Key Files

**Scripts (Python)**:
- `scripts/queue.py` — aria2c wrapper. Magnet validation, `--wait`/`--seed`/`--autosheets`/`--no-autosheets` flags, integration with sweeper post-aria2c.
- `scripts/contact_sheet.py` — main pipeline. ~500 lines. Argparse: positional `mkv`, flags `--out --threshold --floor --target --cols --rows --width --workers --title --keep-raw --kb-export --kb-force`. Helpers: `slugify`, `fmt_tc`, `fmt_tc_ff`, `probe_fps`, `probe_duration`, `parse_year_from_title`, `escape_movie_path`. Functions: `detect_scenes` (cached), `apply_floor`, `cap_target`, `_extract_one`, `label_frame`, `tile_sheets` (now with clean=True mode), `export_kb`.
- `scripts/sheets_sweep.py` — opportunistic sweeper. ~330 lines. Argparse: `--downloads --skip --dry-run --force --kb/--no-kb`. fcntl.flock LOCK_NB on `logs/.sheets_sweep.lock`. Walk → filter → resolve+assert → run_contact_sheet (with kb_root forward).

**Tests**:
- `scripts/tests/test_sweep.sh` — 12 assertions, ~150 lines bash. Hermetic via `mktemp -d`.
- `scripts/tests/test_kb_export.sh` — 18 assertions, ~230 lines bash. Hermetic. Fixture: 7-color concat lavfi.

**Skill**:
- `.claude/skills/pirata-deck/SKILL.md` — main skill spec. PT-BR conversation, English technical terms. 12-branch menu (HELP, MOVIE, SERIES, ANIME, MUSIC, DOC, LIVE, COURSE, SOFT, STATUS, DOCTOR, QUEUE).
- `.claude/skills/pirata-deck/references/menu-style.md` — TR-100 panel templates. 55-char fixed width, 12-char label col, 40-char data col. Monochrome strict (no ANSI in fences).

**Docs**:
- `docs/brainstorms/2026-04-24-kb-rag-multimodal-frames-requirements.md` — KB export brainstorm.
- `docs/plans/2026-04-24-001-feat-hunter-py-orchestrator-plan.md` — pre-existing plan (different topic, untouched).
- `docs/plans/2026-04-24-002-feat-auto-contact-sheets-plan.md` — sweeper plan (originally hook, pivoted post-doc-review).
- `docs/plans/2026-04-24-003-feat-kb-rag-multimodal-frames-plan.md` — KB export plan.

**Logs / runtime**:
- `logs/sheets_sweep.log` — append-only sweep log.
- `logs/.sheets_sweep.lock` — flock sentinel.

**Memory**:
- `~/.claude/projects/-Users-vidigal-claude-code-pirata/memory/MEMORY.md` — index.
- `~/.claude/projects/-Users-vidigal-claude-code-pirata/memory/feedback_ansi_in_code_fence.md` — feedback memory about ANSI escape leakage in code fences.

## Warnings & Gotchas

- **ANSI escapes don't render in Claude Code fences.** Any future TR-100 / figlet skill output rendered inside ``` blocks must be monochrome. Saved as feedback memory.
- **`scripts/queue.py` shadows stdlib `queue`.** Every Python script in `scripts/` MUST apply the sys.path filter at the very top before any import that may transitively touch `queue` (most stdlib concurrency does). Pattern: `sys.path[:] = [p for p in sys.path if os.path.abspath(p) != os.path.dirname(os.path.abspath(__file__))]`.
- **macOS bash is 3.2.57 (frozen)**. Avoid `${VAR,,}`, associative arrays, `mapfile`. The sweeper is pure Python so this doesn't bite — but any future bash code in this repo must work in 3.2.
- **`Path.resolve(strict=True)` BOTH sides** of `is_relative_to` checks. macOS has `/tmp → /private/tmp`; if `downloads_root` from config isn't resolved, every input file (which IS resolved) will silently fail the comparison.
- **scdet output is one row per frame** — most are empty (`,`). Parser must skip empty/comma-only lines. `out.split()` on whitespace breaks (newlines are sep, but bare commas aren't).
- **Argparse flag injection via filenames.** Files named `--evil.mkv` are legal on macOS/Linux. Subprocess argv MUST use `--` terminator before positional args. Tested in both smoke tests.
- **Output dir is `contact-sheets/`, NOT `contact/`.** Torrent payloads ship `contact/` subdirs; co-mingling breaks re-seeding hash integrity.
- **`--kb-force` rerun duplicates JSONL lines.** Documented; future `--kb-rebuild-manifest` utility will dedupe by `slug + idx` key. Acceptable for v1.
- **scdet cache is per-threshold.** Changing `--threshold` invalidates the cache (key includes the value). User-facing tip: don't change threshold lightly, you eat 9min per change.
- **Disk gate skips on dry-run** — by design (dry-run is read-only); real run still enforces. This was a smoke-test issue; the production path is fine.
- **Sweep-level flock is `LOCK_NB`** — non-blocking. Concurrent sweeps log "already active" and exit 0. Means user can spam-invoke sweeps without consequence.
- **KB JSONL is append-only with no rotation.** Will grow unbounded. Not a v1 concern (~50KB/movie); revisit if it crosses 100MB.
- **Roger Rabbit's existing sheets are at `contact/` not `contact-sheets/`** (pre-sweeper-pivot artifact). Sweep would regenerate to new path on next real run. Disk pressure means user should free space first.
- **Moondream caption pass is opt-in only** in v1 schema (`caption: null`). If we add `--kb-caption` flag later, remember it adds ~10min/movie — flag should be very intentional.
- **Manifest paths are RELATIVE to `kb/` root.** Means user can rsync `kb/` to another machine without breaking refs. Don't change to absolute without thinking through portability.
- **`sheets_sweep.py`'s own `slugify()` returns the HUMAN-READABLE TITLE**, not the URL slug. Confusing naming — `slugify(release.name)` strips `[tags]` and returns "Who Framed Roger Rabbit (1988)". The actual URL slug derivation happens inside `contact_sheet.py`'s separate `slugify()`. Two functions with same name, different output. Don't unify without checking callers.
- **Pillow JPEG `optimize=True` is slow on large images** but worth it for storage. Tested — adds ~1s per sheet, saves ~30% size. Keep it.
