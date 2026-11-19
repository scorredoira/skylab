//! Lap timing: where the car is round the circuit, its laps from line to line, each lap's three
//! sectors, the best lap, and whether a lap kept within the track's limits (a lap with the car's
//! middle beyond the kerbs does not count for the best).

use sim_core::circuit::{Circuit, KERB_WIDTH};

/// The timing of one car.
#[derive(Clone, Debug, Default)]
pub struct Timing {
    /// Metres from the start line, the last time it was on the circuit.
    from_start: Option<f64>,
    /// Laps begun: 0 before the line is first crossed.
    pub lap: u32,
    /// When the lap under way began, seconds of the simulation.
    lap_start: f64,
    /// The sector times of the lap under way so far.
    pub sectors: Vec<f64>,
    pub last: Option<f64>,
    pub last_sectors: Vec<f64>,
    pub best: Option<f64>,
    pub best_sectors: Vec<f64>,
    /// The lap under way has left the track's limits.
    pub invalid: bool,
    /// The last lap did not count.
    pub last_invalid: bool,
    pub laps: Vec<f64>,
}

impl Timing {
    /// Follows the car to `(n, e)` at simulation time `time`.
    pub fn update(&mut self, circuit: &Circuit, n: f64, e: f64, time: f64) {
        let Some(at) = circuit.locate(n, e) else {
            self.invalid = true;
            return;
        };
        let edge = circuit.sample(at.along).width / 2.0;
        if self.lap > 0 && at.across.abs() > edge + KERB_WIDTH + 1.0 {
            self.invalid = true;
        }
        let along = circuit.from_start(at.along);
        if let Some(before) = self.from_start {
            let sector = |s: f64| s * circuit.length / 3.0;
            // Across the line forwards: a lap done (the first one begun).
            if before > circuit.length * 0.75 && along < circuit.length * 0.25 {
                if self.lap > 0 {
                    let t = time - self.lap_start;
                    let done = self.sectors.iter().sum::<f64>();
                    self.sectors.push(t - done);
                    self.laps.push(t);
                    self.last = Some(t);
                    self.last_sectors = std::mem::take(&mut self.sectors);
                    self.last_invalid = self.invalid;
                    if !self.invalid && self.best.is_none_or(|b| t < b) {
                        self.best = Some(t);
                        self.best_sectors = self.last_sectors.clone();
                    }
                }
                self.lap += 1;
                self.lap_start = time;
                self.sectors.clear();
                self.invalid = false;
            } else if self.lap > 0 {
                for k in 1..=2 {
                    if before < sector(k as f64) && along >= sector(k as f64) && self.sectors.len() == k - 1 {
                        let done = self.sectors.iter().sum::<f64>();
                        self.sectors.push(time - self.lap_start - done);
                    }
                }
            }
        }
        self.from_start = Some(along);
    }

    /// Seconds into the lap under way.
    pub fn current(&self, time: f64) -> Option<f64> {
        (self.lap > 0).then_some(time - self.lap_start)
    }

    /// Starts again (the car placed somewhere new): the laps and the best are kept.
    pub fn restart(&mut self) {
        self.from_start = None;
        self.lap = 0;
        self.sectors.clear();
        self.invalid = false;
    }
}

/// A time as minutes, seconds and thousandths: 1:23.456.
pub fn clock(t: f64) -> String {
    let minutes = (t / 60.0).floor();
    format!("{}:{:06.3}", minutes as u32, t - minutes * 60.0)
}
