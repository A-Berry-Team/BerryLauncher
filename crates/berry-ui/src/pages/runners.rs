use adw::prelude::*;
use gtk::{
    glib, Align, Box, Button, Label, ListBox, ListBoxRow, Orientation, ProgressBar, ScrolledWindow,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::widgets::page_scroller;
use berry_locale::fl;
use berry_runners::RunnerService;
use berry_runners::{Runner, RunnerProvider};

pub(crate) type Catalogs = Rc<RefCell<HashMap<RunnerProvider, Vec<Runner>>>>;
pub(crate) type Lists = Rc<RefCell<HashMap<RunnerProvider, ListBox>>>;
pub(crate) type ShowButtons = Rc<RefCell<HashMap<RunnerProvider, Button>>>;

pub(crate) type RunnerSender = async_channel::Sender<RunnerMessage>;

pub(crate) enum RunnerMessage {
    Fetched(RunnerProvider, anyhow::Result<Vec<Runner>>),
    /// Download progress of a runner being installed (`None` = indeterminate, e.g. unpacking).
    Progress(RunnerProvider, String, Option<f64>),
    ActionFinished(RunnerProvider, String, bool, anyhow::Result<()>),
}

// Download progress bars. Rows are rebuilt whenever the list refreshes (and the "Show all" dialog
// holds its own copies), so progress lives here, keyed by runner, and every bar registers itself.
// Everything runs on the GTK main thread.
type ProgressKey = (RunnerProvider, String);

struct Tracker {
    /// `None`: indeterminate, the bars pulse.
    fraction: Option<f64>,
    pulsing: bool,
    bars: Vec<glib::WeakRef<ProgressBar>>,
}

/// The list inside an open "Show all" dialog. The dialog builds its own rows, so the main
/// refresh has to rebuild them too or their buttons would show a stale installed / removed state.
struct FullList {
    list: glib::WeakRef<ListBox>,
    scroll: glib::WeakRef<ScrolledWindow>,
}

thread_local! {
    static TRACKERS: RefCell<HashMap<ProgressKey, Tracker>> = RefCell::new(HashMap::new());
    /// Cancel flag of every installation in progress (download or unpacking), shared with the
    /// installing thread. An entry exists exactly while that runner is installing.
    static CANCELS: RefCell<HashMap<ProgressKey, Arc<AtomicBool>>> = RefCell::new(HashMap::new());
    static FULL_LISTS: RefCell<HashMap<RunnerProvider, FullList>> = RefCell::new(HashMap::new());
}

fn apply_progress(bar: &ProgressBar, fraction: Option<f64>) {
    bar.set_visible(true);
    match fraction {
        Some(fraction) => {
            bar.set_fraction(fraction);
            bar.set_tooltip_text(Some(&format!("{:.0}%", fraction * 100.0)));
        }
        None => bar.set_tooltip_text(None),
    }
}

/// Bar shown to the left of a runner's download button. Hidden unless that runner is installing.
fn progress_bar(provider: RunnerProvider, name: &str) -> ProgressBar {
    let bar = ProgressBar::new();
    bar.set_valign(Align::Center);
    bar.set_size_request(110, -1);
    bar.set_visible(false);
    register_bar(provider, name, &bar);
    bar
}

/// Lets `bar` follow the installation of `name`; a no-op while that runner is not installing.
fn register_bar(provider: RunnerProvider, name: &str, bar: &ProgressBar) {
    TRACKERS.with(|trackers| {
        if let Some(tracker) = trackers.borrow_mut().get_mut(&(provider, name.to_owned())) {
            apply_progress(bar, tracker.fraction);
            tracker.bars.push(bar.downgrade());
        }
    });
}

/// Starts tracking an installation: the bars of that runner appear at 0 %.
pub(crate) fn begin_progress(provider: RunnerProvider, name: &str) {
    TRACKERS.with(|trackers| {
        let mut trackers = trackers.borrow_mut();
        let tracker = trackers
            .entry((provider, name.to_owned()))
            .or_insert(Tracker {
                fraction: Some(0.0),
                pulsing: false,
                bars: Vec::new(),
            });
        tracker.fraction = Some(0.0);
        tracker.bars.retain(|bar| bar.upgrade().is_some());
        for bar in tracker.bars.iter().filter_map(|bar| bar.upgrade()) {
            apply_progress(&bar, tracker.fraction);
        }
    });
}

pub(crate) fn set_progress(provider: RunnerProvider, name: &str, fraction: Option<f64>) {
    let start_pulsing = TRACKERS.with(|trackers| {
        let mut trackers = trackers.borrow_mut();
        let Some(tracker) = trackers.get_mut(&(provider, name.to_owned())) else {
            return false;
        };
        tracker.fraction = fraction;
        tracker.bars.retain(|bar| bar.upgrade().is_some());
        for bar in tracker.bars.iter().filter_map(|bar| bar.upgrade()) {
            apply_progress(&bar, fraction);
        }
        let start = fraction.is_none() && !tracker.pulsing;
        tracker.pulsing = fraction.is_none();
        start
    });
    if start_pulsing {
        let key = (provider, name.to_owned());
        glib::timeout_add_local(std::time::Duration::from_millis(120), move || {
            TRACKERS.with(|trackers| {
                let mut trackers = trackers.borrow_mut();
                match trackers.get_mut(&key) {
                    Some(tracker) if tracker.fraction.is_none() => {
                        tracker.bars.retain(|bar| bar.upgrade().is_some());
                        for bar in tracker.bars.iter().filter_map(|bar| bar.upgrade()) {
                            bar.pulse();
                        }
                        glib::ControlFlow::Continue
                    }
                    Some(tracker) => {
                        tracker.pulsing = false;
                        glib::ControlFlow::Break
                    }
                    None => glib::ControlFlow::Break,
                }
            })
        });
    }
}

/// Registers the cancel flag of an installation that is about to start.
fn register_cancel(provider: RunnerProvider, name: &str) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    CANCELS.with(|cancels| {
        cancels
            .borrow_mut()
            .insert((provider, name.to_owned()), flag.clone())
    });
    flag
}

