//! Skylab Drive, the driving game, as a library: its own program (`main.rs`) runs it, and so does
//! Skylab's, which shows it in its own window when "Cars" is chosen (`start`).

pub mod audio;
pub mod camera;
pub mod controls;
pub mod effects;
#[cfg(test)]
pub mod feel;
pub mod game;
pub mod ghost;
pub mod guide;
pub mod hud;
pub mod look;
pub mod pause;
pub mod race;
pub mod replay;
pub mod rivals;
pub mod timing;

/// The game as it starts with nothing on the command line: what another program shows in its
/// window in place of its own game.
pub fn start(gpu: &engine::Gpu) -> game::Drive {
    let options = game::Options::default();
    game::Drive::new(gpu, &options, audio::Audio::start(&options.car))
}
