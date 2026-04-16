# Pirata Download History And Fast TUI Design

## Goal

Make the TUI remember completed downloads across relaunches, hide stale history entries when the downloaded file or folder is gone, and remove the slow/glitchy feel from arrow-key navigation.

## Scope

This design covers:

- Durable Pirata-owned history for downloads started from Pirata
- Reloading prior completed downloads on startup even when Transmission no longer reports them
- Hiding completed entries whose file or folder is missing on disk
- Replacing the current fuzzy-selector browsing path with a faster list-style selector for normal up/down navigation

This design does not cover:

- Full-screen `ratatui`/`crossterm` application architecture
- Recovering completed downloads that were never started by Pirata
- Real-time transfer percentage polling from Transmission after add/start

## Current State

The current interactive mode is a promoted `dialoguer::FuzzySelect` search-and-add flow. It is not a true multi-screen TUI and it does not persist download history. Search results are cached, but downloads are not. Once Pirata exits, it forgets prior completed downloads entirely.

The current selector is also mismatched to the primary interaction. `FuzzySelect` is optimized for scoring text matches while the user types, but the current usage is mostly a plain arrow-key browser over already-fetched items. That creates avoidable redraw work and makes the interaction feel slow.

## Design Decisions

### 1. Source of truth for completed history

Pirata will maintain its own durable download history file. This becomes the source of truth for completed downloads previously started by Pirata.

Transmission remains the source of truth for live/download-in-progress state when available. Pirata history fills the gap for completed items that are no longer visible through Transmission.

### 2. History storage

Add a new history store under Pirata’s app data/cache area using a JSON file. The history entry should store only the fields needed to rebuild the TUI list and validate disk existence:

- stable download identifier, using info hash when available
- torrent/display name
- magnet/info hash metadata
- downloader kind
- final or expected download path
- whether the entry was last known completed
- timestamps for add/completion

History should only be written for downloads Pirata actually starts. Pirata should not attempt to backfill arbitrary existing Transmission items into permanent history unless the user started them through Pirata.

### 3. Startup reconstruction

On interactive startup:

1. Load Pirata history
2. Remove completed entries whose file or directory path no longer exists
3. Load current live state from Transmission if the configured downloader is Transmission
4. Merge the live state with local history into a unified TUI model

Merge rules:

- Active/in-progress Transmission items win over local history for the same info hash
- Completed local history entries remain visible even if Transmission no longer lists them
- Completed local history entries are hidden if their target path is gone

### 4. Filesystem validation

Completed-history visibility requires an existence check against the stored path.

Rules:

- If the stored path exists as either a file or directory, keep the item
- If the path does not exist, drop it from the startup view and prune it from persisted history on save
- If Pirata cannot determine a path for a started download, do not persist it as a durable completed-history entry

This keeps the history honest and avoids showing phantom completed items.

### 5. TUI interaction model

Replace the current `FuzzySelect` browsing path with a simpler, non-fuzzy interactive list for the main selection experience.

Recommended shape for this iteration:

- Prompt for the query with `Input`
- Fetch results
- Render a compact numbered list/table in the terminal
- Use `dialoguer::Select` for arrow-key navigation and selection

This keeps the existing terminal-first architecture but removes fuzzy matching from the hot navigation path. The list should show the highest-signal columns only, with shorter preformatted labels to minimize redraw cost.

### 6. Add/start lifecycle updates

When Pirata starts a download successfully:

- create or update the history entry immediately with the best-known target path
- mark it active/pending if completion is not yet known

When Pirata can determine from Transmission that a tracked item is complete:

- update the history entry to completed
- keep it visible on future launches as long as the stored path still exists

For this iteration, completion can be inferred only when Pirata has access to a completed/live Transmission record. Background completion tracking outside Pirata runtime is not required.

### 7. Transmission integration boundary

To support startup reconstruction, the Transmission integration needs a read path in addition to `torrent-add`. Add a small read-only RPC query that can fetch a compact set of torrent fields such as:

- hashString / info hash
- name
- percentDone
- isFinished / doneDate / status
- downloadDir

Pirata should use only the minimum fields needed for TUI reconstruction and history updates.

### 8. Performance constraints

The new selector path should avoid expensive fuzzy matching and repeated long string formatting on every move.

Practical constraints:

- precompute display labels once per result set
- keep labels short and stable
- avoid live recomputation while moving selection

This is the expected root fix for the “super slow” arrow-key behavior.

## Files Likely Affected

- `src/app.rs`
- `src/downloader/transmission.rs`
- `src/downloader/mod.rs`
- `src/config.rs`
- `src/model.rs`
- `src/cache.rs` or a new dedicated history module
- `README.md`

Preferred new file:

- `src/history.rs` for durable download-history storage

## Testing Strategy

Verification should cover:

- history entries persist and reload across process restarts
- stale completed entries are filtered out when the stored path is missing
- merge behavior prefers live Transmission state over old local state for the same item
- selector label generation is stable and compact
- existing interactive command parsing still works

## Open Constraints

The current codebase still uses a terminal prompt/list flow rather than a full-screen TUI. This design keeps that architecture and fixes the persistence/performance problems without turning the project into a different application.
