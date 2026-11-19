//! The application's icon: a PNG the game hands over once, before `run`. It goes on the window
//! (title bar and taskbar on Windows and Linux, through winit) and, on macOS, where windows have no
//! icon, on the Dock while the game runs; a `.app` bundle has its own (`scripts/bundle-macos.sh`).

use std::sync::OnceLock;

static PNG: OnceLock<&'static [u8]> = OnceLock::new();

/// Sets the icon, a square PNG (512 px is plenty). Call it before `run`; later calls are ignored.
pub fn set_icon(png: &'static [u8]) {
    let _ = PNG.set(png);
}

/// The icon for winit's window attributes; `None` without one or if the PNG does not decode.
/// macOS ignores it (`dock`).
pub(crate) fn window_icon() -> Option<winit::window::Icon> {
    let png = PNG.get()?;
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    winit::window::Icon::from_rgba(image.into_raw(), width, height).ok()
}

/// Puts the icon on the Dock. On the main thread, once the application exists (winit's `resumed`).
#[cfg(target_os = "macos")]
pub(crate) fn dock() {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;
    let (Some(png), Some(main)) = (PNG.get(), MainThreadMarker::new()) else { return };
    let data = NSData::with_bytes(png);
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
        // SAFETY: an image made just above, set on the main thread.
        unsafe { NSApplication::sharedApplication(main).setApplicationIconImage(Some(&image)) };
    }
}
