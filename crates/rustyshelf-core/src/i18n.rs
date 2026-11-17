//! The interface in another language: an English text on screen swapped for its translation, from the Electron app's
//! dictionaries (`assets/i18n`: its renderer/i18n-fr.js with Zak's French from ZakAuMiel/boxshelf-source merged in,
//! renderer/i18n-tr.js, and Thanks2111's Brazilian Portuguese, renderer/i18n-pt-data.js, as JSON). The matching is
//! renderer/i18n-dict.js's and, for Portuguese, renderer/i18n.js's.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex, OnceLock};

use regex::Regex;
use serde_json::Value;

pub struct Dict {
    exact: HashMap<String, String>,
    pats: Vec<(Regex, String)>,
    cache: Mutex<HashMap<String, Option<String>>>,
}

const FR: &str = include_str!("../assets/i18n/fr.json");
const TR: &str = include_str!("../assets/i18n/tr.json");
const PT: &str = include_str!("../assets/i18n/pt.json");

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
    /// `{"exact": {...}, "patterns": [[source, template], ...]}`
    fn parse(json: &str) -> Option<Dict> {
        let v: Value = serde_json::from_str(json).ok()?;
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
    if lang == "pt" {
        static PTD: OnceLock<Option<Pt>> = OnceLock::new();
        return PTD.get_or_init(|| Pt::parse(PT)).as_ref().map_or_else(|| text.to_string(), |d| d.tr(text));
    }
    dict(lang).map_or_else(|| text.to_string(), |d| d.tr(text))
}

/// A part of a Portuguese template's output: words, a group's text (itself translated), or one of two words by a
/// group (a plural).
enum Piece {
    Text(String),
    Group(usize),
    Pick { g: usize, a: String, b: String, words: [String; 2] },
}

struct Template {
    full: Regex,
    full_i: Regex,
    part: Regex,
    lit: String,
    lit_lower: String,
    out: Vec<Piece>,
}

/// The Brazilian Portuguese dictionary: exact texts, templates with the parts that change, and fragments of longer
/// texts put together from pieces.
struct Pt {
    exact: HashMap<String, String>,
    lower: HashMap<String, String>,
    /// texts already in Portuguese: never translated again
    done: Mutex<HashSet<String>>,
    templates: Vec<Template>,
    /// fixed pieces of two words or more, longest first
    fragments: Vec<(String, String)>,
    cache: Mutex<HashMap<String, Option<String>>>,
}

impl Pt {
    /// `{"d": {english: portuguese}, "t": [[source, [piece…], literal, groups], …]}`
    fn parse(json: &str) -> Option<Pt> {
        let v: Value = serde_json::from_str(json).ok()?;
        let pairs: Vec<(String, String)> = v["d"].as_object()?.iter().filter_map(|(k, t)| Some((k.clone(), t.as_str()?.to_string()))).collect();
        let exact: HashMap<String, String> = pairs.iter().cloned().collect();
        let mut lower = HashMap::new();
        for (k, t) in &pairs {
            lower.entry(k.to_lowercase()).or_insert_with(|| t.clone());
        }
        let done = exact.values().cloned().collect();
        let piece = |p: &Value| -> Option<Piece> {
            if let Some(t) = p.as_str() {
                return Some(Piece::Text(t.to_string()));
            }
            let g = p.get("g")?.as_u64()? as usize;
            match p.get("o").and_then(Value::as_array) {
                Some(o) => Some(Piece::Pick {
                    g,
                    a: p.get("a").and_then(Value::as_str).unwrap_or("").to_string(),
                    b: p.get("b").and_then(Value::as_str).unwrap_or("").to_string(),
                    words: [o.first()?.as_str()?.to_string(), o.get(1)?.as_str()?.to_string()],
                }),
                None => Some(Piece::Group(g)),
            }
        };
        let templates = v["t"]
            .as_array()?
            .iter()
            .filter_map(|t| {
                let src = t.get(0)?.as_str()?;
                let lit = t.get(2)?.as_str()?.to_string();
                Some(Template {
                    full: js_regex(&format!("^(?:{src})$"))?,
                    full_i: js_regex(&format!("(?i)^(?:{src})$"))?,
                    part: js_regex(src)?,
                    lit_lower: lit.to_lowercase(),
                    lit,
                    out: t.get(1)?.as_array()?.iter().filter_map(piece).collect(),
                })
            })
            .collect();
        let mut fragments: Vec<(String, String)> =
            pairs.into_iter().filter(|(k, _)| k.encode_utf16().count() >= 8 && k.contains(char::is_whitespace) && has_three_letters(k)).collect();
        fragments.sort_by_key(|(k, _)| std::cmp::Reverse(k.encode_utf16().count()));
        Some(Pt { exact, lower, done: Mutex::new(done), templates, fragments, cache: Mutex::new(HashMap::new()) })
    }

    fn fill(&self, out: &[Piece], m: &regex::Captures) -> String {
        let group = |g: usize| m.get(g + 1).map_or("", |v| v.as_str());
        out.iter()
            .map(|p| match p {
                Piece::Text(t) => t.clone(),
                Piece::Group(g) => self.tr(group(*g)),
                Piece::Pick { g, a, b, words } => {
                    let v = group(*g);
                    let one = if v == a { 0 } else if v == b { 1 } else if v.is_empty() || v == "1" { 0 } else { 1 };
                    words[one].clone()
                }
            })
            .collect()
    }

