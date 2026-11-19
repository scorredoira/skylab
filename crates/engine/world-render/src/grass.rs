//! Tufts of grass on the ground round the camera, when it is low (`Quality::grass_range`): on the
//! ground, taxiing, in the last metres of an approach, the ground beside it is grass and not paint.
//! A tuft every metre or so, in tiles of `TILE` metres made in the background (`scatter.rs`), where
//! the ground is grass: a meadow, dry grass, a field, rough ground; not on water, roads, towns, the
//! mown ground of an aerodrome or bare soil. In a real place the parcels of its picture say what the
//! ground is and its colour (`parcels.rs`); in a generated world, `Terrain::surface`. They are drawn,
//! never run into. A race circuit's own grass (its verges and run-offs, `sim_core::circuit`) has
//! them too, short as mown grass is, longer towards its edge.
//!
//! An instance is a tree's (`sim_core::scenery`): place and height, then tone, width, how dry it is
//! (0 green to 1 straw) and nothing.

use crate::parcels::{self, Kind, Parcels};
use sim_core::scenery::Instances;
use sim_core::{Terrain, smoothstep};

/// Side of the tiles of tufts, metres.
pub const TILE: f64 = 32.0;
/// Metres between the spots a tuft may stand on: a whole number of them to a tile.
const SPOT: f64 = TILE / 44.0;

