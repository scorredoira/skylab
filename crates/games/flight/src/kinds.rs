//! What the game does differently for each type of aircraft, behind one door. A type is a file
//! (`assets/aircraft/<key>/aircraft.ron`, `sim_core::aircraft::file`); what of it is drawn is
//! files too, in the folders its `visual` names: its model (`<model>/exterior.glb` and `.ron`:
//! mesh, moving parts, lights, the shadow it casts; `models`) and its cockpit
//! (`<cockpit>/cockpit.glb` and `.ron`: the cabin, where the eye can be in it, its instruments,
//! what is printed in it, what the mouse works; `cockpit::file`). They are read once, here, and
//! checked when the game starts (`check`). The rest comes from the type's numbers: the marks of
//! the instrument panel, the engine's sound, the speeds the player's autopilot holds. Nothing in
//! the game matches on a kind of aircraft.
//!
//! The player can fly a multirotor too (`sim_core::multirotor`): what is flown is a [`Craft`], an
//! aircraft's kind or a multirotor's. A multirotor's look is a folder of `assets/aircraft` as an
//! aircraft's is (`exterior.glb` and `.ron`, its propellers `Moves::Rotor`); it has no cockpit,
//! its camera is its FPV camera (`MultirotorType::visual`).

use crate::cockpit::file::CockpitFile;
use crate::cockpit::instruments;
pub use crate::cockpit::instruments::Shown;
use crate::cockpit::{CabinSpec, Spot};
use crate::g1000::Instruments;
use crate::models::Exterior;
use crate::ui::panel::Airframe;
use sim_core::{AircraftKind, AircraftType, MultirotorKind, MultirotorType, Vehicle};
use std::sync::Mutex;
use world_render::ShadowShape;

/// What has been read of each look, by its name: read once, kept for the game's life.
struct Read<T: 'static> {
    looks: Mutex<Vec<(String, &'static T)>>,
}

impl<T> Read<T> {
    const fn new() -> Read<T> {
        Read { looks: Mutex::new(Vec::new()) }
    }

    fn get(&self, name: &str, read: impl FnOnce(&str) -> Result<T, String>) -> Result<&'static T, String> {
        let mut looks = self.looks.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, look)) = looks.iter().find(|(n, _)| n == name) {
            return Ok(look);
        }
        let look: &'static T = Box::leak(Box::new(read(name)?));
        looks.push((name.to_string(), look));
        Ok(look)
    }
}

/// What the menu says of a craft beside its picture (`<model>/card.ron`): what it is in a line,
/// what it is good for, how hard it is to fly (1 the easiest to 5), and a few figures (their
/// name, their value). All of it may be missing: the menu then shows the name alone.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct Card {
    pub class: String,
    pub tagline: String,
    pub good_for: String,
    pub difficulty: u8,
    pub facts: Vec<(String, String)>,
}

static EXTERIORS: Read<Exterior> = Read::new();
static CARDS: Read<Card> = Read::new();

/// A craft's card for the menu, read once; an empty one if its folder has none or it is wrong
/// (said on the terminal).
pub fn card(craft: Craft) -> &'static Card {
    let read = |name: &str| -> Result<Card, String> {
        if !crate::fleet::has(name, "card.ron") {
            return Ok(Card::default());
        }
        ron::from_str(&crate::fleet::text(name, "card.ron")?).map_err(|e| format!("{name}/card.ron: {e}"))
    };
    let card = CARDS.get(craft.model(), |name| {
        Ok(read(name).unwrap_or_else(|error| {
            eprintln!("{error}");
            Card::default()
        }))
    });
    card.expect("a card is always read: an empty one in place of a wrong one")
}
static COCKPITS: Read<(CockpitFile, Vec<Spot>)> = Read::new();

fn read_exterior(name: &str) -> Result<&'static Exterior, String> {
    EXTERIORS.get(name, |name| Exterior::parse(&crate::fleet::text(name, "exterior.ron")?).map_err(|e| format!("{name}/exterior.ron: {e}")))
}

fn read_cockpit(name: &str) -> Result<&'static (CockpitFile, Vec<Spot>), String> {
    COCKPITS.get(name, |name| {
        let file = CockpitFile::parse(&crate::fleet::text(name, "cockpit.ron")?).map_err(|e| format!("{name}/cockpit.ron: {e}"))?;
        if file.spots.is_empty() {
            return Err(format!("{name}/cockpit.ron: no spots"));
        }
        let spots = file.spots();
        Ok((file, spots))
    })
}

