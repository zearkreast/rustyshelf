//! A Steam game's achievements, read from what Steam keeps on this PC. Steam caches every game's achievement
//! list and your progress in its `appcache/stats` folder, in its binary KeyValues format:
//!   `UserGameStatsSchema_<appid>.bin`: stats → <stat> → bits → <bit> { name, bit, display { name, desc, hidden,
//!   icon, icon_gray } } (names in every language Steam has; English is used, else the first)
//!   `UserGameStats_<account id>_<appid>.bin`: cache → <stat> { data (the unlocked bits), AchievementTimes }
//! The account is the one Steam last signed in. Nothing is sent anywhere: a game Steam hasn't cached stats for
//! (never started on this PC) has none. The answer has the shape RetroAchievements' has (`ra::Ra::progress`),
//! with no points.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::{js, vdf};

/// A value of Valve's binary KeyValues.
#[derive(Debug, Clone, PartialEq)]
pub enum Kv {
    /// by key, as they came
    Map(Vec<(String, Kv)>),
    Text(String),
    Num(f64),
}

/// A key that's an array index to JavaScript: such keys come first in an object, in number order.
fn index_key(k: &str) -> Option<u32> {
    let n: u32 = k.parse().ok()?;
    (n != u32::MAX && n.to_string() == k).then_some(n)
}

fn put(map: &mut Vec<(String, Kv)>, key: String, value: Kv) {
    match map.iter_mut().find(|e| e.0 == key) {
        Some(e) => e.1 = value,
        None => map.push((key, value)),
    }
}

impl Kv {
    /// The keys and values in the order JavaScript walks an object in.
    fn entries(&self) -> Vec<(&str, &Kv)> {
        let Kv::Map(o) = self else { return Vec::new() };
        let mut idx: Vec<(u32, &str, &Kv)> = o.iter().filter_map(|(k, v)| index_key(k).map(|n| (n, k.as_str(), v))).collect();
        idx.sort_by_key(|e| e.0);
        idx.into_iter().map(|(_, k, v)| (k, v)).chain(o.iter().filter(|(k, _)| index_key(k).is_none()).map(|(k, v)| (k.as_str(), v))).collect()
    }

    /// The same with every key in lower case: of two that only differ by case, the later one's value.
    fn lower(&self) -> Kv {
        let mut out = Vec::new();
        for (k, v) in self.entries() {
            put(&mut out, k.to_lowercase(), v.clone());
        }
        Kv::Map(out)
    }

    fn get(&self, key: &str) -> Option<&Kv> {
        match self {
            Kv::Map(o) => o.iter().find(|e| e.0 == key).map(|e| &e.1),
            _ => None,
        }
    }

    fn truthy(&self) -> bool {
        match self {
            Kv::Map(_) => true,
            Kv::Text(t) => !t.is_empty(),
            Kv::Num(n) => *n != 0.0 && !n.is_nan(),
        }
    }

    /// JavaScript's `+value`.
    fn number(&self) -> f64 {
        match self {
            Kv::Num(n) => *n,
            Kv::Text(t) => js::number(Some(&json!(t))),
            Kv::Map(_) => f64::NAN,
        }
    }

    /// JavaScript's `String(value)`, for the texts and the numbers.
    fn string(&self) -> String {
        match self {
            Kv::Text(t) => t.clone(),
            Kv::Num(n) => js::string(Some(&js::num(*n))),
            Kv::Map(_) => "[object Object]".into(),
        }
    }

    pub fn to_json(&self) -> Value {
        match self {
            Kv::Map(_) => Value::Object(self.entries().into_iter().map(|(k, v)| (k.to_string(), v.to_json())).collect()),
            Kv::Text(t) => json!(t),
            Kv::Num(n) => js::num(*n),
        }
    }
}

