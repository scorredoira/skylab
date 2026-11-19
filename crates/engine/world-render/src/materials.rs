//! The photographs of ground and buildings (`assets/textures`): two texture arrays of one layer per
//! material, colour and normal + roughness + occlusion, and the sampler, for group 0 of every
//! pipeline (bindings 3, 4 and 5). `shaders.wgsl` has the layer numbers (`M_...`) and says which
//! material goes where; `assets/textures/APPLYING.md` explains the method.
//!
//! # Where the files are looked for
//!
//! The first of these folders that holds `grass_short_color.jpg`:
//! 1. `$FLIGHT_ASSETS/textures`;
//! 2. `assets/textures` next to the executable or in any folder above it (an installed game has
//!    `assets` beside the binary; `target/release/skylab` finds the repository's);
//! 3. `assets/textures` in the current folder or any folder above it;
//! 4. the `assets/textures` of the source tree the game was compiled from.
//!
//! On the web, `assets/textures` of the files the page downloaded (`engine::files`).

use crate::Quality;
use engine::texture::{prepare_layers, Kind, PreparedLayers, Texture, TextureSource};
use engine::{wgpu, Gpu};
use std::path::{Path, PathBuf};

/// The materials in array order. The shader's `M_...` constants are these positions.
pub const NAMES: [&str; 20] = [
    "grass_short", "grass_meadow", "grass_dry", "soil_ploughed", "crop_stubble", "crop_green", "forest_floor", "rock_granite", "rock_layered", "scree",
    "snow", "sand", "asphalt", "concrete", "dirt_road", "metal_painted", "metal_corrugated", "plaster", "roof_clay", "roof_slate",
];

/// The file every lookup checks for.
const PROBE: &str = "grass_short_color.jpg";

pub struct Materials {
    pub colour: Texture,
    pub detail: Texture,
    pub sampler: wgpu::Sampler,
    /// What the arrays were loaded for: `Quality::texture_size` and whether the detail array is the real one.
    loaded: (u32, bool),
}

/// The photographs of `Materials::read`, read and decoded, not yet on the GPU.
pub struct ReadMaterials {
    wanted: (u32, bool),
    colour: Option<PreparedLayers>,
    detail: Option<PreparedLayers>,
}

/// The folders `assets/textures` is looked for in, in order (see the module's documentation).
#[cfg(not(target_arch = "wasm32"))]
fn candidates() -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Some(dir) = std::env::var_os("FLIGHT_ASSETS") {
        found.push(Path::new(&dir).join("textures"));
    }
    let above = |start: Option<PathBuf>, found: &mut Vec<PathBuf>| {
        if let Some(start) = start {
            found.extend(start.ancestors().map(|dir| dir.join("assets/textures")));
        }
    };
    above(std::env::current_exe().ok().and_then(|exe| exe.canonicalize().ok()).and_then(|exe| exe.parent().map(Path::to_path_buf)), &mut found);
    above(std::env::current_dir().ok(), &mut found);
    found.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../assets/textures"));
    found
}

/// Web: where the page downloaded them to (`engine::files`).
#[cfg(target_arch = "wasm32")]
fn candidates() -> Vec<PathBuf> {
    vec![PathBuf::from("assets/textures")]
}

/// The folder with the textures, or the list of the places where they were not.
pub fn find_textures() -> Result<PathBuf, String> {
    let candidates = candidates();
    match candidates.iter().find(|dir| engine::files::exists(&dir.join(PROBE))) {
        Some(dir) => Ok(dir.clone()),
        None => {
            let tried: Vec<String> = candidates.iter().map(|dir| format!("  {}", dir.display())).collect();
            Err(format!("the textures of the game were not found: no {PROBE} in\n{}\nRun the game from its folder or set FLIGHT_ASSETS to the `assets` folder.", tried.join("\n")))
        }
    }
}

/// One texel per array: what the shaders read when they use no photographs.
fn blank(gpu: &Gpu, label: &str, kind: Kind, rgba: [u8; 4]) -> Texture {
    gpu.texture_array(label, &[TextureSource::Pixels { width: 1, height: 1, rgba: rgba.to_vec() }], kind).expect("a 1 x 1 texture")
}

