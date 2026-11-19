//! The replay (F2): the last minutes of the world, kept all the time, shown again as they happened
//! while nobody flies.
//!
//! [`Recorder`] keeps, after every step of the world, what is due of it in rings that forget what
//! is older than the window (ten minutes by default, the setting `replay_minutes`):
//!
//! - the player's aircraft as it is seen (`sim_core::Look`: position, attitude, velocities,
//!   controls and surfaces, propeller, flaps, what the dials show, damage, crash) and its height
//!   above the ground and the hour, 15 times a second, blended in between (`Look::blend`); a
//!   multirotor's the same way (`MultirotorLook`: body, sticks, rotors, battery, armed, crash, the propellers' blades);
//! - a whole copy of the player's aircraft every two seconds (its type, its lamps' switches, what is
//!   slow to change), which each look is put over (`Aircraft::show`), and its GFC 700 five times a
//!   second;
//! - the other aircraft (traffic, trained models), each one's state, controls and flaps, five times a
//!   second, blended in between;
//! - the wrecks when they change, 15 times a second while the newest is young, once a second after.
//!
//! [`Playback`] holds the world as it was when the replay began (its aircraft, wrecks, GFC 700 and
//! hour) while the world's own are written with the moment shown; leaving puts them back as they
//! were, so the flight goes on exactly from there. The world's clock does not move during a replay:
//! the picture's clock (water, fire, clouds) is the replay's.

use sim_core::{act, euler_angles, Aircraft, AircraftControls, Airport, Dynamics, Gfc700, Look, MultirotorLook, State, Vehicle, VehicleId, World, Wreck, DEG, FEET, KNOTS};
use std::collections::VecDeque;
use std::sync::Arc;

/// Seconds kept unless the settings say otherwise (`MINUTES_KEY`).
pub const DEFAULT_MINUTES: f64 = 10.0;
/// The setting: minutes kept.
pub const MINUTES_KEY: &str = "replay_minutes";
/// The minutes the setting can ask for.
pub const MINUTES_RANGE: (f64, f64) = (1.0, 30.0);
/// Seconds between the looks kept of the player's aircraft.
pub const LOOK_EVERY: f64 = 1.0 / 15.0;
/// Seconds between the whole copies of the player's aircraft.
pub const COPY_EVERY: f64 = 2.0;
/// Seconds between the copies of the GFC 700, and between the states of the other aircraft.
pub const GFC_EVERY: f64 = 0.2;
pub const OTHERS_EVERY: f64 = 0.2;
/// The wrecks are kept as often as the looks while the newest is younger than this, then once a second.
const YOUNG_WRECK: f64 = 30.0;
/// The speeds a replay can run at.
pub const SPEEDS: [f64; 6] = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0];
/// A touchdown counts as a landing after this long in the air.
const AIRBORNE_BEFORE_LANDING: f64 = 3.0;
/// The g of a landing is the most felt this long after the wheels touch.
const LANDING_G_WINDOW: f64 = 0.6;

/// One thing kept, and when.
type Kept<'a, T> = &'a (f64, T);

/// Things kept at a steady rate, the oldest forgotten.
struct Track<T> {
    every: f64,
    items: VecDeque<(f64, T)>,
}

impl<T> Track<T> {
    fn new(every: f64) -> Track<T> {
        Track { every, items: VecDeque::new() }
    }

    fn due(&self, time: f64) -> bool {
        self.items.back().is_none_or(|(last, _)| time - last >= self.every - 1e-9)
    }

    fn push(&mut self, time: f64, item: T) {
        // The clock never goes back; if it seems to (a new world), start afresh.
        if self.items.back().is_some_and(|(last, _)| time < *last) {
            self.items.clear();
        }
        self.items.push_back((time, item));
    }

    /// Forgets what is older than `start`, keeping the last one before it (what was so at `start`).
    fn forget(&mut self, start: f64) {
        while self.items.len() > 1 && self.items[1].0 <= start {
            self.items.pop_front();
        }
    }

