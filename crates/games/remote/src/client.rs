//! A program's end: connects to a game, says `hello`, and sends requests one at a time.
//!
//! ```no_run
//! use remote::{Client, Json};
//! let mut game = Client::connect(remote::DEFAULT_PORT, "my-program")?;
//! let state = game.state()?;
//! println!("{:?} kt", state.get("airspeed_kt"));
//! game.controls(&Json::obj(vec![("throttle", Json::num(1.0))]))?;
//! # Ok::<(), String>(())
//! ```

use std::net::{Ipv4Addr, TcpStream};
use std::time::Duration;

use crate::wire::{self, message, Incoming, Json, PROTOCOL};

/// How long a request waits for its answer.
const ANSWER_WAIT: Duration = Duration::from_secs(10);

/// A session with a game.
pub struct Client {
    stream: TcpStream,
    /// What the game said of itself to `hello` (`describe`): its vehicle, its controls, its commands.
    pub description: Json,
}

impl Client {
    /// Connects to the game listening on `127.0.0.1:port` and greets it as `name`.
    pub fn connect(port: u16, name: &str) -> Result<Client, String> {
        let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).map_err(|e| format!("no game on 127.0.0.1:{port} ({e}): start it with --remote"))?;
        let _ = stream.set_nodelay(true);
        let _ = stream.set_read_timeout(Some(ANSWER_WAIT));
        let mut client = Client { stream, description: Json::Null };
        let hello = Json::obj(vec![("protocol", Json::num(PROTOCOL as f64)), ("client", Json::str(name))]);
        client.description = client.request(message::HELLO, &hello)?;
        Ok(client)
    }

    /// Sends a message and returns the payload of its answer. An `error` answered is the `Err`.
    pub fn request(&mut self, kind: u8, body: &Json) -> Result<Json, String> {
        wire::write_json(&mut self.stream, kind, body).map_err(|e| format!("the game is gone: {e}"))?;
        match wire::read_json(&mut self.stream) {
            Ok(Incoming::Message(message::ERROR, body)) => Err(body.get("error").and_then(Json::as_str).unwrap_or("an error without a text").to_string()),
            Ok(Incoming::Message(_, body)) => Ok(body),
            Ok(Incoming::Garbled(kind, error)) => Err(format!("the game's {} is not JSON: {error}", message::name(kind))),
            Ok(Incoming::End) => Err("the game closed the connection".to_string()),
            Err(e) => Err(format!("no answer from the game: {e}")),
        }
    }

    /// Where the vehicle is and how it flies: the `state_result`.
    pub fn state(&mut self) -> Result<Json, String> {
        self.request(message::STATE, &Json::Null)
    }

    /// Sets controls by name, for good or for `for` seconds; the state as it is then.
    pub fn controls(&mut self, set: &Json) -> Result<Json, String> {
        self.request(message::CONTROLS, set)
    }

    /// An order to the game (`{"do": ...}`); what it left set.
    pub fn command(&mut self, command: &Json) -> Result<Json, String> {
        self.request(message::COMMAND, command)
    }

    /// Says goodbye. Dropping the client without it is fine too.
    pub fn close(mut self) {
        let _ = self.request(message::CLOSE, &Json::Null);
    }
}
