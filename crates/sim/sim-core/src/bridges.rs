//! A bridge's parts, from what a `Bridge` says of it: the deck in short straight boxes following its
//! line and its rise, the piers down to the ground or the bed of the water, and what its structure
//! has besides (the towers, cables and hangers of a suspension bridge, the stays of a cable-stayed
//! one, the ribs and columns of an arch, the girders of a truss). The simulation runs into them
//! (`solids`) and the world draws them (`world_render`'s `bridges`), from the same numbers.
//!
//! Proportions where the maps say nothing, from how such bridges are built: piers some 32 m apart
//! on a road (18 m for a footbridge, 35 m for a railway); a suspension bridge's towers rise above
//! the deck an eighth of its main span (the Golden Gate's 152 m over 1280 m; the Humber's 155 m
//! over 1410 m) and its main span is two thirds of its length; its deck is stiffened by a girder
//! a 170th of the span deep (the Golden Gate's truss, 7.6 m); a cable-stayed bridge's pylons rise
//! a fifth of their span; a deck arch rises a fifth to a sixth of its span.

use crate::region::{Bridge, Carries, Structure};

/// What a part of a bridge is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartKind {
    Deck,
    Pier,
    /// A suspension or cable-stayed bridge's tower: its legs and the beams between them.
    Tower,
    /// A suspension bridge's main cable, or a stay.
    Cable,
    /// The vertical ropes that hang a suspension deck from its cables, or a through arch's.
    Hanger,
    /// An arch's rib, and the columns between it and the deck.
    Arch,
    /// A truss's side girders.
    Girder,
}

/// The solid of a part.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Piece {
    /// A box standing on a four-cornered outline (north, east; turning either way): its bottom and
    /// top at the first corner, above the sea, and how much both rise per metre north and east.
    Prism { ring: [[f32; 2]; 4], bottom: f32, top: f32, grade: [f32; 2] },
    /// A round bar between two points (north, east, height above the sea).
    Bar { a: [f32; 3], b: [f32; 3], radius: f32 },
}

impl Piece {
    /// The bottom and top of a prism at a point (north, east).
    pub fn heights(&self, n: f64, e: f64) -> (f64, f64) {
        match *self {
            Piece::Prism { ring, bottom, top, grade } => {
                let rise = (n - ring[0][0] as f64) * grade[0] as f64 + (e - ring[0][1] as f64) * grade[1] as f64;
                (bottom as f64 + rise, top as f64 + rise)
            }
            Piece::Bar { a, b, .. } => (a[2].min(b[2]) as f64, a[2].max(b[2]) as f64),
        }
    }

    /// Whether a point (north, east, height) is inside.
    pub fn contains(&self, n: f64, e: f64, h: f64) -> bool {
        match *self {
            Piece::Prism { ring, .. } => {
                let (bottom, top) = self.heights(n, e);
                h > bottom && h < top && crate::region::inside_ring(&ring, n, e)
            }
            Piece::Bar { a, b, radius } => {
                let (ax, ay, az) = (a[0] as f64, a[1] as f64, a[2] as f64);
                let (dx, dy, dz) = (b[0] as f64 - ax, b[1] as f64 - ay, b[2] as f64 - az);
                let length2 = dx * dx + dy * dy + dz * dz;
                let t = if length2 > 0.0 { (((n - ax) * dx + (e - ay) * dy + (h - az) * dz) / length2).clamp(0.0, 1.0) } else { 0.0 };
                let (x, y, z) = (n - ax - t * dx, e - ay - t * dy, h - az - t * dz);
                x * x + y * y + z * z < (radius as f64) * (radius as f64)
            }
        }
    }

    /// The rectangle (south, west, north, east) round it.
    pub fn bounds(&self) -> [f64; 4] {
        match *self {
            Piece::Prism { ring, .. } => {
                let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
                for p in ring {
                    b = [b[0].min(p[0] as f64), b[1].min(p[1] as f64), b[2].max(p[0] as f64), b[3].max(p[1] as f64)];
                }
                b
            }
            Piece::Bar { a, b, radius } => {
                let r = radius as f64;
                [(a[0].min(b[0])) as f64 - r, (a[1].min(b[1])) as f64 - r, (a[0].max(b[0])) as f64 + r, (a[1].max(b[1])) as f64 + r]
            }
        }
    }
}

