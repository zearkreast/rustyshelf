// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// RustyShelf's shared/museum-layout.js and shared/arcade-plan.js (TrapEmAll's museum rooms and walkable arcade).
//! Planning the museum, with nothing drawn: its rooms by maker or genre (their walls, shelf segments and where every
//! game lives), and the arcade wing's floor plan (genre rooms off a corridor, cabinet slots, walls to walk into).
//! Every game is placed exactly once; overflow makes another room, never a cut. Keep in sync with those two files.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::f64::consts::PI;

use crate::collate;

/// A room's games at most, its shelf segments at most, a segment's cases, segments on one wall, an arcade room's
/// cabinets, and the reading & media room's displays.
pub const ROOM_CAP: usize = 1200;
pub const SEGMENT_MAX: usize = 40;
pub const SEGMENTS_MAX: usize = 30;
pub const WALL_MAX: usize = 10;
pub const ARCADE_CAP: usize = 300;
pub const MEDIA_CAP: usize = 2000;

const MANUFACTURERS: [(&str, &[&str]); 5] = [
    ("nintendo", &["gc", "wii", "wiiu", "sw", "sw2", "nes", "snes", "n64", "gb", "gbc", "gba", "nds", "n3ds"]),
    ("sony", &["psx", "ps2", "ps3", "ps4", "ps5", "psp", "psvita"]),
    ("sega", &["megadrive", "mastersystem", "gamegear", "dreamcast", "saturn", "segacd", "sega32x"]),
    ("atari", &["atari2600", "atarilynx"]),
    ("microsoft", &["xbox", "x360", "xboxone"]),
];
const MAKER_ORDER: [&str; 6] = ["nintendo", "sony", "sega", "microsoft", "atari", "other"];
const COMPUTERS: [&str; 3] = ["dos", "pc", "custom"];

fn maker_title(mk: &str) -> &'static str {
    match mk {
        "nintendo" => "Nintendo",
        "sony" => "Sony",
        "sega" => "Sega",
        "microsoft" => "Microsoft",
        "atari" => "Atari",
        _ => "Other",
    }
}

/// An id another adapter may use for a known system.
pub fn system_of(id: &str) -> String {
    match id {
        "xone" | "xsx" => "xboxone".into(),
        s => s.into(),
    }
}

/// A system's maker room; one it doesn't know goes to Other.
pub fn maker_of(id: &str) -> &'static str {
    let s = system_of(id);
    MANUFACTURERS.iter().find(|(_, ids)| ids.contains(&s.as_str())).map_or("other", |(mk, _)| mk)
}

pub fn is_computer(id: &str) -> bool {
    COMPUTERS.contains(&system_of(id).as_str())
}

pub fn is_arcade(id: &str) -> bool {
    system_of(id) == "arcade"
}

/// A genre as a room's key: lower case, spaces collapsed; "" for none.
pub fn genre_key(primary: &str) -> String {
    primary.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn genre_label(primary: &str) -> String {
    let s = primary.trim();
    if s.is_empty() { "Unknown".into() } else { s.into() }
}

/// A game as the plans see it: its id, its system, its first genre ("" for none).
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: String,
    pub system: String,
    pub genre: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub id: String,
    pub system: String,
    pub game_ids: Vec<String>,
    /// "back", "left" or "right"
    pub wall: &'static str,
    /// its place on the wall
    pub row: usize,
    /// "shelves" or "cabinets"
    pub kind: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Room {
    pub id: String,
    pub bucket: String,
    pub group: String,
    /// "shelves", "arcade", "computers" or "media"
    pub kind: &'static str,
    pub title: String,
    pub part: usize,
    pub parts: usize,
    pub game_ids: Vec<String>,
    pub segments: Vec<Segment>,
    pub theme: String,
}

/// Where a game lives: its room, its segment there and its place in the segment.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub room: String,
    pub seg: usize,
    pub i: usize,
}

#[derive(Debug, Default)]
pub struct Plan {
    pub rooms: Vec<Room>,
    pub by_game: HashMap<String, Place>,
}

struct Bucket {
    key: String,
    title: String,
    kind: &'static str,
    theme: String,
    games: Vec<Entry>,
}

