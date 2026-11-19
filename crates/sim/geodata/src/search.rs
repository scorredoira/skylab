//! Finding an aerodrome by its name, its town, its country or a code, among the some 48 000 of
//! OurAirports that an aeroplane can fly from: what the game's place picker searches and draws on
//! its map. It reads the same two tables a region is built from (`ourairports`), from the cache.
//!
//! An aerodrome is found by a code typed whole or its start (ICAO, IATA, GPS or OurAirports' own),
//! or when every word typed starts one of its words (its name, town, country in Spanish or English,
//! the table's keywords), any case and without accents. The best come first: a code typed whole,
//! then the town, then the name, a word that is whole before one that only starts; among equals
//! the bigger aerodromes, those with airlines, the longer runway, then by name.

use std::collections::HashMap;

use crate::fetch::Fetcher;
use crate::ourairports::{self, records, Tables};
use crate::Error;

/// How big an aerodrome is, as OurAirports sorts them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Size {
    Small,
    Medium,
    Large,
}

impl Size {
    fn of(kind: &str) -> Option<Size> {
        match kind {
            "small_airport" => Some(Size::Small),
            "medium_airport" => Some(Size::Medium),
            "large_airport" => Some(Size::Large),
            _ => None,
        }
    }

    /// What the player reads.
    pub fn name(self) -> &'static str {
        match self {
            Size::Small => "small",
            Size::Medium => "medium",
            Size::Large => "large",
        }
    }
}

/// One end of a runway: where it is and the true heading taking off from it.
#[derive(Clone, Debug, PartialEq)]
pub struct RunwayEnd {
    /// As painted on it: `09`, `27L`...
    pub ident: String,
    pub latitude: f64,
    pub longitude: f64,
    /// True heading, degrees, taking off from this end towards the other.
    pub heading: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Runway {
    pub ends: [RunwayEnd; 2],
    /// Metres.
    pub length: f64,
    pub width: f64,
    pub paved: bool,
}

/// An aerodrome as the picker shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Airfield {
    pub id: u64,
    /// OurAirports' own code, the ICAO one when there is one; what `Place::parse` takes.
    pub code: String,
    pub iata: Option<String>,
    pub name: String,
    pub town: String,
    /// ISO 3166 code of the country (`ES`); `country_name` says it in English.
    pub country: String,
    pub size: Size,
    /// Airlines fly from it.
    pub scheduled: bool,
    pub latitude: f64,
    pub longitude: f64,
    /// Metres, when the table says.
    pub elevation: Option<f64>,
    /// The open runways of 250 m or more (those a region places), paved first, then the longest.
    pub runways: Vec<Runway>,
}

impl Airfield {
    /// `Barcelona, Spain`, or just the country.
    pub fn where_it_is(&self) -> String {
        let country = country_name(&self.country);
        if self.town.is_empty() {
            country.to_string()
        } else {
            format!("{}, {country}", self.town)
        }
    }
}

/// Which field of an aerodrome a word comes from: decides how much a match weighs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Town,
    Name,
    Country,
    Keyword,
}

impl Field {
    /// What a word typed is worth when it starts a word of this field, and more when it is the whole word.
    fn weight(self) -> (i32, i32) {
        match self {
            Field::Town => (120, 60),
            Field::Name => (100, 40),
            Field::Country => (40, 20),
            Field::Keyword => (30, 10),
        }
    }
}

/// Every aerodrome for aeroplanes of the tables, with its words ready to be searched.
#[derive(Clone, Debug, Default)]
pub struct Index {
    pub airfields: Vec<Airfield>,
    /// For each aerodrome, its words without accents in lower case, and where each comes from.
    words: Vec<Vec<(String, Field)>>,
    /// For each aerodrome, its codes as `compact` writes them (`US-0001` is `us0001`).
    codes: Vec<Vec<String>>,
}

impl Index {
    /// Both tables, from the cache or downloaded the first time (17 MB).
    pub fn load(fetcher: &Fetcher) -> Result<Index, Error> {
        let get = |key: &str, url: &str| -> Result<String, Error> {
            let bytes = fetcher.cached(key, url)?.ok_or_else(|| Error::Http(url.to_string()))?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        };
        let airports = get("ourairports/airports.csv", ourairports::AIRPORTS_URL)?;
        let runways = get("ourairports/runways.csv", ourairports::RUNWAYS_URL)?;
        Ok(Index::parse(&airports, &runways))
    }

