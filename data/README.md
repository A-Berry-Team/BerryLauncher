# Desktop integration data

This folder holds the system integration files that Meson installs: the desktop entry, AppStream metadata, GSettings schema, D-Bus service and application icon.

## Files

| File | Purpose |
| --- | --- |
| `app.berrylauncher.berry.desktop.in` | Desktop application entry |
| `app.berrylauncher.berry.metainfo.xml.in` | AppStream metadata and release history |
| `app.berrylauncher.berry.gschema.xml` | GSettings schema. It is currently empty and the code does not use it; the launcher saves its preferences in `preferences.ini` in the data directory |
| `app.berrylauncher.berry.service.in` | D-Bus activation service |
| `icons/` | The installed 256×256 hicolor application icon |

The desktop entry, the D-Bus service and the AppStream component all share the application ID `app.berrylauncher.berry`.

The AppStream metadata links to the A-Berry-Team organization and the BerryLauncher repository.

## Application icon

The application icon is a 256×256 PNG at `data/icons/hicolor/256x256/apps/app.berrylauncher.berry.png`. Meson installs it into the matching hicolor directory. There is no scalable SVG application icon.

If you replace the icon, keep the same filename and dimensions.
