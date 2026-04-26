---
title: "feat: IMDb KB enrichment in contact_sheet manifest builder"
type: feat
status: active
date: 2026-04-26
origin: docs/plans/2026-04-24-004-feat-imdb-local-pirata-coupling-plan.md
---

# feat: IMDb KB enrichment in contact_sheet manifest builder

## Overview

Line-level executable plan for **Unit 3** of the IMDb × /pirata coupling (plan 004). Unit 1 (`scripts/imdb_ingest.py`) and Unit 2 (`scripts/imdb_lookup.py`) shipped at commits `fb4b4a4` and `8e31539`. This plan deepens Unit 3 — the integration that connects the lookup helper to the contact-sheet KB-export pipeline so per-movie manifests (and the downstream `kb/kh-export/` surface that knowledge-hub indexes) get IMDb-resolved title, year, and metadata fields instead of slug-derived placeholders.

The target failure mode is concrete: `kb/per-movie/the-super-mario-galaxy-movie-2026.json` currently has `title="the-super-mario-galaxy-movie-2026"` and `year=null` because `scripts/contact_sheet.py:434` falls back to `args.title or "Contact Sheet"` and `parse_year_from_title` only matches a trailing `(YYYY)` regex. The slug round-trips through the export and into knowledge-hub as a polluted record. After this plan lands, the same file emerges with `title="Super Mario Galaxy"` (IMDb primaryTitle), `year=2026`, plus a structured `imdb` block carrying tconst, genres, rating, director, plot, top_cast, and PT/EN/ES akas.

## Problem Frame

Today's KB enrichment surface is a manifest builder at `scripts/contact_sheet.py:282-330` that snapshots scene-detected frames + sheets metadata into `kb/per-movie/<slug>.json`. The title and year fields ride from whatever caller passed via `--title` (sheets_sweep passes the release directory name unmodified). Any release whose dirname doesn't end in `(YYYY)` lands with `year=null`; any release whose dirname is the slug-shaped form (dotted/dashed) lands with both title and year corrupted.

knowledge-hub indexes `kb/kh-export/04-derived/per-movie/<slug>.md` (auto-generated YAML frontmatter + body from `scripts/build_kh_export.py`). The frontmatter and body read top-level `title` / `year` from the per-movie JSON. So the bug ripples into the retrieval surface — Mario Galaxy is queryable by `super-mario-galaxy-2026` but not by `Super Mario Galaxy` or `2026 nintendo movie`.

Unit 1 + Unit 2 give us everything needed to fix this in-pipeline:
- Local IMDb FTS5 corpus (15 GB, ~10M titles, refresh 2026-04-25).
- `lookup_by_title(query, year=None, kind=None)` with locked composite-score formula (RapidFuzz WRatio + tier separation, 4 ms p99).
- `lookup_by_tconst(tconst)` returning enriched `Title` (genres, rating, director, top_cast, akas).
- `IMDbDBUnavailable` exception for refresh-in-flight handling.

What's missing: a filename-cleaning pass (PTT) that turns release dirnames into queryable titles + years, a confidence-threshold gate that suppresses bad picks, the manifest writer integration, the sweep pass-through, and the DOCTOR contract update.

## Requirements Trace

