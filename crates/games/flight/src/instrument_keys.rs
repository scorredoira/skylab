//! The instruments' knobs and switches at the keyboard, and the failures that can be tried by hand
//! until the failures of their own arrive: what each key does to the aircraft's
//! `sim_core::Instruments`, and the words that say it.
//!
//! | Key | Does |
//! |---|---|
//! | K, Shift+K | altimeter setting (and the G1000's BARO) up / down 1 hPa |
//! | Y | altimeter set to the QNH of the nearest aerodrome |
//! | J | heading indicator set to the compass |
//! | U, Shift+U | pitot heat; alternate static source |
//! | F5 F6 F7 F8 | try a failure: pitot (inlet, then sealed, then clear), static ports, vacuum pump, icing |

use engine::KeyCode;
use sim_core::instruments::{ICING_BELOW, SETTING_RANGE};
use sim_core::{clamp, GyroDrive, PitotBlock, Vehicle, HPA};

/// What a key does to the instruments of `plane`, and what to say about it; `None` if the key is not
/// one of theirs. `qnh` is the nearest aerodrome's, Pa.
pub fn press(plane: &mut Vehicle, key: KeyCode, shift: bool, qnh: f64) -> Option<String> {
    // Only an aircraft has these instruments.
    let aircraft = plane.aircraft_mut()?;
    let fit = aircraft.p.instruments;
    let attitude = aircraft.state.q;
    let oat = aircraft.instruments.readings().air_data.temperature;
    let i = &mut aircraft.instruments;
    let on = |b: bool| if b { "on" } else { "off" };
    let text = match key {
        KeyCode::KeyK => {
            let step = if shift { -HPA } else { HPA };
            i.altimeter_setting = clamp((i.altimeter_setting / HPA).round() * HPA + step, SETTING_RANGE.0, SETTING_RANGE.1);
            format!("Altimeter: {:.0} hPa", i.altimeter_setting / HPA)
        }
        KeyCode::KeyY => {
            // A QNH is given in whole hectopascals, rounded down.
            i.altimeter_setting = clamp((qnh / HPA).floor() * HPA, SETTING_RANGE.0, SETTING_RANGE.1);
            format!("Altimeter to the aerodrome's QNH: {:.0} hPa", i.altimeter_setting / HPA)
        }
        KeyCode::KeyJ if fit.heading == GyroDrive::None => "This aircraft has no heading gyro".to_string(),
        KeyCode::KeyJ => {
            i.align_heading_indicator(attitude);
            "Heading indicator set to the compass".to_string()
        }
        KeyCode::KeyU if shift => match fit.alternate_static {
            None => "This aircraft has no alternate static source".to_string(),
            Some(_) => {
                i.alternate_static = !i.alternate_static;
                format!("Alternate static: {}", if i.alternate_static { "open (cabin pressure)" } else { "closed" })
            }
        },
        KeyCode::KeyU if !fit.pitot_heat => "This aircraft has no pitot heat".to_string(),
        KeyCode::KeyU => {
            i.pitot_heat = !i.pitot_heat;
            format!("Pitot heat: {}", on(i.pitot_heat))
        }
        KeyCode::F5 => {
            i.failures.pitot = match i.failures.pitot {
                PitotBlock::Clear => PitotBlock::Inlet,
                PitotBlock::Inlet => PitotBlock::Sealed,
                PitotBlock::Sealed => PitotBlock::Clear,
            };
            format!("Failure test: {}", match i.failures.pitot {
                PitotBlock::Clear => "pitot clear",
                PitotBlock::Inlet => "pitot blocked at the inlet (the drain empties it)",
                PitotBlock::Sealed => "pitot fully blocked (ice in the inlet and the drain)",
            })
        }
        KeyCode::F6 => {
            i.failures.static_blocked = !i.failures.static_blocked;
            format!("Failure test: static ports {}", if i.failures.static_blocked { "blocked" } else { "clear" })
        }
        KeyCode::F7 if fit.attitude != GyroDrive::Vacuum && fit.heading != GyroDrive::Vacuum => "This aircraft has no vacuum system".to_string(),
        KeyCode::F7 => {
            i.failures.vacuum_pump = !i.failures.vacuum_pump;
            format!("Failure test: vacuum pump {}", if i.failures.vacuum_pump { "failed" } else { "working" })
        }
        KeyCode::F8 => {
            i.failures.icing = !i.failures.icing;
            let warm = if i.failures.icing && oat >= ICING_BELOW { format!(" (at {:.0} °C no ice forms)", oat - 273.15) } else { String::new() };
            format!("Test: icing {}{warm}", if i.failures.icing { "in cloud" } else { "over" })
        }
        _ => return None,
    };
    Some(text)
}

