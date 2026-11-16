//! What JS does with loosely typed values and paths, for reading the files other programs write the way the Electron
//! app reads them.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// JS `!!v`.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// JS `String(v)`.
pub fn string(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e21 => format!("{f:.0}"),
            _ => n.to_string(),
        },
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a.iter().map(|x| if x.is_null() { String::new() } else { string(Some(x)) }).collect::<Vec<_>>().join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

/// JS `String(v || '')`.
pub fn text(v: Option<&Value>) -> String {
    if truthy(v) {
        string(v)
    } else {
        String::new()
    }
}

/// The first truthy value, as `a || b || c`.
pub fn first<'a>(vs: &[Option<&'a Value>]) -> Option<&'a Value> {
    vs.iter().copied().find(|v| truthy(*v)).flatten()
}

/// A value that's a string (`typeof v === 'string'`).
pub fn str_of(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str)
}

/// A string that isn't empty.
pub fn filled(v: Option<&Value>) -> Option<&str> {
    str_of(v).filter(|s| !s.is_empty())
}

fn space(c: char) -> bool {
    // U+0085 is white space to Rust, not to JS; the byte-order mark the other way round
    (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
}

/// JS `s.trim()`.
pub fn trim(s: &str) -> &str {
    s.trim_matches(space)
}

/// JS `s.slice(0, n)`, in UTF-16 units.
pub fn slice(s: &str, n: usize) -> &str {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        units += c.len_utf16();
        if units > n {
            return &s[..i];
        }
    }
    s
}

/// A JS array index ("0", "17"): such keys come first in a JS object, in number order.
fn index_key(k: &str) -> Option<u32> {
    let n: u32 = k.parse().ok()?;
    (n != u32::MAX && n.to_string() == k).then_some(n)
}

/// `Object.entries(o)`: index keys by number, then the rest as they were written.
pub fn entries(o: &Map<String, Value>) -> Vec<(&String, &Value)> {
    let mut idx: Vec<(u32, &String, &Value)> = o.iter().filter_map(|(k, v)| index_key(k).map(|n| (n, k, v))).collect();
    idx.sort_by_key(|e| e.0);
    idx.into_iter().map(|(_, k, v)| (k, v)).chain(o.iter().filter(|(k, _)| index_key(k).is_none())).collect()
}

/// A list's items, an object's values, else nothing (`Array.isArray(v) ? v : Object.values(v)`).
pub fn items(v: Option<&Value>) -> Vec<&Value> {
    match v {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(Value::Object(o)) => entries(o).into_iter().map(|e| e.1).collect(),
        _ => Vec::new(),
    }
}

/// JS `encodeURIComponent(s)`.
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn is_sep(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

/// A Windows path's drive (`C:`) or share (`\\\\server\\share`), and the rest; nothing on other systems.
fn split_prefix(p: &str) -> (String, &str) {
    if !cfg!(windows) {
        return (String::new(), p);
    }
    let b = p.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return (p[..2].to_string(), &p[2..]);
    }
    let mut c = p.chars();
    if c.next().is_some_and(is_sep) && c.next().is_some_and(is_sep) && c.next().is_some_and(|x| !is_sep(x)) {
        let mut names = p[2..].splitn(3, is_sep);
        if let (Some(server), Some(share)) = (names.next(), names.next().filter(|s| !s.is_empty())) {
            let used = 2 + server.len() + 1 + share.len();
            return (format!("\\\\{server}\\{share}\\"), &p[used..]);
        }
    }
    (String::new(), p)
}

