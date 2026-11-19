//! Where the picture is taken from: behind the car (the chase camera, which lags as the car turns
//! and brakes), on its bonnet, from the driver's seat, or from the trackside cameras of the circuit.

use glam::DVec3;
use sim_core::circuit::Circuit;
use sim_core::{act, Car};
use world_render::{to_render, ViewPoint};

/// The views, in the order the camera key goes through them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Chase,
    Bonnet,
    Cockpit,
    Trackside,
}

impl View {
    pub const ALL: [View; 4] = [View::Chase, View::Bonnet, View::Cockpit, View::Trackside];

    pub fn parse(word: &str) -> Option<View> {
        match word {
            "chase" => Some(View::Chase),
            "bonnet" | "hood" => Some(View::Bonnet),
            "cockpit" | "inside" => Some(View::Cockpit),
            "trackside" | "tv" => Some(View::Trackside),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            View::Chase => "chase",
            View::Bonnet => "bonnet",
            View::Cockpit => "cockpit",
            View::Trackside => "trackside",
        }
    }

    pub fn next(self) -> View {
        let i = View::ALL.iter().position(|&v| v == self).unwrap_or(0);
        View::ALL[(i + 1) % View::ALL.len()]
    }

    /// Whether the eye is inside the car (its cabin drawn, its glass see-through).
    pub fn inside(self) -> bool {
        self == View::Cockpit
    }
}

/// The cameras' state: where the chase camera is and how fast it moves.
pub struct Cameras {
    pub view: View,
    chase: Option<(DVec3, DVec3)>,
    /// The chase camera turned round the car (radians, left and right) and up, by the player.
    pub orbit: (f64, f64),
    /// How far behind the car the chase camera sits, m.
    pub distance: f64,
    /// Seconds of driving seen, for what shakes.
    time: f64,
    /// How far the trackside camera's lens is focused, m (on the car).
    pub focus: f64,
    /// The chase camera's look into the corner, radians, smoothed.
    lead: f64,
}

/// How far behind the car the chase camera sits, and how high above it.
const CHASE_BACK: f64 = 6.2;
const CHASE_UP: f64 = 1.75;

impl Cameras {
    pub fn new(view: View) -> Cameras {
        Cameras { view, chase: None, orbit: (0.0, 0.0), distance: CHASE_BACK, time: 0.0, focus: 0.0, lead: 0.0 }
    }

    /// Forgets where the chase camera was (after a restart): it starts behind the car again.
    pub fn reset(&mut self) {
        self.chase = None;
    }

