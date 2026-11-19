//! A race round a course (`sim_core::course`) in the game: the courses read at start (every
//! `assets/courses/<key>.ron` but `gates.ron`, else the built-in ones), and the race the player
//! flies: on the start's pad, a countdown (3, 2, 1, GO) with the sticks held, the clock from GO,
//! each gate flown through (a chime, its split against the last lap's), a gate missed (a buzz and a
//! warning), the laps, the best lap kept in the settings (`best_lap_<course>`), the results at the
//! finish. What it shows is `ui::osd::RaceOsd`; the gates are drawn by `render::course`.
//!
//! `Session` owns one (`Session::race`): `start_race`, `restart_race` (the restart keys) and
//! `end_race`; after every step of the world it calls `RaceSession::step`. `racer` is the
//! simulation's racing pilot flying the player's quad instead (L, and recordings). `rivals` are
//! quads the game flies against the player (`--rival`, `--ai`): the racer, or a network trained on
//! the `gates` task (`trainer/GATES.md`), each with its own count of the race; the standings and the
//! position (P1/3) come from all of them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sim_core::course::racer::Racer;
use sim_core::course::{file, Course, Event, Race, Track};
use sim_core::{MultirotorControls, VehicleId, World, V3};
use sim_env::{Guidance, Model, ModelPilot};

use crate::render::{to_render, ViewPoint};
use crate::settings::Store;
use crate::ui::osd::{Label, Marker, RaceOsd, Standing};

/// Seconds of the countdown: 3, 2, 1, then GO.
pub const COUNTDOWN: f64 = 3.0;
/// The notes of the chimes, Hz (a negative one is the missed gate's buzz).
const BEEP: f32 = 660.0;
const GO: f32 = 1320.0;
const GATE: f32 = 1046.5;
const LAP: f32 = 1568.0;
const MISSED: f32 = -196.0;

/// Reads the courses: the built-in ones, with any file of `assets/courses` in place of the
/// built-in one of its key or after them. Without the folder, the built-in ones.
pub fn load() -> Result<(), String> {
    let files = match folder() {
        Some(dir) => read(&dir)?,
        None => Vec::new(),
    };
    let list = file::courses_with(&files).map_err(|e| e.to_string())?;
    file::set_courses(list)
}

fn folder() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::var_os("FLIGHT_ASSETS") {
        candidates.push(Path::new(&dir).join("courses"));
    }
    let exe = std::env::current_exe().ok().and_then(|exe| exe.canonicalize().ok()).and_then(|exe| exe.parent().map(Path::to_path_buf));
    for start in [exe, std::env::current_dir().ok()].into_iter().flatten() {
        candidates.extend(start.ancestors().map(|dir| dir.join("assets/courses")));
    }
    candidates.into_iter().find(|dir| dir.is_dir())
}

/// Each course's file of the folder (every `.ron` but `gates.ron`), sorted: (its path, its text).
fn read(dir: &Path) -> Result<Vec<(String, String)>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "ron") && p.file_name().is_some_and(|n| n != "gates.ron")).collect();
    paths.sort();
    paths.into_iter().map(|p| std::fs::read_to_string(&p).map(|text| (p.display().to_string(), text)).map_err(|e| format!("{}: {e}", p.display()))).collect()
}

/// The course of this key (or of a word of its name), and its place in the list.
pub fn find(word: &str) -> Option<(usize, &'static Course)> {
    let word = word.to_lowercase();
    file::courses().iter().enumerate().find(|(_, c)| c.key == word || c.name.to_lowercase().split_whitespace().any(|w| w == word))
}

/// The settings' key a course's best lap is kept under.
pub fn best_key(course: &Course) -> String {
    format!("best_lap_{}", course.key)
}

/// The word of `--rival` (and `--model`) for the course's own racer as an opponent.
pub const RACER: &str = "racer";
/// The word of `--rival` for the course's trained network (`--ai`): `models/gates-<course>.fltmodel`
/// if there is one (`gates-deepwood.fltmodel`), else `models/gates.fltmodel` (Pinewood's).
pub const NET: &str = "net";
/// The course raced with rivals when none is named (`--ai` alone).
pub const DEFAULT_COURSE: &str = "deepwood";

/// The network trained for a course, by its key (`NET`).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn course_model(key: &str) -> Option<PathBuf> {
    find_model(&format!("gates-{key}.fltmodel")).or_else(|| find_model("gates.fltmodel"))
}

