# berry-runners

Wine/Proton runners and the Steam Linux Runtime container.

## Modules

- `model`: `Runner` and `RunnerProvider`.
- `service`: `RunnerService`. It lists, downloads, installs and removes runners, and extracts archives. Installs take a cancel flag, and a cancelled install leaves nothing behind.
- `steamrt`: Steam Linux Runtime selection and install. It is a thin layer over `berry_installer::steamrt`.

## Dependencies

`berry-core` and `berry-installer`. It uses no GTK.
