//! "Choose place": the screen where the player chooses where to fly (key M, "Place" on the start
//! and pause screen or in the settings, or at the start). On the left, a search of the aerodromes
//! of OurAirports (`geodata::search`), the favourites and the places flown lately, the featured
//! places (`geodata::PRESETS` and the generated world) and the race courses as cards with their
//! pictures (`ui::menu::Pictures`), each saying whether it is already downloaded or how much it
//! needs, and how to start: on a runway (the end into the wind, or one chosen), on final some
//! miles out, or in the air over the place. On the right, a map of the world that the mouse drags
//! and zooms (`map`): the featured places as pins with their names, the aerodromes by size, their
//! runways when near; a click picks a pin, an aerodrome, or any point (to start in the air over
//! it). "Fly" hands the choice to the game (`places::Choice`). It wears the menus' look (`ui::menu::theme`).

mod map;
mod tiles;

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use engine::egui::{self, pos2, vec2, Align, Align2, Color32, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke};
use engine::Instant;
use geodata::cache::Need;
use geodata::search::{Airfield, Index, Size};
use geodata::Place;

use crate::places::{self, Choice, Start};
use crate::settings::Store;
use crate::ui::menu::theme::{self, icon};
use crate::ui::menu::{no_picture, Pictures};
pub use map::View;

/// The width of the column on the left, points.
const SIDE: f32 = 436.0;
/// The height of a place's card with its picture, points.
const CARD: f32 = 118.0;
/// How near the map goes to an aerodrome chosen in the list: its runways across a good part of it.
const AERODROME_ZOOM: f64 = 12.5;
/// Half the side of the square a region covers round its place, metres (`geodata`'s maps).
const REGION_HALF: f64 = 48_000.0;

/// What is chosen on the screen.
#[derive(Clone, Debug, PartialEq)]
pub enum Selected {
    /// The world of the seed.
    Generated,
    /// An aerodrome, by its place in the index.
    Airfield(usize),
    /// A point of the Earth, degrees.
    Point(f64, f64),
    /// A race course, by its place in `course::file::courses`: its world, raced in a quad.
    Course(usize),
}

/// How to start, as the radio buttons choose it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Runway,
    Final,
    Air,
}

/// The aerodromes, being read on a thread of their own or read.
enum Aerodromes {
    Loading(Receiver<Result<Index, geodata::Error>>),
    Ready(Arc<Index>),
    Failed(String),
}

/// How the picker opens when the game starts: as left (the setting "Open this screen at
/// start"), or as `--picker` with `--map`, `--search` and `--select` say, for a screenshot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Opening {
    /// Where the map looks; else at what is chosen.
    pub view: Option<View>,
    /// What is typed in the search.
    pub query: String,
    /// The place chosen (as the settings name it); else the one flown.
    pub select: Option<String>,
}

/// What the game tells the picker each frame.
pub struct Context<'a> {
    pub store: &'a mut Store,
    /// The place flown now as the settings name it; `None`, the generated world.
    pub current: Option<&'a str>,
    /// How the loading of a place chosen before goes, if one is loading.
    pub loading: Option<String>,
    /// Where the wind blows from, degrees: the runway end into it is the one offered first.
    pub wind_from: f64,
    /// The pictures of the featured places and the courses.
    pub pictures: &'a mut crate::ui::menu::Pictures,
}

pub struct Picker {
    aerodromes: Aerodromes,
    /// Each aerodrome's point on the map's square, by its place in the index.
    points: Vec<(f64, f64)>,
    tiles: map::Tiles,
    pub view: View,
    /// What is typed in the search, and the aerodromes found for it (places in the index).
    pub query: String,
    searched: Option<String>,
    found: Vec<usize>,
    /// How long the last search took, milliseconds.
    pub search_ms: f64,
    pub selected: Option<Selected>,
    /// A place named by its key (a favourite, a recent one) waiting for the index to be chosen.
    pending: Option<String>,
    /// The view asked for with it, which the place chosen must not move.
    pending_view: Option<View>,
    mode: Mode,
    /// The runway end chosen, by its place among the aerodrome's ends; `None`, into the wind.
    end: Option<usize>,
    miles: f64,
    feet: f64,
    /// What the cache holds of each place shown, by key; worked out a few a frame.
    needs: Needs,
    /// The search box takes the keyboard when the screen opens.
    focus: bool,
    /// Tiles of the map not drawn in the last frame (still loading).
    missing: usize,
    /// Milliseconds the screen took to build, the last frame and on average.
    pub cost: (f64, f64),
}

impl Picker {
    /// Starts reading the aerodromes (from the cache, or downloading them the first time) on a
    /// thread of its own. The map starts on `around` (latitude, longitude) if given, else the
    /// whole world.
    pub fn new(around: Option<(f64, f64)>) -> Picker {
        let (sender, receiver) = channel();
        engine::threads::spawn(move || {
            let fetcher = geodata::fetch::Fetcher::new(geodata::fetch::cache_dir(), false);
            let _ = sender.send(Index::load(&fetcher));
        });
        let view = match around {
            Some((lat, lon)) => View::at(lat, lon, 7.0),
            None => View { centre: (0.5, 0.42), zoom: 0.0 },
        };
        Picker {
            aerodromes: Aerodromes::Loading(receiver),
            points: Vec::new(),
            tiles: map::Tiles::new(false),
            view,
            query: String::new(),
            searched: None,
            found: Vec::new(),
            search_ms: 0.0,
            selected: None,
            pending: None,
            pending_view: None,
            mode: Mode::Runway,
            end: None,
            miles: 3.0,
            feet: 3000.0,
            needs: Needs::default(),
            focus: true,
            missing: 0,
            cost: (0.0, 0.0),
        }
    }

    /// The screen is opened again: the search box takes the keyboard, what is on disk is looked at
    /// anew, and the choice starts from the place flown now.
    pub fn reopen(&mut self, current: Option<&str>) {
        self.focus = true;
        self.needs.clear();
        self.choose_key(current);
    }

    /// Opens as `opening` says, the place flown now being `current`.
    pub fn open(&mut self, opening: &Opening, current: Option<&str>) {
        self.reopen(opening.select.as_deref().or(current));
        self.query = opening.query.clone();
        if let Some(view) = opening.view {
            self.view = view;
            // Over the view of a place chosen once the aerodromes are read.
            self.pending_view = self.pending.is_some().then_some(view);
        }
    }

    fn index(&self) -> Option<&Arc<Index>> {
        match &self.aerodromes {
            Aerodromes::Ready(index) => Some(index),
            _ => None,
        }
    }

    /// Waiting for something before the picture is whole: the aerodromes, tiles of the map.
    pub fn pending(&self) -> usize {
        matches!(self.aerodromes, Aerodromes::Loading(_)) as usize + self.tiles.loading() + self.missing
    }

    /// The map's tiles kept in memory, and their bytes.
    pub fn tiles_kept(&self) -> (usize, usize) {
        self.tiles.kept()
    }

