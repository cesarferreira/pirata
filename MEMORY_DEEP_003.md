# Memory Deep #003

| Field       | Value                                              |
|-------------|----------------------------------------------------|
| Created     | 2026-04-25 03:36 BRT                               |
| Project     | pirata — personal media download + contact-sheet workspace |
| Session     | Brainstorm + plan + Unit 1 implementation for IMDb local catalog × /pirata coupling. 3 brainstorm passes, 1 plan + headless review pass, ingest script + 23-scenario test suite. 4 commits. |
| Previous    | MEMORY_DEEP_002.md                                |

---

## ⚡ Continue Trigger (auto-fire on resume)

**When user types `continue`, `continua`, `retoma`, or any variant of "voltei / vamo continuar":**

1. Confirm orientation: read this snapshot, run `git log --oneline -6` to confirm we're at commit `fb4b4a4` or later.
2. **Auto-fire the real-corpus ingest** (Phase 0 acceptance criterion):
   ```bash
   python3 -u scripts/imdb_ingest.py --refresh > logs/imdb_ingest.log 2>&1 &
   ```
   Background mode. Log to `logs/imdb_ingest.log`. PID announced inline.
3. Monitor via `tail -f logs/imdb_ingest.log` (or read tail every ~30s) until either:
   - "OK refresh complete" line appears → validate Phase 0 success criterion (under 10 min wallclock; `sqlite3 imdb/imdb.db 'PRAGMA integrity_check'` returns "ok"; row counts non-zero in all 8+ tables; ft_titles populated).
   - "FAIL" line appears → diagnose (memory, disk, malformed TSV, sort violation).
4. Once Phase 0 is durably done, **immediately start Unit 2** (`scripts/imdb_lookup.py`) per the plan at `docs/plans/2026-04-24-004-feat-imdb-local-pirata-coupling-plan.md`. Use the real DB for development. Run the 20-item PT-BR fixture validation (Unit 2 Verification step) and lock the RapidFuzz threshold from the data.

If the user qualifies the resume (e.g., "continue but skip ingest" or "continue with Unit 2 against fixture"), honor that. The trigger is the default action, not a forced one.

---

## Project Context

`pirata` is Vidigal's personal Mac-based media workspace at `~/claude-code/pirata`. Two muscles: (a) a `torrentclaw` MCP for rich movie/TV search with metadata, and (b) a Rust `pirata` CLI scraper for non-TC sources. Downloads go through `aria2c` orchestrated by `scripts/queue.py`. On top: cinema-grade contact-sheet pipeline for human review (`release/contact-sheets/`) and a parallel KB export for RAG-multimodal ingest (`kb/`). Path-agnostic sweeper picks up any new release dir without sheets. **New as of session 003:** offline IMDb non-commercial dataset (~12.4 M titles, ~9.3 GB raw TSVs) is becoming the workspace's metadata anchor — Phase 0 ingest infra now exists; Phase 1 (KB enrichment + skill TC-failover wiring) and Phase 2 (IMDb-primary pipeline + cast browser) are planned and evidence-gated.

## What Happened This Session

Massive session. Three discrete arcs: brainstorm → plan → Unit 1 implementation, with multiple review passes throughout.

### Arc 1: `/ce-brainstorm` (3 passes)

User invoked `/pirata` to open the deck, then asked: *"veja este catalogo nao-ofocial do imdb (nao sei se esta atualizado) '/Users/vidigal/claude-code/pirata/imdb/unnoficial', e faca o brainstorm para saber se ajudaria termos ele acoplado a nossa skill /pirata?"*

Confirmed dump = full IMDb non-commercial dataset (7 TSVs, 9.3 GB, 12.4 M titles in `title.basics` alone; `title.akas` alone is 56 M rows / 2.8 GB; `title.principals` 99 M / 4.4 GB). Ran `/ce-brainstorm` in collaborative mode.

**Pass 1 — Initial framing.** User picked option C (full skill integration: KB enrichment + TC failover + cast shortcut + IMDb-primary live search reorder). Sub-decisions: manual on-demand refresh; akas PT-BR + EN + ES + FR; IMDb-primary on every query (TC enriches). Wrote `docs/brainstorms/2026-04-24-imdb-local-pirata-coupling-requirements.md` — 16 requirements, scope boundaries, key decisions.

Ran `compound-engineering:ce-doc-review` interactive — 6 reviewers (coherence, feasibility, product-lens, design-lens, scope-guardian, adversarial). 54 findings, 5 auto-fixes applied (akas terminology drift, TC `imdb_id` fallback path, R3 atomic protocol, R5 ranking ambiguity, R12 wrong-script reference).

