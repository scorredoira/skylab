//! The little JSON the protocol and the model file need: a value, a parser and a writer.
//! Numbers are `f64`, so integers are exact up to 2^53.

use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    /// Keys keep the order they were written in.
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn obj<K: Into<String>>(pairs: Vec<(K, Json)>) -> Json {
        Json::Obj(pairs.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    pub fn str(s: impl Into<String>) -> Json {
        Json::Str(s.into())
    }

    pub fn num(x: impl Into<f64>) -> Json {
        Json::Num(x.into())
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(x) => Some(*x),
            _ => None,
        }
    }

    /// A non-negative whole number.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Num(x) if *x >= 0.0 && x.fract() == 0.0 && *x <= 18_446_744_073_709_549_568.0 => {
                Some(*x as u64)
            }
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Obj(o) => Some(o),
            _ => None,
        }
    }

    pub fn parse(text: &str) -> Result<Json, String> {
        let mut p = Parser {
            s: text.as_bytes(),
            i: 0,
        };
        let v = p.value(0)?;
        p.space();
        if p.i != p.s.len() {
            return Err(p.error("unexpected text after the value"));
        }
        Ok(v)
    }
}

impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Json::Null => f.write_str("null"),
            Json::Bool(b) => write!(f, "{b}"),
            Json::Num(x) if !x.is_finite() => f.write_str("null"),
            Json::Num(x) if x.fract() == 0.0 && x.abs() < 9.0e15 => write!(f, "{}", *x as i64),
            Json::Num(x) => write!(f, "{x}"),
            Json::Str(s) => write_string(f, s),
            Json::Arr(a) => {
                f.write_str("[")?;
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{v}")?;
                }
                f.write_str("]")
            }
            Json::Obj(o) => {
                f.write_str("{")?;
                for (i, (k, v)) in o.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write_string(f, k)?;
                    write!(f, ":{v}")?;
                }
                f.write_str("}")
            }
        }
    }
}

fn write_string(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    f.write_str("\"")?;
    for c in s.chars() {
        match c {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            c if (c as u32) < 0x20 => write!(f, "\\u{:04x}", c as u32)?,
            c => write!(f, "{c}")?,
        }
    }
    f.write_str("\"")
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn error(&self, what: &str) -> String {
        format!("JSON: {what} at byte {}", self.i)
    }

    fn space(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }

    fn eat(&mut self, word: &str) -> bool {
        if self.s[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > 64 {
            return Err(self.error("nested too deep"));
        }
        self.space();
        let Some(&c) = self.s.get(self.i) else {
            return Err(self.error("unexpected end"));
        };
        match c {
            b'n' if self.eat("null") => Ok(Json::Null),
            b't' if self.eat("true") => Ok(Json::Bool(true)),
            b'f' if self.eat("false") => Ok(Json::Bool(false)),
            b'"' => self.string().map(Json::Str),
            b'[' => {
                self.i += 1;
                let mut out = Vec::new();
                self.space();
                if self.eat("]") {
                    return Ok(Json::Arr(out));
                }
                loop {
                    out.push(self.value(depth + 1)?);
                    self.space();
                    if self.eat("]") {
                        return Ok(Json::Arr(out));
                    }
                    if !self.eat(",") {
                        return Err(self.error("expected , or ]"));
                    }
                }
            }
            b'{' => {
                self.i += 1;
                let mut out = Vec::new();
                self.space();
                if self.eat("}") {
                    return Ok(Json::Obj(out));
                }
                loop {
                    self.space();
                    if self.s.get(self.i) != Some(&b'"') {
                        return Err(self.error("expected a key"));
                    }
                    let key = self.string()?;
                    self.space();
                    if !self.eat(":") {
                        return Err(self.error("expected :"));
                    }
                    out.push((key, self.value(depth + 1)?));
                    self.space();
                    if self.eat("}") {
                        return Ok(Json::Obj(out));
                    }
                    if !self.eat(",") {
                        return Err(self.error("expected , or }"));
                    }
                }
            }
            b'-' | b'0'..=b'9' => {
                let start = self.i;
                while self.i < self.s.len()
                    && matches!(
                        self.s[self.i],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.i += 1;
                }
                std::str::from_utf8(&self.s[start..self.i])
                    .ok()
                    .and_then(|t| t.parse::<f64>().ok())
                    .map(Json::Num)
                    .ok_or_else(|| self.error("bad number"))
            }
            _ => Err(self.error("unexpected character")),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1; // the opening quote
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(&c) = self.s.get(self.i) else {
                return Err(self.error("unterminated string"));
            };
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let Some(&e) = self.s.get(self.i) else {
                        return Err(self.error("unterminated string"));
                    };
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'u' => {
                            let mut code = self.hex4()?;
                            // A surrogate pair is one character.
                            if (0xD800..0xDC00).contains(&code) && self.eat("\\u") {
                                let low = self.hex4()?;
                                code = 0x10000
                                    + ((code - 0xD800) << 10)
                                    + (low.wrapping_sub(0xDC00) & 0x3FF);
                            }
                            char::from_u32(code).unwrap_or('\u{FFFD}')
                        }
                        _ => return Err(self.error("bad escape")),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                c => out.push(c),
            }
        }
        String::from_utf8(out).map_err(|_| self.error("string is not UTF-8"))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .s
            .get(self.i..self.i + 4)
            .and_then(|d| std::str::from_utf8(d).ok())
            .and_then(|d| u32::from_str_radix(d, 16).ok())
            .ok_or_else(|| self.error("bad \\u escape"))?;
        self.i += 4;
        Ok(digits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let text = r#"{"a":[1,2.5,-3e2,true,null],"b":{"c":"x\n\"y\" é 😀"},"d":[]}"#;
        let v = Json::parse(text).unwrap();
        assert_eq!(
            v.get("a").unwrap().as_array().unwrap()[2],
            Json::Num(-300.0)
        );
        assert_eq!(
            v.get("b").unwrap().get("c").unwrap().as_str(),
            Some("x\n\"y\" é 😀")
        );
        assert_eq!(Json::parse(&v.to_string()).unwrap(), v);
        assert_eq!(Json::Num(42.0).to_string(), "42");
        assert_eq!(Json::Num(0.1).to_string(), "0.1");
    }

    #[test]
    fn rejects_garbage() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\"}",
            "tru",
            "1 2",
            "\"abc",
            "{\"a\":1,}",
        ] {
            assert!(Json::parse(bad).is_err(), "{bad}");
        }
    }
}
