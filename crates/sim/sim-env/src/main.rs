//! `flight-env`, the binary of `sim-env`: serves the training environment to one client at a time over a local socket.
//! The protocol is in `PROTOCOL.md`.

use std::net::{Ipv4Addr, TcpListener};

use sim_env::json::Json;
use sim_env::server::{serve, Settings};

const USAGE: &str = "flight-env: the flight training environment, served over a local socket

  --port N            listen on 127.0.0.1:N (default 7447)
  --unix PATH         listen on a Unix-domain socket instead
  --once              exit when the first client leaves

Defaults for a session (a client can change any of them in its hello):
  --task NAME         hold, takeoff, landing, circuit, race (an aircraft)
                      or hover, gates (a multirotor) or drive (a car round a circuit) (default hold)
  --aircraft NAME     c172 or extra (default c172)
  --multirotor NAME   fpv5 (default fpv5), flown in the tasks hover and gates
  --num-envs N        environments stepped together (default 64)
  --threads N         worker threads (default: one per performance core)
  --world-seed N      seed of the terrain (default 1)
  --decision-dt S     seconds between decisions (default 0.1)
  --action-mode M     incremental or direct (default incremental)
  --flaps             add a flap action
  --trim              add an elevator trim action
  --set NAME=VALUE    a number of the task, e.g. --set air.wind_max=5 (repeatable)
";

fn fail(message: &str) -> ! {
    eprintln!("flight-env: {message}");
    std::process::exit(2);
}

fn main() {
    let mut settings = Settings::default();
    let mut port: u16 = 7447;
    let mut unix: Option<String> = None;
    let mut once = false;
    let mut params: Vec<(String, f64)> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .unwrap_or_else(|| fail(&format!("{flag} needs a value")))
        };
        match flag.as_str() {
            "--help" | "-h" => {
                print!("{USAGE}");
                return;
            }
            "--once" => once = true,
            "--port" => port = value().parse().unwrap_or_else(|_| fail("bad --port")),
            "--unix" => unix = Some(value()),
            "--flaps" | "--trim" => settings
                .set(&flag[2..], &Json::Bool(true))
                .unwrap_or_else(|e| fail(&e)),
            "--task" | "--action-mode" | "--aircraft" | "--multirotor" => settings
                .set(&flag[2..].replace('-', "_"), &Json::Str(value()))
                .unwrap_or_else(|e| fail(&e)),
            "--num-envs" | "--threads" | "--world-seed" | "--decision-dt" => {
                let number: f64 = value()
                    .parse()
                    .unwrap_or_else(|_| fail(&format!("bad {flag}")));
                settings
                    .set(&flag[2..].replace('-', "_"), &Json::Num(number))
                    .unwrap_or_else(|e| fail(&e));
            }
            "--set" => {
                let pair = value();
                let parsed = pair
                    .split_once('=')
                    .and_then(|(k, v)| Some((k.to_string(), v.parse::<f64>().ok()?)));
                params.push(parsed.unwrap_or_else(|| fail("--set takes NAME=VALUE")));
            }
            other => fail(&format!("unknown option {other} (see --help)")),
        }
    }
    // After --task, wherever it was written.
    for (name, value) in params {
        if !settings.env.task.set(&name, value) {
            fail(&format!(
                "the task {} has no parameter {name}",
                settings.env.task.name()
            ));
        }
    }

    if let Some(path) = unix {
        serve_unix(&path, &settings, once);
        return;
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .unwrap_or_else(|e| fail(&format!("cannot listen on 127.0.0.1:{port}: {e}")));
    let port = listener.local_addr().map_or(port, |a| a.port());
    eprintln!("flight-env: listening on 127.0.0.1:{port}");
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                // Small frames go out at once instead of waiting to be batched.
                let _ = stream.set_nodelay(true);
                eprintln!("flight-env: client connected");
                match serve(&mut stream, &settings) {
                    Ok(()) => eprintln!("flight-env: client left"),
                    Err(e) => eprintln!("flight-env: client lost: {e}"),
                }
            }
            Err(e) => eprintln!("flight-env: {e}"),
        }
        if once {
            break;
        }
    }
}

#[cfg(unix)]
fn serve_unix(path: &str, settings: &Settings, once: bool) {
    use std::os::unix::net::UnixListener;
    // A socket file left by an earlier run would make bind fail.
    let _ = std::fs::remove_file(path);
    let listener =
        UnixListener::bind(path).unwrap_or_else(|e| fail(&format!("cannot listen on {path}: {e}")));
    eprintln!("flight-env: listening on {path}");
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                eprintln!("flight-env: client connected");
                match serve(&mut stream, settings) {
                    Ok(()) => eprintln!("flight-env: client left"),
                    Err(e) => eprintln!("flight-env: client lost: {e}"),
                }
            }
            Err(e) => eprintln!("flight-env: {e}"),
        }
        if once {
            break;
        }
    }
    let _ = std::fs::remove_file(path);
}

#[cfg(not(unix))]
fn serve_unix(_path: &str, _settings: &Settings, _once: bool) {
    fail("Unix-domain sockets are not available on this system");
}
