# berry-launcher

Everything that happens when you press Play. The main type is `GameLauncher`.

## Modules

- `env`: Wine/Proton environment variables, locale, sync mode, and DXVK/FSR settings.
- `gamescope`: the gamescope wrapper and its arguments.
- `sync`: detects the sync primitive that Wine reports at runtime.
- `fps_unlock`: downloads the FPS unlocker and sets the FPS value in the registry.
- `report`: the diagnostic header written at the top of each game log.

## Dependencies

`berry-core`, `berry-games`, `berry-runners`, `berry-installer`, `berry-downloads` and `berry-locale`. It uses no GUI toolkit.