**Pass 2 — Architecture recall.** Reviewers strongly converged on rescoping. Key user pushback: *"pq vc acha que e melhor manter TC-primary?"* — saved a feedback memory `feedback_show_reasoning.md` capturing this pattern (user wants steel-manned reasoning, not "trust me" recommendations). After honest both-sides reasoning, user picked **Phase 0+1 with IMDb-primary as deferred Phase 2 hypothesis**. Wrote substantially restructured doc with phasing table, Phase 1 → Phase 2 measurement gate, Identity & Carrying Cost section, deferred items. Re-reviewed with 4 personas (skipped design-lens + adversarial since their P0/P1s were absorbed structurally). Pass 2 surfaced new issues: akas predicate captured 32 M rows not 8.5 M (measured against live TSV during review), R7c heuristic was matematicamente broken (cross-language token containment = zero overlap by definition), interactive disambig panel was over-built scope, no event-log writer infra, etc.

**Pass 3 — Surgical consensus fixes.** Applied the 6 strongest consensus auto-fixes: defer R7c entirely to R-deferred-3, replace interactive panel with text fallback (`re-roda com <title> <year>`), tighten akas predicate to drop FR + US/GB regions, drop FR from R12 + R15, add R16b for `logs/skill_imdb_events.jsonl` event log writer + `scripts/pirata_evaluate.py` reader, run TC `imdb_id` MCP introspection NOW (verified absent). Doc reached 165 lines, 16 reqs Phase 0+1, 3 deferred Phase 2 items, 3 RBP items remaining.

### Arc 2: `/ce-plan` + headless review

User confirmed `knowledge-hub MCP via ingest_sync` as the KB consumer. Ran `/ce-plan` against the brainstorm. Phase 1.1 spawned 3 research agents in parallel (`ce-repo-research-analyst`, `ce-learnings-researcher`, `ce-framework-docs-researcher`).

**Research findings consolidated:**
- **Workspace stack:** Python 3.11+, scripts in `scripts/` flat, bash smoke tests in `scripts/tests/`. No linter/formatter config. Pillow is the only third-party Python dep (global install). **No SQLite/FTS5/RapidFuzz prior art** — Phase 0 is greenfield.
- **Injection points confirmed:** `contact_sheet.py:313-325` (manifest dict — R12 hook), `:327-330` (atomic write pattern to mirror), `:85-88` (parse_year_from_title), argparse `:353-375`. Sweep `run_contact_sheet()` `:137-181` is the only argv builder.
- **TR-100 contract confirmed:** 12-char label / 40-char data / 55-char total. SHORTLIST `RESOLVED` row would overflow with full title+tconst+confidence — needs abbreviation rule.
- **Critical SQLite finding:** `os.replace()` on `.db` only is INCORRECT — WAL = 3 files (`.db`, `.db-wal`, `.db-shm`); concurrent readers get corrupted. Correct pattern: build `.new` → `PRAGMA wal_checkpoint(TRUNCATE)` → close all → `os.replace`. Plus orphan WAL/SHM cleanup post-swap.
- **FTS5 schema choice:** contentless single virtual table with `tconst UNINDEXED` is recommended (NOT external-content because tconst is TEXT not INTEGER PK).
- **Filename parser:** **PTT** (`parse-torrent-title`) chosen over `guessit` (LGPLv3 + 4 deps; slow maintenance). Zero deps, MIT, active.
- **macOS Python `sqlite3`:** ships with FTS5 compiled in (verified 3.43.2 on this Mac).

Wrote `docs/plans/2026-04-24-004-feat-imdb-local-pirata-coupling-plan.md` — 547 lines, 6 implementation units, full Output Structure tree, Mermaid HLD diagram, locked Key Decisions, Risks table.