/// The museum's rooms, by maker ("manufacturer") or by genre ("genre"), from the games in their shelves' order.
pub fn plan_museum(entries: &[Entry], grouping: &str) -> Plan {
    let by_genre = grouping == "genre";
    let mut seen = HashSet::new();
    let mut buckets: Vec<Bucket> = Vec::new();
    let mut put = |key: String, title: String, kind: &'static str, theme: String, e: &Entry| {
        match buckets.iter_mut().find(|b| b.key == key) {
            Some(b) => b.games.push(e.clone()),
            None => buckets.push(Bucket { key, title, kind, theme, games: vec![e.clone()] }),
        }
    };
    for e in entries {
        if !seen.insert(e.id.clone()) {
            continue;
        }
        let sys = system_of(&e.system);
        let gk = genre_key(&e.genre);
        if by_genre {
            if is_arcade(&sys) {
                put("genre:arcade".into(), "Arcade".into(), "arcade", "genre:arcade".into(), e);
            } else if is_computer(&sys) {
                put("genre:computers".into(), "Computers".into(), "computers", "genre:computers".into(), e);
            } else {
                let gk = if gk.is_empty() { "unknown".to_string() } else { gk };
                put(format!("genre:{gk}"), genre_label(&e.genre), "shelves", format!("genre:{gk}"), e);
            }
        } else if is_arcade(&sys) {
            let k = if gk.is_empty() { "unknown".to_string() } else { gk.clone() };
            let title = if k == "unknown" { "Arcade".into() } else { format!("Arcade · {}", genre_label(&e.genre)) };
            put(format!("arcade:{k}"), title, "arcade", "arcade".into(), e);
        } else if is_computer(&sys) {
            put("computers:0".into(), "Computers".into(), "computers", "computers".into(), e);
        } else {
            let mk = maker_of(&sys);
            put(format!("manufacturer:{mk}"), maker_title(mk).into(), "shelves", mk.into(), e);
        }
    }
    let rank = |b: &Bucket| -> usize {
        if by_genre {
            match b.key.as_str() {
                "genre:arcade" => 0,
                "genre:computers" => 1,
                "genre:unknown" => 3,
                _ => 2,
            }
        } else if b.kind == "arcade" {
            MAKER_ORDER.len()
        } else if b.kind == "computers" {
            MAKER_ORDER.len() + 1
        } else {
            MAKER_ORDER.iter().position(|m| b.key.strip_prefix("manufacturer:") == Some(m)).unwrap_or(usize::MAX)
        }
    };
    // (a stable sort, as Array.prototype.sort is)
    buckets.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| collate::compare(&a.title, &b.title)));
    let mut plan = Plan::default();
    for b in &buckets {
        pack(b, &mut plan);
    }
    plan.rooms.push(Room {
        id: "media:0".into(),
        bucket: String::new(),
        group: "media".into(),
        kind: "media",
        title: "Reading & media".into(),
        part: 0,
        parts: 1,
        game_ids: Vec::new(),
        segments: Vec::new(),
        theme: "media".into(),
    });
    plan
}