/// What the player flies: a kind of aircraft or a kind of multirotor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Craft {
    Aircraft(AircraftKind),
    Multirotor(MultirotorKind),
}

impl Default for Craft {
    fn default() -> Craft {
        Craft::Aircraft(AircraftKind::default())
    }
}

impl Craft {
    /// Every kind there is: the aircraft, then the multirotors. The renderer keeps their models in
    /// this order (`index`).
    pub fn all() -> impl Iterator<Item = Craft> {
        AircraftKind::all().map(Craft::Aircraft).chain(MultirotorKind::all().map(Craft::Multirotor))
    }

    /// Its place in `all`.
    pub fn index(self) -> usize {
        Craft::all().position(|c| c == self).unwrap_or(0)
    }

    /// The kind a word names (the key of its file or an alias): the aircraft's first.
    pub fn parse(word: &str) -> Option<Craft> {
        AircraftKind::parse(word).map(Craft::Aircraft).or_else(|| MultirotorKind::parse(word).map(Craft::Multirotor))
    }

    /// What a vehicle of the world is.
    pub fn of(vehicle: &Vehicle) -> Craft {
        match &vehicle.dynamics {
            sim_core::Dynamics::Aircraft(a) => Craft::Aircraft(a.p.kind),
            sim_core::Dynamics::Multirotor(m) => Craft::Multirotor(m.p.kind),
            sim_core::Dynamics::Car(_) => unreachable!("the flight game has no cars: they are the driving game's"),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Craft::Aircraft(k) => k.name(),
            Craft::Multirotor(k) => k.name(),
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Craft::Aircraft(k) => k.key(),
            Craft::Multirotor(k) => k.key(),
        }
    }

    /// The folder of `assets/aircraft` its model is in.
    pub fn model(self) -> &'static str {
        match self {
            Craft::Aircraft(k) => k.spec().visual.model,
            Craft::Multirotor(k) => k.spec().visual.model,
        }
    }

    /// Which of the game's two families it is of: a tilt-rotor, with its pilot aboard, goes
    /// with the planes.
    pub fn family(self) -> Family {
        match self {
            Craft::Aircraft(_) => Family::Planes,
            Craft::Multirotor(k) if k.spec().tilt.is_some() => Family::Planes,
            Craft::Multirotor(_) => Family::Drones,
        }
    }
}

/// The families of craft this game flies, as the first screen offers them (`ui::menu::vehicles`;
/// the cars are the driving game's). The aircraft to choose from are those of the family flown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    Planes,
    Drones,
}

impl Family {
    /// Its crafts, in the order of `Craft::all`.
    pub fn crafts(self) -> impl Iterator<Item = Craft> {
        Craft::all().filter(move |c| c.family() == self)
    }

    /// The word the settings keep its craft flown last under.
    pub fn key(self) -> &'static str {
        match self {
            Family::Planes => "plane",
            Family::Drones => "drone",
        }
    }

    /// The craft flown when the family is chosen: the one `word` names (what the settings kept),
    /// if it is of the family, else its first.
    pub fn craft(self, word: Option<&str>) -> Option<Craft> {
        word.and_then(Craft::parse).filter(|c| c.family() == self).or_else(|| self.crafts().next())
    }
}

/// What moves of a type's model, its lights and its shadow (`models::Exterior`), as its files say.
/// The game has checked them when it started (`check`).
pub fn exterior(kind: AircraftKind) -> &'static Exterior {
    exterior_of(Craft::Aircraft(kind))
}

/// The same of any craft.
pub fn exterior_of(craft: Craft) -> &'static Exterior {
    read_exterior(craft.model()).unwrap_or_else(|e| panic!("{e}"))
}

/// The model any craft is drawn with, read from its files.
pub fn model_of(craft: Craft) -> Result<crate::models::AircraftModel, String> {
    let name = craft.model();
    let glb = crate::fleet::file(name, "exterior.glb")?;
    crate::models::build(exterior_of(craft), &glb).map_err(|e| format!("{name}: {e}"))
}

