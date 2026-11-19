//! Just enough JSON for the answers of a STAC catalogue (`sentinel`): values parsed whole into a
//! tree, read by key. Numbers are `f64`; strings keep their escapes decoded (`\uXXXX` included).

/// A JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    List(Vec<Json>),
    /// Keys in the order they came.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// Parses a whole document; `None` if it is not JSON.
    pub fn parse(text: &str) -> Option<Json> {
        let mut p = Parser { b: text.as_bytes(), i: 0 };
        let v = p.value(0)?;
        p.space();
        (p.i == p.b.len()).then_some(v)
    }

    /// The value of a key of an object.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Follows a path of keys.
    pub fn at(&self, path: &[&str]) -> Option<&Json> {
        path.iter().try_fold(self, |v, k| v.get(k))
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Json::Text(s) => Some(s),
            _ => None,
        }
    }

    pub fn number(&self) -> Option<f64> {
        match self {
            Json::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn list(&self) -> &[Json] {
        match self {
            Json::List(l) => l,
            _ => &[],
        }
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

/// Deeper than this is not a catalogue's answer.
const DEPTH: usize = 64;

impl Parser<'_> {
    fn space(&mut self) {
        while self.b.get(self.i).is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> Option<()> {
        self.space();
        (self.b.get(self.i) == Some(&c)).then(|| self.i += 1)
    }

    fn value(&mut self, depth: usize) -> Option<Json> {
        if depth > DEPTH {
            return None;
        }
        self.space();
        match *self.b.get(self.i)? {
            b'{' => {
                self.i += 1;
                let mut pairs = Vec::new();
                if self.eat(b'}').is_some() {
                    return Some(Json::Object(pairs));
                }
                loop {
                    self.space();
                    let key = self.string()?;
                    self.eat(b':')?;
                    pairs.push((key, self.value(depth + 1)?));
                    if self.eat(b',').is_none() {
                        self.eat(b'}')?;
                        return Some(Json::Object(pairs));
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                if self.eat(b']').is_some() {
                    return Some(Json::List(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    if self.eat(b',').is_none() {
                        self.eat(b']')?;
                        return Some(Json::List(items));
                    }
                }
            }
            b'"' => self.string().map(Json::Text),
            b't' => self.word("true", Json::Bool(true)),
            b'f' => self.word("false", Json::Bool(false)),
            b'n' => self.word("null", Json::Null),
            _ => {
                let start = self.i;
                while self.b.get(self.i).is_some_and(|c| c.is_ascii_digit() || b"+-.eE".contains(c)) {
                    self.i += 1;
                }
                std::str::from_utf8(&self.b[start..self.i]).ok()?.parse().ok().map(Json::Number)
            }
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Option<Json> {
        self.b[self.i..].starts_with(word.as_bytes()).then(|| {
            self.i += word.len();
            value
        })
    }

    fn string(&mut self) -> Option<String> {
        if self.b.get(self.i) != Some(&b'"') {
            return None;
        }
        self.i += 1;
        let mut out = String::new();
        loop {
            let start = self.i;
            while self.b.get(self.i).is_some_and(|&c| c != b'"' && c != b'\\') {
                self.i += 1;
            }
            out.push_str(std::str::from_utf8(&self.b[start..self.i]).ok()?);
            match *self.b.get(self.i)? {
                b'"' => {
                    self.i += 1;
                    return Some(out);
                }
                _ => {
                    let escape = *self.b.get(self.i + 1)?;
                    self.i += 2;
                    out.push(match escape {
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'u' => {
                            let hex = |p: &mut Parser| -> Option<u32> {
                                let digits = std::str::from_utf8(p.b.get(p.i..p.i + 4)?).ok()?;
                                p.i += 4;
                                u32::from_str_radix(digits, 16).ok()
                            };
                            let mut code = hex(self)?;
                            // A pair of surrogates is one character.
                            if (0xD800..0xDC00).contains(&code) && self.b[self.i..].starts_with(b"\\u") {
                                self.i += 2;
                                let low = hex(self)?;
                                code = 0x10000 + ((code - 0xD800) << 10) + (low.wrapping_sub(0xDC00) & 0x3FF);
                            }
                            char::from_u32(code).unwrap_or('\u{FFFD}')
                        }
                        other => other as char,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents_are_read_as_trees() {
        let j = Json::parse(r#" {"a": [1, -2.5e2, true, null], "b": {"c": "x\"yé\n"}, "e": []} "#).unwrap();
        assert_eq!(j.get("a").unwrap().list().len(), 4);
        assert_eq!(j.get("a").unwrap().list()[1].number(), Some(-250.0));
        assert_eq!(j.at(&["b", "c"]).and_then(Json::text), Some("x\"yé\n"));
        assert_eq!(j.get("e"), Some(&Json::List(Vec::new())));
        assert_eq!(Json::parse("{\"a\": }"), None);
        assert_eq!(Json::parse("[1] x"), None);
        assert_eq!(Json::parse(&"[".repeat(100)), None);
    }
}
