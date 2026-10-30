//! The Fluent files are embedded with `rust-embed`. Tell cargo to rebuild this crate when any of
//! them changes (or one is added), so the binary can never keep translations from an older build.

fn main() {
    println!("cargo:rerun-if-changed=i18n");
}
