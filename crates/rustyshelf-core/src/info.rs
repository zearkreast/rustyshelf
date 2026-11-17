// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from the
// game info of Boxshelf's main/covers.js and main/steamart.js, as RustyShelf has them.
//! What's known about a game beyond its file: release, developer, publisher, genres, players, rating, a description
//! and screenshots. A console game's comes from the LaunchBox index on disk (never fetched for this), a Steam game's
//! from Steam's store info (`data/steam-info.json`). Answers are kept in `boxart-cache/gameinfo-cache.json`, tied to
//! the index file's date, so a normal start never reads the index: only new or renamed games do.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::boxart::{self, Index};
use crate::covers;
use crate::js;
use crate::lbindex::{norm, Game as LbGame, ShardFile};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Info {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overview: Option<String>,
    /// the release date as the source writes it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev: Option<String>,
    #[serde(default, rename = "pub", skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// separated by ";"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genres: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub esrb: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub players: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coop: Option<String>,
    /// LaunchBox's community score, out of 5
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
    /// Steam's Metacritic score, out of 100
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metacritic: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wiki: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Steam's long description, its paragraphs on lines of their own
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
    #[serde(default)]
    pub shots: Vec<String>,
    #[serde(default)]
    pub bg: Option<String>,
    #[serde(default)]
    pub logo: Option<String>,
    /// LaunchBox's front cover
    #[serde(default)]
    pub front: Option<String>,
    /// Steam's store header
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
}

impl Info {
    /// What Details says about the game: Steam's long description when it's the longer, on one line (its headings
    /// run into the next sentence across a line break, so a break gets a full stop), else the short one.
    pub fn description(&self) -> Option<String> {
        static BREAK: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"([^.!?:;,\s])[ \t]*\n+\s*").unwrap());
        match (&self.about, &self.overview) {
            (Some(a), o) if a.chars().count() > o.as_deref().map_or(0, |o| o.chars().count()) => Some(BREAK.replace_all(a, "$1. ").split_whitespace().collect::<Vec<_>>().join(" ")),
            (_, o) => o.clone(),
        }
    }

    /// The release year, from the date or the year alone.
    pub fn year(&self) -> Option<u32> {
        let y: String = self.release.as_deref().or(self.year.as_deref())?.chars().take(4).collect();
        (y.len() == 4 && y.bytes().all(|b| b.is_ascii_digit())).then(|| y.parse().ok()).flatten()
    }

    /// The first genre.
    pub fn genre(&self) -> Option<&str> {
        self.genres.as_deref()?.split(';').next().map(js::trim).filter(|g| !g.is_empty())
    }

    pub fn genres(&self) -> Vec<&str> {
        self.genres.as_deref().unwrap_or("").split(';').map(js::trim).filter(|g| !g.is_empty()).collect()
    }

    /// A score out of 5: LaunchBox's, else Metacritic's brought to that scale.
    pub fn score(&self) -> Option<f64> {
        self.rating.filter(|r| *r != 0.0).or(self.metacritic.filter(|m| *m != 0.0).map(|m| m / 20.0))
    }

    /// The most players the game takes, from words like "1-4".
    pub fn max_players(&self) -> Option<u32> {
        let p = self.players.as_deref()?;
        p.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse().ok()).max()
    }

    pub fn is_coop(&self) -> bool {
        self.coop.as_deref().map(js::trim).is_some_and(|c| !c.is_empty() && !matches!(c.to_lowercase().as_str(), "no" | "false" | "0"))
    }
}

/// Answers remembered under another matching version are asked again (main/covers.js MATCH_V, keep in sync).
const MATCH_V: u64 = 2;

