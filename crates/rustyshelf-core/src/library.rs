// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the shelves of
// Boxshelf's main.js library build (buildLibrary), as RustyShelf has it.
//! Every shelf: each console's, read from the settings and the games folders; the PC shelf's Steam and store games;
//! the Custom shelf's Quiver apps and programs added by hand.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::art::{Art, ArtLibrary};
use crate::collate::compare_plain;
use crate::config::{Config, LoadError, SystemConfig};
use crate::paths::{self, is_file};
use crate::pccovers;
use crate::pcgames::{self, PcGame};
use crate::scanner::{self, Game, ScanData, Source};
use crate::steam::{self, SteamGame};
use crate::systems::{systems, Console};
use crate::{js, pcfolder};

#[derive(Debug, Clone)]
pub enum ShelfKind {
    Console { console: Box<Console>, config: SystemConfig },
    /// the PC shelf and the Custom shelf
    Pc,
    /// the Media shelf
    Apps,
    /// a shelf that mixes the others' games: Home, All, a collection
    Mixed,
}

#[derive(Debug, Clone)]
pub struct Shelf {
    /// the console's id, "pc", "custom" or "media"
    pub id: String,
    pub name: String,
    pub kind: ShelfKind,
    pub games: Vec<Game>,
    /// on a mixed shelf, the shelf each game is from; empty on the others
    pub from: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Library {
    pub app_root: PathBuf,
    pub config: Config,
    /// consoles that are on, with an emulator or a games folder (even with no games yet), in the settings' console
    /// order, then the PC, Custom and Media shelves
    pub shelves: Vec<Shelf>,
}

/// What a PC game's case needs from the data folder.
struct PcArt<'a> {
    cfg: &'a Config,
    data: &'a Path,
    /// Settings → PC games → Console box art
    boxes: bool,
    /// a store game's title (lower case) → the Steam app it was matched to
    steam_ids: HashMap<String, String>,
    /// Steam's store info by app id
    steam_infos: HashMap<String, crate::info::Info>,
    sizes: &'a ScanData,
}

impl PcArt<'_> {
    /// The art the Electron app fetched from Steam's store for an app (`data/steam-art`): its cover and its hero.
    fn steam_art(&self, appid: &str) -> (Option<PathBuf>, Option<PathBuf>) {
        let local = |name: String| Some(self.data.join("steam-art").join(name)).filter(|f| is_file(f));
        (local(format!("{appid}_cover.jpg")), local(format!("{appid}_hero.jpg")))
    }

    /// A game's front, and with a console box its back, its spine and the console it's from. The player's pick comes
    /// first, then its console box, then `auto`: the automatic covers, best first.
    fn case(&self, id: &str, auto: impl FnOnce() -> Option<PathBuf>) -> (Art, Option<String>) {
        let chosen = pccovers::cover(self.data, id);
        let boxed = if chosen.is_none() && self.boxes { pccovers::console_cover(self.data, id, "") } else { None };
        let plat = if chosen.is_some() || boxed.is_some() { pccovers::console_plat(self.data, id) } else { None };
        let in_box = boxed.is_some() || (chosen.is_some() && plat.is_some());
        let part = |p: &str| if in_box { pccovers::console_cover(self.data, id, p) } else { None };
        let (back, spine) = (part("back"), part("spine"));
        (Art { front: chosen.or(boxed).or_else(auto), back, spine, ..Art::default() }, plat)
    }

    fn steam_game(&self, g: SteamGame) -> Game {
        let id = format!("steam-{}", g.appid);
        let (cover, hero) = self.steam_art(&g.appid);
        let (art, box_plat) = self.case(&id, || g.front.or(cover));
        Game {
            title: self.cfg.game_title(&id).map(String::from).unwrap_or(g.title),
            id,
            file: g.dir.unwrap_or_default(),
            size_bytes: g.size_bytes,
            art,
            hero: g.hero.or(hero),
            box_plat,
            info: self.steam_infos.get(&g.appid).cloned(),
            source: Source::Steam { appid: g.appid, minutes: g.minutes, last_played: g.last_played },
            ..Game::default()
        }
    }

    fn pc_game(&self, g: PcGame) -> Game {
        // a store's title names the Steam app whose art it gets; a recomp or a program added by hand often shares its
        // name with an unrelated Steam game
        let store = matches!(g.source, "epic" | "gog" | "ubisoft" | "ea" | "xbox" | "folder");
        let sid = store.then(|| self.steam_ids.get(&g.title.to_lowercase())).flatten();
        let (cover, hero) = sid.map(|sid| self.steam_art(sid)).unwrap_or_default();
        let (art, box_plat) = self.case(&g.id, || {
            pccovers::auto_cover(self.data, &g.id).or_else(|| Some(&g.exe).filter(|e| !e.is_empty()).and_then(|e| pccovers::folder_cover(e))).or(cover)
        });
        // only the games whose own folder is the game: a program added by hand may sit in a shared folder
        let sized = g.installed != Some(false) && matches!(g.source, "folder" | "epic" | "hoyoplay") && !g.dir.is_empty();
        Game {
            id: g.id.clone(),
            title: g.title.clone(),
            file: PathBuf::from(if g.exe.is_empty() { &g.dir } else { &g.exe }),
            size_bytes: if sized { self.sizes.pc_dir_size(&g.dir) } else { 0 },
            art,
            hero,
            not_installed: g.installed == Some(false),
            box_plat,
            info: sid.and_then(|sid| self.steam_infos.get(sid)).cloned(),
            source: Source::Pc(Box::new(g)),
            ..Game::default()
        }
    }
}