- **R1** — Per-movie manifest gets a top-level `title` and `year` resolved from IMDb when lookup succeeds with confidence ≥ threshold; falls back to PTT-extracted values when below threshold or no match. Original filename trace preserved under `filename.{raw_title, ptt_title, ptt_year}` for debuggability (advances plan 004 R7, R7b, R7c).
- **R2** — Per-movie manifest gets an `imdb` block carrying `{tconst, primaryTitle, originalTitle, year, genres, rating: {average, votes}, director: [{nconst, name}], plot, top_cast: [{nconst, name, role}], akas: [{title, region, language}], confidence, multi_tie}` when IMDb resolved, OR `{lookup_attempted: true, result: <reason>, candidates_considered, top_score?, runner_up_score?, multi_tie?}` when not. `result` is one of `resolved`, `multi_tie`, `below_threshold`, `no_match`, `db_unavailable` (advances plan 004 R7c).
- **R3** — `scripts/contact_sheet.py` accepts `--kb-imdb` / `--no-kb-imdb` (BooleanOptionalAction, default=True). When off, manifest builder skips the IMDb resolution entirely and writes the historical shape (no `imdb` block, no `filename` block). When on but `kb_root` is None, the flag is a no-op (matches existing `--kb-export` semantics).
- **R4** — `scripts/sheets_sweep.py` accepts the same `--kb-imdb` / `--no-kb-imdb` and propagates to its `contact_sheet.py` invocation. Default on per plan 004 Key Decisions.
- **R5** — `.claude/skills/pirata-deck/SKILL.md` DOCTOR `CONTRACT` line (`SKILL.md:163`) extended to include `--kb-imdb` in the expected `--help` flag set so DOCTOR doesn't report drift after R3 lands.
- **R6** — When `imdb/.refresh.lock` is present (Unit 1's flock semaphore) or `imdb/imdb.db` is missing, manifest builder emits `imdb.result="db_unavailable"`, logs one line to `logs/sweep_imdb_misses.log`, and continues. The sheet pipeline never crashes on IMDb unavailability (advances plan 004 R7b).
- **R7** — Re-running `python3 scripts/build_kh_export.py` after this lands regenerates `kb/kh-export/04-derived/per-movie/<slug>.md` so the YAML frontmatter and body surface the new `imdb.{primaryTitle, year, genres, rating.average, top_cast names}` fields and the body grounds smoke retrieval on those literals.
- **R8** — `logs/sweep_imdb_misses.log` (already gitignored under plan 004 Unit 1) gets one append per non-`resolved` outcome, JSONL: `{ts, slug, raw_title, ptt_title, ptt_year, result, top_score?, runner_up_score?, multi_tie?, candidates_considered}`.
- **R9** — Confidence threshold is locked at 15 % (Key Decisions in plan 004); below threshold OR `multi_tie=true` produces a non-`resolved` outcome and the fallback path. Threshold lives as a module-level constant `IMDB_CONFIDENCE_PCT` near the top of the new helper module so calibration after real-data runs is a one-line change.

## Scope Boundaries

- NOT modifying `scripts/imdb_lookup.py` (Unit 2 contract is locked).
- NOT modifying `scripts/imdb_ingest.py` (Unit 1 ingest contract is locked).
- NOT touching `kb/manifest.jsonl` schema — manifest builder still emits the same JSONL line shape; only `kb/per-movie/<slug>.json` gains new fields.
- NOT calling knowledge-hub MCP tools (`ingest_sync`, `retrieve`, `list_kbs`) from this plan. Re-staging + re-ingest is operator-driven (paste-ready prompt at `docs/prompts/2026-04-26-kh-ingest-FIRE-v3.md` already covers it).
- NOT changing R9/R10 (anime / music / soft / course paths in `/pirata` skill). Those are Unit 4. The confidence threshold + `multi_tie` gate naturally suppresses bad picks for anime titles that don't match IMDb's western catalog.
- NOT adding a CAG pack for `pirata-kb`. Defer until catalog has ≥ 10 enriched manifests.
- NOT mutating `/Users/vidigal/projects/knowledge-hub` or `/Users/vidigal/knowledge-base`. Pirata-side only.

### Deferred to Separate Tasks

- **20-item PT-BR fixture full validation** — fixture file `scripts/tests/fixtures/imdb_pt_br_20.txt` currently seeds 7 entries (`FIXTURE: 7/7 ok` from Unit 2 smoke). Filling out the remaining 13 user-chosen entries is calibration work that benefits from real Unit-3 enrichment data; tracked as Phase 1 verification item, not this plan's blocker.
- **Confidence threshold re-calibration** — locked at 15 % per plan 004 Key Decisions. After Unit 3 runs against ~10 real releases, revisit if the fallback rate is too high or too aggressive. Threshold lives as a single module-level constant for one-line tuning.
- **R9 anime detection in `/pirata` skill** — Unit 4 territory. This plan accepts that anime titles will mostly land as `result="no_match"` or `result="below_threshold"` and fall back to PTT-extracted title/year, which is the right behavior for now.

## Context & Research

### Relevant Code and Patterns

- **`scripts/contact_sheet.py:282-330`** — current manifest builder. The `manifest = {…}` dict assembly is where new fields land. Top-level `title` / `year` already exist; new code resolves them via IMDb before the dict is built.
- **`scripts/contact_sheet.py:85-88`** — `parse_year_from_title` regex (trailing `(YYYY)`). Stays as a hard-coded fallback when PTT also fails to extract a year.
- **`scripts/contact_sheet.py:354-376`** — argparse block. New `--kb-imdb` flag lands here, mirroring `--kb-export`'s shape.
- **`scripts/contact_sheet.py:434-444`** — main entry. `title = args.title or "Contact Sheet"` is the bug surface; the IMDb resolution layer runs after this line and overwrites `title` / `year` when confident.
- **`scripts/sheets_sweep.py:266-268`** — `--kb` / `--no-kb` argparse pattern (BooleanOptionalAction, default=True). New `--kb-imdb` flag mirrors this exactly.
- **`scripts/sheets_sweep.py:138-153`** — `run_contact_sheet` builds the `argv` list passed to `contact_sheet.py`. New `--kb-imdb` / `--no-kb-imdb` thread-through lands here.
- **`scripts/sheets_sweep.py:296-308`** — main entry that resolves `kb_root` and calls `sweep`. New `kb_imdb` boolean threads through `sweep` → `run_contact_sheet`.
- **`scripts/imdb_lookup.py`** — public surface used by this plan: `lookup_by_title(query, year=None, kind=None)` returns `list[Match]`; `lookup_by_tconst(tconst)` returns `Title | None`; `IMDbDBUnavailable` raises when `imdb/.refresh.lock` exists or `imdb/imdb.db` missing.
- **`scripts/imdb_lookup.py:Match` dataclass** — fields needed for confidence gate: `tconst`, `primaryTitle`, `score`, `is_tier1`, `field_kind` (`primary` / `original` / `aka`), `multi_tie`.
- **`scripts/imdb_lookup.py:Title` dataclass** — fields surfaced into manifest: `primaryTitle`, `originalTitle`, `startYear`, `genres` (list[str]), `rating: {averageRating, numVotes}`, `directors: list[{nconst, primaryName}]`, `plot` (Wikipedia-derived if present, else None), `top_cast: list[{nconst, primaryName, characters}]`, `akas: list[{title, region, language}]`.
- **`scripts/build_kh_export.py`** — markdown wrapper builder (R7). Reads `kb/per-movie/<slug>.json`; emits `kb/kh-export/04-derived/per-movie/<slug>.md`. After this plan lands, the wrapper renderer needs to know how to surface the new `imdb` block. **In scope for Unit C** (one helper change in `build_kh_export.py`).
- **`.claude/skills/pirata-deck/SKILL.md:163`** — DOCTOR contract: `python3 scripts/contact_sheet.py --help` must contain every flag in the locked list. Adding `--kb-imdb` requires updating the locked list in lockstep — that's R5.
- **`scripts/queue.py:VIDEO_EXTS`** — already defines the video file matcher used by the wrap helper. Same set is the right one for filename detection in PTT (mkv, mp4, avi, mov, ts, m2ts, webm).

### Institutional Learnings

- `docs/solutions/` does not exist in pirata. Skipping institutional pattern lookup.
- Plan 004 Key Decisions captures every load-bearing institutional decision that touches Unit 3 (PTT vs guessit, RapidFuzz threshold, akas language slice, license stance).

### External References

- **PTT (parse-torrent-title)** — Python package `parse-torrent-title`, MIT-licensed, zero deps, mature filename parser for movie/TV release names. Returns dict with keys including `title`, `year`, `season`, `episode`, `resolution`, `quality`, `codec`, `audio`, `group`. Install: `pip3 install parse-torrent-title`. Import: `import PTT`. Usage: `parsed = PTT.parse_title(filename_stem)`.
- **IMDb non-commercial license** — already documented at `kb/kh-export/04-derived/README.md`; fields that ride into the manifest (genres, rating, plot, akas) are derivative and carry the same single-user / non-commercial restriction. No new license concerns introduced by this plan.

## Key Technical Decisions

- **Top-level `title` / `year` overwrite when confident.** When IMDb lookup returns `result="resolved"`, the manifest's top-level `title` becomes IMDb `primaryTitle` and top-level `year` becomes IMDb `startYear`. The PTT-extracted values are preserved under `filename.ptt_title` / `filename.ptt_year` for traceability. Reason: every existing consumer (`build_kh_export.py`, knowledge-hub retrieval) reads top-level `title` / `year`; nesting the IMDb-resolved values under a sub-key would force every consumer to know about both. Overwrite is the cleanest fix for the Mario Galaxy bug.
- **Fallback shape: PTT-extracted values become canonical when IMDb fails.** Below threshold / no match / DB unavailable → top-level `title` = `ptt_title`, top-level `year` = `ptt_year`. The `imdb` block records the failure reason. The historical `parse_year_from_title` regex stays as a last-ditch fallback when PTT also extracts no year.
- **`imdb.confidence` is the WRatio score of the chosen Match.** Tier-1 matches always get `confidence=100` per Unit 2's tier-separation rule. Tier-2 matches carry their RapidFuzz score (capped at 99). The 15 % threshold from plan 004 applies to the gap between top-1 and runner-up: if `(top.score - runner_up.score) / top.score < 0.15` AND both are tier-1 OR both are tier-2, set `multi_tie=true` and route to the fallback. Exception: when there's only one match, `multi_tie=false` regardless of score (no runner-up to tie with).
- **Year hint passes through PTT to IMDb.** When PTT extracts a year, `lookup_by_title(query=ptt_title, year=ptt_year)` runs first. Year-filtered lookup typically resolves multi-tie cases (e.g., two `Dune` titles disambiguated by `1984` vs `2021`). When PTT does NOT extract a year, `lookup_by_title(query=ptt_title)` runs without year filter and the multi_tie heuristic does the work.
- **No `kind` filter at the contact_sheet layer.** Plan 004 R9/R10 say music/soft/courses skip IMDb entirely — but those release types don't go through `contact_sheet.py` (they bypass the contact-sheet pipeline at the `/pirata` skill layer). Anime DOES go through contact_sheet. Anime + IMDb interaction: the lookup will mostly produce `no_match` or `below_threshold` for non-western anime titles, which falls through to PTT. No special anime detection needed at this layer; that's Unit 4's job.
- **Failure logs at the contact_sheet layer, not the sweep layer.** `logs/sweep_imdb_misses.log` (named per plan 004 Unit 1's gitignore convention — keeping the filename for consistency even though contact_sheet writes it directly now) is appended per non-`resolved` outcome. Format: JSONL with the fields enumerated in R8. Sweep doesn't log; contact_sheet logs because it's the layer that knows the resolution outcome.
- **`build_kh_export.py` markdown body surfaces only resolved IMDb fields.** When `imdb.result != "resolved"`, the body skips the IMDb section entirely (renders only the historical scdet / fps / runtime block). Reason: dropping a `result: below_threshold` block into the markdown wrapper pollutes retrieval surface with negative signals.
- **PTT install is a Phase 0 prerequisite, not a runtime install.** Unit 1 imdb_ingest.py already assumes RapidFuzz is `pip3 install`'d globally (per plan 004 Key Decisions). Same convention here: `pip3 install parse-torrent-title` runs before Unit 3 lands; `import PTT` failing is a hard error with a clear message pointing at the install command.
- **One new test script, not unit-level test breakdown.** `scripts/tests/test_contact_sheet_imdb.sh` covers all R3-R9 scenarios in a single hermetic harness following the same shape as `test_imdb_lookup.sh` and `test_kh_export.sh`. Reason: the existing pirata test discipline is shell-driven hermetic smokes, not pytest unit tests; introducing pytest here is a workspace convention change beyond this plan's scope.

## Open Questions

### Resolved During Planning

- **Where does the IMDb resolution code live?** New helper module at `scripts/imdb_kb_enrich.py` (sibling to `imdb_lookup.py`). Reason: keeps `contact_sheet.py` focused on frame extraction; the resolution logic (PTT parse → lookup → confidence gate → fallback) is reusable from other call sites later (e.g., a CLI utility for back-filling existing manifests).
- **What's the field ordering inside the `imdb` block?** Match the order in plan 004 Approach: `tconst, primaryTitle, originalTitle, year, genres, rating, director, plot, top_cast, akas`. Then `confidence` and `multi_tie` last so the IMDb-derived fields read as a coherent record before the calibration metadata.
- **Plural or singular `director`?** Singular `director` field but always a list; matches plan 004 Approach. A movie can have multiple directors; using a singular field name + list value matches IMDb's data model (`title_crew.directors` is a comma-separated list of nconsts).
- **`top_cast.role` source?** `title_principals_top5.characters` (Unit 1 ingested this column). When IMDb has no characters string, `role` is `null`.
- **Akas count cap.** Up to 10 akas per title (PT/EN/ES + isOriginal=1 only, per Unit 1 ingest predicate). If a title has > 10 matching akas, take the 10 most common regions in the order: `BR, PT, ES, MX, AR, US, GB, ` + remaining matches. Reason: keeps manifest size bounded; > 10 akas is signal that the title is too generic to be useful for retrieval anyway.
- **Manifest schema versioning?** No bump. The new fields are additive (top-level `title` / `year` shape unchanged; new `filename` and `imdb` blocks are sub-objects that consumers can ignore). `build_kh_export.py` is the only consumer that needs to be aware of the new fields and it lands updated in this plan.

### Deferred to Implementation

- **Exact PTT API surface to use.** PTT exposes `parse_title(filename)` returning a dict; some forks also expose `Parser` class with custom regex. First-pass uses `parse_title`; if it produces drifty results on real releases (e.g., extracts `1080p` as part of the title), revisit during implementation and pin a specific fork or pre-clean the input.
- **Plot field source.** Unit 1 did NOT ingest plot summaries (IMDb plot data isn't in the non-commercial TSV bundle). Plot stays `null` for now; Wikipedia-derived plot is a Phase 2 extension and lives behind a flag if/when added. The `imdb.plot` field exists in the schema but is always `null` in this plan.
- **Multi-tie disambiguation prompt for the user.** Plan 004 Unit 4 talks about a `RESOLVED` row in the SHORTLIST when IMDb engages in the `/pirata` skill. At the contact_sheet layer there's no user prompt — `multi_tie=true` simply routes to fallback and logs. Interactive disambiguation is Unit 4's concern.

## High-Level Technical Design

> *This illustrates the resolution flow and field shape. It is directional guidance for review, not implementation specification.*

### Resolution flow

```
                     ┌──────────────────────────────────┐
release file ───────►│  contact_sheet.py main entry     │
                     │  args.title or release dirname   │
                     └──────────┬───────────────────────┘
                                │
                                ▼ (if --kb-imdb on AND kb_root not None)
                     ┌──────────────────────────────────┐
                     │ imdb_kb_enrich.resolve(raw)      │
                     │                                  │
                     │ 1. PTT.parse_title(raw)          │
                     │    → ptt_title, ptt_year         │
                     │ 2. lookup_by_title(ptt_title,    │
                     │      year=ptt_year)              │
                     │    → list[Match] (or raises      │
                     │       IMDbDBUnavailable)         │
                     │ 3. confidence gate:              │
                     │    - 0 hits → no_match           │
                     │    - 1 hit  → resolved           │
                     │    - 2+ hits, gap < 15%          │
                     │      AND same tier               │
                     │      → multi_tie                 │
                     │    - 2+ hits, gap >= 15%         │
                     │      OR different tier           │
                     │      → resolved (top-1)          │
                     │ 4. if resolved:                  │
                     │      lookup_by_tconst(top.tconst)│
                     │      → Title (full enrichment)   │
                     │ 5. assemble imdb block + canon   │
                     │    title/year per resolution     │
                     └──────────┬───────────────────────┘
                                │
                                ▼
                     ┌──────────────────────────────────┐
                     │ manifest dict assembly           │
                     │ (existing code, lines 313-326)   │
                     │                                  │
                     │ canonical title/year on top      │
                     │ + filename block                 │
                     │ + imdb block                     │
                     └──────────┬───────────────────────┘
                                │
                                ▼
                     atomic write to kb/per-movie/<slug>.json
```

### Manifest field shape (resolved case)

```json
{
  "slug": "the-super-mario-galaxy-movie-2026",
  "title": "The Super Mario Galaxy Movie",
  "year": 2026,
  "fps": 23.976,
  "runtime_s": 5847.123,
  "source_file": "/path/to/release.mkv",
  "source_size_bytes": 12345678901,
  "scdet": {"threshold": 8, "floor_s": 1.0, "target": 300},
  "extracted_at": "2026-04-26T15:32:11Z",
  "filename": {
    "raw_title": "The.Super.Mario.Galaxy.Movie.2026.2160p.UHD.BluRay.x265-RELEASEGROUP",
    "ptt_title": "The Super Mario Galaxy Movie",
    "ptt_year": 2026
  },
  "imdb": {
    "tconst": "tt99999999",
    "primaryTitle": "The Super Mario Galaxy Movie",
    "originalTitle": "The Super Mario Galaxy Movie",
    "year": 2026,
    "genres": ["Animation", "Adventure", "Family"],
    "rating": {"average": 7.4, "votes": 18234},
    "director": [
      {"nconst": "nm12345", "name": "Aaron Horvath"},
      {"nconst": "nm67890", "name": "Michael Jelenic"}
    ],
    "plot": null,
    "top_cast": [
      {"nconst": "nm111", "name": "Chris Pratt", "role": "Mario"},
      {"nconst": "nm222", "name": "Anya Taylor-Joy", "role": "Princess Peach"}
    ],
    "akas": [
      {"title": "Super Mario Galaxy", "region": "BR", "language": "pt"},
      {"title": "Super Mario Galaxia", "region": "ES", "language": "es"}
    ],
    "confidence": 100,
    "multi_tie": false
  },
  "frames": [...300 entries...],
  "sheets": [...10 entries...]
}
```

### Manifest field shape (fallback case — below threshold)

```json
{
  "slug": "obscure-2024-foreign-film-2024",
  "title": "Obscure 2024 Foreign Film",
  "year": 2024,
  "filename": {
    "raw_title": "Obscure.2024.Foreign.Film.2024.1080p.WEBRip.x265",
    "ptt_title": "Obscure 2024 Foreign Film",
    "ptt_year": 2024
  },
  "imdb": {
    "lookup_attempted": true,
    "result": "below_threshold",
    "candidates_considered": 4,
    "top_score": 86,
    "runner_up_score": 81,
    "multi_tie": true
  },
  "frames": [...],
  "sheets": [...]
}
```

## Implementation Units

- [ ] **Unit A: PTT install + `scripts/imdb_kb_enrich.py` resolution helper**

  **Goal:** Stand up the IMDb resolution layer as a standalone helper module so it's testable in isolation and reusable from non-`contact_sheet` callers later. Install PTT globally as a Phase 0 prerequisite.

  **Requirements:** R1, R2, R6, R9.

  **Dependencies:** Unit 2 (`scripts/imdb_lookup.py` and IMDb DB).

  **Files:**
  - Create: `scripts/imdb_kb_enrich.py`
  - Test: `scripts/tests/test_imdb_kb_enrich.sh` (covers helper-level scenarios in isolation; integration scenarios live in Unit F's `test_contact_sheet_imdb.sh`)

  **Approach:**
  - Module-level constants: `IMDB_CONFIDENCE_PCT = 15` (gap threshold for multi-tie); `AKAS_CAP = 10`; `LOG_PATH = Path("logs/sweep_imdb_misses.log")`.
  - `import PTT` at module top; raises `ImportError` with a clear message ("install: pip3 install parse-torrent-title") if missing — hard fail at import, not at first call.
  - Public function `resolve(raw_title: str, *, slug: str | None = None) -> ResolutionResult` where `ResolutionResult` is a dataclass with fields `canonical_title: str`, `canonical_year: int | None`, `filename: dict`, `imdb: dict`. The `slug` arg is optional; when present, it's passed to the log line so misses are debuggable by slug.
  - Internal sequence inside `resolve`:
    1. `filename = {"raw_title": raw_title, "ptt_title": parsed["title"], "ptt_year": parsed.get("year")}` from `PTT.parse_title(raw_title)`. If PTT raises or returns no `title` key, fall back to `parse_year_from_title`-equivalent logic (regex `r"\((\d{4})\)\s*$"` for year extraction, raw_title as title).
    2. Try `imdb_lookup.lookup_by_title(filename["ptt_title"], year=filename["ptt_year"])`. Catch `IMDbDBUnavailable` → return `ResolutionResult(canonical_title=ptt_title, canonical_year=ptt_year, filename=..., imdb={"lookup_attempted": False, "result": "db_unavailable", "candidates_considered": 0})` and log.
    3. Confidence gate (Key Technical Decisions section above):
       - `len(matches) == 0` → `result="no_match"`, fallback canonical to ptt values, log.
       - `len(matches) == 1` → `result="resolved"`, lookup_by_tconst, build full imdb block with `confidence = matches[0].score`, `multi_tie=False`.
       - `len(matches) >= 2`:
         - Compute `gap = (top.score - runner_up.score) / top.score` (clamp top.score to ≥ 1 to avoid div0).
         - If `gap < 0.15` AND `top.is_tier1 == runner_up.is_tier1` → `result="multi_tie"`, fallback canonical to ptt values, log with `top_score`, `runner_up_score`, `candidates_considered`.
         - Else → `result="resolved"`, lookup_by_tconst on top, full imdb block.
    4. On `result="resolved"`: call `imdb_lookup.lookup_by_tconst(top.tconst)` → `Title`. Assemble imdb dict per the field shape in High-Level Technical Design. Apply `AKAS_CAP` ordering.
    5. Append one JSONL line to `LOG_PATH` for any non-`resolved` outcome (`no_match`, `multi_tie`, `below_threshold`, `db_unavailable`). Schema: R8.

  **Patterns to follow:**
  - Module-level connection cache + atexit cleanup pattern — see `scripts/imdb_lookup.py:_conn` and `close_connection`.
  - Dataclass `Match` / `Title` shapes — see `scripts/imdb_lookup.py:64,80`.
  - Atomic log append: open in `"a"` mode (POSIX `O_APPEND` is atomic for line-buffered writes < 4 KB).

  **Test scenarios** (in `test_imdb_kb_enrich.sh` — hermetic shell harness):
  - **Happy path: resolved** — input `"Dune.Part.Two.2024.2160p.UHD.BluRay.x265"` → `canonical_title="Dune: Part Two"`, `canonical_year=2024`, `imdb.result="resolved"`, `imdb.tconst=tt15239678` (or whatever the live DB returns; assert non-null tconst + matching primaryTitle case-insensitive).
  - **Happy path: year hint disambiguates** — input `"Dune.1984.1080p.WEBRip"` → `imdb.year=1984` (Lynch's Dune, not 2021); assert tconst matches `tt0087182`.
  - **Edge case: no year in filename** — input `"Bacurau.1080p.WEBRip"` → resolves to tt9683478 (2019 Brazilian film); assert resolved.
  - **Edge case: PTT extracts wrong title (numbers in title)** — input `"2001.A.Space.Odyssey.1968.1080p.BluRay"` → PTT might split awkwardly; assert resolved tconst=tt0062622 OR fallback to ptt_title preserving "2001 A Space Odyssey".
  - **Edge case: title with colon / special chars** — input `"Star.Wars.Episode.IV.A.New.Hope.1977"` → assert resolves to tt0076759 OR documents the failure mode in test output.
  - **Error path: multi_tie on close scores** — input `"The.Office.2005.S01E01"` → multiple "The Office" tconsts may tie; assert `imdb.result="multi_tie"`, canonical falls back to ptt values, log file gains a JSONL line with `multi_tie=true`.
  - **Error path: no_match for gibberish** — input `"Xyzzyplugh.Definitely.Not.A.Real.Movie.2026"` → assert `imdb.result="no_match"`, canonical falls back, log JSONL line emitted.
  - **Error path: db_unavailable when lookup raises** — temporarily rename `imdb/imdb.db` aside (or set `imdb/.refresh.lock`) → assert `IMDbDBUnavailable` is caught, `imdb.result="db_unavailable"`, function returns cleanly without raising.
  - **Edge case: PTT import missing** — temporarily mask the PTT module (PYTHONPATH trick or wrapper import shim) → assert clear ImportError with install hint at module load.
  - **Edge case: AKAS_CAP enforcement** — pick a title with > 10 akas (e.g., a major Disney film); assert `len(imdb.akas) <= 10` and the priority order is preserved.
  - **Verification:** `bash scripts/tests/test_imdb_kb_enrich.sh` exits 0; log file at `logs/sweep_imdb_misses.log` contains exactly one JSONL line per non-resolved scenario.

- [ ] **Unit B: `scripts/contact_sheet.py` integration**

  **Goal:** Wire the resolution helper into the manifest builder so per-movie JSON output gains the new fields when `--kb-imdb` is on.

  **Requirements:** R1, R2, R3, R6.

  **Dependencies:** Unit A.

  **Files:**
  - Modify: `scripts/contact_sheet.py`

  **Approach:**
  - Add `--kb-imdb` argparse flag at `contact_sheet.py:368` near `--title`: `ap.add_argument("--kb-imdb", action=argparse.BooleanOptionalAction, default=True, help="Resolve title/year/genres/rating/cast via IMDb local catalog (default on; pass --no-kb-imdb to skip)")`.
  - At `contact_sheet.py:434` (where `title = args.title or "Contact Sheet"` lives): if `args.kb_imdb` AND `args.kb_export`, import `imdb_kb_enrich` and call `result = imdb_kb_enrich.resolve(args.title or mkv.stem, slug=slugify(args.title or mkv.stem))`. Use `result.canonical_title` as `title` and `result.canonical_year` as `year` from this point forward.
  - Carry `result.filename` and `result.imdb` into the manifest dict assembly at `contact_sheet.py:313-326`. Insert as new top-level keys after `extracted_at`, before `frames`.
  - When `--no-kb-imdb` OR `args.kb_export` is None: skip the resolve call entirely. Manifest emits the historical shape (no `filename`, no `imdb` keys).
  - Year fallback: if `args.kb_imdb=False`, year still uses `parse_year_from_title(title)` per current behavior (line 287). If `args.kb_imdb=True`, year comes from `result.canonical_year`; manifest's top-level `year` field still exists either way.

  **Patterns to follow:**
  - `--kb-export` flag definition at `contact_sheet.py:373` for argparse shape.
  - `slugify(title)` invocation at `contact_sheet.py:435` for slug derivation.

  **Test scenarios** (covered in Unit F's `test_contact_sheet_imdb.sh`).

  **Verification:**
  - `python3 scripts/contact_sheet.py --help` lists `--kb-imdb` / `--no-kb-imdb`.
  - Running with `--kb-imdb` against a real release produces manifest with `filename` + `imdb` blocks; running with `--no-kb-imdb` produces the historical shape.

- [ ] **Unit C: `scripts/build_kh_export.py` markdown wrapper update**

  **Goal:** Surface the new IMDb fields in the auto-generated markdown wrappers so knowledge-hub indexes the enriched metadata.

  **Requirements:** R7.

  **Dependencies:** Unit B (manifest shape settled before export rebuild matters).

  **Files:**
  - Modify: `scripts/build_kh_export.py`

  **Approach:**
  - Detect `imdb` block in the source per-movie JSON. When `imdb` is present AND `imdb.result == "resolved"` (not `"no_match"`, etc.), surface IMDb fields:
    - YAML frontmatter gains: `tconst`, `genres` (comma-joined), `rating_average`, `rating_votes`, `directors` (semicolon-joined names), `imdb_confidence`. The existing `slug`, `title`, `year`, `fps`, `runtime_s`, `frame_count`, `sheet_count`, `scdet` keys stay unchanged in shape but `title` / `year` are now the IMDb-resolved values when applicable.
    - Markdown body gains a new section after the existing scdet/fps block: `## IMDb metadata` followed by a bulleted list with `**Genres:** {genres}`, `**Rating:** {average} ({votes} votes)`, `**Director:** {names}`, `**Top cast:** {top 5 names with roles}`. Include the title's `originalTitle` as a literal string in the body (helps retrieval recall on transliteration cases).
    - When there's a `plot` field with non-null value, render it as a paragraph in the body. (For this plan, plot is always null; the field is reserved for Phase 2.)
  - When `imdb.result != "resolved"` or `imdb` block absent: render the historical wrapper shape unchanged (no new YAML keys, no IMDb section in body). Reason: don't pollute retrieval surface with negative-signal metadata.
  - Keep the byte-deterministic guarantee: identical inputs produce byte-identical outputs across re-runs.

  **Patterns to follow:**
  - Existing wrapper-rendering code in `scripts/build_kh_export.py` — match the YAML-frontmatter + markdown-body style already in use.
  - `kb/kh-export/04-derived/per-movie/who-framed-roger-rabbit-1988.md` for the current shape; the new IMDb block lands after the existing content.

  **Test scenarios** (in `scripts/tests/test_kh_export.sh` — extending the existing suite):
  - **Happy path: enriched manifest produces enriched wrapper** — feed a JSON with `imdb.result="resolved"` + full block; assert frontmatter contains `tconst:`, `genres:`, `rating_average:`, `rating_votes:`, `directors:`; assert body contains `## IMDb metadata` section with all 4 bullets.
  - **Happy path: bare manifest produces bare wrapper** — feed a JSON without `imdb` key (pre-Unit-3 shape); assert frontmatter and body match historical output byte-for-byte (regression guard).
  - **Edge case: imdb.result="multi_tie"** — assert wrapper renders WITHOUT the `## IMDb metadata` section and WITHOUT new YAML keys (treated as bare).
  - **Edge case: imdb.result="db_unavailable"** — same as multi_tie: wrapper is bare.
  - **Determinism: re-run produces byte-identical output** — extend the existing fingerprint test to cover both bare and enriched manifests.
  - **Verification:** `bash scripts/tests/test_kh_export.sh` exits 0 with `PASS: N/N` count increased by 5 from current 29 → 34.

- [ ] **Unit D: `scripts/sheets_sweep.py` pass-through**

  **Goal:** Sweep accepts and propagates `--kb-imdb` / `--no-kb-imdb` to its `contact_sheet.py` invocations.

  **Requirements:** R4.

  **Dependencies:** Unit B (flag must exist on contact_sheet.py first).

  **Files:**
  - Modify: `scripts/sheets_sweep.py`

  **Approach:**
  - Add argparse at `sheets_sweep.py:266` near `--kb`: `ap.add_argument("--kb-imdb", action=argparse.BooleanOptionalAction, default=True, help="Pass through to contact_sheet.py: resolve metadata via IMDb local catalog (default on; pass --no-kb-imdb to skip)")`.
  - Thread the boolean through `sweep()` signature → `run_contact_sheet()` signature → the `argv` list passed to subprocess. When `kb_imdb=False`, append `--no-kb-imdb` to argv; when `kb_imdb=True`, omit (default on the contact_sheet side).
  - Emit on the start log line: append `kb_imdb=on/off` to `start_detail` (matching the existing `kb=on/off` and `ignore_disk_floor=True` patterns at lines 297-302).

  **Patterns to follow:**
  - `--kb` / `--no-kb` thread-through pattern: `sheets_sweep.py:138-153` (run_contact_sheet) and `:266-307` (main).
  - `--ignore-disk-floor` boolean pass-through: just shipped at commit `a56b949`.

  **Test scenarios** (extending `scripts/tests/test_sweep.sh`):
  - **T11: --kb-imdb default on logged** — `python3 sheets_sweep.py --downloads <fixture> --dry-run` → assert log line contains `kb_imdb=on`.
  - **T12: --no-kb-imdb logged** — `python3 sheets_sweep.py --downloads <fixture> --dry-run --no-kb-imdb` → assert log line contains `kb_imdb=off`.

  **Verification:**
  - `bash scripts/tests/test_sweep.sh` PASS count increases by 2 from current ~22 to ~24.

- [ ] **Unit E: `.claude/skills/pirata-deck/SKILL.md` DOCTOR contract update**

  **Goal:** DOCTOR doesn't report drift after `--kb-imdb` lands.

  **Requirements:** R5.

  **Dependencies:** Unit B (flag must exist before contract is updated).

  **Files:**
  - Modify: `.claude/skills/pirata-deck/SKILL.md`

  **Approach:**
  - At `SKILL.md:163`, the locked flag list is: `--out --threshold --floor --target --cols --rows --width --workers --title --kb-export`. Append `--kb-imdb` to this list (preserving the existing space-separated, parens-wrapped, single-backtick formatting).
  - The DOCTOR check (which runs `python3 scripts/contact_sheet.py --help` and greps for each flag) will then verify all eleven flags including the new one.

  **Patterns to follow:**
  - The existing single-line backtick-wrapped flag list at the same line.

  **Test scenarios:**
  - **Test expectation: none** — pure documentation update. Verification is via the DOCTOR check itself running on the live skill (manually invoked via `/pirata doctor` or whatever the deck's contract-drift command is) and reporting `[OK]`.

  **Verification:**
  - Manual: `/pirata` deck DOCTOR row for CONTRACT shows `[OK]` after Unit B + Unit E land together. (Don't merge B without E or DOCTOR will report `[FAIL] sheet contract drift`.)

- [ ] **Unit F: Integration tests — `scripts/tests/test_contact_sheet_imdb.sh`**

  **Goal:** Hermetic end-to-end integration coverage of the `--kb-imdb` flag at the contact_sheet layer, exercising the full pipeline from raw input through manifest write.

  **Requirements:** R3, R6, R8 (the helper-level scenarios live in Unit A's test).

  **Dependencies:** Units A + B.

  **Files:**
  - Create: `scripts/tests/test_contact_sheet_imdb.sh`

  **Approach:**
  - Hermetic mktemp tmpdir following the pattern at `scripts/tests/test_kh_export.sh` and `test_imdb_lookup.sh`. Stub out the actual ffmpeg scene-detect call so the test runs in <30s (real scdet on a test fixture is slow; just generate a 10-frame stub manifest manually and feed contact_sheet a `--target 10` flag, OR mock at the subprocess level).
  - Test cases covering integration:
    - **T1: --kb-imdb on with real release name** — invoke contact_sheet with `--title "Dune.Part.Two.2024.2160p.UHD.BluRay.x265"` `--kb-imdb` `--kb-export <tmpdir>`; assert resulting per-movie JSON has `imdb.result="resolved"`, `imdb.tconst != null`, `title="Dune: Part Two"` or similar IMDb-resolved value, `year=2024`.
    - **T2: --no-kb-imdb produces historical shape** — same input but `--no-kb-imdb`; assert per-movie JSON does NOT contain `imdb` key, does NOT contain `filename` key, top-level `title` and `year` come from `args.title` / `parse_year_from_title`.
    - **T3: --kb-imdb on without --kb-export is a no-op** — invoke without `--kb-export`; assert no per-movie JSON written (existing behavior preserved); assert no IMDb resolution happens.
    - **T4: multi_tie release routes to fallback** — invoke with a release name known to multi-tie; assert `imdb.result="multi_tie"`, top-level title/year come from PTT, JSONL line appended to `logs/sweep_imdb_misses.log`.
    - **T5: db_unavailable handling** — temporarily move `imdb/imdb.db` aside (or create `imdb/.refresh.lock` empty file); invoke contact_sheet; assert `imdb.result="db_unavailable"`, top-level title/year come from PTT/regex, JSONL line in misses log, exit 0 (sheet pipeline doesn't crash on IMDb unavailability).
    - **T6: anime-like title naturally falls through** — invoke with `--title "[SubsPlease] Bocchi the Rock - 01 (1080p)"`; assert `imdb.result` is one of `no_match`, `below_threshold`, or `multi_tie` (any non-resolved); assert top-level title/year come from PTT cleanup; sheet pipeline succeeds.
    - **T7: re-run idempotency** — run T1 twice in succession; assert second run produces byte-identical per-movie JSON.

  **Patterns to follow:**
  - `scripts/tests/test_imdb_lookup.sh` for shell harness shape (assert function, summary block).
  - `scripts/tests/test_kh_export.sh` for hermetic tmpdir + cleanup discipline.

  **Verification:**
  - `bash scripts/tests/test_contact_sheet_imdb.sh` exits 0 with `PASS: 7/7` (or higher if scenarios get split).

- [ ] **Unit G: Operational verification + Mario Galaxy regression check**

  **Goal:** End-to-end validation: re-export the existing fixtures and confirm the bug that motivated this plan is fixed.

  **Requirements:** R7 (downstream side: verify export surfaces enriched fields).

  **Dependencies:** Units A + B + C + D + E + F.

  **Files:**
  - No code changes. Operational steps only.

  **Approach:**
  1. Run `python3 scripts/contact_sheet.py --kb-imdb --kb-export $(pwd)/kb --title <raw release name> --out /tmp/sheet-test <fixture-mkv-or-stub>` for the Mario Galaxy fixture (or a stub equivalent that reproduces the bug shape).
  2. Inspect `kb/per-movie/the-super-mario-galaxy-movie-2026.json` (real path, real overwrite) — assert `title="The Super Mario Galaxy Movie"` (or whatever IMDb returns; the point is NOT the slug literal), assert `year=2026`, assert `imdb.result="resolved"` (or document the actual outcome with reason).
  3. Run `python3 scripts/build_kh_export.py`; assert the regenerated `kb/kh-export/04-derived/per-movie/the-super-mario-galaxy-movie-2026.md` frontmatter contains `tconst:`, `genres:`, `rating_average:`; assert body contains `## IMDb metadata` section.
  4. Re-stage to knowledge-hub via the FIRE-v3 prompt at `docs/prompts/2026-04-26-kh-ingest-FIRE-v3.md` (operator-driven; not part of this plan's automation). Confirm smoke retrieve `mcp__knowledge-hub__retrieve(query="Super Mario Galaxy", kb_slugs=["pirata-kb"])` returns the Mario Galaxy chunk with score in the resolved-match range.

  **Patterns to follow:**
  - Operational verification convention from plan 005 Unit 6 (the original KH-export ingest verification).

  **Test scenarios:**
  - **Test expectation: none** — operational + manual regression pass. The structured tests in Units A, C, D, F already cover the automated coverage.

  **Verification:**
  - Mario Galaxy bug closed: `kb/per-movie/the-super-mario-galaxy-movie-2026.json` has IMDb-resolved title (or documented `result != "resolved"` reason).
  - Re-export idempotent: `bash scripts/tests/test_kh_export.sh` still passes after the second build run.
  - Re-ingest succeeds: KH catalog still has `pirata-kb` with ≥ 5 indexed docs after re-staging.

## System-Wide Impact

- **Interaction graph:** `sheets_sweep.py` → `contact_sheet.py` → (new) `imdb_kb_enrich.py` → `imdb_lookup.py` → `imdb/imdb.db`. Adding `imdb_kb_enrich` as a sibling helper keeps `contact_sheet` focused on frame extraction and lets future callers (e.g., a back-fill CLI) reuse the resolution logic without duplicating.
- **Error propagation:** `IMDbDBUnavailable` from `imdb_lookup` is caught at the `imdb_kb_enrich` layer and translated to `result="db_unavailable"`. Below that layer, the contact_sheet pipeline never sees IMDb-related exceptions. Above that layer, sweep / queue pipelines are completely unaware. No exceptions cross the `imdb_kb_enrich` boundary.
- **State lifecycle risks:** Manifest writes are atomic (temp + rename, existing pattern at `contact_sheet.py:341-342`); the new `imdb` block adds JSON keys but doesn't change the write protocol. Concurrent sweep + manual contact_sheet runs are race-free because each run writes to a different per-movie path under `kb/per-movie/`. Log writes to `logs/sweep_imdb_misses.log` use POSIX `O_APPEND` semantics (atomic for line-buffered writes < 4 KB).
- **API surface parity:** The `--kb-imdb` flag exists on both `contact_sheet.py` and `sheets_sweep.py` (R3 + R4). The DOCTOR contract (R5) checks both. Any future caller that invokes `contact_sheet.py` directly (manual debugging, ad-hoc runs) gets the flag too.
- **Integration coverage:** Unit F's hermetic test exercises the contact_sheet → imdb_kb_enrich → lookup → DB chain end-to-end with real DB access (not mocks), which is the only way to verify the score+threshold gate behaves correctly against the real corpus. Unit A's helper test does the same for the unit-isolated scenarios.
- **Unchanged invariants:** `kb/manifest.jsonl` shape unchanged (per-movie JSON path is the same; new fields ride inside the JSON without bumping any envelope schema). `kb/manifest.jsonl` MD5 will change on regeneration, but that's expected — it's a fresh ledger entry per release. `build_kh_export.py` byte-determinism guarantee preserved (same inputs → same outputs). `--kb-export` flag semantics unchanged. `imdb_lookup.py` and `imdb_ingest.py` unchanged. `/pirata` skill `STATUS` / `DOCTOR` panels unchanged except for the new flag in the CONTRACT row.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| PTT produces drifty title parsing on edge releases (e.g., titles with trailing year markers like `2001.A.Space.Odyssey.1968`) | Fallback path: when PTT extracts a title that doesn't pass IMDb confidence threshold, the helper logs the miss and the manifest carries `filename.ptt_title` for debugging. Unit A test covers this case. Calibration via real-data run after landing. |
| 15 % confidence threshold is too aggressive (suppresses too many valid resolutions) OR too loose (lets through wrong matches) | Threshold lives as a single module-level constant in `imdb_kb_enrich.py`. After Unit 3 runs against ~10 real releases, revisit. The PT-BR fixture (Unit 2) gave 7/7 ok at the lookup layer; the manifest-level miss rate after Unit 3 is the real signal. |
| `multi_tie` floods `logs/sweep_imdb_misses.log` for common titles (e.g., "The Office", "Friends") | The fallback path still produces a usable manifest (canonical title/year from PTT). Log growth is bounded by `log_rotate` policy from plan 004 Phase 2 (out of scope here, but the unbounded growth is the same problem the existing log already has — no regression). |
| IMDb DB refresh in flight blocks sweep enrichment | `IMDbDBUnavailable` handling at the helper layer (R6); sweep continues, manifest carries `result="db_unavailable"`, log records the miss. Re-running the sweep after refresh completes back-fills naturally on the next pass (manifests are regenerated each run by default). |
| `--kb-imdb` flag added to `contact_sheet.py` but DOCTOR contract not updated → drift `[FAIL]` | Units B and E land in lockstep (same commit or back-to-back commits); verification step in Unit G runs DOCTOR explicitly. |
| New `imdb` block in per-movie JSON breaks an existing consumer that doesn't know to ignore unknown keys | Only consumer today is `build_kh_export.py` (Unit C handles it). `kb/manifest.jsonl` envelope is unchanged. No external programs read per-movie JSONs (verified by grepping the workspace for `per-movie/.*\.json` references). If knowledge-hub's ingester chokes on the new fields, it's a kh-side bug, not a pirata regression — and the FIRE-v3 ingest verification in Unit G catches it before declaring success. |
| Plot field is documented as "always null" but the schema reserves it — risk of consumers depending on null-presence vs key-absence | Always emit the `plot` key with `null` value when `imdb.result="resolved"`; never omit. Distinguishes "we have the schema slot" from "we don't have IMDb resolution" cleanly. |

## Documentation / Operational Notes

- **PTT install** is a one-time prerequisite. Add a line to the `## Setup` section of pirata's project README (or create one if absent — but that's a separate doc-update task, not in scope here): `pip3 install parse-torrent-title rapidfuzz pillow` covers all three runtime deps in one command.
- **First-run cost:** the first sweep with `--kb-imdb` on against a backlog of 20+ releases will hit IMDb DB harder than before (one lookup per release × ~50ms per lookup = ~1 second of overhead). Negligible; documented for completeness.
- **Re-export trigger:** any Unit-3 change to a per-movie JSON requires a fresh `python3 scripts/build_kh_export.py` run + re-staging via FIRE-v3 prompt for the knowledge-hub side to reflect the changes. This is documented at `kb/kh-export/04-derived/README.md` already.
- **License carveout** for IMDb-derived fields (R8 of plan 004) is handled by the existing `kb/kh-export/04-derived/README.md` license stance section; no new doc work needed for Unit 3 specifically.

## Sources & References

- **Origin document:** [docs/plans/2026-04-24-004-feat-imdb-local-pirata-coupling-plan.md](2026-04-24-004-feat-imdb-local-pirata-coupling-plan.md) — Phase 0 + 1 plan, Unit 3 outline.
- **Unit 1 ship:** commit `fb4b4a4` — `feat(imdb): Unit 1 — TSV → SQLite ingest with WAL-safe atomic refresh`.
- **Unit 2 ship:** commit `8e31539` — `feat(imdb): Unit 2 — imdb_lookup.py with FTS5 + RapidFuzz tier separation`.
- **KH-export landing:** commit `6c9a13b` — `feat(kh-export): KB export surface for knowledge-hub ingest`.
- **First successful KH ingest:** 2026-04-26 SUCCESS-WITH-CAVEATS report from Codex — pirata-kb registered, 6 docs indexed, 3/3 smoke retrieve hits.
- **PTT package:** [github.com/dreulavelle/PTT](https://github.com/dreulavelle/PTT) — MIT, zero deps. Plan 004 Key Decisions cites this as the chosen filename parser.
- **knowledge-hub kb_discovery:** `/Users/vidigal/projects/knowledge-hub/src/knowledge_hub/kb_discovery.py` — read-only reference for the suffix whitelist + sub-folder convention.
- **Mario Galaxy bug evidence:** `kb/per-movie/the-super-mario-galaxy-movie-2026.json` (`title="the-super-mario-galaxy-movie-2026"`, `year=null` — fixed by this plan).
