<div align="center">
  <h1>pirata</h1>

  <p><strong>A torrent CLI with a TUI-first search and download flow.</strong></p>

  <p>
    <a href="https://crates.io/crates/pirata"><img alt="Crates.io" src="https://img.shields.io/crates/v/pirata"></a>
    <img alt="License" src="https://img.shields.io/badge/license-MIT-green">
  </p>

  <p>
    <a href="#build">Build</a>
    &nbsp;·&nbsp;
    <a href="#setup">Setup</a>
    &nbsp;·&nbsp;
    <a href="#tui">TUI</a>
    &nbsp;·&nbsp;
    <a href="#fast-paths">Fast paths</a>
  </p>
</div>

---

## Fast Paths

- `pirata` launches the full-screen TUI
- `pirata tui ubuntu` opens the TUI with a prefilled query
- `pirata lucky "ubuntu 24.04"` picks the best match and starts downloading
- `pirata add 81462446` downloads a known torrent id directly
- `pirata resume` resumes saved built-in downloads and retries failed completion hooks
- `pirata search "ubuntu 24.04" --interactive` keeps the one-shot interactive picker

Short aliases are available for the common commands:

- `pirata s ...`
- `pirata i ...`
- `pirata m ...`
- `pirata a ...`
- `pirata l ...`
- `pirata t ...`

