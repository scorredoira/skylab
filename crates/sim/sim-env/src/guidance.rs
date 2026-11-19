//! What the pilot is asked to do, as numbers: a target heading, altitude and airspeed, and where
//! the runway is. The environment and `ModelPilot` both get their task-relative observation terms
//! from here, so a model sees in the game exactly what it saw in training.
//!
//! A `Guidance` sees only the `Observation`, like a pilot does.

use std::f64::consts::PI;

use std::sync::Arc;

use sim_core::car::driver::RacingLine;
use sim_core::circuit::Circuit;
use sim_core::course::{Event, Race, Track};
use sim_core::{clamp, remainder, AircraftType, Airport, Detail, Observation, DEG, KNOTS, V3};

/// Angle of the approach path the lights show.
pub const GLIDE_SLOPE: f64 = 3.0 * DEG;
/// The approach path meets the runway this far past the threshold, metres.
pub const AIMING_POINT: f64 = 150.0;
/// Height above the runway at which a takeoff is complete, metres.
pub const TAKEOFF_HEIGHT: f64 = 150.0;
/// Best-rate climb speed, m/s.
pub const CLIMB_SPEED: f64 = 74.0 * KNOTS;
/// Speed on the crosswind and downwind legs, m/s.
pub const PATTERN_SPEED: f64 = 85.0 * KNOTS;
/// Speed on base, m/s.
pub const BASE_SPEED: f64 = 72.0 * KNOTS;
/// Speed on final, m/s.
pub const APPROACH_SPEED: f64 = 65.0 * KNOTS;
/// Height of the circuit above the runway, metres.
pub const PATTERN_HEIGHT: f64 = 200.0;
/// The downwind leg runs this far to the left of the runway, metres.
pub const PATTERN_OFFSET: f64 = 1000.0;
/// The turn to base is made this far before the threshold, metres.
pub const DOWNWIND_EXTENT: f64 = 2500.0;
/// `race`: height above the runway to reach before the landing counts, metres (400 ft).
pub const RACE_HEIGHT: f64 = 120.0;
/// `race`: the aircraft has turned back once its heading is this far from the takeoff direction.
pub const RACE_TURN: f64 = 150.0 * DEG;

/// A runway used in one direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Runway {
    pub airport: Airport,
    /// Use it along `airport.heading + π`.
    pub reversed: bool,
}

impl Runway {
    /// Direction of takeoff and landing, radians from north.
    pub fn heading(&self) -> f64 {
        self.airport.heading + if self.reversed { PI } else { 0.0 }
    }

    /// Position of a point in runway axes: metres past the threshold along the runway, and metres
    /// to the right of the centreline.
    pub fn local(&self, n: f64, e: f64) -> (f64, f64) {
        let (along, across) = self.airport.local(n, e);
        let sign = if self.reversed { -1.0 } else { 1.0 };
        (sign * along + self.airport.length / 2.0, sign * across)
    }

    /// Height of the 3° approach path above the runway at `along` metres past the threshold
    /// (zero beyond the aiming point).
    pub fn path_height(&self, along: f64) -> f64 {
        (AIMING_POINT - along).max(0.0) * GLIDE_SLOPE.tan()
    }

    pub fn on_runway(&self, n: f64, e: f64) -> bool {
        self.airport.on_runway(n, e)
    }

    /// The direction of the nearest airport's runway closest to the heading being flown.
    pub fn nearest(obs: &Observation) -> Option<Runway> {
        let a = obs.nearest_airport.as_ref()?;
        let airport = Airport {
            id: a.id,
            n: a.north,
            e: a.east,
            heading: a.heading,
            elevation: a.elevation,
            length: a.length,
            width: a.width,
            // Only its geometry is flown by.
            detail: Detail::GENERATED,
        };
        Some(Runway {
            airport,
            reversed: (obs.heading - a.heading).cos() < 0.0,
        })
    }
}

/// Where the aircraft is relative to the runway in use.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct RunwayView {
    /// Metres past the threshold (negative on the approach).
    pub along: f64,
    /// Metres to the right of the centreline.
    pub across: f64,
    /// Height of the wheels above the runway, metres.
    pub height: f64,
    /// Height above the 3° approach path, metres.
    pub path_error: f64,
    /// Runway direction minus heading, in (-π, π]: positive means the runway points to the right.
    pub heading_error: f64,
    /// Horizontal distance to the aiming point, metres.
    pub distance: f64,
}