    /// A text in capitals (covers, signs): found whatever its case, given back in capitals.
    fn caps(&self, key: &str) -> Option<String> {
        let l = key.to_lowercase();
        if let Some(v) = self.lower.get(&l) {
            return Some(v.to_uppercase());
        }
        self.templates.iter().filter(|t| l.contains(&t.lit_lower)).find_map(|t| t.full_i.captures(key).map(|m| self.fill(&t.out, &m).to_uppercase()))
    }

    /// A text put together from several pieces: the known pieces in it swapped.
    fn pieces(&self, key: &str) -> Option<String> {
        static OF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d[\d.,]*) of (\d[\d.,]*)").unwrap());
        let mut s = key.to_string();
        let mut changed = false;
        for t in self.templates.iter().filter(|t| t.lit.encode_utf16().count() >= 6) {
            if !s.contains(&t.lit) {
                continue;
            }
            let n = t.part.replace_all(&s, |m: &regex::Captures| self.fill(&t.out, m)).into_owned();
            changed |= t.part.is_match(&s);
            s = n;
        }
        let g = OF.replace_all(&s, "$1 de $2").into_owned();
        if g != s {
            (s, changed) = (g, true);
        }
        for (k, v) in &self.fragments {
            if s.contains(k.as_str()) {
                s = s.replace(k.as_str(), v);
                changed = true;
            }
        }
        changed.then_some(s)
    }

    fn tr(&self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }
        let key = norm(text);
        if key.is_empty() || !key.chars().any(|c| c.is_ascii_alphabetic()) || self.done.lock().unwrap().contains(&key) {
            return text.to_string();
        }
        let cached = self.cache.lock().unwrap().get(&key).cloned();
        let hit = match cached {
            Some(h) => h,
            None => {
                let hit = self
                    .exact
                    .get(&key)
                    .cloned()
                    .or_else(|| self.templates.iter().filter(|t| key.contains(&t.lit)).find_map(|t| t.full.captures(&key).map(|m| self.fill(&t.out, &m))))
                    .or_else(|| (key == key.to_uppercase() && has_three_capitals(&key)).then(|| self.caps(&key)).flatten())
                    .or_else(|| self.pieces(&key));
                let mut c = self.cache.lock().unwrap();
                if c.len() > 20000 {
                    c.clear();
                }
                c.insert(key, hit.clone());
                if let Some(h) = &hit {
                    self.done.lock().unwrap().insert(h.clone());
                }
                hit
            }
        };
        match hit {
            Some(t) => {
                let lead = &text[..text.len() - text.trim_start().len()];
                let trail = &text[text.trim_end().len()..];
                format!("{lead}{t}{trail}")
            }
            None => text.to_string(),
        }
    }
}

fn has_three_letters(s: &str) -> bool {
    s.as_bytes().windows(3).any(|w| w.iter().all(u8::is_ascii_alphabetic))
}

fn has_three_capitals(s: &str) -> bool {
    s.as_bytes().windows(3).any(|w| w.iter().all(u8::is_ascii_uppercase))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pattern_compiles() {
        for json in [FR, TR] {
            let v: Value = serde_json::from_str(json).unwrap();
            let bad: Vec<&str> = v["patterns"].as_array().unwrap().iter().map(|p| p[0].as_str().unwrap()).filter(|s| js_regex(s).is_none()).collect();
            assert!(bad.is_empty(), "{bad:?}");
        }
    }

    #[test]
    fn every_language_loads() {
        assert_eq!(tr("fr", "Home console"), "Console de salon");
        assert_eq!(tr("tr", "Home console"), "Ev konsolu");
        assert_eq!(tr("pt", "Home"), "Início");
    }

    #[test]
    fn plurals_by_count_and_short_patterns_only_on_short_texts() {
        assert_eq!((tr("fr", "1 hidden"), tr("fr", "3 hidden")), ("1 masqué".into(), "3 masqués".into()));
        assert_eq!(tr("fr", "Every game you hid stays hidden"), "Every game you hid stays hidden");
        assert_eq!(tr("fr", "1 track found"), "1 morceau trouvé");
    }

    #[test]
    fn game_streaming_and_pc_backs_rows() {
        assert_eq!((tr("fr", "Game streaming"), tr("tr", "Game streaming")), ("Streaming de jeux".into(), "Oyun akışı".into()));
        assert_eq!(tr("fr", "12 apps on the host"), "12 applis sur l'hôte");
        assert_eq!((tr("fr", "1 game from mypc"), tr("fr", "12 games from living-room-pc")), ("1 jeu de mypc".into(), "12 jeux de living-room-pc".into()));
        assert_eq!(tr("tr", "1 game from mypc"), "mypc: 1 oyun");
        assert_eq!(tr("tr", "LaunchBox's PC games: Downloading the LaunchBox game database… 7% · 8 / 105 MB"), "LaunchBox'ın PC oyunları: LaunchBox oyun veritabanı indiriliyor… %7 · 8 / 105 MB");
        assert_eq!(tr("tr", "Downloading… 7%"), "İndiriliyor… %7");
        assert_eq!(tr("pt", "Reading… 40%"), "Lendo… 40%");
        assert_eq!(tr("tr", "3 of 5 ready"), "5 emülatörden 3 tanesi hazır");
        assert_eq!(tr("tr", "4.2% · 15 pts"), "%4.2 · 15 puan");
        assert_eq!(tr("tr", "How to get it"), "Nasıl alınır?");
        assert_eq!(tr("pt", "3 of 5 ready"), "3 de 5 prontos");
        assert_eq!(tr("fr", "21 · Earned"), "21 · Obtenu");
        assert_eq!(tr("tr", "LaunchBox's PC games: Reading the game database… 12%"), "LaunchBox'ın PC oyunları: Oyun veritabanı okunuyor… %12");
    }
}
