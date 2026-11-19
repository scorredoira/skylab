//! Universal Transverse Mercator on WGS 84, the projection of the Sentinel-2 tiles: a latitude and
//! longitude to the easting and northing of a zone, by Krüger's series to the fourth power of the
//! third flattening (Karney 2011, "Transverse Mercator with an accuracy of a few nanometres":
//! these terms keep it within a millimetre across a zone).

/// WGS 84's semi-major axis, metres, and flattening.
const A: f64 = 6_378_137.0;
const F: f64 = 1.0 / 298.257_223_563;
/// UTM's scale on the central meridian, and its false easting and the southern false northing, metres.
const K0: f64 = 0.9996;
const FALSE_EASTING: f64 = 500_000.0;
const FALSE_NORTHING_SOUTH: f64 = 10_000_000.0;

/// A zone of UTM: 1 to 60, and the hemisphere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Zone {
    pub number: u8,
    pub north: bool,
}

impl Zone {
    /// The zone of an EPSG code: 326zz north, 327zz south.
    pub fn from_epsg(epsg: u32) -> Option<Zone> {
        let number = (epsg % 100) as u8;
        let north = match epsg / 100 {
            326 => true,
            327 => false,
            _ => return None,
        };
        (1..=60).contains(&number).then_some(Zone { number, north })
    }

    /// Easting and northing, metres, of a latitude and longitude in degrees (also beyond the zone's
    /// six degrees, as the tiles that overlap the next zone are).
    pub fn project(&self, latitude: f64, longitude: f64) -> (f64, f64) {
        let n = F / (2.0 - F);
        let (n2, n3, n4) = (n * n, n * n * n, n * n * n * n);
        // The rectifying radius and Krüger's alphas.
        let a_hat = A / (1.0 + n) * (1.0 + n2 / 4.0 + n4 / 64.0);
        let alpha = [n / 2.0 - 2.0 * n2 / 3.0 + 5.0 * n3 / 16.0 + 41.0 * n4 / 180.0, 13.0 * n2 / 48.0 - 3.0 * n3 / 5.0 + 557.0 * n4 / 1440.0, 61.0 * n3 / 240.0 - 103.0 * n4 / 140.0, 49561.0 * n4 / 161280.0];
        let central = (self.number as f64 * 6.0 - 183.0).to_radians();
        let (phi, lambda) = (latitude.to_radians(), longitude.to_radians() - central);
        // The conformal latitude, through its tangent.
        let e = (F * (2.0 - F)).sqrt();
        let t = phi.tan();
        let sigma = (e * (e * t / (1.0 + t * t).sqrt()).atanh()).sinh();
        let tau = t * (1.0 + sigma * sigma).sqrt() - sigma * (1.0 + t * t).sqrt();
        let xi = tau.atan2(lambda.cos());
        let eta = (lambda.sin() / (tau * tau + lambda.cos().powi(2)).sqrt()).asinh();
        let (mut x, mut y) = (eta, xi);
        for (j, a) in alpha.iter().enumerate() {
            let k = 2.0 * (j + 1) as f64;
            x += a * (k * xi).cos() * (k * eta).sinh();
            y += a * (k * xi).sin() * (k * eta).cosh();
        }
        let easting = FALSE_EASTING + K0 * a_hat * x;
        let northing = K0 * a_hat * y + if self.north { 0.0 } else { FALSE_NORTHING_SOUTH };
        (easting, northing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Snyder's series ("Map Projections: A Working Manual", USGS 1987, 8-9 to 8-11): another
    /// derivation, good to a millimetre within a few degrees of the central meridian.
    fn snyder(zone: Zone, lat: f64, lon: f64) -> (f64, f64) {
        let e2 = F * (2.0 - F);
        let ep2 = e2 / (1.0 - e2);
        let phi = lat.to_radians();
        let (s, c) = phi.sin_cos();
        let t = phi.tan().powi(2);
        let cc = ep2 * c * c;
        let a = (lon - (zone.number as f64 * 6.0 - 183.0)).to_radians() * c;
        let nu = A / (1.0 - e2 * s * s).sqrt();
        let (e4, e6) = (e2 * e2, e2 * e2 * e2);
        let m = A * ((1.0 - e2 / 4.0 - 3.0 * e4 / 64.0 - 5.0 * e6 / 256.0) * phi - (3.0 * e2 / 8.0 + 3.0 * e4 / 32.0 + 45.0 * e6 / 1024.0) * (2.0 * phi).sin() + (15.0 * e4 / 256.0 + 45.0 * e6 / 1024.0) * (4.0 * phi).sin() - (35.0 * e6 / 3072.0) * (6.0 * phi).sin());
        let x = K0 * nu * (a + (1.0 - t + cc) * a.powi(3) / 6.0 + (5.0 - 18.0 * t + t * t + 72.0 * cc - 58.0 * ep2) * a.powi(5) / 120.0);
        let y = K0 * (m + nu * phi.tan() * (a * a / 2.0 + (5.0 - t + 9.0 * cc + 4.0 * cc * cc) * a.powi(4) / 24.0 + (61.0 - 58.0 * t + t * t + 600.0 * cc - 330.0 * ep2) * a.powi(6) / 720.0));
        (FALSE_EASTING + x, y + if zone.north { 0.0 } else { FALSE_NORTHING_SOUTH })
    }

    #[test]
    fn points_fall_where_another_derivation_puts_them() {
        let z31 = Zone::from_epsg(32631).unwrap();
        assert_eq!(z31, Zone { number: 31, north: true });
        assert_eq!(Zone::from_epsg(32733), Some(Zone { number: 33, north: false }));
        assert_eq!(Zone::from_epsg(4326), None);
        // On the central meridian at the equator: the false easting and nothing north.
        let (e, n) = z31.project(0.0, 3.0);
        assert!((e - 500_000.0).abs() < 1e-6 && n.abs() < 1e-6);
        // A degree of latitude on the meridian at 45° is 111 131.78 m (111 132.954 − 559.822 cos 2φ + 1.175 cos 4φ), times the scale.
        let (_, n44) = z31.project(44.5, 3.0);
        let (_, n45) = z31.project(45.5, 3.0);
        assert!(((n45 - n44) / K0 - 111_131.78).abs() < 0.05, "{}", (n45 - n44) / K0);
        for (zone, lat, lon) in [(31, 39.8626, 4.2186), (31, 41.2971, 2.0785), (32, 47.2602, 11.3439), (31, 40.0, 0.5), (33, -33.92, 18.42)] {
            let zone = Zone { number: zone, north: lat > 0.0 };
            let (e, n) = zone.project(lat, lon);
            let (se, sn) = snyder(zone, lat, lon);
            assert!((e - se).abs() < 0.005 && (n - sn).abs() < 0.005, "{lat} {lon}: {e:.4} {n:.4} against {se:.4} {sn:.4}");
        }
    }
}
