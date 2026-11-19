//! The game: turns keyboard and mouse into flight controls, runs the simulation clock, places the
//! camera and writes the texts.
//!
//! [`Session`] is everything that needs no window (it is what the tests drive); [`Flight`] adds the
//! renderer, the overlay and the sound and is what the engine runs.

use crate::ai::{self, ModelFlier};
use crate::audio::Audio;
use crate::g1000::Instruments;
use crate::controls::{self, Mappings, Pads};
use crate::cockpit::{self, CockpitControls, Grip, KnobTurns, Lever, Over, Panel, PanelKnob, Spot};
use crate::kinds::{self, Craft, Family};
use crate::multirotor as drone;
use crate::render::{to_render, PlaneVisual, Preset, Renderer, Scene, ViewPoint};
use crate::settings::{self, Conditions, Store};
use crate::ui::hud::{self, Hud};
use crate::ui::loading as waiting;
use crate::ui::menu::{self, Menus, Nav, PadNav};
use crate::ui::panel::{self, PanelData};
use engine::egui::{self, pos2, Rect};
use engine::{wgpu, Game, Gpu, Input, KeyCode};
use sim_core::{act, axis_angle, clamp, euler_angles, Aircraft, AircraftControls, AircraftKind, Airport, Body, Dynamics, KeyboardPilot, Multirotor, Region, State, Vehicle, VehicleId, Start, Terrain, World, DEG, FEET, HPA, INHG, KNOTS, V3};
use std::sync::Arc;
use crate::instrument_keys;
use crate::manual::Manual;
use crate::picker::{self, Picker};
use crate::places;
use crate::race::{RaceSession, Rival};
use crate::remote::Remote;
use crate::replay::{self, Playback, Recorder};
use sim_core::{AfcsKey, AfcsKnob, Approach, Autopilot, Controls, Gfc700, Goal, Observation, Phase, Pilot, Vertical, VerticalMode, Wreck};

pub const HELP: &str = "\
F1         illustrated manual: first steps,
           views, controls, autopilot
G          keyboard assist: on / off

With assist:
↓ ↑        nose up / down, and it stays
← →        bank the wings, and they stay
Q E        nose: fine adjustment
           beyond 30° of pitch or 60° of bank,
           or holding the arrow: they turn freely
           (loop, roll, dive, spin)
Without assist:
↓ ↑ ← →    move the yoke, and it stays
Q E        trim down / up

W S        throttle (also PgUp / PgDn)
B          brakes (hold; with the throttle closed
           on the ground they brake a little alone)
F R        flaps: down / up
A D        pedals
C          centre yoke and pedals
           (or click its box)

P          autopilot: in the C172 the GFC 700
           (below); in the Extra it holds heading,
           altitude and speed (← → ↑ ↓ W S)
L          land by itself at the nearest aerodrome
GFC 700    P AP: holds this heading and altitude
(C172)     ← → heading 5° (⇧ 1°) · ↑ ↓ altitude
           100 ft (⇧ 1000): it turns and climbs there
           P or C disconnect · ⇧P FD
           4 5 6 HDG NAV APR · 7 8 9 ALT VS FLC
           , . wheel UP/DN · [ ] heading (⇧ 10°)
           \\ current heading · ; ' altitude (⇧ 1000)
           / D→ to the nearest · or click the G1000
           W S, the throttle: add it to climb
           mouse: click a key; drag a knob (up or
           right: more) or the wheel over it turns it
           (HDG, ALT, BARO); ⇧ wheel over the HDG knob
           or the keys: altitude (the ALT knob is low
           on the bezel, by the yoke)

K ⇧K       altimeter: +1 / −1 hPa
Y          altimeter to the aerodrome's QNH
J          heading indicator to the compass
U          pitot heat
⇧U         alternate static source
F5 F6      try a failure: pitot, static
F7 F8      try a failure: vacuum, icing

V          cockpit / outside
N          outside: go to the next aircraft
           (the top says which one and who flies it)
Z X        look left / right
Mouse      drag: look (the view stays; the settings turn
           it the natural way round); on the panel: move levers
0          look ahead again
           wheel: in the cockpit it zooms where you
           look, and stays; outside, camera closer /
           further
+ −        the same as the wheel
Space      view: normal ↔ close
I ⇧I       panel lights (DIMMING): more / less

Gamepad    left stick: yoke; right: pedals
           triggers: throttle; d-pad: trim
           A B: flaps; LB RB: look; Y: view
           X: centre; Start: pause
           d-pad ← →: time slower / faster
Joystick   it is the yoke, always: with it the
           keyboard assist does not act
           trigger: view; 2 3: flaps
           hat: trim (↕), look (↔)
           4: view normal → close → panel
           5 6 (base): zoom in / out
           7 8 9 10 (base): AP HDG ALT FLC
           11 12 (base): selected heading ← →

FPV quad   W S throttle stick · ↑ ↓ pitch (↑ nose
           down) · ← → roll · A D yaw
           R arm / disarm (throttle down)
           V its camera / behind it
           1 on the ground here · 2 hovering
Tilt-rotor it holds itself still: the collective
           lever (W S, a throttle lever) asks for
           a climb, its middle holds the height;
           the stick for a speed over the ground:
           let go, it stops and stays
           F G: nacelles forward / back a notch,
           to go fast: then ↑ ↓ tilts its rings
           more or less, ← → banks · A D pedals
           Q E (the trim's hat) nose down / up
           C: stability: hold / level / rates (it
           stays as it is left) / off
           Enter: the engines · V: its cockpit
Race       M: a course (Race courses) or --course
           1 2 3: race again from the pad
           L: the autopilot races / gives it back
           with AI quads (--ai, --rival): N the camera
           on the next quad (V: its camera / behind)

1          back to the runway
2          start in flight
3          start on final
M          choose place: aerodrome, map point
T ⇧T       time of day: forward / back
F9 F10     time slower / faster: ×1 ×2 ×4 ×8 ×16
           (more steps, as accurate; top right ×4;
           ×1 again on a touchdown, a crash, a pause)
O          settings, beside the flight: it goes on
           (weather, time, flight aids, graphics);
           O or Esc closes them
F3         frames per second
F12        record a video, with its sound; F12
           again saves it in Movies/Skylab
Esc        pause: the menu (fly, aircraft, place,
           weather...); Esc again flies on. First it
           closes this help, the settings, a replay

F2         replay of the last minutes:
           Space play / stop, ← → 10 s (⇧ 30 s),
           ↑ ↓ speed, F2 or Esc: back to the flight

Take off: W full, ↓ at 55 knots.
Land: 2 white lights and 2 red, S to
close the throttle and ↓ just before touchdown.";

/// Throttle travel per second while W or S is held, and trim travel per second on Q or E.
pub(crate) const THROTTLE_RATE: f64 = 0.45;
pub(crate) const TRIM_RATE: f64 = 0.12;
/// Metres in a nautical mile.
const NAUTICAL_MILE: f64 = 1852.0;

/// Where the player's aircraft starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StartAt {
    Runway,
    /// In flight over where the aircraft is.
    Air,
    /// On final approach to the nearest runway.
    Final,
    /// In flight over this point (north, east), heading 0.6 rad.
    Position(f64, f64),
}

/// How a game starts: the command line, mostly.
pub struct Options {
    pub seed: u64,
    pub start: StartAt,
    /// Start in the outside view.
    pub exterior: bool,
    pub hour: Option<f64>,
    /// The hour of the computer's clock, for a start the player sees (`main`, the web): the game
    /// starts at it if the settings say so (`settings::LOCAL_TIME`), unless `hour` says otherwise.
    /// `None` for screenshots, recordings and tests, which stay the same whenever they run.
    pub clock: Option<f64>,
    /// Time this many times faster than real from the start (`--time`; `TIME_RATES`).
    pub time_rate: u32,
    /// The kind of day (`--sky`), instead of the saved sliders.
    pub sky: Option<crate::settings::Sky>,
    /// Start with the help column open.
    pub help: bool,
    /// Aircraft flown by the autopilot around the home airport and its neighbours, 0 to 6.
    pub traffic: usize,
    /// Model files (`file` or `file:task`), each flown in an aircraft of its own.
    pub models: Vec<String>,
    /// How much the renderer draws; `None` is what the settings file says, or everything.
    pub quality: Option<Preset>,
    /// Start with frames per second in the status line.
    pub fps: bool,
    /// Start with the head turned this far right and up in the cockpit, radians, and held there.
    pub look: Option<(f64, f64)>,
    /// Start at this spot of the cockpit (`kinds::spots`), 0 the seat.
    pub spot: usize,
    /// For a screenshot: the mouse's pointer at this point of the picture (`--hover`).
    pub hover: Option<(f32, f32)>,
    /// A tilt-rotor's nacelles' lever from the start, 0 upright to 1 forward (`--nacelles`).
    pub nacelles: Option<f64>,
    /// Starting in flight: this many metres above the ground instead of 900.
    pub low: Option<f64>,
    /// For a screenshot: dive into the ground ahead and show the wreck this many seconds later.
    pub crash: Option<f64>,
    /// For a screenshot: what to break of the player's aircraft before the picture.
    pub damage: Option<Damaged>,
    /// What is flown; `None` is what the settings file says, or the C172.
    pub aircraft: Option<Craft>,
    /// A real place to fly in instead of the seed's world, built before (`--place`, `--at`, the
    /// settings), and how the settings name it (`LEMH`, or `lat,lon`).
    pub region: Option<Arc<Region>>,
    pub place: Option<String>,
    /// Open the place picker at the start, and how (the setting, or `--picker` for a screenshot).
    pub picker: Option<picker::Opening>,
    /// Open the manual at the start at this page, 0 the first (`--manual`, for a screenshot).
    pub manual: Option<usize>,
    /// Open a menu at the start (`--menu`): `vehicles`, `home`, `settings` and its tabs, `aircraft`.
    pub menu: Option<String>,
    /// Nothing over the picture but the maps' credit (`--clean`): for the menus' pictures.
    pub clean: bool,
    /// The outside camera this many lines of the mouse wheel nearer the aircraft (`--zoom`; negative: further).
    pub zoom: f64,
    /// Race this course at the start (`--course`): its place in `course::file::courses`.
    pub course: Option<usize>,
    /// The race's autopilot flies it (`--racer`).
    pub racer: bool,
    /// Quads flown by the game in every race against the player (`--rival`, `--ai`, a `gates`
    /// model given to `--model`): `racer` or a model file of the `gates` task.
    pub rivals: Vec<String>,
    /// In a race with rivals, the camera on this quad from the start (`--watch`): 0 the player's.
    pub watch: usize,
    /// Other programs can fly the game through this port of 127.0.0.1 (`--remote`, `remote.rs`).
    pub remote: Option<u16>,
}

/// What a screenshot breaks of the player's aircraft (`--damage`), by putting it where that gives way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Damaged {
    /// On the runway, a wing tip dropped onto it: the outer wing crushed and torn off.
    Tip,
    /// A touchdown too hard for the gear: the main legs collapse.
    Gear,
    /// In flight, the outer part of the left wing and half the right tailplane torn off.
    Wing,
    /// In a dive past the flutter speed, the first surfaces flutter away.
    Flutter,
    /// A quad's propellers after a touch with another quad: the front left one broken, the front
    /// right one chipped; disarmed, so that the blades stand still to be seen.
    Props,
}

impl Damaged {
    pub fn named(word: &str) -> Option<Damaged> {
        match word {
            "tip" => Some(Damaged::Tip),
            "gear" => Some(Damaged::Gear),
            "wing" => Some(Damaged::Wing),
            "flutter" => Some(Damaged::Flutter),
            "props" => Some(Damaged::Props),
            _ => None,
        }
    }
}

impl Default for Options {
    fn default() -> Options {
        Options { seed: 1, start: StartAt::Runway, exterior: false, hour: None, clock: None, time_rate: 1, sky: None, help: false, traffic: 0, quality: None, fps: false, look: None, spot: 0, hover: None, nacelles: None, low: None, crash: None, damage: None, models: Vec::new(), aircraft: None, region: None, place: None, picker: None, manual: None, menu: None, clean: false, zoom: 0.0, course: None, racer: false, rivals: Vec::new(), watch: 0, remote: None }
    }
}

/// The keys that act for as long as they are held.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Held {
    /// -1, 0 or 1 each: right arrow minus left, down minus up (pull), D minus A, E minus Q.
    pub roll: f64,
    pub pitch: f64,
    pub pedals: f64,
    pub fine: f64,
    /// W or Page Up, S or Page Down.
    pub throttle_up: bool,
    pub throttle_down: bool,
    /// X minus Z: turn the head, or swing the outside camera.
    pub look: f64,
    /// T moves the clock; with shift, backwards.
    pub time: bool,
    pub shift: bool,
    /// B: both brakes, fully.
    pub brake: bool,
}

impl Held {
    pub fn of(input: &Input) -> Held {
        Held::with(|key| input.is_down(key), input.shift)
    }

    /// The keys held, as `down` says each is (the manual's tests ask it of the keys it names).
    pub fn with(down: impl Fn(KeyCode) -> bool, shift: bool) -> Held {
        let axis = |plus, minus| (down(plus) as i32 - down(minus) as i32) as f64;
        Held {
            roll: axis(KeyCode::ArrowRight, KeyCode::ArrowLeft),
            pitch: axis(KeyCode::ArrowDown, KeyCode::ArrowUp),
            pedals: axis(KeyCode::KeyD, KeyCode::KeyA),
            fine: axis(KeyCode::KeyE, KeyCode::KeyQ),
            throttle_up: down(KeyCode::KeyW) || down(KeyCode::PageUp),
            throttle_down: down(KeyCode::KeyS) || down(KeyCode::PageDown),
            look: axis(KeyCode::KeyX, KeyCode::KeyZ),
            time: down(KeyCode::KeyT),
            shift,
            brake: down(KeyCode::KeyB),
        }
    }
}

/// Moves throttle, trim and brakes for `dt` seconds of held keys. Trim is on Q and E only without the
/// assist; with it those keys nudge the nose instead (the pilot reads `fine`).
pub fn engine_keys(c: &mut AircraftControls, held: &Held, assist: bool, dt: f64) {
    if held.throttle_up {
        c.throttle = (c.throttle + THROTTLE_RATE * dt).min(1.0);
    }
    if held.throttle_down {
        c.throttle = (c.throttle - THROTTLE_RATE * dt).max(0.0);
    }
    if !assist && held.fine < 0.0 {
        c.trim = (c.trim - TRIM_RATE * dt).max(-1.0);
    }
    if !assist && held.fine > 0.0 {
        c.trim = (c.trim + TRIM_RATE * dt).min(1.0);
    }
    let brake = if held.brake { 1.0 } else { 0.0 };
    (c.brake_left, c.brake_right) = (brake, brake);
}

/// Who flies an aircraft: the player (the keyboard, or the player's autopilot while it is
/// engaged) or an autopilot of its own, for the traffic, or a trained model (`--model`).
enum Flier {
    Player,
    Traffic(Box<Autopilot>),
    Model(Box<ModelFlier>),
    /// A quad racing against the player: the race flies it (`RaceSession::rivals`).
    Rival,
}

/// The pilot's hands (the keyboard's, or the game's autopilot's) with the GFC 700 behind them: it
/// sees every step, and while it is engaged its servos have the ailerons, elevator and trim.
struct Hands<'a> {
    hands: &'a mut dyn Pilot,
    gfc: &'a mut Gfc700,
}

impl Pilot for Hands<'_> {
    fn fly(&mut self, obs: &Observation, current: &Controls, dt: f64) -> Controls {
        let hands = self.hands.fly(obs, current, dt);
        self.gfc.fly(obs, &hands, dt)
    }
}

/// The player's autopilot moves its targets in these steps, and the airspeed at this rate
/// while W or S is held.
/// How far the outside camera starts from the aircraft, metres.
const ORBIT_DISTANCE: f64 = 17.0;
const HEADING_STEP: f64 = 5.0 * DEG;
const ALTITUDE_STEP: f64 = 100.0 * FEET;
/// The cockpit view's zoom: how much a line of the wheel magnifies, how far it goes either way of
/// a spot's own field of view, and the seconds the picture takes to go two thirds of the way to
/// where the wheel left it (the outside camera's distance too).
const ZOOM_STEP: f64 = 1.12;
const ZOOM_RANGE: (f64, f64) = (0.8, 6.0);
const ZOOM_LAG: f64 = 0.12;
/// The vertical speed the GFC 700 is given to go to an altitude selected with the arrows, m/s.
const ARROW_CLIMB: f64 = 500.0 * FEET / 60.0;
const AIRSPEED_RATE: f64 = 8.0 * KNOTS;
// The airspeeds it holds are `kinds::autopilot_speeds`.

/// How many times faster than real time the world can go (F9 slower, F10 faster).
pub const TIME_RATES: [u32; 5] = [1, 2, 4, 8, 16];
/// The wall-clock time each frame's steps beyond real time may take; past it they are let go of
/// and the world goes slower than chosen. A frame at 60 Hz is 16.7 ms, and the picture needs its share.
const STEP_BUDGET: std::time::Duration = std::time::Duration::from_millis(10);
/// Seconds the rate the world keeps up (`Session::achieved_rate`) is smoothed over.
const RATE_SMOOTHING: f64 = 0.7;

/// The most aircraft `--traffic` adds (the renderer draws eight in all).
pub const MAX_TRAFFIC: usize = 6;

/// Metres between the pads of a race's start, side by side across it: a quad's propellers reach
/// 0.14 m from its middle, and quads that take off together drift a metre or so either way.
const PAD_SPACING: f64 = 3.0;

/// Where the `k`th rival's pad is, metres to the right of the start's middle (the player's pad):
/// left, right, then further left and right.
fn rival_pad(k: usize) -> f64 {
    let side = if k.is_multiple_of(2) { -1.0 } else { 1.0 };
    side * PAD_SPACING * (k / 2 + 1) as f64
}

/// The seconds the replay keeps, as the settings ask.
fn replay_window(store: &Store) -> f64 {
    let (least, most) = replay::MINUTES_RANGE;
    store.number(replay::MINUTES_KEY).unwrap_or(replay::DEFAULT_MINUTES).clamp(least, most) * 60.0
}

/// Adds what the player flies to a world, placed at `start`.
fn add_craft(world: &mut World, craft: Craft, start: Start) -> VehicleId {
    match craft {
        Craft::Aircraft(kind) => world.add_aircraft(kind, start),
        Craft::Multirotor(kind) => world.add_multirotor(kind, start),
    }
}

/// The outside camera's heading as it follows the craft watched (`of`), behind its turns
/// (`Session::follow`), and where that craft was when it was last followed.
#[derive(Clone, Copy, Debug)]
struct Chase {
    of: VehicleId,
    heading: f64,
    at: V3,
}

/// The heading a camera behind a craft in attitude `q` looks along, radians, steady at any
/// attitude: its nose's way over the ground, and, the more it points up or down, its back's
/// (diving straight down its back faces where it is going, climbing straight up its belly does).
/// The Euler heading spins round as the nose passes the vertical; this does not. `None` only
/// upside down with the nose some 50° up, where neither tells.
fn chase_heading(q: sim_core::Quat) -> Option<f64> {
    let forward = act(q, V3::X);
    let back = act(q, V3::NEG_Z);
    // The nose's climb, the sine of its pitch.
    let climb = -forward.z;
    let along = V3::new(forward.x, forward.y, 0.0) - climb * V3::new(back.x, back.y, 0.0);
    (along.length() > 0.2).then(|| along.y.atan2(along.x))
}

/// An angle into (−π, π].
fn wrapped(angle: f64) -> f64 {
    let a = angle.rem_euclid(std::f64::consts::TAU);
    if a > std::f64::consts::PI { a - std::f64::consts::TAU } else { a }
}

/// An aircraft of the world and who flies it.
struct Flown {
    id: VehicleId,
    flier: Flier,
}

/// The game without a window: the world, the player's hands and eyes, and the texts.
pub struct Session {
    pub world: World,
    /// The aircraft the camera and the instruments follow.
    pub player: VehicleId,
    planes: Vec<Flown>,
    pub pilot: KeyboardPilot,
    /// The game controllers: see `controls.rs`.
    pub pads: Pads,
    /// Flies the player's aircraft instead of `pilot` while it is engaged (L; P in an aircraft
    /// without a GFC 700).
    pub autopilot: Option<Autopilot>,
    /// The GFC 700 of an aircraft that has one (`AircraftType::afcs`): behind the pilot's hands,
    /// or the game's autopilot's, every step (`Hands`).
    pub gfc: Option<Gfc700>,
    /// The aerodrome of the last D→, for the approach APR flies.
    destination: Option<Airport>,
    /// The aircraft the outside camera follows, as an index into `planes` (N).
    watched: usize,
    pub store: Store,
    pub paused: bool,
    pub cockpit: bool,
    pub show_help: bool,
    /// The settings are shown (O): a panel down the right side, the world still in view.
    pub settings_open: bool,
    /// The start and pause screen is shown (Esc, or as the game opens). It stays under the
    /// settings and the aircraft opened from it, and is back when they close.
    pub home_open: bool,
    /// The aircraft to choose from are shown (the menu's "Aircraft").
    pub hangar_open: bool,
    /// The first screen is shown (planes, drones or cars): as the game opens, or the menu's
    /// "Change vehicle". It covers the start and pause screen, which is back when it closes.
    pub vehicles_open: bool,
    /// The aircraft were opened from the settings: closing them goes back there.
    settings_under: bool,
    /// The keys and the controllers' buttons pressed while a menu was shown, as its moves, for the
    /// overlay to take (`ui::menu`).
    pub menu_keys: Vec<Nav>,
    pad_nav: PadNav,
    /// The place picker is shown (M): the game waits, as with the settings.
    pub picker_open: bool,
    /// The manual is shown (F1): the game waits, as with the picker.
    pub manual_open: bool,
    /// The simulation stands still without the game being paused: for screenshots.
    pub frozen: bool,
    /// The simulation stands still while another place is being loaded (or its loading failed and
    /// the player has not said what to do): the flight does not go on in the place being left.
    pub waiting_for_place: bool,
    /// Where the picture is taken from instead of the cockpit or the outside camera: a recording's
    /// camera (`record.rs`).
    pub view_override: Option<ViewPoint>,
    looking: bool,
    look_yaw: f64,
    look_pitch: f64,
    /// The head is turned by Z or X (or on its way back from them), not left where the mouse put it.
    look_keys: bool,
    orbit_yaw: f64,
    orbit_pitch: f64,
    /// The outside camera's heading behind the craft watched, as it follows its turns.
    chase: Option<Chase>,
    /// How far the outside camera stands from the aircraft, metres (the mouse wheel), and where
    /// it is drawn on its way there.
    orbit_distance: f64,
    orbit_shown: f64,
    /// Where the eye is in the cockpit: the spot chosen (`kinds::spots`) and where it is on its way
    /// there.
    spot: usize,
    camera: Spot,
    /// How much the wheel has magnified the cockpit's view (1: the spot's own field of view), and
    /// as it is drawn on its way there.
    zoom: f64,
    zoom_shown: f64,
    /// The DIMMING knobs, 0 off to 1 full: PANEL and STBY IND.
    pub panel_lights: f32,
    pub standby_lights: f32,
    /// How far the G1000's knobs have been turned, to draw them turning.
    pub knobs: KnobTurns,
    accumulator: f64,
    /// How many times faster than real time the world goes: one of `TIME_RATES` (F9 F10).
    time_rate: u32,
    /// How many times faster than real time it has gone lately, smoothed: below `time_rate` when
    /// the computer cannot step it that fast in `step_budget`.
    achieved_rate: f64,
    /// Real time not yet stepped, less than a step: what `pass` carries to the next frame for the
    /// steps real time asks for (`accumulator` is the world's, at the rate chosen).
    owed: f64,
    /// The wall-clock time each frame's steps beyond real time may take (`STEP_BUDGET`).
    pub step_budget: std::time::Duration,
    /// Real seconds the world has been going (not paused, not frozen, not waiting): what the texts
    /// in the middle of the screen are timed by, so that at ×16 they stay as long as at ×1.
    clock: f64,
    nearest: Option<Airport>,
    nearest_refresh: i32,
    touchdown_shown: Option<f64>,
    touchdown_until: f64,
    assist_shown_until: f64,
    /// The last thing that broke on the player's aircraft, said until `damage_until`.
    damage_seen: u32,
    damage_text: &'static str,
    damage_until: f64,
    /// What a knob, a switch or a tried failure of the instruments did, said until `notice_until`.
    notice: String,
    notice_until: f64,
    /// The last minutes of the world, kept all the time (`replay.rs`).
    pub recorder: Recorder,
    /// A replay is shown (F2): the world as it was when it began, put back when it ends.
    pub playback: Option<Playback>,
    /// A race round a course (`race.rs`), while one is flown.
    pub race: Option<RaceSession>,
    /// Who races against the player in every race (`Options::rivals`): `racer` or a model file.
    pub rival_specs: Vec<String>,
    /// The quads of the world the rivals fly, kept from race to race.
    rival_ids: Vec<VehicleId>,
    /// Another program's hands on the player's aircraft, once it has taken it (`remote.rs`).
    pub remote: Remote,
}

impl Session {
    /// A world with the player's aircraft on the home runway: the kind the settings file names, or the C172.
    pub fn new(seed: u64, store: Store) -> Session {
        Session::with_terrain(seed, Arc::new(Terrain::new(seed)), store)
    }

    /// `new`, on a terrain made beforehand: a real place's (`Terrain::from_region`) or a seed's.
    pub fn with_terrain(seed: u64, terrain: Arc<Terrain>, store: Store) -> Session {
        let mut world = World::with_terrain(seed, terrain);
        let craft = store.word("aircraft").and_then(Craft::parse).unwrap_or_default();
        let player = add_craft(&mut world, craft, Start::Home);
        let mut pilot = KeyboardPilot::new();
        let gfc = world.vehicle(player).aircraft().and_then(|a| a.p.afcs).map(Gfc700::new);
        if let Some(assist) = store.flag("assist") {
            pilot.assist = assist;
        }
        let recorder = Recorder::new(replay_window(&store));
        Session {
            world,
            player,
            planes: vec![Flown { id: player, flier: Flier::Player }],
            pilot,
            pads: Pads::new(Mappings::memory()),
            autopilot: None,
            gfc,
            destination: None,
            watched: 0,
            store,
            paused: false,
            cockpit: true,
            show_help: false,
            settings_open: false,
            home_open: false,
            hangar_open: false,
            vehicles_open: false,
            settings_under: false,
            menu_keys: Vec::new(),
            pad_nav: PadNav::default(),
            picker_open: false,
            manual_open: false,
            frozen: false,
            waiting_for_place: false,
            view_override: None,
            looking: false,
            look_yaw: 0.0,
            look_pitch: 0.0,
            look_keys: false,
            orbit_yaw: 0.0,
            orbit_pitch: drone::orbit_pitch(craft),
            chase: None,
            orbit_distance: ORBIT_DISTANCE * drone::chase_scale(craft),
            orbit_shown: ORBIT_DISTANCE * drone::chase_scale(craft),
            spot: 0,
            camera: drone::spots(craft)[0],
            zoom: 1.0,
            zoom_shown: 1.0,
            panel_lights: 0.6,
            standby_lights: 0.6,
            knobs: KnobTurns::default(),
            accumulator: 0.0,
            time_rate: 1,
            achieved_rate: 1.0,
            owed: 0.0,
            step_budget: STEP_BUDGET,
            clock: 0.0,
            nearest: None,
            nearest_refresh: 0,
            touchdown_shown: None,
            touchdown_until: 0.0,
            assist_shown_until: -1.0,
            damage_seen: 0,
            damage_text: "",
            damage_until: -1.0,
            notice: String::new(),
            notice_until: -1.0,
            recorder,
            playback: None,
            race: None,
            rival_specs: Vec::new(),
            rival_ids: Vec::new(),
            remote: Remote::default(),
        }
    }

    pub fn vehicle(&self) -> &Vehicle {
        self.world.vehicle(self.player)
    }

    /// The player's aircraft, if the player flies one.
    pub fn aircraft(&self) -> Option<&Aircraft> {
        self.vehicle().aircraft()
    }

    /// The player's multirotor, if the player flies one.
    pub fn multirotor(&self) -> Option<&Multirotor> {
        self.vehicle().multirotor()
    }

    /// The kind of aircraft the player flies, if it is an aircraft.
    pub fn aircraft_kind(&self) -> Option<AircraftKind> {
        self.aircraft().map(|a| a.p.kind)
    }

    /// What the player flies.
    pub fn craft(&self) -> Craft {
        Craft::of(self.vehicle())
    }

    /// The player's rigid body.
    pub fn body(&self) -> Body {
        self.vehicle().body()
    }

    /// The player's vehicle as a state to place it in (`World::place`): an aircraft's own, a
    /// multirotor's body with nothing else.
    fn state(&self) -> State {
        match self.aircraft() {
            Some(a) => a.state,
            None => {
                let b = self.body();
                State { pos: b.pos, vel: b.vel, q: b.q, omega: b.omega, ..State::default() }
            }
        }
    }

    /// The places the eye can be in the player's craft: an aircraft's cockpit's spots
    /// (`kinds::spots`), a multirotor's FPV camera.
    fn spots(&self) -> Vec<Spot> {
        drone::spots(self.craft())
    }

    /// Moves to another world (a real place loaded while the game runs, or back to the seed's): the
    /// same weather, hour and aircraft, on the runway of its home aerodrome (in the air over its
    /// middle if it has none), with as many aircraft of traffic as there were; the trained models
    /// are left behind.
    pub fn move_to(&mut self, terrain: Arc<Terrain>) {
        self.end_replay();
        self.set_time_rate(1);
        self.recorder.clear();
        self.race = None;
        let craft = self.craft();
        let traffic = self.planes.iter().filter(|f| matches!(f.flier, Flier::Traffic(_))).count();
        let mut world = World::with_terrain(self.world.seed(), terrain);
        (world.weather, world.hour) = (self.world.weather, self.world.hour);
        self.player = add_craft(&mut world, craft, Start::Home);
        self.world = world;
        self.planes = vec![Flown { id: self.player, flier: Flier::Player }];
        self.rival_ids.clear();
        (self.watched, self.nearest, self.nearest_refresh) = (0, None, 0);
        if self.vehicle().home.is_some() {
            self.restart_on_runway();
        } else {
            self.restart_over(0.0, 0.0, 0.0);
        }
        self.add_traffic(traffic);
    }

    /// The player flies a craft of another kind from now on, starting on the runway.
    pub fn set_craft(&mut self, craft: Craft) {
        if self.craft() == craft {
            return;
        }
        self.end_race();
        let vehicle = self.world.vehicle_mut(self.player);
        match (craft, &mut vehicle.dynamics) {
            // Another airframe; the instruments (their seed, their settings) stay.
            (Craft::Aircraft(kind), Dynamics::Aircraft(aircraft)) => {
                let instruments = aircraft.instruments.clone();
                *aircraft = Aircraft::of(kind);
                aircraft.instruments = instruments;
            }
            (Craft::Aircraft(kind), dynamics) => *dynamics = Dynamics::Aircraft(Aircraft::of(kind)),
            (Craft::Multirotor(kind), dynamics) => *dynamics = Dynamics::Multirotor(Multirotor::of(kind)),
        }
        self.gfc = self.aircraft().and_then(|a| a.p.afcs).map(Gfc700::new);
        self.autopilot = None;
        self.orbit_distance = ORBIT_DISTANCE * drone::chase_scale(craft);
        self.orbit_shown = self.orbit_distance;
        self.orbit_pitch = drone::orbit_pitch(craft);
        self.restart_on_runway();
        self.set_spot(0);
        self.camera = self.spot_target();
    }

    /// Races course `index` of `course::file::courses` in this world (the course's: the caller
    /// moves to it first): the player in a multirotor (the course's `quad` if it names one, else
    /// the one flown, else the first of the fleet) on the start's pad, its FPV camera, the gates in
    /// the world, and the countdown begun.
    pub fn start_race(&mut self, index: usize) -> Result<(), String> {
        let course = sim_core::course::file::courses().get(index).ok_or("no such course")?;
        match &course.place {
            sim_core::course::Place::Seed(seed) if self.world.terrain.region().is_some() || self.world.terrain.seed() != *seed => {
                return Err(format!("{} is in the world of seed {seed}", course.name));
            }
            _ => {}
        }
        let quad = course.quad.as_deref().and_then(sim_core::MultirotorKind::parse);
        if let Some(kind) = quad.filter(|k| self.multirotor().is_none_or(|m| m.p.kind != *k)) {
            self.set_craft(Craft::Multirotor(kind));
        } else if self.multirotor().is_none() {
            let kind = sim_core::MultirotorKind::all().next().ok_or("no multirotor to race")?;
            self.set_craft(Craft::Multirotor(kind));
        }
        let track = Arc::new(sim_core::course::Track::new(course, &self.world.terrain));
        self.world.obstacles = Arc::new(track.obstacles());
        self.race = Some(RaceSession::new(index, track, self.world.time(), &self.store));
        self.restart_race();
        self.cockpit = true;
        self.set_spot(0);
        self.camera = self.spot_target();
        Ok(())
    }

    /// The race again from the start's pad, the countdown begun; the best lap stays.
    pub fn restart_race(&mut self) {
        let Some(race) = &self.race else { return };
        let (track, index, racer) = (race.track.clone(), race.index, race.racer.is_some());
        let (terrain, weather, earth) = (&*self.world.terrain, &self.world.weather, &self.world.earth);
        self.world.vehicles[self.player.0].reset_on_ground(terrain, weather, earth, track.start.x, track.start.y, track.start_heading);
        if let Some(m) = self.world.vehicle_mut(self.player).multirotor_mut() {
            m.controls = sim_core::MultirotorControls { arm: true, ..Default::default() };
        }
        self.let_go();
        let mut fresh = RaceSession::new(index, track, self.world.time(), &self.store);
        if racer {
            fresh.toggle_racer(&self.world, self.player);
        }
        self.add_rivals(&mut fresh);
        self.race = Some(fresh);
    }

    /// The rivals (`rival_specs`) into a race about to start, each in a quad of its own (the
    /// player's kind) on a pad of its own, side by side across the start with the player's in the
    /// middle, `PAD_SPACING` metres apart (`rival_pad`). The network finishes as surely and as
    /// quickly from a side pad as from the middle (`trainer/GATES.md`). A rival that cannot be made
    /// (a model file that is not a `gates` model) is said and left out.
    fn add_rivals(&mut self, race: &mut RaceSession) {
        self.planes.retain(|f| !matches!(f.flier, Flier::Rival));
        self.watched = self.watched.min(self.planes.len() - 1);
        let Some(kind) = self.multirotor().map(|m| m.p.kind) else { return };
        let track = race.track.clone();
        let (c, s) = (track.start_heading.cos(), track.start_heading.sin());
        for (k, spec) in self.rival_specs.clone().iter().enumerate() {
            let id = match self.rival_ids.get(k) {
                Some(id) => *id,
                None => {
                    let id = self.world.add_multirotor(kind, Start::Home);
                    self.rival_ids.push(id);
                    id
                }
            };
            let across = rival_pad(k);
            let (terrain, weather, earth) = (&*self.world.terrain, &self.world.weather, &self.world.earth);
            self.world.vehicles[id.0].reset_on_ground(terrain, weather, earth, track.start.x - s * across, track.start.y + c * across, track.start_heading);
            match Rival::new(spec, id, k, &track, &self.world) {
                Ok(rival) => {
                    race.rivals.push(rival);
                    self.planes.push(Flown { id, flier: Flier::Rival });
                }
                Err(error) => {
                    eprintln!("rival {spec}: {error}");
                    self.notice = format!("rival {spec}: {error}");
                    self.notice_until = self.clock + 8.0;
                }
            }
        }
    }

