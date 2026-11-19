//! What the cockpits of every kind of aircraft share. Each kind's own cockpit is behind its door
//! (`kinds::cockpit`); this is what they are made of and where the eye can be in them.

pub mod file;
pub mod glass;
pub mod instruments;
pub mod print;
pub mod render;

use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};

/// A place the eye can be in a cockpit, as the mouse wheel and the + and − keys go through them:
/// the pilot's seat first, then nearer the panel, then close to each instrument.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    /// What it shows, for the status line.
    pub name: &'static str,
    /// The eye, body axes.
    pub eye: Vec3,
    /// Where it looks: radians right and up of the nose.
    pub yaw: f32,
    pub pitch: f32,
    /// The view's height, radians, and how far its middle is below where it looks (a fraction of
    /// half the screen: the nose that far above the middle).
    pub fov: f32,
    pub shift: f32,
}

impl Spot {
    /// The pilot's own view: from `eye` along the nose.
    pub fn seat(name: &'static str, eye: Vec3, fov_degrees: f32, shift: f32) -> Spot {
        Spot { name, eye, yaw: 0.0, pitch: 0.0, fov: fov_degrees.to_radians(), shift }
    }

    /// From `eye` looking at `at`, `height` metres of it filling the view's height there.
    pub fn looking_at(name: &'static str, eye: Vec3, at: Vec3, height: f32) -> Spot {
        let d = at - eye;
        let yaw = d.y.atan2(d.x);
        let pitch = (-d.z).atan2(d.x.hypot(d.y));
        Spot { name, eye, yaw, pitch, fov: 2.0 * (height / 2.0 / d.length()).atan(), shift: 0.0 }
    }
}

use crate::render::{to_render, ViewPoint};
use engine::egui::{pos2, Pos2, Rect, Vec2};
use engine::MeshBuilder;
use sim_core::{act, AfcsKey, Vehicle, V3};

pub use crate::ui::panel::Lever;

/// Where the cockpit's controls are, as the pilot moves them.
#[derive(Clone, Copy, Debug, Default)]
pub struct CockpitControls {
    /// -1 to 1: right roll, nose up (yoke or stick back), right pedal.
    pub aileron: f32,
    pub elevator: f32,
    pub rudder: f32,
    /// 0 idle to 1 full.
    pub throttle: f32,
    /// The flap lever: 0 full flaps (at the bottom) to 1 up.
    pub flap_lever: f32,
    /// -1 full nose down to 1 full nose up.
    pub trim: f32,
    /// The DIMMING knobs, 0 off to 1 full: the panel's lights (and the displays at night), the
    /// standby instruments'.
    pub panel_lights: f32,
    pub standby_lights: f32,
    /// How far each knob of the panel is drawn turned, in clicks clockwise (`KnobTurns::shown`:
    /// one for each `PanelKnob`, in its order).
    pub knobs: [f32; PanelKnob::COUNT],
    /// Whether the yokes or the stick are drawn (a setting: hidden, the panel behind them shows).
    pub yokes: bool,
    /// A transport's levers: the speed brakes' (0 down to 1 up) and the gear's (0 up, 1 down).
    pub spoilers: f32,
    pub gear: f32,
}

/// The yokes, levers and pedals as the player's aircraft has them, the panel's lights at half.
pub fn controls(plane: &Vehicle) -> CockpitControls {
    // A multirotor's, for the one that has a cockpit: its sticks and its collective.
    if let Some(m) = plane.multirotor() {
        let c = &m.controls;
        return CockpitControls { aileron: c.roll as f32, elevator: c.pitch as f32, rudder: c.yaw as f32, throttle: c.throttle as f32, ..CockpitControls::default() };
    }
    let Some(aircraft) = plane.aircraft() else { return CockpitControls::default() };
    let c = &aircraft.controls;
    let most = aircraft.p.max_flap_notch().max(1);
    CockpitControls {
        aileron: c.aileron as f32,
        elevator: c.elevator as f32,
        rudder: c.rudder as f32,
        throttle: c.throttle as f32,
        flap_lever: 1.0 - c.flap_notch as f32 / most as f32,
        trim: c.trim as f32,
        panel_lights: 0.6,
        standby_lights: 0.6,
        knobs: [0.0; PanelKnob::COUNT],
        yokes: true,
        spoilers: c.spoilers as f32,
        gear: if c.gear_up { 0.0 } else { 1.0 },
    }
}

