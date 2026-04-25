---
date: 2026-04-24
topic: imdb-local-pirata-coupling
---

# IMDb Local Catalog × /pirata Skill Coupling

## Problem Frame

The 9.3 GB IMDb non-commercial dataset (`imdb/unnoficial/`, 7 TSVs, 12.4 M titles) is sitting on disk. The seed for this brainstorm was exploration — *"vi o dump e pensei: dá pra usar?"* — not a felt pain. Treating it honestly:

- **There is no recurring failure** that demands this work. TorrentClaw (TC) returned `[DOWN]` once in a single session (HTTP 403). One observation is not a frequency estimate.
- **There is opportunistic value** if the dataset can anchor the workspace's only compounding asset — `kb/per-movie/*.json` produced by the contact-sheet sweep, which feeds RAG-multimodal retrieval. Today that metadata comes from filename parsing; replacing it with `tconst`-anchored fields (rating, runtime, genre, top cast, akas) is a real upgrade *if* a downstream consumer ever queries those fields.
- **There is reversible resilience** if a local lookup layer can stand in for TC when it degrades, or disambiguate PT-BR queries when TC's title fuzzing fails.

This doc commits to the work in **two phases**, evidence-gated. Phase 0 + Phase 1 ship; Phase 2 (the architectural pipeline reorder + cast browser) is held until Phase 1 measurements justify it.

## Phasing Overview

| Phase | Goal | Trigger to next phase |
|---|---|---|
| **Phase 0** — Data + Lookup | Local SQLite from TSVs, `lookup_by_title` / `lookup_by_tconst` / `lookup_episodes` working at <50 ms p99. **No skill changes yet.** | Phase 0 ingest passes integrity check + lookup latency target met. |
| **Phase 1** — KB enrichment + TC fallback | Sweep enriches `kb/per-movie/*.json` with tconst-anchored fields. Skill adds IMDb-fallback when TC is unreachable or returns zero. /pirata stays TC-primary on the happy path. The `logs/skill_imdb_events.jsonl` event log (R16b) is the Phase-2 evidence pipe. | After 30 d of use, replay event log via `scripts/pirata_evaluate.py`. **Default closed**: Phase 2 reopens only if `(tc_zero_results AND imdb_fallback_produced_hits) ≥ 10 %` of total `tc_call` AND ≥ 5 of those failures would have been prevented by IMDb-primary. Below 2 % → Phase 2 stays killed. 2-10 % → defaults to closed, burden of proof on reopening. |
| **Phase 2** — IMDb-primary upgrade + cast browser + PT-BR rerun | Reorder the live-search pipeline so IMDb local resolves canonical (tconst, primaryTitle, year) *before* every TC call. Add `/pirata cast <name>` shortcut + `lookup_cast` / `filmography` API. Add silent PT-BR / ES rerun on weak TC matches. | Out of scope for this doc; revisit only after Phase 1 evidence. |

Phase 1 is designed to stand on its own — KB enrichment + TC fallback deliver value whether or not Phase 2 ever ships. Phase 2 is **not pre-committed to**; it is a hypothesis evaluated against the event log.

## Requirements

