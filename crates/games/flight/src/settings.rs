//! Weather and time settings, the aircraft and the graphics: what they are, their units and limits, the
//! kinds of day, and the file that remembers them. The panel that shows them (key O) is `ui::menu::side`.
//!
//! The file is plain `key=value` text at `$XDG_CONFIG_HOME/flight/settings` or
//! `~/.config/flight/settings`. Only what the player has changed is in it.

use sim_core::{Weather, FEET, HPA, KNOTS};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// What the sliders change, borrowed from the world and the renderer.
pub struct Conditions<'a> {
    pub weather: &'a mut Weather,
    /// Hour of the day, 0..24.
    pub hour: &'a mut f64,
    /// 0..1.
    pub cloud_cover: &'a mut f64,
    /// Cloud base above sea level, metres.
    pub cloud_altitude: &'a mut f64,
    /// Storm clouds about (cumulonimbus), 0 to 3.
    pub storms: &'a mut u32,
}

/// A kind of day: the sliders it sets all at once, and its storms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sky {
    Clear,
    Fair,
    Haze,
    Overcast,
    Windy,
    Storms,
}

impl Sky {
    pub const ALL: [Sky; 6] = [Sky::Clear, Sky::Fair, Sky::Haze, Sky::Overcast, Sky::Windy, Sky::Storms];

    /// Name on the command line.
    pub fn name(self) -> &'static str {
        match self {
            Sky::Clear => "clear",
            Sky::Fair => "fair",
            Sky::Haze => "haze",
            Sky::Overcast => "overcast",
            Sky::Windy => "windy",
            Sky::Storms => "storms",
        }
    }

    /// The kind of day of a name, or of the name it had before (`despejado`, `buen-tiempo`...),
    /// so that older scripts still work.
    pub fn named(name: &str) -> Option<Sky> {
        let name = match name {
            "despejado" => "clear",
            "buen-tiempo" => "fair",
            "calima" => "haze",
            "cubierto" => "overcast",
            "ventoso" => "windy",
            "tormentas" => "storms",
            other => other,
        };
        Sky::ALL.into_iter().find(|s| s.name() == name)
    }

    pub fn title(self) -> &'static str {
        match self {
            Sky::Clear => "Clear",
            Sky::Fair => "Fair",
            Sky::Haze => "Haze",
            Sky::Overcast => "Overcast",
            Sky::Windy => "Windy",
            Sky::Storms => "Storms",
        }
    }

    /// The sliders it sets, in their units, and its storms.
    fn values(self) -> ([(Setting, f64); 6], u32) {
        let day = |wind: f64, turbulence: f64, visibility: f64, humidity: f64, cover: f64, base: f64| {
            [(Setting::WindSpeed, wind), (Setting::Turbulence, turbulence), (Setting::Visibility, visibility), (Setting::Humidity, humidity), (Setting::CloudCover, cover), (Setting::CloudBase, base)]
        };
        match self {
            // Calm: a clear day is the one with still air, so that "Clear" never brings a wind back.
            Sky::Clear => (day(0.0, 0.0, 150.0, 20.0, 0.0, 5000.0), 0),
            Sky::Fair => (day(6.0, 10.0, 80.0, 40.0, 55.0, 5000.0), 0),
            Sky::Haze => (day(2.0, 5.0, 8.0, 15.0, 15.0, 7000.0), 0),
            Sky::Overcast => (day(10.0, 20.0, 20.0, 80.0, 100.0, 3000.0), 0),
            Sky::Windy => (day(26.0, 60.0, 60.0, 45.0, 60.0, 4500.0), 0),
            Sky::Storms => (day(14.0, 30.0, 45.0, 85.0, 70.0, 4000.0), 3),
        }
    }

    /// What it sets, for the player: the wind and the turbulence, the visibility and the clouds.
    pub fn summary(self) -> String {
        let (sliders, storms) = self.values();
        let value = |which: Setting| sliders.iter().find(|(s, _)| *s == which).map_or(0.0, |(_, v)| *v);
        let wind = match value(Setting::WindSpeed) {
            0.0 => "no wind".to_string(),
            w => format!("wind {w:.0} kt"),
        };
        let storms = if storms > 0 { format!(", {storms} storms") } else { String::new() };
        format!("{wind}, turbulence {:.0} %, visibility {:.0} km, clouds {:.0} %{storms}", value(Setting::Turbulence), value(Setting::Visibility), value(Setting::CloudCover))
    }

    /// The kind of day the sliders make now, if they are exactly one's.
    pub fn current(c: &Conditions) -> Option<Sky> {
        Sky::ALL.into_iter().find(|sky| {
            let (sliders, storms) = sky.values();
            *c.storms == storms && sliders.iter().all(|(setting, value)| (setting.get(c) - value).abs() < setting.step() * 0.5)
        })
    }

    /// Makes the day this kind; with a store, remembered as the sliders are.
    pub fn apply(self, c: &mut Conditions, mut store: Option<&mut Store>) {
        let (sliders, storms) = self.values();
        for (setting, value) in sliders {
            setting.set(c, value);
            if let Some(store) = store.as_deref_mut() {
                store.set(setting.key(), value);
            }
        }
        *c.storms = storms;
        if let Some(store) = store {
            store.set(STORMS, storms);
        }
    }
}