fn image_urls(shard: &ShardFile, id: &str, wanted: &dyn Fn(&str) -> bool) -> Vec<String> {
    shard.images.get(id).into_iter().flatten().filter(|x| wanted(&x.r#type)).map(|x| format!("{}{}", covers::LB_IMG, x.file)).collect()
}

/// One game's info from its platform's part of the index: by its exact name, the same without spaces, else the
/// close-call matching the art uses. Duplicates share a name: the entry with pictures wins.
fn from_shard(shard: &ShardFile, title: &str) -> Option<Info> {
    let art = |g: &LbGame| shard.images.get(&g.id).map_or(0, Vec::len);
    let n = norm(title);
    let named = |want: &str| {
        let mut best: Option<&LbGame> = None;
        for g in shard.games.iter().filter(|g| g.n == want || g.n.replace(' ', "") == want) {
            if best.is_none_or(|b| art(g) > art(b)) {
                best = Some(g);
            }
        }
        best
    };
    let g = named(&n).or_else(|| named(&n.replace(' ', ""))).or_else(|| covers::launchbox_match(&shard.games, &shard.images, title))?;
    let shots: Vec<String> = image_urls(shard, &g.id, &|t| t.starts_with("Screenshot - Gameplay"))
        .into_iter()
        .chain(image_urls(shard, &g.id, &|t| t.starts_with("Screenshot - Game Title") || t.starts_with("Screenshot - Game Select")))
        .take(8)
        .collect();
    // to one decimal; a rating that isn't a number is none
    let rating = g.rating.as_deref().filter(|r| !r.is_empty()).map(|r| js::number(Some(&Value::from(r)))).filter(|r| r.is_finite()).map(|r| js::round(r * 10.0) / 10.0);
    Some(Info {
        about: None,
        overview: g.overview.clone(),
        release: g.release.clone(),
        year: g.year.clone(),
        dev: g.dev.clone(),
        publisher: g.pub_.clone(),
        genres: g.genres.clone(),
        esrb: g.esrb.clone(),
        players: g.players.clone(),
        coop: g.coop.clone(),
        rating,
        metacritic: None,
        wiki: g.wiki.clone(),
        video: g.video.clone(),
        name: Some(g.name.clone()),
        shots,
        bg: image_urls(shard, &g.id, &|t| t.starts_with("Fanart - Background")).into_iter().next(),
        logo: image_urls(shard, &g.id, &|t| t == "Clear Logo").into_iter().next(),
        front: image_urls(shard, &g.id, &|t| t == "Box - Front").into_iter().next(),
        header: None,
    })
}

/// Console games' info, looked up in the LaunchBox index and remembered.
pub struct InfoCache {
    file: PathBuf,
    stamp: u64,
    map: Map<String, Value>,
    index: Option<Index>,
    dirty: bool,
}

impl InfoCache {
    /// `cache_dir`: `data/boxart-cache`.
    pub fn load(cache_dir: &Path) -> InfoCache {
        let file = cache_dir.join("gameinfo-cache.json");
        let stamp = boxart::index_stamp(cache_dir).unwrap_or(0);
        let saved = crate::files::read_json(&file);
        let map = match saved {
            Some(mut j) if j["stamp"].as_u64() == Some(stamp) && j["v"].as_u64() == Some(MATCH_V) => j["map"].as_object_mut().map(std::mem::take).unwrap_or_default(),
            _ => Map::new(),
        };
        InfoCache { file, stamp, map, index: boxart::ready_index(cache_dir), dirty: false }
    }

    /// A console game's info by the title its art is looked up under; None when the database doesn't know it, or
    /// there's no index yet (which isn't remembered as "nothing found").
    pub fn get(&mut self, console_id: &str, title: &str) -> Option<Info> {
        let key = format!("{console_id}|{title}");
        if let Some(v) = self.map.get(&key) {
            return serde_json::from_value(v.clone()).ok();
        }
        let plat = covers::launchbox_platform(console_id)?;
        let found = from_shard(self.index.as_mut()?.platform(plat)?, title);
        self.map.insert(key, found.as_ref().map_or(Value::Null, |i| serde_json::to_value(i).unwrap_or(Value::Null)));
        self.dirty = true;
        found
    }

    /// Keeps the answers found since it was loaded.
    pub fn flush(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let _ = std::fs::write(&self.file, json!({ "stamp": self.stamp, "v": MATCH_V, "map": self.map }).to_string());
    }
}

/// Steam's store info by app id (`data/steam-info.json`), for the apps the store answered for.
pub fn steam_infos(data_dir: &Path) -> HashMap<String, Info> {
    let Some(Value::Object(all)) = crate::files::read_json(&data_dir.join("steam-info.json")) else { return HashMap::new() };
    all.into_iter().filter(|(_, d)| js::truthy(d.get("ok"))).filter_map(|(id, d)| Some((id, serde_json::from_value(d).ok()?))).collect()
}

/// A screenshot's address (LaunchBox's, Steam's) as a file on this PC: fetched once into `data/shot-cache` and read
/// from there after. None when it can't be fetched or isn't a picture.
pub fn shot_file(data_dir: &Path, url: &str) -> Option<std::path::PathBuf> {
    use sha1::Digest;
    if !url.starts_with("https://") {
        return None;
    }
    let name: String = sha1::Sha1::digest(url.as_bytes()).iter().take(12).map(|b| format!("{b:02x}")).collect();
    let dir = data_dir.join("shot-cache");
    let file = dir.join(format!("{name}.img"));
    if crate::paths::is_file(&file) {
        return Some(file);
    }
    let bytes = crate::net::get_bytes(url, std::time::Duration::from_secs(20), 15_000_000).ok().flatten().filter(|b| crate::steamart::is_image(b))?;
    crate::files::write_atomic(&file, &bytes).ok()?;
    Some(file)
}