/// A part of a bridge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    pub kind: PartKind,
    pub piece: Piece,
}

/// The longest straight box of deck, metres: a curved deck follows its line in boxes this long.
const DECK_PIECE: f64 = 24.0;
/// Hangers and stays are this far apart along the deck, metres (the Golden Gate's suspenders, 50 ft).
pub const HANGER_STEP: f64 = 15.24;

/// How deep the deck is under its top, metres.
pub fn deck_depth(b: &Bridge) -> f64 {
    match b.structure {
        Structure::Suspension => (main_span(b).1 / 170.0).clamp(2.5, 9.0),
        Structure::CableStayed => 2.6,
        _ => match b.carries {
            Carries::Road => 1.8,
            Carries::Rail => 2.2,
            Carries::Path => 0.9,
        },
    }
}

/// The towers of a bridge that has them (suspension, cable-stayed) or where its arch springs from:
/// metres along its line, and the main span between them.
pub fn main_span(b: &Bridge) -> (Vec<f64>, f64) {
    let length = b.length();
    let towers: Vec<f64> = if b.towers.len() >= 2 || (b.towers.len() == 1 && b.structure == Structure::CableStayed) {
        b.towers.iter().map(|&t| t as f64).collect()
    } else {
        match b.structure {
            Structure::Suspension | Structure::Arch => vec![length * 0.17, length * 0.83],
            Structure::CableStayed if length < 260.0 => vec![length * 0.5],
            Structure::CableStayed => vec![length * 0.27, length * 0.73],
            _ => Vec::new(),
        }
    };
    let span = match towers.len() {
        0 => 0.0,
        1 => (towers[0]).max(length - towers[0]),
        _ => towers[towers.len() - 1] - towers[0],
    };
    (towers, span)
}

/// How high the towers rise above the deck, metres.
pub fn tower_height(b: &Bridge) -> f64 {
    if b.tower_height > 0.0 {
        return b.tower_height as f64;
    }
    let (towers, span) = main_span(b);
    match b.structure {
        Structure::Suspension => (span / 8.0).clamp(15.0, 320.0),
        // One pylon holds a span on each side; two, the span between them.
        Structure::CableStayed if towers.len() == 1 => (span * 0.4).clamp(12.0, 240.0),
        Structure::CableStayed => (span / 5.0).clamp(12.0, 240.0),
        _ => 0.0,
    }
}

/// Every part of a bridge. `ground` is the height of the ground (or of the bed of the water) at a
/// point (north, east).
pub fn parts(b: &Bridge, ground: &dyn Fn(f64, f64) -> f64) -> Vec<Part> {
    let mut out = Vec::new();
    let length = b.length();
    if b.line.len() < 2 || length < 1.0 {
        return out;
    }
    let width = b.width as f64;
    let depth = deck_depth(b);
    // The deck, in boxes along the line.
    for w in b.line.windows(2) {
        let (a, c) = (w[0], w[1]);
        let d = ((c[0] - a[0]) as f64).hypot((c[1] - a[1]) as f64);
        let pieces = (d / DECK_PIECE).ceil().max(1.0) as usize;
        for k in 0..pieces {
            let (t0, t1) = (k as f64 / pieces as f64, (k + 1) as f64 / pieces as f64);
            let p = |t: f64| [a[0] as f64 + (c[0] - a[0]) as f64 * t, a[1] as f64 + (c[1] - a[1]) as f64 * t, a[2] as f64 + (c[2] - a[2]) as f64 * t];
            out.push(Part { kind: PartKind::Deck, piece: slab(p(t0), p(t1), width, depth) });
        }
    }
    let (towers, span) = main_span(b);
    let hung = matches!(b.structure, Structure::Suspension | Structure::CableStayed);
    let (first, last) = (towers.first().copied().unwrap_or(0.0), towers.last().copied().unwrap_or(0.0));
    // The piers: evenly along it, not in a main span hung from towers or held by an arch, and
    // only where the deck stands clear of the ground.
    let spacing = match (b.structure, b.carries) {
        (Structure::Suspension | Structure::CableStayed, _) => 60.0,
        (_, Carries::Path) => 18.0,
        (_, Carries::Rail) => 35.0,
        _ => 32.0,
    };
    let count = (length / spacing).round().max(1.0) as usize;
    for k in 1..count {
        let s = length * k as f64 / count as f64;
        // A suspension bridge hangs whole from its cables, side spans too, down to its anchorages.
        if b.structure == Structure::Suspension {
            break;
        }
        if (hung || b.structure == Structure::Arch) && towers.len() >= 2 && s > first - spacing * 0.5 && s < last + spacing * 0.5 {
            continue;
        }
        if hung && towers.len() == 1 && (s - first).abs() < span {
            continue;
        }
        pier(&mut out, b, s, depth, ground);
    }
    match b.structure {
        Structure::Suspension => suspension(&mut out, b, &towers, depth, ground),
        Structure::CableStayed => cable_stayed(&mut out, b, &towers, span, depth, ground),
        Structure::Arch if towers.len() >= 2 => arch(&mut out, b, first, last, depth, ground),
        Structure::Truss => truss(&mut out, b, length),
        _ => {}
    }
    out
}

