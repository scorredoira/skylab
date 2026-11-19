//! Renders the instruments alone to a PNG, to check them by eye: the G1000's pictures as the C172's
//! cockpit shows them on its displays (PFD, MFD, the three standby dials), and under them the row
//! of dials of the outside view (with `extra`, the Extra 300L's: its airspeed marks, no flaps, the
//! accelerometer).
//! `cargo run -q -p flight --example panel -- out.png [ground|air] [extra] [key=value ...]`
//! Keys: kt, ft, fpm, pitch, roll, heading, compass (degrees), turn (degrees/s), slip (degrees), rpm,
//! flaps, trim, throttle, stall (0 or 1), g, gmin, gmax, hpa (the altimeter setting), suction (inHg;
//! under 3 the PFD says LOW VACUUM). They override the chosen state; the PFD shows the same as the
//! standby dials. `afcs=<n>` adds the GFC 700 in HDG and VS with GPS and ALTS armed, the
//! altitude alerter 0 silent, 1 approaching, 2 within, 3 deviating.

use engine::{egui, wgpu, Game, Gpu, Input};

#[path = "../src/g1000/display.rs"]
mod g1000;
#[allow(dead_code)]
#[path = "../src/ui/panel.rs"]
mod panel;
// `g1000` takes the dials from `crate::ui::panel`.
mod ui {
    pub(crate) use super::panel;
}

struct Shot {
    data: g1000::G1000Data,
}

impl Game for Shot {
    fn update(&mut self, _input: &Input, _dt: f64, _gpu: &Gpu) {}

    fn ui(&mut self, ctx: &egui::Context) {
        let (painter, screen) = (ctx.layer_painter(egui::LayerId::background()), ctx.content_rect());
        // As the C172's cockpit paints its canvas: the PFD and the MFD side by side, the standby dials under them.
        let rect = |x: f32, y: f32, w: f32, h: f32| egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h));
        g1000::draw_pfd(&painter.with_clip_rect(rect(0.0, 0.0, 512.0, 384.0)), rect(0.0, 0.0, 512.0, 384.0), &self.data);
        g1000::draw_mfd(&painter.with_clip_rect(rect(512.0, 0.0, 512.0, 384.0)), rect(512.0, 0.0, 512.0, 384.0), &self.data);
        let d = &self.data.panel;
        for k in 0..3 {
            let r = rect(k as f32 * 128.0, 384.0, 128.0, 128.0);
            let p = painter.with_clip_rect(r);
            p.rect_filled(r, 0.0, egui::Color32::from_gray(20));
            match k {
                0 => panel::airspeed(&p, r, d.airspeed_kt, &d.airframe),
                1 => panel::attitude(&p, r, d.pitch, d.roll),
                _ => panel::altimeter(&p, r, d.altitude_ft, d.altimeter_hpa),
            }
        }
        panel::draw_strip(&painter, screen, &self.data.panel);
    }

    fn draw(&mut self, _gpu: &Gpu, _pass: &mut wgpu::RenderPass<'_>) {}
}

/// On the ground, the engine stopped: the C172's gauges at rest.
fn still() -> panel::PanelData {
    panel::PanelData { heading: Some(0.0), altimeter_hpa: 1013.0, suction_inhg: Some(0.0), ..Default::default() }
}