    /// The last one kept at or before `time` and the one after it (the same one at the ends), and
    /// how far `time` is from the first to the second, 0...1.
    fn around(&self, time: f64) -> Option<(Kept<'_, T>, Kept<'_, T>, f64)> {
        let i = self.items.partition_point(|(t, _)| *t <= time);
        let before = &self.items[i.saturating_sub(1).min(self.items.len().checked_sub(1)?)];
        let after = self.items.get(i).unwrap_or(before);
        let span = after.0 - before.0;
        let k = if span > 0.0 { ((time - before.0) / span).clamp(0.0, 1.0) } else { 0.0 };
        Some((before, after, k))
    }

    /// The last one kept at or before `time`, or the first.
    fn at(&self, time: f64) -> Option<&T> {
        self.around(time).map(|(before, _, _)| &before.1)
    }

    fn len(&self) -> usize {
        self.items.len()
    }
}

/// What is seen of a vehicle, of its kind.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
enum Seen {
    Aircraft(Look),
    Multirotor(MultirotorLook),
}

impl Seen {
    fn of(dynamics: &Dynamics) -> Seen {
        match dynamics {
            Dynamics::Aircraft(a) => Seen::Aircraft(a.look()),
            Dynamics::Multirotor(m) => Seen::Multirotor(m.look()),
            Dynamics::Car(_) => unreachable!("the flight game has no cars: they are the driving game's"),
        }
    }

    /// `k` of the way to `next`, taken `dt` later; of another kind (the player changed craft), the nearer.
    fn blend(&self, next: &Seen, k: f64, dt: f64) -> Seen {
        match (self, next) {
            (Seen::Aircraft(a), Seen::Aircraft(b)) => Seen::Aircraft(a.blend(b, k, dt)),
            (Seen::Multirotor(a), Seen::Multirotor(b)) => Seen::Multirotor(a.blend(b, k, dt)),
            _ if k < 0.5 => *self,
            _ => *next,
        }
    }

    /// Puts it over the vehicle, if the vehicle is of its kind.
    fn show(&self, dynamics: &mut Dynamics) {
        match (self, dynamics) {
            (Seen::Aircraft(look), Dynamics::Aircraft(a)) => a.show(look),
            (Seen::Multirotor(look), Dynamics::Multirotor(m)) => m.show(look),
            _ => {}
        }
    }

    fn position(&self) -> sim_core::V3 {
        match self {
            Seen::Aircraft(l) => l.state.pos,
            Seen::Multirotor(l) => l.body.pos,
        }
    }

    /// The airspeed shown, m/s: an aircraft's dial's, a multirotor's true.
    fn airspeed(&self) -> f64 {
        match self {
            Seen::Aircraft(l) => l.readings.airspeed,
            Seen::Multirotor(l) => l.data.airspeed,
        }
    }
}

/// The player's craft at one moment.
#[derive(Clone, Copy, Debug)]
struct Moment {
    look: Seen,
    agl: f64,
    hour: f64,
}

/// Another vehicle at one moment: an aircraft's state, controls and flaps, a multirotor's look
/// (boxed: the traffic are aircraft, and their moments stay small).
#[derive(Clone, Debug)]
enum Motion {
    Aircraft { state: State, controls: AircraftControls, flap_deg: f64, agl: f64 },
    Multirotor { look: Box<MultirotorLook>, agl: f64 },
}

impl Motion {
    fn of(vehicle: &Vehicle) -> Motion {
        match &vehicle.dynamics {
            Dynamics::Aircraft(a) => Motion::Aircraft { state: a.state, controls: a.controls, flap_deg: a.flap_deg, agl: vehicle.agl() },
            Dynamics::Multirotor(m) => Motion::Multirotor { look: Box::new(m.look()), agl: vehicle.agl() },
            Dynamics::Car(_) => unreachable!("the flight game has no cars: they are the driving game's"),
        }
    }

