#!/usr/bin/env python3
"""IMDb non-commercial dataset ingest into local SQLite + FTS5.

Reads TSVs from imdb/unnoficial/ (or any --source dir) and builds
imdb/imdb.db with all tables required by Phase 1 KB enrichment and
Phase 1 /pirata skill TC-failover lookup.

Tables built:
- title_basics, title_ratings, title_episode, title_crew  (1:1 from TSV)
- title_principals_top5  (streaming top-5 per tconst, category filtered)
- title_akas             (filtered: PT/EN/ES regions + langs + isOriginal)
- name_basics            (1:1; needed for principals name denorm)
- series_top_cast        (materialized: per-series most-frequent nconsts
                          across child episodes — amortizes R12 sweep cost)
- ft_titles              (FTS5 virtual; populated from primary+original+aka)
- ingest_meta            (schema version, refresh timestamps)

Indexes:
- B-tree COLLATE NOCASE on title_basics(primaryTitle), (originalTitle),
  title_akas(title) for tier-1 exact-match lookups (microseconds).
- title_episode(parentTconst), title_akas(tconst), title_ratings(numVotes
  DESC), name_basics(primaryName COLLATE NOCASE) for downstream queries.

Refresh protocol (R3, WAL-safe):
1. Acquire imdb/.refresh.lock (flock); abort if another refresh runs.
2. Pre-flight: <25 GB free → abort. Cleanup any stale imdb/imdb.db.new*
   from a prior killed run.
3. Build at imdb/imdb.db.new (WAL pragmas tuned for bulk load).
4. PRAGMA integrity_check; abort + leave live DB intact on failure.
5. PRAGMA wal_checkpoint(TRUNCATE) → fold WAL into single .db file.
6. Close all connections.
7. Move current imdb/imdb.db → imdb/imdb.db.prev (rollback gen).
8. os.replace imdb/imdb.db.new → imdb/imdb.db (atomic on APFS).
9. Cleanup stale .db.new-wal/-shm and old .db-wal/-shm from prior live
   readers (new inode generates fresh siblings).
10. Write imdb/state.json with last_refresh_ts + source checksums.
11. Release lock.

Use:
  python3 scripts/imdb_ingest.py --refresh
  python3 scripts/imdb_ingest.py --refresh --source /path/to/tsvs
  python3 scripts/imdb_ingest.py --refresh --db /tmp/test.db --source <fixtures>

Exit codes:
  0  refresh completed
  1  config / prereq error
  2  pre-flight failure (disk space, missing source, lock held)
  3  ingest failure (TSV parse error, integrity_check fail, sort violation)
"""
from __future__ import annotations

import os
import sys

# scripts/queue.py (pirata aria2c wrapper) shadows stdlib 'queue' when this
# file runs — Python adds the script's dir to sys.path[0]. Drop it early so
# any stdlib import (csv, sqlite3, json) lands cleanly.
sys.path[:] = [p for p in sys.path
               if os.path.abspath(p) != os.path.dirname(os.path.abspath(__file__))]

import argparse
import csv
import fcntl
import hashlib
import json
import shutil
import sqlite3
import time
from datetime import datetime, timezone
from pathlib import Path

# csv module's default field_size_limit is 128k; some IMDb fields (genre
# arrays, knownForTitles) can push past that. Bump to 4 MB defensively.
csv.field_size_limit(4 * 1024 * 1024)

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_SOURCE = REPO_ROOT / "imdb" / "unnoficial"
DEFAULT_DB = REPO_ROOT / "imdb" / "imdb.db"
DEFAULT_LOCK = REPO_ROOT / "imdb" / ".refresh.lock"
DEFAULT_STATE = REPO_ROOT / "imdb" / "state.json"

SCHEMA_VERSION = 1
DISK_FREE_FLOOR_GB = 25
BATCH_SIZE = 50_000

# Akas filter predicate per Key Decisions: PT/EN/ES coverage only.
AKAS_REGIONS = ("BR", "PT", "ES", "MX", "AR")
AKAS_LANGUAGES = ("pt", "en", "es")

# Cast filter for top-5 principals: actor/actress/self capture the
# audience-facing roster (excludes director/producer/writer).
CAST_CATEGORIES = ("actor", "actress", "self")