    /// The pads of the race's start, one for each quad racing (the player's and the rivals'): metres
    /// to the right of its middle.
    pub fn race_pads(&self) -> Vec<f64> {
        let rivals = self.race.as_ref().map_or(0, |r| r.rivals.len());
        std::iter::once(0.0).chain((0..rivals).map(rival_pad)).collect()
    }

    /// No race any more: the gates go from the world, and the rivals' quads (left standing on
    /// their pads, disarmed, not drawn: each on its own, not in one another).
    pub fn end_race(&mut self) {
        if let Some(race) = self.race.take() {
            self.world.obstacles = Arc::new(Vec::new());
            self.planes.retain(|f| !matches!(f.flier, Flier::Rival));
            self.watched = 0;
            let (terrain, weather, earth) = (&*self.world.terrain, &self.world.weather, &self.world.earth);
            let (start, heading) = (race.track.start, race.track.start_heading);
            for (k, id) in self.rival_ids.iter().enumerate() {
                let across = rival_pad(k);
                self.world.vehicles[id.0].reset_on_ground(terrain, weather, earth, start.x - heading.sin() * across, start.y + heading.cos() * across, heading);
                if let Some(m) = self.world.vehicles[id.0].multirotor_mut() {
                    m.controls.arm = false;
                }
            }
        }
    }

    /// The player's aircraft's controls, if it flies one.
    fn controls(&mut self) -> Option<&mut AircraftControls> {
        self.world.vehicle_mut(self.player).aircraft_mut().map(|a| &mut a.controls)
    }

    // ---- Input

    /// A key went down. Esc closes what is on top (the help, a menu, the settings, a replay), and
    /// otherwise shows the start and pause screen, the game waiting: Esc on it flies on. O opens
    /// the settings beside the flight, which goes on (O again closes them). While paused any other
    /// key except H and T only resumes.
    pub fn key_pressed(&mut self, key: KeyCode) {
        self.key_pressed_with(key, false);
    }

    /// `key_pressed`, with shift held or not (the instruments' keys use it).
    pub fn key_pressed_with(&mut self, key: KeyCode, shift: bool) {
        match key {
            KeyCode::Escape => {
                if self.show_help && !self.menu_shown() {
                    self.show_help = false;
                } else if self.paused || self.menu_shown() {
                    self.back();
                } else if self.playback.is_some() {
                    self.end_replay();
                } else {
                    self.open_home();
                }
                return;
            }
            // The settings opened over the flight close again; over the menu, they stay.
            KeyCode::KeyO if self.settings_open && !self.paused => {
                self.back();
                return;
            }
            KeyCode::KeyO => {
                self.open_settings();
                return;
            }
            KeyCode::KeyM => {
                self.open_picker();
                return;
            }
            KeyCode::F1 => {
                self.open_manual();
                return;
            }
            _ => {}
        }
        if self.paused && key != KeyCode::KeyH && key != KeyCode::KeyT {
            // A menu has the keyboard: its moves are kept for it. On the start and pause screen any
            // other key flies on, as it always did; the settings and the aircraft wait for Esc.
            if self.menu_shown() {
                if let Some(nav) = Nav::of_key(key, shift) {
                    self.menu_keys.push(nav);
                    return;
                }
                if self.settings_open || self.hangar_open || self.vehicles_open {
                    return;
                }
            }
            self.resume();
            return;
        }
        if matches!(key, KeyCode::F9 | KeyCode::F10) {
            self.step_time_rate(key == KeyCode::F10);
            return;
        }
        if key == KeyCode::F2 {
            if self.playback.is_some() {
                self.end_replay();
            } else {
                self.start_replay();
            }
            return;
        }
        if self.playback.is_some() && !self.replay_key(key, shift) {
            return;
        }
        let s = self.body();
        let qnh = match self.world.terrain.nearest_airport(s.pos.x, s.pos.y) {
            Some(a) => self.world.weather.air.qnh(a.elevation),
            None => self.world.weather.air.sea_level_pressure,
        };
        if let Some(text) = instrument_keys::press(self.world.vehicle_mut(self.player), key, shift, qnh) {
            // K is the G1000's BARO knob too, where there is one.
            if key == KeyCode::KeyK && self.gfc.is_some() {
                self.knobs.turn(PanelKnob::Baro, if shift { -1 } else { 1 });
            }
            self.notice = text;
            self.notice_until = self.clock + 3.0;
            return;
        }
        // The G1000's keys of an aircraft with a GFC 700; P goes to the game's autopilot while it flies.
        if self.afcs_arrow(key, shift) {
            return;
        }
        if let Some(input) = controls::panel_key(key, shift).filter(|_| self.gfc.is_some() && !(key == KeyCode::KeyP && self.autopilot.is_some())) {
            // P with nothing on: the autopilot, holding the heading and the altitude of now.
            let fresh = input == Panel::Key(AfcsKey::Ap) && self.gfc.as_ref().is_some_and(|g| !g.director());
            self.panel(input);
            if fresh {
                self.hold_here();
            }
            return;
        }
        // A multirotor's own keys: arm.
        if let Some(text) = self.world.vehicle_mut(self.player).multirotor_mut().and_then(|m| drone::key(m, key)) {
            self.notice = text;
            self.notice_until = self.clock + 2.0;
            return;
        }
        // A transport's levers: the gear, the speed brakes, reverse thrust.
        if let Some(text) = self.transport_key(key, shift) {
            self.notice = text.to_string();
            self.notice_until = self.clock + 2.0;
            return;
        }
        match key {
            KeyCode::KeyF => {
                let most = self.aircraft().map_or(0, |a| a.p.max_flap_notch());
                if let Some(c) = self.controls() {
                    c.flap_notch = (c.flap_notch + 1).min(most);
                }
            }
            KeyCode::KeyR => {
                if let Some(c) = self.controls() {
                    c.flap_notch = (c.flap_notch - 1).max(0);
                }
            }
            KeyCode::KeyC if self.autopilot.is_some() => self.disengage(),
            // AP DISC: disconnects, or silences the alert.
            KeyCode::KeyC if self.gfc.as_ref().is_some_and(|g| g.engaged() || g.alert().is_some()) => self.panel(Panel::Key(AfcsKey::Disconnect)),
            KeyCode::KeyC => self.pilot.centre(),
            KeyCode::KeyP if self.autopilot.is_some() => self.disengage(),
            KeyCode::KeyP => self.engage_hold(),
            // In a race the autopilot races, or hands the quad back.
            KeyCode::KeyL if self.race.is_some() => {
                let (world, player) = (&self.world, self.player);
                if let Some(race) = &mut self.race {
                    race.toggle_racer(world, player);
                }
            }
            KeyCode::KeyL => self.engage_landing(),
            KeyCode::ArrowLeft => self.adjust_hold(-HEADING_STEP, 0.0),
            KeyCode::ArrowRight => self.adjust_hold(HEADING_STEP, 0.0),
            KeyCode::ArrowUp => self.adjust_hold(0.0, ALTITUDE_STEP),
            KeyCode::ArrowDown => self.adjust_hold(0.0, -ALTITUDE_STEP),
            KeyCode::KeyN => self.watch_next(),
            KeyCode::KeyG => self.set_assist(!self.pilot.assist),
            KeyCode::KeyV => {
                self.cockpit = !self.cockpit;
                self.watched = 0;
                self.look_yaw = 0.0;
                self.look_pitch = 0.0;
                self.set_spot(0);
                self.camera = self.spot_target();
            }
            KeyCode::KeyH => self.show_help = !self.show_help,
            KeyCode::Digit0 | KeyCode::Numpad0 => (self.look_yaw, self.look_pitch) = (0.0, 0.0),
            KeyCode::Equal | KeyCode::NumpadAdd => self.zoom(1.0),
            KeyCode::Minus | KeyCode::NumpadSubtract => self.zoom(-1.0),
            KeyCode::Space => self.look_at_panel(),
            KeyCode::KeyI => self.dim_panel(if shift { -0.2 } else { 0.2 }),
            // In a race, each restart key starts it again from the pad.
            KeyCode::Digit1 | KeyCode::Backspace | KeyCode::Digit2 | KeyCode::Digit3 if self.race.is_some() => self.restart_race(),
            // A multirotor starts again on the ground where it is; an aircraft on the runway.
            KeyCode::Digit1 | KeyCode::Backspace if self.multirotor().is_some() => self.restart_here(),
            KeyCode::Digit1 | KeyCode::Backspace => self.restart_on_runway(),
            KeyCode::Digit2 => self.restart_in_air(),
            KeyCode::Digit3 => self.restart_on_final(),
            _ => {}
        }
    }

