//! A vehicle in the world, whatever it is: a rigid body (`body::Body`) moved by its own dynamics,
//! with what every vehicle shares round it: the wind it flies or drives through, the ground under
//! it and ahead of it, the trees and buildings it can run into, and where it was last placed.
//!
//! What kind of vehicle it is lives in `Dynamics`, one variant per kind (aircraft and multirotors;
//! a car later, `docs/ROADMAP.md` seam 4), and its controls in `Controls`, the same kinds. Plain
//! enums: stepping stays one `match`, deterministic, with no trait objects. Code that only makes
//! sense for one kind asks for it (`Vehicle::aircraft`, `multirotor`: `Option`s) and says what it
//! does with the others.

use std::collections::BTreeMap;
use std::f64::consts::PI;

use crate::aircraft::{Aircraft, AircraftControls, Environment, GroundKind, State};
use crate::body::Body;
use crate::car::{Car, CarControls, TerrainGround};
use crate::instruments::Earth;
use crate::math::{euler_angles, DEG, V3};
use crate::multirotor::{Multirotor, MultirotorControls};
use crate::scenery::{self, Obstacle, Shape};
use crate::terrain::{Airport, Terrain};
use crate::wind::{Weather, WindField};
use crate::world::{World, GROUND_AHEAD};
use crate::wreck::Wreck;

/// Index of a vehicle in `World::vehicles`. Vehicles are never removed, so it stays valid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VehicleId(pub usize);

/// What moves a vehicle: its own state, controls and forces. One variant per kind of vehicle.
/// The vehicles are kept by value in `World::vehicles` and stepped in place: an aircraft is much
/// bigger than a multirotor, and boxing it would only add a pointer to follow every step.
#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Dynamics {
    Aircraft(Aircraft),
    Multirotor(Multirotor),
    Car(Car),
}

/// What a pilot sets, for each kind of vehicle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Controls {
    Aircraft(AircraftControls),
    Multirotor(MultirotorControls),
    Car(CarControls),
}

impl Dynamics {
    /// The rigid body.
    pub fn body(&self) -> Body {
        match self {
            Dynamics::Aircraft(a) => a.state.body(),
            Dynamics::Multirotor(m) => m.body,
            Dynamics::Car(c) => c.body,
        }
    }

    /// Its centre of mass, body axes from its reference point (what `Body::pos` is).
    pub fn cg(&self) -> V3 {
        match self {
            Dynamics::Aircraft(a) => a.cg(),
            // Its body is placed by its centre of gravity.
            Dynamics::Multirotor(_) | Dynamics::Car(_) => V3::ZERO,
        }
    }

    /// Speed through the air, m/s, as the last step measured it.
    pub fn airspeed(&self) -> f64 {
        match self {
            Dynamics::Aircraft(a) => a.data().tas,
            Dynamics::Multirotor(m) => m.data().airspeed,
            Dynamics::Car(c) => c.data().speed,
        }
    }

    pub fn on_ground(&self) -> bool {
        match self {
            Dynamics::Aircraft(a) => a.data().on_ground(),
            Dynamics::Multirotor(m) => m.data().on_ground,
            Dynamics::Car(c) => c.data().on_ground,
        }
    }