TSV_FILES = (
    "title.basics.tsv",
    "title.ratings.tsv",
    "title.episode.tsv",
    "title.crew.tsv",
    "title.principals.tsv",
    "title.akas.tsv",
    "name.basics.tsv",
)


def now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def sanitize(x) -> str:
    """repr-escape user-controlled content for log/error lines."""
    return repr(str(x))


def log(msg: str) -> None:
    print(f"{now_iso()} ingest {msg}", flush=True)


def parse_int(s: str) -> int | None:
    if s == r"\N" or s == "":
        return None
    try:
        return int(s)
    except ValueError:
        return None


def parse_float(s: str) -> float | None:
    if s == r"\N" or s == "":
        return None
    try:
        return float(s)
    except ValueError:
        return None


def parse_bool(s: str) -> int:
    """IMDb stores 0/1 as text; default 0 on \\N."""
    return 1 if s == "1" else 0


def nullable(s: str) -> str | None:
    return None if s == r"\N" else s


def file_sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


# ---------------------------------------------------------------------------
# Schema
# ---------------------------------------------------------------------------

SCHEMA_DDL = """
CREATE TABLE title_basics (
    tconst         TEXT PRIMARY KEY,
    titleType      TEXT NOT NULL,
    primaryTitle   TEXT NOT NULL,
    originalTitle  TEXT NOT NULL,
    isAdult        INTEGER NOT NULL,
    startYear      INTEGER,
    endYear        INTEGER,
    runtimeMinutes INTEGER,
    genres         TEXT
);

CREATE TABLE title_ratings (
    tconst        TEXT PRIMARY KEY,
    averageRating REAL NOT NULL,
    numVotes      INTEGER NOT NULL
);

CREATE TABLE title_episode (
    tconst        TEXT PRIMARY KEY,
    parentTconst  TEXT NOT NULL,
    seasonNumber  INTEGER,
    episodeNumber INTEGER
);

CREATE TABLE title_crew (
    tconst    TEXT PRIMARY KEY,
    directors TEXT,
    writers   TEXT
);

CREATE TABLE title_principals_top5 (
    tconst     TEXT NOT NULL,
    ordering   INTEGER NOT NULL,
    nconst     TEXT NOT NULL,
    category   TEXT NOT NULL,
    name       TEXT,
    characters TEXT,
    PRIMARY KEY (tconst, ordering)
);

CREATE TABLE title_akas (
    tconst          TEXT NOT NULL,
    ordering        INTEGER NOT NULL,
    title           TEXT NOT NULL,
    region          TEXT,
    language        TEXT,
    types           TEXT,
    attributes      TEXT,
    isOriginalTitle INTEGER,
    PRIMARY KEY (tconst, ordering)
);

CREATE TABLE name_basics (
    nconst            TEXT PRIMARY KEY,
    primaryName       TEXT NOT NULL,
    birthYear         INTEGER,
    deathYear         INTEGER,
    primaryProfession TEXT,
    knownForTitles    TEXT
);

CREATE TABLE series_top_cast (
    parent_tconst   TEXT PRIMARY KEY,
    top_5_nconsts   TEXT NOT NULL  -- JSON: [{nconst, name, count}]
);

CREATE VIRTUAL TABLE ft_titles USING fts5(
    title,
    title_source UNINDEXED,
    tconst       UNINDEXED,
    tokenize = 'unicode61 remove_diacritics 2',
    prefix = '2 3'
);

CREATE TABLE ingest_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"""

INDEX_DDL = """
CREATE INDEX idx_basics_primary_lower
    ON title_basics(primaryTitle COLLATE NOCASE);
CREATE INDEX idx_basics_original_lower
    ON title_basics(originalTitle COLLATE NOCASE);
CREATE INDEX idx_basics_titletype
    ON title_basics(titleType);

CREATE INDEX idx_ratings_votes
    ON title_ratings(numVotes DESC);

CREATE INDEX idx_episode_parent
    ON title_episode(parentTconst);

CREATE INDEX idx_principals_tconst
    ON title_principals_top5(tconst);

CREATE INDEX idx_akas_title_lower
    ON title_akas(title COLLATE NOCASE);
CREATE INDEX idx_akas_tconst
    ON title_akas(tconst);

CREATE INDEX idx_names_primary_lower
    ON name_basics(primaryName COLLATE NOCASE);
"""


