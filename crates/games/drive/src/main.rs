//! Skylab Drive, the driving game: the command line, then either the window or one frame written
//! to a PNG (`--shot`).

mod audio;
mod camera;
mod controls;
mod effects;
#[cfg(test)]
mod feel;
mod game;
mod ghost;
mod guide;
mod hud;
mod look;
mod pause;
mod race;
mod replay;
mod rivals;
mod timing;

use std::time::{Duration, Instant};

use camera::View;
use game::{Drive, Options};
use world_render::Preset;

const USAGE: &str = "\
skylab-drive [--circuit <name>] [--car <name>] [--hour <h>] [--view <chase|bonnet|cockpit|trackside>]
             [--driver] [--at <m>] [--quality <ultra|high|medium|low>] [--cover <%>]
             [--paint <colour>] [--rain [<0-1>]] [--stunt <launch|lock|slide|run>]
             [--race [<laps>]] [--rivals <n>] [--rival <driver|net|<file>.fltmodel>] [--grid <k>] [--seed <n>]
             [--manual] [--no-abs] [--no-tc] [--no-esc] [--orbit <deg> [<deg> [<m>]]] [--menu]
             [--shot <dir> [--wait <s>] [--size <w>x<h>] [--hud] [--frames <n> [--sound <wav>]] [--replay <s>]]

  --circuit <name>  the circuit (default lakeside: Lakeside Park, in the world of seed 1)
  --car <name>      the car (default gt: the GT coupé of assets/cars/gt)
  --hour <h>        hour of the day, 0-24 (default 17.5, the golden hour)
  --view <v>        the camera: chase (behind, default), bonnet, cockpit or trackside
  --driver          the racing driver drives (X in the game takes over or hands back)
  --at <m>          start on the racing line this many metres past the start line, at speed
                    (with the racing driver driving, or yourself)
  --quality <q>     how much of the world is drawn
  --cover <%>       cloud cover (default 35)
  --manual          the sequential gearbox (E up, Q down) instead of the automatic
  --no-abs, --no-tc, --no-esc  anti-lock brakes, traction control, stability control off
                    (on by default, as in the road car; B, T and Y in the game)
  --paint <c>       the car's paint: red, blue, orange, green, yellow, white, silver, grey,
                    black, or rrggbb
  --rain [<r>]      it rains, r from 0 (dry) to 1 (a downpour), 0.6 if not given: the sky
                    closed, the road wet, its grip less, puddles that aquaplane the tyres at speed
                    (F6 and F7 in the game: harder, lighter; stopped, the racing line dries first)
  --race [<laps>]   a race against rivals from a standing start (3 laps; 5 rivals unless --rivals)
  --rivals <n>      rivals on the grid (0: free practice, alone with the ghost of your best lap)
  --rival <who>     what drives the next rival: driver (the racing driver, the default), net (the
                    trained network, models/drive.fltmodel) or a model file; repeatable
  --grid <k>        your place on the grid, 0 the pole (default: at the back)
  --seed <n>        the rivals' characters (pace, braking, line, mistakes) and the start's lights
  --no-lens         without what the lens adds (vignette, blur of speed, flare, depth of field)
  --stunt <what>    for pictures: launch (full throttle from rest), lock (the brakes hard on),
                    slide (a turn on the hand brake), or run (twenty seconds for the sound: idle,
                    full throttle up the gears, lift off, the brakes locked, a slide), nobody driving
  --line <full|braking|off>  the driving line on the road: the whole of it (as the game starts),
                    only where to slow down, or none (L in the game; off in a picture unless given)
  --orbit <y> [<p> [<m>]]  the chase camera turned y degrees round the car, p up, m metres from it
  --menu            start on the pause menu (Esc in the game): drive on, change vehicle (back to
                    Skylab's first screen: planes, drones or cars), quit; with --shot, to see it
  --shot <dir>      write one frame to <dir>/drive.png once the world is drawn, without a window;
                    --wait <s> simulates that many seconds first (the racing driver driving if
                    --driver), --size its size (1600x1000), --hud with the overlay; --frames <n>
                    then drives on and writes n frames, 60 a second, as <dir>/00000.png...;
                    --sound <wav> also writes their sound (48 kHz stereo) to <wav>, and what
                    it followed (revs, load, slides...) a line a frame to <wav without .wav>.csv;
                    --replay <s> shows the replay (F2) stopped s seconds before the end

Keys in the game: H shows them. Esc is the pause menu: its \"Change vehicle\" starts Skylab, the
program beside this one, on its first screen (planes, drones or cars), as Skylab's \"Cars\" starts
this game.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut options = Options::default();
    let mut shot: Option<String> = None;
    let (mut wait, mut size, mut hud, mut frames) = (0.0, (1600u32, 1000u32), false, 0usize);
    let mut sound: Option<String> = None;
    let mut i = 0;
    let next = |i: &mut usize| -> String {
        *i += 1;
        args.get(*i).cloned().unwrap_or_else(|| {
            eprintln!("{} needs a value\n\n{USAGE}", args[*i - 1]);
            std::process::exit(2);
        })
    };
    let number = |s: String| -> f64 {
        s.parse().unwrap_or_else(|_| {
            eprintln!("not a number: {s}");
            std::process::exit(2);
        })
    };
    while i < args.len() {
        match args[i].as_str() {
            "--circuit" => options.circuit = next(&mut i),
            "--car" => options.car = next(&mut i),
            "--hour" => options.hour = number(next(&mut i)).rem_euclid(24.0),
            "--view" => {
                let v = next(&mut i);
                options.view = View::parse(&v).unwrap_or_else(|| {
                    eprintln!("--view: chase, bonnet, cockpit or trackside, not {v}");
                    std::process::exit(2);
                })
            }
            "--driver" => options.driver = true,
            "--at" => options.at = Some(number(next(&mut i))),
            "--quality" => {
                let q = next(&mut i);
                options.quality = Preset::named(&q).unwrap_or_else(|| {
                    eprintln!("--quality: ultra, high, medium or low, not {q}");
                    std::process::exit(2);
                }).quality()
            }
            "--cover" => options.cover = (number(next(&mut i)) / 100.0).clamp(0.0, 1.0),
            "--rain" => {
                options.rain = match args.get(i + 1).and_then(|s| s.parse::<f64>().ok()) {
                    Some(r) => {
                        i += 1;
                        r.clamp(0.0, 1.0)
                    }
                    None => 0.6,
                }
            }
            "--race" => {
                if let Some(l) = args.get(i + 1).and_then(|s| s.parse::<u32>().ok()) {
                    options.laps = l.max(1);
                    i += 1;
                }
                if options.rivals == 0 {
                    options.rivals = 5;
                }
            }
            "--rivals" => options.rivals = number(next(&mut i)).clamp(0.0, 11.0) as usize,
            "--rival" => {
                let who = next(&mut i);
                options.rival_kinds.push(match who.as_str() {
                    "driver" | "racer" => rivals::Kind::Driver,
                    "net" | "network" => rivals::Kind::Network(look::assets().map(|a| a.parent().map(|p| p.join("models").join("drive.fltmodel")).unwrap_or_default().to_string_lossy().into_owned()).unwrap_or_else(|_| "models/drive.fltmodel".into())),
                    file => rivals::Kind::Network(file.into()),
                });
                options.rivals = options.rivals.max(options.rival_kinds.len());
            }
            "--grid" => options.grid = Some(number(next(&mut i)).max(0.0) as usize),
            "--seed" => options.seed = number(next(&mut i)) as u64,
            "--replay" => options.replay = Some(number(next(&mut i)).max(0.0)),
            "--no-lens" => options.plain = true,
            "--line" => {
                let w = next(&mut i);
                options.line = Some(guide::Mode::parse(&w).unwrap_or_else(|| {
                    eprintln!("--line: full, braking or off, not {w}");
                    std::process::exit(2);
                }));
            }
            "--stunt" => {
                let w = next(&mut i);
                options.stunt = Some(game::Stunt::parse(&w).unwrap_or_else(|| {
                    eprintln!("--stunt: launch, lock, slide or run, not {w}");
                    std::process::exit(2);
                }))
            }
            "--menu" => options.menu = true,
            "--manual" => options.automatic = false,
            "--abs" => options.abs = true,
            "--tc" => options.traction_control = true,
            "--no-abs" => options.abs = false,
            "--no-tc" => options.traction_control = false,
            "--no-esc" => options.stability_control = false,
            "--orbit" => {
                options.orbit.0 = number(next(&mut i));
                if let Some(p) = args.get(i + 1).and_then(|s| s.parse::<f64>().ok()) {
                    options.orbit.1 = p;
                    i += 1;
                    if let Some(d) = args.get(i + 1).and_then(|s| s.parse::<f64>().ok()) {
                        options.distance = Some(d);
                        i += 1;
                    }
                }
            }
            "--paint" => {
                let c = next(&mut i);
                options.paint = Some(paint(&c).unwrap_or_else(|| {
                    eprintln!("--paint: red, blue, orange, green, yellow, white, silver, grey, black or rrggbb, not {c}");
                    std::process::exit(2);
                }))
            }
            "--shot" => shot = Some(next(&mut i)),
            "--wait" => wait = number(next(&mut i)),
            "--size" => {
                let s = next(&mut i);
                size = s.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))).unwrap_or_else(|| {
                    eprintln!("--size: <w>x<h>, not {s}");
                    std::process::exit(2);
                });
            }
            "--hud" => hud = true,
            "--frames" => frames = number(next(&mut i)).max(0.0) as usize,
            "--sound" => sound = Some(next(&mut i)),
            "--help" | "-h" => {
                println!("{USAGE}");
                return;
            }
            other => {
                eprintln!("unknown option: {other}\n\n{USAGE}");
                std::process::exit(2);
            }
        }
        i += 1;
    }
    // A picture is of the game's world: the driving line only if asked for.
    if shot.is_some() && options.line.is_none() {
        options.line = Some(guide::Mode::Off);
    }
    match shot {
        Some(dir) if frames > 0 => take_frames(&dir, &options, wait, size, hud, frames, sound.as_deref()),
        Some(dir) => take_shot(&dir, &options, wait, size, hud),
        None => engine::run("Skylab Drive", 1600, 1000, |gpu| Drive::new(gpu, &options, audio::Audio::start(&options.car))),
    }
}

