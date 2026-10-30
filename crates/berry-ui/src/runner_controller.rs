use gtk::glib;
use std::sync::Arc;

use crate::pages::runners::{
    end_progress, refresh_runner_list, run_runner_action, set_progress, show_runner_error,
    Catalogs, Lists, RunnerMessage, RunnerSender, ShowButtons,
};
use berry_runners::RunnerProvider;
use berry_runners::RunnerService;

pub(crate) fn start_runner_updates(
    window: &adw::ApplicationWindow,
    runner_service: &Arc<RunnerService>,
    catalogs: &Catalogs,
    lists: &Lists,
    show_buttons: &ShowButtons,
    receiver: async_channel::Receiver<RunnerMessage>,
    sender: RunnerSender,
) {
    let service_ui = runner_service.clone();
    let catalogs_ui = catalogs.clone();
    let lists_ui = lists.clone();
    let buttons_ui = show_buttons.clone();
    let window_for_runner_updates = window.clone();
    let sender_ui = sender.clone();

    glib::MainContext::default().spawn_local(async move {
        while let Ok(message) = receiver.recv().await {
            match message {
                RunnerMessage::Fetched(provider, result) => match result {
                    Ok(runners) => {
                        catalogs_ui.borrow_mut().insert(provider, runners);
                        refresh_runner_list(
                            provider,
                            &lists_ui,
                            &buttons_ui,
                            &catalogs_ui,
                            &window_for_runner_updates,
                            &service_ui,
                            &sender_ui,
                        );

                        // The launcher never stays without a runner: with none installed, the
                        // newest DW-Proton is downloaded right away.
                        if provider == RunnerProvider::DwProton && service_ui.installed_count() == 0
                        {
                            // Bound first: a `borrow()` kept alive in the `if let` below would
                            // still be held when `run_runner_action` takes `borrow_mut()`.
                            let newest = catalogs_ui
                                .borrow()
                                .get(&provider)
                                .and_then(|runners| runners.first())
                                .cloned();
                            if let Some(runner) = newest {
                                run_runner_action(
                                    provider,
                                    runner,
                                    true,
                                    service_ui.clone(),
                                    catalogs_ui.clone(),
                                    lists_ui.clone(),
                                    buttons_ui.clone(),
                                    window_for_runner_updates.clone(),
                                    sender_ui.clone(),
                                );
                            }
                        }
                    }
                    Err(error) => {
                        tracing::warn!("[{provider}] failed to fetch runners: {error:#}");
                        if let Some(list) = lists_ui.borrow().get(&provider) {
                            show_runner_error(list, &error.to_string());
                        }
                    }
                },
                RunnerMessage::Progress(provider, name, fraction) => {
                    set_progress(provider, &name, fraction);
                }
                RunnerMessage::ActionFinished(provider, name, installed, result) => {
                    // The bar goes away whether the installation worked or not.
                    end_progress(provider, &name);
                    if let Some(items) = catalogs_ui.borrow_mut().get_mut(&provider) {
                        if let Some(item) = items.iter_mut().find(|item| item.name == name) {
                            item.busy = false;
                            if result.is_ok() {
                                item.installed = installed;
                            }
                        }
                    }
                    if let Err(error) = result {
                        if berry_runners::is_cancelled(&error) {
                            tracing::warn!("[{provider}] installation of {name} cancelled");
                        } else {
                            tracing::warn!(
                                "[{provider}] runner operation failed for {name}: {error:#}"
                            );
                        }
                    }
                    refresh_runner_list(
                        provider,
                        &lists_ui,
                        &buttons_ui,
                        &catalogs_ui,
                        &window_for_runner_updates,
                        &service_ui,
                        &sender_ui,
                    );
                }
            }
        }
    });

    for provider in RunnerProvider::ALL {
        let service = runner_service.clone();
        let sender = sender.clone();
        std::thread::spawn(move || {
            let result = service.fetch(provider);
            let _ = sender.send_blocking(RunnerMessage::Fetched(provider, result));
        });
    }
}
