#!/usr/bin/env python3
"""IMDb local-DB lookup helper for /pirata.

APIs:
  lookup_by_title(query, year=None, kind=None) -> list[Match]
  lookup_by_tconst(tconst) -> Title | None
  lookup_episodes(parent_tconst, season=None) -> list[Episode]

Tier 1 (B-tree COLLATE NOCASE) for exact match, Tier 2 (FTS5 + RapidFuzz)
for fuzzy. Composite score = fuzz_ratio x field_multiplier; numVotes desc
breaks ties. Top-1 within 15% of runner-up AND both Tier 1 -> multi_tie.

Use:
  from scripts.imdb_lookup import lookup_by_title
  matches = lookup_by_title("Cidade de Deus")

  python3 scripts/imdb_lookup.py "Dune" --year 2021
  python3 scripts/imdb_lookup.py --tconst tt0317248
"""
from __future__ import annotations

import os
import sys

# scripts/queue.py shadows stdlib 'queue' on script dir; drop it defensively.
sys.path[:] = [p for p in sys.path
               if os.path.abspath(p) != os.path.dirname(os.path.abspath(__file__))]

import argparse
import json
import re
import sqlite3
from dataclasses import dataclass, replace
from pathlib import Path
from typing import Iterable, Optional

from rapidfuzz import fuzz


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_DB = REPO_ROOT / "imdb" / "imdb.db"

# Locked composite-score field multipliers (Key Decisions in plan).
FIELD_MULTIPLIERS: dict[str, float] = {
    "primary":      3.0,
    "original":     2.0,
    "aka_original": 1.8,
    "aka_regional": 1.5,
}

FUZZ_CUTOFF = 70           # RapidFuzz token_set_ratio floor
CONF_THRESHOLD_PCT = 15    # multi-tie window: top-1 within 15% of runner-up
FTS5_LIMIT = 200           # FTS5 candidate cap before RapidFuzz post-pass

# Strip FTS5 operators that would break a query when echoed verbatim.
_FTS5_BAD = re.compile(r'["()*+\-^:.,!?]')


class IMDbDBUnavailable(RuntimeError):
    """Raised when the IMDb DB is missing, locked, or unreadable."""


@dataclass(frozen=True)
class Match:
    tconst: str
    primary_title: str
    original_title: str
    title_type: str
    start_year: Optional[int]
    score: float           # fuzz_ratio * field_multiplier
    field: str             # primary|original|aka_original|aka_regional
    matched_text: str      # the title field that produced the match
    fuzz_ratio: float      # 0..100
    num_votes: int
    average_rating: float
    multi_tie: bool = False


@dataclass(frozen=True)
class Title:
    tconst: str
    primary_title: str
    original_title: str
    title_type: str
    is_adult: bool
    start_year: Optional[int]
    end_year: Optional[int]
    runtime_minutes: Optional[int]
    genres: list[str]
    average_rating: Optional[float]
    num_votes: Optional[int]
    akas_pt: list[str]
    akas_en: list[str]
    akas_es: list[str]
    top_cast: list[dict]   # [{nconst, name, count?}]


@dataclass(frozen=True)
class Episode:
    tconst: str
    parent_tconst: str
    season_number: Optional[int]
    episode_number: Optional[int]


# --- connection cache ---

_CONN: Optional[sqlite3.Connection] = None
_DB_PATH: Optional[Path] = None


def _conn(db_path: Path) -> sqlite3.Connection:
    global _CONN, _DB_PATH
    if _CONN is not None and _DB_PATH == db_path:
        return _CONN
    if not db_path.exists():
        raise IMDbDBUnavailable(f"IMDb DB not found at {db_path}")
    try:
        c = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    except sqlite3.OperationalError as e:
        raise IMDbDBUnavailable(f"cannot open {db_path}: {e}") from e
    c.row_factory = sqlite3.Row
    _CONN = c
    _DB_PATH = db_path
    return c


def close_connection() -> None:
    global _CONN, _DB_PATH
    if _CONN is not None:
        _CONN.close()
        _CONN = None
        _DB_PATH = None


# --- public API ---

