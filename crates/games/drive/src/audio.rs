//! The car's sound, made of recordings as racing games make it (`assets/cars/<model>/sound/`,
//! described by its `sound.ron`, credited in its `CREDITS.md`):
//!
//! - **The engine**: loops of one real engine recorded on a dynamometer at steady revs, on load
//!   and off load, by a microphone at the exhaust and one in the engine bay. At every sample the
//!   two loops of each set nearest the revs are played, each resampled to the exact revs (its
//!   pitch times the revs over the revs it was recorded at), crossfaded by where the revs are
//!   between theirs; on and off load are blended by the engine's torque (so a gear change's fuel
//!   cut and lifting off are heard), and the limiter chops it.
//! - **An electric car's motors** (`motor` in place of `engine`), made here: the whine of each
//!   motor and its reduction gears, tones at whole orders of the shaft's turning (the electrical
//!   frequency of its poles, the slots of its stator, the teeth of its gears in mesh), louder the
//!   more torque they pass driving or regenerating; and, outside below 30 km/h, the hum it plays
//!   to warn people on foot (the law's AVAS), fading as the tyres take over.
//! - **The tyres**: a squeal as they come near the end of their grip (silent in an ordinary bend), a harsher scream from wheels
//!   spinning or locked, their roll on asphalt by the speed, the rumble of a kerb's ridges at the
//!   rate they pass under the wheels, gravel, grass.
//! - **The wind**, **impacts** and **scraping** along a wall.
//! - **Where it is heard from** (`Listener`): from the seat the engine bay's microphone through
//!   the cabin (muffled), the exhaust's from outside; from a trackside camera fainter with the
//!   distance, duller with the air, raised and lowered by the car's speed towards it (Doppler),
//!   to the left or right.
//!
//! The game writes a few numbers each frame (`Audio::update`); the audio thread reads them
//! through atomics and never waits for a lock. Every number the sound follows is smoothed sample
//! by sample, so nothing steps (no clicks, no zipper noise). `Mixer` is the same off line: it
//! renders the sound of a recorded run to a WAV file (`--shot ... --frames <n> --sound <wav>`).

use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use engine::audio::{AudioOutput, Clip};
use glam::{DVec3, Vec3};
use serde::Deserialize;
use sim_core::car::Surface;
use sim_core::Car;

use crate::camera::View;

/// Where the sound is heard from.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Listener {
    /// 1 inside the cabin (the cockpit view), 0 outside.
    pub inside: f32,
    /// How much of the engine is heard through the engine bay's microphone (the rest through the
    /// exhaust's): the intake and the mechanism.
    pub bay: f32,
    /// How far the car is, m, and how fast it comes closer, m/s (negative going away).
    pub distance: f32,
    pub closing: f32,
    /// Where it is, −1 left to 1 right.
    pub pan: f32,
}

impl Listener {
    /// The listener of a view: `eye` and the car's position and velocity in world axes, `right`
    /// the view's right in render axes.
    pub fn of(view: View, eye: DVec3, right: Vec3, car: DVec3, velocity: DVec3, eye_velocity: DVec3) -> Listener {
        let to_car = car - eye;
        let distance = to_car.length().max(0.1);
        let along = to_car / distance;
        let closing = -(velocity - eye_velocity).dot(along);
        let pan = world_render::to_render(along).dot(right).clamp(-1.0, 1.0);
        match view {
            View::Chase => Listener { inside: 0.0, bay: 0.15, distance: distance as f32, closing: 0.0, pan: 0.0 },
            View::Bonnet => Listener { inside: 0.0, bay: 0.65, distance: 2.0, closing: 0.0, pan: 0.0 },
            View::Cockpit => Listener { inside: 1.0, bay: 0.6, distance: 1.5, closing: 0.0, pan: -0.15 },
            View::Trackside => Listener { inside: 0.0, bay: 0.1, distance: distance as f32, closing: closing as f32, pan },
        }
    }
}

/// What the sound depends on.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Params {
    pub rpm: f32,
    /// The engine's torque over its most, −1 to 1: its load.
    pub load: f32,
    /// The limiter is cutting the fuel.
    pub limiter: f32,
    /// Speed over the ground, m/s.
    pub speed: f32,
    /// How near the tyres on a hard surface are to the end of their grip, by their share of the load:
    /// nothing below 80 % of what they can hold, 1 at the limit, more as they slide past it (a real
    /// tyre is silent in an ordinary bend and sings only near the limit); and how hard they spin or
    /// lock (m/s of sliding along times their share of the load).
    pub squeal: f32,
    pub skid: f32,
    /// The share of the wheels touching asphalt (or concrete), a kerb, gravel, grass or earth.
    pub hard: f32,
    pub kerb: f32,
    pub gravel: f32,
    pub grass: f32,
    /// The hardest impact since the last frame, m/s, and the scraping's speed.
    pub impact: f32,
    pub scrape: f32,
    /// 0 silences everything (paused).
    pub volume: f32,
    pub listener: Listener,
}