/// A box of deck from point `a` to `c` (north, east, height of its top), `width` across, `depth` deep.
fn slab(a: [f64; 3], c: [f64; 3], width: f64, depth: f64) -> Piece {
    let (dn, de) = (c[0] - a[0], c[1] - a[1]);
    let d = dn.hypot(de).max(1e-6);
    let (ln, le) = (-de / d * width / 2.0, dn / d * width / 2.0);
    let ring = [[(a[0] + ln) as f32, (a[1] + le) as f32], [(c[0] + ln) as f32, (c[1] + le) as f32], [(c[0] - ln) as f32, (c[1] - le) as f32], [(a[0] - ln) as f32, (a[1] - le) as f32]];
    let slope = (c[2] - a[2]) / d;
    // The first corner is beside `a`: its heights are a's.
    Piece::Prism { ring, bottom: (a[2] - depth) as f32, top: a[2] as f32, grade: [(slope * dn / d) as f32, (slope * de / d) as f32] }
}

/// An upright box round a point (north, east), `along` by `across` metres turned to `heading`
/// (radians from north), from `bottom` to `top`.
pub fn post(n: f64, e: f64, heading: f64, along: f64, across: f64, bottom: f64, top: f64) -> Piece {
    let (c, s) = (heading.cos(), heading.sin());
    let corner = |u: f64, v: f64| [(n + u * c - v * s) as f32, (e + u * s + v * c) as f32];
    let (u, v) = (along / 2.0, across / 2.0);
    Piece::Prism { ring: [corner(-u, -v), corner(u, -v), corner(u, v), corner(-u, v)], bottom: bottom as f32, top: top as f32, grade: [0.0, 0.0] }
}

/// The point `across` metres to the right of the line at `s` (north, east, height of the deck's top).
fn beside(b: &Bridge, s: f64, across: f64) -> [f64; 3] {
    let (p, heading) = b.at(s);
    [p[0] - across * heading.sin(), p[1] + across * heading.cos(), p[2]]
}

/// A pier under the deck at `s` along the line: one column under a narrow deck, two under a wide one.
fn pier(out: &mut Vec<Part>, b: &Bridge, s: f64, depth: f64, ground: &dyn Fn(f64, f64) -> f64) {
    let (p, heading) = b.at(s);
    let under = p[2] - depth;
    let width = b.width as f64;
    let columns: &[f64] = if width <= 14.0 { &[0.0] } else { &[-0.3, 0.3] };
    for &k in columns {
        let q = beside(b, s, k * width);
        let floor = ground(q[0], q[1]);
        if under - floor < 1.5 {
            continue;
        }
        let across = if columns.len() == 1 { (0.55 * width).clamp(1.2, 7.0) } else { 2.4 };
        let along = if b.carries == Carries::Path { 0.6 } else { 1.8 };
        out.push(Part { kind: PartKind::Pier, piece: post(q[0], q[1], heading, along, across, floor - 2.0, under) });
    }
}