    /// Why it can go no further, if so (text for the player).
    pub fn crashed(&self) -> Option<&'static str> {
        match self {
            Dynamics::Aircraft(a) => a.crashed(),
            Dynamics::Multirotor(m) => m.crashed(),
            Dynamics::Car(c) => c.crashed(),
        }
    }

    /// In pieces or under water: nothing moves it any more.
    pub fn gone(&self) -> bool {
        match self {
            Dynamics::Aircraft(a) => a.damage().broken_up || a.damage().sunk,
            Dynamics::Multirotor(m) => m.sunk(),
            Dynamics::Car(c) => c.sunk(),
        }
    }

    pub fn controls(&self) -> Controls {
        match self {
            Dynamics::Aircraft(a) => Controls::Aircraft(a.controls),
            Dynamics::Multirotor(m) => Controls::Multirotor(m.controls),
            Dynamics::Car(c) => Controls::Car(c.controls),
        }
    }

    pub fn set_controls(&mut self, controls: Controls) {
        match (self, controls) {
            (Dynamics::Aircraft(a), Controls::Aircraft(c)) => a.controls = c,
            (Dynamics::Multirotor(m), Controls::Multirotor(c)) => m.controls = c,
            (Dynamics::Car(car), Controls::Car(c)) => car.controls = c,
            // Controls of another kind move nothing (a pilot that cannot fly it hands back its own).
            _ => {}
        }
    }

    /// The boxes of its solid (lower and upper corners, body axes from its reference point): what
    /// another vehicle runs into.
    pub fn solid(&self) -> &[(V3, V3)] {
        match self {
            Dynamics::Aircraft(a) => &a.p.airframe,
            Dynamics::Multirotor(m) => m.p.solid,
            Dynamics::Car(c) => &c.p.solid,
        }
    }

    /// The points of it that run into things, where they are and how they move now.
    pub(crate) fn contact_points(&self) -> Vec<ContactPoint> {
        match self {
            Dynamics::Aircraft(a) => a.contact_points(),
            Dynamics::Multirotor(m) => m.contact_points(),
            Dynamics::Car(c) => c.contact_points(),
        }
    }

    /// Takes `energy` (J) out of its motion at a point of it (body axes); whether it had that much.
    pub fn absorb(&mut self, r: V3, energy: f64) -> bool {
        match self {
            Dynamics::Aircraft(a) => a.absorb(r, energy),
            Dynamics::Multirotor(m) => m.absorb(energy),
            Dynamics::Car(c) => c.absorb(energy),
        }
    }

    /// A collision at `speed` against something that holds: stopped, or broken up. `keep` is the
    /// share of its velocity its pieces keep.
    pub fn collide(&mut self, what: &'static str, speed: f64, keep: f64) {
        match self {
            Dynamics::Aircraft(a) => a.collide(what, speed, keep),
            Dynamics::Multirotor(m) => m.collide(what, speed, keep),
            Dynamics::Car(c) => c.collide(what, speed, keep),
        }
    }

    /// What has come off it since it was last asked: the world's now.
    pub fn take_wrecks(&mut self) -> Vec<Wreck> {
        match self {
            Dynamics::Aircraft(a) => a.take_wrecks(),
            // A crashed multirotor or car stays whole: it is its own wreck.
            Dynamics::Multirotor(_) | Dynamics::Car(_) => Vec::new(),
        }
    }
}

/// What part of a vehicle a point is on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum On {
    /// Fuselage, engine, tail cone, fin: the body.
    Body,
    /// A leg of the gear (a wheel), in the order of its type's.
    Wheel(usize),
    /// The leading edge of a wing (0 left, 1 right) at a station, m from the centre line.
    Wing { side: usize, station: f64 },
    /// The same of half of the tailplane.
    Tailplane { side: usize, station: f64 },
}

/// A point of a vehicle as it moves, that can run into things.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ContactPoint {
    /// Where it is and how fast it moves, world axes.
    pub at: V3,
    pub vel: V3,
    /// Where it is on the vehicle, body axes from its reference point.
    pub body: V3,
    pub on: On,
    /// Length of the vehicle it stands for, m (what drags through a crown).
    pub length: f64,
}

/// A trunk a point has run into: the force that fells it, N, the energy that takes, J, and its
/// radius there, m.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Trunk {
    pub fells_at: f64,
    pub felling: f64,
    pub radius: f64,
}

/// Modulus of rupture of green wood, Pa: 35 to 60 MPa for the common hardwoods and softwoods
/// (USDA Wood Handbook, FPL-GTR-282, tables 5-3), the lower end for a live trunk. A trunk of radius r
/// takes a bending moment of `WOOD_RUPTURE · π r³ / 4` at its foot.
const WOOD_RUPTURE: f64 = 40.0e6;
/// What a trunk bends through, radians, splitting and splintering, before its fibres all part:
/// the energy that fells it is the moment it takes times this. Estimated.
const TRUNK_BREAK_ANGLE: f64 = 0.2;
/// Drag of the branches of a crown on a vehicle in it, N per metre of it and (m/s)²: through 30 m
/// of crown a light aircraft at 30 m/s loses about half its speed. Estimated.
const CROWN_DRAG: f64 = 1.7;