/// What moves a part of a cockpit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Control {
    Throttle,
    FlapLever,
    Trim,
    Rudder,
    PanelLights,
    StandbyLights,
    /// A knob of the panel the mouse and the keys turn (`Panel::Knob`).
    Knob(PanelKnob),
    /// A transport's speed brake lever and its gear lever.
    Spoilers,
    Gear,
}

impl Control {
    /// Its value: throttle and flap lever 0 to 1; trim and rudder -1 to 1; a dimming knob from its
    /// middle, -0.5 to 0.5; a knob of the panel its clicks clockwise, so its `Turn`'s `angle` is
    /// that of a click.
    fn of(self, c: &CockpitControls) -> f32 {
        match self {
            Control::Throttle => c.throttle,
            Control::FlapLever => c.flap_lever,
            Control::Trim => c.trim,
            Control::Rudder => c.rudder,
            Control::PanelLights => c.panel_lights - 0.5,
            Control::StandbyLights => c.standby_lights - 0.5,
            Control::Knob(knob) => c.knobs[knob as usize],
            Control::Spoilers => c.spoilers,
            Control::Gear => c.gear,
        }
    }
}

/// How a part of a cockpit moves with the controls, about its `hinge` and `axis`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Motion {
    /// Lining, panel, seats: everything that does not move.
    #[default]
    Fixed,
    /// A yoke: turns about its shaft (`axis`) with the ailerons, `turn` radians at full, and slides
    /// back along it with the elevator, `travel` metres at full.
    Yoke { turn: f32, travel: f32 },
    /// A stick: leans about its foot (`hinge`), back with the elevator and right with the
    /// ailerons, `lean` radians at full.
    Stick { lean: f32 },
    /// Slides `along` (metres at a value of 1) with a control.
    Slide { by: Control, along: Vec3 },
    /// Turns about `axis`, `angle` radians at a value of 1 (a knob of the panel: a click).
    Turn { by: Control, angle: f32 },
}

/// A range of the cockpit's vertices that moves as one piece.
#[derive(Clone, Copy, Debug)]
pub struct CabinPart {
    pub motion: Motion,
    pub first: u32,
    pub count: u32,
    pub hinge: Vec3,
    pub axis: Vec3,
}

impl CabinPart {
    /// Where the part is drawn: `body` moved as the controls say.
    pub fn matrix(&self, body: Mat4, c: &CockpitControls) -> Mat4 {
        let about = |axis: Vec3, angle: f32| Mat4::from_translation(self.hinge) * Mat4::from_axis_angle(axis, angle) * Mat4::from_translation(-self.hinge);
        match self.motion {
            Motion::Fixed => body,
            Motion::Yoke { turn, travel } => body * Mat4::from_translation(-self.axis * (c.elevator * travel)) * about(self.axis, c.aileron * turn),
            Motion::Stick { lean } => body * about(Vec3::Y, c.elevator * lean) * about(Vec3::X, c.aileron * lean),
            Motion::Slide { by, along } => body * Mat4::from_translation(along * by.of(c)),
            Motion::Turn { by, angle } => body * about(self.axis, by.of(c) * angle),
        }
    }

    /// Whether it is a yoke or a stick: what the setting can hide.
    pub fn is_yoke(&self) -> bool {
        matches!(self.motion, Motion::Yoke { .. } | Motion::Stick { .. })
    }
}

