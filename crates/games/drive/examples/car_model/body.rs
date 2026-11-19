//! The GT coupé's shell: an original two-door fastback of no make. Its form is a field (`shell`):
//! the lower body is what its outline seen from the side and its outline seen from above share,
//! rounded where they meet, its flanks leaning in above and below the widest line, its bonnet and
//! deck crowned; the wings swell over the wheels; the greenhouse is the same made of two narrower
//! outlines, leaning in to the roof; the wheel arches, the nose's mouth and intakes and the
//! diffuser's hollow are cut out of it. Glass, lamps, the black lower trim and the shut lines are
//! outlines drawn on that surface from the side, the front or above (`split`).
//!
//! The tail, which the chase camera looks at, has its own form: the deck ends in a ducktail's lip;
//! under it the lamps' band is set into the shell (a red lens with a line of light along its top
//! and a row of lit cells towards each corner); under the band a panel is set back between the
//! haunches, with the number plate in its hollow; then the bumper, its top a shut line, two
//! reflectors in it; and the black valance round the diffuser's hollow, with its fins and the
//! exhausts' pipes.
//!
//! Design axes: x forward, y to the right, z up from the ground, metres; x = 0 is the centre of
//! gravity of `assets/cars/gt/car.ron`.

use glam::{Mat4, Vec3};

use crate::sdf::{band, capsule, ellipsoid, lathe, moved, openness, sift, smax, smin, smoothstep, split, sq, surface, Corner, Shape, Triangle};

/// Where the axles are, the tyres' radius, the radius of the arches round them.
pub const FRONT_X: f32 = 1.334;
pub const REAR_X: f32 = -1.136;
pub const TYRE_RADIUS: f32 = 0.333;
const ARCH: f32 = 0.374;
/// The wells go this far in from the middle.
const WELL_INNER: f32 = 0.50;

/// The number plate's hollow in the tail, seen from behind, and where its floor is.
fn plate(p: Vec3) -> f32 {
    crate::sdf::round_box2(glam::Vec2::new(p.y, p.z - PLATE_Z), glam::Vec2::new(0.285, 0.076), 0.018)
}
const PLATE_Z: f32 = 0.605;
const PLATE_FLOOR: f32 = -2.168;

/// The panel set back into the tail under the lamps, between the haunches, seen from behind.
fn tail_panel(p: Vec3) -> f32 {
    crate::sdf::round_box2(glam::Vec2::new(p.y, p.z - 0.612), glam::Vec2::new(0.69, 0.150), 0.06)
}

/// The height of the bumper's top round the tail: a shut line.
const BUMPER_TOP: f32 = 0.447;
/// Where the exhausts' pipes are across the car, and how high.
const EXHAUST_Y: f32 = 0.335;
const EXHAUST_Z: f32 = 0.262;

pub struct Body {
    side: Shape,
    plan: Shape,
    green_side: Shape,
    green_plan: Shape,
    mouth: Shape,
    intake: Shape,
    diffuser: Shape,
    vent: Shape,
    window: Shape,
    screen: Shape,
    rear_window: Shape,
    door: Shape,
    bonnet: Shape,
    hatch: Shape,
    head: Shape,
    tail: Shape,
}

