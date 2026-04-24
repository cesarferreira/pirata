#!/usr/bin/env python3
"""Opportunistic contact sheet sweeper for pirata.

Walks downloads/ looking for release directories containing a qualifying
video file but no contact-sheets/ with sheets inside, and invokes
contact_sheet.py on each. Serial, idempotent, path-agnostic.

Usage:
  python3 scripts/sheets_sweep.py [--downloads PATH] [--skip GLOB ...]
                                   [--dry-run] [--force]

Exit codes:
  0  sweep completed (possibly with skips)
  1  config/prereq error
  4  sweep completed but at least one release failed
"""
from __future__ import annotations

import os
import sys

# Drop scripts/ from sys.path so stdlib queue (needed by subprocess /
# concurrent.futures etc.) imports cleanly rather than shadowing with
# scripts/queue.py in this repo.
sys.path[:] = [p for p in sys.path
               if os.path.abspath(p) != os.path.dirname(os.path.abspath(__file__))]

import argparse
import fcntl
import fnmatch
import re
import shutil
import signal
import subprocess
import time
import tomllib
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
PIRATA_CONFIG = Path.home() / ".config" / "pirata" / "config.toml"
CONTACT_SHEET = REPO_ROOT / "scripts" / "contact_sheet.py"
LOG_FILE = REPO_ROOT / "logs" / "sheets_sweep.log"
LOCK_FILE = REPO_ROOT / "logs" / ".sheets_sweep.lock"

VIDEO_EXTS = {".mkv", ".mp4", ".avi", ".mov", ".ts", ".m2ts", ".webm"}
SKIP_PATTERNS = ("sample", "trailer", "extras", "making")
MIN_SIZE_BYTES = int(os.environ.get("AUTOSHEETS_MIN_SIZE_MB", "300")) * 1024 * 1024
DISK_FREE_FLOOR = 0.10


def now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def sanitize(x) -> str:
    """repr-escape for user-controlled content in log lines."""
    return repr(str(x))


def read_downloads_root() -> Path | None:
    if not PIRATA_CONFIG.exists():
        return None
    try:
        cfg = tomllib.loads(PIRATA_CONFIG.read_text())
        d = cfg.get("aria2", {}).get("download_dir")
        if d:
            return Path(d).resolve()
    except tomllib.TOMLDecodeError:
        pass
    return None


def log(state: str, detail: str = "") -> None:
    """Append one log line. Filename-derived content must be pre-sanitized."""
    LOG_FILE.parent.mkdir(parents=True, exist_ok=True)
    line = f"{now_iso()} sweep {state}"
    if detail:
        line += f" {detail}"
    with LOG_FILE.open("a") as f:
        f.write(line + "\n")


def slugify(name: str) -> str:
    """Strip [tags], collapse whitespace; fallback to raw name if empty."""
    s = re.sub(r"\[[^\]]+\]", "", name).strip()
    s = re.sub(r"\s+", " ", s)
    return s or name


def is_skip_name(name: str) -> bool:
    lower = name.lower()
    return any(p in lower for p in SKIP_PATTERNS)


def already_sheeted(release_dir: Path) -> bool:
    """True iff contact-sheets/ exists AND contains >=1 *_sheet_*.png file."""
    cs = release_dir / "contact-sheets"
    if not cs.is_dir():
        return False
    return any(cs.glob("*_sheet_*.png"))


def find_videos(release_dir: Path, downloads_root: Path) -> list[Path]:
    """Qualifying video files inside release_dir. Returns resolved paths
    confined to downloads_root. Out-of-root items are logged + skipped."""
    out: list[Path] = []
    for p in release_dir.rglob("*"):
        if any(part.startswith(".") for part in p.parts):
            continue
        try:
            if not p.is_file():
                continue
        except OSError:
            continue
        if p.suffix.lower() not in VIDEO_EXTS:
            continue
        if is_skip_name(p.name):
            continue
        try:
            size = p.stat().st_size
        except OSError:
            continue
        if size < MIN_SIZE_BYTES:
            continue
        try:
            resolved = p.resolve(strict=True)
        except (OSError, RuntimeError):
            continue
        if not resolved.is_relative_to(downloads_root):
            log("reject", f"path outside root {sanitize(resolved)}")
            continue
        out.append(resolved)
    return out


def run_contact_sheet(video: Path, out_dir: Path, title: str) -> tuple[int, str]:
    """Invoke contact_sheet.py in a new session; propagate SIGTERM/SIGINT
    to the subprocess tree via killpg. Returns (rc, stderr_tail)."""
    argv = [
        sys.executable, str(CONTACT_SHEET),
        "--out", str(out_dir),
        "--title", title,
        "--threshold", "8",
        "--floor", "4",
        "--target", "300",
        "--cols", "6", "--rows", "5",
        "--width", "640",
        "--workers", "6",
        "--",
        str(video),
    ]
    proc = subprocess.Popen(
        argv,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )

    def handler(signum, frame):
        try:
            os.killpg(proc.pid, signal.SIGTERM)
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        raise SystemExit(130 if signum == signal.SIGINT else 143)

    old_term = signal.signal(signal.SIGTERM, handler)
    old_int = signal.signal(signal.SIGINT, handler)
    try:
        _, stderr = proc.communicate()
        tail_lines = (stderr or b"").decode("utf-8", errors="replace").splitlines()
        return proc.returncode, "\n".join(tail_lines[-5:])
    finally:
        signal.signal(signal.SIGTERM, old_term)
        signal.signal(signal.SIGINT, old_int)


