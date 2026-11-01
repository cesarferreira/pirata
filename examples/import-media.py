#!/usr/bin/env python3
"""Optional completion hook for Plex-style movie and TV folders."""

import argparse
import os
import re
from pathlib import Path

VIDEOS = {".mkv", ".mp4", ".m4v", ".avi", ".mov", ".ts", ".mpg", ".mpeg"}
SIDECARS = {".srt", ".ass", ".ssa", ".sub", ".idx", ".vtt", ".jpg", ".png"}


def episode(filename):
    match = re.search(r"(?i)(?<![a-z0-9])s(\d{1,2})e\d{1,3}", filename)
    if not match:
        return None
    title = re.sub(r"[._]+", " ", filename[:match.start()]).strip(" -[]")
    return (title, int(match.group(1))) if title else None


def component(value):
    result = re.sub(r'[<>:"/\\|?*\x00-\x1f]', "_", value).strip(" .")
    return result or "download"


def show_folder(tv, title):
    direct = tv / component(title)
    if direct.exists() or not tv.exists():
        return direct
    key = lambda name: re.sub(r"\s*\(\d{4}\)$", "", name).casefold()
    matches = [path for path in tv.iterdir() if path.is_dir() and key(path.name) == key(title)]
    if len(matches) > 1:
        raise ValueError(f"Multiple shows match {title}; include a year in the episode filenames")
    return matches[0] if matches else direct


def link(source, target, library):
    if not target.parent.resolve().is_relative_to(library.resolve()):
        raise ValueError(f"Library path escapes its root: {target}")
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.is_symlink():
        raise FileExistsError(f"Library target is a symlink: {target}")
    try:
        os.link(source, target)
    except FileExistsError:
        if not source.samefile(target):
            raise FileExistsError(f"Refusing to replace existing library file: {target}")


def import_media(source, movies, tv, name):
    source, movies, tv = (Path(path).expanduser() for path in (source, movies, tv))
    if not source.is_dir():
        raise ValueError(f"Completed download is not a directory: {source}")
    for library in (movies, tv):
        if source.resolve().is_relative_to(library.resolve()) or library.resolve().is_relative_to(source.resolve()):
            raise ValueError("Completed downloads and media libraries must be separate folders")
    files = [path for path in sorted(source.rglob("*"))
             if path.is_file() and not path.is_symlink()
             and path.resolve().is_relative_to(source.resolve())
             and not any(part.startswith(".") for part in path.relative_to(source).parts)]
    videos = [path for path in files if path.suffix.lower() in VIDEOS]
    if not videos:
        return []
    television = any(episode(path.name) for path in videos)
    movie_name = Path(name).stem if Path(name).suffix.lower() in VIDEOS else name
    imported = []
    for path in files:
        if path.suffix.lower() not in VIDEOS | SIDECARS:
            continue
        if television:
            details = episode(path.name)
            if details is None:
                continue
            title, season = details
            library = tv
            target = show_folder(tv, title) / f"Season {season:02d}" / path.name
        else:
            library = movies
            target = movies / component(movie_name) / path.relative_to(source)
        link(path, target, library)
        imported.append(target)
    return imported


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--movies-dir", type=Path, default=Path("~/media/movies"))
    parser.add_argument("--tv-dir", type=Path, default=Path("~/media/tv"))
    args = parser.parse_args()
    directory = os.environ.get("PIRATA_DOWNLOAD_DIR")
    if not directory:
        parser.error("PIRATA_DOWNLOAD_DIR is missing; configure this script as downloads.on_complete")
    try:
        imported = import_media(directory, args.movies_dir, args.tv_dir,
                                os.environ.get("PIRATA_DOWNLOAD_NAME", Path(directory).name))
    except (OSError, ValueError) as error:
        parser.exit(1, f"Media import failed: {error}\n")
    print(f"Linked {len(imported)} media files; original download retained for seeding.")


if __name__ == "__main__":
    main()
