//! A running game driven from another program: the messages (`wire`), the game's end of the
//! socket (`server`) and a program's end (`client`). It knows no game and no vehicle: a game
//! answers the requests with what it has (`crates/games/flight/src/remote.rs`). The protocol is
//! `PROTOCOL.md`; `skylab-ctl` (`src/bin`) is the client at the command line.

pub mod client;
pub mod server;
pub mod wire;

pub use client::Client;
pub use server::{Request, Server};
pub use wire::{message, Json, DEFAULT_PORT, PROTOCOL};
