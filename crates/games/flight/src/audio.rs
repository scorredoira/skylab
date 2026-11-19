//! Engine (four cylinders or six: its note and its exhaust's bark), wind, wheels, stall horn and
//! the autopilot's disconnect tone, and a multirotor's propellers and motors, synthesised sample
//! by sample.
//!
//! The game writes a few numbers once per frame ([`Audio::update`]); the audio thread reads them
//! through atomics and never waits for a lock.
//!
//! A recording has no audio thread: its sound is made frame by frame with the pictures ([`Mixer`]),
//! every craft a voice of its own heard from the camera, and written as a WAV file.

use engine::audio::AudioOutput;
use sim_core::{act, Dynamics, Vehicle};
use std::f32::consts::PI;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// Overall output level.
const MASTER: f32 = 0.8;
/// The two notes of the autopilot's disconnect tone, Hz.
const TONE_HIGH: f32 = 990.0;
const TONE_LOW: f32 = 740.0;

/// What the sound depends on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub rpm: f32,
    /// 0..1.
    pub throttle: f32,
    /// True airspeed, m/s.
    pub airspeed: f32,
    /// Speed over the ground, m/s.
    pub ground_speed: f32,
    /// 1 on the ground, 0 in the air.
    pub on_ground: f32,
    /// 1 while the stall horn sounds.
    pub stall: f32,
    /// Strength of the last touchdown; a new value plays the thump.
    pub touchdown: f32,
    /// 0 silences everything (paused, crashed).
    pub volume: f32,
    /// Cylinders of the engine: 4 in the C172, 6 (and 300 hp, louder) in the Extra.
    pub cylinders: f32,
    /// 1 while the autopilot's disconnect tone sounds.
    pub tone: f32,
    /// A multirotor's rotors' speeds, rad/s (no engine then: `cylinders` is 0).
    pub rotors: [f32; ROTORS],
    /// A race's chime: a new number plays it (`Audio::chime`), at `chime_note` Hz (negative: the
    /// missed gate's buzz at that pitch).
    pub chime: f32,
    pub chime_note: f32,
    /// The fan speed of a jet aircraft's engines (their mean N1; no jets: 0, and `cylinders` is
    /// then the piston engine's).
    pub fan: f32,
}

/// The rotors heard.
pub const ROTORS: usize = 4;

impl Default for Params {
    fn default() -> Params {
        Params { rpm: 600.0, throttle: 0.0, airspeed: 0.0, ground_speed: 0.0, on_ground: 0.0, stall: 0.0, touchdown: 0.0, volume: 0.0, cylinders: 4.0, tone: 0.0, rotors: [0.0; ROTORS], chime: 0.0, chime_note: 0.0, fan: 0.0 }
    }
}

impl Params {
    /// The sound of an aircraft as it is now. `touchdown` keeps its last value until the next landing;
    /// `tone`: the autopilot's disconnect tone sounds.
    pub fn of(plane: &Vehicle, paused: bool, previous_touchdown: f32, tone: bool) -> Params {
        let a = match &plane.dynamics {
            Dynamics::Aircraft(a) => a,
            // The flight game has no cars (they are `drive`'s): silence.
            Dynamics::Car(_) => return Params { volume: 0.0, ..Params::default() },
            Dynamics::Multirotor(m) => {
                let silent = paused || m.sunk();
                let v = m.body.world_velocity();
                let mut rotors = [0.0; ROTORS];
                for (r, w) in rotors.iter_mut().zip(m.rotor_speeds()) {
                    *r = *w as f32;
                }
                return Params {
                    rpm: 0.0,
                    throttle: m.controls.throttle as f32,
                    airspeed: m.data().airspeed as f32,
                    ground_speed: v.x.hypot(v.y) as f32,
                    on_ground: if m.data().on_ground { 1.0 } else { 0.0 },
                    stall: 0.0,
                    touchdown: previous_touchdown,
                    volume: if silent { 0.0 } else { 1.0 },
                    cylinders: 0.0,
                    tone: 0.0,
                    rotors,
                    chime: 0.0,
                    chime_note: 0.0,
                    fan: 0.0,
                };
            }
        };
        let d = a.data();
        let world = act(a.state.q, a.state.vel);
        let silent = paused || a.crashed().is_some();
        Params {
            rpm: a.rpm() as f32,
            throttle: a.controls.throttle as f32,
            airspeed: d.tas as f32,
            ground_speed: world.x.hypot(world.y) as f32,
            on_ground: if d.on_ground() { 1.0 } else { 0.0 },
            stall: if d.stall_warning && !silent { 1.0 } else { 0.0 },
            touchdown: a.last_touchdown().map_or(previous_touchdown, |t| (0.25 + t / 3.0) as f32),
            volume: if silent { 0.0 } else { 1.0 },
            cylinders: crate::kinds::cylinders(a.p.kind),
            tone: if tone && !silent { 1.0 } else { 0.0 },
            rotors: [0.0; ROTORS],
            chime: 0.0,
            chime_note: 0.0,
            fan: a.p.jets.as_ref().map_or(0.0, |jets| (a.state.n1.iter().sum::<f64>() / jets.mounts.len() as f64) as f32),
        }
    }
}