/// Steps between two looks at the ground ahead (a quarter of a second).
const AHEAD_EVERY: u32 = 60;

/// One vehicle in the world: its dynamics and what is round it.
#[derive(Clone, Debug)]
pub struct Vehicle {
    pub dynamics: Dynamics,
    /// The turbulence it is going through.
    pub wind: WindField,
    /// The airport it was last placed at.
    pub home: Option<Airport>,
    environment: Environment,
    agl: f64,
    /// See `Observation::ground_ahead`; looked up again every `AHEAD_EVERY` steps.
    ground_ahead: [f64; 4],
    ahead_countdown: u32,
    /// The trees and buildings around it while it is low enough to hit them.
    nearby: Nearby,
}

impl Vehicle {
    /// Default distance of `reset_on_final`, metres before the aiming point.
    pub const FINAL_DISTANCE: f64 = 2500.0;

    /// A vehicle at the origin, not yet placed: call one of the `reset_*` functions. `seed` seeds
    /// its turbulence and what of it is random (an aircraft's gyros).
    pub fn new(mut dynamics: Dynamics, seed: u64) -> Vehicle {
        match &mut dynamics {
            Dynamics::Aircraft(a) => a.instruments = crate::instruments::Instruments::new(seed),
            Dynamics::Multirotor(m) => m.seed(seed),
            Dynamics::Car(c) => c.seed(seed),
        }
        Vehicle {
            dynamics,
            wind: WindField::new(seed),
            home: None,
            environment: Environment::default(),
            agl: 0.0,
            ground_ahead: [0.0; 4],
            ahead_countdown: 0,
            nearby: Nearby::default(),
        }
    }

    /// The aircraft it is, if it is one: what calls it says what it does with the others.
    pub fn aircraft(&self) -> Option<&Aircraft> {
        match &self.dynamics {
            Dynamics::Aircraft(a) => Some(a),
            _ => None,
        }
    }

    pub fn aircraft_mut(&mut self) -> Option<&mut Aircraft> {
        match &mut self.dynamics {
            Dynamics::Aircraft(a) => Some(a),
            _ => None,
        }
    }

    /// The multirotor it is, if it is one.
    pub fn multirotor(&self) -> Option<&Multirotor> {
        match &self.dynamics {
            Dynamics::Multirotor(m) => Some(m),
            _ => None,
        }
    }

    pub fn multirotor_mut(&mut self) -> Option<&mut Multirotor> {
        match &mut self.dynamics {
            Dynamics::Multirotor(m) => Some(m),
            _ => None,
        }
    }

    /// The car it is, if it is one.
    pub fn car(&self) -> Option<&Car> {
        match &self.dynamics {
            Dynamics::Car(c) => Some(c),
            _ => None,
        }
    }

    pub fn car_mut(&mut self) -> Option<&mut Car> {
        match &mut self.dynamics {
            Dynamics::Car(c) => Some(c),
            _ => None,
        }
    }

    /// The rigid body.
    pub fn body(&self) -> Body {
        self.dynamics.body()
    }

    /// The air and ground it saw in its last step.
    pub fn environment(&self) -> &Environment {
        &self.environment
    }

    /// Height of its reference point above the ground below it.
    pub fn agl(&self) -> f64 {
        self.agl
    }

    /// For a replay: this height above the ground, as it was when what is shown of it was kept
    /// (`Aircraft::show`). Its next step finds the ground again.
    pub fn show_agl(&mut self, agl: f64) {
        self.agl = agl;
    }

    pub(crate) fn ground_ahead(&self) -> [f64; 4] {
        self.ground_ahead
    }