const SLOTS: usize = 18;

impl Params {
    /// The sound of a car as it is now, heard by `listener`; `impact` the hardest since the last.
    pub fn of(car: &Car, paused: bool, listener: Listener, impact: f64) -> Params {
        let d = car.data();
        let wheels = car.wheels();
        let load_total: f64 = wheels.iter().map(|w| w.load).sum::<f64>().max(1.0);
        let (mut squeal, mut skid) = (0.0, 0.0);
        for (k, w) in wheels.iter().enumerate().filter(|(_, w)| w.contact && matches!(w.surface, Surface::Asphalt | Surface::Kerb | Surface::Concrete)) {
            let share = w.load / load_total;
            // How much of the sliding is along the wheel (spinning or locked): by the slips.
            let along = w.slip.abs() / (w.slip.abs() + w.slip_angle.abs() + 1e-6);
            let spin = ((w.slip.abs() - 0.12) / 0.25).clamp(0.0, 1.0) * along;
            skid += w.sliding * share * spin;
            // The share of its grip the tyre uses: its force against what its load and the surface
            // let it hold.
            let can = w.load * car.p.axles[k / 2].tyre.grip * w.surface.grip();
            let used = if can > 1.0 { w.force.0.hypot(w.force.1) / can } else { 0.0 };
            let near = smoothstep64(0.8, 1.05, used);
            squeal += share * near * (1.0 + (w.sliding / 3.0).min(1.5)) * (1.0 - spin);
        }
        let share = |f: &dyn Fn(Surface) -> bool| wheels.iter().filter(|w| w.contact && f(w.surface)).count() as f32 / 4.0;
        Params {
            rpm: car.rpm() as f32,
            load: (d.engine_torque / peak_torque(car)) as f32,
            limiter: if d.limiter { 1.0 } else { 0.0 },
            // From the body itself: right after the car is put somewhere, the last step's data is old.
            speed: car.body.world_velocity().length() as f32,
            squeal: squeal as f32,
            skid: skid as f32,
            hard: share(&|s| matches!(s, Surface::Asphalt | Surface::Concrete)),
            kerb: share(&|s| s == Surface::Kerb),
            gravel: share(&|s| s == Surface::Gravel),
            grass: share(&|s| matches!(s, Surface::Grass | Surface::Dirt)),
            impact: impact as f32,
            scrape: d.scrape as f32,
            volume: if paused || car.sunk() { 0.0 } else { 1.0 },
            listener,
        }
    }

    fn to_slots(self) -> [f32; SLOTS] {
        let l = self.listener;
        [self.rpm, self.load, self.limiter, self.speed, self.squeal, self.skid, self.hard, self.kerb, self.gravel, self.grass, self.impact, self.scrape, self.volume, l.inside, l.bay, l.distance, l.closing, l.pan]
    }

    fn from_slots(v: [f32; SLOTS]) -> Params {
        Params {
            rpm: v[0],
            load: v[1],
            limiter: v[2],
            speed: v[3],
            squeal: v[4],
            skid: v[5],
            hard: v[6],
            kerb: v[7],
            gravel: v[8],
            grass: v[9],
            impact: v[10],
            scrape: v[11],
            volume: v[12],
            listener: Listener { inside: v[13], bay: v[14], distance: v[15], closing: v[16], pan: v[17] },
        }
    }
}

/// The most torque its engine, or its motors together, give, N m.
fn peak_torque(car: &Car) -> f64 {
    match &car.p.electric {
        Some(e) => e.motors.iter().map(|m| m.torque.iter().map(|t| t.1).fold(0.0, f64::max)).sum(),
        None => car.p.engine.peak_torque(),
    }
}

// --- The car's sound file (`sound.ron`) ---

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
enum Mic {
    Exhaust,
    Bay,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
enum Load {
    On,
    Off,
}

#[derive(Deserialize)]
struct LayerFile {
    file: String,
    mic: Mic,
    load: Load,
    rpm: f32,
}

#[derive(Deserialize)]
struct EngineFile {
    #[allow(dead_code)]
    cylinders: u32,
    layers: Vec<LayerFile>,
    on_level: Vec<(f32, f32)>,
    off_level: Vec<(f32, f32)>,
}

#[derive(Deserialize)]
struct SampleFile {
    file: String,
    gain: f32,
}

#[derive(Deserialize)]
struct StarterFile {
    file: String,
    gain: f32,
    catches: f32,
}

#[derive(Deserialize)]
struct KerbFile {
    file: String,
    gain: f32,
    beat_hz: f32,
}

/// A tone of an electric motor: its order (cycles a turn of the motor's shaft) and loudness.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Tone {
    order: f32,
    gain: f32,
}

/// An electric car's motors: their tones, how loud they are idle and at full torque, and the
/// pedestrians' warning hum (its pitches, Hz, and loudness), heard from outside below `avas_until` m/s.
#[derive(Clone, Debug, Deserialize)]
pub struct MotorFile {
    tones: Vec<Tone>,
    idle: f32,
    full: f32,
    avas: Vec<f32>,
    avas_gain: f32,
    avas_until: f32,
}