/// `Params` as the audio thread sees them: each `f32` as its bits in an atomic.
struct Shared([AtomicU32; 13 + ROTORS]);

impl Shared {
    fn new(p: &Params) -> Shared {
        let shared = Shared(std::array::from_fn(|_| AtomicU32::new(0)));
        shared.store(p);
        shared
    }

    fn store(&self, p: &Params) {
        let values = [p.rpm, p.throttle, p.airspeed, p.ground_speed, p.on_ground, p.stall, p.touchdown, p.volume, p.cylinders, p.tone, p.chime, p.chime_note, p.fan];
        for (slot, v) in self.0.iter().zip(values.into_iter().chain(p.rotors)) {
            slot.store(v.to_bits(), Ordering::Relaxed);
        }
    }

    fn load(&self) -> Params {
        let v = |i: usize| f32::from_bits(self.0[i].load(Ordering::Relaxed));
        Params { rpm: v(0), throttle: v(1), airspeed: v(2), ground_speed: v(3), on_ground: v(4), stall: v(5), touchdown: v(6), volume: v(7), cylinders: v(8), tone: v(9), chime: v(10), chime_note: v(11), fan: v(12), rotors: std::array::from_fn(|i| v(13 + i)) }
    }
}

/// The synthesiser: everything the audio thread remembers between samples.
pub struct Synth {
    phase: f32,
    horn_phase: f32,
    /// The disconnect tone: its phase, how long it has sounded, its level.
    tone_phase: f32,
    tone_time: f32,
    tone: f32,
    seed: u32,
    wind: f32,
    wind2: f32,
    rumble: f32,
    /// The noise the exhaust's bark is made of, its top taken off.
    exhaust: f32,
    rpm: f32,
    load: f32,
    speed: f32,
    ground: f32,
    horn: f32,
    bump: f32,
    volume: f32,
    last_touch: f32,
    /// Each rotor: the phase of its blades' passing and of its motor's windings, its speed as heard.
    rotor_phase: [f32; ROTORS],
    winding_phase: [f32; ROTORS],
    rotor_speed: [f32; ROTORS],
    buzz: f32,
    /// A race's chime: the last one started, its note, phase, time and level.
    last_chime: f32,
    chime_note: f32,
    chime_phase: f32,
    chime_time: f32,
    /// What the rotors' notes are multiplied by (a recording's Doppler and slow motion); 1 in the game.
    pitch: f32,
    /// A jet's engines: their fan speed as heard, the noise their roar is made of, the fan's phase.
    fan: f32,
    jet: f32,
    jet2: f32,
    fan_phase: f32,
}

impl Default for Synth {
    fn default() -> Synth {
        Synth::new()
    }
}

impl Synth {
    pub fn new() -> Synth {
        Synth {
            phase: 0.0,
            horn_phase: 0.0,
            tone_phase: 0.0,
            tone_time: 0.0,
            tone: 0.0,
            seed: 12345,
            wind: 0.0,
            wind2: 0.0,
            rumble: 0.0,
            exhaust: 0.0,
            rpm: 600.0,
            load: 0.0,
            speed: 0.0,
            ground: 0.0,
            horn: 0.0,
            bump: 0.0,
            volume: 0.0,
            last_touch: 0.0,
            rotor_phase: [0.0, 0.25, 0.5, 0.75],
            winding_phase: [0.0; ROTORS],
            rotor_speed: [0.0; ROTORS],
            buzz: 0.0,
            last_chime: 0.0,
            chime_note: 0.0,
            chime_phase: 0.0,
            chime_time: 10.0,
            pitch: 1.0,
            fan: 0.0,
            jet: 0.0,
            jet2: 0.0,
            fan_phase: 0.0,
        }
    }

    /// Call once per block before its samples: starts the thump of a new touchdown.
    pub fn begin_block(&mut self, p: &Params) {
        if p.touchdown != self.last_touch {
            self.last_touch = p.touchdown;
            self.bump = p.touchdown.min(1.0);
        }
        if p.chime != self.last_chime {
            self.last_chime = p.chime;
            self.chime_note = p.chime_note;
            self.chime_time = 0.0;
        }
    }