impl Body {
    pub fn new() -> Body {
        Body {
            // The lower body from the side (x, height): chin, nose, bonnet, cowl, belt, deck, the
            // ducktail's lip, the tail, the diffuser's ramp, the flat floor.
            side: Shape::closed(
                &[
                    (2.05, 0.150), (2.215, 0.165), (2.240, 0.30), (2.215, 0.50), (2.12, 0.615), (1.80, 0.715), (1.334, 0.775), (0.90, 0.835),
                    (0.20, 0.885), (-0.60, 0.905), (-1.136, 0.935), (-1.70, 0.958), (-2.02, 0.975), (-2.14, 1.004), (-2.205, 1.040), (-2.205, 1.040),
                    (-2.212, 0.95), (-2.225, 0.70), (-2.215, 0.42), (-2.19, 0.27), (-2.08, 0.185), (-1.80, 0.155), (-1.40, 0.145), (0.0, 0.140),
                ],
                3,
            ),
            // From above (x, half width): a round nose, the waist at the doors, the hips over the rear wheels.
            plan: Shape::mirrored_second(
                &[
                    (2.240, 0.0), (2.240, 0.42), (2.17, 0.74), (2.02, 0.90), (1.60, 0.945), (1.00, 0.935), (0.30, 0.915), (-0.40, 0.93),
                    (-1.00, 0.972), (-1.60, 0.965), (-2.00, 0.90), (-2.17, 0.76), (-2.22, 0.45), (-2.22, 0.0),
                ],
                3,
            ),
            // The greenhouse from the side: a raked screen, the roof's peak over the seats, a long fastback.
            green_side: Shape::closed(
                &[(1.05, 0.70), (0.95, 0.80), (0.42, 1.10), (0.12, 1.250), (-0.30, 1.298), (-0.80, 1.238), (-1.35, 1.09), (-1.85, 0.965), (-2.05, 0.90), (-1.90, 0.60), (0.90, 0.60)],
                3,
            ),
            green_plan: Shape::mirrored_second(&[(1.00, 0.0), (1.00, 0.50), (0.80, 0.76), (0.30, 0.80), (-0.50, 0.80), (-1.20, 0.77), (-1.80, 0.70), (-2.05, 0.55), (-2.10, 0.0)], 3),
            // Openings seen from the front (y, height): the mouth, an intake at each corner; from
            // behind, the diffuser's hollow.
            mouth: Shape::mirrored_first(&[(0.0, 0.205), (0.40, 0.205), (0.57, 0.275), (0.50, 0.405), (0.0, 0.405)], 2),
            intake: Shape::closed(&[(0.655, 0.235), (0.86, 0.255), (0.885, 0.36), (0.80, 0.415), (0.64, 0.40), (0.635, 0.30)], 2),
            diffuser: Shape::mirrored_first(&[(0.0, 0.10), (0.60, 0.10), (0.70, 0.30), (0.60, 0.365), (0.0, 0.365)], 2),
            // A vent at each corner of the tail, and the hollow for the number plate between them.
            vent: Shape::closed(&[(0.72, 0.45), (0.86, 0.475), (0.885, 0.60), (0.84, 0.64), (0.74, 0.52)], 2),
            // The side glass from the side (x, height), tapering back with the fastback.
            window: Shape::closed(&[(0.72, 0.928), (0.50, 1.04), (0.15, 1.21), (-0.35, 1.252), (-0.90, 1.175), (-1.28, 1.065), (-1.22, 0.985), (-0.60, 0.957), (0.0, 0.938)], 2),
            // The windscreen from the front (y, height) and the rear glass from above (x, y).
            screen: Shape::mirrored_first(&[(0.0, 0.868), (0.54, 0.868), (0.735, 0.905), (0.725, 0.95), (0.60, 1.215), (0.0, 1.232)], 2),
            rear_window: Shape::mirrored_second(&[(-0.66, 0.0), (-0.66, 0.50), (-1.05, 0.545), (-1.66, 0.47), (-1.75, 0.0)], 2),
            // Shut lines: the door from the side, the bonnet and the hatch from above.
            door: Shape::closed(&[(0.80, 0.215), (0.80, 0.215), (0.755, 0.55), (0.71, 1.02), (-0.50, 1.02), (-0.565, 0.60), (-0.52, 0.215), (-0.52, 0.215)], 2),
            bonnet: Shape::mirrored_second(&[(0.905, 0.0), (0.905, 0.56), (1.04, 0.705), (1.72, 0.715), (2.01, 0.55), (2.095, 0.0)], 2),
            hatch: Shape::mirrored_second(&[(-0.555, 0.0), (-0.555, 0.61), (-1.0, 0.665), (-1.93, 0.655), (-2.10, 0.50), (-2.13, 0.0)], 2),
            // The lamps: a slim blade at each front corner, seen from the front (y, height); a
            // band across the tail, seen from behind.
            head: Shape::closed(&[(0.47, 0.565), (0.80, 0.600), (0.905, 0.685), (0.905, 0.685), (0.875, 0.735), (0.74, 0.705), (0.50, 0.648), (0.44, 0.60)], 2),
            tail: Shape::mirrored_first(&[(0.0, 0.812), (0.80, 0.812), (0.91, 0.862), (0.91, 0.862), (0.85, 0.908), (0.0, 0.896)], 2),
        }
    }

