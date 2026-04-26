---
title: "feat: hunter.py — TorrentClaw + pirata orchestrator"
type: feat
status: active
date: 2026-04-24
---

# feat: hunter.py — TorrentClaw + pirata orchestrator

## Overview

Build a single-file Python CLI (`scripts/hunter.py`) that unites rich metadata from the
**TorrentClaw HTTP API** with the download capability of the **pirata** CLI (already
installed globally via `cargo install`, configured to `./downloads/` in this workspace).

The orchestrator exposes human-friendly subcommands (`find`, `grab`, `show`,
`where`, `watchlist`, `scan`) that batch, rank, filter, and delegate. Metadata lives
at TorrentClaw; downloads happen via `pirata add <magnet>` (or `aria2c` direct as
fallback). The two tools are complementary: TorrentClaw is the brain, pirata is the
brawn.

## Problem Frame

The user has two complementary torrent tools in this workspace:

- **`pirata`** — Rust CLI wrapping a PirateBay scraper. Strong on downloading (aria2
  integration, TUI, configured target dir). Weak on metadata (no ratings, no cast, no
  streaming check, no HDR/codec filters).
- **`torrentclaw-mcp`** — MCP server wrapping `https://torrentclaw.com` API. Strong
  on metadata (IMDb/TMDB ratings, streaming providers, HDR/audio/quality filters, TV
  season/episode routing). Only exposed to Claude Code, not usable from shell, and
  does not download.

The gap: outside of a Claude Code session, the user cannot leverage TorrentClaw's
rich search. And inside Claude Code, the MCP returns magnet URLs but cannot pipe
them into the local downloader.

This plan closes the gap with a thin Python orchestrator that calls TorrentClaw's
HTTP API directly and shells out to pirata/aria2c.

## Requirements Trace

- **R1.** Expose TorrentClaw search + metadata from a plain shell CLI (no MCP, no
  Claude required).
- **R2.** Rank and filter torrent results by seeders, quality, HDR, size, before
  presenting options.
- **R3.** One-shot "best match" flow that picks the top torrent and kicks off
  `pirata add` without user intervention.
- **R4.** TV season batch mode: enumerate episodes of a show/season and download
  all available.
- **R5.** Streaming-first check: optionally verify whether content is on Netflix /
  Disney+ / Prime for the user's country before torrenting.
- **R6.** Watchlist mode: given `watchlist.txt`, check for new/updated releases
  and download only new matches (dedup via `.hunter-seen.json`).
- **R7.** All downloads land in `./downloads/` (the pirata-configured target dir,
  already verified via `pirata doctor`).
- **R8.** Rich terminal output (tables, colors) without a heavyweight TUI.

## Scope Boundaries

- **Non-goal:** Reimplementing pirata's TUI or search. Hunter delegates download
  to pirata, not the other way around.
- **Non-goal:** Building a daemon, web UI, or long-lived background service.
- **Non-goal:** Mutating pirata's config or wrapping its commands as a Python lib.
- **Non-goal:** Supporting the `transmission` downloader path (pirata is pinned
  to `aria2` in this workspace; transmission binaries are missing per `pirata doctor`).
- **Non-goal:** Calling the torrentclaw-mcp stdio server from Python. We hit the
  HTTP API directly — it's the same backend the MCP wraps.

### Deferred to Separate Tasks

- **Rate-limit-aware scheduling for `watchlist` cron execution** — punted to a
  future iteration once usage patterns surface.
- **Packaging as `hunter` on PATH via `pipx install -e .`** — separate follow-up
  after the single-file version proves itself.

## Context & Research

### Relevant Code and Patterns

- **`src/` (this repo, Rust)** — `Cargo.toml`, `src/config.rs` defines the toml
  config; we do not modify these. Hunter reads pirata's config indirectly via
  `pirata doctor --json` to discover the download dir at runtime.
- **`~/.config/pirata/config.toml`** — already written with
  `[aria2] download_dir = "/Users/vidigal/claude-code/pirata/downloads"`. Hunter
  respects this by calling `pirata add <magnet>` (default path) or reading
  `pirata --json doctor` to resolve the dir when falling back to `aria2c` direct.
- **No existing `scripts/` or `docs/` convention in this workspace.** Hunter
  introduces `scripts/hunter.py` as a new top-level directory.

