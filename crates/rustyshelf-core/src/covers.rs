// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/covers.js, as RustyShelf has it. LB_PC is from ZakAuMiel/boxshelf-source
// (https://github.com/ZakAuMiel/boxshelf-source, GPL-3.0), Zak's fork of Boxshelf: its main/covers.js.
//! Finding a game's box art: which entry of libretro's thumbnail list or of the LaunchBox Games Database is this
//! game, and which of its pictures to take. A title is compared after `norm` (case, accents, punctuation, articles
//! and Roman numerals off); a region chosen in Settings puts that region's scans first.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

use crate::art::art_base;
use crate::js;
use crate::lbindex::{norm, Game as LbGame, GameImage};
use crate::titles::clean_title;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Sources {
    libretro: HashMap<String, String>,
    launchbox: HashMap<String, String>,
    pc_box_plats: Vec<(String, String)>,
}

/// main/covers.js LIBRETRO, LAUNCHBOX and PC_BOX_PLATS.
static SOURCES: LazyLock<Sources> = LazyLock::new(|| serde_json::from_str(include_str!("../data/art-sources.json")).expect("art-sources.json"));

/// A console's folder in libretro's thumbnail library.
pub fn libretro_system(console_id: &str) -> Option<&'static str> {
    SOURCES.libretro.get(console_id).map(String::as_str)
}

/// A console's platform name in the LaunchBox Games Database.
pub fn launchbox_platform(console_id: &str) -> Option<&'static str> {
    SOURCES.launchbox.get(console_id).map(String::as_str)
}

/// The platforms a PC game's console box is looked up in, newest first: (its short name, LaunchBox's name).
pub fn pc_box_platforms() -> &'static [(String, String)] {
    &SOURCES.pc_box_plats
}

/// The name a console game's art is looked up under when it isn't its title (Details → Art match: a ROM hack, a
/// translation, a mod named after its serial).
pub fn art_name<'a>(cfg: &'a Value, id: &str) -> Option<&'a str> {
    cfg["games"][id]["artName"].as_str().map(str::trim).filter(|n| !n.is_empty())
}

/// LaunchBox's platform of PC games.
pub const LB_PC: &str = "Windows";

/// Where the box images are served from.
pub const LB_IMG: &str = "https://images.launchbox-app.com/";

/// A LaunchBox picture's address, from its index file name.
pub fn lb_url(file: &str) -> String {
    format!("{LB_IMG}{}", js::encode_uri(file))
}

/// libretro's thumbnail server, folders by libretro's console name.
pub const LIBRETRO_THUMBS: &str = "https://thumbnails.libretro.com/";

/// A box art region chosen in Settings: how each source names it.
pub struct ArtRegion {
    /// LaunchBox's region of a picture
    lb: Regex,
    /// the region tag in a libretro name
    lr: Regex,
    /// GameTDB's language folders
    pub tdb: &'static [&'static str],
}

static ART_REGIONS: LazyLock<[(&str, ArtRegion); 3]> = LazyLock::new(|| {
    let tag = |words: &str| Regex::new(&format!(r"(?i)\((?:[^)]*[ ,])?({words})(?:[ ,][^)]*)?\)")).unwrap();
    let name = |words: &str| Regex::new(&format!("^(?i:{words})$")).unwrap();
    [
        ("usa", ArtRegion { lb: name("north america|united states|usa|canada"), lr: tag(r"USA|U\.S\.A\.|Canada|North America"), tdb: &["US"] }),
        (
            "eur",
            ArtRegion {
                lb: name("europe|pal|united kingdom|uk|germany|france|spain|italy|netherlands|sweden|australia|scandinavia|oceania"),
                lr: tag("Europe|UK|PAL|Germany|France|Spain|Italy|Netherlands|Sweden|Australia|Scandinavia"),
                tdb: &["EN", "FR", "DE", "ES", "IT", "NL", "AU"],
            },
        ),
        ("jpn", ArtRegion { lb: name("japan"), lr: tag("Japan"), tdb: &["JA"] }),
    ]
});

/// "usa", "eur" or "jpn" (Settings → Box art region); None for "auto" or anything else.
pub fn art_region(region: &str) -> Option<&'static ArtRegion> {
    ART_REGIONS.iter().find(|(id, _)| *id == region).map(|(_, r)| r)
}

