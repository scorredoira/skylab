//! The lights of an aerodrome, as data: where each one stands, its colour, how bright it is and in
//! which directions, whether it flashes, and which system it belongs to. Like `scenery`, a pure
//! function of the aerodrome and the terrain; the world's renderer draws them.
//!
//! What an aerodrome has follows from its class (`Class::of`), and where each light goes, its
//! colour and its intensity follow ICAO Annex 14, Volume I (chapter 5 and its appendices 1 and 2)
//! and the FAA's AC 150/5340-30, taken as references, for a runway of code number 2 (800 to
//! 1200 m) such as every runway of the generated world:
//!
//! - **Runway edge**: every 60 m at most, 1.5 m outside the edges, elevated. White; on an
//!   instrument runway yellow towards the far end over its last third or 600 m, whichever is less
//!   (Annex 14, 5.3.9.7). High intensity (10 000 cd, toed in 3.5°, with 50 cd all round) on
//!   precision approach runways, medium (100 cd all round) on the others.
//! - **Threshold and runway end**: a row across each end, green outwards for those landing and red
//!   inwards for those running out of runway; every 3 m on a precision approach runway, in two
//!   groups at the sides on the others.
//! - **Centre line** (category II): every 15 m, 0.6 m off the line; white, then red and white from
//!   the middle of a runway shorter than 1800 m to 300 m before its end, red over the last 300 m.
//! - **Touchdown zone** (category II): barrettes of three each side of the centre line every 30 m
//!   over the first half of the runway (under 1800 m long; 900 m on longer ones).
//! - **Approach lighting**, on the main approach only: a MALSR for category I (five-light barrettes
//!   every 60 m out to 420 m, a bar across at 300 m, five sequenced flashers to 730 m) and an
//!   ALSF-2 for category II (barrettes every 30 m out to 730 m, red side rows over the first 300 m,
//!   bars across at 150 and 300 m, sequenced flashers from 300 m out). Level with the threshold
//!   on masts where the ground falls away, on the ground where it rises.
//! - **Runway end identifier lights** (REIL): two white strobes beside the threshold of every
//!   non-precision approach and of the other end of a precision runway.
//! - **PAPI**: four units on the left of each runway, 15 m from the edge and 9 m apart, beside the
//!   point the approach aims at (`AIM`); from the runway outwards they turn white above 3°30',
//!   3°10', 2°50' and 2°30', so that on the 3° path two are red and two white. The transition
//!   from red to white takes 3' of arc (Annex 14, 5.3.5.25). The small fields have a two-bar VASI
//!   instead: red over white is on the path.
//! - **Taxiway and apron edges**: blue, all round, 8 cd.
//! - **Apron floodlights** on two masts 15 m tall, giving about 20 lux on the stands.
//! - **Aerodrome beacon** on the office's mast: white and green beams, 12 turns a minute.
//! - **Obstacle lights**, low intensity type B (32 cd red, Annex 14 Table 6-3): on the windsock's
//!   pole and on both ends of every hangar's ridge.
//!
//! Intensities are of the main beam at the highest setting; how much of it is lit is `setting`,
//! the stage a tower would choose for the light of the day and the visibility.

use crate::math::{hash2, DEG};
use crate::scenery::{self, ObstacleKind, Shape};
use crate::terrain::{Airport, Terrain};
use crate::V3;

/// Distance from the threshold at which the approaches aim, metres: where the 3° path meets the
/// runway (`Vehicle::reset_on_final`, `Autopilot`'s `aim`), and where the PAPI stands.
pub const AIM: f64 = 150.0;
/// The angle of the approach path the PAPI and VASI show, radians.
pub const GLIDE_PATH: f64 = 3.0 * DEG;

/// What an aerodrome's runway is equipped for, from the approaches flown to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// Flown to by eye only: medium intensity lights and a VASI.
    Visual,
    /// With an instrument approach to a minimum height of its own: medium intensity lights, the
    /// strobes at each threshold and a PAPI.
    NonPrecision,
    /// A category I instrument landing system: high intensity lights and a MALSR on the main approach.
    PrecisionOne,
    /// Category II: an ALSF-2, centre line and touchdown zone lights as well.
    PrecisionTwo,
}

impl Class {
    /// The class of an aerodrome: its data's (`Detail::lighting`), or, for a generated one, decided
    /// by its id: some 35 % visual, 30 % non-precision, 20 % category I and 15 % category II.
    pub fn of(a: &Airport) -> Class {
        if let Some(class) = a.detail.lighting {
            return class;
        }
        let r = hash2(a.id as i64, 1, 0x11_6475);
        if r < 0.35 {
            Class::Visual
        } else if r < 0.65 {
            Class::NonPrecision
        } else if r < 0.85 {
            Class::PrecisionOne
        } else {
            Class::PrecisionTwo
        }
    }