impl Materials {
    /// Arrays of one grey texel, for a renderer that has not loaded the photographs (yet).
    pub fn blank(gpu: &Gpu) -> Materials {
        Materials {
            colour: blank(gpu, "materials: colour (none)", Kind::Color, [128, 128, 128, 255]),
            detail: blank(gpu, "materials: detail (none)", Kind::NormalPacked, [128, 128, 204, 255]),
            sampler: gpu.texture_sampler(),
            loaded: (0, false),
        }
    }

    /// What `quality` needs loaded.
    fn wanted(quality: &Quality) -> (u32, bool) {
        (quality.texture_size, quality.texture_size > 0 && quality.texture_relief > 0.0)
    }

    /// True when the arrays are the ones `quality` draws with.
    pub fn serves(&self, quality: &Quality) -> bool {
        self.loaded == Materials::wanted(quality)
    }

    /// Reads the photographs `quality` asks for from `dir`: both arrays, the colour alone, or nothing.
    pub fn load(gpu: &Gpu, dir: &Path, quality: &Quality) -> Result<Materials, String> {
        Materials::upload(gpu, &Materials::read(dir, quality)?)
    }

    /// The reading and decoding of `load`, which takes a while (some 0.1 to 0.25 s): on any thread.
    pub fn read(dir: &Path, quality: &Quality) -> Result<ReadMaterials, String> {
        let wanted = Materials::wanted(quality);
        if wanted.0 == 0 {
            return Ok(ReadMaterials { wanted, colour: None, detail: None });
        }
        let colours: Vec<TextureSource> = NAMES.iter().map(|name| TextureSource::file(dir.join(format!("{name}_color.jpg")))).collect();
        let colour = prepare_layers("materials: colour", &colours, Kind::Color).map_err(|e| e.to_string())?;
        let detail = if wanted.1 {
            let details: Vec<TextureSource> = NAMES
                .iter()
                .map(|name| {
                    // Not every material has an occlusion map; its channel is then 1.
                    let ao = dir.join(format!("{name}_ao.jpg"));
                    TextureSource::Channels { rg: dir.join(format!("{name}_normal.png")), b: Some(dir.join(format!("{name}_rough.jpg"))), a: engine::files::exists(&ao).then_some(ao) }
                })
                .collect();
            Some(prepare_layers("materials: normal, roughness, occlusion", &details, Kind::NormalPacked).map_err(|e| e.to_string())?)
        } else {
            None
        };
        Ok(ReadMaterials { wanted, colour: Some(colour), detail })
    }

    /// The arrays of what `read` read, on the GPU.
    pub fn upload(gpu: &Gpu, read: &ReadMaterials) -> Result<Materials, String> {
        let mut materials = Materials::blank(gpu);
        // Each level dropped halves the side: 1024 on disk, so 512 drops one.
        let drop_levels = (1024 / read.wanted.0.clamp(1, 1024)).ilog2();
        if let Some(colour) = &read.colour {
            materials.colour = gpu.upload_layers("materials: colour", colour, drop_levels).map_err(|e| e.to_string())?;
        }
        if let Some(detail) = &read.detail {
            materials.detail = gpu.upload_layers("materials: normal, roughness, occlusion", detail, drop_levels).map_err(|e| e.to_string())?;
        }
        materials.loaded = read.wanted;
        Ok(materials)
    }

    /// Whether what `read` read is what `quality` draws with.
    pub fn read_serves(read: &ReadMaterials, quality: &Quality) -> bool {
        read.wanted == Materials::wanted(quality)
    }

    /// Per layer: rgb = average colour (linear), w = average roughness. `FrameUniforms::materials`.
    pub fn means(&self) -> [[f32; 4]; 20] {
        let mut means = [[0.5, 0.5, 0.5, 0.8]; 20];
        for (layer, mean) in means.iter_mut().enumerate() {
            if let Some(colour) = self.colour.means.get(layer) {
                mean[..3].copy_from_slice(&colour[..3]);
            }
            if let Some(detail) = self.detail.means.get(layer).filter(|_| self.loaded.1) {
                mean[3] = detail[2];
            }
        }
        means
    }

    /// GPU memory of both arrays, bytes.
    pub fn bytes(&self) -> u64 {
        self.colour.bytes() + self.detail.bytes()
    }
}