    /// Takes in what finished in the background.
    fn update(&mut self) {
        if let Aerodromes::Loading(receiver) = &self.aerodromes {
            match receiver.try_recv() {
                Ok(Ok(index)) => {
                    self.points = index.airfields.iter().map(|a| map::mercator(a.latitude, a.longitude)).collect();
                    self.aerodromes = Aerodromes::Ready(Arc::new(index));
                    self.searched = None;
                    if let Some(key) = self.pending.take() {
                        self.choose_key(Some(&key));
                    }
                    if let Some(view) = self.pending_view.take() {
                        self.view = view;
                    }
                }
                Ok(Err(error)) => self.aerodromes = Aerodromes::Failed(error.to_string()),
                Err(_) => {}
            }
        }
        self.tiles.update();
        if self.searched.as_deref() != Some(self.query.as_str()) {
            if let Some(index) = self.index().cloned() {
                let start = Instant::now();
                self.found = index.search(&self.query, 60);
                self.search_ms = start.elapsed().as_secs_f64() * 1000.0;
                self.searched = Some(self.query.clone());
            }
        }
    }

    /// Chooses the place a key of the settings names (`None` the generated world), and shows it on
    /// the map; an aerodrome waits for the index if it is not read yet.
    pub fn choose_key(&mut self, key: Option<&str>) {
        let Some(key) = key else {
            self.select(Selected::Generated);
            return;
        };
        if let Some((lat, lon)) = places::point_of(key) {
            self.select(Selected::Point(lat, lon));
            self.view = View::at(lat, lon, 8.5);
            return;
        }
        let found = self.index().and_then(|index| index.position(key).map(|i| (i, index.airfields[i].latitude, index.airfields[i].longitude)));
        match found {
            Some((i, lat, lon)) => {
                self.select(Selected::Airfield(i));
                self.view = View::at(lat, lon, AERODROME_ZOOM);
            }
            None if self.index().is_none() => self.pending = Some(key.to_string()),
            None => {}
        }
    }

    fn select(&mut self, selected: Selected) {
        if self.selected.as_ref() != Some(&selected) {
            self.end = None;
            if matches!(selected, Selected::Point(..)) {
                self.mode = Mode::Air;
            }
        }
        self.selected = Some(selected);
    }

    fn airfield(&self, i: usize) -> Option<&Airfield> {
        self.index().and_then(|index| index.airfields.get(i))
    }

    /// The settings' key of what is chosen (`None` the generated world).
    fn key_of(&self, selected: &Selected) -> Option<String> {
        match selected {
            Selected::Generated | Selected::Course(_) => None,
            Selected::Airfield(i) => self.airfield(*i).map(|a| a.code.clone()),
            Selected::Point(lat, lon) => Some(places::point_key(*lat, *lon)),
        }
    }

    /// The ends of the chosen aerodrome's runways: their ident and true heading.
    fn ends(&self) -> Vec<(String, f64)> {
        match &self.selected {
            Some(Selected::Airfield(i)) => self.airfield(*i).map_or(Vec::new(), |a| a.runways.iter().flat_map(|r| r.ends.iter().map(|e| (e.ident.clone(), e.heading))).collect()),
            _ => Vec::new(),
        }
    }

    /// How the player will start, as chosen.
    pub fn start(&self, wind_from: f64) -> Start {
        let ends = self.ends();
        let headings: Vec<f64> = ends.iter().map(|e| e.1).collect();
        let heading = self.end.or_else(|| places::into_wind(&headings, wind_from)).and_then(|i| headings.get(i).copied());
        match (self.mode, &self.selected) {
            (_, Some(Selected::Point(..))) | (Mode::Air, _) => Start::Air { feet: self.feet },
            (Mode::Runway, _) => Start::Runway { heading },
            (Mode::Final, _) => Start::Final { heading, miles: self.miles },
        }
    }

    /// What the cache holds of a place, worked out the first time it is asked (`None` until then,
    /// and for the generated world).
    fn need(&mut self, key: &str, budget: &mut usize) -> Option<Need> {
        if let Some(need) = self.needs.get(key) {
            return Some(need);
        }
        if *budget == 0 {
            return None;
        }
        let place = Place::parse(key)?;
        let (lat, lon) = match &place {
            Place::At(lat, lon) => (*lat, *lon),
            Place::Aerodrome(code) => {
                let a = self.index()?.find(code)?;
                (a.latitude, a.longitude)
            }
        };
        *budget -= 1;
        self.needs.work_out(key, place, lat, lon);
        self.needs.get(key)
    }