fn hash(x: i64, z: i64, k: u64) -> f64 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (z as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ k.wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// How dry a ground of linear colour `c` looks: 0 green, 1 straw.
fn dryness(c: [f64; 3]) -> f64 {
    1.0 - smoothstep(0.95, 1.35, c[1] / c[0].max(1e-4))
}

/// The tufts of the tile `key` of side `size`: one list.
pub fn tufts(terrain: &Terrain, parcels: Option<&[Parcels]>, key: (i32, i32), size: f64) -> Instances {
    let (ox, oz) = (key.0 as f64 * size, key.1 as f64 * size);
    let pad = 20.0;
    let airports = terrain.airports(-(oz + size) - pad, -oz + pad, ox - pad, ox + size + pad);
    let roads = terrain.roads(-(oz + size) - pad, -oz + pad, ox - pad, ox + size + pad);
    let towns = terrain.towns(-(oz + size), -oz, ox, ox + size);
    let spots = (size / SPOT).round() as i64;
    // A race course's trail is worn (a third of the grass), its start's pads stand on bare ground.
    let cuts: Vec<sim_core::course::Cut> = sim_core::course::cuts(terrain).into_iter().filter(|c| {
        let reach = c.half.0.max(c.half.1) + 2.0;
        c.from.0.min(c.to.0) - reach < -oz + pad && c.from.0.max(c.to.0) + reach > -(oz + size) - pad && c.from.1.min(c.to.1) - reach < ox + size + pad && c.from.1.max(c.to.1) + reach > ox - pad
    }).collect();
    let starts = sim_core::course::starts(terrain);
    // The circuits that reach this tile.
    let circuits: Vec<&sim_core::circuit::Circuit> = terrain.circuits().iter().map(|c| c.as_ref()).filter(|c| c.bounds[0] < -oz + pad && c.bounds[1] > -(oz + size) - pad && c.bounds[2] < ox + size + pad && c.bounds[3] > ox - pad).collect();
    let mut list = Vec::new();
    for j in 0..spots {
        for i in 0..spots {
            let (gx, gz) = ((key.0 as i64) * spots + i, (key.1 as i64) * spots + j);
            let r = |k: u64| hash(gx, gz, k);
            let (x, z) = (ox + (i as f64 + r(0)) * SPOT, oz + (j as f64 + r(1)) * SPOT);
            let (n, e) = (-z, x);
            let g = terrain.ground_with(n, e, &airports);
            // A circuit's own ground: tufts on its grass, standing on it (the terrain is laid a
            // little lower under it), none on what is paved or gravel.
            if let Some((c, at)) = circuits.iter().find_map(|c| c.locate(n, e).map(|at| (c, at))) {
                if let Some((surface, _)) = c.surface_at(&at) {
                    let Some(patch) = c.patch(n, e).filter(|_| surface == sim_core::car::Surface::Grass) else { continue };
                    // Mown: a tuft in two spots of three, a hand high; twice that from 8 m out
                    // of the asphalt, where the mower goes less often.
                    if r(2) >= 0.66 {
                        continue;
                    }
                    let s = c.sample(at.along);
                    let rough = smoothstep(6.0, 12.0, at.across.abs() - s.width / 2.0);
                    let height = (0.07 + 0.09 * r(3)) * (1.0 + 1.3 * rough);
                    let tone = ((0.72 + 0.28 * r(4)) * 32.0).floor() + r(5) * 0.999;
                    list.extend_from_slice(&[(x - ox) as f32, patch.height as f32 - 0.015, (z - oz) as f32, height as f32, tone as f32, (1.1 + 0.7 * r(6)) as f32, 0.0, 0.0]);
                    continue;
                }
            }
            if g.h < 0.6 || g.river > 0.05 || g.flat > 0.0 {
                continue;
            }
            if towns.iter().any(|t| t.weight(n, e) > 0.15) || roads.iter().any(|road| road.distance(n, e) < road.width / 2.0 + 1.0) {
                continue;
            }
            // How much grass, and how dry, the ground has here.
            let parcel = parcels.and_then(|p| parcels::sample_at(p, n, e)).filter(|s| s.kind != Kind::None);
            let (density, dry) = match parcel {
                Some(s) => {
                    let linear = parcels::linear_table();
                    let dry = dryness(s.colour.map(|v| linear[v as usize] as f64));
                    let density = match s.kind {
                        Kind::Meadow => 0.9,
                        Kind::DryGrass => 0.75,
                        Kind::Crop => 0.6,
                        Kind::Stubble => 0.35,
                        Kind::Scrub => 0.35,
                        Kind::Wood => 0.12,
                        Kind::Rock => 0.05,
                        Kind::Ploughed | Kind::None => 0.0,
                    };
                    (density, dry)
                }
                None => {
                    let d = 2.0;
                    let slope = ((terrain.ground_with(n, e + d, &airports).h - g.h) / d).hypot((terrain.ground_with(n - d, e, &airports).h - g.h) / d);
                    let s = terrain.surface(n, e, &g, slope, 0.0);
                    if s.water > 0.1 {
                        continue;
                    }
                    let c = [s.color.x as f64, s.color.y as f64, s.color.z as f64];
                    (0.8 * (1.0 - 0.85 * s.forest as f64) * (1.0 - smoothstep(0.5, 0.9, slope)), dryness(c))
                }
            };
            let worn = if starts.iter().any(|s| (s.0 - n).hypot(s.1 - e) < 9.0) {
                0.0
            } else if cuts.iter().any(|c| { let (d, half) = c.distance(n, e); d < half }) {
                0.33
            } else {
                1.0
            };
            if r(2) >= density * worn {
                continue;
            }
            let height = 0.3 + 0.45 * r(3) * (1.0 - 0.35 * dry);
            let tone = ((0.7 + 0.3 * r(4)) * 32.0).floor() + r(5) * 0.999;
            list.extend_from_slice(&[(x - ox) as f32, g.h as f32 - 0.03, (z - oz) as f32, height as f32, tone as f32, (1.0 + 0.8 * r(6)) as f32, dry as f32, 0.0]);
        }
    }
    vec![list]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grass_grows_on_land_and_not_on_the_runway() {
        let terrain = Terrain::new(1);
        let home = terrain.nearest_airport(0.0, 0.0).expect("an aerodrome");
        // A tile on the runway's middle has none; the land round it has plenty somewhere.
        let key = ((home.e / TILE).floor() as i32, ((-home.n) / TILE).floor() as i32);
        assert!(tufts(&terrain, None, key, TILE)[0].is_empty());
        let mut total = 0;
        for k in 0..40 {
            total += tufts(&terrain, None, (key.0 + 10 + k, key.1 + 25), TILE)[0].len() / 8;
        }
        assert!(total > 2000, "{total} tufts over 40 tiles");
        // The same tufts in a tile twice as big.
        let big = tufts(&terrain, None, (key.0 / 2 + 20, key.1 / 2 + 20), 2.0 * TILE)[0].len();
        let small: usize = [(0, 0), (1, 0), (0, 1), (1, 1)].iter().map(|(a, b)| tufts(&terrain, None, ((key.0 / 2 + 20) * 2 + a, (key.1 / 2 + 20) * 2 + b), TILE)[0].len()).sum();
        assert_eq!(big, small);
    }
}
