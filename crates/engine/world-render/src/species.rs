//! Which tree is drawn where. The simulation knows eight shapes of tree, each the crown something may
//! run into (`sim_core::scenery::Tree`, `CROWNS`); the world draws each as one of a few species of
//! that shape, chosen by where it stands: the climate of the place (its latitude, or a generated
//! world's moisture), the height, the coast. A Mediterranean place has holm oaks, olives, cypresses,
//! stone and Aleppo pines and the scrub of the maquis; an Alpine one spruces, firs, larches and
//! beeches; the tropics and a warm coast palms. The choice is made per tree from its place alone,
//! so a tree drawn from a small tile and from a big one is the same species.

use crate::foliage::Layer;
use glam::Vec3;
use sim_core::scenery::{self, Instances, Tree};
use sim_core::{DEG, Terrain, smoothstep};

/// The species drawn, in the order of the lists `assign` returns and of the meshes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Species {
    Oak,
    Beech,
    Ash,
    HolmOak,
    Olive,
    Poplar,
    Cypress,
    Spruce,
    Fir,
    Larch,
    StonePine,
    Pine,
    Palm,
    Maquis,
    Shrub,
}

pub const COUNT: usize = 15;

impl Species {
    pub const ALL: [Species; COUNT] = [
        Species::Oak,
        Species::Beech,
        Species::Ash,
        Species::HolmOak,
        Species::Olive,
        Species::Poplar,
        Species::Cypress,
        Species::Spruce,
        Species::Fir,
        Species::Larch,
        Species::StonePine,
        Species::Pine,
        Species::Palm,
        Species::Maquis,
        Species::Shrub,
    ];

    /// The simulation's shape this species is drawn for: its crown fits the one something runs into.
    pub fn shape(self) -> Tree {
        match self {
            Species::Oak | Species::Beech => Tree::Oak,
            Species::Ash => Tree::Ash,
            Species::HolmOak | Species::Olive => Tree::HolmOak,
            Species::Poplar | Species::Cypress => Tree::Poplar,
            Species::Spruce => Tree::Spruce,
            Species::Fir | Species::Larch => Tree::Fir,
            Species::StonePine | Species::Pine | Species::Palm => Tree::Pine,
            Species::Maquis | Species::Shrub => Tree::Bush,
        }
    }

    pub fn look(self) -> &'static Look {
        &LOOKS[self as usize]
    }
}

/// The barks, layers of the bark arrays (`flora.rs`), in the order of `BARK_FILES`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bark {
    /// Deep furrows: oaks, ashes, poplars, olives, the stems of shrubs.
    Furrowed,
    /// Plates: pines, spruces, firs, larches, cypresses.
    Pine,
    /// Rings of old leaf bases.
    Palm,
    /// Smooth and grey: beeches.
    Smooth,
}

/// The files of the barks, in `assets/textures` (`_color.jpg`, `_normal.png`).
pub const BARK_FILES: [&str; 4] = ["bark_furrowed", "bark_pine", "bark_palm", "bark_smooth"];

/// How a species looks: what its mesh is made of and how its foliage takes the light.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    /// The albedo of its foliage, linear: the cards' pictures are scaled to it.
    pub leaf: Vec3,
    /// How much its leaves let through, by colour (thin broad leaves the most, needles the least).
    pub through: Vec3,
    /// How glossy the upper side of its leaves is, 0 to 1 (holm oaks and olives are waxy).
    pub sheen: f32,
    /// Its foliage's picture, its bark and the bark's tint.
    pub foliage: Layer,
    pub bark: Bark,
    pub bark_tint: Vec3,
    /// Its crown as an ellipsoid in the unit model (1 m tall, see `vegetation`): the height of its
    /// middle and its radii across and up. The shaders shade the crown as this volume.
    pub crown: (f32, f32, f32),
    /// How dense the crown is: the fraction of the sun's light a metre of it (in crown radii)
    /// stops, for its own shade.
    pub density: f32,
}

#[allow(clippy::too_many_arguments)]
const fn look(leaf: [f32; 3], through: [f32; 3], sheen: f32, foliage: Layer, bark: Bark, bark_tint: [f32; 3], crown: (f32, f32, f32), density: f32) -> Look {
    Look { leaf: Vec3::from_array(leaf), through: Vec3::from_array(through), sheen, foliage, bark, bark_tint: Vec3::from_array(bark_tint), crown, density }
}

