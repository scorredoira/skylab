//! The cache's files (`fetch::cache_dir`), by path: on disk natively; on the web in the browser's
//! Cache Storage, through the page's service worker (the game's `web/sw.js`), which keeps what is
//! put at `cache/<key>` and gives it back. The web's requests are synchronous: the crate loads on
//! threads that wait, which on the web are workers, where a synchronous request is allowed. The
//! page's own thread must never wait, so there the store holds nothing (nothing is read, nothing kept).

use std::io;
use std::path::Path;

/// The whole file.
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    #[cfg(target_arch = "wasm32")]
    return web::read(path);
    #[cfg(not(target_arch = "wasm32"))]
    std::fs::read(path)
}

/// Its first `length` bytes, or fewer if it is shorter.
pub fn head(path: &Path, length: usize) -> io::Result<Vec<u8>> {
    #[cfg(target_arch = "wasm32")]
    return web::head(path, length);
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Read;
        let mut bytes = Vec::with_capacity(length);
        std::fs::File::open(path)?.take(length as u64).read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

/// Whether the file is there.
pub fn exists(path: &Path) -> bool {
    #[cfg(target_arch = "wasm32")]
    return web::exists(path);
    #[cfg(not(target_arch = "wasm32"))]
    path.exists()
}

/// Writes the file whole or not at all: natively under a temporary name, then renamed, so that an
/// interrupted write leaves nothing half written; its folder is made if there is none.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    #[cfg(target_arch = "wasm32")]
    return web::write(path, bytes);
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Unique to the process and the write: two threads may fetch the same file at once.
        static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let part = path.with_extension(format!("part{}-{}", std::process::id(), WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::write(&part, bytes).and_then(|_| std::fs::rename(&part, path))
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) mod web {
    use std::io;
    use std::path::Path;
    use wasm_bindgen::JsCast;
    use web_sys::{XmlHttpRequest, XmlHttpRequestResponseType};

    /// The page's own thread may not wait for a request (see the module).
    fn on_the_page() -> bool {
        js_sys::global().is_instance_of::<web_sys::Window>()
    }

    /// A synchronous request (on a worker; refused on the page's thread): the answer's status and
    /// body. `url` is absolute, or relative to the worker's script, which is beside the page.
    pub(crate) fn request(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>) -> Result<(u16, Vec<u8>), String> {
        if on_the_page() {
            return Err(format!("{url}: the page's thread may not wait for it"));
        }
        let failed = |e: wasm_bindgen::JsValue| format!("{url}: {}", e.dyn_ref::<js_sys::Error>().map_or_else(|| format!("{e:?}"), |e| String::from(e.message())));
        let xhr = XmlHttpRequest::new().map_err(failed)?;
        xhr.open_with_async(method, url, false).map_err(failed)?;
        xhr.set_response_type(XmlHttpRequestResponseType::Arraybuffer);
        xhr.set_timeout(60_000);
        for (name, value) in headers {
            xhr.set_request_header(name, value).map_err(failed)?;
        }
        match body {
            // Copied out of the module's memory, which is shared: the browser sends no shared bytes.
            Some(bytes) => xhr.send_with_opt_buffer_source(Some(&js_sys::Uint8Array::from(bytes))),
            None => xhr.send(),
        }
        .map_err(failed)?;
        let status = xhr.status().map_err(failed)?;
        let body = xhr.response().ok().filter(|r| r.is_instance_of::<js_sys::ArrayBuffer>()).map(|r| js_sys::Uint8Array::new(&r).to_vec()).unwrap_or_default();
        Ok((status, body))
    }

    fn file(method: &str, path: &Path, body: Option<&[u8]>) -> io::Result<(u16, Vec<u8>)> {
        request(method, &path.to_string_lossy(), &[], body).map_err(io::Error::other)
    }

    pub fn read(path: &Path) -> io::Result<Vec<u8>> {
        match file("GET", path, None)? {
            (200, body) => Ok(body),
            (404, _) => Err(io::Error::new(io::ErrorKind::NotFound, path.display().to_string())),
            (status, _) => Err(io::Error::other(format!("{}: {status}", path.display()))),
        }
    }

    pub fn head(path: &Path, length: usize) -> io::Result<Vec<u8>> {
        let range = format!("bytes=0-{}", length.max(1) - 1);
        match request("GET", &path.to_string_lossy(), &[("Range", &range)], None).map_err(io::Error::other)? {
            (200 | 206, mut body) => {
                body.truncate(length);
                Ok(body)
            }
            (404, _) => Err(io::Error::new(io::ErrorKind::NotFound, path.display().to_string())),
            (status, _) => Err(io::Error::other(format!("{}: {status}", path.display()))),
        }
    }

    pub fn exists(path: &Path) -> bool {
        matches!(file("HEAD", path, None), Ok((200, _)))
    }

    pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        match file("PUT", path, Some(bytes))?.0 {
            200..=299 => Ok(()),
            status => Err(io::Error::other(format!("{}: {status}", path.display()))),
        }
    }
}