    /// Places it at rest 40 m from the start of the runway. `reversed` uses the opposite end.
    pub fn reset_on_runway(&mut self, terrain: &Terrain, weather: &Weather, earth: &Earth, a: &Airport, reversed: bool) {
        self.home = Some(*a);
        let heading = a.heading + if reversed { PI } else { 0.0 };
        let back = a.length / 2.0 - 40.0;
        let (n, e) = (a.n - heading.cos() * back, a.e - heading.sin() * back);
        match &mut self.dynamics {
            Dynamics::Aircraft(craft) => craft.stand(n, e, a.elevation, heading),
            Dynamics::Multirotor(m) => m.stand(n, e, a.elevation, heading),
            Dynamics::Car(c) => c.stand(&TerrainGround::dry(terrain), n, e, heading),
        }
        self.placed(terrain, weather, earth, a.elevation);
    }

    /// Starts in the air: an aircraft in trimmed level flight 900 m above the ground, a
    /// multirotor hovering its type's `air_height` above it.
    pub fn reset_in_air(&mut self, terrain: &Terrain, weather: &Weather, earth: &Earth, n: f64, e: f64, heading: f64) {
        let ground = terrain.height(n, e).max(0.0);
        match &mut self.dynamics {
            Dynamics::Aircraft(craft) => {
                let tas = craft.p.air_start_speed;
                craft.fly_steady(V3::new(n, e, -(ground + 900.0)), heading, tas, 0, 0.0, &weather.air, (0.0, 250.0, 0.7));
            }
            Dynamics::Multirotor(m) => {
                let altitude = ground + m.p.air_height;
                m.hover(V3::new(n, e, -altitude), heading, weather.air.density(altitude), altitude - m.p.air_height);
            }
            // A car has no air to start in: it stands on the ground there.
            Dynamics::Car(c) => c.stand(&TerrainGround::dry(terrain), n, e, heading),
        }
        self.placed(terrain, weather, earth, ground);
    }

    /// Stands it on the ground at (`n`, `e`), facing `heading`: an aircraft on its wheels, engine
    /// idling, a multirotor on its feet, disarmed.
    pub fn reset_on_ground(&mut self, terrain: &Terrain, weather: &Weather, earth: &Earth, n: f64, e: f64, heading: f64) {
        let ground = terrain.height(n, e).max(0.0);
        match &mut self.dynamics {
            Dynamics::Aircraft(craft) => craft.stand(n, e, ground, heading),
            Dynamics::Multirotor(m) => m.stand(n, e, ground, heading),
            Dynamics::Car(c) => c.stand(&TerrainGround::dry(terrain), n, e, heading),
        }
        self.placed(terrain, weather, earth, ground);
    }

    /// Puts it in a state set by hand (a teleport: another height, attitude or speed for a test or
    /// a picture), and settles its instruments there as every placement does: the pitot-static
    /// lines, the vertical speed's capsule, the gyros and the lagged air data start from the new
    /// place instead of chasing the jump to it (a drop of 500 m would read for seconds as a descent
    /// of tens of thousands of feet a minute). The altimeter is set to the QNH of the ground below.
    /// A multirotor takes the body of `state` (its position, attitude, velocity and rates).
    pub fn place(&mut self, terrain: &Terrain, weather: &Weather, earth: &Earth, state: State) {
        match &mut self.dynamics {
            Dynamics::Aircraft(craft) => craft.put(state, &weather.air),
            Dynamics::Multirotor(m) => m.put(state.body()),
            Dynamics::Car(c) => c.put(state.body()),
        }
        self.placed(terrain, weather, earth, terrain.height(state.pos.x, state.pos.y).max(0.0));
    }

