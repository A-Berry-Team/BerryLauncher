//! Window with the details of the running (or last) install / update / repair of a game: the
//! step it is in, how each step went, the network speed over time and an activity log.
//!
//! Everything shown comes from [`downloads::download_details`], which is fed by what
//! berry-installer reports. The window only polls that snapshot, so it can be opened and closed
//! at any time without touching the operation.

use adw::prelude::*;
use gtk::pango::{EllipsizeMode, WrapMode};
use gtk::{glib, Align, Box, DrawingArea, Image, Label, Orientation, ProgressBar, ScrolledWindow};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use berry_downloads::{self as downloads, DownloadDetails, Outcome, StepState, MAX_SAMPLES};
use berry_games::catalog::GameCatalogEntry;
use berry_locale::fl;

use crate::layout::{apply_page_padding, PADDED_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT};
use crate::widgets::symbolic_image;

const REFRESH_INTERVAL: Duration = Duration::from_millis(500);
const GRAPH_HEIGHT: i32 = 150;
/// Newest log lines shown; older ones are dropped from the view only.
const VISIBLE_LOG_LINES: usize = 80;

thread_local! {
    /// One details window per game: opening it again brings the existing one to the front.
    static OPEN_WINDOWS: RefCell<HashMap<&'static str, glib::WeakRef<adw::Dialog>>> =
        RefCell::new(HashMap::new());
}

struct StepWidgets {
    row: adw::ActionRow,
    spinner: gtk::Spinner,
    icon: Image,
    time: Label,
}

struct Ui {
    status: Label,
    progress: ProgressBar,
    caption: Label,
    state: Label,
    operation: Label,
    version: Label,
    backend: Label,
    job: Label,
    elapsed: Label,
    steps: Vec<StepWidgets>,
    graph: DrawingArea,
    graph_hint: Label,
    graph_max: Label,
    samples: Rc<RefCell<Vec<u64>>>,
    stat_current: Label,
    stat_average: Label,
    stat_peak: Label,
    stat_remaining: Label,
    stat_downloaded: Label,
    log: Label,
}

pub(crate) fn open_download_details(parent: &adw::ApplicationWindow, game: GameCatalogEntry) {
    let existing = OPEN_WINDOWS.with(|open| open.borrow().get(game.id).and_then(|w| w.upgrade()));
    if let Some(window) = existing {
        window.present(Some(parent));
        return;
    }

    let window = adw::Dialog::builder()
        .title(format!(
            "{} · {}",
            fl!("details-title"),
            game.localized_title()
        ))
        // Same size as the game settings dialog. Nothing inside may ask for more than this:
        // long texts wrap or are ellipsized and the page scrolls vertically.
        .content_width(PADDED_WINDOW_WIDTH)
        .content_height(SETTINGS_WINDOW_HEIGHT)
        .build();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let scroll = ScrolledWindow::new();
    scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroll.set_propagate_natural_width(false);
    scroll.set_propagate_natural_height(false);
    let content = Box::new(Orientation::Vertical, 24);
    content.set_margin_top(24);
    content.set_margin_bottom(24);
    apply_page_padding(&content);
    scroll.set_child(Some(&content));
    toolbar.set_content(Some(&scroll));
    window.set_child(Some(&toolbar));

    let ui = Rc::new(build_ui(&content));

    let game_id = game.id;
    // Once the operation has ended and its final state is on screen there is nothing left to
    // update, so the timer neither copies the record nor touches the widgets until the next
    // operation of this game starts.
    let settled = Rc::new(std::cell::Cell::new(false));
    let tick = {
        let ui = ui.clone();
        move || {
            let active = downloads::is_downloading(game_id);
            if !active && settled.get() {
                return;
            }
            if let Some(details) = downloads::download_details(game_id) {
                refresh(&ui, &details);
                settled.set(!active);
            }
        }
    };
    tick();
    let weak_window = window.downgrade();
    glib::timeout_add_local(REFRESH_INTERVAL, move || {
        if weak_window.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
        tick();
        glib::ControlFlow::Continue
    });

    OPEN_WINDOWS.with(|open| {
        open.borrow_mut().insert(game.id, window.downgrade());
    });
    window.present(Some(parent));
}