fn flying() -> panel::PanelData {
    panel::PanelData {
        altimeter_hpa: 1013.0,
        suction_inhg: Some(5.0),
        airspeed_kt: 95.0,
        altitude_ft: 2300.0,
        vertical_speed_fpm: 500.0,
        pitch: 5.0_f32.to_radians(),
        roll: 20.0_f32.to_radians(),
        heading: Some(165.0_f32.to_radians()),
        compass: 160.0_f32.to_radians(),
        turn_rate: 3.0_f32.to_radians(),
        slip: 4.5_f32.to_radians(),
        rpm: 2400.0,
        flap_deg: 10.0,
        trim: 0.3,
        throttle: 0.8,
        stall_warning: true,
        ..Default::default()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: panel <out.png> [ground|air] [extra] [key=value ...]");
        return;
    };
    let mut data = still();
    let mut afcs = None;
    for arg in &args[1..] {
        match arg.split_once('=') {
            None if arg == "air" => data = flying(),
            None if arg == "ground" => data = still(),
            None if arg == "extra" => {
                data.airframe = panel::Airframe::of(sim_core::AircraftKind::EXTRA300.spec());
                (data.heading, data.suction_inhg) = (None, None);
            }
            Some((key, value)) => {
                let Ok(v) = value.parse::<f32>() else {
                    eprintln!("not a number: {arg}");
                    return;
                };
                match key {
                    "kt" => data.airspeed_kt = v,
                    "ft" => data.altitude_ft = v,
                    "fpm" => data.vertical_speed_fpm = v,
                    "pitch" => data.pitch = v.to_radians(),
                    "roll" => data.roll = v.to_radians(),
                    "heading" => data.heading = Some(v.to_radians()),
                    "compass" => data.compass = v.to_radians(),
                    "hpa" => data.altimeter_hpa = v,
                    "suction" => data.suction_inhg = Some(v),
                    "turn" => data.turn_rate = v.to_radians(),
                    "slip" => data.slip = v.to_radians(),
                    "rpm" => data.rpm = v,
                    "flaps" => data.flap_deg = v,
                    "trim" => data.trim = v,
                    "throttle" => data.throttle = v,
                    "stall" => data.stall_warning = v != 0.0,
                    "g" => data.load_factor = v,
                    "gmax" => data.g_felt.1 = v,
                    "gmin" => data.g_felt.0 = v,
                    "afcs" => afcs = Some(v as u8),
                    _ => {
                        eprintln!("unknown key: {key}");
                        return;
                    }
                }
            }
            None => {
                eprintln!("unknown argument: {arg}");
                return;
            }
        }
    }
    let pfd = g1000::Pfd {
        airspeed_kt: data.airspeed_kt,
        altitude_ft: data.altitude_ft,
        vertical_speed_fpm: data.vertical_speed_fpm,
        pitch: data.pitch,
        roll: data.roll,
        heading: data.heading.unwrap_or(data.compass),
        baro_hpa: data.altimeter_hpa,
        low_vacuum: data.suction_inhg.is_some_and(|s| s < 3.0),
    };
    let suction_inhg = data.suction_inhg.unwrap_or(0.0);
    let data = g1000::G1000Data { panel: data, pfd, suction_inhg, ground_speed_kt: data.airspeed_kt, track: pfd.heading, true_airspeed_kt: data.airspeed_kt * 1.05, wind_from: 250f32.to_radians(), wind_kt: 8.0, oat_c: 9.0, hour: 10.5, fuel_gal: 40.0, fuel_flow_gph: 8.5, nearest: Some((16, 4.2, 2.6)), map: g1000::MapData::default(), afcs: afcs.map(|alert| sample_afcs(&pfd, alert)) };
    engine::render_to_png(path, 2048, 1400, 2.0, |_| Shot { data }, |_, frames| frames >= 2);
}

/// The GFC 700 flying HDG and VS, GPS and ALTS armed, the command bars a little off the attitude.
fn sample_afcs(pfd: &g1000::Pfd, alert: u8) -> g1000::Afcs {
    g1000::Afcs {
        modes: ["HDG", "GPS", "VS", "ALTS"],
        reference: "500FPM".to_string(),
        ap: true,
        alert: None,
        flash: [false, false],
        blink: true,
        command: Some((pfd.pitch + 2.0_f32.to_radians(), pfd.roll - 8.0_f32.to_radians())),
        heading_bug: pfd.heading + 25.0_f32.to_radians(),
        selected_ft: 3000.0,
        alerter: match alert {
            1 => g1000::AltitudeAlert::Approaching,
            2 => g1000::AltitudeAlert::Within,
            3 => g1000::AltitudeAlert::Deviation,
            _ => g1000::AltitudeAlert::None,
        },
        airspeed_ref_kt: None,
        vertical_speed_ref_fpm: Some(500.0),
        protection: None,
        leg: Some(g1000::LegData { name: "D\u{2192} RW16".to_string(), course: 0.6, to_go_nm: 4.2, across_nm: 0.4, scale_nm: 2.0 }),
        legs: Vec::new(),
        glide_ft: None,
    }
}