/// The PC shelf's Steam games, from the Steam folder Settings names, else the one found in the usual place.
fn steam_games(cfg: &Config) -> Vec<SteamGame> {
    let (enabled, path, hide_tools) = cfg.steam();
    if !enabled {
        return Vec::new();
    }
    // unset, wrong, or on a drive that doesn't answer: the Steam in the usual place
    let path = match steam::steam_root_within(&path) {
        Some(_) => path,
        None => steam::find_steam()
            .map(|p| p.to_string_lossy().into_owned())
            .filter(|p| steam::steam_root_within(p).is_some())
            .unwrap_or(path),
    };
    if path.is_empty() {
        return Vec::new();
    }
    // a library on another drive set as the Steam folder: Steam's own folder lists every library
    let main = steam::steam_root_within(&path)
        .filter(|r| !steam::is_main_steam_within(r))
        .and_then(|_| steam::find_steam())
        .filter(|p| steam::steam_root_within(&p.to_string_lossy()).is_some_and(|r| steam::is_main_steam_within(&r)))
        .map(|p| p.to_string_lossy().into_owned());
    steam::scan(&path, main.as_deref(), hide_tools)
}

/// The PC games that aren't Steam's, under the names the player gave them: the stores' and the PC games folder's
/// (for the PC shelf), then Quiver's and the ones added by hand.
fn pc_games(cfg: &Config, data: &Path) -> (Vec<PcGame>, Vec<PcGame>) {
    let pc = cfg.raw().get("pc");
    let (get, on) = (|k: &str| pc.and_then(|p| p.get(k)), |k: &str| pcgames::wanted(pc, k));
    let mut stores = Vec::new();
    // TODO: on Linux the Electron app lists the games the Epic account owns (main/linuxpc.js)
    if on("epic") && cfg!(windows) {
        stores.extend(pcgames::epic_games(&pcgames::epic_dir()));
    }
    // Details → Installation: an Epic game set by hand to installed / not installed
    for g in stores.iter_mut() {
        match cfg.game(&g.id).and_then(|o| o.get("install")).and_then(Value::as_str) {
            Some("installed") => g.installed = Some(true),
            Some("notinstalled") => g.installed = Some(false),
            _ => {}
        }
    }
    // Epic's installs in the PC games folder are already on the shelf
    let taken: Vec<String> = stores.iter().map(|g| g.dir.clone()).collect();
    stores.extend(pcfolder::scan(js::str_of(get("gamesDir")).unwrap_or(""), js::str_of(get("userGamesDir")).unwrap_or(""), get("programs"), &taken));
    stores.extend(pcgames::store_games_of(pc, data));
    let mut mine = Vec::new();
    if on("quiver") {
        mine.extend(pcgames::quiver_games(js::str_of(get("quiverPath")).unwrap_or("")));
    }
    mine.extend(pcgames::custom_games(get("custom")));
    for g in stores.iter_mut().chain(mine.iter_mut()) {
        if let Some(t) = cfg.game_title(&g.id) {
            g.title = t.to_string();
        }
    }
    (stores, mine)
}