    /// ⇧G, ⇧B and ⇧S on an aircraft that has what they work: the gear lever (a gear that
    /// retracts), the speed brake lever (spoilers) and reverse thrust (reversers). What it did, for
    /// the player; `None` if the key is not one of them or the aircraft has not that.
    fn transport_key(&mut self, key: KeyCode, shift: bool) -> Option<&'static str> {
        if !shift {
            return None;
        }
        let (retracts, spoilers, reversers, on_ground) = self.aircraft().map(|a| (a.p.retract.is_some(), a.p.spoilers.is_some(), a.p.jets.as_ref().is_some_and(|j| j.reverser.is_some()), a.data().on_ground()))?;
        let c = self.controls()?;
        match key {
            KeyCode::KeyG if retracts && on_ground => Some("The gear stays down on the ground"),
            KeyCode::KeyG if retracts => {
                c.gear_up = !c.gear_up;
                Some(if c.gear_up { "Gear up" } else { "Gear down" })
            }
            KeyCode::KeyB if spoilers => {
                c.spoilers = if c.spoilers > 0.0 { 0.0 } else { 1.0 };
                Some(if c.spoilers > 0.0 { "Speed brakes out" } else { "Speed brakes in" })
            }
            KeyCode::KeyS if reversers => {
                c.reverse = !c.reverse;
                // The levers come back to idle as the reversers move.
                c.throttle = 0.0;
                Some(if c.reverse { "Reverse thrust: W for more" } else { "Forward thrust" })
            }
            _ => None,
        }
    }

    /// The game controllers as they are this frame: their buttons act now, as the keys they stand
    /// for; their sticks and levers fly during `advance`.
    pub fn controllers(&mut self, controllers: &[engine::Controller]) {
        let keys = self.pads.read(controllers);
        let (moves, back) = self.pad_nav.read(controllers);
        // The picker and the manual cover the window and are worked with the keyboard and the mouse:
        // a button pressed meanwhile is not for the flight behind them, nor for a menu under them.
        if self.covered() {
            return;
        }
        // A menu has the controllers too: the direction pad or the stick moves in it, A chooses, B
        // goes back, and of what the buttons stand for in flight only the pause counts.
        if self.paused && self.menu_shown() {
            self.menu_keys.extend(moves);
            if back || keys.contains(&KeyCode::Escape) {
                self.key_pressed(KeyCode::Escape);
            }
            return;
        }
        for key in keys {
            self.key_pressed(key);
        }
    }

    /// Stops the clock and lets go of the view. Any key resumes.
    pub fn pause(&mut self) {
        self.set_looking(false);
        self.paused = true;
        self.set_time_rate(1);
    }

    /// The flight goes on: nothing of the menus stays open.
    pub fn resume(&mut self) {
        self.paused = false;
        self.close_menus();
    }

    fn close_menus(&mut self) {
        (self.home_open, self.settings_open, self.hangar_open, self.settings_under, self.vehicles_open) = (false, false, false, false, false);
        self.menu_keys.clear();
    }

    /// A menu is shown: the first screen, the start and pause screen, the settings or the aircraft.
    pub fn menu_shown(&self) -> bool {
        self.home_open || self.settings_open || self.hangar_open || self.vehicles_open
    }

    /// A menu is shown that the game waits under: all but the settings opened in flight, beside
    /// which it goes on.
    pub fn waiting_menu_shown(&self) -> bool {
        self.home_open || self.hangar_open || self.vehicles_open
    }

    /// Esc while the game waits: out of the settings or the aircraft, back to the start and pause
    /// screen if they were opened from it; otherwise the flight goes on.
    pub fn back(&mut self) {
        if self.vehicles_open {
            // Nothing chosen on the first screen: the start and pause screen, with the craft that waits.
            (self.vehicles_open, self.home_open) = (false, true);
            self.menu_keys.clear();
        } else if self.hangar_open && std::mem::take(&mut self.settings_under) {
            // The aircraft opened from the settings: back to them, and, if they were opened in
            // flight, the flight goes on beside them.
            (self.hangar_open, self.settings_open) = (false, true);
            self.menu_keys.clear();
            if !self.home_open {
                self.paused = false;
            }
        } else if (self.settings_open || self.hangar_open) && self.home_open {
            (self.settings_open, self.hangar_open) = (false, false);
            self.menu_keys.clear();
        } else {
            self.resume();
        }
    }

    /// Shows the start and pause screen. The game waits.
    pub fn open_home(&mut self) {
        self.end_replay();
        self.home_open = true;
        self.pause();
    }

    /// Shows the first screen: planes, drones or cars. The game waits; the start and pause screen
    /// is under it.
    pub fn open_vehicles(&mut self) {
        self.end_replay();
        (self.settings_open, self.hangar_open, self.settings_under) = (false, false, false);
        (self.vehicles_open, self.home_open) = (true, true);
        self.menu_keys.clear();
        self.pause();
    }

    /// A family chosen on the first screen: its craft flown last, if another family's was flown
    /// (`Family::craft`), then its aircraft to choose from, over the start and pause screen.
    pub fn choose_family(&mut self, family: Family) {
        if self.craft().family() != family {
            if let Some(craft) = family.craft(self.store.word(family.key())) {
                self.fly(craft);
            }
        }
        (self.vehicles_open, self.home_open) = (false, true);
        self.menu_keys.clear();
        self.open_hangar();
    }

    /// The craft chosen in the menus: flown from now on, and kept in the settings as the one the
    /// game opens with and as its family's.
    pub fn fly(&mut self, craft: Craft) {
        self.store.set("aircraft", craft.key());
        self.store.set(craft.family().key(), craft.key());
        self.set_craft(craft);
    }

    /// Shows the settings, a panel on the right. Opened in flight, the flight goes on beside them
    /// and the keys and the controllers still fly (the panel is worked with the mouse); opened
    /// from the start and pause screen, the game waits under them as it did there. What they
    /// change of the day is seen at once in the world beside them.
    pub fn open_settings(&mut self) {
        self.end_replay();
        // From the aircraft (opened from the settings in flight): the flight goes on again.
        if self.hangar_open && !self.home_open {
            self.paused = false;
        }
        (self.hangar_open, self.vehicles_open) = (false, false);
        self.settings_open = true;
    }

    /// Shows the aircraft to choose from (instead of the settings, if they are open). The game waits meanwhile.
    pub fn open_hangar(&mut self) {
        self.end_replay();
        self.settings_under = self.settings_open;
        (self.settings_open, self.vehicles_open) = (false, false);
        self.hangar_open = true;
        self.pause();
    }

    /// Shows the place picker (instead of the settings, if they are open). The game waits meanwhile.
    pub fn open_picker(&mut self) {
        self.end_replay();
        (self.settings_open, self.hangar_open, self.vehicles_open) = (false, false, false);
        self.picker_open = true;
        self.pause();
    }

    /// Shows the manual (instead of the settings, if they are open). The game waits meanwhile.
    pub fn open_manual(&mut self) {
        (self.settings_open, self.hangar_open, self.vehicles_open) = (false, false, false);
        self.manual_open = true;
        self.pause();
    }

    /// A screen covers the whole window (the picker or the manual): the world is not drawn and
    /// the keyboard and the mouse are that screen's.
    pub fn covered(&self) -> bool {
        self.picker_open || self.manual_open
    }

    // ---- Replay

    /// Shows the last minutes again (F2): from 30 s before the last landing kept, else from a
    /// minute before now. Nothing to show yet: says so.
    pub fn start_replay(&mut self) {
        let Some((start, end)) = self.recorder.span().filter(|(start, end)| end - start >= 1.0) else {
            self.notice = "Nothing to replay yet".to_string();
            self.notice_until = self.clock + 3.0;
            return;
        };
        let from = match self.recorder.last_landing() {
            Some(landing) => landing.time - 30.0,
            None => end - 60.0,
        };
        self.set_looking(false);
        self.set_time_rate(1);
        self.playback = Some(Playback {
            vehicles: self.world.vehicles.clone(),
            wrecks: self.world.wrecks.clone(),
            gfc: self.gfc.clone(),
            hour: self.world.hour,
            time: from.clamp(start, end),
            speed: 1.0,
            playing: true,
        });
    }

    /// Back to the flight, exactly as it was when the replay began.
    pub fn end_replay(&mut self) {
        let Some(p) = self.playback.take() else { return };
        (self.world.vehicles, self.world.wrecks, self.gfc, self.world.hour) = (p.vehicles, p.wrecks, p.gfc, p.hour);
    }

    /// Whether a replay is shown.
    pub fn replaying(&self) -> bool {
        self.playback.is_some()
    }

    /// The clock of the picture: the moment a replay shows, else the world's.
    pub fn scene_time(&self) -> f64 {
        self.playback.as_ref().map_or(self.world.time(), |p| p.time)
    }

    /// A key during a replay: Space or P plays or stops, ← → jump 10 s (⇧ 30 s), ↑ ↓ change the
    /// speed, Home and End go to the start and the end. Returns whether the key is left to do what
    /// it always does: only the camera's and the help's are; the rest do nothing.
    fn replay_key(&mut self, key: KeyCode, shift: bool) -> bool {
        let (Some(p), Some(span)) = (&mut self.playback, self.recorder.span()) else { return false };
        let jump = if shift { 30.0 } else { 10.0 };
        match key {
            KeyCode::Space | KeyCode::KeyP => {
                if !p.playing && p.time >= span.1 {
                    p.time = span.0;
                }
                p.playing = !p.playing;
            }
            KeyCode::ArrowLeft => p.jump(-jump, span),
            KeyCode::ArrowRight => p.jump(jump, span),
            KeyCode::ArrowUp => p.change_speed(true),
            KeyCode::ArrowDown => p.change_speed(false),
            KeyCode::Home => p.time = span.0,
            KeyCode::End => p.time = span.1,
            KeyCode::KeyV | KeyCode::KeyN | KeyCode::KeyH | KeyCode::KeyI | KeyCode::Digit0 | KeyCode::Numpad0 | KeyCode::Equal | KeyCode::NumpadAdd | KeyCode::Minus | KeyCode::NumpadSubtract => return true,
            _ => {}
        }
        false
    }

    /// Turns the keyboard assist on or off and remembers it.
    pub fn set_assist(&mut self, on: bool) {
        self.pilot.assist = on;
        self.store.set("assist", on);
        self.assist_shown_until = self.clock + 3.0;
    }

    // ---- Autopilot, traffic and the camera's aircraft

    /// P: the autopilot holds the heading, altitude and airspeed of this moment. Not on the ground.
    pub fn engage_hold(&mut self) {
        let plane = self.vehicle();
        let Some(a) = plane.aircraft() else { return };
        if a.data().on_ground() || plane.agl() < 2.0 || a.crashed().is_some() {
            return;
        }
        let (slowest, fastest) = kinds::autopilot_speeds(a.p.kind);
        let goal = Goal::Hold {
            heading: Some(euler_angles(a.state.q).heading),
            vertical: Vertical::Altitude(a.indicated_altitude()),
            airspeed: Some(clamp(a.indicated_airspeed(), slowest, fastest)),
        };
        self.autopilot = Some(Autopilot::for_type(goal, &a.p));
    }

    /// L: the autopilot flies the circuit of the nearest airport, lands and stops (from the
    /// ground: takes off first). The runway direction is the one into the wind.
    pub fn engage_landing(&mut self) {
        let Some(a) = self.aircraft().filter(|a| a.crashed().is_none()) else { return };
        let p = a.p.clone();
        let Some((airport, reversed)) = self.restart_airport() else { return };
        self.autopilot = Some(Autopilot::for_type(Goal::Land { airport, reversed }, &p));
        // The GFC 700 lets go of it (its two-tone sounds).
        if let Some(gfc) = self.gfc.as_mut().filter(|g| g.engaged()) {
            gfc.press(AfcsKey::Ap);
        }
    }

    /// The keyboard has the aircraft again, holding the attitude it is handed.
    pub fn disengage(&mut self) {
        if self.autopilot.take().is_some() {
            self.pilot.reset();
        }
    }

    /// The GFC 700 just engaged by P with its flight director off: instead of ROL and PIT, HDG with
    /// the bug on the heading of now and ALT at the altitude of now, the selected altitude the
    /// nearest hundred feet to it. (The bezel's AP key is the real one: ROL and PIT.)
    fn hold_here(&mut self) {
        let Some(gfc) = self.gfc.as_mut().filter(|g| g.engaged()) else { return };
        gfc.press(AfcsKey::HeadingSync);
        if gfc.lateral() != sim_core::LateralMode::Heading {
            gfc.press(AfcsKey::Hdg);
        }
        if !matches!(gfc.vertical(), VerticalMode::Altitude(_)) {
            gfc.press(AfcsKey::Alt);
        }
        if let VerticalMode::Altitude(held) = gfc.vertical() {
            gfc.set_selected_altitude((held / (100.0 * FEET)).round() * 100.0 * FEET);
        }
    }

    /// An arrow with the GFC 700 engaged: not the yoke's, the autopilot's targets. Left and right
    /// the heading bug 5° (with Shift 1°), in HDG; up and down the selected altitude 100 ft (with
    /// Shift 1000 ft), and VS at 500 ft/min towards it, which levels at it by itself (ALTS, then
    /// ALT). False when it is not engaged, or the key is not an arrow.
    fn afcs_arrow(&mut self, key: KeyCode, shift: bool) -> bool {
        let altitude = self.aircraft().map_or(0.0, |a| a.instruments.readings().air_data.altitude);
        let Some(gfc) = self.gfc.as_mut().filter(|g| g.engaged()) else { return false };
        let (knob, clicks) = match key {
            KeyCode::ArrowLeft => (PanelKnob::Heading, if shift { -1 } else { -5 }),
            KeyCode::ArrowRight => (PanelKnob::Heading, if shift { 1 } else { 5 }),
            KeyCode::ArrowUp => (if shift { PanelKnob::AltitudeCoarse } else { PanelKnob::AltitudeFine }, 1),
            KeyCode::ArrowDown => (if shift { PanelKnob::AltitudeCoarse } else { PanelKnob::AltitudeFine }, -1),
            _ => return false,
        };
        if knob == PanelKnob::Heading {
            gfc.turn(AfcsKnob::Heading, clicks);
            if gfc.lateral() != sim_core::LateralMode::Heading {
                gfc.press(AfcsKey::Hdg);
            }
        } else {
            // A thousand feet from what is selected, not to the nearest thousand as the outer ring.
            gfc.set_selected_altitude(gfc.selected_altitude() + f64::from(clicks) * if shift { 1000.0 } else { 100.0 } * FEET);
            let to_go = gfc.selected_altitude() - altitude;
            if to_go.abs() > 50.0 * FEET {
                gfc.set_vertical_speed(ARROW_CLIMB.copysign(to_go));
            } else if !matches!(gfc.vertical(), VerticalMode::Altitude(_)) {
                gfc.press(AfcsKey::Alt);
            }
        }
        self.knobs.turn(knob, clicks);
        true
    }

    /// A knob of the G1000 and what it is set to: "HDG 080°", "ALT 5500 ft", "BARO 1013 hPa".
    pub fn knob_name(&self, knob: PanelKnob) -> String {
        let gfc = self.gfc.as_ref();
        match knob {
            PanelKnob::Heading => format!("HDG {:03.0}°", gfc.map_or(0.0, |g| (g.heading_bug() / DEG).round().rem_euclid(360.0))),
            PanelKnob::AltitudeFine | PanelKnob::AltitudeCoarse => format!("ALT {:.0} ft", gfc.map_or(0.0, |g| g.selected_altitude() / FEET)),
            PanelKnob::Baro => format!("BARO {:.0} hPa", self.aircraft().map_or(0.0, |a| a.instruments.altimeter_setting / HPA)),
            PanelKnob::Course => "CRS · for the VOR, not fitted yet".to_string(),
        }
    }

    /// The tag beside the key or the knob of the panel the pointer is over (a click there `does`
    /// this; `shift` is held): its name, with what a knob is set to, and under it what else the
    /// mouse does there. The wheel with Shift over the HDG knob and the AFCS keys is the ALT knob,
    /// which is hard to find from the seat.
    pub fn panel_tag(&self, does: Panel, shift: bool) -> (String, String) {
        let altitude = self.knob_name(PanelKnob::AltitudeFine);
        match (does, cockpit::wheel_knob(Some(does), shift)) {
            (Panel::Knob(PanelKnob::AltitudeFine, _), _) => (altitude, "inner knob: 100 ft a click".to_string()),
            (Panel::Knob(PanelKnob::AltitudeCoarse, _), _) => (altitude, "outer ring: 1000 ft a click".to_string()),
            (_, Some(PanelKnob::AltitudeFine)) => (altitude, "Shift + wheel: 100 ft a click".to_string()),
            (Panel::Knob(PanelKnob::Heading, _), _) => (self.knob_name(PanelKnob::Heading), format!("Shift + wheel: {altitude}")),
            (Panel::Knob(knob, _), _) => (self.knob_name(knob), String::new()),
            (Panel::Key(key), _) => (key_name(key).to_string(), format!("Shift + wheel: {altitude}")),
            (Panel::DirectTo, _) => ("D→ · direct to the nearest aerodrome".to_string(), String::new()),
        }
    }

    /// A key of the G1000's bezel pressed, or a knob turned (from the keyboard, a controller or the
    /// mouse on the panel). Says why when it does nothing.
    pub fn panel(&mut self, input: Panel) {
        let Some(gfc) = &mut self.gfc else { return };
        let was = gfc.engaged();
        let refused = match input {
            Panel::Key(AfcsKey::Ap) if gfc.engaged() => !gfc.press(AfcsKey::Ap),
            Panel::Key(AfcsKey::Ap) => {
                // The game's autopilot lets go of the aircraft for the GFC 700.
                let engaged = gfc.press(AfcsKey::Ap);
                if engaged {
                    self.autopilot = None;
                }
                !engaged
            }
            Panel::Key(AfcsKey::Apr) if gfc.plan().is_none_or(|p| p.approach.is_none()) => {
                // The approach of the last D→, or of the nearest aerodrome, to the runway into the wind.
                let s = self.world.vehicle(self.player).body();
                let airport = self.destination.or_else(|| self.world.terrain.nearest_airport(s.pos.x, s.pos.y));
                match airport {
                    Some(runway) => {
                        let reversed = (self.world.weather.wind_from * DEG - runway.heading).cos() < 0.0;
                        let approach = Approach { runway, reversed };
                        gfc.load_approach(approach);
                        self.notice = format!("GPS approach to runway {}: IF at 9 NM, FAF at 5 NM, 3° path", &approach.name()[2..]);
                        self.notice_until = self.clock + 4.0;
                        !gfc.press(AfcsKey::Apr)
                    }
                    None => true,
                }
            }
            Panel::Key(key) => !gfc.press(key),
            Panel::Knob(knob, clicks) => {
                // The BARO knob's clicks are K's, which turns it.
                if knob != PanelKnob::Baro {
                    self.knobs.turn(knob, clicks);
                }
                match knob {
                    PanelKnob::Heading => gfc.turn(AfcsKnob::Heading, clicks),
                    PanelKnob::AltitudeFine => gfc.turn(AfcsKnob::AltitudeFine, clicks),
                    PanelKnob::AltitudeCoarse => gfc.turn(AfcsKnob::AltitudeCoarse, clicks),
                    PanelKnob::Baro => {
                        for _ in 0..clicks.unsigned_abs() {
                            self.key_pressed_with(KeyCode::KeyK, clicks < 0);
                        }
                    }
                    PanelKnob::Course => {
                        self.notice = "CRS: it is for the VOR, which comes in phase 2".to_string();
                        self.notice_until = self.clock + 3.0;
                    }
                }
                false
            }
            Panel::DirectTo => {
                let s = self.world.vehicle(self.player).body();
                match self.world.terrain.nearest_airport(s.pos.x, s.pos.y) {
                    Some(airport) => {
                        gfc.direct_to(&airport);
                        self.destination = Some(airport);
                        let (dn, de) = (airport.n - s.pos.x, airport.e - s.pos.y);
                        self.notice = format!("D→ aerodrome of runway {:02}: {:.1} NM at {:03.0}° (NAV to follow it)", airport.number(), dn.hypot(de) / 1852.0, (de.atan2(dn) / DEG).rem_euclid(360.0));
                        self.notice_until = self.clock + 4.0;
                        false
                    }
                    None => true,
                }
            }
        };
        if refused {
            self.notice = refusal(input, self.vehicle().dynamics.on_ground()).to_string();
            self.notice_until = self.clock + 3.0;
        }
        if was && !self.gfc.as_ref().is_some_and(Gfc700::engaged) {
            self.pilot.reset();
        }
    }

    /// One word of `--afcs` (for screenshots): a key of the bezel (ap fd hdg nav apr alt vs flc up
    /// dn dto sync disc), or a setting: `bug=<degrees>`, `sel=<feet>` (with a sign, from the heading
    /// or the altitude of now), `throttle=<0..1>`, or a knob turned some clicks:
    /// `turn=<hdg|alt|alt1000|baro|crs>:<clicks>`.
    pub fn afcs_command(&mut self, word: &str) -> Result<(), String> {
        if self.gfc.is_none() {
            return Err("this aircraft has no GFC 700".to_string());
        }
        let number = |text: &str| text.parse::<f64>().map_err(|_| format!("--afcs: {word} is not a number"));
        let key = match word {
            "ap" => AfcsKey::Ap,
            "fd" => AfcsKey::Fd,
            "hdg" => AfcsKey::Hdg,
            "nav" => AfcsKey::Nav,
            "apr" => AfcsKey::Apr,
            "alt" => AfcsKey::Alt,
            "vs" => AfcsKey::Vs,
            "flc" => AfcsKey::Flc,
            "up" => AfcsKey::NoseUp,
            "dn" => AfcsKey::NoseDown,
            "sync" => AfcsKey::HeadingSync,
            "disc" => AfcsKey::Disconnect,
            "dto" => {
                self.panel(Panel::DirectTo);
                return Ok(());
            }
            _ => {
                match word.split_once('=') {
                    Some(("bug", v)) => {
                        let from = if v.starts_with(['+', '-']) { self.aircraft().map_or(0.0, |a| a.instruments.readings().ahrs[2]) } else { 0.0 };
                        self.gfc.as_mut().expect("checked above").set_heading_bug(from + number(v)? * DEG);
                    }
                    Some(("sel", v)) => {
                        let from = if v.starts_with(['+', '-']) { self.aircraft().map_or(0.0, |a| a.instruments.readings().air_data.altitude) } else { 0.0 };
                        self.gfc.as_mut().expect("checked above").set_selected_altitude(((from / FEET + number(v)?) / 100.0).round() * 100.0 * FEET);
                    }
                    Some(("turn", v)) => {
                        let (name, clicks) = v.split_once(':').ok_or_else(|| format!("--afcs: {word} needs a knob and its clicks, as turn=hdg:10"))?;
                        let knob = match name {
                            "hdg" => PanelKnob::Heading,
                            "alt" => PanelKnob::AltitudeFine,
                            "alt1000" => PanelKnob::AltitudeCoarse,
                            "baro" => PanelKnob::Baro,
                            "crs" => PanelKnob::Course,
                            _ => return Err(format!("--afcs: unknown knob {name}")),
                        };
                        self.panel(Panel::Knob(knob, number(clicks)? as i32));
                    }
                    Some(("throttle", v)) => {
                        let throttle = clamp(number(v)?, 0.0, 1.0);
                        if let Some(c) = self.controls() {
                            c.throttle = throttle;
                        }
                    }
                    _ => return Err(format!("--afcs: unknown {word}")),
                }
                return Ok(());
            }
        };
        self.panel(Panel::Key(key));
        Ok(())
    }

    /// Moves the targets of a heading and altitude hold; does nothing otherwise.
    fn adjust_hold(&mut self, turn: f64, climb: f64) {
        self.change_hold(|heading, altitude, _| {
            *heading = (*heading + turn).rem_euclid(360.0 * DEG);
            *altitude += climb;
        });
    }

    fn change_hold(&mut self, change: impl FnOnce(&mut f64, &mut f64, &mut f64)) {
        let Some(autopilot) = &mut self.autopilot else { return };
        let Goal::Hold { heading: Some(mut heading), vertical: Vertical::Altitude(mut altitude), airspeed: Some(mut airspeed) } = *autopilot.goal() else {
            return;
        };
        change(&mut heading, &mut altitude, &mut airspeed);
        autopilot.set_goal(Goal::Hold { heading: Some(heading), vertical: Vertical::Altitude(altitude), airspeed: Some(airspeed) });
    }

    /// Adds `count` aircraft (at most `MAX_TRAFFIC`) flying circuits with the autopilot: at the
    /// player's airport and, every other one, at the nearest airports around it. None starts on
    /// the runway the player is on: those at home begin in the air, apart from each other.
    pub fn add_traffic(&mut self, count: usize) {
        let Some(home) = self.vehicle().home else { return };
        let into = self.world.weather.wind_from * DEG;
        let reversed = |a: &Airport| (into - a.heading).cos() < 0.0;
        let mut others = self.world.terrain.airports(home.n - 40_000.0, home.n + 40_000.0, home.e - 40_000.0, home.e + 40_000.0);
        others.retain(|a| a.id != home.id);
        others.sort_by(|a, b| (a.n - home.n).hypot(a.e - home.e).total_cmp(&(b.n - home.n).hypot(b.e - home.e)));
        let course = home.heading + if reversed(&home) { std::f64::consts::PI } else { 0.0 };
        let (c, s) = (course.cos(), course.sin());
        // A point given along the runway from its centre and to the right of it.
        let at = |along: f64, right: f64| (home.n + c * along - s * right, home.e + s * along + c * right);
        let mut at_home = 0;
        for i in 0..count.min(MAX_TRAFFIC.saturating_sub(self.planes.len() - 1)) {
            let (airport, start) = match others.get(i / 2).filter(|_| i % 2 == 1) {
                Some(other) => (*other, Start::Runway { airport: *other, reversed: reversed(other) }),
                None => {
                    at_home += 1;
                    let start = match at_home {
                        // 900 m up, 250 m ahead and a little to the right of the start of the runway.
                        1 => {
                            let (n, e) = at(290.0 - home.length / 2.0, 60.0);
                            Start::Air { n, e, heading: course }
                        }
                        2 => Start::Final { airport: home, reversed: reversed(&home), distance: 4500.0 },
                        k => {
                            let (n, e) = at(-3000.0 * (k - 1) as f64, -2500.0);
                            Start::Air { n, e, heading: course }
                        }
                    };
                    (home, start)
                }
            };
            let id = self.world.add_aircraft(AircraftKind::C172, start);
            let goal = Goal::Circuit { airport, reversed: reversed(&airport), repeat: true };
            let pilot = Autopilot::for_type(goal, AircraftKind::C172.spec());
            self.planes.push(Flown { id, flier: Flier::Traffic(Box::new(pilot)) });
        }
    }

    /// Adds an aircraft flown by a trained model (`file` or `file:task`) at the start of its task;
    /// see `ai.rs`. Counts against `MAX_TRAFFIC`.
    pub fn add_model(&mut self, spec: &str) -> Result<(), String> {
        if self.planes.len() > MAX_TRAFFIC {
            return Err(format!("at most {MAX_TRAFFIC} aircraft besides the player's"));
        }
        let flying: Vec<&str> = self.planes.iter().filter_map(|p| if let Flier::Model(m) = &p.flier { Some(m.task_name()) } else { None }).collect();
        let (id, flier) = ModelFlier::add(spec, &mut self.world, &flying)?;
        self.planes.push(Flown { id, flier: Flier::Model(Box::new(flier)) });
        Ok(())
    }

    /// What is flying for the player, shown as small tags at the top right: the keyboard assist, the
    /// autopilot and what it holds, and the trained model flying the aircraft watched.
    pub fn annunciators(&self) -> Vec<String> {
        let mut tags = Vec::new();
        let time = self.time_text();
        if !time.is_empty() && self.playback.is_none() {
            tags.push(time);
        }
        // The keyboard's assist is an aircraft's; a multirotor's flight controller says its mode itself.
        if self.pilot.assist && self.aircraft().is_some() {
            tags.push("ASSIST".to_string());
        }
        if let Some(autopilot) = &self.autopilot {
            tags.push(autopilot_text(autopilot));
        } else if let Some(gfc) = self.gfc.as_ref().filter(|g| g.director()) {
            tags.push(gfc_text(gfc));
        }
        let viewed = self.viewed();
        match &self.planes[viewed].flier {
            Flier::Model(m) if !self.cockpit => tags.push(format!("AI {}", m.name)),
            Flier::Rival => {
                if let Some(r) = self.race.as_ref().and_then(|r| r.rival(self.planes[viewed].id)) {
                    tags.push(format!("AI {}", r.name));
                }
            }
            _ => {}
        }
        tags
    }

    /// Which aircraft the camera is on and who flies it, for the status line.
    fn camera_text(&self) -> String {
        let at = self.viewed();
        let who = match &self.planes[at].flier {
            Flier::Rival => match self.race.as_ref().and_then(|r| r.rival(self.planes[at].id)) {
                Some(r) if r.network() => format!("the trained network {} (the gates race)", r.name),
                Some(_) => "the race's autopilot".to_string(),
                None => "nobody".to_string(),
            },
            Flier::Player if self.autopilot.is_some() || self.gfc.as_ref().is_some_and(Gfc700::engaged) => "you, with the autopilot".to_string(),
            Flier::Player => "you".to_string(),
            Flier::Traffic(_) => "the autopilot".to_string(),
            Flier::Model(m) => {
                let mut who = format!("the model {} ({})", m.name, ai::task_text(m.task_name()));
                if let Some(t) = m.flying_for(self.world.time()).filter(|_| m.task_name() == "race") {
                    who += &format!(", {t:.0} s");
                }
                if let Some(ending) = m.last_ending {
                    who += &format!(", last flight: {}", ai::ending_text(ending));
                    if let (sim_env::Reason::Success, Some(t)) = (ending, m.last_time) {
                        who += &format!(" in {t:.1} s");
                    }
                }
                if let Some(seconds) = m.restarting_in(self.world.time()) {
                    who += &format!(", starts again in {seconds:.0} s");
                }
                who
            }
        };
        format!("Camera: aircraft {} of {}, flown by {who}", at + 1, self.planes.len())
    }

    /// N: the outside camera goes to the next aircraft (and the view goes outside). In a race the
    /// camera goes to the next quad, as it is (its FPV camera, or behind it: V).
    pub fn watch_next(&mut self) {
        if self.planes.len() < 2 {
            return;
        }
        if self.race.is_some() {
            self.watched = (self.watched + 1) % self.planes.len();
            return;
        }
        self.watched = if self.cockpit { 1 } else { (self.watched + 1) % self.planes.len() };
        self.cockpit = false;
    }

    /// The aircraft the outside camera follows.
    pub fn watched(&self) -> VehicleId {
        self.planes[self.watched].id
    }

    /// The aircraft the view belongs to, as an index into `planes`: the one watched outside, and
    /// in a race in the FPV view too (a rival's quad's camera); else the player's.
    pub fn viewed(&self) -> usize {
        let watched = self.watched.min(self.planes.len() - 1);
        let quad = self.world.vehicle(self.planes[watched].id).multirotor().is_some();
        if !self.cockpit || (self.race.is_some() && quad) {
            watched
        } else {
            0
        }
    }

    /// The vehicle the view belongs to (`viewed`).
    pub fn viewed_vehicle(&self) -> &Vehicle {
        self.world.vehicle(self.viewed_vehicle_id())
    }

    pub fn viewed_vehicle_id(&self) -> VehicleId {
        self.planes[self.viewed()].id
    }

    /// Moves a panel lever; `value` is 0 at the bottom of its track and 1 at the top.
    pub fn set_lever(&mut self, lever: Lever, value: f64) {
        let Some(most) = self.aircraft().map(|a| a.p.max_flap_notch()) else { return };
        let Some(c) = self.controls() else { return };
        match lever {
            Lever::Flaps => c.flap_notch = ((1.0 - value) * most as f64).round() as i32,
            Lever::Trim => c.trim = clamp((0.5 - value) * 2.0, -1.0, 1.0),
            Lever::Throttle => c.throttle = value,
        }
    }

    /// The pointer moved by (dx, dy) points, y down, while a button holds the view.
    pub fn mouse_moved(&mut self, dx: f64, dy: f64) {
        if !self.looking {
            return;
        }
        // The natural way round (a setting): the scene goes with the pointer, as if taken by hand.
        let (dx, dy) = if self.store.flag(settings::LOOK_NATURAL).unwrap_or(false) { (-dx, -dy) } else { (dx, dy) };
        if self.cockpit {
            self.look_yaw = clamp(self.look_yaw + dx * 0.004, -2.6, 2.6);
            self.look_pitch = clamp(self.look_pitch - dy * 0.004, -1.0, 1.2);
        } else {
            self.orbit_yaw += dx * 0.005;
            self.orbit_pitch = clamp(self.orbit_pitch + dy * 0.005, -0.4, 1.4);
        }
    }

    /// The mouse wheel turned `lines` (positive up; any fraction), or + and − pressed (one line):
    /// outside, the camera comes closer or goes back, a tenth of the distance a line, between 5 and
    /// 150 m; in the cockpit the view is magnified `ZOOM_STEP` a line, about where it looks (the
    /// eye stays in its spot and the head as it is turned), within `ZOOM_RANGE`. Neither jumps:
    /// the picture eases there (`ease_zoom`), and stays there.
    pub fn zoom(&mut self, lines: f64) {
        if !self.inside() {
            let scale = drone::chase_scale(self.craft());
            self.orbit_distance = clamp(self.orbit_distance * 0.9f64.powf(lines), 5.0 * scale, 150.0 * scale);
            return;
        }
        self.zoom = clamp(self.zoom * ZOOM_STEP.powf(lines), ZOOM_RANGE.0, ZOOM_RANGE.1);
    }

    /// `dt` seconds on: the cockpit's zoom and the outside camera's distance go `1 − e^(−dt/ZOOM_LAG)`
    /// of the way (as ratios) to where the wheel left them, the same at any frame rate.
    fn ease_zoom(&mut self, dt: f64) {
        let k = 1.0 - (-dt.max(0.0) / ZOOM_LAG).exp();
        self.zoom_shown *= (self.zoom / self.zoom_shown).powf(k);
        self.orbit_shown *= (self.orbit_distance / self.orbit_shown).powf(k);
    }

    /// The zoom and the outside camera where the wheel left them, at once: for a screenshot.
    pub fn settle_zoom(&mut self) {
        (self.zoom_shown, self.orbit_shown) = (self.zoom, self.orbit_distance);
    }

    /// Space (a stick's fourth button): the seat and leaning in, in turn ("pilot" and "pilot,
    /// leaning in" in `kinds::spots`); from any other spot back to the seat; outside, into the
    /// cockpit in the seat.
    pub fn look_at_panel(&mut self) {
        if !self.inside() {
            if self.player_wreck().is_some() {
                return;
            }
            self.cockpit = true;
            self.watched = 0;
            (self.look_yaw, self.look_pitch) = (0.0, 0.0);
            self.set_spot(0);
            self.camera = self.spot_target();
            return;
        }
        let leaning = self.spots().iter().position(|s| s.name == "pilot, leaning in").unwrap_or(0);
        self.set_spot(if self.spot == 0 { leaning } else { 0 });
    }

    /// The eye goes to spot `i` of the cockpit (the last if there are fewer), the head ahead again;
    /// the status line says where.
    fn set_spot(&mut self, i: usize) {
        let last = self.spots().len() - 1;
        if i.min(last) != self.spot {
            self.spot = i.min(last);
            // Another spot is seen as it is.
            self.zoom = 1.0;
            self.notice = format!("View: {}", self.spot_target().name);
            self.notice_until = self.clock + 1.5;
        }
        if !self.looking {
            (self.look_yaw, self.look_pitch) = (0.0, 0.0);
        }
    }

    /// The spot the eye goes to.
    fn spot_target(&self) -> Spot {
        let spots = self.spots();
        spots[self.spot.min(spots.len() - 1)]
    }

    /// Turns the DIMMING knobs by `step` (of their whole turn): the panel's lights and the standby
    /// instruments' together.
    pub fn dim_panel(&mut self, step: f32) {
        self.panel_lights = (self.panel_lights + step).clamp(0.0, 1.0);
        self.standby_lights = self.panel_lights;
        self.notice = format!("Panel lights: {:.0} %", self.panel_lights * 100.0);
        self.notice_until = self.clock + 2.0;
    }

    /// Turns the head to `yaw` right and `pitch` up, radians, and holds it there as the mouse does.
    pub fn hold_look(&mut self, yaw: f64, pitch: f64) {
        self.looking = true;
        self.look_yaw = clamp(yaw, -2.6, 2.6);
        self.look_pitch = clamp(pitch, -1.0, 1.2);
    }

    /// The mouse takes or lets go of the view. In the cockpit letting go looks ahead again.
    pub fn set_looking(&mut self, on: bool) {
        // Letting go leaves the head where the mouse put it; 0 looks ahead again.
        self.looking = on;
        if on {
            self.look_keys = false;
        }
    }

    /// The runway the player's aircraft needs, m: 0 for any.
    fn runway_needed(&self) -> f64 {
        self.aircraft().map_or(0.0, |a| a.p.runway)
    }

    /// The airport a restart uses, and whether the wind calls for the far end of its runway.
    fn restart_airport(&self) -> Option<(Airport, bool)> {
        let plane = self.vehicle();
        let s = plane.body();
        // The nearest with a runway as long as the aircraft needs (a transport's).
        let airport = self.world.terrain.nearest_runway(s.pos.x, s.pos.y, self.runway_needed()).or(plane.home)?;
        let into = self.world.weather.wind_from * DEG;
        Some((airport, (into - airport.heading).cos() < 0.0))
    }

    pub fn restart_on_runway(&mut self) {
        let Some((airport, reversed)) = self.restart_airport() else { return };
        self.world.reset(self.player, Start::Runway { airport, reversed });
        self.let_go();
    }

    pub fn restart_on_final(&mut self) {
        let Some((airport, reversed)) = self.restart_airport() else { return };
        self.world.reset(self.player, Start::Final { airport, reversed, distance: Vehicle::FINAL_DISTANCE });
        self.let_go();
    }

    pub fn restart_in_air(&mut self) {
        let s = self.body();
        self.restart_over(s.pos.x, s.pos.y, euler_angles(s.q).heading);
    }

    /// On the ground right under where it is now, facing the way it faces: a multirotor's start
    /// on the ground at the current place (key 1). Over water, or nowhere near an aerodrome's
    /// flat ground, back to the runway.
    pub fn restart_here(&mut self) {
        let s = self.body();
        let ground = self.world.terrain.ground(s.pos.x, s.pos.y);
        if ground.h <= 0.3 || ground.river > 0.5 {
            self.restart_on_runway();
            return;
        }
        let (terrain, weather, earth) = (&*self.world.terrain, &self.world.weather, &self.world.earth);
        self.world.vehicles[self.player.0].reset_on_ground(terrain, weather, earth, s.pos.x, s.pos.y, euler_angles(s.q).heading);
        self.let_go();
    }

    pub fn restart_over(&mut self, n: f64, e: f64, heading: f64) {
        self.world.reset(self.player, Start::Air { n, e, heading });
        self.let_go();
    }

    /// In flight down the runway it starts on (into the wind), `back` metres before its middle and
    /// `agl` metres above it: a low pass, for a recording.
    pub fn restart_over_runway(&mut self, back: f64, agl: f64) {
        let Some((airport, reversed)) = self.restart_airport() else { return };
        let heading = airport.heading + if reversed { std::f64::consts::PI } else { 0.0 };
        self.restart_over(airport.n - heading.cos() * back, airport.e - heading.sin() * back, heading);
        let mut s = self.state();
        s.pos.z = -(airport.elevation + agl);
        self.world.place(self.player, s);
    }

    /// In flight, puts the aircraft this many metres above the ground below it, as it flies.
    pub fn set_height(&mut self, agl: f64) {
        let mut s = self.state();
        s.pos.z = -(self.world.terrain.height(s.pos.x, s.pos.y).max(0.0) + agl);
        self.world.place(self.player, s);
    }

    /// Starts as chosen in the place picker, at the place the world is round (the middle of its
    /// main runway, or the point): on a runway of its aerodrome, on final to one, or in the air
    /// over it heading into the wind.
    pub fn start_at(&mut self, start: places::Start) {
        match start {
            places::Start::Course { index } => {
                if let Err(error) = self.start_race(index) {
                    self.notice = error;
                    self.notice_until = self.clock + 4.0;
                }
            }
            places::Start::Air { feet } => {
                self.restart_over(0.0, 0.0, self.world.weather.wind_from * DEG);
                self.set_height(feet * FEET);
            }
            places::Start::Runway { heading } => {
                if let Some((airport, reversed)) = self.home_runway(heading) {
                    self.world.reset(self.player, Start::Runway { airport, reversed });
                    self.let_go();
                }
            }
            places::Start::Final { heading, miles } => {
                if let Some((airport, reversed)) = self.home_runway(heading) {
                    self.world.reset(self.player, Start::Final { airport, reversed, distance: miles * NAUTICAL_MILE });
                    self.let_go();
                }
            }
        }
    }

    /// The runway of the aerodrome at the world's origin that lies most along a true heading
    /// (degrees; `None`, the wind's), and whether that heading takes it from its far end.
    fn home_runway(&self, heading: Option<f64>) -> Option<(Airport, bool)> {
        let home = self.world.terrain.nearest_runway(0.0, 0.0, self.runway_needed())?;
        let along = heading.unwrap_or(self.world.weather.wind_from) * DEG;
        let reach = home.length + 5000.0;
        let runways = self.world.terrain.airports(home.n - reach, home.n + reach, home.e - reach, home.e + reach);
        let best = runways.into_iter().filter(|a| a.id == home.id).max_by(|a, b| (along - a.heading).cos().abs().total_cmp(&(along - b.heading).cos().abs())).unwrap_or(home);
        Some((best, (along - best.heading).cos() < 0.0))
    }

    /// After a restart: nothing engaged, the pilot's hands as new, the GFC 700 as after taxiing
    /// out (its modes off, its bug and selected altitude where the aircraft is).
    fn let_go(&mut self) {
        // The camera behind it starts behind it again, not swinging round from where it was.
        self.chase = None;
        self.autopilot = None;
        self.pilot.reset();
        self.gfc = self.aircraft().and_then(|a| a.p.afcs).map(Gfc700::new);
        self.destination = None;
        // A remote pilot holds the controls of this start, not of the flight before.
        let aircraft = self.world.vehicle(self.player).aircraft();
        self.remote.restarted(aircraft);
    }

    // ---- Time

    /// How many times faster than real time the world is to go (`TIME_RATES`).
    pub fn time_rate(&self) -> u32 {
        self.time_rate
    }

    /// How many times faster than real time the world has gone lately (`achieved_rate`).
    pub fn achieved_rate(&self) -> f64 {
        self.achieved_rate
    }

    /// Sets how many times faster than real time the world goes, the nearest of `TIME_RATES` at
    /// or below `rate`.
    pub fn set_time_rate(&mut self, rate: u32) {
        let rate = TIME_RATES.into_iter().rev().find(|r| *r <= rate).unwrap_or(1);
        if rate != self.time_rate {
            self.time_rate = rate;
            // The rate shown starts from the one chosen, not from the last one's.
            self.achieved_rate = rate as f64;
        }
    }

    /// F10 and F9: the next rate of `TIME_RATES` up or down. Not while paused, in a replay (it has
    /// its own speeds) or with the player's craft in pieces.
    fn step_time_rate(&mut self, up: bool) {
        if self.paused || self.playback.is_some() || self.vehicle().dynamics.crashed().is_some() {
            return;
        }
        let at = TIME_RATES.iter().position(|r| *r == self.time_rate).unwrap_or(0);
        let next = if up { (at + 1).min(TIME_RATES.len() - 1) } else { at.saturating_sub(1) };
        self.set_time_rate(TIME_RATES[next]);
    }

    /// The tag at the top right while time goes faster: `×4`, and how fast it really went when the
    /// computer cannot keep up (`×16 → ×9.5`). Empty at real time.
    pub fn time_text(&self) -> String {
        if self.time_rate == 1 {
            return String::new();
        }
        let rate = self.time_rate as f64;
        if self.achieved_rate < rate * 0.92 {
            format!("×{} → ×{:.1}", self.time_rate, self.achieved_rate)
        } else {
            format!("×{}", self.time_rate)
        }
    }

    /// How many times faster than real time the picture's own motion goes (the propellers, the
    /// wheels): as the world went, 1 at real time.
    pub fn time_scale(&self) -> f64 {
        if self.time_rate == 1 { 1.0 } else { self.achieved_rate }
    }

    /// Advances by `dt` seconds of real time with these keys held; the outside camera follows.
    pub fn advance(&mut self, dt: f64, held: &Held) {
        self.pass(dt, held);
        self.follow(dt);
    }

    /// The outside camera's heading after the craft watched turns, behind it by `drone::chase_lag`:
    /// each frame it goes `1 − e^(−dt/lag)` of the way (the shorter way round), so it is the same
    /// at any frame rate. On another craft, or one that has jumped (a restart, a new place, a
    /// replay), it is straight behind it at once.
    fn follow(&mut self, dt: f64) {
        let id = self.watched();
        let vehicle = self.world.vehicle(id);
        let s = vehicle.body();
        let lag = drone::chase_lag(Craft::of(vehicle));
        let target = chase_heading(s.q);
        self.chase = match self.chase {
            // Faster than anything flies: it was moved.
            Some(c) if c.of == id && (s.pos - c.at).length() < 20.0 + 150.0 * dt => {
                let k = 1.0 - (-dt.max(0.0) / lag).exp();
                let heading = target.map_or(c.heading, |t| wrapped(c.heading + wrapped(t - c.heading) * k));
                Some(Chase { heading, at: s.pos, ..c })
            }
            _ => Some(Chase { of: id, heading: target.unwrap_or_else(|| euler_angles(s.q).heading), at: s.pos }),
        };
    }

    /// The world and what moves with it by `dt` seconds of real time (`advance`).
    fn pass(&mut self, dt: f64, held: &Held) {
        // A paused game has let go of every key but the clock's.
        let held = if self.paused { Held { time: held.time, shift: held.shift, ..Held::default() } } else { *held };
        let held = Held { look: self.pads.look(held.look, self.paused), ..held };
        self.recorder.window = replay_window(&self.store);
        // Fuel is used up only if the player chose so in the settings (off by default).
        let burns = self.store.flag(settings::FUEL_BURNS).unwrap_or(false);
        if let Some(a) = self.world.vehicle_mut(self.player).aircraft_mut() {
            a.burns_fuel = burns;
        }

        self.knobs.advance(dt);
        self.ease_zoom(dt);
        // The eye moves to the spot chosen, two thirds of the way in a sixth of a second.
        let target = self.spot_target();
        let k = 1.0 - (-dt as f32 * 7.0).exp();
        self.camera = Spot {
            eye: self.camera.eye.lerp(target.eye, k),
            yaw: self.camera.yaw + (target.yaw - self.camera.yaw) * k,
            pitch: self.camera.pitch + (target.pitch - self.camera.pitch) * k,
            fov: self.camera.fov + (target.fov - self.camera.fov) * k,
            shift: self.camera.shift + (target.shift - self.camera.shift) * k,
            ..target
        };
        // Z and X turn the head (or swing the outside camera); letting go looks ahead again.
        if self.cockpit {
            if held.look != 0.0 {
                self.look_keys = true;
            }
            if !self.looking && self.look_keys {
                self.look_yaw += clamp(held.look * 110.0 * DEG - self.look_yaw, -3.5 * dt, 3.5 * dt);
                self.look_keys = held.look != 0.0 || self.look_yaw != 0.0;
            }
        } else {
            self.orbit_yaw += held.look * 1.6 * dt;
        }
        if let Some(playback) = &mut self.playback {
            // A replay: nobody flies and the world stands still; the moment shown moves on.
            if let Some(span) = self.recorder.span() {
                if !self.paused {
                    playback.advance(dt, span);
                }
                let live = &playback.vehicles;
                self.recorder.show(playback.time, &mut self.world, self.player, &mut self.gfc, live);
            }
            return;
        }
        if held.time {
            let direction = if held.shift { -1.0 } else { 1.0 };
            self.world.hour = (self.world.hour + direction * dt * 1.5 + 24.0) % 24.0;
        }
        if self.paused || self.frozen || self.waiting_for_place {
            return;
        }
        // After a crash the world goes on (the wreck tumbles and burns, the others fly), but
        // nobody flies the player's aircraft.
        let crashed = self.vehicle().dynamics.crashed().is_some();
        if crashed {
            self.autopilot = None;
        }

        let mut engaged = false;
        let holding = self.race.as_ref().is_some_and(|r| r.holding(self.world.time()));
        if let Some(m) = self.world.vehicle_mut(self.player).multirotor_mut() {
            // A multirotor: the sticks, from the keys and the controllers, straight to its radio.
            if !crashed {
                drone::sticks(&mut m.controls, &held, &mut self.pads, dt);
            }
            // A race's countdown holds the sticks, the throttle down, until GO.
            if holding {
                m.controls = sim_core::MultirotorControls { arm: m.controls.arm, ..Default::default() };
            }
        } else {
            let trim_before = self.aircraft().map_or(0.0, |a| a.controls.trim);
            // Throttle and trim go straight to the aircraft; the pilot moves yoke and pedals each step.
            // With the autopilot engaged the throttle is its own: W and S move the airspeed it holds.
            let assist = self.pilot.assist;
            // A remote pilot has every control: the keys and the controllers fly nothing.
            let remote = self.remote.flying();
            if crashed || remote {
                // Nothing: the controls stay as they were (the remote's hands set them each step).
            } else if let (Some(_), Some(kind)) = (&self.autopilot, self.aircraft_kind()) {
                let faster = (held.throttle_up as i32 - held.throttle_down as i32) as f64;
                let (slowest, fastest) = kinds::autopilot_speeds(kind);
                self.change_hold(|_, _, airspeed| *airspeed = clamp(*airspeed + faster * AIRSPEED_RATE * dt, slowest, fastest));
            } else if let Some(c) = self.controls() {
                engine_keys(c, &held, assist, dt);
            }
            self.pilot.roll = held.roll;
            self.pilot.pitch = held.pitch;
            self.pilot.pedals = held.pedals;
            self.pilot.fine = held.fine;
            let airspeed = self.aircraft().map_or(0.0, |a| a.indicated_airspeed()) / KNOTS;
            if let (false, Some(a)) = (crashed || remote, self.world.vehicle_mut(self.player).aircraft_mut()) {
                self.pads.fly(&mut self.pilot, &mut a.controls, airspeed, dt);
            }
            // The pilot's hands against the GFC 700's servos: the manual trim, or a firm hand on the yoke
            // (a stick moved well away from where the servos hold it), disconnect it. The arrows do
            // not: with it engaged they are its targets' (`afcs_arrow`).
            engaged = self.gfc.as_ref().is_some_and(Gfc700::engaged);
            if let (Some(gfc), Some(c)) = (self.gfc.as_mut().filter(|g| g.engaged()), self.world.vehicle(self.player).aircraft().map(|a| a.controls)) {
                if c.trim != trim_before {
                    gfc.manual_trim();
                }
                let stick = self.pilot.mouse_yoke.map_or(0.0, |(x, y)| (x - c.aileron).abs().max((y - c.elevator).abs()));
                gfc.push_yoke(stick, dt);
            }
            // While the servos fly, the keyboard's pilot (whose yoke they override) starts afresh each
            // frame from the attitude there is, so it takes over from there when they let go.
            if engaged {
                self.pilot.reset();
            }
        }

        // Time goes faster (F9 F10) by more steps of the same length, never by longer ones: the
        // physics is as accurate at ×16 as at ×1. The steps real time asks for are always taken;
        // the ones beyond it only while they fit in `step_budget`, the rest let go of (the world
        // then goes slower than chosen, and the tag says how fast it went).
        let real = dt.clamp(0.0, 0.1);
        self.clock += real;
        self.accumulator += real * self.time_rate as f64;
        // The steps real time asks for: as many as at ×1, the fraction of one carried on.
        self.owed += real;
        let owed = (self.owed / World::DT).floor();
        self.owed -= owed * World::DT;
        let owed = owed as usize;
        let started = engine::Instant::now();
        let touchdown = self.aircraft().and_then(Aircraft::last_touchdown);
        let mut steps = 0usize;
        while self.accumulator >= World::DT {
            if self.time_rate > 1 && steps >= owed && started.elapsed() >= self.step_budget {
                self.accumulator %= World::DT;
                break;
            }
            for flown in &mut self.planes {
                match (&mut flown.flier, &mut self.autopilot) {
                    // A multirotor's radio was set for the frame: nobody else flies it, but the
                    // race's autopilot when it races.
                    (Flier::Player, _) if self.world.vehicle(flown.id).multirotor().is_some() => {
                        if let Some(racer) = self.race.as_mut().and_then(|r| r.racer.as_mut()) {
                            self.world.fly(flown.id, racer.as_mut());
                        }
                    }
                    (Flier::Player, _) if self.world.vehicle(flown.id).dynamics.crashed().is_some() => {
                        // Nobody flies it; the GFC 700 still sees it, and lets go.
                        if let (Some(gfc), Some(a)) = (&mut self.gfc, self.world.vehicle(flown.id).aircraft()) {
                            let c = a.controls;
                            gfc.fly_aircraft(&self.world.observe(flown.id), &c, World::DT);
                        }
                    }
                    (Flier::Player, autopilot) => {
                        let hands: &mut dyn Pilot = match (&mut self.remote.hands, autopilot) {
                            (Some(remote), _) => remote,
                            (None, Some(autopilot)) => autopilot,
                            (None, None) => &mut self.pilot,
                        };
                        match &mut self.gfc {
                            Some(gfc) => self.world.fly(flown.id, &mut Hands { hands, gfc }),
                            None => self.world.fly(flown.id, hands),
                        }
                    }
                    (Flier::Traffic(autopilot), _) => self.world.fly(flown.id, autopilot.as_mut()),
                    (Flier::Model(model), _) => model.fly(&mut self.world, flown.id),
                    (Flier::Rival, _) => {
                        if let Some(race) = &mut self.race {
                            race.fly_rival(&mut self.world, flown.id);
                        }
                    }
                }
            }
            self.world.step();
            for flown in &mut self.planes {
                if let Flier::Model(model) = &mut flown.flier {
                    model.after_step(&mut self.world, flown.id);
                }
            }
            if let Some(race) = &mut self.race {
                race.step(&self.world, self.player, &mut self.store);
            }
            self.recorder.record(&self.world, self.player, self.gfc.as_ref());
            self.accumulator -= World::DT;
            steps += 1;
            // A crash or a touchdown is lived at real time: the rest of the frame's steps beyond
            // it are let go of.
            if self.time_rate > 1 && (self.vehicle().dynamics.crashed().is_some() || self.aircraft().and_then(Aircraft::last_touchdown) != touchdown) {
                self.set_time_rate(1);
                self.accumulator %= World::DT;
            }
        }
        // How fast the world went, smoothed over about a second (`time_text`).
        if real > 0.0 {
            let went = steps as f64 * World::DT / real;
            self.achieved_rate += (went - self.achieved_rate) * (1.0 - (-real / RATE_SMOOTHING).exp());
        }
        // The GFC 700 let go: the keyboard has the aircraft, holding the attitude it is handed.
        if engaged && !self.gfc.as_ref().is_some_and(Gfc700::engaged) {
            self.pilot.reset();
        }
        // Landed and stopped: the aircraft is the player's again.
        if self.autopilot.as_ref().is_some_and(|a| a.finished() && a.phase() == Phase::Stopped) {
            self.disengage();
        }
        let Some(a) = self.world.vehicle(self.player).aircraft() else { return };
        // The last half minute of the flight, for the means a remote pilot reads.
        self.remote.record(self.world.time(), a);
        let damage = *a.damage();
        if damage.events != self.damage_seen {
            if damage.events > self.damage_seen {
                self.damage_text = damage.last;
                self.damage_until = self.clock + 5.0;
            }
            self.damage_seen = damage.events;
        }
        if let Some(t) = self.aircraft().and_then(Aircraft::last_touchdown) {
            if Some(t) != self.touchdown_shown {
                self.touchdown_shown = Some(t);
                self.touchdown_until = self.clock + 6.0;
            }
        }
    }

    /// Every craft of the world the game flies, the player's first.
    pub fn craft_ids(&self) -> Vec<VehicleId> {
        self.planes.iter().map(|flown| flown.id).collect()
    }

    /// A recording's frames fall between the simulation's steps (more so in slow motion): this
    /// moves every multirotor on along its velocity and its turning by the time since its last
    /// step, to be drawn there, and gives what `settle` puts back before the next step. The
    /// simulation itself never sees the difference.
    pub fn lead(&mut self) -> Vec<(VehicleId, Body)> {
        let ahead = self.accumulator;
        let ids = self.craft_ids();
        let mut saved = Vec::new();
        for id in ids {
            let Some(m) = self.world.vehicle_mut(id).multirotor_mut() else { continue };
            saved.push((id, m.body));
            let turn = m.body.omega.length();
            m.body.pos += m.body.world_velocity() * ahead;
            if turn > 1e-9 {
                m.body.q = (m.body.q * axis_angle(turn * ahead, m.body.omega / turn)).normalize();
            }
        }
        saved
    }

    /// Puts back what `lead` moved.
    pub fn settle(&mut self, saved: Vec<(VehicleId, Body)>) {
        for (id, body) in saved {
            if let Some(m) = self.world.vehicle_mut(id).multirotor_mut() {
                m.body = body;
            }
        }
    }

    // ---- Camera

    /// Where the picture is taken from: the pilot's eyes, or 17 m behind the aircraft.
    pub fn view(&self) -> ViewPoint {
        if let Some(view) = self.view_override {
            return view;
        }
        let s = self.body();
        // In pieces: the camera stands back from the wreck, whichever view was chosen.
        if let Some(wreck) = self.player_wreck() {
            let h = self.orbit_yaw + 0.6;
            return self.looking_at(wreck.main().pos, h, self.orbit_pitch.max(0.3), self.orbit_shown * 30.0 / ORBIT_DISTANCE, 1.0);
        }
        if self.cockpit {
            // A rival's quad: its FPV camera, looking ahead.
            let (s, c, look_yaw, look_pitch, zoom) = match self.viewed() {
                0 => (s, self.camera, self.look_yaw, self.look_pitch, self.zoom_shown as f32),
                k => {
                    let other = self.world.vehicle(self.planes[k].id);
                    (other.body(), drone::spots(Craft::of(other))[0], 0.0, 0.0, 1.0)
                }
            };
            let head = s.q * axis_angle(c.yaw as f64 + look_yaw, V3::Z) * axis_angle(c.pitch as f64 + look_pitch, V3::Y);
            let eye = c.eye;
            ViewPoint {
                position: s.pos + act(s.q, V3::new(eye.x as f64, eye.y as f64, eye.z as f64)),
                forward: to_render(act(head, V3::X)),
                up: to_render(act(head, V3::NEG_Z)),
                right: to_render(act(head, V3::Y)),
                // The wheel's zoom: what is seen that many times larger.
                fov_y: 2.0 * ((c.fov / 2.0).tan() / zoom).atan(),
                shift_y: c.shift,
                // Near the walls beside the head.
                near: 0.05,
            }
        } else {
            let id = self.watched();
            let watched = self.world.vehicle(id);
            let s = watched.body();
            let scale = drone::chase_scale(Craft::of(watched));
            // Behind it as the camera has followed its turns (`follow`); before that, behind it.
            let heading = match self.chase {
                Some(c) if c.of == id => c.heading,
                _ => chase_heading(s.q).unwrap_or_else(|| euler_angles(s.q).heading),
            };
            self.looking_at(s.pos, heading + self.orbit_yaw, self.orbit_pitch, self.orbit_shown, scale)
        }
    }

    /// The outside camera: `distance` metres from `target`, looking at it along `heading`, from
    /// `pitch` above the horizon, never below the ground; `scale` is the size of what it looks at
    /// against an aircraft's (`multirotor::chase_scale`): how far above the ground it keeps and
    /// above the target it aims.
    fn looking_at(&self, target: V3, heading: f64, pitch: f64, distance: f64, scale: f64) -> ViewPoint {
        let dir = V3::new(heading.cos() * pitch.cos(), heading.sin() * pitch.cos(), pitch.sin());
        let mut pos = target - dir * distance;
        let floor = self.world.terrain.height(pos.x, pos.y).max(0.0) + 1.5 * scale;
        if -pos.z < floor {
            pos.z = -floor;
        }
        let forward = (target + V3::new(0.0, 0.0, -0.6 * scale) - pos).normalize();
        let right = V3::Z.cross(forward).normalize();
        ViewPoint { position: pos, forward: to_render(forward), up: to_render(right.cross(forward)), right: to_render(right), fov_y: 62f32.to_radians(), shift_y: 0.0, near: (0.25 * scale).max(0.02) as f32 }
    }

    /// Whether the picture is from the cockpit: not once the aircraft is in pieces.
    pub fn inside(&self) -> bool {
        self.cockpit && self.player_wreck().is_none()
    }

    /// The wreck of the player's aircraft, once it has broken up: the one nearest to where it was.
    pub fn player_wreck(&self) -> Option<&Wreck> {
        // A multirotor stays whole: it is its own wreck.
        let a = self.aircraft()?;
        if !a.damage().broken_up {
            return None;
        }
        let at = a.state.pos;
        self.world.wrecks.iter().filter(|w| w.pieces.len() > 1).min_by(|x, y| (x.main().pos - at).length().total_cmp(&(y.main().pos - at).length()))
    }

    // ---- Text and instruments

    /// The lines at the top left: wind, airfield, and what is going on. The instruments are on the
    /// dials, not here. Call once per frame.
    pub fn status_text(&mut self) -> String {
        let s = self.body();
        self.nearest_refresh -= 1;
        if self.nearest_refresh <= 0 {
            self.nearest_refresh = 90;
            self.nearest = self.world.terrain.nearest_airport(s.pos.x, s.pos.y);
        }
        let w = self.world.weather;
        let mut line1 = format!("Wind {:03.0}° {:.0} kt", w.wind_from, w.wind_speed / KNOTS);
        if let Some(n) = &self.nearest {
            let (dn, de) = (n.n - s.pos.x, n.e - s.pos.y);
            let mut bearing = de.atan2(dn) / DEG;
            if bearing < 0.0 {
                bearing += 360.0;
            }
            line1 += &format!(
                " · Aerodrome: {:.1} NM at {:03.0}°, runway {:02}/{:02}, {:.0} ft, QNH {:.0} hPa",
                dn.hypot(de) / 1852.0,
                bearing,
                n.number(),
                n.number() + 18,
                n.elevation / FEET,
                (w.air.qnh(n.elevation) / HPA).floor()
            );
        }
        line1 += &self.pads.status();
        if self.remote.flying() {
            line1 += " · Flown by remote control";
        }
        let instruments = instrument_keys::status(self.vehicle());
        if !instruments.is_empty() {
            line1 += "\n";
            line1 += &instruments;
        }
        if self.planes.len() > 1 {
            line1 += "\n";
            line1 += &self.camera_text();
        }
        line1
    }

    pub fn help_text(&self) -> String {
        if self.show_help {
            format!("{HELP}\n\nH: close the help")
        } else {
            String::new()
        }
    }

    /// The message in the middle of the screen, if there is anything to say.
    pub fn message_text(&self) -> String {
        let plane = self.vehicle();
        if self.playback.is_some() {
            return plane.dynamics.crashed().map(str::to_string).unwrap_or_default();
        }
        if let (Some(reason), Some(_)) = (plane.dynamics.crashed(), &self.race) {
            return format!("{reason}\n\n1: race again");
        }
        if let Some(reason) = plane.dynamics.crashed() {
            return match plane.multirotor() {
                Some(_) => format!("{reason}\n\n1: on the ground here · 2: in the air here"),
                None => format!("{reason}\n\n1: back to the runway · 2: start in flight · 3: on final"),
            };
        }
        if self.paused {
            return "PAUSED\nPress a key to go on".to_string();
        }
        let time = self.clock;
        if time < self.assist_shown_until {
            return if self.pilot.assist {
                "Assist on: the arrows move the nose and the wings, and they stay"
            } else {
                "Assist off: the arrows move the controls directly"
            }
            .to_string();
        }
        if time < self.damage_until {
            return self.damage_text.to_string();
        }
        if time < self.notice_until {
            return self.notice.clone();
        }
        let Some(a) = plane.aircraft() else { return drone::message(plane.multirotor()) };
        let (d, p) = (a.data(), &a.p);
        let limit = if a.flap_deg > 1.0 { p.load_limit_flaps } else { p.load_limit };
        if d.aero_load > limit || d.aero_load < p.load_limit_negative {
            return format!("OVERLOAD: {:.1} g", d.aero_load);
        }
        if d.overspeed {
            return format!("OVERSPEED: {:.0} kt (VNE {:.0} kt)", a.indicated_airspeed() / KNOTS, p.vne / KNOTS);
        }
        if d.flap_overspeed {
            let vfe = if a.flap_deg <= 10.5 { p.vfe[0] } else { p.vfe[1] };
            return format!("Too fast with flaps: {:.0} kt (maximum {:.0} kt)", a.indicated_airspeed() / KNOTS, vfe / KNOTS);
        }
        match self.touchdown_shown {
            Some(t) if time < self.touchdown_until && a.data().on_ground() => format!("Touchdown at {:.0} ft/min · F2: replay", t / FEET * 60.0),
            _ => String::new(),
        }
    }

    /// What the instrument panel shows: what its pneumatic, vacuum and magnetic instruments read.
    pub fn panel_data(&self) -> PanelData {
        let plane = self.vehicle();
        // A multirotor's dials, for the one that has a cockpit: what it is doing, as it is.
        let Some(a) = plane.aircraft() else { return plane.multirotor().map(drone::panel).unwrap_or_default() };
        let r = a.instruments.readings();
        // Every type has an attitude indicator; without one the AHRS would stand in.
        let [pitch, roll] = r.attitude.unwrap_or([r.ahrs[0], r.ahrs[1]]);
        PanelData {
            airspeed_kt: (r.airspeed / KNOTS) as f32,
            altitude_ft: (r.altitude / FEET) as f32,
            vertical_speed_fpm: (r.vertical_speed / FEET * 60.0) as f32,
            pitch: pitch as f32,
            roll: roll as f32,
            heading: r.heading_indicator.map(|h| h as f32),
            compass: r.compass as f32,
            altimeter_hpa: (r.altimeter_setting / HPA) as f32,
            suction_inhg: r.suction.map(|s| (s / INHG) as f32),
            turn_rate: a.turn_rate() as f32,
            slip: a.slip_ball() as f32,
            rpm: a.rpm() as f32,
            flap_deg: a.flap_deg as f32,
            trim: a.controls.trim as f32,
            throttle: a.controls.throttle as f32,
            stall_warning: a.data().stall_warning,
            airframe: kinds::panel(a.p.kind),
            load_factor: a.data().load_factor() as f32,
            g_felt: (a.damage().least_g as f32, a.damage().most_g as f32),
        }
    }

    /// For a screenshot of a wreck: the player's aircraft dives into the ground 40 m below at
    /// 90 kt, 35° nose down, and the world runs on until `seconds` after it breaks up.
    pub fn crash_ahead(&mut self, seconds: f64) {
        self.dive(40.0, 35.0, 90.0);
        let mut after = None;
        for _ in 0..240 * 120 {
            self.world.step();
            let broken = match self.aircraft() {
                Some(a) => a.damage().broken_up,
                None => self.vehicle().dynamics.crashed().is_some(),
            };
            if after.is_none() && broken {
                after = Some(self.world.time());
            }
            if after.is_some_and(|t| self.world.time() - t >= seconds) {
                break;
            }
        }
    }

    /// The player's aircraft diving at the ground along its heading: `agl` metres above it, the nose
    /// `pitch` degrees down, at `knots`.
    pub fn dive(&mut self, agl: f64, pitch: f64, knots: f64) {
        let mut s = self.state();
        let heading = euler_angles(s.q).heading;
        let ground = self.world.terrain.height(s.pos.x, s.pos.y).max(0.0);
        s.pos.z = -(ground + agl);
        s.q = sim_core::quaternion(heading, -pitch * DEG, 0.0);
        s.vel = V3::new(knots * KNOTS, 0.0, 0.0);
        s.omega = V3::ZERO;
        self.world.place(self.player, s);
    }

    /// For a screenshot of damage that is not a wreck (`Damaged`): the player's aircraft is put
    /// where it gives way, and the world runs on until it has and a moment more.
    pub fn damage_ahead(&mut self, what: Damaged) {
        if let (Damaged::Props, Some(m)) = (what, self.world.vehicle_mut(self.player).multirotor_mut()) {
            // The front left (fourth) struck at 5 m/s, the front right (second) brushed 4 mm deep.
            m.strike_propeller(3, 0.03, 5.0);
            m.strike_propeller(1, 0.004, 0.5);
            m.controls.arm = false;
            for _ in 0..240 {
                self.world.step();
            }
            return;
        }
        // What breaks is an aircraft's.
        let Some(a) = self.aircraft() else { return };
        let (s, p) = (a.state, a.p.clone());
        let heading = euler_angles(s.q).heading;
        let ground = self.world.terrain.height(s.pos.x, s.pos.y).max(0.0);
        let Some(a) = self.world.vehicle_mut(self.player).aircraft_mut() else { return };
        let mut seconds = 3.0;
        match what {
            Damaged::Tip => {
                // Banked 35°, the left tip just over the ground, coming down onto it at 4 m/s.
                a.state.q = sim_core::quaternion(heading, p.ground_pitch, -35.0 * DEG);
                a.state.pos.z = -ground - 0.03 - act(a.state.q, p.hard_points[0] - a.cg()).z;
                a.state.vel = act(sim_core::inverse(a.state.q), act(sim_core::quaternion(heading, 0.0, 0.0), V3::new(20.0, 0.0, 4.0)));
            }
            Damaged::Gear => {
                // A level touchdown sinking at 4.5 m/s: the main legs collapse.
                a.state.q = sim_core::quaternion(heading, p.ground_pitch, 0.0);
                a.state.pos.z = -(ground + p.cg_height + 0.05);
                a.state.vel = act(sim_core::inverse(a.state.q), act(sim_core::quaternion(heading, 0.0, 0.0), V3::new(25.0, 0.0, 4.5)));
            }
            Damaged::Wing => {
                // In flight: the outer left wing and half the right tailplane torn off (a tree's
                // top, a mid-air brush), a second before the picture.
                a.tear_wing(0, p.strip_edges[2], 1.0, "Left wing broken off");
                let span = p.pieces.stabiliser;
                a.tear_stabiliser(1, (span.root + span.tip) / 2.0, 1.0, "Right stabiliser broken off");
                seconds = 1.0;
            }
            Damaged::Props => return,
            Damaged::Flutter => {
                // Diving at full power far past the never-exceed speed until a surface flutters away.
                a.state.q = sim_core::quaternion(heading, -30.0 * DEG, 0.0);
                a.state.vel = V3::new(1.08 * p.flutter_speed, 0.0, 0.0);
                a.controls.throttle = 1.0;
                seconds = 0.5;
            }
        }
        let Some(events) = self.aircraft().map(|a| a.damage().events) else { return };
        let mut after = None;
        for _ in 0..240 * 60 {
            self.world.step();
            let Some(damage) = self.aircraft().map(Aircraft::damage) else { return };
            let done = match what {
                Damaged::Flutter => damage.ailerons.contains(&sim_core::SurfaceState::Gone),
                _ => damage.events > events || what == Damaged::Wing,
            };
            if after.is_none() && done {
                after = Some(self.world.time());
            }
            if after.is_some_and(|t| self.world.time() - t >= seconds) {
                break;
            }
        }
    }


    /// Where the player's aircraft is, for the log of a screenshot.
    pub fn position_line(&self) -> String {
        let plane = self.vehicle();
        let s = plane.body();
        format!("position n {:.0} e {:.0} alt {:.0} m, agl {:.1} m", s.pos.x, s.pos.y, -s.pos.z, plane.agl())
    }
}