/// Reads binary KeyValues: a type byte, the key (zero-ended), the value; 0 opens a map, 8 (or 11) closes it.
pub fn parse_bin(b: &[u8]) -> Result<Kv, String> {
    fn cstr(b: &[u8], i: &mut usize) -> String {
        let start = (*i).min(b.len());
        let end = b[start..].iter().position(|c| *c == 0).map_or(b.len(), |n| start + n);
        *i = end + 1;
        String::from_utf8_lossy(&b[start..end]).into_owned()
    }
    fn fixed<const N: usize>(b: &[u8], i: &mut usize) -> Result<[u8; N], String> {
        let bytes = b.get(*i..*i + N).ok_or("The file ends in the middle of a number")?;
        *i += N;
        Ok(bytes.try_into().expect("N bytes"))
    }
    fn map(b: &[u8], i: &mut usize) -> Result<Kv, String> {
        let mut o = Vec::new();
        while *i < b.len() {
            let t = b[*i];
            *i += 1;
            if t == 8 || t == 11 {
                break;
            }
            let k = cstr(b, i);
            let v = match t {
                0 => map(b, i)?,
                1 => Kv::Text(cstr(b, i)),
                2 | 4 | 6 => Kv::Num(f64::from(i32::from_le_bytes(fixed(b, i)?))),
                3 => Kv::Num(f64::from(f32::from_le_bytes(fixed(b, i)?))),
                7 => Kv::Num(u64::from_le_bytes(fixed(b, i)?) as f64),
                10 => Kv::Num(i64::from_le_bytes(fixed(b, i)?) as f64),
                5 => {
                    let start = *i;
                    while *i + 1 < b.len() && (b[*i] != 0 || b[*i + 1] != 0) {
                        *i += 2;
                    }
                    let units: Vec<u16> = b[start..(*i).min(b.len())].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
                    *i += 2;
                    Kv::Text(String::from_utf16_lossy(&units))
                }
                _ => return Err(format!("Unknown KeyValues type {t}")),
            };
            put(&mut o, k, v);
        }
        Ok(Kv::Map(o))
    }
    map(b, &mut 0)
}

fn read_bin(file: &Path) -> Option<Kv> {
    parse_bin(&fs::read(file).ok()?).ok()
}

/// A text as Steam keeps it: itself, or its English, else its first language.
fn text(v: Option<&Kv>) -> String {
    match v {
        Some(Kv::Text(t)) => t.clone(),
        Some(map @ Kv::Map(_)) => match map.lower().get("english") {
            Some(Kv::Text(t)) if !t.is_empty() => t.clone(),
            _ => map.entries().into_iter().find_map(|(_, x)| if let Kv::Text(t) = x { Some(t.clone()) } else { None }).unwrap_or_default(),
        },
        _ => String::new(),
    }
}

/// The Steam account last signed in on this PC: its 32-bit account id, "" when there's none.
pub fn account_id(steam_dir: &Path) -> String {
    let Ok(bytes) = fs::read(steam_dir.join("config").join("loginusers.vdf")) else { return String::new() };
    let parsed = vdf::parse(&String::from_utf8_lossy(&bytes));
    let (mut best, mut best_at) = (None, -1.0);
    for (sid, u) in parsed.get(&["users"]).map(vdf::Vdf::entries).unwrap_or_default() {
        if u.text(&["MostRecent"]) == Some("1") {
            best = Some(sid);
            break;
        }
        let at = Some(u.num(&["Timestamp"])).filter(|t| !t.is_nan()).unwrap_or(0.0);
        if at > best_at {
            (best, best_at) = (Some(sid), at);
        }
    }
    match best.filter(|s| s.len() == 17 && s.bytes().all(|c| c.is_ascii_digit())).and_then(|s| s.parse::<u64>().ok()) {
        Some(id) => (i128::from(id) - 76_561_197_960_265_728).to_string(),
        None => String::new(),
    }
}

/// The progress kept for a game: the account's own, else the only one there is (one person on this PC).
fn user_stats_file(dir: &Path, appid: &str, account: &str) -> Option<PathBuf> {
    let own = dir.join(format!("UserGameStats_{account}_{appid}.bin"));
    if !account.is_empty() && own.exists() {
        return Some(own);
    }
    let end = format!("_{appid}.bin");
    let names: Vec<String> = fs::read_dir(dir)
        .ok()?
        .filter_map(|e| Some(e.ok()?.file_name().to_string_lossy().into_owned()))
        .filter(|n| n.strip_prefix("UserGameStats_").and_then(|rest| rest.strip_suffix(&end)).is_some_and(|id| !id.is_empty() && id.bytes().all(|c| c.is_ascii_digit())))
        .collect();
    (names.len() == 1).then(|| dir.join(&names[0]))
}

