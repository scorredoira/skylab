//! The game's end: a listener on `127.0.0.1` and, for the game's loop, the requests that came in.
//!
//! ```text
//! client ─ socket ─► a thread per client ─ channel ─► Server::next()   (the game, once a frame)
//!        ◄────────── the answer          ◄─ channel ─ Request::answer
//! ```
//!
//! The threads only read and write frames. Everything a request means is the game's to say, in
//! its own loop, between two steps of its world: nothing of the game is shared with a thread.
//! What the protocol itself asks for is checked here: `hello` first and with the right version,
//! `close` answered with `bye`, a payload that is not JSON refused.

use std::io;
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use crate::wire::{self, message, Incoming, Json, PROTOCOL};

/// How long a client's thread waits for the game's answer before saying it did not come: a game
/// whose window is hidden may draw no frames, and so answer nothing.
const ANSWER_WAIT: Duration = Duration::from_secs(5);

/// A request from a client, to be answered once.
pub struct Request {
    /// The message's type (`wire::message`).
    pub kind: u8,
    /// Its payload; `Json::Null` for an empty one.
    pub body: Json,
    reply: Sender<(u8, Json)>,
}

impl Request {
    /// Answers with a message of this type.
    pub fn answer(self, kind: u8, body: Json) {
        // The client may have left meanwhile: nobody to tell.
        let _ = self.reply.send((kind, body));
    }

    /// Answers with an `error` that leaves the session as it was.
    pub fn refuse(self, why: &str) {
        self.answer(message::ERROR, wire::error(why, false));
    }
}

/// The listener and the requests waiting for the game.
pub struct Server {
    requests: Receiver<Request>,
    port: u16,
}

impl Server {
    /// Listens on `127.0.0.1:port` (0: any free port, see `port`), for any number of clients.
    pub fn listen(port: u16) -> io::Result<Server> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
        let port = listener.local_addr()?.port();
        let (to_game, requests) = channel();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let to_game = to_game.clone();
                std::thread::spawn(move || {
                    // A client that leaves mid-frame is only a client that left.
                    let _ = serve(stream, &to_game);
                });
            }
        });
        Ok(Server { requests, port })
    }

    /// The port it listens on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The next request waiting, if any. The game calls it until it is `None`, once a frame.
    pub fn next(&self) -> Option<Request> {
        self.requests.try_recv().ok()
    }
}

/// One client, until it leaves.
fn serve(mut stream: TcpStream, to_game: &Sender<Request>) -> io::Result<()> {
    // Small frames go out at once instead of waiting to be batched.
    let _ = stream.set_nodelay(true);
    let mut greeted = false;
    loop {
        let (kind, body) = match wire::read_json(&mut stream) {
            Ok(Incoming::Message(kind, body)) => (kind, body),
            Ok(Incoming::Garbled(kind, error)) => {
                wire::write_json(&mut stream, message::ERROR, &wire::error(&format!("{}: its payload is not JSON: {error}", message::name(kind)), false))?;
                continue;
            }
            Ok(Incoming::End) => return Ok(()),
            Err(e) => {
                // The framing is lost: say so, if it can still be said, and close.
                let _ = wire::write_json(&mut stream, message::ERROR, &wire::error(&e.to_string(), true));
                return Err(e);
            }
        };
        match kind {
            message::CLOSE => return wire::write_json(&mut stream, message::BYE, &Json::Null),
            message::HELLO => {
                if body.get("protocol").and_then(Json::as_u64) != Some(PROTOCOL) {
                    wire::write_json(&mut stream, message::ERROR, &wire::error(&format!("hello must carry \"protocol\": {PROTOCOL}"), true))?;
                    return Ok(());
                }
                greeted = true;
            }
            _ if !greeted => {
                wire::write_json(&mut stream, message::ERROR, &wire::error(&format!("{} before hello", message::name(kind)), false))?;
                continue;
            }
            _ => {}
        }
        let (reply, answer) = channel();
        if to_game.send(Request { kind, body, reply }).is_err() {
            // The game is gone.
            return Ok(());
        }
        let (kind, body) = answer.recv_timeout(ANSWER_WAIT).unwrap_or_else(|_| (message::ERROR, wire::error("the game did not answer (it answers once a frame: is its window hidden?)", false)));
        wire::write_json(&mut stream, kind, &body)?;
    }
}