/// One bucket's games into rooms: each system's games in segments of their own, and a system kept whole in one room
/// unless it's larger than a room.
fn pack(b: &Bucket, plan: &mut Plan) {
    let group = match b.kind {
        "arcade" => "arcade".to_string(),
        "computers" => "computers".to_string(),
        _ => b.key.clone(),
    };
    let mut by_sys: Vec<(String, Vec<&Entry>)> = Vec::new();
    for e in &b.games {
        let sys = { let s = system_of(&e.system); if s.is_empty() { "other".into() } else { s } };
        match by_sys.iter_mut().find(|(s, _)| *s == sys) {
            Some((_, g)) => g.push(e),
            None => by_sys.push((sys, vec![e])),
        }
    }
    let room_cap = if b.kind == "arcade" { ARCADE_CAP } else { ROOM_CAP };
    let first = plan.rooms.len();
    let (mut games, mut segs, mut part) = (0usize, 0usize, 0usize);
    let new_room = |plan: &mut Plan, part: usize| {
        plan.rooms.push(Room {
            id: format!("{}:{part}", b.key),
            bucket: b.key.clone(),
            group: group.clone(),
            kind: b.kind,
            title: b.title.clone(),
            part,
            parts: 0,
            game_ids: Vec::new(),
            segments: Vec::new(),
            theme: b.theme.clone(),
        });
    };
    new_room(plan, part);
    for (sys, list) in &by_sys {
        let need = list.len().div_ceil(SEGMENT_MAX);
        if games > 0 && (games + list.len() > room_cap || segs + need > SEGMENTS_MAX) {
            part += 1;
            new_room(plan, part);
            (games, segs) = (0, 0);
        }
        for (k, chunk) in list.chunks(SEGMENT_MAX).enumerate() {
            if k > 0 && (segs >= SEGMENTS_MAX || games + chunk.len() > room_cap) {
                part += 1;
                new_room(plan, part);
                (games, segs) = (0, 0);
            }
            let room = plan.rooms.last_mut().expect("a room");
            let seg_i = room.segments.len();
            let mut s = Segment {
                id: format!("{}:s{segs}", room.id),
                system: sys.clone(),
                game_ids: Vec::new(),
                wall: ["back", "left", "right"].get(segs / WALL_MAX).copied().unwrap_or("back"),
                row: segs % WALL_MAX,
                kind: if b.kind == "shelves" { "shelves" } else { "cabinets" },
            };
            for e in chunk {
                s.game_ids.push(e.id.clone());
                room.game_ids.push(e.id.clone());
                plan.by_game.insert(e.id.clone(), Place { room: room.id.clone(), seg: seg_i, i: s.game_ids.len() - 1 });
            }
            room.segments.push(s);
            games += chunk.len();
            segs += 1;
        }
    }
    let n = plan.rooms.len() - first;
    for (k, r) in plan.rooms[first..].iter_mut().enumerate() {
        (r.parts, r.part) = (n, k);
        if n > 1 {
            r.title = format!("{} · {} of {n}", b.title, k + 1);
        }
    }
}

/// The rooms a system's games are in.
pub fn rooms_for_system(plan: &Plan, system: &str) -> Vec<String> {
    let sys = system_of(system);
    plan.rooms.iter().filter(|r| r.segments.iter().any(|s| s.system == sys)).map(|r| r.id.clone()).collect()
}

/// (Haseosama's fork) The games of one of `shelves` museum shelves that get a case: all of them, or for a big
/// library its share of 6,000 cases (60 to 300), favourites first, then the most played, then the rest in their
/// order, kept in that order. `fav` and `plays` are each game's, in the shelf's order. Keep in sync with
/// renderer/app.js musPick.
pub fn pick(fav: &[bool], plays: &[u64], shelves: usize) -> Vec<usize> {
    let cap = (6000 / shelves.max(1)).clamp(60, 300);
    let n = fav.len();
    if n <= cap {
        return (0..n).collect();
    }
    let score = |i: usize| (if fav[i] { 1e9 } else { 0.0 }) + plays.get(i).copied().unwrap_or(0) as f64 * 1e3 - i as f64 / 1e6;
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|a, b| score(*b).total_cmp(&score(*a)));
    order.truncate(cap);
    order.sort_unstable();
    order
}

// ---------- the arcade wing ----------

/// An arcade room's cabinets: the usual, and the least and most a player can choose.
pub const ARCADE_ROOM: usize = 24;
pub const CAP_MIN: usize = 20;
pub const CAP_MAX: usize = 30;
/// Floor sizes in world units (a case is 1 tall, eye level 1.6): the foyer, the corridor's width, a room, the gap
/// between rooms, a cabinet's footprint and the clearance kept around cabinet rows.
pub const FOYER: (f64, f64) = (9.0, 7.0);
pub const CORRIDOR_W: f64 = 4.2;
pub const ROOM_W: f64 = 12.5;
pub const ROOM_D: f64 = 9.5;
const GAP: f64 = 2.4;
pub const CABINET: (f64, f64) = (0.78, 0.86);
pub const CLEAR: f64 = 1.15;
/// a doorway's width
const DOOR: f64 = 1.6;

