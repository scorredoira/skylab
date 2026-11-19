//! The world: terrain, weather, time and any number of vehicles, advanced in fixed steps.
//!
//! `World` is the single authority over every vehicle (`vehicle::Vehicle`): it steps them, lets
//! them run into each other, keeps what is left of them when they come apart, and tells each pilot
//! what it can see (`Observation`).

use std::f64::consts::PI;
use std::sync::Arc;

use crate::aircraft::{Aircraft, AircraftKind, AircraftObservation, AircraftType, State};
use crate::car::wet::RoadWater;
use crate::car::{collide_cars, Car, CarKind, CarObservation, CarType};
use crate::instruments::Earth;
use crate::math::{act, euler_angles, inverse, DEG, V3};
use crate::multirotor::{collide_multirotors, Multirotor, MultirotorKind, MultirotorObservation, MultirotorType};
use crate::pilot::Pilot;
use crate::scenery::Obstacle;
use crate::terrain::{Airport, Terrain};
use crate::vehicle::{Dynamics, Vehicle, VehicleId};
use crate::wind::Weather;
use crate::wreck::Wreck;

/// Where a new vehicle is placed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Start {
    /// On the runway of the airport nearest to the origin, facing into the wind
    /// (in the air over the origin if the world has no airport near it).
    Home,
    /// Lined up 40 m from the start of a runway, engine idling.
    Runway { airport: Airport, reversed: bool },
    /// Trimmed level flight 900 m above the ground, heading in radians.
    Air { n: f64, e: f64, heading: f64 },
    /// On final approach, `distance` metres before the aiming point.
    Final {
        airport: Airport,
        reversed: bool,
        distance: f64,
    },
}

/// Where each stretch of `Observation::ground_ahead` ends, metres ahead of the vehicle: the
/// stretches are 0-500 m, 500-1000 m, 1-2 km and 2-4 km.
pub const GROUND_AHEAD: [f64; 4] = [500.0, 1000.0, 2000.0, 4000.0];

/// The nearest airport as seen from a vehicle.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct AirportObservation {
    pub id: u64,
    /// Centre of the runway.
    pub north: f64,
    pub east: f64,
    /// Runway direction, radians from north, in [0, π).
    pub heading: f64,
    pub elevation: f64,
    pub length: f64,
    pub width: f64,
    /// Horizontal distance to the centre of the runway, m.
    pub distance: f64,
    /// Direction to go to reach it, radians from north, in [0, 2π).
    pub bearing: f64,
}

/// Another vehicle as seen from the observing one.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Traffic {
    /// `VehicleId` of the other vehicle, as a plain number.
    pub id: usize,
    /// Its position minus the observer's: north, east, down, in metres.
    pub relative: [f64; 3],
    /// Its velocity over the ground: north, east, down, in m/s.
    pub velocity: [f64; 3],
}

/// What a pilot observes of its own kind of vehicle.
#[derive(Clone, Debug, PartialEq)]
pub enum VehicleObservation {
    Aircraft(AircraftObservation),
    Multirotor(MultirotorObservation),
    Car(CarObservation),
}

impl Default for VehicleObservation {
    fn default() -> VehicleObservation {
        VehicleObservation::Aircraft(AircraftObservation::default())
    }
}

/// What a pilot knows when deciding the controls: plain numbers, SI units, angles in radians.
/// What any vehicle's pilot sees, and in `vehicle` what its kind's sees besides.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Observation {
    /// Seconds since the world started.
    pub time: f64,

    // Attitude and motion.
    /// In [0, 2π).
    pub heading: f64,
    /// Positive nose up.
    pub pitch: f64,
    /// Positive right wing down.
    pub roll: f64,
    /// Body angular rates: roll, pitch, yaw, rad/s.
    pub angular_rates: [f64; 3],
    /// Velocity over the ground: north, east, down.
    pub velocity: [f64; 3],
    /// Speed through space (the body's velocity), m/s.
    pub speed: f64,

    // Situation.
    pub on_ground: bool,
    /// Height of the vehicle's reference point above the ground.
    pub agl: f64,
    /// Why the vehicle is wrecked, if it is (text for the player).
    pub crashed: Option<String>,

    // Position.
    pub north: f64,
    pub east: f64,
    /// Above sea level.
    pub altitude: f64,
    /// Highest ground (above sea level, water is 0) in each of four stretches of the straight
    /// line along which the vehicle is moving over the ground; they end at `GROUND_AHEAD`
    /// metres. Sampled every 250 m and refreshed four times a second.
    pub ground_ahead: [f64; 4],

    pub nearest_airport: Option<AirportObservation>,
    /// Every other vehicle in the world, in `VehicleId` order.
    pub traffic: Vec<Traffic>,

    /// What only its kind of vehicle has: an aircraft's instruments and configuration.
    pub vehicle: VehicleObservation,
}