def open_build_db(path: Path) -> sqlite3.Connection:
    """Open a fresh build DB with bulk-load pragmas tuned for ingest."""
    conn = sqlite3.connect(path, isolation_level=None)  # autocommit; we BEGIN/COMMIT manually
    conn.execute("PRAGMA journal_mode = WAL")
    conn.execute("PRAGMA synchronous = NORMAL")
    conn.execute("PRAGMA cache_size = -65536")     # 64 MB
    conn.execute("PRAGMA mmap_size  = 268435456")  # 256 MB
    conn.execute("PRAGMA temp_store = MEMORY")
    conn.execute("PRAGMA page_size  = 8192")
    conn.execute("PRAGMA foreign_keys = OFF")
    conn.executescript(SCHEMA_DDL)
    return conn


# ---------------------------------------------------------------------------
# Per-table ingest
# ---------------------------------------------------------------------------

def _open_tsv(path: Path):
    """Open a TSV with QUOTE_NONE and return (reader, header). Caller owns close."""
    f = path.open("rt", encoding="utf-8", newline="")
    reader = csv.reader(f, delimiter="\t", quoting=csv.QUOTE_NONE)
    header = next(reader)
    return f, reader, header


def _executemany_chunked(conn: sqlite3.Connection, sql: str, rows_iter):
    """Stream rows into executemany() in BATCH_SIZE chunks. Wraps in a
    single transaction. Returns total row count."""
    cur = conn.cursor()
    cur.execute("BEGIN")
    total = 0
    batch: list[tuple] = []
    try:
        for row in rows_iter:
            batch.append(row)
            if len(batch) >= BATCH_SIZE:
                cur.executemany(sql, batch)
                total += len(batch)
                batch.clear()
        if batch:
            cur.executemany(sql, batch)
            total += len(batch)
        cur.execute("COMMIT")
    except Exception:
        cur.execute("ROLLBACK")
        raise
    return total


def ingest_basics(conn: sqlite3.Connection, tsv: Path) -> int:
    log(f"basics: ingesting {sanitize(tsv.name)}")
    f, reader, _header = _open_tsv(tsv)
    sql = ("INSERT INTO title_basics "
           "(tconst, titleType, primaryTitle, originalTitle, isAdult, "
           "startYear, endYear, runtimeMinutes, genres) "
           "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")

    def gen():
        for row in reader:
            if len(row) < 9:
                continue
            yield (row[0], row[1], row[2], row[3], parse_bool(row[4]),
                   parse_int(row[5]), parse_int(row[6]), parse_int(row[7]),
                   nullable(row[8]))

    try:
        n = _executemany_chunked(conn, sql, gen())
    finally:
        f.close()
    log(f"basics: {n} rows")
    return n


def ingest_ratings(conn: sqlite3.Connection, tsv: Path) -> int:
    log(f"ratings: ingesting {sanitize(tsv.name)}")
    f, reader, _ = _open_tsv(tsv)
    sql = "INSERT INTO title_ratings (tconst, averageRating, numVotes) VALUES (?, ?, ?)"

    def gen():
        for row in reader:
            if len(row) < 3:
                continue
            avg = parse_float(row[1])
            votes = parse_int(row[2])
            if avg is None or votes is None:
                continue
            yield (row[0], avg, votes)

    try:
        n = _executemany_chunked(conn, sql, gen())
    finally:
        f.close()
    log(f"ratings: {n} rows")
    return n


def ingest_episode(conn: sqlite3.Connection, tsv: Path) -> int:
    log(f"episode: ingesting {sanitize(tsv.name)}")
    f, reader, _ = _open_tsv(tsv)
    sql = ("INSERT INTO title_episode (tconst, parentTconst, seasonNumber, episodeNumber) "
           "VALUES (?, ?, ?, ?)")

    def gen():
        for row in reader:
            if len(row) < 4:
                continue
            yield (row[0], row[1], parse_int(row[2]), parse_int(row[3]))

    try:
        n = _executemany_chunked(conn, sql, gen())
    finally:
        f.close()
    log(f"episode: {n} rows")
    return n


def ingest_crew(conn: sqlite3.Connection, tsv: Path) -> int:
    log(f"crew: ingesting {sanitize(tsv.name)}")
    f, reader, _ = _open_tsv(tsv)
    sql = "INSERT INTO title_crew (tconst, directors, writers) VALUES (?, ?, ?)"

    def gen():
        for row in reader:
            if len(row) < 3:
                continue
            yield (row[0], nullable(row[1]), nullable(row[2]))

    try:
        n = _executemany_chunked(conn, sql, gen())
    finally:
        f.close()
    log(f"crew: {n} rows")
    return n


