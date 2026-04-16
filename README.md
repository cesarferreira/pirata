# pirata

`pirata` is a fast, scriptable Rust CLI for searching torrents, inspecting metadata, extracting magnet links, and sending them directly to a local downloader.

The current implementation ships with:

- Pirate Bay search and info lookup
- Magnet extraction with fallback magnet construction from the info hash
- Transmission support through RPC, with `transmission-remote` as a fallback
- System magnet opening via the OS default handler
- Interactive TUI-style search and selection
- JSON output for automation and shell pipelines
- Short-lived search result caching

## Why this exists

The goal is to make the common terminal workflow short:

1. Search for something
2. Inspect or pick a result
3. Send it to a downloader without copy-pasting magnet links around

`pirata` is intended to stay thin. It is not a torrent manager or a daemon. It is an orchestration layer between indexers and downloaders.

## Features

### TUI

```bash
pirata
pirata tui
pirata tui "ubuntu 24.04"
```

- `pirata` launches the interactive search-and-add flow by default
- `pirata tui` makes the same mode explicit
- The interactive mode shows tracked downloads from Transmission plus Pirata's local completed history
- Completed history entries are hidden automatically if their file or folder is gone
- The interactive mode prompts for a query if you do not pass one
- `--json` is only available for non-interactive subcommands

### Search

```bash
pirata search "ubuntu 24.04"
pirata s "ubuntu 24.04"
pirata search "ubuntu 24.04" --sort seeders
pirata search "ubuntu 24.04" --interactive
pirata search "ubuntu 24.04" --json
```

- Prints a table with `id`, `name`, `seeders`, `leechers`, `size`, and `status`
- Supports fast arrow-key selection and immediate add
- Supports JSON output for scripts

### Info and magnet

```bash
pirata info 81462446
pirata i 81462446
pirata magnet 81462446
pirata m 81462446 --json
```

- `info` prints detailed torrent metadata plus the resolved magnet
- `magnet` prints only the magnet link by default, which makes it shell-friendly

### Add

```bash
pirata add 81462446
pirata a 81462446
pirata add 81462446 --open
pirata add 81462446 --downloader system
```

- Fetches torrent info
- Resolves the magnet from the source when available
- Falls back to `magnet:?xt=urn:btih:...&dn=...` when necessary
- Sends the magnet to Transmission or to the system magnet handler

### Lucky mode

```bash
pirata lucky "ubuntu server 24.04"
pirata l "ubuntu server 24.04"
pirata lucky "ubuntu server 24.04" --dry-run
pirata lucky "ubuntu server 24.04" --min-seeders 5 --trusted-only
pirata lucky "ubuntu server 24.04" --min-size 1GB --max-size 5GB
```

Lucky mode searches, scores, filters, selects the best candidate, and optionally adds it.

Scoring uses:

```text
score = sqrt(seeders) * 10
      + 30 for vip
      + 15 for trusted
      - 0.5 * leechers
```

## Global flags

These flags work across commands:

```text
--json
--indexer piratebay
--downloader transmission|qbittorrent|aria2|system
--config ~/.config/pirata/config.toml
--open
```

Notes:

- `qbittorrent` and `aria2` are accepted as CLI values for forward compatibility, but are not implemented yet
- `--open` overrides the downloader selection and uses the OS default magnet handler

## Installation

### Build locally

```bash
cargo build --release
./target/release/pirata --help
```

### Install into Cargo's bin directory

```bash
cargo install --path .
```

## Configuration

By default, `pirata` reads:

```text
~/.config/pirata/config.toml
```

Legacy fallback:

```text
~/.config/pirate-ctl/config.toml
```

Example:

```toml
[defaults]
indexer = "piratebay"
downloader = "transmission"
search_limit = 20

[transmission]
rpc_url = "http://localhost:9091/transmission/rpc"
username = ""
password = ""
download_dir = ""

[cache]
ttl_minutes = 5
```

You can override the config path with `--config`.

## Transmission behavior

Transmission support works in two stages:

1. Try the RPC endpoint configured by `transmission.rpc_url`
2. Fall back to `transmission-remote` if RPC fails

This keeps the default path clean while still working on machines where the CLI is available but RPC is not configured exactly as expected.

## Output modes

Human-readable output is the default:

- Search prints a table
- Info prints labeled fields
- Magnet prints only the magnet URI

For automation, `--json` emits structured JSON for:

- `search`
- `info`
- `magnet`
- `add`
- `lucky`

## Caching

Recent search results are cached for a short period to reduce repeated indexer calls.

- Cache TTL defaults to 5 minutes
- Cache location uses the OS cache directory via the `directories` crate

Pirata also keeps a local download history for items it started itself.

- Completed downloads survive relaunches even if Transmission no longer lists them
- History entries disappear automatically when the downloaded file or folder no longer exists

## Architecture

The code is structured around a small core model and traits:

- `Indexer`
- `Downloader`
- `Torrent`

Current implementations:

- `PirateBayIndexer`
- `TransmissionDownloader`
- `SystemDownloader`

Main modules:

- `src/app.rs`: command orchestration
- `src/indexer/`: search and info lookup
- `src/downloader/`: downloader integrations
- `src/config.rs`: TOML config loading
- `src/cache.rs`: cached search results

## Current implementation notes

This first version uses direct HTTP integrations for Pirate Bay and Transmission instead of separate wrapper crates. That keeps the behavior explicit and easy to debug while the command surface settles.

Live service behavior still depends on:

- Pirate Bay / ApiBay availability
- Transmission RPC availability or local `transmission-remote` presence

## Development

Useful commands:

```bash
cargo fmt
RUSTC_WRAPPER= cargo check
RUSTC_WRAPPER= cargo test
RUSTC_WRAPPER= cargo run -- --help
```

## Roadmap

Likely next steps:

- qBittorrent support
- aria2 support
- additional indexers behind the same trait boundary
- richer result filtering and sorting
- fallback scraper behavior when the JSON API changes