/// A paint by name or as sRGB hex digits (`rrggbb`), in linear light.
fn paint(word: &str) -> Option<[f32; 3]> {
    let hex = match word {
        "red" => "b0120e",
        "blue" => "0b2f8a",
        "orange" => "d6480a",
        "green" => "0d4a2a",
        "yellow" => "e0b010",
        "white" => "e8e8e4",
        "silver" => "9a9ea3",
        "grey" => "3a3d42",
        "black" => "0c0c0e",
        other => other,
    };
    if hex.len() != 6 {
        return None;
    }
    let channel = |k: usize| u8::from_str_radix(&hex[k..k + 2], 16).ok().map(|c| (c as f32 / 255.0).powf(2.2));
    Some([channel(0)?, channel(2)?, channel(4)?])
}

/// The frames of a video to `<dir>/00000.png`...: the world simulated `wait` seconds, drawn until
/// it is loaded, then driven on a sixtieth of a second a frame, `count` frames; and their sound to
/// `sound` (a WAV file, 48 kHz stereo), the car's starter first if it stands still.
fn take_frames(dir: &str, options: &Options, wait: f64, (width, height): (u32, u32), hud: bool, count: usize, sound: Option<&str>) {
    use engine::Game;
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("cannot make {dir}: {e}");
        std::process::exit(1);
    }
    let mut screen = engine::Offscreen::new(width, height, 1.0);
    let mut game = Drive::new(&screen.gpu, options, audio::Audio::silent());
    game.overlay = hud;
    game.run(wait.max(0.0));
    let input = engine::Input::default();
    let step = 1.0 / 60.0;
    // The world loads with the picture standing still, and the exposure settles.
    game.paused = true;
    let mut settled = 0;
    for frame in 0..3000 {
        screen.gpu.set_samples(game.samples());
        game.update(&input, step, &screen.gpu);
        screen.draw(&mut game, step);
        screen.gpu.wait();
        if game.renderer.pending() > 0 && frame < 2900 {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        settled += 1;
        if settled > 60 {
            break;
        }
    }
    game.paused = false;
    const RATE: u32 = 48000;
    let mut mixer = sound.map(|_| {
        let model = &sim_core::CarKind::parse(&options.car).expect("the car").spec().visual.model;
        let sounds = audio::Sounds::of_model(model).unwrap_or_else(|e| {
            eprintln!("the car's sound: {e}");
            std::process::exit(1);
        });
        audio::Mixer::new(std::sync::Arc::new(sounds), game.sound.speed < 1.0)
    });
    let mut samples: Vec<f32> = Vec::new();
    // What the sound followed, a line a frame, to check it against (`<wav>.csv`).
    let mut lines = String::from("t,rpm,load,limiter,speed,squeal,skid,hard,kerb,gravel,grass,impact,scrape,distance,closing\n");
    let mut block = vec![0.0f32; 2 * RATE as usize / 60];
    for k in 0..count {
        screen.gpu.set_samples(game.samples());
        game.update(&input, step, &screen.gpu);
        if let Some(m) = &mut mixer {
            m.render(&game.sound, &mut block, 2, RATE as f32);
            samples.extend_from_slice(&block);
            let p = &game.sound;
            let l = &p.listener;
            lines += &format!(
                "{:.4},{:.1},{:.3},{},{:.2},{:.3},{:.3},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.1},{:.2}\n",
                k as f64 * step, p.rpm, p.load, p.limiter, p.speed, p.squeal, p.skid, p.hard, p.kerb, p.gravel, p.grass, p.impact, p.scrape, l.distance, l.closing
            );
        }
        screen.draw(&mut game, step);
        let pixels = screen.gpu.read_pixels();
        let path = format!("{dir}/{k:05}.png");
        if let Err(e) = image::save_buffer(&path, &pixels, width, height, image::ColorType::Rgba8) {
            eprintln!("{path}: {e}");
            std::process::exit(1);
        }
    }
    println!("{count} frames in {dir}");
    if let Some(path) = sound {
        if let Err(e) = std::fs::write(path, engine::audio::wav_bytes(&samples, 2, RATE)) {
            eprintln!("{path}: {e}");
            std::process::exit(1);
        }
        let csv = format!("{}.csv", path.strip_suffix(".wav").unwrap_or(path));
        if let Err(e) = std::fs::write(&csv, lines) {
            eprintln!("{csv}: {e}");
            std::process::exit(1);
        }
        println!("their sound in {path}, what it followed in {csv}");
    }
}