    fn blend(&self, next: &Motion, k: f64, dt: f64) -> Motion {
        match (self, next) {
            (Motion::Aircraft { state, controls, flap_deg, agl }, Motion::Aircraft { state: s2, controls: c2, flap_deg: f2, agl: a2 }) => {
                // The same blend as the player's, through a look of a stand-in aircraft.
                let look = |state: State, controls: AircraftControls, flap_deg: f64| {
                    let mut a = Aircraft::default();
                    (a.state, a.controls, a.flap_deg) = (state, controls, flap_deg);
                    a.look()
                };
                let l = look(*state, *controls, *flap_deg).blend(&look(*s2, *c2, *f2), k, dt);
                Motion::Aircraft { state: l.state, controls: l.controls, flap_deg: l.flap_deg, agl: agl + (a2 - agl) * k }
            }
            (Motion::Multirotor { look, agl }, Motion::Multirotor { look: l2, agl: a2 }) => Motion::Multirotor { look: Box::new(look.blend(l2, k, dt)), agl: agl + (a2 - agl) * k },
            _ if k < 0.5 => self.clone(),
            _ => next.clone(),
        }
    }

    /// Puts it over the vehicle, if the vehicle is of its kind.
    fn show(&self, vehicle: &mut Vehicle) {
        match (self, &mut vehicle.dynamics) {
            (Motion::Aircraft { state, controls, flap_deg, agl }, Dynamics::Aircraft(craft)) => {
                (craft.state, craft.controls, craft.flap_deg) = (*state, *controls, *flap_deg);
                vehicle.show_agl(*agl);
            }
            (Motion::Multirotor { look, agl }, Dynamics::Multirotor(m)) => {
                m.show(look);
                vehicle.show_agl(*agl);
            }
            _ => {}
        }
    }
}

/// The last landing: how the wheels met the ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Landing {
    /// World time of the touchdown.
    pub time: f64,
    /// Sink rate, ft/min.
    pub sink_fpm: f64,
    /// The most load factor felt in the instant after.
    pub g: f64,
    /// Indicated and ground speed, kt.
    pub airspeed_kt: f64,
    pub ground_speed_kt: f64,
    /// Degrees, right wing down and nose up positive.
    pub bank_deg: f64,
    pub pitch_deg: f64,
    /// On a runway: its number as landed on, metres from its threshold along it, and metres right
    /// (positive) or left of its centre line.
    pub runway: Option<(u32, f64, f64)>,
}

impl Landing {
    /// How the touchdown measured on `airport`'s runway, flying along `heading` at (`n`, `e`): the
    /// number of the end landed towards, metres past its threshold, metres right of the centre line.
    pub fn on_runway(airport: &Airport, n: f64, e: f64, heading: f64) -> (u32, f64, f64) {
        let (dn, de) = (n - airport.n, e - airport.e);
        let (sin, cos) = airport.heading.sin_cos();
        let way = if (heading - airport.heading).cos() >= 0.0 { 1.0 } else { -1.0 };
        let along = (dn * cos + de * sin) * way;
        let right = (-dn * sin + de * cos) * way;
        let landed_towards = if way > 0.0 { airport.heading } else { airport.heading + std::f64::consts::PI };
        let number = ((landed_towards / DEG / 10.0).round() as u32 + 35) % 36 + 1;
        (number, along + airport.length / 2.0, right)
    }
}

/// What the replay keeps: see the module's notes.
pub struct Recorder {
    /// Seconds kept.
    pub window: f64,
    looks: Track<Moment>,
    copies: Track<Box<Dynamics>>,
    gfc: Track<Option<Box<Gfc700>>>,
    others: Track<Vec<Motion>>,
    wrecks: Track<Arc<Vec<Wreck>>>,
    landings: VecDeque<Landing>,
    /// Seconds the player's aircraft has been off the ground, where it was a step ago.
    airborne: f64,
    last_pos: Option<sim_core::V3>,
}