/// The colours of the rivals' quads, in turn (the player's quad keeps its own).
pub const RIVAL_COLOURS: [[u8; 3]; 4] = [[255, 70, 60], [70, 150, 255], [255, 200, 40], [190, 90, 255]];

/// Where a model of the `gates` race is looked for when only its name is given (`--ai`): the
/// folder `models` of the working directory or of a folder above it or above the program.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn find_model(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok().and_then(|exe| exe.canonicalize().ok()).and_then(|exe| exe.parent().map(Path::to_path_buf));
    let mut candidates = Vec::new();
    for start in [std::env::current_dir().ok(), exe].into_iter().flatten() {
        candidates.extend(start.ancestors().map(|dir| dir.join("models").join(name)));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Whether `spec` (a `--model`) is an opponent for a race: the racer, or a model of `gates`.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn is_rival(spec: &str) -> bool {
    spec == RACER || spec == NET || (spec.ends_with(".fltmodel") && Model::load(spec).is_ok_and(|m| m.task == "gates"))
}

/// What flies a rival: a trained network (and, once it has finished, the racer to hover where it
/// is), or the course's racer.
enum Brain {
    Network { model: Box<Model>, pilot: Option<Box<ModelPilot>>, after: Option<Box<Racer>> },
    Racer(Box<Racer>),
}

/// A quad flown by the game round the course against the player (`--rival`, `--ai`): its own
/// race, counted as the player's from the same GO.
pub struct Rival {
    pub id: VehicleId,
    /// For the display: `NET`, `RACER`...
    pub name: String,
    pub colour: [u8; 3],
    brain: Brain,
    pub race: Race,
    pub crashed: bool,
}

impl Rival {
    /// A rival of `spec` (`racer`, or a model file of the `gates` task) in quad `id` of `world`,
    /// its colour the `k`th.
    pub fn new(spec: &str, id: VehicleId, k: usize, track: &Arc<Track>, world: &World) -> Result<Rival, String> {
        let colour = RIVAL_COLOURS[k % RIVAL_COLOURS.len()];
        let laps = track.course.laps;
        if spec == RACER {
            let p = &world.vehicle(id).multirotor().ok_or("a rival flies a multirotor")?.p;
            let brain = Brain::Racer(Box::new(Racer::new(track.clone(), p)));
            return Ok(Rival { id, name: "RACER".to_string(), colour, brain, race: Race::new(laps), crashed: false });
        }
        let found;
        let spec = if spec == NET {
            found = course_model(&track.course.key).ok_or_else(|| format!("no models/gates-{}.fltmodel nor models/gates.fltmodel here or above", track.course.key))?;
            found.to_str().ok_or("a model's path that is not text")?
        } else {
            spec
        };
        let model = Model::load(spec)?;
        if model.task != "gates" {
            return Err(format!("{spec} was trained for {}, not for the gates race", model.task));
        }
        ModelPilot::new(model.clone())?;
        let stem = Path::new(spec).file_stem().map_or("NET".to_string(), |s| s.to_string_lossy().to_uppercase());
        let name = if stem == "GATES" || stem.starts_with("GATES-") { "NET".to_string() } else { stem };
        let brain = Brain::Network { model: Box::new(model), pilot: None, after: None };
        Ok(Rival { id, name, colour, brain, race: Race::new(laps), crashed: false })
    }

    /// Whether a trained network flies it (not the racer).
    pub fn network(&self) -> bool {
        matches!(self.brain, Brain::Network { .. })
    }

    /// The start signal, the quad where it is.
    fn go(&mut self, track: &Arc<Track>, world: &World, now: f64) {
        let pos = world.vehicle(self.id).body().pos;
        self.race.go(now, pos);
        match &mut self.brain {
            Brain::Network { model, pilot, after } => {
                let guidance = Guidance::gates(track.clone(), now, pos, 0);
                *pilot = ModelPilot::new((**model).clone()).ok().map(|p| Box::new(p.with_guidance(guidance)));
                *after = None;
            }
            Brain::Racer(racer) => racer.go(now, pos),
        }
    }

    /// Before each step of the world: before GO the sticks are held (armed, the throttle down);
    /// then its pilot flies it; once finished it hovers; crashed, nobody flies it.
    fn fly(&mut self, track: &Arc<Track>, world: &mut World) {
        if self.crashed {
            return;
        }
        if self.race.started.is_none() {
            if let Some(m) = world.vehicle_mut(self.id).multirotor_mut() {
                m.controls = MultirotorControls { arm: true, ..MultirotorControls::default() };
            }
            return;
        }
        let finished = self.race.finished.is_some();
        match &mut self.brain {
            Brain::Racer(racer) => world.fly(self.id, racer.as_mut()),
            Brain::Network { after: Some(racer), .. } => world.fly(self.id, racer.as_mut()),
            Brain::Network { pilot, after, .. } if finished => {
                // Over the line: the racer takes it, to slow down and hover.
                let Some(m) = world.vehicle(self.id).multirotor() else { return };
                let mut racer = Racer::new(track.clone(), &m.p);
                racer.take_over(&self.race);
                *pilot = None;
                world.fly(self.id, &mut racer);
                *after = Some(Box::new(racer));
            }
            Brain::Network { pilot: Some(pilot), .. } => world.fly(self.id, pilot.as_mut()),
            Brain::Network { .. } => {}
        }
    }

    /// After each step of the world: the gates it flew through, a crash.
    fn step(&mut self, track: &Track, world: &World) {
        if self.race.started.is_none() || self.crashed || self.race.finished.is_some() {
            return;
        }
        let vehicle = world.vehicle(self.id);
        if vehicle.dynamics.crashed().is_some() {
            self.crashed = true;
            return;
        }
        self.race.update(track, vehicle.body().pos, world.time());
    }
}

/// Where a racer stands: finished first (the quickest first), then those racing (the furthest
/// round first), then those crashed (the furthest first). Higher is better.
fn order_key(race: &Race, crashed: bool, track: &Track, pos: V3) -> (u8, f64) {
    match (race.finished, crashed) {
        (Some(total), _) => (3, -total),
        (None, false) => (2, if race.started.is_some() { race.progress(track, pos) } else { 0.0 }),
        (None, true) => (1, f64::from(race.passed)),
    }
}

/// One of those in a race, for the standings: its quad, its name, its colour, its race, whether it crashed.
pub type Entrant<'a> = (VehicleId, String, [u8; 3], &'a Race, bool);

