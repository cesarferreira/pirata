---
title: IMDb Local Catalog × /pirata Skill Coupling (Phase 0 + 1)
type: feat
status: active
date: 2026-04-24
origin: docs/brainstorms/2026-04-24-imdb-local-pirata-coupling-requirements.md
---

# IMDb Local Catalog × /pirata Skill Coupling (Phase 0 + 1)

## Overview

Couple the local IMDb non-commercial dataset (`imdb/unnoficial/`, 9.3 GB / 12.4 M titles) to the `/pirata` workspace as an offline metadata layer. Phase 0 ships data + lookup infrastructure; Phase 1 enriches `kb/per-movie/*.json` with `tconst`-anchored fields (consumed by knowledge-hub MCP via `ingest_sync`) and adds TC-failover wiring to the `/pirata` skill. Phase 2 (IMDb-primary live-search reorder + cast browser + PT-BR rerun) is deferred and evidence-gated by a 30-day event log written in Phase 1.

## Problem Frame

The brainstorm seed was exploration — *"vi o dump e pensei: dá pra usar?"* — not a felt pain. Treating that honestly, this plan delivers the two pieces with concrete value:

1. **KB enrichment** — `kb/per-movie/*.json` is the workspace's compounding RAG asset (consumed by knowledge-hub MCP via `ingest_sync`). Replacing filename-parsed metadata with `tconst`-anchored fields (rating, runtime, genre, top cast, akas) gives the local KB structured anchors a retrieval layer can filter and rerank on.
2. **Reversible TC failover** — when TC degrades (observed `[DOWN]` / HTTP 403 once in current session), the skill currently loses all metadata. A local IMDb lookup + `pirata` fallback restores disambiguation without touching the happy path.

The architectural bet ("IMDb-primary always-on") is *not* attempted here. Phase 1 instruments measurement (`logs/skill_imdb_events.jsonl`); Phase 2 only opens if 30 days of evidence justify the lookup tax.

(see origin: `docs/brainstorms/2026-04-24-imdb-local-pirata-coupling-requirements.md`)

## Requirements Trace

- R1-R3 (Data Layer) — SQLite ingest from TSVs with atomic refresh + 25 GB pre-flight gate.
- R4-R6 (Lookup API) — `lookup_by_title` / `lookup_by_tconst` / `lookup_episodes` Python helper backed by FTS5; 3-tier ranking with locked composite score formula; PT-BR / EN / ES akas slice.
- R7 / R7b / R7c / R8 (Skill Integration) — TC-primary on happy path, IMDb engages only on TC failure / zero results, `RESOLVED` row in SHORTLIST when IMDb engaged, `[TC OFFLINE]` row when TC fails.
- R9 / R10 — anime fallback to pirata; music/soft/courses skip IMDb.
- R11a / R11b — text-only disambiguation prompt (no new TR-100 panel in Phase 1).
- R12-R14 — KB JSON enrichment in `scripts/contact_sheet.py` manifest builder; sweep no-match logged + read back in STATUS.
- R15 / R16 / R16b — DOCTOR rows; `.gitignore`; structured event log (Phase 2 evidence pipe).
- R-deferred-1, R-deferred-2, R-deferred-3 — Phase 2 hypotheses, not built here.

## Scope Boundaries

- Not building a web UI, REST API, or daemon — strictly local CLI / skill-driven.
- Not exposing full `title.principals` graph beyond top-5 cast; no full crew helpers in v1.
- Not shipping posters or stills (out of dataset).
- Not replacing TC as primary indexer of seeders / quality / HDR / release groups.
- Not auto-refreshing the dump — manual cadence; stale signal in DOCTOR + session-start stderr nag (no per-shortlist badge).
- Not reordering the live-search pipeline (IMDb-primary stays Phase-2 deferred).
- Not adding `/pirata cast <name>` or any person-search surface.
- Music, software, courses, ROMs, live events skip IMDb resolution entirely.

### Deferred to Separate Tasks

- **Phase 2 — IMDb-primary live search pipeline** (R-deferred-1): activated only if R16b event log replay supports it.
- **Phase 2 — Cast browser shortcut** (`/pirata cast <name>`, R-deferred-2): pulls in `lookup_cast` / `filmography` API + full TR-100 disambiguation panel framework.
- **Phase 2 — Silent PT-BR / ES rerun on weak TC matches** (R-deferred-3).

## Context & Research

### Relevant Code and Patterns

- `scripts/contact_sheet.py:244-350` — `export_kb()`. Manifest dict at `:313-325` is the injection point for R12. Atomic write via `tmp.replace(target)` at `:327-330` (existing pattern, mirror it).
- `scripts/contact_sheet.py:85-88` — `parse_year_from_title` regex. The new filename-extraction step in R13 extends this idea but uses a real parser library (PTT — see Key Decisions).
- `scripts/contact_sheet.py:353-375` — argparse block that the `/pirata` DOCTOR `CONTRACT` check parses (`SKILL.md:163`). Adding `--kb-imdb` / `--no-kb-imdb` flags here is a CONTRACT-touching change — DOCTOR's expected flag list must be updated in lockstep.
- `scripts/sheets_sweep.py:137-181` — `run_contact_sheet()`: only place that builds argv for `contact_sheet.py`. New `--kb-imdb` plumbing lands at the equivalent of `:152-154` and `:265-267` (mirroring existing `--kb` / `--no-kb` BooleanOptionalAction pattern).
- `scripts/sheets_sweep.py:72-81` + `:56-58` — `log()` + `sanitize()` repr-escape pattern. **Reuse `sanitize()` for both `logs/sweep_imdb_misses.log` and `logs/skill_imdb_events.jsonl`** to keep the existing log-injection defenses (validated by `test_sweep.sh:117-126`).
- `scripts/contact_sheet.py:21-22` + `scripts/sheets_sweep.py:25-26` — sys.path prefix drop guard (prevents `scripts/queue.py` from shadowing stdlib `queue`). **Every new Python file in `scripts/` that imports stdlib `queue` family must include the same guard.**
- `.claude/skills/pirata-deck/references/menu-style.md:394-417` — STATUS panel template. R14's two new rows land after `KB SIZE` (line 413), before `ADVICE` (line 415).
- `.claude/skills/pirata-deck/references/menu-style.md:421-443` — DOCTOR panel template. R15's IMDB block lands as a new section between SWEEP/CONTRACT/KB DIR group and ADVICE.
- `.claude/skills/pirata-deck/references/menu-style.md:447-462` — SHORTLIST panel. R7c's `RESOLVED` row + R8's `TC STATUS` row insert into the top metadata block (between `SHOWING` and the numbered results).
- `scripts/tests/test_kb_export.sh` and `scripts/tests/test_sweep.sh` — bash smoke-test patterns: `mktemp -d`, `trap cleanup EXIT`, local `assert "name" cmd...` PASS/FAIL counter, ffmpeg-lavfi-synthesized fixtures, adversarial fixtures (`--evil.mkv`, `\x1b[31m\n` log injection). All Phase 0 / Phase 1 tests follow this style.
- `kb/per-movie/who-framed-roger-rabbit-1988.json` — sample of current schema; new R12 fields slot into the existing `manifest` dict.

### Institutional Learnings

- `docs/solutions/` does not exist in this repo. No prior post-mortems for SQLite FTS5, RapidFuzz, IMDb ingest, or atomic file replacement. After this plan lands, consider `/ce-compound` to capture the gotchas.
- Adjacent prior plans (`docs/plans/2026-04-24-001/002/003-*.md`) establish the structural conventions used here.

### External References