    /// The view now, `dt` seconds after the last.
    pub fn view_point(&mut self, car: &Car, circuit: &Circuit, dt: f64) -> ViewPoint {
        let b = car.body;
        let up_axis = act(b.q, DVec3::NEG_Z);
        // The car's own velocity: the step between two frames is not (the world steps at its own
        // rate, the frames at theirs), and a speed taken from it jumps from frame to frame.
        let moving = b.world_velocity();
        self.time += dt;
        let speed = moving.length();
        // The road coming up through the car: a tremble on the kerbs and off the asphalt (the
        // wheels say what they are on), none on smooth asphalt.
        let rough = car.wheels().iter().filter(|w| w.contact).map(|w| match w.surface {
            sim_core::car::Surface::Asphalt | sim_core::car::Surface::Concrete => 0.0,
            sim_core::car::Surface::Kerb => 1.0,
            _ => 1.6,
        }).fold(0.0, f64::max);
        let shake = 0.012 * rough * (speed / 30.0).min(1.0) * if dt > 0.0 { 1.0 } else { 0.0 };
        let wobble = |k: f64| (self.time * (31.0 + 7.0 * k)).sin() * 0.6 + (self.time * (53.0 + 11.0 * k)).sin() * 0.4;
        let tremble = (act(b.q, DVec3::Y) * wobble(0.0) + up_axis * wobble(1.0)) * shake;
        match self.view {
            View::Chase => {
                // Behind the way the car points, levelled; the camera pulled after that point by a
                // critically damped spring, so it swings out in a turn and closes up under braking.
                let forward = act(b.q, DVec3::X);
                let level = DVec3::new(forward.x, forward.y, 0.0).normalize_or(DVec3::X);
                let (yaw, pitch) = self.orbit;
                let (c, s) = (yaw.cos(), yaw.sin());
                let back = DVec3::new(level.x * c - level.y * s, level.x * s + level.y * c, 0.0);
                let up = CHASE_UP * (self.distance / CHASE_BACK).min(1.0);
                let target = b.pos - back * self.distance * pitch.cos() + DVec3::new(0.0, 0.0, -(up + self.distance * pitch.sin()));
                let (mut pos, mut vel) = self.chase.unwrap_or((target, moving));
                let w = 7.0;
                let steps = ((dt / 0.004).ceil() as usize).clamp(1, 50);
                let h = dt / steps as f64;
                for _ in 0..steps {
                    let accel = (target - pos) * w * w - (vel - moving) * 2.0 * w;
                    vel += accel * h;
                    pos += vel * h;
                }
                // Never under the road.
                pos.z = pos.z.min(b.pos.z - 0.6);
                self.chase = Some((pos, vel));
                // A little ahead of the car when behind it; at the car when turned round it.
                // It looks a little ahead of the car and into the corner (by how fast the car
                // turns); its lens stays as it is (one that opened with speed read as a zoom).
                let turn = (b.omega.z * 0.22).clamp(-0.16, 0.16);
                self.lead += (turn - self.lead) * (1.0 - (-dt * 2.5).exp());
                let side = DVec3::new(-level.y, level.x, 0.0);
                let ahead = yaw.cos().max(0.0);
                let look_at = b.pos + (level * 2.5 + side * self.lead * 14.0) * ahead + DVec3::new(0.0, 0.0, -0.75);
                looking(pos + tremble * 1.5, look_at - pos, 55.0, 0.25)
            }
            View::Bonnet => {
                let (x, y, z) = car.p.visual.bonnet_eye;
                let eye = b.pos + act(b.q, DVec3::new(x, y, z)) + tremble * 2.0;
                along_body(car, eye, 0.0, 64.0, 0.1)
            }
            View::Cockpit => {
                let (x, y, z) = car.p.visual.driver_eye;
                let eye = b.pos + act(b.q, DVec3::new(x, y, z));
                // The head turned a little into the corner, as a driver looks to the apex.
                let into = car.controls.steer * 0.18;
                along_body(car, eye + tremble, into, 62.0, 0.03)
            }
            View::Trackside => {
                // The nearest of the cameras round the circuit, every 300 m, 18 m out on the
                // outside and 5 m up, its lens closing in on the car as it goes away.
                let along = circuit.locate(b.pos.x, b.pos.y).map_or(0.0, |a| a.along);
                let spacing = 300.0;
                let k = (along / spacing).round();
                let at = k * spacing;
                let sample = circuit.sample(at);
                let side = if sample.curvature > 0.0 { -1.0 } else { 1.0 };
                // One in three stands low by the kerb, one on a tower behind the run-off, one high
                // and far back; each follows the car with a long lens, a little ahead of it.
                let ss = &sample.sides[if side < 0.0 { 0 } else { 1 }];
                let (out, up) = match (k as i64).rem_euclid(3) {
                    0 => (sample.width / 2.0 + ss.runoff_width.min(4.0) + 2.5, 1.3),
                    1 => (sample.width / 2.0 + ss.runoff_width + 6.0, 5.0),
                    _ => (sample.width / 2.0 + ss.runoff_width + 14.0, 9.0),
                };
                let (n, e, h) = circuit.point(at, side * out);
                let eye = DVec3::new(n, e, -(h + up));
                let to = b.pos + up_axis * 0.45 + moving * 0.06 - eye;
                let fov = (2.0 * (3.4 / to.length().max(1.0)).atan()).to_degrees().clamp(3.5, 50.0);
                // A hand on the camera: it never holds quite still.
                let sway = (DVec3::Y * (self.time * 1.7).sin() + DVec3::Z * (self.time * 2.3).cos()) * to.length() * 0.0012;
                self.focus = to.length();
                looking(eye, to + sway, fov, 0.5)
            }
        }
    }
}

/// A view from `eye` towards `direction` (world axes), level, with a vertical field of `fov` degrees.
fn looking(eye: DVec3, direction: DVec3, fov: f64, near: f32) -> ViewPoint {
    let f = direction.normalize_or(DVec3::X);
    let right = DVec3::Z.cross(f).normalize_or(DVec3::Y);
    let up = f.cross(right);
    ViewPoint { position: eye, forward: to_render(f), up: to_render(-up), right: to_render(right), fov_y: (fov as f32).to_radians(), shift_y: 0.0, near }
}

/// A view along the car's body from `eye`, turned `yaw` radians to the right.
fn along_body(car: &Car, eye: DVec3, yaw: f64, fov: f64, near: f32) -> ViewPoint {
    let q = car.body.q;
    let f = act(q, DVec3::new(yaw.cos(), yaw.sin(), 0.0));
    let right = act(q, DVec3::new(-yaw.sin(), yaw.cos(), 0.0));
    let down = act(q, DVec3::Z);
    ViewPoint { position: eye, forward: to_render(f), up: to_render(-down), right: to_render(right), fov_y: (fov as f32).to_radians(), shift_y: 0.0, near }
}