Ran `compound-engineering:ce-doc-review` in `mode:headless`. 6 reviewers in parallel. 50+ findings. Applied 7 critical auto-fixes:
- **FTS5 Tier 1** rewritten to use B-tree COLLATE NOCASE indexes for exact match (FTS5 equality scan was benchmarked unfit for <50 ms p99 target).
- **WAL refresh protocol** spelled out: pre-flight unlink stale `.db.new*`, `imdb/.refresh.lock` flock-style file, `imdb_lookup.py` checks for it and returns `[IMDB OFFLINE: refresh]`, post-swap orphan cleanup.
- **RESOLVED row formula** locked with concrete rendering function in `imdb_lookup.py` (suffix measured at 24 chars; safe title budget = 16 chars; earlier draft formula was wrong by 9 chars).
- **Phase 2 reopen criterion** restored both AND-clauses from origin doc that had been silently dropped (`≥50 events AND `tc_zero+imdb_recovered ≥10%` AND ≥5 replay-prevented`).
- **Abbreviation table** locked for event-type and reason-code enums (`disambig_user_recovered` → `disambig_ok`, `fuzzy_below_threshold` → `fuzz_low`, etc.) — fixes DOCTOR/STATUS row overflow.
- **CONTRACT coupling** moved Unit 5's SKILL.md edit into Unit 3 Files (eliminates drift `[FAIL]` window between commits).
- **state.json → .example pattern** — track stub, gitignore runtime file (eliminates perpetually dirty `git status` after refresh).

Plus consolidated open questions: 4 RBP items resolved during planning (TC `imdb_id`, KB consumer, score formula, fixture target), 2 promoted to Resolve-Before-Phase-1 (knowledge-hub schema verification, 15% threshold calibration), rest moved to Deferred to Implementation.

### Arc 3: knowledge-hub MCP verification (resolved Phase-1 RBP)

Called `mcp__knowledge-hub__health` + `list_kbs` + `topology`. **Critical finding:** pirata `kb/` is NOT registered as a knowledge-hub KB; watched roots are all under `/Users/vidigal/knowledge-base/`. Chunking is markdown-oriented (160-260 word range). Per-document filtering on user-defined JSON fields is NOT exposed in the retrieve surface — filters are score-based only. So the brainstorm's KB consumer claim was overstated.

**Plan updated** (commit `2971eb8`): R12 reframed as `schema-only prep + searchable-text enrichment` (tconst/cast/genres become BM25-searchable text), not as filter-and-rerank substrate. Added Unit 6 op-step: one-time `mcp__knowledge-hub__ingest_sync` call against `kb/` after first `--kb-imdb` sweep. Removed the now-resolved Resolve-Before-Phase-1 items.

### Arc 4: `/ce-work` Unit 1 — `scripts/imdb_ingest.py`

Created 7 tasks via TaskCreate. Read patterns from `contact_sheet.py` and `sheets_sweep.py` (sys.path guard, log/sanitize helpers, argparse style, atomic write pattern). Inspected 7 TSV headers + row counts:

- `name.basics`: 15.3 M rows (nconst, primaryName, birthYear, deathYear, primaryProfession, knownForTitles)
- `title.akas`: 56 M rows (titleId, ordering, title, region, language, types, attributes, isOriginalTitle)
- `title.basics`: 12.5 M rows (tconst, titleType, primaryTitle, originalTitle, isAdult, startYear, endYear, runtimeMinutes, genres)
- `title.crew`: 12.5 M (tconst, directors, writers)
- `title.episode`: 9.6 M (tconst, parentTconst, seasonNumber, episodeNumber)
- `title.principals`: 99.1 M (tconst, ordering, nconst, category, job, characters)
- `title.ratings`: 1.7 M (tconst, averageRating, numVotes)

**Wrote `scripts/imdb_ingest.py`** (521 lines):

- Streaming `csv.reader` + chunked `executemany` (50k batch, single transaction per table). No pandas dep.
- Tables: `title_basics`, `title_ratings`, `title_episode`, `title_crew`, `title_principals_top5`, `title_akas`, `name_basics`, `series_top_cast`, `ingest_meta`. Plus `ft_titles` virtual.
- **Deviations from plan (justified inline in commit message):**
  - Added `name_basics` as a 7th table (plan didn't list it, but Unit 2's `lookup_by_tconst` returns top_cast with names → either store name_basics + denorm at ingest, or join at read time. Chose ingest + UPDATE-based denorm into `title_principals_top5.name`).
  - Filter `title.principals` to `category IN ('actor', 'actress', 'self')` — top-5 by ordering captures cast roster, not director/writer (mixed in raw stream by ordering).