#[derive(Deserialize)]
struct SoundFile {
    #[serde(default)]
    engine: Option<EngineFile>,
    #[serde(default)]
    motor: Option<MotorFile>,
    #[serde(default)]
    starter: Option<StarterFile>,
    squeal: SampleFile,
    skid: SampleFile,
    road: SampleFile,
    kerb: KerbFile,
    gravel: SampleFile,
    wind: SampleFile,
    impact: SampleFile,
    scrape: SampleFile,
}

/// One loop of the engine at one speed.
struct Layer {
    clip: Clip,
    rpm: f32,
}

/// A recording and how loud it is played.
pub struct Sample {
    clip: Clip,
    gain: f32,
}

/// A car's sounds, read from its folder.
pub struct Sounds {
    /// The engine's loops: exhaust on load, exhaust off load, bay on load, bay off load, each by rpm.
    banks: [Vec<Layer>; 4],
    on_level: Vec<(f32, f32)>,
    off_level: Vec<(f32, f32)>,
    /// An electric car's in place of the engine's loops (then empty).
    motor: Option<MotorFile>,
    starter: Option<Sample>,
    catches: f32,
    squeal: Sample,
    skid: Sample,
    road: Sample,
    kerb: Sample,
    kerb_beat: f32,
    gravel: Sample,
    wind: Sample,
    impact: Sample,
    scrape: Sample,
}

fn bank(mic: Mic, load: Load) -> usize {
    (mic == Mic::Bay) as usize * 2 + (load == Load::Off) as usize
}

impl Sounds {
    /// The sounds of `assets/cars/<model>/sound`.
    pub fn of_model(model: &str) -> Result<Sounds, String> {
        Sounds::load(&crate::look::assets()?.join("cars").join(model).join("sound"))
    }

    /// The sounds described by `<dir>/sound.ron`.
    pub fn load(dir: &Path) -> Result<Sounds, String> {
        let path = dir.join("sound.ron");
        let text = engine::files::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let options = ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
        let file: SoundFile = options.from_str(&String::from_utf8_lossy(&text)).map_err(|e| format!("{}: {e}", path.display()))?;
        let clip = |name: &str| -> Result<Clip, String> {
            let p = dir.join(name);
            let bytes = engine::files::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            let clip = Clip::from_wav(&bytes).map_err(|e| format!("{}: {e}", p.display()))?;
            if clip.samples.len() < 64 {
                return Err(format!("{}: too short", p.display()));
            }
            Ok(clip)
        };
        let sample = |s: &SampleFile| -> Result<Sample, String> { Ok(Sample { clip: clip(&s.file)?, gain: s.gain }) };
        let mut banks: [Vec<Layer>; 4] = Default::default();
        let (on_level, off_level) = match (&file.engine, &file.motor) {
            (Some(engine), None) => {
                for l in &engine.layers {
                    banks[bank(l.mic, l.load)].push(Layer { clip: clip(&l.file)?, rpm: l.rpm });
                }
                for b in &mut banks {
                    if b.is_empty() {
                        return Err(format!("{}: the engine needs loops on and off load from both microphones", path.display()));
                    }
                    b.sort_by(|x, y| x.rpm.total_cmp(&y.rpm));
                }
                (engine.on_level.clone(), engine.off_level.clone())
            }
            (None, Some(_)) => (Vec::new(), Vec::new()),
            _ => return Err(format!("{}: either an engine or a motor", path.display())),
        };
        let starter = match &file.starter {
            Some(s) => Some(Sample { clip: clip(&s.file)?, gain: s.gain }),
            None => None,
        };
        Ok(Sounds {
            banks,
            on_level,
            off_level,
            motor: file.motor,
            starter,
            catches: file.starter.as_ref().map_or(0.0, |s| s.catches),
            squeal: sample(&file.squeal)?,
            skid: sample(&file.skid)?,
            road: sample(&file.road)?,
            kerb: Sample { clip: clip(&file.kerb.file)?, gain: file.kerb.gain },
            kerb_beat: file.kerb.beat_hz,
            gravel: sample(&file.gravel)?,
            wind: sample(&file.wind)?,
            impact: sample(&file.impact)?,
            scrape: sample(&file.scrape)?,
        })
    }
}

/// A piecewise straight line through `points` (x ascending), constant beyond its ends.
fn curve(points: &[(f32, f32)], x: f32) -> f32 {
    let Some(&(x0, y0)) = points.first() else { return 1.0 };
    if x <= x0 {
        return y0;
    }
    for w in points.windows(2) {
        let ((a, ya), (b, yb)) = (w[0], w[1]);
        if x <= b {
            return ya + (yb - ya) * (x - a) / (b - a).max(1e-6);
        }
    }
    points[points.len() - 1].1
}