By default, `search --interactive`, `add`, and `lucky` download with the built-in [librqbit](https://crates.io/crates/librqbit) engine and wait for the download to finish. If you switch to the `transmission` downloader, those commands use the configured Transmission client instead. The full-screen TUI always downloads with librqbit.

## Build

Prebuilt Linux binaries are available for x86_64 and ARM64:

```bash
cargo binstall pirata
```

To build from this checkout:

```bash
cargo install --path .
pirata --help
```

## Setup

If no config exists, `pirata` starts a first-run setup wizard before running your command.

Run setup explicitly:

```bash
pirata setup
pirata setup --download-dir ~/Downloads/torrents
```

Check local dependencies and active config source:

```bash
pirata doctor
pirata --json doctor
```

## Download Folders and Resume

The built-in downloader uses `incomplete/` and `completed/` beneath the configured download root. CLI and TUI downloads share the same lifecycle. A torrent reaches `completed/` only after its data is verified; Pirata then registers the completed location with the engine so it can continue seeding while Pirata runs.

Interrupted downloads resume when you reopen the TUI, repeat the download command, or run:

```bash
pirata resume
```

`Ctrl-C` pauses CLI work and `q` closes the TUI. Resume metadata is kept in `.pirata/` under the download root; preserve that folder alongside your downloads. Resuming verifies the existing pieces instead of starting over. Only one Pirata instance can manage a given root at a time; parallel downloads inside the TUI and `resume` remain supported.

Existing `[librqbit].download_dir` and legacy `[aria2].download_dir` settings now identify the **root** containing these folders. Existing loose files are left in place; they are not automatically adopted or moved. Changing `incomplete_dir` or `completed_dir` affects new downloads; saved downloads retain their original locations. Changing the root selects a different `.pirata/` state directory.

### Example: `~/media` with Plex

```text
~/media/
├── .pirata/       # Saved torrent metadata and resume records
├── incomplete/    # Active or interrupted downloads
├── completed/     # Verified downloads, retained for seeding
├── movies/        # Plex movie library
└── tv/            # Plex TV library
```

Movie/TV organization is optional. The included completion hook requires Python 3.9+ and hardlinks verified media into Plex-style folders without copying the file data. Install it from this checkout:

```bash
mkdir -p ~/.local/bin
install -m 755 examples/import-media.py ~/.local/bin/pirata-import-media
```

Example `~/.config/pirata/config.toml`:

```toml
[defaults]
indexer = "piratebay"
downloader = "librqbit"
search_limit = 20

[librqbit]
download_dir = "~/media"

[downloads]
incomplete_dir = "~/media/incomplete"
completed_dir = "~/media/completed"

# Keep the larger of this byte reserve and the percentage below free.
min_free_space_bytes = 1073741824 # 1 GiB
min_free_space_percent = 15

on_complete = [
  "~/.local/bin/pirata-import-media",
  "--movies-dir", "~/media/movies",
  "--tv-dir", "~/media/tv",
]
hook_timeout_seconds = 60
```

Configure Plex's **Movies** library to watch `~/media/movies` and **TV Shows** to watch `~/media/tv`. Enable automatic library scans. Keep `incomplete/` and `completed/` outside those library sources; do not point Plex at all of `~/media`.

The example hook recognizes TV filenames such as `Louie.S01E01.mp4`, groups episodes into show/season folders, and reuses an existing show folder such as `Louie (2010)`. Other video downloads go into movie folders. Non-media downloads are left alone. The helper preserves movie subtitle paths and episode sidecars with season/episode names. It does not perform online title matching or rename ambiguous episode files.

The incomplete, completed, and media library folders must be on the same filesystem for hardlinks. Hardlinks preserve seeding and share the underlying file data. To reclaim space, remove both the completed download and its library links; removing only one link keeps the data allocated. Pirata does not automatically delete media to free space.

### Completion Hooks and Space Limits

`on_complete` is an executable and argument list, with no implicit shell expansion. `~` is supported in folder settings and the executable path. The example helper expands `~` in its own folder arguments. For other hooks, use absolute argument paths unless the program supports expansion itself. Omit `on_complete` or set it to `[]` to use Pirata without media organization or Plex.

Hooks receive:

| Variable | Value |
| --- | --- |
| `PIRATA_DOWNLOAD_DIR` | Absolute path to the completed torrent directory |
| `PIRATA_DOWNLOAD_NAME` | Torrent name |
| `PIRATA_INFO_HASH` | Torrent info hash |
| `PIRATA_TORRENT_ID` | Search result ID, or hash for direct magnets |

Transmission-compatible `TR_TORRENT_DIR`, `TR_TORRENT_NAME`, and `TR_TORRENT_HASH` are also provided. Hook output is suppressed to keep the TUI intact; hook scripts should write their own logs when needed. A failed or timed-out hook leaves the download completed and records the notification for retry with `pirata resume`. Hooks should be idempotent: a process interruption after a hook succeeds but before its result is saved can cause it to run again.

The default space reserve is the larger of 1 GiB and 10% of the filesystem. Pirata checks room for concurrent downloads before starting their data transfer and pauses active downloads if free space drops below the reserve. Free some space, then retry the download or use `pirata resume`. Set both reserve values to `0` to disable the reserve; the actual download still needs enough free space to fit.

These lifecycle settings apply to the built-in downloader. Transmission and system-handler downloads continue to use their external client's folder, resume, and hook settings.

## TUI

Full-screen search, category browse, torrent details, and live librqbit downloads:

```bash
pirata
pirata tui
pirata tui ubuntu
pirata tui "ubuntu 24.04" --sort seeders
```

Set the download folder with `pirata setup --download-dir ~/Downloads/torrents`.

The TUI opens on Search. It shows a search bar, a Search / Trending switch, category tabs, a results table, a detail popup, and per-torrent progress bars. Press `t` for Trending, the top releases from the last 48 hours. Categories stay on their own row and only filter search. It lists up to 100 results unless you pass `--limit`. Downloads run in-process with librqbit and can overlap.

Keys:

| Key | Action |
| --- | --- |
| `/` or `i` | Focus the search box |
| `Enter` | Run search / open the detail popup |
| `Esc` | Leave the search box / close a popup |
| `↑` `↓` or `j` `k` | Move the selection |
| `PgUp` `PgDn` | Jump by a page |
| `g` `G` | Jump to top / bottom |
| `t` | Switch between Search and Trending |
| `←` `→` or `Tab` | Previous / next category |
| `d` | Download the selected torrent |
| `c` or `y` | Copy the magnet link to the clipboard |
| `r` | Refresh the current view |
| `?` | Toggle the help popup |
| `q` | Quit |

## Search

```bash
pirata search "ubuntu 24.04"
pirata search "ubuntu 24.04" --sort seeders
pirata search "ubuntu 24.04" --interactive
pirata search "ubuntu 24.04" --json
```

## Info and Magnet

```bash
pirata info 81462446
pirata magnet 81462446
pirata magnet 81462446 --json
```

## Add

```bash
pirata add 81462446
pirata add 81462446 --downloader system
pirata add 81462446 --open
```

## Lucky

```bash
pirata lucky "ubuntu server 24.04"
pirata lucky "ubuntu server 24.04" --dry-run
pirata lucky "ubuntu server 24.04" --min-seeders 5 --trusted-only
pirata lucky "ubuntu server 24.04" --min-size 1GB --max-size 5GB
```

## Global Flags

```text
--json
--indexer piratebay
--downloader transmission|qbittorrent|librqbit|system

`aria2` is still accepted as an alias for `librqbit`, including in existing config files.
--config ~/.config/pirata/config.toml
--open
```

## Config Path

Default config path:

```text
~/.config/pirata/config.toml
```

Legacy `~/.config/pirate-ctl/config.toml` is still read as a fallback.

## Useful Commands

```bash
cargo fmt
RUSTC_WRAPPER= cargo check
RUSTC_WRAPPER= cargo test
pirata doctor
pirata lucky ubuntu
pirata tui ubuntu
```