/// The key the storms are remembered by.
const STORMS: &str = "storms";

/// One slider of the settings window. Values are in the units shown to the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    WindFrom,
    WindSpeed,
    Turbulence,
    Hour,
    Visibility,
    /// Relative humidity: denser air, and a whiter haze.
    Humidity,
    CloudCover,
    CloudBase,
    /// The air of the day: the QNH and the temperature at sea level.
    Pressure,
    Temperature,
}

impl Setting {
    pub const ALL: [Setting; 10] = [
        Setting::WindFrom,
        Setting::WindSpeed,
        Setting::Turbulence,
        Setting::Hour,
        Setting::Visibility,
        Setting::Humidity,
        Setting::CloudCover,
        Setting::CloudBase,
        Setting::Pressure,
        Setting::Temperature,
    ];

    /// Name in the settings file.
    pub fn key(self) -> &'static str {
        match self {
            Setting::WindFrom => "windFrom",
            Setting::WindSpeed => "windSpeed",
            Setting::Turbulence => "turbulence",
            Setting::Hour => "hour",
            Setting::Visibility => "visibility",
            Setting::Humidity => "humidity",
            Setting::CloudCover => "cloudCover",
            Setting::CloudBase => "cloudBase",
            Setting::Pressure => "qnh",
            Setting::Temperature => "temperature",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Setting::WindFrom => "Wind: direction",
            Setting::WindSpeed => "Wind: speed",
            Setting::Turbulence => "Turbulence",
            Setting::Hour => "Time of day",
            Setting::Visibility => "Visibility",
            Setting::Humidity => "Humidity",
            Setting::CloudCover => "Clouds: cover",
            Setting::CloudBase => "Clouds: base",
            Setting::Pressure => "Pressure (QNH)",
            Setting::Temperature => "Sea-level temperature",
        }
    }

    /// Lowest and highest value.
    pub fn range(self) -> (f64, f64) {
        match self {
            Setting::WindFrom => (0.0, 355.0),
            Setting::WindSpeed => (0.0, 40.0),
            Setting::Turbulence => (0.0, 100.0),
            Setting::Hour => (0.0, 23.75),
            Setting::Visibility => (0.2, 200.0),
            Setting::Humidity => (0.0, 100.0),
            Setting::CloudCover => (0.0, 100.0),
            Setting::CloudBase => (1000.0, 12000.0),
            Setting::Pressure => (960.0, 1050.0),
            Setting::Temperature => (-30.0, 45.0),
        }
    }

    /// The slider moves in multiples of this.
    pub fn step(self) -> f64 {
        match self {
            Setting::WindFrom | Setting::Turbulence | Setting::CloudCover | Setting::Humidity => 5.0,
            Setting::WindSpeed | Setting::Pressure | Setting::Temperature => 1.0,
            Setting::Visibility => 0.1,
            Setting::Hour => 0.25,
            Setting::CloudBase => 500.0,
        }
    }

    /// The value as shown next to the slider.
    pub fn format(self, v: f64) -> String {
        match self {
            Setting::WindFrom => format!("{v:03.0}°"),
            Setting::WindSpeed => format!("{v:.0} kt"),
            Setting::Turbulence | Setting::CloudCover | Setting::Humidity => format!("{v:.0} %"),
            Setting::Hour => format!("{:02}:{:02}", v as i64, ((v - v.trunc()) * 60.0 + 0.5) as i64),
            Setting::Visibility if v < 10.0 => format!("{v:.1} km"),
            Setting::Visibility => format!("{v:.0} km"),
            Setting::CloudBase => format!("{v:.0} ft"),
            Setting::Pressure => format!("{v:.0} hPa"),
            Setting::Temperature => format!("{v:+.0} °C"),
        }
    }

    pub fn get(self, c: &Conditions) -> f64 {
        match self {
            Setting::WindFrom => c.weather.wind_from,
            Setting::WindSpeed => c.weather.wind_speed / KNOTS,
            Setting::Turbulence => c.weather.turbulence * 100.0,
            Setting::Hour => *c.hour,
            Setting::Visibility => c.weather.visibility / 1000.0,
            Setting::Humidity => c.weather.air.relative_humidity * 100.0,
            Setting::CloudCover => *c.cloud_cover * 100.0,
            Setting::CloudBase => *c.cloud_altitude / FEET,
            Setting::Pressure => c.weather.air.sea_level_pressure / HPA,
            Setting::Temperature => c.weather.air.sea_level_temperature - 273.15,
        }
    }

    pub fn set(self, c: &mut Conditions, v: f64) {
        match self {
            Setting::WindFrom => c.weather.wind_from = v,
            Setting::WindSpeed => c.weather.wind_speed = v * KNOTS,
            Setting::Turbulence => c.weather.turbulence = v / 100.0,
            Setting::Hour => *c.hour = v,
            Setting::Visibility => c.weather.visibility = v * 1000.0,
            Setting::Humidity => c.weather.air.relative_humidity = v / 100.0,
            Setting::CloudCover => *c.cloud_cover = v / 100.0,
            Setting::CloudBase => *c.cloud_altitude = v * FEET,
            Setting::Pressure => c.weather.air.sea_level_pressure = v * HPA,
            Setting::Temperature => c.weather.air.sea_level_temperature = v + 273.15,
        }
    }
}

