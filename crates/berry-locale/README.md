# berry-locale

Localization for Berry Launcher. It embeds the Fluent translation files and picks the language to use.

## Translation files

Translations are [Project Fluent](https://projectfluent.org/) files at `i18n/<code>/berrylauncher.ftl`. They are embedded in the binary with `rust-embed`.

To add a language, add a folder with that file. Messages missing from it fall back to English.

When you add or fix a translation:

- Use the English file (`i18n/en`) as your reference.
- Keep message IDs, variables and selectors unchanged.

## API

| Item | Purpose |
| --- | --- |
| `init(preference)`, `choices()`, `SYSTEM` | Language selection: the system language or an explicit one |
| `fl!("message-id")` and `fl!("message-id", name = value)` | Translate a message |
| `background_language()`, `play_locale()`, `wine_locale()` | Locale codes that the launcher passes to game servers and Wine |

Message IDs are resolved at runtime. An unknown ID renders as the ID itself, so check new IDs against `i18n/en`.

Any crate can write `use berry_locale::fl;`. No per-crate `i18n.toml` is needed.