    /// Draws the screen over the whole window. `open` turns false when it is closed; the choice is
    /// returned when "Volar" is pressed.
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool, game: Context) -> Option<Choice> {
        let start = Instant::now();
        self.update();
        let screen = ctx.content_rect();
        let mut choice = None;
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            *open = false;
        }
        let Context { store, current, loading, wind_from, pictures } = game;
        egui::Area::new(egui::Id::unique("place_picker")).fixed_pos(screen.min).movable(false).order(egui::Order::Middle).show(ctx, |ui| {
            let mut whole = ui.new_child(egui::UiBuilder::new().max_rect(screen));
            whole.set_clip_rect(screen);
            egui::Panel::left("place_picker_list").exact_size(SIDE).resizable(false).frame(egui::Frame::new().fill(Color32::from_rgb(14, 17, 24)).inner_margin(18.0)).show(&mut whole, |ui| {
                theme::style(ui);
                choice = self.side(ui, open, store, current, loading, wind_from, pictures);
            });
            egui::CentralPanel::no_frame().show(&mut whole, |ui| self.map(ui, pictures));
        });
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        self.cost = (ms, if self.cost.1 == 0.0 { ms } else { self.cost.1 * 0.95 + ms * 0.05 });
        choice
    }

    // ---- The column on the left

    #[allow(clippy::too_many_arguments)]
    fn side(&mut self, ui: &mut egui::Ui, open: &mut bool, store: &mut Store, current: Option<&str>, loading: Option<String>, wind_from: f64, pictures: &mut Pictures) -> Option<Choice> {
        let mut choice = None;
        // The head: what the screen is, and how to leave it.
        let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
        let painter = ui.painter().clone();
        theme::icon(&painter, pos2(head.left() + 13.0, head.center().y), icon::PIN, 25.0, theme::ACCENT);
        theme::text(&painter, pos2(head.left() + 34.0, head.center().y), Align2::LEFT_CENTER, "Choose place", theme::bold(25.0), theme::TEXT);
        let close = Rect::from_center_size(pos2(head.right() - 16.0, head.center().y), vec2(32.0, 32.0));
        let response = ui.interact(close, ui.scope_id().with("close"), Sense::click());
        painter.rect_filled(close, 9.0, if response.hovered() { theme::CARD_HOVER } else { theme::CARD });
        theme::icon(&painter, close.center(), icon::CLOSE, 16.0, theme::TEXT);
        if response.on_hover_text("Close (Esc)").clicked() {
            *open = false;
        }
        theme::key_cap(&painter, pos2(close.left() - 10.0, head.center().y), "Esc");
        ui.add_space(10.0);
        // The search: a lens, then the box.
        ui.horizontal(|ui| {
            let (lens, _) = ui.allocate_exact_size(vec2(22.0, 32.0), Sense::hover());
            theme::icon(ui.painter(), lens.center(), icon::SEARCH, 18.0, theme::DIM);
            let search = ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("Aerodrome, city or ICAO / IATA code").desired_width(f32::INFINITY).margin(egui::Margin::symmetric(10, 8)));
            if std::mem::take(&mut self.focus) {
                search.request_focus();
            }
        });
        ui.add_space(4.0);

        let mut budget = 3;
        let list_height = (ui.available_height() - 296.0).max(150.0);
        egui::ScrollArea::vertical().max_height(list_height).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(ui.available_width() - 6.0);
            if self.query.trim().is_empty() {
                self.lists(ui, store, current, pictures, &mut budget);
            } else {
                self.results(ui, store, &mut budget);
            }
        });
        ui.add_space(8.0);
        if let Some(c) = self.chosen(ui, wind_from, pictures, &mut budget) {
            choice = Some(c);
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            let mut at_start = store.flag(places::PICKER_AT_START).unwrap_or(false);
            if ui.checkbox(&mut at_start, RichText::new("Open this screen at start").font(theme::regular(13.0)).color(theme::DIM)).changed() {
                store.set(places::PICKER_AT_START, at_start);
            }
            if let Some(text) = &loading {
                ui.label(RichText::new(text).color(theme::WARM));
            }
        });
        choice
    }

    /// The favourites and the places flown lately as rows; the featured places and the race courses
    /// as cards with their pictures, two across.
    fn lists(&mut self, ui: &mut egui::Ui, store: &mut Store, current: Option<&str>, pictures: &mut Pictures, budget: &mut usize) {
        for (key, title, glyph) in [(places::FAVOURITES, "Favourites", icon::CHECK), (places::RECENT, "Recent", icon::CLOCK)] {
            let list = places::list(store, key);
            if !list.is_empty() {
                heading(ui, glyph, title);
                for key in list {
                    self.key_row(ui, store, current, &key, None, budget);
                }
            }
        }
        heading(ui, icon::GLOBE, "Featured places");
        let width = ui.available_width();
        let gap = 10.0;
        let size = vec2((width - gap) / 2.0, CARD);
        // The seed's world first, then the real places.
        let featured: Vec<Option<&geodata::Preset>> = std::iter::once(None).chain(geodata::PRESETS.iter().map(Some)).collect();
        for pair in featured.chunks(2) {
            let (row, _) = ui.allocate_exact_size(vec2(width, CARD), Sense::hover());
            for (k, preset) in pair.iter().enumerate() {
                let cell = Rect::from_min_size(pos2(row.left() + (size.x + gap) * k as f32, row.top()), size);
                match preset {
                    None => {
                        let look = CardLook { picture: pictures.place(ui.ctx(), "generated"), glyph: icon::GLOBE, title: "Generated world", what: "Islands, mountains, aerodromes", need: Some(("no download".to_string(), true)), selected: self.selected == Some(Selected::Generated), here: current.is_none(), favourite: None };
                        if place_card(ui, cell, "generated", &look).clicked {
                            self.select(Selected::Generated);
                        }
                    }
                    Some(preset) => {
                        let (title, what) = preset.title.split_once(": ").unwrap_or((preset.title, ""));
                        let selected = self.selected.as_ref().is_some_and(|s| self.key_of(s).as_deref() == Some(preset.code));
                        let favourite = places::list(store, places::FAVOURITES).iter().any(|k| k == preset.code);
                        let need = self.need(preset.code, budget).map(need_text);
                        let look = CardLook { picture: pictures.place(ui.ctx(), preset.code), glyph: icon::PIN, title, what, need, selected, here: current == Some(preset.code), favourite: Some(favourite) };
                        let card = place_card(ui, cell, preset.code, &look);
                        if card.starred {
                            places::toggle_favourite(store, preset.code);
                        }
                        if card.clicked {
                            self.choose_key(Some(preset.code));
                        }
                    }
                }
            }
            ui.add_space(gap);
        }
        heading(ui, icon::FLAG, "Race courses");
        let courses = sim_core::course::file::courses();
        for (line, pair) in courses.chunks(2).enumerate() {
            let (row, _) = ui.allocate_exact_size(vec2(width, CARD), Sense::hover());
            for (k, course) in pair.iter().enumerate() {
                let i = line * 2 + k;
                let cell = Rect::from_min_size(pos2(row.left() + (size.x + gap) * k as f32, row.top()), size);
                let best = store.number(&crate::race::best_key(course)).map(|t| (format!("best {}", crate::ui::osd::lap_time(t as f32)), true));
                let what = format!("{} gates · {} laps", course.gates.len(), course.laps);
                let look = CardLook { picture: pictures.place(ui.ctx(), &course.key), glyph: icon::FLAG, title: &course.name, what: &what, need: best.or(Some(("race".to_string(), false))), selected: self.selected == Some(Selected::Course(i)), here: false, favourite: None };
                if place_card(ui, cell, &course.key, &look).clicked {
                    self.select(Selected::Course(i));
                }
            }
            ui.add_space(gap);
        }
    }

    /// One place named by its key, with its star and what the cache holds of it.
    fn key_row(&mut self, ui: &mut egui::Ui, store: &mut Store, current: Option<&str>, key: &str, titles: Option<(&str, &str)>, budget: &mut usize) {
        let airfield = self.index().and_then(|index| index.find(key).map(|a| (a.name.clone(), a.where_it_is())));
        let (title, what) = match (titles, &airfield, places::point_of(key)) {
            (Some((t, w)), _, _) => (t.to_string(), w.to_string()),
            (None, Some((name, place)), _) => (format!("{key} · {name}"), place.clone()),
            (None, None, Some((lat, lon))) => (format!("Point {lat:.3}, {lon:.3}"), "in flight above it".to_string()),
            (None, None, None) => (key.to_string(), String::new()),
        };
        let selected = match &self.selected {
            Some(s) => self.key_of(s).as_deref() == Some(key),
            None => false,
        };
        let favourite = places::list(store, places::FAVOURITES).iter().any(|k| k == key);
        let need = self.need(key, budget).map(need_text);
        let row = place_row(ui, selected, current == Some(key), Some(favourite), &title, &what, need);
        if row.starred {
            places::toggle_favourite(store, key);
        }
        if row.clicked {
            self.choose_key(Some(key));
        }
    }

    /// The aerodromes found for what is typed.
    fn results(&mut self, ui: &mut egui::Ui, store: &mut Store, budget: &mut usize) {
        let Some(index) = self.index().cloned() else {
            match &self.aerodromes {
                Aerodromes::Failed(error) => ui.label(format!("Could not read the list of aerodromes: {error}")),
                _ => ui.label("Reading the list of aerodromes…"),
            };
            return;
        };
        if self.found.is_empty() {
            ui.label(RichText::new("No aerodrome with that name or code.").weak());
        }
        for i in self.found.clone() {
            let a = &index.airfields[i];
            let selected = self.selected == Some(Selected::Airfield(i));
            let favourite = places::list(store, places::FAVOURITES).contains(&a.code);
            let runways = match a.runways.len() {
                0 => "no runway".to_string(),
                1 => "1 runway".to_string(),
                n => format!("{n} runways"),
            };
            let codes = match &a.iata {
                Some(iata) => format!("{} {iata}", a.code),
                None => a.code.clone(),
            };
            let need = self.need(&a.code, budget).map(need_text);
            let row = place_row(ui, selected, false, Some(favourite), &format!("{codes} · {}", a.name), &format!("{} · {} · {runways}", a.where_it_is(), a.size.name()), need);
            if row.starred {
                places::toggle_favourite(store, &a.code);
            }
            if row.clicked {
                self.select(Selected::Airfield(i));
                self.view = View::at(a.latitude, a.longitude, AERODROME_ZOOM);
            }
        }
    }

    /// What is chosen, how to start there, and "Fly".
    fn chosen(&mut self, ui: &mut egui::Ui, wind_from: f64, pictures: &mut Pictures, budget: &mut usize) -> Option<Choice> {
        let Some(selected) = self.selected.clone() else {
            ui.label(RichText::new("Choose a place in the list or on the map.").color(theme::DIM));
            return None;
        };
        let key = self.key_of(&selected);
        if let Selected::Course(i) = selected {
            return Self::course(ui, i, pictures);
        }
        let (title, what) = match &selected {
            Selected::Generated | Selected::Course(_) => ("Generated world".to_string(), "The world from the seed: islands, mountains and aerodromes".to_string()),
            Selected::Airfield(i) => {
                let a = self.airfield(*i)?;
                (format!("{} · {}", a.code, a.name), format!("{} · {}{}", a.where_it_is(), a.size.name(), a.elevation.map_or(String::new(), |e| format!(" · {:.0} ft", e / sim_core::FEET))))
            }
            Selected::Point(lat, lon) => (format!("Point {lat:.3}, {lon:.3}"), "Any place: you start in flight above it".to_string()),
        };
        let picture = match &selected {
            Selected::Generated => pictures.place(ui.ctx(), "generated"),
            _ => key.as_deref().and_then(|k| pictures.place(ui.ctx(), k)),
        };
        let glyph = if matches!(selected, Selected::Point(..)) { icon::IN_FLIGHT } else { icon::PIN };
        chosen_head(ui, picture, glyph, &title, &what);
        ui.add_space(8.0);

        let ends = self.ends();
        let has_runway = !matches!(selected, Selected::Point(..)) && (selected == Selected::Generated || !ends.is_empty());
        if !has_runway {
            self.mode = Mode::Air;
        }
        let headings: Vec<f64> = ends.iter().map(|e| e.1).collect();
        let windward = places::into_wind(&headings, wind_from);
        let end_name = |i: usize| format!("{} ({:03.0}°)", ends[i].0, ends[i].1);
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.add_enabled_ui(has_runway, |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.mode, Mode::Runway, "On the runway");
                ui.add_space(4.0);
                let into = match windward {
                    Some(i) => format!("into the wind: {}", end_name(i)),
                    None => "into the wind".to_string(),
                };
                let shown = self.end.map_or(into.clone(), end_name);
                ui.add_enabled_ui(self.mode != Mode::Air, |ui| {
                    egui::ComboBox::from_id_salt("runway_end").selected_text(shown).width(200.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.end, None, into);
                        for i in 0..ends.len() {
                            ui.selectable_value(&mut self.end, Some(i), end_name(i));
                        }
                    });
                });
            });
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.mode, Mode::Final, "On final at");
                ui.add(egui::DragValue::new(&mut self.miles).range(1.0..=10.0).speed(0.1).fixed_decimals(1).suffix(" NM"));
                ui.label(RichText::new("from that end").color(theme::DIM));
            });
        });
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.mode, Mode::Air, "In flight at");
            ui.add(egui::DragValue::new(&mut self.feet).range(500.0..=12_000.0).speed(50.0).fixed_decimals(0).suffix(" ft"));
            ui.label(RichText::new("above the ground").color(theme::DIM));
        });
        ui.add_space(6.0);
        let need = key.as_deref().and_then(|k| self.need(k, budget));
        let note = match (&key, need) {
            (None, _) => "No download: starts at once".to_string(),
            (Some(_), Some(Need { kept: true, .. })) => "Already downloaded: loads at once".to_string(),
            (Some(_), Some(n)) => format!("About {:.0} MB to download the first time", (n.bytes as f64 / 1e6).max(1.0)),
            (Some(_), None) => String::new(),
        };
        big_button(ui, "fly", icon::TAKEOFF, "Fly here", &note, theme::ACCENT_FILL).then(|| Choice { key: key.clone(), start: self.start(wind_from) })
    }

    /// A race course chosen: what it is, and "Race".
    fn course(ui: &mut egui::Ui, index: usize, pictures: &mut Pictures) -> Option<Choice> {
        let course = sim_core::course::file::courses().get(index)?;
        chosen_head(ui, pictures.place(ui.ctx(), &course.key), icon::FLAG, &course.name, &course.description);
        ui.add_space(8.0);
        ui.label(RichText::new(format!("{} gates, {} laps, in an FPV quad. On the start's pad: 3, 2, 1, GO. Key 1 starts it again; L lets the autopilot race.", course.gates.len(), course.laps)).font(theme::regular(13.0)).color(theme::DIM));
        ui.add_space(8.0);
        big_button(ui, "race", icon::FLAG, "Race", "No download: starts at once", Color32::from_rgb(214, 84, 44)).then_some(Choice { key: None, start: Start::Course { index } })
    }

    // ---- The map

    fn map(&mut self, ui: &mut egui::Ui, pictures: &mut Pictures) {
        let rect = ui.max_rect();
        self.view.keep_in(rect);
        let response = ui.interact(rect, ui.scope_id().with("map"), Sense::click_and_drag());
        if response.dragged() {
            self.view.pan(rect, response.drag_delta());
        }
        if response.hovered() {
            let (scroll, zoom, pointer) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
            let at = pointer.unwrap_or(rect.center());
            // The wheel, or two fingers up and down, zooms; sideways they move the map; a pinch zooms.
            if scroll.y != 0.0 {
                self.view.zoom_about(rect, at, scroll.y as f64 / 120.0);
            }
            if scroll.x != 0.0 {
                self.view.pan(rect, vec2(scroll.x, 0.0));
            }
            if zoom != 1.0 {
                self.view.zoom_about(rect, at, (zoom as f64).log2());
            }
        }
        if response.double_clicked() {
            if let Some(at) = response.interact_pointer_pos() {
                self.view.zoom_about(rect, at, 1.0);
            }
        }

        let painter = ui.painter_at(rect);
        // Names stay clear of the zoom buttons and of the scale and the credits at the bottom.
        let mut taken = vec![Rect::from_min_max(pos2(rect.right() - 56.0, rect.top()), pos2(rect.right(), rect.top() + 100.0)), Rect::from_min_max(pos2(rect.left(), rect.bottom() - 32.0), rect.max)];
        let (missing, labels) = map::draw(&painter, rect, &self.view, &mut self.tiles);
        self.missing = missing;
        if let Some(Selected::Airfield(_) | Selected::Point(..)) = &self.selected {
            self.region_square(&painter, rect);
        }
        let hovered = self.airfields(&painter, rect, response.hover_pos(), &mut taken);
        let pin = self.pins(&painter, rect, response.hover_pos(), &mut taken);
        map::draw_labels(&painter, rect, labels, &mut taken);
        self.marker(&painter, rect);

        if response.clicked() {
            match (hovered, response.interact_pointer_pos()) {
                _ if pin.is_some() => self.choose_key(pin.map(|p| p.code)),
                (Some(i), _) => self.select(Selected::Airfield(i)),
                (None, Some(at)) => {
                    let (lat, lon) = map::earth(self.view.map(rect, at).0, self.view.map(rect, at).1);
                    self.select(Selected::Point(lat.clamp(-84.0, 84.0), lon.clamp(-180.0, 180.0)));
                }
                _ => {}
            }
        }
        if let (Some(preset), Some(at)) = (pin, response.hover_pos()) {
            // A featured place under the pointer: its picture and what it is.
            let (title, what) = preset.title.split_once(": ").unwrap_or((preset.title, ""));
            let size = vec2(236.0, 190.0);
            let corner = pos2((at.x + 18.0).min(rect.right() - size.x - 8.0), (at.y + 14.0).min(rect.bottom() - size.y - 40.0));
            let card = Rect::from_min_size(corner, size);
            theme::glass(&painter, card, 12.0, theme::SHEET, 1.0);
            let frame = Rect::from_min_size(card.min + vec2(6.0, 6.0), vec2(size.x - 12.0, 128.0));
            match pictures.place(ui.ctx(), preset.code) {
                Some(texture) => theme::picture(&painter, frame, 8.0, &texture, Color32::WHITE),
                None => no_picture(&painter, frame, 8.0, icon::PIN),
            }
            theme::text(&painter, pos2(card.left() + 12.0, frame.bottom() + 8.0), Align2::LEFT_TOP, title, theme::bold(15.5), theme::TEXT);
            theme::paragraph(&painter, pos2(card.left() + 12.0, frame.bottom() + 28.0), size.x - 24.0, 2, what, theme::regular(12.0), theme::DIM);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        } else if let (Some(i), Some(at)) = (hovered, response.hover_pos()) {
            if let Some(a) = self.airfield(i) {
                let text = format!("{} · {}\n{} · {}", a.code, a.name, a.where_it_is(), a.size.name());
                let galley = painter.layout_no_wrap(text, FontId::proportional(12.0), Color32::WHITE);
                let r = Rect::from_min_size(at + vec2(14.0, 10.0), galley.size()).expand(5.0);
                painter.rect_filled(r, 4.0, Color32::from_black_alpha(220));
                painter.galley(r.min + vec2(5.0, 5.0), galley, Color32::WHITE);
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        self.controls(ui, rect);
    }

    /// The aerodromes on the map, by size, those that fit at this zoom; their runways when near.
    /// Returns the one under the pointer.
    fn airfields(&self, painter: &egui::Painter, rect: Rect, pointer: Option<Pos2>, taken: &mut Vec<Rect>) -> Option<usize> {
        let index = self.index()?;
        let zoom = self.view.zoom;
        let (a, b) = (self.view.map(rect, rect.min - vec2(30.0, 30.0)), self.view.map(rect, rect.max + vec2(30.0, 30.0)));
        let mut shown: Vec<(usize, Pos2)> = Vec::new();
        for (i, (p, airfield)) in self.points.iter().zip(&index.airfields).enumerate() {
            if p.0 < a.0 || p.0 > b.0 || p.1 < a.1 || p.1 > b.1 || zoom < shown_from(airfield) {
                continue;
            }
            shown.push((i, self.view.screen(rect, *p)));
        }
        // Small first, so that the big are drawn over them.
        shown.sort_by_key(|(i, _)| index.airfields[*i].size);
        let metres = self.view.metres_per_point() as f32;
        if zoom >= 10.5 {
            for (i, _) in &shown {
                for r in &index.airfields[*i].runways {
                    let [p, q] = [&r.ends[0], &r.ends[1]].map(|e| self.view.screen(rect, map::mercator(e.latitude, e.longitude)));
                    let colour = if r.paved { Color32::from_rgb(205, 207, 214) } else { Color32::from_rgb(140, 170, 120) };
                    painter.line_segment([p, q], Stroke::new((r.width as f32 / metres).max(2.5), colour));
                    if zoom >= 13.5 {
                        for (end, at, to) in [(&r.ends[0], p, q), (&r.ends[1], q, p)] {
                            let inward = (to - at).normalized() * 16.0;
                            let galley = painter.layout_no_wrap(end.ident.clone(), FontId::monospace(11.0), Color32::WHITE);
                            map::halo(painter, at + inward - galley.size() / 2.0, galley);
                        }
                    }
                }
            }
        }
        let mut under = None;
        let mut nearest = 10.0f32;
        for (i, at) in &shown {
            let airfield = &index.airfields[*i];
            let (radius, colour) = dot(airfield.size, zoom);
            if zoom < 10.5 || airfield.runways.is_empty() {
                painter.circle(*at, radius, colour, Stroke::new(1.0, Color32::from_black_alpha(180)));
            } else {
                painter.circle_stroke(*at, radius + 2.0, Stroke::new(1.5, colour));
            }
            if let Some(p) = pointer {
                let d = p.distance(*at);
                if d < nearest.max(radius + 3.0) {
                    (nearest, under) = (d, Some(*i));
                }
            }
        }
        // Codes, the big first, where there is room.
        for (i, at) in shown.iter().rev() {
            let airfield = &index.airfields[*i];
            if zoom < shown_from(airfield) + 2.5 {
                continue;
            }
            let (radius, colour) = dot(airfield.size, zoom);
            let text = if zoom >= 11.0 { format!("{} {}", airfield.code, airfield.name) } else { airfield.code.clone() };
            let galley = painter.layout_no_wrap(text, FontId::proportional(11.0), colour);
            let place = Rect::from_min_size(*at + vec2(radius + 4.0, -galley.size().y / 2.0), galley.size()).expand(1.0);
            if rect.contains_rect(place) && !taken.iter().any(|t| t.intersects(place)) {
                taken.push(place);
                taken.push(Rect::from_center_size(*at, vec2(radius * 2.0, radius * 2.0)));
                map::halo(painter, place.min + vec2(1.0, 1.0), galley);
            }
        }
        under
    }

    /// The featured places on the map: a pin each, with its name where there is room. Returns the
    /// one under the pointer.
    fn pins(&self, painter: &egui::Painter, rect: Rect, pointer: Option<Pos2>, taken: &mut Vec<Rect>) -> Option<&'static geodata::Preset> {
        let index = self.index()?;
        let mut under = None;
        for preset in &geodata::PRESETS {
            let Some(a) = index.find(preset.code) else { continue };
            let at = self.view.screen(rect, map::mercator(a.latitude, a.longitude));
            if !rect.expand(20.0).contains(at) {
                continue;
            }
            // A pin: a drop standing on the place.
            let head = at - vec2(0.0, 15.0);
            let over = pointer.is_some_and(|p| p.distance(head) < 13.0 || p.distance(at) < 8.0);
            let colour = if over { Color32::WHITE } else { theme::ACCENT };
            painter.circle_filled(at + vec2(0.0, 1.0), 3.0, Color32::from_black_alpha(120));
            painter.add(egui::Shape::convex_polygon(vec![at, head + vec2(-7.0, 4.5), head + vec2(7.0, 4.5)], colour, Stroke::NONE));
            painter.circle(head, 8.5, colour, Stroke::new(1.5, Color32::from_black_alpha(150)));
            painter.circle_filled(head, 3.2, Color32::from_rgb(14, 17, 24));
            if over {
                under = Some(preset);
            }
            let name = preset.title.split(':').next().unwrap_or(preset.code);
            let galley = painter.layout_no_wrap(name.to_string(), theme::medium(12.5), Color32::WHITE);
            let place = Rect::from_min_size(head + vec2(13.0, -galley.size().y / 2.0), galley.size()).expand(2.0);
            if rect.contains_rect(place) && !taken.iter().any(|t| t.intersects(place)) {
                taken.push(place);
                taken.push(Rect::from_center_size(head, vec2(20.0, 20.0)));
                map::halo(painter, place.min + vec2(2.0, 2.0), galley);
            }
        }
        under
    }

    /// The square of ground a region of the chosen place covers.
    fn region_square(&self, painter: &egui::Painter, rect: Rect) {
        let Some((lat, lon)) = self.selected_point() else { return };
        let (x, y) = map::mercator(lat, lon);
        let half = REGION_HALF / (geodata::geo::EARTH_RADIUS * std::f64::consts::TAU * lat.to_radians().cos());
        let square = Rect::from_min_max(self.view.screen(rect, (x - half, y - half)), self.view.screen(rect, (x + half, y + half)));
        if square.width() > 40.0 && square.width() < rect.width() * 4.0 {
            painter.rect_filled(square, 0.0, Color32::from_rgba_unmultiplied(120, 170, 255, 14));
            let corners = [square.left_top(), square.right_top(), square.right_bottom(), square.left_bottom(), square.left_top()];
            painter.add(egui::Shape::dashed_line(&corners, Stroke::new(1.2, Color32::from_rgba_unmultiplied(150, 190, 255, 160)), 8.0, 6.0));
        }
    }

    fn selected_point(&self) -> Option<(f64, f64)> {
        match &self.selected {
            Some(Selected::Airfield(i)) => self.airfield(*i).map(|a| (a.latitude, a.longitude)),
            Some(Selected::Point(lat, lon)) => Some((*lat, *lon)),
            _ => None,
        }
    }

    /// A ring round what is chosen.
    fn marker(&self, painter: &egui::Painter, rect: Rect) {
        let Some((lat, lon)) = self.selected_point() else { return };
        let at = self.view.screen(rect, map::mercator(lat, lon));
        let accent = Color32::from_rgb(110, 180, 255);
        painter.circle_stroke(at, 11.0, Stroke::new(2.5, Color32::from_black_alpha(160)));
        painter.circle_stroke(at, 11.0, Stroke::new(1.8, accent));
        if matches!(self.selected, Some(Selected::Point(..))) {
            for d in [vec2(1.0, 0.0), vec2(0.0, 1.0)] {
                painter.line_segment([at - d * 18.0, at - d * 5.0], Stroke::new(1.6, accent));
                painter.line_segment([at + d * 5.0, at + d * 18.0], Stroke::new(1.6, accent));
            }
        }
    }

    /// Zoom buttons, the scale, how to use it and the credits, on the map.
    fn controls(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let painter = ui.painter_at(rect);
        for (k, (text, steps)) in [("+", 1.0), ("−", -1.0)].into_iter().enumerate() {
            let button = Rect::from_min_size(pos2(rect.right() - 46.0, rect.top() + 14.0 + k as f32 * 40.0), vec2(32.0, 32.0));
            let response = ui.interact(button, ui.scope_id().with(("zoom", k)), Sense::click());
            painter.rect_filled(button, 6.0, if response.hovered() { Color32::from_rgb(60, 66, 76) } else { Color32::from_rgb(36, 40, 46) });
            painter.text(button.center(), Align2::CENTER_CENTER, text, FontId::proportional(20.0), Color32::WHITE);
            if response.clicked() {
                self.view.zoom_about(rect, rect.center(), steps);
            }
        }
        // A scale bar of a round number of kilometres, some 100 points long.
        let metres = self.view.metres_per_point() * 100.0;
        let round = [1.0, 2.0, 5.0].iter().flat_map(|m| (0..8).map(move |p| m * 10f64.powi(p))).filter(|v| *v <= metres).fold(1.0, f64::max);
        let length = (round / self.view.metres_per_point()) as f32;
        let base = pos2(rect.left() + 16.0, rect.bottom() - 18.0);
        painter.line_segment([base, base + vec2(length, 0.0)], Stroke::new(2.0, Color32::WHITE));
        for x in [0.0, length] {
            painter.line_segment([base + vec2(x, -5.0), base + vec2(x, 0.0)], Stroke::new(2.0, Color32::WHITE));
        }
        let scale = if round >= 1000.0 { format!("{:.0} km", round / 1000.0) } else { format!("{round:.0} m") };
        painter.text(base + vec2(length + 8.0, 0.0), Align2::LEFT_BOTTOM, scale, FontId::proportional(11.0), Color32::WHITE);
        painter.text(pos2(rect.center().x, rect.bottom() - 10.0), Align2::CENTER_BOTTOM, "Drag to move · scroll or pinch to zoom · click a pin, an aerodrome or any point", theme::regular(12.0), Color32::from_white_alpha(150));
    }
}