- WAL pragmas at build time (`journal_mode=WAL, synchronous=NORMAL, cache=64MB, mmap=256MB, page_size=8192`).
- `series_top_cast` materialized via window-ranked `ROW_NUMBER() OVER (PARTITION BY pt ORDER BY freq DESC, nconst ASC)` aggregation — top-5 per parent_tconst as JSON array.
- FTS5 populated as 3 sequential `INSERT … SELECT` (NOT a single UNION ALL — pass-2 adversarial review flagged the memory risk).
- B-tree `COLLATE NOCASE` indexes built post-load for tier-1 exact match: `idx_basics_primary_lower`, `idx_basics_original_lower`, `idx_akas_title_lower`. Plus supporting indexes (`idx_episode_parent`, `idx_ratings_votes`, `idx_principals_tconst`, `idx_names_primary_lower`).
- WAL-safe atomic refresh: `acquire_lock` (flock on `imdb/.refresh.lock`), `precheck_disk` (<25 GB → exit 2), `cleanup_stale_artifacts` (unlink stale `.db.new*`), build at `.db.new`, `integrity_check`, `wal_checkpoint(TRUNCATE)`, promote previous → `.prev`, `os.replace`, `cleanup_post_swap` (orphan `.db.new-wal`/`.db.new-shm`), `write_state_json` with SHA-256 source checksums via temp + atomic rename.
- Sort assumption guard: `seen_tconsts` set tracks every previous tconst; aborts loudly with `RuntimeError` if a tconst reappears after a different one was seen (per pass-2 adversarial review — interleaved tconsts edge case).
- CLI: `--refresh, --source <dir>, --db <path>, --state <path>, --lock <path>, --min-free-gb <int>, --no-sort-check`. Exit codes 0/1/2/3 per refresh/config/precheck/ingest fail.

**Wrote `scripts/tests/test_imdb_ingest.sh`** (~270 lines, 23 scenarios):

- Hermetic mktemp tmpdir; `trap cleanup EXIT`.
- Synthesized 7 fixture TSVs via `printf` with tab separators (5 titles, 1 series with 2 eps, principals with cast filter + top-5 cap test cases, akas with filter test cases including DE drop + FR isOriginal=1 keep).
- Asserts: ingest exits 0, all 8 tables exist, row counts per table, `\N → NULL` handling, cast filter drops director/writer, top-5 cap on tt001, principal name denorm populated, akas filter (7 kept / 2 dropped), `series_top_cast` aggregation correctness, FTS5 populated from primary/original/aka, `state.json` schema, integrity_check, B-tree indexes built, idempotency (`.prev` exists after second run), disk pre-flight failure, principals sort violation.
- Uses `sqlite3` CLI for assertions (verified at `/usr/bin/sqlite3` v3.43.2).

**First run: 46/46 PASS** (3 PASS messages were silently swallowed by `>/dev/null` redirects in two asserts — cosmetic, all underlying tests passed).

### Commits this session

```
fb4b4a4 feat(imdb): Unit 1 — TSV → SQLite ingest with WAL-safe atomic refresh
2971eb8 docs(plan): update KB consumer reality after knowledge-hub MCP verification
53d5180 docs(plan): Phase 0+1 plan for IMDb × /pirata coupling
221f755 docs(brainstorm): IMDb local catalog × /pirata coupling requirements
```

## Decisions Made

