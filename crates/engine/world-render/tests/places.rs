//! A real place drawn on a real device: the renderer starts on a generated world and is moved to a
//! small region (`WorldRenderer::set_terrain`), whose road is drawn from the road map, and whose
//! ground takes the colour of a picture of it (`WorldRenderer::set_imagery`). The same view with
//! and without the road, and with and without the picture, tells they are there. A region round
//! another middle put in its place (`replace_terrain`) is drawn with no hole the frame it comes. Two
//! fields of a picture meet sharp with their parcels (`Quality::ground_detail`), soft without. The
//! images are written to the test's temporary directory.

use engine::{wgpu, Game, Gpu, Input};
use sim_core::{Cover, Grid, Imagery, Region, Road, Terrain, Weather};
use glam::DVec3;
use std::sync::Arc;
use world_render::{buildings, to_render, Quality, Scene, ViewPoint, WorldRenderer};

struct Place {
    world: WorldRenderer,
    /// The region it moves to after its first frames.
    region: Option<Arc<Terrain>>,
    /// A picture of its ground, given with it, and whether its parcels are drawn (`Quality::ground_detail`).
    imagery: Option<Arc<Imagery>>,
    detail: bool,
    /// Another terrain of the region, put in its place once it is drawn whole, and the frame it was.
    next: Option<Arc<Terrain>>,
    replaced: Option<usize>,
    frames: usize,
}

impl Game for Place {
    fn samples(&self) -> u32 {
        self.world.quality().samples
    }

    fn update(&mut self, _input: &Input, _dt: f64, gpu: &Gpu) {
        self.frames += 1;
        if self.frames == 3 {
            if let Some(terrain) = self.region.take() {
                self.world.set_quality(gpu, Quality { ground_detail: self.detail, ..Quality::HIGH });
                self.world.set_terrain(gpu, terrain);
                self.world.set_imagery(gpu, self.imagery.take());
            }
        } else if self.frames > 3 && self.world.pending() == 0 {
            if let Some(terrain) = self.next.take() {
                self.world.replace_terrain(terrain);
                self.replaced = Some(self.frames);
            }
        }
        // 40 m over a flat field, looking north and down at the road 80 m ahead.
        let eye = DVec3::new(-60.0, 0.0, -60.0);
        let forward = DVec3::new(80.0, 0.0, 40.0).normalize();
        let right = DVec3::Z.cross(forward).normalize();
        let view = ViewPoint { position: eye, forward: to_render(forward), up: to_render(right.cross(forward)), right: to_render(right), fov_y: 50f32.to_radians(), shift_y: 0.0, near: 0.25 };
        let scene = Scene { view, hour: 12.0, time: 0.0, weather: &Weather::default(), height_above_ground: 40.0, body: None, casters: &[], objects: &[], glows: &[], lamps: &[], see_through: &[], wrecks: &[] };
        self.world.prepare(gpu, &scene);
    }

    fn draw(&mut self, _gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>) {
        self.world.draw_sky(pass);
        self.world.draw(pass);
    }
}

/// A flat field 20 m up, with a road 12 m wide running east to west across it, or none.
fn field(road: bool) -> Arc<Terrain> {
    let region = Region {
        name: "Campo".into(),
        latitude: 0.7,
        longitude: 0.0,
        heights: vec![Grid::centred(0.0, 0.0, 30_000.0, 30.0, 20.0f32)],
        cover: Grid::centred(0.0, 0.0, 30_000.0, 30.0, Cover::Grass as u8),
        airports: Vec::new(),
        roads: if road { vec![Road { n0: 20.0, e0: -3000.0, n1: 20.0, e1: 3000.0, width: 12.0 }] } else { Vec::new() },
        towns: Vec::new(),
        buildings: Vec::new(),
        bridges: Vec::new(),
    };
    Arc::new(Terrain::from_region(1, Arc::new(region)))
}

/// A picture of the field's ground from above: all of it a strong red, as no field is.
fn red() -> Arc<Imagery> {
    let near = Grid::centred(0.0, 0.0, 2_550.0, 10.0, [200u8, 40, 40, 255]);
    let far = Grid::centred(0.0, 0.0, 8_160.0, 32.0, [200u8, 40, 40, 255]);
    Arc::new(Imagery { levels: vec![near, far], years: vec![2025] })
}

fn draw(road: bool, imagery: bool) -> Vec<u8> {
    let name = match (road, imagery) {
        (true, _) => "road",
        (false, true) => "picture",
        (false, false) => "field",
    };
    let path = format!("{}/place_{name}.png", env!("CARGO_TARGET_TMPDIR"));
    engine::render_to_png(
        &path,
        320,
        200,
        1.0,
        |gpu| Place { world: WorldRenderer::new(gpu, Arc::new(Terrain::new(1)), buildings::stand_in()), region: Some(field(road)), imagery: imagery.then(red), detail: true, next: None, replaced: None, frames: 0 },
        |place, frames| {
            if place.region.is_some() || place.world.pending() > 0 {
                std::thread::sleep(std::time::Duration::from_millis(20));
                return frames > 1500;
            }
            true
        },
    );
    println!("image written to {path}");
    image::open(&path).expect("the picture").to_rgb8().into_raw()
}