    /// The next sample, before the master level. `rate` is the output's sample rate in Hz.
    pub fn sample(&mut self, p: &Params, rate: f32) -> f32 {
        // Smooth the controls so nothing clicks.
        self.rpm += (p.rpm - self.rpm) * 0.0006;
        self.load += (p.throttle - self.load) * 0.0006;
        self.speed += (p.airspeed - self.speed) * 0.0004;
        self.ground += (p.ground_speed * p.on_ground - self.ground) * 0.001;
        self.horn += (p.stall - self.horn) * 0.002;
        self.volume += (p.volume - self.volume) * 0.0005;

        self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let white = self.seed as i32 as f32 / 2147483648.0;

        // Four cylinders fire twice per revolution, six three times; the propeller beats along.
        let f0 = self.rpm / 60.0 * p.cylinders / 2.0;
        self.phase += f0 / rate;
        if self.phase > 1.0 {
            self.phase -= 1.0;
        }
        let w = 2.0 * PI * self.phase;
        let mut e = 0.0;
        e += w.sin() * 1.0 + (2.0 * w + 0.5).sin() * 0.55 + (3.0 * w + 1.1).sin() * 0.35 + (4.0 * w + 0.3).sin() * 0.22;
        e += (5.0 * w + 2.0).sin() * 0.14 + (6.0 * w + 0.7).sin() * 0.10 + (8.0 * w + 1.9).sin() * 0.06;
        e += (0.5 * w).sin() * 0.45 + (1.5 * w + 0.4).sin() * 0.25; // uneven firing: the lumpy idle
        let engine_level = (0.10 + 0.22 * self.load) * (0.6 + 0.4 * (self.rpm / 2500.0).min(1.2)) * p.cylinders.min(1.0);
        // The big six: louder, harder driven, with a growl at the crankshaft's own rate.
        let six = ((p.cylinders - 4.0) / 2.0).clamp(0.0, 1.0);
        e += six * ((w / 3.0).sin() * 0.5 + (2.0 * w / 3.0 + 0.8).sin() * 0.3);
        let mut out = (e * (0.7 + 0.3 * six)).tanh() * engine_level * (1.0 + 0.3 * six);
        // The exhaust's bark and the blades' tearing of the air: a burst of noise with each
        // firing, more of it the harder the engine is driven. Without it the engine is a hum.
        self.exhaust += (white - self.exhaust) * 0.3;
        let burst = (0.5 + 0.5 * w.sin()).powi(3);
        out += self.exhaust * burst * (0.3 + 0.9 * self.load) * engine_level * (1.0 + 0.3 * six);

        out += self.rotors(p, rate, white);
        out += self.jets(p, rate, white);

        // Air over the airframe.
        let cut = (0.02 + self.speed * 0.0025).min(0.5);
        self.wind += (white - self.wind) * cut;
        self.wind2 += (self.wind - self.wind2) * cut;
        let q = (self.speed / 60.0).min(2.0);
        out += self.wind2 * q * q * 0.55;

        // Wheels on the ground.
        self.rumble += (white - self.rumble) * 0.012;
        out += self.rumble * (self.ground / 25.0).min(1.5) * 0.9;
        if self.bump > 0.0005 {
            out += white * self.bump * 0.5;
            self.bump *= 0.9993;
        }

        // Stall horn: a reed, so a buzzy tone.
        self.horn_phase += 2900.0 / rate * (0.85 + 0.15 * self.horn);
        if self.horn_phase > 1.0 {
            self.horn_phase -= 1.0;
        }
        out += (if self.horn_phase < 0.5 { 1.0 } else { -1.0 }) * self.horn * 0.05 + (2.0 * PI * self.horn_phase).sin() * self.horn * 0.06;

        // The autopilot's disconnect tone: two notes, high then low, a quarter of a second each.
        self.tone += (p.tone - self.tone) * 0.004;
        self.tone_time = if p.tone > 0.0 { self.tone_time + 1.0 / rate } else { 0.0 };
        if self.tone > 0.001 {
            let pitch = if self.tone_time % 0.5 < 0.25 { TONE_HIGH } else { TONE_LOW };
            self.tone_phase = (self.tone_phase + pitch / rate).fract();
            let w = 2.0 * PI * self.tone_phase;
            out += (w.sin() + (2.0 * w).sin() * 0.2) * self.tone * 0.09;
        }

        out += self.chime(rate);
        out * self.volume
    }