- **Decision:** Phase 0 + 1 phased approach with Phase 2 evidence-gated — **Why:** brainstorm pass-2 reviewers (product-lens + scope-guardian consensus) flagged that user's option C was over-scope for an admittedly exploratory "vi o dump e pensei: dá pra usar?" goal. User pushed back asking why TC-primary was better; honest both-sides reasoning landed on "TC-primary keeps reversibility, evidence gates IMDb-primary at day 30". User picked Phase 0+1 with IMDb-primary as Phase 2 hypothesis.
- **Decision:** Akas slice = PT-BR + EN + ES (FR dropped) — **Why:** pass-2 measurement showed earlier FR-included predicate captured ~32 M rows (3-4× the original disk-budget estimate). FR was acknowledged marginal in Key Decisions; dropping cuts disk + ingest cost meaningfully.
- **Decision:** TC `imdb_id` parameter is NOT supported — **Why:** verified 2026-04-24 via `mcp__torrentclaw` MCP schema introspection. R8 retired the conditional "deterministic-join via imdb_id" branch; Phase 1 always TC-keys by canonical title + year.
- **Decision:** knowledge-hub consumer story is searchable-not-filterable — **Why:** verified 2026-04-25 via `mcp__knowledge-hub__health` + `list_kbs` + `topology`. Pirata kb/ is NOT a registered KB; watched roots are under `/Users/vidigal/knowledge-base/`. Chunking is markdown-oriented; per-document filtering on user-defined JSON fields is NOT in the retrieve surface. R12 reframed: enriched fields become searchable as text, not filterable as structured fields. Real filter-and-rerank requires Phase 2 design.
- **Decision:** FTS5 contentless table for fuzzy tier; B-tree `COLLATE NOCASE` indexes for tier-1 exact match — **Why:** pass-2 feasibility benchmark showed FTS5 equality scan was 141 ms / 2 M rows; would miss <50 ms p99 target on 70 M+ rows of ft_titles. B-tree indexes deliver microseconds for exact match.
- **Decision:** `name_basics` IS ingested in Phase 0 (deviation from plan) — **Why:** Unit 2's `lookup_by_tconst` returns top_cast with names. Either ingest name_basics + denorm via UPDATE post-load, or JOIN at read time. Chose ingest + denorm: zero JOIN cost at lookup, ~1 GB disk for the table + index. Documented in commit message.
- **Decision:** Filter `title.principals` to `category IN ('actor', 'actress', 'self')` — **Why:** raw stream interleaves directors/producers/writers/cast by `ordering`. For top_cast intent, filter to acting categories. `self` included for documentaries.
- **Decision:** Sort-assumption guard via `seen_tconsts` set (~1 GB RAM transient) — **Why:** pass-2 adversarial flagged that simple "track last_tconst" guard misses interleaved-tconst edge case (e.g., `tt001, tt002, tt001`). Set-based guard is bulletproof. Cost is ~1 GB during ingest only; freed after.
- **Decision:** state.json is gitignored runtime file; tracked stub is `state.json.example` (Unit 6 owns) — **Why:** pass-2 review consensus that tracked stub overwritten at runtime produces perpetually dirty `git status` after every refresh.
- **Decision:** Phase 2 reopen criterion is BOTH `(tc_zero+imdb_recovered ≥10%)` AND `(≥5 replay-prevented)` AND `≥50 tc_call total` — **Why:** product-lens caught that the Mermaid diagram had silently dropped the second AND-clause from the origin doc, weakening the gate. Restored both predicates literally + locked N_min=50 floor for INSUFFICIENT-DATA verdict.
- **Decision:** RESOLVED row uses `tt{tconst[:8]}...` abbreviation; locked rendering function in `imdb_lookup.py` as single source of truth — **Why:** design-lens caught that the original truncation formula `len(title)+len(year)+15>40` was wrong by 9 chars (suffix actually measures 24 chars). Single rendering function used by both skill and panel template eliminates drift.
- **Decision:** SKILL.md CONTRACT flag-list edit moved to Unit 3 Files — **Why:** adversarial caught that having Unit 3 add the `--kb-imdb` flag to contact_sheet.py and Unit 5 update SKILL.md CONTRACT check creates a drift `[FAIL]` window if commits land separately. Coupling lives in one unit now.
- **Decision:** Commit Unit 1 .gitignore additions early (Unit 6 plan-wise) — **Why:** otherwise the smoke test runs and any future real ingest would dirty the working tree with imdb/imdb.db, .refresh.lock, etc. Scope creep is justified by Unit 1 needing clean test runs.
- **Decision:** Saved feedback memory `feedback_show_reasoning.md` after user pushback during brainstorm pass-2 — **Why:** observed that user wants steel-manned reasoning + dependency-on-unverified-facts visibility before accepting a recommendation, not "trust me, the consensus says X". Pattern: lead with recommendation but immediately follow with what unverified facts the recommendation depends on, what the steel-manned counter is, and why the recommendation wins for THIS user's context.

## Current State

**Working / done:**
- Brainstorm doc finalized (3 review passes): `docs/brainstorms/2026-04-24-imdb-local-pirata-coupling-requirements.md` (164 lines).
- Plan doc finalized (1 plan pass + 1 headless review pass + 1 knowledge-hub update): `docs/plans/2026-04-24-004-feat-imdb-local-pirata-coupling-plan.md` (~550 lines).
- Unit 1 implementation: `scripts/imdb_ingest.py` (521 lines) + `scripts/tests/test_imdb_ingest.sh` (~270 lines, 23 scenarios). 46/46 PASS on smoke test.
- 4 commits on main this session (none pushed).
- Auto-memory updated: `feedback_show_reasoning.md` added to `~/.claude/projects/-Users-vidigal-claude-code-pirata/memory/`.

**Phase 0 status:** Unit 1 done; Unit 2 (`scripts/imdb_lookup.py`) pending. Real-corpus ingest verification (the Phase 0 acceptance criterion: `python3 scripts/imdb_ingest.py --refresh` against the real 9.3 GB) NOT yet run — that's the continue-trigger task.

