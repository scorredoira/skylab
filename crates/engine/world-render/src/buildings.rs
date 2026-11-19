//! The buildings of towns and aerodromes, and what stands parked on their aprons. Every one is y up with
//! its origin on the ground at its middle; x runs along the ridge of its main roof.
//!
//! The alpha of a vertex colour says what the surface is, for `building_fragment`: `WALL` takes the
//! colour of the building's walls and has windows, `ROOF` takes the colour of its roof, `PLAIN`
//! keeps its own colour (stone, doors, chimneys, sheet metal, what is parked).
//!
//! Each building also has a hull: a few convex solids with every face turned outwards, which the
//! renderer flattens on to the ground along the sunlight to draw its shadow.

use engine::MeshBuilder;
use glam::{Vec3, Vec4};

pub const WALL: f32 = 0.0;
pub const PLAIN: f32 = 0.5;
pub const ROOF: f32 = 1.0;
/// A `PLAIN` surface of corrugated metal sheet: the renderer lays the photograph of one on it.
pub const SHEET: f32 = 0.6;

/// The buildings, in the order of the kinds of `sim_core::scenery::houses`; the last of those kinds,
/// [`PARKED`], is what stands parked on the aprons of aerodromes, and the game gives it
/// (`WorldRenderer::new`).
pub const KINDS: [fn() -> Building; 8] = [cottage, hip_house, corner_house, town_house, shed, church, hangar, clubhouse];
/// The kind of `sim_core::scenery::houses` that is a vehicle parked on an apron.
pub const PARKED: usize = KINDS.len();

pub struct Building {
    pub mesh: MeshBuilder,
    pub hull: MeshBuilder,
}

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn wall_color() -> Vec4 {
    Vec3::new(0.62, 0.58, 0.50).extend(WALL)
}

fn roof_color() -> Vec4 {
    Vec3::new(0.31, 0.115, 0.065).extend(ROOF)
}

/// A triangle whose normal points away from `inside`.
fn outward(b: &mut MeshBuilder, p: [Vec3; 3], inside: Vec3, color: Vec4) {
    let n = (p[1] - p[0]).cross(p[2] - p[0]);
    if n.dot(p[0] - inside) >= 0.0 {
        b.triangle(p[0], p[1], p[2], color);
    } else {
        b.triangle(p[0], p[2], p[1], color);
    }
}

/// A convex solid with a flat floor: the outline `floor` at height `base`, walls up to `eaves` and a
/// roof that rises to a ridge `ridge` above them, `hip` metres shorter than the outline at each end
/// (0 = gable ends). `centre` and `half` are the middle and half the size of the outline; the ridge
/// runs along x. With `closed` the floor is made too, as a hull needs.
#[allow(clippy::too_many_arguments)]
fn block(b: &mut MeshBuilder, centre: Vec3, half: (f32, f32), eaves: f32, ridge: f32, hip: f32, over: f32, wall: Vec4, roof: Vec4, closed: bool) {
    let (hx, hz) = half;
    let (x0, x1, z0, z1) = (centre.x - hx, centre.x + hx, centre.z - hz, centre.z + hz);
    let (y0, y1, y2) = (centre.y, centre.y + eaves, centre.y + eaves + ridge);
    let inside = v(centre.x, centre.y + eaves * 0.5, centre.z);
    let mut quad = |p: [Vec3; 4], color: Vec4| {
        outward(b, [p[0], p[1], p[2]], inside, color);
        outward(b, [p[0], p[2], p[3]], inside, color);
    };
    for (a, c) in [((x0, z0), (x1, z0)), ((x1, z0), (x1, z1)), ((x1, z1), (x0, z1)), ((x0, z1), (x0, z0))] {
        quad([v(a.0, y0, a.1), v(c.0, y0, c.1), v(c.0, y1, c.1), v(a.0, y1, a.1)], wall);
    }
    if closed {
        quad([v(x0, y0, z0), v(x1, y0, z0), v(x1, y0, z1), v(x0, y0, z1)], wall);
    }
    if ridge <= 0.0 {
        quad([v(x0, y1, z0), v(x1, y1, z0), v(x1, y1, z1), v(x0, y1, z1)], roof);
        return;
    }
    // The roof overhangs the walls by `over`, lower by what the slope falls in that distance.
    let drop = over * ridge / hz;
    let (ex0, ex1, ez0, ez1, ey) = (x0 - over, x1 + over, z0 - over, z1 + over, y1 - drop);
    let (r0, r1) = (v(x0 + hip - if hip > 0.0 { 0.0 } else { over }, y2, centre.z), v(x1 - hip + if hip > 0.0 { 0.0 } else { over }, y2, centre.z));
    quad([v(ex0, ey, ez0), v(ex1, ey, ez0), r1, r0], roof);
    quad([v(ex1, ey, ez1), v(ex0, ey, ez1), r0, r1], roof);
    if hip > 0.0 {
        outward(b, [v(ex0, ey, ez1), v(ex0, ey, ez0), r0], inside, roof);
        outward(b, [v(ex1, ey, ez0), v(ex1, ey, ez1), r1], inside, roof);
    } else {
        // Gable ends: the wall goes on up to the ridge.
        outward(b, [v(x0, y1, z0), v(x0, y1, z1), v(x0, y2, centre.z)], inside, wall);
        outward(b, [v(x1, y1, z0), v(x1, y1, z1), v(x1, y2, centre.z)], inside, wall);
    }
}