def ingest_principals_top5(conn: sqlite3.Connection, tsv: Path,
                           strict_sort_check: bool = True) -> int:
    """Stream title.principals.tsv keeping only top-5 per tconst by ordering,
    filtered to actor/actress/self categories. Aborts loudly if the input
    is not sorted by tconst (per R2 assumption, verified against current dump)."""
    log(f"principals: ingesting {sanitize(tsv.name)} (top-5 cast/tconst)")
    f, reader, _ = _open_tsv(tsv)
    sql = ("INSERT INTO title_principals_top5 "
           "(tconst, ordering, nconst, category, characters) "
           "VALUES (?, ?, ?, ?, ?)")

    seen_tconsts: set[str] = set() if strict_sort_check else set()
    last_tconst: str | None = None
    kept_per_tconst = 0

    def gen():
        nonlocal last_tconst, kept_per_tconst
        for row in reader:
            if len(row) < 6:
                continue
            tconst, ordering_s, nconst, category, _job, characters = row
            if tconst != last_tconst:
                if strict_sort_check:
                    if tconst in seen_tconsts:
                        raise RuntimeError(
                            f"principals sort assumption violated: tconst {sanitize(tconst)} "
                            f"reappeared after {sanitize(last_tconst)}. "
                            "Streaming top-5 selection requires sorted-by-tconst input.")
                    if last_tconst is not None:
                        seen_tconsts.add(last_tconst)
                last_tconst = tconst
                kept_per_tconst = 0
            if category not in CAST_CATEGORIES:
                continue
            if kept_per_tconst >= 5:
                continue
            kept_per_tconst += 1
            ordering_int = parse_int(ordering_s) or 0
            yield (tconst, ordering_int, nconst, category, nullable(characters))

    try:
        n = _executemany_chunked(conn, sql, gen())
    finally:
        f.close()
    log(f"principals: {n} rows kept (filter: category in {CAST_CATEGORIES})")
    return n


def ingest_akas(conn: sqlite3.Connection, tsv: Path) -> int:
    """Stream title.akas.tsv applying the PT/EN/ES + isOriginalTitle filter."""
    log(f"akas: ingesting {sanitize(tsv.name)} (filter: regions + langs + isOriginal)")
    f, reader, _ = _open_tsv(tsv)
    sql = ("INSERT INTO title_akas "
           "(tconst, ordering, title, region, language, types, attributes, isOriginalTitle) "
           "VALUES (?, ?, ?, ?, ?, ?, ?, ?)")

    def gen():
        for row in reader:
            if len(row) < 8:
                continue
            tconst, ordering_s, title, region, language, types, attrs, is_orig = row
            region_n = nullable(region)
            language_n = nullable(language)
            is_orig_n = parse_int(is_orig) or 0
            # Predicate per Key Decisions:
            #   region IN AKAS_REGIONS OR language IN AKAS_LANGUAGES OR isOriginalTitle = 1
            if not (
                (region_n in AKAS_REGIONS)
                or (language_n in AKAS_LANGUAGES)
                or (is_orig_n == 1)
            ):
                continue
            yield (tconst, parse_int(ordering_s) or 0, title,
                   region_n, language_n, nullable(types), nullable(attrs),
                   is_orig_n)

    try:
        n = _executemany_chunked(conn, sql, gen())
    finally:
        f.close()
    log(f"akas: {n} rows kept")
    return n


def ingest_names(conn: sqlite3.Connection, tsv: Path) -> int:
    log(f"names: ingesting {sanitize(tsv.name)}")
    f, reader, _ = _open_tsv(tsv)
    sql = ("INSERT INTO name_basics "
           "(nconst, primaryName, birthYear, deathYear, primaryProfession, knownForTitles) "
           "VALUES (?, ?, ?, ?, ?, ?)")

    def gen():
        for row in reader:
            if len(row) < 6:
                continue
            yield (row[0], row[1], parse_int(row[2]), parse_int(row[3]),
                   nullable(row[4]), nullable(row[5]))

    try:
        n = _executemany_chunked(conn, sql, gen())
    finally:
        f.close()
    log(f"names: {n} rows")
    return n