/// The names a title is looked up under: the title, then the title with an edition, bundle or DLC note off its end
/// ("… – Nintendo Switch 2 Edition", "Definitive Edition", "+ DLC", "GOTY"). Subtitles are never cut.
pub fn lookup_names(title: &str) -> Vec<String> {
    static EDITION: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\s*(?:[-–—:+&]\s*)?(?-u:\b)(?:nintendo switch 2 edition|switch 2 edition|ns2 edition|definitive edition|complete edition|deluxe edition|game of the year(?: edition)?|goty(?: edition)?|dlc|all dlc|with dlc)(?-u:\b).*$")
            .unwrap()
    });
    let mut out: Vec<String> = Vec::new();
    let mut add = |t: &str| {
        let t = js::trim(t.trim_end_matches(|c: char| c.is_whitespace() || c == '\u{feff}' || "-–—:+&".contains(c)));
        if !t.is_empty() && !out.iter().any(|x| norm(x) == norm(t)) {
            out.push(t.to_string());
        }
    };
    add(title);
    let mut t = title.to_string();
    for _ in 0..3 {
        let u = EDITION.replace(&t, "").into_owned();
        if u == t || js::trim(&u).is_empty() {
            break;
        }
        t = u;
        add(&t);
    }
    out
}

/// The best entry for a title among names already normalised: exact, then the close calls, else None. `art(i)` is
/// how much art entry `i` has (a tie-breaker, and an entry without pictures only wins when nothing else is close).
/// `exact_only`: no close calls.
pub fn match_title(names: &[&str], title: &str, art: &dyn Fn(usize) -> usize, exact_only: bool) -> Option<usize> {
    static GENERIC: LazyLock<Regex> = LazyLock::new(|| Regex::new("^(?i:(dvd|disc|game|image|iso|default)|xbox 360 game [0-9a-f]{8})$").unwrap());
    // brand prefixes that dumps carry and the database usually doesn't ("Disney-Pixar Finding Nemo")
    static BRAND: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(disney s|disneys|disney pixar|disney|pixar|nickelodeon|nick|mtv|cartoon network|dreamworks|activision|ea sports|ea|sega|capcom|konami|ubisoft|thq|namco)\s+").unwrap());
    static AND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?-u:\b)and(?-u:\b)").unwrap());
    static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
    static NUMBER_IN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?-u:\b)([0-9]+|[ivx]+)(?-u:\b)").unwrap());
    static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new("^([0-9]+|[ivx]+)$").unwrap());
    // a generic file name matches nothing honestly
    if names.is_empty() || title.is_empty() || GENERIC.is_match(title) {
        return None;
    }
    let want = norm(title);
    if want.is_empty() {
        return None;
    }
    let tight = want.replace(' ', "");
    let richest = |keep: &dyn Fn(&str) -> bool| -> Option<usize> {
        let mut best: Option<usize> = None;
        for i in (0..names.len()).filter(|i| keep(names[*i])) {
            if best.is_none_or(|b| art(i) > art(b)) {
                best = Some(i);
            }
        }
        best
    };
    let exact = richest(&|n| n == want).or_else(|| richest(&|n| n.replace(' ', "") == tight));
    if exact.is_some() || exact_only {
        return exact;
    }
    let unbrand = |x: &str| {
        let mut y = x.to_string();
        for _ in 0..2 {
            y = BRAND.replace(&y, "").into_owned();
        }
        y
    };
    let want2 = unbrand(&want);
    if want2 != want {
        if let Some(e) = richest(&|n| n == want2 || unbrand(n) == want2) {
            return Some(e);
        }
    }
    // "A and B" vs "A / B"
    let no_and = |x: &str| js::trim(&SPACES.replace_all(&AND.replace_all(x, " "), " ")).to_string();
    let plain = no_and(&want);
    if plain != want {
        if let Some(e) = richest(&|n| no_and(n) == plain) {
            return Some(e);
        }
    }
    // close calls: one name is the other plus a few words, or the same words in another order
    let words: Vec<&str> = want2.split(' ').filter(|w| !w.is_empty()).collect();
    let set: HashSet<&str> = words.iter().copied().collect();
    let (mut best, mut best_score) = (None, 0.0f64);
    for (i, n) in names.iter().enumerate() {
        let mut score = 0.0;
        if n.starts_with(&format!("{want2} ")) || want2.starts_with(&format!("{n} ")) {
            let extra = if n.len() > want2.len() { &n[want2.len()..] } else { &want2[n.len()..] };
            // "Pikmin" must not stand in for "Pikmin 2"
            if !NUMBER_IN.is_match(extra) {
                score = 0.9 - (n.len() as f64 - want2.len() as f64).abs() / 200.0;
            }
        } else {
            let theirs: Vec<&str> = n.split(' ').filter(|w| !w.is_empty()).collect();
            if !theirs.is_empty() {
                let hit = theirs.iter().filter(|w| set.contains(*w)).count();
                let all: HashSet<&str> = set.iter().chain(theirs.iter()).copied().collect();
                let j = hit as f64 / all.len() as f64;
                if j >= 0.75 {
                    score = j;
                } else if set.len() >= 3 {
                    // ours is theirs minus a few filler words, in the same order
                    let extra: Vec<&&str> = theirs.iter().filter(|w| !set.contains(*w)).collect();
                    let mut k = 0;
                    for w in &theirs {
                        if k < words.len() && *w == words[k] {
                            k += 1;
                        }
                    }
                    if k == words.len() && extra.len() <= 3 && !extra.iter().any(|w| NUMBER.is_match(w)) {
                        score = 0.8 - extra.len() as f64 * 0.03;
                    }
                }
            }
        }
        if score != 0.0 && art(i) == 0 {
            score -= 0.05;
        }
        if score > best_score {
            best_score = score;
            best = Some(i);
        }
    }
    best
}