/// Whether the game can draw every multirotor: the message for the player if not.
pub fn check_multirotors(types: &[MultirotorType]) -> Result<(), String> {
    for t in types {
        let model = t.visual.model;
        let exterior = read_exterior(model).map_err(|e| format!("{}: the model {e}", t.kind.key()))?;
        if !crate::fleet::has(model, "exterior.glb") {
            return Err(format!("{}: {model}/exterior.glb is missing", t.kind.key()));
        }
        if let Some(cockpit) = t.visual.cockpit {
            read_cockpit(cockpit).map_err(|e| format!("{}: the cockpit {e}", t.kind.key()))?;
            if !crate::fleet::has(cockpit, "cockpit.glb") {
                return Err(format!("{}: {cockpit}/cockpit.glb is missing", t.kind.key()));
            }
        }
        for part in &exterior.parts {
            if let crate::models::Moves::Rotor { index } | crate::models::Moves::RotorDisc { index } = part.moves {
                if index >= t.motors.len() {
                    return Err(format!("{}: {model}/exterior.ron: rotor {index} of {} motors", t.kind.key(), t.motors.len()));
                }
            }
        }
    }
    Ok(())
}

/// A type's cockpit's file (`cockpit::file`), checked as `exterior`.
pub fn cockpit(kind: AircraftKind) -> &'static CockpitFile {
    cockpit_in(kind.spec().visual.cockpit)
}

/// The folder of `assets/aircraft` a craft's cockpit is in: every aircraft's, and a multirotor's
/// if it has one.
pub fn cabin(craft: Craft) -> Option<&'static str> {
    match craft {
        Craft::Aircraft(k) => Some(k.spec().visual.cockpit),
        Craft::Multirotor(k) => k.spec().visual.cockpit,
    }
}

/// The cockpit's file of a folder (`cabin`).
pub fn cockpit_in(name: &str) -> &'static CockpitFile {
    &read_cockpit(name).unwrap_or_else(|e| panic!("{e}")).0
}

/// The cockpit of a folder, as the cockpit's renderer takes it, read from its files.
pub fn cockpit_spec_in(name: &str) -> Result<CabinSpec, String> {
    let glb = crate::fleet::file(name, "cockpit.glb")?;
    cockpit_in(name).spec(&glb).map_err(|e| format!("{name}: {e}"))
}

/// Where the eye can be in the cockpit of a folder, the pilot's seat first.
pub fn spots_in(name: &str) -> Vec<Spot> {
    read_cockpit(name).unwrap_or_else(|e| panic!("{e}")).1.clone()
}

/// The model a type is drawn with, read from its files.
pub fn model(kind: AircraftKind) -> Result<crate::models::AircraftModel, String> {
    model_of(Craft::Aircraft(kind))
}

/// A type's cockpit, as the cockpit's renderer takes it (`cockpit::CabinSpec`), read from its files.
#[cfg(test)]
pub fn cockpit_spec(kind: AircraftKind) -> Result<CabinSpec, String> {
    cockpit_spec_in(kind.spec().visual.cockpit)
}

/// The aircraft parked at the aerodromes (`models::parked`): the first type's whose model says how
/// it stands parked, or else the first type's.
pub fn parked_aircraft() -> Result<world_render::buildings::Building, String> {
    let kind = parked_kind().ok_or("no aircraft")?;
    Ok(crate::models::parked(&model(kind)?, exterior(kind).parked.as_ref()))
}

/// The type parked at the aerodromes (`parked_aircraft`).
pub fn parked_kind() -> Option<AircraftKind> {
    AircraftKind::all().find(|&k| exterior(k).parked.is_some()).or_else(|| AircraftKind::all().next())
}

/// Whether the game can draw every type of a fleet: the message for the player if not. The
/// meshes are only looked for; they are read when they are drawn.
pub fn check(fleet: &[AircraftType]) -> Result<(), String> {
    for t in fleet {
        let (model, cockpit) = (t.visual.model, t.visual.cockpit);
        read_exterior(model).map_err(|e| format!("{}: the model {e}", t.kind.key()))?;
        read_cockpit(cockpit).map_err(|e| format!("{}: the cockpit {e}", t.kind.key()))?;
        for (name, file) in [(model, "exterior.glb"), (cockpit, "cockpit.glb")] {
            if !crate::fleet::has(name, file) {
                return Err(format!("{}: {name}/{file} is missing", t.kind.key()));
            }
        }
    }
    Ok(())
}

/// Where the eye can be in each kind's cockpit (`cockpit::Spot`), the pilot's seat first.
pub fn spots(kind: AircraftKind) -> Vec<Spot> {
    read_cockpit(kind.spec().visual.cockpit).unwrap_or_else(|e| panic!("{e}")).1.clone()
}

/// Paints the instruments of the player's cockpit into its canvas, each frame in the cockpit, as
/// its file fits them (`cockpit::instruments`; `g1000` keeps what a G1000 needs between frames).
pub fn paint_instruments(kind: AircraftKind, gpu: &engine::Gpu, canvas: &mut engine::Canvas, g1000: &mut Instruments, shown: Shown) {
    instruments::paint(&cockpit(kind).instruments, gpu, canvas, g1000, shown);
}