def denormalize_principal_names(conn: sqlite3.Connection) -> int:
    """Fill title_principals_top5.name from name_basics.primaryName via JOIN."""
    log("denorm: backfilling principal names from name_basics")
    cur = conn.cursor()
    cur.execute("BEGIN")
    try:
        cur.execute(
            "UPDATE title_principals_top5 "
            "SET name = (SELECT primaryName FROM name_basics "
            "            WHERE name_basics.nconst = title_principals_top5.nconst)"
        )
        n = cur.rowcount
        cur.execute("COMMIT")
    except Exception:
        cur.execute("ROLLBACK")
        raise
    log(f"denorm: {n} principals updated")
    return n


def build_series_top_cast(conn: sqlite3.Connection) -> int:
    """Materialize series_top_cast(parent_tconst, top_5_nconsts JSON) by
    aggregating per-series principal frequencies across child episodes."""
    log("series_top_cast: aggregating per-series cast frequencies")
    # Only series titleTypes — avoid materializing for movies (no parent).
    cur = conn.cursor()
    cur.execute("BEGIN")
    try:
        cur.execute("""
            WITH ep_cast AS (
                SELECT te.parentTconst AS pt,
                       p.nconst,
                       p.name,
                       COUNT(*) AS freq
                FROM title_episode te
                JOIN title_principals_top5 p ON p.tconst = te.tconst
                GROUP BY te.parentTconst, p.nconst
            ),
            ranked AS (
                SELECT pt, nconst, name, freq,
                       ROW_NUMBER() OVER (PARTITION BY pt ORDER BY freq DESC, nconst ASC) AS rk
                FROM ep_cast
            )
            INSERT INTO series_top_cast (parent_tconst, top_5_nconsts)
            SELECT pt,
                   json_group_array(json_object('nconst', nconst, 'name', name, 'count', freq))
            FROM ranked
            WHERE rk <= 5
            GROUP BY pt
        """)
        n = cur.rowcount
        cur.execute("COMMIT")
    except Exception:
        cur.execute("ROLLBACK")
        raise
    log(f"series_top_cast: {n} series materialized")
    return n


def build_fts5(conn: sqlite3.Connection) -> int:
    """Populate ft_titles via 3 sequential INSERTs (NOT a single UNION ALL —
    materializes 35M+ rows in memory; per pass-2 adversarial review)."""
    log("fts5: indexing primaryTitle / originalTitle / aka.title")
    cur = conn.cursor()
    total = 0
    for src, sql in (
        ("primary",
         "INSERT INTO ft_titles(title, title_source, tconst) "
         "SELECT primaryTitle, 'primary', tconst FROM title_basics"),
        ("original",
         "INSERT INTO ft_titles(title, title_source, tconst) "
         "SELECT originalTitle, 'original', tconst FROM title_basics "
         "WHERE originalTitle != primaryTitle"),
        ("aka",
         "INSERT INTO ft_titles(title, title_source, tconst) "
         "SELECT title, 'aka', tconst FROM title_akas"),
    ):
        cur.execute("BEGIN")
        try:
            cur.execute(sql)
            cur.execute("COMMIT")
        except Exception:
            cur.execute("ROLLBACK")
            raise
        # FTS5 doesn't expose row count via .rowcount reliably; query.
        n = cur.execute(
            "SELECT count(*) FROM ft_titles WHERE title_source = ?", (src,)
        ).fetchone()[0]
        log(f"fts5 [{src}]: {n} rows")
        total += n
    log(f"fts5: {total} total")
    return total


def build_indexes(conn: sqlite3.Connection) -> None:
    log("indexes: building B-tree indexes (post-load for speed)")
    conn.executescript(INDEX_DDL)
    log("indexes: done")


def write_meta(conn: sqlite3.Connection, started_at: str, finished_at: str) -> None:
    cur = conn.cursor()
    cur.execute("BEGIN")
    cur.executemany(
        "INSERT OR REPLACE INTO ingest_meta (key, value) VALUES (?, ?)",
        [
            ("schema_version", str(SCHEMA_VERSION)),
            ("ingest_started_at", started_at),
            ("ingest_finished_at", finished_at),
        ],
    )
    cur.execute("COMMIT")


# ---------------------------------------------------------------------------
# Refresh orchestration
# ---------------------------------------------------------------------------

