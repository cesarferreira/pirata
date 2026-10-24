---
name: pirata
description: Use this skill when working on the pirata Rust CLI. It covers the command surface, repo structure, validation workflow, and the downloader behavior around librqbit, transmission-cli, transmission-remote, transmission-daemon, system magnet opening, doctor, setup, and the ratatui TUI.
---

# pirata

Use this skill when the task is about changing, debugging, or extending this repository.

## What This Repo Does

`pirata` is a Rust CLI for:

- torrent search
- torrent info and magnet extraction
- add-by-id and lucky selection flows
- a full-screen TUI for search, category browse, details, and downloads
- `doctor` and `setup` commands for local environment/config checks
- first-run interactive setup when the config file is missing

## Important Behavior

- `librqbit` is the default downloader. It is built in; there is no `aria2c` binary.
- Existing configs and flags that say `aria2` still select librqbit, and an `[aria2]` table is read as the librqbit download directory.
- Normal CLI download flows (`add`, `lucky`, `search --interactive`) use librqbit unless the user explicitly selects another downloader.
- The TUI always downloads with librqbit, even when the configured default is transmission or system.
- `system` means “hand the magnet to the OS”, which may open the Transmission GUI app.
- `transmission` means:
  1. use the configured Transmission client mode (`cli`, `rpc`, or `auto`)
  2. `auto` can fall back across CLI and RPC paths
- Missing `transmission-cli` should produce an install hint instead of a generic spawn failure.
- Missing config should trigger the setup wizard for normal interactive commands.

## Files To Check First

- `src/cli.rs`: command definitions and flags
- `src/app.rs`: command routing and top-level behavior
- `src/tui/`: ratatui search, category, detail, and download UI
- `src/downloader/librqbit.rs`: default librqbit download flow
- `src/downloader/transmission.rs`: RPC and CLI fallback logic
- `src/config.rs`: defaults and config load/save
- `src/util.rs`: shared helpers, command detection, install hints
- `README.md`: usage-only user documentation

## Working Rules For This Repo

- Prefer small, behavior-preserving edits.
- Keep README focused on usage, not architecture or implementation notes.
- When changing CLI behavior, update README examples if the user-visible flow changed.
- If changing downloader defaults or fallback behavior, think through whether magnets will go to the GUI app or stay in CLI flow.
- The TUI downloads with librqbit. Do not send TUI downloads through transmission or the system handler.
- Keep Debian/Ubuntu compatibility in mind for install hints and executable names.

## Validation

Run these after code changes unless the task clearly does not need them:

```bash
cargo test
cargo run -- doctor
```

Useful targeted checks:

```bash
cargo run -- tui ubuntu
cargo run -- search ubuntu --interactive
cargo run -- lucky ubuntu --dry-run
cargo run -- lucky ubuntu
```

If testing a missing dependency path, verify the user-facing error message and not just the exit code.

## Notes For Agents

- If a user says the app opened the Transmission UI, inspect whether the `system` downloader path was used.
- If a user asks for setup or install guidance, prefer `doctor` and `setup` over ad hoc explanations.
- If working on the TUI, preserve terminal cleanup and avoid leaving the shell in raw mode.
