// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the shelves of
// Boxshelf's main.js library build (buildLibrary), as RustyShelf has it. The Game streaming shelf, PC games' backs of
// the box, and recomps on their console's shelf (by hand or by themselves) in its whole case are from
// ZakAuMiel/boxshelf-source (https://github.com/ZakAuMiel/boxshelf-source, GPL-3.0), Zak's fork of Boxshelf: ported
// from its main.js.
//! Every shelf: each console's, read from the settings and the games folders; the PC shelf's Steam and store games;
//! the Custom shelf's Quiver apps and programs added by hand; the Game streaming shelf's apps of another PC.

use std::collections::{HashMap, HashSet};
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
    /// the console's id, "pc", "custom", "ports", "streaming" or "media"
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

    /// The back made from the store's screenshots, for a game whose case isn't a console box.
    fn steam_back(&self, art: &mut Art, box_plat: &Option<String>, appid: &str) {
        let made = crate::steamart::back_file(self.data, appid);
        if art.back.is_none() && box_plat.is_none() && is_file(&made) {
            (art.back, art.made_back) = (Some(made), true);
        }
    }

    /// A game's front, its back, and with a console box its spine and the console it's from. The player's pick comes
    /// first, then its console box, then `auto`: the automatic covers, best first. The back: the player's, the console
    /// box's, else LaunchBox's PC game's unless "Made-up back" was chosen.
    fn case(&self, id: &str, auto: impl FnOnce() -> Option<PathBuf>) -> (Art, Option<String>) {
        let chosen = pccovers::cover(self.data, id);
        let boxed = if chosen.is_none() && self.boxes { pccovers::console_cover(self.data, id, "") } else { None };
        let plat = if chosen.is_some() || boxed.is_some() { pccovers::console_plat(self.data, id) } else { None };
        let in_box = boxed.is_some() || (chosen.is_some() && plat.is_some());
        let part = |p: &str| if in_box { pccovers::console_cover(self.data, id, p) } else { None };
        let launchbox = || if in_box || pccovers::made_up_back(self.data, id) { None } else { pccovers::auto_back(self.data, id) };
        let (back, spine) = (pccovers::back(self.data, id).or_else(|| part("back")).or_else(launchbox), part("spine"));
        (Art { front: chosen.or(boxed).or_else(auto), back, spine, ..Art::default() }, plat)
    }

    fn steam_game(&self, g: SteamGame) -> Game {
        let id = format!("steam-{}", g.appid);
        let (cover, hero) = self.steam_art(&g.appid);
        let (mut art, box_plat) = self.case(&id, || g.front.or(cover));
        self.steam_back(&mut art, &box_plat, &g.appid);
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
        let (mut art, box_plat) = self.case(&g.id, || {
            pccovers::auto_cover(self.data, &g.id).or_else(|| Some(&g.exe).filter(|e| !e.is_empty()).and_then(|e| pccovers::folder_cover(e))).or(cover)
        });
        if let Some(sid) = sid {
            self.steam_back(&mut art, &box_plat, sid);
        }
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

    /// A PC game on a console's shelf, with the spine, disc and manual the Art Studio and Details gave it.
    fn on_console(&self, mut g: Game) -> Game {
        if let Some(spine) = pccovers::part(self.data, &g.id, "spine") {
            (g.art.spine, g.art.made_spine) = (Some(spine), false);
        }
        g.art.disc = pccovers::part(self.data, &g.id, "disc");
        (g.art.manual, g.art.manual_pdf) = pccovers::manual(self.data, &g.id);
        g
    }

    /// A Game streaming app: the cover chosen for it, its console box, Moonlight's box art, else the automatic one.
    fn streamed(&self, g: PcGame, boxart: Option<PathBuf>) -> Game {
        let (art, box_plat) = self.case(&g.id, || boxart.or_else(|| pccovers::auto_cover(self.data, &g.id)));
        Game { id: g.id.clone(), title: g.title.clone(), file: PathBuf::from(&g.exe), art, box_plat, source: Source::Pc(Box::new(g)), ..Game::default() }
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
    if on("epic") && cfg!(windows) {
        stores.extend(pcgames::epic_games(&pcgames::epic_dir()));
    }
    // (Linux) every game the Epic account owns: the ones not installed are greyed, and play through Wine / Proton once
    // installed
    if on("epic") && cfg!(target_os = "linux") {
        stores.extend(crate::linuxpc::epic_owned(data).into_iter().map(|g| PcGame {
            id: g.id,
            source: "epic",
            title: g.title,
            exe: g.exe,
            dir: g.dir,
            installed: (!g.installed).then_some(false),
            app_name: g.app_name,
            cover_url: g.cover_url,
            ..PcGame::default()
        }));
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
    let mut taken: Vec<String> = stores.iter().map(|g| g.dir.clone()).collect();
    if cfg!(target_os = "linux") {
        taken.extend(crate::linuxpc::epic_unfinished(data));
    }
    let mut games_dirs = vec![js::str_of(get("gamesDir")).unwrap_or("").to_string()];
    games_dirs.extend(get("moreGamesDirs").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(String::from));
    stores.extend(pcfolder::scan_all(&games_dirs, js::str_of(get("userGamesDir")).unwrap_or(""), get("programs"), &taken));
    stores.extend(pcgames::store_games_of(pc, data));
    let mut mine = Vec::new();
    if on("quiver") {
        mine.extend(pcgames::quiver_games(js::str_of(get("quiverPath")).unwrap_or("")));
    }
    mine.extend(pcgames::custom_games(get("custom")));
    mine.extend(pcfolder::scan_ports(js::str_of(get("portsDir")).unwrap_or("")));
    for g in stores.iter_mut().chain(mine.iter_mut()) {
        if let Some(t) = cfg.game_title(&g.id) {
            g.file_title = Some(std::mem::replace(&mut g.title, t.to_string()));
        }
    }
    (stores, mine)
}

/// The games folders that didn't answer at the last read, and each console's games then.
static GONE: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
static KEPT: std::sync::LazyLock<std::sync::Mutex<HashMap<String, Vec<Game>>>> = std::sync::LazyLock::new(Default::default);

/// The folders of these that don't answer within 5 s, or aren't there.
fn unreachable(dirs: &[String]) -> Vec<String> {
    let asks: Vec<_> = dirs
        .iter()
        .filter(|d| !d.is_empty())
        .map(|d| {
            let (tx, rx) = std::sync::mpsc::channel();
            let at = crate::art::resolve(Path::new(d));
            // (a thread of its own: a network drive that's gone can hold a folder read up far longer)
            std::thread::spawn(move || {
                let _ = tx.send(std::fs::read_dir(at).is_ok());
            });
            (d.clone(), rx)
        })
        .collect();
    asks.into_iter().filter(|(_, rx)| !rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap_or(false)).map(|(d, _)| d).collect()
}

/// A console's games: its folders that answer are read, and a folder that can't be reached (a network drive gone for a
/// moment) keeps the games it had at the last read, instead of the shelf coming out empty.
fn reachable_games(c: &Console, sc: &SystemConfig, art_dirs: &[PathBuf], data: &ScanData, cfg: &Config, data_dir: &Path) -> Vec<Game> {
    if sc.rom_dirs.is_empty() {
        return Vec::new();
    }
    if crate::android::lister().is_some() {
        return crate::android::scan(c, sc, cfg, data_dir);
    }
    let gone = unreachable(&sc.rom_dirs);
    let mut games = if gone.is_empty() {
        scanner::scan(c, sc, art_dirs, data)
    } else {
        let here = SystemConfig { rom_dirs: sc.rom_dirs.iter().filter(|d| !gone.contains(d)).cloned().collect(), ..sc.clone() };
        if here.rom_dirs.is_empty() { Vec::new() } else { scanner::scan(c, &here, art_dirs, data) }
    };
    let mut kept = KEPT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !gone.is_empty() {
        let roots: Vec<PathBuf> = gone.iter().map(|d| crate::art::resolve(Path::new(d))).collect();
        let have: HashSet<PathBuf> = games.iter().map(|g| g.file.clone()).collect();
        let from_before: Vec<Game> = kept.get(&c.id).into_iter().flatten().filter(|g| !have.contains(&g.file) && roots.iter().any(|r| g.file.starts_with(r))).cloned().collect();
        games.extend(from_before);
        GONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).extend(gone);
    }
    kept.insert(c.id.clone(), games.clone());
    games
}

/// Whether a games folder that couldn't be reached at the last read answers again: the library is then read again.
pub fn folders_back() -> bool {
    let was = std::mem::take(&mut *GONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    if was.is_empty() {
        return false;
    }
    let still = unreachable(&was);
    let back = still.len() < was.len();
    GONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).extend(still);
    back
}

/// Whether a games folder was out of reach at the last read.
pub fn folders_gone() -> bool {
    !GONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_empty()
}

impl Library {
    pub fn load() -> Result<Library, LoadError> {
        Self::load_from(&paths::app_root())
    }

    /// The shelf the `i`th game of a mixed shelf (Home, All, a collection) comes from.
    pub fn own_shelf(&self, shelf: &Shelf, i: usize) -> Option<&Shelf> {
        shelf.from.get(i).and_then(|id| self.shelves.iter().find(|s| s.id == *id))
    }

    pub fn load_from(app_root: &Path) -> Result<Library, LoadError> {
        GONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
        let data_dir = paths::data_dir(app_root);
        let config = Config::load(&data_dir)?;
        crate::provenance::init(&data_dir);
        let lib = ArtLibrary::from_config(&config, app_root);
        let data = ScanData::load(&data_dir);
        let retroarch = config.retroarch();
        // a console that's on, with an emulator or a games folder, has its place among the shelves
        let wanted: Vec<(&Console, SystemConfig)> = systems()
            .consoles
            .iter()
            .map(|c| (c, config.system(c)))
            .filter(|(c, sc)| sc.enabled && !c.standalone && (!sc.emulator.is_empty() || (c.retro && !retroarch.is_empty()) || !sc.rom_dirs.is_empty()))
            // (Android) only the consoles the Android app has
            .filter(|(c, _)| crate::android::lister().is_none() || crate::android::system(&c.id).is_some())
            .collect();
        let (mut shelves, steam, (stores, mine)) = std::thread::scope(|s| {
            let steam = s.spawn(|| steam_games(&config));
            let pc = s.spawn(|| pc_games(&config, &data_dir));
            let jobs: Vec<_> = wanted
                .into_iter()
                .map(|(c, sc)| {
                    let (dirs, data, config, data_dir) = (lib.dirs_for(&sc, app_root), &data, &config, &data_dir);
                    s.spawn(move || Shelf {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        games: reachable_games(c, &sc, &dirs, data, config, data_dir),
                        kind: ShelfKind::Console { console: Box::new(c.clone()), config: sc },
                        from: Vec::new(),
                    })
                })
                .collect();
            let shelves: Vec<Shelf> = jobs.into_iter().filter_map(|j| j.join().ok()).collect();
            (shelves, steam.join().unwrap_or_default(), pc.join().unwrap_or_default())
        });
        let own_shelf: HashSet<String> = shelves.iter().map(|s| s.id.clone()).collect();
        let romm = crate::romm::Setup::from_config(&config, crate::seal::key(&paths::env_var).as_ref());
        if romm.on() && romm.cloud {
            let lib = romm.client(&data_dir).cached();
            let done = crate::romm::local_files(&data_dir);
            for shelf in shelves.iter_mut() {
                let (Some(games), ShelfKind::Console { config: sc, .. }) = (lib.consoles.get(&shelf.id), &shelf.kind) else { continue };
                let dir = crate::romm::folder(&sc.rom_dirs, &data_dir, &shelf.id, app_root);
                let extra = crate::romm::shelf_games(&shelf.id, &shelf.games, games, &dir, &done);
                shelf.games.extend(extra);
            }
        }
        // a console game's info goes by the name its art is looked up under: the one picked by hand (Details → Art
        // match), else the title read from its file
        let mut infos = crate::info::InfoCache::load(&crate::paths::boxart_cache(&data_dir));
        // what IGDB gave, kept from Details' lookups, fills the gaps LaunchBox leaves (nothing is asked here)
        let igdb = crate::igdb::Store::load(&data_dir);
        let more = crate::moreshots::Index::load(&data_dir);
        for shelf in shelves.iter_mut() {
            for g in shelf.games.iter_mut() {
                let name = config.game(&g.id).and_then(|o| o.get("artName")).and_then(Value::as_str).filter(|n| !n.is_empty()).unwrap_or(&g.title);
                g.info = infos.get(&shelf.id, name).or(g.info.take());
                if let Some(x) = igdb.get(&crate::gamekey::game_key(&shelf.id, &g.file, &g.title)) {
                    let mut info = g.info.take().unwrap_or_default();
                    crate::igdb::fill(&mut info, x);
                    g.info = Some(info);
                }
                // the screenshots added to LaunchBox's (Details' more screenshots)
                if g.source == Source::Rom && !g.file.as_os_str().is_empty() {
                    let base = g.info.as_ref().map(|i| i.shots.clone()).unwrap_or_default();
                    let all = more.list(&shelf.id, &g.file, &g.title, &base);
                    if all.len() > base.len() {
                        g.info.get_or_insert_with(Default::default).shots = all;
                    }
                }
                if let Some(t) = config.game_title(&g.id) {
                    g.title = t.to_string();
                }
            }
        }
        infos.flush();
        let studio = crate::manualstudio::Db::load(&data_dir);
        for shelf in shelves.iter_mut().filter(|s| s.id == "wiiu") {
            for g in shelf.games.iter_mut() {
                if wiiu_cover_first(&data_dir, &studio, g) {
                    let front = g.art.front.clone().unwrap_or_default();
                    g.art.manual.insert(0, front);
                }
            }
        }
        let art = PcArt { cfg: &config, data: &data_dir, boxes: config.pc_console_box(), steam_ids: pcgames::steam_ids(&data_dir), steam_infos: crate::info::steam_infos(&data_dir), sizes: &data };
        let mut pc: Vec<Game> = steam.into_iter().map(|g| art.steam_game(g)).collect();
        pc.extend(stores.into_iter().map(|g| art.pc_game(g)));
        // a game added by hand can sit on the PC shelf or on a console's, beside its ROMs: it's still started as its
        // own program. A console that's switched off sends its games back to Custom
        let (mut custom, mut ports, mut back) = (Vec::new(), Vec::new(), Vec::new());
        let mut moved: HashMap<String, Vec<Game>> = HashMap::new();
        let found = if config.recomp_detect() { crate::recomps::found(&data_dir) } else { HashMap::new() };
        for g in mine {
            let (shelf, port) = (g.shelf.clone(), g.port);
            let on = Some(&g).filter(|g| crate::recomps::candidate(g)).and_then(|g| found.get(&crate::recomps::key(&g.id, crate::recomps::title(g)))).filter(|sys| own_shelf.contains(*sys)).cloned();
            let game = art.pc_game(g);
            match shelf.as_deref() {
                // Ports & Recomps: a program set there, and Quiver's ports and recompilations
                Some("ports") => ports.push(game),
                None if port => ports.push(game),
                None => match on {
                    Some(sys) => moved.entry(sys).or_default().push(art.on_console(game)),
                    None => custom.push(game),
                },
                Some("pc") => pc.push(game),
                Some(id) if systems().console(id).is_some_and(|c| config.system(c).enabled) => moved.entry(id.to_string()).or_default().push(art.on_console(game)),
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
        let streamed: Vec<Game> = crate::moonlight::games(&config.moonlight())
            .into_iter()
            .map(|(mut g, boxart)| {
                if let Some(t) = config.game_title(&g.id) {
                    g.title = t.to_string();
                }
                art.streamed(g, boxart)
            })
            .collect();
        for (id, name, mut games) in [("pc", "PC", pc), ("custom", "Custom", custom), ("ports", "Ports & Recomps", ports), ("streaming", "Game streaming", streamed)] {
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

/// A Wii U manual made from the game's e-manual opens on its contents page: the box front goes first, so the
/// booklet opens on a cover like a printed one. Not over a scanned manual picked in Manual Studio.
fn wiiu_cover_first(data_dir: &Path, studio: &crate::manualstudio::Db, g: &Game) -> bool {
    let Some(first) = g.art.manual.first() else { return false };
    let picked = studio.cur(&format!("wiiu|{}", g.file.to_string_lossy().to_lowercase())).and_then(|c| c.get("src")).and_then(Value::as_str).is_some_and(|s| !s.is_empty() && s != "builtin");
    g.art.front.is_some() && !picked && (crate::wiiumanual::generated(data_dir, &g.title) || crate::wiiumanual::looks_made(first))
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
    changed |= pc_backs(lib, &data_dir);
    changed |= recomp_consoles(lib, &data_dir);
    // (before any picture is replaced: an HD upgrade asks it)
    if !crate::provenance::looked() {
        let art: Vec<PathBuf> = lib.shelves.iter().filter(|s| matches!(s.kind, ShelfKind::Console { .. })).flat_map(|s| &s.games).flat_map(|g| [&g.art.front, &g.art.back, &g.art.spine, &g.art.disc]).flatten().cloned().collect();
        let now = crate::now_ms();
        crate::provenance::first_look(&art, &data_dir.join("art-cand"), now);
    }
    if cfg!(target_os = "linux") {
        changed |= epic_upkeep(lib, &data_dir);
    }
    changed |= auto_covers(lib, &data_dir);
    changed |= auto_manuals(lib, &data_dir);
    changed |= wiiu_manuals(lib, &data_dir);
    changed | convert_pdf_manuals(lib)
}

/// Every game whose manual is still a PDF (just downloaded, or left over from before) gets its pages.
fn convert_pdf_manuals(lib: &Library) -> bool {
    let mut changed = false;
    for shelf in lib.shelves.iter().filter(|s| matches!(s.kind, ShelfKind::Console { .. })) {
        for g in shelf.games.iter().filter(|g| g.art.manual.is_empty()) {
            let Some(pdf) = &g.art.manual_pdf else { continue };
            let Some(dir) = pdf.parent() else { continue };
            let base = if matches!(g.source, Source::Pc(_)) { pccovers::safe_id(&g.id) } else { crate::art::art_base(&g.file) };
            changed |= crate::manuals::pdf_to_pages(pdf, dir, &base).is_ok_and(|n| n > 0);
        }
    }
    changed
}

/// Manuals, when fetching them is switched on. First the scan collections on the Internet Archive, for every
/// console that has one: the sharpest manuals there are, also over a manual that's there (eighty games a run at
/// most). Then, for the console games still without one, the confident matches on archive.org and
/// GamesDatabase.org, forty games a run at most, each looked at again only when it's due. Last, manuals scanned
/// open are cut into pages. The Wii U reads its games' own. True when a manual changed.
fn auto_manuals(lib: &Library, data_dir: &Path) -> bool {
    use crate::manuals::{auto_fetch, Online, Stores, Wanted};
    use crate::{manualcoll, spreads};
    if !lib.config.auto_manuals() {
        return false;
    }
    let art = ArtLibrary::from_config(&lib.config, &lib.app_root);
    let mut all = Vec::new();
    for shelf in lib.shelves.iter().filter(|s| s.id != "wiiu") {
        let ShelfKind::Console { config, .. } = &shelf.kind else { continue };
        for g in shelf.games.iter().filter(|g| g.source == Source::Rom) {
            let game = manualcoll::Game {
                sys: shelf.id.clone(),
                system: shelf.name.clone(),
                title: g.title.clone(),
                title_id: g.title_id.clone(),
                file: g.file.to_string_lossy().into_owned(),
                region: g.region.clone(),
                dir: art.manual_dir_for(config, &g.file, &lib.app_root),
                base: crate::art::art_base(&g.file),
            };
            all.push((game, g.art.manual_pdf.is_some()));
        }
    }
    if all.is_empty() {
        return false;
    }
    let now = || crate::js::date_now();
    let mut stores = Stores::load(data_dir);
    let games: Vec<manualcoll::Game> = all.iter().map(|g| g.0.clone()).collect();
    // (the collections are English, North American: not for a console set to another language or region)
    let mut changed = !manualcoll::run(&Online, data_dir, &games, &stores, &now, &|sys| stores.pref(sys) != "best", &|| false).is_empty();
    // each folder is listed once, not once a game
    let with_manual = |games: &[(manualcoll::Game, bool)]| -> HashSet<(PathBuf, String)> {
        let mut have = HashSet::new();
        for dir in games.iter().map(|g| &g.0.dir).collect::<HashSet<_>>() {
            for name in std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()) {
                if let Some(base) = name.strip_suffix("-manual.pdf") {
                    have.insert((dir.clone(), base.to_string()));
                } else if [".jpg", ".jpeg", ".png", ".webp"].iter().any(|e| name.ends_with(e)) {
                    have.extend(name.match_indices("-manual-").map(|(i, _)| (dir.clone(), name[..i].to_string())));
                }
            }
        }
        have
    };
    let have = with_manual(&all);
    let has = |g: &manualcoll::Game, have: &HashSet<(PathBuf, String)>| have.contains(&(g.dir.clone(), g.base.to_lowercase()));
    let wanted: Vec<Wanted> = all
        .iter()
        .filter(|(g, pdf)| !pdf && !has(g, &have))
        .map(|(g, _)| Wanted { sys: g.sys.clone(), title: g.title.clone(), file: g.file.clone(), dir: g.dir.clone(), base: g.base.clone() })
        .collect();
    changed |= auto_fetch(&Online, &wanted, &mut stores, now(), &|| false).1 > 0;
    let have = with_manual(&all);
    for (g, _) in all.iter().filter(|g| has(&g.0, &have)) {
        changed |= spreads::split_manual(&g.dir, &g.base, spreads::Split::default()).is_ok_and(|pages| pages > 0);
    }
    changed
}

/// Each Wii U game's own e-manual made into pages, once a game. True when a manual was made.
fn wiiu_manuals(lib: &Library, data_dir: &Path) -> bool {
    let art = ArtLibrary::from_config(&lib.config, &lib.app_root);
    let mut want = Vec::new();
    for shelf in lib.shelves.iter().filter(|s| s.id == "wiiu") {
        let ShelfKind::Console { config, .. } = &shelf.kind else { continue };
        for g in shelf.games.iter().filter(|g| g.source == Source::Rom) {
            want.push(crate::wiiumanual::Want {
                title: g.title.clone(),
                file: g.file.clone(),
                art_dir: art.manual_dir_for(config, &g.file, &lib.app_root),
                base: crate::art::art_base(&g.file),
            });
        }
    }
    crate::wiiumanual::collect(data_dir, &want, &crate::wiiumanual_draw::render) > 0
}

/// (Linux) The Epic library asked for again when it's over 6 hours old, and Epic's box art for the games without a
/// cover yet, three at a time. True when either brought something.
fn epic_upkeep(lib: &Library, data_dir: &Path) -> bool {
    let pc = lib.config.raw().get("pc");
    if !pcgames::wanted(pc, "epic") {
        return false;
    }
    let mut changed = false;
    if crate::linuxpc::epic_account(data_dir).is_some() && crate::linuxpc::library_age(data_dir).is_none_or(|a| a > 6 * 3_600_000) {
        if let Some(bin) = crate::linuxpc::legendary_bin(js::str_of(pc.and_then(|p| p.get("legendaryPath"))).unwrap_or(""), &lib.app_root) {
            changed |= crate::linuxpc::epic_library(&bin, data_dir).is_ok();
        }
    }
    let want: Vec<(String, String)> = lib
        .shelves
        .iter()
        .flat_map(|s| &s.games)
        .filter_map(|g| match &g.source {
            Source::Pc(pc) if pc.source == "epic" && !pc.cover_url.is_empty() && pccovers::auto_cover(data_dir, &pc.id).is_none() => Some((pc.id.clone(), pc.cover_url.clone())),
            _ => None,
        })
        .collect();
    let got = crate::pool::map(&want, 3, |(id, url)| pccovers::fetch_auto(data_dir, id, url).is_some());
    changed || got.contains(&true)
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
    let mut memo = crate::files::read_object(&file);
    let now = crate::now_ms();
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
    let cache = crate::paths::boxart_cache(data_dir);
    let raw = Value::Object(lib.config.raw().clone());
    let run = crate::boxart::Run { art: &art, app_root: &lib.app_root, cache_dir: &cache, kinds: &["front", "back", "spine", "disc"], region: lib.config.art_region(), hd: lib.config.art_hd(), explicit: false, cfg: &raw, index_ready_only: false };
    let report = crate::boxart::download(&run, &wanted, &|_, _, _| {}, &cancel);
    // saved once the run has finished: a run cut short mustn't skip these games for a week
    let _ = crate::files::write_atomic(&file, Value::Object(memo).to_string());
    report.systems.iter().any(|r| r.front + r.back + r.spine + r.disc > 0)
}

/// Covers, heroes and store info for the PC shelf's games that have none, from Steam's store: its own games, and
/// with box art downloads on the other stores' games by the Steam app of the same name (their titles go to Steam's
/// store search); then SteamGridDB's posters. True when something new came in.
fn steam_fill(lib: &Library, data_dir: &Path) -> bool {
    use crate::steamart::Wanted;
    if lib.config.auto_covers() == Some(false) {
        return false;
    }
    let posters = poster_fill(lib, data_dir);
    let Some(pc) = lib.shelves.iter().find(|s| s.id == "pc") else { return posters };
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
    let backs = if lib.config.make_backs() { crate::steamart::make_backs(data_dir, &backs_for(pc, data_dir)) } else { 0 };
    posters || covers + other + matched + backs > 0
}

/// The PC shelf's games a Steam back could be made for: their app, title and cover, when the case isn't a console box.
fn backs_for(pc: &Shelf, data_dir: &Path) -> Vec<(String, String, PathBuf)> {
    let ids = pcgames::steam_ids(data_dir);
    pc.games
        .iter()
        .filter(|g| g.art.back.is_none() && g.box_plat.is_none())
        .filter_map(|g| {
            let appid = match &g.source {
                Source::Steam { appid, .. } => appid.clone(),
                Source::Pc(p) if matches!(p.source, "epic" | "gog" | "ubisoft" | "ea" | "xbox" | "folder") => ids.get(&g.title.to_lowercase())?.clone(),
                _ => return None,
            };
            Some((appid, g.title.clone(), g.art.front.clone()?))
        })
        .collect()
}

/// LaunchBox's backs for the PC shelves' games with no back or only a made one (a port's from its console's
/// release), when Settings → PC games → Backs of the box from LaunchBox is on. True when one came in.
fn pc_backs(lib: &Library, data_dir: &Path) -> bool {
    if !lib.config.pc_auto_backs() {
        return false;
    }
    let wants: Vec<crate::pcbacks::Want> = lib
        .shelves
        .iter()
        .filter(|s| matches!(s.kind, ShelfKind::Pc))
        .flat_map(|s| &s.games)
        .filter(|g| g.box_plat.is_none() && (g.art.back.is_none() || g.art.made_back))
        .filter_map(crate::pcbacks::Want::of)
        .collect();
    let now = crate::now_ms();
    // a port or recomp not in a console's folder: its console looked up by its title, as a recomp's is
    let loose: Vec<(String, String)> = lib
        .shelves
        .iter()
        .filter(|s| s.id == "ports")
        .flat_map(|s| &s.games)
        .filter(|g| g.art.back.is_none() || g.art.made_back)
        .filter_map(|g| match &g.source {
            Source::Pc(p) if p.port && p.port_of.is_empty() => Some((g.id.clone(), crate::recomps::title(p).to_string())),
            _ => None,
        })
        .collect();
    let mut wants = wants;
    if !loose.is_empty() {
        crate::recomps::detect(data_dir, &crate::paths::boxart_cache(data_dir), &loose, &HashSet::new(), now);
        let found = crate::recomps::found(data_dir);
        for (id, title) in loose {
            if let Some(plat) = found.get(&crate::recomps::key(&id, &title)).and_then(|sys| crate::covers::launchbox_platform(sys)) {
                wants.push(crate::pcbacks::Want { id, title, store: true, plat });
            }
        }
    }
    !wants.is_empty() && crate::pcbacks::run(data_dir, &crate::paths::boxart_cache(data_dir), &wants, lib.config.art_region(), now) > 0
}

/// The Custom shelf's games that may be recomps, looked up in LaunchBox's database for their console, when Settings →
/// Custom games → Put recomps on their console's shelf is on. True when one was found for a console with a shelf.
fn recomp_consoles(lib: &Library, data_dir: &Path) -> bool {
    if !lib.config.recomp_detect() {
        return false;
    }
    let games: Vec<(String, String)> = lib
        .shelves
        .iter()
        .filter(|s| s.id == "custom")
        .flat_map(|s| &s.games)
        .filter_map(|g| match &g.source {
            Source::Pc(p) if crate::recomps::candidate(p) => Some((g.id.clone(), crate::recomps::title(p).to_string())),
            _ => None,
        })
        .collect();
    let shelves: HashSet<&str> = lib.shelves.iter().filter(|s| matches!(s.kind, ShelfKind::Console { .. })).map(|s| s.id.as_str()).collect();
    let now = crate::now_ms();
    !games.is_empty() && crate::recomps::detect(data_dir, &crate::paths::boxart_cache(data_dir), &games, &shelves, now)
}

/// SteamGridDB posters for the PC games that aren't Steam's and have no cover at all, wherever they sit (a program
/// can be on a console's shelf). Only with the player's SteamGridDB key. True when one came in.
fn poster_fill(lib: &Library, data_dir: &Path) -> bool {
    let bare: Vec<(&str, &str)> = lib.shelves.iter().flat_map(|s| &s.games).filter(|g| matches!(g.source, Source::Pc(_)) && g.art.front.is_none()).map(|g| (g.id.as_str(), g.title.as_str())).collect();
    if bare.is_empty() {
        return false;
    }
    let Some(sg) = crate::steamgrid::SteamGrid::from_config(&lib.config, data_dir, crate::seal::key(&paths::env_var).as_ref()) else { return false };
    crate::pccovers::fill_missing(data_dir, &bare, &sg).1 > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wii_u_manual_made_here_opens_on_the_box_front() {
        let t = tempfile::tempdir().unwrap();
        let data = t.path();
        let page = data.join("art/Game-manual-01.jpg");
        std::fs::create_dir_all(page.parent().unwrap()).unwrap();
        image::RgbImage::new(20, 40).save(&page).unwrap();
        let mut g = Game { title: "Game".into(), file: PathBuf::from("/games/Game.wua"), ..Game::default() };
        g.art.manual = vec![page.clone()];
        let studio = crate::manualstudio::Db::load(data);
        assert!(!wiiu_cover_first(data, &studio, &g), "pages not made here");
        std::fs::create_dir_all(data.join("wiiu-manuals")).unwrap();
        std::fs::write(data.join("wiiu-manuals/Game.made"), "x").unwrap();
        assert!(!wiiu_cover_first(data, &studio, &g), "no front");
        g.art.front = Some(data.join("art/Game.png"));
        assert!(wiiu_cover_first(data, &studio, &g));
        let mut studio = studio;
        studio.all["cur"]["wiiu|/games/game.wua"] = serde_json::json!({ "src": "coll:x" });
        assert!(!wiiu_cover_first(data, &studio, &g), "a manual picked in Manual Studio has its own cover");
        studio.all["cur"]["wiiu|/games/game.wua"] = serde_json::json!({ "src": "builtin" });
        assert!(wiiu_cover_first(data, &studio, &g));
        // made by another copy of the app: known by the pages' shape
        std::fs::remove_file(data.join("wiiu-manuals/Game.made")).unwrap();
        assert!(!wiiu_cover_first(data, &studio, &g));
        image::RgbImage::new(1376, 2000).save(&page).unwrap();
        assert!(wiiu_cover_first(data, &studio, &g));
    }

    /// (the folders out of reach are kept for the whole app: one read at a time here)
    static ONE_LOAD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn a_games_folder_out_of_reach_keeps_its_games() {
        let _one = ONE_LOAD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        for f in ["near/Zeta Run (USA).gb", "far/Omega Quest (USA).gb", "emu/gb.exe"] {
            touch(&root.join(f));
        }
        let at = |f: &str| root.join(f).to_string_lossy().replace('\\', "/");
        let cfg = serde_json::json!({ "steam": { "enabled": false }, "pc": { "epic": false, "gog": false, "ubisoft": false, "ea": false, "xbox": false, "hoyoplay": false, "quiver": false },
            "systems": { "gb": { "romDirs": [at("near"), at("far")], "emulator": at("emu/gb.exe") } } });
        touch(&root.join("data/config.json"));
        std::fs::write(root.join("data/config.json"), cfg.to_string()).unwrap();
        let titles = |lib: &Library| lib.shelves.iter().find(|s| s.id == "gb").map(|s| s.games.iter().map(|g| g.title.clone()).collect::<Vec<_>>()).unwrap_or_default();
        assert_eq!(titles(&Library::load_from(root).unwrap()), ["Omega Quest", "Zeta Run"]);
        assert!(!folders_gone());
        // the network drive goes for a moment: its game stays on the shelf, and it's looked at again
        std::fs::rename(root.join("far"), root.join("away")).unwrap();
        let lib = Library::load_from(root).unwrap();
        assert_eq!(titles(&lib).len(), 2, "{:?}", titles(&lib));
        assert!(folders_gone() && !folders_back());
        std::fs::rename(root.join("away"), root.join("far")).unwrap();
        assert!(folders_back() && !folders_gone());
        assert_eq!(titles(&Library::load_from(root).unwrap()), ["Omega Quest", "Zeta Run"]);
    }

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
    fn a_program_on_a_console_s_shelf_has_its_whole_case() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        for f in ["pc-covers/custom-zeta.png", "pc-covers/custom-zeta.back.jpg", "pc-covers/custom-zeta.spine.png", "pc-covers/custom-zeta.disc.webp", "pc-covers/custom-zeta-manual-01.jpg", "pc-console-covers/custom-omega.jpg", "pc-console-covers/custom-omega-spine.jpg", "pc-covers/custom-omega-manual.pdf"] {
            touch(&d.join(f));
        }
        let (cfg, sizes) = (Config::parse("{}").unwrap(), ScanData::default());
        let art = PcArt { cfg: &cfg, data: d, boxes: true, steam_ids: HashMap::new(), steam_infos: HashMap::new(), sizes: &sizes };
        let on = |id: &str| art.on_console(art.pc_game(PcGame { id: id.into(), source: "custom", title: "Zeta".into(), ..PcGame::default() })).art;
        let p = |f: &str| Some(d.join(f));
        let zeta = on("custom-zeta");
        assert_eq!((zeta.front, zeta.back, zeta.spine, zeta.disc), (p("pc-covers/custom-zeta.png"), p("pc-covers/custom-zeta.back.jpg"), p("pc-covers/custom-zeta.spine.png"), p("pc-covers/custom-zeta.disc.webp")));
        assert_eq!((zeta.manual, zeta.manual_pdf), (vec![d.join("pc-covers/custom-zeta-manual-01.jpg")], None));
        let omega = on("custom-omega");
        assert_eq!((omega.spine, omega.disc, omega.manual_pdf), (p("pc-console-covers/custom-omega-spine.jpg"), None, p("pc-covers/custom-omega-manual.pdf")));
    }

    #[test]
    fn pc_backs_in_order() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        for f in [
            "steam-art/20_backmade.jpg",
            "steam-art/21_backmade.jpg",
            "pc-covers-auto/steam-20.back.jpg",
            "pc-covers-auto/steam-21.back.jpg",
            "pc-covers/steam-21.back.made",
            "pc-covers-auto/steam-22.back.png",
            "pc-covers/steam-22.back.webp",
            "pc-console-covers/custom-zeta.jpg",
            "pc-covers-auto/custom-zeta.back.jpg",
            "pc-console-covers/custom-omega.jpg",
            "pc-console-covers/custom-omega-back.jpg",
            "pc-covers/custom-omega.back.png",
        ] {
            touch(&d.join(f));
        }
        let cfg = Config::parse("{}").unwrap();
        let sizes = ScanData::default();
        let art = PcArt { cfg: &cfg, data: d, boxes: true, steam_ids: HashMap::new(), steam_infos: HashMap::new(), sizes: &sizes };
        let back = |appid: &str| {
            let g = art.steam_game(SteamGame { appid: appid.into(), title: "Zeta".into(), size_bytes: 1, dir: None, minutes: 0, last_played: 0, front: Some("/s/f.jpg".into()), hero: None });
            (g.art.back.map(|b| b.strip_prefix(d).unwrap().to_string_lossy().replace('\\', "/")), g.art.made_back)
        };
        assert_eq!(back("20"), (Some("pc-covers-auto/steam-20.back.jpg".into()), false), "LaunchBox's over the made one");
        assert_eq!(back("21"), (Some("steam-art/21_backmade.jpg".into()), true), "Made-up back chosen");
        assert_eq!(back("22"), (Some("pc-covers/steam-22.back.webp".into()), false), "the player's own first");
        let custom = |id: &str| art.pc_game(PcGame { id: id.into(), source: "custom", title: "Zeta".into(), ..PcGame::default() }).art.back;
        assert_eq!(custom("custom-zeta"), None, "no Windows back on a console box");
        assert_eq!(custom("custom-omega"), Some(d.join("pc-covers/custom-omega.back.png")));
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
                { "title": "Ported", "exe": at("tools/a.exe"), "shelf": "xboxone" }, { "title": "No Emulator", "exe": format!(" {} ", at("tools/b.exe")), "shelf": "nes" },
                { "title": "A Recomp", "exe": at("tools/c.exe"), "shelf": "ports", "portOf": "n64" } ] },
            "systems": { "snes": { "romDirs": [at("snes")] }, "gc": { "enabled": false } },
            "games": { "custom-on-pc": { "title": "Renamed" } },
            "apps": { "stremio": { "enabled": true, "path": at("tools/a.exe") }, "nuvio": { "enabled": false, "path": at("tools/b.exe") } },
        });
        touch(&root.join("data/config.json"));
        std::fs::write(root.join("data/config.json"), cfg.to_string()).unwrap();
        let _one = ONE_LOAD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let lib = Library::load_from(root).unwrap();
        let shelves: Vec<(&str, Vec<&str>)> = lib.shelves.iter().map(|s| (s.id.as_str(), s.games.iter().map(|g| g.title.as_str()).collect())).collect();
        assert_eq!(shelves, [("snes", vec!["Zeta", "On SNES"]), ("nes", vec!["No Emulator"]), ("xboxone", vec!["Ported"]), ("pc", vec!["Renamed"]), ("custom", vec!["Console Off", "On Custom"]), ("ports", vec!["A Recomp"]), ("media", vec!["Stremio"])]);
    }

    #[test]
    fn recomps_found_go_on_their_console_s_shelf_when_asked() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        for f in ["tools/a.exe", "tools/b.exe", "tools/c.exe", "tools/d.exe", "n64/Omega (USA).z64"] {
            touch(&root.join(f));
        }
        let at = |f: &str| root.join(f).to_string_lossy().replace('\\', "/");
        let mut cfg = serde_json::json!({
            "steam": { "enabled": false },
            "pc": { "epic": false, "gog": false, "ubisoft": false, "ea": false, "xbox": false, "hoyoplay": false, "quiver": false, "custom": [
                { "title": "Zeta Run", "exe": at("tools/a.exe") }, { "title": "Placed", "exe": at("tools/b.exe"), "shelf": "pc" },
                { "title": "Ported", "exe": at("tools/c.exe"), "shelf": "ports", "portOf": "n64" }, { "title": "Snes Only", "exe": at("tools/d.exe") } ] },
            "systems": { "n64": { "romDirs": [at("n64")] } },
        });
        let hit = |sys: &str, name: &str| serde_json::json!({ "sys": sys, "name": name });
        let memo = serde_json::json!({ "custom-zeta-run|Zeta Run": hit("n64", "Zeta Run"), "custom-placed|Placed": hit("n64", "Placed"), "custom-ported|Ported": hit("n64", "Ported"), "custom-snes-only|Snes Only": hit("snes", "Snes Only") });
        touch(&root.join("data/config.json"));
        std::fs::write(root.join("data/recomp-detect.json"), memo.to_string()).unwrap();
        let _one = ONE_LOAD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let shelves = |cfg: &Value| {
            std::fs::write(root.join("data/config.json"), cfg.to_string()).unwrap();
            Library::load_from(root).unwrap().shelves.into_iter().map(|s| (s.id, s.games.into_iter().map(|g| g.title).collect::<Vec<_>>())).collect::<Vec<_>>()
        };
        let mine = |n64: &[&str], custom: &[&str]| -> Vec<(String, Vec<String>)> {
            let v = |l: &[&str]| l.iter().map(|x| x.to_string()).collect();
            vec![("n64".into(), v(n64)), ("pc".into(), v(&["Placed"])), ("custom".into(), v(custom)), ("ports".into(), v(&["Ported"]))]
        };
        assert_eq!(shelves(&cfg), mine(&["Omega"], &["Snes Only", "Zeta Run"]));
        cfg["pc"]["recompDetect"] = true.into();
        assert_eq!(shelves(&cfg), mine(&["Omega", "Zeta Run"], &["Snes Only"]), "no SNES shelf");
        cfg["games"] = serde_json::json!({ "custom-zeta-run": { "title": "Zeta 64" } });
        assert_eq!(shelves(&cfg), mine(&["Omega", "Zeta 64"], &["Snes Only"]), "found by its own title");
    }

    #[test]
    fn the_host_s_apps_on_the_game_streaming_shelf() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        for f in ["tools/moonlight", "boxart/b.png", "data/pc-covers/moonlight-alpha.png", "data/pc-covers-auto/moonlight-gamma.jpg", "data/pc-covers-auto/moonlight-beta.jpg"] {
            touch(&root.join(f));
        }
        let app = |name: &str, boxart: String| serde_json::json!({ "name": name, "id": 1, "hidden": false, "boxart": boxart });
        let boxart = url::Url::from_file_path(root.join("boxart/b.png")).unwrap().to_string();
        let mut cfg = serde_json::json!({
            "steam": { "enabled": false },
            "pc": { "epic": false, "gog": false, "ubisoft": false, "ea": false, "xbox": false, "hoyoplay": false, "quiver": false },
            "moonlight": { "enabled": true, "path": root.join("tools/moonlight"), "host": "pc", "apps": [app("Gamma", String::new()), app("Beta", boxart), app("Alpha", String::new()), app("Desktop", String::new())] },
            "games": { "moonlight-beta": { "title": "Beta Renamed" } },
        });
        touch(&root.join("data/config.json"));
        let _one = ONE_LOAD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let shelf = |cfg: &Value| {
            std::fs::write(root.join("data/config.json"), cfg.to_string()).unwrap();
            Library::load_from(root).unwrap().shelves.into_iter().find(|s| s.id == "streaming")
        };
        let s = shelf(&cfg).unwrap();
        let games: Vec<(&str, PathBuf)> = s.games.iter().map(|g| (g.title.as_str(), g.art.front.clone().unwrap())).collect();
        let d = root.join("data");
        assert_eq!(s.name, "Game streaming");
        assert_eq!(games, [("Alpha", d.join("pc-covers/moonlight-alpha.png")), ("Beta Renamed", root.join("boxart/b.png")), ("Gamma", d.join("pc-covers-auto/moonlight-gamma.jpg"))]);
        assert!(matches!(&s.games[0].source, Source::Pc(g) if g.source == "moonlight" && g.args == r#"stream "pc" "Alpha" --display-mode fullscreen"#));
        cfg["moonlight"]["enabled"] = false.into();
        assert!(shelf(&cfg).is_none());
    }
}
