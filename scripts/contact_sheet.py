#!/usr/bin/env python3
"""Editorial contact sheet generator for PIRATA.

Pipeline:
1. ffprobe scdet (scaled-down decode) → raw scene timestamps
2. min-interval floor + target cap → final frame list
3. ffmpeg per-frame seek in parallel → raw frames (fast+accurate seek)
4. Pillow labels (frame# + TC, Monaco mono) → labeled frames
5. Pillow tiles (CxR) with header bar → multi-sheet PNG

Use: python3 scripts/contact_sheet.py <mkv> --out <dir> [--title X]
"""
from __future__ import annotations

import os
import sys

# scripts/queue.py (pirata aria2c wrapper) shadows stdlib 'queue' when this
# file runs — Python adds the script's dir to sys.path[0]. Drop it early so
# concurrent.futures/multiprocessing can import stdlib queue cleanly.
sys.path[:] = [p for p in sys.path
               if os.path.abspath(p) != os.path.dirname(os.path.abspath(__file__))]

import argparse
import json
import re
import subprocess
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


FFMPEG = os.environ.get("FFMPEG", "/opt/homebrew/opt/ffmpeg-full/bin/ffmpeg")
FFPROBE = os.environ.get("FFPROBE", "/opt/homebrew/opt/ffmpeg-full/bin/ffprobe")