/// `None` when `name` is not installing; otherwise whether its cancellation was already requested.
fn install_cancel_requested(provider: RunnerProvider, name: &str) -> Option<bool> {
    CANCELS.with(|cancels| {
        cancels
            .borrow()
            .get(&(provider, name.to_owned()))
            .map(|flag| flag.load(Ordering::Relaxed))
    })
}

/// Asks the installation of `name` to stop. Returns `false` when it is not installing.
fn request_cancel(provider: RunnerProvider, name: &str) -> bool {
    CANCELS.with(
        |cancels| match cancels.borrow().get(&(provider, name.to_owned())) {
            Some(flag) => {
                flag.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        },
    )
}

/// The installation is over (finished, failed or cancelled): every bar of that runner disappears.
pub(crate) fn end_progress(provider: RunnerProvider, name: &str) {
    CANCELS.with(|cancels| cancels.borrow_mut().remove(&(provider, name.to_owned())));
    TRACKERS.with(|trackers| {
        if let Some(tracker) = trackers.borrow_mut().remove(&(provider, name.to_owned())) {
            for bar in tracker.bars.iter().filter_map(|bar| bar.upgrade()) {
                bar.set_visible(false);
            }
        }
    });
}

pub(crate) fn build_runners_page(lists: &Lists, show_buttons: &ShowButtons) -> ScrolledWindow {
    let (scroll, content) = page_scroller();
    let heading = Label::new(Some(&fl!("runners-title")));
    heading.set_halign(Align::Start);
    heading.add_css_class("title-1");
    content.append(&heading);
    let description = Label::new(Some(&fl!("runners-subtitle")));
    description.set_halign(Align::Start);
    description.set_wrap(true);
    description.add_css_class("dim-label");
    content.append(&description);

    for provider in RunnerProvider::ALL {
        let section = Box::new(Orientation::Vertical, 8);
        let title = Label::new(Some(provider.display_name()));
        title.set_halign(Align::Start);
        title.add_css_class("title-3");
        let list = ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("boxed-list");
        let show_all = Button::with_label(&fl!("runners-show-all"));
        show_all.set_halign(Align::Start);
        show_all.set_visible(false);
        section.append(&title);
        section.append(&list);
        section.append(&show_all);
        lists.borrow_mut().insert(provider, list);
        show_buttons.borrow_mut().insert(provider, show_all);
        content.append(&section);
    }
    scroll
}

pub(crate) fn refresh_runner_list(
    provider: RunnerProvider,
    lists: &Lists,
    show_buttons: &ShowButtons,
    catalogs: &Catalogs,
    window: &adw::ApplicationWindow,
    service: &Arc<RunnerService>,
    sender: &RunnerSender,
) {
    let Some(list) = lists.borrow().get(&provider).cloned() else {
        return;
    };
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    let entries = catalogs
        .borrow()
        .get(&provider)
        .cloned()
        .unwrap_or_default();
    for runner in entries.iter().take(5).cloned() {
        list.append(&runner_row(
            runner,
            provider,
            lists,
            show_buttons,
            catalogs,
            window,
            service,
            sender,
        ));
    }
    if let Some(button) = show_buttons.borrow().get(&provider) {
        button.set_visible(entries.len() > 5);
        button.set_label(&fl!("runners-show-all-count", count = entries.len()));
    }
    refresh_full_list(
        provider,
        lists,
        show_buttons,
        catalogs,
        window,
        service,
        sender,
    );
}

/// Rebuilds the rows of the "Show all" dialog of `provider`, if it is open, keeping its scroll
/// position. A no-op once the dialog is closed.
fn refresh_full_list(
    provider: RunnerProvider,
    lists: &Lists,
    show_buttons: &ShowButtons,
    catalogs: &Catalogs,
    window: &adw::ApplicationWindow,
    service: &Arc<RunnerService>,
    sender: &RunnerSender,
) {
    let open = FULL_LISTS.with(|full| {
        let mut full = full.borrow_mut();
        let alive = full.get(&provider).and_then(|entry| {
            let (list, scroll) = (entry.list.upgrade()?, entry.scroll.upgrade()?);
            list.root().is_some().then_some((list, scroll))
        });
        if alive.is_none() {
            full.remove(&provider);
        }
        alive
    });
    let Some((list, scroll)) = open else {
        return;
    };
    let position = scroll.vadjustment().value();
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    let entries = catalogs
        .borrow()
        .get(&provider)
        .cloned()
        .unwrap_or_default();
    for runner in entries {
        list.append(&runner_row(
            runner,
            provider,
            lists,
            show_buttons,
            catalogs,
            window,
            service,
            sender,
        ));
    }
    // The rebuilt list is shorter for a moment, which resets the offset: put it back afterwards.
    glib::idle_add_local_once(move || {
        scroll.vadjustment().set_value(position);
    });
}

pub(crate) fn runner_row(
    runner: Runner,
    provider: RunnerProvider,
    lists: &Lists,
    show_buttons: &ShowButtons,
    catalogs: &Catalogs,
    window: &adw::ApplicationWindow,
    service: &Arc<RunnerService>,
    sender: &RunnerSender,
) -> ListBoxRow {
    let row = ListBoxRow::new();
    let box_ = Box::new(Orientation::Horizontal, 12);
    box_.set_margin_top(10);
    box_.set_margin_bottom(10);
    box_.set_margin_start(12);
    box_.set_margin_end(12);
    let labels = Box::new(Orientation::Vertical, 2);
    labels.set_hexpand(true);
    labels.set_valign(Align::Center);
    let title = Label::new(Some(&runner.name));
    title.set_halign(Align::Start);
    title.set_valign(Align::Center);
    labels.append(&title);
    // Only an installation shows a bar; removing a runner is quick and has no progress.
    let bar = progress_bar(provider, &runner.name);
    // While the runner installs, its download button turns into a "stop" button.
    let installing = if runner.installed {
        None
    } else {
        install_cancel_requested(provider, &runner.name)
    };
    let (icon, style) = if runner.installed {
        ("user-trash-symbolic", "destructive-action")
    } else if installing.is_some() {
        ("process-stop-symbolic", "destructive-action")
    } else {
        ("folder-download-symbolic", "berry-runner-download")
    };
    let button = Button::from_icon_name(icon);
    button.set_has_frame(true);
    button.add_css_class("circular");
    button.set_valign(Align::Center);
    button.add_css_class(style);
    match installing {
        Some(cancelling) => {
            button.set_tooltip_text(Some(&fl!("runners-cancel")));
            // Once the stop was requested there is nothing left to press until the row rebuilds.
            button.set_sensitive(!cancelling);
        }
        None => button.set_sensitive(!runner.busy),
    }
    let service = service.clone();
    let catalogs = catalogs.clone();
    let lists = lists.clone();
    let show_buttons = show_buttons.clone();
    let window = window.clone();
    let sender = sender.clone();
    let runner_for_click = runner.clone();
    button.connect_clicked(move |button| {
        // The runner is installing: this button stops the download or the unpacking. The
        // installing thread deletes what it created and reports back through `ActionFinished`.
        if !runner_for_click.installed && request_cancel(provider, &runner_for_click.name) {
            button.set_sensitive(false);
            return;
        }
        if runner_for_click.installed {
            // Immediate feedback; the row is rebuilt right below.
            button.set_sensitive(false);
        }
        run_runner_action(
            provider,
            runner_for_click.clone(),
            !runner_for_click.installed,
            service.clone(),
            catalogs.clone(),
            lists.clone(),
            show_buttons.clone(),
            window.clone(),
            sender.clone(),
        );
    });
    box_.append(&labels);
    box_.append(&bar);
    box_.append(&button);
    row.set_child(Some(&box_));
    row
}

pub(crate) fn run_runner_action(
    provider: RunnerProvider,
    runner: Runner,
    install: bool,
    service: Arc<RunnerService>,
    catalogs: Catalogs,
    lists: Lists,
    show_buttons: ShowButtons,
    window: adw::ApplicationWindow,
    sender: RunnerSender,
) {
    if let Some(items) = catalogs.borrow_mut().get_mut(&provider) {
        if let Some(item) = items.iter_mut().find(|item| item.name == runner.name) {
            item.busy = true;
        }
    }
    // Tracking must exist before the rows are rebuilt, so the new rows get their bar.
    let cancel = install.then(|| {
        begin_progress(provider, &runner.name);
        register_cancel(provider, &runner.name)
    });
    refresh_runner_list(
        provider,
        &lists,
        &show_buttons,
        &catalogs,
        &window,
        &service,
        &sender,
    );
    std::thread::spawn(move || {
        let result = if let Some(cancel) = cancel {
            let (progress_sender, name) = (sender.clone(), runner.name.clone());
            service.install_with_progress(&runner, &cancel, move |fraction| {
                let _ = progress_sender.send_blocking(RunnerMessage::Progress(
                    provider,
                    name.clone(),
                    fraction,
                ));
            })
        } else {
            service.remove(&runner)
        };
        let _ = sender.send_blocking(RunnerMessage::ActionFinished(
            provider,
            runner.name,
            install,
            result,
        ));
    });
}

pub(crate) fn show_runner_error(list: &ListBox, message: &str) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    let row = ListBoxRow::new();
    let label = Label::new(Some(message));
    label.set_wrap(true);
    label.set_xalign(0.0);
    label.add_css_class("error");
    row.set_child(Some(&label));
    list.append(&row);
}