/// A suspension bridge's towers, cables and hangers.
fn suspension(out: &mut Vec<Part>, b: &Bridge, towers: &[f64], depth: f64, ground: &dyn Fn(f64, f64) -> f64) {
    let length = b.length();
    let height = tower_height(b);
    let (first, last) = (towers[0], towers[towers.len() - 1]);
    let span = last - first;
    let half = b.width as f64 / 2.0;
    let leg = (span * 0.008).clamp(1.5, 12.0);
    let radius = (span / 2800.0).clamp(0.12, 0.6);
    for &t in towers {
        tower(out, b, t, height, leg, depth, ground);
    }
    // The main cables, over the towers' tops, down to the deck's ends (their anchorages), sagging
    // in each span as a parabola: to just over the deck in the middle of the main span.
    for side in [-1.0, 1.0] {
        let across = side * (half + 0.3);
        let top_at = |s: f64| b.at(s).0[2] + height - 1.5;
        let cable = |s: f64| -> f64 {
            if s < first {
                let (z0, z1) = (b.at(0.0).0[2] + 1.0, top_at(first));
                let t = s / first.max(1.0);
                z0 + (z1 - z0) * t - 0.04 * first * 4.0 * t * (1.0 - t)
            } else if s > last {
                let (z0, z1) = (top_at(last), b.at(length).0[2] + 1.0);
                let t = (s - last) / (length - last).max(1.0);
                z0 + (z1 - z0) * t - 0.04 * (length - last) * 4.0 * t * (1.0 - t)
            } else {
                let mid = (first + last) / 2.0;
                let low = b.at(mid).0[2] + 1.0 + depth * 0.2;
                let x = (s - mid) / (span / 2.0);
                low + (top_at(first) - low) * x * x
            }
        };
        let steps = (length / 16.0).ceil().max(2.0) as usize;
        let point = |s: f64| {
            let q = beside(b, s, across);
            [q[0] as f32, q[1] as f32, cable(s) as f32]
        };
        for k in 0..steps {
            let (s0, s1) = (length * k as f64 / steps as f64, length * (k + 1) as f64 / steps as f64);
            out.push(Part { kind: PartKind::Cable, piece: Piece::Bar { a: point(s0), b: point(s1), radius: radius as f32 } });
        }
        // The hangers, every `HANGER_STEP` from each tower outwards.
        let mut s = first + HANGER_STEP;
        while s < last - 1.0 {
            hanger(out, b, s, across, cable(s));
            s += HANGER_STEP;
        }
        for (from, to, step) in [(first - HANGER_STEP, 0.0, -HANGER_STEP), (last + HANGER_STEP, length, HANGER_STEP)] {
            let mut s = from;
            while (step < 0.0 && s > to + 8.0) || (step > 0.0 && s < to - 8.0) {
                if cable(s) - b.at(s).0[2] > 2.0 {
                    hanger(out, b, s, across, cable(s));
                }
                s += step;
            }
        }
    }
}

fn hanger(out: &mut Vec<Part>, b: &Bridge, s: f64, across: f64, top: f64) {
    let q = beside(b, s, across);
    out.push(Part { kind: PartKind::Hanger, piece: Piece::Bar { a: [q[0] as f32, q[1] as f32, q[2] as f32], b: [q[0] as f32, q[1] as f32, top as f32], radius: 0.12 } });
}

