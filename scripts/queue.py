#!/usr/bin/env python3
"""Queue magnet URIs to aria2c.

Usage:
  python scripts/queue.py "magnet:?xt=..." "magnet:?xt=..."
  python scripts/queue.py -f magnets.txt
  echo "magnet:?xt=..." | python scripts/queue.py -

Notes:
  Magnet URIs contain '&' — always quote them in the shell.
  Default: fire-and-forget (aria2c detached). Use --wait to block.
  Download dir defaults to pirata's configured aria2.download_dir.
"""
from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

MAGNET_RE = re.compile(
    r"magnet:\?.*xt=urn:btih:(?:[A-Fa-f0-9]{40}|[A-Za-z2-7]{32})"
)
PIRATA_CONFIG = Path.home() / ".config" / "pirata" / "config.toml"
VIDEO_EXTS = {".mkv", ".mp4", ".avi", ".mov", ".ts", ".m2ts", ".webm"}


def snapshot_loose_videos(root: Path) -> set[Path]:
    """Top-level files in root with video extensions."""
    if not root.is_dir():
        return set()
    return {p for p in root.iterdir()
            if p.is_file() and p.suffix.lower() in VIDEO_EXTS}


def wrap_loose_videos(new_videos: set[Path]) -> list[Path]:
    """Move each loose video into <stem>/<name>. Skip on collision."""
    wrapped: list[Path] = []
    for video in sorted(new_videos):
        target_dir = video.parent / video.stem
        if target_dir.exists():
            print(f"warn: cannot wrap {video.name}; {target_dir.name}/ exists",
                  file=sys.stderr)
            continue
        target_dir.mkdir()
        new_path = target_dir / video.name
        video.rename(new_path)
        print(f"wrapped: {video.name} -> {target_dir.name}/")
        wrapped.append(new_path)
    return wrapped


def read_default_dir() -> str:
    if PIRATA_CONFIG.exists():
        try:
            cfg = tomllib.loads(PIRATA_CONFIG.read_text())
            d = cfg.get("aria2", {}).get("download_dir")
            if d:
                return d
        except tomllib.TOMLDecodeError:
            pass
    return str(Path.cwd() / "downloads")


def collect(args: argparse.Namespace) -> list[str]:
    raw: list[str] = list(args.magnets)
    if "-" in raw:
        raw = [m for m in raw if m != "-"]
        raw.extend(l for l in sys.stdin.read().splitlines())
    if args.file:
        raw.extend(Path(args.file).read_text().splitlines())
    return [l.strip() for l in raw if l.strip() and not l.lstrip().startswith("#")]


def validate(magnets: list[str]) -> tuple[list[str], list[str]]:
    good, bad = [], []
    for m in magnets:
        (good if MAGNET_RE.search(m) else bad).append(m)
    return good, bad


def main() -> int:
    p = argparse.ArgumentParser(
        description="Queue magnet URIs to aria2c",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    p.add_argument("magnets", nargs="*", help="Magnet URIs (use '-' for stdin)")
    p.add_argument("-f", "--file", help="Read magnets from file (# comments ok)")
    p.add_argument("--dir", default=None, help="Download dir (default: pirata config)")
    p.add_argument("--max-concurrent", type=int, default=3, help="Parallel downloads (default 3)")
    p.add_argument("--wait", action="store_true", help="Block until downloads finish")
    p.add_argument("--seed", action="store_true", help="Keep seeding after completion (default: stop)")
    p.add_argument("--autosheets", action=argparse.BooleanOptionalAction, default=True,
                   help="After --wait completes, sweep downloads/ for new releases and generate "
                        "contact sheets. Ignored when --wait is not set. Default: on.")
    p.add_argument("--ignore-disk-floor", action="store_true",
                   help="Propagate --ignore-disk-floor to the autosheets sweep "
                        "(overrides the 10%% free-disk safety gate).")
    args = p.parse_args()

    magnets = collect(args)
    if not magnets:
        print("no magnets provided (args, -f FILE, or stdin via '-')", file=sys.stderr)
        return 1

    good, bad = validate(magnets)
    for b in bad:
        print(f"skip (invalid): {b[:80]}", file=sys.stderr)
    if not good:
        print("no valid magnets", file=sys.stderr)
        return 2

    if not shutil.which("aria2c"):
        print("aria2c not found in PATH — install with 'brew install aria2'", file=sys.stderr)
        return 3

    download_dir = args.dir or read_default_dir()
    Path(download_dir).mkdir(parents=True, exist_ok=True)

    fd, tmp_path = tempfile.mkstemp(prefix="queue-", suffix=".txt")
    os.close(fd)
    Path(tmp_path).write_text("\n".join(good) + "\n")

    log = Path(download_dir) / ".aria2.log"
    cmd = [
        "aria2c",
        "-i", tmp_path,
        "--dir", download_dir,
        "--max-concurrent-downloads", str(args.max_concurrent),
        "--log", str(log),
        "--log-level=notice",
        "--summary-interval=15",
        "--auto-file-renaming=true",
        "--continue=true",
    ]
    if not args.seed:
        cmd.append("--seed-time=0")

    if args.wait:
        download_path = Path(download_dir)
        before_loose = snapshot_loose_videos(download_path)
        rc = subprocess.run(cmd).returncode
        os.unlink(tmp_path)
        if rc == 0:
            after_loose = snapshot_loose_videos(download_path)
            wrap_loose_videos(after_loose - before_loose)
        if rc == 0 and args.autosheets:
            sweep = Path(__file__).parent / "sheets_sweep.py"
            if sweep.is_file():
                print("\nrunning sheets_sweep.py ...")
                sweep_cmd = [sys.executable, str(sweep)]
                if args.ignore_disk_floor:
                    sweep_cmd.append("--ignore-disk-floor")
                subprocess.run(sweep_cmd)
            else:
                print(f"warning: sweep script missing at {sweep}; skipping autosheets",
                      file=sys.stderr)
        return rc

    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    print(f"queued {len(good)} magnet(s) -> {download_dir}")
    print(f"pid:  {proc.pid}")
    print(f"log:  {log}")
    print(f"list: {tmp_path}  (kept for debugging; safe to delete)")
    if bad:
        print(f"skipped {len(bad)} invalid magnet(s)")
    print("\ntail live:  tail -f " + str(log))
    print(f"stop all:   kill {proc.pid}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