/// The best entry for a title or, when nothing is close, the first of its other names that's listed exactly.
pub fn match_name(names: &[&str], title: &str, art: &dyn Fn(usize) -> usize) -> Option<usize> {
    let looked = lookup_names(title);
    let mut hit = match_title(names, looked.first().map_or("", String::as_str), art, false);
    for other in looked.iter().skip(1) {
        if hit.is_some() {
            break;
        }
        hit = match_title(names, other, art, true);
    }
    hit
}

/// A game in one platform's list of the LaunchBox index. Duplicates share a name (a bare user-added entry next to the
/// real one): the one with the most box art wins.
pub fn launchbox_match<'a>(list: &'a [LbGame], images: &HashMap<String, Vec<GameImage>>, title: &str) -> Option<&'a LbGame> {
    let names: Vec<&str> = list.iter().map(|g| g.n.as_str()).collect();
    let art = |i: usize| images.get(&list[i].id).map_or(0, |l| l.iter().filter(|im| im.r#type.starts_with("Box - ") || im.r#type == "Disc").count());
    match_name(&names, title, &art).map(|i| &list[i])
}

const REGION_PREF: &[&str] = &["North America", "United States", "World", "", "Canada", "Europe", "United Kingdom", "Japan"];
/// North American boxes print English, French and Spanish side by side: English-only regions come first for a back.
const BACK_PREF: &[&str] = &["United Kingdom", "Australia", "North America", "United States", "World", "", "Canada", "Europe", "Japan"];

/// A game's pictures of one type, the best region first: the usual order, then the region chosen in Settings.
pub fn pick_images<'a>(imgs: &'a [GameImage], kind: &str, region: &str) -> Vec<&'a GameImage> {
    let pref = if kind.contains("Back") { BACK_PREF } else { REGION_PREF };
    let mut c: Vec<&GameImage> = imgs.iter().filter(|i| i.r#type == kind && !i.file.is_empty()).collect();
    c.sort_by_key(|i| pref.iter().position(|r| *r == i.region).unwrap_or(99));
    if let Some(want) = art_region(region) {
        let (mut first, rest): (Vec<&GameImage>, Vec<&GameImage>) = c.into_iter().partition(|i| want.lb.is_match(js::trim(&i.region)));
        first.extend(rest);
        return first;
    }
    c
}

