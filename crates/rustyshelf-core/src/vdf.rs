// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/vdf.js, as RustyShelf has it.
//! Valve's text KeyValues format (.vdf / .acf).

/// A value: text, or keys in the order a JS object keeps them (see `Vdf::entries`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Vdf {
    Str(String),
    Obj(Vec<(String, Vdf)>),
}

fn space(c: char) -> bool {
    // JS's \s, which takes a byte-order mark too
    c.is_whitespace() || c == '\u{feff}'
}

struct Parser {
    t: Vec<char>,
    i: usize,
}

impl Parser {
    fn skip(&mut self) {
        while self.i < self.t.len() {
            let c = self.t[self.i];
            if space(c) {
                self.i += 1;
            } else if c == '/' && self.t.get(self.i + 1) == Some(&'/') {
                while self.i < self.t.len() && self.t[self.i] != '\n' {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
    }

    fn str(&mut self) -> String {
        self.skip();
        let mut out = String::new();
        if self.t.get(self.i) == Some(&'"') {
            self.i += 1;
            while self.i < self.t.len() && self.t[self.i] != '"' {
                if self.t[self.i] == '\\' && self.i + 1 < self.t.len() {
                    out.push(match self.t[self.i + 1] {
                        'n' => '\n',
                        't' => '\t',
                        e => e,
                    });
                    self.i += 2;
                } else {
                    out.push(self.t[self.i]);
                    self.i += 1;
                }
            }
            self.i += 1;
            return out;
        }
        while self.i < self.t.len() && !(space(self.t[self.i]) || matches!(self.t[self.i], '{' | '}' | '"')) {
            out.push(self.t[self.i]);
            self.i += 1;
        }
        out
    }

    fn obj(&mut self) -> Vec<(String, Vdf)> {
        let mut o: Vec<(String, Vdf)> = Vec::new();
        loop {
            self.skip();
            if self.i >= self.t.len() {
                return o;
            }
            if self.t[self.i] == '}' {
                self.i += 1;
                return o;
            }
            let at = self.i;
            let key = self.str();
            self.skip();
            let v = if self.t.get(self.i) == Some(&'{') {
                self.i += 1;
                Vdf::Obj(self.obj())
            } else {
                Vdf::Str(self.str())
            };
            // a key given twice keeps its first place and its last value, as in a JS object
            match o.iter_mut().find(|(k, _)| *k == key) {
                Some(e) => e.1 = v,
                None => o.push((key, v)),
            }
            // whatever stops a string without being read is stepped over: always forward
            if self.i == at {
                self.i += 1;
            }
        }
    }
}

pub fn parse(text: &str) -> Vdf {
    Vdf::Obj(Parser { t: text.chars().collect(), i: 0 }.obj())
}

/// A JS array index ("0", "17"): such keys come first in a JS object, in number order.
fn index_key(k: &str) -> Option<u32> {
    let n: u32 = k.parse().ok()?;
    (n != u32::MAX && n.to_string() == k).then_some(n)
}

impl Vdf {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Vdf::Str(s) => Some(s),
            Vdf::Obj(_) => None,
        }
    }

    /// The keys and values in JS's order: index keys by number, then the rest as they came.
    pub fn entries(&self) -> Vec<(&str, &Vdf)> {
        let Vdf::Obj(o) = self else { return Vec::new() };
        let mut idx: Vec<(u32, &str, &Vdf)> = o.iter().filter_map(|(k, v)| index_key(k).map(|n| (n, k.as_str(), v))).collect();
        idx.sort_by_key(|e| e.0);
        idx.into_iter().map(|(_, k, v)| (k, v)).chain(o.iter().filter(|(k, _)| index_key(k).is_none()).map(|(k, v)| (k.as_str(), v))).collect()
    }

    /// A value down a path of keys, any case.
    pub fn get(&self, keys: &[&str]) -> Option<&Vdf> {
        let mut cur = self;
        for k in keys {
            cur = cur.entries().into_iter().find(|(x, _)| x.to_lowercase() == k.to_lowercase())?.1;
        }
        Some(cur)
    }

    pub fn text(&self, keys: &[&str]) -> Option<&str> {
        self.get(keys).and_then(Vdf::as_str)
    }

    /// JS `+(value || 0)`: 0 when it's missing or empty, NaN when it isn't a number.
    pub fn num(&self, keys: &[&str]) -> f64 {
        match self.get(keys) {
            None => 0.0,
            Some(Vdf::Str(s)) if s.is_empty() => 0.0,
            Some(Vdf::Str(s)) => {
                let t = s.trim_matches(space);
                if t.is_empty() {
                    0.0
                } else {
                    t.parse().unwrap_or(f64::NAN)
                }
            }
            Some(Vdf::Obj(_)) => f64::NAN,
        }
    }
}

/// JS's `n & mask` on a number.
pub fn bits(n: f64) -> i64 {
    if n.is_finite() {
        n.trunc() as i64 & 0xFFFF_FFFF
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyvalues() {
        let v = parse("\u{feff}\"libraryfolders\"\n{\n \"1\" { \"path\" \"D:\\\\SteamLibrary\" }\n \"0\"\u{a0}{ \"path\" \"C:\\\\Steam\" } // c\n \"x\" \"a\\\"b\" \"x\" \"c\" }");
        let lf = v.get(&["LibraryFolders"]).unwrap();
        let keys: Vec<&str> = lf.entries().iter().map(|e| e.0).collect();
        assert_eq!(keys, ["0", "1", "x"]);
        assert_eq!(lf.text(&["1", "PATH"]), Some("D:\\SteamLibrary"));
        assert_eq!(lf.text(&["x"]), Some("c"));
        assert_eq!(parse("a {").get(&["a"]), Some(&Vdf::Obj(vec![])));
        // stray characters never stop the parser
        assert!(matches!(parse("}}{{\"\"} \u{1}"), Vdf::Obj(_)));
    }

    #[test]
    fn numbers() {
        let v = parse(r#""s" { "a" "1026" "b" "" "c" "x" }"#);
        let s = v.get(&["s"]).unwrap();
        assert_eq!(bits(s.num(&["a"])) & 4, 0);
        assert_eq!(s.num(&["b"]), 0.0);
        assert!(s.num(&["c"]).is_nan());
        assert_eq!(s.num(&["none"]), 0.0);
    }
}