/// What a surface of a cockpit is made of: the texture and the light the cockpit's shader gives it.
/// The integer part of a cabin's vertex colour's alpha; the fraction is its gloss times 0.9
/// (`file::CockpitFile::spec` makes them from a file's materials).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Material {
    /// Only its colour.
    Plain = 0,
    Plastic = 1,
    Leather = 2,
    Fabric = 3,
    Carpet = 4,
    Rubber = 5,
    Metal = 6,
    /// A picture from the instruments' canvas: red and green are where in it, blue 0 for a display
    /// that shines by itself, 1 for a dial lit like any surface.
    Screen = 7,
    /// Shines with its own colour: lamps, lit legends.
    Lamp = 8,
    /// Plastic with what the cockpit's print canvas has where it faces aft (the panel, bezels,
    /// keys) or up (the plates on the floor); its relief is the panel's crinkle paint.
    Printed = 9,
    /// Textured trim: moulded panels, covers, the frames round the windows (the leather's grain, fine).
    Vinyl = 10,
    /// Polished metal: it mirrors the cabin and the sky in its own colour.
    Chrome = 11,
    /// A material with a picture of its own (a glTF material with a base colour texture): red and
    /// green are its texture coordinates, blue which of the cabin's pictures (`CabinSpec::pictures`).
    Textured = 12,
}

/// A picture of a cabin's textured material: its colour and, if it has one, its normal map.
pub struct Picture {
    pub colour: engine::gltf::Image,
    pub normal: Option<engine::gltf::Image>,
}

/// The material and gloss of a vertex colour.
#[cfg(test)]
pub fn material_of(color: glam::Vec4) -> (u8, f32) {
    (color.w.floor() as u8, color.w.fract() / 0.9)
}

/// A cockpit in body axes, as `models::AircraftModel` is the aircraft.
pub struct CabinModel {
    pub mesh: MeshBuilder,
    pub parts: Vec<CabinPart>,
}

impl CabinModel {
    pub fn new() -> CabinModel {
        CabinModel { mesh: MeshBuilder::new(), parts: Vec::new() }
    }
}

/// A part of what is printed in a cockpit: what it covers, from `min` over `size` metres (on the
/// panel: y and z; on the floor: y and x, forward up), painted at `top` points down the print's
/// canvas, `points` wide.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Print {
    pub min: (f32, f32),
    pub size: (f32, f32),
    pub top: f32,
    pub points: f32,
}

/// Everything a kind gives the cockpit's renderer (`render::CockpitRenderer`): the cockpit's mesh
/// and parts, its glass, the box the light through its windows is baked in, the size of its
/// instruments' canvas and what is printed in it.
pub struct CabinSpec {
    pub model: CabinModel,
    /// The pictures of its textured materials (`Material::Textured`), in the order its vertices
    /// name them; all the colours of one size and all the normal maps of one size.
    pub pictures: Vec<Picture>,
    pub glass: MeshBuilder,
    pub sky_box: (Vec3, Vec3),
    pub canvas_points: (f32, f32),
    /// The print's canvas in points, its panel's part (laid along x on what faces aft, ahead of
    /// `panel_x`) and its floor's (along z on what faces up), and what paints it.
    pub print_points: (f32, f32),
    pub panel_print: Print,
    pub floor_print: Print,
    pub panel_x: f32,
    pub print: Vec<print::Shape>,
    /// The knobs a mouse can take: each lever's two ends in body axes, at its values 0 and 1 (as
    /// `Session::set_lever` takes them: flaps 1 is up, trim 1 is full nose down).
    pub levers: Vec<(Lever, [Vec3; 2])>,
    /// The keys and knobs the mouse can press and turn (`panel_at`).
    pub controls: Vec<PanelControl>,
}

/// A lever's value now, 0 to 1, as `CabinSpec::levers` has it.
fn value(lever: Lever, c: &CockpitControls) -> f32 {
    match lever {
        Lever::Throttle => c.throttle,
        Lever::Flaps => c.flap_lever,
        Lever::Trim => 0.5 - c.trim / 2.0,
    }
}