/// A building made of blocks: (centre, half size, eaves, ridge, hip, wall colour, roof colour). The
/// hull is the same blocks without overhang and with floors.
type Block = (Vec3, (f32, f32), f32, f32, f32, Vec4, Vec4);

fn of_blocks(blocks: &[Block]) -> Building {
    let (mut mesh, mut hull) = (MeshBuilder::new(), MeshBuilder::new());
    for &(centre, half, eaves, ridge, hip, wall, roof) in blocks {
        block(&mut mesh, centre, half, eaves, ridge, hip, 0.45, wall, roof, false);
        block(&mut hull, centre, half, eaves, ridge, hip, 0.0, Vec4::ZERO, Vec4::ZERO, true);
    }
    Building { mesh, hull }
}

fn chimney(b: &mut MeshBuilder, x: f32, z: f32, top: f32) {
    b.cuboid(v(x - 0.35, top - 2.2, z - 0.35), v(x + 0.35, top, z + 0.35), Vec3::new(0.40, 0.33, 0.28).extend(PLAIN));
}

/// A village house: two floors under a gable roof.
pub fn cottage() -> Building {
    let mut b = of_blocks(&[(Vec3::ZERO, (4.5, 3.5), 5.4, 2.3, 0.0, wall_color(), roof_color())]);
    chimney(&mut b.mesh, 2.4, 1.2, 7.9);
    b
}

/// A squarer house under a hip roof.
pub fn hip_house() -> Building {
    let mut b = of_blocks(&[(Vec3::ZERO, (5.0, 4.5), 5.8, 2.4, 3.2, wall_color(), roof_color())]);
    chimney(&mut b.mesh, -1.5, -1.0, 8.6);
    b
}

/// A house with a wing at a right angle, and a lean-to.
pub fn corner_house() -> Building {
    of_blocks(&[
        (v(-1.0, 0.0, -1.5), (5.5, 3.4), 5.6, 2.2, 0.0, wall_color(), roof_color()),
        (v(3.4, 0.0, 3.2), (3.0, 3.2), 3.0, 1.6, 2.0, wall_color(), roof_color()),
    ])
}

/// A taller house of the middle of a town: three floors and a shallow roof.
pub fn town_house() -> Building {
    let mut b = of_blocks(&[(Vec3::ZERO, (4.6, 4.2), 8.8, 1.7, 0.0, wall_color(), roof_color())]);
    chimney(&mut b.mesh, 2.0, -1.4, 10.9);
    b
}

/// A long farm or workshop shed with a sheet roof.
pub fn shed() -> Building {
    let metal = Vec3::new(0.36, 0.38, 0.40).extend(PLAIN);
    let side = Vec3::new(0.34, 0.33, 0.30).extend(PLAIN);
    let mut b = of_blocks(&[(Vec3::ZERO, (12.0, 5.5), 4.6, 1.8, 0.0, side, metal)]);
    b.mesh.cuboid(v(-2.2, 0.0, 5.5), v(2.2, 3.8, 5.62), Vec3::new(0.16, 0.19, 0.22).extend(PLAIN));
    b
}