    /// Starts on final approach to a runway: on a 3° glide path, the one the approach lights show,
    /// at the type's `final_speed` and `final_flap_notch` (68 knots and flaps 20 in the C172).
    /// `distance` is in metres before the aiming point, which is 150 m past the threshold
    /// (`Vehicle::FINAL_DISTANCE` is the usual one).
    pub fn reset_on_final(&mut self, terrain: &Terrain, weather: &Weather, earth: &Earth, a: &Airport, reversed: bool, distance: f64) {
        self.home = Some(*a);
        let heading = a.heading + if reversed { PI } else { 0.0 };
        let glide = 3.0 * DEG;
        let back = a.length / 2.0 - crate::lights::AIM + distance;
        let (n, e) = (a.n - heading.cos() * back, a.e - heading.sin() * back);
        match &mut self.dynamics {
            Dynamics::Aircraft(craft) => {
                let altitude = a.elevation + distance * glide.tan() + craft.p.cg_height;
                let (tas, notch) = (craft.p.final_speed, craft.p.final_flap_notch);
                craft.fly_steady(V3::new(n, e, -altitude), heading, tas, notch, -glide, &weather.air, (-glide, 180.0, 0.3));
            }
            // A multirotor has no approach to fly: it hovers there, as high as it starts in the air.
            Dynamics::Multirotor(m) => {
                let altitude = terrain.height(n, e).max(0.0) + m.p.air_height;
                m.hover(V3::new(n, e, -altitude), heading, weather.air.density(altitude), altitude - m.p.air_height);
            }
            // Nor an approach: it stands on the ground there.
            Dynamics::Car(c) => c.stand(&TerrainGround::dry(terrain), n, e, heading),
        }
        self.placed(terrain, weather, earth, a.elevation);
    }

    /// After every placement: the ground and air round it, and its instruments settled, the
    /// altimeter set to the QNH of `elevation`.
    fn placed(&mut self, terrain: &Terrain, weather: &Weather, earth: &Earth, elevation: f64) {
        self.ahead_countdown = 0;
        self.sample_environment(terrain, weather);
        match &mut self.dynamics {
            Dynamics::Aircraft(craft) => craft.settle_instruments(&self.environment, &weather.air, earth, weather.air.qnh(elevation)),
            Dynamics::Multirotor(_) | Dynamics::Car(_) => {}
        }
    }

    fn sample_environment(&mut self, terrain: &Terrain, weather: &Weather) {
        let s = self.body();
        let airports = terrain.airports(s.pos.x, s.pos.x, s.pos.y, s.pos.y);
        let g = terrain.ground_with(s.pos.x, s.pos.y, &airports);
        let d = 4.0;
        let gn = terrain.ground_with(s.pos.x + d, s.pos.y, &airports).h;
        let ge = terrain.ground_with(s.pos.x, s.pos.y + d, &airports).h;
        let mut env = Environment::default();
        let altitude = -s.pos.z;
        env.rho = weather.air.density(altitude);
        env.temperature = weather.air.temperature(altitude);
        env.vapour = weather.air.vapour_share(altitude);
        env.ref_n = s.pos.x;
        env.ref_e = s.pos.y;
        if g.h <= 0.3 || g.river > 0.5 {
            env.ground_height = g.h.max(0.0);
            env.slope_n = 0.0;
            env.slope_e = 0.0;
            env.kind = GroundKind::Water;
        } else {
            env.ground_height = g.h;
            env.slope_n = (gn - g.h) / d;
            env.slope_e = (ge - g.h) / d;
            env.kind = if airports.iter().any(|a| a.on_runway(s.pos.x, s.pos.y)) { GroundKind::Runway } else { GroundKind::Grass };
        }
        self.agl = altitude - env.ground_height;
        self.wind.update(weather, World::DT, self.agl, self.dynamics.airspeed(), env.slope_n, env.slope_e);
        env.air_velocity = self.wind.air_velocity();
        env.roll_gust = self.wind.roll_gust();
        self.environment = env;

        if self.ahead_countdown == 0 {
            self.ahead_countdown = AHEAD_EVERY;
            self.look_ahead(terrain);
        }
        self.ahead_countdown -= 1;
    }

