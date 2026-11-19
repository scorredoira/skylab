//! Downloads kept on disk: each file is fetched once, then read from the cache for ever (delete the
//! folder to fetch again). The only code of the project that touches the network.
//!
//! A file is kept as the server sent it, written whole or not at all (`disk::write`). The requests
//! are polite: a User-Agent that names the program and nothing about the person, and a pause
//! between the starts of two requests to the same server (`Turns`).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use crate::{disk, Error};
use http::Http;

/// The folder the downloads are kept in: `$FLIGHT_CACHE`, or the user's cache folder
/// (`~/Library/Caches/flight` on macOS, `$XDG_CACHE_HOME/flight` or `~/.cache/flight` elsewhere).
/// On the web, `cache`: what the page's service worker keeps in the browser (`disk`).
pub fn cache_dir() -> PathBuf {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    if cfg!(target_arch = "wasm32") {
        PathBuf::from("cache")
    } else if let Some(dir) = var("FLIGHT_CACHE") {
        dir
    } else if cfg!(target_os = "macos") {
        var("HOME").unwrap_or_else(std::env::temp_dir).join("Library/Caches/flight")
    } else if let Some(dir) = var("XDG_CACHE_HOME") {
        dir.join("flight")
    } else {
        var("HOME").unwrap_or_else(std::env::temp_dir).join(".cache/flight")
    }
}

pub struct Fetcher {
    http: Http,
    root: PathBuf,
    /// Nothing is asked of the network: only what the cache has.
    offline: bool,
    /// The keys whose servers answered that there is no such file, not asked for again.
    absent: Mutex<HashSet<String>>,
    /// Bytes that came from the network (not from the cache).
    downloaded: AtomicU64,
}

impl Fetcher {
    pub fn new(root: PathBuf, offline: bool) -> Fetcher {
        Fetcher { http: Http::new(), root, offline, absent: Mutex::default(), downloaded: AtomicU64::new(0) }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Bytes downloaded so far.
    pub fn downloaded(&self) -> u64 {
        self.downloaded.load(Ordering::Relaxed)
    }

    /// The file kept at `<root>/<key>`, fetched from `url` the first time. `Ok(None)` when the server
    /// says there is nothing there (a tile of open sea, for some servers).
    pub fn cached(&self, key: &str, url: &str) -> Result<Option<Vec<u8>>, Error> {
        self.kept_or(key, || self.http.get(url))
    }

    /// Bytes `start..start + length` of the file at `url` (an HTTP range request), kept at
    /// `<root>/<key>` as a whole file is: a part of a Cloud-Optimized GeoTIFF (`tiff`). An error
    /// when the server has no such file.
    pub fn cached_range(&self, key: &str, url: &str, start: u64, length: u64) -> Result<Vec<u8>, Error> {
        match self.kept_or(key, || self.http.get_range(url, start, length))? {
            Some(bytes) => Ok(bytes),
            None => Err(Error::Http(format!("{url} ({start}+{length})"))),
        }
    }

    /// The file at `url`, never kept: what may change from one build to the next (the TileJSON that
    /// names the planet in use). Offline it is an `Error::Offline`.
    pub fn get(&self, url: &str) -> Result<Option<Vec<u8>>, Error> {
        self.must_be_online(url)?;
        let body = self.http.get(url)?;
        if let Some(bytes) = &body {
            self.count(bytes);
        }
        Ok(body)
    }

    /// The answer to a query sent as JSON (a STAC catalogue's search), never kept: a search is
    /// asked again each time a place is built. Offline it is an `Error::Offline`.
    pub fn query(&self, url: &str, json: &str) -> Result<Vec<u8>, Error> {
        self.must_be_online(url)?;
        let body = self.http.post(url, json)?;
        self.count(&body);
        Ok(body)
    }

    fn must_be_online(&self, what: &str) -> Result<(), Error> {
        match self.offline {
            true => Err(Error::Offline(what.to_string())),
            false => Ok(()),
        }
    }

    fn count(&self, bytes: &[u8]) {
        self.downloaded.fetch_add(bytes.len() as u64, Ordering::Relaxed);
    }

    fn absent(&self) -> std::sync::MutexGuard<'_, HashSet<String>> {
        self.absent.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// What the cache keeps at `key` (an empty file counts as none), else what `download` brings,
    /// which is then kept.
    fn kept_or(&self, key: &str, download: impl FnOnce() -> Result<Option<Vec<u8>>, Error>) -> Result<Option<Vec<u8>>, Error> {
        let path = self.root.join(key);
        match disk::read(&path) {
            Ok(bytes) if !bytes.is_empty() => return Ok(Some(bytes)),
            _ => {}
        }
        if self.absent().contains(key) {
            return Ok(None);
        }
        self.must_be_online(key)?;
        match download()? {
            None => {
                self.absent().insert(key.to_string());
                Ok(None)
            }
            Some(bytes) => {
                self.count(&bytes);
                disk::write(&path, &bytes).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
                Ok(Some(bytes))
            }
        }
    }
}

/// The least time between the starts of two requests to the same server.
const GAP: Duration = Duration::from_millis(40);

/// When each server was last asked something, to keep `GAP` between two requests to it. The clock
/// is the platform's, in milliseconds (`http::now`).
#[derive(Default)]
struct Turns {
    last: Mutex<HashMap<String, f64>>,
}

impl Turns {
    /// Returns once `GAP` has passed since the last request to the server of `url` started, the
    /// start of this one noted.
    fn wait(&self, url: &str) {
        // `scheme://host/...`: the host is what comes between the second and third slashes.
        let host = url.split('/').nth(2).unwrap_or_default();
        let gap = GAP.as_secs_f64() * 1000.0;
        loop {
            let pause = {
                let mut last = self.last.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                let now = http::now();
                let ready = last.get(host).map_or(now, |&t| t + gap);
                if ready <= now {
                    last.insert(host.to_string(), now);
                    return;
                }
                ready - now
            };
            std::thread::sleep(Duration::from_secs_f64(pause / 1000.0));
        }
    }
}

/// The header asking for bytes `start..start + length` (an empty length asks for one byte).
fn range_header(start: u64, length: u64) -> String {
    format!("bytes={start}-{}", start + length.max(1) - 1)
}

/// The part asked for of a whole body, sent by a server that ignored the range.
fn part_of(body: &[u8], start: u64, length: u64) -> Vec<u8> {
    let span = usize::try_from(start).ok().zip(usize::try_from(start.saturating_add(length)).ok());
    span.and_then(|(a, b)| body.get(a..b)).map(<[u8]>::to_vec).unwrap_or_default()
}

/// The network natively: blocking HTTPS with ureq.
#[cfg(not(target_arch = "wasm32"))]
mod http {
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    use super::{part_of, range_header, Turns};
    use crate::Error;