**Phase 0 — Data Layer**
- R1. Build a local SQLite database from the IMDb non-commercial TSVs in `imdb/unnoficial/`, persisted at `imdb/imdb.db`.
- R2. Ingest tables: `title.basics`, `title.ratings`, `title.episode`, `title.crew`, top-5 rows of `title.principals` per `tconst` by `ordering` (streamed: assume `title.principals.tsv` is sorted by tconst — verified against current dump format, abort ingest if assumption breaks). And `title.akas` filtered by the predicate `region IN ('BR','PT','ES','MX','AR') OR language IN ('pt','en','es') OR isOriginalTitle = 1` — region and language are independent IMDb columns, NULL-safe (NULL never matches `IN`); the `isOriginalTitle = 1` clause guarantees every title carries at least its source aka. Empirical sizing: pass-2 measurement showed earlier broader predicates captured ~32 M rows. The narrowed predicate above (FR dropped, US/GB region dropped) is expected to land closer to ~10-15 M; **the actual count is part of Phase 0 success criteria, and R15 DOCTOR example numbers must be replaced with measured values post-ingest**. Smoke-test the predicate against a 20-title PT-BR fixture (Duna, Oppenheimer, Interestelar, Cidade de Deus, Tropa de Elite, Bacurau, Ainda Estou Aqui, …) before declaring ingest done. When both `region=BR` and `region=PT` rows exist for the same tconst+language, prefer `BR > PT` for the Brazilian-Portuguese slot. **Plus** during ingest, materialize a derived `series_top_cast(parent_tconst, top_5_nconsts JSON)` table by aggregating most-frequent nconsts across child episodes — amortizes R12's series cast aggregation into the one-time ingest cost.
- R3. Refresh is manual on-demand via `scripts/imdb_ingest.py --refresh`. No cron, no daemon. Pre-flight gate: abort if the `imdb/` partition has <25 GB free (peak usage during refresh ≈ old DB + new TSVs + new DB temp). The script downloads fresh TSVs to `imdb/tmp/`, builds a new DB at `imdb/imdb.db.new`, runs `PRAGMA integrity_check`, then atomically replaces the live DB via POSIX `os.replace()`. The previous DB is kept at `imdb/imdb.db.prev` as a one-generation rollback. On crash mid-refresh, the live DB remains untouched.

**Phase 0 — Lookup API**
- R4. A Python helper module backed by SQLite FTS5 indexes (over `title.basics.primaryTitle`, `title.basics.originalTitle`, and `title.akas.title` joined by `tconst`, with `numVotes` stored as a covering index column for tie-break ordering; lowercase normalized columns for case-insensitive match). Exposes:
  - `lookup_by_title(query: str, year: int | None = None, kind: Literal["movie","tv","short","..."] | None = None) -> list[Match]` — fuzzy title resolution targeting <50 ms p99 on the 12.4 M-row corpus.
  - `lookup_by_tconst(tconst: str) -> Title | None` — full enrichment for one entity.
  - `lookup_episodes(parent_tconst: str, season: int | None = None) -> list[Episode]` — season/episode listing for series routing in Phase 1 sweep enrichment and series search.
  - **Deferred to Phase 2** (gate: Phase 1 measurement): `lookup_cast(name_query: str) -> list[Person]`, `filmography(nconst: str) -> list[Title]`. Built with R11 only, when the cast shortcut is greenlit.
- R5. Disambiguation ranking is 3-tier and tiers are not mixed in the result list: (1) exact match on `primaryTitle` / `originalTitle` / aka.title (case-insensitive, normalized) is the highest tier; (2) fuzzy match (FTS5 + RapidFuzz post-pass with ratio cutoff TBD in planning) is the second tier. Within each tier, `year` filter excludes mismatches when supplied (±0 tolerance unless explicitly relaxed by the caller), `titleType` filter narrows when caller specifies, and `numVotes` descending breaks ties. **Confidence threshold:** if the top-1 result's score is within X % (TBD in planning, default proposal 15 %) of the runner-up *and* both are in tier 1, the helper returns both as a multi-tie and the caller is required to disambiguate — never silently auto-pick.
- R6. Helper accepts PT-BR / EN / ES title input transparently via the akas index (e.g., `lookup_by_title("Duna: Parte Dois")` → tconst tt15239678 with canonical title "Dune: Part Two"). The exact composite score formula is a Resolve-Before-Planning item (see Outstanding Questions); the proposed shape is `score = fuzz_ratio_0_to_100 × field_multiplier` with field multipliers `primaryTitle=3.0`, `originalTitle=2.0`, `aka isOriginalTitle=1=1.8`, `aka regional translation=1.5`, and `numVotes` desc as the tiebreak when scores are equal. Final formula must lock before plan freeze so the implementer doesn't invent it.

