# berry-core

The shared foundation of Berry Launcher. It has no UI and no game logic.

## Modules

| Module | Purpose |
| --- | --- |
| `settings` | Data directories (`data_dir`, `runners_dir`, `prefixes_dir`, ...), launcher preferences, Flatpak and host-launch detection, and migration of legacy data |
| `fs` | Filesystem helpers (`find_file`) |
| `http` | User-agent strings used by every HTTP client |
| `system` | Host probes (`/dev/ntsync`) |

## Dependencies

It depends only on `glib` (for GKeyFile and XDG directories) and `anyhow`.

Every other `berry-*` crate may depend on `berry-core`. It must not depend on any of them.