/// The same for the cockpit of a folder (`cabin`).
pub fn paint_instruments_in(name: &str, gpu: &engine::Gpu, canvas: &mut engine::Canvas, g1000: &mut Instruments, shown: Shown) {
    instruments::paint(&cockpit_in(name).instruments, gpu, canvas, g1000, shown);
}

/// The airspeed indicator's marks, and the flap slider or the accelerometer.
pub fn panel(kind: AircraftKind) -> Airframe {
    Airframe::of(kind.spec())
}

/// Cylinders of the engine, for its sound.
pub fn cylinders(kind: AircraftKind) -> f32 {
    kind.spec().cylinders as f32
}

/// The airspeeds the player's autopilot holds, m/s.
pub fn autopilot_speeds(kind: AircraftKind) -> (f64, f64) {
    let s = kind.spec().speeds;
    (s.least, s.most)
}

/// The shadow of an aircraft of a kind with `damage`: its wing (the first flat piece) as short on each
/// side as a tear left it.
pub fn damaged_shadow(kind: AircraftKind, damage: &sim_core::Damage) -> ShadowShape {
    let mut shape = shadow(kind);
    let tip = shape.flat[0][3];
    let [left, right] = damage.wing_torn.map(|torn| torn.map_or(tip, |end| end.min(tip as f64) as f32));
    shape.flat[0][2] = (right - left) / 2.0;
    shape.flat[0][3] = (right + left) / 2.0;
    shape
}

/// The shadow each model casts (`world_render::ShadowShape`), as its `exterior.ron` says.
pub fn shadow(kind: AircraftKind) -> ShadowShape {
    exterior(kind).shadow()
}

/// The same of any craft, whole.
pub fn shadow_of(craft: Craft) -> ShadowShape {
    exterior_of(craft).shadow()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every craft has a model, and a word that names it.
    #[test]
    fn every_craft_has_a_model_and_a_name() {
        check_multirotors(sim_core::multirotor::file::fleet()).unwrap();
        for craft in Craft::all() {
            assert_eq!(Craft::parse(craft.key()), Some(craft));
            model_of(craft).unwrap();
            assert_eq!(Craft::all().nth(craft.index()), Some(craft));
        }
        assert_eq!(Craft::parse("drone"), Some(Craft::Multirotor(MultirotorKind::FPV5)));
        assert_eq!(Craft::parse("cessna"), Some(Craft::Aircraft(AircraftKind::C172)));
    }

    /// Every craft is of one family, and a family's craft is the one kept if it is of it, else its first.
    #[test]
    fn the_families_share_out_the_crafts() {
        let keys = |family: Family| family.crafts().map(Craft::key).collect::<Vec<_>>();
        assert_eq!(keys(Family::Planes), ["c172", "extra300", "kestrel"]);
        assert_eq!(keys(Family::Drones), ["fpv5", "race5"]);
        assert_eq!(Family::Planes.craft(Some("extra")).map(Craft::key), Some("extra300"));
        assert_eq!(Family::Planes.craft(Some("race5")).map(Craft::key), Some("c172"));
        assert_eq!(Family::Drones.craft(None).map(Craft::key), Some("fpv5"));
        assert_eq!(Family::Drones.craft(Some("race5")).map(Craft::key), Some("race5"));
    }

    /// Every craft of the repository has its card for the menu, whole, and its picture.
    #[test]
    fn every_craft_has_a_card_and_a_picture() {
        for craft in Craft::all() {
            let card = card(craft);
            assert!(!card.class.is_empty() && !card.tagline.is_empty() && !card.good_for.is_empty(), "{}", craft.key());
            assert!((1..=5).contains(&card.difficulty) && (2..=4).contains(&card.facts.len()), "{}", craft.key());
            let picture = crate::fleet::file(craft.model(), "picture.jpg").unwrap();
            engine::texture::decode_bytes(&picture).unwrap();
        }
    }

    /// The game draws every built-in type, and says which look it does not find.
    #[test]
    fn every_type_has_a_model_and_a_cockpit() {
        check(sim_core::aircraft::file::fleet()).unwrap();
        let mut odd = AircraftType::c172();
        odd.visual.cockpit = "piper";
        let error = check(&[odd]).unwrap_err();
        assert!(error.starts_with("c172: the cockpit ") && error.contains("piper"), "{error}");
    }
}