/// What a key of the bezel is, for the tag beside it under the pointer.
fn key_name(key: AfcsKey) -> &'static str {
    match key {
        AfcsKey::Ap => "AP · autopilot",
        AfcsKey::Fd => "FD · flight director",
        AfcsKey::Hdg => "HDG · fly the heading bug",
        AfcsKey::Alt => "ALT · hold this altitude",
        AfcsKey::Nav => "NAV · follow the flight plan",
        AfcsKey::Vnv => "VNV · not fitted yet",
        AfcsKey::Apr => "APR · approach",
        AfcsKey::Bc => "BC · not fitted yet",
        AfcsKey::Vs => "VS · hold a vertical speed",
        AfcsKey::Flc => "FLC · climb or descend at this airspeed",
        AfcsKey::NoseUp => "NOSE UP",
        AfcsKey::NoseDown => "NOSE DN",
        AfcsKey::Disconnect => "AP DISC",
        AfcsKey::HeadingSync => "HDG SYNC",
    }
}

/// Why a key of the bezel did nothing, for the message in the middle of the screen.
fn refusal(input: Panel, on_ground: bool) -> &'static str {
    match input {
        Panel::Key(AfcsKey::Ap) if on_ground => "AP: does not engage on the ground",
        Panel::Key(AfcsKey::Ap) => "AP: does not engage with the aircraft banked so far",
        Panel::Key(AfcsKey::Fd) => "FD: stays on while the autopilot is engaged",
        Panel::Key(AfcsKey::Nav) => "NAV: no flight plan (D→, key /)",
        Panel::Key(AfcsKey::Apr) | Panel::DirectTo => "No aerodrome nearby",
        Panel::Key(AfcsKey::Vnv) => "VNV: needs a flight plan with altitudes, not yet",
        Panel::Key(AfcsKey::Bc) => "BC: it is for the localizer, which comes in phase 2",
        Panel::Key(AfcsKey::NoseUp | AfcsKey::NoseDown) => "UP/DN: nothing to move in this mode",
        _ => "",
    }
}

/// The GFC 700's tag at the top right: AP or FD, the lateral mode (and the one armed), the vertical
/// mode with its reference (and the one armed), a protection.
fn gfc_text(gfc: &Gfc700) -> String {
    let [lateral, lateral_armed, vertical, vertical_armed] = gfc.annunciations();
    let mut text = format!("{}  {lateral}", if gfc.engaged() { "AP" } else { "FD" });
    if lateral == "HDG" {
        text += &format!(" {:03.0}°", (gfc.heading_bug() / DEG).round().rem_euclid(360.0));
    }
    if !lateral_armed.is_empty() {
        text += &format!(" ({lateral_armed})");
    }
    text += &format!(" · {vertical}");
    match gfc.vertical() {
        VerticalMode::Altitude(a) => text += &format!(" {:.0} ft", a / FEET),
        VerticalMode::VerticalSpeed(v) => text += &format!(" {:+.0} ft/min", v / FEET * 60.0),
        VerticalMode::Flc(v) => text += &format!(" {:.0} kt", v / KNOTS),
        VerticalMode::Capture => text += &format!(" {:.0} ft", gfc.selected_altitude() / FEET),
        VerticalMode::Pitch(_) | VerticalMode::GlidePath => {}
    }
    if vertical_armed == "ALTS" {
        text += &format!(" (ALTS {:.0} ft)", gfc.selected_altitude() / FEET);
    } else if !vertical_armed.is_empty() {
        text += &format!(" ({vertical_armed})");
    }
    if let Some(protection) = gfc.protection() {
        text += &format!(" · {protection}");
    }
    text
}

/// What the player's autopilot is doing, for its tag.
fn autopilot_text(autopilot: &Autopilot) -> String {
    if let Goal::Hold { heading: Some(heading), vertical: Vertical::Altitude(altitude), airspeed: Some(airspeed) } = autopilot.goal() {
        return format!("AP  HDG {:03.0}°  ALT {:.0} ft  IAS {:.0} kt", (heading / DEG).rem_euclid(360.0), altitude / FEET, airspeed / KNOTS);
    }
    let leg = match autopilot.phase() {
        Phase::Takeoff => "takeoff",
        Phase::Climbout => "initial climb",
        Phase::Crosswind => "crosswind",
        Phase::Join => "joining the circuit",
        Phase::Orbit => "descending in circles",
        Phase::Downwind => "downwind",
        Phase::Base => "base",
        Phase::Final => "final",
        Phase::Flare => "flare",
        Phase::Rollout => "rollout",
        Phase::Stopped => "stopped",
        Phase::Hold | Phase::Route | Phase::RouteDone => "en route",
    };
    format!("AP  landing: {leg}")
}

/// A knob's key held: after `REPEAT_AFTER` seconds it goes on at `REPEAT_RATE` clicks a second.
struct Repeat {
    key: KeyCode,
    shift: bool,
    held: f64,
    clicks: u32,
}

const REPEAT_AFTER: f64 = 0.4;
const REPEAT_RATE: f64 = 12.0;

/// What the left mouse button is doing.
#[derive(Clone, Copy)]
enum Drag {
    None,
    Look,
    /// A lever of the cockpit, in 3D.
    Lever(Grip),
    /// A knob of the panel held and turned by moving the pointer (`cockpit::drag_clicks`): the
    /// knob, its control, and the part of a click not turned yet.
    Knob(PanelKnob, cockpit::PanelControl, f64),
}

/// The latitude and longitude (degrees) a real place's world is round; `None` for the seed's.
fn around(terrain: &Terrain) -> Option<(f64, f64)> {
    terrain.region().map(|r| (r.latitude.to_degrees(), r.longitude.to_degrees()))
}

fn is_modifier(key: KeyCode) -> bool {
    use KeyCode::*;
    matches!(key, ShiftLeft | ShiftRight | ControlLeft | ControlRight | AltLeft | AltRight | SuperLeft | SuperRight | CapsLock | Fn)
}

/// Frames per second and the time of a frame, averaged over half a second, for the status line (key F3).
#[derive(Default)]
struct FrameRate {
    shown: bool,
    frames: u32,
    seconds: f64,
    text: String,
}

impl FrameRate {
    fn frame(&mut self, dt: f64) {
        self.frames += 1;
        self.seconds += dt;
        if self.seconds >= 0.5 {
            self.text = format!("{:.0} fps, {:.1} ms", self.frames as f64 / self.seconds, self.seconds / self.frames as f64 * 1000.0);
            (self.frames, self.seconds) = (0, 0.0);
        }
    }

    /// The status text with the frame rate at the end of its first line.
    fn added_to(&self, status: String) -> String {
        if !self.shown || self.text.is_empty() {
            return status;
        }
        match status.split_once('\n') {
            Some((first, rest)) => format!("{first} · {}\n{rest}", self.text),
            None => format!("{status} · {}", self.text),
        }
    }
}

/// The game as the engine runs it.
pub struct Flight {
    pub session: Session,
    /// The socket other programs fly the game through (`--remote`).
    #[cfg(not(target_arch = "wasm32"))]
    remote: Option<::remote::Server>,
    renderer: Renderer,
    audio: Audio,
    drag: Drag,
    left_was_down: bool,
    right_was_down: bool,
    /// The window in logical points, as the overlay last saw it.
    screen: Option<Rect>,
    status: String,
    message: String,
    /// The quality chosen; the renderer takes it at the next `update`.
    quality: Preset,
    frame_rate: FrameRate,
    /// What the G1000 shows that changes slowly.
    instruments: Instruments,
    /// The real place flown in, as the settings name it; `None` in the seed's world.
    place: Option<String>,
    /// The place chosen in the settings, being loaded.
    loading: Option<places::Loading>,
    /// The satellites' picture of the real place's ground, being made.
    photos: Option<places::Photos>,
    /// The real place followed beyond its region as the aircraft flies on.
    following: Option<places::Following>,
    /// Back to the seed's world, chosen in the settings: done at the next `update`.
    to_generated: bool,
    /// The seed of the generated world to go to (a course's), if not the world's own.
    generated_seed: Option<u64>,
    /// What went wrong loading the last place chosen, said in the settings, and the place
    /// (`LEMH`) whose loading failed while the flight waits for the player to say what to do.
    place_error: Option<String>,
    failed: Option<String>,
    /// What the player chose on the card of the place being gone to (`ui::loading`), for the next `update`.
    card_choice: Option<waiting::Choice>,
    /// The place picker, made the first time it is opened; what was chosen in it, for the next
    /// `update`; and how to start once the place chosen has loaded.
    picker: Option<Picker>,
    /// The picker was shown in the last frame (to know when it opens again).
    picker_shown: bool,
    /// The manual (F1): its page and its pictures, once shown.
    manual: Manual,
    /// The menus over the flight (`ui::menu`): the start and pause screen, the settings, the aircraft.
    menus: Menus,
    choice: Option<places::Choice>,
    pending_start: Option<places::Start>,
    /// A knob's key held, the pointer over something of the panel it can work, and the wheel's
    /// lines not yet a click of a knob.
    repeat: Option<Repeat>,
    hover: bool,
    knob_lines: f64,
    /// The key or the knob of the panel under the pointer, and whether Shift is held.
    over: Option<Over>,
    shift: bool,
    /// The pointer as if at this point (`--hover`, for a screenshot), instead of where the mouse is.
    hover_at: Option<egui::Pos2>,
    /// Nothing over the picture but the maps' credit: for a recording.
    pub clean: bool,
    /// With `clean`, a multirotor's on-screen display all the same, as its goggles show it.
    pub clean_osd: bool,
    /// A mouse pointer drawn over the picture (a recording's hand on the panel), with its click.
    pub pointer: Option<Pointer>,
    /// What a recording's lens adds to the picture (`record.rs`); `None`: the game's own look.
    pub lens: Option<Lens>,
}

/// A recording's lens and grade over the game's picture (`engine::Post`): what each is, there.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lens {
    pub vignette: f32,
    /// The sun's flare and where the sun is in the picture.
    pub flare: f32,
    pub flare_sun: [f32; 2],
    /// The depth of field: the distance focused on (metres), how far either side of it stays
    /// sharp, the widest blur (a fraction of the picture's height; 0 none) and the view's near distance.
    pub focus: f32,
    pub focus_span: f32,
    pub focus_blur: f32,
    pub near: f32,
    /// The bloom instead of the quality's.
    pub bloom: Option<f32>,
    pub warmth: f32,
    /// Added to the game's contrast and exposure (stops); the game's saturation times this.
    pub contrast: f32,
    pub exposure: f32,
    pub saturation: f32,
}

/// A pointer drawn over a recording: where it is, in points, and the click it just made.
#[derive(Clone, Copy, Debug)]
pub struct Pointer {
    pub at: egui::Pos2,
    /// The last click: how long ago as a share of its fading (0 just now, 1 gone), and the corners of
    /// the key it pressed on the screen, if it was a key (the key lights up).
    pub click: Option<(f32, Option<[egui::Pos2; 4]>)>,
}

impl Pointer {
    /// An arrow, white edged in black, its tip at `at`; a ring spreading from a click, and the key
    /// clicked lit.
    pub fn paint(&self, painter: &egui::Painter) {
        use egui::{Color32, Shape, Stroke};
        if let Some((age, key)) = self.click {
            let fade = (1.0 - age).clamp(0.0, 1.0);
            if let Some(corners) = key {
                painter.add(Shape::convex_polygon(corners.to_vec(), Color32::from_white_alpha((110.0 * fade) as u8), Stroke::new(2.0, Color32::from_rgba_unmultiplied(120, 220, 255, (230.0 * fade) as u8))));
            }
            painter.circle_stroke(self.at, 6.0 + 26.0 * age, Stroke::new(3.0 * fade + 0.5, Color32::from_rgba_unmultiplied(255, 255, 255, (220.0 * fade) as u8)));
        }
        let k = 1.7;
        let arrow = [(0.0, 0.0), (0.0, 17.0), (4.0, 13.2), (6.9, 19.8), (9.6, 18.7), (6.8, 12.3), (12.0, 12.3)].map(|(x, y)| self.at + egui::vec2(x * k, y * k));
        let shadow: Vec<egui::Pos2> = [arrow[0], arrow[1], arrow[6]].iter().map(|p| *p + egui::vec2(2.0, 3.0)).collect();
        painter.add(Shape::convex_polygon(shadow, Color32::from_black_alpha(70), Stroke::NONE));
        let white = Color32::from_gray(250);
        painter.add(Shape::convex_polygon(vec![arrow[0], arrow[1], arrow[6]], white, Stroke::NONE));
        painter.add(Shape::convex_polygon(vec![arrow[2], arrow[3], arrow[4], arrow[5]], white, Stroke::NONE));
        painter.add(Shape::closed_line(arrow.to_vec(), Stroke::new(1.6, Color32::BLACK)));
    }
}

impl Flight {
    pub fn new(gpu: &Gpu, options: &Options, store: Store, audio: Audio) -> Flight {
        let mut session = match &options.region {
            Some(region) => Session::with_terrain(options.seed, Arc::new(Terrain::from_region(options.seed, region.clone())), store),
            None => Session::new(options.seed, store),
        };
        let mut renderer = Renderer::new(gpu, session.world.terrain.clone());
        session.store.restore(&mut Conditions {
            weather: &mut session.world.weather,
            hour: &mut session.world.hour,
            cloud_cover: &mut renderer.world.cloud_cover,
            cloud_altitude: &mut renderer.world.cloud_altitude,
            storms: &mut renderer.world.cloud_storms,
        });
        // --sky: the kind of day, for this run.
        if let Some(sky) = options.sky {
            sky.apply(
                &mut Conditions {
                    weather: &mut session.world.weather,
                    hour: &mut session.world.hour,
                    cloud_cover: &mut renderer.world.cloud_cover,
                    cloud_altitude: &mut renderer.world.cloud_altitude,
                    storms: &mut renderer.world.cloud_storms,
                },
                None,
            );
        }
        // The computer's hour, wherever the flight is (a real place far away too: the hour the
        // player lives in, not the place's); --hour still wins below.
        if let Some(hour) = options.clock.filter(|_| session.store.flag(settings::LOCAL_TIME).unwrap_or(true)) {
            session.world.hour = hour.rem_euclid(24.0);
        }
        // --fps shows the frame rate as the setting does, and is remembered with it.
        if options.fps {
            session.store.set(settings::FPS_SHOWN, true);
        }
        if let Some(craft) = options.aircraft {
            session.set_craft(craft);
        }
        // The saved wind may call for the other end of the runway.
        session.restart_on_runway();
        if let Some(hour) = options.hour {
            session.world.hour = hour;
        }
        match options.start {
            StartAt::Runway => {}
            StartAt::Air => session.restart_in_air(),
            StartAt::Final => session.restart_on_final(),
            StartAt::Position(n, e) => session.restart_over(n, e, 0.6),
        }
        if let (Some(low), StartAt::Air | StartAt::Position(..)) = (options.low, options.start) {
            session.set_height(low);
        }
        session.pads = Pads::new(Mappings::user());
        for spec in &options.models {
            if let Err(error) = session.add_model(spec) {
                eprintln!("--model {spec}: {error}");
            }
        }
        session.add_traffic(options.traffic);
        session.cockpit = !options.exterior;
        session.rival_specs = options.rivals.clone();
        if let Some(index) = options.course {
            if let Err(error) = session.start_race(index) {
                eprintln!("--course: {error}");
            }
            session.cockpit = !options.exterior;
            let (world, player) = (&session.world, session.player);
            if let (true, Some(race)) = (options.racer, &mut session.race) {
                race.toggle_racer(world, player);
            }
            session.watched = options.watch.min(session.planes.len() - 1);
        }
        if let Some((yaw, pitch)) = options.look {
            if options.exterior {
                // Outside, the camera goes round the aircraft.
                (session.orbit_yaw, session.orbit_pitch) = (yaw, clamp(pitch, -0.4, 1.4));
            } else {
                session.hold_look(yaw, pitch);
            }
        }
        session.set_spot(options.spot);
        session.camera = session.spot_target();
        if let (Some(lever), Some(m)) = (options.nacelles, session.world.vehicle_mut(session.player).multirotor_mut()) {
            m.controls.tilt = lever.clamp(0.0, 1.0);
        }
        if options.exterior && options.zoom != 0.0 {
            session.zoom(options.zoom);
        }
        session.settle_zoom();
        session.set_time_rate(options.time_rate);
        session.show_help = options.help;
        let quality = options.quality.or_else(|| session.store.word("quality").and_then(Preset::named)).unwrap_or(Preset::High);
        renderer.set_quality(gpu, quality.quality());
        let picker = options.picker.as_ref().map(|opening| {
            session.open_picker();
            let mut picker = Picker::new(around(&session.world.terrain));
            picker.open(opening, options.place.as_deref());
            picker
        });
        let mut manual = Manual::default();
        if let Some(page) = options.manual {
            session.open_manual();
            manual.turn_to(page);
        }
        // A menu at the start: the start screen in the window, any of them for a screenshot.
        let mut menus = Menus::default();
        match options.menu.as_deref() {
            Some("home") => session.open_home(),
            Some("vehicles") => session.open_vehicles(),
            Some("aircraft") => session.open_hangar(),
            Some(word) => {
                session.open_settings();
                match menu::side::Tab::named(word) {
                    Some(tab) => menus.side.turn_to(tab),
                    None if word != "settings" => eprintln!("--menu needs vehicles, home, aircraft, settings, weather, flight, graphics or about"),
                    None => {}
                }
            }
            None => {}
        }
        let photos = places::Photos::start(&session.world.terrain);
        let following = options.place.as_deref().and_then(|key| places::Following::start(key, session.world.seed()));
        Flight {
            session,
            #[cfg(not(target_arch = "wasm32"))]
            remote: options.remote.and_then(crate::remote::listen),
            renderer,
            audio,
            drag: Drag::None,
            left_was_down: false,
            right_was_down: false,
            screen: None,
            status: String::new(),
            message: String::new(),
            quality,
            frame_rate: FrameRate::default(),
            instruments: Instruments::default(),
            place: options.place.clone(),
            loading: None,
            photos,
            following,
            to_generated: false,
            generated_seed: None,
            place_error: None,
            failed: None,
            card_choice: None,
            picker_shown: picker.is_some(),
            picker,
            manual,
            menus,
            choice: None,
            pending_start: None,
            repeat: None,
            hover: false,
            knob_lines: 0.0,
            over: None,
            shift: false,
            hover_at: options.hover.map(|(x, y)| pos2(x, y)),
            clean: options.clean,
            clean_osd: false,
            lens: None,
            pointer: None,
        }
    }

    /// Goes to a real place (`LEMH`, or `lat,lon`) on its main runway once its region has loaded, as
    /// if chosen in the picker: the web's start, where nothing may wait for a place to load.
    #[cfg(target_arch = "wasm32")]
    pub fn choose_place(&mut self, key: String) {
        self.choice = Some(places::Choice { key: Some(key), start: places::Start::Runway { heading: None } });
    }

    /// Takes what was chosen in the picker: remembers it, and starts there as chosen, at once if
    /// it is the place flown now, else once it has loaded (`change_place`).
    fn take_choice(&mut self) {
        let Some(places::Choice { key, start }) = self.choice.take() else { return };
        let store = &mut self.session.store;
        store.set("place", key.as_deref().unwrap_or(""));
        if let Some(key) = &key {
            places::remember(store, key);
        }
        self.session.paused = false;
        // A course is raced in its own world: the generated one of its seed, made anew if this is another.
        let course_seed = match start {
            places::Start::Course { index } => match sim_core::course::file::courses().get(index).map(|c| &c.place) {
                Some(sim_core::course::Place::Seed(seed)) => Some(*seed),
                _ => None,
            },
            _ => None,
        };
        if let Some(seed) = course_seed.filter(|seed| self.place.is_some() || self.session.world.terrain.seed() != *seed) {
            self.generated_seed = Some(seed);
            self.pending_start = Some(start);
            self.loading = None;
            self.to_generated = true;
            return;
        }
        let here = self.loading.is_none() && key == self.place;
        if here {
            self.session.start_at(start);
            return;
        }
        self.pending_start = Some(start);
        self.place_error = None;
        self.failed = None;
        match key {
            Some(key) => {
                self.loading = places::Loading::start(&key, self.session.world.seed());
                // The flight waits where it is until the new place is there.
                self.session.waiting_for_place = self.loading.is_some();
            }
            None => self.to_generated = true,
        }
    }

    /// Stops waiting for a place: loading it is let go of (its thread finishes on its own, unheard),
    /// the flight goes on where it was, and the settings remember the place flown again.
    pub fn stay_here(&mut self) {
        self.loading = None;
        self.failed = None;
        self.place_error = None;
        self.pending_start = None;
        self.session.waiting_for_place = false;
        self.session.store.set("place", self.place.as_deref().unwrap_or(""));
    }

    /// Loads again the place whose loading failed.
    fn try_again(&mut self) {
        let Some(key) = self.failed.take() else { return };
        self.place_error = None;
        self.pending_start = self.pending_start.or(Some(places::Start::Runway { heading: None }));
        self.loading = places::Loading::start(&key, self.session.world.seed());
        self.session.store.set("place", &key);
    }

    /// Goes on with a place chosen in the settings or the picker: back to the seed's world at once,
    /// or, once the region of a real one has loaded, there. The world and its drawing are made
    /// again, and the player starts as the picker said, or on the main runway.
    fn change_place(&mut self, gpu: &Gpu) {
        let terrain = if std::mem::take(&mut self.to_generated) {
            self.loading = None;
            self.place = None;
            Arc::new(Terrain::new(self.generated_seed.take().unwrap_or(self.session.world.terrain.seed())))
        } else {
            let Some(result) = self.loading.as_ref().and_then(places::Loading::finished) else { return };
            let key = self.loading.take().map(|l| l.key);
            match result {
                Ok(terrain) => {
                    self.place = key;
                    terrain
                }
                Err(error) => {
                    // The flight keeps waiting: the player reads why and chooses (`ui::loading`).
                    self.place_error = Some(error.to_string());
                    self.failed = key;
                    return;
                }
            }
        };
        self.session.move_to(terrain.clone());
        self.session.waiting_for_place = false;
        if let Some(start) = self.pending_start.take() {
            self.session.start_at(start);
        }
        self.photos = places::Photos::start(&terrain);
        self.following = self.place.as_deref().filter(|_| terrain.region().is_some()).and_then(|key| places::Following::start(key, self.session.world.seed()));
        self.renderer.world.set_terrain(gpu, terrain);
    }

    /// Flies on the region further along the way once it has been built: the world and its drawing
    /// change their terrain under the aircraft, in the same axes (`WorldRenderer::replace_terrain`
    /// keeps what is drawn until it is made again), and its ground's picture follows.
    fn follow_place(&mut self) {
        // A replay goes back over ground already loaded.
        if self.session.replaying() {
            return;
        }
        let Some(following) = &mut self.following else { return };
        let s = self.session.body();
        let v = act(s.q, s.vel);
        let Some(terrain) = following.update(s.pos.x, s.pos.y, v.x, v.y) else { return };
        // The old region is let go of on a thread: freeing its grids takes a few milliseconds.
        let old = std::mem::replace(&mut self.session.world.terrain, terrain.clone());
        self.photos = places::Photos::start(&terrain);
        self.renderer.world.replace_terrain(terrain);
        engine::threads::spawn(move || drop(old));
    }

    /// Draws the picture of the place's ground once it has been made.
    fn take_photos(&mut self, gpu: &Gpu) {
        let Some(imagery) = self.photos.as_ref().and_then(places::Photos::finished) else { return };
        self.photos = None;
        self.renderer.world.set_imagery(gpu, imagery);
    }