    pub fn precision(self) -> bool {
        matches!(self, Class::PrecisionOne | Class::PrecisionTwo)
    }

    pub fn instrument(self) -> bool {
        self != Class::Visual
    }
}

/// Which end of an aerodrome's runway has the precision approach: `true` when it is the one
/// landed on towards `heading + π` (from the end at `+length / 2`), as `Start::Final`'s `reversed`.
pub fn main_end_reversed(a: &Airport) -> bool {
    hash2(a.id as i64, 2, 0x11_6475) < 0.5
}

/// The aviation colours of Annex 14, Appendix 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    /// Variable white of filament lamps (about 2800 K).
    White,
    /// The white of a discharge lamp: strobes and sequenced flashers.
    Strobe,
    Yellow,
    Green,
    Red,
    Blue,
    /// The neutral white (4000 K) of the apron's floodlights.
    Flood,
}

/// The systems an aerodrome's lights belong to: each is set brighter or dimmer, or switched off,
/// as one (`setting`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum System {
    /// Edge, threshold, end, centre line and touchdown zone lights of the runway.
    Runway,
    Approach,
    /// Sequenced flashers and runway end identifier lights.
    Strobes,
    /// PAPI or VASI.
    Slope,
    /// Taxiway and apron edges.
    Taxiway,
    /// Floodlights.
    Apron,
    Beacon,
    Obstacle,
}

impl System {
    pub const ALL: [System; 8] = [System::Runway, System::Approach, System::Strobes, System::Slope, System::Taxiway, System::Apron, System::Beacon, System::Obstacle];
}

/// The main beam of a light: intensity falls to half at `half_width` either side of its axis
/// horizontally and `half_height` above and below it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Beam {
    /// The axis: radians from north, and above the horizon.
    pub heading: f64,
    pub elevation: f64,
    pub half_width: f64,
    pub half_height: f64,
    /// How fast the axis turns about the vertical, radians per second (a beacon).
    pub spin: f64,
}

/// A slope indicator's unit: red below `angle` above the horizon, white above, changing over
/// `transition` (the whole width, radians).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slope {
    pub angle: f64,
    pub transition: f64,
}

/// A light that flashes: for `duration` seconds every `period`, starting `offset` seconds into it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flash {
    pub period: f64,
    pub offset: f64,
    pub duration: f64,
}

/// One light.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    /// Where it shines from: north, east, down (world axes), metres.
    pub at: V3,
    pub color: Color,
    /// Of the main beam at the highest setting, candelas. A slope indicator's white; its red is
    /// what the red filter lets through of it.
    pub intensity: f64,
    pub beam: Option<Beam>,
    /// Shone all round above the horizon, besides the beam, candelas.
    pub omni: f64,
    pub slope: Option<Slope>,
    pub flash: Option<Flash>,
    /// Radius of what shines (the lens), metres: how big it looks from near.
    pub radius: f64,
    pub system: System,
}

/// The two ends of the ridge of a building's block, where its obstacle lights go (north, east, down).
fn ridge_ends(o: &scenery::Obstacle) -> Vec<V3> {
    let Shape::Block { block: b, yaw, scale } = o.shape else { return Vec::new() };
    let (c, s) = (yaw.cos(), yaw.sin());
    // Model x and z to east and south, as `Obstacle::contains` takes them back.
    let world = |x: f64, z: f64, up: f64| {
        let (x, z) = ((b.x + x) * scale, (b.z + z) * scale);
        let (east, south) = (x * c - z * s, x * s + z * c);
        V3::new(o.n - south, o.e + east, -(o.base + up * scale))
    };
    let top = b.eaves + b.ridge;
    if b.ridge_z {
        vec![world(0.0, -b.half_z, top), world(0.0, b.half_z, top)]
    } else {
        vec![world(-b.half_x, 0.0, top), world(b.half_x, 0.0, top)]
    }
}

/// Where an aerodrome's windsock stands (north, east): on the side of the runway away from the
/// apron (to the left of `heading` on a generated one) near the end landed from along `heading`,
/// clear of the strip's edge lights. A runway without the apron (`Detail::apron`) has none.
pub fn windsock(a: &Airport) -> Option<(f64, f64)> {
    let along = -(a.length / 2.0 - 120.0);
    let across = -(a.width / 2.0 + 32.0) * a.detail.apron;
    (a.detail.apron != 0.0).then(|| position(a, along, across))
}

/// Height of the windsock's pole, metres.
pub const WINDSOCK_POLE: f64 = 6.0;
/// Height of the apron's floodlight masts, and how far beyond the runway's edge they stand, metres.
pub const FLOOD_MAST: f64 = 15.0;
const FLOOD_MAST_ACROSS: f64 = 106.0;

/// A point given along and across (to the right of) the runway from its middle.
fn position(a: &Airport, along: f64, across: f64) -> (f64, f64) {
    let (c, s) = (a.heading.cos(), a.heading.sin());
    (a.n + along * c - across * s, a.e + along * s + across * c)
}