/// Where a point of the aircraft (body axes) is on the screen, in points; `None` behind the eye.
pub fn on_screen(view: &ViewPoint, screen: Rect, plane: &Vehicle, body: Vec3) -> Option<Pos2> {
    let s = plane.body();
    let world = s.pos + act(s.q, V3::new(body.x as f64, body.y as f64, body.z as f64));
    let rel = to_render(world - view.position);
    let depth = rel.dot(view.forward);
    if depth < 0.02 {
        return None;
    }
    let f = 1.0 / (view.fov_y / 2.0).tan();
    let aspect = screen.width() / screen.height().max(1.0);
    let x = f / aspect * rel.dot(view.right) / depth;
    let y = f * rel.dot(view.up) / depth + view.shift_y;
    Some(pos2(screen.left() + (x + 1.0) / 2.0 * screen.width(), screen.top() + (1.0 - y) / 2.0 * screen.height()))
}

/// A lever taken with the mouse: which, its value then, where the pointer was, and how far on the
/// screen its whole travel goes.
#[derive(Clone, Copy, Debug)]
pub struct Grip {
    pub lever: Lever,
    start: f32,
    at: Pos2,
    axis: Vec2,
}

impl Grip {
    /// The knob of one of `levers` under the pointer, if one is.
    pub fn take(view: &ViewPoint, screen: Rect, plane: &Vehicle, levers: &[(Lever, [Vec3; 2])], at: Pos2) -> Option<Grip> {
        let c = controls(plane);
        levers
            .iter()
            .filter_map(|&(lever, [p0, p1])| {
                let now = value(lever, &c);
                let knob = on_screen(view, screen, plane, p0.lerp(p1, now))?;
                let axis = on_screen(view, screen, plane, p1)? - on_screen(view, screen, plane, p0)?;
                let distance = (knob - at).length();
                (distance < 26.0 && axis.length() > 4.0).then_some((distance, Grip { lever, start: now, at, axis }))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, grip)| grip)
    }

    /// The lever's value with the pointer at `at`: dragged along its travel as the screen shows it.
    pub fn value(&self, at: Pos2) -> f64 {
        (self.start + (at - self.at).dot(self.axis) / self.axis.length_sq()).clamp(0.0, 1.0) as f64
    }
}

/// What a key of the panel does, or one of its knobs turned some clicks: the GFC 700's keys and
/// knobs, the BARO knob, D→. The keyboard (`controls::panel_key`), a controller's buttons (through
/// the same keys) and the mouse on the panel (`panel_at`) all come to this.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Panel {
    Key(#[serde(with = "AfcsKeyName")] AfcsKey),
    Knob(PanelKnob, i32),
    /// D→ the nearest aerodrome.
    DirectTo,
}

/// The GFC 700's keys, as the files name them (`sim_core::AfcsKey`).
#[derive(Serialize, Deserialize)]
#[serde(remote = "AfcsKey")]
enum AfcsKeyName {
    Ap,
    Fd,
    Hdg,
    Alt,
    Nav,
    Vnv,
    Apr,
    Bc,
    Vs,
    Flc,
    NoseUp,
    NoseDown,
    Disconnect,
    HeadingSync,
}

/// A knob of the panel. Clicks clockwise are positive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanelKnob {
    /// The heading bug, a degree a click.
    Heading,
    /// The selected altitude: the inner knob 100 ft a click, the outer 1000 ft.
    AltitudeFine,
    AltitudeCoarse,
    /// The altimeter setting, a hectopascal a click.
    Baro,
    /// The course: for a VOR, not fitted yet.
    Course,
}

impl PanelKnob {
    /// How many there are.
    pub const COUNT: usize = 5;
}

/// Seconds the drawing of a knob takes to go two thirds of the way to where it was turned.
const KNOB_LAG: f64 = 0.05;
/// Seconds a knob turned stays named on the screen (`KnobTurns::turning`).
const KNOB_NAMED: f64 = 1.5;

