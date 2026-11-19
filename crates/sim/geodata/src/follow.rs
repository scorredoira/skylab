//! A real place that goes on as far as the aircraft flies: a region holds 96 km round its middle,
//! and before the aircraft leaves what it serves, the region round a middle further along the way
//! is built in the background (`build::load_around`) and flown on in its stead.
//!
//! Every region of a place keeps the place's origin and axes (its projection): only its middle
//! moves, on a lattice of `LATTICE` metres, a multiple of the step of every grid of a region and
//! of its ground's picture, so that their nodes fall where the region before had them and the
//! same tiles give the same heights and the same cover there. Positions, wrecks and the picture
//! of the ground of the region before stay where they were. Built once, a region is kept like
//! the place's own (`<place>@<n>_<e>.region`): flown again, and offline, it is read back.
//!
//! This decides when and where; the caller builds on a thread of its own (`flight`'s
//! `places::Following`, the example `follow`) and swaps the world's terrain when it comes.

/// The lattice of the regions' middles, metres: a multiple of 12, 30, 400 (heights), 24 (cover),
/// 10 and 32 (the ground's picture), and of 2400 metres.
pub const LATTICE: f64 = 24_000.0;
/// The aircraft stays this far from its region's middle, metres, north and east: inside the
/// finest maps (48 km), with 18 km to fly while the next region is built.
pub const SERVES: f64 = 30_000.0;
/// How far ahead, in seconds of flight, the next region is centred.
const LEAD: f64 = 180.0;

/// Where a place's regions are, and which is being built.
#[derive(Clone, Debug, PartialEq)]
pub struct Follow {
    /// The middle of the region flown in, north and east.
    pub centre: (f64, f64),
    /// The middle of the region being built.
    pub building: Option<(f64, f64)>,
}

impl Follow {
    /// A place flown in the region round `centre` (its own: `(0.0, 0.0)`).
    pub fn new(centre: (f64, f64)) -> Follow {
        Follow { centre, building: None }
    }

    /// The middle of a region to build now for an aircraft at (north, east) moving at (north,
    /// east) metres a second over the ground, if one is needed and none is being built: when it,
    /// or where it will be in `LEAD` seconds, is beyond `SERVES` of the middle of its region.
    pub fn next(&mut self, n: f64, e: f64, vn: f64, ve: f64) -> Option<(f64, f64)> {
        if self.building.is_some() {
            return None;
        }
        let ahead = (n + vn * LEAD, e + ve * LEAD);
        if serves(self.centre, n, e) && serves(self.centre, ahead.0, ahead.1) {
            return None;
        }
        let snap = |v: f64| (v / LATTICE).round() * LATTICE;
        let centre = (snap(ahead.0), snap(ahead.1));
        // Ahead may be beyond what one step serves (a fast aircraft): the aircraft must be served first.
        let centre = if serves(centre, n, e) { centre } else { (snap(n), snap(e)) };
        (centre != self.centre).then(|| {
            self.building = Some(centre);
            centre
        })
    }

    /// The region being built came: whether to fly in it now (the aircraft at (north, east) is
    /// where it serves; if not, it is dropped and another asked for at the next `next`).
    pub fn arrived(&mut self, n: f64, e: f64) -> bool {
        let Some(centre) = self.building.take() else { return false };
        let take = serves(centre, n, e);
        if take {
            self.centre = centre;
        }
        take
    }

    /// The region being built could not be had: asked for again at the next `next`.
    pub fn failed(&mut self) {
        self.building = None;
    }
}

/// Whether the region round `centre` serves a point.
pub fn serves(centre: (f64, f64), n: f64, e: f64) -> bool {
    (n - centre.0).abs().max((e - centre.1).abs()) <= SERVES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_next_region_lies_ahead_on_the_lattice_and_is_taken_where_it_serves() {
        // The steps of every grid of a region and of its picture divide the lattice, and a region
        // round another point of it has its nodes where the place's own has them.
        for step in [12.0, 30.0, 400.0, 24.0, 10.0, 32.0] {
            assert_eq!(LATTICE % step, 0.0, "{step}");
        }
        for level in &crate::build::HEIGHTS {
            let own = sim_core::Grid::centred(0.0, 0.0, level.half * 2.0, level.step, 0.0f32);
            let next = sim_core::Grid::centred(LATTICE, -2.0 * LATTICE, level.half * 2.0, level.step, 0.0f32);
            let (rows, cols) = ((next.south - own.south) / level.step, (next.west - own.west) / level.step);
            assert!(rows == rows.round() && cols == cols.round(), "{} m: {rows} {cols}", level.step);
        }
        let mut f = Follow::new((0.0, 0.0));
        // Near the middle, flying east at 60 m/s: nothing yet.
        assert_eq!(f.next(0.0, 10_000.0, 0.0, 60.0), None);
        // 20 km out, 30.8 km in three minutes: the region round 24 km east, once.
        assert_eq!(f.next(0.0, 20_000.0, 0.0, 60.0), Some((0.0, 24_000.0)));
        assert_eq!(f.next(0.0, 21_000.0, 0.0, 60.0), None);
        assert!(f.arrived(0.0, 25_000.0));
        assert_eq!(f, Follow { centre: (0.0, 24_000.0), building: None });
        // Very fast, south-west: the aircraft is served first.
        let mut f = Follow::new((0.0, 0.0));
        assert_eq!(f.next(-25_000.0, -25_000.0, -300.0, -300.0), Some((-24_000.0, -24_000.0)));
        // Turned back meanwhile, far from it: dropped, and asked again.
        assert!(!f.arrived(20_000.0, 20_000.0));
        assert_eq!(f.centre, (0.0, 0.0));
        assert_eq!(f.next(20_000.0, 20_000.0, 0.0, 0.0), None);
        f.building = Some((1.0, 1.0));
        f.failed();
        assert_eq!(f.building, None);
    }
}
