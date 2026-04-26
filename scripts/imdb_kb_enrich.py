"""IMDb-backed enrichment helper for KB per-movie manifests.

Reads a release filename → parses via PTN → resolves the title via
`scripts/imdb_lookup.py` → assembles enriched manifest fields. Helper
layer between `scripts/contact_sheet.py` (manifest builder) and
`scripts/imdb_lookup.py` (locked Unit 2 surface).

Plan: docs/plans/2026-04-26-007-feat-imdb-kb-enrichment-plan.md (Unit A).
Origin: docs/plans/2026-04-24-004-feat-imdb-local-pirata-coupling-plan.md (Unit 3).

v1 limitations (deferred to Phase 2):
- imdb.plot is always null (IMDb non-commercial TSV bundle has no plot
  data; Wikipedia-derived plot is a Phase 2 extension).
- imdb.top_cast[].role is always null (Unit 2's Title dataclass does
  not expose `characters`; not modifying imdb_lookup per plan boundary).
- Confidence threshold (15 %) is enforced by imdb_lookup's multi_tie
  heuristic (tier-1-only). Plan 007 sketched a tier-2 extension; v1
  defers to Unit 2's locked behavior to keep contracts aligned.
"""
from __future__ import annotations

import json
import sqlite3
import sys
from dataclasses import asdict, dataclass, replace
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO_ROOT / "scripts"))

try:
    import PTN  # parse-torrent-title v2.8.2 from PyPI; imports as PTN.
except ImportError as e:
    raise ImportError(
        "PTN (parse-torrent-title) not installed. Run: "
        "pip3 install parse-torrent-title"
    ) from e

import imdb_lookup
from imdb_lookup import IMDbDBUnavailable, Match, Title

IMDB_CONFIDENCE_PCT = 15  # documented; enforcement lives in imdb_lookup.
AKAS_CAP = 10
TIE_BREAK_VOTE_RATIO = 10  # plan 008: top.num_votes >= this * runner.num_votes
                           # demotes multi_tie → resolved (with same-year guard).
                           # 10× catches Roger Rabbit (1110×) and Mario Galaxy (∞)
                           # while preserving genuine same-fame ambiguity (<10×).
DEFAULT_LOG_PATH = REPO_ROOT / "logs" / "sweep_imdb_misses.log"
DEFAULT_DB = REPO_ROOT / "imdb" / "imdb.db"


@dataclass(frozen=True)
class ResolutionResult:
    canonical_title: str
    canonical_year: Optional[int]
    filename: dict
    imdb: dict

    def as_dict(self) -> dict:
        return asdict(self)


def resolve(
    raw_title: str,
    *,
    slug: Optional[str] = None,
    db_path: Optional[Path] = None,
    log_path: Optional[Path] = None,
) -> ResolutionResult:
    """Resolve raw filename → canonical title/year + imdb block.

    Never raises for IMDb-side failures; surfaces them via imdb.result
    so the caller's pipeline (contact_sheet manifest write) is not
    blocked by IMDb unavailability.
    """
    log_path = log_path or DEFAULT_LOG_PATH
    db_path = db_path or DEFAULT_DB

    filename = _parse_filename(raw_title)
    ptt_title = filename["ptt_title"]
    ptt_year = filename["ptt_year"]

    try:
        matches = imdb_lookup.lookup_by_title(ptt_title, year=ptt_year, db=db_path)
    except IMDbDBUnavailable:
        imdb = {
            "lookup_attempted": False,
            "result": "db_unavailable",
            "candidates_considered": 0,
        }
        _log_miss(log_path, slug, raw_title, ptt_title, ptt_year, imdb)
        return ResolutionResult(ptt_title, ptt_year, filename, imdb)

    # Plan 008: vote-spread tie-breaker. imdb_lookup's multi_tie heuristic
    # is purely score-based (15 % gap on tier-1) and ignores numVotes.
    # Real-world IMDb noise (videoGames, tvEpisodes named after parents)
    # commonly polls 100×+ less than the famous same-titled film. The
    # override demotes top.multi_tie when same-year + vote-dominance both
    # hold, so the resolved path runs and the manifest gains full IMDb
    # enrichment instead of falling back to PTN-only canonical.
    matches = _apply_vote_tie_breaker(matches)

    if not matches:
        imdb = {
            "lookup_attempted": True,
            "result": "no_match",
            "candidates_considered": 0,
        }
        _log_miss(log_path, slug, raw_title, ptt_title, ptt_year, imdb)
        return ResolutionResult(ptt_title, ptt_year, filename, imdb)

    top = matches[0]
    if top.multi_tie:
        runner = matches[1] if len(matches) > 1 else None
        imdb = {
            "lookup_attempted": True,
            "result": "multi_tie",
            "candidates_considered": len(matches),
            "top_score": round(top.fuzz_ratio, 1),
            "runner_up_score": round(runner.fuzz_ratio, 1) if runner else None,
            "multi_tie": True,
        }
        _log_miss(log_path, slug, raw_title, ptt_title, ptt_year, imdb)
        return ResolutionResult(ptt_title, ptt_year, filename, imdb)

    # resolved
    title_obj = imdb_lookup.lookup_by_tconst(top.tconst, db=db_path)
    if title_obj is None:
        imdb = {
            "lookup_attempted": True,
            "result": "no_match",
            "candidates_considered": len(matches),
        }
        _log_miss(log_path, slug, raw_title, ptt_title, ptt_year, imdb)
        return ResolutionResult(ptt_title, ptt_year, filename, imdb)

    directors = _get_directors(db_path, top.tconst)
    imdb = _assemble_imdb_block(top, title_obj, directors)
    return ResolutionResult(
        title_obj.primary_title, title_obj.start_year, filename, imdb
    )


