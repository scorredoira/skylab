//! How much the renderer draws, for machines that cannot afford everything.

/// What the renderer may leave out. [`Quality::HIGH`] is everything, as the game was designed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quality {
    /// Slices the cloud layer is drawn with; 0 draws no clouds (their shadows on the ground stay).
    pub cloud_slices: usize,
    /// How the volumetric clouds are marched (`clouds.rs`); `None` draws the flat slices instead.
    pub cloud_march: Option<CloudMarch>,
    /// The clouds marched at every pixel instead of one of every sixteen: for pictures and video.
    pub cloud_full: bool,
    /// Distance from the camera up to which trees are drawn, metres.
    pub tree_range: f64,
    /// Where a tree gives up its full mesh for the simpler one, and that one for its picture
    /// (`impostors.rs`), as times the distance a tree is drawn in detail in a picture 1000 pixels
    /// tall (220 m; `renderer.rs`, `TREE_DETAIL`).
    pub tree_detail: f32,
    pub tree_pictures: f32,
    /// Side of each view of a tree's picture from far, texels (64 views a species).
    pub tree_picture_size: u32,
    /// How far round the camera tufts of grass stand (`grass.rs`), metres; 0 = none. They are drawn
    /// when the eye is less than half as high.
    pub grass_range: f64,
    /// Steps each pixel of ground takes towards the sun to see if the relief shades it; 0 = no such shadows.
    pub relief_steps: u32,
    /// Samples per pixel that smooth the edges: 4 or 1 (the only counts every GPU has).
    pub samples: u32,
    /// Distance from the camera up to which terrain is drawn, metres. Below [`Quality::HIGH`]'s the
    /// haze is thickened to hide where it ends.
    pub view_distance: f64,
    /// Side of the photographs of ground and buildings in pixels: 1024 as they are on disk, 512 for a
    /// quarter of the memory, 0 for none (the ground is then the procedural one, and nothing is loaded).
    pub texture_size: u32,
    /// Distance from the camera at which the photographs have faded into their average colour, metres.
    pub texture_range: f32,
    /// Distance up to which the normal maps light the surface, metres; 0 = colour only, and the
    /// normal, roughness and occlusion maps are not loaded.
    pub texture_relief: f32,
    /// A second, shifted fetch of every photograph that hides its repetition seen from the air.
    pub texture_scatter: bool,
    /// Part of the light spread into a glow round what is bright (`engine::Post::bloom`); 0 saves its passes.
    pub bloom: f32,
    /// The fireball, the flames and the smoke of a crash as a gas simulated on the GPU (`fire.rs`);
    /// without it, as particles alone.
    pub fire_simulation: bool,
    /// How finely that gas is drawn, at half the picture's size: 1 in long steps, 2 in steps of a cell.
    pub fire_detail: u32,
    /// Trains of waves the water is drawn with (`sea.rs`, at most 32): the shortest are left out
    /// first, and their slopes go into the roughness the light is spread with.
    pub sea_waves: usize,
    /// The sky the water mirrors spread over its waves (three looks instead of one), the clouds
    /// in it, the bed of the sea with its photographs and the light the waves focus on it
    /// (without: its plain colour).
    pub sea_detail: bool,
    /// The land, trees, buildings and lights mirrored in the sea, drawn at the screen's size over
    /// this (`reflection.rs`); 0 = not drawn: the sea mirrors the sky alone.
    pub reflection: u32,
    /// Distance from the camera up to which the buildings of a real place's maps are drawn, metres.
    pub building_range: f64,
    /// A real place's ground cut into its fields, woods and scrub (`parcels.rs`), drawn sharp close up
    /// with their rows, walls and hedges, and the trees its picture shows; without, the picture alone.
    pub ground_detail: bool,
    /// The vehicles' shadows drawn from their meshes (`casters.rs`); without, the body followed
    /// casts its `ShadowShape` alone and the others none.
    pub vehicle_shadows: bool,
}

