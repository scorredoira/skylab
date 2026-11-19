//! The instruments a cockpit's file can fit, each a kind of picture painted into the cockpit's
//! instruments' canvas where the file says (`file::Fitted`); the cockpit's mesh shows the canvas
//! on its displays and dials (the `screen` and `dial` materials, by their texture coordinates).
//! The kinds are the game's: a G1000's PFD and MFD (`g1000`), the round dials of `ui::panel`, a
//! magnetic compass's card and its deviation card, a radio's display.

use crate::g1000;
use crate::ui::panel::{self, PanelData};
use engine::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect};
use serde::{Deserialize, Serialize};
use sim_core::{Gfc700, World};

/// A kind of instrument.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Instrument {
    /// The G1000's primary flight display and its multi-function display (512 x 384 points each,
    /// as drawn: they scale).
    Pfd,
    Mfd,
    /// Round dials, square.
    Airspeed,
    Attitude,
    Altimeter,
    VerticalSpeed,
    TurnCoordinator,
    Tachometer,
    /// An accelerometer, its limits the type's (`Airframe::g_limits`).
    Accelerometer,
    /// A magnetic compass's card round its drum (128 x 80 points), and its deviation card.
    CompassCard,
    DeviationCard,
    /// A radio's display: its active and standby frequencies.
    Radio { active: String, standby: String },
    /// A heading indicator, round.
    Heading,
    /// A turbine tilt-rotor's (`Rotorcraft`): the engines' torque and the rotors' speed, round
    /// gauges of a share; the fuel left; where the nacelles are; and a row of lamps (the engines
    /// stopped, the rotors slow, the fuel low, and the flight controller's help).
    Torque,
    Rotors,
    Fuel,
    Nacelles,
    Annunciators,
    /// Its displays (`glass`): the flight display, the systems' and the situation's.
    FlightDisplay,
    SystemsDisplay,
    SituationDisplay,
    /// A jet engine's fan speed, N1, a round gauge of a share: the engine's place among the
    /// aircraft's (`sim_core::Jets::mounts`); its exhaust's temperature, tens of °C, and the fuel
    /// it burns, hundreds of kg an hour.
    FanSpeed { engine: usize },
    ExhaustTemperature { engine: usize },
    FuelFlow { engine: usize },
    /// Where the flaps are, degrees, and the gear's lamps: a green one for each leg down and
    /// locked, a red one while it moves.
    FlapPosition,
    GearLamps,
}

impl Instrument {
    /// Whether it shows the G1000's picture.
    fn g1000(&self) -> bool {
        matches!(self, Instrument::Pfd | Instrument::Mfd)
    }
}

/// An instrument fitted: where it is in the canvas (left, top, width, height, points), whether
/// what it paints is cut to that, and what it is painted on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fitted {
    pub kind: Instrument,
    pub at: [f32; 4],
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub clip: bool,
    /// RGBA, 8 bits, premultiplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<[u8; 4]>,
}

/// What a cockpit's instruments show: the world and the player's aircraft, what its dials read, its
/// GFC 700 if it has one.
pub struct Shown<'a> {
    pub world: &'a World,
    /// The aircraft, if it is one (a G1000 is an aircraft's).
    pub plane: Option<&'a sim_core::Aircraft>,
    pub panel: PanelData,
    pub gfc: Option<&'a Gfc700>,
    /// A turbine tilt-rotor's engines, rotors and nacelles, if it is one.
    pub rotorcraft: Option<Rotorcraft>,
}

/// What a turbine tilt-rotor's own instruments read: plain numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rotorcraft {
    /// The engines' torque and the rotors' speed, 1 all of each; whether the engines run.
    pub torque: f32,
    pub rotors: f32,
    pub engines: bool,
    /// The fuel left over what it carries full.
    pub fuel: f32,
    /// The nacelles, degrees forward of upright, and how far they go back and forward.
    pub nacelles: f32,
    pub nacelles_back: f32,
    pub nacelles_forward: f32,
    /// The flight controller's help: 0 rates, 1 level, 2 none, 3 hold.
    pub stability: u8,
    /// The fuel left, kg, and how long it lasts as it is burnt now, minutes.
    pub fuel_kg: f32,
    pub endurance_min: f32,
    /// Its height over the ground, feet; its speed over it, knots, and the way it goes, radians.
    pub agl_ft: f32,
    pub ground_speed_kt: f32,
    pub track: f32,
    /// The aerodrome it started from: its bearing (radians), how far (m) and its runway's heading.
    pub home: Option<(f32, f32, f32)>,
}