/// The two loops of a bank to play at `rpm`, and their weights (equal power: the loops are not
/// alike sample by sample, so their powers add). Below the first and above the last, that one alone.
fn pair(layers: &[Layer], rpm: f32) -> [(usize, f32); 2] {
    let i = layers.partition_point(|l| l.rpm <= rpm);
    if i == 0 {
        return [(0, 1.0), (0, 0.0)];
    }
    if i == layers.len() {
        return [(i - 1, 1.0), (i - 1, 0.0)];
    }
    let (a, b) = (layers[i - 1].rpm, layers[i].rpm);
    let x = ((rpm / a).ln() / (b / a).ln()).clamp(0.0, 1.0);
    [(i - 1, (x * FRAC_PI_2).cos()), (i, (x * FRAC_PI_2).sin())]
}

/// Where a loop is being read, in samples of its clip.
#[derive(Clone, Copy, Default)]
struct Voice {
    pos: f64,
}

impl Voice {
    /// The clip's sample here (cubic interpolation, round its end), then on by `step` samples.
    fn read(&mut self, clip: &Clip, step: f64) -> f32 {
        let s = &clip.samples;
        let n = s.len();
        let i = self.pos as usize % n;
        let t = (self.pos - self.pos.floor()) as f32;
        let p0 = s[(i + n - 1) % n];
        let (p1, p2, p3) = (s[i], s[(i + 1) % n], s[(i + 2) % n]);
        let v = p1 + 0.5 * t * (p2 - p0 + t * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3 + t * (3.0 * (p1 - p2) + p3 - p0)));
        self.pos = (self.pos + step) % n as f64;
        v
    }
}

/// A one-shot being played: where, how loud, how fast.
#[derive(Clone, Copy)]
struct Shot {
    pos: f64,
    gain: f32,
    rate: f32,
}

/// A value that follows its target smoothly (one pole), sample by sample.
#[derive(Clone, Copy, Default)]
struct Smooth {
    value: f32,
}

impl Smooth {
    fn to(&mut self, target: f32, k: f32) -> f32 {
        self.value += (target - self.value) * k;
        self.value
    }
}

/// A low-pass filter of two poles (12 dB an octave).
#[derive(Clone, Copy, Default)]
struct LowPass {
    a: f32,
    b: f32,
}

impl LowPass {
    fn run(&mut self, x: f32, k: f32) -> f32 {
        self.a += (x - self.a) * k;
        self.b += (self.a - self.b) * k;
        self.b
    }
}

/// The coefficient of a one-pole low-pass at `hz`.
fn pole(hz: f32, rate: f32) -> f32 {
    1.0 - (-TAU * hz / rate).exp()
}

/// Everything the audio thread remembers between samples, and the sounds it plays.
pub struct Mixer {
    sounds: Arc<Sounds>,
    engine: [Vec<Voice>; 4],
    squeal: Voice,
    skid: Voice,
    road: Voice,
    kerb: Voice,
    gravel: Voice,
    grass: Voice,
    wind: Voice,
    scrape: Voice,
    impact: Option<Shot>,
    starter: Option<Shot>,
    /// Seconds since it started (the starter, the limiter's beat).
    time: f64,
    /// Seconds since the last impact was played.
    since_impact: f32,
    // What the sound follows, smoothed.
    rpm: Smooth,
    on: Smooth,
    limiter: Smooth,
    speed: Smooth,
    squeal_level: Smooth,
    skid_level: Smooth,
    hard: Smooth,
    kerb_level: Smooth,
    gravel_level: Smooth,
    grass_level: Smooth,
    scrape_level: Smooth,
    volume: Smooth,
    inside: Smooth,
    bay: Smooth,
    distance: Smooth,
    doppler: Smooth,
    pan: Smooth,
    engine_filter: LowPass,
    rest_filter: LowPass,
    grass_filter: LowPass,
    /// The engine's fade in once the starter has turned it over (1 without a starter).
    running: f32,
    /// An electric car's: the phase of each motor tone and of the warning hum's pitches (turns),
    /// its revs and torque smoothed, and a filter for the hum.
    tones: Vec<f64>,
    hum: Vec<f64>,
    motor_rpm: Smooth,
    motor_load: Smooth,
}