    /// A race's chime: a bell-like ding, its partials fading over a third of a second (the start's
    /// GO and a lap twice as long); the missed gate's buzz, a square note for a quarter of a second.
    fn chime(&mut self, rate: f32) -> f32 {
        if self.chime_time > 1.2 || self.chime_note == 0.0 {
            return 0.0;
        }
        self.chime_time += 1.0 / rate;
        let note = self.chime_note.abs();
        self.chime_phase = (self.chime_phase + note / rate).fract();
        let w = 2.0 * PI * self.chime_phase;
        let attack = (self.chime_time / 0.004).min(1.0);
        if self.chime_note < 0.0 {
            let level = if self.chime_time < 0.25 { 1.0 } else { (1.0 - (self.chime_time - 0.25) / 0.03).max(0.0) };
            return (if self.chime_phase < 0.5 { 1.0 } else { -1.0 }) * 0.05 * level * attack;
        }
        let fade = if note > 1300.0 { 0.55 } else { 0.3 };
        let decay = (-self.chime_time / fade).exp();
        (w.sin() + 0.4 * (2.76 * w).sin() + 0.2 * (5.4 * w).sin()) * 0.07 * decay * attack
    }
}

impl Synth {
    /// A jet's engines: the roar of the jets, deeper at idle and far louder with the thrust (which
    /// goes with the cube of the fan's speed), and over it the fan's whine, rising with it.
    fn jets(&mut self, p: &Params, rate: f32, white: f32) -> f32 {
        self.fan += (p.fan - self.fan) * 0.0006;
        let n = self.fan;
        if n < 0.01 {
            return 0.0;
        }
        let cut = 0.012 + 0.05 * n;
        self.jet += (white - self.jet) * cut;
        self.jet2 += (self.jet - self.jet2) * cut;
        let roar = self.jet2 * (0.5 + 2.4 * n * n * n);
        self.fan_phase = (self.fan_phase + 1250.0 * n * self.pitch / rate).fract();
        let w = 2.0 * PI * self.fan_phase;
        let whine = (w.sin() + 0.45 * (2.0 * w + 0.6).sin() + 0.25 * (3.02 * w).sin()) * 0.014 * (0.5 + n);
        roar + whine
    }

    /// A multirotor's rotors: each two-blade propeller a buzz at twice its turns a second (rich in
    /// harmonics, as blades cutting the air are), its motor's whine at seven times them (its 14
    /// poles), louder the faster it turns; the four a little apart, so they beat as real ones do.
    fn rotors(&mut self, p: &Params, rate: f32, white: f32) -> f32 {
        let mut out = 0.0;
        self.buzz += (white - self.buzz) * 0.25;
        for i in 0..ROTORS {
            self.rotor_speed[i] += (p.rotors[i] - self.rotor_speed[i]) * 0.002;
            let turns = self.rotor_speed[i] / (2.0 * PI);
            if turns < 1.0 {
                continue;
            }
            self.rotor_phase[i] = (self.rotor_phase[i] + 2.0 * turns * self.pitch / rate).fract();
            self.winding_phase[i] = (self.winding_phase[i] + 7.0 * turns * self.pitch / rate).fract();
            let w = 2.0 * PI * self.rotor_phase[i];
            // A saw-like wave with its edge softened, and air that rushes with each blade.
            let blade = w.sin() + 0.5 * (2.0 * w).sin() + 0.33 * (3.0 * w + 0.4).sin() + 0.22 * (4.0 * w + 0.9).sin() + 0.12 * (6.0 * w).sin();
            let rush = self.buzz * (0.5 + 0.5 * w.cos());
            let whine = (2.0 * PI * self.winding_phase[i]).sin();
            let level = (turns / 450.0).min(1.3).powi(2);
            out += (0.06 * blade + 0.09 * rush + 0.015 * whine) * level;
        }
        out
    }
}

/// The game's sound. Without an output device it does nothing.
pub struct Audio {
    shared: Arc<Shared>,
    touchdown: f32,
    /// The chimes asked for so far, and the last one's note.
    chime: u32,
    chime_note: f32,
    _output: Option<AudioOutput>,
}

impl Audio {
    /// Starts playing on the default output device, if there is one.
    pub fn start() -> Audio {
        let shared = Arc::new(Shared::new(&Params::default()));
        let state = shared.clone();
        let mut synth = Synth::new();
        let output = AudioOutput::start(move |samples, channels, rate| {
            let p = state.load();
            synth.begin_block(&p);
            for frame in samples.chunks_mut(channels.max(1)) {
                frame.fill(synth.sample(&p, rate) * MASTER);
            }
        });
        if output.is_none() {
            eprintln!("audio: no sound output; going on without it");
        }
        Audio { shared, touchdown: 0.0, chime: 0, chime_note: 0.0, _output: output }
    }

    /// No sound at all: for screenshots and tests.
    pub fn silent() -> Audio {
        Audio { shared: Arc::new(Shared::new(&Params::default())), touchdown: 0.0, chime: 0, chime_note: 0.0, _output: None }
    }