def sweep(downloads_root: Path, skip_patterns: list[str],
          dry_run: bool, force: bool) -> dict:
    stats = {"done": 0, "skip": 0, "fail": 0}
    for entry in sorted(downloads_root.iterdir()):
        try:
            release = entry.resolve(strict=True)
        except (OSError, RuntimeError):
            continue
        if not release.is_relative_to(downloads_root):
            log("reject", f"release outside root {sanitize(release)}")
            stats["skip"] += 1
            continue
        if not release.is_dir():
            continue

        if any(fnmatch.fnmatch(entry.name, pat) for pat in skip_patterns):
            log("skip", f"{sanitize(entry.name)} (--skip)")
            stats["skip"] += 1
            continue

        if not force and already_sheeted(release):
            log("skip", f"{sanitize(entry.name)} (already sheeted)")
            stats["skip"] += 1
            continue

        videos = find_videos(release, downloads_root)
        if not videos:
            stats["skip"] += 1
            continue

        # Dry-run doesn't write sheets, so disk gating is irrelevant.
        if not dry_run:
            disk = shutil.disk_usage(downloads_root)
            if disk.free / disk.total < DISK_FREE_FLOOR:
                log("warn", f"{sanitize(entry.name)} (disk <10% free)")
                stats["skip"] += 1
                continue

        out_dir = release / "contact-sheets"
        try:
            out_dir.mkdir(parents=True, exist_ok=True)
        except OSError as e:
            log("fail", f"{sanitize(entry.name)} mkdir: {sanitize(e)}")
            stats["fail"] += 1
            continue

        title = slugify(entry.name)
        for video in videos:
            if dry_run:
                log("dry-run", f"{sanitize(entry.name)} -> {sanitize(video.name)}")
                stats["done"] += 1
                continue
            log("start", f"{sanitize(entry.name)} {sanitize(video.name)}")
            t0 = time.time()
            rc, err_tail = run_contact_sheet(video, out_dir, title)
            dt = time.time() - t0
            if rc == 0:
                log("done", f"{sanitize(entry.name)} {sanitize(video.name)} {dt:.0f}s")
                stats["done"] += 1
            else:
                log("fail",
                    f"{sanitize(entry.name)} rc={rc} tail={sanitize(err_tail)}")
                stats["fail"] += 1
    return stats


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Opportunistic contact sheet sweeper for pirata.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    ap.add_argument("--downloads", type=Path, default=None,
                    help="Downloads root (default: pirata config aria2.download_dir)")
    ap.add_argument("--skip", action="append", default=[],
                    help="Glob against release dir name to skip (repeatable)")
    ap.add_argument("--dry-run", action="store_true",
                    help="Log what would run; don't invoke contact_sheet.py")
    ap.add_argument("--force", action="store_true",
                    help="Regenerate sheets for already-sheeted releases")
    args = ap.parse_args()

    downloads_root = args.downloads.resolve() if args.downloads else read_downloads_root()
    if not downloads_root:
        print("ERROR: no downloads root. Set aria2.download_dir in "
              "~/.config/pirata/config.toml or pass --downloads", file=sys.stderr)
        return 1
    if not downloads_root.is_dir():
        print(f"ERROR: downloads root not a directory: {downloads_root}",
              file=sys.stderr)
        return 1
    if not CONTACT_SHEET.is_file():
        print(f"ERROR: contact_sheet.py missing at {CONTACT_SHEET}",
              file=sys.stderr)
        return 1

    LOCK_FILE.parent.mkdir(parents=True, exist_ok=True)
    lock_fd = open(LOCK_FILE, "a")
    try:
        fcntl.flock(lock_fd.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        log("skip", "another sweep already active")
        print("another sweep is already running; exit 0", file=sys.stderr)
        lock_fd.close()
        return 0

    try:
        log("start",
            f"downloads={sanitize(downloads_root)} dry_run={args.dry_run} "
            f"force={args.force}")
        t0 = time.time()
        stats = sweep(downloads_root, args.skip, args.dry_run, args.force)
        dt = time.time() - t0
        summary = (f"done={stats['done']} skip={stats['skip']} "
                   f"fail={stats['fail']} duration={dt:.0f}s")
        log("finish", summary)
        print(summary, file=sys.stderr)
        return 0 if stats["fail"] == 0 else 4
    finally:
        try:
            fcntl.flock(lock_fd.fileno(), fcntl.LOCK_UN)
        finally:
            lock_fd.close()


if __name__ == "__main__":
    sys.exit(main())