    /// What every request says it comes from: the program, never the person.
    pub(super) const USER_AGENT: &str = concat!("flight/", env!("CARGO_PKG_VERSION"), " (open-source flight simulator)");
    /// The most a body may weigh: a file is far less, a query's answer less still.
    const MOST: u64 = 256 << 20;
    const MOST_ANSWER: u64 = 64 << 20;

    /// Milliseconds since the first time this was asked.
    pub(super) fn now() -> f64 {
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
    }

    pub(super) struct Http {
        agent: ureq::Agent,
        turns: Turns,
    }

    fn failed(url: &str, e: impl std::fmt::Display) -> Error {
        Error::Http(format!("{url}: {e}"))
    }

    fn body(url: &str, response: &mut ureq::http::Response<ureq::Body>, most: u64) -> Result<Vec<u8>, Error> {
        response.body_mut().with_config().limit(most).read_to_vec().map_err(|e| failed(url, e))
    }

    impl Http {
        pub(super) fn new() -> Http {
            let config = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(60))).user_agent(USER_AGENT).build();
            Http { agent: config.into(), turns: Turns::default() }
        }

        /// The body at `url`; `None` when the server says there is nothing there (404 or 204).
        pub(super) fn get(&self, url: &str) -> Result<Option<Vec<u8>>, Error> {
            self.turns.wait(url);
            match self.agent.get(url).call() {
                Err(ureq::Error::StatusCode(404 | 204)) => Ok(None),
                Err(e) => Err(failed(url, e)),
                Ok(mut response) => body(url, &mut response, MOST).map(Some),
            }
        }

        /// The answer to a JSON body posted to `url`.
        pub(super) fn post(&self, url: &str, json: &str) -> Result<Vec<u8>, Error> {
            self.turns.wait(url);
            let mut response = self.agent.post(url).header("Content-Type", "application/json").send(json).map_err(|e| failed(url, e))?;
            body(url, &mut response, MOST_ANSWER)
        }

        /// Bytes `start..start + length` of the body at `url`; `None` when there is no such file.
        pub(super) fn get_range(&self, url: &str, start: u64, length: u64) -> Result<Option<Vec<u8>>, Error> {
            self.turns.wait(url);
            match self.agent.get(url).header("Range", &range_header(start, length)).call() {
                Err(ureq::Error::StatusCode(404)) => Ok(None),
                Err(e) => Err(failed(url, e)),
                Ok(mut response) => {
                    let partial = response.status() == 206;
                    let whole = body(url, &mut response, MOST)?;
                    Ok(Some(if partial { whole } else { part_of(&whole, start, length) }))
                }
            }
        }
    }
}