/// A race being flown.
pub struct RaceSession {
    /// The course, by its place in `file::courses`.
    pub index: usize,
    pub track: Arc<Track>,
    pub race: Race,
    /// When the countdown began (the world's seconds); GO is `COUNTDOWN` later.
    pub countdown_at: f64,
    /// The best lap of this course ever (the settings'), and whether this race has set it.
    pub best: Option<f64>,
    pub new_best: bool,
    /// The quad crashed: the race is over without a time.
    pub crashed: bool,
    /// Flies the player's quad instead of the player.
    pub racer: Option<Box<Racer>>,
    /// The quads flown by the game against the player (`Rival`).
    pub rivals: Vec<Rival>,
    /// The chimes to sound, notes in Hz (`audio::Audio::chime`), taken each frame.
    pub chimes: Vec<f32>,
    /// The last gate's split and its difference from the last lap's, shown until a time.
    split: Option<(f64, Option<f64>, f64)>,
    /// A gate missed, said until a time.
    missed: Option<(usize, f64)>,
    beeps: u32,
}

impl RaceSession {
    /// A race on `track` (course `index`) whose countdown begins at `now`.
    pub fn new(index: usize, track: Arc<Track>, now: f64, store: &Store) -> RaceSession {
        let best = store.number(&best_key(&track.course)).filter(|t| *t > 0.0);
        RaceSession { index, race: Race::new(track.course.laps), track, countdown_at: now, best, new_best: false, crashed: false, racer: None, rivals: Vec::new(), chimes: Vec::new(), split: None, missed: None, beeps: 0 }
    }

    /// Before GO: the sticks are held where they are, the throttle down.
    pub fn holding(&self, now: f64) -> bool {
        self.race.started.is_none() && now < self.countdown_at + COUNTDOWN + 1.0
    }

    /// The racer flies the quad from now on, where the race is (L), or the player again.
    pub fn toggle_racer(&mut self, world: &World, id: VehicleId) {
        if self.racer.take().is_some() {
            return;
        }
        let Some(m) = world.vehicle(id).multirotor() else { return };
        let mut racer = Racer::new(self.track.clone(), &m.p);
        if self.race.started.is_some() {
            racer.take_over(&self.race);
        }
        self.racer = Some(Box::new(racer));
    }