    /// Once per frame: what the aircraft sounds like now, and whether the disconnect tone sounds.
    pub fn update(&mut self, plane: &Vehicle, paused: bool, tone: bool) {
        let p = Params { chime: self.chime as f32, chime_note: self.chime_note, ..Params::of(plane, paused, self.touchdown, tone) };
        self.touchdown = p.touchdown;
        self.shared.store(&p);
    }

    /// What the last `update` gave the audio thread: a recording makes its sound of it (`Mixer`).
    pub fn params(&self) -> Params {
        self.shared.load()
    }

    /// Plays a race's chime at `note` Hz (negative: the missed gate's buzz) from the next update.
    pub fn chime(&mut self, note: f32) {
        self.chime += 1;
        self.chime_note = note;
    }
}

/// A craft as a recording's camera hears it.
#[derive(Clone, Copy, Debug)]
pub struct Heard {
    /// What it sounds like where it is (`Params::of`).
    pub params: Params,
    /// How far from the camera, metres.
    pub distance: f32,
    /// Where across the picture: -1 left, 1 right.
    pub pan: f32,
    /// What its notes are multiplied by: the Doppler shift of its speed towards the camera or away.
    pub pitch: f32,
}

/// The distance at which a craft is heard at its full level, metres; beyond, the level falls as
/// one over the distance.
const NEAR: f32 = 1.5;
/// How much a slow motion lowers the notes: the rate to this power (0.25 of the speed, an octave
/// down). A film slowed four times falls two octaves, which leaves a quad's buzz a rumble.
const SLOW_PITCH: f32 = 0.5;
/// Seconds of sound made and thrown away before the first frame, for the smoothed levels and
/// speeds to be where they are and not on their way up from nothing.
const LEAD_IN: f32 = 0.4;
/// A recording's level over the game's: a race quad cruises at a third of its rotors' top speed,
/// where the synthesiser is quiet, and a film has no volume knob. The limiter keeps the peaks.
const FILM_GAIN: f32 = 2.5;
/// The air the camera hears standing still, m/s: the wood is never silent.
const STILL_AIR: f32 = 7.0;

/// One craft's voice in the mix: its own synthesiser, and where it is heard, moved there smoothly.
struct Voice {
    synth: Synth,
    left: f32,
    right: f32,
    /// The distance's dulling: a low-pass's coefficient and what it holds.
    cut: f32,
    held: f32,
}

/// A recording's sound: the craft as the camera hears them (each nearer or farther, to one side,
/// its pitch bent by its speed), the air past the camera and the race's chimes, in stereo.
pub struct Mixer {
    rate: f32,
    voices: Vec<Voice>,
    /// The camera's own: the wind and the chimes.
    own: Synth,
    /// The world's rate as the notes follow it (`SLOW_PITCH`).
    slow: f32,
    started: bool,
    /// Interleaved, left and right.
    pub samples: Vec<f32>,
}

impl Mixer {
    pub fn new(rate: u32) -> Mixer {
        Mixer { rate: rate as f32, voices: Vec::new(), own: Synth::new(), slow: 1.0, started: false, samples: Vec::new() }
    }