impl Default for Recorder {
    fn default() -> Recorder {
        Recorder::new(DEFAULT_MINUTES * 60.0)
    }
}

impl Recorder {
    pub fn new(window: f64) -> Recorder {
        Recorder {
            window,
            looks: Track::new(LOOK_EVERY),
            copies: Track::new(COPY_EVERY),
            gfc: Track::new(GFC_EVERY),
            others: Track::new(OTHERS_EVERY),
            wrecks: Track::new(LOOK_EVERY),
            landings: VecDeque::new(),
            airborne: 0.0,
            last_pos: None,
        }
    }

    /// Forgets everything: the world was made anew.
    pub fn clear(&mut self) {
        *self = Recorder::new(self.window);
    }

    /// After a step of `world`: keeps what is due, and notes a landing of `player`.
    pub fn record(&mut self, world: &World, player: VehicleId, gfc: Option<&Gfc700>) {
        self.record_at(world.time(), world, player, gfc);
    }

    fn record_at(&mut self, time: f64, world: &World, player: VehicleId, gfc: Option<&Gfc700>) {
        let vehicle = world.vehicle(player);
        self.note_landing(time, world, vehicle);
        if self.looks.due(time) {
            self.looks.push(time, Moment { look: Seen::of(&vehicle.dynamics), agl: vehicle.agl(), hour: world.hour });
            // The wrecks, when they have changed.
            let young = world.wrecks.iter().any(|w| time - w.time < YOUNG_WRECK);
            let slow = self.wrecks.items.back().is_some_and(|(last, _)| !young && time - last < 1.0 - 1e-9);
            let same = self.wrecks.items.back().is_some_and(|(_, w)| **w == world.wrecks);
            if !slow && !same {
                self.wrecks.push(time, Arc::new(world.wrecks.clone()));
            }
        }
        if self.copies.due(time) {
            self.copies.push(time, Box::new(vehicle.dynamics.clone()));
        }
        if self.gfc.due(time) {
            self.gfc.push(time, gfc.map(|g| Box::new(g.clone())));
        }
        if world.vehicles.len() > 1 && self.others.due(time) {
            self.others.push(time, world.vehicles.iter().enumerate().filter(|(i, _)| *i != player.0).map(|(_, v)| Motion::of(v)).collect());
        }
        let start = time - self.window;
        self.looks.forget(start);
        self.copies.forget(start);
        self.gfc.forget(start);
        self.others.forget(start);
        self.wrecks.forget(start);
        while self.landings.front().is_some_and(|l| l.time < start) {
            self.landings.pop_front();
        }
    }

    /// A touchdown after a while in the air, not a placement: a landing, its g measured over the
    /// instant after.
    fn note_landing(&mut self, time: f64, world: &World, vehicle: &Vehicle) {
        // An aircraft's landings: a multirotor's touches are not measured.
        let Some(a) = vehicle.aircraft() else {
            self.last_pos = Some(vehicle.body().pos);
            return;
        };
        let s = a.state;
        let jumped = self.last_pos.is_none_or(|p| (p - s.pos).length() > 5.0);
        self.last_pos = Some(s.pos);
        let on_ground = a.data().on_ground() && a.crashed().is_none();
        if let Some(last) = self.landings.back_mut().filter(|l| time - l.time <= LANDING_G_WINDOW) {
            last.g = last.g.max(a.data().load_factor());
        }
        if !on_ground {
            self.airborne += World::DT;
            return;
        }
        let landed = self.airborne >= AIRBORNE_BEFORE_LANDING && !jumped;
        self.airborne = 0.0;
        if !landed {
            return;
        }
        let v = act(s.q, s.vel);
        let e = euler_angles(s.q);
        let (n, east) = (s.pos.x, s.pos.y);
        let runway = world.terrain.airports(n - 100.0, n + 100.0, east - 100.0, east + 100.0).into_iter().find(|r| r.on_runway(n, east)).map(|r| Landing::on_runway(&r, n, east, v.y.atan2(v.x)));
        self.landings.push_back(Landing {
            time,
            sink_fpm: v.z / FEET * 60.0,
            g: a.data().load_factor(),
            airspeed_kt: a.indicated_airspeed() / KNOTS,
            ground_speed_kt: v.x.hypot(v.y) / KNOTS,
            bank_deg: e.roll / DEG,
            pitch_deg: e.pitch / DEG,
            runway,
        });
    }

