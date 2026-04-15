# Pirata CLI Redesign

## Goal

Rename the project from `pirate-ctl` to `pirata`, make `pirata` launch the interactive terminal experience by default, and simplify the non-interactive command syntax so common actions are shorter and easier to remember.

## Scope

This design covers:

- Project rename in local source, package metadata, docs, config paths, and user-facing strings
- CLI redesign for a TUI-first default entrypoint
- Backward-compatible short aliases for the current subcommand flows
- Local git remote update after the GitHub repository is renamed

This design does not cover:

- Implementing a new full-screen TUI framework
- Adding new downloader or indexer integrations
- Supporting both `pirate-ctl` and `pirata` as first-class binary names long-term

## Current State

The current binary is named `pirate-ctl` and requires an explicit subcommand for every action. The only interactive flow today is `search --interactive`, implemented with `dialoguer::FuzzySelect`. There is no standalone TUI mode.

That means "launch the TUI" should be defined as launching an interactive search-and-select flow built on the existing terminal UI primitives, not as introducing a new `ratatui` or `crossterm` application in the same change.

## Design Decisions

### 1. Product model

`pirata` becomes a TUI-first CLI:

- Running `pirata` with no subcommand launches the interactive search flow
- Scriptable and automation-friendly subcommands remain available
- An explicit `pirata tui` command also launches the same interactive flow

This keeps the default experience optimized for humans while preserving the current command-oriented behavior for shell use.

### 2. Command model

The command set keeps the existing verbs and adds short aliases:

- `pirata` -> launch TUI
- `pirata tui` -> launch TUI
- `pirata search <query>` and `pirata s <query>`
- `pirata info <id>` and `pirata i <id>`
- `pirata magnet <id>` and `pirata m <id>`
- `pirata add <id>` and `pirata a <id>`
- `pirata lucky <query>` and `pirata l <query>`

The long forms remain the canonical help text. The short forms exist for recall and speed.

### 3. TUI definition for this iteration

The default TUI is the existing interactive search selection flow, promoted to a top-level mode:

- Prompt for a search query when `pirata` or `pirata tui` is invoked without a query
- Fetch search results using the configured/default indexer
- Present results in the fuzzy selector
- Add the selected torrent through the configured/default downloader

If the user already supplies a query to `pirata tui`, the query prompt is skipped and the search goes straight to result selection.

This gives the user a meaningful interactive mode now without inventing a second UI architecture.

### 4. Configuration and filesystem naming

The default config path and app cache directory move from `pirate-ctl` to `pirata`.

To avoid breaking existing users, config loading should support this order:

1. Explicit `--config`
2. New default path: `~/.config/pirata/config.toml`
3. Legacy fallback path: `~/.config/pirate-ctl/config.toml`

Cache data should use the new `pirata` project directory only. Cache compatibility is not important enough to justify dual-write or migration logic.

### 5. User-facing naming

All user-facing strings should say `pirata`, including:

- Cargo package name
- Clap command name and help text
- README examples
- HTTP user-agent strings
- Config path references

Internal type names like `PirateBayIndexer` do not need renaming because they describe the upstream service, not the project.

### 6. GitHub repository rename

The intended repository name becomes `cesarferreira/pirata`.

Implementation responsibilities in this repo:

- Update all documentation and visible metadata to use `pirata`
- Update the local `origin` remote URL after the GitHub repo is renamed upstream

If the current tooling cannot rename the remote repository directly, the upstream rename remains a manual GitHub operation, followed by a local remote URL update in this workspace.

## Error Handling

- `pirata --json` with no subcommand should fail clearly, because the default mode is interactive
- `pirata tui --json` should also fail clearly for the same reason
- Empty or cancelled interactive selection should exit cleanly without side effects
- Legacy config fallback should be silent and automatic

## Testing Strategy

Verification should cover:

- Clap parsing for `pirata` with no subcommand
- Clap parsing for each short alias
- Default interactive mode guardrails around incompatible flags like `--json`
- Config path resolution preferring `pirata` and falling back to `pirate-ctl`
- Build/test pass after the package rename updates `Cargo.lock`

## Files Likely Affected

- `Cargo.toml`
- `Cargo.lock`
- `README.md`
- `src/cli.rs`
- `src/app.rs`
- `src/config.rs`
- `src/indexer/pirate_bay.rs`
- `src/downloader/transmission.rs`

## Open Constraints

The GitHub repository rename may require either GitHub web access or a CLI/API capability that is not currently available in this session. If so, implementation should complete the in-repo rename and local remote update, then report the exact remaining manual GitHub step.