/// The folder of the player's files (the settings, the controllers' mappings):
/// `$XDG_CONFIG_HOME/flight` or `~/.config/flight`; `None` if no home folder is known. On the web,
/// `flight`: the files are kept in the browser under `flight/<name>` (`engine::files::save_text`).
pub fn config_dir() -> Option<PathBuf> {
    if cfg!(target_arch = "wasm32") {
        return Some(PathBuf::from("flight"));
    }
    let config = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from);
    let home = std::env::var_os("HOME").filter(|v| !v.is_empty()).map(|h| PathBuf::from(h).join(".config"));
    Some(config.or(home)?.join("flight"))
}

/// The settings file. Every `set` writes it again.
pub struct Store {
    path: Option<PathBuf>,
    values: BTreeMap<String, String>,
}

impl Store {
    /// Remembers nothing between runs: for screenshots and tests.
    pub fn memory() -> Store {
        Store { path: None, values: BTreeMap::new() }
    }

    /// Reads the file at `path` if there is one; changes are written back to it.
    pub fn open(path: PathBuf) -> Store {
        let values = engine::files::load_text(&path).map(|text| parse(&text)).unwrap_or_default();
        Store { path: Some(path), values }
    }

    /// The player's settings file, or nothing remembered if no home directory is known.
    pub fn user() -> Store {
        match config_dir() {
            Some(dir) => Store::open(dir.join("settings")),
            None => Store::memory(),
        }
    }

    pub fn number(&self, key: &str) -> Option<f64> {
        self.values.get(key)?.parse::<f64>().ok().filter(|v| v.is_finite())
    }

    pub fn word(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn flag(&self, key: &str) -> Option<bool> {
        match self.values.get(key)?.as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        }
    }

    /// Remembers a value and writes the file.
    pub fn set(&mut self, key: &str, value: impl ToString) {
        self.values.insert(key.to_string(), value.to_string());
        let Some(path) = &self.path else { return };
        let written = engine::files::save_text(path, &self.text());
        if let Err(error) = written {
            eprintln!("settings: could not write {}: {error}", path.display());
        }
    }

    /// The contents of the file.
    pub fn text(&self) -> String {
        self.values.iter().map(|(k, v)| format!("{k}={v}\n")).collect()
    }

    /// Applies every saved slider, kept inside its range.
    pub fn restore(&self, c: &mut Conditions) {
        for s in Setting::ALL {
            if let Some(v) = self.number(s.key()) {
                let (min, max) = s.range();
                s.set(c, v.clamp(min, max));
            }
        }
        if let Some(storms) = self.number(STORMS) {
            *c.storms = storms.clamp(0.0, 3.0) as u32;
        }
    }
}

