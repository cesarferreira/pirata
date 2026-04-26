---
title: KB-ready frames + clean contact sheet export for multimodal RAG
type: feature
status: ready-for-plan
date: 2026-04-24
---

# KB-ready frames + clean contact sheet export for multimodal RAG

## Problem Frame

The existing pipeline (`scripts/contact_sheet.py` + `scripts/sheets_sweep.py`) produces **human-facing** labeled contact sheets: 10 tiled sheets per movie with `N 001 · TC HH:MM:SS:FF` caption strips below each thumb. Great for editorial browsing; **wrong shape for multimodal RAG**.

RAG/vision retrieval pipelines (CLIP / SigLIP / Moondream / GPT-4V / Claude 4 vision / Gemini Pro Vision) expect:
- **Clean frames** — no text overlays (vision models OCR them and pollute the embedding signal)
- **Deterministic naming** — filename as retrieval key
- **Structured metadata** — TC, frame index, movie context in sidecar JSON, not burned into the image
- **Consistent layout** — predictable root for crawlers

Additionally, a **contact-sheet-without-labels** variant is useful as a "movie fingerprint" — embeddable as a single image representing the full film.

Goal: extend the existing pipeline to also emit a parallel KB-ready set of artifacts per movie, without touching the labeled human-facing output.

## Users & Value

- **Primary user**: single operator (Vidigal). Personal KB.
- **Primary value**: enable future multimodal retrieval across the user's pirata collection — "show me frames similar to X", "what movies have a duck shot", "find the scene matching this reference" — without requiring re-extraction or manual cleanup.
- **Secondary value**: if the user later wires a RAG pipeline (LlamaIndex, Haystack, custom embedder, or the existing `knowledge-hub` MCP), the KB artifacts are drop-in ready — no preprocessing layer needed.

## Scope

### In scope

- Extend `contact_sheet.py` with an optional `--kb-export <dir>` flag
- When set, emit per-movie to `<kb>/`:
  - 300 clean frame images at 640px, JPEG q=90
  - 10 clean contact sheets (same 6×5 layout, but without caption strips and without header)
  - Per-movie JSON manifest with title, year, fps, runtime, frame list with TC/timestamp/sheet-position
  - Append-mode global JSONL manifest (one line per frame, streaming RAG-ingest friendly)
- Extend `sheets_sweep.py` to pass `--kb-export` by default; add `--no-kb` opt-out
- Central KB root: `~/claude-code/pirata/kb/`
- Labeled (human) contact sheets in `<release>/contact-sheets/` are **unchanged**
- Idempotent: re-running the sweep on an already-exported movie skips the KB re-export

### Out of scope (deferred)

- **Moondream captions per frame** — user has the skill + hardware (Apple Silicon MLX); opt-in flag could be added later (`--kb-caption`). Adds ~10min per movie. Manifest schema should reserve a `caption` key but leave null.
- **`knowledge-hub` MCP auto-ingest** — user has `mcp__knowledge-hub__ingest_sync` available. Could wire as sweep post-step. Deferred — ingestion choice belongs in a follow-up.
- **IPTC/XMP embedded metadata** — cross-domain best-practice (Getty, Shutterstock), lets metadata travel with file. Requires `exiftool` dep. Deferred; sidecar JSON is adequate for v1.
- **Mega-sheet** (single 300-thumb image for "movie fingerprint" retrieval) — nice-to-have; user chose standard 10-sheet layout for v1. Can add later if similar-film retrieval becomes a use case.
- **Dedup across rips of same movie** — if user downloads 1080p and 2160p of the same film, slug collides. v1 accepts: the second run overwrites frames (same visual content, higher-res takes precedence). Manifest's `source_file` field is the tiebreaker for interested queries.
- **KB file rotation / cleanup** — KB grows unbounded. User manages disk. Consider `sheets_sweep.py --kb-prune` utility in v2.
- **Labeled-sheet variant of mega-sheet** — N/A, out of scope entirely.

## Key Decisions (confirmed via Q&A)

| Axis | Choice | Rationale |
|---|---|---|
| **KB location** | Central `~/claude-code/pirata/kb/` | Single root for RAG crawlers; survives release deletion; separate from ephemeral `downloads/` |
| **Image format** | JPEG q=90 | ~80KB/frame vs PNG ~300KB. 300 frames × 100 movies = 2.4GB JPEG vs 9GB PNG. Embedding quality indistinguishable. User disk at 92% used — cost matters. |
| **Frame naming** | `<slug>_frame_NNN.jpg` | User-requested. TC and position live in manifest, not filename. Minimal, deterministic, human-browsable. |
| **Clean sheet layout** | Same 10×(6×5) as labeled, minus captions + header | Consistent visual with labeled sheets; reuses tile math; no new layout decision needed. |
| **Manifest strategy** | Both per-movie JSON + global JSONL | Per-movie for human inspection + debugging; JSONL for RAG pipeline streaming ingest. Both cost ~5KB extra per movie, negligible. |