**Untouched working tree (pre-existing, not from this session):**
- `M .gitignore` (only the `target` line was tracked; we modified to add imdb + log entries this session — already committed)
- Untracked: `.claude/commands/`, `CLAUDE.md`, `MEMORY_DEEP_001.md`, `MEMORY_DEEP_002.md`, `docs/brainstorms/2026-04-24-kb-rag-multimodal-frames-requirements.md`, `docs/plans/2026-04-24-001/002/003-*.md`, `kb/`, `logs/`. None of these are from session 003 — leaving them for separate user decision.

**Disk:** Same as session 002 (~9% free, 166 GB) — was not exercised this session beyond `git` operations. Real ingest will need ~25 GB free per the script's pre-flight gate; this is **a blocker** for the continue-trigger if disk has not been freed since session 002. Verify before running.

## Done (Cumulative)

- [x] `/pirata` skill spec'd (TR-100 monochrome, 12-branch menu)
- [x] Memory feedback saved: ANSI escapes don't render in Claude code fences
- [x] `scripts/queue.py` — aria2c wrapper
- [x] `scripts/contact_sheet.py` — full pipeline (scdet → extract → label → tile)
- [x] Caption strip below thumb design
- [x] LLM-readable label fonts (auto-scaled with thumb width)
- [x] fps auto-detection via `probe_fps()`
- [x] Slug-prefixed sheet filenames
- [x] scdet result caching for fast re-runs
- [x] `scripts/sheets_sweep.py` — opportunistic sweeper, path-agnostic
- [x] sweep-level flock + security defenses (resolve+is_relative_to, --terminator, repr-sanitize, killpg)
- [x] `scripts/queue.py` `--autosheets`/`--no-autosheets` integration
- [x] `/pirata` skill panel rows: STATUS (LAST SWEEP, SHEETED, KB SIZE), DOCTOR (SWEEP, DL DIR, CONTRACT, KB DIR)
- [x] `scripts/tests/test_sweep.sh` — 12 assertions
- [x] `contact_sheet.py --kb-export` + `--kb-force` flags
- [x] `tile_sheets()` (clean= mode removed in session 002)
- [x] `export_kb()` — frames JPEG + sheets JPEG + per-movie JSON + JSONL append
- [x] `sheets_sweep.py --kb`/`--no-kb` integration
- [x] `scripts/tests/test_kb_export.sh` — 18 assertions
- [x] First real-world KB export run validated on Roger Rabbit (session 002)
- [x] KB sheet refactor: clean re-tile → labeled JPEG (~80% lighter) (session 002)
- [x] Dir rename: kb/contact-sheets-clean/ → kb/contact-sheets/ (session 002)
- [x] Manifest.jsonl deduplicated post --kb-force via per-movie JSON reconstruction (session 002)
- [x] **Brainstorm doc for IMDb × /pirata coupling — 3 passes, 6 reviewers each, scope landed at Phase 0+1 + deferred Phase 2** (session 003)
- [x] **Verified TC `search_content` does NOT accept `imdb_id` via MCP introspection** (session 003)
- [x] **Verified knowledge-hub `kb/` consumer story: searchable-not-filterable, pirata kb/ not registered** (session 003)
- [x] **Plan doc with 6 implementation units, locked Key Decisions, headless ce-doc-review pass, knowledge-hub follow-up** (session 003)
- [x] **Saved `feedback_show_reasoning.md` auto-memory** (session 003)
- [x] **Unit 1: `scripts/imdb_ingest.py` + `scripts/tests/test_imdb_ingest.sh` shipped, 46/46 PASS** (session 003)
- [x] **`.gitignore` extended for IMDb + runtime log artifacts** (session 003)

## Pending (By Priority)

### P1 — Urgent / Blocking

- [ ] **Free disk to >25 GB** — Phase 0 real-corpus ingest pre-flight requires this. Roger Rabbit alone is 5 GB in `downloads/`. If not done, `--refresh` exits 2 immediately. Check via `df -h /Users/vidigal` before continue-trigger fires.
- [ ] **Continue-trigger:** real-corpus ingest + Phase 0 acceptance verification + Unit 2 implementation (see top of this snapshot).

### P2 — Important