/// A church: a nave and a square tower with a pointed roof.
pub fn church() -> Building {
    let stone = Vec3::new(0.50, 0.46, 0.40).extend(PLAIN);
    let mut b = of_blocks(&[
        (v(2.5, 0.0, 0.0), (10.0, 4.6), 8.0, 4.0, 0.0, stone, roof_color()),
        (v(-10.0, 0.0, 0.0), (2.9, 2.9), 19.0, 5.0, 2.9, stone, roof_color()),
    ]);
    // The openings of the belfry.
    let dark = Vec3::new(0.03, 0.03, 0.035).extend(PLAIN);
    for (dx, dz) in [(1.0f32, 0.0f32), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        let c = v(-10.0 + dx * 2.9, 16.0, dz * 2.9);
        let half = v(0.06 + dz.abs() * 0.75, 1.5, 0.06 + dx.abs() * 0.75);
        b.mesh.cuboid(c - half, c + half, dark);
    }
    b
}

/// A hangar: sheet walls, a shallow roof and a wide door on the +z side.
pub fn hangar() -> Building {
    let side = Vec3::new(0.50, 0.52, 0.53).extend(SHEET);
    let roof = Vec3::new(0.30, 0.33, 0.36).extend(SHEET);
    let mut b = of_blocks(&[(Vec3::ZERO, (11.0, 9.0), 5.6, 2.4, 0.0, side, roof)]);
    let door = Vec3::new(0.20, 0.23, 0.27).extend(SHEET);
    // The gable is over the door: the ridge runs from the door to the back.
    for vertex in b.mesh.data.as_chunks_mut::<10>().0.iter_mut().chain(b.hull.data.as_chunks_mut::<10>().0.iter_mut()) {
        for o in [0, 3] {
            (vertex[o], vertex[o + 2]) = (-vertex[o + 2], vertex[o]);
        }
    }
    // Sliding door leaves with the gaps between them, and a stripe along the top of the walls.
    for k in 0..4 {
        let x0 = -8.0 + k as f32 * 4.0;
        b.mesh.cuboid(v(x0 + 0.06, 0.0, 11.0), v(x0 + 3.94, 4.9, 11.0 + 0.1 + 0.05 * (k % 2) as f32), door);
    }
    let stripe = Vec3::new(0.55, 0.10, 0.07).extend(PLAIN);
    b.mesh.cuboid(v(-9.05, 5.0, -11.05), v(9.05, 5.5, 11.05), stripe);
    b
}

/// The aerodrome's office: a low building with a small glazed tower.
pub fn clubhouse() -> Building {
    let roof = Vec3::new(0.33, 0.34, 0.36).extend(PLAIN);
    let mut b = of_blocks(&[
        (Vec3::ZERO, (8.0, 4.5), 3.4, 0.0, 0.0, wall_color(), roof),
        (v(5.0, 0.0, 0.0), (2.4, 2.4), 7.0, 0.0, 0.0, wall_color(), roof),
    ]);
    // The cab: glass all round under a flat roof, and a mast.
    let glass = Vec3::new(0.03, 0.05, 0.06).extend(PLAIN);
    b.mesh.cuboid(v(2.3, 7.0, -2.7), v(7.7, 9.2, 2.7), glass);
    b.mesh.cuboid(v(2.0, 9.2, -3.0), v(8.0, 9.5, 3.0), roof);
    b.mesh.cylinder(v(5.0, 9.5, 0.0), v(5.0, 13.0, 0.0), 0.06, 0.04, 5, Vec3::new(0.7, 0.7, 0.7).extend(PLAIN));
    b
}

/// A closed box `half` wide (x, z) and `height` tall, its floor at `base`: a piece of a hull.
pub fn slab(hull: &mut MeshBuilder, base: Vec3, half: (f32, f32), height: f32) {
    block(hull, base, half, height, 0.0, 0.0, 0.0, Vec4::ZERO, Vec4::ZERO, true);
}