fn build_ui(content: &Box) -> Ui {
    // Header: what the operation is doing right now.
    let header = Box::new(Orientation::Vertical, 8);
    let status = Label::new(None);
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.add_css_class("title-3");
    let progress = ProgressBar::new();
    let caption = Label::new(None);
    caption.set_xalign(0.0);
    caption.set_ellipsize(EllipsizeMode::End);
    caption.add_css_class("dim-label");
    header.append(&status);
    header.append(&progress);
    header.append(&caption);
    content.append(&header);

    let group = |title: String| {
        let group = adw::PreferencesGroup::new();
        group.set_title(&title);
        content.append(&group);
        group
    };

    let overview = group(fl!("details-section-overview"));
    let state = value_row(&overview, &fl!("details-row-state"));
    let operation = value_row(&overview, &fl!("details-row-operation"));
    let version = value_row(&overview, &fl!("details-row-version"));
    let backend = value_row(&overview, &fl!("details-row-backend"));
    let job = value_row(&overview, &fl!("details-row-job"));
    let elapsed = value_row(&overview, &fl!("details-row-elapsed"));

    let steps_group = group(fl!("details-section-steps"));
    let mut steps = Vec::new();
    for phase in downloads::PHASES {
        let row = adw::ActionRow::new();
        row.set_title(&downloads::phase_title(phase));
        let prefix = Box::new(Orientation::Horizontal, 0);
        prefix.set_size_request(16, 16);
        prefix.set_valign(Align::Center);
        let spinner = gtk::Spinner::new();
        spinner.set_visible(false);
        let icon = symbolic_image("emblem-ok-symbolic");
        icon.set_visible(false);
        prefix.append(&spinner);
        prefix.append(&icon);
        row.add_prefix(&prefix);
        let time = dim_label();
        row.add_suffix(&time);
        steps_group.add(&row);
        steps.push(StepWidgets {
            row,
            spinner,
            icon,
            time,
        });
    }

    let speed_group = group(fl!("details-section-speed"));
    let panel = Box::new(Orientation::Vertical, 12);
    panel.add_css_class("card");
    panel.set_margin_top(0);

    let samples: Rc<RefCell<Vec<u64>>> = Rc::new(RefCell::new(Vec::new()));
    let graph = DrawingArea::builder()
        .content_height(GRAPH_HEIGHT)
        .hexpand(true)
        .build();
    {
        let samples = samples.clone();
        graph.set_draw_func(move |area, cr, width, height| {
            draw_graph(area, cr, width, height, &samples.borrow());
        });
    }
    let graph_hint = Label::new(Some(&fl!("details-graph-waiting")));
    graph_hint.add_css_class("dim-label");
    graph_hint.set_halign(Align::Center);
    graph_hint.set_valign(Align::Center);
    let graph_max = Label::new(None);
    graph_max.add_css_class("caption");
    graph_max.add_css_class("dim-label");
    graph_max.set_halign(Align::Start);
    graph_max.set_valign(Align::Start);
    graph_max.set_margin_start(6);
    graph_max.set_margin_top(4);
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&graph));
    overlay.add_overlay(&graph_hint);
    overlay.add_overlay(&graph_max);
    overlay.set_margin_top(12);
    overlay.set_margin_start(12);
    overlay.set_margin_end(12);
    panel.append(&overlay);

    let stats = Box::new(Orientation::Horizontal, 12);
    stats.set_homogeneous(true);
    stats.set_margin_start(12);
    stats.set_margin_end(12);
    stats.set_margin_bottom(12);
    let (current_box, stat_current) = stat_box(&fl!("details-stat-current"));
    let (average_box, stat_average) = stat_box(&fl!("details-stat-average"));
    let (peak_box, stat_peak) = stat_box(&fl!("details-stat-peak"));
    let (remaining_box, stat_remaining) = stat_box(&fl!("details-stat-remaining"));
    let (downloaded_box, stat_downloaded) = stat_box(&fl!("details-stat-downloaded"));
    for stat in [
        &current_box,
        &average_box,
        &peak_box,
        &remaining_box,
        &downloaded_box,
    ] {
        stats.append(stat);
    }
    panel.append(&stats);
    speed_group.add(&panel);

    let log_group = group(fl!("details-section-log"));
    let log_panel = Box::new(Orientation::Vertical, 0);
    log_panel.add_css_class("card");
    let log = Label::new(None);
    log.set_xalign(0.0);
    log.set_yalign(0.0);
    log.set_wrap(true);
    // Paths and URLs have no spaces: let them break anywhere instead of widening the window.
    log.set_wrap_mode(WrapMode::WordChar);
    log.set_selectable(true);
    log.add_css_class("monospace");
    log.set_margin_top(12);
    log.set_margin_bottom(12);
    log.set_margin_start(12);
    log.set_margin_end(12);
    log_panel.append(&log);
    log_group.add(&log_panel);

    Ui {
        status,
        progress,
        caption,
        state,
        operation,
        version,
        backend,
        job,
        elapsed,
        steps,
        graph,
        graph_hint,
        graph_max,
        samples,
        stat_current,
        stat_average,
        stat_peak,
        stat_remaining,
        stat_downloaded,
        log,
    }
}