/// The network on the web: the browser's synchronous requests, made on the workers this crate
/// loads on (`disk::web::request`). Every source answers a page from elsewhere (CORS). The browser
/// sets the User-Agent itself.
#[cfg(target_arch = "wasm32")]
mod http {
    use super::{part_of, range_header, Turns};
    use crate::disk::web::request;
    use crate::Error;

    /// The browser's clock, in milliseconds.
    pub(super) fn now() -> f64 {
        js_sys::Date::now()
    }

    pub(super) struct Http {
        turns: Turns,
    }

    fn status(url: &str, code: u16) -> Error {
        Error::Http(format!("{url}: {code}"))
    }

    impl Http {
        pub(super) fn new() -> Http {
            Http { turns: Turns::default() }
        }

        /// The body at `url`; `None` when the server says there is nothing there (404 or 204).
        pub(super) fn get(&self, url: &str) -> Result<Option<Vec<u8>>, Error> {
            self.turns.wait(url);
            let (code, body) = request("GET", url, &[], None).map_err(Error::Http)?;
            match code {
                404 | 204 => Ok(None),
                200..=299 => Ok(Some(body)),
                _ => Err(status(url, code)),
            }
        }

        /// The answer to a JSON body posted to `url`.
        pub(super) fn post(&self, url: &str, json: &str) -> Result<Vec<u8>, Error> {
            self.turns.wait(url);
            let (code, body) = request("POST", url, &[("Content-Type", "application/json")], Some(json.as_bytes())).map_err(Error::Http)?;
            match code {
                200..=299 => Ok(body),
                _ => Err(status(url, code)),
            }
        }

        /// Bytes `start..start + length` of the body at `url`; `None` when there is no such file.
        pub(super) fn get_range(&self, url: &str, start: u64, length: u64) -> Result<Option<Vec<u8>>, Error> {
            self.turns.wait(url);
            let (code, body) = request("GET", url, &[("Range", &range_header(start, length))], None).map_err(Error::Http)?;
            match code {
                206 => Ok(Some(body)),
                200 => Ok(Some(part_of(&body, start, length))),
                404 => Ok(None),
                _ => Err(status(url, code)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_reads_only_the_cache() {
        let root = std::env::temp_dir().join(format!("flight-fetch-test-{}", std::process::id()));
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::write(root.join("a/b.bin"), [1, 2, 3]).unwrap();
        std::fs::write(root.join("a/empty.bin"), []).unwrap();
        let f = Fetcher::new(root.clone(), true);
        assert_eq!(f.cached("a/b.bin", "https://example.invalid/b").unwrap(), Some(vec![1, 2, 3]));
        assert!(matches!(f.cached("a/c.bin", "https://example.invalid/c"), Err(Error::Offline(_))));
        assert!(matches!(f.cached("a/empty.bin", "https://example.invalid/e"), Err(Error::Offline(_))), "an empty file is not kept");
        assert!(matches!(f.get("https://example.invalid/d"), Err(Error::Offline(_))));
        assert!(matches!(f.query("https://example.invalid/q", "{}"), Err(Error::Offline(_))));
        assert_eq!(f.cached_range("a/b.bin", "https://example.invalid/b", 0, 3).unwrap(), vec![1, 2, 3]);
        assert_eq!(f.downloaded(), 0);
        std::fs::remove_dir_all(root).unwrap();
        assert!(http::USER_AGENT.starts_with("flight/") && !http::USER_AGENT.contains('@'));
    }

    #[test]
    fn ranges_are_inclusive_and_a_whole_body_is_cut() {
        assert_eq!(range_header(0, 16), "bytes=0-15");
        assert_eq!(range_header(100, 0), "bytes=100-100");
        let body: Vec<u8> = (0..10).collect();
        assert_eq!(part_of(&body, 2, 3), vec![2, 3, 4]);
        assert_eq!(part_of(&body, 8, 5), Vec::<u8>::new());
        assert_eq!(part_of(&body, u64::MAX, 2), Vec::<u8>::new());
    }

    #[test]
    fn turns_keep_a_gap_per_server() {
        let turns = Turns::default();
        let start = std::time::Instant::now();
        turns.wait("https://a.example/1");
        turns.wait("https://b.example/1");
        assert_eq!(turns.last.lock().unwrap().len(), 2, "one turn for each server");
        turns.wait("https://a.example/2");
        assert!(start.elapsed() >= GAP - Duration::from_millis(1));
    }
}