/// Every light of an aerodrome.
pub fn aerodrome(terrain: &Terrain, a: &Airport) -> Vec<Light> {
    let mut b = Builder { a: *a, lights: Vec::new() };
    let class = Class::of(a);
    let (hl, hw) = (a.length / 2.0, a.width / 2.0);
    let high = class.precision();
    let main_reversed = main_end_reversed(a);

    // The two ways the runway is landed on: `way` +1 along the heading (threshold at -hl), -1 the other way.
    for way in [1.0, -1.0] {
        let main = (way < 0.0) == main_reversed;
        let approach = class.precision() && main;

        // Runway edge lights, as seen from those landing this way.
        let intervals = (a.length / 60.0).ceil();
        let caution = if class.instrument() { (a.length / 3.0).min(600.0) } else { 0.0 };
        for k in 0..=intervals as usize {
            let along = -hl + a.length * k as f64 / intervals;
            for side in [-1.0, 1.0] {
                // Distance still to go along the runway, landing this way.
                let to_go = hl - along * way;
                let color = if to_go < caution - 1.0 { Color::Yellow } else { Color::White };
                let at = b.on_runway(along, side * (hw + 1.5), 0.35);
                let light = if high {
                    // Toed in 3.5° towards the centre line, ICAO Appendix 2, figure A2-9; 50 cd all round
                    // from one of the two (the omnidirectional part is one lamp for both ways).
                    let toe = side * way * 3.5 * DEG;
                    let beam = b.beam(way, toe, 3.5, 7.0, 4.5);
                    let omni = if way > 0.0 { 50.0 } else { 0.0 };
                    Light { at, color, intensity: 10_000.0 * share(color), beam: Some(beam), omni, slope: None, flash: None, radius: 0.1, system: System::Runway }
                } else {
                    // Medium intensity: one omnidirectional lamp, behind a yellow half where it shows yellow.
                    let half = b.beam(way, 0.0, 3.0, 60.0, 30.0);
                    Light { at, color, intensity: 100.0 * share(color), beam: Some(half), omni: 0.0, slope: None, flash: None, radius: 0.08, system: System::Runway }
                };
                b.lights.push(light);
            }
        }

        // Threshold (green, outwards) and the end beyond it (red, seen from the runway): the row
        // at the end this way lands from. Its red is the end of the runway landing the other way.
        let end = -(hl + 1.0) * way;
        let across: Vec<f64> = if high {
            (0..=8).map(|k| -hw + a.width * k as f64 / 8.0).collect()
        } else {
            [-hw, -hw + 3.0, -hw + 6.0, hw - 6.0, hw - 3.0, hw].to_vec()
        };
        for &x in &across {
            let (green, red) = if high {
                (Light { intensity: 10_000.0, beam: Some(b.beam(way, 0.0, 5.5, 8.0, 5.5)), radius: 0.1, ..b.plain(end, x, 0.1, Color::Green) },
                 Light { intensity: 2_500.0, beam: Some(b.beam(-way, 0.0, 3.0, 7.0, 4.0)), radius: 0.1, ..b.plain(end, x, 0.1, Color::Red) })
            } else {
                (Light { intensity: 100.0, beam: Some(b.beam(way, 0.0, 3.0, 60.0, 30.0)), ..b.plain(end, x, 0.35, Color::Green) },
                 Light { intensity: 50.0, beam: Some(b.beam(-way, 0.0, 3.0, 60.0, 30.0)), ..b.plain(end, x, 0.35, Color::Red) })
            };
            b.lights.push(green);
            b.lights.push(red);
        }

        // Centre line and touchdown zone, inset, for category II.
        if class == Class::PrecisionTwo {
            let alternate = if a.length < 1800.0 { a.length / 2.0 } else { 900.0 };
            let count = (a.length / 15.0) as usize;
            for k in 1..count {
                let along = -hl + 15.0 * k as f64;
                let to_go = hl - along * way;
                let color = if to_go < 300.0 || (to_go < alternate && k % 2 == 0) { Color::Red } else { Color::White };
                let at = b.on_runway(along, 0.6, 0.03);
                b.lights.push(Light { at, color, intensity: 5_000.0 * share(color), beam: Some(b.beam(way, 0.0, 4.5, 6.0, 4.5)), radius: 0.06, ..b.plain(0.0, 0.0, 0.0, color) });
            }
            let zone = if a.length < 1800.0 { a.length / 2.0 } else { 900.0 };
            for row in 1..=(zone / 30.0) as usize {
                let along = (-hl + 30.0 * row as f64) * way;
                for side in [-1.0, 1.0] {
                    for j in 0..3 {
                        let at = b.on_runway(along, side * (4.8 + 1.5 * j as f64), 0.03);
                        b.lights.push(Light { at, intensity: 5_000.0, beam: Some(b.beam(way, 0.0, 6.0, 6.0, 6.0)), radius: 0.06, ..b.plain(0.0, 0.0, 0.0, Color::White) });
                    }
                }
            }
        }

        // Approach lights on the main approach of a precision runway; strobes at the other thresholds.
        if approach {
            b.approach(terrain, way, class);
        } else if class.instrument() {
            for side in [-1.0, 1.0] {
                let at = b.on_runway(-hl * way, side * (hw + 12.0), 0.6);
                // Aimed 10° up and 15° outwards, flashing together twice a second, FAA L-849.
                let beam = b.beam(way, -side * way * 15.0 * DEG, 10.0, 15.0, 10.0);
                let flash = Flash { period: 0.5, offset: 0.0, duration: 0.04 };
                b.lights.push(Light { at, color: Color::Strobe, intensity: 15_000.0, beam: Some(beam), omni: 0.0, slope: None, flash: Some(flash), radius: 0.12, system: System::Strobes });
            }
        }

        // The slope indicator, on the left as seen landing this way.
        let left = -way;
        if class == Class::Visual {
            // VASI: a downwind bar beside the aiming point, white above 2.5°, and an upwind bar
            // 213 m (700 ft) further, white above 3.5°.
            for (distance, angle) in [(AIM, 2.5), (AIM + 213.0, 3.5)] {
                let at = b.on_runway((-hl + distance) * way, left * (hw + 15.0), 0.6);
                let slope = Slope { angle: angle * DEG, transition: 0.25 * DEG };
                b.lights.push(Light { at, intensity: 10_000.0, beam: Some(b.beam(way, 0.0, 4.0, 10.0, 4.0)), slope: Some(slope), radius: 0.25, system: System::Slope, ..b.plain(0.0, 0.0, 0.0, Color::White) });
            }
        } else {
            for k in 0..4 {
                let at = b.on_runway((-hl + AIM) * way, left * (hw + 15.0 + 9.0 * k as f64), 0.6);
                let angle = GLIDE_PATH + (0.5 - k as f64 / 3.0) * DEG;
                let slope = Slope { angle, transition: 3.0 / 60.0 * DEG };
                b.lights.push(Light { at, intensity: 15_000.0, beam: Some(b.beam(way, 0.0, 4.5, 10.0, 4.5)), slope: Some(slope), radius: 0.2, system: System::Slope, ..b.plain(0.0, 0.0, 0.0, Color::White) });
            }
        }
    }

    // The taxiway to the apron (on its side of the runway at its middle, 10 m wide) and the apron's
    // edge (from 38 to 102 m beyond the runway's edge, 120 m long), as `airport_paint` draws them.
    // A runway without the apron has none of it, nor its floodlights.
    let side = a.detail.apron;
    let taxi = |b: &mut Builder, along: f64, across: f64| {
        let at = b.on_runway(along, across * side, 0.3);
        b.lights.push(Light { at, omni: 8.0, intensity: 0.0, beam: None, radius: 0.05, system: System::Taxiway, ..b.plain(0.0, 0.0, 0.0, Color::Blue) });
    };
    if side != 0.0 {
        for x in [8.0, 20.0, 32.0] {
            for y in [-6.5, 6.5] {
                taxi(&mut b, y, hw + x);
            }
        }
        let (near, far, half) = (hw + 36.5, hw + 103.5, 61.5);
        for k in 0..=8 {
            let y = -half + 2.0 * half * k as f64 / 8.0;
            if y.abs() > 8.0 {
                taxi(&mut b, y, near);
            }
            taxi(&mut b, y, far);
        }
        for k in 1..4 {
            let x = near + (far - near) * k as f64 / 4.0;
            taxi(&mut b, -half, x);
            taxi(&mut b, half, x);
        }
    }

    // Floodlights on two masts beyond the apron, in the gaps between the hangars and the office,
    // aimed down at the stands.
    for (along, aim_along) in [(-26.5, -35.0), (8.5, 0.0)].into_iter().filter(|_| side != 0.0) {
        let at = b.on_runway(along, (FLOOD_MAST_ACROSS + hw) * side, FLOOD_MAST);
        let (an, ae) = position(a, aim_along, (hw + 68.0) * side);
        let (dn, de, down) = (an - at.x, ae - at.y, FLOOD_MAST);
        let beam = Beam { heading: de.atan2(dn), elevation: -(down.atan2(dn.hypot(de))), half_width: 30.0 * DEG, half_height: 20.0 * DEG, spin: 0.0 };
        b.lights.push(Light { at, color: Color::Flood, intensity: 50_000.0, beam: Some(beam), omni: 300.0, slope: None, flash: None, radius: 0.35, system: System::Apron });
    }

    // Obstacle lights and the beacon, on the office's mast.
    for o in scenery::aerodrome_obstacles(terrain, a) {
        match o.kind {
            ObstacleKind::Hangar => {
                for at in ridge_ends(&o) {
                    b.lights.push(Light { at: at - V3::new(0.0, 0.0, 0.3), ..b.obstacle() });
                }
            }
            ObstacleKind::Building => {
                let Shape::Block { block, yaw, .. } = o.shape else { continue };
                // The office's tower, the block that rises highest: the beacon tops the mast over its middle, 3.5 m tall.
                if block.eaves + block.ridge > 9.0 {
                    let (east, south) = (block.x * yaw.cos() - block.z * yaw.sin(), block.x * yaw.sin() + block.z * yaw.cos());
                    let at = V3::new(o.n - south, o.e + east, -(o.base + block.eaves + block.ridge + 3.8));
                    for (color, offset, intensity) in [(Color::White, 0.0, 60_000.0), (Color::Green, std::f64::consts::PI, 12_000.0)] {
                        // 12 turns a minute: a flash of each colour every 5 s, about 0.15 s long.
                        let beam = Beam { heading: a.heading + offset, elevation: 4.0 * DEG, half_width: 4.0 * DEG, half_height: 4.0 * DEG, spin: 12.0 / 60.0 * std::f64::consts::TAU };
                        b.lights.push(Light { at, color, intensity, beam: Some(beam), omni: 0.0, slope: None, flash: None, radius: 0.25, system: System::Beacon });
                    }
                }
            }
            _ => {}
        }
    }
    if let Some((n, e)) = windsock(a) {
        b.lights.push(Light { at: V3::new(n, e, -(a.elevation + WINDSOCK_POLE + 0.15)), ..b.obstacle() });
    }
    b.lights
}