    /// The first and last moments kept, world time.
    pub fn span(&self) -> Option<(f64, f64)> {
        Some((self.looks.items.front()?.0.max(self.looks.items.back()?.0 - self.window), self.looks.items.back()?.0))
    }

    /// The last landing kept.
    pub fn last_landing(&self) -> Option<&Landing> {
        self.landings.back()
    }

    /// The altitude (ft) and indicated airspeed (kt) of the player's aircraft at about `points`
    /// moments spread over what is kept: (time, altitude, airspeed).
    pub fn profile(&self, points: usize) -> Vec<(f64, f64, f64)> {
        let n = self.looks.len();
        let step = (n / points.max(1)).max(1);
        self.looks.items.iter().step_by(step).map(|(t, m)| (*t, -m.look.position().z / FEET, m.look.airspeed() / KNOTS)).collect()
    }

    /// What is kept, in bytes (the rings' items and what they point to).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn bytes(&self) -> usize {
        use std::mem::size_of;
        let looks = self.looks.items.len() * size_of::<(f64, Moment)>();
        let copies = self.copies.items.len() * (size_of::<(f64, Box<Dynamics>)>() + size_of::<Dynamics>());
        let gfc = self.gfc.items.iter().map(|(_, g)| size_of::<(f64, Option<Box<Gfc700>>)>() + g.as_ref().map_or(0, |_| size_of::<Gfc700>())).sum::<usize>();
        let others = self.others.items.iter().map(|(_, o)| size_of::<(f64, Vec<Motion>)>() + o.capacity() * size_of::<Motion>()).sum::<usize>();
        let wreck = |w: &Wreck| size_of::<Wreck>() + w.pieces.capacity() * size_of::<sim_core::Piece>() + w.fragments.capacity() * size_of::<sim_core::Fragment>() + w.scars.capacity() * size_of::<sim_core::Scar>() + w.splashes.capacity() * size_of::<sim_core::Splash>();
        let wrecks = self.wrecks.items.iter().map(|(_, w)| size_of::<(f64, Arc<Vec<Wreck>>)>() + w.iter().map(wreck).sum::<usize>()).sum::<usize>();
        looks + copies + gfc + others + wrecks + self.landings.len() * size_of::<Landing>()
    }

    /// Writes the moment `time` into the world: the player's aircraft (over the copy kept before
    /// it), the others (over what they are in `live`), the wrecks, the hour, and the GFC 700.
    pub fn show(&self, time: f64, world: &mut World, player: VehicleId, gfc: &mut Option<Gfc700>, live: &[Vehicle]) {
        if let Some((others_before, others_after, k)) = self.others.around(time) {
            let dt = others_after.0 - others_before.0;
            for (i, vehicle) in world.vehicles.iter_mut().enumerate() {
                // The others are kept without the player.
                let Some(j) = (if i < player.0 { Some(i) } else { i.checked_sub(1).filter(|_| i != player.0) }) else { continue };
                let (Some(a), Some(b)) = (others_before.1.get(j), others_after.1.get(j)) else { continue };
                let m = a.blend(b, k, dt);
                if let Some(base) = live.get(i) {
                    vehicle.dynamics = base.dynamics.clone();
                }
                m.show(vehicle);
            }
        }
        if let Some((before, after, k)) = self.looks.around(time) {
            let moment = before.1.look.blend(&after.1.look, k, after.0 - before.0);
            let vehicle = world.vehicle_mut(player);
            if let Some(copy) = self.copies.at(time) {
                vehicle.dynamics = (**copy).clone();
            }
            moment.show(&mut vehicle.dynamics);
            vehicle.show_agl(before.1.agl + (after.1.agl - before.1.agl) * k);
            world.hour = before.1.hour;
        }
        if let Some(wrecks) = self.wrecks.at(time) {
            world.wrecks = (**wrecks).clone();
        }
        if let Some(kept) = self.gfc.at(time) {
            *gfc = kept.as_deref().cloned();
        }
    }
}

