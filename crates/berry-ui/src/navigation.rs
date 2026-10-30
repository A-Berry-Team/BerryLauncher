use adw::prelude::*;

pub(crate) fn restore_header_for_root(
    navigation: &adw::NavigationView,
    header: &adw::HeaderBar,
    root_tag: &'static str,
) {
    let header = header.clone();
    navigation.connect_visible_page_notify(move |navigation| {
        let at_root = navigation
            .visible_page()
            .and_then(|page| page.tag())
            .as_deref()
            == Some(root_tag);
        header.set_visible(at_root);
    });
}