    /// Terrain still being generated, a place being loaded, or the picker's map and list; 0 when
    /// the picture is complete.
    pub fn pending(&self) -> usize {
        let place = (self.loading.is_some() || self.choice.is_some() || self.photos.is_some()) as usize;
        match &self.picker {
            Some(picker) if self.session.picker_open => picker.pending(),
            _ => place + self.renderer.pending(),
        }
    }


    /// The picker, if it was opened: for `--shot` to say what it cost.
    pub fn picker(&self) -> Option<&Picker> {
        self.picker.as_ref()
    }

    /// What the game sounds like now, as the last `prepare` left it (a recording's sound is made of it).
    pub fn sound(&self) -> crate::audio::Params {
        self.audio.params()
    }

    /// Draw calls of the scene in the last frame.
    pub fn draw_calls(&self) -> usize {
        self.renderer.draw_calls()
    }

    /// What goes on behind the frames: a place loading or chosen, its satellites' picture, the
    /// place followed as the aircraft flies on.
    pub fn background(&mut self, gpu: &Gpu) {
        match self.card_choice.take() {
            Some(waiting::Choice::Stay) => self.stay_here(),
            Some(waiting::Choice::Retry) => self.try_again(),
            Some(waiting::Choice::Another) => {
                self.stay_here();
                self.session.open_picker();
            }
            None => {}
        }
        self.take_choice();
        self.change_place(gpu);
        self.take_photos(gpu);
        self.follow_place();
    }

    /// After the world has moved on `dt` seconds: the texts, the instruments and the scene to draw.
    pub fn prepare(&mut self, gpu: &Gpu, dt: f64) {
        self.frame_rate.shown = self.session.store.flag(settings::FPS_SHOWN).unwrap_or(false);
        self.frame_rate.frame(dt);
        self.status = self.frame_rate.added_to(self.session.status_text());
        self.renderer.change_quality(gpu, self.quality.quality());
        self.message = self.session.message_text();

        let session = &self.session;
        let planes: Vec<PlaneVisual> = session
            .planes
            .iter()
            .map(|flown| {
                let visual = PlaneVisual::of(session.world.vehicle(flown.id));
                // A rival's quad in its colour.
                match session.race.as_ref().and_then(|r| r.rival(flown.id)) {
                    Some(r) => PlaneVisual { tint: crate::render::rival_tint(r.colour), ..visual },
                    None => visual,
                }
            })
            .collect();
        if let (true, Some(a)) = (session.inside(), session.aircraft()) {
            let kind = a.p.kind;
            let shown = kinds::Shown { world: &session.world, plane: Some(a), panel: session.panel_data(), gfc: session.gfc.as_ref(), rotorcraft: None };
            kinds::paint_instruments(kind, gpu, self.renderer.instruments_canvas(gpu, kind), &mut self.instruments, shown);
        } else if let (true, Some(m), Some(cabin)) = (session.inside(), session.multirotor(), kinds::cabin(session.craft())) {
            // A multirotor with a cockpit: its panel's dials, and its engines' and nacelles'.
            let shown = kinds::Shown { world: &session.world, plane: None, panel: session.panel_data(), gfc: None, rotorcraft: drone::rotorcraft(m, session.vehicle()) };
            kinds::paint_instruments_in(cabin, gpu, self.renderer.instruments_canvas_of(gpu, session.craft()), &mut self.instruments, shown);
        }
        let pads = session.race_pads();
        let scene = Scene {
            view: session.view(),
            planes: &planes,
            player: session.viewed(),
            inside_player: session.inside(),
            propeller_inside: session.store.flag(settings::PROPELLER_SHOWN).unwrap_or(true),
            player_agl: session.viewed_vehicle().agl(),
            hour: session.world.hour,
            time: session.scene_time(),
            weather: &session.world.weather,
            wrecks: &session.world.wrecks,
            dt: match &session.playback {
                _ if session.paused => 0.0,
                Some(p) if !p.playing => 0.0,
                Some(p) => (dt * p.speed) as f32,
                None => (dt * session.time_scale()) as f32,
            },
            cockpit: CockpitControls { panel_lights: session.panel_lights, standby_lights: session.standby_lights, knobs: session.knobs.shown(), yokes: session.store.flag(settings::YOKES_SHOWN).unwrap_or(true), ..cockpit::controls(session.vehicle()) },
            course: session.race.as_ref().map(|r| crate::render::CourseLook { track: &r.track, next: r.next(), lap_started: r.race.started.is_some(), pads: &pads }),
        };
        // The picker and the manual cover the whole window: the world is not drawn under them.
        if !session.covered() {
            self.renderer.prepare(gpu, &scene);
        }
        let still = session.paused || session.waiting_for_place || session.playback.as_ref().is_some_and(|p| !p.playing);
        self.audio.update(session.vehicle(), still, session.gfc.as_ref().is_some_and(Gfc700::tone));
        if let Some(race) = &mut self.session.race {
            for note in race.chimes.drain(..) {
                self.audio.chime(note);
            }
        }
    }

    fn keys(&mut self, input: &Input, dt: f64) {
        // The picker and the manual have the keyboard while they are shown (Esc closes them).
        if self.session.covered() {
            self.repeat = None;
            return;
        }
        // The card of a place being gone to has the keyboard: Esc stays here, the rest waits.
        if self.session.waiting_for_place {
            self.repeat = None;
            if input.pressed.contains(&KeyCode::Escape) {
                self.card_choice = Some(waiting::Choice::Stay);
            }
            return;
        }
        let command = input.is_down(KeyCode::SuperLeft) || input.is_down(KeyCode::SuperRight);
        for &key in &input.pressed {
            if is_modifier(key) {
                continue;
            }
            if command {
                // Shortcuts with the command key are not flying keys; ⌘, opens the settings.
                if key == KeyCode::Comma {
                    self.session.open_settings();
                }
                continue;
            }
            if key == KeyCode::F3 {
                let shown = !self.session.store.flag(settings::FPS_SHOWN).unwrap_or(false);
                self.session.store.set(settings::FPS_SHOWN, shown);
                continue;
            }
            self.session.key_pressed_with(key, input.shift);
        }
        // A knob's key held turns the knob on, as a knob turned on.
        let engaged = self.session.gfc.as_ref().is_some_and(Gfc700::engaged);
        let arrow = |key: KeyCode| engaged && matches!(key, KeyCode::ArrowLeft | KeyCode::ArrowRight | KeyCode::ArrowUp | KeyCode::ArrowDown);
        if let Some(&key) = input.pressed.iter().rev().find(|k| controls::panel_key_repeats(**k) || arrow(**k)) {
            self.repeat = Some(Repeat { key, shift: input.shift, held: 0.0, clicks: 0 });
        }
        if let Some(r) = &mut self.repeat {
            if !input.is_down(r.key) || self.session.gfc.is_none() {
                self.repeat = None;
            } else {
                r.held += dt;
                let due = ((r.held - REPEAT_AFTER) * REPEAT_RATE).max(0.0) as u32;
                let (key, shift, from) = (r.key, r.shift, std::mem::replace(&mut r.clicks, due));
                for _ in from..due {
                    self.session.key_pressed_with(key, shift);
                }
            }
        }
    }

    /// What to name on the cockpit's panel: the key or the knob under the pointer, or else the
    /// knob that was just turned (the PFD's, by a key as well), where it is on the screen. A knob's
    /// edge is lit; a key's is not (the real keys have no light: the PFD's status bar says what is on).
    fn pointed(&self, screen: Rect) -> Option<hud::Pointed> {
        let session = &self.session;
        let kind = session.aircraft_kind().filter(|_| session.inside() && !session.replaying() && !session.covered())?;
        let (over, name, hint) = match self.over {
            Some(over) => {
                let (name, hint) = session.panel_tag(over.does, self.shift);
                (over, name, hint)
            }
            None => {
                let knob = session.knobs.turning()?;
                let control = *self.renderer.panel_controls(kind).iter().filter(|c| c.has(knob)).min_by(|a, b| a.centre.y.total_cmp(&b.centre.y))?;
                (Over { control, does: Panel::Knob(knob, 1) }, session.knob_name(knob), String::new())
            }
        };
        let view = session.view();
        let place = |body: glam::Vec3| cockpit::on_screen(&view, screen, session.vehicle(), body);
        let outline = over.control.outline(over.does).into_iter().map(place).collect::<Option<Vec<egui::Pos2>>>()?;
        let whole = over.control.whole().into_iter().map(place).collect::<Option<Vec<egui::Pos2>>>()?;
        Some(hud::Pointed { lit: matches!(over.does, Panel::Knob(..)), outline, whole, at: place(over.control.centre)?, name, hint })
    }

    fn mouse(&mut self, input: &Input, dt: f64) {
        let Some(screen) = self.screen.filter(|_| !self.session.covered()) else { return };
        let session = &mut self.session;
        let at = self.hover_at.unwrap_or(pos2(input.mouse.0, input.mouse.1));
        let delta = (input.mouse_delta.0 as f64, input.mouse_delta.1 as f64);
        // In the cockpit: the key or knob of the panel under the pointer, and the lever.
        // In a replay nothing of the panel can be worked: the mouse only looks.
        let (over, grip) = match session.aircraft_kind() {
            Some(kind) if session.inside() && !input.pointer_over_ui && !session.replaying() => {
                let view = session.view();
                (cockpit::panel_at(&view, screen, session.vehicle(), self.renderer.panel_controls(kind), at), Grip::take(&view, screen, session.vehicle(), self.renderer.levers(kind), at))
            }
            _ => (None, None),
        };
        self.hover = (over.is_some() || grip.is_some()) && !matches!(self.drag, Drag::Look);
        (self.over, self.shift) = (over.filter(|_| self.hover), input.shift);
        // A knob held is the one named, wherever the pointer has gone.
        if let Drag::Knob(knob, control, _) = self.drag {
            (self.over, self.hover) = (Some(Over { control, does: Panel::Knob(knob, 1) }), true);
        }
        if input.scroll != 0.0 && !input.pointer_over_ui {
            // Over a knob the wheel turns it, a click a line; with Shift over the HDG knob or the
            // AFCS keys, the selected altitude.
            match cockpit::wheel_knob(over.map(|o| o.does), input.shift) {
                Some(knob) => {
                    self.knob_lines += input.scroll as f64;
                    let clicks = self.knob_lines.trunc();
                    self.knob_lines -= clicks;
                    if clicks != 0.0 {
                        session.panel(Panel::Knob(knob, clicks as i32));
                    }
                }
                None => session.zoom(input.scroll as f64),
            }
        }

        if input.clicked && !input.pointer_over_ui {
            if session.store.flag(settings::CONTROLS_SHOWN).unwrap_or(false) && hud::on_controls(screen, at) && !session.replaying() {
                session.pilot.centre();
                self.drag = Drag::None;
            } else if let Some(Over { control, does: Panel::Knob(knob, _) }) = over {
                // A knob is taken, and turns as the pointer moves; a click without moving turns nothing.
                self.drag = Drag::Knob(knob, control, 0.0);
            } else if let Some(over) = over {
                session.panel(over.does);
                self.drag = Drag::None;
            } else if let Some(grip) = grip {
                self.drag = Drag::Lever(grip);
            } else {
                self.drag = Drag::Look;
                session.set_looking(true);
            }
        } else if input.left_button {
            match &mut self.drag {
                Drag::Lever(grip) => session.set_lever(grip.lever, grip.value(at)),
                Drag::Look if !input.right_button => session.mouse_moved(delta.0, delta.1),
                Drag::Knob(knob, _, part) => {
                    // The screen's y grows downwards.
                    *part += cockpit::drag_clicks(input.mouse_delta.0, -input.mouse_delta.1, dt);
                    let clicks = part.trunc();
                    *part -= clicks;
                    if clicks != 0.0 {
                        session.panel(Panel::Knob(*knob, clicks as i32));
                    }
                }
                _ => {}
            }
        }
        if !input.left_button && self.left_was_down {
            self.drag = Drag::None;
            session.set_looking(false);
        }
        self.left_was_down = input.left_button;

        // The right button always looks around.
        if input.right_button && !self.right_was_down {
            session.set_looking(true);
        } else if input.right_button {
            session.mouse_moved(delta.0, delta.1);
        } else if self.right_was_down {
            session.set_looking(false);
        }
        self.right_was_down = input.right_button;
    }
}

impl Game for Flight {
    fn samples(&self) -> u32 {
        self.renderer.samples()
    }

    fn update(&mut self, input: &Input, dt: f64, gpu: &Gpu) {
        self.background(gpu);
        self.keys(input, dt);
        self.session.controllers(&input.controllers);
        self.mouse(input, dt);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(server) = &self.remote {
            crate::remote::serve(&mut self.session, server);
        }
        self.session.advance(dt, &Held::of(input));
        self.prepare(gpu, dt);
    }

