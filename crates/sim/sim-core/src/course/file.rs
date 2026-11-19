//! Courses and kinds of gate as data: `assets/courses/<key>.ron` read into a `Course` and checked,
//! the courses there are (`courses`), and `assets/courses/gates.ron`, the kinds of gate
//! (`shapes`). As `aircraft::file`, nothing here reads the disk: the game hands the files' text
//! over (`courses_with`, `set_courses`); without that the courses are those built into the program.
//!
//! A course's file:
//!
//! ```ron
//! (
//!     name: "Airfield GP",
//!     description: "...",
//!     place: Seed(1),                        // the generated world of seed 1, or Airport("LEMH")
//!     frame: Runway(near: (-6659, 11971), reversed: true),   // x along that aerodrome's runway, y to its right
//!     start: (x: -60, y: -75, heading: 0),   // degrees from the frame's x
//!     laps: 3,
//!     quad: "race5",                         // what it is raced in (optional: the player's quad)
//!     racer: (top: 40, straight: 30, hairpin: 12, braking: 14, tilt_deg: 68, floor: 1.2),  // optional
//!     trail: (clearance: 5.5, closed: true, points: [(x: 0, y: 0, width: 9), ...]),       // optional
//!     gates: [
//!         (kind: Arch, x: -40, y: -75, heading: 0),
//!         (kind: Square, x: 90, y: -70, heading: 0, scale: 1.2),
//!         (kind: Flag, x: 200, y: -60, heading: 0, side: Right),
//!         ...
//!     ],
//! )
//! ```
//!
//! - `quad`: the multirotor the race is flown in, by its key (`assets/aircraft/<key>`): the game
//!   puts the player and the rivals in it, the `gates` task flies it. Without it, the player's.
//! - `racer`: the pace of the course's racer (`racer::Pace`) for its quad and its turns: the
//!   fastest it flies, through a gate straight on and at a hairpin, how hard it brakes, the most it
//!   tilts (degrees), the least height it keeps over the ground, and whether it follows the ground
//!   between the gates (`follow_ground`, false if not given), and the fastest it turns its nose
//!   (`yaw_rate_deg`, 286 if not given). Without it, `Pace::default()`.
//! - `trail`: a trail cut through the forest along the line of the race, as forest tracks are: no
//!   tree has its trunk within half the `width` (m) of the line through `points` (closed back to
//!   the first if `closed`), and none whose crown starts lower than `clearance` (m) reaches over
//!   it (its low branches trimmed back); taller trees keep their crowns over it, the canopy. The world's trees
//!   are cut so where they grow (`scenery::trees`), drawn and run into alike. A point may give a
//!   `clearance` of its own (a clearing open to the sky), for the stretches either side of it.
//!   Within `band` metres of the trail the forest is `dense` times as thick as the world grows it
//!   (more trees to a cell of `scenery::trees`'s grid: the old wood round the track).

use std::sync::OnceLock;

use serde::Deserialize;

use super::{GateKind, GateShape, Opening, Part, Side};
use crate::aircraft::file::FileError;
use crate::terrain::Terrain;

/// The kinds of gate, built into the program.
pub const GATES: (&str, &str) = ("assets/courses/gates.ron", include_str!("../../../../../assets/courses/gates.ron"));

/// The courses of `assets/courses` built into the program: (where it is in the repository, its text).
pub const BUILT_IN: [(&str, &str); 2] = [
    ("assets/courses/pinewood.ron", include_str!("../../../../../assets/courses/pinewood.ron")),
    ("assets/courses/deepwood.ron", include_str!("../../../../../assets/courses/deepwood.ron")),
];

/// Where a course is.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub enum Place {
    /// The world generated from this seed.
    Seed(u64),
    /// A real aerodrome's place (`geodata`), by its ICAO code: its main runway's middle is the origin.
    Airport(String),
}

/// The axes a course's positions are given in.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub enum Frame {
    /// North and east of the world's origin; headings from north.
    World,
    /// The runway of the aerodrome nearest `near` (north, east; the origin, the home aerodrome, if
    /// not given): from its middle, x along it (the other way if `reversed`), y to the right;
    /// headings from x.
    Runway {
        #[serde(default)]
        near: (f64, f64),
        reversed: bool,
    },
}

