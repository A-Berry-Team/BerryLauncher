use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

const CATALOG_RESOURCE_BASE: &str = "/app/berrylauncher/berry/resources/catalog";

const EXTENSIONS: &[&str] = &[
    "webp", "png", "jpg", "jpeg", "avif", "gif", "bmp", "tif", "tiff", "svg",
];

/// Largest image (in pixels) that is decoded; a bigger one is refused instead of allocating
/// gigabytes for a crafted file.
const MAX_IMAGE_PIXELS: u64 = 64_000_000;

thread_local! {
    static CACHE: RefCell<HashMap<String, glib::WeakRef<gdk::Paintable>>> = RefCell::new(
        HashMap::new(),
    );
}

fn resource_exists(path: &str) -> bool {
    gio::resources_get_info(path, gio::ResourceLookupFlags::NONE).is_ok()
}

fn strip_known_extension(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, ext))
            if EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known)) =>
        {
            stem
        }
        _ => name,
    }
}

pub fn find_catalog_resource(name: &str) -> Option<String> {
    let stem = strip_known_extension(name);
    EXTENSIONS
        .iter()
        .map(|ext| format!("{CATALOG_RESOURCE_BASE}/{stem}.{ext}"))
        .find(|path| resource_exists(path))
}

pub fn catalog_paintable(name: &str) -> Option<gdk::Paintable> {
    if let Some(cached) =
        CACHE.with(|cache| cache.borrow().get(name).and_then(|weak| weak.upgrade()))
    {
        return Some(cached);
    }

    let Some(path) = find_catalog_resource(name) else {
        tracing::warn!("Catalog image '{name}' not found in the bundled resources");
        return None;
    };
    let paintable = if path.ends_with(".svg") {
        gtk::Picture::for_resource(&path).paintable()
    } else {
        match gio::resources_lookup_data(&path, gio::ResourceLookupFlags::NONE) {
            Ok(bytes) => texture_from_bytes(&bytes, &path).map(|texture| texture.upcast()),
            Err(error) => {
                tracing::warn!("Could not read resource {path}: {error}");
                None
            }
        }
    };

    if let Some(paintable) = &paintable {
        CACHE.with(|cache| {
            cache
                .borrow_mut()
                .insert(name.to_owned(), paintable.downgrade())
        });
    } else {
        tracing::warn!("Could not decode catalog image {path}");
    }
    paintable
}

pub fn texture_from_file(path: &Path) -> Option<gdk::Texture> {
    match std::fs::read(path) {
        Ok(bytes) => {
            texture_from_bytes(&glib::Bytes::from_owned(bytes), &path.display().to_string())
        }
        Err(error) => {
            tracing::warn!("Could not read image {}: {error}", path.display());
            None
        }
    }
}

pub fn texture_from_bytes(bytes: &glib::Bytes, label: &str) -> Option<gdk::Texture> {
    if is_webp(bytes) {
        return decode_webp(bytes, label);
    }

    if is_avif(bytes) {
        return decode_avif(bytes, label);
    }

    match gdk::Texture::from_bytes(bytes) {
        Ok(texture) => return Some(texture),
        Err(error) => {
            tracing::debug!("GdkTexture could not decode {label}: {error}; trying GdkPixbuf",)
        }
    }

    let stream = gio::MemoryInputStream::from_bytes(bytes);
    match gdk_pixbuf::Pixbuf::from_stream(&stream, gio::Cancellable::NONE) {
        Ok(pixbuf) => Some(gdk::Texture::for_pixbuf(&pixbuf)),
        Err(error) => {
            tracing::warn!("Could not decode image {label}: {error}");
            None
        }
    }
}

/// Pixel size of a WebP image, read from its header without decoding it.
pub fn webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if !is_webp(bytes) {
        return None;
    }
    let (mut width, mut height) = (0i32, 0i32);
    // SAFETY: the pointer and length come from a live slice; the call only reads the header.
    let ok =
        unsafe { libwebp_sys::WebPGetInfo(bytes.as_ptr(), bytes.len(), &mut width, &mut height) };
    (ok != 0 && width > 0 && height > 0).then_some((width as u32, height as u32))
}

fn is_webp(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
}

fn is_avif(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[4..8] == b"ftyp" && matches!(&bytes[8..12], b"avif" | b"avis")
}

fn decode_avif(bytes: &[u8], label: &str) -> Option<gdk::Texture> {
    let image = match image::load_from_memory_with_format(bytes, image::ImageFormat::Avif) {
        Ok(image) => image.into_rgba8(),
        Err(error) => {
            tracing::warn!("Could not decode {label}: AVIF decoder failed: {error}");
            return None;
        }
    };
    let (width, height) = image.dimensions();
    Some(
        gdk::MemoryTexture::new(
            width as i32,
            height as i32,
            gdk::MemoryFormat::R8g8b8a8,
            &glib::Bytes::from_owned(image.into_raw()),
            width as usize * 4,
        )
        .upcast(),
    )
}

/// RGBA pixels allocated by libwebp. They are handed to GDK as they are and released with
/// `WebPFree` once the last reference to the `glib::Bytes` wrapping them is gone, so a decoded
/// image is never copied.
struct WebpPixels {
    ptr: *mut u8,
    len: usize,
}

// SAFETY: the buffer is exclusively owned by this value and is only ever read.
unsafe impl Send for WebpPixels {}

impl AsRef<[u8]> for WebpPixels {
    fn as_ref(&self) -> &[u8] {
        // SAFETY: `ptr` points to `len` initialised bytes that live until `drop`.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl Drop for WebpPixels {
    fn drop(&mut self) {
        // SAFETY: `ptr` was returned by `WebPDecodeRGBA` and is freed exactly once.
        unsafe { libwebp_sys::WebPFree(self.ptr.cast::<std::ffi::c_void>()) }
    }
}

fn decode_webp(bytes: &[u8], label: &str) -> Option<gdk::Texture> {
    let Some((header_width, header_height)) = webp_dimensions(bytes) else {
        tracing::warn!("Could not decode {label}: invalid WebP dimensions");
        return None;
    };
    if u64::from(header_width) * u64::from(header_height) > MAX_IMAGE_PIXELS {
        tracing::warn!("Could not decode {label}: image is too large");
        return None;
    }

    let (mut width, mut height) = (0i32, 0i32);
    // SAFETY: the pointer and length come from a live slice; libwebp allocates the result.
    let decoded = unsafe {
        libwebp_sys::WebPDecodeRGBA(bytes.as_ptr(), bytes.len(), &mut width, &mut height)
    };
    if decoded.is_null() {
        tracing::warn!("Could not decode {label}: WebP decoder failed");
        return None;
    }

    let size = (width as usize)
        .checked_mul(height as usize)
        .and_then(|size| size.checked_mul(4));
    let pixels = WebpPixels {
        ptr: decoded,
        len: size.unwrap_or(0),
    };
    if size.is_none() || width <= 0 || height <= 0 {
        tracing::warn!("Could not decode {label}: image dimensions overflow");
        return None;
    }

    Some(
        gdk::MemoryTexture::new(
            width,
            height,
            gdk::MemoryFormat::R8g8b8a8,
            &glib::Bytes::from_owned(pixels),
            width as usize * 4,
        )
        .upcast(),
    )
}