/// A tower of two legs either side of the deck, from the ground (or the bed of the water) to
/// `height` above the deck, with beams between them; a footing round its feet where it stands in
/// water or low ground.
fn tower(out: &mut Vec<Part>, b: &Bridge, s: f64, height: f64, leg: f64, depth: f64, ground: &dyn Fn(f64, f64) -> f64) {
    if b.masonry {
        return stone_tower(out, b, s, height, depth, ground);
    }
    let (p, heading) = b.at(s);
    let half = b.width as f64 / 2.0 + leg * 0.6;
    let top = p[2] + height;
    let mut lowest = f64::MAX;
    for side in [-1.0, 1.0] {
        let q = beside(b, s, side * half);
        let floor = ground(q[0], q[1]);
        lowest = lowest.min(floor);
        out.push(Part { kind: PartKind::Tower, piece: post(q[0], q[1], heading, leg, leg * 0.85, floor - 2.0, top) });
    }
    // The beams across: one under the deck, three above it, the last at the top.
    let thick = (height * 0.035).clamp(1.0, 6.0);
    for at in [p[2] - depth - thick, p[2] + height * 0.42, p[2] + height * 0.72, top - thick] {
        if at - thick < lowest {
            continue;
        }
        out.push(Part { kind: PartKind::Tower, piece: post(p[0], p[1], heading, leg * 0.7, half * 2.0, at - thick, at) });
    }
    // A footing where the tower stands in the water or well below the deck.
    if lowest < 2.0 {
        out.push(Part { kind: PartKind::Pier, piece: post(p[0], p[1], heading, leg * 2.6, half * 2.0 + leg * 2.6, lowest - 2.0, lowest.max(0.0) + 6.0) });
    }
}

/// A tower of stone: one block from the bed of the water to the deck, some two thirds wider than the
/// deck and a third as long as it is wide; above the deck three piers either side of an arch over
/// each half of the deck, rising to seven tenths of the tower's height, and the block whole again
/// above the arches to its top (the Brooklyn Bridge's: 43 by 18 m at the water, arches 35 m high).
fn stone_tower(out: &mut Vec<Part>, b: &Bridge, s: f64, height: f64, depth: f64, ground: &dyn Fn(f64, f64) -> f64) {
    let (p, heading) = b.at(s);
    let width = b.width as f64 * 1.65;
    let along = width * 0.42;
    let floor = ground(p[0], p[1]).min(p[2] - depth - 5.0);
    let deck = p[2] - depth;
    let springing = p[2] + height * 0.7;
    out.push(Part { kind: PartKind::Tower, piece: post(p[0], p[1], heading, along, width, floor - 2.0, deck) });
    // The piers between and beside the arches, each arch as wide as half the deck.
    let arch = b.width as f64 * 0.4;
    let pier = (width - 2.0 * arch) / 3.0;
    for k in [-1.0, 0.0, 1.0] {
        let q = beside(b, s, k * (arch + pier));
        out.push(Part { kind: PartKind::Tower, piece: post(q[0], q[1], heading, along, pier, deck, springing) });
    }
    out.push(Part { kind: PartKind::Tower, piece: post(p[0], p[1], heading, along * 0.92, width * 0.96, springing, p[2] + height) });
}

/// A cable-stayed bridge's pylons and the stays fanning down from them in two planes.
fn cable_stayed(out: &mut Vec<Part>, b: &Bridge, towers: &[f64], span: f64, depth: f64, ground: &dyn Fn(f64, f64) -> f64) {
    let height = tower_height(b);
    let length = b.length();
    let leg = (span * 0.012).clamp(1.2, 8.0);
    let half = b.width as f64 / 2.0;
    for (i, &t) in towers.iter().enumerate() {
        tower(out, b, t, height, leg, depth, ground);
        // Each side reaches halfway to the next pylon, or to the end.
        let back = if i == 0 { t } else { (t - towers[i - 1]) / 2.0 };
        let ahead = if i + 1 == towers.len() { length - t } else { (towers[i + 1] - t) / 2.0 };
        for side in [-1.0, 1.0] {
            let across = side * (half + leg * 0.6);
            let anchor = beside(b, t, across);
            for (reach, sign) in [(back, -1.0), (ahead, 1.0)] {
                let count = ((reach * 0.85) / HANGER_STEP).floor() as usize;
                for k in 1..=count {
                    let s = t + sign * k as f64 * HANGER_STEP;
                    let deck = beside(b, s, side * half);
                    // The stays leave the pylon spread over its upper third, the farthest highest.
                    let up = anchor[2] + height * (0.66 + 0.32 * k as f64 / count.max(1) as f64);
                    out.push(Part { kind: PartKind::Cable, piece: Piece::Bar { a: [anchor[0] as f32, anchor[1] as f32, up as f32], b: [deck[0] as f32, deck[1] as f32, deck[2] as f32], radius: 0.14 } });
                }
            }
        }
    }
}