def fmt_tc(seconds: float) -> str:
    h = int(seconds // 3600)
    m = int((seconds % 3600) // 60)
    s = int(seconds % 60)
    return f"{h:02d}:{m:02d}:{s:02d}"


def fmt_tc_ff(seconds: float, fps: float) -> str:
    """Broadcast timecode HH:MM:SS:FF (non-drop-frame)."""
    nominal = int(round(fps))
    total_frames = int(seconds * fps)
    ff = total_frames % nominal
    total_s = int(total_frames // nominal) if nominal else int(seconds)
    s = total_s % 60
    m = (total_s // 60) % 60
    h = total_s // 3600
    return f"{h:02d}:{m:02d}:{s:02d}:{ff:02d}"


def probe_fps(mkv: Path) -> float:
    cmd = [FFPROBE, "-v", "error", "-select_streams", "v:0",
           "-show_entries", "stream=r_frame_rate",
           "-of", "default=noprint_wrappers=1:nokey=1", str(mkv)]
    out = subprocess.check_output(cmd, text=True).strip().splitlines()[0]
    if "/" in out:
        num, den = out.split("/")
        n, d = int(num), int(den)
        return n / d if d else 23.976
    try:
        return float(out)
    except ValueError:
        return 23.976


def probe_duration(mkv: Path) -> float:
    """Probe video duration in seconds via ffprobe; 0.0 on failure."""
    cmd = [FFPROBE, "-v", "error", "-show_entries", "format=duration",
           "-of", "default=noprint_wrappers=1:nokey=1", str(mkv)]
    try:
        out = subprocess.check_output(cmd, text=True).strip()
        return float(out) if out else 0.0
    except (subprocess.CalledProcessError, ValueError):
        return 0.0


def parse_year_from_title(title: str) -> int | None:
    """Extract trailing (YYYY) from a movie title, or None."""
    m = re.search(r"\((\d{4})\)\s*$", title)
    return int(m.group(1)) if m else None


def escape_movie_path(p: Path) -> str:
    """Escape a filesystem path for ffmpeg's movie= source (filter expression)."""
    s = str(p).replace("\\", "\\\\").replace("'", r"\'")
    return f"'{s}'"


def detect_scenes(mkv: Path, threshold: int, out_dir: Path,
                  scale_w: int = 640) -> list[float]:
    """Scene timestamps via scdet on a scaled-down decode. Cached to disk."""
    cache = out_dir / f"scenes_raw_t{threshold}.txt"
    if cache.exists() and cache.stat().st_size > 0:
        scenes = sorted({float(x) for x in cache.read_text().split() if x.strip()})
        print(f"[scdet] {len(scenes)} scenes loaded from cache {cache.name}", flush=True)
        return scenes

    src = escape_movie_path(mkv)
    filt = f"movie={src},scale={scale_w}:-2,scdet=threshold={threshold}"
    cmd = [
        FFPROBE, "-v", "quiet",
        "-f", "lavfi",
        "-i", filt,
        "-show_entries", "frame_tags=lavfi.scd.time",
        "-of", "csv=p=0",
    ]
    t0 = time.time()
    print(f"[scdet] ffprobe threshold={threshold} scale={scale_w}w ...", flush=True)
    out = subprocess.check_output(cmd, text=True, timeout=1800)
    dt = time.time() - t0
    scenes: list[float] = []
    for line in out.splitlines():
        line = line.strip().rstrip(",")
        if not line:
            continue
        try:
            scenes.append(float(line))
        except ValueError:
            continue
    scenes = sorted(set(scenes))
    cache.write_text("\n".join(f"{t:.3f}" for t in scenes) + "\n")
    print(f"[scdet] {len(scenes)} raw scenes in {dt:.1f}s (cached: {cache.name})", flush=True)
    return scenes


def apply_floor(scenes: list[float], floor: float) -> list[float]:
    if not scenes:
        return []
    kept = [scenes[0]]
    for t in scenes[1:]:
        if t - kept[-1] >= floor:
            kept.append(t)
    return kept


def cap_target(scenes: list[float], target: int) -> list[float]:
    if len(scenes) <= target:
        return scenes
    step = len(scenes) / target
    return [scenes[int(i * step)] for i in range(target)]


def _extract_one(args):
    idx, t, mkv, out_dir, width = args
    out_path = out_dir / f"raw_{idx:04d}.png"
    cmd = [
        FFMPEG, "-nostdin", "-loglevel", "error",
        "-ss", f"{t:.3f}",
        "-i", str(mkv),
        "-frames:v", "1",
        "-vf", f"scale={width}:-2",
        "-y",
        str(out_path),
    ]
    try:
        subprocess.run(cmd, check=True, capture_output=True, timeout=90)
    except subprocess.CalledProcessError as e:
        return (idx, t, None, e.stderr.decode("utf-8", errors="replace")[:200])
    except Exception as e:
        return (idx, t, None, str(e)[:200])
    return (idx, t, out_path, None)


def slugify(s: str) -> str:
    import re
    s = s.lower().strip()
    s = re.sub(r"[^a-z0-9]+", "-", s)
    return s.strip("-") or "contact"


def label_frame(img: Image.Image, idx: int, t: float, fps: float,
                caption_font: ImageFont.FreeTypeFont) -> Image.Image:
    """Compose thumb + caption strip below. Frame stays clean, no overlays."""
    img = img.convert("RGB")
    w, h = img.size
    font_size = caption_font.size
    strip_h = max(50, int(font_size * 1.7))
    composite = Image.new("RGB", (w, h + strip_h), (22, 22, 22))
    composite.paste(img, (0, 0))

    draw = ImageDraw.Draw(composite)
    caption = f"N {idx:03d}  ·  TC {fmt_tc_ff(t, fps)}"
    bbox = draw.textbbox((0, 0), caption, font=caption_font)
    ch = bbox[3] - bbox[1]
    x = max(12, font_size // 3)
    y = h + (strip_h - ch) // 2 - 2
    draw.text((x, y), caption, fill=(230, 230, 230), font=caption_font)
    return composite


def tile_sheets(items, cols: int, rows: int,
                out_dir: Path, title: str, slug: str,
                header_font_size: int,
                ext: str = "png") -> list[Path]:
    """
    items: list of (idx, t, labeled_image) tuples where labeled_image is a
    PIL Image with caption strip below the frame (from label_frame()).
    Outputs PNG by default; pass ext="jpg" for KB-lighter JPEG q=90.
    """
    per = cols * rows
    if not items:
        return []
    out_dir.mkdir(parents=True, exist_ok=True)
    tw, th = items[0][2].size
    header_h = max(48, int(header_font_size * 1.8))
    total = len(items)
    num_sheets = (total + per - 1) // per
    sheets: list[Path] = []
    hfont = ImageFont.truetype("/System/Library/Fonts/Helvetica.ttc", header_font_size)
    for s in range(num_sheets):
        chunk = items[s * per:(s + 1) * per]
        sheet = Image.new("RGB", (cols * tw, header_h + rows * th), (10, 10, 10))
        hdraw = ImageDraw.Draw(sheet)
        first_idx = chunk[0][0]
        last_idx = chunk[-1][0]
        htxt = (f"{title}  ·  sheet {s+1:02d}/{num_sheets:02d}"
                f"  ·  frames {first_idx:03d}–{last_idx:03d}  ·  {len(chunk)} thumbs")
        hy = (header_h - header_font_size) // 2 - 2
        hdraw.text((max(14, header_font_size // 2), max(8, hy)),
                   htxt, fill=(230, 230, 230), font=hfont)
        for i, (_, _, src) in enumerate(chunk):
            r = i // cols
            c = i % cols
            sheet.paste(src, (c * tw, header_h + r * th))
        out = out_dir / f"{slug}_sheet_{s+1:02d}.{ext}"
        if ext.lower() in ("jpg", "jpeg"):
            sheet.save(out, format="JPEG", quality=90, optimize=True)
        else:
            sheet.save(out, optimize=True)
        size_mb = out.stat().st_size / 1024 / 1024
        print(f"[tile] {out.name}  ({len(chunk)} thumbs, {size_mb:.1f}MB)", flush=True)
        sheets.append(out)
    return sheets


def export_kb(kb_root: Path, slug: str, title: str,
              labeled: list,
              results: list[tuple[int, float, Path]],
              mkv: Path, fps: float,
              threshold: int, floor: float, target: int,
              cols: int, rows: int,
              header_font_size: int, force: bool,
              kb_imdb: bool = True) -> bool:
    """Export RAG-ready artifacts to <kb_root>/. Frames are pristine JPEGs;
    sheets are the labeled tile re-encoded as JPEG q=90 (lighter than the
    PNG that lives in release/contact-sheets/). Returns True if exported,
    False if skipped due to idempotency."""
    kb_root = Path(kb_root).resolve()
    movie_json = kb_root / "per-movie" / f"{slug}.json"
    if movie_json.exists() and not force:
        print(f"[kb] skip: {slug} already exported (use --kb-force to redo)", flush=True)
        return False

    frames_dir = kb_root / "frames" / slug
    sheets_dir = kb_root / "contact-sheets" / slug
    movie_dir = kb_root / "per-movie"
    frames_dir.mkdir(parents=True, exist_ok=True)
    sheets_dir.mkdir(parents=True, exist_ok=True)
    movie_dir.mkdir(parents=True, exist_ok=True)

    # 1. Save raw frames as JPEG q=90 — pristine, no overlay.
    print(f"[kb] writing {len(results)} clean frames to {frames_dir}", flush=True)
    for idx, _, raw_path in results:
        out = frames_dir / f"{slug}_frame_{idx:03d}.jpg"
        with Image.open(raw_path) as im:
            im.convert("RGB").save(out, format="JPEG", quality=90, optimize=True)

    # 2. Re-tile labeled sheets as JPEG — same numbering + TC + header as the
    #    release PNG sheet, just lighter (~30% size).
    kb_sheets = tile_sheets(labeled, cols, rows, sheets_dir,
                            title, slug, header_font_size, ext="jpg")

    # 3. Build per-movie manifest.
    runtime = probe_duration(mkv)
    try:
        size_bytes = mkv.stat().st_size
    except OSError:
        size_bytes = 0

    # IMDb resolution: when kb_imdb is on, resolve title/year/metadata
    # via the local IMDb catalog. Top-level manifest title/year become
    # the canonical values when imdb.result == resolved; original
    # filename trace lives under filename.{raw_title, ptt_title, ptt_year}
    # for debuggability. On any IMDb miss (no_match, multi_tie,
    # db_unavailable), canonical falls back to PTN-cleaned values so
    # the slug-shaped-title bug still gets cleaned up at the parser
    # level even when full IMDb resolution doesn't lock onto a tconst.
    filename_block = None
    imdb_block = None
    if kb_imdb:
        try:
            # scripts/ was stripped from sys.path at module load (lines 18-22)
            # to keep stdlib `queue` from being shadowed by scripts/queue.py
            # at import time. By the time export_kb runs, concurrent.futures
            # / multiprocessing have already cached stdlib queue, so we can
            # safely re-add scripts/ here for the sibling import.
            scripts_dir = os.path.dirname(os.path.abspath(__file__))
            if scripts_dir not in sys.path:
                sys.path.insert(0, scripts_dir)
            from imdb_kb_enrich import resolve as imdb_resolve
            res = imdb_resolve(title, slug=slug)
            manifest_title = res.canonical_title
            manifest_year = res.canonical_year
            filename_block = res.filename
            imdb_block = res.imdb
            print(f"[kb-imdb] {slug}: result={imdb_block.get('result')} "
                  f"canonical={manifest_title!r} year={manifest_year}", flush=True)
        except ImportError as e:
            print(f"[kb-imdb] disabled (PTN missing): {e}", flush=True)
            manifest_title = title
            manifest_year = parse_year_from_title(title)
    else:
        manifest_title = title
        manifest_year = parse_year_from_title(title)

    per = cols * rows
    frames_meta = []
    for idx, t, _ in results:
        sheet_idx = ((idx - 1) // per) + 1
        pos = (idx - 1) % per
        frames_meta.append({
            "idx": idx,
            "file": f"{slug}_frame_{idx:03d}.jpg",
            "tc": fmt_tc_ff(t, fps),
            "t_s": round(t, 3),
            "sheet": sheet_idx,
            "pos": [pos // cols, pos % cols],
            "caption": None,
        })

    sheets_meta = []
    for s in range(len(kb_sheets)):
        first_idx = s * per + 1
        last_idx = min((s + 1) * per, len(results))
        sheets_meta.append({
            "n": s + 1,
            "file": f"{slug}_sheet_{s+1:02d}.jpg",
            "frame_range": [first_idx, last_idx],
        })

    manifest = {
        "slug": slug,
        "title": manifest_title,
        "year": manifest_year,
        "fps": round(fps, 3),
        "runtime_s": round(runtime, 3),
        "source_file": str(mkv),
        "source_size_bytes": size_bytes,
        "scdet": {"threshold": threshold, "floor_s": floor, "target": target},
        "extracted_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    }
    if filename_block is not None:
        manifest["filename"] = filename_block
    if imdb_block is not None:
        manifest["imdb"] = imdb_block
    manifest["frames"] = frames_meta
    manifest["sheets"] = sheets_meta

    # 4. Atomic write of per-movie JSON via temp+rename.
    tmp = movie_json.with_suffix(".json.tmp")
    tmp.write_text(json.dumps(manifest, indent=2, ensure_ascii=False))
    tmp.replace(movie_json)
    print(f"[kb] wrote {movie_json}", flush=True)

    # 5. Append global JSONL (single write call for atomicity).
    jsonl_path = kb_root / "manifest.jsonl"
    lines = []
    for fm in frames_meta:
        line_obj = {
            "slug": slug,
            "idx": fm["idx"],
            "file": f"frames/{slug}/{fm['file']}",
            "tc": fm["tc"],
            "t_s": fm["t_s"],
            "title": manifest_title,
            "year": manifest_year,
        }
        lines.append(json.dumps(line_obj, ensure_ascii=False))
    with jsonl_path.open("a") as f:
        f.write("\n".join(lines) + "\n")
    print(f"[kb] appended {len(lines)} lines to {jsonl_path}", flush=True)
    return True


def main():
    ap = argparse.ArgumentParser(description="Editorial scene-detected contact sheet")
    ap.add_argument("mkv", type=Path)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--threshold", type=int, default=8,
                    help="scdet threshold (lower=more scenes; default 8)")
    ap.add_argument("--floor", type=float, default=4.0,
                    help="min seconds between kept scenes (default 4)")
    ap.add_argument("--target", type=int, default=300,
                    help="cap final scene count (default 300)")
    ap.add_argument("--cols", type=int, default=10)
    ap.add_argument("--rows", type=int, default=10)
    ap.add_argument("--width", type=int, default=480,
                    help="thumbnail width px (default 480)")
    ap.add_argument("--workers", type=int, default=6)
    ap.add_argument("--title", default="")
    ap.add_argument("--keep-raw", action="store_true",
                    help="retain raw frames dir after tiling")
    ap.add_argument("--kb-export", type=Path, default=None,
                    help="root dir to write RAG-ready clean frames + sheets + manifests")
    ap.add_argument("--kb-force", action="store_true",
                    help="re-export KB artifacts even if per-movie JSON exists")
    ap.add_argument("--kb-imdb", action=argparse.BooleanOptionalAction, default=True,
                    help="Resolve title/year/genres/rating/cast via IMDb local catalog "
                         "(default on; pass --no-kb-imdb to skip). Only applies when "
                         "--kb-export is set.")
    args = ap.parse_args()

    mkv = args.mkv.resolve()
    if not mkv.exists():
        sys.exit(f"ERROR: mkv not found: {mkv}")

    args.out.mkdir(parents=True, exist_ok=True)
    frames_dir = args.out / "frames"
    frames_dir.mkdir(parents=True, exist_ok=True)

    # 1. scene detection (cached)
    raw = detect_scenes(mkv, args.threshold, args.out)

    # 2. floor + cap
    floored = apply_floor(raw, args.floor)
    print(f"[floor] {len(floored)} after {args.floor}s floor", flush=True)
    final = cap_target(floored, args.target)
    print(f"[cap]   {len(final)} after target={args.target}", flush=True)
    if not final:
        sys.exit("ERROR: no scenes after filtering; try --threshold lower")

    # 3. extract
    t0 = time.time()
    print(f"[extract] {len(final)} frames · {args.workers} workers ...", flush=True)
    tasks = [(i + 1, t, mkv, frames_dir, args.width)
             for i, t in enumerate(final)]
    results: list[tuple[int, float, Path]] = []
    errors = 0
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures = [pool.submit(_extract_one, task) for task in tasks]
        for fut in as_completed(futures):
            idx, t, path, err = fut.result()
            if err:
                errors += 1
                if errors <= 3:
                    print(f"[extract] fail idx={idx}: {err}", flush=True)
                continue
            results.append((idx, t, path))
            if len(results) % 25 == 0:
                print(f"[extract] {len(results)}/{len(tasks)}", flush=True)
    dt = time.time() - t0
    print(f"[extract] done {len(results)} ok · {errors} err · {dt:.1f}s", flush=True)
    if not results:
        sys.exit("ERROR: no frames extracted")
    results.sort(key=lambda x: x[0])

    # 4. label — caption strip below each thumb (no overlay on frame).
    #    Format: "N 001  ·  TC HH:MM:SS:FF" using source fps.
    fps = probe_fps(mkv)
    caption_font_size = max(22, args.width // 20)
    header_font_size = max(24, args.width // 14)
    print(f"[label]  caption={caption_font_size}pt  header={header_font_size}pt  "
          f"fps={fps:.3f}", flush=True)
    caption_font = ImageFont.truetype("/System/Library/Fonts/Monaco.ttf",
                                      caption_font_size)
    labeled = [(idx, t, label_frame(Image.open(p), idx, t, fps, caption_font))
               for idx, t, p in results]

    # 5. tile
    title = args.title or "Contact Sheet"
    slug = slugify(title)
    sheets = tile_sheets(labeled, args.cols, args.rows, args.out,
                         title, slug, header_font_size)

    # 6. KB export (optional) — runs BEFORE cleanup so raw frames are still on disk.
    if args.kb_export:
        export_kb(
            kb_root=args.kb_export,
            slug=slug,
            title=title,
            labeled=labeled,
            results=results,
            mkv=mkv,
            fps=fps,
            threshold=args.threshold,
            floor=args.floor,
            target=args.target,
            cols=args.cols,
            rows=args.rows,
            header_font_size=header_font_size,
            force=args.kb_force,
            kb_imdb=args.kb_imdb,
        )

    # cleanup
    if not args.keep_raw:
        for _, _, p in results:
            try:
                p.unlink()
            except OSError:
                pass
        try:
            frames_dir.rmdir()
        except OSError:
            pass
        print("[clean] raw frames removed (use --keep-raw to retain)", flush=True)

    total_mb = sum(p.stat().st_size for p in sheets) / 1024 / 1024
    print(f"\n[done] {len(sheets)} sheets · {total_mb:.1f}MB total · {args.out}")
    for p in sheets:
        print(f"  {p}")


if __name__ == "__main__":
    main()
