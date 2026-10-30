pub mod fs;
pub mod http;
pub mod settings;
pub mod system;

/// Launcher version (the workspace version, kept equal to meson.build by `check-version.py`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