def lookup_by_title(
    query: str,
    year: Optional[int] = None,
    kind: Optional[str] = None,
    db: Optional[Path] = None,
) -> list[Match]:
    db_path = db or DEFAULT_DB
    c = _conn(db_path)

    q = (query or "").strip()
    if not q:
        return []

    tier1 = _tier1_exact(c, q, year=year, kind=kind)
    tier2 = _tier2_fuzzy(c, q, year=year, kind=kind)

    seen = {m.tconst for m in tier1}
    merged = list(tier1) + [m for m in tier2 if m.tconst not in seen]
    if not merged:
        return []

    # Tier separation: Tier 1 (fuzz_ratio=100, exact match) always ranks above
    # Tier 2 (fuzzy, capped at 99). Within tier: score desc, numVotes desc.
    merged.sort(key=lambda m: (0 if m.fuzz_ratio == 100 else 1, -m.score, -m.num_votes))

    # Multi-tie: top-1 within CONF_THRESHOLD_PCT of runner-up AND both Tier 1.
    if len(merged) >= 2:
        top, runner = merged[0], merged[1]
        both_tier1 = top.fuzz_ratio == 100 and runner.fuzz_ratio == 100
        denom = max(top.score, 0.01)
        within_pct = abs(top.score - runner.score) / denom * 100 <= CONF_THRESHOLD_PCT
        if both_tier1 and within_pct:
            merged[0] = replace(top, multi_tie=True)
            merged[1] = replace(runner, multi_tie=True)

    return merged


def lookup_by_tconst(
    tconst: str,
    db: Optional[Path] = None,
) -> Optional[Title]:
    db_path = db or DEFAULT_DB
    c = _conn(db_path)

    row = c.execute("""
        SELECT b.tconst, b.titleType, b.primaryTitle, b.originalTitle, b.isAdult,
               b.startYear, b.endYear, b.runtimeMinutes, b.genres,
               r.averageRating, r.numVotes
        FROM title_basics b
        LEFT JOIN title_ratings r ON r.tconst = b.tconst
        WHERE b.tconst = ?
    """, (tconst,)).fetchone()
    if row is None:
        return None

    akas_pt: list[str] = []
    akas_en: list[str] = []
    akas_es: list[str] = []
    for ar in c.execute("""
        SELECT title, language FROM title_akas
        WHERE tconst = ? AND language IN ('pt', 'en', 'es')
        ORDER BY ordering ASC
    """, (tconst,)).fetchall():
        bucket = {'pt': akas_pt, 'en': akas_en, 'es': akas_es}[ar['language']]
        if ar['title'] not in bucket:
            bucket.append(ar['title'])

    title_type = row['titleType']
    if title_type in ('tvSeries', 'tvMiniSeries'):
        cast_row = c.execute(
            "SELECT top_5_nconsts FROM series_top_cast WHERE parent_tconst = ?",
            (tconst,),
        ).fetchone()
        top_cast = json.loads(cast_row['top_5_nconsts']) if cast_row else []
    else:
        cast_rows = c.execute("""
            SELECT nconst, name FROM title_principals_top5
            WHERE tconst = ? ORDER BY ordering ASC LIMIT 5
        """, (tconst,)).fetchall()
        top_cast = [{'nconst': cr['nconst'], 'name': cr['name']} for cr in cast_rows]

    return Title(
        tconst=row['tconst'],
        primary_title=row['primaryTitle'],
        original_title=row['originalTitle'],
        title_type=title_type,
        is_adult=bool(row['isAdult']),
        start_year=row['startYear'],
        end_year=row['endYear'],
        runtime_minutes=row['runtimeMinutes'],
        genres=row['genres'].split(',') if row['genres'] else [],
        average_rating=row['averageRating'],
        num_votes=row['numVotes'],
        akas_pt=akas_pt,
        akas_en=akas_en,
        akas_es=akas_es,
        top_cast=top_cast,
    )


def lookup_episodes(
    parent_tconst: str,
    season: Optional[int] = None,
    db: Optional[Path] = None,
) -> list[Episode]:
    db_path = db or DEFAULT_DB
    c = _conn(db_path)
    rows = c.execute("""
        SELECT tconst, parentTconst, seasonNumber, episodeNumber
        FROM title_episode
        WHERE parentTconst = ? AND (? IS NULL OR seasonNumber = ?)
        ORDER BY seasonNumber, episodeNumber
    """, (parent_tconst, season, season)).fetchall()
    return [
        Episode(
            tconst=r['tconst'],
            parent_tconst=r['parentTconst'],
            season_number=r['seasonNumber'],
            episode_number=r['episodeNumber'],
        )
        for r in rows
    ]


# --- tier 1: exact case-insensitive match (B-tree COLLATE NOCASE) ---