    /// The aerodromes of OurAirports' tables that a region can be built round (those `Tables::parse`
    /// keeps), each with its runways.
    pub fn parse(airports: &str, runways: &str) -> Index {
        let mut rows = records(airports).into_iter();
        let names = rows.next().unwrap_or_default();
        let column = |name: &str| names.iter().position(|n| n == name).unwrap_or(usize::MAX);
        let [id, ident, kind, name, lat, lon, elev, country, town, scheduled, icao, iata, gps, keywords] =
            ["id", "ident", "type", "name", "latitude_deg", "longitude_deg", "elevation_ft", "iso_country", "municipality", "scheduled_service", "icao_code", "iata_code", "gps_code", "keywords"].map(column);
        let text = |row: &[String], c: usize| row.get(c).map_or(String::new(), |s| s.trim().to_string());
        let number = |row: &[String], c: usize| row.get(c).and_then(|s| s.trim().parse::<f64>().ok()).filter(|v| v.is_finite());

        let mut index = Index::default();
        let mut by_id = HashMap::new();
        for row in rows {
            let Some(size) = Size::of(&text(&row, kind)) else { continue };
            let (Ok(id), Some(latitude), Some(longitude)) = (text(&row, id).parse::<u64>(), number(&row, lat), number(&row, lon)) else { continue };
            let airfield = Airfield {
                id,
                code: text(&row, ident).to_ascii_uppercase(),
                iata: Some(text(&row, iata).to_ascii_uppercase()).filter(|c| !c.is_empty()),
                name: text(&row, name),
                town: text(&row, town),
                country: text(&row, country).to_ascii_uppercase(),
                size,
                scheduled: text(&row, scheduled) == "yes",
                latitude,
                longitude,
                elevation: number(&row, elev).map(|f| f * sim_core::FEET),
                runways: Vec::new(),
            };
            let mut words: Vec<(String, Field)> = Vec::new();
            let mut add = |text: &str, field: Field| {
                for w in normalize(text).split(' ').filter(|w| !w.is_empty()) {
                    if !words.iter().any(|(x, f)| x == w && *f == field) {
                        words.push((w.to_string(), field));
                    }
                }
            };
            add(&airfield.town, Field::Town);
            add(&airfield.name, Field::Name);
            add(country_name(&airfield.country), Field::Country);
            add(country_spanish(&airfield.country), Field::Country);
            add(&text(&row, keywords), Field::Keyword);
            let mut codes: Vec<String> = Vec::new();
            for code in [&airfield.code, &text(&row, icao), &text(&row, gps)].into_iter().chain(airfield.iata.as_ref()) {
                let code = compact(code);
                if !code.is_empty() && !codes.contains(&code) {
                    codes.push(code);
                }
            }
            by_id.insert(id, index.airfields.len());
            index.airfields.push(airfield);
            index.words.push(words);
            index.codes.push(codes);
        }

        // The runways as a region places them: open, not on water, 250 m or more.
        let tables = Tables::parse("", runways);
        for row in &tables.runways {
            let Some(&i) = by_id.get(&row.airport) else { continue };
            let a = &index.airfields[i];
            if let Some(runway) = runway(row, a.latitude, a.longitude) {
                index.airfields[i].runways.push(runway);
            }
        }
        for a in &mut index.airfields {
            a.runways.sort_by(|x, y| y.paved.cmp(&x.paved).then(y.length.total_cmp(&x.length)));
        }
        index
    }

    /// The aerodrome with this code, any case: its own, or its ICAO, GPS or IATA code.
    pub fn find(&self, code: &str) -> Option<&Airfield> {
        self.position(code).map(|i| &self.airfields[i])
    }

    /// Where the aerodrome with this code is in `airfields`.
    pub fn position(&self, code: &str) -> Option<usize> {
        let code = code.trim().to_ascii_uppercase();
        self.airfields.iter().position(|a| a.code == code).or_else(|| {
            let code = compact(&code);
            self.codes.iter().position(|c| c.contains(&code))
        })
    }