#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub x: f64,
    pub z: f64,
    pub ry: f64,
    pub kind: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Door {
    pub x: f64,
    pub z: f64,
    pub ry: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArcadeRoom {
    pub id: String,
    pub genre: String,
    pub title: String,
    pub plain: String,
    pub game_ids: Vec<String>,
    pub cap: usize,
    pub x: f64,
    pub z: f64,
    pub w: f64,
    pub d: f64,
    pub door: Door,
    /// "upright", "sitdown" (racing), "cocktail" (puzzle and classics) or "pedestal" (fighting)
    pub cabinet: &'static str,
    pub slots: Vec<Slot>,
}

#[derive(Debug, Default)]
pub struct Arcade {
    pub cap: usize,
    pub rooms: Vec<ArcadeRoom>,
    pub corridor_end: f64,
    /// solid wall segments [x1, z1, x2, z2], the doorways left open
    pub walls: Vec<[f64; 4]>,
    pub by_game: HashMap<String, (String, usize)>,
}

/// A number as JavaScript's toFixed(n) leaves it, read back.
fn fixed(v: f64, n: i32) -> f64 {
    let s = format!("{:.*}", n as usize, v);
    // toFixed rounds the decimal written value half up; format! rounds to even on exact halves of the binary value,
    // which only differs on numbers like 0.125: these are nudged the JavaScript way
    let k = 10f64.powi(n);
    let up = (v * k).round() / k;
    if (v * k).fract().abs() == 0.5 { up } else { s.parse().unwrap_or(up) }
}

/// A genre's key, the name its first game spells it with, and its games with their places in the list.
type Genre<'a> = (String, String, Vec<(usize, &'a Entry)>);