/// The farthest zoom an aerodrome is shown at.
fn shown_from(a: &Airfield) -> f64 {
    match (a.size, a.runways.is_empty()) {
        (Size::Large, _) => 0.0,
        (Size::Medium, _) => 4.5,
        (Size::Small, false) => 7.0,
        (Size::Small, true) => 9.5,
    }
}

/// The dot of an aerodrome: its radius at this zoom and its colour.
fn dot(size: Size, zoom: f64) -> (f32, Color32) {
    let grow = ((zoom - 4.0) * 0.25).clamp(0.0, 1.5) as f32;
    match size {
        Size::Large => (2.4 + grow, Color32::from_rgb(255, 146, 64)),
        Size::Medium => (2.2 + grow, Color32::from_rgb(250, 202, 96)),
        Size::Small => (1.9 + grow * 0.7, Color32::from_rgb(150, 200, 240)),
    }
}

/// What a list says of what the cache holds of a place, and whether it is all there.
fn need_text(need: Need) -> (String, bool) {
    if need.kept {
        ("downloaded".to_string(), true)
    } else {
        (format!("~{:.0} MB", (need.bytes as f64 / 1e6).max(1.0)), false)
    }
}

fn heading(ui: &mut egui::Ui, glyph: char, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
    let painter = ui.painter();
    theme::icon(painter, pos2(rect.left() + 9.0, rect.bottom() - 13.0), glyph, 15.0, theme::ACCENT);
    let job = egui::text::LayoutJob::single_section(text.to_uppercase(), egui::TextFormat { font_id: theme::bold(11.5), color: theme::DIM, extra_letter_spacing: 1.4, ..Default::default() });
    let galley = painter.layout_job(job);
    let at = pos2(rect.left() + 24.0, rect.bottom() - 13.0 - galley.size().y / 2.0);
    let end = at.x + galley.size().x + 10.0;
    painter.galley(at, galley, theme::DIM);
    painter.hline(end..=rect.right(), rect.bottom() - 13.0, Stroke::new(1.0, theme::LINE));
}