fn dim_label() -> Label {
    let label = Label::new(None);
    label.add_css_class("dim-label");
    label.add_css_class("numeric");
    label.set_valign(Align::Center);
    label.set_xalign(1.0);
    label.set_ellipsize(EllipsizeMode::End);
    label.set_max_width_chars(40);
    label
}

/// A row whose value sits on the right; returns the label to update.
fn value_row(group: &adw::PreferencesGroup, title: &str) -> Label {
    let row = adw::ActionRow::new();
    row.set_title(title);
    let value = dim_label();
    row.add_suffix(&value);
    group.add(&row);
    value
}

fn stat_box(caption: &str) -> (Box, Label) {
    let container = Box::new(Orientation::Vertical, 2);
    container.set_hexpand(true);
    let value = Label::new(Some("—"));
    value.set_ellipsize(EllipsizeMode::End);
    value.add_css_class("title-4");
    value.add_css_class("numeric");
    let name = Label::new(Some(caption));
    name.set_ellipsize(EllipsizeMode::End);
    name.add_css_class("caption");
    name.add_css_class("dim-label");
    container.append(&value);
    container.append(&name);
    (container, value)
}

fn refresh(ui: &Ui, details: &DownloadDetails) {
    // Header.
    let status = if details.status.is_empty() {
        details.kind.label()
    } else {
        details.status.clone()
    };
    ui.status.set_text(&status);
    let current_stat = details.phase.map(|phase| *details.stat(phase));
    match (details.phase, current_stat) {
        (Some(phase), Some(stat)) => {
            if stat.total > 0 {
                ui.progress
                    .set_fraction((stat.current as f64 / stat.total as f64).clamp(0.0, 1.0));
                ui.caption.set_text(&format!(
                    "{} · {} / {}",
                    downloads::phase_title(phase),
                    downloads::format_bytes(stat.current),
                    downloads::format_bytes(stat.total),
                ));
            } else {
                ui.progress.set_fraction(0.0);
                ui.caption.set_text(&downloads::phase_title(phase));
            }
        }
        _ => {
            ui.progress.set_fraction(0.0);
            ui.caption.set_text("");
        }
    }
    if details.outcome == Outcome::Completed {
        ui.progress.set_fraction(1.0);
    }

    // Overview.
    ui.state.remove_css_class("error");
    ui.state.remove_css_class("success");
    match &details.outcome {
        Outcome::Running => ui.state.set_text(&fl!("details-state-running")),
        Outcome::Paused => ui.state.set_text(&fl!("details-state-paused")),
        Outcome::Completed => {
            ui.state.set_text(&fl!("details-state-completed"));
            ui.state.add_css_class("success");
        }
        Outcome::Failed(_) => {
            ui.state.set_text(&fl!("details-state-failed"));
            ui.state.add_css_class("error");
        }
    }
    ui.operation.set_text(&details.kind.label());
    ui.version
        .set_text(details.version.as_deref().unwrap_or("—"));
    ui.backend.set_text(details.backend.unwrap_or("—"));
    if details.job_index == 0 {
        ui.job.set_text(&fl!("details-job-none"));
    } else {
        ui.job.set_text(&format!(
            "{} ({}/{})",
            details.job_label, details.job_index, details.job_total
        ));
    }
    ui.elapsed.set_text(&format_duration(details.elapsed()));

    // Steps.
    for (widgets, phase) in ui.steps.iter().zip(downloads::PHASES) {
        let state = details.step_state(phase);
        let stat = details.stat(phase);
        let active = state == StepState::Active;
        widgets.spinner.set_visible(active);
        widgets.spinner.set_spinning(active);
        let show_icon = matches!(state, StepState::Done | StepState::Interrupted);
        widgets.icon.set_visible(show_icon);
        if state == StepState::Interrupted {
            widgets
                .icon
                .set_icon_name(Some("media-playback-pause-symbolic"));
        } else {
            widgets.icon.set_icon_name(Some("emblem-ok-symbolic"));
        }
        let subtitle = match state {
            StepState::Pending => fl!("details-step-pending"),
            StepState::Skipped => fl!("details-step-skipped"),
            _ if stat.total > 0 => format!(
                "{} / {}",
                downloads::format_bytes(stat.current),
                downloads::format_bytes(stat.total)
            ),
            _ if stat.current > 0 => downloads::format_bytes(stat.current),
            _ => String::new(),
        };
        widgets.row.set_subtitle(&subtitle);
        widgets.time.set_text(&if stat.seen {
            format_duration(details.phase_elapsed(phase))
        } else {
            String::new()
        });
    }

    // Speed.
    let speed_text = |speed: u64| {
        if speed == 0 {
            "—".to_owned()
        } else {
            format!("{}/s", downloads::format_bytes(speed))
        }
    };
    ui.stat_current.set_text(&speed_text(details.speed));
    ui.stat_average
        .set_text(&speed_text(details.average_speed()));
    ui.stat_peak.set_text(&speed_text(details.peak_speed));
    ui.stat_remaining.set_text(
        &details
            .eta()
            .map(format_duration)
            .unwrap_or_else(|| "—".to_owned()),
    );
    ui.stat_downloaded
        .set_text(&downloads::format_bytes(details.downloaded_bytes()));

    let visible_max = details.samples.iter().copied().max().unwrap_or(0);
    ui.graph_hint.set_visible(details.samples.len() < 2);
    ui.graph_max.set_text(&if visible_max > 0 {
        format!("{}/s", downloads::format_bytes(visible_max))
    } else {
        String::new()
    });
    *ui.samples.borrow_mut() = details.samples.iter().copied().collect();
    ui.graph.queue_draw();

    // Activity log.
    let skip = details.log.len().saturating_sub(VISIBLE_LOG_LINES);
    let log = details
        .log
        .iter()
        .skip(skip)
        .map(|entry| format!("[{}] {}", format_duration(entry.at), entry.text))
        .collect::<Vec<_>>()
        .join("\n");
    ui.log.set_text(&log);
}

