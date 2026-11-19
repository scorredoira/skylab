//! Skylab, the flight simulator: the command line, then either the window or one frame written to a PNG; in a browser,
//! the page's canvas (`web.rs`).

// The web has no command line: what only `--shot`, `--controllers` and the like use is left unused there.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

mod ai;
mod audio;
mod cockpit;
mod controls;
mod fleet;
mod g1000;
mod game;
mod instrument_keys;
mod kinds;
mod manual;
mod models;
mod multirotor;
mod picker;
mod places;
mod race;
mod remote;
#[cfg(not(target_arch = "wasm32"))]
mod record;
mod render;
mod replay;
mod settings;
mod ui;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
fn main() {
    web::main();
}

#[cfg(not(target_arch = "wasm32"))]
use game::{Flight, Options, StartAt};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

#[cfg(not(target_arch = "wasm32"))]
const USAGE: &str = "\
flight [--seed <n>] [--air | --final | --pos <n> <e>] [--ext] [--hour <h>] [--sky <day>] [--time <n>] [--help-shown]
       [--traffic <n>] [--model <file>[:<task>]]... [--quality <ultra|high|medium|low>] [--fps]
       [--look <yaw> [<pitch>]] [--spot <n>] [--hover <x> <y>] [--low <m>] [--aircraft <c172|extra|fpv|...>]
       [--shot <dir> [--retina | --size <w>x<h>] [--crash <s> | --damage <what>] [--fail <what>[,<what>...]] [--wait <s>]
              [--afcs <what>[,<what>...]] [--land] [--replay <s>]]
       [--controllers] [--place <ICAO> | --at <lat>,<lon>] [--offline] [--course <name> [--racer]]
       [--ai] [--rival <racer | file.fltmodel>]... [--watch <n>]
       [--picker [--map <lat>,<lon>,<zoom>] [--search <text>] [--select <ICAO | lat,lon>]] [--manual <page>]
       [--menu <vehicles|home|aircraft|settings|weather|flight|graphics|about>] [--clean] [--zoom <lines>]
       [--record <dir> --script <file.ron> [--only <shot>[,<shot>...]] [--storyboard]]
       [--remote [<port>]]

  --seed <n>     world to generate (default 1)
  --aircraft <a> c172 (Cessna 172), extra (Extra 300L), fpv (a 5-inch FPV quad) or the key of
                 another file of assets/aircraft;
                 default: the one chosen in the settings (O)
  --air          start in flight over the home airport (a multirotor: hovering, armed)
  --final        start on final approach
  --pos <n> <e>  start in flight over this point, metres north and east
  --ext          start in the outside view (a multirotor: the chase camera behind it)
  --hour <h>     hour of the day, 0-24
  --sky <day>    the kind of day: clear, fair, haze, overcast, windy or storms
  --time <n>     time this many times faster than real: 1, 2, 4, 8 or 16 (keys F9 F10)
  --help-shown   start with the help column open
  --traffic <n>  0-6 aircraft flown by the autopilot around the home airport and its neighbours
  --model <file>[:<task>]
                 add an aircraft flown by a trained model (repeatable): a takeoff model on the
                 home runway, landing on final, hold in the air beside the airport; it starts
                 again a few seconds after each episode ends. The task is the model's own
                 unless given (hold, takeoff, landing, circuit, race). A .pattern file (sim-env's
                 race search) is the autopilot flying the race with that circuit. A model of the
                 gates race (models/gates.fltmodel) is a rival quad: see --rival
  --quality <q>  how much is drawn: high (everything), medium or low for slower machines, ultra for pictures and video
  --fps          frames per second in the status line (key F3)
  --look <y> [<p>] in the cockpit, the head turned y degrees right (negative: left) and p up;
                 with --ext, the camera turned y degrees round the aircraft and p above it
  --spot <n>     in the cockpit, the eye at spot n: 0 the seat, 1 the panel, then close to each
                 instrument (the mouse wheel or + and − go through them)
  --low <m>      starting in flight: this many metres above the ground instead of 900
  --crash <s>    with --shot: dive into the ground ahead and show the wreck <s> seconds later
  --damage <d>   with --shot: break the aircraft first: tip (a wing tip dropped onto the runway),
                 gear (a touchdown that collapses the gear), wing (with --air: the outer left
                 wing and half the right tailplane torn off), flutter (with --air: a dive until
                 a surface flutters away)
  --fail <what>  with --shot: fail instruments first: pitot (sealed), inlet (blocked, drain
                 open), static (ports blocked), vacuum (pump), icing; several with commas
  --wait <s>     with --shot: fly <s> seconds more before the picture (the gyros run down...)
  --afcs <what>   with --shot, in a C172: work the GFC 700 first, then fly the wait with it: its keys
                 (ap fd hdg nav apr alt vs flc up dn dto sync disc) and bug=<deg>, sel=<ft> (+ or -:
                 from the heading or altitude of now), throttle=<0..1>, a knob turned some clicks
                 (turn=<hdg|alt|alt1000|baro|crs>:<clicks>), in order, several with commas
  --hover <x> <y> with --shot, in a cockpit: the mouse's pointer at that point of the picture
                 (1600x1000 points), to see what it lights and names on the panel
  --nacelles <0..1> a tilt-rotor's nacelles' lever from the start: 0 upright, 1 forward
  --land         with --shot: L first, the autopilot lands at the nearest aerodrome (with --final)
  --replay <s>   with --shot: after the wait, the replay (F2), stopped <s> seconds before the end
  --shot <dir>   write <dir>/scene.png (1600x1000) instead of opening a window
  --retina       with --shot: 2880x1800, two pixels per point
  --size <w>x<h> with --shot: a picture of this size instead (to see a screen in a small window)
  --place <ICAO> fly from a real aerodrome (its main runway), downloaded the first time and kept
                 in the cache (some 50 MB): heights, coast, woods, fields, towns, roads, runways
  --at <lat>,<lon> a real place round that point, starting in flight over it
  --offline      with --place or --at: only what is in the cache, nothing downloaded
  --course <name> race a course of assets/courses (deepwood, pinewood) in its quad: on its start's
                 pad, the countdown, the gates (also in the place picker, M)
  --racer        with --course: the autopilot races (L in the game), for --shot with --wait
  --rival <who>  a quad flown by the game races against you, in its own colour (repeatable): racer
                 (the race's autopilot), net (the course's trained network) or a network of the
                 gates race (a .fltmodel file); without --course, at Deepwood. N puts the camera on the next quad (its FPV
                 camera, or behind it with V); the display says the positions (P1/3)
  --ai           race against the course's trained network (models/gates-<course>.fltmodel, else
                 models/gates.fltmodel) and the autopilot: --rival net --rival racer; without
                 --course, at Deepwood Forest
  --watch <n>    in a race with rivals, the camera on quad n from the start: 0 yours, 1 the first
                 rival... (N in the game); with --ext behind it, else its FPV camera
  --picker       open the place picker (M) at the start; with --shot, to see it:
  --map <lat>,<lon>,<zoom>  where its map looks (zoom 1: the world, 9: an island, 13: an aerodrome)
  --search <text>  what is typed in its search
  --select <place> the place chosen in it (an aerodrome's code, or lat,lon)
  --manual <page> open the manual (F1) at the start at this page, 1 the first
  --menu <which> open a menu at the start: vehicles (the first screen: planes, drones or cars),
                 home (the start and pause screen, Esc), aircraft (its pictures), settings (O) or
                 one of its sections: weather, flight, graphics, about

The game opens on the first screen (planes, drones or cars) unless the command line already says
what is flown or where (--aircraft, --course, --place, --at, --air, --final, --pos, --seed,
--model, --rival, --ai), opens another screen, or nobody is there to choose (--shot, --record,
--remote).
  --clean        nothing written over the picture (for the menus' pictures: scripts/menu-shots.sh)
  --zoom <lines> with --ext: the camera this many lines of the mouse wheel nearer (negative: further)
  --remote [<port>] other programs can fly the game: it listens on 127.0.0.1 (port 7448 unless
                 given) and starts without the start screen. `skylab-ctl` is the client at the
                 command line; the protocol is crates/games/remote/PROTOCOL.md
  --controllers  list the game controllers and what they send for 10 seconds, and exit
  --record <dir> render the shots of a script (--script, see record.rs) to <dir>/<shot>/00000.png...,
                 a frame every 1/60 s of the game's time however long it takes, without a window
                 (and, if the script says `sound: true`, each shot's sound to <dir>/<shot>.wav)
  --only <names> with --record: only these shots of the script
  --storyboard   with --record: one still of each shot, from its middle, to <dir>/<shot>.png
                 (and where the aircraft is then, printed: to place a standing camera)";

/// The frames a screenshot waits for the terrain before giving up.
#[cfg(not(target_arch = "wasm32"))]
const SHOT_FRAME_CAP: usize = 1500;
/// Frames drawn after loading and before timing (see `write_shot`).
#[cfg(not(target_arch = "wasm32"))]
const SHOT_WARM_UP: usize = 60;
/// Frames timed after the warm-up.
#[cfg(not(target_arch = "wasm32"))]
const SHOT_TIMED: usize = 240;

/// What the command line asks for.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct Command {
    options: Options,
    shot: Option<String>,
    retina: bool,
    /// The size of the picture of `--shot`, if not the usual one (`--size`).
    size: Option<(u32, u32)>,
    fail: Vec<String>,
    afcs: Vec<String>,
    wait: f64,
    land: bool,
    replay: Option<f64>,
    record: Option<String>,
    script: Option<String>,
    only: Vec<String>,
    storyboard: bool,
    /// The command line says what is flown or where: the first screen has nothing to ask.
    chosen: bool,
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    // The types of aircraft first: the command line and the settings name them.
    if let Err(e) = fleet::load() {
        eprintln!("the aircraft (assets/aircraft): {e}");
        std::process::exit(1);
    }
    if let Err(e) = race::load() {
        eprintln!("the race courses (assets/courses): {e}");
        std::process::exit(1);
    }
    let Some(Command { options, shot, retina, size, fail, afcs, wait, land, replay, record, script, only, storyboard, .. }) = parse(std::env::args().skip(1), true) else { return };
    if let Some(directory) = record {
        let Some(script) = script else {
            eprintln!("--record needs --script <file.ron>");
            std::process::exit(1);
        };
        if let Err(error) = record::run(&directory, &script, &only, storyboard) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    engine::set_icon(include_bytes!("../../../../assets/brand/skylab-icon-512.png"));
    match shot {
        Some(directory) => write_shot(&directory, &options, Picture { retina, size }, &fail, &afcs, wait, Flown { land, replay }),
        None => {
            // A start the player sees: at the hour of the computer's clock, if the settings say so.
            let options = Options { clock: engine::clock::local_hour(), ..options };
            // The driving game is inside this program: "Cars" shows it in this window, and its
            // "Change vehicle" brings this game back (`Shown`).
            engine::program::host(&[ui::menu::vehicles::DRIVE, SKYLAB]);
            engine::run("Skylab", 1440, 900, |gpu| Shown::Flight(Box::new(Flight::new(gpu, &options, settings::Store::user(), audio::Audio::start()))))
        }
    }
}

/// This program's name, as the driving game hands back to it.
#[cfg(not(target_arch = "wasm32"))]
const SKYLAB: &str = "skylab";

/// What Skylab's window shows: the flight game, or the driving game in its place. A hand-over
/// (`engine::program::hand_over`) between the two is made here, after the frame's `update`: the
/// game handed over to is made then, and the one it replaces dropped, its sound with it.
#[cfg(not(target_arch = "wasm32"))]
enum Shown {
    Flight(Box<Flight>),
    Drive(Box<drive::game::Drive>),
}

#[cfg(not(target_arch = "wasm32"))]
impl Shown {
    /// The game a hand-over names, made: the driving game, or this one with the arguments given
    /// (`--menu vehicles`: its first screen), at the hour of the computer's clock.
    fn make(name: &str, arguments: Vec<String>, gpu: &engine::Gpu) -> Shown {
        if name == ui::menu::vehicles::DRIVE {
            return Shown::Drive(Box::new(drive::start(gpu)));
        }
        let options = parse(arguments.into_iter(), true).map(|c| c.options).unwrap_or_default();
        let options = Options { clock: engine::clock::local_hour(), ..options };
        Shown::Flight(Box::new(Flight::new(gpu, &options, settings::Store::user(), audio::Audio::start())))
    }
}

/// Each of the game's calls passed to the game shown.
#[cfg(not(target_arch = "wasm32"))]
macro_rules! shown {
    ($self:expr, $game:ident => $call:expr) => {
        match $self {
            Shown::Flight($game) => $call,
            Shown::Drive($game) => $call,
        }
    };
}

#[cfg(not(target_arch = "wasm32"))]
impl engine::Game for Shown {
    fn update(&mut self, input: &engine::Input, dt: f64, gpu: &engine::Gpu) {
        shown!(self, g => g.update(input, dt, gpu));
        if let Some((name, arguments)) = engine::program::take_switch() {
            *self = Shown::make(&name, arguments, gpu);
            // Its first frame is drawn now: updated first, as every frame.
            shown!(self, g => g.update(input, dt, gpu));
        }
    }
    fn ui(&mut self, ctx: &engine::egui::Context) {
        shown!(self, g => g.ui(ctx))
    }
    fn draw(&mut self, gpu: &engine::Gpu, pass: &mut engine::wgpu::RenderPass<'_>) {
        shown!(self, g => g.draw(gpu, pass))
    }
    fn translucent(&self) -> bool {
        shown!(self, g => g.translucent())
    }
    fn before_translucent(&mut self, gpu: &engine::Gpu, encoder: &mut engine::wgpu::CommandEncoder) {
        shown!(self, g => g.before_translucent(gpu, encoder))
    }
    fn draw_translucent(&mut self, gpu: &engine::Gpu, pass: &mut engine::wgpu::RenderPass<'_>) {
        shown!(self, g => g.draw_translucent(gpu, pass))
    }
    fn reads_scene(&self) -> bool {
        shown!(self, g => g.reads_scene())
    }
    fn samples(&self) -> u32 {
        shown!(self, g => g.samples())
    }
    fn post(&self) -> engine::Post {
        shown!(self, g => g.post())
    }
    fn clip_name(&self) -> Option<&str> {
        shown!(self, g => g.clip_name())
    }
}

/// Reads the command line (or a shot's in a recording's script), and loads the real place it names.
/// `None` when it was all done here (`--help`, `--controllers`). With `window`, a window would open:
/// the saved place and the picker at the start count.
#[cfg(not(target_arch = "wasm32"))]
fn parse(arguments: impl Iterator<Item = String>, window: bool) -> Option<Command> {
    let mut command = Command::default();
    let Command { options, shot, retina, size, fail, afcs, wait, land, replay, record, script, only, storyboard, chosen } = &mut command;
    let mut air = false;
    let mut on_final = false;
    let mut position = None;
    let (mut place, mut over_point, mut offline) = (None, false, false);
    // A course in a generated world: no real place, not even the one saved.
    let mut course_seed = false;
    let mut args = arguments.peekable();
    while let Some(arg) = args.next() {
        // A value that does not parse is left for the next round, as an option of its own.
        let number = |args: &mut std::iter::Peekable<_>| -> Option<f64> {
            let v = args.peek().and_then(|v: &String| v.parse::<f64>().ok())?;
            args.next();
            Some(v)
        };
        match arg.as_str() {
            "--seed" => {
                if let Some(seed) = args.peek().and_then(|v| v.parse::<u64>().ok()) {
                    options.seed = seed;
                    *chosen = true;
                    args.next();
                }
            }
            "--shot" => *shot = args.next().or(shot.take()),
            "--hour" => options.hour = number(&mut args).or(options.hour),
            "--time" => options.time_rate = number(&mut args).map_or(options.time_rate, |n| n.max(1.0) as u32),
            "--sky" => options.sky = args.next().as_deref().and_then(crate::settings::Sky::named).or(options.sky),
            "--traffic" => {
                if let Some(n) = number(&mut args) {
                    options.traffic = (n.max(0.0) as usize).min(game::MAX_TRAFFIC);
                }
            }
            "--model" => match args.next().inspect(|_| *chosen = true) {
                // A model of the gates race is a rival quad.
                Some(spec) if race::is_rival(&spec) => options.rivals.push(spec),
                Some(spec) => options.models.push(spec),
                None => eprintln!("--model needs a model file"),
            },
            "--watch" => options.watch = number(&mut args).map_or(0, |n| n.max(0.0) as usize),
            "--rival" => match args.next().inspect(|_| *chosen = true) {
                Some(spec) => options.rivals.push(spec),
                None => eprintln!("--rival needs racer or a model file of the gates race"),
            },
            "--ai" => {
                *chosen = true;
                // The course's network (`race::NET`, found when the race starts) and its racer.
                options.rivals.push(race::NET.to_string());
                options.rivals.push(race::RACER.to_string());
            }
            "--pos" => {
                if let Some(n) = number(&mut args) {
                    match number(&mut args) {
                        Some(e) => {
                            position = Some((n, e));
                            *chosen = true;
                        }
                        None => eprintln!("--pos needs two numbers: north and east"),
                    }
                }
            }
            "--spot" => options.spot = number(&mut args).map_or(0, |n| n.max(0.0) as usize),
            "--nacelles" => options.nacelles = number(&mut args).or(options.nacelles),
            "--hover" => match (number(&mut args), number(&mut args)) {
                (Some(x), Some(y)) => options.hover = Some((x as f32, y as f32)),
                _ => eprintln!("--hover needs two numbers: x and y, points of the picture"),
            },
            "--look" => {
                if let Some(yaw) = number(&mut args) {
                    let pitch = number(&mut args).unwrap_or(0.0);
                    options.look = Some((yaw.to_radians(), pitch.to_radians()));
                }
            }
            "--aircraft" => match args.next().as_deref().map(crate::kinds::Craft::parse) {
                Some(Some(kind)) => {
                    options.aircraft = Some(kind);
                    *chosen = true;
                }
                _ => eprintln!("--aircraft needs c172, extra, fpv or another aircraft or multirotor of assets/aircraft"),
            },
            "--air" => (air, *chosen) = (true, true),
            "--final" => (on_final, *chosen) = (true, true),
            "--ext" => options.exterior = true,
            "--retina" => *retina = true,
            "--size" => match args.next().as_deref().and_then(picture_size) {
                Some(given) => *size = Some(given),
                None => eprintln!("--size needs <width>x<height>, in pixels"),
            },
            "--help-shown" => options.help = true,
            "--controllers" => {
                controls::list_live(10.0);
                return None;
            }
            "--fps" => options.fps = true,
            "--low" => options.low = number(&mut args).or(options.low),
            "--fail" => match args.next() {
                Some(list) => fail.extend(list.split(',').map(str::to_string)),
                None => eprintln!("--fail needs pitot, inlet, static, vacuum or icing"),
            },
            "--wait" => *wait = number(&mut args).unwrap_or(*wait),
            "--land" => *land = true,
            "--replay" => *replay = number(&mut args).or(Some(10.0)),
            "--afcs" => match args.next() {
                Some(list) => afcs.extend(list.split(',').map(str::to_string)),
                None => eprintln!("--afcs needs GFC 700 keys: ap,hdg,alt..."),
            },
            "--crash" => options.crash = number(&mut args).or(Some(6.0)),
            "--damage" => match args.next().as_deref().map(game::Damaged::named) {
                Some(Some(what)) => options.damage = Some(what),
                _ => eprintln!("--damage needs tip, gear, wing, flutter or props"),
            },
            "--quality" => match args.next().as_deref().map(render::Preset::named) {
                Some(Some(quality)) => options.quality = Some(quality),
                _ => eprintln!("--quality needs ultra, high, medium or low"),
            },
            "--place" | "--at" => match args.next() {
                Some(key) => {
                    over_point = arg == "--at";
                    place = Some(key);
                    *chosen = true;
                }
                None => eprintln!("{arg} needs an aerodrome code (LEMH) or latitude,longitude"),
            },
            "--offline" => offline = true,
            "--racer" => options.racer = true,
            "--course" => match args.next().as_deref().and_then(race::find) {
                Some((index, course)) => {
                    options.course = Some(index);
                    *chosen = true;
                    match &course.place {
                        sim_core::course::Place::Seed(seed) => {
                            options.seed = *seed;
                            course_seed = true;
                        }
                        sim_core::course::Place::Airport(code) => place = Some(code.clone()),
                    }
                }
                None => eprintln!("--course needs the name of a course of assets/courses: {}", sim_core::course::file::courses().iter().map(|c| c.key.as_str()).collect::<Vec<_>>().join(", ")),
            },
            "--picker" => {
                options.picker.get_or_insert_with(Default::default);
            }
            "--map" => match args.next().as_deref().and_then(view) {
                Some(view) => options.picker.get_or_insert_with(Default::default).view = Some(view),
                None => eprintln!("--map needs latitude,longitude,zoom"),
            },
            "--search" => match args.next() {
                Some(text) => options.picker.get_or_insert_with(Default::default).query = text,
                None => eprintln!("--search needs a text"),
            },
            "--select" => match args.next() {
                Some(key) => options.picker.get_or_insert_with(Default::default).select = Some(key),
                None => eprintln!("--select needs an aerodrome code or latitude,longitude"),
            },
            "--menu" => options.menu = args.next(),
            "--clean" => options.clean = true,
            "--zoom" => options.zoom = number(&mut args).unwrap_or(0.0),
            "--manual" => options.manual = Some(number(&mut args).map_or(0, |n| (n.max(1.0) as usize) - 1)),
            "--help" | "-h" => {
                println!("{USAGE}");
                return None;
            }
            "--record" => *record = args.next(),
            "--script" => *script = args.next(),
            "--only" => only.extend(args.next().iter().flat_map(|list| list.split(',').map(str::to_string))),
            "--storyboard" => *storyboard = true,
            "--remote" => options.remote = Some(number(&mut args).map_or(::remote::DEFAULT_PORT, |port| port as u16)),
            other => eprintln!("unknown option: {other}"),
        }
    }
    // Rivals and no course: the built-in one.
    if !options.rivals.is_empty() && options.course.is_none() {
        if let Some((index, course)) = race::find(race::DEFAULT_COURSE).or_else(|| sim_core::course::file::courses().iter().enumerate().next()) {
            options.course = Some(index);
            match &course.place {
                sim_core::course::Place::Seed(seed) => {
                    options.seed = *seed;
                    course_seed = true;
                }
                sim_core::course::Place::Airport(code) => place = Some(code.clone()),
            }
        }
    }
    options.start = match position {
        Some((n, e)) => StartAt::Position(n, e),
        None if on_final => StartAt::Final,
        None if air => StartAt::Air,
        // Over the point given, which is the middle of the region.
        None if over_point => StartAt::Position(0.0, 0.0),
        None => StartAt::Runway,
    };
    // The menus in the window, unless the player took them away or another screen was asked for:
    // the first screen (planes, drones or cars), or the start screen if the command line already
    // says what is flown or where. A game opened to be flown by another program starts flying:
    // nobody is there to close a menu.
    if window && shot.is_none() && record.is_none() && options.remote.is_none() && options.menu.is_none() && options.picker.is_none() && options.manual.is_none() && settings::Store::user().flag(ui::menu::side::HOME_AT_START).unwrap_or(true) {
        options.menu = Some(opening_menu(*chosen).to_string());
    }
    // The picker at the start, if the player left it so and asked for no place.
    if window && shot.is_none() && record.is_none() && place.is_none() && settings::Store::user().flag(places::PICKER_AT_START).unwrap_or(false) {
        options.picker.get_or_insert_with(Default::default);
    }
    // A real place: the one asked for, or in the window the one chosen last time in the settings.
    let saved = || settings::Store::user().word("place").filter(|key| !key.is_empty()).map(str::to_string);
    if let Some(key) = place.or_else(|| (window && shot.is_none() && record.is_none() && !course_seed).then(saved).flatten()) {
        match geodata::Place::parse(&key).ok_or_else(|| format!("unknown place {key}")).and_then(|p| places::load_now(&p, offline).map_err(|e| e.to_string())) {
            Ok(region) => {
                options.region = Some(Arc::new(region));
                options.place = Some(key);
            }
            Err(error) => eprintln!("{error}: flying in the generated world"),
        }
    }
    Some(command)
}

/// The menu the window opens on (`--menu`'s word): the first screen, where the player says what to
/// drive, unless the command line has `chosen` what is flown or where: then the start screen.
#[cfg(not(target_arch = "wasm32"))]
fn opening_menu(chosen: bool) -> &'static str {
    if chosen {
        "home"
    } else {
        "vehicles"
    }
}

/// The size of `--size`: `<width>x<height>`, each from 200 to 8192 pixels.
#[cfg(not(target_arch = "wasm32"))]
fn picture_size(text: &str) -> Option<(u32, u32)> {
    let (w, h) = text.split_once('x')?;
    let (w, h) = (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?);
    ((200..=8192).contains(&w) && (200..=8192).contains(&h)).then_some((w, h))
}

/// The picture of a screenshot: `--retina`, `--size`.
#[cfg(not(target_arch = "wasm32"))]
struct Picture {
    retina: bool,
    size: Option<(u32, u32)>,
}

/// How a screenshot flies before its picture: `--land`, `--replay`.
#[cfg(not(target_arch = "wasm32"))]
struct Flown {
    land: bool,
    replay: Option<f64>,
}

#[cfg(not(target_arch = "wasm32"))]
/// Renders one frame with its overlay to `<directory>/scene.png` without opening a window, and
/// prints how long the terrain took, the cost of a frame and where the aircraft is.
#[cfg(not(target_arch = "wasm32"))]
fn write_shot(directory: &str, options: &Options, picture: Picture, fail: &[String], afcs: &[String], wait: f64, flown: Flown) {
    if let Err(error) = std::fs::create_dir_all(directory) {
        eprintln!("could not create {directory}: {error}");
        std::process::exit(1);
    }
    let (width, height, scale) = match picture {
        Picture { size: Some((width, height)), .. } => (width, height, 1.0),
        Picture { retina: true, .. } => (2880, 1800, 2.0),
        _ => (1600, 1000, 1.0),
    };
    let path = format!("{directory}/scene.png");
    let start = Instant::now();
    // Once the terrain is there: the frame it was ready at and, after the warm-up, the time, to
    // measure `SHOT_TIMED` more frames, and how long each of them took.
    let mut measuring: Option<(usize, Option<Instant>)> = None;
    let mut times: Vec<f64> = Vec::with_capacity(SHOT_TIMED);
    let mut last = Instant::now();
    let game = engine::render_to_png(
        &path,
        width,
        height,
        scale,
        |gpu| {
            // Saved settings and sound stay out of a screenshot, so it is the same on every machine.
            let mut game = Flight::new(gpu, options, settings::Store::memory(), audio::Audio::silent());
            // Instruments failed first, then two seconds (and the wait) for the aircraft and its
            // instruments to settle; then the picture stands still.
            let player = game.session.player;
            for name in fail {
                if !instrument_keys::fail(game.session.world.vehicle_mut(player), name) {
                    eprintln!("--fail: unknown {name}");
                }
            }
            if flown.land || flown.replay.is_some() || game.session.race.is_some() {
                // Flown as the game flies, so the replay keeps it (and a race is raced); landing
                // first if asked.
                if flown.land {
                    game.session.engage_landing();
                }
                for _ in 0..((2.0 + wait.max(0.0)) * 60.0) as usize {
                    game.session.advance(1.0 / 60.0, &game::Held::default());
                }
            } else if afcs.is_empty() {
                for _ in 0..480 + (wait.max(0.0) * 240.0) as usize {
                    game.session.world.step();
                }
            } else {
                // Two seconds flown for the G1000 to see the aircraft, the GFC 700 worked, then the wait
                // flown by it (and the keyboard's hands).
                for _ in 0..120 {
                    game.session.advance(1.0 / 60.0, &game::Held::default());
                }
                for word in afcs {
                    if let Err(error) = game.session.afcs_command(word) {
                        eprintln!("{error}");
                    }
                }
                for _ in 0..(wait.max(0.0) * 60.0) as usize {
                    game.session.advance(1.0 / 60.0, &game::Held::default());
                }
            }
            if let Some(seconds) = options.crash {
                game.session.crash_ahead(seconds);
            }
            if let Some(what) = options.damage {
                game.session.damage_ahead(what);
            }
            if let Some(back) = flown.replay {
                game.session.start_replay();
                if let (Some(p), Some((_, end))) = (&mut game.session.playback, game.session.recorder.span()) {
                    (p.time, p.playing) = (end - back, false);
                }
                game.session.advance(0.0, &game::Held::default());
            }
            game.session.frozen = true;
            game
        },
        |game, frames| match measuring {
            None => {
                // The overlay only knows the size of the picture from its second frame on, hence the minimum.
                if (game.pending() > 0 || frames < 5) && frames < SHOT_FRAME_CAP {
                    std::thread::sleep(Duration::from_millis(20));
                    return false;
                }
                println!("terrain ready in {:.1} s ({frames} frames)", start.elapsed().as_secs_f64());
                measuring = Some((frames, None));
                false
            }
            // Loading leaves pauses between frames and the GPU slows its clock down meanwhile: it takes
            // some twenty frames in a row to be back at full speed, so those are not timed.
            Some((from, None)) => {
                if frames - from == SHOT_WARM_UP {
                    measuring = Some((frames, Some(Instant::now())));
                }
                false
            }
            Some((from, Some(since))) => {
                let now = Instant::now();
                if frames > from {
                    times.push((now - last).as_secs_f64() * 1000.0);
                }
                last = now;
                if frames - from < SHOT_TIMED {
                    return false;
                }
                // A frame now and then waits on the system: the median and the quartile say more than the mean.
                times.sort_by(f64::total_cmp);
                println!("frames: min {:.2} p25 {:.2} median {:.2} ms", times[0], times[times.len() / 4], times[times.len() / 2]);
                println!("render: {:.1} ms per frame at {width}x{height}, {} draw calls", since.elapsed().as_secs_f64() / SHOT_TIMED as f64 * 1000.0, game.draw_calls());
                true
            }
        },
    );
    if let Some(picker) = game.picker().filter(|_| game.session.picker_open) {
        let (tiles, bytes) = picker.tiles_kept();
        println!("choose place: {:.2} ms per frame for the screen (CPU), search {:.2} ms, {tiles} map tiles in memory ({:.0} MB)", picker.cost.1, picker.search_ms, bytes as f64 / 1e6);
    }
    println!("{}", game.session.position_line());
}

/// The view of `--map`: latitude, longitude and zoom.
#[cfg(not(target_arch = "wasm32"))]
fn view(text: &str) -> Option<picker::View> {
    let mut numbers = text.split(',').map(|n| n.trim().parse::<f64>().ok());
    let (lat, lon, zoom) = (numbers.next()??, numbers.next()??, numbers.next()??);
    Some(picker::View::at(lat.clamp(-85.0, 85.0), lon.clamp(-180.0, 180.0), zoom))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn menu_of(line: &str) -> Option<String> {
        parse(line.split_whitespace().map(str::to_string), false).and_then(|c| c.options.menu)
    }

    fn chosen(line: &str) -> bool {
        parse(line.split_whitespace().map(str::to_string), false).is_some_and(|c| c.chosen)
    }

    /// The window opens on the first screen; a command line that says what is flown or where goes
    /// past it, to the start screen.
    #[test]
    fn the_first_screen_is_for_a_command_line_that_chooses_nothing() {
        assert_eq!(opening_menu(false), "vehicles");
        assert_eq!(opening_menu(true), "home");
        for line in ["", "--quality low --fps", "--hour 18 --sky fair", "--ext --traffic 2"] {
            assert!(!chosen(line), "{line}");
        }
        for line in ["--aircraft fpv", "--course deepwood", "--air", "--final", "--pos 100 200", "--seed 3", "--ai", "--rival racer", "--model models/hold.fltmodel"] {
            assert!(chosen(line), "{line}");
        }
    }

    /// Without a window (a screenshot, a recording's shot) no menu opens unless it is asked for.
    #[test]
    fn no_menu_opens_without_a_window_unless_asked_for() {
        assert_eq!(menu_of("--quality low"), None);
        assert_eq!(menu_of("--aircraft fpv --air"), None);
        assert_eq!(menu_of("--menu vehicles").as_deref(), Some("vehicles"));
    }

    #[test]
    fn the_size_of_a_picture_is_read() {
        assert_eq!(picture_size("900x600"), Some((900, 600)));
        assert_eq!(picture_size("900"), None);
        assert_eq!(picture_size("900x0"), None);
        assert_eq!(picture_size("axb"), None);
    }
}