- **SQLite FTS5** — `https://www.sqlite.org/fts5.html`. Confirmed: macOS Python 3.11+ stdlib `sqlite3` ships with FTS5 compiled in.
- **RapidFuzz** — `https://rapidfuzz.github.io/RapidFuzz/`. `process.extract(query, choices, scorer=fuzz.token_set_ratio, limit=5, score_cutoff=70)` is the batch entry point.
- **IMDb non-commercial datasets** — `https://datasets.imdbws.com/` (daily refresh; `\N` = NULL; `region` and `language` columns in `title.akas` are independent).
- **`parse-torrent-title` (PTT)** — `https://pypi.org/project/parse-torrent-title/`. Zero deps, MIT, active.
- **`os.replace()` atomicity on macOS APFS** — same-volume rename is atomic for crash recovery; **but SQLite WAL = 3 files; the safe protocol is `PRAGMA wal_checkpoint(TRUNCATE)` then `os.replace` of the single `.db` file** (see Key Decisions).

## Key Technical Decisions

- **FTS5 schema: contentless single virtual table for the FUZZY tier; B-tree indexes for the EXACT tier** — chosen over external-content FTS5 (requires INTEGER rowid; tconst is TEXT) and per-source separate tables (3× more index work, no shared BM25 ranking). FTS5 virtual table `ft_titles(title, title_source UNINDEXED, tconst UNINDEXED, tokenize='unicode61 remove_diacritics 2', prefix='2 3')` powers tier-2 fuzzy match. **Tier-1 exact match uses regular B-tree indexes** on `title_basics(primaryTitle COLLATE NOCASE)`, `title_basics(originalTitle COLLATE NOCASE)`, and `title_akas(title COLLATE NOCASE)` — FTS5 does NOT maintain B-tree indexes on its content columns for `=` equality; benchmarked at 141 ms / 2 M rows during pass-2 review, would be multi-second on the real corpus and miss the <50 ms p99 target. With B-tree indexes on the source tables, tier-1 equality is microseconds; FTS5 only carries the fuzzy tier where it earns its cost.
- **WAL-safe atomic refresh protocol (R3)** — build at `imdb/imdb.db.new`, run `PRAGMA wal_checkpoint(TRUNCATE)` on it, close all connections, then `os.replace('imdb/imdb.db.new', 'imdb/imdb.db')`. Single-file rename is atomic on APFS. **Sibling cleanup:** after successful swap, unlink any orphan `imdb/imdb.db.new-wal` / `imdb/imdb.db.new-shm` siblings (truncate doesn't delete them); also unlink stale `imdb/imdb.db-wal` / `imdb/imdb.db-shm` from any prior live readers since the new inode generates fresh siblings. **Pre-flight cleanup:** at ingest start, unlink any pre-existing `imdb/imdb.db.new*` from a prior killed run before building. **Concurrent reader guard:** `imdb_ingest.py` writes `imdb/.refresh.lock` (flock-style) for the build duration; `imdb_lookup.py` checks for it on every call and returns `IMDbDBUnavailable(reason="refresh_in_flight")` so the skill renders `[IMDB OFFLINE: refresh]` and the event log captures it cleanly. Single-process refresh remains the contract (no concurrent ingests), but the skill can keep running gracefully during refresh — no longer "out of scope".
- **Filename parser: PTT (`parse-torrent-title`)** — chosen over `guessit` (LGPLv3 + 4 transitive deps; slow maintenance) and custom regex (drift risk, only handles year today). PTT is zero-deps, MIT, mature, and parses title / year / season / episode / quality reliably. Installed as a global pip dep matching the Pillow convention. The `parse_year_from_title` regex at `contact_sheet.py:85-88` stays as a hard-coded simple fallback.
- **RapidFuzz install: global pip** — matches the existing Pillow convention (also a global install, no `requirements.txt`). Adopting `requirements.txt` or `pyproject.toml` would be a workspace convention change beyond this plan's scope.
- **Disambiguation composite score formula (locks RBP item #2 from origin doc):** `score = fuzz_ratio_0_to_100 × field_multiplier`, where `fuzz_ratio` is `100` for exact case-insensitive match and `RapidFuzz fuzz.token_set_ratio` for fuzzy match. Field multipliers: `primaryTitle=3.0`, `originalTitle=2.0`, `aka isOriginalTitle=1=1.8`, `aka regional translation=1.5`. `numVotes` desc breaks ties when scores tie within 0.5. Confidence threshold for forced disambiguation: top-1 score within 15 % of runner-up *and* both in tier 1 (exact-match tier).
- **License scope for KB sync** — knowledge-hub on Dante is single-user, local-only (per workspace identity). IMDb non-commercial license carveout for personal use applies. If `kb/` is ever served beyond Dante (cross-workspace, multi-tenant, or external sync), strip IMDb-derived fields or re-evaluate the license. Recorded as a Scope Boundary so the question doesn't get re-asked silently.
- **Akas slice: PT + EN + ES (no FR)** — narrowed from earlier brainstorm draft after pass-2 measurement showed the broader predicate captured ~32 M rows (3-4× over original disk-budget estimate). Predicate: `region IN ('BR','PT','ES','MX','AR') OR language IN ('pt','en','es') OR isOriginalTitle = 1`. FR revisited only if a real PT-FR mis-resolution surfaces.
- **Series cast aggregated at ingest, not sweep time** — R12 series-aggregation runs in `imdb_ingest.py` once and materializes a `series_top_cast(parent_tconst, top_5_nconsts JSON)` table. Sweep enrichment reads it directly; no runtime per-episode joins.
- **Disambiguation in Phase 1 is text-only** — no new TR-100 panel. The skill returns a single chat-line message asking the user to re-query with `<title> <year>` or `<title> <tconst>`. The full TR-100 disambiguation panel framework ships in Phase 2 alongside the cast browser, where it's earned.
- **SHORTLIST `RESOLVED` row format — locked rendering function in `imdb_lookup.py`** (single source of truth used by both skill and panel template). Suffix ` (YYYY) ttNNNNNN... · 0.XX` measures 24 chars; safe title length = 40 − 24 = 16 chars. Pseudo-Python: `def render_resolved(title, year, tconst, conf): suffix = f" ({year}) {tconst[:8]}... · {conf:.2f}"; budget = 40 - len(suffix); t = title if len(title) <= budget else title[:budget-1] + "…"; return t + suffix`. Concrete examples (all exactly 40 chars in data column):  `Dune: Part Two   (2024) tt1523967... · 0.97` (truncated to 16+1 = 17 with ellipsis), `Oppenheimer       (2023) tt1539877... · 0.99`. Earlier draft formula `len(title)+len(year)+15>40` was wrong by 9 chars; titles 17-21 chars overflowed silently.

## Open Questions

### Resolved During Planning

- **TC `imdb_id` parameter support** (RBP item from origin doc) — verified 2026-04-24 via `torrentclaw` MCP schema introspection; `search_content` does NOT accept `imdb_id`. R8 is title+year keyed always; the conditional "deterministic-join" branch was retired in pass-3 of the brainstorm.
- **KB enrichment consumer named** (RBP item from origin doc) — confirmed knowledge-hub MCP via `ingest_sync` (local on Dante). However, **the consumer's actual query behavior on the new fields is NOT yet verified** — see new RBP item below.
- **Composite score formula** (RBP item from origin doc) — locked above as `score = fuzz_ratio_0_to_100 × field_multiplier`.
- **20-item PT-BR fixture target** (RBP item from origin doc) — folded into Phase 0 success criteria as Unit 2's verification step. Pass threshold locked at ≥18/20 with documented exception class for known-edge titles (e.g., direct-translation primaryTitle vs transliteration); exception list closed at Phase 0 sign-off, additions are Phase-2 reopen.

### Resolve Before Phase 1

Phase 0 (Units 1, 2, 6's tests + state.json.example + README + gitignore) ships first and is unblocked. The following items must resolve before Phase 1 (Units 3, 4, 5) lands:

- [Affects R12 / Unit 3] **Verify knowledge-hub `ingest_sync` schema contract.** Call `mcp__knowledge-hub__retrieve` against an existing pirata KB doc with a hypothetical filter referencing `tconst` or `genres`. Output:
  - If knowledge-hub auto-picks new JSON fields and supports per-document filtering → R12 is justified as written; document the filter syntax in `imdb/README.md`.
  - If knowledge-hub treats per-movie JSON as opaque text → R12 ships as schema-only prep with `imdb_lookup_confidence` retained only as debug metadata; consider trimming fields the consumer demonstrably won't use (rating, votes, runtime if not filterable).
  - If knowledge-hub requires schema registration → add a Unit 4.5 to update knowledge-hub's schema in lockstep with Phase 1 land.
- [Affects R5 / R6] **Calibrate the 15 % confidence threshold against the 20-item PT-BR fixture.** The threshold is locked at 15 % as a default proposal but never tested. Run the fixture; surface cases where R5 forces vs auto-picks and verify the calls match intent. Adjust the percentage if it produces unexpected silent picks on known multi-tie cases (e.g., aka collisions like `O Iluminado` resolving across multiple films).

### Deferred to Implementation

- **FTS5 latency target measurement** — `<50 ms p99` is a Phase 0 success target. Confirmed feasible by external research, but the actual bm25 + RapidFuzz pipeline must be **measured** on the real ingested corpus before Phase 0 is durably done. If not met, planning-time options were already noted (drop trigram tokenizer, narrow akas predicate further); apply during implementation.
- **Bulk ingest implementation strategy** — `executemany` chunks vs `pandas.to_sql` chunks vs streaming `csv.reader` + chunked transactions. The `<10 min full re-ingest` budget on M-series Mac is the constraint. Decide based on profile during Unit 1.
- **IMDb DB unavailability detection in skill** — health-probe at skill init vs lazy try/except with cached failure flag. Unit 4 picks based on what's lightest in the skill execution model.
- **Log rotation policy** — both `logs/sweep_imdb_misses.log` and `logs/skill_imdb_events.jsonl` grow unboundedly. Rotation is deferred until Phase 2 evaluation; for Phase 1, the STATUS / DOCTOR read-back tolerates whole-file scans.
- **DOCTOR's age-check input** — `imdb/state.json.last_refresh_ts` (tracked, may exist without DB) vs `mtime(imdb/imdb.db)` (absent if DB never built). Default: prefer `state.json.last_refresh_ts` if present, fall back to `mtime`, FAIL if neither.

## Output Structure

```
imdb/
├── imdb.db                    # generated, gitignored
├── imdb.db.new                # transient build target, gitignored
├── imdb.db.prev               # one-gen rollback, gitignored
├── tmp/                       # download/staging area, gitignored
├── unnoficial/                # raw TSVs, gitignored, already on disk
├── state.json                 # tracked: last_refresh_ts, source_checksums, schema_version
└── README.md                  # tracked: layout description

scripts/
├── imdb_ingest.py             # NEW: TSV → SQLite ingest with WAL-safe atomic refresh
├── imdb_lookup.py             # NEW: lookup_by_title / lookup_by_tconst / lookup_episodes helper
├── pirata_evaluate.py         # NEW: Phase 1 → Phase 2 gate evaluator (reads skill_imdb_events.jsonl)
├── contact_sheet.py           # MODIFIED: KB enrichment in manifest builder
├── sheets_sweep.py            # MODIFIED: --kb-imdb / --no-kb-imdb pass-through
└── tests/
    ├── test_imdb_ingest.sh    # NEW: hermetic-tmpdir bash smoke test
    └── test_imdb_lookup.sh    # NEW: PT-BR fixture + score-formula assertions

logs/
├── sweep_imdb_misses.log      # NEW, gitignored: <iso-ts>\t<filename>\t<reason>
└── skill_imdb_events.jsonl    # NEW, gitignored: structured per-event JSONL

.claude/skills/pirata-deck/
├── SKILL.md                   # MODIFIED: STATUS / DOCTOR new rows; CONTRACT flag list
└── references/
    └── menu-style.md          # MODIFIED: STATUS / DOCTOR / SHORTLIST templates updated
```

This shows the expected shape; per-unit `**Files:**` sections are authoritative for what each unit creates or modifies.

## High-Level Technical Design

> *This illustrates the intended approach and is directional guidance for review, not implementation specification. The implementing agent should treat it as context, not code to reproduce.*

```mermaid
flowchart TD
    subgraph "Phase 0 — Data + Lookup"
        TSV[imdb/unnoficial/*.tsv] --> INGEST[scripts/imdb_ingest.py]
        INGEST --> DB[(imdb/imdb.db<br/>SQLite + FTS5)]
        DB --> LOOKUP[scripts/imdb_lookup.py<br/>lookup_by_title / lookup_by_tconst / lookup_episodes]
    end

    subgraph "Phase 1 — KB Enrichment"
        SWEEP[scripts/sheets_sweep.py] --> CS[scripts/contact_sheet.py<br/>manifest builder]
        CS -.--> LOOKUP
        CS --> KB[kb/per-movie/*.json<br/>tconst-anchored fields]
        CS -. miss .-> MISSLOG[logs/sweep_imdb_misses.log]
    end

    subgraph "Phase 1 — Skill Integration"
        USER[/pirata user query/] --> SKILL[/pirata skill]
        SKILL --> TC[torrentclaw MCP]
        TC -. ok .-> SHORTLIST[SHORTLIST panel]
        TC -. fail/zero .-> LOOKUP
        LOOKUP -. canonical title+year .-> PIRATA[pirata search CLI]
        PIRATA --> SHORTLIST
        SKILL --> EVENTLOG[logs/skill_imdb_events.jsonl]
    end

    subgraph "Phase 2 Evidence Pipe (deferred decision)"
        EVENTLOG --> EVAL[scripts/pirata_evaluate.py]
        EVAL -. 30d gate .-> P2DECISION{Phase 2 reopen?<br/>≥50 tc_call events AND<br/>tc_zero+imdb_recovered ≥10% AND<br/>≥5 of those would have been<br/>prevented by IMDb-primary}
    end

    DB -. status/doctor .-> STATUS[/pirata STATUS panel]
    DB -. status/doctor .-> DOCTOR[/pirata DOCTOR panel]
    MISSLOG -. read-back .-> STATUS
    EVENTLOG -. read-back .-> DOCTOR
```

The diagram shows the *flow* of the system. Boundary points to validate during review:

- **Phase 0 outputs are consumed by Phase 1 alone** — no skill changes in Phase 0. This makes Phase 0 independently shippable + verifiable before any /pirata behavior change.
- **The lookup helper is called from two places**: contact_sheet.py manifest builder (KB enrichment, R12) and the /pirata skill (TC failover, R7/R8). Same module, two consumers.
- **Two log files, two readers**: miss log feeds STATUS bucketing; event log feeds DOCTOR + the deferred Phase 2 evaluator. Both reuse `sheets_sweep.py:56-58` `sanitize()` for log-injection defense.

## Implementation Units

- [ ] **Unit 1: `scripts/imdb_ingest.py` — TSV → SQLite ingest with FTS5 + WAL-safe atomic refresh**

  **Goal:** A `--refresh` script that downloads fresh IMDb TSVs, builds `imdb/imdb.db` (new) with all required tables + FTS5 + materialized `series_top_cast`, runs `PRAGMA integrity_check`, and atomically replaces the live DB. Backed by a 25 GB pre-flight free-space gate and one-generation rollback at `imdb/imdb.db.prev`.

  **Requirements:** R1, R2, R3, R16 (write `imdb/state.json` after success).

  **Dependencies:** None (greenfield script; reads only `imdb/unnoficial/*.tsv` which is already on disk).

  **Files:**
  - Create: `scripts/imdb_ingest.py`
  - Create: `imdb/README.md` (describes layout)
  - Create: `imdb/state.json` (written by script; tracked in git as a stub on first run)
  - Test: `scripts/tests/test_imdb_ingest.sh`

  **Approach:**
  - Streaming TSV reader (csv module, `delimiter='\t'`, `quoting=QUOTE_NONE`); converts `\N` → `None` on load.
  - Single connection with WAL pragmas on the `.new` DB during build (see Key Decisions for the exact pragma block). Disable `journal_mode=WAL` *before* the swap by `wal_checkpoint(TRUNCATE)` so the live DB is single-file at swap time.
  - Tables: `title_basics`, `title_ratings`, `title_episode`, `title_crew`, `title_principals_top5` (filtered streaming top-5-per-tconst by `ordering`, asserts input is sorted by tconst else aborts loudly), `title_akas` (with the 3-language predicate from Key Decisions), `series_top_cast` (materialized once after `title_episode` + `title_principals_top5` are loaded).
  - FTS5 virtual table populated last from a JOIN: `INSERT INTO ft_titles(title, title_source, tconst) SELECT primaryTitle, 'primary', tconst FROM title_basics UNION ALL ... originalTitle, 'original' ... UNION ALL ... title, 'aka' FROM title_akas`. Single bulk insert.
  - Indexes on `(tconst)` for `title_basics`, `(tconst)` for `title_ratings`, `(parent_tconst)` for `title_episode`, `(parent_tconst)` for `series_top_cast`.
  - Pre-flight: `shutil.disk_usage(imdb_dir)` >= 25 GB free; otherwise abort with a printed reason.
  - Refresh sequence: download → build `imdb/imdb.db.new` → integrity_check → checkpoint → swap → write `imdb/state.json{last_refresh_ts, source_checksums, schema_version}` → optionally archive previous `imdb/imdb.db.prev`.
  - Reuse the sys.path prefix-drop guard from `contact_sheet.py:21-22` (necessary even though this script doesn't import `queue` directly — defensive habit per the workspace pattern).

  **Patterns to follow:**
  - `scripts/contact_sheet.py:21-22` — sys.path guard.
  - `scripts/sheets_sweep.py:72-81` — `log()` + ISO timestamps.
  - `scripts/sheets_sweep.py:56-58` — `sanitize()` for any user-visible filename in error messages.
  - `argparse` + `BooleanOptionalAction` style of `scripts/sheets_sweep.py:265-267`.
  - Atomic write pattern: `scripts/contact_sheet.py:327-330` (`tmp.replace(target)`).

  **Test scenarios:**
  - **Happy path:** Synthesize fixture TSVs (10-20 titles, 5 episodes, 30 principals rows, 50 akas rows) via `printf` with tab separators; run `python3 scripts/imdb_ingest.py --refresh --src <fixturedir> --dest <tmpdir>/imdb`; assert `<tmpdir>/imdb/imdb.db` exists, has correct row counts via `sqlite3 ... 'SELECT count(*) FROM ...'`, `state.json` contains `last_refresh_ts`.
  - **Happy path:** Same as above; assert `series_top_cast` table contains pre-computed rows for fixture series (one or two parent_tconsts).
  - **Edge case:** TSV has `\N` in a NULL slot → assert it's stored as `NULL` in SQLite, not the literal string `\N`.
  - **Edge case:** Akas filter test — fixture includes rows in regions {BR, PT, US, GB, ES, MX, AR, FR} and languages {pt, en, es, fr, de, ja}. Assert FR/DE/JA rows are dropped unless tagged `isOriginalTitle=1`.
  - **Error path:** Disk pre-flight — mock < 25 GB free → assert ingest exits non-zero with a "needs 25 GB free" message.
  - **Error path:** principals sort assumption — fixture includes interleaved tconsts (`tt001, tt002, tt001, tt003`) → assert ingest aborts loudly with a sort-violation message rather than silently miss the second tt001 batch.
  - **Error path:** integrity_check fails — corrupt the `.new` DB after build (write garbage) → assert refresh refuses to swap and live DB at `imdb/imdb.db` is unchanged.
  - **Integration:** Full refresh of a 100-title fixture → measure wallclock < 30 s (proxy for the <10 min real-corpus target); the test does not enforce real-corpus timing but flags 10×+ regressions.
  - **Integration:** Run twice in succession → assert `imdb/imdb.db.prev` exists after second run with the contents from the first run.

  **Verification:**
  - Running `python3 scripts/imdb_ingest.py --refresh` on the real `imdb/unnoficial/` produces `imdb/imdb.db` with non-zero row counts in all expected tables.
  - `sqlite3 imdb/imdb.db 'PRAGMA integrity_check'` returns `ok`.
  - `imdb/state.json` parses and contains all three fields.

- [ ] **Unit 2: `scripts/imdb_lookup.py` — Python helper module backed by FTS5 + RapidFuzz**

  **Goal:** Expose `lookup_by_title(query, year=None, kind=None) -> list[Match]`, `lookup_by_tconst(tconst) -> Title | None`, `lookup_episodes(parent_tconst, season=None) -> list[Episode]` with locked composite score formula and 3-tier ranking. Hits <50 ms p99 on the real corpus.

  **Requirements:** R4, R5, R6 (PT-BR / EN / ES akas transparency). The `lookup_cast` / `filmography` API is **not** built (deferred to Phase 2 / R-deferred-2).

  **Dependencies:** Unit 1 (DB must exist); RapidFuzz pip-installed globally.

  **Files:**
  - Create: `scripts/imdb_lookup.py`
  - Test: `scripts/tests/test_imdb_lookup.sh`

  **Approach:**
  - Module-level connection cache (open once, reuse across calls within a process; close at exit). Connect read-only via `sqlite3.connect(f'file:{db_path}?mode=ro', uri=True)`.
  - Tier 1 (exact match): `SELECT tconst, title, title_source FROM ft_titles WHERE title = ? COLLATE NOCASE` — direct equality, no FTS5 query needed for this tier.
  - Tier 2 (fuzzy match): FTS5 `MATCH` with prefix syntax → top-N candidates → RapidFuzz `process.extract` post-pass with `scorer=fuzz.token_set_ratio, limit=10, score_cutoff=70`.
  - Apply `year` filter (±0 default) and `titleType` filter to candidates.
  - Composite score per Key Decisions: `score = fuzz_ratio × field_multiplier`. `numVotes` desc breaks ties within 0.5 score range.
  - Confidence threshold: if top-1 score within 15 % of runner-up *and* both in tier 1, return both as a multi-tie (caller raises a disambiguation prompt; never silent auto-pick).
  - `lookup_by_tconst` is a single SQL JOIN: `title_basics × title_ratings × title_akas (LEFT JOIN, GROUP BY tconst, JSON aggregation by language) × series_top_cast (LEFT JOIN where titleType matches series)`.
  - `lookup_episodes` is a single SQL: `SELECT tconst, seasonNumber, episodeNumber FROM title_episode WHERE parentTconst = ? AND (? IS NULL OR seasonNumber = ?) ORDER BY seasonNumber, episodeNumber`.

  **Patterns to follow:**
  - Type hints + `from __future__ import annotations` per the workspace style.
  - Plain dataclass or `TypedDict` for `Match`, `Title`, `Episode` return types — no Pydantic dep.
  - Module-level functions, not a class — matches `scripts/contact_sheet.py` flat style.

  **Test scenarios:**
  - **Happy path:** `lookup_by_title("Dune", year=2021)` → returns at least tt1160419 (Dune 2021), score above any alternative.
  - **Happy path (PT-BR aka):** `lookup_by_title("Duna: Parte Dois")` → returns tt15239678 with canonical `primaryTitle="Dune: Part Two"` (verifies the akas index works).
  - **Happy path (year filter):** `lookup_by_title("Dune", year=1984)` → returns tt0087182 (Dune 1984), not the 2021 version.
  - **Happy path (titleType):** `lookup_by_tconst("tt15239678")` → returns runtime, rating, top_cast[5] — series_top_cast is NOT consulted because titleType is `movie`, not `tvSeries`.
  - **Happy path (series):** `lookup_by_tconst("<series tconst>")` → top_cast[5] comes from `series_top_cast` table, not `title_principals_top5`.
  - **Edge case (multi-tie):** `lookup_by_title("Dune")` (no year) → if Dune 1984 and Dune 2021 both score in tier 1, returns both with a multi-tie flag set. Caller is expected to disambiguate.
  - **Edge case (zero match):** `lookup_by_title("aslkdjflaskdjflasdjklasjdflk")` → returns empty list.
  - **Edge case (low confidence):** `lookup_by_title("Dunne", year=2024)` → fuzzy match produces tt15239678 in tier 2; score reflects confidence < threshold.
  - **Error path:** `imdb/imdb.db` missing → module import succeeds but first lookup raises a clear `IMDbDBUnavailable` exception.
  - **Edge case (akas language):** `lookup_by_title("Cidade de Deus")` → matches tt0317248 via PT akas; canonical `primaryTitle="City of God"`.
  - **Performance check:** Run `lookup_by_title` 1000 times against the real DB; assert p99 < 50 ms (logged, not a hard test gate at this stage).
  - **20-item PT-BR fixture validation** (RBP item from origin doc, folded into Unit 2): all of (Duna, Oppenheimer, Interestelar, Cidade de Deus, Tropa de Elite, Bacurau, Ainda Estou Aqui, plus 13 more chosen by user) resolve to correct tconst with confidence ≥ 80. The fixture list is committed at `scripts/tests/fixtures/imdb_pt_br_20.txt` (slug → expected tconst). Output of this run drives the RapidFuzz threshold and Phase 1's enrichment-rate baseline target.

  **Verification:**
  - All 20 PT-BR fixture titles resolve to expected tconsts.
  - `python3 -c 'from scripts.imdb_lookup import lookup_by_title; print(lookup_by_title("Dune", year=2021)[:1])'` returns a sane row in <100 ms (cold cache).

- [ ] **Unit 3: KB enrichment in `scripts/contact_sheet.py` manifest builder + sweep `--kb-imdb` flag**

  **Goal:** When `--kb-imdb` is set, `contact_sheet.py`'s `export_kb()` extracts (title, year) from the input filename via PTT, calls `lookup_by_title`, and merges `tconst, imdb_rating, imdb_votes, genres[], runtime_minutes, top_cast[5], akas{pt, en, es}, imdb_resolved_at, imdb_lookup_confidence` into the per-movie JSON manifest before atomic write. On no-match, append a `<iso-ts>\t<filename>\t<reason>` line to `logs/sweep_imdb_misses.log` and write the JSON without IMDb fields. The sweep wires `--kb-imdb` / `--no-kb-imdb` through to `contact_sheet.py`.

  **Requirements:** R12, R13.

  **Dependencies:** Unit 2 (lookup helper); PTT pip-installed globally.

  **Files:**
  - Modify: `scripts/contact_sheet.py` (manifest builder at `:313-325`; argparse at `:353-375` adds `--kb-imdb` / `--no-kb-imdb` BooleanOptionalAction).
  - Modify: `scripts/sheets_sweep.py` (mirror existing `--kb` plumbing at `:152-154` + `:265-267`).
  - Modify: `.claude/skills/pirata-deck/SKILL.md` (CONTRACT flag list at `:163` — adds `--kb-imdb` / `--no-kb-imdb` to the expected flag set in lockstep so DOCTOR doesn't report drift; coupling lives in this unit, NOT split across Unit 3 + Unit 5).
  - Test: `scripts/tests/test_kb_export.sh` (extend existing test with `--kb-imdb` cases).

  **Approach:**
  - In `contact_sheet.py`, refactor or extend `parse_year_from_title` (`:85-88`) to call PTT first for richer parsing (title + year + season + episode); keep the simple regex as fallback if PTT fails or returns no year.
  - The manifest dict at `:313-325` gets a new conditional block (only when `--kb-imdb` is set): `if args.kb_imdb: imdb = lookup_by_title(extracted_title, year=parsed_year); if imdb: manifest.update({"tconst": imdb.tconst, "imdb_rating": imdb.rating, ...})`. If `imdb` is None or below threshold, append a miss-log line.
  - Miss-log writer reuses `sheets_sweep.py:56-58` `sanitize()` for filename safety. Format: `<iso-ts>\t<sanitize(source_file)>\t<reason>` where reason ∈ `{no_title, no_year, fuzzy_below_threshold, multi_tie_unresolved, lookup_unavailable}`.
  - Sweep pass-through: `--kb-imdb` is a tri-state (on, off, default-off) via `BooleanOptionalAction`. The default is **off** in Phase 1 — opt-in until the 20-item baseline is run and a target is set. After baseline, can flip default to on.
  - **Critical: SKILL.md `CONTRACT` row** (`:163` of `SKILL.md`) parses `contact_sheet.py --help`. The new `--kb-imdb` / `--no-kb-imdb` flags must be added to the expected flag list in the SKILL.md DOCTOR check, otherwise DOCTOR will report `[FAIL] sheet contract drift`.

  **Patterns to follow:**
  - `scripts/contact_sheet.py:327-330` — atomic write (mirror exactly; do not break this).
  - `scripts/sheets_sweep.py:265-267` — BooleanOptionalAction style.
  - `scripts/sheets_sweep.py:152-154` — pass-through into argv for `contact_sheet.py`.

  **Test scenarios:**
  - **Happy path:** Run `contact_sheet.py --kb-export --kb-imdb` on a fixture filename `Dune.Part.Two.2024.2160p.mkv` → assert per-movie JSON contains `tconst="tt15239678"`, `imdb_rating > 0`, `top_cast` non-empty, `imdb_lookup_confidence >= 0.8`.
  - **Happy path (PT-BR):** filename `Cidade.de.Deus.2002.BluRay.mkv` → assert `tconst="tt0317248"` (resolves via PT akas).
  - **Edge case (no year in filename):** filename `Random.Show.S01E03.mkv` → PTT extracts title but year is None → IMDb lookup runs without year filter; if multi-tie occurs, miss-log gets `multi_tie_unresolved`, JSON written without tconst.
  - **Edge case (`--no-kb-imdb`):** Same fixture → JSON has no `tconst` field at all (graceful skip, not enriched, no miss-log entry).
  - **Edge case (`--kb-imdb` + DB missing):** Run after `rm imdb/imdb.db` → JSON written without IMDb fields, miss-log line includes `reason=lookup_unavailable`. Sweep run does not crash.
  - **Error path (corrupt filename):** filename `\x1b[31m\n.mkv` (log injection attempt) → miss-log line is sanitized; no ANSI escape leaks into the log file (verified by `grep -P '\x1b' logs/sweep_imdb_misses.log` returning zero matches).
  - **Integration (sweep pass-through):** `sheets_sweep.py --once --kb-imdb` on a synthesized fixture release → assert the spawned `contact_sheet.py` argv includes `--kb-imdb`; assert the final per-movie JSON has IMDb fields.
  - **Integration (atomic write):** Kill the process during `export_kb` → assert no half-written JSON exists; either old file or new file, never garbled.
  - **Integration (CONTRACT check):** `python3 scripts/contact_sheet.py --help` includes `--kb-imdb` and `--no-kb-imdb`; the SKILL.md DOCTOR check lists these in the expected flag set (so `[OK]` not `[FAIL] sheet contract drift`).

  **Verification:**
  - Existing `test_kb_export.sh` continues to pass (no regression on the non-IMDb path).
  - New `--kb-imdb` cases in `test_kb_export.sh` all pass.
  - A real sweep run on `./downloads/` with `--kb-imdb` produces enriched JSONs for at least one release (smoke check, not the full 90 % target).

- [ ] **Unit 4: `/pirata` skill TC-failover wiring + event log + text disambiguation**

  **Goal:** Update `.claude/skills/pirata-deck/SKILL.md` so `/pirata` movie / series / doc workflows engage IMDb lookup only on TC failure / zero results (R7), surface `[TC OFFLINE]` and `RESOLVED` rows in SHORTLIST (R7c, R8), use text-only disambiguation when confidence is low (R11a, R11b), and write a per-engagement event line to `logs/skill_imdb_events.jsonl` (R16b).

  **Requirements:** R7, R7b, R7c, R8, R9, R10, R11a, R11b, R16b.

  **Dependencies:** Unit 2 (lookup helper).

  **Files:**
  - Modify: `.claude/skills/pirata-deck/SKILL.md` (workflow section: movie / series / doc dispatch; failure handling; disambiguation prompt template).

  **Approach:**
  - The skill is markdown + workflow prompts, not Python. The "wiring" is workflow guidance the skill follows when invoked: (a) call TC `search_content` with user query; (b) on error / zero results, call `python3 -c 'from scripts.imdb_lookup import lookup_by_title; ...'` (or a thin wrapper invokable from bash) to resolve canonical title + year; (c) call `pirata search` with canonical title + year; (d) render shortlist with `RESOLVED` and `TC STATUS` rows.
  - **Event log writer:** Each skill invocation that calls TC appends one JSON line to `logs/skill_imdb_events.jsonl` with `{ts, event, query, query_lang_guess, tc_status, imdb_engaged, resolved_tconst, resolved_confidence, duration_ms}`. `event ∈ {tc_call, tc_zero_results, tc_error, imdb_fallback_fired, disambig_text_shown, disambig_user_recovered}`. Implementing this in pure SKILL.md prose is fragile; provide a thin `scripts/skill_log.py` helper (single function, ~20 lines) the skill invokes via `python3 scripts/skill_log.py <event> <json-payload>`.
  - **Text disambiguation prompt template** (R11a): when the lookup returns a multi-tie, the skill emits a fixed-format chat message:
    > Match ambíguo pra "<query>": <title1> (<year1>) <tconst1> · <votes1> · <title2> (<year2>) <tconst2> · <votes2>. Re-roda com ano (ex: `<title> <year>`) ou tconst (ex: `<title> <tconst>`).

    No new TR-100 panel — single chat-line prose.
  - **PT-BR / ES rerun is NOT implemented** (deferred to Phase 2 R-deferred-3). The skill counts PT-BR mis-resolutions in the event log via `query_lang_guess` so Phase 2 can decide.
  - **TC `imdb_id` parameter is NOT used** — confirmed unsupported by `torrentclaw` MCP schema. TC calls are always `query=<title>` + `year=<year>`.

  **Patterns to follow:**
  - Existing SKILL.md workflow tables (movie / series / doc rows) — extend, don't rewrite.
  - Routing-decision verbiage matches the existing tone ("se ambíguo, pergunte numa frase só, não por rodadas" — `SKILL.md:97`).
  - `scripts/sheets_sweep.py:56-58` `sanitize()` — `skill_log.py` reuses it for the `query` field to defend against log injection.

  **Test scenarios:**
  - **Happy path (TC online):** `/pirata m oppenheimer` → TC returns shortlist; SHORTLIST has no `RESOLVED` row, no `TC STATUS` row; event log line has `event=tc_call, tc_status=ok, imdb_engaged=false`.
  - **Happy path (TC fail):** Block TC at the MCP boundary (mock); `/pirata m oppenheimer` → IMDb resolves canonical "Oppenheimer (2023)"; pirata search runs with that; SHORTLIST has `RESOLVED │ Oppenheimer (2023) tt15398776 · 0.99` and `TC STATUS │ [TC OFFLINE] · fallback: pirata`; event log has `event=imdb_fallback_fired`.
  - **Happy path (TC zero results):** TC returns empty (some obscure title) → IMDb resolves; same fallback flow as TC fail; event log distinguishes `event=tc_zero_results` from `event=tc_error`.
  - **Edge case (multi-tie):** `/pirata m dune` → IMDb returns Dune 1984 + Dune 2021 in tier 1 within 15 % score → skill emits text-disambig message; event log has `event=disambig_text_shown`. User responds `dune 2021` → next invocation resolves cleanly; `event=disambig_user_recovered`.
  - **Edge case (DB missing):** `imdb/imdb.db` absent → on first TC failure, skill bypasses to pre-coupling pipeline (no IMDb engagement); SHORTLIST has `[IMDB OFFLINE]` badge; event log line has `event=tc_error, imdb_engaged=false, reason=db_unavailable`.
  - **Edge case (zero seeds in both TC and pirata):** TC returns empty + pirata returns empty + IMDb resolved → SHORTLIST shows `RESOLVED` row + `SEEDS │ none — try later` (distinct from `[TC OFFLINE]`); user knows the title was identified.
  - **Anime path:** `/pirata a "Bocchi the Rock"` → raw `pirata search` (no IMDb engagement on happy path per R9); event log line `event=tc_call` not emitted (anime doesn't go through TC). Verify the anime workflow signature is unchanged.
  - **Music / soft path:** `/pirata 4 "Radiohead OK Computer"` → no IMDb engagement, no event log line. R10 enforced.
  - **Log injection:** `/pirata m "X\x1b[31m"` → event log line is sanitized; ANSI escape doesn't leak.
  - **Integration:** Event log file accumulates across multiple `/pirata` invocations within a session; line count matches invocation count.

  **Verification:**
  - Manually run `/pirata m oppenheimer` (TC up) and `/pirata m oppenheimer` with TC blocked; verify SHORTLIST visual difference and event log lines.
  - `wc -l logs/skill_imdb_events.jsonl` increases by exactly 1 per non-anime / non-music invocation.

- [ ] **Unit 5: TR-100 panel template updates — STATUS, DOCTOR, SHORTLIST**

  **Goal:** Update `.claude/skills/pirata-deck/references/menu-style.md` and `.claude/skills/pirata-deck/SKILL.md` so STATUS, DOCTOR, and SHORTLIST panels render the new IMDb-related rows correctly within the 55-char TR-100 grid.

  **Requirements:** R14 (STATUS), R15 (DOCTOR), R7c / R8 SHORTLIST rows.

  **Dependencies:** Units 1, 2, 3, 4 (so the data sources for the rows exist).

  **Files:**
  - Modify: `.claude/skills/pirata-deck/references/menu-style.md` (sections 394-417 STATUS, 421-443 DOCTOR, 447-462 SHORTLIST).
  - Modify: `.claude/skills/pirata-deck/SKILL.md` (DOCTOR/STATUS check list at `:144-152` and `:154-165`; CONTRACT flag list at `:163`).

  **Approach:**
  - **STATUS additions** (after `KB SIZE` at `menu-style.md:413`): two new rows, total still 55 chars per row.
    ```
    │ KB ENRICHED│ <enriched>/<total> titles              │
    │ KB MISSES  │ <n> since refresh · top: <reason>      │
    ```
    Concrete row spec measured to 55 chars. Empty values still render the row (no conditional hiding) — keeps the grid stable.
  - **DOCTOR additions** (new section between SWEEP/CONTRACT/KB DIR and ADVICE): six new rows.
    ```
    │ IMDB DB    │ imdb/imdb.db · age 12d            [OK] │
    │ IMDB ROWS  │ ttl=12.4M · rat=1.4M · aka=10.8M  [OK] │
    │ IMDB LANGS │ pt=480k · en=2.8M · es=420k       [OK] │
    │ IMDB DEPS  │ ptt=ok · rapidfuzz=ok             [OK] │
    │ KB MISSES  │ 47 since refresh · top: fuzz_low  [OK] │
    │ IMDB EVENTS│ 23 events · 7d · top: imdb_fb     [OK] │
    ```
    Each row exactly 55 chars; data column abbreviated to fit. Status badge column right-aligned. **Abbreviation table (locked, used by `skill_log.py` + miss-log writer + DOCTOR reader; max 12 chars per abbreviated label):**

    | Event type (Unit 4) | Abbrev | Reason code (Unit 3) | Abbrev |
    |---|---|---|---|
    | `tc_call` | `tc_call` | `no_title` | `no_title` |
    | `tc_zero_results` | `tc_zero` | `no_year` | `no_year` |
    | `tc_error` | `tc_error` | `fuzzy_below_threshold` | `fuzz_low` |
    | `imdb_fallback_fired` | `imdb_fb` | `multi_tie_unresolved` | `multi_tie` |
    | `disambig_text_shown` | `disambig` | `non_latin` | `non_latin` |
    | `disambig_user_recovered` | `disambig_ok` | `lookup_unavailable` | `db_miss` |

    Both `IMDB EVENTS` and `KB MISSES` rows display only the most-frequent abbreviated label after `top:`; full reason code remains in the source log. The new `IMDB DEPS` row (one of the six) catches the PTT preflight gap surfaced in pass-2 review.
  - **SHORTLIST additions** (insert into top metadata block, between `SHOWING` and the numbered results):
    ```
    │ RESOLVED   │ Dune: Part Two (2024) tt1523... · 0.97 │
    │ TC STATUS  │ [TC OFFLINE] · fallback: pirata        │
    ```
    `RESOLVED` row only renders when IMDb engaged (R7c: omitted on happy-path TC search). `TC STATUS` only when TC failed. Title is right-truncated with `…` if `len(title)+len(year)+15 > 40` so the row stays 55 chars. tconst abbreviated as `tt1523...` (first 6 chars + `...`).
  - **CONTRACT flag list update** (`SKILL.md:163`): add `--kb-imdb`, `--no-kb-imdb` to the expected flag set so DOCTOR doesn't report drift.
  - **Numbers in DOCTOR template are placeholders** — actual row values come from runtime queries. The `IMDB ROWS` `aka=10.8M` example is a guess based on the narrowed predicate; the test in Unit 1 reports the actual count, and that becomes the post-ingest example used here.

  **Patterns to follow:**
  - `references/menu-style.md:25` — column-split arithmetic (12+1+40+1+1 = 55).
  - `references/menu-style.md:101-110` — status badge conventions (`[OK]`, `[STALE]`, `[FAIL]`, `[WARN]`).
  - `references/menu-style.md:74-78` — label column 12-char convention.

  **Test scenarios:**
  - Test expectation: none — this is a documentation/template change. No behavioral logic in `menu-style.md`. Validation is char-counting and visual review.
  - Char-count validation done as part of code review: every new row literally `wc -L`'d (longest line) ≤ 55.
  - Indirect test: Unit 4's "Happy path (TC fail)" scenario verifies the SHORTLIST renders the new rows correctly via skill invocation.

  **Verification:**
  - `awk '{ if (length > 55) print NR ": " length " " $0 }' .claude/skills/pirata-deck/references/menu-style.md` returns nothing.
  - Manually invoke `/pirata 9` (STATUS) and `/pirata 10` (DOCTOR) after Phase 0 + 1 land; verify the new rows render correctly and don't break the existing grid.

- [ ] **Unit 6: Operations — `.gitignore`, `imdb/state.json` stub, `imdb/README.md`, `scripts/pirata_evaluate.py`, smoke tests**

  **Goal:** Wire all the side-quest deliverables: gitignore additions, the Phase 2 gate evaluator, the `imdb/` directory layout doc, and one consolidated bash smoke test.

  **Requirements:** R16, R16b reader (`scripts/pirata_evaluate.py`).

  **Dependencies:** Unit 4 (event log format must be locked).

  **Files:**
  - Modify: `.gitignore`
  - Create: `imdb/state.json` (stub; written by Unit 1 ingest at runtime, but a stub commits the convention)
  - Create: `imdb/README.md`
  - Create: `scripts/pirata_evaluate.py` (Phase 2 gate evaluator)
  - Create: `scripts/skill_log.py` (thin event-log writer used by Unit 4)
  - Create: `scripts/tests/test_imdb_ingest.sh` (Unit 1's test, listed here for visibility — actually owned by Unit 1)
  - Create: `scripts/tests/test_imdb_lookup.sh` (Unit 2's test, listed here for visibility — actually owned by Unit 2)

  **Approach:**
  - `.gitignore` additions (per R16). Switched to `imdb/imdb.db*` glob to catch ALL DB-related siblings including the `.db.new-wal`, `.db.new-shm`, `.db.prev-wal`, etc. that the original per-suffix list missed:
    ```
    # imdb dataset (gitignored)
    imdb/unnoficial/
    imdb/tmp/
    imdb/imdb.db*
    imdb/state.json
    imdb/.refresh.lock
    # imdb runtime logs (gitignored)
    logs/skill_imdb_events.jsonl
    logs/sweep_imdb_misses.log
    ```
    Tracked: `imdb/state.json.example`, `imdb/README.md`.
  - `imdb/state.json.example` stub: `{"last_refresh_ts": null, "source_checksums": {}, "schema_version": 1}` — tracked, never overwritten, documents the shape for fresh clones. Runtime `imdb/state.json` is gitignored and written by Unit 1 ingest. Switched from earlier draft (tracked `state.json` stub overwritten at runtime) per pass-2 consensus: that pattern produced perpetually dirty `git status` after every refresh.
  - `imdb/README.md`: 30-line doc describing layout, refresh procedure (`scripts/imdb_ingest.py --refresh`), schema version, and the license carveout note (knowledge-hub on Dante, single-user, local-only).
  - `scripts/skill_log.py`: a single function `append_event(event_log_path, payload)` that opens the file in append mode, writes one JSON line, and `sanitize`s the `query` field. Reuses the sanitize helper from `sheets_sweep.py:56-58` (extract to a shared `scripts/_log_safety.py` if reused in 3+ places; for now duplicate with a `# duplicated from sheets_sweep.py:56-58` comment).
  - `scripts/pirata_evaluate.py`: reads `logs/skill_imdb_events.jsonl`; produces both numerator/denominator + a verdict so the user can sanity-check rather than just consume a label. **Locked thresholds** (per origin doc Phase 2 gate): `N_min = 50` total `tc_call` events over the window. Below `N_min` → `INSUFFICIENT-DATA` always. At-or-above `N_min`: verdict `REOPEN` only when BOTH `(tc_zero_results AND imdb_fallback_produced_hits) ≥ 10 % of total tc_call` AND `replaying the failed queries against imdb_lookup confirms ≥ 5 distinct queries that would have been prevented by IMDb-primary disambiguation` (replay re-runs each `query` from a `tc_zero_results` event through `lookup_by_title` and counts the cases where IMDb returned a confidence-decisive single match — i.e., would have routed canonical title to TC successfully under Phase 2). Below `N_min` or below either AND-clause → `STAY-CLOSED`. The 2-10 % zone defaults to `STAY-CLOSED` (burden of proof on reopening). Output format: numerator/denominator on each clause + verdict label.

  **Patterns to follow:**
  - `scripts/sheets_sweep.py:56-58` — `sanitize()` for log writers.
  - `scripts/contact_sheet.py:21-22` — sys.path guard for any new script in `scripts/`.
  - Existing `.gitignore` style.

  **Test scenarios:**
  - **Happy path (gitignore):** `git status` after a real ingest → `imdb/imdb.db` not staged; `imdb/state.json` shows as modified.
  - **Happy path (state.json):** `python3 -c 'import json; print(json.load(open("imdb/state.json")))'` → returns valid dict with all three fields.
  - **Happy path (skill_log.py):** Call `append_event` 100 times → `logs/skill_imdb_events.jsonl` has exactly 100 lines, each parses as JSON.
  - **Edge case (skill_log injection):** Call with payload containing `\x1b[31m\n` → output line is sanitized; no ANSI escape in the file.
  - **Happy path (pirata_evaluate.py):** Synthesize 30 days of fake events (15% TC failure rate); run evaluator → outputs `REOPEN`. Synthesize 1% TC failure rate → outputs `STAY-CLOSED`. Synthesize <10 events total → outputs `INSUFFICIENT-DATA`.
  - **Integration:** Run all `scripts/tests/test_imdb_*.sh` end-to-end → all pass.

  **Verification:**
  - `git diff --check` shows no lint issues on `.gitignore`.
  - `python3 -c 'import scripts.skill_log; scripts.skill_log.append_event("/tmp/test.jsonl", {"event": "tc_call", "query": "test"})'` writes one valid JSON line.
  - `python3 scripts/pirata_evaluate.py --since 30d` runs against the real (initially small) event log and reports a verdict.

## System-Wide Impact

- **Interaction graph:** `scripts/contact_sheet.py` gains a new dependency on `scripts/imdb_lookup.py`; `scripts/sheets_sweep.py` gains a new pass-through flag. The `/pirata` skill gains a new dependency on `scripts/imdb_lookup.py` (via `scripts/skill_log.py` thin wrapper). knowledge-hub MCP `ingest_sync` consumes the enriched `kb/per-movie/*.json` (no protocol change — same JSON files, more fields).
- **Error propagation:** Lookup helper raises a single `IMDbDBUnavailable` exception when the DB is missing; both contact_sheet.py (graceful skip + miss log) and the skill (`[IMDB OFFLINE]` badge) catch this. Other lookup errors (corrupt DB, FTS5 query syntax) propagate as-is and abort the calling context — no silent failures.
- **State lifecycle risks:** R3's WAL atomic-refresh protocol is the highest-risk surface. If implemented incorrectly (e.g., renaming `imdb.db` while readers hold connections), readers see a DB whose WAL/SHM siblings don't match → corruption. Mitigation: enforce single-process refresh; document explicitly in `imdb/README.md`; integrity check after build before swap.
- **API surface parity:** `contact_sheet.py` CLI flags are read by SKILL.md DOCTOR `CONTRACT` check. Adding `--kb-imdb` requires updating that check in lockstep (Unit 5). No other external consumers of script CLI flags.
- **Integration coverage:** Unit tests alone won't prove (a) the WAL atomic refresh actually works under macOS APFS — needs a real refresh smoke run; (b) the SKILL.md prose actually causes the right MCP/CLI calls — needs a manual `/pirata m oppenheimer` invocation post-implementation; (c) knowledge-hub `ingest_sync` correctly indexes the new `tconst` field — verifiable via `mcp__knowledge-hub__retrieve` with a tconst-anchored query after sync. These three are explicit verification steps in Units 1, 4, and (deferred sync verification) post-merge.
- **Unchanged invariants:** Everything in `aria2c` queue management, `pirata search` PirateBay scraping, contact-sheet frame extraction, and torrentclaw MCP behavior is unchanged. The skill's anime / music / soft / course paths (R9, R10) explicitly stay as today.

## Risks & Dependencies

| Risk | Mitigation |
|---|---|
| WAL atomic refresh implemented incorrectly → DB corruption for readers | Single-process refresh contract; `wal_checkpoint(TRUNCATE)` before swap; integrity check before swap; rollback DB at `imdb/imdb.db.prev`; documented in `imdb/README.md`. |
| FTS5 latency target <50 ms p99 not met on real corpus | Phase 0 gate measures it before Phase 1 starts. If missed, fall back options pre-identified: drop trigram tokenizer (use unicode61 only — already chosen), narrow akas predicate further (drop ES if needed), revisit FTS5 column weighting. |
| RapidFuzz `token_set_ratio` produces wrong-tconst poisoning on edge cases | R5 confidence threshold forces text disambiguation when score is below threshold; R7c `RESOLVED` row in SHORTLIST gives user visibility; 20-item PT-BR fixture in Unit 2 catches the obvious cases before Phase 1 sweep runs. |
| 30-day measurement gate produces ambiguous data (2-10 % zone) | Default closed per origin doc Key Decisions; burden of proof on reopening, not on closing. `pirata_evaluate.py` outputs explicit verdict to remove human bias. |
| IMDb dump URL changes / TSV schema drifts | Schema version field in `imdb/state.json` + ingest aborts loudly on column-count mismatch. Future work: schema-version assertion. |
| KB JSONs accidentally synced beyond Dante via knowledge-hub (license breach) | Documented as a Scope Boundary; addressed by `imdb/README.md` license note. If knowledge-hub ever serves cross-workspace, IMDb-derived fields must be stripped. |
| Sweep miss-log + event log grow unboundedly | Rotation deferred to Phase 2 evaluation per origin doc. STATUS / DOCTOR read-back tolerates whole-file scans for now. |
| `--kb-imdb` flag added to `contact_sheet.py` but SKILL.md DOCTOR `CONTRACT` check not updated → drift `[FAIL]` | Unit 3 explicitly couples both changes; Unit 5 verifies CONTRACT check. |

## Documentation / Operational Notes

- **`imdb/README.md`** — created in Unit 6, documents directory layout, refresh procedure, license carveout, and rollback (`mv imdb/imdb.db.prev imdb/imdb.db`).
- **No new CLAUDE.md / AGENTS.md additions** — the workspace's CLAUDE.md is intentionally minimal and the skill's behavior is documented in SKILL.md.
- **Rollout:** Phase 0 (Units 1, 2, 6's tests + state.json + README + gitignore) lands first as one PR. Phase 1 (Units 3, 4, 5, 6's `pirata_evaluate.py` + `skill_log.py`) lands second after Phase 0 success criteria are met.
- **Monitoring:** `/pirata DOCTOR` is the primary surface — IMDB DB age, KB MISSES count, IMDB EVENTS count are all visible there. After 30 days of Phase 1 use, run `python3 scripts/pirata_evaluate.py --since 30d` to get the Phase 2 verdict.
- **No feature flag** — `--kb-imdb` defaults to off; flipping the default to on after the 20-item baseline run is a one-line `contact_sheet.py` change. The skill's TC-failover wiring is always on; if the user wants to disable it temporarily, they can just `mv imdb/imdb.db imdb/imdb.db.disabled` and DOCTOR will report `[FAIL]` while the rest of the pipeline keeps working (R7b shadow path covers DB-absent gracefully).

## Sources & References

- **Origin document:** [docs/brainstorms/2026-04-24-imdb-local-pirata-coupling-requirements.md](../brainstorms/2026-04-24-imdb-local-pirata-coupling-requirements.md)
- **Related code:**
  - `scripts/contact_sheet.py:244-350` (KB enrichment injection point)
  - `scripts/sheets_sweep.py:137-181` (sweep pass-through pattern)
  - `scripts/sheets_sweep.py:56-58` (`sanitize()` log-injection defense)
  - `.claude/skills/pirata-deck/SKILL.md:144-165` (DOCTOR / STATUS / CONTRACT)
  - `.claude/skills/pirata-deck/references/menu-style.md:394-462` (TR-100 panel templates)
  - `kb/per-movie/who-framed-roger-rabbit-1988.json` (current schema sample)
- **Related plans:**
  - `docs/plans/2026-04-24-001-feat-hunter-py-orchestrator-plan.md`
  - `docs/plans/2026-04-24-002-feat-auto-contact-sheets-plan.md`
  - `docs/plans/2026-04-24-003-feat-kb-rag-multimodal-frames-plan.md`
- **External docs:**
  - SQLite FTS5 — <https://www.sqlite.org/fts5.html>
  - RapidFuzz — <https://rapidfuzz.github.io/RapidFuzz/>
  - IMDb non-commercial datasets — <https://datasets.imdbws.com/>
  - parse-torrent-title (PTT) — <https://pypi.org/project/parse-torrent-title/>
- **MCP schema introspection** (2026-04-24): `mcp__torrentclaw__search_content` parameters confirmed via skill runtime; `imdb_id` not present.