    /// After a step of the world: the countdown, GO, and what the quad did at the gates.
    pub fn step(&mut self, world: &World, id: VehicleId, store: &mut Store) {
        let now = world.time();
        let vehicle = world.vehicle(id);
        let pos = vehicle.body().pos;
        if self.race.started.is_none() {
            // A beep a second, then GO.
            let due = ((now - self.countdown_at).floor() as i64 + 1).clamp(0, 4) as u32;
            while self.beeps < due {
                self.beeps += 1;
                self.chimes.push(if self.beeps == 4 { GO } else { BEEP });
            }
            if now >= self.countdown_at + COUNTDOWN {
                self.race.go(now, pos);
                if let Some(racer) = &mut self.racer {
                    racer.go(now, pos);
                }
                for rival in &mut self.rivals {
                    rival.go(&self.track, world, now);
                }
            }
            return;
        }
        for rival in &mut self.rivals {
            rival.step(&self.track, world);
        }
        if self.crashed || self.race.finished.is_some() {
            return;
        }
        if vehicle.dynamics.crashed().is_some() {
            self.crashed = true;
            self.chimes.push(MISSED);
            return;
        }
        for event in self.race.update(&self.track, pos, now) {
            match event {
                Event::Gate { gate, split } => {
                    self.chimes.push(GATE);
                    let k = self.race.splits.len().saturating_sub(1);
                    let delta = (gate != 0).then(|| self.race.last_splits.get(k).map(|last| split - last)).flatten();
                    if gate != 0 {
                        self.split = Some((split, delta, now + 2.0));
                    }
                    self.missed = None;
                }
                Event::Lap { time, .. } => {
                    self.chimes.push(LAP);
                    self.split = Some((time, None, now + 3.0));
                    if self.best.is_none_or(|best| time < best) {
                        self.best = Some(time);
                        self.new_best = true;
                        store.set(&best_key(&self.track.course), time);
                    }
                }
                Event::Finished { .. } => self.chimes.push(LAP),
                Event::Missed { gate } => {
                    if self.missed.is_none_or(|(_, until)| now > until - 1.5) {
                        self.chimes.push(MISSED);
                    }
                    self.missed = Some((gate, now + 2.5));
                }
            }
        }
    }

    /// Before each step of the world: the rival in quad `id` flies it.
    pub fn fly_rival(&mut self, world: &mut World, id: VehicleId) {
        let track = self.track.clone();
        if let Some(rival) = self.rivals.iter_mut().find(|r| r.id == id) {
            rival.fly(&track, world);
        }
    }

    /// The rival in quad `id`.
    pub fn rival(&self, id: VehicleId) -> Option<&Rival> {
        self.rivals.iter().find(|r| r.id == id)
    }