/// A leg of the circuit. The numbers are what the observation carries (`leg`).
pub mod leg {
    pub const NONE: u8 = 0;
    pub const UPWIND: u8 = 1;
    pub const CROSSWIND: u8 = 2;
    pub const DOWNWIND: u8 = 3;
    pub const BASE: u8 = 4;
    pub const FINAL: u8 = 5;
    /// `race`: past `RACE_HEIGHT`, not yet turned back.
    pub const TURNING: u8 = 2;
    /// `race`: turned back, on the way to the landing.
    pub const RETURNING: u8 = 3;
}

/// What the guidance asks for right now.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Targets {
    /// Heading to fly, radians.
    pub heading: Option<f64>,
    /// Altitude above sea level to fly, metres.
    pub altitude: Option<f64>,
    /// Indicated airspeed to fly, m/s.
    pub airspeed: Option<f64>,
    pub runway: Option<RunwayView>,
    /// See `leg`.
    pub leg: u8,
    /// A point to hover over (north, east), for a multirotor.
    pub point: Option<(f64, f64)>,
    /// A race round a course (`gates`): the next two gates to fly through, and how far round it is.
    pub gates: [Option<GateTarget>; 2],
    pub race: Option<RaceProgress>,
    /// A drive round a circuit (`drive`): where the car is on it and what lies ahead.
    pub drive: Option<DriveTarget>,
}

/// Where a car is on a circuit and what lies ahead of it (`drive`), as its guidance sees it.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct DriveTarget {
    /// Metres driven round since the start (going backwards takes them off).
    pub progress: f64,
    /// Metres to the right of the centre line, and half the asphalt's width there.
    pub across: f64,
    pub half_width: f64,
    /// The circuit's direction where the car is, minus its heading: rad in (−π, π].
    pub heading_error: f64,
    /// The circuit's curvature (1/m, positive turning right) at `AHEAD` metres ahead.
    pub curvature: [f64; 5],
    /// The racing line: metres right of the centre line at `LINE_AHEAD`, and its speed (m/s) at `SPEED_AHEAD`.
    pub line: [f64; 3],
    pub line_speed: [f64; 3],
    /// The lap's length.
    pub length: f64,
}

/// Where `DriveTarget::curvature`, `line` and `line_speed` look, metres ahead.
pub const AHEAD: [f64; 5] = [0.0, 20.0, 50.0, 100.0, 150.0];
pub const LINE_AHEAD: [f64; 3] = [10.0, 30.0, 60.0];
pub const SPEED_AHEAD: [f64; 3] = [0.0, 50.0, 100.0];

/// A gate to fly through: the middle of its opening and the way through it (a unit vector), world
/// axes (north, east, down).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct GateTarget {
    pub middle: [f64; 3],
    pub through: [f64; 3],
}

/// How far round a race the racer is (`sim_core::course::Race`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct RaceProgress {
    /// Gates flown through and the share of the way to the next (`Race::progress`).
    pub progress: f64,
    /// Gates flown through, the start's too.
    pub passed: u32,
    /// Every lap done.
    pub finished: bool,
    /// Gates missed so far (flown past outside the opening, or another flown instead).
    pub missed: u32,
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Hold {
        heading: f64,
        altitude: f64,
        airspeed: f64,
    },
    Takeoff(Runway),
    Landing(Runway),
    Circuit(Runway, u8),
    /// `race`: the runway as taken off from, and the leg (`leg::UPWIND`, `TURNING`, `RETURNING`).
    Race(Runway, u8),
    Hover {
        north: f64,
        east: f64,
        altitude: f64,
        heading: f64,
    },
    /// A race round a course's gates, counted as it is flown.
    Gates { track: Arc<Track>, race: Box<Race>, missed: u32 },
    /// A drive round a circuit: where along it the car last was and how far it has come.
    Drive { circuit: Same<Circuit>, line: Same<RacingLine>, along: f64, progress: f64 },
}

/// Something shared, equal to another only if it is the same one.
#[derive(Clone, Debug)]
struct Same<T>(Arc<T>);

impl<T> PartialEq for Same<T> {
    fn eq(&self, other: &Same<T>) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Turns the situation into targets. One per aircraft; `update` is called once per decision.
#[derive(Clone, Debug, PartialEq)]
pub struct Guidance {
    kind: Kind,
    /// Height of the centre of gravity with the wheels on the ground.
    cg_height: f64,
}

impl Guidance {
    fn new(kind: Kind) -> Guidance {
        Guidance {
            kind,
            cg_height: AircraftType::c172().cg_height,
        }
    }

