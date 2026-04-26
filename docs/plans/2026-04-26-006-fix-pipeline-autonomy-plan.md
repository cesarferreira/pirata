---
title: "fix: pipeline autonomy — wrap loose downloads + override sweep disk floor"
type: fix
status: active
date: 2026-04-26
---

# fix: pipeline autonomy — wrap loose downloads + override sweep disk floor

## Overview

Two friction points surfaced during the first end-to-end `/pirata` autorun (Mario Galaxy, 2026-04-26) where the user explicitly requested fully autonomous queue → sheets → kh-export:

1. `aria2c` landed the `.mkv` as a loose top-level file in `downloads/` because the magnet had a single-file payload. `sheets_sweep.py` walks subdirs only, so the file was invisible — sweep finished `done=0 skip=2` and the kh-export rebuild had no Mario Galaxy entry.
2. `sheets_sweep.py` has a hardcoded `DISK_FREE_FLOOR = 0.10` (10%) with no flag override. Workspace was at ~5% free, which blocked sweep indefinitely even though contact-sheet output is ~60MB total — disk-OOM risk is negligible.

## Requirements

- **R1** — `queue.py --wait` snapshots top-level video files in `download_dir` before invoking aria2c. After rc==0, any newly-arrived loose video file is wrapped into `<file_stem>/<file_name>/` before autosheets fires. Idempotent: pre-existing loose files (the snapshot diff) are not touched. Collision-safe: if `<file_stem>/` already exists, log a warn and skip.
- **R2** — `sheets_sweep.py` accepts `--ignore-disk-floor`. When set, the sweep proceeds even if free disk < 10%. The start-log line records `ignore_disk_floor=True` so the override is auditable.
- **R3** — `queue.py` accepts its own `--ignore-disk-floor` and propagates it to the autosheets sweep invocation.
- **R4** — `scripts/tests/test_sweep.sh` extended with a case asserting `--ignore-disk-floor` reaches the start log line.
- **R5** — New `scripts/tests/test_queue_wrap.sh` covers snapshot + wrap helpers in a hermetic tmpdir without invoking aria2c.

## Scope Boundaries

- NOT changing aria2c flags or seeding behavior.
- NOT modifying `contact_sheet.py` or `build_kh_export.py`.
- NOT adding dependencies (stdlib only).
- NOT renaming files inside subdirs aria2c creates (multi-file torrents stay untouched).