impl Frame {
    /// Its origin (north, east) and the direction of its x axis (radians from north) on a terrain.
    pub fn origin(&self, terrain: &Terrain) -> (f64, f64, f64) {
        match self {
            Frame::World => (0.0, 0.0, 0.0),
            Frame::Runway { near, reversed } => match terrain.nearest_airport(near.0, near.1) {
                Some(a) => (a.n, a.e, a.heading + if *reversed { std::f64::consts::PI } else { 0.0 }),
                None => (0.0, 0.0, 0.0),
            },
        }
    }
}

/// A gate of a course.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CourseGate {
    pub kind: GateKind,
    /// Where its foot is, metres in the course's frame.
    pub x: f64,
    pub y: f64,
    /// The way through it, degrees from the frame's x (a dive gate's level edges' way).
    pub heading: f64,
    /// How much bigger than its kind's shape.
    #[serde(default = "one")]
    pub scale: f64,
    /// A flag's side: which side of the racer it is kept on.
    #[serde(default)]
    pub side: Side,
}

fn one() -> f64 {
    1.0
}

/// A trail cut through the forest (the file's `trail`): see the module.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trail {
    /// Crowns that start lower than this over the ground are cleared from the middle of it, m.
    pub clearance: f64,
    #[serde(default)]
    pub closed: bool,
    /// Within `band` metres of it the forest is `dense` times as thick as the world's (1: as it is).
    #[serde(default = "one")]
    pub dense: f64,
    #[serde(default)]
    pub band: f64,
    pub points: Vec<TrailPoint>,
}

/// A point of a trail, in the course's frame, the trail's width there, m, and the height its
/// crowns are cleared to there if not the trail's (a clearing open to the sky has a high one).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrailPoint {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    #[serde(default)]
    pub clearance: Option<f64>,
}

/// The racer's pace as a course's file gives it.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PaceFile {
    top: f64,
    straight: f64,
    hairpin: f64,
    braking: f64,
    tilt_deg: f64,
    floor: f64,
    #[serde(default)]
    follow_ground: bool,
    #[serde(default = "five")]
    yaw_rate_deg: f64,
}

/// `Pace::default()`'s yaw rate, about 286°/s, in degrees a second.
fn five() -> f64 {
    5.0 / crate::math::DEG
}