/// Something to stand on the aprons when the game gives nothing of its own: the solids the
/// simulation runs into there (`sim_core::scenery::BLOCKS[PARKED]`), plain and grey.
pub fn stand_in() -> Building {
    let (mut mesh, mut hull) = (MeshBuilder::new(), MeshBuilder::new());
    let grey = Vec3::splat(0.55).extend(PLAIN);
    for b in sim_core::scenery::BLOCKS[PARKED] {
        let (base, half, height) = (v(b.x as f32, b.floor as f32, b.z as f32), (b.half_x as f32, b.half_z as f32), (b.eaves - b.floor) as f32);
        block(&mut mesh, base, half, height, 0.0, 0.0, 0.0, grey, grey, true);
        slab(&mut hull, base, half, height);
    }
    Building { mesh, hull }
}

/// Whether a building of kind `kind` is one the renderer can draw and the simulation agrees with:
/// it stands on the ground, every surface is of a known kind, its hull is closed and faces out
/// (seen from far above, the faces turned up and those turned down cover the same area), and the
/// hull spans the solids of `sim_core::scenery::BLOCKS[kind]` within 0.3 m across and
/// `height_slack` in height. The tests run it on every kind; the game on what it parks.
pub fn check(kind: usize, building: &Building, height_slack: f32) -> Result<(), String> {
    let b = building;
    for mesh in [&b.mesh, &b.hull] {
        if mesh.count() == 0 || mesh.count() % 3 != 0 {
            return Err(format!("building {kind} has {} vertices", mesh.count()));
        }
        if !mesh.data.iter().all(|x| x.is_finite()) {
            return Err(format!("building {kind} has a non-finite value"));
        }
        let low = mesh.data.chunks(10).fold(f32::INFINITY, |low, v| low.min(v[1]));
        if !(-0.05..1.0).contains(&low) {
            return Err(format!("building {kind} starts at {low}"));
        }
    }
    if let Some(v) = b.mesh.data.chunks(10).find(|v| ![WALL, PLAIN, SHEET, ROOF].contains(&v[9])) {
        return Err(format!("building {kind} has a surface of kind {}", v[9]));
    }
    let (mut up, mut down) = (0.0, 0.0);
    for t in b.hull.data.chunks(30) {
        let p = |k: usize| Vec3::new(t[k * 10], t[k * 10 + 1], t[k * 10 + 2]);
        let area = (p(1) - p(0)).cross(p(2) - p(0)).y / 2.0;
        let normal_y = t[4];
        if area * normal_y < -1e-4 {
            return Err(format!("building {kind}: a hull face is wound against its normal"));
        }
        if area > 0.0 { up += area } else { down -= area }
    }
    if (up - down).abs() >= 0.02 * up {
        return Err(format!("building {kind}: hull covers {up} from above and {down} from below"));
    }
    // The simulation runs into `sim_core::scenery::BLOCKS`: the same solids the hulls are.
    let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for v in b.hull.data.chunks(10) {
        lo = lo.min(Vec3::new(v[0], v[1], v[2]));
        hi = hi.max(Vec3::new(v[0], v[1], v[2]));
    }
    let (mut blo, mut bhi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for b in sim_core::scenery::BLOCKS[kind] {
        blo = blo.min(Vec3::new((b.x - b.half_x) as f32, b.floor as f32, (b.z - b.half_z) as f32));
        bhi = bhi.max(Vec3::new((b.x + b.half_x) as f32, (b.eaves + b.ridge) as f32, (b.z + b.half_z) as f32));
    }
    if (lo.x - blo.x).abs() >= 0.3 || (hi.x - bhi.x).abs() >= 0.3 || (lo.z - blo.z).abs() >= 0.3 || (hi.z - bhi.z).abs() >= 0.3 {
        return Err(format!("building {kind}: {lo} {hi} against {blo} {bhi}"));
    }
    if (hi.y - bhi.y).abs() >= height_slack {
        return Err(format!("building {kind}: height {} against {}", hi.y, bhi.y));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_building_is_sound_and_is_what_the_simulation_runs_into() {
        for (i, make) in KINDS.iter().enumerate() {
            // The office's glazed cab, up to 9.5 m, casts no shadow of its own.
            let slack = if i == 7 { 2.6 } else { 0.05 };
            check(i, &make(), slack).unwrap();
        }
        check(PARKED, &stand_in(), 0.05).unwrap();
    }
}