/// How far the panel's knobs have been turned, to draw them turning (`Control::Knob`): each one's
/// clicks, clockwise positive, where its drawing is on its way there, and the last one turned.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KnobTurns {
    turned: [f64; PanelKnob::COUNT],
    shown: [f64; PanelKnob::COUNT],
    last: Option<(PanelKnob, f64)>,
}

impl KnobTurns {
    /// A knob turned some clicks, by the mouse, a key or a controller.
    pub fn turn(&mut self, knob: PanelKnob, clicks: i32) {
        self.turned[knob as usize] += clicks as f64;
        self.last = Some((knob, KNOB_NAMED));
    }

    /// `dt` seconds on: each drawing goes `1 − e^(−dt/KNOB_LAG)` of the way to where its knob was
    /// turned, the same at any frame rate.
    pub fn advance(&mut self, dt: f64) {
        let k = 1.0 - (-dt.max(0.0) / KNOB_LAG).exp();
        for (shown, turned) in self.shown.iter_mut().zip(self.turned) {
            *shown += (turned - *shown) * k;
        }
        self.last = self.last.map(|(knob, left)| (knob, left - dt)).filter(|(_, left)| *left > 0.0);
    }

    /// Each knob as it is drawn now, in clicks (`CockpitControls::knobs`).
    pub fn shown(&self) -> [f32; PanelKnob::COUNT] {
        self.shown.map(|clicks| clicks as f32)
    }

    /// The knob turned in the last second and a half, to name it and what it is set to.
    pub fn turning(&self) -> Option<PanelKnob> {
        self.last.map(|(knob, _)| knob)
    }
}

/// A key or a knob of a cockpit's panel the mouse presses or turns: the middle of its face (body
/// axes; it faces aft, as the panel does) and its shape there.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PanelControl {
    pub centre: Vec3,
    pub shape: PanelShape,
}

/// The shape of a `PanelControl` on its face, and what it does.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PanelShape {
    /// A key: its half width and half height (y, z).
    Key { half: glam::Vec2, does: Panel },
    /// A knob of this radius, turned by dragging it or by the wheel over it; within the radius of
    /// `inner`, the inner knob of a dual one. (`panel_at` says which side of it the pointer is on.)
    Knob { radius: f32, outer: PanelKnob, inner: Option<(f32, PanelKnob)> },
}

/// Where the pointer at `at` looks, in body axes: the eye and a direction.
fn pointer_ray(view: &ViewPoint, screen: Rect, plane: &Vehicle, at: Pos2) -> (V3, V3) {
    let f = 1.0 / (view.fov_y / 2.0).tan();
    let aspect = screen.width() / screen.height().max(1.0);
    let x = (at.x - screen.left()) / screen.width() * 2.0 - 1.0;
    let y = 1.0 - (at.y - screen.top()) / screen.height() * 2.0;
    let r = view.forward + view.right * (x * aspect / f) + view.up * ((y - view.shift_y) / f);
    // Render axes (east, up, south) back to the world's (north, east, down).
    let world = V3::new(-r.z as f64, r.x as f64, -r.y as f64);
    let s = plane.body();
    let back = sim_core::inverse(s.q);
    (act(back, view.position - s.pos), act(back, world))
}

impl PanelControl {
    /// The edge of what a click that `does` this turns or presses, on its face: a key's corners;
    /// round a knob, its inner one's edge or its own.
    pub fn outline(&self, does: Panel) -> Vec<Vec3> {
        match self.shape {
            PanelShape::Key { half, .. } => [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].iter().map(|(y, z)| self.centre + Vec3::new(0.0, y * half.x, z * half.y)).collect(),
            PanelShape::Knob { radius, inner, .. } => {
                let radius = match (inner, does) {
                    (Some((r, knob)), Panel::Knob(turned, _)) if knob == turned => r,
                    _ => radius,
                };
                (0..32).map(|k| k as f32 / 32.0 * std::f32::consts::TAU).map(|a| self.centre + Vec3::new(0.0, a.cos(), a.sin()) * radius).collect()
            }
        }
    }