pub fn libretro_safe(s: &str) -> String {
    s.chars().map(|c| if "&*/:`<>?\\|\"".contains(c) { '_' } else { c }).collect()
}

/// libretro's list of a console's box art names, with each name's clean title worked out once.
pub struct LibretroNames {
    names: Vec<String>,
    set: HashSet<String>,
    by_title: HashMap<String, Vec<usize>>,
}

impl LibretroNames {
    pub fn new(names: Vec<String>) -> LibretroNames {
        let mut by_title: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, n) in names.iter().enumerate() {
            by_title.entry(norm(&clean_title(&format!("{n}.png"), true).title)).or_default().push(i);
        }
        LibretroNames { set: names.iter().cloned().collect(), names, by_title }
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// The names of the first of a title's lookup names that's listed.
    fn hits(&self, title: &str) -> Vec<&str> {
        lookup_names(title).iter().find_map(|t| self.by_title.get(&norm(t))).map(|l| l.iter().map(|i| self.names[*i].as_str()).collect()).unwrap_or_default()
    }

    /// The game's own file name, as it is and as libretro writes one.
    fn own(&self, file: &Path) -> Vec<String> {
        let base = art_base(file);
        [base.clone(), libretro_safe(&base)].into_iter().filter(|c| self.set.contains(c)).collect()
    }

    /// The name of this game's cover: its file's own name, else the same clean title in the region Settings asks
    /// for, the game's own region, the USA, or whichever comes first.
    pub fn find(&self, file: &Path, title: &str, game_region: &str, region: &str) -> Option<String> {
        if self.names.is_empty() {
            return None;
        }
        let (pref, own) = (art_region(region), self.own(file));
        if let Some(c) = own.iter().find(|c| pref.is_none_or(|p| p.lr.is_match(c))) {
            return Some(c.clone());
        }
        let hits = self.hits(title);
        if hits.is_empty() {
            return own.first().cloned();
        }
        if let Some(p) = pref {
            // no scan for that region: the file's own
            if let Some(h) = hits.iter().find(|h| p.lr.is_match(h)).map(|h| h.to_string()).or_else(|| own.first().cloned()) {
                return Some(h);
            }
        }
        let reg = format!("({}", if game_region.is_empty() { "usa".to_string() } else { game_region.to_lowercase() });
        let pick = hits.iter().find(|h| h.to_lowercase().contains(&reg)).or_else(|| hits.iter().find(|h| h.to_lowercase().contains("(usa"))).or(hits.first());
        pick.map(|h| h.to_string())
    }

    /// Every name for this game (all regions and editions), the file's own name first.
    pub fn all(&self, file: &Path, title: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for n in self.own(file).into_iter().chain(self.hits(title).into_iter().map(String::from)) {
            if !out.contains(&n) {
                out.push(n);
            }
        }
        out
    }
}

/// A libretro cover's address.
pub fn libretro_url(console_id: &str, name: &str) -> Option<String> {
    let sys = libretro_system(console_id)?;
    Some(format!("{LIBRETRO_THUMBS}{}/Named_Boxarts/{}", js::encode_uri_component(sys), js::encode_uri_component(&format!("{name}.png"))))
}

/// A release's edition from its own words: "reprint" (Player's Choice, Greatest Hits …), "special" (collector's,
/// limited …) or "original", and the word that said so.
pub fn edition(text: &str) -> (&'static str, String) {
    static REPRINT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(?-u:\b)(player'?s choice|greatest hits|platinum|nintendo selects|essentials|classics|best price|hits)(?-u:\b)").unwrap());
    static SPECIAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(?-u:\b)(collector'?s|limited|special|premium|deluxe|steelbook|signature)(?-u:\b)").unwrap());
    match (REPRINT.find(text), SPECIAL.find(text)) {
        (Some(m), _) => ("reprint", m.as_str().to_string()),
        (None, Some(m)) => ("special", m.as_str().to_string()),
        _ => ("original", String::new()),
    }
}

/// LaunchBox's region of a picture as "usa", "eur", "jpn", "world" or "".
pub fn lb_region(s: &str) -> &'static str {
    let t = js::trim(s);
    ART_REGIONS.iter().find(|(_, r)| r.lb.is_match(t)).map_or(if t.eq_ignore_ascii_case("world") { "world" } else { "" }, |(id, _)| id)
}

/// The region tag of a libretro name as "usa", "eur", "jpn", "world" or "".
pub fn lr_region(name: &str) -> &'static str {
    static WORLD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\((?:[^)]*[ ,])?World(?:[ ,][^)]*)?\)").unwrap());
    ART_REGIONS.iter().find(|(_, r)| r.lr.is_match(name)).map_or(if WORLD.is_match(name) { "world" } else { "" }, |(id, _)| id)
}

