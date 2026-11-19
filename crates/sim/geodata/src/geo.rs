//! Where a point of the Earth falls in the world (north, east, metres) and in the map tiles.
//!
//! The world is flat; the Earth is a sphere. North and east are the azimuthal equidistant
//! projection round the region's origin: distances and directions from the origin are exact, and
//! across a region 100 km wide the error is a few metres (a tangent plane would leave the ground
//! 785 m low there). True north turns across the region by the difference of longitude times the
//! sine of the latitude, so runway headings are taken between their projected ends.

use std::f64::consts::PI;

/// Mean radius of the Earth, metres (IUGG).
pub const EARTH_RADIUS: f64 = 6_371_008.8;

/// The azimuthal equidistant projection round an origin, degrees in and out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projection {
    pub latitude: f64,
    pub longitude: f64,
    sin0: f64,
    cos0: f64,
}

impl Projection {
    pub fn new(latitude: f64, longitude: f64) -> Projection {
        let phi = latitude.to_radians();
        Projection { latitude, longitude, sin0: phi.sin(), cos0: phi.cos() }
    }

    /// North and east, metres, of a latitude and longitude in degrees.
    pub fn to_world(&self, latitude: f64, longitude: f64) -> (f64, f64) {
        let (phi, dl) = (latitude.to_radians(), (longitude - self.longitude).to_radians());
        let (sp, cp) = phi.sin_cos();
        let cos_c = (self.sin0 * sp + self.cos0 * cp * dl.cos()).clamp(-1.0, 1.0);
        let c = cos_c.acos();
        let k = if c < 1e-12 { 1.0 } else { c / c.sin() };
        let east = EARTH_RADIUS * k * cp * dl.sin();
        let north = EARTH_RADIUS * k * (self.cos0 * sp - self.sin0 * cp * dl.cos());
        (north, east)
    }

    /// Latitude and longitude, degrees, of a point north and east of the origin.
    pub fn to_earth(&self, north: f64, east: f64) -> (f64, f64) {
        let rho = north.hypot(east);
        if rho < 1e-9 {
            return (self.latitude, self.longitude);
        }
        let c = rho / EARTH_RADIUS;
        let (sc, cc) = c.sin_cos();
        let phi = (cc * self.sin0 + north * sc * self.cos0 / rho).clamp(-1.0, 1.0).asin();
        let dl = (east * sc).atan2(rho * self.cos0 * cc - north * self.sin0 * sc);
        (phi.to_degrees(), self.longitude + dl.to_degrees())
    }
}

/// A square of the Web Mercator tiling: zoom, column (from the antimeridian eastwards) and row (from
/// the north).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileId {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

/// Where a latitude and longitude (degrees) fall in the tiles of a zoom: column and row, fractional.
pub fn tile_xy(latitude: f64, longitude: f64, z: u8) -> (f64, f64) {
    let n = (1u64 << z) as f64;
    let phi = latitude.clamp(-85.051_128, 85.051_128).to_radians();
    let x = (longitude + 180.0) / 360.0 * n;
    let y = (1.0 - (phi.tan() + 1.0 / phi.cos()).ln() / PI) / 2.0 * n;
    (x, y)
}

/// The latitude and longitude (degrees) of a point of the tiles of a zoom given in columns and rows.
pub fn tile_to_earth(x: f64, y: f64, z: u8) -> (f64, f64) {
    let n = (1u64 << z) as f64;
    let longitude = x / n * 360.0 - 180.0;
    let latitude = (PI * (1.0 - 2.0 * y / n)).sinh().atan().to_degrees();
    (latitude, longitude)
}

/// The tiles of a zoom that a square of the world (centre and half side, metres) touches.
pub fn tiles_over(projection: &Projection, centre: (f64, f64), half: f64, z: u8) -> Vec<TileId> {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    // The edges are not straight in the tiles: walk them.
    for k in 0..=16 {
        let t = -half + 2.0 * half * k as f64 / 16.0;
        for (n, e) in [(-half, t), (half, t), (t, -half), (t, half)] {
            let (lat, lon) = projection.to_earth(centre.0 + n, centre.1 + e);
            let (x, y) = tile_xy(lat, lon, z);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    let last = (1u64 << z) as f64 - 1.0;
    let mut list = Vec::new();
    for y in y0.floor().max(0.0) as u32..=y1.floor().min(last) as u32 {
        for x in x0.floor().max(0.0) as u32..=x1.floor().min(last) as u32 {
            list.push(TileId { z, x, y });
        }
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_projection_goes_there_and_back() {
        let p = Projection::new(39.8626, 4.2186);
        assert_eq!(p.to_world(39.8626, 4.2186), (0.0, 0.0));
        for (n, e) in [(1000.0, 0.0), (0.0, -2500.0), (-48_000.0, 51_000.0), (90_000.0, 90_000.0)] {
            let (lat, lon) = p.to_earth(n, e);
            let (n2, e2) = p.to_world(lat, lon);
            assert!((n2 - n).abs() < 1e-6 && (e2 - e).abs() < 1e-6, "{n} {e}: {n2} {e2}");
        }
        // Distances from the origin are great circles: one degree of latitude is 111.2 km.
        let (n, e) = p.to_world(40.8626, 4.2186);
        assert!((n - 111_195.0).abs() < 1.0 && e.abs() < 1e-6, "{n}");
        // East along the parallel, the meridians close in: cos(39.86°) of the equator's degree.
        let (_, e) = p.to_world(39.8626, 5.2186);
        assert!((e - 111_195.0 * 39.8626f64.to_radians().cos()).abs() < 30.0, "{e}");
    }

    #[test]
    fn tiles_are_numbered_as_the_maps_do() {
        // The tile of Menorca's aerodrome at zoom 12, as the map servers number it.
        let (x, y) = tile_xy(39.8626, 4.2186, 12);
        assert_eq!((x as u32, y as u32), (2095, 1552));
        let (lat, lon) = tile_to_earth(x, y, 12);
        assert!((lat - 39.8626).abs() < 1e-9 && (lon - 4.2186).abs() < 1e-9);
        let p = Projection::new(39.8626, 4.2186);
        let tiles = tiles_over(&p, (0.0, 0.0), 10_000.0, 12);
        // 20 km is about three tiles of 7.5 km each way.
        assert!((9..=16).contains(&tiles.len()), "{}", tiles.len());
        assert!(tiles.contains(&TileId { z: 12, x: 2095, y: 1552 }));
    }
}