/// What a row of a list was clicked for.
struct Row {
    clicked: bool,
    starred: bool,
}

/// A place in a list: its star (if it can be a favourite), its name, what it is, and what the cache
/// holds of it on the right (in green when nothing is to be downloaded); marked when chosen, and
/// when it is the place flown now.
fn place_row(ui: &mut egui::Ui, selected: bool, here: bool, favourite: Option<bool>, title: &str, what: &str, need: Option<(String, bool)>) -> Row {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 44.0), Sense::click());
    let painter = ui.painter_at(rect);
    let rect = rect.shrink2(vec2(0.0, 2.0));
    if selected {
        painter.rect(rect, 9.0, Color32::from_rgba_premultiplied(16, 34, 60, 150), Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Inside);
    } else if response.hovered() {
        painter.rect_filled(rect, 9.0, theme::CARD_HOVER);
    }
    let mut starred = false;
    let mut left = rect.left() + 10.0;
    if let Some(on) = favourite {
        let star = Rect::from_min_size(pos2(left - 6.0, rect.top()), vec2(26.0, rect.height()));
        let r = ui.interact(star, response.id.with("star"), Sense::click());
        let colour = if on { theme::WARM } else if r.hovered() { Color32::from_gray(210) } else { theme::FAINT };
        draw_star(&painter, star.center(), 7.0, on, colour);
        starred = r.clicked();
        left += 22.0;
    }
    let right = rect.right() - 10.0;
    let mut text_right = right;
    if let Some((need, ready)) = need {
        let colour = if ready { theme::GOOD } else { theme::DIM };
        let galley = painter.layout_no_wrap(need, theme::medium(11.5), colour);
        text_right -= galley.size().x + 8.0;
        painter.galley(pos2(right - galley.size().x, rect.center().y - galley.size().y / 2.0), galley, colour);
    }
    // The place flown now: a dot before its name.
    let title_left = if here {
        painter.circle_filled(pos2(left + 3.5, rect.top() + 12.5), 3.5, theme::GOOD);
        left + 12.0
    } else {
        left
    };
    theme::text_within(&painter, pos2(title_left, rect.top() + 3.0), text_right - title_left, title, theme::medium(14.5), theme::TEXT);
    theme::text_within(&painter, pos2(left, rect.top() + 21.0), text_right - left, what, theme::regular(12.0), theme::DIM);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if here {
        response.clone().on_hover_text("Where you are flying now");
    }
    Row { clicked: response.clicked() && !starred, starred }
}