/// In the order of `Species::ALL`. Leaf albedos after measured canopy reflectances (broadleaves
/// 0.05 to 0.10 in green, conifers half that), seasonally neutral: the green of early summer.
const LOOKS: [Look; COUNT] = [
    look([0.050, 0.078, 0.026], [0.55, 0.75, 0.20], 0.10, Layer::Broadleaf, Bark::Furrowed, [0.80, 0.78, 0.74], (0.63, 0.40, 0.33), 2.2),
    look([0.052, 0.088, 0.024], [0.60, 0.80, 0.18], 0.12, Layer::Broadleaf, Bark::Smooth, [1.05, 1.08, 1.12], (0.62, 0.35, 0.36), 2.6),
    look([0.058, 0.090, 0.030], [0.55, 0.78, 0.22], 0.08, Layer::Broadleaf, Bark::Furrowed, [1.00, 1.00, 1.00], (0.66, 0.31, 0.32), 1.6),
    look([0.034, 0.048, 0.026], [0.25, 0.35, 0.10], 0.30, Layer::SmallLeaf, Bark::Furrowed, [0.70, 0.68, 0.66], (0.62, 0.44, 0.33), 2.8),
    look([0.070, 0.078, 0.058], [0.30, 0.38, 0.18], 0.25, Layer::Olive, Bark::Furrowed, [1.15, 1.15, 1.12], (0.62, 0.42, 0.28), 1.4),
    look([0.055, 0.092, 0.034], [0.55, 0.80, 0.25], 0.15, Layer::Broadleaf, Bark::Furrowed, [1.10, 1.10, 1.08], (0.56, 0.14, 0.43), 2.0),
    look([0.026, 0.040, 0.022], [0.15, 0.22, 0.08], 0.05, Layer::Scale, Bark::Pine, [0.90, 0.85, 0.80], (0.54, 0.10, 0.46), 3.5),
    look([0.022, 0.038, 0.026], [0.15, 0.22, 0.10], 0.06, Layer::Spruce, Bark::Pine, [0.55, 0.50, 0.48], (0.50, 0.19, 0.46), 2.6),
    look([0.024, 0.044, 0.026], [0.15, 0.24, 0.10], 0.06, Layer::Spruce, Bark::Pine, [0.62, 0.62, 0.64], (0.52, 0.23, 0.44), 2.4),
    look([0.060, 0.098, 0.034], [0.45, 0.70, 0.20], 0.04, Layer::Larch, Bark::Pine, [0.95, 0.80, 0.75], (0.52, 0.21, 0.44), 1.2),
    look([0.032, 0.050, 0.026], [0.18, 0.28, 0.10], 0.08, Layer::Pine, Bark::Pine, [1.00, 0.92, 0.85], (0.82, 0.37, 0.11), 2.4),
    look([0.038, 0.056, 0.030], [0.20, 0.30, 0.10], 0.08, Layer::Pine, Bark::Pine, [1.05, 0.92, 0.82], (0.72, 0.30, 0.2), 1.5),
    look([0.060, 0.080, 0.032], [0.40, 0.55, 0.15], 0.25, Layer::Palm, Bark::Palm, [0.95, 0.92, 0.88], (0.86, 0.32, 0.14), 1.0),
    look([0.032, 0.046, 0.024], [0.22, 0.32, 0.10], 0.25, Layer::SmallLeaf, Bark::Furrowed, [0.70, 0.68, 0.66], (0.45, 0.48, 0.45), 3.0),
    look([0.048, 0.076, 0.028], [0.50, 0.72, 0.20], 0.08, Layer::Broadleaf, Bark::Furrowed, [0.80, 0.78, 0.74], (0.48, 0.47, 0.48), 2.6),
];

/// The table the shaders read the species from, by the number of a draw's species
/// (`ChunkUniforms::wrap.w`): `crown` = height of the middle, radius across, radius up, density;
/// `light` = what the leaves let through (rgb), their sheen.
pub fn wgsl() -> String {
    let rows = |f: &dyn Fn(&Look) -> [f32; 4]| Species::ALL.iter().map(|s| f(s.look())).map(|v| format!("vec4<f32>({:?}, {:?}, {:?}, {:?})", v[0], v[1], v[2], v[3])).collect::<Vec<_>>().join(", ");
    format!(
        "\n// The species of trees (species.rs), written by `pipelines::shader_source`.\nconst SPECIES_CROWN = array<vec4<f32>, {COUNT}>({});\nconst SPECIES_LIGHT = array<vec4<f32>, {COUNT}>({});\n",
        rows(&|l| [l.crown.0, l.crown.1, l.crown.2, l.density]),
        rows(&|l| [l.through.x, l.through.y, l.through.z, l.sheen]),
    )
}

