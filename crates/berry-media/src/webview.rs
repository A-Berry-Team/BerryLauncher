use anyhow::{bail, Result};
use gtk::glib;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use webkit6::prelude::*;
use webkit6::{HardwareAccelerationPolicy, NetworkSession, WebView};

const POLL: Duration = Duration::from_millis(500);
const TIMEOUT: Duration = Duration::from_secs(45);

// Looks for a signed `<file_name>` URL where the page's scripts may have put it: the video
// element, its sources, or any resource the page requested.
fn finder_script(file_name: &str) -> String {
    format!(
        r#"(() => {{
            const wanted = u => typeof u === 'string' && u.includes('{file_name}') && u.includes('auth_key=');
            const urls = [...document.querySelectorAll('video, video source')]
                .flatMap(node => [node.currentSrc, node.src, node.getAttribute('src')]);
            urls.push(...performance.getEntriesByType('resource').map(entry => entry.name));
            const url = urls.find(wanted);
            return url ? new URL(url, location.href).href : '';
        }})()"#
    )
}

async fn probe(page_url: &str, script: &str) -> Option<String> {
    // Ephemeral session: nothing from the page is written to disk.
    let session = NetworkSession::new_ephemeral();
    let view = WebView::builder().network_session(&session).build();
    if let Some(settings) = WebViewExt::settings(&view) {
        settings.set_enable_media(false);
        settings.set_enable_webaudio(false);
        settings.set_enable_webgl(false);
        settings.set_enable_page_cache(false);
        settings.set_hardware_acceleration_policy(HardwareAccelerationPolicy::Never);
    }
    view.set_size_request(1280, 720);
    view.load_uri(page_url);

    let deadline = Instant::now() + TIMEOUT;
    let mut found = None;
    while Instant::now() < deadline {
        glib::timeout_future(POLL).await;
        if let Ok(value) = view.evaluate_javascript_future(script, None, None).await {
            let text = value.to_str();
            if !text.is_empty() {
                found = Some(text.to_string());
                break;
            }
        }
    }
    view.terminate_web_process();
    found
}

// Loads `page_url` in a hidden WebKit view and returns the signed URL of `file_name` once the page
// scripts have produced it. Blocks the calling thread; the view itself lives on the main thread.
pub fn find_signed_url(page_url: &str, file_name: &str) -> Result<String> {
    let (sender, receiver) = mpsc::channel();
    let page_url = page_url.to_owned();
    let script = finder_script(file_name);
    glib::MainContext::default().invoke(move || {
        glib::spawn_future_local(async move {
            let _ = sender.send(probe(&page_url, &script).await);
        });
    });
    match receiver.recv_timeout(TIMEOUT + Duration::from_secs(15)) {
        Ok(Some(url)) => Ok(url),
        Ok(None) => bail!("the page did not produce a signed {file_name} URL within {TIMEOUT:?}"),
        Err(_) => bail!("the page probe did not answer"),
    }
}