impl Observation {
    /// What an aircraft's pilot sees of it, if the vehicle is one.
    pub fn aircraft(&self) -> Option<&AircraftObservation> {
        match &self.vehicle {
            VehicleObservation::Aircraft(a) => Some(a),
            _ => None,
        }
    }

    /// What a multirotor's pilot sees of it, if the vehicle is one.
    pub fn multirotor(&self) -> Option<&MultirotorObservation> {
        match &self.vehicle {
            VehicleObservation::Multirotor(m) => Some(m),
            _ => None,
        }
    }

    /// What a car's driver sees of it, if the vehicle is one.
    pub fn car(&self) -> Option<&CarObservation> {
        match &self.vehicle {
            VehicleObservation::Car(c) => Some(c),
            _ => None,
        }
    }
}

/// Terrain, weather, time and every vehicle. Advances only when `step` is called.
pub struct World {
    pub terrain: Arc<Terrain>,
    pub weather: Weather,
    /// The planet under it: latitude and magnetic field, for the gyros and the compass.
    pub earth: Earth,
    /// Hour of the day, 0..<24.
    pub hour: f64,
    pub vehicles: Vec<Vehicle>,
    /// What is left of the vehicles that have come apart, oldest first; they stay where they
    /// fell when the vehicle is placed somewhere new.
    pub wrecks: Vec<Wreck>,
    /// What has been placed in the world besides its scenery, that the vehicles run into as into
    /// the scenery: a race course's gates (`course::Track::obstacles`). Shared, as a course is
    /// the same for every world that flies it.
    pub obstacles: Arc<Vec<Obstacle>>,
    /// The water on each of the terrain's circuits (`Terrain::circuits`), which follows
    /// `weather.rain` and which the cars' tyres wipe from their line.
    pub water: Vec<RoadWater>,
    time: f64,
    seed: u64,
}

impl World {
    /// The fixed simulation step, seconds.
    pub const DT: f64 = 1.0 / 240.0;
    /// Wrecks kept; older ones are cleared away.
    pub const MOST_WRECKS: usize = 8;

    /// An empty world: add vehicles with `add_aircraft`, `add_multirotor` or `add_vehicle`.
    pub fn new(seed: u64) -> World {
        World::with_terrain(seed, Arc::new(Terrain::new(seed)))
    }