/// What decides the species round a tile: the place's climate.
#[derive(Clone, Copy, Debug)]
pub struct Climate {
    /// Dry summers (`sim_core::scenery::summer_drought`), 0 to 1; `None` in a generated world,
    /// where the moisture of each point decides.
    drought: Option<f64>,
    /// How tropical, 0 to 1: palms are common there.
    tropic: f64,
    /// How far north (or south): the tree line and the larches come lower.
    latitude: f64,
}

impl Climate {
    pub fn of(terrain: &Terrain) -> Climate {
        match terrain.region() {
            Some(region) => {
                let latitude = region.latitude.abs() / DEG;
                Climate { drought: Some(scenery::summer_drought(terrain)), tropic: 1.0 - smoothstep(23.0, 29.0, latitude), latitude }
            }
            None => Climate { drought: None, tropic: 0.0, latitude: 42.0 },
        }
    }

    /// How Mediterranean a point is, 0 to 1.
    pub fn dry_at(&self, terrain: &Terrain, n: f64, e: f64) -> f64 {
        match self.drought {
            Some(d) => d,
            None => 1.0 - smoothstep(0.35, 0.65, scenery::moisture(terrain, n, e)),
        }
    }

    /// The height above which larches join the conifers, metres.
    fn larches(&self) -> f64 {
        1500.0 - 25.0 * (self.latitude - 47.0).max(0.0)
    }
}

/// Whether there is sea within a kilometre or so of a point, looked for at eight points round it.
fn by_the_sea(terrain: &Terrain, n: f64, e: f64) -> bool {
    (0..8).any(|k| {
        let a = k as f64 * std::f64::consts::FRAC_PI_4;
        let r = if k % 2 == 0 { 450.0 } else { 1000.0 };
        terrain.height(n + r * a.cos(), e + r * a.sin()) < 0.0
    })
}

/// Three numbers in 0..1 of a tree's own, from what the simulation gave it (the same in every tile
/// that holds it, unlike its position in the tile).
fn own(instance: &[f32]) -> [f64; 3] {
    let (tone, height, width) = (instance[4] as f64, instance[3] as f64, instance[5] as f64);
    let f = |x: f64| x - x.floor();
    [f(tone * 7.31 + height * 0.137), f(tone * 3.17 + width * 5.91), f(height * 1.713 + width * 3.3)]
}

/// The species of a tree of the simulation's `shape` at (north, east), `h` metres up, with its numbers `r`.
pub fn choose(shape: Tree, climate: &Climate, terrain: &Terrain, (n, e, h): (f64, f64, f64), r: [f64; 3]) -> Species {
    let dry = climate.dry_at(terrain, n, e) > 0.5;
    let tropic = climate.tropic > 0.5;
    match shape {
        Tree::Oak if dry || tropic => {
            if r[0] < 0.7 || tropic {
                Species::HolmOak
            } else {
                Species::Oak
            }
        }
        Tree::Oak => {
            if h > 650.0 + 500.0 * r[1] {
                Species::Beech
            } else {
                Species::Oak
            }
        }
        Tree::Ash => {
            if !dry && h > 800.0 && r[0] < 0.45 {
                Species::Beech
            } else if tropic {
                Species::HolmOak
            } else {
                Species::Ash
            }
        }
        Tree::HolmOak if dry => {
            if r[0] < 0.35 {
                Species::Olive
            } else {
                Species::HolmOak
            }
        }
        Tree::HolmOak => {
            if tropic {
                Species::HolmOak
            } else {
                Species::Oak
            }
        }
        Tree::Poplar => {
            if dry && r[0] < 0.45 {
                Species::Cypress
            } else {
                Species::Poplar
            }
        }
        Tree::Spruce => {
            if !dry && h > climate.larches() && r[0] < 0.3 {
                Species::Larch
            } else {
                Species::Spruce
            }
        }
        Tree::Fir => {
            if !dry && h > climate.larches() - 100.0 && r[0] < 0.4 {
                Species::Larch
            } else {
                Species::Fir
            }
        }
        Tree::Pine => {
            // Palms by warm coasts (and many in the tropics), never high up.
            let palms = if tropic {
                0.6
            } else if dry {
                0.22
            } else {
                0.0
            };
            if r[0] < palms && h < 80.0 && (tropic || by_the_sea(terrain, n, e)) {
                Species::Palm
            } else if dry || tropic {
                if r[1] < 0.5 { Species::StonePine } else { Species::Pine }
            } else {
                Species::Pine
            }
        }
        Tree::Bush => {
            if dry || tropic {
                Species::Maquis
            } else {
                Species::Shrub
            }
        }
    }
}