/// What a card of a place shows.
struct CardLook<'a> {
    picture: Option<egui::TextureHandle>,
    /// The icon in place of a picture that is not there.
    glyph: char,
    title: &'a str,
    what: &'a str,
    /// What the cache holds of it, and whether it is all there.
    need: Option<(String, bool)>,
    selected: bool,
    /// It is the place flown now.
    here: bool,
    /// Its star, if it can be a favourite.
    favourite: Option<bool>,
}

/// A place as a card: its picture, its name and what it is over the picture's foot, what the
/// cache holds of it and its star in the top corners.
fn place_card(ui: &mut egui::Ui, card: Rect, id: &str, look: &CardLook) -> Row {
    let response = ui.interact(card, ui.scope_id().with(("place-card", id)), Sense::click());
    let lit = ui.ctx().animate_bool_with_time(response.id, response.hovered(), 0.12);
    let painter = ui.painter().with_clip_rect(card.intersect(ui.clip_rect()));
    let radius = 11.0;
    match &look.picture {
        Some(texture) => theme::picture(&painter, card, radius, texture, Color32::from_gray((214.0 + 41.0 * lit) as u8)),
        None => no_picture(&painter, card, radius, look.glyph),
    }
    let foot = Rect::from_min_max(pos2(card.left(), card.bottom() - 64.0), card.max);
    theme::rounded_gradient(&painter.with_clip_rect(foot.intersect(ui.clip_rect())), card, radius, |p| Color32::from_black_alpha((225.0 * ((p.y - foot.top()) / foot.height()).clamp(0.0, 1.0).powf(0.75)) as u8));
    let title_left = if look.here {
        painter.circle_filled(pos2(card.left() + 14.0, card.bottom() - 31.0), 3.5, theme::GOOD);
        card.left() + 22.0
    } else {
        card.left() + 10.0
    };
    theme::text_within(&painter, pos2(title_left, card.bottom() - 41.0), card.right() - title_left - 8.0, look.title, theme::bold(15.0), Color32::WHITE);
    theme::text_within(&painter, pos2(card.left() + 10.0, card.bottom() - 22.0), card.width() - 18.0, look.what, theme::regular(11.5), Color32::from_gray(214));
    // What the cache holds of it, top right.
    if let Some((need, ready)) = &look.need {
        let colour = if *ready { theme::GOOD } else { Color32::from_gray(225) };
        let galley = painter.layout_no_wrap(need.clone(), theme::medium(11.0), colour);
        let chip = Rect::from_min_size(pos2(card.right() - galley.size().x - 20.0, card.top() + 7.0), vec2(galley.size().x + 13.0, 19.0));
        painter.rect_filled(chip, 9.5, Color32::from_black_alpha(210));
        painter.galley(pos2(chip.left() + 6.5, chip.center().y - galley.size().y / 2.0), galley, colour);
    }
    let mut starred = false;
    if let Some(on) = look.favourite {
        let star = Rect::from_min_size(card.min + vec2(5.0, 5.0), vec2(24.0, 24.0));
        let r = ui.interact(star, response.id.with("star"), Sense::click());
        painter.circle_filled(star.center(), 11.0, Color32::from_black_alpha(200));
        let colour = if on { theme::WARM } else if r.hovered() { Color32::WHITE } else { Color32::from_gray(190) };
        draw_star(&painter, star.center(), 6.5, on, colour);
        starred = r.clicked();
    }
    let edge = if look.selected { Stroke::new(2.0, theme::ACCENT) } else { Stroke::new(1.0, theme::mix(theme::LINE, Color32::from_white_alpha(150), lit)) };
    painter.rect_stroke(card, radius, edge, egui::StrokeKind::Inside);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let response = if look.here { response.on_hover_text("Where you are flying now") } else { response.on_hover_text(look.what) };
    Row { clicked: response.clicked() && !starred, starred }
}