/// A course: see the module.
#[derive(Clone, Debug, PartialEq)]
pub struct Course {
    /// The file's name without `.ron`: what names it on the command line and in the settings.
    pub key: String,
    pub name: String,
    pub description: String,
    pub place: Place,
    pub frame: Frame,
    /// Where the racers start (x, y) and the way they face (degrees), in the frame.
    pub start: (f64, f64, f64),
    pub laps: u32,
    pub gates: Vec<CourseGate>,
    /// The multirotor it is raced in, by its key, if the course says.
    pub quad: Option<String>,
    /// Its racer's pace.
    pub pace: super::racer::Pace,
    /// The trail cut for it through the forest, if there is one.
    pub trail: Option<Trail>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CourseFile {
    name: String,
    #[serde(default)]
    description: String,
    place: Place,
    frame: Frame,
    start: StartFile,
    laps: u32,
    #[serde(default)]
    quad: Option<String>,
    #[serde(default)]
    racer: Option<PaceFile>,
    #[serde(default)]
    trail: Option<Trail>,
    gates: Vec<CourseGate>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartFile {
    x: f64,
    y: f64,
    heading: f64,
}

/// Reads a course from the text of its file; `file` names it in the errors and gives its key.
pub fn load(text: &str, file: &str) -> Result<Course, FileError> {
    let error = |message: String| FileError { file: file.to_string(), message };
    // `quad`, `racer` and `trail` may be left out; given, they are written plainly (no `Some`).
    let options = ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    let parsed: CourseFile = options.from_str(text).map_err(|e| error(e.to_string()))?;
    if parsed.gates.len() < 2 {
        return Err(error("a course needs two gates at least".to_string()));
    }
    if parsed.gates[0].kind == GateKind::Flag || parsed.gates[0].kind == GateKind::Dive {
        return Err(error("the first gate is the start and finish line: not a flag nor a dive gate".to_string()));
    }
    if parsed.laps == 0 || parsed.laps > 20 {
        return Err(error("laps: 1 to 20".to_string()));
    }
    if let Some((i, g)) = parsed.gates.iter().enumerate().find(|(_, g)| !(g.scale > 0.2 && g.scale < 5.0)) {
        return Err(error(format!("gate {i} ({:?}): scale 0.2 to 5", g.kind)));
    }
    if let Some(t) = &parsed.trail {
        if t.points.len() < 2 || t.points.iter().any(|p| !(p.width > 0.0 && p.width < 100.0)) || !(t.clearance >= 0.0) {
            return Err(error("trail: two points at least, each 0 to 100 m wide, the clearance not negative".to_string()));
        }
    }
    let pace = match parsed.racer {
        Some(p) => {
            if !(p.top > 0.0 && p.straight > 0.0 && p.hairpin > 0.0 && p.braking > 0.0 && p.tilt_deg > 0.0 && p.tilt_deg < 85.0 && p.floor >= 0.0) {
                return Err(error("racer: speeds and braking positive, tilt_deg 0 to 85, floor not negative".to_string()));
            }
            super::racer::Pace { top: p.top, straight: p.straight, hairpin: p.hairpin, braking: p.braking, tilt: p.tilt_deg * crate::math::DEG, floor: p.floor, follow_ground: p.follow_ground, yaw_rate: p.yaw_rate_deg * crate::math::DEG }
        }
        None => super::racer::Pace::default(),
    };
    let key = file.rsplit('/').next().unwrap_or(file).trim_end_matches(".ron").to_string();
    Ok(Course {
        key,
        name: parsed.name,
        description: parsed.description,
        place: parsed.place,
        frame: parsed.frame,
        start: (parsed.start.x, parsed.start.y, parsed.start.heading),
        laps: parsed.laps,
        gates: parsed.gates,
        quad: parsed.quad,
        pace,
        trail: parsed.trail,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShapesFile {
    shapes: Vec<ShapeFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShapeFile {
    kind: GateKind,
    name: String,
    opening: Opening,
    tube: f64,
    parts: Vec<Part>,
    number: (f64, f64, f64),
}

/// Reads the kinds of gate from the text of their file: every kind once.
pub fn load_shapes(text: &str, file: &str) -> Result<Vec<GateShape>, FileError> {
    let error = |message: String| FileError { file: file.to_string(), message };
    let parsed: ShapesFile = ron::from_str(text).map_err(|e| error(e.to_string()))?;
    let shapes: Vec<GateShape> = parsed.shapes.into_iter().map(|s| GateShape { kind: s.kind, name: s.name, opening: s.opening, tube: s.tube, parts: s.parts, number: s.number }).collect();
    for kind in GateKind::ALL {
        if shapes.iter().filter(|s| s.kind == kind).count() != 1 {
            return Err(error(format!("{kind:?}: every kind of gate once")));
        }
    }
    Ok(shapes)
}

/// The kinds of gate, in the order of `GateKind::ALL`.
pub fn shapes() -> &'static [GateShape] {
    static SHAPES: OnceLock<Vec<GateShape>> = OnceLock::new();
    SHAPES.get_or_init(|| {
        let mut shapes = load_shapes(GATES.1, GATES.0).unwrap_or_else(|e| panic!("{e}"));
        shapes.sort_by_key(|s| s.kind.index());
        shapes
    })
}

static COURSES: OnceLock<Vec<Course>> = OnceLock::new();

/// The courses there are: those given to `set_courses`, or else the built-in ones.
pub fn courses() -> &'static [Course] {
    COURSES.get_or_init(|| BUILT_IN.iter().map(|(file, text)| load(text, file).unwrap_or_else(|e| panic!("{e}"))).collect())
}

/// The course of this key, if there is one.
pub fn course(key: &str) -> Option<&'static Course> {
    courses().iter().find(|c| c.key.eq_ignore_ascii_case(key))
}

/// Makes `list` the courses, before anything has asked for them (the game does it at start).
pub fn set_courses(list: Vec<Course>) -> Result<(), String> {
    COURSES.set(list).map_err(|_| "the courses were already in use when they were read".to_string())
}

/// The built-in courses with these files' in their place: a file of a built-in course's key
/// replaces it, any other is added after them, in the order given.
pub fn courses_with(files: &[(String, String)]) -> Result<Vec<Course>, FileError> {
    let mut list: Vec<Course> = Vec::new();
    for (file, text) in BUILT_IN.iter().map(|(f, t)| (*f, *t)).chain(files.iter().map(|(f, t)| (f.as_str(), t.as_str()))) {
        let c = load(text, file)?;
        match list.iter_mut().find(|old| old.key == c.key) {
            Some(old) => *old = c,
            None => list.push(c),
        }
    }
    Ok(list)
}