    /// Everyone in the race (the player's quad is `player`), in order: the place of each is its
    /// position, P1 the first.
    pub fn standings(&self, world: &World, player: VehicleId) -> Vec<Entrant<'_>> {
        let mut all: Vec<Entrant<'_>> = vec![(player, "YOU".to_string(), [255, 255, 255], &self.race, self.crashed)];
        all.extend(self.rivals.iter().map(|r| (r.id, r.name.clone(), r.colour, &r.race, r.crashed)));
        let key = |e: &Entrant<'_>| order_key(e.3, e.4, &self.track, world.vehicle(e.0).body().pos);
        // A stable sort: those level stay in the order they started in.
        all.sort_by(|a, b| key(b).partial_cmp(&key(a)).unwrap_or(std::cmp::Ordering::Equal));
        all
    }

    /// What the display shows of the race now, seen from `view` on a picture `aspect` wide for
    /// its height: of the quad `watched` (the player's, `player`, or a rival's), with the
    /// positions and each other quad's name over it when there are rivals.
    pub fn osd_for(&self, world: &World, player: VehicleId, watched: VehicleId, view: &ViewPoint, aspect: f32) -> RaceOsd {
        let now = world.time();
        let rival = self.rivals.iter().position(|r| r.id == watched);
        let mut osd = match rival {
            Some(k) => self.osd_rival(k, now, view, aspect),
            None => self.osd_own(now, view, aspect),
        };
        if self.rivals.is_empty() {
            return osd;
        }
        let standings = self.standings(world, player);
        osd.position = standings.iter().position(|s| s.0 == watched).map(|p| (p + 1, standings.len()));
        osd.standings = standings
            .iter()
            .map(|(id, name, colour, race, crashed)| {
                let detail = match (race.finished, crashed) {
                    (Some(total), _) => crate::ui::osd::lap_time(total as f32),
                    (None, true) => "CRASHED".to_string(),
                    (None, false) if race.started.is_none() => String::new(),
                    (None, false) => format!("LAP {} {}", (race.lap + 1).min(race.laps), gate_text(race, self.track.gates.len())),
                };
                Standing { name: name.clone(), colour: *colour, detail, watched: *id == watched }
            })
            .collect();
        // Each other quad's name over it, in its colour.
        for (id, name, colour, _, _) in &standings {
            if *id == watched {
                continue;
            }
            let m = marker(view, world.vehicle(*id).body().pos, aspect);
            if m.on_screen && m.distance < 400.0 {
                osd.labels.push(Label { marker: m, name: name.clone(), colour: *colour });
            }
        }
        osd.watching = rival.map(|k| self.rivals[k].name.clone());
        osd
    }

    /// The display of rival `k`'s race: its clock, laps and next gate.
    fn osd_rival(&self, k: usize, now: f64, view: &ViewPoint, aspect: f32) -> RaceOsd {
        let rival = &self.rivals[k];
        let r = &rival.race;
        let countdown = match self.race.started {
            None => Some((COUNTDOWN - (now - self.countdown_at)).ceil().max(1.0) as u32),
            Some(start) if now - start < 1.0 => Some(0),
            _ => None,
        };
        let racing = r.finished.is_none() && !rival.crashed;
        RaceOsd {
            name: self.track.course.name.clone(),
            countdown,
            time: r.elapsed(now) as f32,
            lap: (r.lap + 1).min(r.laps),
            laps: r.laps,
            lap_time: r.lap_time(now) as f32,
            gate: gate_text(r, self.track.gates.len()),
            last_lap: r.lap_times.last().map(|t| *t as f32),
            best: r.best().map(|t| t as f32),
            marker: racing.then(|| marker(view, self.track.target(r.next).0, aspect)),
            results: r.finished.map(|total| (r.lap_times.iter().map(|t| *t as f32).collect(), total as f32)),
            crashed: rival.crashed,
            clock: now as f32,
            ..RaceOsd::default()
        }
    }

    fn osd_own(&self, now: f64, view: &ViewPoint, aspect: f32) -> RaceOsd {
        let r = &self.race;
        let gates = self.track.gates.len();
        let countdown = match r.started {
            None => Some((COUNTDOWN - (now - self.countdown_at)).ceil().max(1.0) as u32),
            Some(start) if now - start < 1.0 => Some(0),
            _ => None,
        };
        let racing = r.started.is_some() && r.finished.is_none() && !self.crashed;
        let marker = (r.finished.is_none() && !self.crashed).then(|| marker(view, self.track.target(r.next).0, aspect));
        RaceOsd {
            name: self.track.course.name.clone(),
            countdown,
            time: r.elapsed(now) as f32,
            lap: (r.lap + 1).min(r.laps),
            laps: r.laps,
            lap_time: r.lap_time(now) as f32,
            gate: gate_text(r, gates),
            last_lap: r.lap_times.last().map(|t| *t as f32),
            best: self.best.map(|t| t as f32),
            new_best: self.new_best,
            split: self.split.filter(|s| now < s.2 && racing).map(|(t, d, _)| (t as f32, d.map(|d| d as f32))),
            missed: self.missed.filter(|m| now < m.1 && racing).map(|m| m.0),
            marker,
            results: r.finished.map(|total| (r.lap_times.iter().map(|t| *t as f32).collect(), total as f32)),
            crashed: self.crashed,
            racer: self.racer.is_some(),
            clock: now as f32,
            ..RaceOsd::default()
        }
    }

    /// The race's place in the order of what the renderer is told: the next gate (none before
    /// the start's pad is left or after the finish).
    pub fn next(&self) -> Option<usize> {
        (self.race.finished.is_none() && !self.crashed).then_some(self.race.next)
    }
}

/// The gate to fly through next, for the display: `3/15`, START or FINISH.
fn gate_text(r: &Race, gates: usize) -> String {
    match r.next {
        0 if r.passed == 0 => "START".to_string(),
        0 => "FINISH".to_string(),
        next => format!("{next}/{}", gates - 1),
    }
}

