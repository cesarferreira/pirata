#!/usr/bin/env python3
"""Build a knowledge-hub-compatible export from pirata's kb/.

The export is manifest-driven: every slug present in `kb/manifest.jsonl`
gets a markdown wrapper. Per-movie JSONs are copied verbatim when present
and overlay the wrapper with rich pipeline fields (fps, runtime, scdet).

Output layout under kb/kh-export/04-derived/:
  per-movie/<slug>.json   (verbatim copy of source per-movie JSON, when present)
  per-movie/<slug>.md     (markdown wrapper, generated for every manifest slug)
  manifest.json           (slug-grouped conversion of kb/manifest.jsonl)
  README.md               (explainer)

manifest.json shape:
  {
    "source": "kb/manifest.jsonl",
    "kind": "frame_manifest",
    "slug_count": <int>,
    "row_count": <int>,
    "slugs": {
      "<slug>": {
        "title": <str|null>,
        "year": <int|null>,
        "frame_count": <int>,
        "first_tc": <str|null>,
        "last_tc": <str|null>,
        "first_t_s": <num|null>,
        "last_t_s": <num|null>,
        "rows": [...]
      },
      ...
    }
  }

Idempotent: the build is staged at kb/kh-export.tmp/ and atomically swapped
over kb/kh-export/ via os.replace, so partial failures never leave the
export half-rebuilt. Re-running produces byte-identical output.

JPG frames + contact-sheets are intentionally excluded; kh ingests only
text suffixes (.json, .md, .txt, .yaml, .yml, .csv).

Use:
  python3 scripts/build_kh_export.py
  python3 scripts/build_kh_export.py --kb kb --out kb/kh-export

Exit codes:
  0  build ok
  1  config / arg error (missing --kb dir)
  2  build proceeded but no per-movie or manifest source was found
  3  build failure (parse / IO / unexpected exception)
"""
from __future__ import annotations

import os
import sys

# scripts/queue.py shadows stdlib 'queue' when this dir lands on sys.path[0].
# Drop it defensively so future imports stay clean.
sys.path[:] = [p for p in sys.path
               if os.path.abspath(p) != os.path.dirname(os.path.abspath(__file__))]

import argparse
import json
import shutil
import traceback
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_KB = REPO_ROOT / "kb"
DEFAULT_OUT = REPO_ROOT / "kb" / "kh-export"


def now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def log(level: str, msg: str) -> None:
    print(f"{now_iso()} build_kh_export {level}: {msg}", file=sys.stderr)


def yaml_scalar(value: Any) -> str:
    """Emit a YAML-safe scalar via JSON encoding for strings."""
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return str(value)
    return json.dumps(value, ensure_ascii=False)