impl Mixer {
    /// A mixer of `sounds`; `starter`: the engine is first turned over by its starter.
    pub fn new(sounds: Arc<Sounds>, starter: bool) -> Mixer {
        let engine = std::array::from_fn(|b| vec![Voice::default(); sounds.banks[b].len()]);
        // The loops start at different places, so that no two play in step.
        let at = |k: f64, clip: &Clip| Voice { pos: (k * 0.37).fract() * clip.samples.len() as f64 };
        let s = &sounds;
        Mixer {
            engine,
            squeal: at(1.0, &s.squeal.clip),
            skid: at(2.0, &s.skid.clip),
            road: at(3.0, &s.road.clip),
            kerb: at(4.0, &s.kerb.clip),
            gravel: at(5.0, &s.gravel.clip),
            grass: at(6.5, &s.gravel.clip),
            wind: at(7.0, &s.wind.clip),
            scrape: at(8.0, &s.scrape.clip),
            impact: None,
            starter: s.starter.as_ref().filter(|_| starter).map(|st| Shot { pos: 0.0, gain: st.gain, rate: 1.0 }),
            time: 0.0,
            since_impact: 10.0,
            rpm: Smooth { value: 900.0 },
            on: Smooth::default(),
            limiter: Smooth::default(),
            speed: Smooth::default(),
            squeal_level: Smooth::default(),
            skid_level: Smooth::default(),
            hard: Smooth { value: 1.0 },
            kerb_level: Smooth::default(),
            gravel_level: Smooth::default(),
            grass_level: Smooth::default(),
            scrape_level: Smooth::default(),
            volume: Smooth::default(),
            inside: Smooth::default(),
            bay: Smooth { value: 0.2 },
            distance: Smooth { value: 6.0 },
            doppler: Smooth { value: 1.0 },
            pan: Smooth::default(),
            engine_filter: LowPass::default(),
            rest_filter: LowPass::default(),
            grass_filter: LowPass::default(),
            running: if starter && s.starter.is_some() { 0.0 } else { 1.0 },
            tones: vec![0.0; s.motor.as_ref().map_or(0, |m| m.tones.len())],
            hum: vec![0.0; s.motor.as_ref().map_or(0, |m| m.avas.len())],
            motor_rpm: Smooth::default(),
            motor_load: Smooth::default(),
            sounds,
        }
    }