    /// Fills `ground_ahead`: the highest ground in each stretch of the line it is moving along.
    fn look_ahead(&mut self, terrain: &Terrain) {
        let s = self.body();
        let v = s.world_velocity();
        let track = if v.x.hypot(v.y) > 5.0 { v.y.atan2(v.x) } else { euler_angles(s.q).heading };
        let (c, sn) = (track.cos(), track.sin());
        let reach = GROUND_AHEAD[3];
        let (end_n, end_e) = (s.pos.x + c * reach, s.pos.y + sn * reach);
        let airports = terrain.airports(s.pos.x.min(end_n), s.pos.x.max(end_n), s.pos.y.min(end_e), s.pos.y.max(end_e));
        self.ground_ahead = [0.0; 4];
        let mut d = 0.0;
        for (i, end) in GROUND_AHEAD.iter().enumerate() {
            while d < *end {
                d += 250.0;
                let h = terrain.ground_with(s.pos.x + c * d, s.pos.y + sn * d, &airports).h;
                self.ground_ahead[i] = self.ground_ahead[i].max(h);
            }
        }
    }

    /// Advances it by one fixed step of `World::DT`.
    pub fn step(&mut self, terrain: &Terrain, weather: &Weather, earth: &Earth) {
        self.step_on(terrain, weather, earth, &[]);
    }

    /// Advances it by one fixed step of `World::DT`, a car on the water of the terrain's circuits.
    pub fn step_on(&mut self, terrain: &Terrain, weather: &Weather, earth: &Earth, water: &[crate::car::wet::RoadWater]) {
        let dt = World::DT;
        self.sample_environment(terrain, weather);
        match &mut self.dynamics {
            Dynamics::Aircraft(craft) => {
                // On the ground with the throttle closed the wheels brake by themselves; the pilot's
                // own brakes (`AircraftControls::brake_left`, `brake_right`) add to that and are left
                // as they were.
                let braking = if craft.data().on_ground() && craft.controls.throttle < 0.02 { 0.6 } else { 0.0 };
                let c = craft.controls;
                craft.controls.brake_left = c.brake_left.max(braking);
                craft.controls.brake_right = c.brake_right.max(braking);
                craft.step(dt, &self.environment);
                craft.controls.brake_left = c.brake_left;
                craft.controls.brake_right = c.brake_right;
                craft.step_instruments(dt, &self.environment, &weather.air, earth);
            }
            Dynamics::Multirotor(m) => m.step(dt, &self.environment),
            Dynamics::Car(c) => c.step(dt, &self.environment, &TerrainGround { terrain, water }),
        }
        self.run_into_scenery(terrain);
    }

    /// Runs into the trees and buildings: a point of it inside one is a collision, and what happens
    /// depends on what it is, what part of the vehicle and how much energy the point brings
    /// (`Aircraft::strike`). Through the crown of a tree the branches drag at every point inside it.
    fn run_into_scenery(&mut self, terrain: &Terrain) {
        if self.dynamics.gone() || self.agl > scenery::tallest(terrain) + 8.0 {
            return;
        }
        let s = self.body();
        if let Dynamics::Multirotor(m) = &mut self.dynamics {
            // Its propellers and frame against the trunks and walls (`Multirotor::hit`).
            let near = self.nearby.near(terrain, s.pos.x, s.pos.y);
            m.hit(&near, multirotor_names, &|p, reach| terrain.solid_surface(p[0], p[1], p[2], reach));
        }
        let points = self.dynamics.contact_points();
        let touches = self.nearby.touches(terrain, s.pos.x, s.pos.y, &points);
        if let Some((n, e)) = self.strike(&points, &touches) {
            self.nearby.felled.push((n, e));
        }
    }

    /// Runs into what has been placed in the world besides the scenery (`World::obstacles`: a race
    /// course's gates), as into the scenery.
    pub(crate) fn run_into(&mut self, obstacles: &[Obstacle]) {
        if obstacles.is_empty() || self.dynamics.gone() {
            return;
        }
        let pos = self.body().pos;
        // The vehicle's span round its reference point, and what it moves in a step besides.
        let reach = 15.0 + self.dynamics.body().vel.length() * World::DT;
        let near: Vec<&Obstacle> = obstacles
            .iter()
            .filter(|o| {
                let (side, top) = o.extent();
                let up = -pos.z - o.base;
                (o.n - pos.x).hypot(o.e - pos.y) < side + reach && up > -top - reach && up < top + reach
            })
            .collect();
        if near.is_empty() {
            return;
        }
        if let Dynamics::Multirotor(m) = &mut self.dynamics {
            let near: Vec<Obstacle> = near.into_iter().copied().collect();
            m.hit(&near, multirotor_names, &|_, _| None);
            return;
        }
        let points = self.dynamics.contact_points();
        let mut touches = Vec::new();
        for (k, p) in points.iter().enumerate() {
            if let Some(o) = near.iter().find(|o| o.contains(p.at.x, p.at.y, -p.at.z)) {
                touches.push((k, **o));
            }
        }
        self.strike(&points, &touches);
    }