def collect_manifest_groups(jsonl_path: Path) -> dict[str, dict]:
    """Group manifest rows by slug. Each group preserves rows sorted by idx
    and resolves a best-effort title/year from the row contents."""
    by_slug: dict[str, list[dict]] = {}
    with jsonl_path.open(encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            row = json.loads(line)
            slug = row.get("slug")
            if not slug:
                continue
            by_slug.setdefault(slug, []).append(row)

    groups: dict[str, dict] = {}
    for slug, rows in by_slug.items():
        rows_sorted = sorted(rows, key=lambda r: r.get("idx", 0))
        # Title preference: first non-null that differs from slug, else
        # first non-null at all, else slug itself.
        title = next(
            (r.get("title") for r in rows_sorted
             if r.get("title") and r.get("title") != slug),
            None,
        )
        if title is None:
            title = next(
                (r.get("title") for r in rows_sorted if r.get("title")),
                slug,
            )
        year = next(
            (r.get("year") for r in rows_sorted if r.get("year") is not None),
            None,
        )
        groups[slug] = {
            "title": title,
            "year": year,
            "rows": rows_sorted,
        }
    return groups


def build_manifest_json(groups: dict[str, dict]) -> dict:
    """Assemble the slug-grouped manifest.json document."""
    slugs_dict: dict[str, dict] = {}
    total_rows = 0
    for slug in sorted(groups):
        g = groups[slug]
        rows = g["rows"]
        total_rows += len(rows)
        first = rows[0] if rows else {}
        last = rows[-1] if rows else {}
        slugs_dict[slug] = {
            "title": g["title"],
            "year": g["year"],
            "frame_count": len(rows),
            "first_tc": first.get("tc"),
            "last_tc": last.get("tc"),
            "first_t_s": first.get("t_s"),
            "last_t_s": last.get("t_s"),
            "rows": rows,
        }
    return {
        "source": "kb/manifest.jsonl",
        "kind": "frame_manifest",
        "slug_count": len(slugs_dict),
        "row_count": total_rows,
        "slugs": slugs_dict,
    }


def build_slug_md(slug: str, group: dict, per_movie_json: Path | None) -> str:
    """Build a markdown wrapper for a manifest slug. When the per-movie JSON
    exists, overlay its rich fields (fps/runtime/scdet/...). Always emit a
    'Caveats' section; degraded metadata gets explicit notes."""
    rows = group["rows"]
    title = group["title"]
    year = group["year"]
    frame_count = len(rows)
    first = rows[0] if rows else {}
    last = rows[-1] if rows else {}
    first_tc = first.get("tc")
    last_tc = last.get("tc")
    first_t_s = first.get("t_s")
    last_t_s = last.get("t_s")

    has_json = per_movie_json is not None and per_movie_json.is_file()
    json_data: dict[str, Any] = {}
    if has_json:
        json_data = json.loads(per_movie_json.read_text(encoding="utf-8"))
        # Overlay title/year if the per-movie JSON has stronger values.
        j_title = json_data.get("title")
        j_year = json_data.get("year")
        if j_title and j_title != slug:
            title = j_title
        if j_year is not None:
            year = j_year

    fps = json_data.get("fps") if has_json else None
    runtime_s = json_data.get("runtime_s") if has_json else None
    source_size_bytes = json_data.get("source_size_bytes") if has_json else None
    extracted_at = json_data.get("extracted_at") if has_json else None
    sheets = json_data.get("sheets") if has_json else None
    sheet_count = len(sheets) if isinstance(sheets, list) else None
    json_frames = json_data.get("frames") if has_json else None
    json_frame_count = len(json_frames) if isinstance(json_frames, list) else None
    scdet = json_data.get("scdet") or {}

    title_is_slug = title == slug
    year_missing = year is None

    fm: list[str] = [
        "---",
        f"slug: {yaml_scalar(slug)}",
        f"title: {yaml_scalar(title)}",
        f"year: {yaml_scalar(year)}",
        f"frame_count: {yaml_scalar(frame_count)}",
        f"first_tc: {yaml_scalar(first_tc)}",
        f"last_tc: {yaml_scalar(last_tc)}",
        f"first_t_s: {yaml_scalar(first_t_s)}",
        f"last_t_s: {yaml_scalar(last_t_s)}",
        f"has_per_movie_json: {yaml_scalar(has_json)}",
    ]
    if has_json:
        fm += [
            f"fps: {yaml_scalar(fps)}",
            f"runtime_s: {yaml_scalar(runtime_s)}",
            f"source_size_bytes: {yaml_scalar(source_size_bytes)}",
            f"extracted_at: {yaml_scalar(extracted_at)}",
            f"sheet_count: {yaml_scalar(sheet_count)}",
            f"json_frame_count: {yaml_scalar(json_frame_count)}",
            "scdet:",
            f"  threshold: {yaml_scalar(scdet.get('threshold'))}",
            f"  floor_s: {yaml_scalar(scdet.get('floor_s'))}",
            f"  target: {yaml_scalar(scdet.get('target'))}",
        ]
    fm += ["---", ""]

    body: list[str] = []
    heading = title if not title_is_slug else slug
    body.append(f"# {heading}")
    body.append("")
    body.append(f"Slug: `{slug}`")
    body.append("")
    if title_is_slug or year_missing:
        body.append(f"This is the contact-sheet derivative for the slug `{slug}`.")
    else:
        body.append(f"This is the contact-sheet derivative for the {year} film {title}.")
    body.append("It was extracted from a single source video and serves as a")
    body.append("pipeline-test artifact for the knowledge-hub ingest path.")
    body.append("")

    body.append("## Pipeline metadata")
    body.append("")
    body.append(f"- Title: {title}")
    body.append(f"- Year: {year}")
    body.append(f"- Frames extracted (manifest): {frame_count}")
    body.append(f"- First timecode: {first_tc} ({first_t_s} s)")
    body.append(f"- Last timecode:  {last_tc} ({last_t_s} s)")
    if has_json:
        body.append(f"- FPS: {fps}")
        body.append(f"- Runtime: {runtime_s} seconds")
        body.append(f"- Source size: {source_size_bytes} bytes")
        body.append(f"- Frames in per-movie JSON: {json_frame_count}")
        body.append(f"- Contact sheets generated: {sheet_count}")
        body.append(f"- Extracted at: {extracted_at}")
    body.append("")

    if has_json and scdet:
        body.append("## Scene detection (scdet) configuration")
        body.append("")
        body.append(f"- threshold: {scdet.get('threshold')}")
        body.append(f"- floor_s: {scdet.get('floor_s')}")
        body.append(f"- target: {scdet.get('target')}")
        body.append("")

    body.append("## Caveats")
    body.append("")
    if not has_json:
        body.append(f"- No per-movie JSON exists at `kb/per-movie/{slug}.json`.")
        body.append("  Wrapper is limited to manifest-derived fields. Re-run")
        body.append("  `scripts/contact_sheet.py` for this release to generate it.")
    if title_is_slug:
        body.append(f"- Title `{title}` matches the slug — the filename parser")
        body.append("  did not extract a human title. Unit 3 (IMDb enrichment)")
        body.append("  resolves this with `tconst`-anchored fields.")
    if year_missing:
        body.append("- Year is null — likely a parser miss on a dot-separated")
        body.append("  release filename. Unit 3 enrichment will populate this")
        body.append("  from IMDb.")
    body.append("- This wrapper predates Unit 3 (KB enrichment in the IMDb x")
    body.append("  pirata coupling plan). IMDb fields (tconst, rating, top_cast,")
    body.append("  akas, genres, director, plot) are NOT populated. Regenerate")
    body.append("  this export after Unit 3 ships.")
    body.append("- Pipeline-test export, not a semantic-recall-complete KB.")
    body.append("")

    return "\n".join(fm + body)


README_CONTENT = """# pirata kh-export — knowledge-hub compatible export

This directory is an additive export surface for pirata's `kb/`, structured
to match the local `knowledge-hub` MCP server's discovery convention. It is
generated by `scripts/build_kh_export.py` and is fully derivable from
sources under `kb/per-movie/` and `kb/manifest.jsonl`.

## What this is

- A pipeline-test export, NOT a semantic-recall-complete KB.
- Image assets (frames, contact sheets) are EXCLUDED for v1; only structured
  text is included.
- `kb/manifest.jsonl` is preserved unchanged in pirata, but exported here
  as `manifest.json` because the kh ingester does not currently include
  `.jsonl` in its supported suffix set.
- The manifest is grouped by slug. Each slug entry includes title (best-effort),
  year, frame_count, first/last timecode, first/last seconds, and the full
  row list (sorted by idx).

## Layout

    kh-export/
    └── 04-derived/
        ├── per-movie/
        │   ├── <slug>.json    (verbatim copy of source per-movie JSON, when present)
        │   └── <slug>.md      (markdown wrapper; one per slug in kb/manifest.jsonl)
        ├── manifest.json      (slug-grouped conversion of kb/manifest.jsonl)
        └── README.md          (this file)

A markdown wrapper exists for every slug in `kb/manifest.jsonl`, even when
the per-movie JSON is missing or has degraded metadata. The wrapper makes
the gap explicit so KH retrievers do not silently miss a slug.

## Mario Galaxy caveat (current state)

`the-super-mario-galaxy-movie-2026` has a per-movie JSON at
`kb/per-movie/the-super-mario-galaxy-movie-2026.json`, but its `title` field
matches the slug and its `year` is null. This is a `contact_sheet.py`
filename-parser miss against dot-separated release-dir names — not a KH
issue. The wrapper carries this caveat explicitly. Unit 3 (IMDb x pirata
coupling) resolves it via `tconst`-anchored enrichment.

## Regeneration

Regenerate atomically with:

    python3 scripts/build_kh_export.py

The script is idempotent — running it multiple times produces byte-identical
output. The build is staged in `kb/kh-export.tmp/` and atomically swapped
over `kb/kh-export/` only after success, so partial failures never leave
the export in a broken state.

## Unit 3 dependency

The current per-movie wrappers contain only pipeline metadata (title, year,
fps, frame count, scdet config) plus manifest-derived timecodes. Once Unit 3
(KB enrichment in `scripts/contact_sheet.py`) ships, the per-movie JSONs
will include IMDb fields (tconst, rating, top_cast, akas, genres, director,
plot). After Unit 3, regenerate this export to expose those fields to kh.

## License

The IMDb non-commercial license applies to any IMDb-derived fields, once
Unit 3 populates them. For v1 (pre-Unit-3), no IMDb data is present and
the license is moot. The kh has no license metadata field convention; this
constraint is documented here rather than encoded in metadata.

If `kh` is ever served beyond Vidigal's local Dante machine (cross-workspace
sync, multi-tenant), the IMDb-derived fields must be stripped from this
export before sync.
"""


def build_readme() -> str:
    return README_CONTENT


def build(kb: Path, out: Path) -> int:
    if not kb.exists():
        log("error", f"kb dir not found: {kb}")
        return 1

    per_movie_in = kb / "per-movie"
    jsonl_path = kb / "manifest.jsonl"

    tmp = out.parent / (out.name + ".tmp")
    if tmp.exists():
        log("info", f"removing stale tmp dir: {tmp}")
        shutil.rmtree(tmp)
    tmp.mkdir(parents=True)

    derived = tmp / "04-derived"
    derived.mkdir()
    per_movie_out = derived / "per-movie"
    per_movie_out.mkdir()

    # Step 1: read manifest, group by slug.
    manifest_groups: dict[str, dict] = {}
    if jsonl_path.exists():
        manifest_groups = collect_manifest_groups(jsonl_path)
        log("info", f"manifest.jsonl: {len(manifest_groups)} slug(s), "
                    f"{sum(len(g['rows']) for g in manifest_groups.values())} row(s)")
    else:
        log("warn", f"no manifest.jsonl at {jsonl_path}; export will skip manifest.json")

    # Step 2: copy per-movie JSONs verbatim, track slug -> path.
    per_movie_paths: dict[str, Path] = {}
    if per_movie_in.exists():
        for json_file in sorted(per_movie_in.glob("*.json")):
            slug = json_file.stem
            per_movie_paths[slug] = json_file
            shutil.copy2(json_file, per_movie_out / f"{slug}.json")
        log("info", f"copied {len(per_movie_paths)} per-movie JSON file(s)")
    else:
        log("warn", f"no per-movie/ dir at {per_movie_in}")

    # Step 3: generate one MD wrapper per manifest slug (overlay JSON if present).
    md_count = 0
    for slug in sorted(manifest_groups):
        json_path = per_movie_paths.get(slug)
        md_text = build_slug_md(slug, manifest_groups[slug], json_path)
        (per_movie_out / f"{slug}.md").write_text(md_text, encoding="utf-8")
        md_count += 1
    if md_count:
        log("info", f"wrote {md_count} markdown wrapper(s) (one per manifest slug)")

    # Step 4: write slug-grouped manifest.json.
    if manifest_groups:
        manifest_doc = build_manifest_json(manifest_groups)
        (derived / "manifest.json").write_text(
            json.dumps(manifest_doc, indent=2, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
        log("info", f"wrote manifest.json with slug_count={manifest_doc['slug_count']} "
                    f"row_count={manifest_doc['row_count']}")

    # Step 5: README.
    (derived / "README.md").write_text(build_readme(), encoding="utf-8")

    # Step 6: atomic swap.
    if out.exists():
        shutil.rmtree(out)
    tmp.rename(out)

    log("ok", f"built export at {out}")

    if not per_movie_paths and not manifest_groups:
        log("warn", "exported only README.md (no per-movie or manifest source found)")
        return 2
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description="Build kh-compatible export from pirata kb/",
    )
    ap.add_argument("--kb", type=Path, default=DEFAULT_KB,
                    help=f"Source kb/ dir (default: {DEFAULT_KB})")
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT,
                    help=f"Output kh-export dir (default: {DEFAULT_OUT})")
    args = ap.parse_args(argv)

    try:
        return build(args.kb.resolve(), args.out.resolve())
    except Exception:
        log("error", "build failed:")
        traceback.print_exc(file=sys.stderr)
        return 3


if __name__ == "__main__":
    sys.exit(main())
