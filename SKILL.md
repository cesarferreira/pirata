---
name: pirate-ctl
description: Use this skill when working on the pirate-ctl Rust CLI. It covers the command surface, repo structure, validation workflow, and the downloader behavior around aria2c, transmission-cli, transmission-remote, transmission-daemon, system magnet opening, doctor, setup, and the ratatui TUI.
---

# pirate-ctl

Use this skill when the task is about changing, debugging, or extending this repository.

## What This Repo Does

`pirate-ctl` is a Rust CLI for:

- torrent search
- torrent info and magnet extraction
- add-by-id and lucky selection flows
- a `tui` command that runs a full-screen picker and foreground `transmission-cli`
- `doctor` and `setup` commands for local environment/config checks
- first-run interactive setup when the config file is missing

## Important Behavior

- `aria2` is the default downloader.
- Normal CLI download flows (`add`, `lucky`, `search --interactive`) should prefer `aria2c` unless the user explicitly selects another downloader.
- `system` means “hand the magnet to the OS”, which may open the Transmission GUI app.
- `transmission` means:
  1. use the configured Transmission client mode (`cli`, `rpc`, or `auto`)
  2. `auto` can fall back across CLI and RPC paths
- The `tui` command uses `transmission-cli` directly and does not use the default downloader or system magnet handler.
- Missing `aria2c` should produce an install hint instead of a generic spawn failure.
- Missing `transmission-cli` should produce an install hint instead of a generic spawn failure.
- Missing config should trigger the setup wizard for normal interactive commands.

## Files To Check First

- `src/cli.rs`: command definitions and flags
- `src/app.rs`: command routing and top-level behavior
- `src/tui.rs`: ratatui UI and foreground transmission-cli flow
- `src/downloader/aria2.rs`: default aria2 CLI flow
- `src/downloader/transmission.rs`: RPC and CLI fallback logic
- `src/config.rs`: defaults and config load/save
- `src/util.rs`: shared helpers, command detection, install hints
- `README.md`: usage-only user documentation

## Working Rules For This Repo

- Prefer small, behavior-preserving edits.
- Keep README focused on usage, not architecture or implementation notes.
- When changing CLI behavior, update README examples if the user-visible flow changed.
- If changing downloader defaults or fallback behavior, think through whether magnets will go to the GUI app or stay in CLI flow.
- If changing aria2 defaults, keep the TUI caveat clear because the TUI still depends on `transmission-cli`.
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
