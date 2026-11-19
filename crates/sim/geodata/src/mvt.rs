//! Mapbox Vector Tiles (specification 2.1): the layers asked for, their features' geometry and
//! properties, read straight from the protobuf wire format with no protobuf crate.
//!
//! A tile is a protobuf message (`Tile`) of layers (field 3). A layer has its name (1), features
//! (2), the keys (3) and values (4) its features' tags point into, and its extent (5, 4096 when
//! absent). A feature has its tags (2, packed pairs of key and value indices), its geometry type (3)
//! and its geometry (4, packed commands). The fields may come in any order, so a layer's features
//! are read once its keys and values are known. Bytes that break the format end what is read of the
//! message they are in; nothing panics.

use std::collections::HashMap;
use std::io::Read;

/// A property's value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Text(String),
    Number(f64),
    Bool(bool),
}

impl Value {
    pub fn text(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }

    /// The number, or the text read as one.
    pub fn number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            Value::Text(s) => s.parse().ok(),
            Value::Bool(_) => None,
        }
    }
}

/// What a feature's geometry is (`GeomType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Unknown,
    Point,
    Line,
    Polygon,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Feature {
    pub kind: Kind,
    /// The lines, or the rings of a polygon (outer ones one way round, holes the other), in the
    /// tile's units: 0 to `Layer::extent` east and south from its north-west corner.
    pub parts: Vec<Vec<[f32; 2]>>,
    pub properties: HashMap<String, Value>,
}

impl Feature {
    pub fn text(&self, key: &str) -> Option<&str> {
        self.properties.get(key).and_then(Value::text)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub extent: f32,
    pub features: Vec<Feature>,
}

/// The layers named in `wanted` of a tile, gzipped or not; nothing for bytes that are not one.
pub fn decode(data: &[u8], wanted: &[&str]) -> HashMap<String, Layer> {
    let mut layers = HashMap::new();
    let inflated;
    let tile = if data.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::with_capacity(data.len() * 4);
        if flate2::read::GzDecoder::new(data).read_to_end(&mut out).is_err() {
            return layers;
        }
        inflated = out;
        &inflated[..]
    } else {
        data
    };
    let mut tile = Wire::new(tile);
    while let Some((field, value)) = tile.field() {
        if let (3, Field::Bytes(bytes)) = (field, value) {
            if let Some((name, layer)) = read_layer(bytes, wanted) {
                layers.insert(name, layer);
            }
        }
    }
    layers
}

/// The protobuf wire types this format uses.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Field<'a> {
    Varint(u64),
    Fixed64(u64),
    Bytes(&'a [u8]),
    Fixed32(u32),
}

/// A cursor over the bytes of one protobuf message.
struct Wire<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Wire<'a> {
    fn new(bytes: &'a [u8]) -> Wire<'a> {
        Wire { bytes, at: 0 }
    }

    fn done(&self) -> bool {
        self.at >= self.bytes.len()
    }

    /// A base-128 varint: seven bits a byte, least significant first, the high bit set on all but
    /// the last. At most ten bytes; `None` when it runs past the end or is longer.
    fn varint(&mut self) -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..70).step_by(7) {
            let byte = *self.bytes.get(self.at)?;
            self.at += 1;
            value |= ((byte & 0x7f) as u64) << shift;
            if byte < 0x80 {
                return Some(value);
            }
        }
        None
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|&end| end <= self.bytes.len())?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Some(slice)
    }

    /// The next field's number and value; `None` at the end or at bytes that break the format
    /// (after which the cursor stays at the end).
    fn field(&mut self) -> Option<(u64, Field<'a>)> {
        if self.done() {
            return None;
        }
        let read = (|| {
            let tag = self.varint()?;
            let value = match tag & 7 {
                0 => Field::Varint(self.varint()?),
                1 => Field::Fixed64(u64::from_le_bytes(self.take(8)?.try_into().ok()?)),
                2 => {
                    let n = usize::try_from(self.varint()?).ok()?;
                    Field::Bytes(self.take(n)?)
                }
                5 => Field::Fixed32(u32::from_le_bytes(self.take(4)?.try_into().ok()?)),
                // Groups (3, 4) are not used by vector tiles; anything else is not protobuf.
                _ => return None,
            };
            Some((tag >> 3, value))
        })();
        if read.is_none() {
            self.at = self.bytes.len();
        }
        read
    }
}