/// Where a point (world axes) is on the picture seen from `view`: within the picture if ahead and
/// inside it, else on its edge towards it.
pub fn marker(view: &ViewPoint, target: V3, aspect: f32) -> Marker {
    let d = to_render(target - view.position);
    let (f, r, u) = (d.dot(view.forward), d.dot(view.right), d.dot(view.up));
    let distance = d.length();
    let tan_y = (view.fov_y / 2.0).tan();
    let tan_x = tan_y * aspect;
    if f > 0.1 {
        let (x, y) = (r / f / tan_x, u / f / tan_y);
        if x.abs() <= 0.92 && y.abs() <= 0.92 {
            return Marker { x, y, on_screen: true, distance };
        }
    }
    // Off the picture: on its edge, the way to turn.
    let (mut x, mut y) = (r, u);
    if f < 0.0 && x.abs() < 1e-3 {
        x = 1.0;
    }
    let len = x.hypot(y).max(1e-6);
    (x, y) = (x / len, y / len);
    let k = 0.9 / x.abs().max(y.abs());
    Marker { x: x * k, y: y * k, on_screen: false, distance }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Held, Session};

    /// A session racing the built-in course, the countdown begun.
    fn racing() -> Session {
        let mut s = Session::new(1, Store::memory());
        s.start_race(0).unwrap();
        s
    }

    fn fly(s: &mut Session, held: Held, seconds: f64) {
        for _ in 0..(seconds * 60.0) as usize {
            s.advance(1.0 / 60.0, &held);
        }
    }

    /// The countdown holds the quad on its pad with the throttle up; then the clock runs from GO.
    #[test]
    fn the_countdown_holds_the_quad_and_go_starts_the_clock() {
        let mut s = racing();
        assert!(s.multirotor().is_some() && s.inside());
        let pad = s.body().pos;
        fly(&mut s, Held { throttle_up: true, ..Held::default() }, 2.0);
        assert!((s.body().pos - pad).length() < 0.3, "held on the pad");
        let race = s.race.as_ref().unwrap();
        assert!(race.race.started.is_none() && race.chimes.len() >= 2);
        fly(&mut s, Held { throttle_up: true, ..Held::default() }, 2.5);
        let race = s.race.as_ref().unwrap();
        assert!(race.race.started.is_some());
        assert!(s.body().pos.z < pad.z - 1.0, "it climbs after GO");
    }

    /// The racer flies the player's quad round the whole race; the best lap is kept in the
    /// settings and a restart key starts it again from the pad.
    #[test]
    fn the_racer_finishes_and_the_best_lap_is_kept() {
        let mut s = racing();
        let id = s.player;
        let (race, world) = (s.race.as_mut().unwrap(), &s.world);
        race.toggle_racer(world, id);
        let laps = race.race.laps as f64;
        fly(&mut s, Held::default(), COUNTDOWN + 100.0 * laps);
        let race = s.race.as_ref().unwrap();
        assert!(race.race.finished.is_some(), "finished: {:?}", race.race);
        let best = race.best.unwrap();
        assert_eq!(s.store.number(&best_key(&race.track.course)).map(|t| (t * 1000.0).round()), Some((best * 1000.0).round()));
        let osd = race.osd_for(&s.world, s.player, s.player, &s.view(), 1.6);
        assert!(osd.results.is_some() && osd.new_best);
        s.key_pressed(engine::KeyCode::Digit1);
        let race = s.race.as_ref().unwrap();
        assert!(race.race.started.is_none() && race.best == Some(best));
        assert!((s.body().pos - race.track.start).length() < 1.0);
    }

    /// The trained networks of the races, as the repository keeps them: Pinewood's, and
    /// Deepwood's (in its race quad).
    const NETWORK: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../models/gates.fltmodel");
    const DEEPWOOD_NETWORK: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../models/gates-deepwood.fltmodel");

    /// The place of a course in the list.
    fn course(key: &str) -> usize {
        find(key).unwrap().0
    }

    /// Rivals race with the player: the trained network and the racer, each in a quad of its
    /// own on the pad, held there until GO, then round the whole course (in calm air both finish);
    /// the standings put them first and the player, who never left the pad, last; N puts the
    /// camera on each quad in turn, its FPV camera; a restart puts them back on the pad, and the
    /// end of the race takes them away.
    #[test]
    fn rivals_race_the_course_and_the_camera_follows_them() {
        assert!(is_rival(NETWORK) && is_rival(RACER) && is_rival(NET) && !is_rival("models/race.pattern"));
        rivals_race(course("pinewood"), NETWORK);
    }

    /// The same at Deepwood, the forest course, in its race quads: the player's quad is the
    /// course's, the network is the course's own (found by `NET`, as `--ai` finds it).
    #[test]
    fn deepwood_rivals_race_the_course_and_the_camera_follows_them() {
        assert!(is_rival(DEEPWOOD_NETWORK));
        assert_eq!(course_model("deepwood").and_then(|p| p.canonicalize().ok()), Path::new(DEEPWOOD_NETWORK).canonicalize().ok());
        let s = rivals_race(course("deepwood"), NET);
        assert_eq!(s.multirotor().map(|m| m.p.kind), Some(sim_core::MultirotorKind::RACE5));
    }

    fn rivals_race(index: usize, network: &str) -> Session {
        let mut s = Session::new(1, Store::memory());
        s.rival_specs = vec![network.to_string(), RACER.to_string()];
        s.start_race(index).unwrap();
        let race = s.race.as_ref().unwrap();
        assert_eq!(race.rivals.len(), 2);
        assert!(race.rivals[0].network() && !race.rivals[1].network());
        let pads: Vec<V3> = race.rivals.iter().map(|r| s.world.vehicle(r.id).body().pos).collect();
        assert!(pads.iter().all(|p| (*p - race.track.start).length() < 4.5 && (*p - s.body().pos).length() > 1.5), "{pads:?}");
        fly(&mut s, Held::default(), 2.0);
        let race = s.race.as_ref().unwrap();
        for (r, pad) in race.rivals.iter().zip(&pads) {
            assert!((s.world.vehicle(r.id).body().pos - *pad).length() < 0.3, "held on the pad");
        }

        // The camera on each quad in turn, the FPV view kept.
        assert_eq!(s.viewed(), 0);
        s.key_pressed(engine::KeyCode::KeyN);
        assert!(s.inside() && s.viewed() == 1);
        let rival = s.race.as_ref().unwrap().rivals[0].id;
        assert!((s.view().position - s.world.vehicle(rival).body().pos).length() < 0.5);
        let osd = s.race.as_ref().unwrap().osd_for(&s.world, s.player, rival, &s.view(), 1.6);
        assert_eq!(osd.watching.as_deref(), Some("NET"));
        assert_eq!(osd.standings.len(), 3);

        let laps = s.race.as_ref().unwrap().race.laps as f64;
        fly(&mut s, Held::default(), COUNTDOWN + 100.0 * laps);
        let race = s.race.as_ref().unwrap();
        for r in &race.rivals {
            assert!(r.race.finished.is_some() && !r.crashed, "{} finished: {:?}", r.name, r.race);
        }
        let standings = race.standings(&s.world, s.player);
        assert_eq!(standings.last().map(|e| e.0), Some(s.player));
        let osd = race.osd_for(&s.world, s.player, rival, &s.view(), 1.6);
        let place = standings.iter().position(|e| e.0 == rival).unwrap() + 1;
        assert_eq!(osd.position, Some((place, 3)));
        assert!(osd.results.is_some());

        // Again from the pad, and no race.
        s.key_pressed(engine::KeyCode::Digit1);
        let race = s.race.as_ref().unwrap();
        assert!(race.rivals.iter().all(|r| r.race.started.is_none() && (s.world.vehicle(r.id).body().pos - race.track.start).length() < 4.5));
        assert_eq!(race.rivals.len(), 2);
        assert!(race.rivals.iter().all(|r| s.world.vehicle(r.id).multirotor().unwrap().p.kind == s.multirotor().unwrap().p.kind));
        s.end_race();
        assert_eq!(s.viewed(), 0);
        s.key_pressed(engine::KeyCode::KeyN);
        assert_eq!(s.viewed(), 0, "no other aircraft");
        s
    }

    /// The rivals from their side pads, in 4 kt of wind from every way (0° to 315°, each 45°): from
    /// 4 s after GO until it finishes neither stops in the air (once the old contact held two quads
    /// that touched hovering, glued, for seconds), and each finishes unless it crashed. Quads that
    /// come within 0.36 m of each other (their propellers' reach) are counted and said: they
    /// touch, and break or chip their propellers as `sim_core::multirotor` says.
    #[test]
    fn rivals_never_freeze_in_the_wind() {
        never_freeze(course("pinewood"), NETWORK);
    }

    /// The same at Deepwood: the trees round the trail, the logs and hoops, the other quad; a
    /// rival that touches a trunk or a gate bounces off or breaks a propeller, and is never held.
    #[test]
    fn deepwood_rivals_never_freeze_in_the_wind() {
        never_freeze(course("deepwood"), NET);
    }

    fn never_freeze(index: usize, network: &str) {
        let mut touches = 0;
        for from in (0..360).step_by(45) {
            let mut s = Session::new(1, Store::memory());
            s.rival_specs = vec![network.to_string(), RACER.to_string()];
            s.world.weather.wind_speed = 4.0 * 0.514444;
            s.world.weather.wind_from = from as f64;
            s.start_race(index).unwrap();
            let ids: Vec<VehicleId> = s.race.as_ref().unwrap().rivals.iter().map(|r| r.id).collect();
            let mut slowest = [f64::MAX; 2];
            let (mut closest, mut near, mut when) = (f64::MAX, false, (0.0, ""));
            let laps = s.race.as_ref().unwrap().race.laps as f64;
            for _ in 0..((COUNTDOWN + 100.0 * laps) * 60.0) as usize {
                s.advance(1.0 / 60.0, &Held::default());
                let race = s.race.as_ref().unwrap();
                for (k, r) in race.rivals.iter().enumerate() {
                    let since = r.race.started.map_or(0.0, |t| s.world.time() - t);
                    if since > 4.0 && r.race.finished.is_none() && !r.crashed {
                        slowest[k] = slowest[k].min(s.world.vehicle(r.id).body().vel.length());
                    }
                }
                // The closest two quads in the air, the player's (on its pad) too.
                let (mut gap, mut who) = (f64::MAX, "");
                for (i, a) in ids.iter().enumerate() {
                    for b in ids[i + 1..].iter().chain([&s.player]) {
                        let d = (s.world.vehicle(*a).body().pos - s.world.vehicle(*b).body().pos).length();
                        if d < gap {
                            (gap, who) = (d, if *b == s.player { "a rival and the player" } else { "the rivals" });
                        }
                    }
                }
                if gap < closest {
                    let go = s.race.as_ref().unwrap().race.started.unwrap_or(s.world.time());
                    (closest, when) = (gap, (s.world.time() - go, who));
                }
                if gap < 0.36 && !near {
                    touches += 1;
                }
                near = gap < 0.36;
            }
            let race = s.race.as_ref().unwrap();
            let outcome: Vec<String> = race
                .rivals
                .iter()
                .zip(slowest)
                .map(|(r, v)| {
                    let blades = s.world.vehicle(r.id).multirotor().unwrap().blades().iter().filter(|b| **b != [1.0, 1.0]).count();
                    let end = match (r.race.finished, r.crashed) {
                        (Some(t), _) => format!("finished {t:.1} s"),
                        (None, true) => format!("crashed: {:?}", s.world.vehicle(r.id).dynamics.crashed()),
                        _ => "racing".to_string(),
                    };
                    let slowest = if v < f64::MAX { format!("{v:.1} m/s") } else { "-".to_string() };
                    format!("{} {end}, slowest {slowest}, {blades} propellers struck", r.name)
                })
                .collect();
            println!("wind from {from:>3}°: closest {closest:.2} m ({}, {:.1} s after GO); {}", when.1, when.0, outcome.join("; "));
            for (r, v) in race.rivals.iter().zip(slowest) {
                assert!(r.crashed || r.race.finished.is_some(), "{} from {from}°: {:?}", r.name, r.race);
                assert!(r.crashed || v > 1.0, "{} from {from}° slowed to {v} m/s", r.name);
            }
        }
        println!("{touches} times two quads came within reach of each other");
    }

    /// A marker ahead is on the picture; one behind, on its edge.
    #[test]
    fn the_marker_points_at_the_next_gate() {
        let s = racing();
        let race = s.race.as_ref().unwrap();
        let view = s.view();
        let ahead = marker(&view, race.track.target(1).0, 1.6);
        assert!(ahead.on_screen && ahead.x.abs() < 0.3, "{ahead:?}");
        let behind = marker(&view, race.track.start - V3::new(race.track.start_heading.cos(), race.track.start_heading.sin(), 0.0) * 50.0, 1.6);
        assert!(!behind.on_screen && (behind.x.abs() > 0.85 || behind.y.abs() > 0.85), "{behind:?}");
    }
}