/// How the volumetric clouds are marched: the quality presets of takram's three-geospatial, whose
/// clouds ours are (`clouds.rs`; ported from packages/clouds/src/qualityPresets.ts, MIT, see
/// THIRD_PARTY_NOTICES.md).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudMarch {
    /// Steps of the march of the view at most, the first of them this long, m, and how far it goes, m.
    pub steps: u32,
    pub min_step: f32,
    pub max_ray: f32,
    /// The least density of the weather and extinction of the cloud that count as cloud.
    pub least: f32,
    /// What the clouds let through under which the march ends.
    pub min_transmittance: f32,
    /// Steps towards the sun and towards the ground from every sample in a cloud.
    pub sun_steps: u32,
    pub ground_steps: u32,
    /// The finer noise that frays the shapes, and the turbulence that pushes them about.
    pub shape_detail: bool,
    pub turbulence: bool,
    /// The shade of the clouds in the air: shafts of light.
    pub shafts: bool,
    /// The march of the shadow map: its steps, its side in texels and its thresholds.
    pub shadow_steps: u32,
    pub shadow_size: u32,
    pub shadow_least: f32,
    pub shadow_min_transmittance: f32,
}

impl CloudMarch {
    pub const HIGH: CloudMarch = CloudMarch { steps: 500, min_step: 50.0, max_ray: 200_000.0, least: 1e-5, min_transmittance: 1e-2, sun_steps: 2, ground_steps: 3, shape_detail: true, turbulence: true, shafts: true, shadow_steps: 50, shadow_size: 1024, shadow_least: 1e-5, shadow_min_transmittance: 1e-4 };
    pub const LOW: CloudMarch = CloudMarch { steps: 200, min_step: 100.0, max_ray: 100_000.0, least: 1e-4, min_transmittance: 1e-1, sun_steps: 1, ground_steps: 0, shape_detail: false, turbulence: false, shafts: false, shadow_steps: 25, shadow_size: 512, shadow_least: 1e-4, shadow_min_transmittance: 1e-2 };
    pub const MEDIUM: CloudMarch = CloudMarch { least: 1e-4, ground_steps: 1, turbulence: false, shafts: false, shadow_size: 512, shadow_least: 1e-4, ..CloudMarch::HIGH };
    pub const ULTRA: CloudMarch = CloudMarch { min_step: 10.0, shadow_size: 2048, ..CloudMarch::HIGH };
}

impl Quality {
    pub const HIGH: Quality = Quality { cloud_slices: 24, cloud_march: Some(CloudMarch::HIGH), cloud_full: false, tree_range: 2600.0, tree_detail: 0.55, tree_pictures: 1.15, tree_picture_size: 80, grass_range: 70.0, relief_steps: 22, samples: 4, view_distance: 72_000.0, texture_size: 1024, texture_range: 4000.0, texture_relief: 1500.0, texture_scatter: true, bloom: 0.03, fire_simulation: true, fire_detail: 2, sea_waves: 32, sea_detail: true, reflection: 2, building_range: 6000.0, ground_detail: true, vehicle_shadows: true };
    pub const MEDIUM: Quality = Quality { cloud_slices: 12, cloud_march: Some(CloudMarch::MEDIUM), cloud_full: false, tree_range: 1800.0, tree_detail: 0.45, tree_pictures: 1.0, tree_picture_size: 64, grass_range: 45.0, relief_steps: 12, samples: 4, view_distance: 56_000.0, texture_size: 1024, texture_range: 3000.0, texture_relief: 250.0, texture_scatter: true, bloom: 0.03, fire_simulation: true, fire_detail: 1, sea_waves: 24, sea_detail: true, reflection: 3, building_range: 4500.0, ground_detail: true, vehicle_shadows: true };
    pub const LOW: Quality = Quality { cloud_slices: 6, cloud_march: Some(CloudMarch::LOW), cloud_full: false, tree_range: 1000.0, tree_detail: 0.35, tree_pictures: 0.8, tree_picture_size: 32, grass_range: 0.0, relief_steps: 0, samples: 1, view_distance: 40_000.0, texture_size: 512, texture_range: 1500.0, texture_relief: 0.0, texture_scatter: false, bloom: 0.0, fire_simulation: false, fire_detail: 0, sea_waves: 12, sea_detail: false, reflection: 0, building_range: 2500.0, ground_detail: false, vehicle_shadows: false };
    /// For pictures and video, not for flying: `HIGH` with the clouds marched at every pixel in finer steps.
    pub const ULTRA: Quality = Quality { cloud_march: Some(CloudMarch::ULTRA), cloud_full: true, ..Quality::HIGH };