def _tier1_exact(
    c: sqlite3.Connection,
    q: str,
    year: Optional[int],
    kind: Optional[str],
) -> list[Match]:
    matches: list[Match] = []
    seen: set[str] = set()

    # primaryTitle
    sql, params = _exact_query("primaryTitle", q, year, kind)
    for r in c.execute(sql, params).fetchall():
        if r['tconst'] in seen:
            continue
        seen.add(r['tconst'])
        matches.append(_make_match(c, r, fuzz_ratio=100.0, field='primary', matched_text=r['primaryTitle']))

    # originalTitle (only when distinct from primaryTitle, FTS5 mirror)
    sql, params = _exact_query("originalTitle", q, year, kind, exclude_eq_primary=True)
    for r in c.execute(sql, params).fetchall():
        if r['tconst'] in seen:
            continue
        seen.add(r['tconst'])
        matches.append(_make_match(c, r, fuzz_ratio=100.0, field='original', matched_text=r['originalTitle']))

    # akas — JOIN to title_basics for filters
    aka_sql = """
        SELECT a.title AS aka_title, a.region, a.language, a.isOriginalTitle,
               b.tconst, b.primaryTitle, b.originalTitle, b.titleType, b.startYear
        FROM title_akas a
        JOIN title_basics b ON b.tconst = a.tconst
        WHERE a.title = ? COLLATE NOCASE
    """
    aka_params: list = [q]
    if year is not None:
        aka_sql += " AND b.startYear = ?"
        aka_params.append(year)
    if kind:
        aka_sql += " AND b.titleType = ?"
        aka_params.append(kind)
    for r in c.execute(aka_sql, tuple(aka_params)).fetchall():
        if r['tconst'] in seen:
            continue
        seen.add(r['tconst'])
        field = 'aka_original' if r['isOriginalTitle'] == 1 else 'aka_regional'
        matches.append(_make_match(c, r, fuzz_ratio=100.0, field=field, matched_text=r['aka_title']))

    return matches


def _exact_query(
    column: str,
    q: str,
    year: Optional[int],
    kind: Optional[str],
    *,
    exclude_eq_primary: bool = False,
) -> tuple[str, tuple]:
    sql = f"""
        SELECT tconst, primaryTitle, originalTitle, titleType, startYear
        FROM title_basics
        WHERE {column} = ? COLLATE NOCASE
    """
    params: list = [q]
    if exclude_eq_primary:
        sql += " AND originalTitle != primaryTitle"
    if year is not None:
        sql += " AND startYear = ?"
        params.append(year)
    if kind:
        sql += " AND titleType = ?"
        params.append(kind)
    return sql, tuple(params)


# --- tier 2: FTS5 candidates + RapidFuzz post-pass ---

def _tier2_fuzzy(
    c: sqlite3.Connection,
    q: str,
    year: Optional[int],
    kind: Optional[str],
) -> list[Match]:
    fts_q = _build_fts_query(q)
    if not fts_q:
        return []

    rows = c.execute(
        """
        SELECT cand.title AS matched_text, cand.title_source AS src,
               b.tconst, b.primaryTitle, b.originalTitle, b.titleType, b.startYear
        FROM (
            SELECT title, title_source, tconst FROM ft_titles
            WHERE ft_titles MATCH ? LIMIT ?
        ) AS cand
        JOIN title_basics b ON b.tconst = cand.tconst
        """,
        (fts_q, FTS5_LIMIT),
    ).fetchall()
    if not rows:
        return []

    matches: list[Match] = []
    seen: set[str] = set()
    for r in rows:
        if year is not None and r['startYear'] != year:
            continue
        if kind and r['titleType'] != kind:
            continue
        if r['tconst'] in seen:
            continue
        # WRatio: length-aware blend (RapidFuzz). Capped at 99 so only Tier 1
        # exact-match (which sets fuzz_ratio=100 explicitly) yields full score —
        # enforces strict tier separation in the merged sort.
        ratio = min(float(fuzz.WRatio(q, r['matched_text'])), 99.0)
        if ratio < FUZZ_CUTOFF:
            continue
        seen.add(r['tconst'])
        field = _classify_field(c, r['tconst'], r['src'], r['matched_text'])
        matches.append(_make_match(c, r, fuzz_ratio=ratio, field=field, matched_text=r['matched_text']))

    return matches