    /// The next `count` samples: the craft `heard` (always the same ones, in the same order), the
    /// camera moving at `speed` m/s through the air, `chimes` as `Audio::params` gives them (the
    /// player's), the world running at `rate` times its speed (1: as it is; less: slow motion).
    pub fn frame(&mut self, heard: &[Heard], speed: f32, chimes: &Params, rate: f32, count: usize) {
        while self.voices.len() < heard.len() {
            let n = self.voices.len() as u32;
            let mut synth = Synth::new();
            // Each craft's noise and blades its own: two of them never sound as one twice as loud.
            synth.seed = 12345 + 7919 * n;
            synth.rotor_phase = std::array::from_fn(|i| (0.25 * i as f32 + 0.37 * n as f32).fract());
            self.voices.push(Voice { synth, left: 0.0, right: 0.0, cut: 1.0, held: 0.0 });
        }
        let slow = rate.clamp(0.05, 4.0).powf(SLOW_PITCH);
        let own = Params { volume: 1.0, cylinders: 0.0, rpm: 0.0, airspeed: (speed * slow).max(STILL_AIR), rotors: [0.0; ROTORS], chime: chimes.chime, chime_note: chimes.chime_note, ..Params::default() };
        let targets: Vec<(Params, f32, f32, f32, f32)> = heard
            .iter()
            .map(|h| {
                let level = (NEAR / h.distance.max(NEAR)) * h.params.volume;
                // Close to, it is all round the camera; a few metres off, to its side.
                let pan = h.pan.clamp(-1.0, 1.0) * 0.85 * (h.distance / 3.0).min(1.0);
                let angle = (pan + 1.0) * PI / 4.0;
                // The air and the trees take the top off what is far.
                let cutoff = (16_000.0 / (1.0 + h.distance / 40.0)).max(1500.0);
                let cut = 1.0 - (-2.0 * PI * cutoff / self.rate).exp();
                // Only its rotors: the wind is the camera's.
                let params = Params { airspeed: 0.0, ground_speed: 0.0, chime: 0.0, chime_note: 0.0, touchdown: 0.0, volume: 1.0, ..h.params };
                (params, level * angle.cos() * std::f32::consts::SQRT_2, level * angle.sin() * std::f32::consts::SQRT_2, cut, h.pitch.clamp(0.5, 2.0))
            })
            .collect();
        let lead = if self.started { 0 } else { (LEAD_IN * self.rate) as usize };
        if !self.started {
            self.slow = slow;
            for (voice, t) in self.voices.iter_mut().zip(&targets) {
                (voice.left, voice.right, voice.cut) = (t.1, t.2, t.3);
                voice.synth.pitch = t.4 * slow;
            }
            // The chimes rung before the first frame are not this shot's.
            self.own.last_chime = own.chime;
            self.started = true;
        }
        self.own.begin_block(&own);
        // Two thirds of the way to a new level or pitch in 15 ms: no steps from frame to frame.
        let k = 1.0 - (-1.0 / (0.015 * self.rate)).exp();
        for n in 0..lead + count {
            self.slow += (slow - self.slow) * k;
            let (mut left, mut right) = (0.0, 0.0);
            for (voice, (params, l, r, cut, pitch)) in self.voices.iter_mut().zip(&targets) {
                voice.left += (l - voice.left) * k;
                voice.right += (r - voice.right) * k;
                voice.cut += (cut - voice.cut) * k;
                voice.synth.pitch += (pitch * self.slow - voice.synth.pitch) * k;
                voice.held += (voice.synth.sample(params, self.rate) - voice.held) * voice.cut;
                left += voice.held * voice.left;
                right += voice.held * voice.right;
            }
            let own_sample = self.own.sample(&own, self.rate);
            if n >= lead {
                self.samples.push(limit((left + own_sample) * MASTER * FILM_GAIN));
                self.samples.push(limit((right + own_sample) * MASTER * FILM_GAIN));
            }
        }
    }

    /// What was made so far as a WAV file (16 bits, stereo), its first and last few milliseconds
    /// faded so that it cuts to and from another without a click.
    pub fn wav(&self) -> Vec<u8> {
        let mut samples = self.samples.clone();
        let frames = samples.len() / 2;
        let fade = ((0.004 * self.rate) as usize).min(frames / 2);
        for i in 0..fade {
            let level = i as f32 / fade as f32;
            for at in [i, frames - 1 - i] {
                samples[2 * at] *= level;
                samples[2 * at + 1] *= level;
            }
        }
        engine::audio::wav_bytes(&samples, 2, self.rate as u32)
    }
}