/// Breaks what `name` says (pitot, inlet, static, vacuum, icing) on `plane`, for a screenshot;
/// false if it is none of those.
pub fn fail(plane: &mut Vehicle, name: &str) -> bool {
    let Some(aircraft) = plane.aircraft_mut() else { return false };
    let f = &mut aircraft.instruments.failures;
    match name {
        "pitot" => f.pitot = PitotBlock::Sealed,
        "inlet" => f.pitot = PitotBlock::Inlet,
        "static" => f.static_blocked = true,
        "vacuum" => f.vacuum_pump = true,
        "icing" => f.icing = true,
        _ => return false,
    }
    true
}

/// The switches that are on and the failures being tried, for the status line; empty if none.
pub fn status(plane: &Vehicle) -> String {
    let Some(aircraft) = plane.aircraft() else { return String::new() };
    let (i, fit) = (&aircraft.instruments, &aircraft.p.instruments);
    let mut parts = Vec::new();
    if i.pitot_heat && fit.pitot_heat {
        parts.push("pitot heat".to_string());
    }
    if i.alternate_static && fit.alternate_static.is_some() {
        parts.push("alternate static".to_string());
    }
    let f = i.failures;
    let mut failed = Vec::new();
    match f.pitot {
        PitotBlock::Clear => {}
        PitotBlock::Inlet => failed.push("pitot (inlet)"),
        PitotBlock::Sealed => failed.push("pitot"),
    }
    if f.static_blocked {
        failed.push("static");
    }
    if f.vacuum_pump {
        failed.push("vacuum");
    }
    if f.icing {
        failed.push("icing");
    }
    if i.pitot_ice() > 0.0 {
        parts.push(format!("pitot ice {:.0} %", i.pitot_ice() * 100.0));
    }
    if !failed.is_empty() {
        parts.push(format!("FAILED: {}", failed.join(", ")));
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{AircraftKind, Start, World};

    #[test]
    fn the_keys_set_the_knobs_and_switches_and_try_failures() {
        let mut world = World::new(1);
        let id = world.add_aircraft(AircraftKind::C172, Start::Home);
        let plane = world.vehicle_mut(id);
        let start = plane.aircraft_mut().unwrap().instruments.altimeter_setting;
        press(plane, KeyCode::KeyK, false, 0.0);
        press(plane, KeyCode::KeyK, false, 0.0);
        press(plane, KeyCode::KeyK, true, 0.0);
        assert!((plane.aircraft_mut().unwrap().instruments.altimeter_setting - ((start / HPA).round() + 1.0) * HPA).abs() < 1e-9);
        let said = press(plane, KeyCode::KeyY, false, 101_799.0).unwrap();
        assert_eq!(plane.aircraft_mut().unwrap().instruments.altimeter_setting, 101_700.0);
        assert!(said.contains("1017 hPa"), "{said}");
        press(plane, KeyCode::KeyU, false, 0.0);
        press(plane, KeyCode::KeyU, true, 0.0);
        assert!(plane.aircraft_mut().unwrap().instruments.pitot_heat && plane.aircraft_mut().unwrap().instruments.alternate_static);
        for (key, n) in [(KeyCode::F5, 2), (KeyCode::F6, 1), (KeyCode::F7, 1), (KeyCode::F8, 1)] {
            for _ in 0..n {
                press(plane, key, false, 0.0);
            }
        }
        let f = plane.aircraft_mut().unwrap().instruments.failures;
        assert!(f.pitot == PitotBlock::Sealed && f.static_blocked && f.vacuum_pump && f.icing);
        let line = status(plane);
        assert!(line.contains("pitot heat") && line.contains("FAILED: pitot, static, vacuum, icing"), "{line}");
        assert_eq!(press(plane, KeyCode::KeyW, false, 0.0), None);
        // Placed again, it is all as after taxiing out.
        world.reset(id, Start::Home);
        assert_eq!(status(world.vehicle(id)), "");
        // The Extra has neither a vacuum system nor a heading gyro, nor pitot heat.
        let x = world.add_aircraft(AircraftKind::EXTRA300, Start::Home);
        let extra = world.vehicle_mut(x);
        assert!(press(extra, KeyCode::F7, false, 0.0).unwrap().contains("has no"));
        assert!(press(extra, KeyCode::KeyJ, false, 0.0).unwrap().contains("has no"));
        assert!(press(extra, KeyCode::KeyU, false, 0.0).unwrap().contains("has no"));
        assert!(!extra.aircraft_mut().unwrap().instruments.failures.vacuum_pump && !extra.aircraft_mut().unwrap().instruments.pitot_heat);
    }
}
