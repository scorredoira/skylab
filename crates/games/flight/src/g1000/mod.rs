//! A Garmin G1000 with its GFC 700, an instrument a cockpit's file can fit (`cockpit::instruments`:
//! its PFD and MFD): what its displays show (`display`), and here what feeds them from the
//! simulation.

pub mod display;

use crate::ui::panel::PanelData;
use display::{Afcs, AltitudeAlert, G1000Data, LegData, MapData, Pfd};
use engine::egui::vec2;
use sim_core::{act, Aircraft, Gfc700, VerticalMode, World, FEET, HPA, INHG, KNOTS};

/// Kilograms in a gallon of avgas.
const KG_PER_GALLON: f64 = 2.72;
/// What the moving map shows round the aircraft, metres.
const MAP_REACH: f64 = 25_000.0;
/// The narrowest road of a real place the map draws, metres: the secondary roads and wider.
const MAIN_ROAD: f64 = 9.0;

/// What the G1000 shows that changes slowly: the map round the aircraft, the nearest aerodrome.
#[derive(Default)]
pub struct Instruments {
    map: MapData,
    /// Where the map was made from, and frames until the next look.
    around: (f64, f64),
    refresh: u32,
    nearest: Option<sim_core::Airport>,
}

impl Instruments {
    /// Everything the displays show, from the player's aircraft and its GFC 700 if it has one.
    pub fn shown(&mut self, world: &World, plane: &Aircraft, panel: PanelData, gfc: Option<&Gfc700>) -> G1000Data {
        let mut data = self.data(world, plane, panel);
        data.afcs = gfc.map(|gfc| afcs(gfc, plane, world.time()));
        data
    }

    fn data(&mut self, world: &World, a: &Aircraft, panel: PanelData) -> G1000Data {
        let s = a.state;
        if self.refresh == 0 {
            self.refresh = 60;
            self.look_around(world, s.pos.x, s.pos.y);
        }
        self.refresh -= 1;
        let ground = act(s.q, s.vel);
        let (dn, de) = (self.around.0 - s.pos.x, self.around.1 - s.pos.y);
        let shift = vec2(dn as f32, de as f32);
        let mut map = self.map.clone();
        for item in &mut map.airports {
            item.0 += shift;
        }
        for item in &mut map.towns {
            item.0 += shift;
        }
        for item in &mut map.roads {
            item.0 += shift;
            item.1 += shift;
        }
        let w = &world.weather;
        let r = a.instruments.readings();
        let air = &r.air_data;
        G1000Data {
            panel,
            pfd: Pfd {
                airspeed_kt: (air.airspeed / KNOTS) as f32,
                altitude_ft: (air.altitude / FEET) as f32,
                vertical_speed_fpm: (air.vertical_speed / FEET * 60.0) as f32,
                pitch: r.ahrs[0] as f32,
                roll: r.ahrs[1] as f32,
                heading: r.ahrs[2] as f32,
                baro_hpa: (r.altimeter_setting / HPA) as f32,
                low_vacuum: r.low_vacuum,
            },
            ground_speed_kt: (ground.x.hypot(ground.y) / KNOTS) as f32,
            track: ground.y.atan2(ground.x) as f32,
            true_airspeed_kt: (air.true_airspeed / KNOTS) as f32,
            wind_from: (w.wind_from as f32).to_radians(),
            wind_kt: (w.wind_speed / KNOTS) as f32,
            oat_c: (air.temperature - 273.15) as f32,
            hour: world.hour as f32,
            fuel_gal: (a.load().fuel / KG_PER_GALLON) as f32,
            fuel_flow_gph: (a.data().fuel_flow * 3600.0 / KG_PER_GALLON) as f32,
            suction_inhg: (r.suction.unwrap_or(0.0) / INHG) as f32,
            nearest: self.nearest.map(|n| {
                let (dn, de) = (n.n - s.pos.x, n.e - s.pos.y);
                (n.number(), (dn.hypot(de) / 1852.0) as f32, de.atan2(dn) as f32)
            }),
            map,
            afcs: None,
        }
    }