/// Paints `fitted` into `canvas`, from what they show (`g1000` keeps what a G1000 needs between frames).
pub fn paint(fitted: &[Fitted], gpu: &engine::Gpu, canvas: &mut engine::Canvas, g1000: &mut g1000::Instruments, shown: Shown) {
    let data = shown.plane.filter(|_| fitted.iter().any(|f| f.kind.g1000())).map(|plane| g1000.shown(shown.world, plane, shown.panel, shown.gfc));
    let d = &shown.panel;
    let rotorcraft = shown.rotorcraft.unwrap_or_default();
    canvas.paint(gpu, [0.0, 0.0, 0.0, 1.0], |ctx| {
        let p = ctx.layer_painter(egui::LayerId::background());
        for f in fitted {
            let [x, y, w, h] = f.at;
            let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
            let p = if f.clip { p.with_clip_rect(r) } else { p.clone() };
            if let Some([red, green, blue, alpha]) = f.fill {
                p.rect_filled(r, 0.0, Color32::from_rgba_premultiplied(red, green, blue, alpha));
            }
            match &f.kind {
                // A G1000 in what is not an aircraft stays dark.
                Instrument::Pfd | Instrument::Mfd if data.is_none() => {}
                Instrument::Pfd => g1000::display::draw_pfd(&p, r, data.as_ref().expect("made for it")),
                Instrument::Mfd => g1000::display::draw_mfd(&p, r, data.as_ref().expect("made for it")),
                Instrument::Heading => panel::heading_dial(&p, r, d.heading.unwrap_or(d.compass)),
                Instrument::Torque => panel::share_gauge(&p, r, rotorcraft.torque, 1.2, 0.2, (0.0, 0.9), 1.0, "TORQUE %"),
                Instrument::Rotors => panel::share_gauge(&p, r, rotorcraft.rotors, 1.2, 0.2, (0.95, 1.03), 1.1, "ROTORS %"),
                Instrument::Fuel => panel::share_gauge(&p, r, rotorcraft.fuel, 1.0, 0.25, (0.1, 1.0), 0.1, "FUEL %"),
                Instrument::Nacelles => panel::nacelle_gauge(&p, r, rotorcraft.nacelles, rotorcraft.nacelles_back, rotorcraft.nacelles_forward),
                Instrument::FlightDisplay => super::glass::flight(&p, r, d, &rotorcraft),
                Instrument::SystemsDisplay => super::glass::systems(&p, r, d, &rotorcraft),
                Instrument::SituationDisplay => super::glass::situation(&p, r, d, &rotorcraft),
                Instrument::Annunciators => panel::annunciators(
                    &p,
                    r,
                    &[
                        ("ENG OUT", !rotorcraft.engines, panel::LAMP_RED),
                        ("LOW ROTOR", rotorcraft.rotors < 0.9, panel::LAMP_RED),
                        ("LOW FUEL", rotorcraft.fuel < 0.1, panel::LAMP_AMBER),
                        ("SAS HOLD", rotorcraft.stability == 3, panel::LAMP_GREEN),
                        ("SAS RATES", rotorcraft.stability == 0, panel::LAMP_GREEN),
                        ("SAS LEVEL", rotorcraft.stability == 1, panel::LAMP_GREEN),
                        ("SAS OFF", rotorcraft.stability == 2, panel::LAMP_AMBER),
                    ],
                ),
                Instrument::Airspeed => panel::airspeed(&p, r, d.airspeed_kt, &d.airframe),
                Instrument::Attitude => panel::attitude(&p, r, d.pitch, d.roll),
                Instrument::Altimeter => panel::altimeter(&p, r, d.altitude_ft, d.altimeter_hpa),
                Instrument::VerticalSpeed => panel::vertical_speed(&p, r, d.vertical_speed_fpm),
                Instrument::TurnCoordinator => panel::turn_coordinator(&p, r, d.turn_rate, d.slip),
                Instrument::Tachometer => panel::tachometer(&p, r, d.rpm),
                Instrument::Accelerometer => panel::g_meter(&p, r, d),
                Instrument::CompassCard => panel::compass_card(&p, r, d.compass, panel::COMPASS_ARC),
                Instrument::DeviationCard => panel::deviation_card(&p, r),
                Instrument::Radio { active, standby } => radio(&p, r, active, standby),
                Instrument::FanSpeed { engine } => {
                    let (n1, idle, redline) = shown.plane.and_then(|a| a.p.jets.as_ref().map(|jets| (a.state.n1.get(*engine).copied().unwrap_or(0.0) as f32, jets.idle as f32, jets.redline as f32))).unwrap_or((0.0, 0.25, 1.0));
                    panel::share_gauge(&p, r, n1, 1.2, 0.2, (idle, 1.0), redline, &format!("N1 % {}", engine + 1));
                }
                Instrument::ExhaustTemperature { engine } => {
                    let celsius = jet(&shown, *engine).map_or(0.0, |(n1, burning, _)| if burning { 320.0 + 560.0 * n1.powf(1.6) } else { 60.0 * n1 });
                    panel::share_gauge(&p, r, celsius / 1000.0, 1.0, 0.2, (0.3, 0.87), 0.92, "EGT °C ×10");
                }
                Instrument::FuelFlow { engine } => {
                    let flow = jet(&shown, *engine).map_or(0.0, |(_, _, flow)| flow);
                    panel::share_gauge(&p, r, flow * 3600.0 / 10_000.0, 1.0, 0.2, (0.0, 0.8), 0.9, "FF kg/h ×100");
                }
                Instrument::FlapPosition => panel::share_gauge(&p, r, d.flap_deg / 100.0, 0.3, 0.05, (0.0, 0.3), 0.3, "FLAPS"),
                Instrument::GearLamps => {
                    let gear = shown.plane.map_or(1.0, |a| a.gear);
                    let (down, moving) = (gear >= 1.0, gear > 0.0 && gear < 1.0);
                    panel::annunciators(&p, r, &[("L", down, panel::LAMP_GREEN), ("NOSE", down, panel::LAMP_GREEN), ("R", down, panel::LAMP_GREEN), ("GEAR", moving, panel::LAMP_RED)]);
                }
            }
        }
    });
}

