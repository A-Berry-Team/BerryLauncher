//! User-agent strings shared by every HTTP client in the launcher.

pub fn user_agent() -> String {
    format!("BerryLauncher/{}", env!("CARGO_PKG_VERSION"))
}

pub const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
