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

## TUI

Full-screen search, category browse, torrent details, and live librqbit downloads:

```bash
pirata
pirata tui
pirata tui ubuntu
pirata tui "ubuntu 24.04" --sort seeders
```

Set the download folder with `pirata setup --download-dir ~/Downloads/torrents`.

The TUI shows a search bar, category tabs, a results table, a detail popup, and per-torrent progress bars. It lists up to 100 results unless you pass `--limit`. Downloads run in-process with librqbit and can overlap.

Keys:

| Key | Action |
| --- | --- |
| `/` or `i` | Focus the search box |
| `Enter` | Run search / open the detail popup |
| `Esc` | Leave the search box / close a popup |
| `↑` `↓` or `j` `k` | Move the selection |
| `PgUp` `PgDn` | Jump by a page |
| `g` `G` | Jump to top / bottom |
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