/// The world as it was when a replay began, and where the replay is.
pub struct Playback {
    pub vehicles: Vec<Vehicle>,
    pub wrecks: Vec<Wreck>,
    pub gfc: Option<Gfc700>,
    pub hour: f64,
    /// The moment shown, world time; how fast it runs, and whether it runs.
    pub time: f64,
    pub speed: f64,
    pub playing: bool,
}

impl Playback {
    /// Moves on by `dt` seconds of real time, stopping at the end of what is kept.
    pub fn advance(&mut self, dt: f64, span: (f64, f64)) {
        if self.playing {
            self.time += dt * self.speed;
        }
        if self.time >= span.1 {
            self.time = span.1;
            self.playing = false;
        }
        self.time = self.time.max(span.0);
    }

    /// Jumps `seconds` (negative: back), within what is kept.
    pub fn jump(&mut self, seconds: f64, span: (f64, f64)) {
        self.time = (self.time + seconds).clamp(span.0, span.1);
    }

    /// The next speed up (`faster`) or down.
    pub fn change_speed(&mut self, faster: bool) {
        let i = SPEEDS.iter().position(|s| *s >= self.speed - 1e-9).unwrap_or(2);
        let i = if faster { (i + 1).min(SPEEDS.len() - 1) } else { i.saturating_sub(1) };
        self.speed = SPEEDS[i];
    }
}

