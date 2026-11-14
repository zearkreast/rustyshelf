//! What JS does with loosely typed values and paths, for reading the files other programs write the way the Electron
//! app reads them.

use std::path::{Component, Path, PathBuf};

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

/// node's `path.normalize`, without the trailing separator it keeps.
pub fn normalize(p: &str) -> PathBuf {
    let mut out = PathBuf::new();
    for c in Path::new(p).components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(".."),
            },
            c => out.push(c),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
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
    }

    #[cfg(unix)]
    #[test]
    fn paths() {
        assert_eq!(join(&["/a/b", "../c", "./d.exe"]), "/a/c/d.exe");
        assert_eq!(join(&["/a", "/b"]), "/a/b");
        assert_eq!(join(&["", "x"]), "x");
        assert_eq!(join(&["", ""]), ".");
        assert_eq!(join(&["a", "../.."]), "..");
        assert_eq!((basename("/a/b.tar.gz"), dirname("/a/b.c"), extname("/a/.b"), stem("/a/b.tar.gz")), ("b.tar.gz".into(), "/a".into(), "".into(), "b.tar".into()));
    }
}
