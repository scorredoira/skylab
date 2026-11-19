//! The messages on the socket: `sim-env`'s frames (four bytes of length, one of type, the
//! payload), every payload JSON or empty. `PROTOCOL.md` says what each message carries.

use std::io::{self, Read, Write};

use sim_env::server::{read_frame, write_frame};

pub use sim_env::json::Json;

/// Version of the protocol, the same number as `sim-env`'s: `hello` must carry it.
pub const PROTOCOL: u64 = 1;
/// Where a game listens unless told otherwise (`flight-env` has 7447).
pub const DEFAULT_PORT: u16 = 7448;

/// Message types: the byte after the length. 1 to 9 are `sim-env`'s, with the same meaning; the
/// ones a game adds start at 10.
pub mod message {
    pub const HELLO: u8 = 1;
    pub const DESCRIBE: u8 = 2;
    pub const CLOSE: u8 = 7;
    pub const BYE: u8 = 8;
    pub const ERROR: u8 = 9;
    /// Kept for the picture of the window, not built yet: `frame` and its answer `image`.
    pub const FRAME: u8 = 10;
    pub const IMAGE: u8 = 11;
    pub const STATE: u8 = 12;
    pub const STATE_RESULT: u8 = 13;
    pub const COMMAND: u8 = 14;
    pub const DONE: u8 = 15;
    /// Kept for a clock the client steps, not built yet.
    pub const ADVANCE: u8 = 16;
    pub const CONTROLS: u8 = 17;

    /// The name `PROTOCOL.md` gives a type, for what is said about one.
    pub fn name(kind: u8) -> &'static str {
        match kind {
            HELLO => "hello",
            DESCRIBE => "describe",
            CLOSE => "close",
            BYE => "bye",
            ERROR => "error",
            FRAME => "frame",
            IMAGE => "image",
            STATE => "state",
            STATE_RESULT => "state_result",
            COMMAND => "command",
            DONE => "done",
            ADVANCE => "advance",
            CONTROLS => "controls",
            _ => "unknown",
        }
    }
}

/// Writes one message. `Json::Null` goes as an empty payload.
pub fn write_json(stream: &mut impl Write, kind: u8, body: &Json) -> io::Result<()> {
    match body {
        Json::Null => write_frame(stream, kind, &[]),
        body => write_frame(stream, kind, body.to_string().as_bytes()),
    }
}

/// What a frame held.
pub enum Incoming {
    /// A message: its type and its payload (`Json::Null` for an empty one).
    Message(u8, Json),
    /// A frame whose payload is not JSON: its type and what is wrong with it.
    Garbled(u8, String),
    /// The other end closed between frames.
    End,
}

/// Reads one message. An error is the stream's: the framing is lost with it.
pub fn read_json(stream: &mut impl Read) -> io::Result<Incoming> {
    let Some((kind, payload)) = read_frame(stream)? else { return Ok(Incoming::End) };
    if payload.is_empty() {
        return Ok(Incoming::Message(kind, Json::Null));
    }
    let parsed = std::str::from_utf8(&payload).map_err(|e| e.to_string()).and_then(Json::parse);
    Ok(match parsed {
        Ok(body) => Incoming::Message(kind, body),
        Err(error) => Incoming::Garbled(kind, error),
    })
}

/// The payload of an `error`.
pub fn error(text: &str, fatal: bool) -> Json {
    Json::obj(vec![("error", Json::str(text)), ("fatal", Json::Bool(fatal))])
}