/// The share of a white light's intensity the coloured filters let through (Annex 14, Appendix 2,
/// collective notes): yellow 0.4, red 0.15; green lights have figures of their own.
fn share(color: Color) -> f64 {
    match color {
        Color::Yellow => 0.4,
        Color::Red => 0.15,
        _ => 1.0,
    }
}

struct Builder {
    a: Airport,
    lights: Vec<Light>,
}

impl Builder {
    /// A point `height` metres over the runway's level, along and across (to the right of) it from its middle.
    fn on_runway(&self, along: f64, across: f64, height: f64) -> V3 {
        let (n, e) = position(&self.a, along, across);
        V3::new(n, e, -(self.a.elevation + height))
    }

    /// A steady light with no beam, at a point of the runway: the rest of a `Light { .. }`.
    fn plain(&self, along: f64, across: f64, height: f64, color: Color) -> Light {
        Light { at: self.on_runway(along, across, height), color, intensity: 0.0, beam: None, omni: 0.0, slope: None, flash: None, radius: 0.1, system: System::Runway }
    }

    /// A beam towards those landing `way` (+1 along the heading), so pointing back down the
    /// approach, turned `turn` radians (positive clockwise from above), `elevation`° up, with its
    /// half width and height in degrees.
    fn beam(&self, way: f64, turn: f64, elevation: f64, half_width: f64, half_height: f64) -> Beam {
        let back = self.a.heading + if way > 0.0 { std::f64::consts::PI } else { 0.0 };
        Beam { heading: back + turn, elevation: elevation * DEG, half_width: half_width * DEG, half_height: half_height * DEG, spin: 0.0 }
    }