/// A deck arch between `from` and `to` along the line: two ribs rising from the ground to just
/// under the deck in the middle, and columns from them up to it.
fn arch(out: &mut Vec<Part>, b: &Bridge, from: f64, to: f64, depth: f64, ground: &dyn Fn(f64, f64) -> f64) {
    let span = to - from;
    let (start, end) = (b.at(from).0, b.at(to).0);
    let spring = ground(start[0], start[1]).max(ground(end[0], end[1])).max(0.0);
    let crown = b.at((from + to) / 2.0).0[2] - depth - 0.5;
    if crown - spring < 3.0 {
        return;
    }
    let rib = (span / 60.0).clamp(0.8, 3.5);
    let steps = (span / 8.0).ceil().max(4.0) as usize;
    let height_at = |s: f64| {
        let x = (s - from) / span * 2.0 - 1.0;
        spring + (crown - spring) * (1.0 - x * x)
    };
    for side in [-0.32, 0.32] {
        let across = side * b.width as f64;
        for k in 0..steps {
            let (s0, s1) = (from + span * k as f64 / steps as f64, from + span * (k + 1) as f64 / steps as f64);
            let (q0, q1) = (beside(b, s0, across), beside(b, s1, across));
            let (z0, z1) = (height_at(s0), height_at(s1));
            out.push(Part { kind: PartKind::Arch, piece: Piece::Bar { a: [q0[0] as f32, q0[1] as f32, z0 as f32], b: [q1[0] as f32, q1[1] as f32, z1 as f32], radius: rib as f32 } });
            // A column every other step, from the rib to the deck.
            let deck = q1[2] - depth;
            if k % 2 == 1 && deck - z1 > 1.5 {
                let (_, heading) = b.at(s1);
                out.push(Part { kind: PartKind::Arch, piece: post(q1[0], q1[1], heading, rib * 0.8, rib * 0.8, z1, deck) });
            }
        }
    }
}