### TorrentClaw HTTP API (discovered from cloned `torrentclaw/torrentclaw-mcp` source)

Base URL: `https://torrentclaw.com` (overridable via `TORRENTCLAW_API_URL`).

**Auth headers (all requests):**
```
User-Agent: hunter-cli/<version>
Accept: application/json
X-Search-Source: cli          # different from MCP's "mcp" — useful for telemetry
Authorization: Bearer <key>   # optional, from TORRENTCLAW_API_KEY env var
```

**Endpoints:**

| Method | Path | Purpose |
|---|---|---|
| GET | `/api/v1/search` | Primary search with filters |
| GET | `/api/v1/autocomplete?q=...` | Up to 8 title suggestions |
| GET | `/api/v1/popular?limit&page&locale` | Popular content by clicks |
| GET | `/api/v1/recent?limit&page&locale` | Recently added |
| GET | `/api/v1/content/{id}/watch-providers?country=XX` | Streaming availability |
| GET | `/api/v1/content/{id}/credits` | Cast + director |
| GET | `/api/v1/stats` | Catalog stats |
| POST | `/api/v1/track` | Telemetry: `{infoHash, action}` where action ∈ magnet/torrent_download/copy |
| POST | `/api/v1/scan-request` | Submit for TrueSpec: `{infoHash, email, website:""}` |
| GET | `/api/v1/scan-request/{infoHash}` | Poll scan status |
| GET | `/api/v1/torrent/{infoHash}` | Download `.torrent` file bytes |

**Search params (on-wire keys differ from MCP tool schema):**

- `q` (not `query`), required, 1–200 chars, no control chars
- `type` ∈ `{movie, show}`
- `genre` (≤50 chars, `[a-zA-Z\s&-]+`)
- `year_min`, `year_max` (int)
- `min_rating` (0–10)
- `quality` ∈ `{480p, 720p, 1080p, 2160p}`
- `lang` (not `language`) — ISO 639-1 lowercase 2-letter
- `audio` (alphanumeric+dots, substring match: `aac`, `dts`, `atmos`, ...)
- `hdr` ∈ `{hdr10, dolby_vision, hdr10plus, hlg}`
- `availability` ∈ `{all, available, unavailable}`
- `season` (0–99), `episode` (0–999)
- `locale` (ISO 639-1), `country` (ISO 3166-1 UPPER)
- `sort` ∈ `{relevance, seeders, year, rating, added}` (default `relevance`)
- `page` (1–1000), `limit` (1–50, default 20)

**SearchResult shape (excerpt relevant to hunter):**

```
id, title, year, genres[], ratingImdb, ratingTmdb, hasTorrents,
torrents: [{
  infoHash, magnetUrl, torrentUrl, seeders, leechers,
  quality, codec, sourceType, sizeBytes, qualityScore,
  hdrType, audioCodec, languages[], releaseGroup,
  isProper, isRepack, isRemastered,
  season, episode,
  audioTracks[], subtitleTracks[], videoInfo{codec,width,height,hdr,...},
  scanStatus
}],
streaming?: { flatrate[], rent[], buy[], free[] }
```

**Error codes (with messages from MCP source):**

- 400 bad request, 401 needs API key, 403 tier insufficient, 404 not found,
  429 rate limit (wait 10–30s), 500/502/503 server transient.
- Retry strategy: only 429, exponential backoff `1s → 2s → 4s`, capped at 10s,
  max 2 retries.
- Request timeout: 15s.
- Response cache: TTL 5min, LRU, max 200 entries (hunter mirrors this to avoid
  hammering the API during `watchlist` sweeps).

### Institutional Learnings

- **ffmpeg/fal-ai rules** in `~/.claude/rules/` do not apply (no media processing,
  no fal.ai).
- **python-background rule** applies only if watchlist is ever run via
  `run_in_background` — use `python3 -u` for unbuffered stdout. Not in scope for
  v1 (watchlist runs foreground).
- **Brand-safety rule** does not apply (no image-gen).

### External References

- TorrentClaw MCP source: `https://github.com/torrentclaw/torrentclaw-mcp`
  (read: `src/api-client.ts`, `src/types.ts`, `src/tools/search-content.ts`,
  `src/config.ts`).