## Directory Structure

```
~/claude-code/pirata/kb/
├── frames/                                                  # raw clean frames, per-movie subdirs
│   ├── who-framed-roger-rabbit-1988/
│   │   ├── who-framed-roger-rabbit-1988_frame_001.jpg
│   │   ├── who-framed-roger-rabbit-1988_frame_002.jpg
│   │   └── ...                                              # 300 frames @ 640px JPEG q=90
│   ├── dune-2021/
│   └── ...
├── contact-sheets-clean/                                    # tiled sheets without captions
│   ├── who-framed-roger-rabbit-1988/
│   │   ├── who-framed-roger-rabbit-1988_sheet_01.jpg
│   │   └── ...                                              # 10 sheets, clean grid
│   └── ...
├── per-movie/                                               # human-inspectable manifests
│   ├── who-framed-roger-rabbit-1988.json
│   └── ...
└── manifest.jsonl                                           # global, 1 line per frame, append-only
```

## Per-Movie Manifest Schema

```json
{
  "slug": "who-framed-roger-rabbit-1988",
  "title": "Who Framed Roger Rabbit",
  "year": 1988,
  "fps": 23.976,
  "runtime_s": 6227.414,
  "source_file": "downloads/Who Framed Roger Rabbit (1988) [...]/Who.Framed....mkv",
  "source_size_bytes": 5046046238,
  "scdet": {
    "threshold": 8,
    "floor_s": 4,
    "target": 300
  },
  "extracted_at": "2026-04-24T20:48:00Z",
  "frames": [
    {
      "idx": 1,
      "file": "who-framed-roger-rabbit-1988_frame_001.jpg",
      "tc": "00:00:09:05",
      "t_s": 9.208,
      "sheet": 1,
      "pos": [0, 0],
      "caption": null
    },
    ...
  ],
  "sheets": [
    {"n": 1, "file": "who-framed-roger-rabbit-1988_sheet_01.jpg", "frame_range": [1, 30]},
    {"n": 2, "file": "who-framed-roger-rabbit-1988_sheet_02.jpg", "frame_range": [31, 60]},
    ...
  ]
}
```

`caption` is `null` in v1 — reserved for future Moondream/vision-model pass.

## Global JSONL Schema

One line per frame. RAG ingestion pipelines typically stream JSONL; this is the lingua franca:

```jsonl
{"slug":"who-framed-roger-rabbit-1988","idx":1,"file":"frames/who-framed-roger-rabbit-1988/who-framed-roger-rabbit-1988_frame_001.jpg","tc":"00:00:09:05","t_s":9.208,"title":"Who Framed Roger Rabbit","year":1988}
{"slug":"who-framed-roger-rabbit-1988","idx":2,"file":"frames/who-framed-roger-rabbit-1988/who-framed-roger-rabbit-1988_frame_002.jpg","tc":"00:01:16:19","t_s":76.79,"title":"Who Framed Roger Rabbit","year":1988}
...
```

Paths are relative to the `kb/` root so the file is portable across machines if user rsyncs.

## Success Criteria

- **R1.** Running `sweeps_sweep.py` on a fresh release produces clean frames + clean sheets + per-movie JSON + appended JSONL lines in `~/claude-code/pirata/kb/`, without disturbing the labeled `<release>/contact-sheets/` output.
- **R2.** Frames in `<kb>/frames/<slug>/` have no visible text, no captions, no index markers — pristine thumbnails ready for CLIP/SigLIP/Moondream ingestion.
- **R3.** Frame naming follows `<slug>_frame_NNN.jpg` exactly; NNN is the frame index from the contact sheet (1-300, zero-padded 3 digits).
- **R4.** Clean contact sheets in `<kb>/contact-sheets-clean/<slug>/` use the same 6×5 grid layout as labeled sheets but omit the caption strip and header, making them embeddable as "movie fingerprint" images.
- **R5.** Per-movie JSON manifest is valid JSON, schema as specified above, always contains all 300 frame entries on a full run.
- **R6.** Global JSONL manifest is valid JSONL (one self-contained JSON object per line, no commas between lines); appends atomically per movie.
- **R7.** Re-running the sweep on an already-exported movie is idempotent — skips the KB re-export unless `--kb-force` is set.
- **R8.** Opt-out via `sheets_sweep.py --no-kb` produces the current behavior (only labeled sheets, no KB artifacts).
- **R9.** Disk cost per movie is ≤80MB (300 frames × ~80KB + 10 sheets × ~5MB + manifest).

## Constraints & Assumptions