/// JS `encodeURI(s)`: as `encode_uri_component`, with an address's own marks kept.
pub fn encode_uri(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'();/?:@&=+$,#".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// JS `decodeURIComponent(s)`; None where JS throws (a `%` without two hex digits, bytes that aren't UTF-8).
pub fn decode_uri_component(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// node's `path.normalize`, without the trailing separator it keeps.
pub fn normalize(p: &str) -> PathBuf {
    let (prefix, rest) = split_prefix(p);
    let rooted = rest.starts_with(is_sep) || prefix.ends_with('\\');
    let mut parts: Vec<&str> = Vec::new();
    for seg in rest.split(is_sep) {
        match seg {
            "" | "." => {}
            ".." => match parts.last() {
                Some(l) if *l != ".." => {
                    parts.pop();
                }
                _ if rooted => {}
                _ => parts.push(".."),
            },
            seg => parts.push(seg),
        }
    }
    let mut out = prefix;
    if rooted && !out.ends_with('\\') {
        out.push(std::path::MAIN_SEPARATOR);
    }
    out.push_str(&parts.join(std::path::MAIN_SEPARATOR_STR));
    PathBuf::from(if out.is_empty() { ".".to_string() } else { out })
}

/// node's `path.join`: the parts end to end (a later absolute one doesn't start over), then normalized.
pub fn join(parts: &[&str]) -> String {
    let all: Vec<&str> = parts.iter().copied().filter(|p| !p.is_empty()).collect();
    normalize(&all.join(std::path::MAIN_SEPARATOR_STR)).to_string_lossy().into_owned()
}

/// node's `path.basename(p)`.
pub fn basename(p: &str) -> String {
    Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// node's `path.dirname(p)`.
pub fn dirname(p: &str) -> String {
    match Path::new(p).parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_string_lossy().into_owned(),
        Some(_) => ".".into(),
        None => p.to_string(),
    }
}

/// node's `path.extname(p)`: from the name's last dot, none for a name that only starts with one.
pub fn extname(p: &str) -> String {
    let n = basename(p);
    match n.rfind('.') {
        Some(i) if i > 0 => n[i..].to_string(),
        _ => String::new(),
    }
}

/// node's `path.basename(p, path.extname(p))`.
pub fn stem(p: &str) -> String {
    let (n, e) = (basename(p), extname(p));
    n[..n.len() - e.len()].to_string()
}

/// A URL's query from its names and values, as a browser's URLSearchParams writes it.
pub fn query(pairs: &[(&str, String)]) -> String {
    let part = |s: &str| s.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => (b as char).to_string(),
        b' ' => "+".to_string(),
        _ => format!("%{b:02X}"),
    }).collect::<String>();
    pairs.iter().map(|(k, v)| format!("{}={}", part(k), part(v))).collect::<Vec<_>>().join("&")
}

/// JS `+v` (`Number(v)`); None is `undefined`.
pub fn number(v: Option<&Value>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => number_of(s),
        Some(Value::Array(a)) => match a.as_slice() {
            [] => 0.0,
            [one] if !one.is_null() => number_of(&string(Some(one))),
            [_] => 0.0,
            _ => f64::NAN,
        },
        Some(Value::Object(_)) => f64::NAN,
    }
}

/// JS `Number("...")`: white space around it off, "" is 0, hex / octal / binary prefixes, Infinity.
fn number_of(s: &str) -> f64 {
    let t = trim(s);
    if t.is_empty() {
        return 0.0;
    }
    let radix = |digits: &str, r: u32| u64::from_str_radix(digits, r).map_or(f64::NAN, |n| n as f64);
    match t.get(..2).map(str::to_ascii_lowercase).as_deref() {
        Some("0x") => return radix(&t[2..], 16),
        Some("0o") => return radix(&t[2..], 8),
        Some("0b") => return radix(&t[2..], 2),
        _ => {}
    }
    match t.strip_prefix(['+', '-']).unwrap_or(t) {
        "Infinity" => return if t.starts_with('-') { f64::NEG_INFINITY } else { f64::INFINITY },
        // Rust also takes "inf", "nan" and "infinity", which JS doesn't
        rest if !rest.starts_with(|c: char| c.is_ascii_digit() || c == '.') => return f64::NAN,
        _ => {}
    }
    t.parse().unwrap_or(f64::NAN)
}

/// `new Date(ms).toISOString()`, for ms since 1970.
pub fn iso_string(ms: i64) -> String {
    let (days, rest) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rest / 3_600_000, rest / 60_000 % 60, rest / 1000 % 60, rest % 1000)
}

/// JS `Date.now()`: milliseconds since 1970, as a number.
pub fn date_now() -> f64 {
    crate::now_ms() as f64
}

/// JS `Math.round`: a half goes up.
pub fn round(n: f64) -> f64 {
    (n + 0.5).floor()
}

/// A number as JS writes it: whole numbers without a fraction.
fn number_text(n: &serde_json::Number) -> String {
    match n.as_f64() {
        Some(0.0) => "0".into(),
        Some(f) if f.fract() == 0.0 && f.abs() < 1e21 => format!("{f:.0}"),
        _ => n.to_string(),
    }
}

/// A whole number or a fraction as a JSON value (2.0 is written 2, as JS has only one kind of number).
pub fn num(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9e15 {
        Value::from(n as i64)
    } else {
        Value::from(n)
    }
}