    /// A low intensity obstacle light, type B: 32 cd of red all round.
    fn obstacle(&self) -> Light {
        Light { omni: 32.0, radius: 0.06, system: System::Obstacle, ..self.plain(0.0, 0.0, 0.0, Color::Red) }
    }

    /// The approach lights for those landing `way`, out from the threshold at `-hl * way`.
    fn approach(&mut self, terrain: &Terrain, way: f64, class: Class) {
        let a = self.a;
        let hl = a.length / 2.0;
        // Out over the approach the lights stand on masts level with the threshold, or 0.4 m over
        // the ground where it rises above that.
        let light = |out: f64, across: f64| {
            let (n, e) = position(&a, (-hl - out) * way, across);
            let ground = terrain.height(n, e).max(0.0);
            V3::new(n, e, -(a.elevation.max(ground) + 0.4))
        };
        let white = |b: &Builder, at: V3| Light { at, intensity: 20_000.0, beam: Some(b.beam(way, 0.0, 6.5, 11.0, 7.0)), radius: 0.12, system: System::Approach, ..b.plain(0.0, 0.0, 0.0, Color::White) };
        let barrette = |b: &mut Builder, out: f64, centre: f64, lights: usize, spacing: f64| {
            for j in 0..lights {
                let x = centre + (j as f64 - (lights - 1) as f64 / 2.0) * spacing;
                let at = light(out, x);
                b.lights.push(white(b, at));
            }
        };
        // Twice a second, running in towards the threshold: each `offset` of the half second after the farthest.
        let flasher = |b: &mut Builder, out: f64, offset: f64| {
            let at = light(out, 0.0) - V3::new(0.0, 0.0, 0.3);
            let flash = Flash { period: 0.5, offset: offset * 0.5, duration: 0.04 };
            b.lights.push(Light { at, color: Color::Strobe, intensity: 20_000.0, beam: Some(b.beam(way, 0.0, 6.0, 12.0, 8.0)), omni: 0.0, slope: None, flash: Some(flash), radius: 0.15, system: System::Strobes });
        };
        match class {
            Class::PrecisionTwo => {
                // ALSF-2, 2400 ft.
                for k in 1..=24 {
                    let out = 30.48 * k as f64;
                    barrette(self, out, 0.0, 5, 1.05);
                    // Red side rows, three lights each, in line with the touchdown zone's barrettes.
                    if k <= 10 {
                        for side in [-1.0, 1.0] {
                            for j in 0..3 {
                                let at = light(out, side * (4.8 + 1.5 * j as f64));
                                self.lights.push(Light { at, intensity: 5_000.0, beam: Some(self.beam(way, 0.0, 6.0, 8.0, 6.0)), radius: 0.12, system: System::Approach, ..self.plain(0.0, 0.0, 0.0, Color::Red) });
                            }
                        }
                    }
                }
                for side in [-1.0, 1.0] {
                    barrette(self, 152.4, side * 5.35, 4, 1.5);
                    barrette(self, 304.8, side * 7.6, 5, 1.05);
                }
                for k in 10..=24 {
                    flasher(self, 30.48 * k as f64, (24 - k) as f64 / 15.0);
                }
            }
            _ => {
                // MALSR: barrettes every 200 ft to 1400 ft, the 1000 ft bar, flashers 1600 to 2400 ft.
                for k in 1..=7 {
                    barrette(self, 60.96 * k as f64, 0.0, 5, 1.05);
                }
                for side in [-1.0, 1.0] {
                    barrette(self, 304.8, side * 7.6, 5, 1.05);
                }
                for k in 8..=12 {
                    flasher(self, 60.96 * k as f64, (12 - k) as f64 / 5.0);
                }
            }
        }
    }
}