/// Speed over time: newest sample at the right edge, scaled to the highest visible value.
fn draw_graph(
    area: &DrawingArea,
    cr: &gtk::cairo::Context,
    width: i32,
    height: i32,
    samples: &[u64],
) {
    let (w, h) = (f64::from(width), f64::from(height));
    let foreground = area.color();
    let accent = adw::StyleManager::default().accent_color_rgba();

    // Horizontal grid lines.
    cr.set_line_width(1.0);
    cr.set_source_rgba(
        f64::from(foreground.red()),
        f64::from(foreground.green()),
        f64::from(foreground.blue()),
        0.12,
    );
    for line in 0..=4 {
        let y = (h - 1.0) * f64::from(line) / 4.0 + 0.5;
        cr.move_to(0.0, y);
        cr.line_to(w, y);
    }
    let _ = cr.stroke();

    if samples.len() < 2 {
        return;
    }
    let max = samples.iter().copied().max().unwrap_or(0).max(1) as f64 * 1.1;
    let step = w / (MAX_SAMPLES - 1) as f64;
    let last = samples.len() - 1;
    let point = |index: usize| -> (f64, f64) {
        let x = w - (last - index) as f64 * step;
        let y = h - 2.0 - (samples[index] as f64 / max) * (h - 4.0);
        (x, y)
    };

    // Area under the curve.
    let (first_x, _) = point(0);
    cr.move_to(first_x, h);
    for index in 0..=last {
        let (x, y) = point(index);
        cr.line_to(x, y);
    }
    cr.line_to(w, h);
    cr.close_path();
    cr.set_source_rgba(
        f64::from(accent.red()),
        f64::from(accent.green()),
        f64::from(accent.blue()),
        0.25,
    );
    let _ = cr.fill();

    // The curve itself.
    for index in 0..=last {
        let (x, y) = point(index);
        if index == 0 {
            cr.move_to(x, y);
        } else {
            cr.line_to(x, y);
        }
    }
    cr.set_line_width(2.0);
    cr.set_line_join(gtk::cairo::LineJoin::Round);
    cr.set_source_rgba(
        f64::from(accent.red()),
        f64::from(accent.green()),
        f64::from(accent.blue()),
        1.0,
    );
    let _ = cr.stroke();
}

/// `mm:ss`, or `h:mm:ss` from one hour on.
fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}