/// One frame to `<dir>/drive.png`: the world simulated `wait` seconds, then drawn once everything is loaded.
fn take_shot(dir: &str, options: &Options, wait: f64, (width, height): (u32, u32), hud: bool) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("cannot make {dir}: {e}");
        std::process::exit(1);
    }
    let path = format!("{dir}/drive.png");
    let start = Instant::now();
    let mut settled = 0;
    // The frames after the world is loaded are timed (each waits for the GPU): the picture's cost.
    let mut times: Vec<f64> = Vec::new();
    let mut last = Instant::now();
    engine::render_to_png(
        &path,
        width,
        height,
        1.0,
        |gpu| {
            let mut game = Drive::new(gpu, options, audio::Audio::silent());
            game.overlay = hud;
            if let Some(g) = &mut game.ghost {
                g.save = false;
            }
            game.run(wait.max(0.0));
            if let Some(before) = options.replay {
                game.begin_replay(Some(before));
                if let Some(p) = &mut game.playback {
                    p.paused = true;
                }
            }
            // The picture stands still while the world loads.
            game.paused = true;
            game
        },
        |game, frames| {
            if game.renderer.pending() > 0 && frames < 3000 {
                std::thread::sleep(Duration::from_millis(20));
                return false;
            }
            // A few frames more for the chase camera and the exposure to settle.
            let now = Instant::now();
            if settled > 0 {
                times.push((now - last).as_secs_f64() * 1000.0);
            }
            last = now;
            settled += 1;
            settled > 90
        },
    );
    times.sort_by(f64::total_cmp);
    if !times.is_empty() {
        println!("frames: min {:.2} p25 {:.2} median {:.2} ms at {width}x{height}", times[0], times[times.len() / 4], times[times.len() / 2]);
    }
    println!("{path} in {:.1} s", start.elapsed().as_secs_f64());
}
