//! The interface in another language: an English text on screen swapped for its translation, from the same
//! dictionaries the Electron app uses (renderer/i18n-fr.js, renderer/i18n-tr.js). Keep the matching in sync with
//! renderer/i18n-dict.js.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, OnceLock};

use regex::Regex;
use serde_json::Value;

pub struct Dict {
    exact: HashMap<String, String>,
    pats: Vec<(Regex, String)>,
    cache: Mutex<HashMap<String, Option<String>>>,
}

const FR: &str = include_str!("../../../renderer/i18n-fr.js");
const TR: &str = include_str!("../../../renderer/i18n-tr.js");

/// The languages there's a dictionary for, by `ui.language`.
pub const LANGS: [&str; 2] = ["fr", "tr"];

/// The dictionary for a language ("fr", "tr"); None for English or one there's none for.
pub fn dict(lang: &str) -> Option<&'static Dict> {
    static FRD: OnceLock<Option<Dict>> = OnceLock::new();
    static TRD: OnceLock<Option<Dict>> = OnceLock::new();
    match lang {
        "fr" => FRD.get_or_init(|| Dict::parse(FR)).as_ref(),
        "tr" => TRD.get_or_init(|| Dict::parse(TR)).as_ref(),
        _ => None,
    }
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A JavaScript pattern as the regex crate reads it: `\'`, `\"` and the like are plain characters there.
fn js_regex(src: &str) -> Option<Regex> {
    let mut out = String::with_capacity(src.len() + 4);
    out.push_str("(?s)");
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(e) if e.is_ascii_alphanumeric() || r"\.+*?()|[]{}^$#&-~".contains(e) => {
                    out.push('\\');
                    out.push(e);
                }
                Some(e) => out.push_str(&regex::escape(&e.to_string())),
                None => out.push_str(r"\\"),
            }
        } else {
            out.push(c);
        }
    }
    Regex::new(&out).ok()
}

impl Dict {
    /// `window.BS_I18N_XX = { exact: {...}, patterns: [[source, template], ...] };` after a comment
    fn parse(js: &str) -> Option<Dict> {
        let body = &js[js.find("= {")? + 2..=js.rfind('}')?];
        let v: Value = serde_json::from_str(body).ok()?;
        let exact = v["exact"].as_object()?.iter().filter_map(|(k, t)| Some((k.clone(), t.as_str()?.to_string()))).collect();
        let pats = v["patterns"].as_array()?.iter().filter_map(|p| Some((js_regex(p.get(0)?.as_str()?)?, p.get(1)?.as_str()?.to_string()))).collect();
        Some(Dict { exact, pats, cache: Mutex::new(HashMap::new()) })
    }

    fn fill(&self, f: &str, m: &regex::Captures) -> String {
        static SLOT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{(\d+)(?:\?([^}]*))?\}").unwrap());
        SLOT.replace_all(f, |s: &regex::Captures| {
            let Some(v) = s[1].parse::<usize>().ok().and_then(|n| m.get(n)).map(|v| v.as_str()) else { return String::new() };
            match s.get(2) {
                Some(opt) => if v.is_empty() { String::new() } else { opt.as_str().to_string() },
                None => self.exact.get(&norm(v)).cloned().unwrap_or_else(|| v.to_string()),
            }
        })
        .into_owned()
    }

    /// The text in this language, or as it is when the dictionary doesn't have it.
    pub fn tr(&self, text: &str) -> String {
        if !text.chars().any(|c| c.is_ascii_alphabetic()) {
            return text.to_string();
        }
        let core = norm(text);
        if core.is_empty() {
            return text.to_string();
        }
        let cached = self.cache.lock().unwrap().get(&core).cloned();
        let out = cached.unwrap_or_else(|| {
            let out = self.exact.get(&core).cloned().or_else(|| {
                (core.encode_utf16().count() < 600).then(|| self.pats.iter().find_map(|(re, f)| re.captures(&core).map(|m| self.fill(f, &m)))).flatten()
            });
            let mut c = self.cache.lock().unwrap();
            if c.len() > 6000 {
                c.clear();
            }
            c.insert(core.clone(), out.clone());
            out
        });
        match out {
            Some(t) => {
                let lead = &text[..text.len() - text.trim_start().len()];
                let trail = &text[text.trim_end().len()..];
                format!("{lead}{t}{trail}")
            }
            None => text.to_string(),
        }
    }
}

/// `text` in `lang`, as it is for English.
pub fn tr(lang: &str, text: &str) -> String {
    dict(lang).map_or_else(|| text.to_string(), |d| d.tr(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pattern_compiles() {
        for js in [FR, TR] {
            let v: Value = serde_json::from_str(&js[js.find("= {").unwrap() + 2..=js.rfind('}').unwrap()]).unwrap();
            let bad: Vec<&str> = v["patterns"].as_array().unwrap().iter().map(|p| p[0].as_str().unwrap()).filter(|s| js_regex(s).is_none()).collect();
            assert!(bad.is_empty(), "{bad:?}");
        }
    }
}