/// A level kept under 1: as it is up to 0.7, then bent over smoothly.
fn limit(x: f32) -> f32 {
    const KNEE: f32 = 0.7;
    if x.abs() <= KNEE {
        x
    } else {
        x.signum() * (KNEE + (1.0 - KNEE) * ((x.abs() - KNEE) / (1.0 - KNEE)).tanh())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(synth: &mut Synth, p: &Params, samples: usize) -> (f32, f32) {
        synth.begin_block(p);
        let (mut peak, mut energy) = (0.0f32, 0.0f32);
        for _ in 0..samples {
            let s = synth.sample(p, 44_100.0);
            assert!(s.is_finite());
            peak = peak.max(s.abs());
            energy += s * s;
        }
        (peak, (energy / samples as f32).sqrt())
    }

    #[test]
    fn silent_until_the_volume_is_raised() {
        let mut synth = Synth::new();
        let (peak, _) = run(&mut synth, &Params::default(), 4410);
        assert_eq!(peak, 0.0);
    }

    #[test]
    fn the_engine_is_louder_at_full_throttle_and_never_clips() {
        let idle = Params { rpm: 650.0, volume: 1.0, on_ground: 1.0, ..Params::default() };
        let full = Params { rpm: 2600.0, throttle: 1.0, airspeed: 60.0, volume: 1.0, ..Params::default() };
        let mut synth = Synth::new();
        run(&mut synth, &idle, 44_100);
        let (idle_peak, idle_rms) = run(&mut synth, &idle, 44_100);
        run(&mut synth, &full, 44_100);
        let (full_peak, full_rms) = run(&mut synth, &full, 44_100);
        assert!(idle_rms > 0.01, "idle rms {idle_rms}");
        assert!(full_rms > idle_rms * 1.5, "idle {idle_rms} full {full_rms}");
        assert!(idle_peak * MASTER < 1.0 && full_peak * MASTER < 1.0, "idle {idle_peak} full {full_peak}");
    }

    #[test]
    fn the_six_of_the_extra_is_louder_than_the_four() {
        let four = Params { rpm: 2700.0, throttle: 1.0, airspeed: 50.0, volume: 1.0, ..Params::default() };
        let six = Params { cylinders: 6.0, ..four };
        let (mut a, mut b) = (Synth::new(), Synth::new());
        run(&mut a, &four, 44_100);
        run(&mut b, &six, 44_100);
        let (_, rms4) = run(&mut a, &four, 44_100);
        let (peak6, rms6) = run(&mut b, &six, 44_100);
        assert!(rms6 > rms4 * 1.1, "four {rms4} six {rms6}");
        assert!(peak6 * MASTER < 1.0, "six peaks at {peak6}");
    }

    #[test]
    fn the_stall_horn_and_the_touchdown_are_heard() {
        let quiet = Params { rpm: 0.0, volume: 1.0, ..Params::default() };
        let mut synth = Synth::new();
        run(&mut synth, &quiet, 44_100);
        let (_, base) = run(&mut synth, &quiet, 22_050);

        let mut horn = Synth::new();
        run(&mut horn, &quiet, 44_100);
        let (_, with_horn) = run(&mut horn, &Params { stall: 1.0, ..quiet }, 22_050);
        assert!(with_horn > base + 0.02, "base {base} horn {with_horn}");

        let mut bump = Synth::new();
        run(&mut bump, &quiet, 44_100);
        let (_, with_bump) = run(&mut bump, &Params { touchdown: 0.9, ..quiet }, 2205);
        assert!(with_bump > base + 0.05, "base {base} bump {with_bump}");
        // The thump dies away by itself.
        let (_, later) = run(&mut bump, &Params { touchdown: 0.9, ..quiet }, 44_100);
        let (_, tail) = run(&mut bump, &Params { touchdown: 0.9, ..quiet }, 22_050);
        assert!(tail < with_bump && later < with_bump, "bump {with_bump} later {later} tail {tail}");
    }

    #[test]
    fn the_disconnect_tone_is_heard_and_stops() {
        let quiet = Params { rpm: 0.0, volume: 1.0, ..Params::default() };
        let mut synth = Synth::new();
        run(&mut synth, &quiet, 44_100);
        let (_, base) = run(&mut synth, &quiet, 22_050);
        let (peak, with_tone) = run(&mut synth, &Params { tone: 1.0, ..quiet }, 44_100);
        assert!(with_tone > base + 0.03 && peak < 1.0, "base {base} tone {with_tone}");
        run(&mut synth, &quiet, 44_100);
        let (_, after) = run(&mut synth, &quiet, 22_050);
        assert!(after < base + 0.005, "base {base} after {after}");
    }

    #[test]
    fn parameters_cross_to_the_audio_thread_unchanged() {
        let p = Params { rpm: 2345.5, throttle: 0.75, airspeed: 51.2, ground_speed: 3.25, on_ground: 1.0, stall: 1.0, touchdown: 0.583, volume: 1.0, cylinders: 6.0, tone: 1.0, rotors: [1.0, 2.0, 3.5, 1600.25], chime: 3.0, chime_note: -196.0, fan: 0.87 };
        let shared = Shared::new(&Params::default());
        assert_eq!(shared.load(), Params::default());
        shared.store(&p);
        assert_eq!(shared.load(), p);
    }

    /// A multirotor sounds of its rotors, higher and louder the faster they turn; its engine is silent.
    #[test]
    fn the_rotors_are_heard_rising_with_their_speed() {
        let mut world = sim_core::World::new(1);
        let id = world.add_multirotor(sim_core::MultirotorKind::FPV5, sim_core::Start::Air { n: 0.0, e: 0.0, heading: 0.0 });
        let p = Params::of(world.vehicle(id), false, 0.0, false);
        assert!(p.cylinders == 0.0 && p.rotors.iter().all(|&w| w > 1000.0), "{p:?}");
        let quiet = Params { volume: 1.0, cylinders: 0.0, rpm: 0.0, rotors: [300.0; ROTORS], ..Params::default() };
        let loud = Params { rotors: [2500.0; ROTORS], ..quiet };
        let (mut a, mut b) = (Synth::new(), Synth::new());
        run(&mut a, &quiet, 88_200);
        run(&mut b, &loud, 88_200);
        let (peak, low) = run(&mut a, &quiet, 22_050);
        let (top, high) = run(&mut b, &loud, 22_050);
        assert!(high > 2.0 * low && top < 1.0 && peak < 1.0, "{low} {high}");
    }

    /// A recording's mix: a quad close by is louder than one far off, one to the right is in the
    /// right channel, a Doppler shift and a slow motion move its note, and nothing clips.
    #[test]
    fn a_recording_hears_each_quad_where_it_is() {
        // One rotor's note: four turning exactly alike, a quarter of a turn apart, cancel theirs.
        let quad = Params { volume: 1.0, cylinders: 0.0, rpm: 0.0, rotors: [2200.0, 0.0, 0.0, 0.0], ..Params::default() };
        let mix = |heard: &[Heard], rate: f32| {
            let mut mixer = Mixer::new(48_000);
            for _ in 0..30 {
                mixer.frame(heard, 0.0, &Params::default(), rate, 800);
            }
            assert_eq!(mixer.samples.len(), 2 * 30 * 800);
            assert!(mixer.samples.iter().all(|s| s.is_finite() && s.abs() < 1.0));
            mixer
        };
        let rms = |m: &Mixer, channel: usize| (m.samples.iter().skip(channel).step_by(2).map(|s| s * s).sum::<f32>() / (m.samples.len() / 2) as f32).sqrt();
        // The period of the left channel's note, in samples: the shortest lag at which it is most like itself.
        let period = |m: &Mixer| {
            let left: Vec<f32> = m.samples.iter().step_by(2).copied().collect();
            let like = |lag: usize| left.iter().zip(&left[lag..]).map(|(a, b)| a * b).sum::<f32>();
            let lags: Vec<(usize, f32)> = (30..300).map(|lag| (lag, like(lag))).collect();
            let best = lags.iter().map(|l| l.1).fold(f32::MIN, f32::max);
            lags.iter().find(|l| l.1 >= 0.9 * best).unwrap().0 as f32
        };
        let at = |distance: f32, pan: f32, pitch: f32| Heard { params: quad, distance, pan, pitch };
        let (near, far) = (mix(&[at(2.0, 0.0, 1.0)], 1.0), mix(&[at(40.0, 0.0, 1.0)], 1.0));
        assert!(rms(&near, 0) > 4.0 * rms(&far, 0), "{} {}", rms(&near, 0), rms(&far, 0));
        let right = mix(&[at(6.0, 1.0, 1.0)], 1.0);
        assert!(rms(&right, 1) > 2.0 * rms(&right, 0), "{} {}", rms(&right, 0), rms(&right, 1));
        let (coming, going) = (mix(&[at(2.0, 0.0, 1.1)], 1.0), mix(&[at(2.0, 0.0, 0.9)], 1.0));
        // 2200 rad/s, two blades: 700 Hz, a period of 69 samples; a tenth higher and lower either way.
        assert!((period(&near) - 68.5).abs() < 2.0, "{}", period(&near));
        assert!(period(&coming) < 0.94 * period(&near) && period(&going) > 1.06 * period(&near), "{} {}", period(&coming), period(&going));
        // A quarter of the speed: an octave down.
        let slow = mix(&[at(2.0, 0.0, 1.0)], 0.25);
        assert!((period(&slow) / period(&near) - 2.0).abs() < 0.1, "{} {}", period(&slow), period(&near));
        // Three of them close by, flat out: still under 1 (asserted in `mix`).
        let flat_out = |distance: f32, pan: f32| Heard { params: Params { rotors: [2900.0, 2950.0, 3000.0, 3050.0], ..quad }, distance, pan, pitch: 1.0 };
        mix(&[flat_out(1.0, 0.0), flat_out(1.5, -1.0), flat_out(1.5, 1.0)], 1.0);
        // The file: 44 bytes of header, two bytes a sample.
        assert_eq!(near.wav().len(), 44 + near.samples.len() * 2);
    }

    #[test]
    fn follows_the_aircraft() {
        let mut world = sim_core::World::new(1);
        let id = world.add_aircraft(sim_core::AircraftKind::C172, sim_core::Start::Home);
        let p = Params::of(world.vehicle(id), false, 0.4, false);
        assert_eq!((p.volume, p.stall, p.touchdown, p.tone), (1.0, 0.0, 0.4, 0.0));
        assert_eq!(Params::of(world.vehicle(id), false, 0.4, true).tone, 1.0);
        assert!(p.rpm > 300.0 && p.rpm < 1200.0, "rpm {}", p.rpm);
        assert_eq!(Params::of(world.vehicle(id), true, 0.0, true), Params { tone: 0.0, ..Params::of(world.vehicle(id), true, 0.0, false) });
        assert_eq!(Params::of(world.vehicle(id), true, 0.0, false).volume, 0.0);
    }
}