    /// The painted shell as a field: negative inside.
    pub fn shell(&self, p: Vec3) -> f32 {
        let (x, ay, h) = (p.x, p.y.abs(), p.z);
        // The bonnet and deck fall away to the sides; the flanks lean in from the widest line.
        let crown = 0.045 * sq(ay / 0.9).min(1.5) * smoothstep(0.35, 0.6, h);
        let inset = if h > 0.6 { 0.085 * sq((h - 0.6) / 0.33) } else { 0.05 * sq((0.6 - h) / 0.45) };
        let lower = smax(self.side.sd(x, h + crown), self.plan.sd(x, ay) + inset, 0.11);
        // The wings over the wheels.
        let front_wing = ellipsoid(Vec3::new(x - FRONT_X, ay - 0.70, h - 0.50), Vec3::new(0.78, 0.235, 0.31));
        let rear_wing = ellipsoid(Vec3::new(x - (REAR_X + 0.03), ay - 0.72, h - 0.56), Vec3::new(0.86, 0.25, 0.385));
        let mut d = smin(smin(lower, front_wing, 0.10), rear_wing, 0.12);
        // A scallop along the flank between the wheels, rising to the rear: it catches the light.
        let between = smoothstep(FRONT_X - 0.50, FRONT_X - 0.75, x) * smoothstep(REAR_X + 0.42, REAR_X + 0.70, x);
        let line = 0.40 + 0.085 * (FRONT_X - 0.6 - x);
        d += 0.016 * between * (-sq((h - line) / 0.085)).exp() * smoothstep(0.6, 0.8, ay);
        // The greenhouse, leaning in to the roof.
        let lean = 0.40 * (h - 0.88).max(0.0);
        let green = smax(self.green_side.sd(x, h + 0.03 * sq(ay / 0.6)), self.green_plan.sd(x, ay) + lean, 0.13);
        d = smin(d, green, 0.035);
        // The arches: a lip round each, then the well.
        for axle in [FRONT_X, REAR_X] {
            let rc = (x - axle).hypot(h - TYRE_RADIUS);
            d -= 0.013 * (-sq((rc - ARCH - 0.03) / 0.032)).exp() * smoothstep(0.55, 0.75, ay);
            d = smax(d, -(rc - ARCH).max(WELL_INNER - ay), 0.012);
        }
        // The mouth and the intakes in the nose, the diffuser's hollow under the tail.
        d = smax(d, -self.mouth.sd(p.y, h).max(2.06 - x), 0.012);
        d = smax(d, -self.intake.sd(ay, h).max(1.985 - x), 0.012);
        d = smax(d, -self.diffuser.sd(p.y, h).max(x + 2.02), 0.012);
        d = smax(d, -self.vent.sd(ay, h).max(x + 2.0), 0.01);
        // The tail: the lamps' band set 11 mm into the shell, and under it a panel set back 24 mm
        // between the haunches, their edges a step (it catches the light above and is dark below).
        let rear = smoothstep(-1.93, -2.03, x);
        if rear > 0.0 {
            d += 0.011 * rear * smoothstep(0.004, -0.004, self.tail.sd(p.y, h));
            d += 0.024 * rear * smoothstep(0.008, -0.008, tail_panel(p));
        }
        d = smax(d, -plate(p).max(x - PLATE_FLOOR), 0.006);
        d
    }