    /// What the points inside obstacles do: through the crown of a tree the branches drag at every
    /// point inside it; the first point inside anything solid strikes it, and what happens depends
    /// on what it is, what part of the vehicle and how much energy the point brings
    /// (`Aircraft::strike`; a multirotor has met the solids already, `Multirotor::hit`). The tree
    /// it felled, if it did.
    fn strike(&mut self, points: &[ContactPoint], touches: &[(usize, Obstacle)]) -> Option<(f64, f64)> {
        let mut struck = None;
        for &(k, o) in touches {
            let p = &points[k];
            match o.shape {
                Shape::Tree { trunk, top, .. } => {
                    let up = -p.at.z - o.base;
                    let radius = trunk * (1.0 - 0.8 * up / top);
                    if (p.at.x - o.n).hypot(p.at.y - o.e) < radius {
                        struck.get_or_insert((k, o, Some((radius, up))));
                    } else {
                        // The branches drag the point back over this step.
                        let speed = p.vel.length();
                        self.dynamics.absorb(p.body, CROWN_DRAG * p.length * speed * speed * speed * World::DT);
                    }
                }
                Shape::Block { .. } | Shape::Bar { .. } | Shape::Mapped => {
                    struck.get_or_insert((k, o, None));
                }
            }
        }
        let (k, o, trunk) = struck?;
        // A trunk breaks at its foot under the bending moment of the push, after bending through
        // `TRUNK_BREAK_ANGLE`: the force that fells it, and the energy it takes.
        let trunk = trunk.map(|(radius, up)| {
            let moment = WOOD_RUPTURE * PI * radius.powi(3) / 4.0;
            Trunk { fells_at: moment / up.max(0.5), felling: moment * TRUNK_BREAK_ANGLE, radius }
        });
        let felled = match &mut self.dynamics {
            Dynamics::Aircraft(craft) => craft.strike(&points[k], o.kind, trunk),
            Dynamics::Multirotor(_) => false,
            Dynamics::Car(c) => {
                use crate::scenery::ObstacleKind;
                let what = match o.kind {
                    ObstacleKind::Tree => "Crashed into a tree",
                    ObstacleKind::Building => "Crashed into a building",
                    ObstacleKind::Hangar => "Crashed into a hangar",
                    ObstacleKind::Aircraft => "Crashed into a parked aircraft",
                    ObstacleKind::Gate => "Crashed into a gate",
                    ObstacleKind::Bridge => "Crashed into a bridge",
                };
                c.strike(&points[k], what);
                false
            }
        };
        felled.then_some((o.n, o.e))
    }
}

/// What a multirotor's crash into a kind of thing, and a propeller broken on it, are called.
fn multirotor_names(kind: scenery::ObstacleKind) -> (&'static str, &'static str) {
    use crate::scenery::ObstacleKind;
    match kind {
        ObstacleKind::Tree => ("Crashed into a tree", "Broke a propeller on a tree"),
        ObstacleKind::Building => ("Crashed into a building", "Broke a propeller on a building"),
        ObstacleKind::Hangar => ("Crashed into a hangar", "Broke a propeller on a hangar"),
        ObstacleKind::Aircraft => ("Crashed into a parked aircraft", "Broke a propeller on a parked aircraft"),
        ObstacleKind::Gate => ("Crashed into a gate", "Broke a propeller on a gate"),
        ObstacleKind::Bridge => ("Crashed into a bridge", "Broke a propeller on a bridge"),
    }
}

