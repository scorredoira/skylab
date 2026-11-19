//! The replay (`F2` or `Tab`): the last minutes of the session, every car as it was, shown again
//! from the start of the player's last lap with the chase, bonnet, cockpit and trackside cameras,
//! while the world stands still.
//!
//! `Recorder` keeps, twenty times a second, each car as it is seen (`sim_core::CarLook`: body,
//! wheels on their springs, steered and turning, the controls, the engine, what the tyres do), and
//! forgets what is older than `KEEP` seconds; in between two the cars are blended (`CarLook::blend`).
//! `Playback` holds the cars as they were when the replay began while the world's are written with
//! the moment shown (`Car::show`); leaving puts them back, so the session goes on from there. The
//! tyres' marks and smoke are made again from the wheels shown (`effects.rs`), as they were made.
//!
//! Keys in it: Space or P stop / go on, ← → five seconds back or on (Shift: thirty), ↓ ↑ slower or
//! faster (a quarter to four times), Home and End, `[` `]` the car watched, C the camera, F2, Tab
//! or Esc back to the session.

use std::collections::VecDeque;

use sim_core::{Car, CarLook, VehicleId, World};

/// Seconds kept.
pub const KEEP: f64 = 300.0;
/// Seconds between the looks kept.
pub const EVERY: f64 = 1.0 / 20.0;
/// The speeds a replay runs at.
pub const SPEEDS: [f64; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

/// The cars as they were at a moment.
struct Frame {
    time: f64,
    looks: Vec<CarLook>,
}

/// What is kept: see the module.
#[derive(Default)]
pub struct Recorder {
    frames: VecDeque<Frame>,
}

impl Recorder {
    /// After a step of the world at `time`: the cars `ids` kept if a look is due.
    pub fn record(&mut self, world: &World, ids: &[VehicleId]) {
        let time = world.time();
        if self.frames.back().is_some_and(|f| time < f.time) {
            // The clock went back (a new session): start afresh.
            self.frames.clear();
        }
        if self.frames.back().is_some_and(|f| time - f.time < EVERY - 1e-9) {
            return;
        }
        let looks = ids.iter().filter_map(|id| world.vehicle(*id).car().map(Car::look)).collect();
        self.frames.push_back(Frame { time, looks });
        while self.frames.front().is_some_and(|f| time - f.time > KEEP) {
            self.frames.pop_front();
        }
    }

    pub fn clear(&mut self) {
        self.frames.clear();
    }

    /// The first and last moments kept.
    pub fn span(&self) -> Option<(f64, f64)> {
        Some((self.frames.front()?.time, self.frames.back()?.time))
    }

    /// The cars as they were at `time` (blended between the two looks either side).
    pub fn at(&self, time: f64) -> Option<Vec<CarLook>> {
        let k = self.frames.partition_point(|f| f.time <= time);
        let a = &self.frames[k.checked_sub(1)?];
        let Some(b) = self.frames.get(k) else { return Some(a.looks.clone()) };
        let dt = b.time - a.time;
        let t = ((time - a.time) / dt.max(1e-9)).clamp(0.0, 1.0);
        Some(a.looks.iter().zip(&b.looks).map(|(x, y)| x.blend(y, t, dt)).collect())
    }
}

/// A replay being shown.
pub struct Playback {
    /// The moment shown, world seconds.
    pub time: f64,
    /// An index into `SPEEDS`.
    pub speed: usize,
    pub paused: bool,
    /// The cars as they were when it began, put back when it ends.
    saved: Vec<(VehicleId, Car)>,
}

impl Playback {
    /// Begins at `from`, holding the cars `ids` as they are.
    pub fn begin(world: &World, ids: &[VehicleId], from: f64) -> Playback {
        let saved = ids.iter().filter_map(|id| Some((*id, world.vehicle(*id).car()?.clone()))).collect();
        Playback { time: from, speed: 2, paused: false, saved }
    }

    /// `dt` seconds of real time: the moment moves on (and stops at the end).
    pub fn advance(&mut self, dt: f64, recorder: &Recorder) -> f64 {
        let Some((first, last)) = recorder.span() else { return 0.0 };
        if self.paused {
            return 0.0;
        }
        let before = self.time;
        self.time = (self.time + dt * SPEEDS[self.speed]).clamp(first, last);
        if self.time >= last {
            self.paused = true;
        }
        self.time - before
    }

    /// Moves the moment by `seconds` (within what is kept).
    pub fn seek(&mut self, seconds: f64, recorder: &Recorder) {
        if let Some((first, last)) = recorder.span() {
            self.time = (self.time + seconds).clamp(first, last);
        }
    }

    /// The world's cars written with the moment shown (to be drawn only).
    pub fn show(&self, world: &mut World, ids: &[VehicleId], recorder: &Recorder) {
        let Some(looks) = recorder.at(self.time) else { return };
        for (id, look) in ids.iter().zip(&looks) {
            if let Some(car) = world.vehicle_mut(*id).car_mut() {
                car.show(look);
            }
        }
    }

    /// Puts the cars back as they were when it began.
    pub fn end(self, world: &mut World) {
        for (id, car) in self.saved {
            if let Some(c) = world.vehicle_mut(id).car_mut() {
                *c = car;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::car::driver::Driver;
    use sim_core::Start;

    #[test]
    fn shows_the_past_and_puts_the_present_back() {
        let (circuit, terrain) = crate::game::circuit_world("lakeside").unwrap();
        let mut world = World::with_terrain(1, terrain);
        let (n, e, heading) = circuit.grid(0);
        let id = world.add_car(sim_core::CarKind::GT, Start::Air { n, e, heading });
        let mut driver = Driver::new(circuit.clone(), sim_core::CarKind::GT.spec());
        let mut rec = Recorder::default();
        for _ in 0..(20.0 / World::DT) as usize {
            world.fly(id, &mut driver);
            world.step();
            rec.record(&world, &[id]);
        }
        let now = world.vehicle(id).body();
        let (first, last) = rec.span().unwrap();
        assert!(last - first > 19.9 && first < 0.1);
        // Ten seconds back the car was elsewhere, and slower than now.
        let mut play = Playback::begin(&world, &[id], last - 10.0);
        play.show(&mut world, &[id], &rec);
        let then = world.vehicle(id).body();
        assert!((then.pos - now.pos).length() > 100.0);
        // Played on at twice the speed for five seconds: ten seconds on, at the end.
        play.speed = 3;
        for _ in 0..300 {
            play.advance(1.0 / 60.0, &rec);
        }
        assert!((play.time - last).abs() < 1e-6 && play.paused);
        play.show(&mut world, &[id], &rec);
        // The last look kept is at most a twentieth of a second old.
        assert!((world.vehicle(id).body().pos - now.pos).length() < EVERY * 60.0);
        play.end(&mut world);
        assert_eq!(world.vehicle(id).body(), now);
    }
}
