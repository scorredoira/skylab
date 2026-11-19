//! The web page a game runs in (`wasm32` only): its canvas, the line that says how loading goes, the
//! game's files downloaded before it starts, and panics shown on the page.
//!
//! What the page must have (the game's `index.html`): a `<canvas id="game">` for the game to draw in,
//! and a `<div id="status">` over it with a `<p id="status-text">` and a `<progress id="status-bar">`,
//! hidden with `ready`. The page must be served cross-origin isolated (the headers
//! `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp`), as
//! the workers share the module's memory (`threads`), and next to `threads::WORKER_SCRIPT`.

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// The id of the canvas the game draws in.
pub const CANVAS: &str = "game";

fn document() -> Option<web_sys::Document> {
    web_sys::window()?.document()
}

/// The window's attributes for the page's canvas (a canvas of its own at the end of the page if it has none).
pub fn window_attributes(title: &str) -> winit::window::WindowAttributes {
    use winit::platform::web::WindowAttributesExtWebSys;
    let canvas = document().and_then(|d| d.get_element_by_id(CANVAS)).and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok());
    let append = canvas.is_none();
    // Keys and the wheel are the game's: the page neither scrolls nor opens its menu with them.
    winit::window::Window::default_attributes().with_title(title).with_canvas(canvas).with_append(append).with_prevent_default(true).with_focusable(true)
}

/// Says on the page how loading goes: `share` of it done (0 to 1), or `None` when that is not known.
pub fn say(text: &str, share: Option<f64>) {
    let Some(document) = document() else { return };
    if let Some(line) = document.get_element_by_id("status-text") {
        line.set_text_content(Some(text));
    }
    if let Some(bar) = document.get_element_by_id("status-bar") {
        match share {
            Some(share) => bar.set_attribute("value", &format!("{:.3}", share.clamp(0.0, 1.0))),
            // A progress without a value moves on its own: "busy".
            None => bar.remove_attribute("value"),
        }
        .ok();
    }
}

/// Hides what `say` showed: the game is drawing.
pub fn ready() {
    if let Some(status) = document().and_then(|d| d.get_element_by_id("status")) {
        status.set_attribute("hidden", "").ok();
    }
}

/// Shows panics on the page as well as in the browser's console: the game stops, and the player
/// should know why rather than look at a frozen picture.
pub fn show_panics() {
    std::panic::set_hook(Box::new(|info| {
        let text = info.to_string();
        web_sys::console::error_1(&text.clone().into());
        // A worker has no page: the console is all it has.
        if let Some(status) = document().and_then(|d| d.get_element_by_id("status")) {
            status.remove_attribute("hidden").ok();
            say(&format!("The game stopped on an error: {text}"), Some(0.0));
        }
    }));
}

fn describe(error: JsValue) -> String {
    error.as_string().or_else(|| error.dyn_ref::<js_sys::Error>().map(|e| e.message().into())).unwrap_or_else(|| format!("{error:?}"))
}

/// The bytes at `url` (relative to the page), with the browser's `fetch`.
pub async fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("no window")?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str(url)).await.map_err(describe)?.dyn_into().map_err(describe)?;
    if !response.ok() {
        return Err(format!("{url}: {} {}", response.status(), response.status_text()));
    }
    let buffer = JsFuture::from(response.array_buffer().map_err(describe)?).await.map_err(describe)?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

/// Downloads the files a manifest lists (a line per file: its size in bytes, a space, its path
/// relative to the page) and keeps them as the game's (`files::add`, then `files::seal`).
/// `progress(bytes_done, bytes_in_all)` after each file.
pub async fn download(manifest: &str, progress: impl Fn(u64, u64)) -> Result<(), String> {
    let list = String::from_utf8(fetch(manifest).await?).map_err(|e| format!("{manifest}: {e}"))?;
    let entries: Vec<(u64, &str)> = list.lines().filter_map(|line| line.split_once(' ')).filter_map(|(size, path)| Some((size.parse().ok()?, path.trim()))).collect();
    let total = entries.iter().map(|(size, _)| size).sum();
    // All asked for at once (the browser keeps a few connections busy), taken in the list's order.
    let window = web_sys::window().ok_or("no window")?;
    let asked: Vec<_> = entries.iter().map(|(_, path)| JsFuture::from(window.fetch_with_str(path))).collect();
    let mut done = 0;
    for ((size, path), answer) in entries.iter().zip(asked) {
        let response: web_sys::Response = answer.await.map_err(describe)?.dyn_into().map_err(describe)?;
        if !response.ok() {
            return Err(format!("{path}: {} {}", response.status(), response.status_text()));
        }
        let buffer = JsFuture::from(response.array_buffer().map_err(describe)?).await.map_err(describe)?;
        crate::files::add(path, js_sys::Uint8Array::new(&buffer).to_vec());
        done += size;
        progress(done, total);
    }
    crate::files::seal();
    Ok(())
}