    /// The edge of all of it: a key's corners, a knob's outer edge.
    pub fn whole(&self) -> Vec<Vec3> {
        self.outline(Panel::DirectTo)
    }

    /// Whether it is the knob (a dual one's inner or outer) that `knob` names.
    pub fn has(&self, knob: PanelKnob) -> bool {
        matches!(self.shape, PanelShape::Knob { outer, inner, .. } if outer == knob || inner.is_some_and(|(_, k)| k == knob))
    }
}

/// How far beyond its edge a key and a knob still take the pointer, metres: the keys are 3.4 mm
/// apart, and a knob is less than 2 cm across, a few points of the screen from the seat.
const KEY_REACH: f32 = 0.0018;
const KNOB_REACH: f32 = 0.007;

/// What of the panel the pointer is over (`panel_at`): the key or knob, and what a click there does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Over {
    pub control: PanelControl,
    pub does: Panel,
}

/// What the pointer at `at` is over of `controls`, and what a click there does: the key's press, or
/// a knob's click to the side it is on. A key and a knob take it a little beyond their edges too
/// (`KEY_REACH`, `KNOB_REACH`); where two would, the one whose edge is nearer.
pub fn panel_at(view: &ViewPoint, screen: Rect, plane: &Vehicle, controls: &[PanelControl], at: Pos2) -> Option<Over> {
    let (eye, dir) = pointer_ray(view, screen, plane, at);
    controls
        .iter()
        .filter_map(|c| {
            // Where the ray meets the plane of the control's face.
            let t = (c.centre.x as f64 - eye.x) / dir.x;
            if t.is_nan() || t <= 0.0 {
                return None;
            }
            let (dy, dz) = ((eye.y + dir.y * t) as f32 - c.centre.y, (eye.z + dir.z * t) as f32 - c.centre.z);
            // How far outside its edge the pointer is, as a share of its reach, and what it does there.
            let (outside, does) = match c.shape {
                PanelShape::Key { half, does } => ((dy.abs() - half.x).max(dz.abs() - half.y).max(0.0) / KEY_REACH, does),
                PanelShape::Knob { radius, outer, inner } => {
                    let off = dy.hypot(dz);
                    let side = if dy < 0.0 { -1 } else { 1 };
                    let knob = match inner {
                        Some((r, knob)) if off < r => knob,
                        _ => outer,
                    };
                    ((off - radius).max(0.0) / KNOB_REACH, Panel::Knob(knob, side))
                }
            };
            (outside < 1.0).then_some((outside, Over { control: *c, does }))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, over)| over)
}

/// Points of the screen a knob is dragged for a click, slowly; and the speed (points a second) at
/// which a drag counts double, up to `DRAG_FASTEST` times: a slow drag sets a degree, a quick one
/// goes round.
const DRAG_POINTS: f64 = 6.0;
const DRAG_FAST: f64 = 500.0;
const DRAG_FASTEST: f64 = 3.0;

/// The clicks (and parts of one) a knob held with the mouse turns when the pointer moves `right`
/// and `up` points in `dt` seconds: up or right clockwise, down or left anticlockwise, a click
/// every `DRAG_POINTS`, more the faster it moves.
pub fn drag_clicks(right: f32, up: f32, dt: f64) -> f64 {
    let moved = (right + up) as f64;
    let speed = moved.abs() / dt.max(1e-3);
    moved / DRAG_POINTS * (1.0 + (speed / DRAG_FAST).powi(2)).min(DRAG_FASTEST)
}

