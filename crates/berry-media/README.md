# berry-media

Game artwork and images.

## Modules

- `backgrounds`: downloads and caches backgrounds, videos and overlays for each game (HoYoverse and Kuro sources).
- `screenshots`: game screenshot gallery data and refresh.
- `webview`: a headless WebKit page used to read the signed Wuthering Waves video URL.
- `images`: AVIF and WebP decoding, plus `gdk::Paintable` and `gdk::Texture` helpers (GTK).

## Dependencies

`berry-core`, `berry-games`, `berry-locale`, GTK 4 and WebKitGTK.