/// The varints of a packed repeated field, or of a single one sent unpacked.
fn varints<'a>(value: Field<'a>) -> impl Iterator<Item = u64> + 'a {
    let (mut packed, single) = match value {
        Field::Bytes(b) => (Wire::new(b), None),
        Field::Varint(v) => (Wire::new(&[]), Some(v)),
        _ => (Wire::new(&[]), None),
    };
    single.into_iter().chain(std::iter::from_fn(move || packed.varint()))
}

/// Zigzag decoding: 0, 1, 2, 3, 4 … stand for 0, −1, 1, −2, 2 …
fn zigzag(n: u64) -> i64 {
    (n >> 1) as i64 ^ -((n & 1) as i64)
}

/// A layer, if its name is one of `wanted`.
fn read_layer(bytes: &[u8], wanted: &[&str]) -> Option<(String, Layer)> {
    let mut name = None;
    let mut keys: Vec<&str> = Vec::new();
    let mut values: Vec<Option<Value>> = Vec::new();
    let mut features: Vec<&[u8]> = Vec::new();
    let mut extent = 4096u64;
    let mut wire = Wire::new(bytes);
    while let Some((field, value)) = wire.field() {
        match (field, value) {
            (1, Field::Bytes(b)) => {
                let text = std::str::from_utf8(b).ok()?;
                if !wanted.contains(&text) {
                    return None;
                }
                name = Some(text);
            }
            (2, Field::Bytes(b)) => features.push(b),
            (3, Field::Bytes(b)) => keys.push(std::str::from_utf8(b).unwrap_or("")),
            (4, Field::Bytes(b)) => values.push(read_value(b)),
            (5, Field::Varint(v)) => extent = v,
            _ => {}
        }
    }
    let name = name?;
    let features = features.into_iter().map(|b| read_feature(b, &keys, &values)).collect();
    Some((name.to_string(), Layer { extent: extent as f32, features }))
}

/// A `Value` message: one of its fields set.
fn read_value(bytes: &[u8]) -> Option<Value> {
    let mut wire = Wire::new(bytes);
    let mut value = None;
    while let Some(field) = wire.field() {
        value = match field {
            (1, Field::Bytes(b)) => Some(Value::Text(String::from_utf8_lossy(b).into_owned())),
            (2, Field::Fixed32(v)) => Some(Value::Number(f32::from_bits(v) as f64)),
            (3, Field::Fixed64(v)) => Some(Value::Number(f64::from_bits(v))),
            (4, Field::Varint(v)) => Some(Value::Number(v as i64 as f64)),
            (5, Field::Varint(v)) => Some(Value::Number(v as f64)),
            (6, Field::Varint(v)) => Some(Value::Number(zigzag(v) as f64)),
            (7, Field::Varint(v)) => Some(Value::Bool(v != 0)),
            _ => value,
        };
    }
    value
}

fn read_feature(bytes: &[u8], keys: &[&str], values: &[Option<Value>]) -> Feature {
    let mut kind = Kind::Unknown;
    let mut tags = Vec::new();
    let mut geometry = Vec::new();
    let mut wire = Wire::new(bytes);
    while let Some((field, value)) = wire.field() {
        match field {
            2 => tags.extend(varints(value)),
            3 => {
                if let Field::Varint(v) = value {
                    kind = match v {
                        1 => Kind::Point,
                        2 => Kind::Line,
                        3 => Kind::Polygon,
                        _ => Kind::Unknown,
                    }
                }
            }
            4 => geometry.extend(varints(value)),
            _ => {}
        }
    }
    let mut properties = HashMap::with_capacity(tags.len() / 2);
    let index = |i: u64| usize::try_from(i).unwrap_or(usize::MAX);
    for &[k, v] in tags.as_chunks::<2>().0 {
        let key = keys.get(index(k));
        let value = values.get(index(v)).and_then(Option::as_ref);
        if let (Some(key), Some(value)) = (key, value) {
            properties.insert(key.to_string(), value.clone());
        }
    }
    Feature { kind, parts: read_geometry(&geometry), properties }
}