    /// Fills `out` (interleaved, `channels` a frame) with the sound of `p`.
    pub fn render(&mut self, p: &Params, out: &mut [f32], channels: usize, rate: f32) {
        let channels = channels.max(1);
        let sounds = self.sounds.clone();
        let s = &*sounds;
        let dt = 1.0 / rate;
        // Smoothing: the revs and the load over a hundredth or two of a second (they arrive 60
        // times a second), the rest slower.
        let revs = 1.0 - (-dt / 0.02).exp();
        let fast = 1.0 - (-dt / 0.012).exp();
        let medium = 1.0 - (-dt / 0.04).exp();
        let slow = 1.0 - (-dt / 0.12).exp();
        let l = p.listener;
        // On load from the engine's torque: closed throttle (negative) is off load.
        let on_target = ((p.load + 0.05) / 0.55).clamp(0.0, 1.0);
        let doppler_target = (343.0 / (343.0 - l.closing.clamp(-150.0, 150.0))).clamp(0.5, 2.0);
        if p.impact > 1.0 && self.since_impact > 0.12 {
            let hard = (p.impact / 12.0).min(1.6);
            self.impact = Some(Shot { pos: 0.0, gain: hard.powf(0.8) * s.impact.gain, rate: 1.15 - 0.35 * (p.impact / 15.0).min(1.0) });
            self.since_impact = 0.0;
        }
        for frame in out.chunks_mut(channels) {
            self.time += dt as f64;
            self.since_impact += dt;
            let volume = self.volume.to(p.volume, 1.0 - (-dt / 0.15).exp());
            let rpm = self.rpm.to(p.rpm.max(300.0), revs);
            let on = self.on.to(on_target, fast);
            let limiter = self.limiter.to(p.limiter, medium);
            let speed = self.speed.to(p.speed, medium);
            let inside = self.inside.to(l.inside, medium);
            let bay = self.bay.to(l.bay, medium);
            let distance = self.distance.to(l.distance.max(0.5), medium);
            let doppler = self.doppler.to(doppler_target, medium);
            let pan = self.pan.to(l.pan, medium);

            // The engine. The limiter: the fuel cut and back some twenty times a second, the revs
            // bouncing with it.
            let beat = 0.5 + 0.5 * (TAU * (self.time * 21.0).fract() as f32).sin();
            let on = on * (1.0 - 0.85 * limiter * beat);
            let rpm = rpm * (1.0 - 0.012 * limiter * beat);
            let shares = [(1.0 - bay) * on.sqrt(), (1.0 - bay) * (1.0 - on).sqrt(), bay * on.sqrt(), bay * (1.0 - on).sqrt()];
            let mut engine = 0.0;
            for (b, share) in shares.iter().enumerate() {
                if *share < 1e-4 || s.banks[b].is_empty() {
                    continue;
                }
                let level = if b % 2 == 0 { curve(&s.on_level, rpm) } else { curve(&s.off_level, rpm) };
                for (i, w) in pair(&s.banks[b], rpm) {
                    if w > 1e-4 {
                        let layer = &s.banks[b][i];
                        let step = (rpm / layer.rpm * doppler * layer.clip.rate / rate) as f64;
                        engine += self.engine[b][i].read(&layer.clip, step) * w * share * level;
                    }
                }
            }
            // An electric car's whine is heard as the tyres are, through the cabin's
            // insulation (less muffled than an engine bay).
            let mut whine = 0.0;
            if let Some(m) = &s.motor {
                // The motors: each tone at its order of the shaft's turning, louder with the torque
                // passed (either way); the hum to warn people on foot, slowly, from outside.
                let shaft = self.motor_rpm.to(p.rpm.max(0.0), revs) / 60.0 * doppler;
                let load = self.motor_load.to(p.load.abs().min(1.5), fast);
                let level = m.idle + (m.full - m.idle) * load.min(1.0);
                let audible = smoothstep(0.0, 40.0, shaft);
                for (phase, tone) in self.tones.iter_mut().zip(&m.tones) {
                    let hz = shaft * tone.order;
                    if hz < rate * 0.45 {
                        *phase = (*phase + (hz / rate) as f64).fract();
                        whine += (TAU * *phase as f32).sin() * tone.gain * level * audible;
                    }
                }
                let warn = (1.0 - smoothstep(m.avas_until * 0.7, m.avas_until, speed)) * (1.0 - 0.8 * inside) * m.avas_gain;
                if warn > 1e-4 {
                    // Its pitch rises a little with the speed, as the law asks (so people hear it coming).
                    let rise = 1.0 + 0.25 * (speed / m.avas_until).min(1.0);
                    let n = m.avas.len().max(1) as f32;
                    for (phase, hz) in self.hum.iter_mut().zip(&m.avas) {
                        *phase = (*phase + (hz * rise * doppler / rate) as f64).fract();
                        let x = TAU * *phase as f32;
                        engine += (x.sin() + 0.3 * (2.0 * x).sin()) * warn / n;
                    }
                }
            }
            if let Some(shot) = &mut self.starter {
                let Some(starter) = &s.starter else { unreachable!("a starter's shot without its sound") };
                let clip = &starter.clip;
                let at = shot.pos as usize;
                if at + 1 < clip.samples.len() {
                    let t = (shot.pos - at as f64) as f32;
                    engine += (clip.samples[at] * (1.0 - t) + clip.samples[at + 1] * t) * shot.gain;
                    shot.pos += (clip.rate / rate * doppler) as f64;
                } else {
                    self.starter = None;
                }
                // The engine's own loops come in once it catches.
                self.running = ((self.time as f32 - s.catches) / 0.5).clamp(0.0, 1.0);
            } else {
                self.running = (self.running + dt / 0.5).min(1.0);
            }
            engine *= self.running;

            // The tyres, the road, the wind.
            let squeal = self.squeal_level.to(smoothstep(0.05, 1.6, p.squeal), medium);
            let skid = self.skid_level.to(smoothstep(0.5, 6.0, p.skid), medium);
            let hard = self.hard.to(p.hard + p.kerb, slow);
            let kerb = self.kerb_level.to(p.kerb, medium);
            let gravel = self.gravel_level.to(p.gravel, slow);
            let grass = self.grass_level.to(p.grass, slow);
            let scrape = self.scrape_level.to((p.scrape / 8.0).min(1.2), medium);
            let r = |clip: &Clip| (clip.rate / rate * doppler) as f64;
            let mut rest = whine;
            if squeal > 1e-3 {
                rest += self.squeal.read(&s.squeal.clip, r(&s.squeal.clip) * (0.92 + 0.12 * (p.squeal / 2.5).min(1.0)) as f64) * squeal * s.squeal.gain;
            }
            if skid > 1e-3 {
                rest += self.skid.read(&s.skid.clip, r(&s.skid.clip) * (0.9 + 0.15 * (speed / 30.0).min(1.0)) as f64) * skid * s.skid.gain;
            }
            let rolling = (speed / 40.0).min(1.3);
            if rolling > 1e-3 && hard > 1e-3 {
                rest += self.road.read(&s.road.clip, r(&s.road.clip) * (0.75 + 0.5 * (speed / 70.0).min(1.0)) as f64) * rolling.powf(0.8) * hard * s.road.gain * (1.0 + 0.15 * inside);
            }
            if kerb > 1e-3 && speed > 0.5 {
                // The ridges every 0.7 m: their beat is the speed over 0.7.
                let beat = (speed / 0.7).max(10.0) / s.kerb_beat;
                rest += self.kerb.read(&s.kerb.clip, r(&s.kerb.clip) * beat as f64) * kerb * (speed / 8.0).min(1.0) * s.kerb.gain;
            }
            let crunch = (speed / 6.0).min(1.0);
            if gravel > 1e-3 && crunch > 1e-3 {
                rest += self.gravel.read(&s.gravel.clip, r(&s.gravel.clip) * (0.7 + 0.6 * (speed / 30.0).min(1.0)) as f64) * gravel * crunch * s.gravel.gain;
            }
            if grass > 1e-3 && crunch > 1e-3 {
                let g = self.grass.read(&s.gravel.clip, r(&s.gravel.clip) * (0.45 + 0.35 * (speed / 30.0).min(1.0)) as f64);
                rest += self.grass_filter.run(g, pole(700.0, rate)) * grass * crunch * s.gravel.gain * 1.2;
            }
            let wind = (speed / 75.0).min(1.6);
            if wind > 1e-3 {
                rest += self.wind.read(&s.wind.clip, r(&s.wind.clip) * (0.85 + 0.3 * (speed / 80.0).min(1.0)) as f64) * wind * wind * s.wind.gain * (1.0 - 0.5 * inside);
            }
            if scrape > 1e-3 {
                rest += self.scrape.read(&s.scrape.clip, r(&s.scrape.clip) * (0.8 + 0.4 * scrape) as f64) * scrape * s.scrape.gain;
            }
            if let Some(shot) = &mut self.impact {
                let clip = &s.impact.clip;
                let at = shot.pos as usize;
                if at + 1 < clip.samples.len() {
                    let t = (shot.pos - at as f64) as f32;
                    rest += (clip.samples[at] * (1.0 - t) + clip.samples[at + 1] * t) * shot.gain;
                    shot.pos += (clip.rate / rate * shot.rate) as f64;
                } else {
                    self.impact = None;
                }
            }

            // Where it is heard from: through the cabin, muffled (the engine most); far off,
            // fainter and duller.
            let air = 18000.0 / (1.0 + distance / 60.0);
            let engine_cut = air.min(20000.0 - 18300.0 * inside);
            let rest_cut = air.min(20000.0 - 16500.0 * inside);
            let engine = self.engine_filter.run(engine, pole(engine_cut.min(rate * 0.45), rate)) * (1.0 - 0.15 * inside);
            let rest = self.rest_filter.run(rest, pole(rest_cut.min(rate * 0.45), rate));
            // Fainter with the distance, but less than in the open air (a camera is a microphone near it).
            let near = (10.0 / distance.max(10.0)).powf(0.75);
            let v = soft_clip((engine + rest) * near * volume * 0.9);
            if channels >= 2 {
                let a = (pan + 1.0) * PI / 4.0;
                let (left, right) = (a.cos() * std::f32::consts::SQRT_2, a.sin() * std::f32::consts::SQRT_2);
                frame[0] = soft_clip(v * left.min(1.0));
                frame[1] = soft_clip(v * right.min(1.0));
                for c in &mut frame[2..] {
                    *c = v;
                }
            } else {
                frame[0] = v;
            }
        }
    }
}

