//! Circuits as data: `assets/circuits/<key>.ron` read into a `CircuitFile` and checked, and the
//! circuits there are (`circuits`). As `aircraft::file`, nothing here reads the disk: the game hands
//! the files' text over (`circuits_with`, `set_circuits`); without that the circuits are those built
//! into the program.
//!
//! A circuit's file (the built-in one, `assets/circuits/lakeside.ron`, is the full example):
//!
//! ```ron
//! (
//!     name: "Lakeside Park",
//!     description: "...",
//!     place: Seed(1),             // the generated world of that seed
//!     origin: (1000, 21700),      // north, east of the world: the points are metres from here
//!     width: 12,                  // of the asphalt, metres
//!     start: 300,                 // the start and finish line, metres along from the first point
//!     laps: 3,
//!     sides: (runoff: Grass, runoff_width: 14, barrier: Armco, kerbs: Auto),   // both sides, until changed
//!     points: [
//!         (n: -700, e: -500),
//!         (n: -300, e: -505, left: (runoff: Asphalt, pit: true)),           // a side changed from here on
//!         (n: 150, e: -480, width: 13, both: (runoff: Gravel, runoff_width: 30, barrier: Tyres)),
//!         (n: 400, e: -300, lift: 4, bank: 3),   // the ground 4 m higher here; banked 3 % (right side lower)
//!         ...
//!     ],
//!     props: [ (kind: Grandstand, along: 120, side: Left, length: 90) ],
//! )
//! ```

use std::sync::OnceLock;

use serde::Deserialize;

use super::Circuit;
use crate::aircraft::file::FileError;
use crate::terrain::Terrain;

/// The circuits of `assets/circuits` built into the program: (where it is in the repository, its text).
pub const BUILT_IN: [(&str, &str); 1] = [("assets/circuits/lakeside.ron", include_str!("../../../../../assets/circuits/lakeside.ron"))];

/// Where a circuit is.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub enum Place {
    /// The world generated from this seed.
    Seed(u64),
}

/// What lies between the asphalt (with its kerb) and the barrier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Default)]
pub enum Runoff {
    #[default]
    Grass,
    Gravel,
    Asphalt,
}

/// The barrier at the end of the run-off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Default)]
pub enum BarrierKind {
    /// None: the run-off goes on into the land.
    None,
    /// Steel guard rail on posts.
    #[default]
    Armco,
    /// A wall of tyres in front of the rail, where cars arrive fast and square.
    Tyres,
    /// Concrete, as along the pits.
    Wall,
}

/// Where a side has kerbs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Default)]
pub enum Kerbs {
    /// In the corners tighter than `KERB_RADIUS`.
    #[default]
    Auto,
    Always,
    Never,
}

/// Left or right of the way the circuit is driven.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Default)]
pub enum Side {
    #[default]
    Left,
    Right,
}

/// One side of the circuit, from a point on.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SideSpec {
    #[serde(default)]
    pub runoff: Runoff,
    /// Metres from the asphalt's edge to the barrier.
    pub runoff_width: f64,
    #[serde(default)]
    pub barrier: BarrierKind,
    #[serde(default)]
    pub kerbs: Kerbs,
    /// Behind the barrier, the pit lane (paved, wider).
    #[serde(default)]
    pub pit: bool,
}

/// What a point changes of a side: the fields it gives.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SideChange {
    pub runoff: Option<Runoff>,
    pub runoff_width: Option<f64>,
    pub barrier: Option<BarrierKind>,
    pub kerbs: Option<Kerbs>,
    pub pit: Option<bool>,
}

impl SideSpec {
    /// This side with a point's changes.
    pub fn with(&self, c: &SideChange) -> SideSpec {
        SideSpec {
            runoff: c.runoff.unwrap_or(self.runoff),
            runoff_width: c.runoff_width.unwrap_or(self.runoff_width),
            barrier: c.barrier.unwrap_or(self.barrier),
            kerbs: c.kerbs.unwrap_or(self.kerbs),
            pit: c.pit.unwrap_or(self.pit),
        }
    }
}

/// A point the centre line goes through, and what changes there.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointFile {
    /// Metres north and east of the circuit's origin.
    pub n: f64,
    pub e: f64,
    /// The asphalt's width from here on.
    #[serde(default)]
    pub width: Option<f64>,
    /// Metres the circuit's ground is raised (lowered if negative) over the smoothed natural ground here.
    #[serde(default)]
    pub lift: Option<f64>,
    /// The cross slope here, per cent, positive with the right side lower (instead of the usual one).
    #[serde(default)]
    pub bank: Option<f64>,
    #[serde(default)]
    pub left: Option<SideChange>,
    #[serde(default)]
    pub right: Option<SideChange>,
    #[serde(default)]
    pub both: Option<SideChange>,
}

/// The kinds of thing placed beside a circuit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum PropKind {
    /// Rows of seats under a roof, facing the track.
    Grandstand,
    /// The pits' garages: a long low building facing the pit lane.
    Pits,
    /// The gantry over the start line with its lights.
    Gantry,
    /// A marshal's post: a small shelter on a platform behind the barrier.
    Marshal,
    /// A footbridge over the track, with advertising boards.
    Bridge,
    /// A floodlight's tower, which lights the circuit by night: `offset` metres beyond the end
    /// of the circuit's ground on its side. A circuit that places none gets one every 90 m.
    Floodlight,
}