    fn look_around(&mut self, world: &World, n: f64, e: f64) {
        let t = &world.terrain;
        let (n0, n1, e0, e1) = (n - MAP_REACH, n + MAP_REACH, e - MAP_REACH, e + MAP_REACH);
        let rel = |pn: f64, pe: f64| vec2((pn - n) as f32, (pe - e) as f32);
        self.map = MapData {
            airports: t.airports(n0, n1, e0, e1).iter().map(|a| (rel(a.n, a.e), a.heading as f32, a.length as f32, a.number())).collect(),
            towns: t.towns(n0, n1, e0, e1).iter().map(|town| (rel(town.n, town.e), town.radius as f32)).collect(),
            // A real place's map has tens of thousands of stretches of road: the main ones only.
            roads: t.roads(n0, n1, e0, e1).iter().filter(|r| t.region().is_none() || r.width >= MAIN_ROAD).map(|r| (rel(r.n0, r.e0), rel(r.n1, r.e1))).collect(),
        };
        self.around = (n, e);
        self.nearest = t.nearest_airport(n, e);
    }
}


/// Metres in a nautical mile.
const NM: f64 = 1852.0;

/// What the PFD and the MFD show of the GFC 700 and its flight plan.
fn afcs(gfc: &Gfc700, plane: &Aircraft, time: f64) -> Afcs {
    let s = plane.state;
    let (n, e) = (s.pos.x, s.pos.y);
    let reference = match gfc.vertical() {
        _ if !gfc.director() => String::new(),
        VerticalMode::Altitude(a) => format!("{:.0}FT", a / FEET),
        VerticalMode::Capture => format!("{:.0}FT", gfc.selected_altitude() / FEET),
        VerticalMode::VerticalSpeed(v) => format!("{:.0}FPM", v / FEET * 60.0),
        VerticalMode::Flc(v) => format!("{:.0}KT", v / KNOTS),
        VerticalMode::Pitch(_) | VerticalMode::GlidePath => String::new(),
    };
    let (leg, legs) = match gfc.plan() {
        Some(plan) => {
            let leg = plan.active_leg();
            let (across, _) = leg.place(n, e);
            // The distance to the point the leg goes to, as the G1000 gives it.
            let to_go = (leg.to.0 - n).hypot(leg.to.1 - e);
            let last = plan.active + 1 == plan.legs.len();
            let name = match (plan.approach, plan.active) {
                (None, _) | (Some(_), 0) => format!("D\u{2192} {}", leg.name),
                (Some(_), k) => format!("{} \u{2192} {}", plan.legs[k - 1].name, leg.name),
            };
            // The CDI's full scale: en route, in the terminal area, on the final approach.
            let scale = match (plan.approach, last) {
                (Some(_), true) => 0.3,
                (Some(_), false) => 1.0,
                (None, _) => 2.0,
            };
            let rel = |(pn, pe): (f64, f64)| vec2((pn - n) as f32, (pe - e) as f32);
            let legs = plan.legs.iter().enumerate().skip(plan.active).map(|(k, l)| (rel(l.from), rel(l.to), k == plan.active)).collect();
            (Some(LegData { name, course: leg.course() as f32, to_go_nm: (to_go / NM) as f32, across_nm: (across / NM) as f32, scale_nm: scale }), legs)
        }
        None => (None, Vec::new()),
    };
    Afcs {
        modes: gfc.annunciations(),
        reference,
        ap: gfc.engaged(),
        alert: gfc.alert().map(|a| a.why.abnormal()),
        flash: [gfc.flashing(false), gfc.flashing(true)],
        blink: time.rem_euclid(1.0) < 0.5,
        command: gfc.command().map(|(pitch, bank)| (pitch as f32, bank as f32)),
        heading_bug: gfc.heading_bug() as f32,
        selected_ft: (gfc.selected_altitude() / FEET) as f32,
        alerter: match gfc.altitude_alert() {
            sim_core::AltitudeAlert::None => AltitudeAlert::None,
            sim_core::AltitudeAlert::Approaching => AltitudeAlert::Approaching,
            sim_core::AltitudeAlert::Within => AltitudeAlert::Within,
            sim_core::AltitudeAlert::Deviation => AltitudeAlert::Deviation,
        },
        airspeed_ref_kt: match gfc.vertical() {
            VerticalMode::Flc(v) if gfc.director() => Some((v / KNOTS) as f32),
            _ => None,
        },
        vertical_speed_ref_fpm: match gfc.vertical() {
            VerticalMode::VerticalSpeed(v) if gfc.director() => Some((v / FEET * 60.0) as f32),
            _ => None,
        },
        protection: gfc.protection().filter(|_| gfc.director()),
        leg,
        legs,
        glide_ft: gfc.glide_deviation().filter(|_| gfc.approach_mode()).map(|d| (d / FEET) as f32),
    }
}