impl Library {
    pub fn load() -> Result<Library, LoadError> {
        Self::load_from(&paths::app_root())
    }

    pub fn load_from(app_root: &Path) -> Result<Library, LoadError> {
        let data_dir = paths::data_dir(app_root);
        let config = Config::load(&data_dir)?;
        let lib = ArtLibrary::from_config(&config, app_root);
        let data = ScanData::load(&data_dir);
        let retroarch = config.retroarch();
        // a console that's on, with an emulator or a games folder, has its place among the shelves
        let wanted: Vec<(&Console, SystemConfig)> = systems()
            .consoles
            .iter()
            .map(|c| (c, config.system(c)))
            .filter(|(c, sc)| sc.enabled && !c.standalone && (!sc.emulator.is_empty() || (c.retro && !retroarch.is_empty()) || !sc.rom_dirs.is_empty()))
            .collect();
        let (mut shelves, steam, (stores, mine)) = std::thread::scope(|s| {
            let steam = s.spawn(|| steam_games(&config));
            let pc = s.spawn(|| pc_games(&config, &data_dir));
            let jobs: Vec<_> = wanted
                .into_iter()
                .map(|(c, sc)| {
                    let (dirs, data) = (lib.dirs_for(&sc, app_root), &data);
                    s.spawn(move || Shelf {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        games: if sc.rom_dirs.is_empty() { Vec::new() } else { scanner::scan(c, &sc, &dirs, data) },
                        kind: ShelfKind::Console { console: Box::new(c.clone()), config: sc },
                        from: Vec::new(),
                    })
                })
                .collect();
            let shelves: Vec<Shelf> = jobs.into_iter().filter_map(|j| j.join().ok()).collect();
            (shelves, steam.join().unwrap_or_default(), pc.join().unwrap_or_default())
        });
        // a console game's info goes by the name its art is looked up under: the one picked by hand (Details → Art
        // match), else the title read from its file
        let mut infos = crate::info::InfoCache::load(&data_dir.join("boxart-cache"));
        for shelf in shelves.iter_mut() {
            for g in shelf.games.iter_mut() {
                let name = config.game(&g.id).and_then(|o| o.get("artName")).and_then(Value::as_str).filter(|n| !n.is_empty()).unwrap_or(&g.title);
                g.info = infos.get(&shelf.id, name);
                if let Some(t) = config.game_title(&g.id) {
                    g.title = t.to_string();
                }
            }
        }
        infos.flush();
        let art = PcArt { cfg: &config, data: &data_dir, boxes: config.pc_console_box(), steam_ids: pcgames::steam_ids(&data_dir), steam_infos: crate::info::steam_infos(&data_dir), sizes: &data };
        let mut pc: Vec<Game> = steam.into_iter().map(|g| art.steam_game(g)).collect();
        pc.extend(stores.into_iter().map(|g| art.pc_game(g)));
        // a game added by hand can sit on the PC shelf or on a console's, beside its ROMs: it's still started as its
        // own program. A console that's switched off sends its games back to Custom
        let (mut custom, mut back, mut moved): (Vec<Game>, Vec<Game>, HashMap<String, Vec<Game>>) = Default::default();
        for g in mine {
            let shelf = g.shelf.clone();
            let game = art.pc_game(g);
            match shelf.as_deref() {
                None => custom.push(game),
                Some("pc") => pc.push(game),
                Some(id) if systems().console(id).is_some_and(|c| config.system(c).enabled) => moved.entry(id.to_string()).or_default().push(game),
                Some(_) => back.push(game),
            }
        }
        custom.extend(back);
        // a console with neither an emulator nor a games folder gets a shelf for them, after the others
        for c in &systems().consoles {
            let Some(games) = moved.remove(&c.id) else { continue };
            match shelves.iter_mut().find(|s| s.id == c.id) {
                Some(shelf) => shelf.games.extend(games),
                None => shelves.push(Shelf { id: c.id.clone(), name: c.name.clone(), games, kind: ShelfKind::Console { console: Box::new(c.clone()), config: config.system(c) }, from: Vec::new() }),
            }
        }
        for (id, name, mut games) in [("pc", "PC", pc), ("custom", "Custom", custom)] {
            if !games.is_empty() {
                games.sort_by(|a, b| compare_plain(&a.title, &b.title));
                shelves.push(Shelf { id: id.into(), name: name.into(), kind: ShelfKind::Pc, games, from: Vec::new() });
            }
        }
        let apps: Vec<Game> = systems()
            .apps
            .iter()
            .filter_map(|a| {
                let c = config.raw().get("apps").and_then(|m| m.get(&a.id))?;
                let path = js::filled(c.get("path")).filter(|_| js::truthy(c.get("enabled")))?;
                Some(Game { id: format!("app-{}", a.id), title: a.name.clone(), file: PathBuf::from(path), source: Source::App { path: path.to_string() }, ..Game::default() })
            })
            .collect();
        if !apps.is_empty() {
            shelves.push(Shelf { id: "media".into(), name: "Media".into(), kind: ShelfKind::Apps, games: apps, from: Vec::new() });
        }
        Ok(Library { app_root: app_root.to_path_buf(), config, shelves })
    }
}