    /// The shell with what shades it: the ground and the wheels in their arches.
    fn shaded(&self, p: Vec3) -> f32 {
        let mut d = self.shell(p).min(p.z);
        for axle in [FRONT_X, REAR_X] {
            let rc = (p.x - axle).hypot(p.z - TYRE_RADIUS);
            d = d.min((rc - TYRE_RADIUS).max(0.62 - p.y.abs()).max(p.y.abs() - 0.93));
        }
        d
    }

    /// The glass on the shell: negative on the side windows, the windscreen and the rear window.
    fn glass(&self, c: &Corner) -> f32 {
        let (x, h) = (c.p.x, c.p.z);
        let facing = |limit: f32| limit * 0.2;
        let side = self.window.sd(x, h).max(facing(0.62 - c.n.y.abs()));
        let screen = self.screen.sd(c.p.y, h).max(facing(c.n.y.abs() - 0.50)).max(facing(0.2 - c.n.x)).max(0.05 - x);
        let rear = self.rear_window.sd(x, c.p.y).max(facing(c.n.y.abs() - 0.42)).max(facing(c.n.x + 0.1));
        side.min(screen).min(rear)
    }

    /// The shell's triangles by material, and what stands on it (splitter, fins, exhausts, mirrors).
    pub fn build(&self, cell: f32) -> Vec<(&'static str, Vec<Triangle>)> {
        let field = |p: Vec3| self.shell(p);
        let shaded = |p: Vec3| self.shaded(p);
        let open = |p: Vec3, n: Vec3| openness(&shaded, p, n);
        let all = surface(&field, Vec3::new(-2.30, -1.02, 0.08), Vec3::new(2.30, 1.02, 1.34), cell, &open);
        let mut out: Vec<(&'static str, Vec<Triangle>)> = Vec::new();

        // The wells, and everything under the floor.
        let (liner, rest) = split(&all, &|c: &Corner| {
            let wells = [FRONT_X, REAR_X].iter().map(|axle| ((c.p.x - axle).hypot(c.p.z - TYRE_RADIUS) - ARCH - 0.004).max(WELL_INNER - 0.01 - c.p.y.abs())).fold(f32::MAX, f32::min);
            wells.min((c.p.z - 0.17).max((c.n.z + 0.5) * 0.2))
        });
        out.push(("liner", liner));
        // The grilles in the mouth and the intakes; the diffuser.
        let (grille, rest) = split(&rest, &|c: &Corner| (self.mouth.sd(c.p.y, c.p.z).min(self.intake.sd(c.p.y.abs(), c.p.z)) - 0.004).max(1.9 - c.p.x));
        out.push(("grille", grille));
        let (diffuser, rest) = split(&rest, &|c: &Corner| (self.diffuser.sd(c.p.y, c.p.z) - 0.004).max(c.p.x + 1.8));
        // The hollow is in the tail's shade (the field's openness, which looks along the normal,
        // sees out of its back wall as if nothing hung over it).
        out.push(("carbon", shaded_by(diffuser, 0.4)));
        let (vents, rest) = split(&rest, &|c: &Corner| (self.vent.sd(c.p.y.abs(), c.p.z) - 0.004).max(c.p.x + 1.9));
        out.push(("grille", vents));
        let (plate, rest) = split(&rest, &|c: &Corner| (plate(c.p) - 0.002).max(c.p.x + 2.0));
        out.push(("trim", plate));
        // The glass, and the black seal round it.
        let (glass, rest) = split(&rest, &|c: &Corner| self.glass(c));
        out.push(("glass", glass));
        let (seal, rest) = split(&rest, &|c: &Corner| self.glass(c) - 0.012);
        out.push(("trim", seal));
        // The head lamps: a line of light just inside the lens's edge, three projectors, the dark housing.
        let head = |c: &Corner| self.head.sd(c.p.y.abs(), c.p.z).max(1.5 - c.p.x);
        let (lamp, rest) = split(&rest, &head);
        let (ring, mut lamp) = band(&lamp, &|c: &Corner| head(c) + 0.011, &|_: &Corner| 0.0032);
        // The line runs along the lens's upper edge and round its outer tip only.
        let (under, ring) = split(&ring, &|c: &Corner| c.p.z - 0.612 - 0.13 * (c.p.y.abs() - 0.47));
        lamp.extend(under);
        let (beams, housing) = split(&lamp, &|c: &Corner| {
            [(0.585, 0.612), (0.685, 0.636), (0.785, 0.664)].iter().map(|&(y, h)| (c.p.y.abs() - y).hypot(c.p.z - h) - 0.019).fold(f32::MAX, f32::min).max(head(c) + 0.02)
        });
        out.push(("lamp_ring", ring));
        out.push(("lamp_head", beams));
        out.push(("lamp_dark", housing));
        // The tail lamp: a band across the tail, set into the shell. Its black bezel (the step
        // and a rim inside it), a red lens, and what lights in it: a line along its top from
        // corner to corner (the tail light), and under the line a row of cells towards each
        // corner (the brake lights), each cell apart from the next.
        let tail = |c: &Corner| self.tail.sd(c.p.y, c.p.z).max(c.p.x + 1.97);
        let (lens, rest) = split(&rest, &|c: &Corner| tail(c) - 0.004);
        let (inner, bezel) = split(&lens, &|c: &Corner| tail(c) + 0.009);
        let line_z = |c: &Corner| 0.874 + 0.012 * sq(c.p.y / 0.9);
        let (line, inner) = band(&inner, &|c: &Corner| c.p.z - line_z(c), &|_: &Corner| 0.0065);
        let (cells, lens) = split(&inner, &|c: &Corner| {
            let ay = c.p.y.abs();
            // A block from 0.40 m out to the corner, between the line and the band's foot; the
            // game's shader parts it into cells.
            (0.40 - ay).max(ay - 0.865).max(c.p.z - line_z(c) + 0.014).max(0.826 + 0.022 * smoothstep(0.62, 0.86, ay) - c.p.z)
        });
        out.push(("lamp_tail", line));
        out.push(("lamp_brake", cells));
        out.push(("lamp_lens", lens));
        out.push(("lamp_dark", bezel));
        // A red reflector at each end of the bumper, low.
        let (reflex, rest) = split(&rest, &|c: &Corner| crate::sdf::round_box2(glam::Vec2::new(c.p.y.abs() - 0.71, c.p.z - 0.318), glam::Vec2::new(0.075, 0.012), 0.008).max(c.p.x + 1.9));
        out.push(("lamp_reflex", reflex));
        // The black lower trim: the sills, the chin, the valance under the tail round the diffuser.
        let (lower, mut rest) = split(&rest, &|c: &Corner| {
            let top = 0.20 + (0.165 + 0.035 * smoothstep(0.80, 0.55, c.p.y.abs())) * smoothstep(-1.86, -2.06, c.p.x) + 0.02 * smoothstep(1.9, 2.1, c.p.x);
            c.p.z - top
        });
        // Under the tail the valance is tucked in under the bumper.
        let (valance, lower) = sift(lower, &|c: &Corner| c.p.x < -1.9);
        out.push(("trim", shaded_by(valance, 0.62)));
        out.push(("trim", lower));
        // The shut lines, 7 mm wide: the door's on the flanks, the bonnet's and the hatch's on top.
        let mut gaps = Vec::new();
        let width = |_: &Corner| 0.0035;
        // Each line: its outline, whether it is drawn from the side (or from above), and where it shows.
        type Shows<'a> = &'a (dyn Fn(&Corner) -> bool + Sync);
        let lines: [(&Shape, bool, Shows); 3] = [
            (&self.door, true, &|c: &Corner| c.n.y.abs() > 0.5 && c.p.y.abs() > 0.5 && c.p.z < 1.0),
            (&self.bonnet, false, &|c: &Corner| c.n.z > 0.3 && c.p.x > 0.6),
            (&self.hatch, false, &|c: &Corner| c.n.z > 0.25 && c.p.x < -0.3),
        ];
        for (shape, from_side, valid) in lines {
            let (line, others) = band(&rest, &|c: &Corner| if from_side { shape.sd(c.p.x, c.p.z) } else { shape.sd(c.p.x, c.p.y) }, &width);
            let (line, back) = sift(line, &|c: &Corner| valid(c));
            rest = others;
            rest.extend(back);
            gaps.extend(line);
        }
        // The bumper's top: a line round the tail from one rear arch to the other, under the panel.
        let (line, others) = band(&rest, &|c: &Corner| c.p.z - BUMPER_TOP - 0.03 * smoothstep(-1.95, -1.62, c.p.x), &width);
        let (line, back) = sift(line, &|c: &Corner| c.p.x < REAR_X - ARCH - 0.035 && c.n.z.abs() < 0.75);
        rest = others;
        rest.extend(back);
        gaps.extend(line);
        // The panel's edge, a fine dark line where it steps back.
        let (line, others) = band(&rest, &|c: &Corner| tail_panel(c.p), &|_: &Corner| 0.0025);
        let (line, back) = sift(line, &|c: &Corner| c.p.x < -2.0 && c.p.z < 0.73);
        rest = others;
        rest.extend(back);
        gaps.extend(line);
        out.push(("gap", gaps));
        out.push(("paint", rest));

        self.fittings(&mut out);
        out
    }

    /// What is fitted to the shell: the splitter, the diffuser's fins, the exhausts, the mirrors.
    fn fittings(&self, out: &mut Vec<(&'static str, Vec<Triangle>)>) {
        // The splitter: a blade under the nose, following its outline a little proud of it.
        let mut splitter = Vec::new();
        let edge: Vec<glam::Vec2> = {
            let mut arc: Vec<glam::Vec2> = self.plan.points.iter().copied().filter(|p| p.x > 1.62).collect();
            arc.sort_by(|a, b| a.y.total_cmp(&b.y));
            arc.iter().map(|p| glam::Vec2::new(1.62 + (p.x - 1.62) * 1.05 + 0.012, p.y * 0.985)).collect()
        };
        let (top, bottom) = (0.150, 0.128);
        let centre = glam::Vec2::new(1.62, 0.0);
        let corner = |p: glam::Vec2, h: f32, n: Vec3| Corner { p: Vec3::new(p.x, p.y, h), n, open: 0.8 };
        for w in edge.windows(2) {
            let out_n = Vec3::new(w[1].y - w[0].y, -(w[1].x - w[0].x), 0.0).normalize_or(Vec3::X);
            splitter.push([corner(centre, top, Vec3::Z), corner(w[0], top, Vec3::Z), corner(w[1], top, Vec3::Z)]);
            splitter.push([corner(centre, bottom, Vec3::NEG_Z), corner(w[1], bottom, Vec3::NEG_Z), corner(w[0], bottom, Vec3::NEG_Z)]);
            splitter.push([corner(w[0], bottom, out_n), corner(w[1], bottom, out_n), corner(w[1], top, out_n)]);
            splitter.push([corner(w[0], bottom, out_n), corner(w[1], top, out_n), corner(w[0], top, out_n)]);
        }
        out.push(("carbon", splitter));
        // The diffuser's fins: eight blades standing in its hollow, their rear edges raked, a
        // little proud of the valance.
        let mut fins = Vec::new();
        for y in [0.085f32, 0.205, 0.465, 0.585] {
            for side in [-1.0f32, 1.0] {
                fins.extend(blade(&[(-1.93, 0.17), (-2.185, 0.205), (-2.222, 0.352), (-1.93, 0.352)], side * y, 0.006));
            }
        }
        out.push(("fin", fins));
        // The number plate, in its hollow: the game's shader letters it.
        let face = |y: f32, z: f32| Corner { p: Vec3::new(PLATE_FLOOR - 0.004, y, PLATE_Z + z), n: Vec3::NEG_X, open: 0.85 };
        let q = [face(-0.26, -0.055), face(0.26, -0.055), face(0.26, 0.055), face(-0.26, 0.055)];
        out.push(("plate", vec![[q[0], q[1], q[2]], [q[0], q[2], q[3]]]));
        // Twin exhausts: a polished pipe with a rolled lip, a second wall inside it burnt dark,
        // and the dark of the pipe 9 cm in.
        for y in [-EXHAUST_Y, EXHAUST_Y] {
            let to = Mat4::from_translation(Vec3::new(-2.12, y, EXHAUST_Z)) * Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2);
            out.push(("chrome", moved(&lathe(&[(0.0, 0.058), (0.108, 0.058), (0.116, 0.0545), (0.108, 0.051)], 32, false), to)));
            out.push(("burnt", moved(&lathe(&[(0.108, 0.051), (0.0, 0.049)], 32, false), to)));
            out.push(("soot", moved(&lathe(&[(0.022, 0.050), (0.022, 0.0)], 32, false), to)));
        }
        // The door mirrors: a body on a short arm from the door's shoulder; the glass in its back.
        for side in [-1.0f32, 1.0] {
            let at = Vec3::new(0.61, side * 0.965, 0.972);
            let field = |p: Vec3| {
                let body = ellipsoid(p - at - Vec3::new(0.02, 0.0, 0.0), Vec3::new(0.085, 0.098, 0.046)).max(at.x - 0.04 - p.x);
                let arm = capsule(p, Vec3::new(0.69, side * 0.80, 0.918), at + Vec3::new(0.015, -side * 0.06, -0.022), 0.019);
                smin(body, arm, 0.02)
            };
            let open = |_: Vec3, _: Vec3| 0.9;
            let all = surface(&field, at - Vec3::new(0.12, 0.22, 0.12), at + Vec3::new(0.12, 0.22, 0.10), 0.006, &open);
            // The glass: the flat back of the body, a rim of the shell left round it.
            let (glass, shell) = split(&all, &|c: &Corner| (c.p.x - at.x + 0.037).max((((c.p.y - at.y) / 0.098).hypot((c.p.z - at.z) / 0.046) - 0.80) * 0.05));
            let (arm, shell) = split(&shell, &|c: &Corner| side * (c.p.y - at.y) + 0.092);
            out.push(("mirror", glass));
            out.push(("trim", arm));
            out.push(("paint", shell));
        }
    }
}

/// Triangles a part of the car hangs over: less open to the sky than the field says, by `share`.
fn shaded_by(triangles: Vec<Triangle>, share: f32) -> Vec<Triangle> {
    triangles.into_iter().map(|t| t.map(|c| Corner { open: c.open * share, ..c })).collect()
}

/// A flat blade standing along the car: `profile` as (x, height) points in order round it, at
/// `y` across, `half` thick either side.
fn blade(profile: &[(f32, f32)], y: f32, half: f32) -> Vec<Triangle> {
    let mut out = Vec::new();
    let at = |k: usize, side: f32, n: Vec3| Corner { p: Vec3::new(profile[k % profile.len()].0, y + side * half, profile[k % profile.len()].1), n, open: 0.42 };
    for side in [-1.0f32, 1.0] {
        let n = Vec3::new(0.0, side, 0.0);
        for k in 1..profile.len() - 1 {
            out.push([at(0, side, n), at(k, side, n), at(k + 1, side, n)]);
        }
    }
    for k in 0..profile.len() {
        let (a, b) = (profile[k], profile[(k + 1) % profile.len()]);
        let n = Vec3::new(b.1 - a.1, 0.0, -(b.0 - a.0)).normalize_or(Vec3::X);
        // The outline may run either way round: the edge's normal points away from the blade's middle.
        let middle = profile.iter().fold(glam::Vec2::ZERO, |s, p| s + glam::Vec2::new(p.0, p.1)) / profile.len() as f32;
        let n = if n.x * (a.0 - middle.x) + n.z * (a.1 - middle.y) < 0.0 { -n } else { n };
        out.push([at(k, -1.0, n), at(k + 1, -1.0, n), at(k + 1, 1.0, n)]);
        out.push([at(k, -1.0, n), at(k + 1, 1.0, n), at(k, 1.0, n)]);
    }
    out
}