    /// The best `limit` aerodromes for what was typed, the best first (see the module's notes), as
    /// their places in `airfields`.
    pub fn search(&self, query: &str, limit: usize) -> Vec<usize> {
        let typed: Vec<String> = normalize(query).split(' ').filter(|w| !w.is_empty()).map(str::to_string).collect();
        if typed.is_empty() {
            return Vec::new();
        }
        let code = compact(query);
        let mut found: Vec<(i32, usize)> = Vec::new();
        for (i, (words, codes)) in self.words.iter().zip(&self.codes).enumerate() {
            // A code typed whole, or its start (two letters at least).
            let by_code = codes
                .iter()
                .filter_map(|c| match c.strip_prefix(code.as_str()) {
                    Some("") => Some(1000),
                    Some(_) if code.len() >= 2 => Some(150),
                    _ => None,
                })
                .max();
            // Every word typed starting one of its words: the best of them, and more for each other
            // field it is in too (Barcelona's airport, in its town and in its name).
            let by_words = typed.iter().try_fold(0, |score, t| {
                let matched: Vec<&(String, Field)> = words.iter().filter(|(w, _)| w.starts_with(t.as_str())).collect();
                let best = matched.iter().map(|(w, f)| f.weight().0 + if w == t { f.weight().1 } else { 0 }).max()?;
                let mut fields: Vec<Field> = matched.iter().map(|m| m.1).collect();
                fields.dedup();
                Some(score + best + 15 * (fields.len() as i32 - 1))
            });
            let Some(score) = by_code.into_iter().chain(by_words).max() else { continue };
            let a = &self.airfields[i];
            let size = match a.size {
                Size::Large => 90,
                Size::Medium => 45,
                Size::Small => 0,
            };
            found.push((score + size + if a.scheduled { 30 } else { 0 } - if a.runways.is_empty() { 40 } else { 0 }, i));
        }
        let longest = |i: usize| self.airfields[i].runways.first().map_or(0.0, |r| r.length);
        found.sort_by(|a, b| b.0.cmp(&a.0).then(longest(b.1).total_cmp(&longest(a.1))).then_with(|| self.airfields[a.1].name.cmp(&self.airfields[b.1].name)));
        found.into_iter().take(limit).map(|(_, i)| i).collect()
    }
}

/// A runway of the table as the picker draws it: from its two ends when the table has them, else
/// round the aerodrome's point, along the heading its number gives, as long as the table says.
fn runway(row: &ourairports::RunwayRow, latitude: f64, longitude: f64) -> Option<Runway> {
    let paved = row.paved();
    let ends = match (row.low.latitude, row.low.longitude, row.high.latitude, row.high.longitude) {
        (Some(a), Some(b), Some(c), Some(d)) if distance(a, b, c, d) > 100.0 => [(a, b), (c, d)],
        _ => {
            let number = row.low.ident.trim_start_matches('0').chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<f64>().ok();
            let heading = row.low.heading.or(number.map(|n| n * 10.0))?.to_radians();
            let half = row.length? / 2.0;
            let dlat = heading.cos() * half / METRES_PER_DEGREE;
            let dlon = heading.sin() * half / (METRES_PER_DEGREE * latitude.to_radians().cos().max(0.01));
            [(latitude - dlat, longitude - dlon), (latitude + dlat, longitude + dlon)]
        }
    };
    let length = row.length.unwrap_or(0.0).max(distance(ends[0].0, ends[0].1, ends[1].0, ends[1].1));
    if length < 250.0 {
        return None;
    }
    let end = |ident: &str, (lat, lon): (f64, f64), (to_lat, to_lon): (f64, f64)| RunwayEnd { ident: ident.to_string(), latitude: lat, longitude: lon, heading: bearing(lat, lon, to_lat, to_lon) };
    let width = row.width.filter(|w| *w >= 8.0).unwrap_or(if paved { 30.0 } else { 20.0 }).min(80.0);
    Some(Runway { ends: [end(&row.low.ident, ends[0], ends[1]), end(&row.high.ident, ends[1], ends[0])], length, width, paved })
}