/// What the shelves want from outside, fetched and measured off the shelf's thread: the Xbox 360 title list (again
/// after a week), the arcade names (once), and the size of each game folder that has none yet or has changed. True
/// when reading the library again would show something new.
pub fn upkeep(lib: &Library) -> bool {
    let data_dir = paths::data_dir(&lib.app_root);
    let has = |ids: &[&str]| lib.shelves.iter().any(|s| ids.contains(&s.id.as_str()));
    let mut changed = false;
    if has(&["x360"]) {
        changed |= crate::x360::refresh(&data_dir).unwrap_or(false);
    }
    if has(&crate::arcade::SYSTEMS) {
        changed |= crate::arcade::fetch(&data_dir).unwrap_or(false);
    }
    let mut dirs: Vec<String> = Vec::new();
    for shelf in &lib.shelves {
        for g in &shelf.games {
            match &g.source {
                Source::Pc(pc) if g.not_installed || pc.dir.is_empty() => {}
                Source::Pc(pc) if matches!(pc.source, "folder" | "epic" | "hoyoplay") => dirs.push(pc.dir.clone()),
                Source::Rom if shelf.id == "sw2" && crate::nx2::is_unpacked(&g.file) => {
                    dirs.push(crate::art::resolve(&crate::nx2::game_root(&g.file)).to_string_lossy().into_owned())
                }
                _ => {}
            }
        }
    }
    changed |= crate::dirsize::DirSizes::load(&data_dir).measure_stale(&dirs);
    changed |= steam_fill(lib, &data_dir);
    changed | auto_covers(lib, &data_dir)
}

