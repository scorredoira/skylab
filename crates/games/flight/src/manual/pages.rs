//! What the manual says: its pages, each a few short blocks, and the marks over its pictures.
//!
//! A mark's box and where its label goes are fractions of its picture (0 left or top, 1 right or
//! bottom), so they stay where they belong at any size; the pictures are the game's own
//! (`scripts/manual-shots.sh` takes them again), built into the program.

use super::inputs::{key, knob, Act};
use crate::cockpit::{Panel, PanelKnob};
use sim_core::AfcsKey;

/// A picture of the game, a JPEG of `assets/manual`.
#[derive(Clone, Copy, Debug)]
pub struct Image {
    pub name: &'static str,
    pub bytes: &'static [u8],
}

macro_rules! image {
    ($name:literal) => {
        Image { name: $name, bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../assets/manual/", $name, ".jpg")) }
    };
}

pub const SEAT: Image = image!("seat");
pub const FINAL: Image = image!("final");
pub const LEVERS: Image = image!("levers");
pub const PFD: Image = image!("pfd");
pub const PICKER: Image = image!("picker");
pub const FPV: Image = image!("fpv");

/// What a mark points at: something to press or turn, something to read, or what was chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Press,
    Read,
    Chosen,
}

/// A box over a picture and its label.
#[derive(Clone, Copy, Debug)]
pub struct Mark {
    pub tone: Tone,
    /// Left, top, right, bottom.
    pub area: [f32; 4],
    pub label: &'static str,
    /// The label's top left corner.
    pub at: [f32; 2],
}

const fn press(area: [f32; 4], label: &'static str, at: [f32; 2]) -> Mark {
    Mark { tone: Tone::Press, area, label, at }
}

const fn read(area: [f32; 4], label: &'static str, at: [f32; 2]) -> Mark {
    Mark { tone: Tone::Read, area, label, at }
}

const fn chosen(area: [f32; 4], label: &'static str, at: [f32; 2]) -> Mark {
    Mark { tone: Tone::Chosen, area, label, at }
}

/// A step: what to do, and the acts it takes (their keys and the joystick's controls are shown).
pub struct Step(pub &'static str, pub &'static [Act]);

/// A row of a table of controls: the acts and what they do.
pub struct Control(pub &'static [Act], pub &'static str);