/// The geometry's commands (`id | count << 3`: MoveTo 1, LineTo 2, ClosePath 7), each followed by
/// its parameters, pairs of zigzag deltas from the cursor. Each point a MoveTo goes to starts a
/// part (a multipoint is a part a point); a ClosePath ends a ring without repeating its first
/// point. A command that runs past the end ends the geometry.
fn read_geometry(commands: &[u64]) -> Vec<Vec<[f32; 2]>> {
    let mut parts: Vec<Vec<[f32; 2]>> = Vec::new();
    let mut part: Vec<[f32; 2]> = Vec::new();
    let (mut x, mut y) = (0i64, 0i64);
    let mut i = 0;
    while i < commands.len() {
        let (id, count) = (commands[i] & 7, commands[i] >> 3);
        i += 1;
        match id {
            1 | 2 => {
                let end = usize::try_from(count).ok().and_then(|n| n.checked_mul(2)).and_then(|n| n.checked_add(i));
                let Some(params) = end.and_then(|end| commands.get(i..end)) else { break };
                i += params.len();
                for &[dx, dy] in params.as_chunks::<2>().0 {
                    if id == 1 && !part.is_empty() {
                        parts.push(std::mem::take(&mut part));
                    }
                    x = x.wrapping_add(zigzag(dx));
                    y = y.wrapping_add(zigzag(dy));
                    part.push([x as f32, y as f32]);
                }
            }
            7 => {
                if !part.is_empty() {
                    parts.push(std::mem::take(&mut part));
                }
            }
            _ => break,
        }
    }
    if !part.is_empty() {
        parts.push(part);
    }
    parts
}