/// The knob the mouse wheel turns over `over`: the knob itself; with Shift held over the HDG knob
/// or one of the AFCS keys, the selected altitude's inner one (the ALT knob is at the bottom of the
/// bezel: beside the yoke from the seat, below the picture leaning in).
pub fn wheel_knob(over: Option<Panel>, shift: bool) -> Option<PanelKnob> {
    match over? {
        Panel::Knob(PanelKnob::Heading, _) | Panel::Key(_) if shift => Some(PanelKnob::AltitudeFine),
        Panel::Knob(knob, _) => Some(knob),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A knob's drawing follows its clicks smoothly, at any frame rate, and the knob is named for a
    /// while after it was turned.
    #[test]
    fn a_knob_s_drawing_follows_its_clicks() {
        let mut fast = KnobTurns::default();
        assert_eq!(fast.shown(), [0.0; PanelKnob::COUNT]);
        assert_eq!(fast.turning(), None);
        fast.turn(PanelKnob::Heading, 3);
        fast.turn(PanelKnob::AltitudeFine, -2);
        let mut slow = fast;
        assert_eq!(fast.shown(), [0.0; PanelKnob::COUNT], "not there at once");
        for _ in 0..12 {
            fast.advance(1.0 / 240.0);
        }
        for _ in 0..3 {
            slow.advance(1.0 / 60.0);
        }
        let (heading, fine) = (fast.shown()[PanelKnob::Heading as usize], fast.shown()[PanelKnob::AltitudeFine as usize]);
        assert!(heading > 1.5 && heading < 2.5, "{heading}");
        assert!((fine + heading * 2.0 / 3.0).abs() < 1e-5, "{fine}");
        assert!((slow.shown()[PanelKnob::Heading as usize] - heading).abs() < 1e-5);
        assert_eq!(fast.shown()[PanelKnob::Baro as usize], 0.0);
        assert_eq!(fast.turning(), Some(PanelKnob::AltitudeFine));
        for _ in 0..60 {
            fast.advance(1.0 / 60.0);
        }
        assert!((fast.shown()[PanelKnob::Heading as usize] - 3.0).abs() < 1e-3 && (fast.shown()[PanelKnob::AltitudeFine as usize] + 2.0).abs() < 1e-3);
        assert_eq!(fast.turning(), Some(PanelKnob::AltitudeFine));
        for _ in 0..40 {
            fast.advance(1.0 / 60.0);
        }
        assert_eq!(fast.turning(), None);
    }

    /// A part turned by a knob of the panel turns a click's angle for each click, clockwise seen
    /// from the seat: its top goes right.
    #[test]
    fn a_knob_s_part_turns_with_its_clicks() {
        let hinge = Vec3::new(0.6, -0.4, -0.3);
        let part = CabinPart { motion: Motion::Turn { by: Control::Knob(PanelKnob::Heading), angle: 0.25 }, first: 0, count: 3, hinge, axis: Vec3::X };
        let mut c = CockpitControls::default();
        let top = hinge + Vec3::new(-0.01, 0.0, -0.008);
        assert!((part.matrix(Mat4::IDENTITY, &c).transform_point3(top) - top).length() < 1e-7);
        c.knobs[PanelKnob::Heading as usize] = 2.0;
        let turned = part.matrix(Mat4::IDENTITY, &c).transform_point3(top);
        let (dy, dz) = (turned.y - hinge.y, turned.z - hinge.z);
        assert!((dy - 0.008 * 0.5f32.sin()).abs() < 1e-6 && (dz + 0.008 * 0.5f32.cos()).abs() < 1e-6 && (turned.x - top.x).abs() < 1e-7, "{turned}");
        c.knobs = [0.0; PanelKnob::COUNT];
        c.knobs[PanelKnob::Baro as usize] = 2.0;
        assert!((part.matrix(Mat4::IDENTITY, &c).transform_point3(top) - top).length() < 1e-7, "another knob's clicks");
    }

    #[test]
    fn the_wheel_turns_the_knob_under_it_and_with_shift_the_altitude() {
        use PanelKnob::*;
        assert_eq!(wheel_knob(None, false), None);
        assert_eq!(wheel_knob(None, true), None);
        for knob in [Heading, AltitudeFine, AltitudeCoarse, Baro, Course] {
            assert_eq!(wheel_knob(Some(Panel::Knob(knob, 1)), false), Some(knob));
        }
        assert_eq!(wheel_knob(Some(Panel::Knob(Heading, -1)), true), Some(AltitudeFine));
        assert_eq!(wheel_knob(Some(Panel::Knob(AltitudeCoarse, 1)), true), Some(AltitudeCoarse));
        assert_eq!(wheel_knob(Some(Panel::Knob(Baro, 1)), true), Some(Baro));
        assert_eq!(wheel_knob(Some(Panel::Key(AfcsKey::Hdg)), true), Some(AltitudeFine));
        assert_eq!(wheel_knob(Some(Panel::Key(AfcsKey::Hdg)), false), None, "over a key the wheel is the view's");
        assert_eq!(wheel_knob(Some(Panel::DirectTo), true), None);
    }

    /// Dragged slowly a knob turns a click every six points, up or right clockwise; quickly, up
    /// to three times as many.
    #[test]
    fn a_knob_dragged_turns_with_the_pointer_and_faster_with_its_speed() {
        let frame = 1.0 / 60.0;
        // 60 points a second: a click every six points, either way, along either axis.
        assert!((drag_clicks(1.0, 0.0, frame) - 1.0 / 6.0).abs() < 0.01);
        assert!((drag_clicks(0.0, 1.0, frame) - 1.0 / 6.0).abs() < 0.01);
        assert!((drag_clicks(-1.0, 0.0, frame) + 1.0 / 6.0).abs() < 0.01);
        assert!((drag_clicks(0.0, -1.0, frame) + 1.0 / 6.0).abs() < 0.01);
        assert_eq!(drag_clicks(0.0, 0.0, frame), 0.0);
        assert_eq!(drag_clicks(3.0, -3.0, frame), 0.0, "along the other diagonal nothing turns");
        // 500 points a second: twice; much faster, three times and no more.
        let fast = drag_clicks(500.0 * frame as f32, 0.0, frame);
        assert!((fast / (500.0 * frame / 6.0) - 2.0).abs() < 1e-3, "{fast}");
        assert!((drag_clicks(-40.0, 0.0, frame) + 20.0).abs() < 1e-9 && (drag_clicks(0.0, 80.0, frame) - 40.0).abs() < 1e-9);
        // The same drag in frames half as long turns the same.
        let (one, two) = (drag_clicks(4.0, 0.0, frame), 2.0 * drag_clicks(2.0, 0.0, frame / 2.0));
        assert!((one - two).abs() < 1e-9);
    }

    /// The edge of a key is its four corners; of a dual knob, the inner one's or the outer's.
    #[test]
    fn a_control_s_outline_is_its_edge() {
        let centre = Vec3::new(0.59, -0.4, -0.2);
        let key = PanelControl { centre, shape: PanelShape::Key { half: glam::Vec2::new(0.008, 0.005), does: Panel::DirectTo } };
        let corners = key.outline(Panel::DirectTo);
        assert_eq!(corners.len(), 4);
        assert!(corners.iter().all(|p| p.x == centre.x && ((p.y - centre.y).abs() - 0.008).abs() < 1e-6 && ((p.z - centre.z).abs() - 0.005).abs() < 1e-6));
        let knob = PanelControl { centre, shape: PanelShape::Knob { radius: 0.009, outer: PanelKnob::AltitudeCoarse, inner: Some((0.0056, PanelKnob::AltitudeFine)) } };
        for (does, radius) in [(Panel::Knob(PanelKnob::AltitudeCoarse, 1), 0.009), (Panel::Knob(PanelKnob::AltitudeFine, -1), 0.0056)] {
            assert!(knob.outline(does).iter().all(|p| ((*p - centre).length() - radius).abs() < 1e-6 && p.x == centre.x));
        }
        assert!(knob.has(PanelKnob::AltitudeFine) && knob.has(PanelKnob::AltitudeCoarse) && !knob.has(PanelKnob::Heading) && !key.has(PanelKnob::Heading));
    }
}
