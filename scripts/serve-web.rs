#!/usr/bin/env cargo
---
[package]
edition = "2021"
---
//! Serves a folder over HTTP on this computer, for trying the web version (`scripts/serve-web.sh`):
//! the standard library only, a thread per connection. Every answer is cross-origin isolated
//! (`Cross-Origin-Opener-Policy` and `Cross-Origin-Embedder-Policy`), which the game's workers
//! need to share its memory, and is not cached, so a new build is what the next reload gets.
//! Only GET and HEAD, only files inside the folder; not meant for anything but `localhost`.
//!
//! Usage: serve-web.rs <folder> [<port>]

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};

fn main() {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().unwrap_or_else(|| "dist".into()));
    let port = args.next().unwrap_or_else(|| "8080".into());
    let listener = TcpListener::bind(format!("127.0.0.1:{port}")).unwrap_or_else(|e| panic!("port {port}: {e}"));
    println!("http://localhost:{port}/ serves {}", root.display());
    for stream in listener.incoming().flatten() {
        let root = root.clone();
        std::thread::spawn(move || {
            if let Err(error) = answer(stream, &root) {
                eprintln!("{error}");
            }
        });
    }
}

/// The type of a file by its name, as the browser needs it (a module must be JavaScript, a
/// WebAssembly module `application/wasm` to be compiled as it streams).
fn kind(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "wasm" => "application/wasm",
        "json" => "application/json",
        "txt" | "md" => "text/plain; charset=utf-8",
        "css" => "text/css",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// The file a request's path names inside `root`, `index.html` for a folder; `None` for a path
/// that climbs out of it.
fn file(root: &Path, target: &str) -> Option<PathBuf> {
    let path = target.split(['?', '#']).next().unwrap_or("/");
    let mut file = root.to_path_buf();
    for component in Path::new(path.trim_start_matches('/')).components() {
        match component {
            Component::Normal(part) => file.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if file.is_dir() {
        file.push("index.html");
    }
    Some(file)
}

fn answer(stream: TcpStream, root: &Path) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    // The headers are read and left: nothing in them changes the answer.
    let mut line = String::new();
    while reader.read_line(&mut line)? > 2 {
        line.clear();
    }
    let mut words = request.split_whitespace();
    let (method, target) = (words.next().unwrap_or(""), words.next().unwrap_or("/"));
    let found = file(root, target).and_then(|path| std::fs::read(&path).ok().map(|body| (path, body)));
    let (status, kind, body) = match (method, found) {
        ("GET" | "HEAD", Some((path, body))) => ("200 OK", kind(&path), body),
        ("GET" | "HEAD", None) => ("404 Not Found", "text/plain; charset=utf-8", b"not found\n".to_vec()),
        _ => ("405 Method Not Allowed", "text/plain; charset=utf-8", b"GET or HEAD\n".to_vec()),
    };
    if !status.starts_with("200") {
        println!("{method} {target}: {status}");
    }
    let mut stream = stream;
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCross-Origin-Opener-Policy: same-origin\r\n\
         Cross-Origin-Embedder-Policy: require-corp\r\nCross-Origin-Resource-Policy: same-origin\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    if method != "HEAD" {
        stream.write_all(&body)?;
    }
    stream.flush()
}