/// `m:ss` of `seconds`.
pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as i64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// The load factor of a landing as said: `1.4 g`.
pub fn g_text(g: f64) -> String {
    format!("{g:.1} g")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{AircraftKind, KeyboardPilot, Start};

    /// A world with the player and two other aircraft in the air, flown `seconds` with the
    /// keyboard's pilot banking a little, recorded.
    fn flown(seconds: f64) -> (World, VehicleId, Recorder, Vec<(f64, Look)>) {
        let mut world = World::new(3);
        let player = world.add_aircraft(AircraftKind::default(), Start::Air { n: 0.0, e: 0.0, heading: 0.0 });
        let others: Vec<VehicleId> = (1..3).map(|k| world.add_aircraft(AircraftKind::default(), Start::Air { n: 3000.0 * k as f64, e: 0.0, heading: 1.0 })).collect();
        let mut pilot = KeyboardPilot::new();
        pilot.roll = 0.2;
        let mut recorder = Recorder::new(60.0);
        let mut truth = Vec::new();
        let steps = (seconds / World::DT) as usize;
        for i in 0..steps {
            world.fly(player, &mut pilot);
            for id in &others {
                world.fly(*id, &mut KeyboardPilot::new());
            }
            world.step();
            recorder.record(&world, player, None);
            if i % 37 == 0 {
                truth.push((world.time(), world.vehicle(player).aircraft().unwrap().look()));
            }
        }
        (world, player, recorder, truth)
    }

    #[test]
    fn the_moments_kept_come_back_as_they_were_and_in_between_close() {
        let (mut world, player, recorder, truth) = flown(12.0);
        let live = world.vehicles.clone();
        let mut gfc = None;
        let mut worst: f64 = 0.0;
        for (time, look) in &truth {
            recorder.show(*time, &mut world, player, &mut gfc, &live);
            let shown = world.vehicle(player).aircraft().unwrap().look();
            worst = worst.max((shown.state.pos - look.state.pos).length());
            assert!(shown.state.q.dot(look.state.q).abs() > 0.9999, "attitude at {time}");
        }
        assert!(worst < 0.05, "{worst} m off between the looks kept");
        // A look kept is given back exactly.
        let (t, moment) = recorder.looks.items[100];
        recorder.show(t, &mut world, player, &mut gfc, &live);
        assert_eq!(Seen::Aircraft(world.vehicle(player).aircraft().unwrap().look()), moment.look);
    }

    #[test]
    fn recording_the_same_flight_twice_keeps_the_same() {
        let (_, _, a, _) = flown(4.0);
        let (_, _, b, _) = flown(4.0);
        assert_eq!(a.looks.len(), b.looks.len());
        assert!(a.looks.items.iter().zip(&b.looks.items).all(|(x, y)| x.0 == y.0 && x.1.look == y.1.look));
        assert_eq!(a.others.len(), b.others.len());
    }

    #[test]
    fn ten_minutes_with_traffic_fit_in_a_few_megabytes_and_older_ones_are_forgotten() {
        let mut world = World::new(5);
        let player = world.add_aircraft(AircraftKind::default(), Start::Home);
        for _ in 0..6 {
            world.add_aircraft(AircraftKind::default(), Start::Home);
        }
        let gfc = world.vehicle(player).aircraft().unwrap().p.afcs.map(Gfc700::new);
        let mut recorder = Recorder::new(600.0);
        // Eleven minutes of steps, the world standing still (the size does not depend on it).
        let steps = (660.0 / World::DT) as usize;
        for i in 0..steps {
            recorder.record_at(i as f64 * World::DT, &world, player, gfc.as_ref());
        }
        let (start, end) = recorder.span().unwrap();
        assert!((end - start - 600.0).abs() < 1.0, "{start}..{end}");
        assert!((recorder.looks.len() as f64 - 600.0 / LOOK_EVERY).abs() < 3.0);
        let mb = recorder.bytes() as f64 / 1e6;
        println!("10 min, the player and 6 others: {mb:.1} MB");
        assert!(mb < 25.0, "{mb} MB");
    }

    #[test]
    fn a_touchdown_on_a_runway_is_measured_from_its_threshold_and_centre_line() {
        let airport = Airport { id: 0, n: 0.0, e: 0.0, heading: 90.0 * DEG, elevation: 0.0, length: 1000.0, width: 30.0, detail: sim_core::Detail::GENERATED };
        // Landing eastwards 200 m past the west end, 3 m south of the centre line (to the right).
        let (number, from, right) = Landing::on_runway(&airport, -3.0, -300.0, 88.0 * DEG);
        assert_eq!(number, 9);
        assert!((from - 200.0).abs() < 1e-9 && (right - 3.0).abs() < 1e-9, "{from} {right}");
        // Westwards, from the east end.
        let (number, from, right) = Landing::on_runway(&airport, -3.0, 300.0, 271.0 * DEG);
        assert_eq!(number, 27);
        assert!((from - 200.0).abs() < 1e-9 && (right + 3.0).abs() < 1e-9, "{from} {right}");
    }

    #[test]
    fn the_playback_stops_at_the_end_and_jumps_within_what_is_kept() {
        let mut p = Playback { vehicles: Vec::new(), wrecks: Vec::new(), gfc: None, hour: 10.0, time: 50.0, speed: 1.0, playing: true };
        p.advance(1.0, (0.0, 60.0));
        assert_eq!(p.time, 51.0);
        p.change_speed(true);
        p.change_speed(true);
        assert_eq!(p.speed, 4.0);
        p.advance(10.0, (0.0, 60.0));
        assert!(p.time == 60.0 && !p.playing);
        p.jump(-30.0, (0.0, 60.0));
        assert_eq!(p.time, 30.0);
        p.jump(-100.0, (0.0, 60.0));
        assert_eq!(p.time, 0.0);
        for _ in 0..10 {
            p.change_speed(false);
        }
        assert_eq!(p.speed, 0.25);
    }
}