fn smoothstep64(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Straight up to 0.8, then bent smoothly towards 1: never past full scale, no hard clipping.
fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.8 {
        x
    } else {
        x.signum() * (0.8 + 0.2 * ((a - 0.8) / 0.2).tanh())
    }
}

struct Shared([AtomicU32; SLOTS]);

impl Shared {
    fn new() -> Shared {
        Shared(std::array::from_fn(|_| AtomicU32::new(0)))
    }

    fn store(&self, p: &Params) {
        for (slot, v) in self.0.iter().zip(p.to_slots()) {
            slot.store(v.to_bits(), Ordering::Relaxed);
        }
    }

    fn load(&self) -> Params {
        Params::from_slots(std::array::from_fn(|i| f32::from_bits(self.0[i].load(Ordering::Relaxed))))
    }
}

/// The game's sound. Without an output device, or without the car's sounds, it does nothing.
pub struct Audio {
    shared: Arc<Shared>,
    _output: Option<AudioOutput>,
}

impl Audio {
    /// Starts the sound of `car` (a car's name, as `--car` takes it), its starter first.
    pub fn start(car: &str) -> Audio {
        let shared = Arc::new(Shared::new());
        let sounds = sim_core::CarKind::parse(car).ok_or(format!("no car \"{car}\"")).and_then(|kind| Sounds::of_model(&kind.spec().visual.model));
        let sounds = match sounds {
            Ok(s) => Arc::new(s),
            Err(e) => {
                eprintln!("the car's sound: {e}");
                return Audio { shared, _output: None };
            }
        };
        let reader = shared.clone();
        let mut mixer = Mixer::new(sounds, true);
        let output = AudioOutput::start(move |data, channels, rate| {
            let p = reader.load();
            mixer.render(&p, data, channels, rate);
        });
        Audio { shared, _output: output }
    }

    /// No sound (a screenshot).
    pub fn silent() -> Audio {
        Audio { shared: Arc::new(Shared::new()), _output: None }
    }