    /// For an aircraft whose centre of gravity stands `cg_height` over the ground on its wheels
    /// (that of the C172 until this is called).
    pub fn set_cg_height(&mut self, cg_height: f64) {
        self.cg_height = cg_height;
    }

    /// Hold a heading (radians), an altitude above sea level (m) and an indicated airspeed (m/s).
    pub fn hold(heading: f64, altitude: f64, airspeed: f64) -> Guidance {
        Guidance::new(Kind::Hold {
            heading,
            altitude,
            airspeed,
        })
    }

    /// A multirotor: hover over a point (north, east) at an altitude above sea level (m), facing a
    /// heading (radians).
    pub fn hover(north: f64, east: f64, altitude: f64, heading: f64) -> Guidance {
        Guidance::new(Kind::Hover { north, east, altitude, heading })
    }

    /// A multirotor: race round a track from `pos` (world axes), the start given at `time` (s),
    /// gate `next` the next to fly through (those before it counted as flown: 0 from the start).
    pub fn gates(track: Arc<Track>, time: f64, pos: V3, next: usize) -> Guidance {
        let mut race = Race::new(track.course.laps);
        race.go(time, pos);
        race.next = next % track.gates.len();
        race.passed = race.next as u32;
        Guidance::new(Kind::Gates { track, race: Box::new(race), missed: 0 })
    }

    /// The race, for `gates`.
    /// A drive round a circuit from `along` metres from its first point, along the racing line `line`.
    pub fn drive(circuit: Arc<Circuit>, line: Arc<RacingLine>, along: f64) -> Guidance {
        Guidance::new(Kind::Drive { circuit: Same(circuit), line: Same(line), along, progress: 0.0 })
    }

    /// A drive's guidance with `progress` metres already made (its line changed on the way).
    pub fn with_progress(mut self, made: f64) -> Guidance {
        if let Kind::Drive { progress, .. } = &mut self.kind {
            *progress = made;
        }
        self
    }

    /// The circuit of a drive.
    pub fn race_circuit(&self) -> Option<&Arc<Circuit>> {
        match &self.kind {
            Kind::Drive { circuit, .. } => Some(&circuit.0),
            _ => None,
        }
    }

    pub fn course(&self) -> Option<(&Track, &Race)> {
        match &self.kind {
            Kind::Gates { track, race, .. } => Some((track, race)),
            _ => None,
        }
    }

    /// Take off and climb straight ahead.
    pub fn takeoff(runway: Runway) -> Guidance {
        Guidance::new(Kind::Takeoff(runway))
    }

    /// Fly the approach, land and stop.
    pub fn landing(runway: Runway) -> Guidance {
        Guidance::new(Kind::Landing(runway))
    }

    /// Take off, fly a left-hand circuit and land.
    pub fn circuit(runway: Runway) -> Guidance {
        Guidance::new(Kind::Circuit(runway, leg::UPWIND))
    }

    /// Take off, climb to `RACE_HEIGHT`, turn back and land on the same runway, either way.
    pub fn race(runway: Runway) -> Guidance {
        Guidance::new(Kind::Race(runway, leg::UPWIND))
    }

    /// The guidance a task needs when nothing else is known, from what the aircraft sees now:
    /// the runway is the nearest one in the direction being flown, and `hold` keeps the present
    /// heading and altitude at 95 knots. `task` is a task name as in the protocol
    /// (`hold`, `takeoff`, `landing`, `circuit`, `race`).
    pub fn for_task(task: &str, obs: &Observation) -> Option<Guidance> {
        match task {
            "hold" => Some(Guidance::hold(obs.heading, obs.altitude, 95.0 * KNOTS)),
            "takeoff" => Runway::nearest(obs).map(Guidance::takeoff),
            "landing" => Runway::nearest(obs).map(Guidance::landing),
            "circuit" => Runway::nearest(obs).map(Guidance::circuit),
            "race" => Runway::nearest(obs).map(Guidance::race),
            "hover" => Some(Guidance::hover(obs.north, obs.east, obs.altitude, obs.heading)),
            _ => None,
        }
    }