/// The simulation's trees of a tile (`key`, side `size`), by its eight shapes, as the species drawn:
/// one list per species, the instances as they were but for the width of one drawn for another
/// shape than its own (a holm oak where the simulation has an oak), narrowed or widened to the
/// crown the simulation runs into (`fit`).
pub fn assign(terrain: &Terrain, key: (i32, i32), size: f64, shapes: &Instances) -> Instances {
    let climate = Climate::of(terrain);
    let (ox, oz) = (key.0 as f64 * size, key.1 as f64 * size);
    let mut out: Instances = vec![Vec::new(); COUNT];
    for (shape, list) in SHAPES.iter().zip(shapes) {
        for instance in list.as_chunks::<8>().0 {
            let (n, e) = (-(instance[2] as f64 + oz), instance[0] as f64 + ox);
            let species = choose(*shape, &climate, terrain, (n, e, instance[1] as f64), own(instance));
            let start = out[species as usize].len();
            out[species as usize].extend_from_slice(instance);
            out[species as usize][start + 5] *= fit(species, *shape);
        }
    }
    out
}

/// What a species' width is multiplied by when it is drawn for `shape`: the ratio of the crowns
/// the simulation runs into.
pub fn fit(species: Species, shape: Tree) -> f32 {
    (scenery::CROWNS[shape as usize].1 / scenery::CROWNS[species.shape() as usize].1) as f32
}

/// The simulation's shapes in the order of its lists (`sim_core::scenery::trees`).
pub const SHAPES: [Tree; scenery::TREE_KINDS] = [Tree::Oak, Tree::Ash, Tree::HolmOak, Tree::Poplar, Tree::Spruce, Tree::Fir, Tree::Pine, Tree::Bush];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_species_is_drawn_for_its_own_shape() {
        for (i, s) in Species::ALL.iter().enumerate() {
            assert_eq!(*s as usize, i);
        }
        for (i, shape) in SHAPES.iter().enumerate() {
            assert_eq!(*shape as usize, i);
        }
        // Whatever the climate, a tree of a shape is drawn as a species of a crown like it.
        let terrain = Terrain::new(1);
        for shape in SHAPES {
            for k in 0..200 {
                let r = [(k % 10) as f64 / 10.0, (k / 10 % 10) as f64 / 10.0, 0.5];
                for climate in [
                    Climate { drought: Some(1.0), tropic: 0.0, latitude: 40.0 },
                    Climate { drought: Some(0.0), tropic: 0.0, latitude: 47.0 },
                    Climate { drought: Some(0.0), tropic: 1.0, latitude: 15.0 },
                ] {
                    let s = choose(shape, &climate, &terrain, (0.0, 0.0, 300.0 + 8.0 * k as f64), r);
                    // Its crown begins where the simulation's does; its width is fitted to it.
                    let (own, drawn) = (scenery::CROWNS[shape as usize], scenery::CROWNS[s.shape() as usize]);
                    assert!((own.0 - drawn.0).abs() < 0.07 && own.2 == drawn.2, "{s:?} for {}", shape as usize);
                    assert!((fit(s, shape) * drawn.1 as f32 - own.1 as f32).abs() < 1e-6);
                }
            }
        }
    }

    #[test]
    fn the_climate_decides() {
        let terrain = Terrain::new(1);
        let med = Climate { drought: Some(1.0), tropic: 0.0, latitude: 40.0 };
        let alps = Climate { drought: Some(0.0), tropic: 0.0, latitude: 47.3 };
        let r = [0.9, 0.9, 0.9];
        assert_eq!(choose(Tree::Bush, &med, &terrain, (0.0, 0.0, 50.0), r), Species::Maquis);
        assert_eq!(choose(Tree::Bush, &alps, &terrain, (0.0, 0.0, 50.0), r), Species::Shrub);
        assert_eq!(choose(Tree::HolmOak, &alps, &terrain, (0.0, 0.0, 500.0), r), Species::Oak);
        assert_eq!(choose(Tree::HolmOak, &med, &terrain, (0.0, 0.0, 50.0), [0.1, 0.5, 0.5]), Species::Olive);
        assert_eq!(choose(Tree::Spruce, &alps, &terrain, (0.0, 0.0, 1700.0), [0.1, 0.5, 0.5]), Species::Larch);
        assert_eq!(choose(Tree::Spruce, &alps, &terrain, (0.0, 0.0, 900.0), [0.1, 0.5, 0.5]), Species::Spruce);
        assert_eq!(choose(Tree::Pine, &med, &terrain, (0.0, 0.0, 300.0), [0.9, 0.2, 0.5]), Species::StonePine);
    }

    #[test]
    fn the_table_is_valid_wgsl_constants() {
        let text = wgsl();
        assert_eq!(text.matches("vec4<f32>(").count(), 2 * COUNT);
        assert!(!text.contains("NaN"));
    }
}