- JPEG q=90 is the chosen compression level. Tested empirically in image-gen literature to be visually indistinguishable from lossless for natural photography at 640px; embeddings from CLIP/SigLIP/Moondream are robust to this level of JPEG compression.
- Manifest JSONL append is "best-effort atomic" — on POSIX, a single `write()` call of ≤4KB is atomic at the kernel level. Our per-frame lines are well under that; concurrent sweep writers are already prevented by `sheets_sweep.py`'s flock.
- Central KB path `~/claude-code/pirata/kb/` is hard-coded as default. User can override via `--kb-export <dir>` if desired (e.g., external drive).
- Assumption: RAG pipeline will be wired later, out of scope for this brainstorm. The structure should be "any reasonable multimodal RAG pipeline can ingest this" — flat-ish dirs, per-movie grouping, JSONL manifest. LlamaIndex / Haystack / custom embedder will all work.

## Risks & Open Questions

### Resolved

- **Where to put KB files?** → Central `~/claude-code/pirata/kb/` (user chose).
- **File format?** → JPEG q=90 (user chose).
- **Frame naming?** → `<slug>_frame_NNN.jpg` (user explicit request).
- **Clean sheet layout?** → Same 10×(6×5) as labeled minus captions (user chose).
- **Manifest strategy?** → Both per-movie JSON + global JSONL (user chose).
- **What happens if two movies have the same slug (e.g., re-rip of same film)?** → Second run overwrites. Manifest's `source_file` field distinguishes. Acceptable for v1; dedup is a future concern.

### Deferred to Planning

- **Exact idempotency check** — is it `kb/per-movie/<slug>.json` exists → skip, or something more robust? Plan decides.
- **Exact JSONL append atomicity implementation** — vanilla `open('a')` write OR write-to-temp-then-append? Plan decides; POSIX small-write atomicity likely sufficient for v1.
- **How to regenerate `manifest.jsonl` from scratch if it gets corrupted?** — probably `scripts/sheets_sweep.py --kb-rebuild-manifest` utility. Plan-level decision.
- **Should labeled sheets' `scenes_raw_t8.txt` cache file move into `kb/` too?** — probably not; keep caches where they are (`<release>/contact-sheets/scenes_raw_t8.txt`). Plan can refine.
- **Thumbnail variant for UI preview?** — some RAG UIs want 128px or 256px thumbs in addition to full 640px. Not requested; skip for v1.

### Risks

| Risk | Severity | Mitigation |
|---|---|---|
| Disk grows fast — 75MB/movie × steady downloads | MEDIUM | JPEG (vs PNG) already cuts 3.5x. User monitors `df` via `/pirata 10`. Future: `--kb-prune` utility. |
| JPEG compression artifacts degrade embedding quality | LOW | q=90 is empirically fine for CLIP-style vision models. Can bump to q=95 if retrieval quality suffers in practice. |
| Manifest JSONL corruption (mid-write crash) | LOW | POSIX single-line writes are atomic for small content. Sweep's flock prevents concurrent sweep writes. |
| `<kb>` on external drive that's unmounted during sweep | MEDIUM | Worker fails cleanly (filesystem error), logs `fail`, moves on. User reconnects and re-sweeps. |
| Future rename of `contact_sheet.py` flags breaks KB export | LOW | DOCTOR's CONTRACT check already covers flag drift. Add `--kb-export` to the check list. |
| User deletes a release dir but KB frames remain orphaned | LOW | By design: KB is separate lifecycle. `--kb-prune` utility would reconcile (deferred). |

## Next Step

Pass this to `/ce-plan` to produce the implementation plan. The plan must address:

1. Exact implementation of `--kb-export` in `contact_sheet.py` — how to emit raw frames without labels (currently `label_frame()` creates the composite; raw frames live transiently and get deleted). Likely: add a `--keep-raw` variant or a parallel JPEG writer.
2. Exact implementation of the clean tile rendering — reuse `tile_sheets()` with a flag that skips the caption strip + header.
3. `sheets_sweep.py` integration: pass `--kb-export ~/claude-code/pirata/kb` by default; add `--no-kb` flag.
4. Per-movie JSON writer with atomic write-then-rename.
5. Global JSONL append — relies on POSIX atomic small-write semantics; no lock beyond the existing sweep-level flock.
6. Idempotency check: skip re-export if `<kb>/per-movie/<slug>.json` exists (unless `--kb-force`).
7. Tests: `scripts/tests/test_kb_export.sh` — assert all 4 artifact classes appear for a fixture run; assert manifest shape; assert frames have no text (OCR check or byte-diff vs a control image).
8. `/pirata 10` (DOCTOR) extension: verify `~/claude-code/pirata/kb/` exists, add KB-export flag to contract drift check.

Estimated scope: **Standard** plan, 3-4 implementation units.
