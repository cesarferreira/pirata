# pirate-ctl

![pirate-ctl screenshot](assets/screenshot.png)

A torrent CLI built to get from query to download quickly.

## Fast Paths

Use the command that matches how much control you want:

- `pirate-ctl lucky "ubuntu 24.04"` picks the best match and starts downloading immediately
- `pirate-ctl add 81462446` downloads a known torrent id directly
- `pirate-ctl search "ubuntu 24.04" --interactive` lets you search, pick, and download in one flow
- `pirate-ctl tui ubuntu` opens the full-screen picker with live download progress

By default, `search --interactive`, `add`, and `lucky` run `aria2c` in the foreground and wait for the download to finish.

If you switch to the `transmission` downloader, those commands use the configured Transmission client instead.

## Quick Download Examples

Set the download directory once, then use the fast commands:

```bash
pirate-ctl setup --download-dir ~/media
pirate-ctl lucky "ubuntu 24.04"
```

A few common variations:

```bash
pirate-ctl lucky "ubuntu 24.04" --min-seeders 20 --trusted-only
pirate-ctl lucky "ubuntu" --dry-run
pirate-ctl search "ubuntu 24.04" --interactive
pirate-ctl --downloader transmission lucky "ubuntu 24.04"
pirate-ctl add 81462446
```

The download target comes from your configured downloader settings. Use `pirate-ctl setup --download-dir ...` to change it.

## Build

```bash
cargo install --path .
pirate-ctl --help
```

## Setup

If no config exists, `pirate-ctl` starts a first-run setup wizard before running your command.

Run setup explicitly:

```bash
pirate-ctl setup
```

The wizard lets you choose:

- default downloader mode
- Aria2 download directory
- Transmission client mode: `cli`, `rpc`, or `auto` when you pick Transmission
- Transmission download directory when you pick Transmission
- RPC URL and credentials when using RPC mode

Start setup and prefill the download directory prompt:

```bash
pirate-ctl setup --download-dir ~/Downloads/torrents
```

Check local dependencies and active config source:

```bash
pirate-ctl doctor
pirate-ctl --json doctor
```

## Search

```bash
pirate-ctl search "ubuntu 24.04"
pirate-ctl search "ubuntu 24.04" --sort seeders
pirate-ctl search "ubuntu 24.04" --interactive
pirate-ctl search "ubuntu 24.04" --json
```

## Info and Magnet

```bash
pirate-ctl info 81462446
pirate-ctl magnet 81462446
pirate-ctl magnet 81462446 --json
```

## Add

Add by torrent id:

```bash
pirate-ctl add 81462446
```

Force OS magnet handler:

```bash
pirate-ctl add 81462446 --downloader system
pirate-ctl add 81462446 --open
```

## Lucky

```bash
pirate-ctl lucky "ubuntu server 24.04"
pirate-ctl lucky "ubuntu server 24.04" --dry-run
pirate-ctl lucky "ubuntu server 24.04" --min-seeders 5 --trusted-only
pirate-ctl lucky "ubuntu server 24.04" --min-size 1GB --max-size 5GB
```

## TUI

Full-screen picker plus foreground `transmission-cli` progress UI:

```bash
pirate-ctl tui
pirate-ctl tui ubuntu
pirate-ctl tui "ubuntu 24.04" --sort seeders
```

Downloads started inside the TUI have live progress in the dashboard.

The TUI is currently Transmission-specific and requires `transmission-cli`, even if your default downloader is `aria2`.

Keys:

- `Tab`: cycle focus between query, results, and downloads
- `Up` / `Down` or `j` / `k`: move
- `Enter`: search or start selected download
- `/`: start a fresh search
- `d`: abort selected download
- `q` or `Esc`: stop active foreground downloads and quit
- `Q`: stop active foreground downloads and quit

## Global Flags

```text
--json
--indexer piratebay
--downloader transmission|qbittorrent|aria2|system
--config ~/.config/pirate-ctl/config.toml
--open
```

## Config Path

Default config path:

```text
~/.config/pirate-ctl/config.toml
```

Write it with:

```bash
pirate-ctl setup
```

## Useful Commands

```bash
cargo fmt
RUSTC_WRAPPER= cargo check
RUSTC_WRAPPER= cargo test
pirate-ctl doctor
pirate-ctl lucky ubuntu
pirate-ctl tui ubuntu
```