    /// An empty world on a terrain made beforehand: a region's (`Terrain::from_region`), whose
    /// latitude the planet under it takes, or a seed's. `seed` decides the turbulence.
    pub fn with_terrain(seed: u64, terrain: Arc<Terrain>) -> World {
        let earth = match terrain.region() {
            Some(region) => Earth { latitude: region.latitude, ..Earth::default() },
            None => Earth::default(),
        };
        let water = terrain.circuits().iter().map(|c| RoadWater::new(c.clone())).collect();
        World {
            water,
            terrain,
            weather: Weather::default(),
            earth,
            hour: 10.5,
            vehicles: Vec::new(),
            wrecks: Vec::new(),
            obstacles: Arc::new(Vec::new()),
            time: 0.0,
            seed,
        }
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// The circuits' water as it is after the rain of `weather.rain` has fallen for long (or dry):
    /// for a start in the rain, or dry, without waiting for it.
    pub fn settle_water(&mut self) {
        for w in &mut self.water {
            w.settle(self.weather.rain);
        }
    }

    /// Each car in the wake of the cars ahead of it (`car::slipstream`): the one that takes most.
    fn slipstreams(&mut self) {
        let cars: Vec<(usize, V3, V3, f64)> = self
            .vehicles
            .iter()
            .enumerate()
            .filter_map(|(i, v)| match &v.dynamics {
                Dynamics::Car(c) => Some((i, c.body.pos, act(c.body.q, V3::X), c.p.body.solid.iter().map(|b| -b.0 .0).fold(2.2, f64::max))),
                _ => None,
            })
            .collect();
        if cars.len() < 2 {
            if let Some(&(i, ..)) = cars.first() {
                if let Dynamics::Car(c) = &mut self.vehicles[i].dynamics {
                    c.draft = 0.0;
                }
            }
            return;
        }
        for &(i, pos, _, _) in &cars {
            let mut draft: f64 = 0.0;
            for &(j, other, forward, tail) in &cars {
                if i == j || (other - pos).length() > 90.0 {
                    continue;
                }
                let d = pos - other;
                let behind = -(d.x * forward.x + d.y * forward.y);
                let side = d.x * -forward.y + d.y * forward.x;
                draft = draft.max(crate::car::slipstream(behind - tail - 2.3, side));
            }
            if let Dynamics::Car(c) = &mut self.vehicles[i].dynamics {
                c.draft = draft;
            }
        }
    }

    /// The water on the road ahead of a car, along the way it goes (`CarObservation::water_ahead`).
    pub fn water_ahead(&self, body: &crate::body::Body) -> [f64; 3] {
        if self.water.iter().all(RoadWater::dry) {
            return [0.0; 3];
        }
        let v = body.world_velocity();
        let way = if v.x.hypot(v.y) > 1.0 { V3::new(v.x, v.y, 0.0).normalize() } else { act(body.q, V3::X) };
        let ground = crate::car::TerrainGround { terrain: &self.terrain, water: &self.water };
        crate::car::AHEAD_WATER.map(|d| crate::car::CarGround::patch(&ground, body.pos.x + way.x * d, body.pos.y + way.y * d).water)
    }

    /// The water of the circuits: the rain lays it, the cars' tyres wipe it from where they roll.
    fn wet_circuits(&mut self) {
        let rain = self.weather.rain;
        if self.water.is_empty() || rain <= 0.0 && self.water.iter().all(RoadWater::dry) {
            return;
        }
        for w in &mut self.water {
            w.step(World::DT, rain);
        }
        for v in &self.vehicles {
            let Dynamics::Car(car) = &v.dynamics else { continue };
            for (i, wheel) in car.wheels().iter().enumerate() {
                if !wheel.contact {
                    continue;
                }
                let (at, _) = car.body.point(car.p.hub(i));
                let rolled = (wheel.omega * car.p.axles[i / 2].tyre.radius).abs() * World::DT;
                for w in &mut self.water {
                    if let Some(l) = w.circuit.locate(at.x, at.y) {
                        w.wipe(l.along, l.across, rolled);
                        break;
                    }
                }
            }
        }
    }

    /// Seconds simulated so far.
    pub fn time(&self) -> f64 {
        self.time
    }

    /// Adds an aircraft of a kind of the fleet and places it.
    pub fn add_aircraft(&mut self, kind: AircraftKind, start: Start) -> VehicleId {
        self.add_aircraft_of(kind.spec().clone(), start)
    }

    /// Adds an aircraft of any type (one read from a file and not in the fleet, say) and places it.
    pub fn add_aircraft_of(&mut self, p: AircraftType, start: Start) -> VehicleId {
        self.add_vehicle(Dynamics::Aircraft(Aircraft::with_type(p)), start)
    }

    /// Adds a multirotor of a kind of the fleet (`multirotor::file::fleet`) and places it: on the
    /// ground where an aircraft would stand, hovering where it would fly.
    pub fn add_multirotor(&mut self, kind: MultirotorKind, start: Start) -> VehicleId {
        self.add_multirotor_of(kind.spec().clone(), start)
    }

    /// Adds a multirotor of any type and places it.
    pub fn add_multirotor_of(&mut self, p: MultirotorType, start: Start) -> VehicleId {
        self.add_vehicle(Dynamics::Multirotor(Multirotor::with_type(p)), start)
    }

    /// Adds a car of a kind of the fleet (`car::file::fleet`) and places it: standing on its
    /// wheels where it is put (`Start::Air` and `Start::Final` stand it on the ground there).
    pub fn add_car(&mut self, kind: CarKind, start: Start) -> VehicleId {
        self.add_car_of(kind.spec().clone(), start)
    }

    /// Adds a car of any type and places it.
    pub fn add_car_of(&mut self, p: CarType, start: Start) -> VehicleId {
        self.add_vehicle(Dynamics::Car(Car::with_type(p)), start)
    }

    /// Stands a vehicle on the ground at (`n`, `e`), facing `heading` (a car on its wheels at a
    /// circuit's grid, say).
    pub fn stand(&mut self, id: VehicleId, n: f64, e: f64, heading: f64) {
        let (terrain, weather, earth) = (&*self.terrain, &self.weather, &self.earth);
        self.vehicles[id.0].reset_on_ground(terrain, weather, earth, n, e, heading);
    }

    /// Adds a vehicle and places it. Each one gets its own turbulence.
    pub fn add_vehicle(&mut self, dynamics: Dynamics, start: Start) -> VehicleId {
        let id = VehicleId(self.vehicles.len());
        let seed = self.seed.wrapping_add(99).wrapping_add((id.0 as u64).wrapping_mul(1013));
        self.vehicles.push(Vehicle::new(dynamics, seed));
        self.reset(id, start);
        id
    }

    /// Puts an aircraft in a state set by hand, its instruments settled there (`Vehicle::place`).
    /// Writing `aircraft_mut().state` directly leaves them chasing the jump: use this to move one.
    pub fn place(&mut self, id: VehicleId, state: State) {
        let (terrain, weather, earth) = (&*self.terrain, &self.weather, &self.earth);
        self.vehicles[id.0].place(terrain, weather, earth, state);
    }

    /// Places an existing vehicle somewhere new, as if it had just been added there.
    pub fn reset(&mut self, id: VehicleId, start: Start) {
        let (terrain, weather, earth) = (&*self.terrain, &self.weather, &self.earth);
        let vehicle = &mut self.vehicles[id.0];
        match start {
            // An aircraft that needs a long runway starts at the nearest aerodrome that has one.
            Start::Home => match terrain.nearest_runway(0.0, 0.0, vehicle.aircraft().map_or(0.0, |a| a.p.runway)) {
                Some(h) => {
                    // Take off into the wind.
                    let into = weather.wind_from * DEG;
                    let reversed = (into - h.heading).cos() < 0.0;
                    vehicle.reset_on_runway(terrain, weather, earth, &h, reversed);
                }
                None => vehicle.reset_in_air(terrain, weather, earth, 0.0, 0.0, 0.0),
            },
            Start::Runway { airport, reversed } => vehicle.reset_on_runway(terrain, weather, earth, &airport, reversed),
            Start::Air { n, e, heading } => vehicle.reset_in_air(terrain, weather, earth, n, e, heading),
            Start::Final { airport, reversed, distance } => vehicle.reset_on_final(terrain, weather, earth, &airport, reversed, distance),
        }
    }

    pub fn vehicle(&self, id: VehicleId) -> &Vehicle {
        &self.vehicles[id.0]
    }

    pub fn vehicle_mut(&mut self, id: VehicleId) -> &mut Vehicle {
        &mut self.vehicles[id.0]
    }

    /// Advances every vehicle by one fixed step of `World::DT`.
    pub fn step(&mut self) {
        self.slipstreams();
        for vehicle in &mut self.vehicles {
            vehicle.step_on(&self.terrain, &self.weather, &self.earth, &self.water);
            vehicle.run_into(&self.obstacles);
        }
        self.collide_vehicles();
        self.wet_circuits();
        // What has come off the vehicles this step is the world's now.
        let time = self.time;
        for vehicle in &mut self.vehicles {
            for mut wreck in vehicle.dynamics.take_wrecks() {
                wreck.time = time;
                if let Some(fire) = &mut wreck.fire {
                    fire.started = time;
                }
                self.wrecks.push(wreck);
            }
        }
        if self.wrecks.len() > World::MOST_WRECKS {
            self.wrecks.drain(..self.wrecks.len() - World::MOST_WRECKS);
        }
        let terrain = &self.terrain;
        for wreck in self.wrecks.iter_mut().filter(|w| w.active()) {
            // What falls comes down on the ground, the water, or a roof or bridge deck under it.
            wreck.step_on(World::DT, time, |n, e, up| {
                if let Some(top) = terrain.solid_under(n, e, up + 0.5) {
                    return (top, false);
                }
                let g = terrain.ground(n, e);
                if g.h <= 0.3 || g.river > 0.5 {
                    (g.h.max(0.0), true)
                } else {
                    (g.h, false)
                }
            });
        }
        self.time += World::DT;
    }

    /// Vehicles that touch each other: an aircraft is stopped or breaks up, and so does what it
    /// hits; two multirotors strike each other's propellers and knock their frames apart
    /// (`multirotor::collide_multirotors`).
    fn collide_vehicles(&mut self) {
        let n = self.vehicles.len();
        for i in 0..n {
            for j in i + 1..n {
                let (a, b) = (&self.vehicles[i].dynamics, &self.vehicles[j].dynamics);
                if a.gone() || b.gone() {
                    continue;
                }
                if (a.body().pos - b.body().pos).length() > 13.0 {
                    continue;
                }
                // Two multirotors: their propellers and frames (`multirotor::collide_multirotors`).
                if let (Dynamics::Multirotor(_), Dynamics::Multirotor(_)) = (a, b) {
                    let (low, high) = self.vehicles.split_at_mut(j);
                    if let (Dynamics::Multirotor(x), Dynamics::Multirotor(y)) = (&mut low[i].dynamics, &mut high[0].dynamics) {
                        collide_multirotors(x, y);
                    }
                    continue;
                }
                // Two cars: their bodies knock apart (`car::collide_cars`).
                if let (Dynamics::Car(_), Dynamics::Car(_)) = (a, b) {
                    let (low, high) = self.vehicles.split_at_mut(j);
                    if let (Dynamics::Car(x), Dynamics::Car(y)) = (&mut low[i].dynamics, &mut high[0].dynamics) {
                        collide_cars(x, y);
                    }
                    continue;
                }
                let hit = touching(a, b).or_else(|| touching(b, a));
                if let Some(speed) = hit {
                    for k in [i, j] {
                        self.vehicles[k].dynamics.collide("Collision with another aircraft", speed, 0.4);
                    }
                }
            }
        }
    }

    /// Lets a pilot move the controls of a vehicle: it observes, flies, and the result becomes
    /// the vehicle's controls for the next step.
    pub fn fly(&mut self, id: VehicleId, pilot: &mut dyn Pilot) {
        let obs = self.observe(id);
        let dynamics = &mut self.vehicles[id.0].dynamics;
        let controls = pilot.fly(&obs, &dynamics.controls(), World::DT);
        dynamics.set_controls(controls);
    }

    /// What the pilot of a vehicle can know right now.
    pub fn observe(&self, id: VehicleId) -> Observation {
        let vehicle = &self.vehicles[id.0];
        let s = vehicle.body();
        let e = euler_angles(s.q);
        let world_vel = s.world_velocity();

        let nearest_airport = self.terrain.nearest_airport(s.pos.x, s.pos.y).map(|ap| {
            let (dn, de) = (ap.n - s.pos.x, ap.e - s.pos.y);
            let mut bearing = de.atan2(dn);
            if bearing < 0.0 {
                bearing += 2.0 * PI;
            }
            AirportObservation {
                id: ap.id,
                north: ap.n,
                east: ap.e,
                heading: ap.heading,
                elevation: ap.elevation,
                length: ap.length,
                width: ap.width,
                distance: dn.hypot(de),
                bearing,
            }
        });
        let traffic = self
            .vehicles
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != id.0)
            .map(|(i, other)| {
                let o = other.body();
                Traffic { id: i, relative: (o.pos - s.pos).to_array(), velocity: o.world_velocity().to_array() }
            })
            .collect();

        Observation {
            time: self.time,
            heading: e.heading,
            pitch: e.pitch,
            roll: e.roll,
            angular_rates: s.omega.to_array(),
            velocity: world_vel.to_array(),
            speed: s.vel.length(),
            on_ground: vehicle.dynamics.on_ground(),
            agl: vehicle.agl(),
            crashed: vehicle.dynamics.crashed().map(str::to_string),
            north: s.pos.x,
            east: s.pos.y,
            altitude: -s.pos.z,
            ground_ahead: vehicle.ground_ahead(),
            nearest_airport,
            traffic,
            vehicle: match &vehicle.dynamics {
                Dynamics::Aircraft(a) => VehicleObservation::Aircraft(a.observe()),
                Dynamics::Multirotor(m) => VehicleObservation::Multirotor(m.observe()),
                Dynamics::Car(c) => VehicleObservation::Car(CarObservation { rain: self.weather.rain, water_ahead: self.water_ahead(&c.body), ..c.observe() }),
            },
        }
    }
}

/// Whether a point of `a` is inside the solid of `b`, and how fast it moves against it.
fn touching(a: &Dynamics, b: &Dynamics) -> Option<f64> {
    let s = b.body();
    let back = inverse(s.q);
    for point in a.contact_points() {
        let (p, v) = (point.at, point.vel);
        let r = act(back, p - s.pos) + b.cg();
        if b.solid().iter().any(|(lo, hi)| r.cmpge(*lo).all() && r.cmple(*hi).all()) {
            let other = act(s.q, s.vel + s.omega.cross(r - b.cg()));
            return Some((v - other).length());
        }
    }
    None
}
