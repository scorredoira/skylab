//! A client and a game's end talking over a real socket: the game here is a thread that answers
//! as a game's loop would, once every few milliseconds.

use std::io::Write;
use std::net::{Ipv4Addr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use remote::wire::{self, Incoming};
use remote::{message, Client, Json, Server};

/// A game that knows one number: `state` tells it, `controls` sets it.
fn game() -> (u16, Arc<AtomicBool>) {
    let server = Server::listen(0).unwrap();
    let port = server.port();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    std::thread::spawn(move || {
        let mut throttle = 0.0;
        while !stopped.load(Ordering::Relaxed) {
            while let Some(request) = server.next() {
                let state = |throttle: f64| Json::obj(vec![("throttle", Json::Num(throttle))]);
                match request.kind {
                    message::HELLO => request.answer(message::DESCRIBE, Json::obj(vec![("vehicle", Json::str("test"))])),
                    message::STATE => request.answer(message::STATE_RESULT, state(throttle)),
                    message::CONTROLS => match request.body.get("throttle").and_then(Json::as_f64) {
                        Some(value) => {
                            throttle = value;
                            request.answer(message::STATE_RESULT, state(throttle));
                        }
                        None => request.refuse("no throttle"),
                    },
                    _ => request.refuse("unknown"),
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    });
    (port, stop)
}

#[test]
fn a_client_asks_sets_and_is_told_what_is_wrong() {
    let (port, stop) = game();
    let mut client = Client::connect(port, "test").unwrap();
    assert_eq!(client.description.get("vehicle").and_then(Json::as_str), Some("test"));
    assert_eq!(client.state().unwrap().get("throttle").and_then(Json::as_f64), Some(0.0));
    let set = client.controls(&Json::obj(vec![("throttle", Json::Num(0.75))])).unwrap();
    assert_eq!(set.get("throttle").and_then(Json::as_f64), Some(0.75));
    assert_eq!(client.controls(&Json::obj(vec![("wheel", Json::Num(1.0))])).unwrap_err(), "no throttle");
    // An error leaves the session going.
    client.close();

    // What one client set, another finds: a connection per request is fine.
    let mut second = Client::connect(port, "test").unwrap();
    assert_eq!(second.state().unwrap().get("throttle").and_then(Json::as_f64), Some(0.75));
    stop.store(true, Ordering::Relaxed);
}

/// Reads the next message of a raw stream: its type and payload.
fn next(stream: &mut TcpStream) -> (u8, Json) {
    match wire::read_json(stream).unwrap() {
        Incoming::Message(kind, body) => (kind, body),
        _ => panic!("no message"),
    }
}

#[test]
fn the_protocol_s_own_rules_are_kept_before_the_game_is_asked() {
    let (port, stop) = game();
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();

    // A request before hello, and a payload that is not JSON: refused, the session goes on.
    wire::write_json(&mut stream, message::STATE, &Json::Null).unwrap();
    let (kind, body) = next(&mut stream);
    assert_eq!((kind, body.get("fatal").and_then(Json::as_bool)), (message::ERROR, Some(false)));
    assert!(body.get("error").and_then(Json::as_str).unwrap().contains("state before hello"));
    stream.write_all(&[4, 0, 0, 0, message::CONTROLS, b'{', b'{', b'{']).unwrap();
    let (kind, body) = next(&mut stream);
    assert_eq!(kind, message::ERROR);
    assert!(body.get("error").and_then(Json::as_str).unwrap().contains("not JSON"));

    wire::write_json(&mut stream, message::HELLO, &Json::obj(vec![("protocol", Json::Num(1.0))])).unwrap();
    assert_eq!(next(&mut stream).0, message::DESCRIBE);
    wire::write_json(&mut stream, message::CLOSE, &Json::Null).unwrap();
    assert_eq!(next(&mut stream).0, message::BYE);
    assert!(matches!(wire::read_json(&mut stream), Ok(Incoming::End)));

    // Another version of the protocol: told, and the connection closed.
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    wire::write_json(&mut stream, message::HELLO, &Json::obj(vec![("protocol", Json::Num(2.0))])).unwrap();
    let (kind, body) = next(&mut stream);
    assert_eq!((kind, body.get("fatal").and_then(Json::as_bool)), (message::ERROR, Some(true)));
    assert!(matches!(wire::read_json(&mut stream), Ok(Incoming::End)));
    stop.store(true, Ordering::Relaxed);
}