def _apply_vote_tie_breaker(matches: list[Match]) -> list[Match]:
    """Demote top.multi_tie → False when same-year vote-dominance is overwhelming.

    Plan 008 calibration: imdb_lookup's score-based multi_tie heuristic flags
    tier-1 ties within 15 % gap regardless of numVotes spread. Real catalogue
    noise (a 1988 video game named identically to the famous Robert Zemeckis
    film; zero-vote tvEpisodes named after the parent series) trips that gate
    even when popularity is asymmetric by 100×+. This Unit-3-layer override
    inspects the spread and clears top.multi_tie when:

    - len(matches) >= 2, and
    - matches[0].multi_tie is True, and
    - matches[0].start_year == matches[1].start_year (same-year guard prevents
      overriding genuine year-disambiguation cases like Dune 1984 vs 2021), and
    - matches[0].num_votes >= TIE_BREAK_VOTE_RATIO * max(1, matches[1].num_votes)
      (max(1, ...) avoids div-by-zero and makes 0-vote runners trivially
      overridden when top has any non-trivial popularity).

    Returns a new list (input list unchanged). The runner's multi_tie flag is
    left intact since downstream resolve() only inspects the top.

    Tune TIE_BREAK_VOTE_RATIO at the module-constant level if calibration
    drifts. Lower = more aggressive override; higher = more conservative.
    """
    if len(matches) < 2:
        return matches
    top, runner = matches[0], matches[1]
    if not top.multi_tie:
        return matches
    if top.start_year != runner.start_year:
        return matches
    runner_votes = max(1, runner.num_votes or 0)
    if (top.num_votes or 0) < TIE_BREAK_VOTE_RATIO * runner_votes:
        return matches
    return [replace(top, multi_tie=False)] + list(matches[1:])


def _parse_filename(raw: str) -> dict:
    """PTN parse + slug-dashes normalization for slug-shaped inputs."""
    parsed = PTN.parse(raw) if raw else {}
    ptt_title = (parsed.get("title") or raw or "").strip()
    ptt_year = parsed.get("year")

    # Slug-shaped input ("the-super-mario-galaxy-movie") → space-separated
    # title-cased ("The Super Mario Galaxy Movie") so IMDb FTS5 matches
    # against natural-language title text.
    if "-" in ptt_title and " " not in ptt_title:
        words = ptt_title.replace("-", " ").strip().split()
        ptt_title = " ".join(
            w if (w.isupper() and len(w) <= 4) else w.capitalize() for w in words
        )

    return {
        "raw_title": raw,
        "ptt_title": ptt_title,
        "ptt_year": ptt_year,
    }