/// The trees and buildings near a vehicle, by tile, worked out from the world's seed as they are
/// needed (`scenery::tree_obstacles`, `scenery::house_obstacles`), and the trees it has felled.
#[derive(Clone, Debug, Default)]
struct Nearby {
    trees: BTreeMap<(i32, i32), Vec<Obstacle>>,
    houses: BTreeMap<(i32, i32), Vec<Obstacle>>,
    felled: Vec<(f64, f64)>,
}

impl Nearby {
    /// The trees and buildings standing within reach of (`n`, `e`): what a multirotor there may
    /// strike (`Multirotor::hit`).
    fn near(&mut self, terrain: &Terrain, n: f64, e: f64) -> Vec<Obstacle> {
        if self.trees.len() > 64 || self.houses.len() > 16 {
            *self = Nearby::default();
        }
        let mut found = Vec::new();
        for (tiles, size, reach, make) in [
            (&mut self.trees, scenery::TREE_TILE, 8.0 + 7.0, scenery::tree_obstacles as fn(&Terrain, (i32, i32), f64) -> Vec<Obstacle>),
            (&mut self.houses, scenery::HOUSE_TILE, 8.0 + scenery::HOUSE_REACH, scenery::house_obstacles),
        ] {
            let (x0, x1) = (((e - reach) / size).floor() as i32, ((e + reach) / size).floor() as i32);
            let (z0, z1) = (((-n - reach) / size).floor() as i32, ((-n + reach) / size).floor() as i32);
            for x in x0..=x1 {
                for z in z0..=z1 {
                    let list = tiles.entry((x, z)).or_insert_with(|| make(terrain, (x, z), size));
                    found.extend(list.iter().filter(|o| (o.n - n).hypot(o.e - e) < o.extent().0 + 2.0 && !self.felled.contains(&(o.n, o.e))).copied());
                }
            }
        }
        found
    }

    /// The points inside a tree or a building, each with the first it is inside. (`n`, `e`) is the
    /// middle of the points.
    fn touches(&mut self, terrain: &Terrain, n: f64, e: f64, points: &[ContactPoint]) -> Vec<(usize, Obstacle)> {
        if self.trees.len() > 64 || self.houses.len() > 16 {
            *self = Nearby::default();
        }
        let mut found = Vec::new();
        let mut inside = vec![false; points.len()];
        // The span of the vehicle, and as far as the widest crown and the biggest building reach.
        for (tiles, size, reach, make) in [
            (&mut self.trees, scenery::TREE_TILE, 8.0 + 7.0, scenery::tree_obstacles as fn(&Terrain, (i32, i32), f64) -> Vec<Obstacle>),
            (&mut self.houses, scenery::HOUSE_TILE, 8.0 + scenery::HOUSE_REACH, scenery::house_obstacles),
        ] {
            let (x0, x1) = (((e - reach) / size).floor() as i32, ((e + reach) / size).floor() as i32);
            let (z0, z1) = (((-n - reach) / size).floor() as i32, ((-n + reach) / size).floor() as i32);
            for x in x0..=x1 {
                for z in z0..=z1 {
                    let list = tiles.entry((x, z)).or_insert_with(|| make(terrain, (x, z), size));
                    for o in list.iter() {
                        let (side, _) = o.extent();
                        if (o.n - n).hypot(o.e - e) > side + 8.0 || self.felled.contains(&(o.n, o.e)) {
                            continue;
                        }
                        for (k, p) in points.iter().enumerate() {
                            if !inside[k] && o.contains(p.at.x, p.at.y, -p.at.z) {
                                inside[k] = true;
                                found.push((k, *o));
                            }
                        }
                    }
                }
            }
        }
        // The buildings and bridges of a real place, each its own solid.
        for (k, p) in points.iter().enumerate() {
            if inside[k] {
                continue;
            }
            if let Some(hit) = terrain.solid_at(p.at.x, p.at.y, -p.at.z) {
                inside[k] = true;
                found.push((k, Obstacle { kind: hit.kind, n: hit.n, e: hit.e, base: 0.0, shape: Shape::Mapped }));
            }
        }
        found
    }
}