/// Box art for the console games missing a front, a back or a spine, unless box art downloads are off. Each game is
/// looked at once a week at most (`data/cover-checks.json`: game id → when): one cover that doesn't exist anywhere
/// mustn't send its console round at every start. True when a picture came in.
fn auto_covers(lib: &Library, data_dir: &Path) -> bool {
    const WEEK: f64 = 7.0 * 86_400_000.0;
    if lib.config.auto_covers() == Some(false) {
        return false;
    }
    let file = data_dir.join("cover-checks.json");
    let mut memo = match std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
        Some(Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    // a made back or spine still wants a real scan
    let incomplete = |g: &Game| g.art.front.is_none() || g.art.back.is_none() || g.art.made_back || g.art.spine.is_none() || g.art.made_spine;
    let mut wanted = Vec::new();
    for shelf in &lib.shelves {
        let ShelfKind::Console { console, config } = &shelf.kind else { continue };
        if config.rom_dirs.is_empty() {
            continue;
        }
        let mut games = Vec::new();
        for g in shelf.games.iter().filter(|g| g.source == Source::Rom && incomplete(g)) {
            if memo.get(&g.id).is_some_and(|at| js::truthy(Some(at)) && (now as f64 - js::number(Some(at))) < WEEK) {
                continue;
            }
            memo.insert(g.id.clone(), Value::from(now));
            games.push(g);
        }
        if !games.is_empty() {
            wanted.push(crate::boxart::Wanted { console, config, games });
        }
    }
    if wanted.is_empty() {
        return false;
    }
    let art = ArtLibrary::from_config(&lib.config, &lib.app_root);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let cache = data_dir.join("boxart-cache");
    let run = crate::boxart::Run { art: &art, app_root: &lib.app_root, cache_dir: &cache, kinds: &["front", "back", "spine", "disc"], region: lib.config.art_region() };
    let report = crate::boxart::download(&run, &wanted, &|_, _, _| {}, &cancel);
    // saved once the run has finished: a run cut short mustn't skip these games for a week
    let tmp = data_dir.join("cover-checks.json.tmp");
    if std::fs::write(&tmp, Value::Object(memo).to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, &file);
    }
    report.systems.iter().any(|r| r.front + r.back + r.spine + r.disc > 0)
}