/// The arcade's genre rooms (at most `cap` cabinets each) laid out on a floor: a foyer with the museum door, a
/// corridor east, rooms alternating north and south of it.
pub fn plan_arcade(entries: &[Entry], cap: Option<f64>) -> Arcade {
    let cap = cap.filter(|c| c.is_finite()).map_or(ARCADE_ROOM as f64, f64::round).clamp(CAP_MIN as f64, CAP_MAX as f64) as usize;
    let mut order: Vec<Genre> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let gk = { let k = genre_key(&e.genre); if k.is_empty() { "unknown".to_string() } else { k } };
        match order.iter_mut().find(|(g, _, _)| *g == gk) {
            Some((_, _, l)) => l.push((i, e)),
            None => order.push((gk, genre_label(&e.genre), vec![(i, e)])),
        }
    }
    for (_, _, l) in &mut order {
        l.sort_by(|a, b| collate::compare(&a.1.id, &b.1.id).then(a.0.cmp(&b.0)));
    }
    order.sort_by(|a, b| (a.0 == "unknown").cmp(&(b.0 == "unknown")).then_with(|| collate::compare_exact(&a.0, &b.0)));
    let mut rooms: Vec<ArcadeRoom> = Vec::new();
    let cor_h = CORRIDOR_W / 2.0;
    let mut x = FOYER.0 / 2.0 + ROOM_D / 2.0 + 1.4;
    for (gk, plain, games) in &order {
        let n = games.len().div_ceil(cap);
        for (k, slice) in games.chunks(cap).enumerate() {
            let north = rooms.len().is_multiple_of(2);
            let cz = if north { -(cor_h + ROOM_D / 2.0 + 0.12) } else { cor_h + ROOM_D / 2.0 + 0.12 };
            let plain = plain.clone();
            rooms.push(ArcadeRoom {
                id: format!("arcade:{gk}:{k}"),
                genre: gk.clone(),
                title: if n > 1 { format!("{plain} · {} of {n}", k + 1) } else { plain.clone() },
                plain,
                game_ids: slice.iter().map(|(_, e)| e.id.clone()).collect(),
                cap,
                x,
                z: cz,
                w: ROOM_W,
                d: ROOM_D,
                door: Door { x, z: if north { -cor_h } else { cor_h }, ry: if north { 0.0 } else { PI } },
                cabinet: "upright",
                slots: Vec::new(),
            });
            x += ROOM_D + GAP;
        }
    }
    let corridor_end = x - ROOM_D / 2.0 - 1.0;
    for r in &mut rooms {
        let g = r.genre.as_str();
        let has = |words: &[&str]| words.iter().any(|w| g.contains(w));
        r.cabinet = if has(&["racing", "driving"]) {
            "sitdown"
        } else if has(&["fight", "beat", "brawler"]) {
            "pedestal"
        } else if has(&["puzzle", "classic", "breakout", "arkanoid"]) {
            "cocktail"
        } else {
            "upright"
        };
        let north = r.z < 0.0;
        let z_back = r.z + if north { -(r.d / 2.0 - CLEAR) } else { r.d / 2.0 - CLEAR };
        let back_ry = if north { PI } else { 0.0 };
        let inner = r.w - 2.0 * CLEAR;
        let n = r.game_ids.len();
        let cols = n.div_ceil(2).min((inner / (CABINET.0 + 0.55)).floor() as usize).max(1);
        let along = inner / cols as f64;
        r.slots = (0..n)
            .map(|i| {
                let (row, k) = (i / cols, i % cols);
                let sx = r.x - inner / 2.0 + along * (k as f64 + 0.5) + if row % 2 == 1 { along / 2.0 } else { 0.0 };
                let step = if row % 2 == 1 { if north { CABINET.1 + 0.35 } else { -CABINET.1 - 0.35 } } else { 0.0 };
                let sz = z_back + step * (row / 2) as f64;
                Slot { x: fixed(sx, 3), z: fixed(sz, 3), ry: back_ry, kind: r.cabinet }
            })
            .collect();
    }
    let mut walls: Vec<[f64; 4]> = Vec::new();
    let mut seg = |x1: f64, z1: f64, x2: f64, z2: f64| walls.push([fixed(x1, 2), fixed(z1, 2), fixed(x2, 2), fixed(z2, 2)]);
    let (fw, fd) = (FOYER.0 / 2.0, FOYER.1 / 2.0);
    seg(-fw, -fd, -fw, fd);
    seg(-fw, -fd, fw - DOOR / 2.0, -fd);
    seg(fw - DOOR / 2.0, fd, -fw, fd);
    seg(fw, -fd, fw, -DOOR / 2.0);
    seg(fw, DOOR / 2.0, fw, fd);
    for (side, north) in [(-cor_h, true), (cor_h, false)] {
        let mut gaps: Vec<(f64, f64)> = rooms.iter().filter(|r| (r.z < 0.0) == north).map(|r| (r.door.x - DOOR / 2.0, r.door.x + DOOR / 2.0)).collect();
        gaps.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
        let mut at = FOYER.0 / 2.0;
        for (g0, g1) in gaps {
            if g0 > at {
                seg(at, side, g0, side);
            }
            at = at.max(g1);
        }
        if at < corridor_end {
            seg(at, side, corridor_end, side);
        }
    }
    seg(corridor_end, -cor_h, corridor_end, cor_h);
    for r in &rooms {
        let (hw, hd, cx, cz) = (r.w / 2.0, r.d / 2.0, r.x, r.z);
        let north = r.z < 0.0;
        let door_side = if north { cz + hd } else { cz - hd };
        seg(cx - r.w / 2.0, door_side, r.door.x - DOOR / 2.0, door_side);
        seg(r.door.x + DOOR / 2.0, door_side, cx + r.w / 2.0, door_side);
        let far = if north { cz - hd } else { cz + hd };
        seg(cx - hw, far, cx + hw, far);
        seg(cx - hw, cz - hd, cx - hw, door_side);
        seg(cx + hw, cz - hd, cx + hw, door_side);
    }
    let by_game = rooms.iter().flat_map(|r| r.game_ids.iter().enumerate().map(|(i, id)| (id.clone(), (r.id.clone(), i)))).collect();
    Arcade { cap, rooms, corridor_end, walls, by_game }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_big_shelf_keeps_its_favourites_and_most_played() {
        // two shelves: up to 300 each
        assert_eq!(pick(&[false; 10], &[0; 10], 2), (0..10).collect::<Vec<_>>());
        let mut fav = vec![false; 400];
        let mut plays = vec![0u64; 400];
        fav[399] = true;
        plays[350] = 5;
        let kept = pick(&fav, &plays, 2);
        assert_eq!(kept.len(), 300);
        assert!(kept.contains(&399) && kept.contains(&350) && kept.contains(&0) && !kept.contains(&299));
        assert!(kept.windows(2).all(|w| w[0] < w[1]));
        // a hundred shelves: 60 each, never fewer
        assert_eq!(pick(&vec![false; 500], &[], 100).len(), 60);
    }
}
