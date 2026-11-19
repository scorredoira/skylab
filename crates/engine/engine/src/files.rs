//! Files, the same on every platform. The game's own (textures): read from disk natively; on the
//! web from those the page downloaded before the game started (`add`, then `seal`), by the same
//! relative paths (`assets/textures/sand_color.jpg`). The player's (settings, controller mappings):
//! small texts, kept on disk natively and in the browser's `localStorage` on the web, by path.

use std::io;
use std::path::Path;

/// The bytes of one of the game's files.
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    #[cfg(target_arch = "wasm32")]
    return web::read(path);
    #[cfg(not(target_arch = "wasm32"))]
    std::fs::read(path)
}

/// Whether the game has this file.
pub fn exists(path: &Path) -> bool {
    #[cfg(target_arch = "wasm32")]
    return web::read(path).is_ok();
    #[cfg(not(target_arch = "wasm32"))]
    path.is_file()
}

/// A text of the player's, if it was ever written.
pub fn load_text(path: &Path) -> io::Result<String> {
    #[cfg(target_arch = "wasm32")]
    return web::load_text(path);
    #[cfg(not(target_arch = "wasm32"))]
    std::fs::read_to_string(path)
}

/// Keeps a text of the player's, making its folder if there is none.
pub fn save_text(path: &Path, text: &str) -> io::Result<()> {
    #[cfg(target_arch = "wasm32")]
    return web::save_text(path, text);
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, text)
    }
}

#[cfg(target_arch = "wasm32")]
pub use web::{add, seal};

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::io;
    use std::path::Path;
    use std::sync::OnceLock;

    thread_local! {
        /// The files being downloaded, on the page's thread, until `seal`.
        static ADDED: RefCell<HashMap<String, Vec<u8>>> = RefCell::new(HashMap::new());
    }
    /// The files once all are there: read from any thread without a lock.
    static SEALED: OnceLock<HashMap<String, Vec<u8>>> = OnceLock::new();

    fn key(path: &Path) -> String {
        path.to_string_lossy().trim_start_matches("./").to_string()
    }

    /// Keeps a file the page downloaded, under its path.
    pub fn add(path: &str, bytes: Vec<u8>) {
        ADDED.with_borrow_mut(|added| added.insert(key(Path::new(path)), bytes));
    }

    /// Makes the files added so far the game's, readable from every thread. Once: later `add`s are lost.
    pub fn seal() {
        let _ = SEALED.set(ADDED.take());
    }

    pub fn read(path: &Path) -> io::Result<Vec<u8>> {
        SEALED.get().and_then(|files| files.get(&key(path))).cloned().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{} was not downloaded", path.display())))
    }

    fn storage() -> io::Result<web_sys::Storage> {
        web_sys::window().and_then(|w| w.local_storage().ok().flatten()).ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "the browser keeps nothing (localStorage)"))
    }

    pub fn load_text(path: &Path) -> io::Result<String> {
        storage()?.get_item(&key(path)).ok().flatten().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.display().to_string()))
    }

    pub fn save_text(path: &Path, text: &str) -> io::Result<()> {
        storage()?.set_item(&key(path), text).map_err(|_| io::Error::new(io::ErrorKind::StorageFull, format!("{}: the browser's storage is full", path.display())))
    }
}
