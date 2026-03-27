# pirate-ctl

Usage-focused torrent search and download CLI.

## Build

```bash
cargo build
cargo run -- --help
```

## Setup

If no config exists, `pirate-ctl` starts a first-run setup wizard before running your command.

Run setup explicitly:

```bash
cargo run -- setup
```

The wizard lets you choose:

- default downloader mode
- Transmission client mode: `cli`, `rpc`, or `auto`
- download directory
- RPC URL and credentials when using RPC mode

Start setup and prefill the download directory prompt:

```bash
cargo run -- setup --download-dir ~/Downloads/torrents
```

Check local dependencies and active config source:

```bash
cargo run -- doctor
cargo run -- --json doctor
```

## Search

```bash
cargo run -- search "ubuntu 24.04"
cargo run -- search "ubuntu 24.04" --sort seeders
cargo run -- search "ubuntu 24.04" --interactive
cargo run -- search "ubuntu 24.04" --json
```

With Transmission client mode set to `cli`, `search --interactive`, `add`, and `lucky` stream `transmission-cli` output in the terminal and wait for the download to finish.

## Info and Magnet

```bash
cargo run -- info 81462446
cargo run -- magnet 81462446
cargo run -- magnet 81462446 --json
```

## Add

Add by torrent id:

```bash
cargo run -- add 81462446
```

Force OS magnet handler:

```bash
cargo run -- add 81462446 --downloader system
cargo run -- add 81462446 --open
```

## Lucky

```bash
cargo run -- lucky "ubuntu server 24.04"
cargo run -- lucky "ubuntu server 24.04" --dry-run
cargo run -- lucky "ubuntu server 24.04" --min-seeders 5 --trusted-only
cargo run -- lucky "ubuntu server 24.04" --min-size 1GB --max-size 5GB
```

## TUI

Full-screen picker plus foreground `transmission-cli` progress UI:

```bash
cargo run -- tui
cargo run -- tui ubuntu
cargo run -- tui "ubuntu 24.04" --sort seeders
```

Downloads started inside the TUI have live progress in the dashboard.

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
cargo run -- setup
```

## Useful Commands

```bash
cargo fmt
RUSTC_WRAPPER= cargo check
RUSTC_WRAPPER= cargo test
RUSTC_WRAPPER= cargo run -- doctor
RUSTC_WRAPPER= cargo run -- tui ubuntu
```