/// A thing placed beside the circuit.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prop {
    pub kind: PropKind,
    /// Metres along the circuit of its middle.
    pub along: f64,
    #[serde(default)]
    pub side: Side,
    /// Its length along the circuit, metres.
    #[serde(default)]
    pub length: f64,
    /// Metres further out than the barrier (or the run-off's end) on its side.
    #[serde(default)]
    pub offset: f64,
}

/// A circuit's file: see the module.
#[derive(Clone, Debug, PartialEq)]
pub struct CircuitFile {
    /// The file's name without `.ron`.
    pub key: String,
    pub name: String,
    pub description: String,
    pub place: Place,
    pub origin: (f64, f64),
    pub width: f64,
    pub start: f64,
    pub laps: u32,
    /// The cross fall on the straights, per cent (the right side lower).
    pub fall: f64,
    /// Over how many metres the natural ground's heights are smoothed into the circuit's.
    pub smoothing: f64,
    pub sides: SideSpec,
    pub points: Vec<PointFile>,
    pub props: Vec<Prop>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Parsed {
    name: String,
    #[serde(default)]
    description: String,
    place: Place,
    origin: (f64, f64),
    width: f64,
    start: f64,
    laps: u32,
    #[serde(default = "default_fall")]
    fall: f64,
    #[serde(default = "default_smoothing")]
    smoothing: f64,
    sides: SideSpec,
    points: Vec<PointFile>,
    #[serde(default)]
    props: Vec<Prop>,
}

fn default_fall() -> f64 {
    1.2
}

fn default_smoothing() -> f64 {
    150.0
}

/// Reads a circuit from the text of its file; `file` names it in the errors and gives its key.
pub fn load(text: &str, file: &str) -> Result<CircuitFile, FileError> {
    let error = |message: String| FileError { file: file.to_string(), message };
    // `Some(..)` left implicit: a point's `width: 13` rather than `width: Some(13)`.
    let options = ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    let p: Parsed = options.from_str(text).map_err(|e| error(e.to_string()))?;
    if p.points.len() < 4 {
        return Err(error("a circuit needs four points at least".to_string()));
    }
    if !(4.0..=30.0).contains(&p.width) {
        return Err(error(format!("width {} m: it must be 4 to 30", p.width)));
    }
    for (i, pt) in p.points.iter().enumerate() {
        let next = &p.points[(i + 1) % p.points.len()];
        if (next.n - pt.n).hypot(next.e - pt.e) < 5.0 {
            return Err(error(format!("points {} and {} are less than 5 m apart", i, (i + 1) % p.points.len())));
        }
    }
    let key = std::path::Path::new(file).file_stem().and_then(|s| s.to_str()).unwrap_or(file).to_lowercase();
    Ok(CircuitFile {
        key,
        name: p.name,
        description: p.description,
        place: p.place,
        origin: p.origin,
        width: p.width,
        start: p.start,
        laps: p.laps.max(1),
        fall: p.fall,
        smoothing: p.smoothing,
        sides: p.sides,
        points: p.points,
        props: p.props,
    })
}

static CIRCUITS: OnceLock<Vec<CircuitFile>> = OnceLock::new();

/// The built-in circuits with `files` (path, text) put in place of the built-in one of the same
/// key, or after them. A file that does not read is left out, and said in the errors.
pub fn circuits_with(files: &[(String, String)]) -> (Vec<CircuitFile>, Vec<FileError>) {
    let mut all: Vec<CircuitFile> = BUILT_IN.iter().map(|(path, text)| load(text, path).expect("a built-in circuit reads")).collect();
    let mut errors = Vec::new();
    for (path, text) in files {
        match load(text, path) {
            Ok(c) => match all.iter_mut().find(|o| o.key == c.key) {
                Some(slot) => *slot = c,
                None => all.push(c),
            },
            Err(e) => errors.push(e),
        }
    }
    (all, errors)
}

/// Makes these the circuits, once, before anything asks for them; false if they were already set.
pub fn set_circuits(all: Vec<CircuitFile>) -> bool {
    CIRCUITS.set(all).is_ok()
}

/// The circuits: those set, or the built-in ones.
pub fn circuits() -> &'static [CircuitFile] {
    CIRCUITS.get_or_init(|| circuits_with(&[]).0)
}

impl CircuitFile {
    /// One of the circuits by its key or a word of its name.
    pub fn find(word: &str) -> Option<&'static CircuitFile> {
        let w = word.to_lowercase();
        circuits().iter().find(|c| c.key == w || c.name.to_lowercase().split_whitespace().any(|x| x == w))
    }

    /// The world it is in: its seed.
    pub fn seed(&self) -> u64 {
        match self.place {
            Place::Seed(s) => s,
        }
    }

    /// Laid out on the natural terrain of its place.
    pub fn lay_out(&self, terrain: &Terrain) -> Circuit {
        Circuit::new(self, terrain)
    }
}
