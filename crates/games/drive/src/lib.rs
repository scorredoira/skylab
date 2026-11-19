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

/// The game as another program shows it in its window in place of its own: as with nothing on the
/// command line but `--car <name>`, if `arguments` have it.
pub fn start(gpu: &engine::Gpu, arguments: &[String]) -> game::Drive {
    let mut options = game::Options::default();
    if let Some(car) = arguments.iter().position(|a| a == "--car").and_then(|i| arguments.get(i + 1)) {
        options.car = car.clone();
    }
    game::Drive::new(gpu, &options, audio::Audio::start(&options.car))
}