/// `key=value` lines; blank lines, lines without `=` and lines starting with `#` are skipped.
fn parse(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

/// The key of the setting that shows the yoke box and the pedal bar; off unless chosen.
pub const CONTROLS_SHOWN: &str = "controls_shown";

/// The key of the setting that makes the engine use up its fuel; off unless chosen, so a flight never
/// runs dry.
pub const FUEL_BURNS: &str = "fuel_burns";
/// The key of the setting that draws the yokes in the cockpit; on unless taken away.
pub const YOKES_SHOWN: &str = "yokes";

/// The key of the setting that draws the propeller (its blades, or its blur turning) in the cockpit's
/// view; on unless taken away. Outside it is always drawn.
pub const PROPELLER_SHOWN: &str = "propeller";
/// The key of the setting that turns the mouse's look the natural way round: dragging takes the
/// scene with the pointer (down looks up, right looks left); off unless chosen.
pub const LOOK_NATURAL: &str = "look_natural";

/// The key of the setting that starts the game at the hour of the computer's clock
/// (`game::Options::clock`); on unless taken away. Off, it starts at the hour the settings keep.
pub const LOCAL_TIME: &str = "local_time";

/// The key of the setting that shows the frame rate in the status line (key F3).
pub const FPS_SHOWN: &str = "fps_shown";

#[cfg(test)]
mod tests {
    use super::*;

    struct Values {
        weather: Weather,
        hour: f64,
        cloud_cover: f64,
        cloud_altitude: f64,
        storms: u32,
    }

    impl Values {
        fn new() -> Values {
            Values { weather: Weather::default(), hour: 10.5, cloud_cover: 0.63, cloud_altitude: 2300.0, storms: 0 }
        }
        fn conditions(&mut self) -> Conditions<'_> {
            Conditions {
                weather: &mut self.weather,
                hour: &mut self.hour,
                cloud_cover: &mut self.cloud_cover,
                cloud_altitude: &mut self.cloud_altitude,
                storms: &mut self.storms,
            }
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flight-settings-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("flight").join("settings")
    }

    #[test]
    fn file_round_trip() {
        let path = scratch("round-trip");
        let mut store = Store::open(path.clone());
        assert_eq!(store.number("windFrom"), None);
        store.set("windFrom", 135.0);
        store.set("hour", 18.25);
        store.set("assist", false);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "assist=false\nhour=18.25\nwindFrom=135\n");

        let again = Store::open(path.clone());
        assert_eq!(again.number("windFrom"), Some(135.0));
        assert_eq!(again.number("hour"), Some(18.25));
        assert_eq!(again.flag("assist"), Some(false));
        assert_eq!(again.flag("hour"), None);
        assert_eq!(again.number("cloudBase"), None);
        std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn restore_applies_units_and_limits() {
        let mut store = Store::memory();
        store.set("windSpeed", 12.0);
        store.set("turbulence", 35.0);
        store.set("visibility", 500.0); // beyond the slider: clamped to 200 km
        store.set("cloudBase", 4000.0);
        store.set("cloudCover", "nonsense");
        let mut values = Values::new();
        store.restore(&mut values.conditions());
        assert!((values.weather.wind_speed - 12.0 * KNOTS).abs() < 1e-12);
        assert!((values.weather.turbulence - 0.35).abs() < 1e-12);
        assert_eq!(values.weather.visibility, 200_000.0);
        assert!((values.cloud_altitude - 4000.0 * FEET).abs() < 1e-9);
        assert_eq!(values.cloud_cover, 0.63);
        assert_eq!(values.weather.wind_from, 250.0);
    }

    #[test]
    fn the_kind_of_day_is_known_while_its_sliders_are_as_it_set_them() {
        let mut values = Values::new();
        assert_eq!(Sky::current(&values.conditions()), None);
        for sky in Sky::ALL {
            sky.apply(&mut values.conditions(), None);
            assert_eq!(Sky::current(&values.conditions()), Some(sky));
        }
        // The hour and the wind's direction are not a kind of day's; the wind's speed is.
        Setting::Hour.set(&mut values.conditions(), 20.0);
        Setting::WindFrom.set(&mut values.conditions(), 90.0);
        assert_eq!(Sky::current(&values.conditions()), Some(Sky::Storms));
        Setting::WindSpeed.set(&mut values.conditions(), 30.0);
        assert_eq!(Sky::current(&values.conditions()), None);
    }

    #[test]
    fn every_setting_reads_back_what_was_set() {
        let mut values = Values::new();
        let mut c = values.conditions();
        for s in Setting::ALL {
            let (min, max) = s.range();
            for v in [min, max, min + s.step()] {
                s.set(&mut c, v);
                assert!((s.get(&c) - v).abs() < 1e-9, "{s:?} {v}");
            }
        }
    }

    #[test]
    fn parsing_skips_what_is_not_a_setting() {
        let values = parse("# comment\n\n hour = 7.5 \nbroken line\n=3\nassist=1\n");
        assert_eq!(values.len(), 2);
        assert_eq!(values["hour"], "7.5");
        assert_eq!(values["assist"], "1");
    }

    #[test]
    fn values_are_shown_as_in_the_original() {
        assert_eq!(Setting::WindFrom.format(5.0), "005°");
        assert_eq!(Setting::WindSpeed.format(12.0), "12 kt");
        assert_eq!(Setting::Turbulence.format(35.0), "35 %");
        assert_eq!(Setting::Hour.format(18.25), "18:15");
        assert_eq!(Setting::Hour.format(7.0), "07:00");
        assert_eq!(Setting::Visibility.format(45.0), "45 km");
        assert_eq!(Setting::CloudBase.format(7500.0), "7500 ft");
        assert_eq!(Setting::Pressure.format(1013.0), "1013 hPa");
        assert_eq!(Setting::Temperature.format(-5.0), "-5 °C");
    }
}