def _get_directors(db_path: Path, tconst: str) -> list[dict]:
    """Lookup directors via title_crew.directors + name_basics join.

    Returns [] when DB is missing, the title has no crew row, or
    the directors field is null/empty. Opens its own read-only
    connection so it doesn't fight imdb_lookup's connection cache.
    """
    if not db_path.exists():
        return []
    try:
        c = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    except sqlite3.OperationalError:
        return []
    try:
        c.row_factory = sqlite3.Row
        row = c.execute(
            "SELECT directors FROM title_crew WHERE tconst = ?", (tconst,)
        ).fetchone()
        if not row or not row["directors"] or row["directors"] == r"\N":
            return []
        nconsts = [n.strip() for n in row["directors"].split(",") if n.strip()]
        if not nconsts:
            return []
        placeholders = ",".join("?" * len(nconsts))
        rows = c.execute(
            f"SELECT nconst, primaryName FROM name_basics "
            f"WHERE nconst IN ({placeholders})",
            nconsts,
        ).fetchall()
        by_nconst = {r["nconst"]: r["primaryName"] for r in rows}
        return [
            {"nconst": nc, "name": by_nconst[nc]}
            for nc in nconsts
            if nc in by_nconst
        ]
    finally:
        c.close()


def _assemble_imdb_block(top: Match, title: Title, directors: list[dict]) -> dict:
    """Compose the imdb sub-object that lands in the per-movie manifest."""
    akas: list[dict] = []
    for attr, lang in (("akas_pt", "pt"), ("akas_en", "en"), ("akas_es", "es")):
        for t in getattr(title, attr) or []:
            akas.append({"title": t, "language": lang})
    akas = akas[:AKAS_CAP]

    rating = None
    if title.average_rating is not None:
        rating = {
            "average": round(title.average_rating, 1),
            "votes": title.num_votes or 0,
        }

    top_cast = [
        {"nconst": c.get("nconst"), "name": c.get("name"), "role": None}
        for c in (title.top_cast or [])[:5]
    ]

    return {
        "tconst": title.tconst,
        "primaryTitle": title.primary_title,
        "originalTitle": title.original_title,
        "year": title.start_year,
        "genres": title.genres or [],
        "rating": rating,
        "director": directors,
        "plot": None,
        "top_cast": top_cast,
        "akas": akas,
        "confidence": 100 if top.fuzz_ratio == 100 else round(top.fuzz_ratio, 1),
        "multi_tie": False,
        "result": "resolved",
        "lookup_attempted": True,
    }


def _log_miss(
    log_path: Path,
    slug: Optional[str],
    raw_title: str,
    ptt_title: str,
    ptt_year: Optional[int],
    imdb: dict,
) -> None:
    """Atomic append of one JSONL line for any non-resolved outcome."""
    log_path.parent.mkdir(parents=True, exist_ok=True)
    entry = {
        "ts": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "slug": slug,
        "raw_title": raw_title,
        "ptt_title": ptt_title,
        "ptt_year": ptt_year,
        "result": imdb.get("result"),
        "candidates_considered": imdb.get("candidates_considered", 0),
    }
    if "top_score" in imdb:
        entry["top_score"] = imdb["top_score"]
    if "runner_up_score" in imdb:
        entry["runner_up_score"] = imdb["runner_up_score"]
    if "multi_tie" in imdb:
        entry["multi_tie"] = imdb["multi_tie"]
    with log_path.open("a") as f:
        f.write(json.dumps(entry, ensure_ascii=False) + "\n")


def _main(argv: Optional[list[str]] = None) -> int:
    """CLI entry: `python3 scripts/imdb_kb_enrich.py "<raw filename>"`.

    Prints the ResolutionResult as JSON. Useful for shell-script tests
    and manual debugging. Exit codes:
      0 — resolve completed (regardless of imdb.result)
      2 — usage error (missing argument)
    """
    args = list(argv) if argv is not None else sys.argv[1:]
    if not args:
        print("usage: imdb_kb_enrich.py <raw_filename> [--slug SLUG]", file=sys.stderr)
        return 2
    raw = args[0]
    slug = None
    if len(args) >= 3 and args[1] == "--slug":
        slug = args[2]
    result = resolve(raw, slug=slug)
    print(json.dumps(result.as_dict(), ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(_main())