/// A truss's two side girders, as tall as a tenth of the span between piers.
fn truss(out: &mut Vec<Part>, b: &Bridge, length: f64) {
    let rise = (length.min(60.0) / 10.0).clamp(3.5, 7.0);
    let half = b.width as f64 / 2.0 + 0.3;
    for w in b.line.windows(2) {
        let (a, c) = (w[0], w[1]);
        let (dn, de) = ((c[0] - a[0]) as f64, (c[1] - a[1]) as f64);
        let d = dn.hypot(de).max(1e-6);
        for side in [-1.0, 1.0] {
            let (on, oe) = (-de / d * half * side, dn / d * half * side);
            let a2 = [a[0] as f64 + on, a[1] as f64 + oe, a[2] as f64 + rise];
            let c2 = [c[0] as f64 + on, c[1] as f64 + oe, c[2] as f64 + rise];
            if let Piece::Prism { ring, bottom, top, grade } = slab(a2, c2, 0.5, rise) {
                out.push(Part { kind: PartKind::Girder, piece: Piece::Prism { ring, bottom, top, grade } });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bridge(structure: Structure, length: f32, deck: f32) -> Bridge {
        Bridge { line: vec![[0.0, 0.0, deck], [length, 0.0, deck]], width: 20.0, carries: Carries::Road, structure, ..Bridge::default() }
    }

    #[test]
    fn a_beam_bridge_has_its_deck_along_the_line_and_piers_down_to_the_ground() {
        let b = bridge(Structure::Beam, 160.0, 12.0);
        let parts = parts(&b, &|_, _| 0.0);
        let deck: Vec<_> = parts.iter().filter(|p| p.kind == PartKind::Deck).collect();
        assert_eq!(deck.len(), 7);
        // On the deck, inside it; just above it, outside; under it, a pier.
        assert!(deck.iter().any(|p| p.piece.contains(50.0, 3.0, 11.5)));
        assert!(!parts.iter().any(|p| p.piece.contains(50.0, 3.0, 12.5)));
        let piers: Vec<_> = parts.iter().filter(|p| p.kind == PartKind::Pier).collect();
        assert_eq!(piers.len(), 2 * 4, "five spans of 32 m, two columns each");
        for p in &piers {
            let (bottom, top) = p.piece.heights(0.0, 0.0);
            assert!(bottom < 0.0 && (top - (12.0 - deck_depth(&b))).abs() < 1e-4);
        }
        // Fly under it between the piers.
        assert!(!parts.iter().any(|p| p.piece.contains(48.0, 0.0, 6.0)));
    }

    #[test]
    fn a_sloping_deck_follows_its_line() {
        let b = Bridge { line: vec![[0.0, 0.0, 10.0], [0.0, 100.0, 20.0]], width: 8.0, ..Bridge::default() };
        let parts = parts(&b, &|_, _| 0.0);
        let inside = |e: f64, h: f64| parts.iter().any(|p| p.kind == PartKind::Deck && p.piece.contains(0.0, e, h));
        assert!(inside(10.0, 10.8) && !inside(10.0, 11.2));
        assert!(inside(90.0, 18.8) && !inside(90.0, 19.2));
    }

    #[test]
    fn a_suspension_bridge_hangs_from_cables_over_two_towers() {
        let mut b = bridge(Structure::Suspension, 2000.0, 67.0);
        b.towers = vec![360.0, 1640.0];
        b.width = 27.0;
        // Stone towers: solid round the carriageways, open over each.
        let mut stone = b.clone();
        stone.masonry = true;
        let stone_parts = parts(&stone, &|_, _| -50.0);
        let inside = |n: f64, e: f64, h: f64| stone_parts.iter().any(|p| p.kind == PartKind::Tower && p.piece.contains(n, e, h));
        assert!(inside(360.0, 0.0, 120.0) && inside(360.0, 0.0, 30.0) && inside(360.0, 0.0, 200.0));
        assert!(!inside(360.0, 6.0, 72.0) && !inside(360.0, -6.0, 72.0), "the arches over the deck");
        assert!((tower_height(&b) - 160.0).abs() < 1e-9);
        let parts = parts(&b, &|_, _| -50.0);
        let towers: Vec<_> = parts.iter().filter(|p| p.kind == PartKind::Tower).collect();
        let top = towers.iter().map(|p| p.piece.heights(360.0, 0.0).1).fold(0.0, f64::max);
        assert!((top - 227.0).abs() < 1e-3, "{top}");
        // No pier in the main span, and room to fly under the deck there.
        assert!(!parts.iter().any(|p| p.kind == PartKind::Pier && p.piece.bounds()[0] > 400.0 && p.piece.bounds()[2] < 1600.0));
        assert!(!parts.iter().any(|p| p.piece.contains(1000.0, 0.0, 30.0)));
        // The cables: near the tower tops at the towers, near the deck in the middle.
        let cable_at = |s: f64| parts.iter().filter(|p| p.kind == PartKind::Cable).filter_map(|p| match p.piece {
            Piece::Bar { a, b: c, .. } if (a[0] as f64) <= s && (c[0] as f64) >= s => Some((a[2] + c[2]) as f64 / 2.0),
            _ => None,
        }).next().unwrap();
        assert!(cable_at(1000.0) < 72.0 && cable_at(1000.0) > 67.0, "{}", cable_at(1000.0));
        assert!(cable_at(365.0) > 200.0);
        assert!(parts.iter().filter(|p| p.kind == PartKind::Hanger).count() > 150);
    }

    #[test]
    fn a_cable_stayed_bridge_and_an_arch_stand_on_their_own_shapes() {
        let b = bridge(Structure::CableStayed, 400.0, 30.0);
        let parts_cs = parts(&b, &|_, _| 0.0);
        assert!(parts_cs.iter().filter(|p| p.kind == PartKind::Cable).count() > 20);
        assert!(parts_cs.iter().any(|p| p.kind == PartKind::Tower));
        let a = bridge(Structure::Arch, 200.0, 40.0);
        let parts_a = parts(&a, &|_, _| 0.0);
        assert!(parts_a.iter().any(|p| p.kind == PartKind::Arch));
        // The arch springs from the ground and rises to under the deck.
        assert!(parts_a.iter().filter(|p| p.kind == PartKind::Arch).any(|p| p.piece.heights(0.0, 0.0).1 > 30.0));
    }
}