- [ ] **Unit 2: `scripts/imdb_lookup.py`** — FTS5+B-tree query layer with `lookup_by_title` / `lookup_by_tconst` / `lookup_episodes`. Composite score formula locked in plan. 20-item PT-BR fixture verification step locks the RapidFuzz threshold from data.
- [ ] **Unit 3: KB enrichment in `scripts/contact_sheet.py`** — manifest builder hook + sweep pass-through `--kb-imdb` flag + SKILL.md CONTRACT update (lockstep coupled in Unit 3 per pass-3 plan).
- [ ] **Unit 4: `/pirata` skill TC-failover wiring + event log** — SKILL.md workflow update + `scripts/skill_log.py` thin event-log writer + RESOLVED / TC STATUS row rendering.
- [ ] **Unit 5: TR-100 panel templates** — STATUS / DOCTOR / SHORTLIST updates in `menu-style.md`. Char-count verification.
- [ ] **Unit 6: Operations** — `imdb/state.json.example`, `imdb/README.md`, `scripts/pirata_evaluate.py` (Phase 2 gate evaluator with locked N_min=50 + both AND-clauses).
- [ ] After Phase 0 land, decide RAG ingestion target details (knowledge-hub `ingest_sync` op-step or alternative).
- [ ] (Carry-forward from 002) Liberar disco em geral; Roger Rabbit migration `contact/` → `contact-sheets/`.

### P3 — Nice to Have

- [ ] (Carry-forward) `--kb-prune`, `--kb-rebuild-manifest`, launchd plist for auto-sweep, IPTC/XMP via exiftool, mega-sheet "movie fingerprint", `--kb-export` flag in `queue.py`, `/pirata` UPDATE for RAG-query workflow, cross-rip dedup, `cols/rows` default decision.

## Technical Notes

**Stack additions for IMDb work:**
- macOS Python `sqlite3` stdlib v3.43.2 (FTS5 compiled in — verified).
- Future deps (Phase 1, NOT yet installed): `pip3 install parse-torrent-title rapidfuzz` (Unit 3 prereq + Unit 2 fuzzy match).
- IMDb dump location: `/Users/vidigal/claude-code/pirata/imdb/unnoficial/` (~9.3 GB, 7 TSVs).
- DB output: `/Users/vidigal/claude-code/pirata/imdb/imdb.db` (target ~500 MB-1 GB after Phase 0 ingest with the narrowed akas predicate).

**FTS5 schema chosen:**
```sql
CREATE VIRTUAL TABLE ft_titles USING fts5(
    title,
    title_source UNINDEXED,
    tconst       UNINDEXED,
    tokenize = 'unicode61 remove_diacritics 2',
    prefix = '2 3'
);
```

Plus B-tree `COLLATE NOCASE` indexes on `title_basics(primaryTitle)`, `title_basics(originalTitle)`, `title_akas(title)` for tier-1 exact match.

**Akas predicate (locked):**
```sql
WHERE region IN ('BR','PT','ES','MX','AR')
   OR language IN ('pt','en','es')
   OR isOriginalTitle = 1
```

**Disambig composite score formula (locked):**
- `score = fuzz_ratio_0_to_100 × field_multiplier`
- `fuzz_ratio = 100` for exact case-insensitive match; RapidFuzz `token_set_ratio` for fuzzy
- Field multipliers: `primaryTitle=3.0`, `originalTitle=2.0`, `aka isOriginal=1.8`, `aka regional=1.5`
- `numVotes` desc breaks ties when scores within 0.5
- Confidence threshold: top-1 within 15% of runner-up + both in tier 1 → multi-tie

**WAL atomic refresh sequence (locked, R3):**
1. Pre-flight: `<25 GB free → exit 2`. Cleanup stale `imdb/imdb.db.new*`.
2. Acquire `imdb/.refresh.lock` (flock, exit 2 if held).
3. Build at `imdb/imdb.db.new` with WAL pragmas.
4. `PRAGMA integrity_check` → exit 3 + leave live DB intact on fail.
5. `PRAGMA wal_checkpoint(TRUNCATE)` to fold WAL.
6. Promote previous `imdb/imdb.db` → `imdb/imdb.db.prev` (rollback gen).
7. `os.replace(imdb.db.new, imdb.db)` (atomic on APFS).
8. Cleanup orphan `.db.new-wal` / `.db.new-shm` post-swap.
9. Write `imdb/state.json` via temp + atomic rename.
10. Release lock.

## Key Files