    /// How far the relief shadow looks with every step, as a factor of the step before: fewer steps
    /// are longer, to reach as far (some 22 km) as the 22 of the full quality.
    pub fn relief_growth(&self) -> f32 {
        if self.relief_steps >= 22 || self.relief_steps < 2 {
            1.3
        } else {
            1.3f32.powf(21.0 / (self.relief_steps - 1) as f32)
        }
    }

    /// Opacity of one cloud slice: fewer slices are thicker, so that the layer is as dense as with 24.
    pub fn cloud_opacity(&self) -> f32 {
        if self.cloud_slices >= 24 || self.cloud_slices == 0 {
            0.42
        } else {
            1.0 - (1.0f32 - 0.42).powf(24.0 / self.cloud_slices as f32)
        }
    }

    /// The visibility the haze is drawn with for the weather's, m: below the full quality's the haze
    /// thickens to hide the nearer end of the terrain (to 2 % of its contrast, at most, by its end).
    pub fn haze_visibility(&self, visibility: f64) -> f64 {
        if self.view_distance < Quality::HIGH.view_distance {
            visibility.min(self.view_distance)
        } else {
            visibility
        }
    }
}

/// The ready-made qualities the player chooses from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Ultra,
    High,
    Medium,
    Low,
}

impl Preset {
    pub const ALL: [Preset; 4] = [Preset::Ultra, Preset::High, Preset::Medium, Preset::Low];

    /// Name on the command line and in the settings file.
    pub fn name(self) -> &'static str {
        match self {
            Preset::Ultra => "ultra",
            Preset::High => "high",
            Preset::Medium => "medium",
            Preset::Low => "low",
        }
    }

    /// The preset of a name, or of the name it had before (`cine`, `alta`, `media`, `baja`), so
    /// that older settings files and scripts still work.
    pub fn named(name: &str) -> Option<Preset> {
        let name = match name {
            "cine" => "ultra",
            "alta" => "high",
            "media" => "medium",
            "baja" => "low",
            other => other,
        };
        Preset::ALL.into_iter().find(|p| p.name() == name)
    }

    pub fn quality(self) -> Quality {
        match self {
            Preset::Ultra => Quality::ULTRA,
            Preset::High => Quality::HIGH,
            Preset::Medium => Quality::MEDIUM,
            Preset::Low => Quality::LOW,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_quality_is_the_original_numbers() {
        let q = Quality::HIGH;
        assert_eq!((q.relief_growth(), q.cloud_opacity()), (1.3, 0.42));
        assert_eq!(q.haze_visibility(45_000.0), 45_000.0);
        assert_eq!(q.haze_visibility(80_000.0), 80_000.0);
    }

    #[test]
    fn lower_qualities_keep_the_reach_and_the_density() {
        for q in [Quality::MEDIUM, Quality::LOW] {
            // The whole stack of slices lets through what 24 slices of 0.42 do.
            let through = (1.0 - q.cloud_opacity()).powi(q.cloud_slices as i32);
            assert!((through - 0.58f32.powi(24)).abs() < 1e-6);
            assert!(q.haze_visibility(80_000.0) < Quality::HIGH.haze_visibility(80_000.0));
        }
        let reach = |q: Quality| 90.0 * q.relief_growth().powi(q.relief_steps as i32 - 1);
        assert!((reach(Quality::MEDIUM) / reach(Quality::HIGH) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn presets_have_names() {
        for p in Preset::ALL {
            assert_eq!(Preset::named(p.name()), Some(p));
        }
        assert_eq!(Preset::named("alta"), Some(Preset::High));
        assert_eq!(Preset::named("cine"), Some(Preset::Ultra));
        assert_eq!(Preset::named("best"), None);
        assert_eq!(Preset::High.quality(), Quality::HIGH);
    }
}