pub enum Block {
    /// A heading over what follows.
    Title(&'static str),
    /// Short lines, one idea each.
    Lines(&'static [&'static str]),
    Picture(Image, &'static [Mark]),
    /// Small pictures side by side with their captions: the cockpit's views.
    Views(&'static [(Image, &'static str)]),
    /// Numbered steps.
    Steps(&'static [Step]),
    /// Keys and the joystick, and what they do.
    Controls(&'static [Control]),
    /// The joystick drawn, with what each of its buttons does.
    Joystick,
}

pub struct Page {
    pub title: &'static str,
    pub blocks: &'static [Block],
}

/// The cockpit's views of the C172, in the order of `kinds::spots`, and the outside.
pub const VIEWS: [(Image, &str); 9] = [
    (image!("spot-pilot"), "pilot"),
    (image!("spot-leaning"), "pilot, leaning in"),
    (image!("spot-panel"), "panel"),
    (image!("spot-pfd"), "PFD"),
    (image!("spot-standby"), "standby instruments"),
    (image!("spot-mfd"), "MFD"),
    (image!("spot-switches"), "switches"),
    (image!("spot-levers"), "levers"),
    (image!("spot-exterior"), "outside"),
];

// The PFD's parts in `PFD` (and `SEAT`, `FINAL`).
const PFD_SPEED: [f32; 4] = [0.228, 0.2, 0.305, 0.675];
const PFD_ALTITUDE: [f32; 4] = [0.655, 0.2, 0.74, 0.675];
const PFD_MODES: [f32; 4] = [0.35, 0.137, 0.595, 0.185];
const SEAT_SPEED: [f32; 4] = [0.326, 0.595, 0.364, 0.832];

pub const PAGES: [Page; 7] = [
    Page {
        title: "First steps",
        blocks: &[
            Block::Lines(&["You start at the end of the runway, with the engine at idle.", "Green: what you read. Yellow: what you touch. Pink: what you choose."]),
            Block::Picture(
                SEAT,
                &[
                    read([0.065, 0.11, 0.092, 0.225], "windsock: where the wind is from", [0.1, 0.13]),
                    read([0.47, 0.18, 0.54, 0.33], "runway centreline: follow it", [0.56, 0.24]),
                    read(SEAT_SPEED, "speed (knots)", [0.17, 0.545]),
                    read([0.405, 0.595, 0.52, 0.76], "nose and wings", [0.4, 0.545]),
                    read([0.542, 0.595, 0.586, 0.832], "altitude (feet)", [0.6, 0.64]),
                    read([0.417, 0.78, 0.51, 0.925], "heading", [0.52, 0.86]),
                ],
            ),
            Block::Title("Take off"),
            Block::Steps(&[
                Step("Full throttle.", &[Act::Throttle]),
                Step("Follow the runway centreline with the pedals.", &[Act::Pedals]),
                Step("At 55 knots, pull gently: the aircraft lifts off.", &[Act::NoseUp]),
                Step("Climb at about 75 knots, nose 10° up.", &[Act::NoseUp, Act::NoseDown]),
                Step("Once high (1000 ft above the ground), a little less throttle.", &[Act::Throttle]),
            ]),
            Block::Title("Fly"),
            Block::Steps(&[
                Step("Straight and level: the nose on the horizon.", &[Act::NoseUp, Act::NoseDown]),
                Step("To turn, bank the wings 20° or 30°.", &[Act::Bank]),
                Step("To stop turning, level the wings.", &[Act::Bank]),
                Step("To climb, more throttle; to descend, less.", &[Act::Throttle]),
                Step("Trim: so the nose stays put on its own.", &[Act::Trim]),
                Step("With keyboard assist, nose and wings stay where you leave them.", &[Act::Assist]),
            ]),
            Block::Title("Land"),
            Block::Picture(
                FINAL,
                &[
                    read([0.484, 0.165, 0.516, 0.21], "the runway: aim at its start", [0.53, 0.13]),
                    read(SEAT_SPEED, "65 knots", [0.22, 0.545]),
                    read([0.584, 0.595, 0.616, 0.832], "descending about 500 ft/min", [0.63, 0.62]),
                ],
            ),
            Block::Steps(&[
                Step("To practise, start on final.", &[Act::Final]),
                Step("Lower the flaps one step at a time, in the white speed arc.", &[Act::FlapsDown]),
                Step("At 65 knots: the throttle decides how fast you descend.", &[Act::Throttle]),
                Step("Lights by the runway: 2 white and 2 red, you are on the path.", &[]),
                Step("Over the runway, throttle to idle.", &[Act::Throttle]),
                Step("Just before touchdown, nose a little up; then brake.", &[Act::NoseUp, Act::Brakes]),
            ]),
            Block::Title("Watch the landing again"),
            Block::Lines(&[
                "The game always keeps the last 10 minutes (1 to 30 in settings).",
                "The replay starts 30 s before the last touchdown: nobody flies, it only plays back.",
                "Top right, the touchdown: ft/min, g, distance to the threshold and to the centreline.",
                "Views, camera and mouse stay free; the flight waits.",
            ]),
            Block::Controls(&[
                Control(&[Act::Replay], "replay; again (or Esc), back to the flight where it was"),
                Control(&[Act::ReplayPlay], "play / pause"),
                Control(&[Act::ReplayJump], "10 s back / forward; with ⇧, 30 s"),
                Control(&[Act::ReplaySpeed], "slower / faster (0.25× to 8×)"),
            ]),
            Block::Title("Share a video"),
            Block::Lines(&[
                "F12 records what you see and hear; a red dot and the time say so (they are not in it).",
                "F12 again saves it in Movies/Skylab, ready for WhatsApp or X; Show in Finder finds it.",
            ]),
            Block::Controls(&[Control(&[Act::Record], "record a video with its sound; again, save it")]),
        ],
    },
    Page {
        title: "Views and cockpit",
        blocks: &[
            Block::Views(&VIEWS),
            Block::Controls(&[
                Control(&[Act::View], "cockpit or outside"),
                Control(&[Act::Views], "normal ↔ close; from outside, to the cockpit"),
                Control(&[Act::Closer, Act::Further], "zoom in where you look, or out"),
                Control(&[Act::LookSide], "look to one side"),
                Control(&[Act::LookAhead], "look ahead again"),
            ]),
            Block::Lines(&[
                "Mouse wheel: zoom in or out, like + and −; it stays.",
                "Drag with the mouse to look: the view stays there.",
                "On the panel, the hand shows what can be touched.",
                "Click the G1000 keys; drag its knobs, or scroll on them.",
                "Shift and scroll on the HDG knob or its keys: the altitude.",
                "Drag the throttle, the flaps and the trim wheel.",
            ]),
        ],
    },
    Page {
        title: "Controls",
        blocks: &[
            Block::Controls(&[
                Control(&[Act::Throttle], "throttle: more / less"),
                Control(&[Act::NoseUp, Act::NoseDown], "nose up / down"),
                Control(&[Act::Bank], "bank the wings"),
                Control(&[Act::Pedals], "pedals"),
                Control(&[Act::Brakes], "brakes, while held"),
                Control(&[Act::FlapsDown, Act::FlapsUp], "flaps: down / up"),
                Control(&[Act::Trim], "trim (without keyboard assist)"),
                Control(&[Act::Assist], "keyboard assist: on / off"),
                Control(&[Act::Land], "land by itself at the nearest aerodrome"),
                Control(&[Act::Pause], "the menu: fly, aircraft, place, weather, settings (the game waits); again, fly on"),
                Control(&[Act::Help], "all the keys"),
                Control(&[Act::Manual], "this manual"),
            ]),
            Block::Title("Joystick T.16000M"),
            Block::Joystick,
            Block::Lines(&["With the joystick connected, keyboard assist does nothing.", "The numbers are the joystick's own: --controllers says which is which."]),
            Block::Title("Levers"),
            Block::Picture(
                LEVERS,
                &[
                    press([0.27, 0.46, 0.37, 0.765], "throttle: drag it", [0.1, 0.8]),
                    press([0.38, 0.42, 0.465, 0.585], "mixture (red)", [0.4, 0.6]),
                    press([0.56, 0.19, 0.7, 0.51], "flaps: 0° · 10° · 20° · FULL", [0.5, 0.53]),
                    press([0.75, 0.0, 0.96, 0.6], "yoke", [0.8, 0.62]),
                    press([0.47, 0.78, 0.78, 1.0], "pedals and brakes", [0.8, 0.86]),
                ],
            ),
        ],
    },
    Page {
        title: "Autopilot",
        blocks: &[
            Block::Lines(&[
                "The C172's GFC 700 flies the yoke and the trim.",
                "The throttle is yours: it has no autothrottle.",
                "P alone: it holds the heading and the altitude of now.",
                "Then ← → turn it 5° (⇧ 1°) and ↑ ↓ take it 100 ft up or down (⇧ 1000).",
                "P again, or C, and the aircraft is yours.",
                "On a long leg, F10 makes time go faster (up to ×16), F9 slower.",
            ]),
            Block::Picture(
                PFD,
                &[
                    press([0.057, 0.51, 0.182, 0.572], "1 AP · FD", [0.005, 0.47]),
                    press([0.059, 0.578, 0.183, 0.641], "2 HDG · ALT", [0.19, 0.59]),
                    press([0.061, 0.646, 0.184, 0.71], "NAV", [0.19, 0.665]),
                    press([0.063, 0.715, 0.185, 0.778], "APR", [0.19, 0.735]),
                    press([0.065, 0.784, 0.186, 0.848], "3 VS · FLC", [0.19, 0.8]),
                    press([0.067, 0.853, 0.187, 0.912], "4 nose up / down", [0.19, 0.87]),
                    press([0.078, 0.375, 0.143, 0.468], "5 HDG knob", [0.02, 0.33]),
                    press([0.086, 0.928, 0.158, 1.0], "6 ALT knob", [0.19, 0.94]),
                    press([0.822, 0.662, 0.876, 0.716], "7 D→", [0.828, 0.615]),
                    read(PFD_MODES, "A modes: HDG · AP · ALT", [0.36, 0.09]),
                    read([0.853, 0.036, 0.992, 0.066], "B the same, always in sight", [0.73, 0.08]),
                    chosen([0.348, 0.598, 0.418, 0.632], "C selected heading", [0.33, 0.55]),
                    chosen([0.661, 0.176, 0.739, 0.204], "D selected altitude", [0.75, 0.2]),
                    chosen([0.416, 0.42, 0.583, 0.484], "E flight director", [0.44, 0.5]),
                ],
            ),
            Block::Title("Hold heading and altitude"),
            Block::Steps(&[
                Step("Press the HDG knob (5): the selected heading (C) becomes the current one.", &[key(AfcsKey::HeadingSync)]),
                Step("Press HDG and ALT (2).", &[key(AfcsKey::Hdg), key(AfcsKey::Alt)]),
                Step("Engage with AP (1): at the top, AP in green (A).", &[key(AfcsKey::Ap)]),
            ]),
            Block::Title("Turn"),
            Block::Steps(&[
                Step("Turn the HDG knob (5): drag it up or right for more, or scroll over it.", &[knob(PanelKnob::Heading, -1), knob(PanelKnob::Heading, 1), knob(PanelKnob::Heading, -10), knob(PanelKnob::Heading, 10)]),
                Step("In HDG, the aircraft turns by itself to the selected heading.", &[]),
            ]),
            Block::Title("Climb or descend with FLC: at a speed"),
            Block::Steps(&[
                Step("Set the altitude with the ALT knob (6) (D).", &[knob(PanelKnob::AltitudeFine, -1), knob(PanelKnob::AltitudeFine, 1), knob(PanelKnob::AltitudeCoarse, -1), knob(PanelKnob::AltitudeCoarse, 1)]),
                Step("To climb, full throttle; to descend, little.", &[Act::Throttle]),
                Step("Press FLC (3): it climbs or descends at the current speed.", &[key(AfcsKey::Flc)]),
                Step("On reaching it, it levels off by itself and goes to ALT.", &[]),
            ]),
            Block::Title("Climb or descend with VS: at a rate"),
            Block::Steps(&[
                Step("Set the altitude with the ALT knob (6).", &[knob(PanelKnob::AltitudeFine, -1), knob(PanelKnob::AltitudeFine, 1)]),
                Step("Press VS (3).", &[key(AfcsKey::Vs)]),
                Step("Set the rate with nose up / down (4).", &[key(AfcsKey::NoseUp), key(AfcsKey::NoseDown)]),
                Step("Watch the speed: the throttle is yours.", &[Act::Throttle]),
            ]),
            Block::Title("Go direct to the nearest aerodrome"),
            Block::Steps(&[
                Step("Press D→ (7).", &[Act::Panel(Panel::DirectTo)]),
                Step("Press NAV: it follows the magenta line.", &[key(AfcsKey::Nav)]),
                Step("Once close, APR: the approach to the runway into the wind.", &[key(AfcsKey::Apr)]),
            ]),
            Block::Title("Disconnect"),
            Block::Steps(&[
                Step("Press AP, or C; or move the yoke firmly.", &[key(AfcsKey::Ap), Act::Disconnect]),
                Step("A tone sounds and AP flashes: you have the aircraft.", &[]),
            ]),
        ],
    },
    Page {
        title: "Instruments",
        blocks: &[
            Block::Lines(&["The PFD, the screen in front of you, at a glance."]),
            Block::Picture(
                PFD,
                &[
                    read(PFD_SPEED, "speed (knots)", [0.1, 0.15]),
                    read([0.39, 0.22, 0.62, 0.56], "nose and wings on the horizon", [0.38, 0.2]),
                    read(PFD_ALTITUDE, "altitude (feet)", [0.79, 0.12]),
                    read([0.747, 0.26, 0.798, 0.598], "climb / descent (ft/min)", [0.81, 0.6]),
                    read([0.41, 0.596, 0.59, 0.85], "heading", [0.6, 0.8]),
                    read(PFD_MODES, "autopilot", [0.36, 0.09]),
                    read([0.655, 0.646, 0.735, 0.675], "altimeter pressure", [0.6, 0.69]),
                    read([0.205, 0.765, 0.292, 0.822], "temperature and wind", [0.2, 0.725]),
                    chosen([0.661, 0.176, 0.739, 0.204], "selected altitude", [0.75, 0.2]),
                ],
            ),
            Block::Lines(&[
                "Speed: green, normal; white, with flaps; yellow, only in calm air.",
                "Nose and wings: the yellow aircraft on the horizon.",
                "Altitude and heading: the number in the box.",
                "At the top, in green, what the autopilot is flying.",
            ]),
            Block::Controls(&[Control(&[Act::Qnh], "altimeter to the aerodrome's pressure (QNH)"), Control(&[Act::Baro], "altimeter: +1 / −1 hPa")]),
        ],
    },
    Page {
        title: "Places and weather",
        blocks: &[
            Block::Picture(
                PICKER,
                &[
                    press([0.028, 0.068, 0.263, 0.106], "1 search: name, city or ICAO", [0.28, 0.06]),
                    press([0.009, 0.122, 0.26, 0.688], "2 featured places, favourites, courses", [0.28, 0.3]),
                    press([0.5, 0.25, 0.889, 0.875], "3 map: drag, scroll; click chooses", [0.56, 0.2]),
                    press([0.009, 0.772, 0.215, 0.855], "4 where to start", [0.28, 0.79]),
                    press([0.01, 0.862, 0.262, 0.916], "5 Fly here", [0.28, 0.9]),
                ],
            ),
            Block::Title("Fly from somewhere else"),
            Block::Steps(&[
                Step("Open the place picker (or Place in the menu, Esc).", &[Act::Picker]),
                Step("Pick a featured place (2), search for an aerodrome (1) or click the map (3).", &[]),
                Step("Choose where to start (4): on the runway, on final or in flight.", &[]),
                Step("Press Fly here (5). The first time it downloads (20 to 50 MB).", &[]),
            ]),
            Block::Title("Settings and time"),
            Block::Controls(&[
                Control(&[Act::Settings], "settings beside the flight: weather, time, aids, graphics"),
                Control(&[Act::Hour], "the time of day, forward / back"),
                Control(&[Act::Time], "time slower / faster: ×1, ×2, ×4, ×8, ×16"),
                Control(&[Act::Runway, Act::Air, Act::Final], "start on the runway / in flight / on final"),
            ]),
            Block::Lines(&[
                "What you change in settings is saved by itself.",
                "The flight goes on beside them; O or Esc closes them.",
                "The menu (Esc), the place picker and this manual wait.",
                "The game starts at your computer's time (Settings, Weather).",
                "Faster time is more steps, not longer ones: as accurate.",
                "Top right, ×4; back to ×1 on a touchdown, a crash, a pause.",
            ]),
        ],
    },
    Page {
        title: "FPV quad",
        blocks: &[
            Block::Lines(&["A 5-inch racing drone, seen through its camera. Choose it in the menu (Esc, Aircraft).", "Its flight controller flies it: the sticks ask, it does."]),
            Block::Picture(
                FPV,
                &[
                    read([0.055, 0.13, 0.145, 0.27], "battery: volts, a cell, amps, used", [0.16, 0.2]),
                    read([0.465, 0.13, 0.535, 0.17], "heading", [0.56, 0.15]),
                    read([0.875, 0.13, 0.945, 0.17], "time armed", [0.8, 0.19]),
                    read([0.34, 0.715, 0.66, 0.755], "the horizon: the camera looks up 25°", [0.35, 0.77]),
                    read([0.055, 0.85, 0.165, 0.92], "height, climb", [0.18, 0.86]),
                    read([0.46, 0.88, 0.535, 0.92], "throttle", [0.56, 0.89]),
                    read([0.875, 0.88, 0.945, 0.92], "speed", [0.79, 0.84]),
                ],
            ),
            Block::Controls(&[
                Control(&[Act::Arm], "arm / disarm (the throttle down)"),
                Control(&[Act::Throttle], "throttle stick: up / down"),
                Control(&[Act::NoseDown, Act::NoseUp], "pitch stick: forward (nose down) / back"),
                Control(&[Act::Bank], "roll stick"),
                Control(&[Act::Pedals], "yaw stick"),
                Control(&[Act::View], "its camera / behind it"),
                Control(&[Act::Runway, Act::Air], "on the ground here / hovering here"),
            ]),
            Block::Lines(&[
                "The sticks tilt it, and it levels when you let go. W and S: more power climbs, less sinks.",
                "An RC transmitter by USB is its radio: arm on its first switch.",
            ]),
            Block::Title("Race round a course"),
            Block::Lines(&[
                "M, Race courses: gates lit by LEDs, flown through in order, the right way (or --course).",
                "3, 2, 1, GO: the clock, the lap, the splits; the diamond marks the next gate, the arrow the way to it.",
                "1 races again from the pad. L lets the autopilot race. Your best lap is kept.",
                "--ai: race a trained network and the autopilot, each quad in its colour, P1/3 on the display.",
                "N puts the camera on the next quad (V: its camera / behind it): watch them race.",
            ]),
        ],
    },
];

/// Every act the manual names.
#[cfg(test)]
pub fn acts() -> Vec<Act> {
    let mut acts = Vec::new();
    for page in &PAGES {
        for block in page.blocks {
            let named: Vec<Act> = match block {
                Block::Steps(steps) => steps.iter().flat_map(|s| s.1.iter().copied()).collect(),
                Block::Controls(rows) => rows.iter().flat_map(|r| r.0.iter().copied()).collect(),
                _ => Vec::new(),
            };
            for act in named {
                if !acts.contains(&act) {
                    acts.push(act);
                }
            }
        }
    }
    acts
}

/// Every picture of a page.
#[cfg(test)]
pub fn images(page: &Page) -> Vec<Image> {
    page.blocks
        .iter()
        .flat_map(|block| match block {
            Block::Picture(image, _) => vec![*image],
            Block::Views(views) => views.iter().map(|(image, _)| *image).collect(),
            _ => Vec::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kinds;
    use sim_core::AircraftKind;

    #[test]
    fn every_page_has_pictures_and_they_can_be_read() {
        for page in &PAGES {
            let images = images(page);
            assert!(!images.is_empty(), "{}", page.title);
            for image in images {
                let (w, h, _) = engine::texture::decode_bytes(image.bytes).unwrap_or_else(|e| panic!("{}: {e:?}", image.name));
                // The game's 1600 x 1000, made smaller.
                assert_eq!(w * 5, h * 8, "{}: {w}x{h}", image.name);
            }
        }
    }

    #[test]
    fn every_mark_is_inside_its_picture() {
        for page in &PAGES {
            for block in page.blocks {
                let Block::Picture(image, marks) = block else { continue };
                for m in *marks {
                    let [x0, y0, x1, y1] = m.area;
                    let inside = |v: f32| (0.0..=1.0).contains(&v);
                    assert!(x0 < x1 && y0 < y1 && [x0, y0, x1, y1].into_iter().all(inside), "{}: {}", image.name, m.label);
                    // The label starts inside, with room for a few words.
                    assert!(inside(m.at[0]) && inside(m.at[1]) && m.at[0] < 0.97 && m.at[1] < 0.97, "{}: {}", image.name, m.label);
                }
            }
        }
    }

    #[test]
    fn the_views_are_the_cockpits_spots() {
        let spots = kinds::spots(AircraftKind::C172);
        let named: Vec<&str> = VIEWS.iter().map(|v| v.1).collect();
        let theirs: Vec<&str> = spots.iter().map(|s| s.name).collect();
        assert_eq!(named[..named.len() - 1], theirs[..]);
    }

    #[test]
    fn every_step_and_control_has_a_key() {
        for act in acts() {
            assert!(!act.keys().is_empty(), "{act:?}");
        }
    }
}