/// Every point (column, row) of a `width` by `height` raster whose centre is inside the rings by
/// the even-odd rule (holes stay out), given to `fill`. The rings are in the raster's units.
pub fn fill_rings(width: usize, height: usize, rings: &[Vec<[f64; 2]>], mut fill: impl FnMut(usize, usize)) {
    let rings = || rings.iter().filter(|r| r.len() >= 3);
    // Only the rows the rings reach: a ring is mostly far smaller than the raster.
    let (south, north) = rings().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p[1]), hi.max(p[1])));
    let top = (south - 0.5).ceil().max(0.0);
    let bottom = ((north - 0.5).ceil() - 1.0).min(height as f64 - 1.0);
    if width == 0 || top > bottom {
        return;
    }
    let top_row = top as usize;
    // Where each row's centre line crosses the rings' edges, an edge counting at its lower end
    // and not its upper one so that a vertex on the line is crossed once.
    let mut crossings: Vec<Vec<f64>> = vec![Vec::new(); bottom as usize - top_row + 1];
    for ring in rings() {
        let mut a = ring[ring.len() - 1];
        for &b in ring {
            let (low, high) = if a[1] <= b[1] { (a, b) } else { (b, a) };
            // Rows whose centre y = r + 0.5 has low.y <= y < high.y.
            let first = (low[1] - 0.5).ceil().max(top);
            let last = ((high[1] - 0.5).ceil() - 1.0).min(bottom);
            if first <= last {
                let slope = (high[0] - low[0]) / (high[1] - low[1]);
                for r in first as usize..=last as usize {
                    crossings[r - top_row].push(low[0] + (r as f64 + 0.5 - low[1]) * slope);
                }
            }
            a = b;
        }
    }
    for (r, xs) in (top_row..).zip(&mut crossings) {
        xs.sort_unstable_by(f64::total_cmp);
        for &[west, east] in xs.as_chunks::<2>().0 {
            // Columns whose centre x = c + 0.5 has west <= x < east.
            let first = (west - 0.5).ceil().max(0.0);
            let end = (east - 0.5).ceil().min(width as f64);
            if first < end {
                for c in first as usize..end as usize {
                    fill(c, r);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint(mut v: u64, out: &mut Vec<u8>) {
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }

    fn zz(n: i64) -> u64 {
        ((n << 1) ^ (n >> 63)) as u64
    }

    fn bytes_field(field: u64, body: &[u8], out: &mut Vec<u8>) {
        varint(field << 3 | 2, out);
        varint(body.len() as u64, out);
        out.extend_from_slice(body);
    }

    fn packed(field: u64, values: &[u64], out: &mut Vec<u8>) {
        let mut body = Vec::new();
        for &v in values {
            varint(v, &mut body);
        }
        bytes_field(field, &body, out);
    }

    fn command(id: u64, count: u64) -> u64 {
        id | count << 3
    }

    /// The example of the specification (4.3.5.3): a polygon of a ring and the same ring again
    /// with a hole, and the line and points of its other examples.
    fn sample_tile() -> Vec<u8> {
        let mut layer = Vec::new();
        bytes_field(1, b"things", &mut layer);
        // Features before the keys and values: the order of fields is free.
        let mut polygon = Vec::new();
        packed(2, &[0, 0, 1, 1], &mut polygon);
        varint(3 << 3, &mut polygon);
        varint(3, &mut polygon);
        let ring = |out: &mut Vec<u64>, start: (i64, i64), deltas: &[(i64, i64)]| {
            out.extend([command(1, 1), zz(start.0), zz(start.1), command(2, deltas.len() as u64)]);
            for d in deltas {
                out.extend([zz(d.0), zz(d.1)]);
            }
            out.push(command(7, 1));
        };
        let mut g = Vec::new();
        ring(&mut g, (3, 6), &[(5, 6), (12, 22)]);
        packed(4, &g, &mut polygon);
        bytes_field(2, &polygon, &mut layer);

        let mut line = Vec::new();
        varint(3 << 3, &mut line);
        varint(2, &mut line);
        packed(4, &[command(1, 1), zz(2), zz(2), command(2, 2), zz(0), zz(8), zz(8), zz(0), command(1, 1), zz(-8), zz(-2), command(2, 1), zz(3), zz(3)], &mut line);
        bytes_field(2, &line, &mut layer);

        let mut points = Vec::new();
        packed(2, &[2, 2], &mut points);
        varint(3 << 3, &mut points);
        varint(1, &mut points);
        packed(4, &[command(1, 2), zz(5), zz(7), zz(-2), zz(-3)], &mut points);
        bytes_field(2, &points, &mut layer);

        for key in ["class", "height", "flag"] {
            bytes_field(3, key.as_bytes(), &mut layer);
        }
        let mut text = Vec::new();
        bytes_field(1, b"wood", &mut text);
        bytes_field(4, &text, &mut layer);
        let mut sint = Vec::new();
        varint(6 << 3, &mut sint);
        varint(zz(-12), &mut sint);
        bytes_field(4, &sint, &mut layer);
        let mut boolean = Vec::new();
        varint(7 << 3, &mut boolean);
        varint(1, &mut boolean);
        bytes_field(4, &boolean, &mut layer);
        varint(5 << 3, &mut layer);
        varint(512, &mut layer);
        varint(15 << 3, &mut layer);
        varint(2, &mut layer);

        let mut tile = Vec::new();
        bytes_field(3, &layer, &mut tile);
        let mut other = Vec::new();
        bytes_field(1, b"other", &mut other);
        bytes_field(3, &other, &mut tile);
        tile
    }

    #[test]
    fn varints_are_seven_bits_a_byte() {
        for v in [0, 1, 127, 128, 150, 300, 16_383, 16_384, u32::MAX as u64, u64::MAX] {
            let mut bytes = Vec::new();
            varint(v, &mut bytes);
            let mut w = Wire::new(&bytes);
            assert_eq!(w.varint(), Some(v));
            assert!(w.done());
        }
        // The specification's own: 150 is 96 01.
        assert_eq!(Wire::new(&[0x96, 0x01]).varint(), Some(150));
        assert_eq!(Wire::new(&[0x96]).varint(), None);
        assert_eq!(Wire::new(&[0xff; 11]).varint(), None);
    }

    #[test]
    fn zigzag_alternates_signs() {
        for (n, v) in [(0, 0), (1, -1), (2, 1), (3, -2), (4, 2), (4_294_967_294, 2_147_483_647), (4_294_967_295, -2_147_483_648)] {
            assert_eq!(zigzag(n), v);
        }
        assert_eq!(zigzag(u64::MAX), i64::MIN);
        for v in [-5i64, 0, 7, i64::MAX, i64::MIN] {
            assert_eq!(zigzag(zz(v)), v);
        }
    }

    #[test]
    fn commands_draw_points_lines_and_rings() {
        let layers = decode(&sample_tile(), &["things"]);
        assert_eq!(layers.len(), 1, "only the layer asked for");
        let layer = &layers["things"];
        assert_eq!((layer.extent, layer.features.len()), (512.0, 3));
        let [polygon, line, points] = &layer.features[..] else { unreachable!() };
        assert_eq!(polygon.kind, Kind::Polygon);
        assert_eq!(polygon.parts, vec![vec![[3.0, 6.0], [8.0, 12.0], [20.0, 34.0]]]);
        assert_eq!(polygon.text("class"), Some("wood"));
        assert_eq!(polygon.properties.get("height"), Some(&Value::Number(-12.0)));
        assert_eq!(line.kind, Kind::Line);
        assert_eq!(line.parts, vec![vec![[2.0, 2.0], [2.0, 10.0], [10.0, 10.0]], vec![[2.0, 8.0], [5.0, 11.0]]]);
        assert!(line.properties.is_empty());
        assert_eq!(points.kind, Kind::Point);
        assert_eq!(points.parts, vec![vec![[5.0, 7.0]], vec![[3.0, 4.0]]]);
        assert_eq!(points.properties.get("flag"), Some(&Value::Bool(true)));
    }

    #[test]
    fn gzipped_tiles_read_the_same() {
        use std::io::Write;
        let tile = sample_tile();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tile).unwrap();
        assert_eq!(decode(&gz.finish().unwrap(), &["things"]), decode(&tile, &["things"]));
    }

    #[test]
    fn values_of_every_type() {
        let value = |field: u64, wire: u64, payload: &[u8]| {
            let mut b = Vec::new();
            varint(field << 3 | wire, &mut b);
            b.extend_from_slice(payload);
            read_value(&b)
        };
        assert_eq!(value(2, 5, &1.5f32.to_le_bytes()), Some(Value::Number(1.5)));
        assert_eq!(value(3, 1, &(-2.25f64).to_le_bytes()), Some(Value::Number(-2.25)));
        let mut minus_three = Vec::new();
        varint(-3i64 as u64, &mut minus_three);
        assert_eq!(value(4, 0, &minus_three), Some(Value::Number(-3.0)));
        assert_eq!(value(5, 0, &[42]), Some(Value::Number(42.0)));
        assert_eq!(value(7, 0, &[0]), Some(Value::Bool(false)));
        assert_eq!(value(9, 0, &[1]), None);
        assert_eq!(Value::Text("12.5".into()).number(), Some(12.5));
    }

    #[test]
    fn malformed_input_never_panics() {
        assert!(decode(b"", &["things"]).is_empty());
        assert!(decode(b"\x1f\x8bnot gzip", &["things"]).is_empty());
        let tile = sample_tile();
        // Every truncation and every single byte changed: whatever comes back, no panic.
        for n in 0..tile.len() {
            decode(&tile[..n], &["things"]);
        }
        for i in 0..tile.len() {
            for byte in [0x00, 0x07, 0x7f, 0x80, 0xff] {
                let mut bad = tile.clone();
                bad[i] = byte;
                decode(&bad, &["things", "other"]);
            }
        }
        // Commands with counts far beyond what follows, and unknown ones.
        let geometry = read_geometry(&[command(2, 1 << 58), 1, 2]);
        assert!(geometry.is_empty());
        assert!(read_geometry(&[command(3, 1), 0, 0]).is_empty());
        assert_eq!(read_geometry(&[command(1, 1), zz(1), zz(1), command(2, 5), 2]), vec![vec![[1.0, 1.0]]]);
        // Tags that point past the keys and values are left out.
        let feature = {
            let mut f = Vec::new();
            packed(2, &[0, 9, 7, 0, 0], &mut f);
            f
        };
        assert!(read_feature(&feature, &["a"], &[Some(Value::Bool(true))]).properties.is_empty());
    }

    #[test]
    fn a_ring_with_a_hole_fills_what_is_between() {
        let outer = vec![[1.0, 1.0], [9.0, 1.0], [9.0, 9.0], [1.0, 9.0]];
        let hole = vec![[3.0, 3.0], [3.0, 7.0], [7.0, 7.0], [7.0, 3.0]];
        let mut filled = [false; 100];
        fill_rings(10, 10, &[outer, hole], |x, y| filled[y * 10 + x] = true);
        assert_eq!(filled.iter().filter(|f| **f).count(), 64 - 16);
        assert!(filled[11] && !filled[55] && !filled[0]);
    }

    #[test]
    fn fills_stay_in_the_raster() {
        let big = vec![[-50.0, -50.0], [50.0, -50.0], [50.0, 50.0], [-50.0, 50.0]];
        let mut count = 0;
        fill_rings(4, 3, std::slice::from_ref(&big), |c, r| {
            assert!(c < 4 && r < 3);
            count += 1;
        });
        assert_eq!(count, 12);
        fill_rings(4, 3, &[big, vec![[f64::NAN, 0.0], [1.0, f64::INFINITY], [2.0, 2.0]]], |c, r| assert!(c < 4 && r < 3));
        fill_rings(0, 0, &[vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]], |_, _| unreachable!());
    }
}