    pub fn runway(&self) -> Option<Runway> {
        match self.kind {
            Kind::Hold { .. } | Kind::Hover { .. } | Kind::Gates { .. } | Kind::Drive { .. } => None,
            Kind::Takeoff(r) | Kind::Landing(r) | Kind::Circuit(r, _) | Kind::Race(r, _) => Some(r),
        }
    }

    fn view(&self, runway: &Runway, obs: &Observation) -> RunwayView {
        let (along, across) = runway.local(obs.north, obs.east);
        let height = obs.altitude - runway.airport.elevation - self.cg_height;
        RunwayView {
            along,
            across,
            height,
            path_error: height - runway.path_height(along),
            heading_error: remainder(runway.heading() - obs.heading, 2.0 * PI),
            distance: (along - AIMING_POINT).hypot(across),
        }
    }

    /// Targets for the situation in `obs`. Moves on to the next leg of the circuit when its end
    /// is reached, so call it once per decision.
    pub fn update(&mut self, obs: &Observation) -> Targets {
        let cg = self.cg_height;
        if let Kind::Drive { circuit, line, along, progress } = &mut self.kind {
            return Targets { drive: Some(drive_target(&circuit.0, &line.0, along, progress, obs)), ..Targets::default() };
        }
        if let Kind::Gates { track, race, missed } = &mut self.kind {
            let pos = V3::new(obs.north, obs.east, -obs.altitude);
            for event in race.update(track, pos, obs.time) {
                if let Event::Missed { .. } = event {
                    *missed += 1;
                }
            }
            let n = track.gates.len();
            let gate = |k: usize| {
                let (middle, through) = track.target((race.next + k) % n);
                Some(GateTarget { middle: middle.to_array(), through: through.to_array() })
            };
            let done = race.finished.is_some();
            return Targets {
                gates: if done { [None; 2] } else { [gate(0), gate(1)] },
                race: Some(RaceProgress { progress: race.progress(track, pos), passed: race.passed, finished: done, missed: *missed }),
                ..Targets::default()
            };
        }
        match self.kind {
            Kind::Hold {
                heading,
                altitude,
                airspeed,
            } => Targets {
                heading: Some(heading),
                altitude: Some(altitude),
                airspeed: Some(airspeed),
                runway: None,
                leg: leg::NONE,
                point: None,
                ..Targets::default()
            },
            Kind::Hover { north, east, altitude, heading } => Targets {
                heading: Some(heading),
                altitude: Some(altitude),
                airspeed: None,
                runway: None,
                leg: leg::NONE,
                point: Some((north, east)),
                ..Targets::default()
            },
            Kind::Takeoff(runway) => {
                let v = self.view(&runway, obs);
                Targets {
                    heading: Some(centreline(&runway, &v, obs.on_ground)),
                    altitude: Some(runway.airport.elevation + cg + PATTERN_HEIGHT),
                    airspeed: Some(CLIMB_SPEED),
                    runway: Some(v),
                    leg: leg::UPWIND,
                    point: None,
                    ..Targets::default()
                }
            }
            Kind::Landing(runway) => {
                let v = self.view(&runway, obs);
                final_targets(&runway, &v, obs, cg)
            }
            Kind::Circuit(runway, mut now) => {
                let v = self.view(&runway, obs);
                let next = match now {
                    leg::UPWIND => v.height >= TAKEOFF_HEIGHT,
                    leg::CROSSWIND => v.across <= -(PATTERN_OFFSET - 250.0),
                    leg::DOWNWIND => v.along <= -DOWNWIND_EXTENT,
                    leg::BASE => v.across >= -300.0,
                    _ => false,
                };
                if next {
                    now += 1;
                    self.kind = Kind::Circuit(runway, now);
                }
                let h = runway.heading();
                let pattern = runway.airport.elevation + cg + PATTERN_HEIGHT;
                let (heading, altitude, airspeed) = match now {
                    leg::UPWIND => (centreline(&runway, &v, obs.on_ground), pattern, CLIMB_SPEED),
                    leg::CROSSWIND => (h - PI / 2.0, pattern, PATTERN_SPEED),
                    leg::DOWNWIND => (
                        // Hold the line `PATTERN_OFFSET` to the left of the runway.
                        h + PI + clamp((v.across + PATTERN_OFFSET) / 400.0, -0.5, 0.5),
                        pattern,
                        PATTERN_SPEED,
                    ),
                    leg::BASE => (
                        h + PI / 2.0,
                        runway.airport.elevation + cg + runway.path_height(-DOWNWIND_EXTENT),
                        BASE_SPEED,
                    ),
                    _ => return final_targets(&runway, &v, obs, cg),
                };
                Targets {
                    heading: Some(heading.rem_euclid(2.0 * PI)),
                    altitude: Some(altitude),
                    airspeed: Some(airspeed),
                    runway: Some(v),
                    leg: now,
                    point: None,
                    ..Targets::default()
                }
            }
            Kind::Race(runway, mut now) => {
                // The view stays in the takeoff direction for the whole flight.
                let v = self.view(&runway, obs);
                let next = match now {
                    leg::UPWIND => !obs.on_ground && v.height >= RACE_HEIGHT,
                    leg::TURNING => v.heading_error.abs() >= RACE_TURN,
                    _ => false,
                };
                if next {
                    now += 1;
                    self.kind = Kind::Race(runway, now);
                }
                let climb = runway.airport.elevation + cg + RACE_HEIGHT + 30.0;
                let (heading, altitude, airspeed) = match now {
                    leg::UPWIND => (centreline(&runway, &v, obs.on_ground), climb, CLIMB_SPEED),
                    leg::TURNING => (runway.heading() + PI, climb, PATTERN_SPEED),
                    _ => {
                        // Back to the runway, landing in the direction nearer the heading.
                        let mut landing = runway;
                        landing.reversed ^= v.heading_error.abs() > PI / 2.0;
                        let lv = self.view(&landing, obs);
                        let t = final_targets(&landing, &lv, obs, cg);
                        (t.heading.unwrap_or(0.0), t.altitude.unwrap_or(climb), APPROACH_SPEED)
                    }
                };
                Targets {
                    heading: Some(heading.rem_euclid(2.0 * PI)),
                    altitude: Some(altitude),
                    airspeed: Some(if obs.on_ground && now == leg::RETURNING { 0.0 } else { airspeed }),
                    runway: Some(v),
                    leg: now,
                    point: None,
                    ..Targets::default()
                }
            }
            Kind::Gates { .. } | Kind::Drive { .. } => unreachable!("answered above"),
        }
    }
}

/// Where a car is on its circuit and what is ahead, its progress since the last call added.
fn drive_target(circuit: &Circuit, line: &RacingLine, along: &mut f64, progress: &mut f64, obs: &Observation) -> DriveTarget {
    let c = circuit;
    let Some(at) = c.locate(obs.north, obs.east) else {
        return DriveTarget { progress: *progress, across: 1e3, half_width: 6.0, length: c.length, ..DriveTarget::default() };
    };
    // Progress: the shorter way round from where it was.
    let step = remainder(at.along - *along, c.length);
    *progress += step;
    *along = at.along;
    let sample = c.sample(at.along);
    let (_, _, _, direction) = c.at(at.along);
    let count = line.across.len();
    let index = |a: f64| (a.rem_euclid(c.length) / sim_core::circuit::STEP) as usize % count;
    DriveTarget {
        progress: *progress,
        across: at.across,
        half_width: sample.width / 2.0,
        heading_error: remainder(direction - obs.heading, 2.0 * PI),
        curvature: AHEAD.map(|d| c.sample(at.along + d).curvature),
        line: LINE_AHEAD.map(|d| line.across[index(at.along + d)]),
        line_speed: SPEED_AHEAD.map(|d| line.speed[index(at.along + d)]),
        length: c.length,
    }
}

/// Heading that brings the aircraft back to the runway centreline and along it.
fn centreline(runway: &Runway, v: &RunwayView, on_ground: bool) -> f64 {
    let (reach, most) = if on_ground { (60.0, 0.2) } else { (400.0, 0.5) };
    (runway.heading() - clamp(v.across / reach, -most, most)).rem_euclid(2.0 * PI)
}

/// On final: follow the centreline and the 3° path at approach speed; once down, stop.
fn final_targets(runway: &Runway, v: &RunwayView, obs: &Observation, cg: f64) -> Targets {
    Targets {
        heading: Some(centreline(runway, v, obs.on_ground)),
        altitude: Some(runway.airport.elevation + cg + runway.path_height(v.along)),
        airspeed: Some(if obs.on_ground { 0.0 } else { APPROACH_SPEED }),
        runway: Some(*v),
        leg: leg::FINAL,
        point: None,
        ..Targets::default()
    }
}
