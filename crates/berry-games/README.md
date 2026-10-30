# berry-games

The game catalogue and the per-game library data.

## Modules

- `catalog`: the supported games (`GameCatalogEntry`, `GameSource`, `GAMES`, `by_id`).
- `library`: library membership, per-game settings (`GameSettings`, loaded and saved as a GKeyFile), the settings enums (`SyncMode`, `FsrQuality`, `DxvkHud`, ...) and `split_command`.

## No translated text here

This crate holds no translated text. It exposes user-visible strings as Fluent message IDs (`label_id()`, `description_key()`). The application translates them in `src/labels.rs`. This keeps the crate free of the UI and i18n layer, so the launcher, downloads and UI crates can all share it.

## Dependencies

`berry-core`, `glib` and `anyhow`.
