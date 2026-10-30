mod config;

use berry_ui::BerryLauncherApplication;
use config::PKGDATADIR;
use gtk::{gio, glib};

// Every crate logs under its own target, so each one needs its own directive.
const DEFAULT_LOG_FILTER: &str = "berrylauncher=info,berry_core=info,berry_downloads=info,\
berry_games=info,berry_installer=info,berry_launcher=info,berry_locale=info,berry_media=info,\
berry_runners=info,berry_ui=info,warn";

fn main() -> glib::ExitCode {
    if let Err(error) = berry_core::settings::migrate_legacy_data() {
        eprintln!("Could not migrate legacy BerryLauncher data: {error:#}");
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| DEFAULT_LOG_FILTER.into()),
        )
        .with_target(false)
        .init();
    tracing::info!("BerryLauncher {} starting", config::VERSION);

    let resources = gio::Resource::load(PKGDATADIR.to_owned() + "/berrylauncher.gresource")
        .expect("Could not load resources");
    gio::resources_register(&resources);

    berry_locale::init(&berry_core::settings::load_preferences().language);

    // Background preparation (FPS unlocker, machine description, Steam Linux Runtime) starts once
    // the window had time to appear, so it does not compete with the application's startup.
    glib::timeout_add_local_once(std::time::Duration::from_secs(2), || {
        berry_launcher::GameLauncher::prefetch_fps_unlocker();
        berry_launcher::GameLauncher::prefetch_system_info();
        berry_runners::steamrt::prefetch_for_installed_runners();
    });

    let application = BerryLauncherApplication::new();
    application.run()
}