/// A GameTDB language folder as a region.
pub fn tdb_region(r: &str) -> &'static str {
    match r {
        "US" => "usa",
        "JA" => "jpn",
        "KO" => "",
        _ => "eur",
    }
}

/// The 6-character game id at the start of a GameCube / Wii disc: plain images keep it at 0, RVZ / WIA a copy of the
/// header at 0x58, CISO stores the disc from 0x8000, WBFS from 0x200. GCZ is compressed throughout: no id.
pub fn disc_id(file: &Path) -> Option<String> {
    use std::io::Read;
    let mut h = Vec::with_capacity(0x8010);
    std::fs::File::open(file).ok()?.take(0x8010).read_to_end(&mut h).ok()?;
    let at = match h.get(..4)? {
        b"RVZ\x01" | b"WIA\x01" => 0x58,
        b"CISO" => 0x8000,
        b"WBFS" => 0x200,
        _ => 0,
    };
    let id = h.get(at..at + 6)?;
    (id.iter().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()) && b"GDRS".contains(&id[0])).then(|| String::from_utf8_lossy(id).into_owned())
}

/// GameTDB's language folders to try for a disc, by the region letter of its id: a region chosen in Settings goes
/// first only where the disc was sold there too.
pub fn tdb_regions(id: &str, region: &str) -> Vec<&'static str> {
    let own = tdb_own(id);
    let wanted = art_region(region).map_or(&[][..], |p| p.tdb);
    let mut out: Vec<&'static str> = Vec::new();
    for r in wanted.iter().filter(|r| own.contains(r)).chain(own).chain(&["US", "EN"]) {
        if !out.contains(r) {
            out.push(r);
        }
    }
    out
}

/// The language folders GameTDB keeps a disc's own scans in, by the region letter of its id.
pub fn tdb_own(id: &str) -> &'static [&'static str] {
    match id.as_bytes().get(3) {
        Some(b'E' | b'N') => &["US"],
        Some(b'J') => &["JA"],
        Some(b'K') => &["KO"],
        Some(b'P') => &["EN", "FR", "DE", "ES", "IT", "NL"],
        Some(b'D') => &["DE", "EN"],
        Some(b'F') => &["FR", "EN"],
        Some(b'S') => &["ES", "EN"],
        Some(b'I') => &["IT", "EN"],
        Some(b'H') => &["NL", "EN"],
        Some(b'U') => &["EN", "AU"],
        Some(b'X' | b'Y' | b'Z') => &["EN", "US"],
        _ => &["US", "EN"],
    }
}

/// The pictures `replaced\` holds for one kind of one game, newest first: (file name, its stamp). Older pictures
/// move there as `<base><suffix> (<stamp>).<ext>` when a new one takes their place.
pub fn replaced_for(names: &[String], base: &str, suffix: &str) -> Vec<(String, String)> {
    static STAMP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([0-9]{4}(?:-[0-9]+){6}(?:-[0-9]+)?)\)\.(?i:png|jpe?g|webp)$").unwrap());
    let pre = format!("{base}{suffix} (").to_lowercase();
    let mut out: Vec<(String, String)> = names
        .iter()
        .filter_map(|n| {
            let l = n.to_lowercase();
            let rest = n.get(l.strip_prefix(&pre).map(|_| pre.len())?..)?;
            Some((n.clone(), STAMP.captures(rest)?[1].to_string()))
        })
        .collect();
    // "-2" (the same millisecond) sorts after the plain one
    let key = |stamp: &str| stamp.split('-').map(|x| format!("{x:0>4}")).collect::<Vec<_>>().join("-");
    out.sort_by_key(|a| std::cmp::Reverse(key(&a.1)));
    out
}