/// What is chosen: its picture (or an icon), its name and what it is.
fn chosen_head(ui: &mut egui::Ui, picture: Option<egui::TextureHandle>, glyph: char, title: &str, what: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 68.0), Sense::hover());
    let painter = ui.painter();
    theme::card(painter, rect, 12.0, false, false);
    let frame = Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(90.0, 56.0));
    match picture {
        Some(texture) => theme::picture(painter, frame, 8.0, &texture, Color32::WHITE),
        None => no_picture(painter, frame, 8.0, glyph),
    }
    let left = frame.right() + 12.0;
    theme::text(painter, pos2(left, rect.top() + 9.0), Align2::LEFT_TOP, "GOING TO", theme::medium(10.5), theme::ACCENT);
    theme::text_within(painter, pos2(left, rect.top() + 22.0), rect.right() - left - 10.0, title, theme::bold(16.0), theme::TEXT);
    theme::text_within(painter, pos2(left, rect.top() + 43.0), rect.right() - left - 10.0, what, theme::regular(12.5), theme::DIM);
}

/// The button that flies: as wide as the column, with what pressing it costs written on it.
fn big_button(ui: &mut egui::Ui, id: &str, glyph: char, label: &str, note: &str, fill: Color32) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
    let _ = id;
    let lit = ui.ctx().animate_bool_with_time(response.id, response.hovered(), 0.12);
    let painter = ui.painter();
    painter.rect_filled(rect, 12.0, theme::mix(fill, Color32::WHITE, lit * 0.16));
    painter.rect_filled(Rect::from_min_max(pos2(rect.left() + 12.0, rect.top()), pos2(rect.right() - 12.0, rect.top() + 1.0)), 0.0, Color32::from_white_alpha(70));
    theme::icon(painter, pos2(rect.left() + 28.0, rect.center().y), glyph, 24.0, Color32::WHITE);
    let title = theme::text(painter, pos2(rect.left() + 52.0, rect.center().y), Align2::LEFT_CENTER, label, theme::bold(19.0), Color32::WHITE);
    theme::text_within(painter, pos2(title.right() + 14.0, rect.center().y - 8.0), rect.right() - title.right() - 26.0, note, theme::regular(12.5), Color32::from_white_alpha(215));
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.clicked()
}