def cleanup_stale_artifacts(db: Path) -> None:
    """Remove pre-existing imdb.db.new* siblings from a prior killed run."""
    parent = db.parent
    base_new = db.name + ".new"
    for p in parent.glob(f"{base_new}*"):
        log(f"cleanup: unlinking stale {sanitize(p.name)}")
        try:
            p.unlink()
        except OSError as e:
            log(f"cleanup: failed to unlink {sanitize(p.name)}: {sanitize(e)}")


def cleanup_post_swap(db: Path) -> None:
    """After os.replace, the swapped-in inode generates fresh WAL/SHM
    siblings; clean up orphaned siblings of the previous live DB inode."""
    parent = db.parent
    base = db.name
    # Note: don't delete <db>-wal / <db>-shm since they belong to the
    # NEW inode now (created lazily on first read). Only delete the
    # transient build artifacts named like *.new-wal / *.new-shm.
    base_new = base + ".new"
    for p in parent.glob(f"{base_new}-*"):
        log(f"cleanup: unlinking post-swap orphan {sanitize(p.name)}")
        try:
            p.unlink()
        except OSError as e:
            log(f"cleanup: failed to unlink {sanitize(p.name)}: {sanitize(e)}")


def precheck_disk(db: Path, min_gb: int) -> None:
    db.parent.mkdir(parents=True, exist_ok=True)
    free_bytes = shutil.disk_usage(db.parent).free
    free_gb = free_bytes / (1024 ** 3)
    if free_gb < min_gb:
        raise RuntimeError(
            f"insufficient disk: {free_gb:.1f} GB free at {db.parent}, "
            f"need >= {min_gb} GB (peak refresh = old DB + new TSVs + new DB)")
    log(f"precheck: disk {free_gb:.1f} GB free OK")


def precheck_source(source: Path) -> None:
    if not source.is_dir():
        raise RuntimeError(f"source dir not found: {source}")
    missing = [name for name in TSV_FILES if not (source / name).is_file()]
    if missing:
        raise RuntimeError(
            f"missing TSVs in {source}: {', '.join(missing)} "
            "(download from https://datasets.imdbws.com/ and ungzip)")
    log(f"precheck: source {sanitize(source)} has all {len(TSV_FILES)} TSVs")


def acquire_lock(lock_path: Path) -> int:
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(lock_path, os.O_CREAT | os.O_RDWR, 0o644)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        os.close(fd)
        raise RuntimeError(
            f"another refresh is in progress (lock held: {lock_path}). "
            "If you are sure no ingest is running, delete the lock file.")
    return fd


def release_lock(fd: int, lock_path: Path) -> None:
    try:
        fcntl.flock(fd, fcntl.LOCK_UN)
    finally:
        os.close(fd)
    try:
        lock_path.unlink()
    except OSError:
        pass


def integrity_check(db_path: Path) -> None:
    conn = sqlite3.connect(db_path)
    try:
        result = conn.execute("PRAGMA integrity_check").fetchone()[0]
    finally:
        conn.close()
    if result != "ok":
        raise RuntimeError(f"integrity_check failed for {db_path}: {result}")
    log("integrity_check: ok")


def checkpoint_truncate(db_path: Path) -> None:
    """Fold WAL into the main DB file so the post-swap DB is a single file."""
    conn = sqlite3.connect(db_path)
    try:
        conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")
    finally:
        conn.close()
    log("wal_checkpoint(TRUNCATE): ok")


def write_state_json(state_path: Path, source: Path, started_at: str,
                     finished_at: str) -> None:
    state_path.parent.mkdir(parents=True, exist_ok=True)
    checksums = {}
    log(f"state: computing source checksums for {len(TSV_FILES)} files")
    for name in TSV_FILES:
        p = source / name
        if p.is_file():
            checksums[name] = file_sha256(p)
    state = {
        "schema_version": SCHEMA_VERSION,
        "last_refresh_started_at": started_at,
        "last_refresh_finished_at": finished_at,
        "source_dir": str(source),
        "source_checksums": checksums,
    }
    tmp = state_path.with_suffix(state_path.suffix + ".tmp")
    tmp.write_text(json.dumps(state, indent=2, sort_keys=True) + "\n")
    tmp.replace(state_path)
    log(f"state: wrote {sanitize(state_path)}")