/// A jet engine of the aircraft shown: its fan speed, whether it burns, and the fuel it burns,
/// kg/s (what the aircraft burns, shared among those that burn by their fan speeds).
fn jet(shown: &Shown, engine: usize) -> Option<(f32, bool, f32)> {
    let a = shown.plane?;
    let jets = a.p.jets.as_ref()?;
    let n1 = *a.state.n1.get(engine)?;
    let burning = |i: usize| !a.damage().engines_failed[i] && a.load().fuel > 0.0;
    let all: f64 = (0..jets.mounts.len()).filter(|&i| burning(i)).map(|i| a.state.n1[i].powi(3)).sum();
    let flow = if burning(engine) && all > 0.0 { a.data().fuel_flow * n1.powi(3) / all } else { 0.0 };
    Some((n1 as f32, burning(engine), flow as f32))
}

/// A radio's display: the active and standby frequencies, green on black.
fn radio(p: &egui::Painter, r: Rect, active: &str, standby: &str) {
    p.rect_filled(r, 0.0, Color32::from_rgb(4, 10, 6));
    let green = Color32::from_rgb(90, 230, 120);
    let h = r.height();
    p.text(r.left_center() + vec2(h * 0.2, h * 0.06), Align2::LEFT_CENTER, active, FontId::monospace(h * 0.5), green);
    p.text(r.right_center() + vec2(-h * 0.2, h * 0.1), Align2::RIGHT_CENTER, standby, FontId::monospace(h * 0.34), green.gamma_multiply(0.7));
    p.text(r.left_top() + vec2(h * 0.2, h * 0.05), Align2::LEFT_TOP, "COM", FontId::proportional(h * 0.2), green.gamma_multiply(0.6));
    p.text(r.right_top() + vec2(-h * 0.2, h * 0.05), Align2::RIGHT_TOP, "STBY", FontId::proportional(h * 0.2), green.gamma_multiply(0.6));
}