/// A five-pointed star round `centre`, filled or only its outline.
fn draw_star(painter: &egui::Painter, centre: Pos2, radius: f32, filled: bool, colour: Color32) {
    let points: Vec<Pos2> = (0..10)
        .map(|k| {
            let angle = std::f32::consts::PI * (k as f32 / 5.0 - 0.5);
            let r = if k % 2 == 0 { radius } else { radius * 0.42 };
            centre + vec2(angle.cos(), angle.sin()) * r
        })
        .collect();
    if filled {
        // A fan from the middle: the star is not convex, but every point of it sees the middle.
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(centre, colour);
        for p in &points {
            mesh.colored_vertex(*p, colour);
        }
        for k in 0..10u32 {
            mesh.add_triangle(0, 1 + k, 1 + (k + 1) % 10);
        }
        painter.add(egui::Shape::mesh(mesh));
    }
    painter.add(egui::Shape::closed_line(points, Stroke::new(1.2, colour)));
}

/// What the cache holds of each place shown, by key (`geodata::cache::need`), worked out as the
/// places are shown. Web: on a worker, as the page's thread cannot wait for the browser's cache
/// (`geodata`'s `disk`): a place's comes a few frames after it is first asked for.
#[derive(Default)]
struct Needs {
    known: HashMap<String, Need>,
    #[cfg(target_arch = "wasm32")]
    asked: web::Asked,
}

impl Needs {
    fn get(&mut self, key: &str) -> Option<Need> {
        #[cfg(target_arch = "wasm32")]
        self.known.extend(self.asked.answers());
        self.known.get(key).copied()
    }

    fn work_out(&mut self, key: &str, place: Place, latitude: f64, longitude: f64) {
        #[cfg(not(target_arch = "wasm32"))]
        self.known.insert(key.to_string(), geodata::cache::need(&place, latitude, longitude));
        #[cfg(target_arch = "wasm32")]
        self.asked.ask(key, place, latitude, longitude);
    }

    /// Forgets what was worked out: the cache may hold more now.
    fn clear(&mut self) {
        self.known.clear();
        #[cfg(target_arch = "wasm32")]
        {
            self.asked = web::Asked::default();
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use geodata::cache::Need;
    use geodata::Place;
    use std::collections::HashSet;
    use std::sync::mpsc::{channel, Receiver, Sender};

    /// The places asked for on workers, and their answers.
    pub(super) struct Asked {
        keys: HashSet<String>,
        sender: Sender<(String, Need)>,
        receiver: Receiver<(String, Need)>,
    }

    impl Default for Asked {
        fn default() -> Asked {
            let (sender, receiver) = channel();
            Asked { keys: HashSet::new(), sender, receiver }
        }
    }

    impl Asked {
        /// Works a place's out on a worker, once.
        pub(super) fn ask(&mut self, key: &str, place: Place, latitude: f64, longitude: f64) {
            if !self.keys.insert(key.to_string()) {
                return;
            }
            let (key, sender) = (key.to_string(), self.sender.clone());
            engine::threads::spawn(move || {
                let _ = sender.send((key, geodata::cache::need(&place, latitude, longitude)));
            });
        }

        pub(super) fn answers(&self) -> Vec<(String, Need)> {
            self.receiver.try_iter().collect()
        }
    }
}
