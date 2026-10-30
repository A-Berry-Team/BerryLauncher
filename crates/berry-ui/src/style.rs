pub(crate) fn install_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        ".berry-image { border-radius: 12px; } \
         .berry-image-frame { border-radius: 12px; } \
         .berry-library-card { border-radius: 18px; } \
         .berry-store-card { border-radius: 18px; } \
         .berry-library-logo-frame { border-radius: 24px; } \
         .berry-library-logo { border-radius: 24px; } \
         .berry-library-promo { border-radius: 20px; } \
         .berry-library-controls { padding: 0; } \
         .berry-tag { background-color: alpha(@accent_bg_color, 0.2); color: @accent_color; \
             border-radius: 999px; padding: 7px 18px 7px 14px; font-weight: 600; } \
         .berry-runner-download { color: @success_color; } \
         .berry-runner-download:hover { color: @success_color; } \
         .berry-library-back-button { background-color: rgba(0, 0, 0, 0.55); color: white; } \
         .berry-library-back-button:hover { background-color: rgba(0, 0, 0, 0.72); } \
         .berry-library-close-button { background-color: rgba(0, 0, 0, 0.55); color: white; } \
         .berry-library-close-button:hover { background-color: rgba(0, 0, 0, 0.72); } \
         .berry-library-info-button { background-color: rgba(0, 0, 0, 0.55); color: white; } \
         .berry-library-info-button:hover { background-color: rgba(0, 0, 0, 0.72); } \
         .berry-library-progress-box { background-color: rgba(0, 0, 0, 0.55); \
             border-radius: 8px; padding: 8px 14px; } \
         .berry-progress-label { color: white; font-size: 0.9em; } \
         .berry-library-progress-box progressbar { min-height: 6px; } \
         .berry-library-progress-box progressbar text { color: white; font-weight: 600; } \
         .berry-card-media { border-radius: 16px; } \
         .berry-card-image, .berry-game-cover, .berry-screenshot { border-radius: 16px; } \
         .berry-card-locked { filter: grayscale(1); opacity: 0.85; } \
         .berry-theme-choice { border-radius: 999px; min-width: 44px; min-height: 44px; \
             padding: 0; border: 2px solid alpha(@window_fg_color, 0.18); box-shadow: none; } \
         .berry-theme-choice:checked { border: 3px solid @accent_color; } \
         .berry-theme-system { background: linear-gradient(135deg, #f2f2f2 0%, #f2f2f2 50%, \
             #303030 50%, #303030 100%); } \
         .berry-theme-light { background: #f2f2f2; } \
         .berry-theme-dark { background: #303030; }",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