**Phase 1 — KB Enrichment (sweep integration)**
- R12. KB JSON enrichment hooks into the manifest builder in `scripts/contact_sheet.py` (where the per-movie JSON is actually written, around the atomic-write block) — not into `scripts/sheets_sweep.py`, which only orchestrates and forwards `--kb-export`. Each `kb/per-movie/*.json` written gets `tconst`, `imdb_rating`, `imdb_votes`, `genres[]`, `runtime_minutes`, `top_cast[5]` (name + role; for `titleType ∈ {tvSeries, tvMiniSeries}`, read pre-computed `series_top_cast` table from R2 ingest — never re-aggregate per-sweep), `akas{pt, en, es}`, `imdb_resolved_at` ISO timestamp, and `imdb_lookup_confidence` score (so a future RAG consumer can filter low-confidence anchors). FR removed from the akas slice for Phase 1 (revisit if a real PT-FR mis-resolution surfaces).
- R13. The matching attempt: extract candidate title from filename (regex strip of year, resolution, codec, group tags), then call `lookup_by_title(extracted_title, year=parsed_year)` against the IMDb FTS5 index (RapidFuzz ratio threshold TBD in planning, validated against a 20-item sample from `./downloads/` before threshold is locked). On no-match (low confidence, malformed filename, non-Latin script not in the akas slice), enrichment is skipped silently and a one-line entry is appended to `logs/sweep_imdb_misses.log` (format: `<iso-ts>\t<filename>\t<reason>` where reason is one of `no_title|no_year|fuzzy_below_threshold|non_latin|multi_tie_unresolved`); the JSON is still written with filename-derived metadata only — never blocks the sweep run.
- R14. `/pirata STATUS` panel adds two rows in the existing sweep section, after `KB SIZE`:
  - `│ KB ENRICHED│ <enriched>/<total> titles                 │` — count of KB JSONs carrying a `tconst`.
  - `│ KB MISSES  │ <n> since refresh · top: <reason>        │` — bucketed read-back of `logs/sweep_imdb_misses.log`. Without this, the miss log is write-only and never informs anything.