**This session (4 commits on main):**
- `docs/brainstorms/2026-04-24-imdb-local-pirata-coupling-requirements.md` — 164 lines. Origin doc for the plan. 3 review passes' worth of decisions captured.
- `docs/plans/2026-04-24-004-feat-imdb-local-pirata-coupling-plan.md` — ~550 lines. 6 implementation units, locked Key Decisions, Risks table, Mermaid HLD diagram, Output Structure tree.
- `scripts/imdb_ingest.py` — 521 lines. CLI ingest with WAL-safe atomic refresh. Phase 0 Unit 1 deliverable.
- `scripts/tests/test_imdb_ingest.sh` — ~270 lines, 23 scenarios via synthesized fixture TSVs. 46/46 PASS.
- `.gitignore` — extended with imdb + runtime log entries.
- `~/.claude/projects/-Users-vidigal-claude-code-pirata/memory/feedback_show_reasoning.md` — auto-memory: lead recommendations with both-sides reasoning, not consensus appeals.

**Carried over from 002 (pre-existing, unchanged):**
- `scripts/contact_sheet.py`, `scripts/sheets_sweep.py`, `scripts/queue.py`
- `scripts/tests/test_sweep.sh`, `scripts/tests/test_kb_export.sh`
- `.claude/skills/pirata-deck/SKILL.md`, `.claude/skills/pirata-deck/references/menu-style.md`
- `kb/per-movie/who-framed-roger-rabbit-1988.json` + supporting `kb/frames/`, `kb/contact-sheets/`, `kb/manifest.jsonl`
- `imdb/unnoficial/*.tsv` (7 files, 9.3 GB raw)

## Warnings & Gotchas

- **Pre-flight disk gate:** real ingest needs ≥25 GB free. Workspace was at ~9% free / 166 GB at start of session 002. Verify with `df -h /Users/vidigal` before continue-trigger fires; if insufficient, free disk first or override `--min-free-gb 5` (risky — peak refresh = old DB + new TSVs + new DB temp).
- **Real ingest performance is unmeasured:** `<10 min on M-series Mac` is the plan target. Untested at 12.4M titles + 99M principals scale. Bulk ingest strategy is `csv.reader` + `executemany` chunks of 50k. If wall-clock exceeds 15-20 min, profile and optimize. Memory peak around `series_top_cast` aggregation (cross-join title_episode × title_principals_top5 with window function) and the FTS5 INSERTs.
- **FTS5 INSERT memory:** chose 3 sequential INSERTs (not UNION ALL) per pass-2 adversarial review. Should be safe but unmeasured at scale.
- **Sort assumption guard:** uses a `set` of all seen tconsts (~1 GB transient memory at full scale). Cleanly freed after ingest. If memory becomes a bottleneck during Phase 0 verification, can be made optional via `--no-sort-check` flag (already wired) — but unsafe (silent drop of duplicate-tconst batches).
- **knowledge-hub consumer reality:** the plan's KB enrichment value claim is now `searchable-not-filterable` — tconst becomes a BM25 hit, not a structured filter. If a future RAG consumer DOES need structured filtering, that's Phase 2 work (separate retrieval layer or knowledge-hub schema design). Don't over-promise downstream value.
- **RESOLVED row truncation:** the formula in the plan was BUGGED in earlier drafts (off by 9 chars). Final locked formula uses `tconst[:8] + "..."` (8 char abbrev) and right-truncates title to budget = 16 chars. Implementer of Unit 5 must use the rendering function from `imdb_lookup.py`, not re-derive.
- **CONTRACT coupling:** Unit 3's `--kb-imdb` flag addition to `contact_sheet.py` + SKILL.md `:163` CONTRACT update MUST land in the same commit. Plan was updated to put both in Unit 3 Files. If split across commits, DOCTOR reports `[FAIL] sheet contract drift` between commits.
- **Phase 2 default is closed:** `pirata_evaluate.py` verdict logic locks `N_min=50 events`, both AND-clauses required for REOPEN. The 2-10 % zone defaults to STAY-CLOSED. Burden of proof on reopening, not on closing — by design.
- **state.json + .gitignore interaction:** runtime `imdb/state.json` is gitignored (per pass-2 consensus). The tracked stub is `imdb/state.json.example` — Unit 6 owns creating it. Phase 0 Unit 1 alone DOES write `imdb/state.json` at runtime; that's gitignored, no `git status` noise.
- **Pre-existing untracked files:** `.claude/commands/`, `CLAUDE.md`, `MEMORY_DEEP_001/002.md`, `kb/`, `logs/`, prior brainstorm + 3 plans — none from this session. NEVER auto-commit these without asking the user; they may have reasons not to have committed them.
- **Don't run /loop or autonomous-loop on the continue trigger:** the trigger is conditional on user typing the resume word. Auto-trigger means "when user types continue, fire the action without re-asking", NOT "fire the action periodically". The trigger lives in this snapshot doc; honor it on resume only.
