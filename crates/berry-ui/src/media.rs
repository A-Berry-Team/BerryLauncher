use adw::prelude::*;
use async_channel::TryRecvError;
use gtk::{gdk, glib, Picture};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::{Duration, SystemTime};

use crate::widgets::cover_picture;
use berry_media::backgrounds;

const MEDIA_RETRIES: u8 = 3;

/// The layers of a game's background: still image, animated video and theme overlay, plus a
/// loading animation that stays until there is an image to show.
#[derive(Clone)]
pub(crate) struct BackgroundView {
    pub(crate) overlay: gtk::Overlay,
    picture: Picture,
    video: Picture,
    overlay_picture: Picture,
    spinner: adw::Spinner,
    /// The player behind `video`, if there is one.
    player: Rc<RefCell<Option<Rc<LoopingVideo>>>>,
}

pub(crate) fn cached_background_view(game_id: &str) -> BackgroundView {
    let overlay = gtk::Overlay::new();
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);

    let picture = cover_picture();
    overlay.set_child(Some(&picture));

    let video = cover_picture();
    video.set_can_target(false);
    // The background video only plays while it is actually on screen. Library pages that are not
    // visible (or a hidden video) must not keep producing frames and waking the frame clock.
    video.connect_map(|video| set_video_playing(video, true));
    video.connect_unmap(|video| set_video_playing(video, false));
    overlay.add_overlay(&video);

    let overlay_picture = cover_picture();
    overlay_picture.set_can_target(false);
    overlay.add_overlay(&overlay_picture);

    let spinner = adw::Spinner::new();
    spinner.set_halign(gtk::Align::Center);
    spinner.set_valign(gtk::Align::Center);
    spinner.set_size_request(48, 48);
    spinner.set_can_target(false);
    overlay.add_overlay(&spinner);

    let handle = gtk::WindowHandle::new();
    handle.set_hexpand(true);
    handle.set_vexpand(true);
    overlay.add_overlay(&handle);

    let view = BackgroundView {
        overlay,
        picture,
        video,
        overlay_picture,
        spinner,
        player: Rc::new(RefCell::new(None)),
    };
    view.refresh(game_id);
    view
}

impl BackgroundView {
    pub(crate) fn refresh(&self, game_id: &str) {
        let preferences = berry_core::settings::load_preferences();
        let assets = backgrounds::cached_backgrounds(game_id);

        if let Some(texture) = assets
            .static_path
            .as_deref()
            .and_then(berry_media::images::texture_from_file)
        {
            self.picture.set_paintable(Some(&texture));
        }
        self.spinner.set_visible(self.picture.paintable().is_none());

        let overlay_texture = assets
            .overlay_path
            .as_deref()
            .and_then(berry_media::images::texture_from_file);
        self.overlay_picture.set_paintable(overlay_texture.as_ref());
        self.overlay_picture.set_visible(overlay_texture.is_some());

        let video_path = assets
            .video_path
            .filter(|_| preferences.animated_background);
        match video_path {
            Some(path) => {
                // A new player is only built when there is none yet or the file on disk changed
                // (an update replaced it); otherwise the one that is playing is left alone.
                let stamp = file_stamp(&path);
                let up_to_date = self.video.is_visible()
                    && self.video.paintable().is_some()
                    && self
                        .player
                        .borrow()
                        .as_ref()
                        .is_some_and(|player| player.stamp == stamp);
                if !up_to_date {
                    let previous = self.player.borrow_mut().take();
                    if let Some(previous) = previous {
                        previous.stop();
                    }
                    *self.player.borrow_mut() = Some(LoopingVideo::attach(&path, &self.video));
                }
                self.video.set_visible(true);
                set_video_playing(&self.video, self.video.is_mapped());
            }
            None => {
                set_video_playing(&self.video, false);
                self.video.set_visible(false);
            }
        }
    }
}

fn stamps(game_id: &str) -> [Option<SystemTime>; 3] {
    [
        backgrounds::background_path(game_id),
        backgrounds::video_path(game_id),
        backgrounds::overlay_path(game_id),
    ]
    .map(|path| file_stamp(&path))
}