fn write_json(v: &Value, indent: usize, depth: usize, out: &mut String) {
    let pad = |d: usize, out: &mut String| out.push_str(&" ".repeat(indent * d));
    match v {
        Value::Number(n) => out.push_str(&number_text(n)),
        Value::Array(a) if !a.is_empty() => {
            out.push_str("[\n");
            for (i, x) in a.iter().enumerate() {
                pad(depth + 1, out);
                write_json(x, indent, depth + 1, out);
                out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
            }
            pad(depth, out);
            out.push(']');
        }
        Value::Object(o) if !o.is_empty() => {
            out.push_str("{\n");
            let ents = entries(o);
            for (i, (k, x)) in ents.iter().enumerate() {
                pad(depth + 1, out);
                out.push_str(&Value::from(k.as_str()).to_string());
                out.push_str(": ");
                write_json(x, indent, depth + 1, out);
                out.push_str(if i + 1 < ents.len() { ",\n" } else { "\n" });
            }
            pad(depth, out);
            out.push('}');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// JS `JSON.stringify(v, null, 2)`.
pub fn stringify_pretty(v: &Value) -> String {
    stringify_indented(v, 2)
}

/// JS `JSON.stringify(v, null, indent)`.
pub fn stringify_indented(v: &Value, indent: usize) -> String {
    let mut out = String::new();
    write_json(v, indent, 0, &mut out);
    out
}

/// node's `path.isAbsolute`.
pub fn is_absolute(p: &str) -> bool {
    let b = p.as_bytes();
    if cfg!(windows) {
        match b {
            [b'/' | b'\\', ..] => true,
            [d, b':', b'/' | b'\\', ..] => d.is_ascii_alphabetic(),
            _ => false,
        }
    } else {
        b.first() == Some(&b'/')
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn values() {
        assert_eq!(string(Some(&json!(12))), "12");
        assert_eq!(string(Some(&json!([1, null, "a"]))), "1,,a");
        assert_eq!(text(Some(&json!(0))), "");
        assert_eq!(text(None), "");
        assert!(truthy(Some(&json!({}))) && !truthy(Some(&json!(""))));
        assert_eq!(trim("\u{feff} a\u{a0}"), "a");
        assert_eq!(slice("ab😀c", 3), "ab");
        let o = json!({"b": 1, "10": 2, "2": 3, "01": 4});
        let keys: Vec<&str> = entries(o.as_object().unwrap()).iter().map(|e| e.0.as_str()).collect();
        assert_eq!(keys, ["2", "10", "b", "01"]);
        assert_eq!(encode_uri_component("a b:c/é'(x)"), "a%20b%3Ac%2F%C3%A9'(x)");
        assert_eq!(encode_uri("Box - Front/a b&é#1.jpg"), "Box%20-%20Front/a%20b&%C3%A9#1.jpg");
        assert_eq!((decode_uri_component("a%20b%C3%A9%2f"), decode_uri_component("%zz"), decode_uri_component("%E9"), decode_uri_component("50%")), (Some("a bé/".into()), None, None, None));
        assert_eq!((number(Some(&json!(" 12.5 "))), number(Some(&json!(""))), number(Some(&json!("0x10"))), number(Some(&json!([7]))), number(Some(&json!(true)))), (12.5, 0.0, 16.0, 7.0, 1.0));
        assert!(number(Some(&json!("inf"))).is_nan() && number(None).is_nan() && number(Some(&json!("1a"))).is_nan());
        assert_eq!((round(2.5), round(-2.5), round(1.4)), (3.0, -2.0, 1.0));
        assert_eq!(stringify_pretty(&json!({"b": [1.0, 0.5, {}], "a": {"x": "q\"", "y": []}, "2": null})), "{\n  \"2\": null,\n  \"b\": [\n    1,\n    0.5,\n    {}\n  ],\n  \"a\": {\n    \"x\": \"q\\\"\",\n    \"y\": []\n  }\n}");
    }

    #[cfg(unix)]
    #[test]
    fn paths() {
        assert_eq!(join(&["/a/b", "../c", "./d.exe"]), "/a/c/d.exe");
        assert_eq!(join(&["/a", "/b"]), "/a/b");
        assert_eq!(join(&["", "x"]), "x");
        assert_eq!(join(&["", ""]), ".");
        assert_eq!(join(&["a", "../.."]), "..");
        assert_eq!((join(&["/", ".."]), join(&["/a/", "b/"]), join(&["a//b", "."])), ("/".into(), "/a/b".into(), "a/b".into()));
        assert_eq!((basename("/a/b.tar.gz"), dirname("/a/b.c"), extname("/a/.b"), stem("/a/b.tar.gz")), ("b.tar.gz".into(), "/a".into(), "".into(), "b.tar".into()));
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths() {
        // as node's path.win32.join gives them
        let cases: &[(&[&str], &str)] = &[
            (&["C:/a", "b"], "C:\\a\\b"),
            (&["C:\\a", "D:\\b"], "C:\\a\\D:\\b"),
            (&["C:\\a\\", "..", "x"], "C:\\x"),
            (&["\\\\srv\\share\\a", "..", "..", "b"], "\\\\srv\\share\\b"),
            (&["C:", "x"], "C:\\x"),
            (&["C:\\", ".."], "C:\\"),
            (&["a", "../.."], ".."),
            (&["//srv/share", "x"], "\\\\srv\\share\\x"),
            (&["/x", "y"], "\\x\\y"),
        ];
        for (parts, want) in cases {
            assert_eq!(join(parts), *want, "{parts:?}");
        }
        assert!(is_absolute("\\x") && is_absolute("/x") && is_absolute("C:\\x") && !is_absolute("C:x") && !is_absolute("x\\y"));
    }
}