def do_refresh(args: argparse.Namespace) -> int:
    source = args.source.resolve()
    db = args.db.resolve()
    db_new = db.with_suffix(db.suffix + ".new")
    db_prev = db.with_suffix(db.suffix + ".prev")
    state_path = args.state.resolve() if args.state else (db.parent / "state.json")
    lock_path = args.lock.resolve() if args.lock else (db.parent / ".refresh.lock")

    try:
        precheck_source(source)
        precheck_disk(db, args.min_free_gb)
    except RuntimeError as e:
        log(f"FAIL precheck: {e}")
        return 2

    try:
        lock_fd = acquire_lock(lock_path)
    except RuntimeError as e:
        log(f"FAIL lock: {e}")
        return 2

    started_at = now_iso()
    t0 = time.time()
    try:
        cleanup_stale_artifacts(db)

        log(f"build: opening {sanitize(db_new)}")
        conn = open_build_db(db_new)
        try:
            ingest_basics(conn,    source / "title.basics.tsv")
            ingest_ratings(conn,   source / "title.ratings.tsv")
            ingest_episode(conn,   source / "title.episode.tsv")
            ingest_crew(conn,      source / "title.crew.tsv")
            ingest_names(conn,     source / "name.basics.tsv")
            ingest_principals_top5(conn, source / "title.principals.tsv",
                                   strict_sort_check=not args.no_sort_check)
            denormalize_principal_names(conn)
            ingest_akas(conn,      source / "title.akas.tsv")
            build_series_top_cast(conn)
            build_indexes(conn)
            build_fts5(conn)
            finished_at = now_iso()
            write_meta(conn, started_at, finished_at)
            log("build: ANALYZE")
            conn.execute("ANALYZE")
        finally:
            conn.close()

        try:
            integrity_check(db_new)
        except RuntimeError as e:
            log(f"FAIL integrity_check: {e}; live DB at {sanitize(db)} unchanged")
            try:
                db_new.unlink()
            except OSError:
                pass
            return 3

        checkpoint_truncate(db_new)

        # Promote previous live DB → .prev (rollback gen).
        if db.exists():
            log(f"rotate: {sanitize(db)} -> {sanitize(db_prev)}")
            if db_prev.exists():
                db_prev.unlink()
            db.replace(db_prev)

        log(f"swap: {sanitize(db_new)} -> {sanitize(db)} (atomic)")
        os.replace(db_new, db)

        cleanup_post_swap(db)

        write_state_json(state_path, source, started_at, finished_at)

        dt = time.time() - t0
        log(f"OK refresh complete in {dt:.1f}s ({dt/60:.1f} min)")
        return 0

    except Exception as e:
        log(f"FAIL ingest: {sanitize(e)}; live DB at {sanitize(db)} unchanged")
        try:
            if db_new.exists():
                db_new.unlink()
        except OSError:
            pass
        return 3
    finally:
        release_lock(lock_fd, lock_path)


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def main() -> int:
    ap = argparse.ArgumentParser(
        description="IMDb non-commercial TSV → SQLite + FTS5 ingest.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    ap.add_argument("--refresh", action="store_true",
                    help="Run a full ingest cycle (download/cleanup/build/swap)")
    ap.add_argument("--source", type=Path, default=DEFAULT_SOURCE,
                    help=f"Directory with the 7 IMDb TSVs (default: {DEFAULT_SOURCE})")
    ap.add_argument("--db", type=Path, default=DEFAULT_DB,
                    help=f"Output SQLite path (default: {DEFAULT_DB})")
    ap.add_argument("--state", type=Path, default=None,
                    help="state.json path (default: <db dir>/state.json)")
    ap.add_argument("--lock", type=Path, default=None,
                    help="refresh lock path (default: <db dir>/.refresh.lock)")
    ap.add_argument("--min-free-gb", type=int, default=DISK_FREE_FLOOR_GB,
                    help=f"Pre-flight free-space gate in GB (default: {DISK_FREE_FLOOR_GB})")
    ap.add_argument("--no-sort-check", action="store_true",
                    help="Disable the title.principals sort-violation guard "
                         "(unsafe: silent drop of duplicate-tconst batches)")
    args = ap.parse_args()

    if not args.refresh:
        ap.print_help()
        print("\nNo action: pass --refresh to run an ingest cycle.", file=sys.stderr)
        return 1

    return do_refresh(args)


if __name__ == "__main__":
    sys.exit(main())