    pub fn update(&self, p: &Params) {
        self.shared.store(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gt() -> Sounds {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../assets/cars/gt/sound");
        Sounds::load(&dir).unwrap()
    }

    #[test]
    fn the_gt_sounds_load_and_cover_the_revs() {
        let s = gt();
        let car = sim_core::CarKind::parse("gt").unwrap().spec();
        for b in &s.banks {
            // From about idle to near the limiter, the loops no further apart than a ratio of 1.6
            // (each is pitched at most some 26 % from where it was recorded).
            assert!(b[0].rpm < car.engine.idle_rpm as f32 * 1.6, "{}", b[0].rpm);
            assert!(b[b.len() - 1].rpm > car.engine.limiter_rpm as f32 * 0.8);
            for w in b.windows(2) {
                assert!(w[1].rpm / w[0].rpm < 2.5, "{} to {}", w[0].rpm, w[1].rpm);
            }
            for l in b {
                assert_eq!(l.clip.rate, 48000.0);
                // A loop is whole engine cycles: about a whole number of them in its length.
                let cycles = l.clip.samples.len() as f32 / l.clip.rate * l.rpm / 120.0;
                assert!((cycles - cycles.round()).abs() < 0.02, "{} rpm: {cycles} cycles", l.rpm);
            }
        }
    }

    /// The Model Y's motors run up from rest at full torque and back regenerating: the sound
    /// stays within full scale and moves smoothly, louder at speed than at rest.
    #[test]
    fn the_electric_motors_whine_smoothly() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../assets/cars/modely/sound");
        let sounds = Arc::new(Sounds::load(&dir).unwrap());
        assert!(sounds.motor.is_some() && sounds.starter.is_none());
        let mut mixer = Mixer::new(sounds, true);
        let rate = 48000.0;
        let mut out = vec![0.0f32; 800];
        let (mut worst, mut slow, mut fast) = (0.0f32, 0.0f32, 0.0f32);
        let mut prev = 0.0f32;
        for frame in 0..600 {
            let t = frame as f32 / 60.0;
            let (rpm, load, speed) = if t < 6.0 { (t / 6.0 * 12000.0, 1.0, t / 6.0 * 47.0) } else { (12000.0 - (t - 6.0) / 4.0 * 12000.0, -0.4, 47.0 - (t - 6.0) / 4.0 * 47.0) };
            let p = Params { rpm, load, speed, volume: 1.0, listener: Listener { distance: 6.0, ..Listener::default() }, ..Params::default() };
            mixer.render(&p, &mut out, 1, rate);
            let rms = (out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32).sqrt();
            if (0.1..0.3).contains(&t) {
                slow = slow.max(rms);
            }
            if (5.5..6.0).contains(&t) {
                fast = fast.max(rms);
            }
            for &x in &out {
                assert!(x.is_finite() && x.abs() <= 1.0);
                worst = worst.max((x - prev).abs());
                prev = x;
            }
        }
        assert!(worst < 0.2, "a jump of {worst}");
        assert!(fast > slow, "fast {fast} slow {slow}");
    }

    #[test]
    fn the_crossfade_between_loops_is_continuous() {
        let layers: Vec<Layer> = [1000.0, 2000.0, 4000.0].iter().map(|&rpm| Layer { clip: Clip { samples: vec![0.0; 64], rate: 48000.0 }, rpm }).collect();
        let weight = |rpm: f32| {
            let mut w = [0.0f32; 3];
            for (i, v) in pair(&layers, rpm) {
                w[i] += v;
            }
            w
        };
        let mut last = weight(500.0);
        let mut rpm = 500.0;
        while rpm < 6000.0 {
            rpm += 5.0;
            let w = weight(rpm);
            for i in 0..3 {
                assert!((w[i] - last[i]).abs() < 0.02, "{rpm}: {w:?} after {last:?}");
            }
            // Equal power.
            assert!((w.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-4);
            last = w;
        }
    }

    /// The engine run up through the revs on load and down off load: the sound stays within full
    /// scale, never jumps from one sample to the next more than the loops themselves do, and is
    /// louder on load at high revs than off load at idle.
    #[test]
    fn a_run_up_and_down_the_revs_is_smooth() {
        let sounds = Arc::new(gt());
        let biggest_step = sounds.banks.iter().flatten().flat_map(|l| l.clip.samples.windows(2).map(|w| (w[1] - w[0]).abs())).fold(0.0f32, f32::max);
        let mut mixer = Mixer::new(sounds, false);
        let rate = 48000.0;
        let mut out = vec![0.0f32; 800];
        let (mut worst, mut idle, mut high) = (0.0f32, 0.0f32, 0.0f32);
        let mut prev = 0.0f32;
        for frame in 0..600 {
            let t = frame as f32 / 60.0;
            let (rpm, load) = if t < 1.0 { (850.0, 0.0) } else if t < 6.0 { (850.0 + (t - 1.0) / 5.0 * 7000.0, 1.0) } else { (7850.0 - (t - 6.0) / 4.0 * 7000.0, -0.3) };
            let p = Params { rpm, load, speed: 0.0, volume: 1.0, listener: Listener { bay: 0.2, distance: 6.0, ..Listener::default() }, ..Params::default() };
            mixer.render(&p, &mut out, 1, rate);
            let rms = (out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32).sqrt();
            if (0.5..1.0).contains(&t) {
                idle = idle.max(rms);
            }
            if (5.5..6.0).contains(&t) {
                high = high.max(rms);
            }
            for &x in &out {
                assert!(x.is_finite() && x.abs() <= 1.0);
                worst = worst.max((x - prev).abs());
                prev = x;
            }
        }
        assert!(worst < biggest_step * 1.2, "a jump of {worst} (the loops' biggest {biggest_step})");
        assert!(high > idle * 1.5, "high {high} idle {idle}");
    }
}