**Phase 1 — Skill Integration (TC-primary, IMDb on failure only)**
- R7. `/pirata` movie / series / doc workflows stay TC-primary on the happy path. The IMDb local layer engages **only on TC failure or zero results** (R8). PT-BR / ES rerun on weak TC matches is **deferred to Phase 2** (R-deferred-3) to keep Phase 1 honest as a resilience-only layer; PT-BR mis-resolutions during Phase 1 are counted in the measurement gate (R16b) and become evidence for Phase 2 rather than silent re-runs that smuggle the lookup tax in.
- R7b. Shadow paths for the IMDb lookup layer itself (collapsed into R7's two-line implementation rather than separate cases):
  - DB missing or `lookup_by_title` errors → skill bypasses to pre-coupling pipeline (TC-primary, no IMDb engagement). Surfaces `[IMDB OFFLINE]` in the SHORTLIST header only when an IMDb engagement was attempted but failed.
  - Zero matches in IMDb during a fallback engagement → user query passes through to `pirata` unchanged; SHORTLIST header shows `[IMDB MISS]`.
  - Multi-tie above the R5 confidence threshold → see Disambiguation UX below (text fallback, no interactive panel in Phase 1).
- R7c. **Visibility — wrong-tconst poisoning guard.** Whenever the IMDb layer engages (failover from R8), the SHORTLIST header gains a `RESOLVED │ <primaryTitle> (<year>) · <tconst> · conf <0.XX>` row so the user can sanity-check the resolution before trusting torrent results. On happy-path TC search (no IMDb engagement), this row is omitted.
- R8. **TC search is always keyed by canonical title + year** — verified via `torrentclaw` MCP schema introspection (2026-04-24): `search_content` does NOT accept an `imdb_id` parameter. The earlier "deterministic-join via imdb_id" framing was unsupported and has been retired; Phase 1's TC call always uses `query=<title>` + `year=<year>`. When TC is unreachable, errors, or times out (5 s), the skill calls `lookup_by_title` for canonical resolution and falls through to `pirata search "<canonical title> <year>"`. User-visible signal: dedicated `TC STATUS │ [TC OFFLINE] · fallback: pirata` row in the SHORTLIST header (12/40-col split, separate from the title row to honor the 55-char TR-100 grid). When TC + pirata both return zero seeds, render the IMDb-resolved `RESOLVED` row anyway with `SEEDS │ none — try later` so the user knows the title was identified but no torrents are currently seeded — distinct from `[TC OFFLINE]`.
- R9. Anime workflow stays as today (raw `pirata search`); IMDb engagement is a fallback only when pirata returns zero or the user explicitly invokes disambiguation. Origin signal in shortlist header: `INDEXER │ [PB]` or `INDEXER │ [PB + IMDB resolved]` if IMDb anchored the canonical title.
- R10. Music, software, courses, ROMs, live events skip IMDb entirely — those types remain `pirata search` only. (Most are out-of-dataset anyway.)

**Phase 1 — Disambiguation UX (TR-100 grid)**

- R11a. **Phase 1 disambiguation is text-only — no new interactive panel.** When R5's confidence threshold flags a multi-tie, the skill returns a short inline message in the chat (no TR-100 panel, no new commands), e.g.:

  > Match ambíguo pra "dune": Dune (1984) tt0087182 · 132k votes · Dune (2021) tt1160419 · 698k votes. Re-roda com ano (ex: `dune 2021`) ou tconst (ex: `dune tt1160419`).

  The user re-queries with disambiguating context. This avoids building a new interactive UI surface for an edge case in Phase 1; the full TR-100 disambiguation panel ships in Phase 2 alongside `/pirata cast` (R-deferred-2), where the panel framework is genuinely earned by the cast browser surface.
- R11b. **Auto-pick is rejected on low confidence.** Wrong-tconst poisoning is silent and cascades into TC search results. When R5's threshold flags a multi-tie, the skill always returns the text-disambiguation message above; auto-pick only fires when confidence is decisive (top-1 score >threshold above runner-up).

**Phase 1 — Operations**
- R15. `/pirata DOCTOR` adds checks (all rows ≤55 chars to fit the TR-100 grid; row layouts must be re-measured to exact char count before implementation):
  - `IMDB DB`     → status of `imdb/imdb.db` + age in days. `[OK]` / `[STALE]` if age >30d / `[FAIL]` if missing or corrupt. Stale signal also surfaces at session start as a one-line stderr nag — no per-shortlist badge (R15b dropped per pass-2 consensus: duplicate of DOCTOR, noisy on every search).
  - `IMDB ROWS`   → counts of `title.basics` / `title.ratings` / `title.akas` / `series_top_cast`. Format: `ttl=12.4M · rat=1.4M · aka=<MEASURED>` — actual aka count is filled in post-ingest; the placeholder example numbers from earlier drafts were wrong by orders of magnitude (live TSV scale, not estimated).
  - `IMDB LANGS`  → counts per language slice (pt / en / es); `[FAIL]` if any of the three has 0.
  - `KB MISSES`   → `<n> since last refresh · top: <reason>` (single most-frequent reason from `logs/sweep_imdb_misses.log`); `[WARN]` if total > 5 % of `KB ENRICHED` count.
  - `IMDB EVENTS` → summary from `logs/skill_imdb_events.jsonl` (R16b): event count, oldest entry date, trigger breakdown (`tc_offline`, `tc_zero_results`, `disambig_text_shown`).
- R16. `.gitignore` additions: `imdb/unnoficial/`, `imdb/tmp/`, `imdb/imdb.db`, `imdb/imdb.db.new`, `imdb/imdb.db.prev`, `imdb/*.db-journal`, `imdb/*.db-wal`, `imdb/*.db-shm`, `logs/skill_imdb_events.jsonl`, `logs/sweep_imdb_misses.log`. Tracked: `imdb/state.json` (refresh metadata — last-refresh-ts, source-tsv-checksums, schema-version) and an `imdb/README.md` describing the local layout.
- R16b. **Phase 1 → Phase 2 measurement infrastructure.** A structured event log at `logs/skill_imdb_events.jsonl` is the deliverable that makes the Phase 2 gate enforceable. Each `/pirata` movie / series / doc engagement appends one JSON line: `{ts, event, query, query_lang_guess, tc_status, imdb_engaged, resolved_tconst, resolved_confidence, duration_ms}` where `event ∈ {tc_call, tc_zero_results, tc_error, imdb_fallback_fired, disambig_text_shown, disambig_user_recovered}`. A `scripts/pirata_evaluate.py` reader runs at the 30-day mark to produce the Phase 2 decision: counts of TC failures, disambig prompts, and (when surfaceable) PT-BR mis-resolutions. **Without this log, the Phase 2 gate has no input — it would resolve by vibes.**

**Phase 2 — Deferred (evidence-gated)**
- R-deferred-1. **IMDb-primary live search pipeline** — reorder /pirata workflows so IMDb resolves canonical (tconst, primaryTitle, year) before every TC call. Activated only if R16b event log replay (Phase 1 → Phase 2 gate) supports it.
- R-deferred-2. **Cast browser shortcut** (`/pirata cast <name>`) — adds person disambig panel, filmography list (paginated TR-100), multi-select-and-batch flow. Pulls in `lookup_cast` and `filmography` from R4 + the full TR-100 disambiguation panel framework (which is also what would justify earning the panel infra in Phase 2). Activated only on user-felt need.
- R-deferred-3. **Silent PT-BR / ES rerun on weak TC matches** — the heuristic from earlier R7c drafts (re-run TC keyed by IMDb-resolved canonical English title when TC's top result has weak title match against a non-English query). Deferred because it reintroduces the lookup tax silently and depends on a heuristic with no stable definition. The R16b event log will count PT-BR mis-resolutions during Phase 1; if those are common, R-deferred-3 is justified at the same gate as R-deferred-1.

## Success Criteria

**Phase 0:**
- `lookup_by_title("Duna", year=2024)` returns tt15239678 in <50 ms p99 with confidence ranking on the 12.4 M-row corpus.
- 20-title PT-BR fixture (Duna, Oppenheimer, Interestelar, Cidade de Deus, Tropa de Elite, Bacurau, Ainda Estou Aqui, etc.) all resolve to correct tconst — measured before declaring ingest done.
- `scripts/imdb_ingest.py --refresh` completes a full re-ingest in <10 min on M-series Mac, atomic replace verified by `PRAGMA integrity_check`.

**Phase 1:**
- A baseline-stamped sweep run on `./downloads/` enriches a measurable percentage of releases with `tconst`-anchored metadata. The target is set after a 20-item manual reconciliation sample establishes a credible upper bound — not a pre-baked 90 %. Misses are bucketed in `logs/sweep_imdb_misses.log`.
- `/pirata m oppenheimer` produces a usable shortlist when TC API is intentionally blocked (proven by `--with-tc-blocked` smoke test that injects a TC failure at the skill adapter, not at the network layer).
- `/pirata DOCTOR` distinguishes stale-DB, missing-language, and crash-rolled-back states without false positives on healthy state.
- Phase 1 → Phase 2 evaluation: the `logs/skill_imdb_events.jsonl` event log (R16b) drives the decision at the 30-day mark. **Default is closed** — Phase 2 stays killed unless evidence reopens it. **Reopen criterion (concrete, not "lookup-tax-supports"):** count of `(tc_zero_results AND imdb_fallback_produced_hits)` events ≥ 10 % of total `tc_call` events over 30 days, AND replaying the failed queries against the Phase 1 lookup layer would have prevented at least 5 of them. **Closed below 2 %.** The 2-10 % zone defaults to closed (burden of proof on reopening, not on closing).

## Scope Boundaries

- Not building a web UI, REST API, or daemon — strictly local CLI / skill-driven.
- Not exposing full `title.principals` graph beyond top 5 cast; no full crew (DPs, editors) in the helper API for v1.
- Not shipping posters or stills — IMDb non-commercial dataset has none; TMDB / TC handle visual assets.
- Not replacing TC as the primary indexer of seeders, quality scores, HDR flags, or release groups — TC remains the source of truth for those signals on the happy path.
- Not auto-refreshing the dump — manual cadence chosen deliberately. Stale signal lives in DOCTOR + a one-line stderr nag at session start when DB age >30d (no per-shortlist badge — that was over-built scope).
- Not reordering the live-search pipeline in Phase 1 — IMDb-primary stays Phase-2 deferred.
- Not adding `/pirata cast` or any person-search surface in Phase 1.
- Music, software, courses, ROMs, live events explicitly excluded from IMDb resolution (out of dataset).
- KB JSONs with IMDb-derived fields are local-only by default. If `kb/` is ever synced to knowledge-hub or external RAG, the IMDb-licensed fields must be stripped or the license re-evaluated for that distribution.

## Key Decisions

- **Phase 0 + 1 first, Phase 2 deferred (R7 / R-deferred-1 / R-deferred-2 / R-deferred-3)** — chose phasing over big-bang IMDb-primary. Reason: the IMDb-primary value depends on facts the workspace hasn't measured yet (TC happy-path failure rate, PT-BR mis-resolution rate). Phase 1 measures both and gates Phase 2 on evidence. Phase 2 stays killed by default; the KB enrichment + failover (the high-value pieces) are already delivered.
- **TC-primary on happy path; IMDb engages only on TC failure or zero results (R7 / R8)** — chosen over IMDb-primary mandatory and over silent PT-BR rerun (deferred to R-deferred-3). Avoids the lookup tax on the common case; PT-BR mis-resolutions during Phase 1 are counted by the event log and become evidence for or against Phase 2 rather than silent re-runs that smuggle the architecture in.
- **TC search is title-keyed always (R8)** — verified by MCP schema introspection 2026-04-24: `search_content` does not accept `imdb_id`. The "deterministic join via tconst" framing was unsupported and retired. Whether TC's response payload includes tconst is a separate question for Phase 2 evaluation.
- **Force text-disambiguation prompt when confidence is low (R5 / R11a / R11b)** — chosen over silent auto-pick + runner-up override (rejected) and over a new TR-100 panel (deferred to Phase 2 alongside the cast browser). The text prompt asks the user to re-query with `<title> <year>` or `<title> <tconst>`; minimal new UI surface in Phase 1.
- **Manual refresh cadence (R3)** — chosen over daily cron. Personal workspace, low-stakes freshness, zero ops surface. DOCTOR row + a one-line stderr nag at session start (when DB age >30d) make staleness visible without per-shortlist badge noise.
- **Akas: PT-BR + EN + ES (R2)** — narrowed from PT-BR + EN + ES + FR after pass-2 measurement showed earlier broader predicates captured ~32 M aka rows (3-4× the original disk-budget estimate). FR dropped — revisit if a real PT-FR mis-resolution surfaces in the event log.
- **Sweep no-match is silent + logged + read back (R13 / R14)** — chosen over "block JSON until disambiguated" (would freeze the sweep) and "log and never read" (write-only logging). The KB MISSES bucket in STATUS makes the log a feedback loop.
- **Series cast aggregated at ingest, not at sweep time (R2 / R12)** — pre-computed `series_top_cast` table during ingest amortizes the per-episode aggregation. Sweep enrichment reads it directly; no runtime joins per release.

## Identity & Carrying Cost

- **Identity:** /pirata stays a "torrent search + queue" surface in Phase 1. The IMDb layer is invisible until TC fails or returns zero (R7 / R8). Phase 2 (IMDb-primary + cast browser + PT-BR rerun) would shift identity toward "media catalog that queues torrents" — that pivot is deferred and evidence-gated. If Phase 2 lands, decide explicitly whether to keep `/pirata` as the umbrella or split off `/movies`.
- **Carrying cost (named honestly):**
  - ~10 GB raw TSVs + ~500 MB SQLite in `imdb/` (gitignored).
  - One-time ingest ~10 min on M-series; full re-ingest on each `--refresh` (no incremental for now).
  - Fuzzy-match heuristic in R13 will drift as filename patterns evolve; KB MISSES bucket gives the feedback loop.
  - DOCTOR + STATUS gain new rows that must be kept current as the schema or akas slice evolves.
  - IMDb dump URL stability assumed; if it changes, ingest breaks loudly (preferable to silent corruption).
- **Opportunity cost:** this work parks the next iteration of `kb-export` and the `/pirata cast` curiosity. Both are lower-value than tconst-anchoring the KB.

## Dependencies / Assumptions

- **aria2c, `pirata` CLI, `scripts/queue.py` contracts** — unchanged. Coupling adds a layer above them, never under.
- **IMDb non-commercial dataset URL stability** — `https://datasets.imdbws.com/` has been stable for years.
- **IMDb non-commercial license terms** — source: <https://developer.imdb.com/non-commercial-datasets/>. Permits personal non-commercial use of the data; restricts redistribution and derivative-DB distribution. The local SQLite + KB JSONs are personal-use derivatives; *if they ever leave the workspace* (synced to knowledge-hub for cross-workspace retrieval, published to a service, indexed by an external RAG), the license must be re-checked and likely the IMDb-derived fields stripped before sync. Recorded here so the question doesn't get re-asked silently.
- **Disk budget** — ~10 GB raw TSVs + ~500 MB SQLite in `imdb/`. R3's 25 GB pre-flight gate guards refresh; current free space verified per workspace volume.
- **`title.principals.tsv` is sorted by tconst** — verified against current dump. Ingest assumes this for the streamed top-5 selection; an unsorted upstream change would fail the per-tconst counter loudly rather than silently corrupt.

## Outstanding Questions

### Resolved during brainstorm

- [Affects R8] ✅ **TC `imdb_id` parameter support** — verified 2026-04-24 via `torrentclaw` MCP schema introspection. `search_content` does NOT accept `imdb_id` or `tconst`; only `query`, `type`, `season`, `episode`, `year_min/max`, `quality`, `hdr`, `audio`, `language`, `locale`, `country`, `min_rating`, `sort`, `limit`, `page`, `availability`, `compact`, `genre`. TC has its own internal `content_id` for `get_credits` / `get_watch_providers` but it is not IMDb-keyed. Whether TC's response payload contains `tconst` per result is a separate Phase 2 evaluation question, not a R7/R8 blocker.

### Resolve Before Planning

- [Affects Phase 1 → Phase 2 gate, R13] **Run a 20-item manual reconciliation against `./downloads/`** matching filenames to IMDb tconsts by hand. Output drives: (a) the RapidFuzz ratio threshold in R13, (b) the realistic enrichment-rate target for Phase 1 success criteria (replaces the aspirational 90 % default), (c) sanity check on R5's confidence multipliers (R6) against real titles. Without this baseline the success criterion is aspirational.
- [Affects R5 / R6][Technical] **Lock the disambiguation score formula.** R5's confidence threshold (X % of runner-up), R6's score multipliers (3.0 / 2.0 / 1.8 / 1.5), and FTS5 bm25 + RapidFuzz are not commensurable as stated. Pick one composite formula (proposal: `score = fuzz_ratio_0_to_100 * field_multiplier`, `fuzz=100` for exact, `numVotes` desc as tiebreak inside ties) before plan freezes; otherwise an implementer must invent it.
- [Affects R12 / Problem Frame] **Name the KB-enrichment consumer.** Phase 1 enriches KB JSONs with tconst-anchored fields under the assumption that "RAG-multimodal retrieval" will use them. If no concrete consumer exists today, decide whether (a) building the consumer is a Phase 1 deliverable, (b) Phase 1 is schema-only prep with no consumer, or (c) the enrichment is dropped entirely. Without a named consumer, R12 is speculative work.

### Deferred to Planning

- [Affects R5 / R6][Technical] FTS5 index physical design — single virtual table over `(primaryTitle, originalTitle, akas.title)` joined by tconst, vs three separate indexes; trigram tokenizer vs default unicode61; pinning `mmap_size`, `cache_size`, `journal_mode=WAL`. Latency target <50 ms p99 must drive the choice and be **measured** on the actual ingested corpus before Phase 0 is declared durably done.
- [Affects R13][Technical] Filename-extraction regex set — what release-group / codec / quality tags to strip; whether to use an existing parser library (e.g., `parse-torrent-name`, `guessit`).
- [Affects R3][Technical] Ingest implementation — bulk INSERT in one transaction vs `executemany` chunks; whether to use `pandas.to_sql` chunked for the 4.4 GB `title.principals` file; cost of the new `series_top_cast` aggregation pass within the <10 min budget.
- [Affects R7b][Technical] How does the skill detect "IMDb DB unavailable"? Health probe at skill init? Lazy try/except on first lookup with cached failure flag?
- [Affects R13 / R16b][Operational] Log rotation for `logs/sweep_imdb_misses.log` and `logs/skill_imdb_events.jsonl` — both grow unboundedly; rotation policy + how `KB MISSES` / `IMDB EVENTS` STATUS rows handle rotated files.
- [Affects R3 / R15][Technical] DOCTOR's age check — uses `imdb/state.json.last_refresh_ts` (tracked in git, may be present without DB) or `mtime(imdb/imdb.db)` (absent if DB never built)? State both, fall back to mtime.

## Next Steps

-> `/ce-plan` for structured Phase 0 implementation planning, blocked on the three "Resolve Before Planning" items above.