/// "1999-11-21 08:00:00" (UTC) for a time in seconds since 1970; None for one JavaScript has no date for.
fn stamp(seconds: f64) -> Option<String> {
    let ms = seconds * 1000.0;
    if !ms.is_finite() || ms.abs() > 8.64e15 {
        return None;
    }
    let total = (ms.trunc() / 1000.0).floor() as i64;
    let (days, rest) = (total.div_euclid(86_400), total.rem_euclid(86_400));
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let y = yoe + era * 400 + i64::from(m <= 2);
    Some(format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", rest / 3600, rest % 3600 / 60, rest % 60))
}

/// A Steam game's achievements and how far you are: `{ id, title, source: "Steam", total, earned, points: 0,
/// pointsEarned: 0, mastered, achievements: [...] }`; None when none are known for it here.
pub fn steam_progress(steam_dir: &Path, appid: &str) -> Option<Value> {
    if appid.is_empty() || !appid.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let dir = steam_dir.join("appcache").join("stats");
    let schema = read_bin(&dir.join(format!("UserGameStatsSchema_{appid}.bin")))?;
    let empty = Kv::Map(Vec::new());
    let lowered = schema.lower();
    let root = lowered.get(appid).filter(|r| r.truthy()).or_else(|| schema.entries().first().map(|e| e.1).filter(|r| r.truthy())).unwrap_or(&empty).lower();
    let stats = root.get("stats").unwrap_or(&empty).lower();
    let user = user_stats_file(&dir, appid, &account_id(steam_dir)).and_then(|f| read_bin(&f));
    let cache = user.as_ref().map(Kv::lower).and_then(|u| u.get("cache").filter(|c| c.truthy()).cloned()).unwrap_or(empty.clone());
    let mut list: Vec<(f64, Value)> = Vec::new();
    for (sid, st) in stats.entries() {
        let s = st.lower();
        let Some(bits @ Kv::Map(_)) = s.get("bits") else { continue };
        let mine = cache.get(sid).unwrap_or(&empty).lower();
        // (JavaScript's `>>> 0`)
        let data = mine.get("data").map(Kv::number).filter(|d| d.is_finite()).map_or(0, |d| d.trunc().rem_euclid(4_294_967_296.0) as u32);
        let times = mine.get("achievementtimes").unwrap_or(&empty).lower();
        for (bid, bt) in bits.entries() {
            let b = bt.lower();
            let d = b.get("display").unwrap_or(&empty).lower();
            let bit = b.get("bit").map_or_else(|| js::number(Some(&json!(bid))), Kv::number);
            let earned = (0.0..32.0).contains(&bit) && data & (1u32 << (bit as u32)) != 0;
            let at = times.get(&js::string(Some(&js::num(bit)))).map(Kv::number).filter(|t| !t.is_nan()).unwrap_or(0.0);
            let icon = [d.get("icon_gray").filter(|_| !earned), d.get("icon")].into_iter().flatten().find(|i| i.truthy()).map(Kv::string);
            let title = Some(text(d.get("name"))).filter(|t| !t.is_empty()).map(Value::String).or_else(|| b.get("name").filter(|n| n.truthy()).map(Kv::to_json)).unwrap_or(json!("Achievement"));
            let hidden = d.get("hidden").is_some_and(|h| h.string() == "1");
            list.push((
                js::number(Some(&json!(sid))) * 64.0 + bit,
                json!({
                    "id": format!("{sid}.{}", js::string(Some(&js::num(bit)))),
                    "title": title,
                    "desc": if hidden && !earned { "Hidden achievement".to_string() } else { text(d.get("desc")) },
                    "points": 0,
                    "badge": icon.map(|i| format!("https://cdn.akamai.steamstatic.com/steamcommunity/public/images/apps/{appid}/{i}")),
                    "earned": earned,
                    "hardcore": false,
                    "date": Some(at).filter(|t| earned && *t != 0.0).and_then(stamp),
                    "rarity": null,
                }),
            ));
        }
    }
    if list.is_empty() {
        return None;
    }
    list.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let achievements: Vec<Value> = list.into_iter().map(|a| a.1).collect();
    let earned = achievements.iter().filter(|a| a["earned"] == true).count();
    Some(json!({ "id": appid, "title": text(root.get("gamename")), "source": "Steam", "total": achievements.len(), "earned": earned, "points": 0, "pointsEarned": 0,
        "mastered": if earned == achievements.len() { "completed" } else { "" }, "achievements": achievements }))
}