    fn ui(&mut self, ctx: &egui::Context) {
        let screen = ctx.content_rect();
        self.screen = Some(screen);
        // The hand over what the mouse can press, turn or take in the cockpit, closed while it holds a
        // lever or a knob.
        if matches!(self.drag, Drag::Lever(_) | Drag::Knob(..)) {
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if self.hover {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let (clean, pointer) = (self.clean, self.pointer);
        let pointed = self.pointed(screen);
        let Flight { session, renderer, quality, place, loading, place_error, failed, card_choice, picker, picker_shown, manual, menus, choice, photos, .. } = self;
        // The menus' fonts, before anything is written this frame.
        menus.begin(ctx);
        let time = ctx.input(|i| i.time);
        // The flight goes on: no menu it waits under stays over it.
        if !session.paused && session.waiting_menu_shown() {
            session.resume();
        }
        // While the game runs no widget of the settings keeps the keyboard: otherwise the arrows,
        // which the game uses, walk the focus into the window and Space or Enter would press there.
        if !session.paused {
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
        }
        // Painted on the background layer: nothing here takes the pointer from the game.
        let painter = ctx.layer_painter(egui::LayerId::background());
        if clean {
            if let (true, Some(m)) = (self.clean_osd, session.viewed_vehicle().multirotor()) {
                crate::ui::osd::draw(&painter, screen, &drone::osd(m, session.viewed_vehicle(), &session.world, session.inside()));
                if let Some(race) = &session.race {
                    crate::ui::osd::draw_race(&painter, screen, &race.osd_for(&session.world, session.player, session.viewed_vehicle_id(), &session.view(), screen.aspect_ratio()));
                }
            }
            if let Some(pointer) = pointer {
                pointer.paint(&ctx.layer_painter(egui::LayerId::debug()));
            }
            return;
        }
        let controls_shown = session.store.flag(settings::CONTROLS_SHOWN).unwrap_or(false);
        // In the cockpit the instruments are its own, in 3D; outside, a row of dials. A multirotor's
        // flight controller writes its own over the picture, in both views.
        // One with a cockpit has its instruments there, and that display only from outside.
        let in_cabin = session.inside() && kinds::cabin(session.craft()).is_some();
        if let Some(m) = session.viewed_vehicle().multirotor().filter(|_| session.multirotor().is_some() && !in_cabin) {
            crate::ui::osd::draw(&painter, screen, &drone::osd(m, session.viewed_vehicle(), &session.world, session.inside()));
            if let Some(race) = &session.race {
                crate::ui::osd::draw_race(&painter, screen, &race.osd_for(&session.world, session.player, session.viewed_vehicle_id(), &session.view(), screen.aspect_ratio()));
            }
        } else if !session.inside() && session.multirotor().is_none() {
            // Clear of the yoke box at the bottom left.
            let left = if controls_shown { hud::yoke_box(screen).right() + 12.0 } else { screen.left() + 16.0 };
            let area = Rect::from_min_max(pos2(left, screen.top()), screen.max);
            panel::draw_strip(&painter, area, &session.panel_data());
        }
        // Where the yoke and pedals are; a multirotor's roll, pitch and yaw sticks.
        let (aileron, elevator, rudder) = match (session.aircraft(), session.multirotor()) {
            (Some(a), _) => (a.controls.aileron, a.controls.elevator, a.controls.rudder),
            (_, Some(m)) => (m.controls.roll, m.controls.pitch, m.controls.yaw),
            _ => (0.0, 0.0, 0.0),
        };
        let help = session.help_text();
        hud::draw(
            &painter,
            screen,
            &Hud {
                // The start and pause screen is over the status line.
                status: if session.home_open && !session.covered() { "" } else { &self.status },
                // A menu says by itself that the game waits; beside the settings the flight goes on.
                message: if session.paused && session.menu_shown() { "" } else { &self.message },
                help: &help,
                aileron: aileron as f32,
                elevator: elevator as f32,
                rudder: rudder as f32,
                controls: controls_shown,
                tags: &session.annunciators(),
                // Below the maps' credit when there is one.
                tags_top: if place.is_some() { 26.0 } else { 10.0 },
                tags_right: if session.settings_open && !session.covered() { menu::side::WIDTH.min(screen.width() - 24.0) + 26.0 } else { 14.0 },
            },
        );

        if let Some(pointed) = &pointed {
            hud::draw_pointed(&painter, screen, pointed);
        }

        // What is made in the background while the flight goes on, at the bottom, clear of the row
        // of instruments outside and of the replay's bar.
        let busy = if renderer.changing_quality() {
            Some("Applying graphics quality…")
        } else if photos.is_some() && place.is_some() && loading.is_none() {
            Some("Loading the satellite picture of the ground…")
        } else {
            None
        };
        if let (Some(text), None) = (busy, &session.playback) {
            let lift = if session.inside() || session.multirotor().is_some() { 18.0 } else { screen.height() * 0.19 };
            waiting::busy(&painter, screen, text, lift, time);
        }
        // Another place being gone to, or that could not be: the card, the flight held behind it.
        if session.waiting_for_place && !session.covered() {
            let key = loading.as_ref().map(|l| l.key.clone()).or_else(|| failed.clone());
            let state = match (loading.as_ref(), place_error.as_deref()) {
                (Some(l), _) => Some(waiting::Place::Loading { progress: l.progress(), seconds: l.started.elapsed().as_secs_f64() }),
                (None, Some(reason)) => Some(waiting::Place::Failed { reason }),
                (None, None) => None,
            };
            if let Some(state) = state {
                let key = key.as_deref();
                if let Some(chosen) = waiting::place_card(ctx, &places::name(key), &places::describe(key), &state, time) {
                    *card_choice = Some(chosen);
                }
            }
        }

        let inside = session.inside();
        if let Some(playback) = &mut session.playback {
            // Outside, above the row of instruments (`panel::draw_strip`: at most 15% of the height and its gaps).
            let lift = if inside { 0.0 } else { screen.height() * 0.19 };
            if crate::ui::replay::show(ctx, playback, &session.recorder, lift) {
                session.end_replay();
            }
        }

        // The menus over the waiting flight. Each is drawn while it is shown and for the moment it
        // takes to leave; only the one on top listens.
        let moves = std::mem::take(&mut session.menu_keys);
        let Menus { home, side, hangar, vehicles, pictures, .. } = menus;
        let covered = session.covered();
        let over_home = session.settings_open || session.hangar_open || session.vehicles_open;
        let shown = |name: &str, on: bool| ctx.animate_bool_with_time(egui::Id::unique(name), on, 0.2);
        let home_shown = shown("menu-home-shown", session.home_open && !over_home && !covered);
        let side_shown = shown("menu-side-shown", session.settings_open && !covered);
        let hangar_shown = shown("menu-hangar-shown", session.hangar_open && !covered);
        let vehicles_shown = shown("menu-vehicles-shown", session.vehicles_open && !covered);
        let place_key = loading.as_ref().map(|l| l.key.clone()).or_else(|| place.clone());
        let place_name = places::name(place_key.as_deref());
        // The picture of where the flight is: the course raced, the real place, or the seed's world.
        let place_picture = match &session.race {
            Some(race) => race.track.course.key.clone(),
            None => place_key.clone().unwrap_or_else(|| "generated".to_string()),
        };
        if home_shown > 0.0 {
            let active = session.home_open && !over_home && !covered && !session.waiting_for_place;
            let (sky, day) = {
                let conditions = Conditions { weather: &mut session.world.weather, hour: &mut session.world.hour, cloud_cover: &mut renderer.world.cloud_cover, cloud_altitude: &mut renderer.world.cloud_altitude, storms: &mut renderer.world.cloud_storms };
                (settings::Sky::current(&conditions), menu::side::day_line(&conditions))
            };
            let info = menu::home::Info {
                craft: session.craft(),
                place_picture: &place_picture,
                place: &place_name,
                place_what: &places::describe(place_key.as_deref()),
                sky,
                day: &day,
                hour: session.world.hour,
                fresh: session.world.time() < 1.0,
            };
            use menu::home::Choice;
            use menu::side::Tab;
            match home.show(ctx, home_shown, active, &moves, &info, pictures) {
                Some(Choice::Fly) => session.resume(),
                Some(Choice::Aircraft) => session.open_hangar(),
                Some(Choice::Vehicles) => session.open_vehicles(),
                Some(Choice::Place) => session.open_picker(),
                Some(Choice::Manual) => session.open_manual(),
                Some(Choice::Replay) => {
                    session.resume();
                    session.start_replay();
                }
                Some(what) => {
                    side.turn_to(match what {
                        Choice::Weather => Tab::Weather,
                        Choice::About => Tab::About,
                        _ => Tab::Flight,
                    });
                    session.open_settings();
                }
                None => {}
            }
        }
        if side_shown > 0.0 {
            let active = session.settings_open && !covered;
            let status = loading.as_ref().map(places::Loading::text).or_else(|| place_error.clone());
            let now = menu::side::Now { craft: session.craft(), place_picture: &place_picture, place: &place_name, status: status.as_deref(), from_home: session.home_open, assist: session.pilot.assist };
            let mut conditions = Conditions { weather: &mut session.world.weather, hour: &mut session.world.hour, cloud_cover: &mut renderer.world.cloud_cover, cloud_altitude: &mut renderer.world.cloud_altitude, storms: &mut renderer.world.cloud_storms };
            let out = side.show(ctx, side_shown, active, &moves, &mut conditions, quality, &now, &mut session.store, pictures);
            if active {
                if let Some(on) = out.assist {
                    session.set_assist(on);
                }
                if out.close {
                    session.back();
                }
                if out.aircraft {
                    session.open_hangar();
                }
                if out.place {
                    session.open_picker();
                }
                if out.manual {
                    session.open_manual();
                }
                if out.replay {
                    session.resume();
                    session.start_replay();
                }
            }
        }
        if !session.hangar_open {
            hangar.hidden();
        }
        if hangar_shown > 0.0 {
            let active = session.hangar_open && !covered;
            let out = hangar.show(ctx, hangar_shown, active, &moves, session.craft(), pictures);
            if let Some(craft) = out.chosen.filter(|_| active) {
                session.fly(craft);
                session.back();
            } else if out.close && active {
                session.back();
            }
        }

        // The first screen, over everything of the menus.
        if !session.vehicles_open {
            vehicles.hidden();
        }
        if vehicles_shown > 0.0 {
            use menu::vehicles::Choice;
            let active = session.vehicles_open && !covered;
            // The cars are another program, which the web has not.
            let cars = cfg!(not(target_arch = "wasm32"));
            match vehicles.show(ctx, vehicles_shown, active, &moves, session.craft().family(), cars, pictures).filter(|_| active) {
                Some(Choice::Fly(family)) => session.choose_family(family),
                // The driving game takes this one's place; if it cannot, the screen says why.
                #[cfg(not(target_arch = "wasm32"))]
                Some(Choice::Cars) => vehicles.error = engine::program::hand_over(menu::vehicles::DRIVE, &[]).err(),
                #[cfg(target_arch = "wasm32")]
                Some(Choice::Cars) => {}
                Some(Choice::Close) => session.back(),
                None => {}
            }
        }

        if session.manual_open {
            manual.show(ctx, &mut session.manual_open);
            // Closed: back on the start and pause screen, where everything else is.
            session.home_open |= !session.manual_open;
        } else {
            manual.hidden();
        }

        if session.picker_open {
            let current = loading.as_ref().map(|l| l.key.clone()).or_else(|| place.clone());
            let picker = picker.get_or_insert_with(|| Picker::new(around(&session.world.terrain)));
            // Opened again: from the place flown now, with what is on disk looked at anew.
            if !*picker_shown {
                picker.reopen(current.as_deref());
            }
            let game = picker::Context {
                store: &mut session.store,
                current: current.as_deref(),
                loading: loading.as_ref().map(places::Loading::text).or_else(|| place_error.clone()),
                wind_from: session.world.weather.wind_from,
                pictures,
            };
            if let Some(chosen) = picker.show(ctx, &mut session.picker_open, game) {
                *choice = Some(chosen);
                session.picker_open = false;
            } else {
                // Closed without a choice: back on the start and pause screen.
                session.home_open |= !session.picker_open;
            }
        }
        *picker_shown = session.picker_open;
    }

    fn draw(&mut self, _gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>) {
        if !self.session.covered() {
            self.renderer.draw(pass);
        }
    }

    fn translucent(&self) -> bool {
        !self.session.covered() && self.renderer.translucent()
    }

    fn before_translucent(&mut self, _gpu: &Gpu, encoder: &mut wgpu::CommandEncoder) {
        self.renderer.before_translucent(encoder);
    }

    fn draw_translucent(&mut self, _gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>) {
        self.renderer.draw_translucent(pass);
    }

    fn post(&self) -> engine::Post {
        let post = self.renderer.post();
        let Some(lens) = &self.lens else { return post };
        engine::Post {
            vignette: lens.vignette,
            flare: lens.flare,
            flare_sun: lens.flare_sun,
            focus: lens.focus,
            focus_span: lens.focus_span,
            focus_blur: lens.focus_blur,
            near: lens.near,
            bloom: lens.bloom.unwrap_or(post.bloom),
            warmth: lens.warmth,
            contrast: post.contrast + lens.contrast,
            exposure: post.exposure + lens.exposure,
            saturation: post.saturation * lens.saturation,
            ..post
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manual::inputs::Act;

    const FRAME: f64 = 1.0 / 60.0;

    #[test]
    fn frame_rate_goes_at_the_end_of_the_first_line() {
        let mut rate = FrameRate { shown: true, ..Default::default() };
        assert_eq!(rate.added_to("one\ntwo".to_string()), "one\ntwo");
        for _ in 0..31 {
            rate.frame(FRAME);
        }
        assert_eq!(rate.added_to("one\ntwo".to_string()), "one · 60 fps, 16.7 ms\ntwo");
        assert_eq!(rate.added_to("one".to_string()), "one · 60 fps, 16.7 ms");
        rate.shown = false;
        assert_eq!(rate.added_to("one".to_string()), "one");
    }

    fn session() -> Session {
        Session::new(1, Store::memory())
    }

    /// Holds these keys for `seconds` of 60 Hz frames.
    fn hold(s: &mut Session, held: Held, seconds: f64) {
        for _ in 0..(seconds / FRAME).round() as usize {
            s.advance(FRAME, &held);
        }
    }

    #[test]
    fn throttle_and_trim_keys_move_at_their_rates_and_stop_at_the_ends() {
        let mut c = AircraftControls::default();
        let up = Held { throttle_up: true, ..Held::default() };
        engine_keys(&mut c, &up, true, 1.0);
        assert!((c.throttle - 0.45).abs() < 1e-12);
        engine_keys(&mut c, &up, true, 5.0);
        assert_eq!(c.throttle, 1.0);
        engine_keys(&mut c, &Held { throttle_down: true, ..Held::default() }, true, 10.0);
        assert_eq!(c.throttle, 0.0);

        // Q and E trim only without the assist.
        let e = Held { fine: 1.0, ..Held::default() };
        engine_keys(&mut c, &e, true, 1.0);
        assert_eq!(c.trim, 0.0);
        engine_keys(&mut c, &e, false, 1.0);
        assert!((c.trim - 0.12).abs() < 1e-12);
        engine_keys(&mut c, &Held { fine: -1.0, ..Held::default() }, false, 20.0);
        assert_eq!(c.trim, -1.0);
    }

    #[test]
    fn one_shot_keys() {
        let mut s = session();
        for _ in 0..5 {
            s.key_pressed(KeyCode::KeyF);
        }
        assert_eq!(s.aircraft().unwrap().controls.flap_notch, 3);
        for _ in 0..5 {
            s.key_pressed(KeyCode::KeyR);
        }
        assert_eq!(s.aircraft().unwrap().controls.flap_notch, 0);

        assert!(s.cockpit && !s.show_help);
        s.key_pressed(KeyCode::KeyV);
        s.key_pressed(KeyCode::KeyH);
        assert!(!s.cockpit && s.show_help);
        assert!(s.help_text().starts_with("F1         illustrated manual") && s.help_text().ends_with("\n\nH: close the help"));

        let assist = s.pilot.assist;
        s.key_pressed(KeyCode::KeyG);
        assert_eq!(s.pilot.assist, !assist);
        assert_eq!(s.store.flag("assist"), Some(!assist));
        assert_eq!(s.message_text(), "Assist off: the arrows move the controls directly");
        hold(&mut s, Held::default(), 3.1);
        assert_eq!(s.message_text(), "");
    }

    #[test]
    fn moving_to_a_real_place_and_back_keeps_the_aircraft_and_the_weather() {
        use sim_core::{Cover, Detail, Grid};
        let runway = Airport { id: 5, n: 0.0, e: 0.0, heading: 1.2, elevation: 40.0, length: 1800.0, width: 30.0, detail: Detail { paved: true, lighting: None, apron: 1.0 } };
        let region = Region {
            name: "Plain".into(),
            latitude: 0.7,
            longitude: 0.0,
            heights: vec![Grid::centred(0.0, 0.0, 20_000.0, 50.0, 40.0f32)],
            cover: Grid::centred(0.0, 0.0, 20_000.0, 50.0, Cover::Grass as u8),
            airports: vec![runway],
            roads: Vec::new(),
            towns: Vec::new(),
            buildings: Vec::new(),
            bridges: Vec::new(),
        };
        let mut s = session();
        s.set_craft(Craft::Aircraft(AircraftKind::EXTRA300));
        s.world.weather.wind_speed = 5.0;
        s.world.hour = 17.0;
        s.add_traffic(1);
        s.move_to(Arc::new(Terrain::from_region(1, Arc::new(region))));
        assert_eq!(s.world.terrain.region().map(|r| r.name.as_str()), Some("Plain"));
        assert_eq!(s.vehicle().home, Some(runway));
        assert!(s.vehicle().agl() < 5.0 && runway.local(s.aircraft().unwrap().state.pos.x, s.aircraft().unwrap().state.pos.y).0.abs() < 900.0);
        assert_eq!((s.aircraft_kind().unwrap(), s.world.weather.wind_speed, s.world.hour), (AircraftKind::EXTRA300, 5.0, 17.0));
        assert_eq!(s.world.earth.latitude, 0.7);
        assert_eq!(s.planes.len(), 2, "the traffic comes along");
        s.move_to(Arc::new(Terrain::new(1)));
        assert!(s.world.terrain.region().is_none());
        assert_eq!(s.vehicle().home.map(|a| a.id), session().vehicle().home.map(|a| a.id));
    }

    #[test]
    fn the_picker_starts_on_the_runway_end_asked_on_final_or_in_the_air() {
        use sim_core::{Cover, Detail, Grid};
        let detail = Detail { paved: true, lighting: None, apron: 1.0 };
        // One aerodrome with two runways: 03/21 in the middle, and 12/30 to the east.
        let long = Airport { id: 7, n: 0.0, e: 0.0, heading: 30.0 * DEG, elevation: 40.0, length: 2000.0, width: 45.0, detail };
        let cross = Airport { id: 7, n: 300.0, e: 600.0, heading: 120.0 * DEG, length: 1500.0, detail: Detail { apron: 0.0, ..detail }, ..long };
        let region = Region {
            name: "Plain".into(),
            latitude: 0.7,
            longitude: 0.0,
            heights: vec![Grid::centred(0.0, 0.0, 30_000.0, 50.0, 40.0f32)],
            cover: Grid::centred(0.0, 0.0, 30_000.0, 50.0, Cover::Grass as u8),
            airports: vec![long, cross],
            roads: Vec::new(),
            towns: Vec::new(),
            buildings: Vec::new(),
            bridges: Vec::new(),
        };
        let mut s = session();
        s.move_to(Arc::new(Terrain::from_region(1, Arc::new(region))));
        let heading = |s: &Session| (euler_angles(s.aircraft().unwrap().state.q).heading / DEG).rem_euclid(360.0);
        let at = |s: &Session, a: &Airport| {
            let p = s.aircraft().unwrap().state.pos;
            a.local(p.x, p.y)
        };
        // Into a wind from 200°: runway 21, from its north-east end.
        s.world.weather.wind_from = 200.0;
        s.start_at(places::Start::Runway { heading: None });
        assert!((heading(&s) - 210.0).abs() < 0.01, "{}", heading(&s));
        let (along, across) = at(&s, &long);
        assert!((along - (1000.0 - 40.0)).abs() < 0.01 && across.abs() < 0.01, "{along} {across}");
        assert!(s.vehicle().agl() < 3.0);
        // The end chosen, 30 (taking off along 300°): the cross runway, from its south-east end.
        s.start_at(places::Start::Runway { heading: Some(299.0) });
        assert!((heading(&s) - 300.0).abs() < 0.01, "{}", heading(&s));
        assert!((at(&s, &cross).0 - (750.0 - 40.0)).abs() < 0.01);
        // On a 5 NM final to 12: that far before the aiming point, on a 3° slope.
        s.start_at(places::Start::Final { heading: Some(120.0), miles: 5.0 });
        assert!((heading(&s) - 120.0).abs() < 0.01);
        let (along, across) = at(&s, &cross);
        assert!((along + 750.0 - sim_core::lights::AIM + 5.0 * 1852.0).abs() < 0.01 && across.abs() < 0.01, "{along}");
        let height = -s.aircraft().unwrap().state.pos.z - 40.0;
        assert!((height - 5.0 * 1852.0 * (3.0 * DEG).tan()).abs() < 3.0, "{height}");
        // In the air over the place, 3000 ft up, into the wind.
        s.start_at(places::Start::Air { feet: 3000.0 });
        let p = s.aircraft().unwrap().state.pos;
        assert!(p.x.hypot(p.y) < 1.0 && (s.vehicle().agl() - 3000.0 * FEET).abs() < 1.0, "{}", s.vehicle().agl());
        assert!((heading(&s) - 200.0).abs() < 0.01);
        assert!(!s.aircraft().unwrap().data().on_ground());
    }

    #[test]
    fn m_opens_the_picker_and_the_game_waits() {
        let mut s = session();
        s.key_pressed(KeyCode::KeyO);
        s.key_pressed(KeyCode::KeyM);
        assert!(s.picker_open && s.paused && !s.settings_open);
    }

    #[test]
    fn the_assist_choice_is_read_from_the_settings() {
        let mut store = Store::memory();
        store.set("assist", false);
        assert!(!Session::new(1, store).pilot.assist);
        assert!(session().pilot.assist);
    }

    #[test]
    fn escape_pauses_and_any_key_only_resumes() {
        let mut s = session();
        s.key_pressed(KeyCode::Escape);
        assert!(s.paused);
        assert_eq!(s.message_text(), "PAUSED\nPress a key to go on");
        let before = s.world.time();
        hold(&mut s, Held { throttle_up: true, ..Held::default() }, 1.0);
        assert_eq!(s.world.time(), before);
        assert_eq!(s.aircraft().unwrap().controls.throttle, 0.0);

        // H works without resuming; 2 resumes and does not restart in the air.
        s.key_pressed(KeyCode::KeyH);
        assert!(s.paused && s.show_help);
        s.key_pressed(KeyCode::Digit2);
        assert!(!s.paused);
        assert!(s.vehicle().agl() < 5.0);
        s.key_pressed(KeyCode::Digit2);
        assert!(s.vehicle().agl() > 800.0);

        // O opens the settings and the game goes on beside them; Esc closes them.
        s.key_pressed(KeyCode::KeyO);
        assert!(s.settings_open && !s.paused);
        s.key_pressed(KeyCode::Escape);
        assert!(!s.paused && !s.settings_open);
    }

    /// Esc pauses (the start and pause screen) and Esc again flies on, from wherever the flight
    /// is: on the ground, in the air, in pieces. Over the settings it first closes them.
    #[test]
    fn escape_pauses_and_escape_again_flies_on() {
        let mut s = session();
        for start in [0, 1, 2] {
            match start {
                1 => s.restart_in_air(),
                2 => s.crash_ahead(1.0),
                _ => {}
            }
            hold(&mut s, Held::default(), 0.5);
            s.key_pressed(KeyCode::Escape);
            assert!(s.paused && s.home_open, "{start}");
            let time = s.world.time();
            hold(&mut s, Held::default(), 0.5);
            assert_eq!(s.world.time(), time);
            s.key_pressed(KeyCode::Escape);
            assert!(!s.paused && !s.menu_shown(), "{start}");
            hold(&mut s, Held::default(), 0.5);
            assert!(s.world.time() > time);
        }
        // The settings open in flight: Esc closes them, the next pauses, the next flies on.
        s.key_pressed(KeyCode::KeyO);
        s.key_pressed(KeyCode::Escape);
        assert!(!s.settings_open && !s.paused);
        s.key_pressed(KeyCode::Escape);
        assert!(s.paused && s.home_open);
        s.key_pressed(KeyCode::Escape);
        assert!(!s.paused && !s.menu_shown());
    }

    /// The settings opened in flight (O) are beside it: the world goes on, the keys fly, the
    /// texts are said; O again closes them. Opened from the start and pause screen the game waits.
    #[test]
    fn the_settings_opened_in_flight_do_not_pause_it() {
        let mut s = session();
        s.restart_in_air();
        s.key_pressed(KeyCode::KeyO);
        assert!(s.settings_open && !s.paused && s.menu_shown() && !s.waiting_menu_shown());
        let (time, throttle) = (s.world.time(), s.aircraft().unwrap().controls.throttle);
        hold(&mut s, Held { throttle_down: true, ..Held::default() }, 1.0);
        assert!((s.world.time() - time - 1.0).abs() < 0.02, "{}", s.world.time() - time);
        assert!(s.aircraft().unwrap().controls.throttle < throttle, "the keys fly");
        // The flight's keys are the flight's, not the panel's: an arrow is not kept for it.
        s.key_pressed(KeyCode::KeyF);
        assert_eq!(s.aircraft().unwrap().controls.flap_notch, 1);
        assert!(s.menu_keys.is_empty() && s.settings_open);
        assert_ne!(s.message_text(), "PAUSED\nPress a key to go on");
        // The aircraft opened from them wait, and back on the settings the flight goes on.
        s.open_hangar();
        assert!(s.hangar_open && s.paused);
        s.back();
        assert!(s.settings_open && !s.paused);
        s.open_hangar();
        s.open_settings();
        assert!(s.settings_open && !s.hangar_open && !s.paused);
        // O again: closed.
        s.key_pressed(KeyCode::KeyO);
        assert!(!s.settings_open && !s.paused);
        // From the start and pause screen the game waits under them, and O leaves them open.
        s.key_pressed(KeyCode::Escape);
        s.key_pressed(KeyCode::KeyO);
        assert!(s.settings_open && s.home_open && s.paused);
        s.key_pressed(KeyCode::KeyO);
        assert!(s.settings_open && s.paused);
    }

    /// F10 and F9 go through the rates; the world takes more steps of the same length, so a
    /// flight at ×4 is the flight at ×1 four times as fast, step for step.
    #[test]
    fn time_goes_faster_by_more_steps_of_the_same_length() {
        let mut fast = session();
        let mut real = session();
        for s in [&mut fast, &mut real] {
            s.restart_in_air();
            s.step_budget = std::time::Duration::from_secs(10);
        }
        assert_eq!(fast.time_rate(), 1);
        assert_eq!(fast.time_text(), "");
        for expected in [2, 4, 8, 16, 16] {
            fast.key_pressed(KeyCode::F10);
            assert_eq!(fast.time_rate(), expected);
        }
        fast.key_pressed(KeyCode::F9);
        fast.key_pressed(KeyCode::F9);
        assert_eq!(fast.time_rate(), 4);
        assert!(fast.annunciators().contains(&"×4".to_string()));
        let (t0, r0) = (fast.world.time(), real.world.time());
        hold(&mut fast, Held::default(), 2.0);
        hold(&mut real, Held::default(), 8.0);
        let (dt_fast, dt_real) = (fast.world.time() - t0, real.world.time() - r0);
        assert!((dt_fast - 8.0).abs() < 0.02, "{dt_fast}");
        assert!((dt_fast - dt_real).abs() < 1e-9, "{dt_fast} {dt_real}");
        // The same steps: the same flight, to the bit.
        assert_eq!(fast.aircraft().unwrap().state.pos, real.aircraft().unwrap().state.pos);
        assert!((fast.achieved_rate() - 4.0).abs() < 0.05, "{}", fast.achieved_rate());
        assert_eq!(fast.time_text(), "×4");
        // The texts in the middle are timed in real seconds: the same at any rate.
        fast.set_assist(false);
        hold(&mut fast, Held::default(), 2.0);
        assert!(fast.message_text().starts_with("Assist off"));
        hold(&mut fast, Held::default(), 1.5);
        assert!(!fast.message_text().starts_with("Assist off"));
    }

    /// A computer that cannot step the world that fast: the steps real time asks for are taken,
    /// the rest let go of, and the tag says how fast it really went.
    #[test]
    fn time_goes_as_fast_as_the_computer_can_and_says_so() {
        let mut s = session();
        s.restart_in_air();
        s.set_time_rate(16);
        s.step_budget = std::time::Duration::ZERO;
        let time = s.world.time();
        hold(&mut s, Held::default(), 3.0);
        let went = (s.world.time() - time) / 3.0;
        assert!((1.0..1.1).contains(&went), "{went}");
        assert_eq!(s.time_rate(), 16, "the rate chosen stays");
        // What it says comes down from ×16 over a second or two.
        hold(&mut s, Held::default(), 3.0);
        assert!(s.achieved_rate() < 1.1, "{}", s.achieved_rate());
        assert!(s.time_text().starts_with("×16 → ×1."), "{}", s.time_text());
    }

    /// Back to real time on a crash, on a touchdown, on a pause, in a replay; and no faster in pieces.
    #[test]
    fn time_goes_back_to_real_on_a_crash_a_touchdown_and_a_pause() {
        let mut s = session();
        s.restart_in_air();
        s.set_time_rate(8);
        s.key_pressed(KeyCode::Escape);
        assert_eq!(s.time_rate(), 1);
        s.key_pressed(KeyCode::Escape);
        hold(&mut s, Held::default(), 3.0);
        s.set_time_rate(8);
        s.key_pressed(KeyCode::F2);
        assert!(s.replaying());
        assert_eq!(s.time_rate(), 1);
        s.key_pressed(KeyCode::F10);
        assert_eq!(s.time_rate(), 1, "a replay has its own speeds");
        s.key_pressed(KeyCode::F2);
        s.step_budget = std::time::Duration::from_secs(10);
        // Until the frame it happens in, at ×16: the first is a touchdown, the second a crash.
        let until_real = |s: &mut Session| {
            s.set_time_rate(16);
            for _ in 0..(10.0 / FRAME) as usize {
                s.advance(FRAME, &Held::default());
                if s.time_rate() == 1 {
                    return true;
                }
            }
            false
        };
        // A metre over the runway, level, slow: it settles onto it.
        s.restart_on_runway();
        s.dive(2.5, 0.0, 50.0);
        assert!(until_real(&mut s));
        assert!(s.aircraft().unwrap().last_touchdown().is_some() && s.vehicle().dynamics.crashed().is_none());
        s.restart_in_air();
        s.dive(40.0, 35.0, 90.0);
        assert!(until_real(&mut s));
        assert!(s.vehicle().dynamics.crashed().is_some());
        // In pieces: no faster.
        s.key_pressed(KeyCode::F10);
        assert_eq!(s.time_rate(), 1);
    }

    #[test]
    fn escape_shows_the_menu_and_the_keys_move_in_it_or_fly_on() {
        let mut s = session();
        s.key_pressed(KeyCode::Escape);
        assert!(s.home_open && s.paused && s.menu_shown());
        // The arrows, Enter and Tab are the menu's: kept for it, and the game still waits.
        for key in [KeyCode::ArrowDown, KeyCode::Enter, KeyCode::Tab] {
            s.key_pressed(key);
        }
        assert_eq!(s.menu_keys, [Nav::Down, Nav::Activate, Nav::NextTab]);
        assert!(s.home_open && s.paused);
        // Any other key flies on, and nothing of the menu stays.
        s.key_pressed(KeyCode::KeyW);
        assert!(!s.paused && !s.menu_shown() && s.menu_keys.is_empty());
        // Esc again on the menu flies on too.
        s.key_pressed(KeyCode::Escape);
        s.key_pressed(KeyCode::Escape);
        assert!(!s.paused && !s.home_open);
    }

    #[test]
    fn the_settings_and_the_aircraft_go_back_to_the_menu_they_were_opened_from() {
        let mut s = session();
        s.key_pressed(KeyCode::Escape);
        s.key_pressed(KeyCode::KeyO);
        assert!(s.settings_open && s.home_open && s.paused);
        // The settings keep the keyboard: a flying key does nothing, an arrow is theirs.
        s.key_pressed(KeyCode::KeyW);
        s.key_pressed(KeyCode::ArrowRight);
        assert!(s.settings_open && s.paused);
        assert_eq!(s.menu_keys, [Nav::Right]);
        s.key_pressed(KeyCode::Escape);
        assert!(!s.settings_open && s.home_open && s.paused && s.menu_keys.is_empty());
        s.open_hangar();
        assert!(s.hangar_open && !s.settings_open);
        s.key_pressed(KeyCode::Escape);
        assert!(!s.hangar_open && s.home_open && s.paused);
        // The aircraft opened from the settings go back to the settings.
        s.open_settings();
        s.open_hangar();
        s.key_pressed(KeyCode::Escape);
        assert!(s.settings_open && !s.hangar_open && s.home_open);
        s.key_pressed(KeyCode::Escape);
        assert!(!s.settings_open && s.home_open && s.paused);
        s.key_pressed(KeyCode::Escape);
        assert!(!s.paused);
        // Opened in flight (O), Esc goes back to the flight.
        s.key_pressed(KeyCode::KeyO);
        assert!(s.settings_open && !s.home_open);
        s.key_pressed(KeyCode::Escape);
        assert!(!s.paused && !s.menu_shown());
    }

    #[test]
    fn a_pad_moves_in_the_menu_and_its_flying_buttons_wait() {
        use engine::{Controller, PadButton};
        let mut s = session();
        s.open_home();
        let mut pad = Controller::new(1, "Pad", true);
        // Down on the direction pad and A: the menu's moves. Y (the view, in flight) does nothing.
        pad.set_button(1, Some(PadButton::DPadDown), Some(true), None);
        pad.set_button(2, Some(PadButton::South), Some(true), None);
        pad.set_button(3, Some(PadButton::North), Some(true), None);
        let cockpit = s.cockpit;
        s.controllers(std::slice::from_ref(&pad));
        assert_eq!(s.menu_keys, [Nav::Down, Nav::Activate]);
        assert!(s.paused && s.home_open && s.cockpit == cockpit);
        // B goes back: from the settings to the menu, from the menu to the flight.
        s.open_settings();
        pad.end_frame();
        pad.set_button(4, Some(PadButton::East), Some(true), None);
        s.controllers(std::slice::from_ref(&pad));
        assert!(!s.settings_open && s.home_open);
        pad.set_button(4, Some(PadButton::East), Some(false), None);
        pad.end_frame();
        pad.set_button(4, Some(PadButton::East), Some(true), None);
        s.controllers(std::slice::from_ref(&pad));
        assert!(!s.paused && !s.home_open);
    }

    /// The first screen waits over the start and pause screen. A family chosen is flown in its
    /// craft flown last (the first, before any was) and its aircraft are shown; Esc chooses
    /// nothing and leaves the start and pause screen.
    #[test]
    fn the_first_screen_goes_on_to_a_family_s_aircraft_or_back_to_the_start_screen() {
        let mut s = session();
        assert_eq!(s.craft().key(), "c172");
        s.open_vehicles();
        assert!(s.vehicles_open && s.home_open && s.paused && s.menu_shown());
        // The arrows are the screen's; no other key flies on from it.
        s.key_pressed(KeyCode::ArrowRight);
        s.key_pressed(KeyCode::KeyW);
        assert_eq!(s.menu_keys, [Nav::Right]);
        assert!(s.vehicles_open && s.paused);
        s.choose_family(Family::Drones);
        assert_eq!(s.craft().key(), "fpv5");
        assert_eq!(s.store.word("aircraft"), Some("fpv5"));
        assert!(!s.vehicles_open && s.hangar_open && s.home_open && s.paused && s.menu_keys.is_empty());
        // The racing quad chosen there; then the planes and the drones again: each family's last.
        s.fly(Craft::parse("race5").unwrap());
        s.back();
        assert!(!s.hangar_open && s.home_open && s.paused);
        s.open_vehicles();
        s.choose_family(Family::Planes);
        assert_eq!(s.craft().key(), "c172");
        s.fly(Craft::parse("extra").unwrap());
        s.open_vehicles();
        assert!(s.vehicles_open && !s.hangar_open);
        s.choose_family(Family::Drones);
        assert_eq!(s.craft().key(), "race5");
        s.open_vehicles();
        s.choose_family(Family::Planes);
        assert_eq!((s.craft().key(), s.store.word("aircraft")), ("extra300", Some("extra300")));
        // The family already flown: its craft stays.
        s.open_vehicles();
        s.choose_family(Family::Planes);
        assert_eq!(s.craft().key(), "extra300");
        // Esc on the first screen: the start and pause screen; Esc again flies on.
        s.open_vehicles();
        s.key_pressed(KeyCode::Escape);
        assert!(!s.vehicles_open && s.home_open && s.paused);
        s.key_pressed(KeyCode::Escape);
        assert!(!s.paused && !s.menu_shown());
    }

    /// Whether the keys the manual names for `act` do it, each pressed (or held) in a fresh session.
    fn manual_keys_do(act: Act) -> Result<(), String> {
        let keys = act.keys();
        let &(key, shift) = keys.first().ok_or("no key")?;
        let held = |k: KeyCode| Held::with(|d| d == k, false);
        let ok = |done: bool, what: &str| if done { Ok(()) } else { Err(format!("{key:?} does not {what}")) };
        let mut s = session();
        if act == Act::Arm {
            s.set_craft(Craft::Multirotor(sim_core::MultirotorKind::FPV5));
        }
        match act {
            Act::Throttle => ok(held(KeyCode::KeyW).throttle_up && held(keys[1].0).throttle_down, "move the throttle"),
            Act::NoseUp => ok(held(key).pitch > 0.0, "pull"),
            Act::NoseDown => ok(held(key).pitch < 0.0, "push"),
            Act::Bank => ok(held(key).roll < 0.0 && held(keys[1].0).roll > 0.0, "bank left and right"),
            Act::Pedals => ok(held(key).pedals < 0.0 && held(keys[1].0).pedals > 0.0, "work the pedals"),
            Act::Brakes => ok(held(key).brake, "brake"),
            Act::Trim => ok(held(key).fine < 0.0 && held(keys[1].0).fine > 0.0, "trim"),
            Act::LookSide => ok(held(key).look < 0.0 && held(keys[1].0).look > 0.0, "look aside"),
            Act::Hour => ok(held(key).time && keys[1].1, "move the clock both ways"),
            Act::Panel(panel) => ok(keys.iter().all(|&(k, shift)| controls::panel_key(k, shift) == Some(panel)), "work the G1000"),
            // The engine records (`engine::video`); the game has nothing on that key.
            Act::Record => ok(key == engine::video::KEY && controls::Action::ALL.iter().all(|a| a.key_code() != Some(key)), "record, and only record"),
            Act::Replay | Act::ReplayPlay | Act::ReplayJump | Act::ReplaySpeed => {
                s.restart_in_air();
                hold(&mut s, Held::default(), 40.0);
                if act != Act::Replay {
                    s.key_pressed(KeyCode::F2);
                    if let (Some(p), Some((_, end))) = (s.playback.as_mut(), s.recorder.span()) {
                        p.time = end - 1.0;
                    }
                }
                let before = s.playback.as_ref().map(|p| (p.time, p.playing, p.speed));
                s.key_pressed_with(key, shift);
                let p = s.playback.as_ref().map(|p| (p.time, p.playing, p.speed));
                let done = match (act, before, p) {
                    (Act::Replay, None, Some(_)) => true,
                    (Act::ReplayPlay, Some((_, true, _)), Some((_, false, _))) => true,
                    (Act::ReplayJump, Some((t, ..)), Some((u, ..))) => u < t && keys[1].0 == KeyCode::ArrowRight && keys[2].1,
                    (Act::ReplaySpeed, Some((.., v)), Some((.., w))) => w < v && keys[1].0 == KeyCode::ArrowUp,
                    _ => false,
                };
                ok(done, &format!("{act:?}"))
            }
            _ => {
                if matches!(act, Act::Land | Act::Disconnect | Act::LookAhead) {
                    s.restart_in_air();
                    hold(&mut s, Held::default(), 1.0);
                }
                if act == Act::Disconnect {
                    s.key_pressed(KeyCode::KeyP);
                    assert!(s.gfc.as_ref().is_some_and(Gfc700::engaged));
                }
                if act == Act::LookAhead {
                    s.hold_look(0.5, 0.2);
                }
                let (spot, notice, zoom) = (s.spot, s.notice_until, s.zoom);
                s.key_pressed_with(key, shift);
                let done = match act {
                    Act::FlapsDown => s.aircraft().unwrap().controls.flap_notch == 1,
                    Act::FlapsUp => {
                        s.controls().unwrap().flap_notch = 2;
                        s.key_pressed(key);
                        s.aircraft().unwrap().controls.flap_notch == 1
                    }
                    Act::Assist => !s.pilot.assist,
                    Act::View => !s.cockpit,
                    Act::Views => s.spot == spot + 1,
                    Act::Closer => s.zoom > zoom && s.spot == spot,
                    Act::Further => s.zoom < zoom && s.spot == spot,
                    Act::LookAhead => (s.look_yaw, s.look_pitch) == (0.0, 0.0),
                    Act::Runway => s.vehicle().agl() < 5.0,
                    Act::Air => s.vehicle().agl() > 800.0,
                    Act::Final => (50.0..800.0).contains(&s.vehicle().agl()) && !s.aircraft().unwrap().data().on_ground(),
                    Act::Picker => s.picker_open && s.paused,
                    Act::Settings => s.settings_open && !s.paused,
                    // F9 at ×1 stays there; F10 twice is ×4, and F9 back to ×2.
                    Act::Time => {
                        let slowest = s.time_rate() == 1;
                        s.key_pressed(keys[1].0);
                        s.key_pressed(keys[1].0);
                        let faster = s.time_rate() == 4;
                        s.key_pressed(key);
                        slowest && faster && s.time_rate() == 2
                    }
                    Act::Manual => s.manual_open && s.paused,
                    Act::Help => s.show_help,
                    Act::Pause => s.paused,
                    Act::Land => s.autopilot.is_some(),
                    Act::Qnh | Act::Baro => s.notice_until > notice,
                    Act::Disconnect => !s.gfc.as_ref().is_some_and(Gfc700::engaged),
                    Act::Arm => !s.multirotor().unwrap().controls.arm,
                    _ => return Err("not checked".to_string()),
                };
                ok(done, &format!("{act:?}"))
            }
        }
    }

    #[test]
    fn the_replay_shows_the_past_while_the_world_waits_and_leaves_the_flight_as_it_was() {
        let mut s = session();
        s.add_traffic(2);
        s.restart_in_air();
        hold(&mut s, Held::default(), 30.0);
        let (now, live) = (s.world.time(), s.aircraft().unwrap().look());
        let others: Vec<V3> = s.world.vehicles.iter().map(|v| v.aircraft().unwrap().state.pos).collect();
        s.key_pressed(KeyCode::F2);
        assert!(s.replaying());
        // No landing: a minute back, or the start of what is kept.
        let (start, end) = s.recorder.span().unwrap();
        assert!(start < 1.0 && (end - now).abs() < 0.1, "{start} {end} {now}");
        assert_eq!(s.playback.as_ref().unwrap().time, start);
        hold(&mut s, Held { throttle_up: true, roll: 1.0, ..Held::default() }, 5.0);
        assert_eq!(s.world.time(), now, "the world waits");
        let shown = s.playback.as_ref().unwrap().time;
        assert!((shown - start - 5.0).abs() < 0.05, "{shown}");
        // The aircraft where it was then: 25 s of flying behind.
        assert!((s.aircraft().unwrap().state.pos - live.state.pos).length() > 500.0);
        // Faster, a jump, the end: it stops there.
        s.key_pressed(KeyCode::ArrowUp);
        s.key_pressed_with(KeyCode::ArrowRight, true);
        hold(&mut s, Held::default(), 10.0);
        let p = s.playback.as_ref().unwrap();
        assert!(p.time == end && !p.playing && p.speed == 2.0);
        // The keys that fly do nothing; F2 goes back to the flight exactly as it was.
        s.key_pressed(KeyCode::Digit1);
        s.key_pressed(KeyCode::KeyF);
        s.key_pressed(KeyCode::F2);
        assert!(!s.replaying());
        assert_eq!(s.aircraft().unwrap().look(), live);
        assert!(s.world.vehicles.iter().zip(&others).all(|(v, p)| v.aircraft().unwrap().state.pos == *p));
        hold(&mut s, Held::default(), 1.0);
        assert!(s.world.time() > now);
    }

    /// The FPV quad from the settings' word: on its feet on the runway, armed with the throttle
    /// down; W lifts it off, the view is its camera's, tilted up, and V the chase
    /// camera close behind it; F does nothing to it; tilted by the stick it levels when let go;
    /// a crash says how to start again.
    #[test]
    fn the_quad_arms_takes_off_and_is_seen_from_its_camera_and_behind() {
        let mut store = Store::memory();
        store.set("aircraft", "fpv");
        let mut s = Session::new(1, store);
        assert_eq!(s.craft(), Craft::Multirotor(sim_core::MultirotorKind::FPV5));
        assert!(s.aircraft().is_none() && s.gfc.is_none());
        hold(&mut s, Held::default(), 0.5);
        assert!(s.vehicle().dynamics.on_ground() && s.multirotor().unwrap().armed(), "{:?}", s.multirotor().unwrap().arming_blocked());
        // Up: W held raises the throttle stick, past the hover.
        hold(&mut s, Held { throttle_up: true, ..Held::default() }, 1.6);
        hold(&mut s, Held::default(), 1.0);
        assert!(s.vehicle().agl() > 1.0, "agl {}", s.vehicle().agl());
        // The camera: on the quad, looking along the nose tilted up 25°.
        assert!(s.inside());
        let v = s.view();
        let b = s.body();
        assert!((v.position - b.pos).length() < 0.1);
        let up = to_render(act(b.q, V3::NEG_Z));
        let forward = to_render(act(b.q, V3::X));
        let tilt = v.forward.dot(up).asin().to_degrees();
        assert!((tilt - 25.0).abs() < 2.0 && v.forward.dot(forward) > 0.85, "{tilt}");
        assert!(v.fov_y.to_degrees() > 80.0);
        // Outside, close behind it.
        s.key_pressed(KeyCode::KeyV);
        let v = s.view();
        let d = (v.position - s.body().pos).length();
        assert!(!s.inside() && (1.0..3.0).contains(&d), "{d}");
        // F: nothing (no flight modes, no flaps).
        let before = s.multirotor().unwrap().controls;
        s.key_pressed(KeyCode::KeyF);
        assert_eq!(s.multirotor().unwrap().controls, before);
        // Tilted by the stick, it levels when it is let go; the throttle stays where it was left.
        hold(&mut s, Held { roll: 1.0, ..Held::default() }, 0.5);
        assert!(sim_core::euler_angles(s.body().q).roll.abs() > 0.3);
        let throttle = s.multirotor().unwrap().controls.throttle;
        hold(&mut s, Held::default(), 1.5);
        let e = sim_core::euler_angles(s.body().q);
        assert!(e.roll.abs() < 0.03 && e.pitch.abs() < 0.03, "{e:?}");
        assert_eq!(s.multirotor().unwrap().controls.throttle, throttle);
        // Disarmed in the air it falls; on the ground the message says how to start again.
        s.key_pressed(KeyCode::KeyR);
        assert!(s.message_text().contains("disarmed"), "{}", s.message_text());
        hold(&mut s, Held::default(), 10.0);
        assert!(s.vehicle().dynamics.on_ground());
        s.key_pressed(KeyCode::Digit2);
        assert!(s.multirotor().unwrap().armed() && s.vehicle().agl() > 10.0);
        s.key_pressed(KeyCode::Digit1);
        assert!(s.vehicle().dynamics.on_ground() && s.multirotor().unwrap().armed() && s.multirotor().unwrap().controls.throttle == 0.0);
        // Back to an aircraft: on the runway, in its cockpit.
        s.set_craft(Craft::Aircraft(AircraftKind::C172));
        assert!(s.aircraft().is_some() && s.gfc.is_some());
    }

    /// The replay shows the quad's past and gives it back as it was.
    #[test]
    fn the_replay_shows_the_quad() {
        let mut s = session();
        s.set_craft(Craft::Multirotor(sim_core::MultirotorKind::FPV5));
        s.restart_in_air();
        hold(&mut s, Held { pitch: -0.5, throttle_up: true, ..Held::default() }, 1.0);
        hold(&mut s, Held { pitch: -0.5, ..Held::default() }, 8.0);
        let live = s.multirotor().unwrap().look();
        s.key_pressed(KeyCode::F2);
        assert!(s.replaying());
        hold(&mut s, Held::default(), 1.0);
        assert!((s.body().pos - live.body.pos).length() > 20.0);
        s.key_pressed(KeyCode::F2);
        assert_eq!(s.multirotor().unwrap().look(), live);
    }

    /// The quad is drawn with its rotors turning, and its display reads its battery.
    #[test]
    fn the_quad_is_drawn_and_its_display_reads_it() {
        let mut s = session();
        s.set_craft(Craft::Multirotor(sim_core::MultirotorKind::FPV5));
        s.restart_in_air();
        hold(&mut s, Held::default(), 0.5);
        let visual = PlaneVisual::of(s.vehicle());
        assert!(visual.rotors[..4].iter().all(|&w| w > 1000.0) && visual.kind.index() >= AircraftKind::all().count());
        let osd = drone::osd(s.multirotor().unwrap(), s.vehicle(), &s.world, true);
        // Started in the air, it was armed as if on the ground 15 m below.
        assert!(osd.armed && (14.0..17.0).contains(&osd.volts) && (osd.altitude - 15.0).abs() < 1.0, "{osd:?}");
        assert!(osd.time > 0.3);
    }

    /// The camera behind a quad follows its turns a little behind (`chase_lag`, 0.4 s), the same at
    /// any frame rate and the short way round; its heading stays put as the nose goes through the
    /// vertical (the Euler heading turns half round there); after a restart, and on another craft
    /// (N), it is straight behind at once; the mouse's orbit still turns it.
    #[test]
    fn the_chase_camera_follows_the_turns_behind() {
        let quad = |fps: f64, from: f64, turn: f64, seconds: f64| {
            let mut s = session();
            s.set_craft(Craft::Multirotor(sim_core::MultirotorKind::FPV5));
            s.restart_in_air();
            (s.cockpit, s.frozen) = (false, true);
            let b = s.body();
            let place = |s: &mut Session, heading: f64| s.world.place(s.player, State { pos: b.pos, q: sim_core::quaternion(heading, 0.0, 0.0), ..State::default() });
            place(&mut s, from);
            s.advance(1.0 / fps, &Held::default());
            assert!(wrapped(s.chase.unwrap().heading - from).abs() < 1e-9, "behind it");
            place(&mut s, from + turn);
            for _ in 0..(seconds * fps).round() as usize {
                s.advance(1.0 / fps, &Held::default());
            }
            s
        };
        let k = 1.0 - (-0.25f64 / drone::chase_lag(Craft::Multirotor(sim_core::MultirotorKind::FPV5))).exp();
        let slow = quad(60.0, 0.5, 90.0 * DEG, 0.25);
        let fast = quad(144.0, 0.5, 90.0 * DEG, 0.25);
        let (h60, h144) = (slow.chase.unwrap().heading, fast.chase.unwrap().heading);
        println!("a quick 90° turn: the camera {:.1}° round after 0.25 s at 60 fps, {:.1}° at 144", (h60 - 0.5) / DEG, (h144 - 0.5) / DEG);
        assert!((h60 - (0.5 + 90.0 * DEG * k)).abs() < 1e-6 && (h60 - h144).abs() < 1e-6);
        // The view looks along it.
        let f = slow.view().forward;
        assert!(wrapped(f64::from(f.x).atan2(-f64::from(f.z)) - h60).abs() < 1e-3);
        let settled = quad(60.0, 0.5, 90.0 * DEG, 2.5).chase.unwrap().heading;
        assert!((settled - (0.5 + 90.0 * DEG)).abs() < 1.0 * DEG, "{}", settled / DEG);
        // From 172° to 192° (−168°): through 180°, not back round through 0°.
        let across = quad(60.0, 3.0, 0.35, 0.25).chase.unwrap().heading;
        assert!(wrapped(across - (3.0 + 0.35 * k)).abs() < 1e-6, "{}", across / DEG);

        // Pitched down through the vertical about its own wing: steady, where the Euler heading flips.
        let h = 1.0;
        for pitch in [-80.0, -89.0, -89.99, -90.0, -90.01, -91.0, -100.0] {
            let q = sim_core::quaternion(h, 0.0, 0.0) * axis_angle(pitch * DEG, V3::Y);
            let ours = chase_heading(q).unwrap();
            assert!(wrapped(ours - h).abs() < 1e-6, "at {pitch}°: {}", ours / DEG);
        }
        let past = sim_core::quaternion(h, 0.0, 0.0) * axis_angle(-100.0 * DEG, V3::Y);
        assert!(wrapped(euler_angles(past).heading - h).abs() > 3.0, "the Euler heading has flipped");

        // A restart: straight behind it, no swing.
        let mut s = quad(60.0, 0.5, 90.0 * DEG, 0.1);
        s.restart_in_air();
        hold(&mut s, Held::default(), FRAME);
        let behind = chase_heading(s.body().q).unwrap();
        assert!((s.chase.unwrap().heading - behind).abs() < 1e-9);
        // Another craft (N): behind it at once.
        s.frozen = false;
        s.add_traffic(1);
        s.watch_next();
        hold(&mut s, Held::default(), FRAME);
        let other = s.world.vehicle(s.watched()).body().q;
        assert!(s.watched() != s.player && (s.chase.unwrap().heading - chase_heading(other).unwrap()).abs() < 1e-9);
        // The mouse's orbit turns the camera round it, on top.
        let before = s.view().forward;
        s.orbit_yaw += 0.5;
        let after = s.view().forward;
        let turned = f64::from(after.x).atan2(-f64::from(after.z)) - f64::from(before.x).atan2(-f64::from(before.z));
        assert!((wrapped(turned) - 0.5).abs() < 1e-3, "{turned}");
    }

    #[test]
    fn the_keys_the_manual_names_do_what_it_says() {
        for act in crate::manual::pages::acts() {
            if let Err(error) = manual_keys_do(act) {
                panic!("{act:?}: {error}");
            }
        }
    }

    #[test]
    fn f1_opens_the_manual_and_the_game_waits() {
        let mut s = session();
        s.key_pressed(KeyCode::KeyO);
        s.key_pressed(KeyCode::F1);
        assert!(s.manual_open && s.paused && !s.settings_open && s.covered());
    }

    #[test]
    fn escape_hides_the_help_before_pausing() {
        let mut s = session();
        s.key_pressed(KeyCode::KeyH);
        s.key_pressed(KeyCode::Escape);
        assert!(!s.show_help && !s.paused);
        s.key_pressed(KeyCode::Escape);
        assert!(s.paused);
    }

    #[test]
    fn the_clock_key_runs_both_ways_and_wraps() {
        let mut s = session();
        s.world.hour = 23.5;
        hold(&mut s, Held { time: true, ..Held::default() }, 1.0);
        assert!((s.world.hour - 1.0).abs() < 1e-9, "{}", s.world.hour);
        hold(&mut s, Held { time: true, shift: true, ..Held::default() }, 2.0);
        assert!((s.world.hour - 22.0).abs() < 1e-9, "{}", s.world.hour);
        // Also while paused.
        s.pause();
        hold(&mut s, Held { time: true, ..Held::default() }, 1.0);
        assert!((s.world.hour - 23.5).abs() < 1e-9, "{}", s.world.hour);
    }

    #[test]
    fn levers_map_as_on_the_panel() {
        let mut s = session();
        s.set_lever(Lever::Throttle, 0.7);
        s.set_lever(Lever::Trim, 0.25);
        s.set_lever(Lever::Flaps, 0.3);
        let c = s.aircraft().unwrap().controls;
        assert_eq!((c.throttle, c.trim, c.flap_notch), (0.7, 0.5, 2));
        s.set_lever(Lever::Flaps, 1.0);
        s.set_lever(Lever::Trim, 1.0);
        let c = s.aircraft().unwrap().controls;
        assert_eq!((c.trim, c.flap_notch), (-1.0, 0));
    }

    #[test]
    fn status_lines() {
        let mut s = session();
        let home = s.vehicle().home.expect("seed 1 has a home airport");
        let line = s.status_text();
        assert!(line.starts_with("Wind 250° 0 kt · Aerodrome: 0."), "{line}");
        let qnh = (s.world.weather.air.qnh(home.elevation) / sim_core::HPA).floor();
        let runway = format!("runway {:02}/{:02}, {:.0} ft, QNH {qnh:.0} hPa", home.number(), home.number() + 18, home.elevation / FEET);
        assert!(line.ends_with(&runway), "{line}");
        assert!(!line.contains('\n'));

        // Outside the dials show what the instruments read; the text does not repeat it.
        s.key_pressed(KeyCode::KeyV);
        assert_eq!(s.status_text(), line);
    }

    #[test]
    fn cockpit_view_looks_along_the_nose_and_the_head_turns() {
        let mut s = session();
        let state = s.aircraft().unwrap().state;
        let nose = to_render(act(state.q, V3::X));
        let v = s.view();
        assert!((v.forward - nose).length() < 1e-5);
        let seat = kinds::spots(AircraftKind::C172)[0];
        assert_eq!(v.shift_y, seat.shift);
        assert!((v.position - state.pos).length() < 0.7);
        assert!((v.fov_y - seat.fov).abs() < 1e-6);

        // X turns the head to the right at 3.5 rad/s up to 110°, and it comes back when released.
        hold(&mut s, Held { look: 1.0, ..Held::default() }, 0.2);
        assert!((s.look_yaw - 0.7).abs() < 1e-9);
        hold(&mut s, Held { look: 1.0, ..Held::default() }, 2.0);
        assert!((s.look_yaw - 110.0 * DEG).abs() < 1e-9);
        let right_wing = to_render(act(s.aircraft().unwrap().state.q, V3::Y));
        assert!(s.view().forward.dot(right_wing) > 0.9);
        hold(&mut s, Held::default(), 1.0);
        assert_eq!(s.look_yaw, 0.0);

        // Dragging looks around within limits; letting go leaves the head there, the mouse alone
        // no longer moves it, and 0 looks ahead.
        s.set_looking(true);
        s.mouse_moved(10_000.0, 10_000.0);
        assert_eq!((s.look_yaw, s.look_pitch), (2.6, -1.0));
        s.set_looking(false);
        hold(&mut s, Held::default(), 1.0);
        assert_eq!((s.look_yaw, s.look_pitch), (2.6, -1.0));
        s.mouse_moved(100.0, 100.0);
        assert_eq!((s.look_yaw, s.look_pitch), (2.6, -1.0));
        s.key_pressed(KeyCode::Digit0);
        assert_eq!((s.look_yaw, s.look_pitch), (0.0, 0.0));
    }

    /// Looking ahead from the seat, both displays are whole on the screen, below the horizon, and
    /// the throttle can be taken with the mouse and pushed in.
    #[test]
    fn the_panel_is_in_view_and_its_levers_can_be_taken() {
        let mut s = session();
        let v = s.view();
        let eye = v.position - s.aircraft().unwrap().state.pos;
        let seat = kinds::spots(AircraftKind::C172)[0].eye;
        assert!((eye.length() - seat.length() as f64).abs() < 1e-6);
        let f = 1.0 / (v.fov_y / 2.0).tan();
        // The displays' pictures: the cockpit's vertices that shine with the PFD's or the MFD's part of the canvas.
        let file = kinds::cockpit(AircraftKind::C172);
        let spec = kinds::cockpit_spec(AircraftKind::C172).unwrap();
        let (w, h) = file.canvas;
        for display in file.instruments.iter().filter(|i| matches!(i.kind, cockpit::instruments::Instrument::Pfd | cockpit::instruments::Instrument::Mfd)) {
            let [x0, y0, dx, dy] = display.at;
            let corners: Vec<glam::Vec3> = spec.model.mesh.data.as_chunks::<10>().0.iter().filter(|c| c[9].floor() == 7.0 && c[8] == 0.0 && (x0..=x0 + dx).contains(&(c[6] * w)) && (y0..=y0 + dy).contains(&(c[7] * h))).map(|c| glam::Vec3::new(c[0], c[1], c[2])).collect();
            assert!(corners.len() >= 4, "{:?}", display.kind);
            for c in corners {
                let ndc = v.shift_y - f * (c.z - seat.z) / (c.x - seat.x);
                assert!(ndc > -0.95 && ndc < v.shift_y - 0.3, "a display's edge at {ndc} of the screen's half height");
            }
        }
        // The throttle is low on the panel: the head bends down to it.
        s.hold_look(0.0, -0.35);
        let v = s.view();
        let screen = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(1600.0, 1000.0));
        s.controls().unwrap().throttle = 0.0;
        let levers = kinds::cockpit(AircraftKind::C172).levers.clone();
        let knob = levers.iter().find(|(lever, _)| *lever == Lever::Throttle).expect("a throttle").1;
        let idle = cockpit::on_screen(&v, screen, s.vehicle(), knob[0]).expect("the throttle in front of the eye");
        assert!(screen.contains(idle), "throttle at {idle:?}");
        let grip = Grip::take(&v, screen, s.vehicle(), &levers, idle + egui::vec2(3.0, 2.0)).expect("the throttle taken");
        assert_eq!(grip.lever, Lever::Throttle);
        let full = cockpit::on_screen(&v, screen, s.vehicle(), knob[1]).unwrap();
        s.set_lever(grip.lever, grip.value(full + egui::vec2(3.0, 2.0)));
        assert!(s.aircraft().unwrap().controls.throttle > 0.95, "{}", s.aircraft().unwrap().controls.throttle);
    }

    /// The Extra 300L flies from its rear seat, its rear panel ahead and below the eye, in the view.
    #[test]
    fn the_extra_s_rear_panel_is_in_view() {
        let mut s = session();
        s.set_craft(Craft::Aircraft(AircraftKind::EXTRA300));
        let v = s.view();
        let eye = v.position - s.aircraft().unwrap().state.pos;
        assert!((eye.length() - kinds::spots(AircraftKind::EXTRA300)[0].eye.length() as f64).abs() < 1e-6);
        let screen = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(1600.0, 1000.0));
        for spot in [glam::Vec3::new(-0.07, -0.15, -0.42), glam::Vec3::new(-0.07, 0.15, -0.42)] {
            let at = cockpit::on_screen(&v, screen, s.vehicle(), spot).expect("ahead of the eye");
            assert!(screen.contains(at) && at.y > 500.0, "an instrument at {at:?}");
        }
    }

    #[test]
    fn outside_view_follows_17_m_behind_and_orbits() {
        let mut s = session();
        s.key_pressed(KeyCode::KeyV);
        let pos = s.aircraft().unwrap().state.pos;
        let v = s.view();
        assert!(((v.position - pos).length() - 17.0).abs() < 1e-6);
        assert_eq!(v.shift_y, 0.0);
        assert!(-v.position.z > -pos.z, "the camera starts above the aircraft");
        let nose = to_render(act(s.aircraft().unwrap().state.q, V3::X));
        assert!(v.forward.dot(nose) > 0.9);
        for axes in [v.forward.dot(v.up), v.forward.dot(v.right), v.up.dot(v.right)] {
            assert!(axes.abs() < 1e-5);
        }
        assert!(v.up.y > 0.9);

        hold(&mut s, Held { look: 1.0, ..Held::default() }, 1.0);
        assert!((s.orbit_yaw - 1.6).abs() < 1e-9);
        // Low orbits stay above the ground.
        s.set_looking(true);
        s.mouse_moved(0.0, -10_000.0);
        assert_eq!(s.orbit_pitch, -0.4);
        let v = s.view();
        let ground = s.world.terrain.height(v.position.x, v.position.y).max(0.0);
        assert!(-v.position.z >= ground + 1.5 - 1e-9);
    }

    #[test]
    fn the_wheel_zooms_the_cockpit_s_view_smoothly_where_it_looks_and_it_stays() {
        let mut s = session();
        s.restart_in_air();
        s.frozen = true;
        // The head turned with the mouse, and left there.
        s.set_looking(true);
        s.mouse_moved(120.0, 60.0);
        s.set_looking(false);
        let before = s.view();
        let wide = (before.fov_y / 2.0).tan();
        let same = |a: &ViewPoint, b: &ViewPoint| (a.position - b.position).length() < 1e-9 && (a.forward - b.forward).length() < 1e-6 && a.shift_y == b.shift_y;
        // Two lines of the wheel, in parts as a trackpad gives them: no step, no other spot; the
        // picture eases there and stays, the eye and the head where they were.
        for _ in 0..8 {
            s.zoom(0.25);
        }
        assert_eq!((s.spot, s.view().fov_y), (0, before.fov_y), "nothing jumps");
        let mut last = wide;
        for frame in 0..90 {
            s.advance(1.0 / 60.0, &Held::default());
            let v = s.view();
            let now = (v.fov_y / 2.0).tan();
            assert!(same(&v, &before), "frame {frame}: the eye or the head moved");
            assert!(now <= last && last / now < 1.04, "frame {frame}: {last} to {now}");
            last = now;
        }
        assert!((wide / last / ZOOM_STEP.powi(2) as f32 - 1.0).abs() < 1e-4, "{}", wide / last);
        assert!(last < wide * 0.9 && (s.look_yaw, s.spot) != (0.0, 1));
        for _ in 0..120 {
            s.advance(1.0 / 60.0, &Held::default());
        }
        assert!(((s.view().fov_y / 2.0).tan() / last - 1.0).abs() < 1e-4 && same(&s.view(), &before), "it stays");
        // + and − are a line of the wheel each; the zoom ends at six times and at 0.8.
        let zoom = s.zoom;
        s.key_pressed(KeyCode::Equal);
        assert!((s.zoom / zoom - ZOOM_STEP).abs() < 1e-12);
        s.key_pressed(KeyCode::Minus);
        s.key_pressed(KeyCode::Minus);
        assert!((s.zoom * ZOOM_STEP / zoom - 1.0).abs() < 1e-12);
        s.zoom(100.0);
        assert_eq!((s.zoom, s.spot), (ZOOM_RANGE.1, 0));
        s.zoom(-200.0);
        assert_eq!(s.zoom, ZOOM_RANGE.0);
        // The spots are Space's and `--spot`'s: another spot is seen as it is, without the zoom.
        s.zoom(5.0);
        s.set_spot(3);
        assert_eq!((s.spot_target().name, s.zoom), ("PFD", 1.0));
        s.advance(1.0, &Held::default());
        let seat = before.position;
        let pfd = s.spot_target();
        let v = s.view();
        let eye = v.position - s.aircraft().unwrap().state.pos;
        assert!((s.camera.eye - pfd.eye).length() < 1e-3, "the eye reaches the spot");
        assert!((eye - seat + s.aircraft().unwrap().state.pos).length() > 0.2, "and leaves the seat");
        assert!((v.fov_y - pfd.fov).abs() < 1e-3);
        s.set_spot(100);
        assert_eq!(s.spot, kinds::spots(AircraftKind::C172).len() - 1);
        s.key_pressed(KeyCode::KeyV);
        s.key_pressed(KeyCode::KeyV);
        assert_eq!(s.spot, 0, "the view key comes back to the seat");
        s.key_pressed(KeyCode::Space);
        assert_eq!(s.spot_target().name, "pilot, leaning in", "space: leaning in");
        s.key_pressed(KeyCode::Space);
        assert_eq!(s.spot, 0, "and back to the seat");
        s.set_spot(4);
        s.key_pressed(KeyCode::Space);
        assert_eq!(s.spot, 0, "from any spot");
        s.key_pressed(KeyCode::KeyV);
        assert!(!s.inside());
        s.key_pressed(KeyCode::Space);
        assert!(s.inside() && s.spot == 0, "from outside, into the cockpit in the seat");
        s.dim_panel(-1.0);
        assert_eq!((s.panel_lights, s.standby_lights), (0.0, 0.0));
    }

    #[test]
    fn dragging_looks_the_way_the_setting_says() {
        let mut s = session();
        s.restart_in_air();
        let drag = |s: &mut Session| {
            s.set_looking(true);
            s.mouse_moved(50.0, 30.0);
            s.set_looking(false);
        };
        // As ever: right looks right, down looks down.
        drag(&mut s);
        assert!((s.look_yaw - 0.2).abs() < 1e-12 && (s.look_pitch + 0.12).abs() < 1e-12);
        // Natural: the scene goes with the pointer, so down looks up and right looks left; at once.
        s.store.set(settings::LOOK_NATURAL, true);
        drag(&mut s);
        drag(&mut s);
        assert!((s.look_yaw + 0.2).abs() < 1e-12 && (s.look_pitch - 0.12).abs() < 1e-12);
        // The outside camera's drag too.
        s.key_pressed(KeyCode::KeyV);
        let (yaw, pitch) = (s.orbit_yaw, s.orbit_pitch);
        drag(&mut s);
        assert!((s.orbit_yaw - yaw + 0.25).abs() < 1e-12 && (s.orbit_pitch - pitch + 0.15).abs() < 1e-12);
        s.store.set(settings::LOOK_NATURAL, false);
        drag(&mut s);
        assert!((s.orbit_yaw - yaw).abs() < 1e-12 && (s.orbit_pitch - pitch).abs() < 1e-12);
    }

    #[test]
    fn the_wheel_brings_the_outside_camera_closer_or_further() {
        let mut s = session();
        s.restart_in_air();
        s.zoom(3.0);
        assert_eq!(s.orbit_distance, ORBIT_DISTANCE, "the wheel does nothing in the cockpit");
        s.key_pressed(KeyCode::KeyV);
        let pos = s.aircraft().unwrap().state.pos;
        s.frozen = true;
        s.zoom(3.0);
        // It eases there, without a jump.
        let mut last = (s.view().position - pos).length();
        assert!((last - ORBIT_DISTANCE).abs() < 1e-6);
        for _ in 0..180 {
            s.advance(1.0 / 60.0, &Held::default());
            let now = (s.view().position - pos).length();
            assert!(now <= last && last - now < 0.7, "{last} to {now}");
            last = now;
        }
        assert!((last - ORBIT_DISTANCE * 0.729).abs() < 1e-4, "{last}");
        s.zoom(-100.0);
        assert_eq!(s.orbit_distance, 150.0);
        s.zoom(100.0);
        assert_eq!(s.orbit_distance, 5.0);
    }

    #[test]
    fn scripted_takeoff_with_the_assist() {
        let mut s = session();
        assert!(s.pilot.assist);
        hold(&mut s, Held { throttle_up: true, ..Held::default() }, 3.0);
        assert_eq!(s.aircraft().unwrap().controls.throttle, 1.0);
        // As the help says: down arrow from 55 knots, here until the nose is 8° up.
        for _ in 0..60 * 45 {
            let pull = s.aircraft().unwrap().indicated_airspeed() > 55.0 * KNOTS && euler_angles(s.aircraft().unwrap().state.q).pitch < 8.0 * DEG;
            s.advance(FRAME, &Held { pitch: if pull { 1.0 } else { 0.0 }, ..Held::default() });
        }
        assert!(s.aircraft().unwrap().crashed().is_none(), "{:?}", s.aircraft().unwrap().crashed());
        assert!(s.vehicle().agl() > 15.0, "agl {}", s.vehicle().agl());
        assert!((s.world.time() - 48.0).abs() < 0.05, "the clock follows real time");

        // The instruments show the flight.
        let panel = s.panel_data();
        assert!(panel.airspeed_kt > 50.0 && panel.rpm > 2000.0 && panel.throttle == 1.0);
        assert!(panel.vertical_speed_fpm > 100.0, "{}", panel.vertical_speed_fpm);

        // Back on the runway with 1: stopped, engine idling, controls centred.
        s.key_pressed(KeyCode::Digit1);
        assert!(s.vehicle().agl() < 5.0 && s.aircraft().unwrap().controls.throttle == 0.0);
        assert_eq!((s.pilot.yoke_x(), s.pilot.yoke_y()), (0.0, 0.0));
    }

    #[test]
    fn a_frame_never_runs_more_than_a_tenth_of_a_second() {
        let mut s = session();
        s.advance(0.25, &Held::default());
        assert!((s.world.time() - 0.1).abs() < World::DT);
    }

    #[test]
    fn frozen_for_screenshots() {
        let mut s = session();
        s.frozen = true;
        hold(&mut s, Held { throttle_up: true, ..Held::default() }, 1.0);
        assert_eq!((s.world.time(), s.aircraft().unwrap().controls.throttle), (0.0, 0.0));
        assert_eq!(s.message_text(), "");
    }

    #[test]
    fn without_a_gfc_700_p_holds_heading_and_altitude_and_the_arrows_move_the_targets() {
        // The Extra 300L: the game's autopilot.
        let mut s = session();
        s.set_craft(Craft::Aircraft(AircraftKind::EXTRA300));
        assert!(s.gfc.is_none());
        // On the ground there is nothing to hold.
        s.key_pressed(KeyCode::KeyP);
        assert!(s.autopilot.is_none());

        s.key_pressed(KeyCode::Digit2);
        hold(&mut s, Held::default(), 1.0);
        let heading = euler_angles(s.aircraft().unwrap().state.q).heading;
        let altitude = s.aircraft().unwrap().indicated_altitude();
        let airspeed = s.aircraft().unwrap().indicated_airspeed();
        s.key_pressed(KeyCode::KeyP);
        let expected = format!("AP  HDG {:03.0}°  ALT {:.0} ft  IAS {:.0} kt", heading / DEG, altitude / FEET, airspeed / KNOTS);
        assert!(s.annunciators().contains(&expected), "{:?}", s.annunciators());

        // Two steps right and three up; the held arrows no longer move the yoke.
        for key in [KeyCode::ArrowRight, KeyCode::ArrowRight, KeyCode::ArrowUp, KeyCode::ArrowUp, KeyCode::ArrowUp] {
            s.key_pressed(key);
        }
        let Some(Goal::Hold { heading: Some(h), vertical: Vertical::Altitude(a), airspeed: Some(v) }) = s.autopilot.as_ref().map(|a| a.goal().clone()) else {
            panic!("not holding");
        };
        assert!((h - heading - 10.0 * DEG).abs() < 1e-9 && (a - altitude - 300.0 * FEET).abs() < 1e-9);
        assert_eq!(v, airspeed);
        hold(&mut s, Held { roll: -1.0, pitch: 1.0, ..Held::default() }, 60.0);
        let now = euler_angles(s.aircraft().unwrap().state.q);
        assert!((now.heading - h).abs() < 1.0 * DEG, "heading {}", now.heading / DEG);
        assert!(now.roll.abs() < 2.0 * DEG);
        assert!((s.aircraft().unwrap().indicated_altitude() - a).abs() < 5.0, "altitude {}", s.aircraft().unwrap().indicated_altitude());

        // W and S move the airspeed it holds, within 90 to 155 knots, and not the throttle lever.
        hold(&mut s, Held { throttle_down: true, ..Held::default() }, 2.0);
        let Some(Goal::Hold { airspeed: Some(slower), .. }) = s.autopilot.as_ref().map(|a| a.goal().clone()) else { panic!("not holding") };
        assert!((slower - v + 16.0 * KNOTS).abs() < 1e-6);
        hold(&mut s, Held { throttle_down: true, ..Held::default() }, 10.0);
        hold(&mut s, Held::default(), 30.0);
        assert!((s.aircraft().unwrap().indicated_airspeed() / KNOTS - 90.0).abs() < 2.0, "{}", s.aircraft().unwrap().indicated_airspeed() / KNOTS);
        assert!(s.annunciators().iter().any(|t| t.contains("IAS 90 kt")));

        // P again, or C, hands the aircraft back, in trim and holding its attitude.
        s.key_pressed(KeyCode::KeyP);
        assert!(s.autopilot.is_none());
        assert!(!s.annunciators().iter().any(|t| t.starts_with("AP")));
        let before = s.aircraft().unwrap().indicated_altitude();
        hold(&mut s, Held::default(), 10.0);
        assert!((s.aircraft().unwrap().indicated_altitude() - before).abs() < 15.0, "{}", s.aircraft().unwrap().indicated_altitude() - before);
        s.key_pressed(KeyCode::KeyP);
        s.key_pressed(KeyCode::KeyC);
        assert!(s.autopilot.is_none());
        // The arrows are the yoke's again, and a restart lets go of the autopilot too.
        s.key_pressed(KeyCode::ArrowUp);
        s.key_pressed(KeyCode::KeyP);
        s.key_pressed(KeyCode::Digit3);
        assert!(s.autopilot.is_none());
    }

    /// P alone holds the heading and the altitude of now; the arrows then move the heading bug and
    /// the selected altitude, and the aircraft turns and climbs to them; P or C give it back.
    #[test]
    fn p_holds_heading_and_altitude_with_the_gfc_700_and_the_arrows_move_its_targets() {
        let mut s = session();
        s.key_pressed(KeyCode::Digit2);
        hold(&mut s, Held::default(), 2.0);
        let gfc = |s: &Session| s.gfc.clone().expect("a GFC 700");
        let heading = |s: &Session| s.aircraft().unwrap().instruments.readings().ahrs[2];
        let altitude = |s: &Session| s.aircraft().unwrap().instruments.readings().air_data.altitude;
        let off = |a: f64, b: f64| sim_core::remainder(a - b, 2.0 * std::f64::consts::PI).abs();
        let (h0, a0) = (heading(&s), altitude(&s));
        // P: AP in HDG on the heading of now and ALT at the altitude of now, no other key.
        s.key_pressed(KeyCode::KeyP);
        let g = gfc(&s);
        assert!(g.engaged() && s.autopilot.is_none());
        assert_eq!(g.lateral(), sim_core::LateralMode::Heading);
        assert!(off(g.heading_bug(), h0) < 0.5 * DEG, "bug {}", g.heading_bug() / DEG);
        assert!(matches!(g.vertical(), VerticalMode::Altitude(a) if (a - a0).abs() < 10.0 * FEET), "{:?}", g.vertical());
        assert!((g.selected_altitude() - a0).abs() <= 50.0 * FEET && (g.selected_altitude() / FEET).round() % 100.0 == 0.0);
        hold(&mut s, Held::default(), 20.0);
        assert!(off(heading(&s), h0) < 2.0 * DEG && (altitude(&s) - a0).abs() < 30.0 * FEET, "it holds them");
        // → four times and ⇧→ twice: the bug 22° right; ← back 5°. Held, an arrow moves no yoke.
        for _ in 0..4 {
            s.key_pressed(KeyCode::ArrowRight);
        }
        s.key_pressed_with(KeyCode::ArrowRight, true);
        s.key_pressed_with(KeyCode::ArrowRight, true);
        s.key_pressed(KeyCode::ArrowLeft);
        let bug = gfc(&s).heading_bug();
        assert!((sim_core::remainder(bug - g.heading_bug(), 2.0 * std::f64::consts::PI) / DEG - 17.0).abs() < 0.51, "bug {}", bug / DEG);
        assert_eq!(s.knobs.turning(), Some(PanelKnob::Heading));
        hold(&mut s, Held { roll: 1.0, ..Held::default() }, 3.0);
        assert!(gfc(&s).engaged(), "an arrow held does not disconnect it");
        hold(&mut s, Held::default(), 30.0);
        assert!(off(heading(&s), bug) < 2.0 * DEG, "heading {} for {}", heading(&s) / DEG, bug / DEG);
        // ↑ three times: 300 ft more, VS towards it with ALTS armed; with throttle it climbs and levels there.
        let selected = gfc(&s).selected_altitude();
        for _ in 0..3 {
            s.key_pressed(KeyCode::ArrowUp);
        }
        let g = gfc(&s);
        assert!((g.selected_altitude() - selected - 300.0 * FEET).abs() < 1.0);
        assert!(matches!(g.vertical(), VerticalMode::VerticalSpeed(v) if (v - ARROW_CLIMB).abs() < 1e-9) && g.alts_armed(), "{:?}", g.vertical());
        s.controls().unwrap().throttle = 1.0;
        hold(&mut s, Held::default(), 90.0);
        assert!(matches!(gfc(&s).vertical(), VerticalMode::Altitude(a) if a == g.selected_altitude()), "{:?}", gfc(&s).vertical());
        assert!((altitude(&s) - g.selected_altitude()).abs() < 40.0 * FEET, "{} ft", altitude(&s) / FEET);
        assert_eq!(gfc(&s).lateral(), sim_core::LateralMode::Heading);
        // ⇧↓: a thousand feet down, descending; ⇧↑ back: at the altitude held, ALT again.
        s.key_pressed_with(KeyCode::ArrowDown, true);
        assert!(matches!(gfc(&s).vertical(), VerticalMode::VerticalSpeed(v) if v < 0.0), "{:?}", gfc(&s).vertical());
        assert!((gfc(&s).selected_altitude() - g.selected_altitude() + 1000.0 * FEET).abs() < 1.0);
        s.key_pressed_with(KeyCode::ArrowUp, true);
        assert!(matches!(gfc(&s).vertical(), VerticalMode::Altitude(_)), "{:?}", gfc(&s).vertical());
        // P again gives the aircraft back; so does C. The arrows are the yoke's then.
        s.key_pressed(KeyCode::KeyP);
        assert!(!gfc(&s).engaged());
        let bug = gfc(&s).heading_bug();
        s.key_pressed(KeyCode::ArrowRight);
        assert_eq!(gfc(&s).heading_bug(), bug);
        // With the flight director left on with its modes, P couples the autopilot to them as they are.
        s.key_pressed(KeyCode::Digit8);
        s.key_pressed(KeyCode::KeyP);
        assert!(gfc(&s).engaged() && matches!(gfc(&s).vertical(), VerticalMode::VerticalSpeed(_)));
        s.key_pressed(KeyCode::KeyC);
        assert!(!gfc(&s).engaged());
    }

    #[test]
    fn the_c172_s_gfc_700_from_the_keys() {
        let mut s = session();
        // On the ground the AP does not engage, and says why.
        hold(&mut s, Held::default(), 0.5);
        s.key_pressed(KeyCode::KeyP);
        assert!(!s.gfc.as_ref().expect("a GFC 700").engaged());
        assert!(s.message_text().contains("ground"), "{}", s.message_text());

        s.key_pressed(KeyCode::Digit2);
        hold(&mut s, Held::default(), 1.0);
        let gfc = |s: &Session| s.gfc.clone().expect("a GFC 700");
        // The bezel's AP key is the real one: the flight director in ROL and PIT.
        s.panel(Panel::Key(AfcsKey::Ap));
        assert!(gfc(&s).engaged());
        assert!(s.annunciators().iter().any(|t| t.starts_with("AP  ROL · PIT")), "{:?}", s.annunciators());
        // ] three clicks and one of ten, then 4 (HDG) and 7 (ALT).
        let bug = gfc(&s).heading_bug();
        for _ in 0..3 {
            s.key_pressed(KeyCode::BracketRight);
        }
        s.key_pressed_with(KeyCode::BracketRight, true);
        s.key_pressed(KeyCode::Digit4);
        s.key_pressed(KeyCode::Digit7);
        let g = gfc(&s);
        assert!((sim_core::remainder(g.heading_bug() - bug, 2.0 * std::f64::consts::PI) / DEG - 13.0).abs() < 1.0, "bug {}", g.heading_bug() / DEG);
        let VerticalMode::Altitude(held) = g.vertical() else { panic!("not ALT") };
        let tag = format!("AP  HDG {:03.0}° · ALT {:.0} ft", (g.heading_bug() / DEG).round().rem_euclid(360.0), held / FEET);
        assert!(s.annunciators().iter().any(|t| t.starts_with(&tag)), "{:?} {tag}", s.annunciators());
        hold(&mut s, Held::default(), 40.0);
        let heading = s.aircraft().unwrap().instruments.readings().ahrs[2];
        assert!(sim_core::remainder(heading - g.heading_bug(), 2.0 * std::f64::consts::PI).abs() < 2.0 * DEG, "heading {}", heading / DEG);

        // ' twice selects 200 ft more; 8 (VS) and the wheel (.) climb to it, ALTS armed.
        s.key_pressed(KeyCode::Quote);
        s.key_pressed(KeyCode::Quote);
        s.key_pressed(KeyCode::Digit8);
        for _ in 0..5 {
            s.key_pressed(KeyCode::Period);
        }
        assert!(s.annunciators().iter().any(|t| t.contains("VS +500 ft/min (ALTS")), "{:?}", s.annunciators());
        // No autothrottle: W is the throttle's, as by hand.
        let throttle = s.aircraft().unwrap().controls.throttle;
        s.controls().unwrap().throttle = 0.4;
        hold(&mut s, Held { throttle_up: true, ..Held::default() }, 1.0);
        assert!(s.aircraft().unwrap().controls.throttle > 0.8);
        s.controls().unwrap().throttle = throttle.max(0.8);
        hold(&mut s, Held::default(), 80.0);
        let selected = gfc(&s).selected_altitude();
        assert!((selected - held - 200.0 * FEET).abs() < 100.0 * FEET);
        assert!(matches!(gfc(&s).vertical(), VerticalMode::Altitude(a) if a == selected), "{:?}", gfc(&s).vertical());

        // An arrow held does not overpower the servos: with the autopilot engaged the arrows are
        // its targets'. A stick pushed against them does; the tone sounds for two seconds, the
        // flight director stays.
        hold(&mut s, Held { roll: 1.0, pitch: -1.0, ..Held::default() }, 1.0);
        assert!(gfc(&s).engaged());
        s.gfc.as_mut().expect("a GFC 700").push_yoke(1.0, 1.0);
        assert!(!gfc(&s).engaged() && gfc(&s).director() && gfc(&s).tone());
        assert!(s.annunciators().iter().any(|t| t.starts_with("FD  HDG")), "{:?}", s.annunciators());
        hold(&mut s, Held::default(), 3.0);
        assert!(!gfc(&s).tone());
        // C is AP DISC; the trim by hand (without the assist, Q and E) disconnects too.
        s.key_pressed(KeyCode::KeyP);
        s.key_pressed(KeyCode::KeyC);
        assert!(!gfc(&s).engaged());
        s.key_pressed(KeyCode::KeyP);
        s.set_assist(false);
        hold(&mut s, Held { fine: 1.0, ..Held::default() }, 0.2);
        assert_eq!(gfc(&s).alert().map(|a| a.why), Some(sim_core::Disconnect::ManualTrim));
        // L: the game's autopilot lands, the GFC 700 lets go.
        s.key_pressed(KeyCode::KeyP);
        s.key_pressed(KeyCode::KeyL);
        assert!(s.autopilot.is_some() && !gfc(&s).engaged());
        // NAV without a plan says why; / (D→) makes one and NAV follows it. (P first hands the
        // aircraft back from the game's autopilot.)
        s.key_pressed(KeyCode::KeyP);
        assert!(s.autopilot.is_none());
        hold(&mut s, Held::default(), 3.5);
        s.key_pressed(KeyCode::Digit5);
        assert!(s.message_text().contains("plan"), "{}", s.message_text());
        s.key_pressed(KeyCode::Slash);
        s.key_pressed(KeyCode::Digit5);
        assert_eq!(gfc(&s).annunciations()[0], "GPS");
        // A restart: everything off.
        s.key_pressed(KeyCode::Digit3);
        assert!(!gfc(&s).engaged() && !gfc(&s).director());
    }

    #[test]
    fn the_bezel_s_keys_and_knobs_are_clicked_where_they_are_drawn() {
        let mut s = session();
        s.key_pressed(KeyCode::Digit2);
        hold(&mut s, Held::default(), 1.0);
        let v = s.view();
        let screen = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(1600.0, 1000.0));
        let controls = kinds::cockpit(AircraftKind::C172).controls.clone();
        // AP, FD, HDG, ALT, NAV, VNV, APR, BC, VS, FLC, NOSE UP, NOSE DN and D→ on both displays,
        // the HDG, ALT and CRS / BARO knobs.
        assert_eq!(controls.len(), 2 * (13 + 3));
        for c in &controls {
            // A millimetre right of the middle: a knob's right half.
            let at = cockpit::on_screen(&v, screen, s.vehicle(), c.centre + glam::Vec3::Y * 0.001).expect("in front of the eye");
            let found = cockpit::panel_at(&v, screen, s.vehicle(), &controls, at).map(|over| over.does);
            assert_eq!(cockpit::panel_at(&v, screen, s.vehicle(), &controls, at).map(|over| over.control), Some(*c));
            match c.shape {
                cockpit::PanelShape::Key { does, .. } => assert_eq!(found, Some(does)),
                cockpit::PanelShape::Knob { outer, inner, .. } => assert_eq!(found, Some(Panel::Knob(inner.map_or(outer, |(_, k)| k), 1))),
            }
        }
        // The left half of the HDG knob turns the bug back.
        let knob = controls.iter().find(|c| matches!(c.shape, cockpit::PanelShape::Knob { outer: PanelKnob::Heading, .. })).expect("HDG knob");
        let left = cockpit::on_screen(&v, screen, s.vehicle(), knob.centre - glam::Vec3::Y * 0.005).expect("in view");
        assert_eq!(cockpit::panel_at(&v, screen, s.vehicle(), &controls, left).map(|over| over.does), Some(Panel::Knob(PanelKnob::Heading, -1)));
        // A knob takes the pointer 7 mm beyond its edge, a key less than 2 mm: between two keys,
        // the nearer one; well clear of them all, nothing.
        let on_panel = |s: &Session, body: glam::Vec3| cockpit::on_screen(&v, screen, s.vehicle(), body).and_then(|at| cockpit::panel_at(&v, screen, s.vehicle(), &controls, at)).map(|over| over.does);
        assert_eq!(on_panel(&s, knob.centre + glam::Vec3::new(0.0, 0.01, -0.01)), Some(Panel::Knob(PanelKnob::Heading, 1)));
        assert_eq!(on_panel(&s, knob.centre + glam::Vec3::new(0.0, -0.012, 0.012)), None);
        let key = |wanted: AfcsKey| *controls.iter().find(|c| matches!(c.shape, cockpit::PanelShape::Key { does: Panel::Key(k), .. } if k == wanted)).expect("the key");
        let (hdg, alt, nav) = (key(AfcsKey::Hdg), key(AfcsKey::Alt), key(AfcsKey::Nav));
        let between = (hdg.centre + alt.centre) / 2.0;
        assert_eq!(on_panel(&s, between - glam::Vec3::Y * 0.0005), Some(Panel::Key(AfcsKey::Hdg)));
        assert_eq!(on_panel(&s, between + glam::Vec3::Y * 0.0005), Some(Panel::Key(AfcsKey::Alt)));
        assert_eq!(on_panel(&s, (hdg.centre + nav.centre) / 2.0 + glam::Vec3::Z * 0.0005), Some(Panel::Key(AfcsKey::Nav)));
        assert_eq!(on_panel(&s, hdg.centre - glam::Vec3::Y * 0.0125), None, "left of the keys");
        // The ALT knob's outer ring beyond its edge, its inner knob in the middle.
        let alt_knob = controls.iter().find(|c| c.has(PanelKnob::AltitudeFine)).expect("ALT knob");
        assert_eq!(on_panel(&s, alt_knob.centre + glam::Vec3::new(0.0, 0.002, 0.0138)), Some(Panel::Knob(PanelKnob::AltitudeCoarse, 1)));
        assert_eq!(on_panel(&s, alt_knob.centre - glam::Vec3::Y * 0.003), Some(Panel::Knob(PanelKnob::AltitudeFine, -1)));
        // Clicked, AP engages.
        let ap = controls.iter().find(|c| matches!(c.shape, cockpit::PanelShape::Key { does: Panel::Key(AfcsKey::Ap), .. })).expect("AP key");
        let at = cockpit::on_screen(&v, screen, s.vehicle(), ap.centre).expect("in view");
        let press = cockpit::panel_at(&v, screen, s.vehicle(), &controls, at).expect("the AP key");
        s.panel(press.does);
        assert!(s.gfc.as_ref().is_some_and(Gfc700::engaged));
    }

    /// Turned by the wheel, a click or its key, a knob's drawing turns its clicks, and the tag
    /// beside it says what it is set to; Shift and the wheel over the HDG knob or a key is the ALT knob.
    #[test]
    fn the_g1000_s_knobs_turn_and_say_what_they_are_set_to() {
        let mut s = session();
        s.key_pressed(KeyCode::Digit2);
        hold(&mut s, Held::default(), 1.0);
        s.afcs_command("bug=80").unwrap();
        s.afcs_command("sel=5500").unwrap();
        let shown = |s: &Session, knob: PanelKnob| s.knobs.shown()[knob as usize];
        assert_eq!(s.knobs.shown(), [0.0; PanelKnob::COUNT]);
        assert_eq!(s.knob_name(PanelKnob::Heading), "HDG 080°");
        assert_eq!(s.knob_name(PanelKnob::AltitudeCoarse), "ALT 5500 ft");
        let baro = s.aircraft().expect("an aircraft").instruments.altimeter_setting / HPA;
        // The wheel over the HDG knob: three clicks.
        let over = Some(Panel::Knob(PanelKnob::Heading, 1));
        s.panel(Panel::Knob(cockpit::wheel_knob(over, false).unwrap(), 3));
        // The keys: ] and ' and Shift+; and K.
        s.key_pressed(KeyCode::BracketRight);
        s.key_pressed(KeyCode::Quote);
        s.key_pressed_with(KeyCode::Semicolon, true);
        s.key_pressed(KeyCode::KeyK);
        s.panel(Panel::Knob(PanelKnob::Baro, -3));
        assert_eq!(s.knobs.turning(), Some(PanelKnob::Baro));
        hold(&mut s, Held::default(), 0.5);
        for (knob, clicks) in [(PanelKnob::Heading, 4.0), (PanelKnob::AltitudeFine, 1.0), (PanelKnob::AltitudeCoarse, -1.0), (PanelKnob::Baro, -2.0), (PanelKnob::Course, 0.0)] {
            assert!((shown(&s, knob) - clicks).abs() < 1e-3, "{knob:?}: {}", shown(&s, knob));
        }
        assert_eq!(s.knob_name(PanelKnob::Heading), "HDG 084°");
        // 100 ft more, and the outer ring a thousand back from the nearest one.
        assert_eq!(s.knob_name(PanelKnob::AltitudeFine), "ALT 5000 ft");
        let baro = format!("BARO {:.0} hPa", baro.round() - 2.0);
        assert_eq!(s.knob_name(PanelKnob::Baro), baro);
        // Shift and the wheel over the HDG knob, or over a key, is the ALT knob's inner one.
        s.panel(Panel::Knob(cockpit::wheel_knob(over, true).unwrap(), 2));
        s.panel(Panel::Knob(cockpit::wheel_knob(Some(Panel::Key(AfcsKey::Nav)), true).unwrap(), 2));
        assert_eq!(s.knob_name(PanelKnob::AltitudeFine), "ALT 5400 ft");
        assert_eq!(s.knob_name(PanelKnob::Heading), "HDG 084°");
        // The tags: the knob and what it is set to, and how the ALT knob is reached.
        assert_eq!(s.panel_tag(Panel::Knob(PanelKnob::Heading, 1), false), ("HDG 084°".to_string(), "Shift + wheel: ALT 5400 ft".to_string()));
        assert_eq!(s.panel_tag(Panel::Knob(PanelKnob::Heading, 1), true), ("ALT 5400 ft".to_string(), "Shift + wheel: 100 ft a click".to_string()));
        assert_eq!(s.panel_tag(Panel::Key(AfcsKey::Ap), false), ("AP · autopilot".to_string(), "Shift + wheel: ALT 5400 ft".to_string()));
        assert_eq!(s.panel_tag(Panel::Key(AfcsKey::Ap), true).0, "ALT 5400 ft");
        assert_eq!(s.panel_tag(Panel::Knob(PanelKnob::AltitudeCoarse, 1), true), ("ALT 5400 ft".to_string(), "outer ring: 1000 ft a click".to_string()));
        assert_eq!(s.panel_tag(Panel::Knob(PanelKnob::AltitudeFine, 1), false).1, "inner knob: 100 ft a click");
        assert_eq!(s.panel_tag(Panel::Knob(PanelKnob::Baro, 1), true), (baro, String::new()));
        assert_eq!(s.panel_tag(Panel::DirectTo, true).1, "");
        // A knob turned for a screenshot.
        s.afcs_command("turn=hdg:-4").unwrap();
        assert_eq!(s.knob_name(PanelKnob::Heading), "HDG 080°");
        assert!(s.afcs_command("turn=vol:1").is_err() && s.afcs_command("turn=hdg").is_err());
        // The knob named for a second and a half after it was turned; standing still it is drawn turning all the same.
        s.frozen = true;
        hold(&mut s, Held::default(), 1.0);
        assert_eq!(s.knobs.turning(), Some(PanelKnob::Heading));
        assert!(shown(&s, PanelKnob::Heading).abs() < 1e-3);
        hold(&mut s, Held::default(), 1.0);
        assert_eq!(s.knobs.turning(), None);
    }

    #[test]
    fn l_lands_at_the_nearest_airport_and_hands_the_aircraft_back() {
        let mut s = session();
        let home = s.vehicle().home.expect("home");
        s.key_pressed(KeyCode::Digit2);
        s.key_pressed(KeyCode::KeyL);
        s.advance(FRAME, &Held::default());
        assert!(s.annunciators().contains(&"AP  landing: joining the circuit".to_string()), "{:?}", s.annunciators());
        let mut seconds = 0.0;
        while s.autopilot.is_some() && seconds < 600.0 {
            hold(&mut s, Held::default(), 1.0);
            seconds += 1.0;
        }
        let plane = s.vehicle();
        let pos = plane.aircraft().unwrap().state.pos;
        assert!(plane.aircraft().unwrap().crashed().is_none(), "{:?}", plane.aircraft().unwrap().crashed());
        assert!(s.autopilot.is_none(), "still flying after {seconds} s");
        assert!(plane.aircraft().unwrap().data().on_ground() && plane.aircraft().unwrap().state.vel.length() < 1.0);
        assert!(home.on_runway(pos.x, pos.y));
    }

    #[test]
    fn traffic_flies_circuits_away_from_the_player_and_n_cycles_the_camera() {
        let mut s = session();
        let home = s.vehicle().home.expect("home");
        s.add_traffic(9);
        assert_eq!(s.planes.len(), 1 + MAX_TRAFFIC);
        let player = s.aircraft().unwrap().state.pos;
        let mut at_home = 0;
        for flown in &s.planes[1..] {
            let plane = s.world.vehicle(flown.id);
            assert!((plane.aircraft().unwrap().state.pos - player).length() > 500.0, "traffic on top of the player");
            let Flier::Traffic(autopilot) = &flown.flier else { panic!("traffic without an autopilot") };
            let Goal::Circuit { airport, repeat: true, .. } = autopilot.goal() else { panic!("not a circuit") };
            at_home += (airport.id == home.id) as usize;
        }
        assert!(at_home >= 3, "{at_home} at home");

        // N goes outside and round the aircraft; V comes back to the player's cockpit.
        s.key_pressed(KeyCode::KeyN);
        assert!(!s.cockpit && s.watched() == s.planes[1].id);
        let other = s.world.vehicle(s.planes[1].id).aircraft().unwrap().state.pos;
        assert!(((s.view().position - other).length() - 17.0).abs() < 1e-6);
        for _ in 0..MAX_TRAFFIC {
            s.key_pressed(KeyCode::KeyN);
        }
        assert_eq!(s.watched(), s.player);
        s.key_pressed(KeyCode::KeyN);
        s.key_pressed(KeyCode::KeyV);
        assert!(s.cockpit && s.watched() == s.player);

        // Three minutes with the player parked: everybody is flying or rolling, nobody wrecked.
        hold(&mut s, Held::default(), 180.0);
        for flown in &s.planes[1..] {
            assert!(s.world.vehicle(flown.id).aircraft().unwrap().crashed().is_none(), "{:?}", s.world.vehicle(flown.id).aircraft().unwrap().crashed());
        }
        // Alone, N has nowhere to go.
        let mut alone = session();
        alone.key_pressed(KeyCode::KeyN);
        assert!(alone.cockpit);
    }

    #[test]
    fn third_key_starts_on_final() {
        let mut s = session();
        s.key_pressed(KeyCode::Digit3);
        let c = s.aircraft().unwrap().controls;
        assert_eq!(c.flap_notch, 2);
        assert!(s.vehicle().agl() > 50.0);
        assert!(s.position_line().starts_with("position n "));
    }

    /// The hand-made wings leveller of sim-env, written to a file as a model would be.
    fn leveller_file(name: &str) -> String {
        let path = std::env::temp_dir().join(format!("flight-game-{}-{name}.fltmodel", std::process::id()));
        sim_env::model::wings_leveller().save(&path).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn model(s: &Session, k: usize) -> &ModelFlier {
        let Flier::Model(m) = &s.planes[k].flier else { panic!("aircraft {k} is not flown by a model") };
        m
    }

    #[test]
    fn a_model_spec_names_a_file_and_maybe_a_task() {
        assert_eq!(ai::parse_spec("models/landing.fltmodel"), ("models/landing.fltmodel", None));
        assert_eq!(ai::parse_spec("a.fltmodel:circuit"), ("a.fltmodel", Some("circuit")));
        assert_eq!(ai::parse_spec("c:/x/a.fltmodel"), ("c:/x/a.fltmodel", None));
        let mut s = session();
        assert!(s.add_model("/nowhere/at/all.fltmodel").is_err());
        assert_eq!(s.planes.len(), 1);
    }

    #[test]
    fn a_hold_model_flies_beside_the_airport_and_goes_on_past_the_time_limit() {
        let path = leveller_file("hold");
        let mut s = session();
        s.add_model(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(s.planes.len(), 2);
        let id = s.planes[1].id;
        let start = s.world.vehicle(id).aircraft().unwrap().state.pos;
        assert!(s.world.vehicle(id).agl() > 500.0);
        assert!((start - s.aircraft().unwrap().state.pos).length() > 1500.0, "on top of the player");
        let altitude = -start.z;

        // The status line says who flies the aircraft the camera is on.
        assert!(s.status_text().contains("Camera: aircraft 1 of 2, flown by you"), "{}", s.status_text());
        s.key_pressed(KeyCode::KeyN);
        let line = s.status_text();
        assert!(line.contains("aircraft 2 of 2, flown by the model flight-game-") && line.contains("(hold heading and altitude)"), "{line}");

        // The leveller holds the 60 s of an episode; the aircraft is not put back, it flies on.
        hold(&mut s, Held::default(), 65.0);
        let m = model(&s, 1);
        assert_eq!((m.episodes, m.last_ending), (1, Some(sim_env::Reason::TimeLimit)));
        assert!(m.restarting_in(s.world.time()).is_none());
        let plane = s.world.vehicle(id);
        assert!(plane.aircraft().unwrap().crashed().is_none());
        assert!((-plane.aircraft().unwrap().state.pos.z - altitude).abs() < 60.0, "altitude {} for {altitude}", -plane.aircraft().unwrap().state.pos.z);
        assert!((plane.aircraft().unwrap().state.pos - start).length() > 2000.0, "it did not fly");
        assert!(s.status_text().contains("last flight: time limit"));
    }

    #[test]
    fn a_takeoff_model_starts_clear_of_the_player_and_starts_again_after_its_episode() {
        // The leveller does not know how to take off: it runs off the runway in about 10 s.
        let path = leveller_file("takeoff");
        let mut s = session();
        s.add_model(&format!("{path}:takeoff")).unwrap();
        std::fs::remove_file(&path).unwrap();
        let id = s.planes[1].id;
        let start = s.world.vehicle(id).aircraft().unwrap().state;
        assert!(s.world.vehicle(id).agl() < 3.0);
        assert!((start.pos - s.aircraft().unwrap().state.pos).length() > 500.0, "on top of the player");
        let mut ended = None;
        for _ in 0..(40.0 / FRAME) as usize {
            s.advance(FRAME, &Held::default());
            let m = model(&s, 1);
            if ended.is_none() && m.episodes == 1 {
                ended = Some(s.world.time());
                assert!(m.restarting_in(s.world.time()).is_some());
            }
            if m.episodes == 1 && m.restarting_in(s.world.time()).is_none() {
                break;
            }
        }
        let ended = ended.expect("its episode never ended");
        let m = model(&s, 1);
        assert_eq!(m.last_ending, Some(sim_env::Reason::OutOfEnvelope));
        assert!((s.world.time() - ended - ai::RESTART_DELAY).abs() < 0.05, "restarted {} s after the end", s.world.time() - ended);
        // Back where it started, standing on the runway.
        let now = s.world.vehicle(id).aircraft().unwrap().state;
        assert!((now.pos - start.pos).length() < 1.0);
        assert!(now.vel.length() < 0.1);

        // Models count against the traffic.
        s.add_traffic(9);
        assert_eq!(s.planes.len(), 1 + MAX_TRAFFIC);
    }

    #[test]
    fn an_autopilot_pattern_flies_the_race_and_its_time_is_shown() {
        // The standard circuit, written as a pattern file.
        let path = std::env::temp_dir().join(format!("flight-game-{}-race.pattern", std::process::id()));
        std::fs::write(&path, sim_core::Pattern::default().to_text()).unwrap();
        let mut s = session();
        s.add_model(&path.to_string_lossy()).unwrap();
        std::fs::remove_file(&path).unwrap();
        s.key_pressed(KeyCode::KeyN);
        for _ in 0..(420.0 / FRAME) as usize {
            s.advance(FRAME, &Held::default());
            if model(&s, 1).episodes == 1 {
                break;
            }
        }
        let m = model(&s, 1);
        assert_eq!(m.last_ending, Some(sim_env::Reason::Success));
        let t = m.last_time.unwrap();
        assert!(t > 250.0 && t < 400.0, "{t} s");
        let line = s.status_text();
        assert!(line.contains("(race") && line.contains("last flight: success in"), "{line}");
    }
}

/// The game as the window runs it, on a GPU without a window: what is made in the background
/// (another place, a graphics quality) while the frames go on.
#[cfg(test)]
mod background {
    use super::*;
    use engine::{Game, Offscreen};

    fn frame(screen: &mut Offscreen, game: &mut Flight) {
        screen.gpu.set_samples(game.samples());
        game.update(&engine::Input::default(), 1.0 / 60.0, &screen.gpu);
        screen.draw(game, 1.0 / 60.0);
    }

    /// A start the player sees is at the computer's hour (`Options::clock`), unless the setting is
    /// off (then the hour the settings keep) or `--hour` says another; without a clock (screenshots,
    /// tests) the settings' hour, the same every run.
    #[test]
    fn the_game_starts_at_the_computer_s_hour_if_the_settings_say_so() {
        let screen = Offscreen::new(320, 200, 1.0);
        let start = |clock: Option<f64>, hour: Option<f64>, local: Option<bool>| {
            let mut store = Store::memory();
            store.set("hour", 9.0);
            if let Some(local) = local {
                store.set(settings::LOCAL_TIME, local);
            }
            let options = Options { clock, hour, quality: Some(Preset::Low), ..Options::default() };
            Flight::new(&screen.gpu, &options, store, crate::audio::Audio::silent()).session.world.hour
        };
        assert_eq!(start(Some(22.5), None, None), 22.5, "on unless taken away");
        assert_eq!(start(Some(22.5), None, Some(true)), 22.5);
        assert_eq!(start(Some(22.5), None, Some(false)), 9.0);
        assert_eq!(start(Some(22.5), Some(14.0), None), 14.0, "--hour wins");
        assert_eq!(start(None, None, None), 9.0);
    }

    /// The wheel over the C172's HDG knob, as the window gives it to the game: from the seat the
    /// game starts in, a notch turns the heading bug a degree, the knob's drawing with it, the tag
    /// says so; with Shift, the selected altitude. Away from the panel the wheel moves the eye.
    #[test]
    fn a_notch_of_the_wheel_over_the_hdg_knob_turns_the_heading_bug() {
        let mut screen = Offscreen::new(1440, 900, 1.0);
        let options = Options { aircraft: Some(Craft::parse("c172").expect("the C172")), start: StartAt::Air, quality: Some(Preset::Low), hour: Some(21.0), ..Options::default() };
        let mut game = Flight::new(&screen.gpu, &options, Store::memory(), crate::audio::Audio::silent());
        // The first frames: the overlay says how large the window is, the cockpit is baked.
        for _ in 0..3 {
            frame(&mut screen, &mut game);
        }
        assert!(game.session.inside() && game.session.spot == 0);
        let bug = |game: &Flight| (game.session.gfc.as_ref().expect("a GFC 700").heading_bug() / DEG).round();
        let selected = |game: &Flight| (game.session.gfc.as_ref().expect("a GFC 700").selected_altitude() / FEET).round();
        let knob = *game.renderer.panel_controls(AircraftKind::C172).iter().find(|c| c.has(PanelKnob::Heading)).expect("the HDG knob");
        let at = cockpit::on_screen(&game.session.view(), game.screen.expect("the window's size"), game.session.vehicle(), knob.centre).expect("in front of the eye");
        let (before, altitude) = (bug(&game), selected(&game));
        // One slow notch, as macOS gives it to winit: a tenth of a line.
        let mut input = engine::Input::default();
        input.mouse = (at.x, at.y);
        input.wheel(0.0, if cfg!(target_os = "macos") { 0.1 } else { 1.0 }, false);
        game.update(&input, 1.0 / 60.0, &screen.gpu);
        assert_eq!((bug(&game) - before).rem_euclid(360.0), 1.0, "the heading bug a degree on");
        assert_eq!(game.session.spot, 0, "the eye stays");
        assert!(game.hover && game.pointed(game.screen.unwrap()).is_some_and(|p| p.name == game.session.knob_name(PanelKnob::Heading) && p.hint.contains("ALT")));
        // The knob's drawing follows.
        input.scroll = 0.0;
        for _ in 0..30 {
            game.update(&input, 1.0 / 60.0, &screen.gpu);
        }
        assert!((game.session.knobs.shown()[PanelKnob::Heading as usize] - 1.0).abs() < 1e-3);
        // With Shift (macOS sends the notch across), the selected altitude, 100 ft.
        input.shift = true;
        input.wheel(if cfg!(target_os = "macos") { -0.1 } else { -1.0 }, 0.0, false);
        game.update(&input, 1.0 / 60.0, &screen.gpu);
        assert_eq!((selected(&game) - altitude, bug(&game)), (-100.0, (before + 1.0).rem_euclid(360.0)));
        // Over the windscreen the wheel is the view's: it zooms in, the eye in its spot.
        let fov = game.session.view().fov_y;
        let mut input = engine::Input::default();
        input.mouse = (700.0, 200.0);
        input.wheel(0.0, 1.0, false);
        game.update(&input, 1.0 / 60.0, &screen.gpu);
        input.scroll = 0.0;
        game.update(&input, 1.0 / 60.0, &screen.gpu);
        assert!(!game.hover && game.session.spot == 0 && game.session.view().fov_y < fov);
    }

    /// The settings take the propeller and the yokes out of the cockpit's view, at once, and only
    /// out of it: outside they are drawn as ever, and the yoke still flies.
    #[test]
    fn the_settings_hide_the_propeller_and_the_yokes_from_the_cockpit_only() {
        let mut screen = Offscreen::new(320, 200, 1.0);
        let options = Options { aircraft: Some(Craft::parse("c172").expect("the C172")), start: StartAt::Air, quality: Some(Preset::Low), ..Options::default() };
        let mut game = Flight::new(&screen.gpu, &options, Store::memory(), crate::audio::Audio::silent());
        frame(&mut screen, &mut game);
        assert!(game.session.inside());
        let cabin = |game: &Flight| game.renderer.cabin_parts();
        let (discs, parts) = (game.renderer.discs(), cabin(&game));
        assert_eq!(discs, 1, "the propeller's blur, from the cockpit");
        game.session.store.set(settings::PROPELLER_SHOWN, false);
        frame(&mut screen, &mut game);
        assert_eq!((game.renderer.discs(), cabin(&game)), (0, parts));
        game.session.store.set(settings::YOKES_SHOWN, false);
        frame(&mut screen, &mut game);
        assert_eq!(cabin(&game), parts - 2, "both yokes gone");
        // The yoke still works: the aircraft answers it.
        game.session.set_assist(false);
        let roll = |game: &Flight| euler_angles(game.session.body().q).roll;
        let before = roll(&game);
        for _ in 0..60 {
            game.session.advance(1.0 / 60.0, &Held { roll: 1.0, ..Held::default() });
        }
        assert!(roll(&game) > before + 5.0 * DEG, "{}", (roll(&game) - before) / DEG);
        // Outside both are drawn whatever the settings say.
        game.session.key_pressed(KeyCode::KeyV);
        frame(&mut screen, &mut game);
        assert!(!game.session.inside());
        assert_eq!(game.renderer.discs(), 1);
        game.session.store.set(settings::PROPELLER_SHOWN, true);
        game.session.key_pressed(KeyCode::KeyV);
        frame(&mut screen, &mut game);
        assert_eq!(game.renderer.discs(), 1);
    }

    /// A knob of the G1000 pressed and dragged: up or right turns it clockwise, down or left
    /// back, slowly a click every six points; a click without moving turns nothing. The ALT knob's
    /// inner knob is 100 ft a click and its ring 1000 ft.
    #[test]
    fn a_knob_of_the_g1000_is_turned_by_dragging_it() {
        let mut screen = Offscreen::new(1440, 900, 1.0);
        let options = Options { aircraft: Some(Craft::parse("c172").expect("the C172")), start: StartAt::Air, quality: Some(Preset::Low), ..Options::default() };
        let mut game = Flight::new(&screen.gpu, &options, Store::memory(), crate::audio::Audio::silent());
        for _ in 0..3 {
            frame(&mut screen, &mut game);
        }
        let bug = |game: &Flight| (game.session.gfc.as_ref().expect("a GFC 700").heading_bug() / DEG).round();
        let selected = |game: &Flight| (game.session.gfc.as_ref().expect("a GFC 700").selected_altitude() / FEET).round();
        let on = |game: &Flight, body: glam::Vec3| cockpit::on_screen(&game.session.view(), game.screen.expect("the window's size"), game.session.vehicle(), body).expect("in front of the eye");
        let controls = game.renderer.panel_controls(AircraftKind::C172).to_vec();
        let hdg = *controls.iter().find(|c| c.has(PanelKnob::Heading)).expect("the HDG knob");
        let alt = *controls.iter().find(|c| c.has(PanelKnob::AltitudeFine)).expect("the ALT knob");
        // Pressed on a place of the panel and let go after moving the pointer as `moves` say, a frame each.
        let drag = |game: &mut Flight, screen: &Offscreen, from: glam::Vec3, moves: &[(f32, f32)]| {
            let at = on(game, from);
            let mut input = engine::Input::default();
            (input.mouse, input.clicked, input.left_button) = ((at.x, at.y), true, true);
            game.update(&input, 1.0 / 60.0, &screen.gpu);
            input.clicked = false;
            for &(dx, dy) in moves {
                input.mouse = (input.mouse.0 + dx, input.mouse.1 + dy);
                input.mouse_delta = (dx, dy);
                game.update(&input, 1.0 / 60.0, &screen.gpu);
                assert!(matches!(game.drag, Drag::Knob(..)) && game.hover, "the knob is held");
            }
            input.left_button = false;
            input.mouse_delta = (0.0, 0.0);
            game.update(&input, 1.0 / 60.0, &screen.gpu);
            assert!(matches!(game.drag, Drag::None));
        };
        // A click on either side of the HDG knob, without moving: nothing turns.
        let (before, altitude) = (bug(&game), selected(&game));
        for side in [-0.005, 0.005] {
            drag(&mut game, &screen, hdg.centre + glam::Vec3::Y * side, &[]);
        }
        assert_eq!(bug(&game), before);
        // Slowly 18 points right (a point a frame): three degrees more; then 12 up: two more.
        drag(&mut game, &screen, hdg.centre, &[(1.0, 0.0); 18]);
        assert_eq!((bug(&game) - before).rem_euclid(360.0), 3.0);
        drag(&mut game, &screen, hdg.centre, &[(0.0, -1.0); 12]);
        assert_eq!((bug(&game) - before).rem_euclid(360.0), 5.0);
        // 30 down and left, well away from the knob: five back, and the knob is held all the way.
        drag(&mut game, &screen, hdg.centre, &[(-1.0, 1.0); 15]);
        assert_eq!(bug(&game), before);
        // Quickly: 60 points in two frames are thirty clicks, not ten.
        drag(&mut game, &screen, hdg.centre, &[(30.0, 0.0); 2]);
        assert_eq!((bug(&game) - before).rem_euclid(360.0), 30.0);
        assert_eq!(selected(&game), altitude, "only the knob held turns");
        // The ALT knob: its inner knob 100 ft a click, its ring 1000 ft.
        drag(&mut game, &screen, alt.centre + glam::Vec3::Y * 0.002, &[(0.0, -1.0); 12]);
        assert_eq!(selected(&game) - altitude, 200.0);
        drag(&mut game, &screen, alt.centre + glam::Vec3::Y * 0.0075, &[(-1.0, 0.0); 6]);
        let ring = selected(&game);
        assert!(ring < altitude && ring % 1000.0 == 0.0, "{ring}");
        // Far down: the selected altitude stops at 0 ft, never below.
        drag(&mut game, &screen, alt.centre + glam::Vec3::Y * 0.0075, &[(-40.0, 0.0); 4]);
        assert_eq!(selected(&game), 0.0);
        // While a knob is held its tag says what it is set to, click by click, beside the whole
        // knob and not over it, wherever the pointer has gone.
        let at = on(&game, alt.centre + glam::Vec3::Y * 0.002);
        let mut input = engine::Input::default();
        (input.mouse, input.clicked, input.left_button) = ((at.x, at.y), true, true);
        game.update(&input, 1.0 / 60.0, &screen.gpu);
        input.clicked = false;
        for click in 1..=5 {
            for _ in 0..6 {
                (input.mouse, input.mouse_delta) = ((input.mouse.0, input.mouse.1 - 1.0), (0.0, -1.0));
                game.update(&input, 1.0 / 60.0, &screen.gpu);
            }
            let tag = game.pointed(game.screen.unwrap()).expect("the knob held is named");
            assert_eq!(tag.name, format!("ALT {} ft", click * 100));
            let (ring, inner) = (Rect::from_points(&tag.whole), Rect::from_points(&tag.outline));
            assert!(tag.lit && ring.contains_rect(inner) && ring.width() > inner.width() * 1.3);
        }
        input.left_button = false;
        game.update(&input, 1.0 / 60.0, &screen.gpu);
        // A key is still pressed by a click.
        let ap = *controls.iter().find(|c| matches!(c.shape, cockpit::PanelShape::Key { does: Panel::Key(AfcsKey::Ap), .. })).expect("the AP key");
        let at = on(&game, ap.centre);
        let mut input = engine::Input::default();
        (input.mouse, input.clicked, input.left_button) = ((at.x, at.y), true, true);
        game.update(&input, 1.0 / 60.0, &screen.gpu);
        assert!(game.session.gfc.as_ref().is_some_and(Gfc700::engaged));
    }

    fn flight(screen: &Offscreen) -> Flight {
        let options = Options { aircraft: Some(Craft::Multirotor(sim_core::MultirotorKind::FPV5)), start: StartAt::Air, quality: Some(Preset::Low), ..Options::default() };
        Flight::new(&screen.gpu, &options, Store::memory(), crate::audio::Audio::silent())
    }

    /// Another place chosen: the flight holds where it is while it loads; staying, it goes on and
    /// the settings keep the place flown. Its loading failed: the flight holds until the player
    /// chooses; back to the flight, it goes on.
    #[test]
    fn the_flight_holds_while_another_place_loads_and_after_it_fails() {
        let mut screen = Offscreen::new(320, 200, 1.0);
        let mut game = flight(&screen);
        frame(&mut screen, &mut game);
        let progress = geodata::Progress { stage: geodata::Stage::Heights, done: 12, total: 40, downloaded: 8_400_000 };
        game.loading = Some(places::Loading::stalled("LOWI", Some(progress), 3));
        game.session.waiting_for_place = true;
        game.session.store.set("place", "LOWI");
        let (time, at) = (game.session.world.time(), game.session.body().pos);
        for _ in 0..10 {
            frame(&mut screen, &mut game);
        }
        assert_eq!((game.session.world.time(), game.session.body().pos), (time, at), "the flight went on");
        assert!(game.loading.as_ref().and_then(places::Loading::progress).is_some_and(|p| (p.0 - 0.185).abs() < 1e-9 && p.1.contains("terrain 12/40")));
        game.card_choice = Some(waiting::Choice::Stay);
        frame(&mut screen, &mut game);
        assert!(!game.session.waiting_for_place && game.loading.is_none() && game.session.world.time() > time);
        assert_eq!(game.session.store.word("place"), Some(""));

        game.place_error = Some("connection refused".into());
        game.failed = Some("LOWI".into());
        game.session.waiting_for_place = true;
        let time = game.session.world.time();
        for _ in 0..5 {
            frame(&mut screen, &mut game);
        }
        assert_eq!(game.session.world.time(), time);
        game.card_choice = Some(waiting::Choice::Stay);
        frame(&mut screen, &mut game);
        assert!(!game.session.waiting_for_place && game.failed.is_none() && game.place_error.is_none());
    }

    /// The menus are drawn, and the keys alone work them: Esc shows the start and pause screen, the
    /// arrows and Enter open the aircraft and choose one, O the settings and Tab its sections.
    #[test]
    fn the_menus_are_drawn_and_the_keys_work_them() {
        let mut screen = Offscreen::new(1280, 800, 1.0);
        let mut game = flight(&screen);
        let frames = |screen: &mut Offscreen, game: &mut Flight, count: usize| {
            for _ in 0..count {
                frame(screen, game);
            }
        };
        let press = |screen: &mut Offscreen, game: &mut Flight, key: KeyCode| {
            game.session.key_pressed(key);
            frame(screen, game);
        };
        frames(&mut screen, &mut game, 2);
        game.session.key_pressed(KeyCode::Escape);
        frames(&mut screen, &mut game, 20);
        assert!(game.session.home_open && game.session.paused);
        // Down shows the cursor on "Fly", down again is "Aircraft"; Enter opens it.
        for key in [KeyCode::ArrowDown, KeyCode::ArrowDown, KeyCode::Enter] {
            press(&mut screen, &mut game, key);
        }
        assert!(game.session.hangar_open);
        frames(&mut screen, &mut game, 20);
        // The cursor is on the quad flown; right is the racing quad beside it, Enter flies it.
        for key in [KeyCode::ArrowRight, KeyCode::ArrowRight, KeyCode::Enter] {
            press(&mut screen, &mut game, key);
        }
        assert_eq!(game.session.craft().key(), "race5");
        assert_eq!(game.session.store.word("aircraft"), Some("race5"));
        assert!(!game.session.hangar_open && game.session.home_open && game.session.paused);
        frames(&mut screen, &mut game, 20);
        // The settings from the menu, every section of them, and back; then on with the flight.
        press(&mut screen, &mut game, KeyCode::KeyO);
        for _ in 0..4 {
            frames(&mut screen, &mut game, 14);
            press(&mut screen, &mut game, KeyCode::Tab);
        }
        assert!(game.session.settings_open);
        // In the weather: down to the kinds of day, and Enter makes it the first of them, a clear one.
        frames(&mut screen, &mut game, 4);
        for key in [KeyCode::ArrowDown, KeyCode::ArrowDown, KeyCode::Enter] {
            press(&mut screen, &mut game, key);
        }
        assert_eq!(game.session.world.weather.wind_speed, 0.0);
        assert_eq!(game.session.store.number("cloudCover"), Some(0.0));
        press(&mut screen, &mut game, KeyCode::Escape);
        assert!(!game.session.settings_open && game.session.home_open);
        let time = game.session.world.time();
        press(&mut screen, &mut game, KeyCode::Escape);
        frames(&mut screen, &mut game, 3);
        assert!(!game.session.paused && !game.session.menu_shown() && game.session.world.time() > time);
    }

    /// The first screen is drawn in a wide window and in a narrow one, and the keys alone work it:
    /// the cursor starts on the family flown, the arrows move it and Enter shows that family's aircraft.
    #[test]
    fn the_first_screen_is_drawn_and_the_keys_choose_a_family() {
        for (width, height) in [(1280, 800), (520, 760)] {
            let mut screen = Offscreen::new(width, height, 1.0);
            let mut game = flight(&screen);
            let press = |screen: &mut Offscreen, game: &mut Flight, key: KeyCode| {
                game.session.key_pressed(key);
                frame(screen, game);
            };
            game.session.open_vehicles();
            for _ in 0..20 {
                frame(&mut screen, &mut game);
            }
            assert!(game.session.vehicles_open && game.session.paused);
            // Side by side the planes are left of the drones; one above the other, over them.
            let back = if width > height { KeyCode::ArrowLeft } else { KeyCode::ArrowUp };
            // The first key shows the cursor, on the drones (an FPV quad is flown); the planes are before them.
            for key in [back, back, KeyCode::Enter] {
                press(&mut screen, &mut game, key);
            }
            assert!(!game.session.vehicles_open && game.session.hangar_open && game.session.home_open);
            assert_eq!(game.session.craft().key(), "c172");
            for _ in 0..20 {
                frame(&mut screen, &mut game);
            }
            // Only the planes are there: right of the Cessna (below it, in the narrow window) is the Extra.
            let on = if width > height { KeyCode::ArrowRight } else { KeyCode::ArrowDown };
            for key in [on, on, KeyCode::Enter] {
                press(&mut screen, &mut game, key);
            }
            assert_eq!(game.session.craft().key(), "extra300");
            assert_eq!(game.session.store.word("plane"), Some("extra300"));
            assert!(!game.session.hangar_open && game.session.home_open && game.session.paused);
        }
    }

    /// Another graphics quality: the frames go on drawing with the old one, and the flight with
    /// them, until the new one has been made in the background; then it is drawn, with its samples.
    #[test]
    fn a_quality_is_made_in_the_background_while_the_flight_goes_on() {
        let mut screen = Offscreen::new(320, 200, 1.0);
        let mut game = flight(&screen);
        frame(&mut screen, &mut game);
        assert_eq!(game.samples(), 1);
        game.quality = Preset::Medium;
        frame(&mut screen, &mut game);
        assert!(game.renderer.changing_quality());
        let time = game.session.world.time();
        let mut frames = 0;
        while game.renderer.changing_quality() && frames < 6000 {
            frame(&mut screen, &mut game);
            screen.gpu.wait();
            frames += 1;
        }
        assert!(!game.renderer.changing_quality(), "never made");
        assert!(game.session.world.time() > time, "the flight waited");
        assert_eq!((game.samples(), screen.gpu.sample_count), (4, 4));
    }
}