def _classify_field(c: sqlite3.Connection, tconst: str, src: str, matched_text: str) -> str:
    if src == 'aka':
        aka = c.execute(
            "SELECT isOriginalTitle FROM title_akas WHERE tconst = ? AND title = ? LIMIT 1",
            (tconst, matched_text),
        ).fetchone()
        return 'aka_original' if (aka and aka['isOriginalTitle'] == 1) else 'aka_regional'
    if src == 'original':
        return 'original'
    return 'primary'


def _build_fts_query(q: str) -> str:
    cleaned = _FTS5_BAD.sub(' ', q.lower())
    tokens = cleaned.split()
    if not tokens:
        return ""
    # Implicit AND across tokens; suffix * on last for prefix recall.
    return ' '.join(tokens[:-1] + [tokens[-1] + '*'])


# --- match builder + tie-breaker ---

def _make_match(
    c: sqlite3.Connection,
    row: sqlite3.Row,
    *,
    fuzz_ratio: float,
    field: str,
    matched_text: str,
) -> Match:
    rating = c.execute(
        "SELECT averageRating, numVotes FROM title_ratings WHERE tconst = ?",
        (row['tconst'],),
    ).fetchone()
    num_votes = rating['numVotes'] if rating else 0
    avg_rating = rating['averageRating'] if rating else 0.0
    score = fuzz_ratio * FIELD_MULTIPLIERS[field]
    return Match(
        tconst=row['tconst'],
        primary_title=row['primaryTitle'],
        original_title=row['originalTitle'],
        title_type=row['titleType'],
        start_year=row['startYear'],
        score=score,
        field=field,
        matched_text=matched_text,
        fuzz_ratio=fuzz_ratio,
        num_votes=num_votes,
        average_rating=avg_rating,
    )


# --- CLI ---

def _match_to_dict(m: Match) -> dict:
    return {
        "tconst": m.tconst,
        "primary_title": m.primary_title,
        "original_title": m.original_title,
        "title_type": m.title_type,
        "start_year": m.start_year,
        "score": round(m.score, 2),
        "field": m.field,
        "matched_text": m.matched_text,
        "fuzz_ratio": round(m.fuzz_ratio, 2),
        "num_votes": m.num_votes,
        "average_rating": m.average_rating,
        "multi_tie": m.multi_tie,
    }


def _title_to_dict(t: Title) -> dict:
    return {
        "tconst": t.tconst,
        "primary_title": t.primary_title,
        "original_title": t.original_title,
        "title_type": t.title_type,
        "is_adult": t.is_adult,
        "start_year": t.start_year,
        "end_year": t.end_year,
        "runtime_minutes": t.runtime_minutes,
        "genres": t.genres,
        "average_rating": t.average_rating,
        "num_votes": t.num_votes,
        "akas_pt": t.akas_pt,
        "akas_en": t.akas_en,
        "akas_es": t.akas_es,
        "top_cast": t.top_cast,
    }


def _main(argv: Optional[Iterable[str]] = None) -> int:
    ap = argparse.ArgumentParser(description="IMDb local-DB lookup helper")
    ap.add_argument("query", nargs="?", default=None,
                    help="Title query (case-insensitive); skip if --tconst or --episodes is set.")
    ap.add_argument("--year", type=int, default=None, help="Filter to startYear.")
    ap.add_argument("--kind", default=None,
                    help="titleType filter (movie, tvSeries, tvMiniSeries, short, ...).")
    ap.add_argument("--tconst", default=None, help="Lookup by tconst (e.g., tt0317248).")
    ap.add_argument("--episodes", default=None, help="List episodes for parent tconst.")
    ap.add_argument("--season", type=int, default=None, help="Filter --episodes by season.")
    ap.add_argument("--db", type=Path, default=DEFAULT_DB)
    ap.add_argument("--limit", type=int, default=10)
    args = ap.parse_args(list(argv) if argv is not None else None)

    if args.tconst:
        t = lookup_by_tconst(args.tconst, db=args.db)
        if t is None:
            print(json.dumps({"error": "tconst not found", "tconst": args.tconst}))
            return 1
        print(json.dumps(_title_to_dict(t), ensure_ascii=False, indent=2))
        return 0

    if args.episodes:
        for e in lookup_episodes(args.episodes, season=args.season, db=args.db):
            print(f"{e.tconst}\tS{e.season_number}E{e.episode_number}")
        return 0

    if not args.query:
        ap.error("query is required when --tconst/--episodes are not set")

    matches = lookup_by_title(args.query, year=args.year, kind=args.kind, db=args.db)
    for m in matches[:args.limit]:
        print(json.dumps(_match_to_dict(m), ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(_main())
