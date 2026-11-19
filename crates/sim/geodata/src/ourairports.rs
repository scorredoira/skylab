//! The aerodromes and runways of OurAirports (public domain): two tables of comma-separated values,
//! `airports.csv` and `runways.csv`, from `davidmegginson.github.io/ourairports-data`. Only what a
//! region needs is kept: where each aerodrome is, its codes and kind, and each runway's ends,
//! length, width, surface and lights.

use sim_core::lights::Class;
use sim_core::{Airport, Detail, FEET};

use crate::geo::Projection;

pub const AIRPORTS_URL: &str = "https://davidmegginson.github.io/ourairports-data/airports.csv";
pub const RUNWAYS_URL: &str = "https://davidmegginson.github.io/ourairports-data/runways.csv";

/// An aerodrome as the table has it.
#[derive(Clone, Debug, PartialEq)]
pub struct Aerodrome {
    pub id: u64,
    /// Its codes: OurAirports' own (usually the ICAO one), the ICAO, GPS and IATA codes.
    pub codes: Vec<String>,
    /// `small_airport`, `medium_airport` or `large_airport` (the others are not kept).
    pub kind: String,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation: Option<f64>,
}

/// One end of a runway.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct End {
    pub ident: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub elevation: Option<f64>,
    /// True heading taking off from this end, degrees.
    pub heading: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RunwayRow {
    pub airport: u64,
    pub length: Option<f64>,
    pub width: Option<f64>,
    pub surface: String,
    pub lighted: bool,
    pub low: End,
    pub high: End,
}

impl RunwayRow {
    /// Asphalt, concrete or the like; grass, earth, gravel and the rest are not.
    pub fn paved(&self) -> bool {
        let s = self.surface.to_ascii_uppercase();
        const PAVED: [&str; 8] = ["ASP", "CON", "PEM", "BIT", "TAR", "PAVED", "MAC", "TARMAC"];
        const SOFT: [&str; 10] = ["TURF", "GRASS", "GRS", "DIRT", "GRAVEL", "GRV", "SAND", "CLAY", "SOIL", "EARTH"];
        if PAVED.iter().any(|p| s.contains(p)) {
            return true;
        }
        if SOFT.iter().any(|p| s.contains(p)) {
            return false;
        }
        // Unknown: the long ones are paved.
        self.length.is_some_and(|l| l >= 1200.0)
    }
}

/// Both tables, as far as a region needs them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tables {
    pub aerodromes: Vec<Aerodrome>,
    pub runways: Vec<RunwayRow>,
}

/// The fields of a CSV text, record by record (quoted fields may hold commas, quotes doubled and
/// line breaks).
pub fn records(text: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let (mut record, mut field) = (Vec::new(), String::new());
    let (mut quoted, mut chars) = (false, text.chars().peekable());
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') => quoted = true,
            (false, ',') => record.push(std::mem::take(&mut field)),
            (false, '\r') => {}
            (false, '\n') => {
                record.push(std::mem::take(&mut field));
                out.push(std::mem::take(&mut record));
            }
            (false, c) => field.push(c),
        }
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        out.push(record);
    }
    out
}

/// Records with their header's names.
struct Table {
    names: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    fn new(text: &str) -> Table {
        let mut rows = records(text);
        let names = if rows.is_empty() { Vec::new() } else { rows.remove(0) };
        Table { names, rows }
    }

    /// The column of a name, or a column past every row's end.
    fn column(&self, name: &str) -> usize {
        self.names.iter().position(|n| n == name).unwrap_or(usize::MAX)
    }
}

fn text(row: &[String], column: usize) -> &str {
    row.get(column).map_or("", |s| s.trim())
}

fn number(row: &[String], column: usize) -> Option<f64> {
    text(row, column).parse().ok().filter(|v: &f64| v.is_finite())
}

impl Tables {
    /// Reads both tables. Keeps only aerodromes for aeroplanes (not heliports, seaplane bases,
    /// balloon ports or closed ones) and the open runways.
    pub fn parse(airports: &str, runways: &str) -> Tables {
        let t = Table::new(airports);
        let (id, ident, kind, name, lat, lon, elev) = (t.column("id"), t.column("ident"), t.column("type"), t.column("name"), t.column("latitude_deg"), t.column("longitude_deg"), t.column("elevation_ft"));
        let code_columns = [ident, t.column("icao_code"), t.column("gps_code"), t.column("iata_code")];
        let mut aerodromes = Vec::new();
        for row in &t.rows {
            let kind = text(row, kind);
            if !matches!(kind, "small_airport" | "medium_airport" | "large_airport") {
                continue;
            }
            let (Some(id), Some(latitude), Some(longitude)) = (text(row, id).parse().ok(), number(row, lat), number(row, lon)) else { continue };
            let mut codes: Vec<String> = Vec::new();
            for code in code_columns.iter().map(|&c| text(row, c).to_ascii_uppercase()) {
                if !code.is_empty() && !codes.contains(&code) {
                    codes.push(code);
                }
            }
            aerodromes.push(Aerodrome { id, codes, kind: kind.to_string(), name: text(row, name).to_string(), latitude, longitude, elevation: number(row, elev).map(|f| f * FEET) });
        }

        let t = Table::new(runways);
        let column = |n: &str| t.column(n);
        let end = |row: &[String], side: &str| End {
            ident: text(row, column(&format!("{side}_ident"))).to_string(),
            latitude: number(row, column(&format!("{side}_latitude_deg"))),
            longitude: number(row, column(&format!("{side}_longitude_deg"))),
            elevation: number(row, column(&format!("{side}_elevation_ft"))).map(|f| f * FEET),
            heading: number(row, column(&format!("{side}_heading_degT"))),
        };
        let mut runway_rows = Vec::new();
        for row in &t.rows {
            if text(row, column("closed")) == "1" {
                continue;
            }
            let Ok(airport) = text(row, column("airport_ref")).parse() else { continue };
            let surface = text(row, column("surface")).to_string();
            if surface.to_ascii_uppercase().contains("WATER") {
                continue;
            }
            runway_rows.push(RunwayRow {
                airport,
                length: number(row, column("length_ft")).map(|f| f * FEET),
                width: number(row, column("width_ft")).map(|f| f * FEET),
                surface,
                lighted: text(row, column("lighted")) == "1",
                low: end(row, "le"),
                high: end(row, "he"),
            });
        }
        Tables { aerodromes, runways: runway_rows }
    }