/// Covers, heroes and store info for the PC shelf's games that have none, from Steam's store: its own games, and
/// with box art downloads on the other stores' games by the Steam app of the same name (their titles go to Steam's
/// store search). True when something new came in.
fn steam_fill(lib: &Library, data_dir: &Path) -> bool {
    use crate::steamart::Wanted;
    if lib.config.auto_covers() == Some(false) {
        return false;
    }
    let Some(pc) = lib.shelves.iter().find(|s| s.id == "pc") else { return false };
    let has = |g: &Game| (g.art.front.is_some(), g.hero.is_some());
    let mut games: Vec<Wanted> = Vec::new();
    let mut others: Vec<&Game> = Vec::new();
    for g in &pc.games {
        match &g.source {
            Source::Steam { appid, .. } => games.push(Wanted { appid: appid.clone(), has_front: has(g).0, has_hero: has(g).1 }),
            Source::Pc(p) if matches!(p.source, "epic" | "gog" | "ubisoft" | "ea" | "xbox" | "folder") => others.push(g),
            _ => {}
        }
    }
    let mut matched = 0;
    if !others.is_empty() && lib.config.auto_covers() == Some(true) {
        matched = pcgames::match_steam(data_dir, &others.iter().map(|g| g.title.clone()).collect::<Vec<_>>());
        let ids = pcgames::steam_ids(data_dir);
        games.extend(others.iter().filter_map(|g| Some(Wanted { appid: ids.get(&g.title.to_lowercase())?.clone(), has_front: has(g).0, has_hero: has(g).1 })));
    }
    let (covers, other) = if games.is_empty() { (0, 0) } else { crate::steamart::fill(data_dir, &games) };
    covers + other + matched > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    #[test]
    fn pc_cases() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        for f in ["pc-covers/steam-10.png", "pc-console-covers/custom-zeta.jpg", "pc-console-covers/custom-zeta-spine.jpg", "steam-art/77_cover.jpg", "steam-art/77_hero.jpg"] {
            touch(&d.join(f));
        }
        let cfg = Config::parse(r#"{"games":{"steam-10":{"title":"Zeta Deluxe"}}}"#).unwrap();
        let sizes = ScanData::default();
        let art = PcArt { cfg: &cfg, data: d, boxes: true, steam_ids: HashMap::from([("omega".to_string(), "77".to_string())]), steam_infos: HashMap::new(), sizes: &sizes };
        let g = art.steam_game(SteamGame { appid: "10".into(), title: "Zeta".into(), size_bytes: 1, dir: None, minutes: 3, last_played: 0, front: Some("/s/f.jpg".into()), hero: None });
        assert_eq!((g.title.as_str(), g.art.front, g.art.spine), ("Zeta Deluxe", Some(d.join("pc-covers/steam-10.png")), None));
        let g = art.pc_game(PcGame { id: "custom-zeta".into(), source: "custom", title: "Omega".into(), ..PcGame::default() });
        assert_eq!((g.art.front, g.art.spine, g.hero), (Some(d.join("pc-console-covers/custom-zeta.jpg")), Some(d.join("pc-console-covers/custom-zeta-spine.jpg")), None));
        let g = art.pc_game(PcGame { id: "epic-omega".into(), source: "epic", title: "Omega".into(), installed: Some(false), ..PcGame::default() });
        assert_eq!((g.art.front, g.hero, g.not_installed), (Some(d.join("steam-art/77_cover.jpg")), Some(d.join("steam-art/77_hero.jpg")), true));
    }

    #[test]
    fn hand_added_games_on_their_shelves() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        for f in ["tools/a.exe", "tools/b.exe", "tools/c.exe", "tools/d.exe", "snes/Zeta (USA).sfc"] {
            touch(&root.join(f));
        }
        let at = |f: &str| root.join(f).to_string_lossy().replace('\\', "/");
        let cfg = serde_json::json!({
            "steam": { "enabled": false },
            "pc": { "epic": false, "gog": false, "ubisoft": false, "ea": false, "xbox": false, "hoyoplay": false, "quiver": false, "custom": [
                { "title": "On Custom", "exe": at("tools/a.exe") }, { "title": "On PC", "exe": at("tools/b.exe"), "shelf": "pc" },
                { "title": "On SNES", "exe": at("tools/c.exe"), "platform": "snes" }, { "title": "Console Off", "exe": at("tools/d.exe"), "shelf": "gc" },
                { "title": "Ported", "exe": at("tools/a.exe"), "shelf": "xboxone" }, { "title": "No Emulator", "exe": format!(" {} ", at("tools/b.exe")), "shelf": "nes" } ] },
            "systems": { "snes": { "romDirs": [at("snes")] }, "gc": { "enabled": false } },
            "games": { "custom-on-pc": { "title": "Renamed" } },
            "apps": { "stremio": { "enabled": true, "path": at("tools/a.exe") }, "nuvio": { "enabled": false, "path": at("tools/b.exe") } },
        });
        touch(&root.join("data/config.json"));
        std::fs::write(root.join("data/config.json"), cfg.to_string()).unwrap();
        let lib = Library::load_from(root).unwrap();
        let shelves: Vec<(&str, Vec<&str>)> = lib.shelves.iter().map(|s| (s.id.as_str(), s.games.iter().map(|g| g.title.as_str()).collect())).collect();
        assert_eq!(shelves, [("snes", vec!["Zeta", "On SNES"]), ("nes", vec!["No Emulator"]), ("xboxone", vec!["Ported"]), ("pc", vec!["Renamed"]), ("custom", vec!["Console Off", "On Custom"]), ("media", vec!["Stremio"])]);
    }
}