/// The field without its road, replaced (`replace_terrain`) by the same with it once drawn: the
/// picture of the frame it is replaced in (`whole`: once everything is made again).
fn replaced(whole: bool) -> Vec<u8> {
    let path = format!("{}/place_replaced_{}.png", env!("CARGO_TARGET_TMPDIR"), if whole { "whole" } else { "at_once" });
    engine::render_to_png(
        &path,
        320,
        200,
        1.0,
        |gpu| Place { world: WorldRenderer::new(gpu, Arc::new(Terrain::new(1)), buildings::stand_in()), region: Some(field(false)), imagery: None, detail: true, next: Some(field(true)), replaced: None, frames: 0 },
        |place, frames| match place.replaced {
            None => {
                std::thread::sleep(std::time::Duration::from_millis(20));
                frames > 1500
            }
            Some(at) if whole => {
                std::thread::sleep(std::time::Duration::from_millis(20));
                (place.frames > at + 1 && place.world.pending() == 0) || frames > 3000
            }
            Some(at) => place.frames >= at,
        },
    );
    image::open(&path).expect("the picture").to_rgb8().into_raw()
}

#[test]
fn another_terrain_of_the_place_is_drawn_over_the_old_without_a_hole() {
    let (field, at_once, whole) = (draw(false, false), replaced(false), replaced(true));
    let differ = |a: &[u8], b: &[u8]| a.chunks(3).zip(b.chunks(3)).filter(|(a, b)| (0..3).any(|k| (a[k] as i32 - b[k] as i32).abs() > 25)).count();
    // The frame it is replaced in still draws the old ground, whole; once made again, the new road.
    println!("{} pixels differ at once, {} once made again", differ(&field, &at_once), differ(&field, &whole));
    assert!(differ(&field, &at_once) < 100, "a hole or a change at once");
    assert!(differ(&field, &whole) > 500, "the new terrain is not drawn");
}

#[test]
fn a_region_is_drawn_after_moving_to_it_with_its_roads() {
    let (with, without) = (draw(true, false), draw(false, false));
    // The asphalt is darker than the grass where the road crosses the picture.
    let darker = with.chunks(3).zip(without.chunks(3)).filter(|(a, b)| (b[1] as i32) - (a[1] as i32) > 25).count();
    println!("{darker} pixels of road");
    assert!(darker > 500, "{darker} pixels darker with the road");
}

#[test]
fn a_region_is_coloured_by_its_picture_from_above() {
    let (with, without) = (draw(false, true), draw(false, false));
    // The ground turns red where the picture is: red over green.
    let red = with.chunks(3).zip(without.chunks(3)).filter(|(a, b)| a[0] as i32 - a[1] as i32 > 40 && (b[0] as i32) - (b[1] as i32) < 10).count();
    println!("{red} pixels redder with the picture");
    assert!(red > 10_000, "{red} pixels red with the picture");
}

/// A picture of two fields side by side, pale to the west and green to the east of the line north
/// the view looks along.
fn two_fields() -> Arc<Imagery> {
    let paint = |mut g: Grid<[u8; 4]>| {
        for r in 0..g.rows {
            for c in 0..g.cols {
                let (_, e) = g.node(r, c);
                g.set(r, c, if e < 0.0 { [170, 150, 120, 255] } else { [90, 120, 60, 255] });
            }
        }
        g
    };
    let near = paint(Grid::centred(0.0, 0.0, 2_550.0, 10.0, [0; 4]));
    let far = paint(Grid::centred(0.0, 0.0, 8_160.0, 32.0, [0; 4]));
    Arc::new(Imagery { levels: vec![near, far], years: vec![2025] })
}

fn fields(detail: bool) -> Vec<u8> {
    let path = format!("{}/place_fields_{}.png", env!("CARGO_TARGET_TMPDIR"), if detail { "sharp" } else { "picture" });
    engine::render_to_png(
        &path,
        320,
        200,
        1.0,
        |gpu| Place { world: WorldRenderer::new(gpu, Arc::new(Terrain::new(1)), buildings::stand_in()), region: Some(field(false)), imagery: Some(two_fields()), detail, next: None, replaced: None, frames: 0 },
        |place, frames| {
            if place.region.is_some() || place.world.pending() > 0 {
                std::thread::sleep(std::time::Duration::from_millis(20));
                return frames > 3000;
            }
            true
        },
    );
    println!("image written to {path}");
    image::open(&path).expect("the picture").to_rgb8().into_raw()
}

#[test]
fn close_up_the_picture_is_cut_into_parcels_with_sharp_edges() {
    // In each row of the ground, the pixels between the two fields' colours: where one blurs
    // into the other (a texel of 10 m spans tens of pixels from 40 m), or a wall between them.
    let between = |image: &[u8]| {
        let mut count = 0;
        for y in 120..200 {
            let row = &image[y * 320 * 3..(y + 1) * 320 * 3];
            let green = |x: usize| row[x * 3 + 1] as f32 - row[x * 3] as f32;
            let (west, east) = (green(10), green(310));
            count += (10..310).filter(|&x| {
                let t = (green(x) - west) / (east - west);
                t > 0.2 && t < 0.8
            }).count();
        }
        count
    };
    let (sharp, soft) = (between(&fields(true)), between(&fields(false)));
    println!("{sharp} pixels between the fields with their parcels, {soft} with the picture alone");
    assert!(sharp * 3 < soft, "the edge is not sharper: {sharp} against {soft}");
}