/// Applies each background file as it lands on disk, until `done` fires (the update finished)
/// or the page is gone.
pub(crate) fn watch_background_update(
    view: &BackgroundView,
    game_id: &str,
    done: async_channel::Receiver<()>,
) {
    let view = view.clone();
    let game_id = game_id.to_string();
    let mut seen = stamps(&game_id);
    glib::timeout_add_seconds_local(1, move || {
        if view.overlay.root().is_none() {
            return glib::ControlFlow::Break;
        }
        let finished = !matches!(done.try_recv(), Err(TryRecvError::Empty));
        let current = stamps(&game_id);
        if current != seen || finished {
            seen = current;
            view.refresh(&game_id);
        }
        if finished {
            // Nothing could be downloaded: stop the animation instead of spinning forever.
            view.spinner.set_visible(false);
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

/// Plays or pauses the `MediaFile` shown by `video`, if it shows one.
fn set_video_playing(video: &Picture, playing: bool) {
    let Some(media) = video
        .paintable()
        .and_then(|p| p.downcast::<gtk::MediaFile>().ok())
    else {
        return;
    };
    if media.error().is_some() || media.is_playing() == playing {
        return;
    }
    // Before it is prepared the stream cannot start; the prepared handler starts it then.
    if playing && !media.is_prepared() {
        return;
    }
    if playing && media.is_ended() {
        media.seek(0);
    }
    media.set_playing(playing);
}

// Plays again once playback stopped by itself, as long as the video is still on screen. Deferred
// so that a pause requested by `set_video_playing` (which also hides the video) is not undone.
fn resume_if_visible(host: &glib::WeakRef<Picture>) {
    let host = host.clone();
    glib::idle_add_local_once(move || {
        if let Some(host) = host.upgrade().filter(|host| host.is_mapped()) {
            set_video_playing(&host, true);
        }
    });
}

fn file_stamp(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
}

/// A muted background video that loops without the stall `GtkMediaFile`'s own `loop` causes.
///
/// `loop = true` rewinds with a flushing seek: the decoder is flushed and re-prerolled while the
/// last frame stays on screen, a hitch (or a freeze, when the seek goes wrong) every time the
/// video wraps. Here two players share the same file instead. One plays while the other waits,
/// already rewound with its first frame decoded. When the playing one reaches the end the
/// picture switches to the waiting one and starts it, so the wrap costs one pipeline state change
/// and no seek on the visible stream. The finished player rewinds in the background, ready for
/// the next wrap. The second player is only created once the first one is prepared, so the first
/// frame is not delayed by it, and if it is ever not ready the video falls back to rewinding the
/// same player.
///
/// It only plays while the host is mapped (see `set_video_playing`), so it never renders frames
/// nobody can see, and a failed stream is rebuilt a few times before giving up.
pub(crate) struct LoopingVideo {
    host: glib::WeakRef<Picture>,
    path: PathBuf,
    stamp: Option<SystemTime>,
    retries: Cell<u8>,
    /// The player shown by the host.
    current: RefCell<Option<gtk::MediaFile>>,
    /// Rewound and waiting for the next wrap.
    standby: RefCell<Option<gtk::MediaFile>>,
}

impl LoopingVideo {
    /// Starts looping `path` on `host` (which must be a Picture that is kept on screen by the
    /// caller). The returned handle is only needed to stop the video before replacing it.
    pub(crate) fn attach(path: &Path, host: &Picture) -> Rc<Self> {
        let video = Rc::new(Self {
            host: host.downgrade(),
            path: path.to_owned(),
            stamp: file_stamp(path),
            retries: Cell::new(MEDIA_RETRIES),
            current: RefCell::new(None),
            standby: RefCell::new(None),
        });
        let media = video.new_media();
        host.set_paintable(Some(&media));
        *video.current.borrow_mut() = Some(media.clone());
        // The host keeps the player alive; both players go when the host is destroyed.
        let owner = video.clone();
        host.connect_destroy(move |_| owner.stop());
        if media.is_prepared() {
            video.on_prepared(&media);
        }
        video
    }

    /// Pauses and releases both players, and removes the video from the host.
    pub(crate) fn stop(&self) {
        let current = self.current.borrow_mut().take();
        let standby = self.standby.borrow_mut().take();
        if let Some(host) = self.host.upgrade() {
            if let (Some(shown), Some(current)) = (host.paintable(), current.as_ref()) {
                if shown == *current.upcast_ref::<gdk::Paintable>() {
                    host.set_paintable(None::<&gdk::Paintable>);
                }
            }
        }
        for media in current.into_iter().chain(standby) {
            media.set_playing(false);
        }
    }

    fn new_media(self: &Rc<Self>) -> gtk::MediaFile {
        let media = gtk::MediaFile::for_filename(&self.path);
        media.set_muted(true);
        // Looping is done by `on_ended`; the stream's own `loop` is the stall this avoids.
        media.set_loop(false);

        // The handlers only hold a weak reference: the host owns the player, not the streams.
        let weak: Weak<Self> = Rc::downgrade(self);
        media.connect_prepared_notify({
            let weak = weak.clone();
            move |media| {
                if let Some(video) = weak.upgrade() {
                    video.on_prepared(media);
                }
            }
        });
        media.connect_ended_notify({
            let weak = weak.clone();
            move |media| {
                if let Some(video) = weak.upgrade() {
                    video.on_ended(media);
                }
            }
        });
        media.connect_playing_notify({
            let weak = weak.clone();
            move |media| {
                if let Some(video) = weak.upgrade() {
                    video.on_paused(media);
                }
            }
        });
        media.connect_error_notify(move |media| {
            if let Some(video) = weak.upgrade() {
                video.on_error(media);
            }
        });
        media
    }

    fn is_current(&self, media: &gtk::MediaFile) -> bool {
        self.current.borrow().as_ref() == Some(media)
    }

    fn on_prepared(self: &Rc<Self>, media: &gtk::MediaFile) {
        if !media.is_prepared() || !self.is_current(media) {
            return;
        }
        if let Some(host) = self.host.upgrade() {
            set_video_playing(&host, host.is_mapped());
        }
        // The visible stream is ready: now get the one that will take over at the wrap.
        if self.standby.borrow().is_none() {
            *self.standby.borrow_mut() = Some(self.new_media());
        }
    }

    fn on_ended(self: &Rc<Self>, media: &gtk::MediaFile) {
        if !self.is_current(media) {
            return;
        }
        if !media.is_ended() {
            // The fallback rewind below landed: play on.
            resume_if_visible(&self.host);
            return;
        }
        let Some(host) = self.host.upgrade() else {
            return;
        };
        let ready = self
            .standby
            .borrow()
            .as_ref()
            .filter(|next| next.is_prepared() && next.error().is_none())
            .cloned();
        match ready {
            Some(next) => {
                host.set_paintable(Some(&next));
                let finished = self.current.borrow_mut().replace(next);
                set_video_playing(&host, host.is_mapped());
                // Rewind the finished stream while it is off screen, so it is ready next time.
                if let Some(finished) = finished {
                    finished.seek(0);
                    *self.standby.borrow_mut() = Some(finished);
                } else {
                    *self.standby.borrow_mut() = None;
                }
            }
            // The second player is not ready (yet): rewind this one instead.
            None => media.seek(0),
        }
    }

    // A stream that stopped without being asked to (the host is still on screen) starts again.
    fn on_paused(&self, media: &gtk::MediaFile) {
        if self.is_current(media)
            && !media.is_playing()
            && !media.is_ended()
            && media.is_prepared()
            && media.error().is_none()
        {
            resume_if_visible(&self.host);
        }
    }

    // A pipeline error would leave the background frozen: build the streams again.
    fn on_error(self: &Rc<Self>, media: &gtk::MediaFile) {
        if media.error().is_none() {
            return;
        }
        if !self.is_current(media) {
            // A broken standby is dropped; a fresh one is made when the current one is prepared.
            let mut standby = self.standby.borrow_mut();
            if standby.as_ref() == Some(media) {
                *standby = None;
            }
            return;
        }
        if self.retries.get() == 0 {
            return;
        }
        self.retries.set(self.retries.get() - 1);
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_secs(1), move || {
            if let Some(video) = weak.upgrade() {
                video.rebuild();
            }
        });
    }

    fn rebuild(self: &Rc<Self>) {
        let Some(host) = self.host.upgrade() else {
            return;
        };
        if let Some(old) = self.current.borrow_mut().take() {
            old.set_playing(false);
        }
        if let Some(old) = self.standby.borrow_mut().take() {
            old.set_playing(false);
        }
        let media = self.new_media();
        host.set_paintable(Some(&media));
        *self.current.borrow_mut() = Some(media.clone());
        if media.is_prepared() {
            self.on_prepared(&media);
        }
    }
}