    /// The aerodrome with this code (ICAO, GPS, IATA or OurAirports' own), any case.
    pub fn find(&self, code: &str) -> Option<&Aerodrome> {
        let code = code.trim().to_ascii_uppercase();
        self.aerodromes.iter().find(|a| a.codes.first() == Some(&code)).or_else(|| self.aerodromes.iter().find(|a| a.codes.contains(&code)))
    }

    pub fn runways_of(&self, id: u64) -> impl Iterator<Item = &RunwayRow> {
        self.runways.iter().filter(move |r| r.airport == id)
    }
}

/// A runway placed in the world: its `Airport` (with `elevation` from the table, if it has one)
/// and which aerodrome it is of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub airport: Airport,
    /// Whether the table gave its height (otherwise the ground's is taken).
    pub surveyed: bool,
}

/// The runways that can be placed of an aerodrome: from their ends where the table has them, or
/// from its reference point, the heading the number gives and the length. Shorter than 250 m, or
/// without a length, they are left out. The longest paved one (or the longest) has the apron, on
/// the side away from the others, or of the reference point when it is alone.
pub fn place(aerodrome: &Aerodrome, runways: &[&RunwayRow], projection: &Projection) -> Vec<Placed> {
    let reference = projection.to_world(aerodrome.latitude, aerodrome.longitude);
    let mut list: Vec<(Placed, bool)> = Vec::new();
    for r in runways {
        let ends = match (r.low.latitude, r.low.longitude, r.high.latitude, r.high.longitude) {
            (Some(a), Some(b), Some(c), Some(d)) => Some((projection.to_world(a, b), projection.to_world(c, d))),
            _ => None,
        };
        let (centre, heading, length) = match ends {
            Some((low, high)) if (high.0 - low.0).hypot(high.1 - low.1) > 100.0 => {
                let (dn, de) = (high.0 - low.0, high.1 - low.1);
                (((low.0 + high.0) / 2.0, (low.1 + high.1) / 2.0), de.atan2(dn), r.length.unwrap_or(0.0).max(dn.hypot(de)))
            }
            _ => {
                let number = r.low.ident.trim_start_matches('0').chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<f64>().ok();
                let Some(heading) = r.low.heading.or(number.map(|n| n * 10.0)) else { continue };
                (reference, heading.to_radians(), r.length.unwrap_or(0.0))
            }
        };
        if length < 250.0 {
            continue;
        }
        let paved = r.paved();
        let width = r.width.filter(|w| *w >= 8.0).unwrap_or(if paved { 30.0 } else { 20.0 }).min(80.0);
        let elevation = match (r.low.elevation, r.high.elevation) {
            (Some(a), Some(b)) => Some((a + b) / 2.0),
            (a, b) => a.or(b).or(aerodrome.elevation),
        };
        let lighting = class(&aerodrome.kind, r.lighted && paved, length);
        let airport = Airport {
            id: aerodrome.id,
            n: centre.0,
            e: centre.1,
            heading: heading.rem_euclid(std::f64::consts::PI),
            elevation: elevation.unwrap_or(0.0),
            length,
            width,
            detail: Detail { paved, lighting: Some(lighting), apron: 0.0 },
        };
        list.push((Placed { airport, surveyed: elevation.is_some() }, paved));
    }
    // The main runway first: paved before not, then the longest.
    list.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.airport.length.total_cmp(&a.0.airport.length)));
    let mut placed: Vec<Placed> = list.into_iter().map(|(p, _)| p).collect();
    if let Some((main, others)) = placed.split_first_mut() {
        let a = &main.airport;
        // Across the main runway (positive to the right of its heading): the other runways' middles,
        // or the reference point.
        let across: f64 = if others.is_empty() {
            a.local(reference.0, reference.1).1
        } else {
            others.iter().map(|o| a.local(o.airport.n, o.airport.e).1).sum()
        };
        let side = if others.is_empty() { across } else { -across };
        main.airport.detail.apron = if side < 0.0 { -1.0 } else { 1.0 };
    }
    placed
}

/// The lights a runway gets (`lighted` a paved runway with lights): none but the medium ones and a
/// VASI unless lit and long enough; the big aerodromes' long runways an ILS of category II, the rest
/// of category I; the middle ones a non-precision approach.
pub fn class(kind: &str, lighted: bool, length: f64) -> Class {
    match kind {
        _ if !lighted => Class::Visual,
        "large_airport" if length >= 2500.0 => Class::PrecisionTwo,
        "large_airport" => Class::PrecisionOne,
        "medium_airport" if length >= 1800.0 => Class::PrecisionOne,
        "medium_airport" => Class::NonPrecision,
        _ if length >= 1200.0 => Class::NonPrecision,
        _ => Class::Visual,
    }
}