/// How much of a system's light is lit (0 off, 1 the highest setting), for the sun `sun_elevation`
/// radians above the horizon and the visibility in metres: the stage a tower sets, after FAA order
/// JO 7110.65 (the tables of runway and approach light settings). The five stages of high intensity
/// lights are 100, 20, 4, 0.8 and 0.16 % of the highest; medium intensity lights have three, but
/// a runway's edge lights here are all set by the same table.
///
/// - Runway and approach lights are lit always, dimmest at night in good visibility (stage 2), at
///   stage 3 by day, brighter as the visibility falls: stage 5 below 1600 m by day; at night
///   never more than stage 4, which in mist is already dazzling.
/// - Slope indicators are lit always, at full strength by day, 20 % at dusk and 4 % at night.
/// - Strobes: full by day, 30 % at dusk, 10 % at night.
/// - Taxiways, the apron's floodlights and obstacle lights: from dusk (the sun 2° above the horizon)
///   to dawn; the beacon then too, and by day in less than 4800 m (3 miles), as when the field is
///   under instrument rules.
pub fn setting(system: System, sun_elevation: f64, visibility: f64) -> f64 {
    const STAGES: [f64; 6] = [0.0, 0.0016, 0.008, 0.04, 0.2, 1.0];
    let night = sun_elevation < -6.0 * DEG;
    let day = sun_elevation > 6.0 * DEG;
    let dark = sun_elevation < 2.0 * DEG;
    match system {
        System::Runway | System::Approach => {
            let stage = if day {
                if visibility < 1600.0 {
                    5
                } else if visibility < 4800.0 {
                    4
                } else {
                    3
                }
            } else if night {
                if visibility < 1600.0 {
                    4
                } else if visibility < 4800.0 {
                    3
                } else {
                    2
                }
            } else if visibility < 1600.0 {
                5
            } else if visibility < 4800.0 {
                4
            } else {
                3
            };
            STAGES[stage]
        }
        System::Slope => {
            if day || visibility < 1600.0 {
                1.0
            } else if night {
                0.04
            } else {
                0.2
            }
        }
        System::Strobes => {
            if day || visibility < 1600.0 {
                1.0
            } else if night {
                0.1
            } else {
                0.3
            }
        }
        System::Taxiway | System::Apron | System::Obstacle => f64::from(u8::from(dark)),
        System::Beacon => f64::from(u8::from(dark || visibility < 4800.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (Terrain, Airport) {
        let t = Terrain::new(1);
        let a = t.nearest_airport(0.0, 0.0).unwrap();
        (t, a)
    }

    /// Along and across the runway (to the right of its heading) from its middle, and height over it.
    fn local(a: &Airport, l: &Light) -> (f64, f64, f64) {
        let (along, across) = a.local(l.at.x, l.at.y);
        (along, across, -l.at.z - a.elevation)
    }

    /// An aerodrome of each class, from the first seed's.
    fn of_each() -> Vec<(Terrain, Airport)> {
        let mut found = Vec::new();
        for class in [Class::Visual, Class::NonPrecision, Class::PrecisionOne, Class::PrecisionTwo] {
            'search: for seed in 1..40 {
                let t = Terrain::new(seed);
                for a in t.airports(-60_000.0, 60_000.0, -60_000.0, 60_000.0) {
                    if Class::of(&a) == class {
                        found.push((Terrain::new(seed), a));
                        break 'search;
                    }
                }
            }
        }
        assert_eq!(found.len(), 4, "an aerodrome of every class");
        found
    }

    #[test]
    fn the_same_aerodrome_has_the_same_lights() {
        let (t, a) = home();
        assert_eq!(aerodrome(&t, &a), aerodrome(&Terrain::new(1), &a));
    }

    #[test]
    fn edge_lights_are_at_most_60_m_apart_and_yellow_only_on_the_last_third_of_instrument_runways() {
        for (t, a) in of_each() {
            let class = Class::of(&a);
            let edges: Vec<(f64, f64, Light)> = aerodrome(&t, &a)
                .into_iter()
                .filter(|l| l.system == System::Runway && local(&a, l).1.abs() > a.width / 2.0 + 1.0)
                .map(|l| (local(&a, &l).0, local(&a, &l).1, l))
                .collect();
            let mut alongs: Vec<f64> = edges.iter().filter(|e| e.1 > 0.0).map(|e| e.0).collect();
            alongs.sort_by(f64::total_cmp);
            alongs.dedup_by(|x, y| (*x - *y).abs() < 0.01);
            assert!(alongs.windows(2).all(|w| w[1] - w[0] <= 60.0 + 1e-9), "{class:?}");
            for (along, _, l) in &edges {
                let Some(beam) = l.beam else { continue };
                // Facing those who land along the heading when the beam points back along it.
                let way = if (beam.heading - a.heading).cos() < 0.0 { 1.0 } else { -1.0 };
                let to_go = a.length / 2.0 - along * way;
                let caution = if class.instrument() { (a.length / 3.0).min(600.0) } else { 0.0 };
                let yellow = l.color == Color::Yellow;
                assert_eq!(yellow, to_go < caution - 1.0, "{class:?} at {along} landing {way}");
            }
        }
    }

    #[test]
    fn each_end_is_green_outwards_and_red_inwards() {
        for (t, a) in of_each() {
            for l in aerodrome(&t, &a).iter().filter(|l| matches!(l.color, Color::Green | Color::Red) && l.system == System::Runway) {
                let (along, ..) = local(&a, l);
                if along.abs() < a.length / 2.0 {
                    continue; // the red centre line
                }
                let beam = l.beam.unwrap();
                let outwards = (beam.heading - a.heading).cos() * along.signum() > 0.0;
                assert_eq!(outwards, l.color == Color::Green);
            }
        }
    }

    #[test]
    fn the_papi_is_on_the_left_at_the_aiming_point_and_shows_two_red_two_white_on_the_path() {
        for (t, a) in of_each().into_iter().filter(|(_, a)| Class::of(a) != Class::Visual) {
            let lights = aerodrome(&t, &a);
            for way in [1.0, -1.0] {
                let mut units: Vec<&Light> = lights
                    .iter()
                    .filter(|l| l.slope.is_some() && (l.beam.unwrap().heading - a.heading).cos() * way < 0.0)
                    .collect();
                assert_eq!(units.len(), 4);
                units.sort_by(|x, y| local(&a, x).1.abs().total_cmp(&local(&a, y).1.abs()));
                for (k, u) in units.iter().enumerate() {
                    let (along, across, _) = local(&a, u);
                    // Left of those landing `way`, 15 m from the edge and 9 m apart.
                    assert!(across * way < 0.0);
                    assert!((across.abs() - (a.width / 2.0 + 15.0 + 9.0 * k as f64)).abs() < 1e-6);
                    assert!((along * way + a.length / 2.0 - AIM).abs() < 1e-6);
                    let minutes = [210.0, 190.0, 170.0, 150.0][k];
                    assert!((u.slope.unwrap().angle / DEG * 60.0 - minutes).abs() < 1e-6, "unit {k}");
                    assert!((u.slope.unwrap().transition / DEG * 60.0 - 3.0).abs() < 1e-9);
                }
                // An eye on the 3° path 2 km out sees the two inner units red and the two outer white.
                let eye_along = -(a.length / 2.0 - AIM) * way - 2000.0 * way;
                let height = 2000.0 * GLIDE_PATH.tan();
                let seen: Vec<bool> = units
                    .iter()
                    .map(|u| {
                        let (along, _, h) = local(&a, u);
                        let elevation = (height - h).atan2((along - eye_along).abs());
                        elevation > u.slope.unwrap().angle
                    })
                    .collect();
                assert_eq!(seen, [false, false, true, true]);
            }
        }
    }

    #[test]
    fn only_the_main_approach_of_a_precision_runway_has_approach_lights() {
        for (t, a) in of_each() {
            let class = Class::of(&a);
            let lights = aerodrome(&t, &a);
            let out: Vec<f64> = lights.iter().filter(|l| l.system == System::Approach).map(|l| local(&a, l).0).collect();
            if !class.precision() {
                assert!(out.is_empty());
                continue;
            }
            // All beyond the main end, out to 2400 ft.
            let main = if main_end_reversed(&a) { 1.0 } else { -1.0 };
            assert!(out.iter().all(|x| x * main > a.length / 2.0 && x * main < a.length / 2.0 + 740.0), "{class:?}");
            let flashers = lights.iter().filter(|l| l.flash.is_some() && local(&a, l).0.abs() > a.length / 2.0 + 20.0).count();
            assert_eq!(flashers, if class == Class::PrecisionTwo { 15 } else { 5 });
            let reds = lights.iter().filter(|l| l.system == System::Approach && l.color == Color::Red).count();
            assert_eq!(reds, if class == Class::PrecisionTwo { 60 } else { 0 });
            // Never under the ground.
            for l in lights.iter().filter(|l| l.system == System::Approach) {
                assert!(-l.at.z >= t.height(l.at.x, l.at.y).max(0.0) + 0.39);
            }
        }
    }

    #[test]
    fn centre_line_turns_red_towards_the_end_and_touchdown_zone_covers_half_the_runway() {
        let (t, a) = of_each().pop().unwrap();
        assert_eq!(Class::of(&a), Class::PrecisionTwo);
        let lights = aerodrome(&t, &a);
        // Seen landing along the heading: beams pointing back.
        let facing = |l: &&Light| (l.beam.unwrap().heading - a.heading).cos() < 0.0;
        let centre: Vec<(f64, Color)> = lights.iter().filter(|l| l.system == System::Runway && local(&a, l).1.abs() < 1.0 && local(&a, l).0.abs() < a.length / 2.0).filter(facing).map(|l| (local(&a, l).0, l.color)).collect();
        assert!(centre.windows(2).all(|w| (w[1].0 - w[0].0 - 15.0).abs() < 1e-6));
        for (along, color) in &centre {
            let to_go = a.length / 2.0 - along;
            if to_go < 300.0 {
                assert_eq!(*color, Color::Red);
            } else if to_go > a.length / 2.0 {
                assert_eq!(*color, Color::White);
            }
        }
        let zone: Vec<f64> = lights.iter().filter(|l| l.system == System::Runway && (3.0..9.0).contains(&local(&a, l).1.abs()) && local(&a, l).0.abs() < a.length / 2.0).filter(facing).map(|l| local(&a, l).0 + a.length / 2.0).collect();
        assert_eq!(zone.len(), 16 * 6);
        assert!(zone.iter().all(|s| (30.0..=a.length / 2.0).contains(s)));
    }

    #[test]
    fn obstacle_lights_crown_the_hangars_and_the_beacon_the_office_mast() {
        let (t, a) = home();
        let lights = aerodrome(&t, &a);
        let hangars = scenery::aerodrome_obstacles(&t, &a).iter().filter(|o| o.kind == ObstacleKind::Hangar).count();
        let obstacle = lights.iter().filter(|l| l.system == System::Obstacle).count();
        assert_eq!(obstacle, hangars * 2 + 1);
        for l in lights.iter().filter(|l| l.system == System::Obstacle) {
            let h = -l.at.z - a.elevation;
            assert!((6.0..8.5).contains(&h), "{h}");
        }
        let beacon: Vec<&Light> = lights.iter().filter(|l| l.system == System::Beacon).collect();
        assert_eq!(beacon.len(), 2);
        assert!((-beacon[0].at.z - a.elevation - 13.3).abs() < 0.2);
    }

    #[test]
    fn lights_dim_at_night_and_the_taxiways_light_only_after_dusk() {
        let (day, dusk, night) = (40.0 * DEG, 0.0, -20.0 * DEG);
        assert!(setting(System::Runway, night, 20_000.0) < setting(System::Runway, day, 20_000.0));
        assert!(setting(System::Runway, night, 600.0) > setting(System::Runway, night, 20_000.0));
        assert_eq!(setting(System::Runway, day, 1000.0), 1.0);
        assert_eq!(setting(System::Taxiway, day, 20_000.0), 0.0);
        assert_eq!(setting(System::Taxiway, dusk, 20_000.0), 1.0);
        assert_eq!(setting(System::Beacon, day, 3000.0), 1.0);
        assert!(setting(System::Slope, night, 20_000.0) > 0.0);
    }
}