const METRES_PER_DEGREE: f64 = crate::geo::EARTH_RADIUS * std::f64::consts::PI / 180.0;

/// Great-circle distance, metres, between two points in degrees.
pub fn distance(lat0: f64, lon0: f64, lat1: f64, lon1: f64) -> f64 {
    let (p0, p1) = (lat0.to_radians(), lat1.to_radians());
    let (dp, dl) = (p1 - p0, (lon1 - lon0).to_radians());
    let a = (dp / 2.0).sin().powi(2) + p0.cos() * p1.cos() * (dl / 2.0).sin().powi(2);
    2.0 * crate::geo::EARTH_RADIUS * a.sqrt().min(1.0).asin()
}

/// The true heading, degrees from 0 to 360, from one point towards another along the great circle.
pub fn bearing(lat0: f64, lon0: f64, lat1: f64, lon1: f64) -> f64 {
    let (p0, p1) = (lat0.to_radians(), lat1.to_radians());
    let dl = (lon1 - lon0).to_radians();
    let y = dl.sin() * p1.cos();
    let x = p0.cos() * p1.sin() - p0.sin() * p1.cos() * dl.cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

/// A code as compared: lower case, only its letters and digits.
fn compact(code: &str) -> String {
    normalize(code).replace(' ', "")
}

/// Lower case, without accents, every run of what is not a letter or a digit one space:
/// `Sant Lluís-Menorca` is `sant lluis menorca`.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = true;
    for c in text.chars().flat_map(char::to_lowercase) {
        let c = match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' | 'ą' | 'æ' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ę' | 'ě' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'ī' | 'ı' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' | 'ō' | 'ő' | 'œ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => 'u',
            'ñ' | 'ń' | 'ň' => 'n',
            'ç' | 'ć' | 'č' => 'c',
            'ś' | 'š' | 'ş' | 'ß' => 's',
            'ź' | 'ż' | 'ž' => 'z',
            'ł' => 'l',
            'ř' => 'r',
            'ý' | 'ÿ' => 'y',
            'ğ' => 'g',
            'đ' | 'ð' => 'd',
            'þ' => 't',
            c => c,
        };
        if c.is_alphanumeric() {
            out.push(c);
            space = false;
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// The countries of ISO 3166, in Spanish and in English, by their code.
const COUNTRIES: &str = "\
AD Andorra|Andorra;AE Emiratos Árabes Unidos|United Arab Emirates;AF Afganistán|Afghanistan;AG Antigua y Barbuda|Antigua and Barbuda;\
AI Anguila|Anguilla;AL Albania|Albania;AM Armenia|Armenia;AO Angola|Angola;AQ Antártida|Antarctica;AR Argentina|Argentina;\
AS Samoa Americana|American Samoa;AT Austria|Austria;AU Australia|Australia;AW Aruba|Aruba;AX Åland|Aland;AZ Azerbaiyán|Azerbaijan;\
BA Bosnia y Herzegovina|Bosnia and Herzegovina;BB Barbados|Barbados;BD Bangladés|Bangladesh;BE Bélgica|Belgium;BF Burkina Faso|Burkina Faso;\
BG Bulgaria|Bulgaria;BH Baréin|Bahrain;BI Burundi|Burundi;BJ Benín|Benin;BL San Bartolomé|Saint Barthelemy;BM Bermudas|Bermuda;\
BN Brunéi|Brunei;BO Bolivia|Bolivia;BQ Caribe Neerlandés|Caribbean Netherlands;BR Brasil|Brazil;BS Bahamas|Bahamas;BT Bután|Bhutan;\
BW Botsuana|Botswana;BY Bielorrusia|Belarus;BZ Belice|Belize;CA Canadá|Canada;CC Islas Cocos|Cocos Islands;CD República Democrática del Congo|Democratic Republic of the Congo;\
CF República Centroafricana|Central African Republic;CG Congo|Congo;CH Suiza|Switzerland;CI Costa de Marfil|Ivory Coast;CK Islas Cook|Cook Islands;\
CL Chile|Chile;CM Camerún|Cameroon;CN China|China;CO Colombia|Colombia;CR Costa Rica|Costa Rica;CU Cuba|Cuba;CV Cabo Verde|Cape Verde;\
CW Curazao|Curacao;CX Isla de Navidad|Christmas Island;CY Chipre|Cyprus;CZ Chequia|Czechia;DE Alemania|Germany;DJ Yibuti|Djibouti;\
DK Dinamarca|Denmark;DM Dominica|Dominica;DO República Dominicana|Dominican Republic;DZ Argelia|Algeria;EC Ecuador|Ecuador;EE Estonia|Estonia;\
EG Egipto|Egypt;EH Sáhara Occidental|Western Sahara;ER Eritrea|Eritrea;ES España|Spain;ET Etiopía|Ethiopia;FI Finlandia|Finland;FJ Fiyi|Fiji;\
FK Islas Malvinas|Falkland Islands;FM Micronesia|Micronesia;FO Islas Feroe|Faroe Islands;FR Francia|France;GA Gabón|Gabon;GB Reino Unido|United Kingdom;\
GD Granada|Grenada;GE Georgia|Georgia;GF Guayana Francesa|French Guiana;GG Guernsey|Guernsey;GH Ghana|Ghana;GI Gibraltar|Gibraltar;\
GL Groenlandia|Greenland;GM Gambia|Gambia;GN Guinea|Guinea;GP Guadalupe|Guadeloupe;GQ Guinea Ecuatorial|Equatorial Guinea;GR Grecia|Greece;\
GS Georgia del Sur|South Georgia;GT Guatemala|Guatemala;GU Guam|Guam;GW Guinea-Bisáu|Guinea-Bissau;GY Guyana|Guyana;HK Hong Kong|Hong Kong;\
HN Honduras|Honduras;HR Croacia|Croatia;HT Haití|Haiti;HU Hungría|Hungary;ID Indonesia|Indonesia;IE Irlanda|Ireland;IL Israel|Israel;\
IM Isla de Man|Isle of Man;IN India|India;IO Territorio Británico del Océano Índico|British Indian Ocean Territory;IQ Irak|Iraq;IR Irán|Iran;\
IS Islandia|Iceland;IT Italia|Italy;JE Jersey|Jersey;JM Jamaica|Jamaica;JO Jordania|Jordan;JP Japón|Japan;KE Kenia|Kenya;\
KG Kirguistán|Kyrgyzstan;KH Camboya|Cambodia;KI Kiribati|Kiribati;KM Comoras|Comoros;KN San Cristóbal y Nieves|Saint Kitts and Nevis;\
KP Corea del Norte|North Korea;KR Corea del Sur|South Korea;KW Kuwait|Kuwait;KY Islas Caimán|Cayman Islands;KZ Kazajistán|Kazakhstan;\
LA Laos|Laos;LB Líbano|Lebanon;LC Santa Lucía|Saint Lucia;LI Liechtenstein|Liechtenstein;LK Sri Lanka|Sri Lanka;LR Liberia|Liberia;\
LS Lesoto|Lesotho;LT Lituania|Lithuania;LU Luxemburgo|Luxembourg;LV Letonia|Latvia;LY Libia|Libya;MA Marruecos|Morocco;MC Mónaco|Monaco;\
MD Moldavia|Moldova;ME Montenegro|Montenegro;MF San Martín|Saint Martin;MG Madagascar|Madagascar;MH Islas Marshall|Marshall Islands;\
MK Macedonia del Norte|North Macedonia;ML Malí|Mali;MM Birmania|Myanmar;MN Mongolia|Mongolia;MO Macao|Macau;MP Islas Marianas del Norte|Northern Mariana Islands;\
MQ Martinica|Martinique;MR Mauritania|Mauritania;MS Montserrat|Montserrat;MT Malta|Malta;MU Mauricio|Mauritius;MV Maldivas|Maldives;\
MW Malaui|Malawi;MX México|Mexico;MY Malasia|Malaysia;MZ Mozambique|Mozambique;NA Namibia|Namibia;NC Nueva Caledonia|New Caledonia;\
NE Níger|Niger;NF Isla Norfolk|Norfolk Island;NG Nigeria|Nigeria;NI Nicaragua|Nicaragua;NL Países Bajos|Netherlands;NO Noruega|Norway;\
NP Nepal|Nepal;NR Nauru|Nauru;NU Niue|Niue;NZ Nueva Zelanda|New Zealand;OM Omán|Oman;PA Panamá|Panama;PE Perú|Peru;\
PF Polinesia Francesa|French Polynesia;PG Papúa Nueva Guinea|Papua New Guinea;PH Filipinas|Philippines;PK Pakistán|Pakistan;PL Polonia|Poland;\
PM San Pedro y Miquelón|Saint Pierre and Miquelon;PN Islas Pitcairn|Pitcairn;PR Puerto Rico|Puerto Rico;PS Palestina|Palestine;PT Portugal|Portugal;\
PW Palaos|Palau;PY Paraguay|Paraguay;QA Catar|Qatar;RE Reunión|Reunion;RO Rumanía|Romania;RS Serbia|Serbia;RU Rusia|Russia;RW Ruanda|Rwanda;\
SA Arabia Saudí|Saudi Arabia;SB Islas Salomón|Solomon Islands;SC Seychelles|Seychelles;SD Sudán|Sudan;SE Suecia|Sweden;SG Singapur|Singapore;\
SH Santa Elena|Saint Helena;SI Eslovenia|Slovenia;SJ Svalbard|Svalbard;SK Eslovaquia|Slovakia;SL Sierra Leona|Sierra Leone;SM San Marino|San Marino;\
SN Senegal|Senegal;SO Somalia|Somalia;SR Surinam|Suriname;SS Sudán del Sur|South Sudan;ST Santo Tomé y Príncipe|Sao Tome and Principe;\
SV El Salvador|El Salvador;SX Sint Maarten|Sint Maarten;SY Siria|Syria;SZ Esuatini|Eswatini;TC Islas Turcas y Caicos|Turks and Caicos Islands;\
TD Chad|Chad;TF Territorios Australes Franceses|French Southern Territories;TG Togo|Togo;TH Tailandia|Thailand;TJ Tayikistán|Tajikistan;\
TL Timor Oriental|East Timor;TM Turkmenistán|Turkmenistan;TN Túnez|Tunisia;TO Tonga|Tonga;TR Turquía|Turkey;TT Trinidad y Tobago|Trinidad and Tobago;\
TV Tuvalu|Tuvalu;TW Taiwán|Taiwan;TZ Tanzania|Tanzania;UA Ucrania|Ukraine;UG Uganda|Uganda;UM Islas menores de Estados Unidos|United States Minor Outlying Islands;\
US Estados Unidos|United States;UY Uruguay|Uruguay;UZ Uzbekistán|Uzbekistan;VA Ciudad del Vaticano|Vatican City;VC San Vicente y las Granadinas|Saint Vincent and the Grenadines;\
VE Venezuela|Venezuela;VG Islas Vírgenes Británicas|British Virgin Islands;VI Islas Vírgenes de los Estados Unidos|United States Virgin Islands;\
VN Vietnam|Vietnam;VU Vanuatu|Vanuatu;WF Wallis y Futuna|Wallis and Futuna;WS Samoa|Samoa;XK Kosovo|Kosovo;YE Yemen|Yemen;YT Mayotte|Mayotte;\
ZA Sudáfrica|South Africa;ZM Zambia|Zambia;ZW Zimbabue|Zimbabwe";

/// The Spanish and English names of a country by its code.
fn country(code: &str) -> Option<(&'static str, &'static str)> {
    COUNTRIES.split(';').find(|e| e.get(..2) == Some(code) && e.as_bytes().get(2) == Some(&b' ')).and_then(|e| e[3..].split_once('|'))
}

/// A country's name in English (`ES`: `Spain`); the code if it is not one.
pub fn country_name(code: &str) -> &str {
    country(code).map_or(code, |c| c.1)
}

/// A country's name in Spanish (`ES`: `España`), which the search also matches; empty if it is not one.
fn country_spanish(code: &str) -> &str {
    country(code).map_or("", |c| c.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A few aerodromes as the table writes them (made up after real ones), and a heliport.
    const AIRPORTS: &str = "\
\"id\",\"ident\",\"type\",\"name\",\"latitude_deg\",\"longitude_deg\",\"elevation_ft\",\"continent\",\"iso_country\",\"iso_region\",\"municipality\",\"scheduled_service\",\"icao_code\",\"iata_code\",\"gps_code\",\"local_code\",\"home_link\",\"wikipedia_link\",\"keywords\"
4372,\"LEBL\",\"large_airport\",\"Josep Tarradellas Barcelona-El Prat Airport\",41.2971,2.07846,12,\"EU\",\"ES\",\"ES-CT\",\"Barcelona\",\"yes\",\"LEBL\",\"BCN\",\"LEBL\",,,,\"El Prat\"
4373,\"LELL\",\"medium_airport\",\"Sabadell Airport\",41.5209,2.10508,485,\"EU\",\"ES\",\"ES-CT\",\"Sabadell\",\"no\",\"LELL\",\"QSA\",\"LELL\",,,,\"Barcelona\"
4380,\"LEMH\",\"medium_airport\",\"Menorca Airport\",39.862598,4.21865,302,\"EU\",\"ES\",\"ES-IB\",\"Mahón\",\"yes\",\"LEMH\",\"MAH\",\"LEMH\",,,,
9001,\"XBCN\",\"small_airport\",\"Barcelona Gliderfield\",41.6,2.2,600,\"EU\",\"ES\",\"ES-CT\",\"Barcelona\",\"no\",,,,,,,
9002,\"US-0001\",\"small_airport\",\"Barcelona Field\",44.0,-71.0,900,\"NA\",\"US\",\"US-NH\",\"Barcelona\",\"no\",,,,,,,
9003,\"ES-0042\",\"heliport\",\"Hospital de Barcelona Heliport\",41.39,2.15,50,\"EU\",\"ES\",\"ES-CT\",\"Barcelona\",\"no\",,,,,,,
9004,\"SVBC\",\"large_airport\",\"General Anzoategui International Airport\",10.1,-64.7,26,\"SA\",\"VE\",\"VE-B\",\"Barcelona\",\"yes\",\"SVBC\",\"BLA\",\"SVBC\",,,,
";
    const RUNWAYS: &str = "\
\"id\",\"airport_ref\",\"airport_ident\",\"length_ft\",\"width_ft\",\"surface\",\"lighted\",\"closed\",\"le_ident\",\"le_latitude_deg\",\"le_longitude_deg\",\"le_elevation_ft\",\"le_heading_degT\",\"le_displaced_threshold_ft\",\"he_ident\",\"he_latitude_deg\",\"he_longitude_deg\",\"he_elevation_ft\",\"he_heading_degT\",\"he_displaced_threshold_ft\"
1,4372,\"LEBL\",11654,148,\"ASP\",1,0,\"06L\",41.2826,2.07364,15,43.5,,\"24R\",41.3068,2.10314,8,223.5,
2,4372,\"LEBL\",8727,148,\"ASP\",1,0,\"02\",41.2878,2.08458,13,20,,\"20\",41.3092,2.09484,26,200,
3,4380,\"LEMH\",7710,148,\"ASP\",1,0,\"01L\",,,,,,\"19R\",,,,,
4,4380,\"LEMH\",7710,148,\"ASP\",1,1,\"01R\",,,,,,\"19L\",,,,,
5,9001,\"XBCN\",500,60,\"GRASS\",0,0,\"09\",,,,,,\"27\",,,,,
6,9004,\"SVBC\",9849,148,\"ASP\",1,0,\"15\",,,,,,\"33\",,,,,
";

    fn index() -> Index {
        Index::parse(AIRPORTS, RUNWAYS)
    }

    fn codes(index: &Index, found: &[usize]) -> Vec<String> {
        found.iter().map(|i| index.airfields[*i].code.clone()).collect()
    }

    #[test]
    fn words_are_compared_without_case_or_accents() {
        assert_eq!(normalize("Sant Lluís-Menorca"), "sant lluis menorca");
        assert_eq!(normalize("  ZÜRICH  (Kloten) "), "zurich kloten");
        assert_eq!(normalize("Ålesund, Vigra"), "alesund vigra");
        assert_eq!(compact("US-0001"), "us0001");
        assert_eq!(country_name("ES"), "Spain");
        assert_eq!(country_spanish("ES"), "España");
        assert_eq!(country_name("NO"), "Norway");
        assert_eq!(country_name("ZZ"), "ZZ");
    }

    #[test]
    fn only_aerodromes_for_aeroplanes_are_kept_with_their_open_runways() {
        let index = index();
        assert_eq!(index.airfields.len(), 6, "the heliport is left out");
        let lebl = index.find("bcn").unwrap();
        assert_eq!((lebl.code.as_str(), lebl.size, lebl.scheduled), ("LEBL", Size::Large, true));
        assert_eq!(lebl.where_it_is(), "Barcelona, Spain");
        assert_eq!(index.find("us-0001").map(|a| a.code.as_str()), Some("US-0001"));
        assert_eq!(lebl.runways.len(), 2);
        // The longest first, its ends where the table puts them, the headings between them.
        let r = &lebl.runways[0];
        assert_eq!((r.ends[0].ident.as_str(), r.ends[1].ident.as_str()), ("06L", "24R"));
        assert!(r.length >= 11654.0 * sim_core::FEET, "the table's length, or the distance between the ends if longer");
        assert!((r.ends[0].heading - 43.5).abs() < 1.5 && (r.ends[1].heading - 223.5).abs() < 1.5, "{} {}", r.ends[0].heading, r.ends[1].heading);
        // Without ends in the table: round the aerodrome's point, along its number. The closed one is left out.
        let lemh = index.find("LEMH").unwrap();
        assert_eq!(lemh.runways.len(), 1);
        let r = &lemh.runways[0];
        assert!((r.ends[0].heading - 10.0).abs() < 0.1 && (r.ends[1].heading - 190.0).abs() < 0.1);
        let middle = ((r.ends[0].latitude + r.ends[1].latitude) / 2.0, (r.ends[0].longitude + r.ends[1].longitude) / 2.0);
        assert!(distance(middle.0, middle.1, lemh.latitude, lemh.longitude) < 1.0);
        assert!((distance(r.ends[0].latitude, r.ends[0].longitude, r.ends[1].latitude, r.ends[1].longitude) - r.length).abs() < 2.0);
        // A grass strip of 152 m is too short for a region.
        assert!(index.find("XBCN").unwrap().runways.is_empty());
    }

    #[test]
    fn a_code_typed_whole_comes_first_then_the_bigger_aerodromes_of_the_town() {
        let index = index();
        assert_eq!(codes(&index, &index.search("lemh", 5)), ["LEMH"]);
        assert_eq!(codes(&index, &index.search("MAH", 5)), ["LEMH"]);
        assert_eq!(codes(&index, &index.search("us-0001", 5)), ["US-0001"]);
        assert_eq!(codes(&index, &index.search("Mahon", 5)), ["LEMH"], "the town, without its accent");
        // The town's big airports, the one with the town in its name too first; then the small
        // fields of that town (both without runways, by name), Sabadell only by its keyword.
        assert_eq!(codes(&index, &index.search("barcelona", 10)), ["LEBL", "SVBC", "US-0001", "XBCN", "LELL"]);
        // Every word must match: the country narrows it down, in Spanish or in English.
        assert_eq!(codes(&index, &index.search("barcelona estados", 10)), ["US-0001"]);
        assert_eq!(codes(&index, &index.search("barcelona united", 10)), ["US-0001"]);
        assert_eq!(codes(&index, &index.search("barcelona venez", 10)), ["SVBC"]);
        // Starts of words, not their middles.
        assert_eq!(codes(&index, &index.search("prat", 10)), ["LEBL"]);
        assert!(index.search("arcelona", 10).is_empty());
        assert!(index.search("  ", 10).is_empty());
        assert_eq!(index.search("barcelona", 2).len(), 2);
        // The start of a code finds it, the bigger first; a code typed whole beats it.
        assert_eq!(codes(&index, &index.search("le", 10)), ["LEBL", "LEMH", "LELL"]);
        assert_eq!(codes(&index, &index.search("lell", 10)), ["LELL"]);
    }
}
