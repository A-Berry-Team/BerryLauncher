# berry-downloads

Install, update and repair jobs for games.

## Modules

- `sources`: asks the HoYoverse and Kuro endpoints what to download and whether an update exists.
- `worker` and `operation`: run a job on a worker thread through `berry-installer`, with cancel and pause.
- `state`: progress and UI state for each game, shared with the interface.
- `metadata`: install markers and version files written next to the game.

## Public API

The public API is in `lib.rs`: `install_game`, `update_game`, `repair_game`, `is_downloading`, `ui_state` and more.

## Notes

This crate has no GTK dependency. Progress travels over `async-channel`.