pub(crate) fn show_all_runners(
    provider: RunnerProvider,
    lists: &Lists,
    show_buttons: &ShowButtons,
    catalogs: &Catalogs,
    parent: &adw::ApplicationWindow,
    service: &Arc<RunnerService>,
    sender: &RunnerSender,
) {
    let dialog = adw::Dialog::builder()
        .title(provider.display_name())
        .content_width(crate::layout::PADDED_WINDOW_WIDTH)
        .content_height(600)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let content = Box::new(Orientation::Vertical, 8);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    crate::layout::apply_page_padding(&content);
    let list = ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("boxed-list");
    content.append(&list);
    let scrolled = ScrolledWindow::new();
    scrolled.set_child(Some(&content));
    toolbar.set_content(Some(&scrolled));
    dialog.set_child(Some(&toolbar));
    // Present first: an AdwDialog's child does not have a root until it is presented.
    // refresh_full_list removes entries whose list has no root, so register only afterwards.
    dialog.present(Some(parent));
    FULL_LISTS.with(|full| {
        full.borrow_mut().insert(
            provider,
            FullList {
                list: list.downgrade(),
                scroll: scrolled.downgrade(),
            },
        );
    });
    refresh_full_list(
        provider,
        lists,
        show_buttons,
        catalogs,
        parent,
        service,
        sender,
    );
}