- pirata README: `README.md` in this workspace.

## Key Technical Decisions

- **Single file, stdlib + 2 deps.** `scripts/hunter.py` uses only `httpx` (async
  isn't needed but httpx has cleaner ergonomics than stdlib urllib) and `rich`
  (tables, colors). Rationale: zero-install friction — `pip install httpx rich`
  is the bar. Rejected alternatives: `requests` (fine, but httpx is strictly
  better and costs nothing), a full Typer/Click CLI package (overkill for 6
  commands; argparse is sufficient).
- **Argparse, not Click/Typer.** Stdlib, less magic, sufficient for the command
  surface we need.
- **Direct HTTP, not MCP stdio.** MCP is a thin wrapper over the same
  `https://torrentclaw.com/api/v1/*` endpoints. Calling HTTP directly from Python
  is simpler and lets hunter work outside Claude Code.
- **Delegate download via `subprocess.run(["pirata", "add", magnet])`** rather
  than talking to aria2 RPC. Keeps hunter coupled to pirata's user-facing contract
  (stable) and lets pirata's downloader config stay the source of truth. Fallback
  to `aria2c --dir=<pirata-dir> "<magnet>"` only if pirata is missing.
- **Ranking function.** `score = seeders * weight_seeders + qualityScore *
  weight_quality - size_gb * penalty_size`. Configurable via CLI flags
  (`--weight-seeders`, `--weight-quality`, `--penalty-size`) but with sensible
  defaults that prefer ≥10 seeders, ≥1080p, ≤8GB for movies.
- **Dedup for watchlist via content-addressed set.** `.hunter-seen.json` stores
  `{infoHash: {title, grabbed_at}}`. Re-running watchlist never re-grabs the
  same hash. Title renames on TorrentClaw don't cause false dups since we key
  on infoHash.
- **Locale/country defaulting.** Default `country=BR` (inferred from the user
  being on a pt-BR system per workspace memory). Overridable via
  `HUNTER_COUNTRY` env var or `--country` flag per-command.
- **API key is optional.** `TORRENTCLAW_API_KEY` env var; without it, anonymous
  rate limits apply and `hunter` surfaces that in `hunter doctor`.
- **No POST telemetry by default.** `/api/v1/track` and `/api/v1/scan-request`
  (which posts user email) are behind explicit `--track` and `--scan` flags. We
  never auto-send the user's email or infoHash for telemetry unless asked.
- **Cache on disk, not in memory.** `~/.cache/hunter/` with TTL 5min on search
  responses. Survives between CLI invocations — a watchlist sweep that hits 20
  titles in 10 seconds stays polite. File format: one JSON per cache key
  (hashed URL).

## Open Questions

### Resolved During Planning

- **Does TorrentClaw expose magnet URLs directly?** Yes — `SearchResult.torrents[].magnetUrl`
  is always present when `hasTorrents=true`. No need to call `get_torrent_url`
  unless we want the `.torrent` file bytes.
- **Do we need the MCP running at all?** No — pure HTTP. The MCP stays registered
  in Claude Code for LLM-driven use; hunter runs independently.
- **What's the "best match" heuristic?** See Key Technical Decisions — weighted
  score, user-tunable, opinionated default.
- **Does pirata support magnet URIs as input to `pirata add`?** Yes per README
  (`pirata add 81462446` works with torrent id; README also shows `--downloader`
  and `--open` flags — magnet URIs are the standard input format for adding
  magnets via aria2; confirmed by reading `src/` CLI). Fallback path uses
  `aria2c` direct if `pirata add <magnet>` fails.

### Deferred to Implementation

- **Exact argparse subparser layout** — likely 6 subparsers under one `hunter`
  root, but tree shape is an implementation detail.
- **rich table column widths / truncation rules** — tune during implementation
  when real output is visible.
- **Whether to short-circuit `grab` when streaming providers are available and
  `--prefer-streaming` is set** — explicit flag; default behavior is torrent-first
  (the whole point of hunter). Design during Unit 5.

## Output Structure

```
scripts/
├── hunter.py             # single-file CLI entry point
└── hunter/               # optional module split if file grows past ~800 lines
    ├── api.py            # TorrentClaw HTTP client
    ├── rank.py           # scoring + filter helpers
    ├── downloader.py     # pirata / aria2c delegation
    └── watchlist.py      # watchlist + seen-set logic

tests/
└── test_hunter.py        # pytest, offline-only (mocked HTTP)

docs/
└── plans/                # this document lives here

watchlist.txt             # user-managed, one title per line
.hunter-seen.json         # auto-generated dedup state (gitignored)
```

The start is single-file `scripts/hunter.py`. Module split into `scripts/hunter/`
is only triggered if the file passes ~800 LOC — deferred to implementation.

## High-Level Technical Design

> *This illustrates the intended approach and is directional guidance for review, not implementation specification. The implementing agent should treat it as context, not code to reproduce.*

**Command tree:**

```
hunter find QUERY [--type movie|show] [--quality 1080p|2160p] [--hdr hdr10|dolby_vision]
                  [--min-seeders N] [--min-rating 7] [--limit 10] [--sort seeders]
hunter grab  QUERY [...same filters...] [--dry-run] [--yes]
hunter show  QUERY --season N [--episode M] [--yes]
hunter where QUERY [--country BR] [--prefer-streaming]
hunter watchlist [--file watchlist.txt] [--dry-run]
hunter scan INFOHASH [--email EMAIL] [--wait]
hunter doctor
```

**Request/response flow for `hunter grab "dune 2" --quality 2160p --hdr dolby_vision`:**

```
hunter grab
  → api.search(q="dune 2", quality="2160p", hdr="dolby_vision", sort="seeders", limit=20)
    → GET https://torrentclaw.com/api/v1/search?q=dune%202&quality=2160p&hdr=dolby_vision&sort=seeders&limit=20
    ← SearchResponse{results: [...]}
  → rank.best_torrent(results, filters={min_seeders: 5, max_size_gb: 40})
    ← TorrentInfo{magnetUrl, infoHash, seeders, ...}
  → downloader.dispatch(magnet)
    → subprocess.run(["pirata", "add", magnet], check=True)
    → pirata → aria2 → ./downloads/<name>
  → state.mark_seen(infoHash, title)
```

**Watchlist flow:**

```
watchlist.txt          .hunter-seen.json
      │                        │
      └──► parse titles ──►   dedup
                ▼
         for each title:
           search → rank → filter out seen hashes → grab top N
                                                     │
                                                     └─► update seen
```

## Implementation Units

- [ ] **Unit 1: TorrentClaw HTTP client (`scripts/hunter.py` — api layer)**

**Goal:** Typed Python client for all 11 TorrentClaw endpoints, with retry, timeout,
and local cache. No CLI yet.

**Requirements:** R1

**Dependencies:** None

**Files:**
- Create: `scripts/hunter.py` (api section) — dataclasses or TypedDicts mirroring
  `SearchResponse`, `SearchResult`, `TorrentInfo`, `StreamingInfo`,
  `WatchProvidersResponse`, `AutocompleteResponse`, `CreditsResponse`,
  `StatsResponse`, `ScanRequestResponse`.
- Create: `tests/test_hunter.py` (api tests)
- Create: `pyproject.toml` minimal (just declares `httpx`, `rich`, `pytest` dev dep)

**Approach:**
- `TorrentClawClient` class with `base_url`, `api_key` from env, `user_agent`.
- `X-Search-Source: cli` header to distinguish from MCP traffic.
- Methods: `search()`, `autocomplete()`, `get_popular()`, `get_recent()`,
  `get_watch_providers()`, `get_credits()`, `get_stats()`, `get_torrent_file()`,
  `track()`, `submit_scan()`, `get_scan_status()`.
- Retry only on 429: exponential backoff (1s, 2s, 4s), max 2 retries.
- Timeout: 15s per request.
- Disk cache under `~/.cache/hunter/` keyed by sha256(url), TTL 5min. Only
  cache GETs. POSTs (`track`, `scan-request`) never cached.
- Error hierarchy: `HunterAPIError` with `.status` attribute; subclasses
  `RateLimitError`, `NotFoundError`, `AuthError`, `ServerError`.
- On 401/403, include hint: "set TORRENTCLAW_API_KEY env var".

**Patterns to follow:** The MCP's `src/api-client.ts` is the reference
implementation — mirror its fetchWithRetry, ResponseCache, and error mapping
but in Python idioms.

**Test scenarios:**
- Happy path: `search(query="ubuntu")` returns a `SearchResponse` with `total`,
  `page`, `results[]` when httpx is mocked to return the documented JSON.
- Happy path: `search` URL-encodes `q` and maps `language → lang` param correctly.
- Edge case: empty query raises `ValueError` before any HTTP call (client-side
  validation matches the 1–200 char constraint).
- Edge case: `quality="4k"` raises `ValueError` (not in enum); only
  `480p/720p/1080p/2160p` accepted.
- Error path: 429 response triggers retry with correct backoff, succeeds on
  second attempt. Assertable via mock call count + sleep patches.
- Error path: 429 on all 3 attempts raises `RateLimitError`.
- Error path: 401 raises `AuthError` with message mentioning `TORRENTCLAW_API_KEY`.
- Error path: 15s timeout raises `HunterAPIError` (not a bare httpx exception).
- Integration: cache round-trip — second identical call within TTL does not
  hit httpx; second call after TTL expiry does hit httpx.
- Integration: `search(country="br")` raises `ValueError` (must be uppercase
  per `/^[A-Z]{2}$/` regex).

**Verification:**
- `pytest tests/test_hunter.py -k api` passes.
- `python scripts/hunter.py --help` (argparse skeleton) runs without import errors.

---

- [ ] **Unit 2: `hunter find` — ranked search and display**

**Goal:** Interactive-free search command that prints a rich table of top matches
with torrent metadata.

**Requirements:** R1, R2, R8

**Dependencies:** Unit 1

**Files:**
- Modify: `scripts/hunter.py` (add `cmd_find`, argparse `find` subparser,
  `format_results` using `rich.table.Table`)
- Modify: `tests/test_hunter.py` (find-command tests)

**Approach:**
- Subparser `find` with args: `query`, `--type`, `--quality`, `--hdr`,
  `--min-seeders`, `--min-rating`, `--year-min`, `--year-max`, `--audio`,
  `--lang`, `--sort` (default `seeders`), `--limit` (default 10),
  `--country` (default from `HUNTER_COUNTRY` env or `BR`),
  `--json` (raw JSON dump, for scripting).
- Calls `client.search()`, flattens `results[].torrents[]`, applies
  client-side `--min-seeders` filter (server doesn't enforce seeder thresholds,
  only `availability`).
- rich Table columns: `#`, `Title (Year)`, `Quality`, `HDR`, `Seeders`,
  `Size`, `Source`, `Group`, `IMDb`.
- Last column truncated to avoid horizontal overflow on 120-col terminals.

**Patterns to follow:** N/A — greenfield.

**Test scenarios:**
- Happy path: given a mocked 3-result response, `cmd_find("ubuntu")` prints
  a table with 3 rows and returns exit code 0.
- Happy path: `--json` mode emits parseable JSON to stdout, no ANSI codes.
- Edge case: zero results prints "No torrents found" and exits 0 (no error —
  empty is a valid answer).
- Edge case: `--min-seeders 10` filters torrents whose server response had
  `seeders<10` even if they came back in the API response.
- Error path: `RateLimitError` from client prints a friendly message and
  exits with code 2 (not a stack trace).

**Verification:**
- `python scripts/hunter.py find "ubuntu 24.04" --limit 5` prints a table.
- `python scripts/hunter.py find "ubuntu" --json | jq .total` returns a number.

---

- [ ] **Unit 3: `hunter grab` — best-match auto-download**

**Goal:** One-shot flow that searches, ranks, picks best, hands magnet to pirata.

**Requirements:** R3, R7

**Dependencies:** Unit 1, Unit 2 (reuses filter + ranking logic)

**Files:**
- Modify: `scripts/hunter.py` (add `cmd_grab`, `rank_torrents`, `dispatch_download`)
- Modify: `tests/test_hunter.py` (grab-command + ranking tests)

**Approach:**
- Ranking: `score = seeders * 1.0 + qualityScore * 2.0 - size_gb * 0.3`.
  `qualityScore` comes from the API. `size_gb` parsed from `sizeBytes` string.
- Hard filters (drop-before-score): `seeders < --min-seeders (default 3)`,
  `sizeBytes > --max-size` if set, `hasTorrents=false`.
- Picks top torrent after ranking.
- `dispatch_download(magnet)`:
  1. Tries `shutil.which("pirata")` → `subprocess.run(["pirata", "add", magnet], check=True)`
  2. If pirata missing or returns non-zero, falls back to
     `aria2c --dir=<pirata-dir> --seed-time=0 "<magnet>"` where pirata-dir is
     read from `pirata --json doctor` or defaults to `./downloads`.
  3. `--dry-run` prints the chosen magnet + command it would run, exits 0.
- `--yes` skips the confirmation prompt (default: asks `[y/N]`).

**Patterns to follow:** N/A.

**Test scenarios:**
- Happy path: ranks 5 torrents by the documented formula, picks the highest
  score. Assertable by constructing `TorrentInfo` fixtures with known values.
- Happy path: `--dry-run` calls subprocess zero times, prints the chosen magnet.
- Edge case: all torrents filtered out (below min-seeders) → prints "no candidates
  meet criteria" and exits code 3.
- Edge case: `sizeBytes=None` — treat as unknown size, rank with size penalty 0
  rather than crashing.
- Error path: pirata command returns non-zero → falls back to aria2c; if aria2c
  also missing, exits code 4 with "no downloader available" message.
- Integration: when subprocess is mocked to succeed, seen-set is updated with
  the chosen `infoHash`.

**Verification:**
- `python scripts/hunter.py grab "ubuntu 24.04" --dry-run` prints the selected
  magnet and command without downloading.
- `python scripts/hunter.py grab "ubuntu 24.04" --yes` fires `pirata add` and
  a real `.iso` begins landing in `./downloads/`.

---

- [ ] **Unit 4: `hunter show` — TV season batch**

**Goal:** Enumerate a show's season and grab all episodes matching the filters.

**Requirements:** R4

**Dependencies:** Unit 3

**Files:**
- Modify: `scripts/hunter.py` (add `cmd_show`)
- Modify: `tests/test_hunter.py`

**Approach:**
- Subparser `show` with args: `query`, `--season N` (required), `--episode M`
  (optional — if set, single episode), quality/HDR filters.
- Strategy: call `client.search(query, type="show", season=N)` — API already
  understands season routing and returns torrents filtered by season.
- For each unique episode in results (grouped by `torrent.episode`), pick the
  best-ranked torrent and dispatch download.
- `--episode M` short-circuits to a single-episode grab (equivalent to `grab`
  with season+episode).
- Prints a summary table (episode → chosen torrent → status).

**Patterns to follow:** Reuse `rank_torrents` and `dispatch_download` from Unit 3.

**Test scenarios:**
- Happy path: given a mocked search with 10 torrents spanning episodes 1–10,
  picks best per episode and dispatches 10 downloads.
- Happy path: `--episode 5` only dispatches for episode 5.
- Edge case: season has gap (episode 3 missing entirely from API) — prints
  "episode 3: no torrents found", continues with the rest, exits 0.
- Edge case: duplicate episodes across multiple release groups rank correctly;
  only top-ranked is dispatched per episode.
- Integration: seen-set excludes already-downloaded episodes on a second run
  (idempotent re-run).

**Verification:**
- `python scripts/hunter.py show "bluey" --season 1 --dry-run` prints a
  per-episode selection table.

---

- [ ] **Unit 5: `hunter where` — streaming providers check**

**Goal:** Look up streaming availability for a title in the user's country.

**Requirements:** R5

**Dependencies:** Unit 1

**Files:**
- Modify: `scripts/hunter.py` (add `cmd_where`)
- Modify: `tests/test_hunter.py`

**Approach:**
- Subparser `where` with args: `query`, `--country` (default from env/BR),
  `--prefer-streaming` (if set and a flatrate provider is available, suggests
  skipping torrent).
- Flow: `autocomplete(q)` → pick top match (or prompt if `--interactive`)
  → `get_watch_providers(content_id, country)` → render rich table with
  flatrate / rent / buy / free columns.
- Attribution footer (required per TorrentClaw API response
  `WatchProvidersResponse.attribution`).

**Patterns to follow:** N/A.

**Test scenarios:**
- Happy path: mocked autocomplete + watch-providers returns a title available
  on Netflix; printed table shows Netflix under "Stream" column.
- Edge case: no autocomplete match → prints "title not found" exits code 3.
- Edge case: content found but no providers in the country → prints "not
  available for streaming in BR", exits 0.
- Integration: `--prefer-streaming` + flatrate match prints a suggestion:
  "Available on Netflix (BR) — consider streaming instead of torrenting."

**Verification:**
- `python scripts/hunter.py where "oppenheimer" --country BR` prints providers.

---

- [ ] **Unit 6: `hunter watchlist` — recurring auto-grab**

**Goal:** Batch-grab every title in `watchlist.txt` that has a new release since
the last run.

**Requirements:** R6, R7

**Dependencies:** Unit 3

**Files:**
- Modify: `scripts/hunter.py` (add `cmd_watchlist`, `load_seen`, `save_seen`)
- Create: `.gitignore` entry for `.hunter-seen.json` (in a separate commit or
  appended to existing — check at implementation time)
- Create: `watchlist.example.txt` at workspace root with sample content
- Modify: `tests/test_hunter.py`

**Approach:**
- Subparser `watchlist` with args: `--file` (default `watchlist.txt`),
  `--dry-run`, `--limit-per-title` (default 1).
- Parses `watchlist.txt`: one title per line, lines starting `#` are comments,
  blank lines skipped. Optional `key=value` suffix parses into filters
  (e.g., `dune 2 quality=2160p hdr=dolby_vision`).
- For each title: search → rank → filter out seen hashes → grab top N
  (default 1).
- `.hunter-seen.json` at workspace root: `{infoHash: {title, grabbed_at}}`.
  Loaded at start, saved after each successful grab (not atomic — a crash
  mid-run may cause one re-grab, acceptable).
- Prints a summary at end: N titles processed, M grabs dispatched, K skipped
  (already seen), E errors.

**Patterns to follow:** Reuse `rank_torrents`, `dispatch_download`, existing
client.

**Test scenarios:**
- Happy path: 3-line watchlist, all new — 3 downloads dispatched, seen file
  has 3 entries.
- Happy path: 3-line watchlist, middle entry already in seen — 2 downloads
  dispatched.
- Edge case: malformed line `dune 2 quality=9999p` → skip line with warning,
  continue with rest, exit 0 (non-fatal).
- Edge case: watchlist file missing → clear error message, exit code 5.
- Edge case: all titles fail — exits non-zero but prints per-title error
  summary.
- Integration: run twice in a row — second run is a no-op (all seen).
- Integration: `--dry-run` prints the grab plan without dispatching or
  mutating seen file.

**Verification:**
- Create a fake `watchlist.txt` with `ubuntu 24.04`, run
  `python scripts/hunter.py watchlist --dry-run` — prints the plan.
- Run without `--dry-run` — one ISO lands in `./downloads/`.
- Re-run — no new downloads, "already seen" summary.

---

- [ ] **Unit 7: `hunter scan` and `hunter doctor` — utility commands**

**Goal:** Two small utility commands: TrueSpec scan submission/polling and a
health check.

**Requirements:** R1 (doctor needed for integration verification)

**Dependencies:** Unit 1

**Files:**
- Modify: `scripts/hunter.py` (add `cmd_scan`, `cmd_doctor`)
- Modify: `tests/test_hunter.py`

**Approach:**
- `scan INFOHASH [--email EMAIL] [--wait]` — submits scan request, optionally
  polls `get_scan_status` every 30s until `completed` or timeout (5min).
  `--email` required for submit (API constraint); persist to
  `~/.config/hunter/config.toml` on first use.
- `doctor` — prints:
  - TorrentClaw API reachability (`GET /api/v1/stats` round-trip)
  - Auth mode (anonymous / bearer token present)
  - pirata binary path + version
  - aria2c binary path
  - Configured download dir (read from pirata)
  - Python dependencies present
  - Seen file path + entry count

**Patterns to follow:** pirata's own `pirata doctor` output as inspiration.

**Test scenarios:**
- Happy path: `doctor` with all deps present exits 0 with green checkmarks.
- Edge case: `doctor` with pirata missing exits 0 but flags in red with a hint
  (not an error — doctor reports, never fails).
- Happy path: `scan <hash> --wait` polls until status=completed (mocked).
- Error path: `scan` with no email and no saved config → prompts for email
  (or errors with `--no-interactive` flag).

**Verification:**
- `python scripts/hunter.py doctor` shows the workspace state.

## System-Wide Impact

- **Interaction graph:** Hunter shells out to `pirata` (and falls back to
  `aria2c`). It reads `~/.config/pirata/config.toml` indirectly via
  `pirata --json doctor`. It reads/writes `.hunter-seen.json` and
  `~/.cache/hunter/`. It never mutates pirata's config or the Rust binary.
- **Error propagation:** API errors raise `HunterAPIError` subclasses caught at
  the `cmd_*` layer, mapped to friendly messages + non-zero exit codes (2–5
  range for different error classes). subprocess errors from pirata/aria2c are
  surfaced with the full stderr for debuggability.
- **State lifecycle risks:** `.hunter-seen.json` write is not atomic. A crash
  mid-run may lose the seen entries for grabs that started after the last
  save. Mitigation: save after every successful grab, not at end-of-run.
  Accepting the risk of at-most-once re-grab across crashes (not duplicate
  downloads — aria2 handles idempotency on same magnet via info-hash).
- **API surface parity:** Hunter exposes a strict subset of TorrentClaw
  endpoints; we do not wrap every MCP tool. `get_popular`, `get_recent`,
  `track` are intentionally not exposed in v1 — add if user asks.
- **Integration coverage:** Mocked httpx covers API layer. Real pirata
  invocation is covered by one manual verification per relevant unit, not
  CI — running pirata in CI requires a real network and magnet sources.
- **Unchanged invariants:** pirata's CLI, config, binary location, download
  dir — all untouched. TorrentClaw API contract — hunter is a consumer only,
  zero-impact on the upstream service. `Cargo.toml` / Rust source — not modified.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| TorrentClaw API rate-limits without API key (watchlist sweeps hit it hardest) | Disk cache with 5min TTL; bail with clear message on 429 after retries; doctor flags anonymous mode |
| TorrentClaw API goes down or changes schema | Graceful 5xx handling; pin `api/v1` in paths; error messages surface raw status to help debugging |
| pirata contract changes (`pirata add` flag) | Pin reliance on `pirata add <magnet>` (documented in README); fallback to aria2c direct keeps hunter functional if pirata breaks |
| Magnet parsing edge cases | Validate magnet URI shape before passing to subprocess; reject if missing `xt=urn:btih:` prefix |
| Python dep drift (httpx major version) | Pin via `pyproject.toml` `httpx>=0.27,<1.0`, `rich>=13,<15` |
| Large seen file grows unbounded | Not in v1; trivially capped via LRU or `--prune-older-than N days` flag if needed later |
| User misconfigures `watchlist.txt` | Per-line parsing tolerates typos/malformed filters; logs a warning, continues with next line |
| TrueSpec email requirement leaks user email in logs | Never logged; stored in `~/.config/hunter/config.toml` with 0600 perms |

## Documentation / Operational Notes

- Add a `## Hunter orchestrator` section to `README.md` (this repo's top-level
  README) with:
  - Install snippet: `pip install httpx rich`
  - Command summary with one-line examples for each subcommand
  - Pointer to the plan doc for design rationale
- `watchlist.example.txt` with comments explaining format
- `.gitignore` entries: `.hunter-seen.json`, `watchlist.txt` (user-local, not
  shared), `~/.cache/hunter/` is already outside the repo

## Sources & References

- TorrentClaw MCP source (authoritative for API shape):
  `https://github.com/torrentclaw/torrentclaw-mcp`
  - `src/api-client.ts` — all 11 endpoint implementations + retry/cache
  - `src/types.ts` — response schemas
  - `src/tools/search-content.ts` — zod param validation (enums, regex, ranges)
  - `src/config.ts` — env var handling
- pirata docs: `README.md` (this repo), `src/config.rs` (config schema)
- Per-workspace MCP registration: `~/.claude.json` (already configured)
- pirata config already set: `~/.config/pirata/config.toml` — download_dir
  pinned to `/Users/vidigal/claude-code/pirata/downloads`
