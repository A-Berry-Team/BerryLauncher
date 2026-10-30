use adw::prelude::*;
use gtk::glib;

use crate::MainWindow;

pub struct BerryLauncherApplication {
    app: adw::Application,
}

impl BerryLauncherApplication {
    pub fn new() -> Self {
        let app = adw::Application::builder()
            .application_id("app.berrylauncher.berry")
            .resource_base_path("/app/berrylauncher/berry")
            .build();

        // Opening the launcher again while it is alive without a window (it is kept running for
        // a game inside the Flatpak sandbox) builds a fresh window; otherwise the existing one
        // is brought to the front.
        app.connect_activate(|app| match app.windows().first() {
            Some(window) => window.present(),
            None => MainWindow::new(app).present(),
        });

        // The launcher outlives its last window only to keep a sandboxed game running (see
        // `GameLauncher::needs_launcher_alive`). Closing it still stops transfers, so nothing
        // keeps downloading in the background with no window to show it.
        app.connect_window_removed(|app, _| {
            if app.windows().is_empty()
                && berry_launcher::GameLauncher::any_game_needs_launcher_alive()
            {
                std::thread::spawn(|| berry_downloads::shutdown(std::time::Duration::from_secs(5)));
            }
        });

        // Stop running transfers and let workers leave resumable state before the process exits.
        app.connect_shutdown(|_| {
            berry_downloads::shutdown(std::time::Duration::from_secs(5));
        });

        Self { app }
    }

    pub fn run(&self) -> glib::ExitCode {
        self.app.run()
    }
}
