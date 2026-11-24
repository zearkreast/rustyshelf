// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the pages and
// their wording follow Add games in Boxshelf's renderer/settings.js, as RustyShelf has it.
//! Settings → Custom games → adding many at once: several programs picked together, a folder of games scanned,
//! and a Playnite library brought over (experimental). What's found is listed with a tick each; a game already
//! on the Custom shelf isn't added twice.

use std::collections::HashSet;

use rustyshelf_core::addgames::{self, Playnite, Scan};
use rustyshelf_core::{pccovers, pcgames};

use super::custom::base36;
use super::*;

struct Item {
    title: String,
    exe: String,
    on: bool,
    /// already on the Custom shelf
    have: bool,
    group: String,
    cover: Option<PathBuf>,
}

/// What Custom games keeps of games being added.
#[derive(Default)]
pub(super) struct Adding {
    several: Option<Later<Vec<PathBuf>>>,
    scanning: Option<Later<Scan>>,
    list: Vec<Item>,
    /// the folders a scan got no game from, each with why
    skipped: Vec<(String, &'static str)>,
    /// Playnite, once looked for; its library being read; what it comes to, and which of its parts are ticked
    /// ("pc" and consoles' ids)
    playnite: Option<Option<Playnite>>,
    reading: Option<Later<Result<Value, String>>>,
    plan: Option<Value>,
    ticked: HashSet<String>,
}

fn plural(n: usize, one: &str) -> String {
    format!("{n} {one}{}", plural_s(n))
}

impl Settings {
    fn customs(&self) -> Vec<Value> {
        at(&self.cfg, &["pc", "custom"]).and_then(Value::as_array).cloned().unwrap_or_default()
    }

    /// The programs already on the Custom shelf, in lower case.
    fn programs(&self) -> HashSet<String> {
        self.customs().iter().filter_map(|c| c.get("exe").and_then(Value::as_str)).map(str::to_lowercase).collect()
    }

    fn found(&mut self, items: Vec<(String, String, String, Option<PathBuf>)>) {
        let have = self.programs();
        self.adding.plan = None;
        self.adding.list = items.into_iter().map(|(title, exe, group, cover)| { let h = have.contains(&exe.to_lowercase()); Item { title: title.chars().take(120).collect(), on: !h, have: h, exe, group, cover } }).collect();
        if self.adding.list.is_empty() && self.adding.skipped.is_empty() {
            self.say("No games found there".into());
        } else {
            self.go(Screen::AddGames);
        }
    }

    pub(super) fn pick_several(&mut self) {
        self.adding.several = Some(Later::spawn(|| crate::dialog::pick_files("Choose the games' programs (hold Ctrl to pick several)")));
    }

    pub(super) fn scan_games(&mut self, root: &Path) {
        let root = root.to_path_buf();
        self.adding.scanning = Some(Later::spawn(move || addgames::scan_folder(&root, 2000)));
        self.say("Looking…".into());
    }

    /// Reads Playnite's library, from the folder chosen or from where it was found.
    pub(super) fn read_playnite(&mut self, dir: Option<&Path>) {
        let Some(found) = addgames::find(dir, &paths::env_var) else {
            self.say("Playnite wasn't found. Choose its folder (the one with Playnite.DesktopApp.exe).".into());
            return;
        };
        self.adding.playnite = Some(Some(found.clone()));
        self.adding.reading = Some(Later::spawn(move || addgames::read(&found).map(|lib| addgames::plan(&lib, &found))));
        self.say("Reading Playnite's library…".into());
    }

    /// What the dialogs and the threads came back with since the last frame.
    pub(super) fn poll_adding(&mut self) {
        if let Some(files) = self.adding.several.as_ref().and_then(Later::poll) {
            self.adding.several = None;
            if !files.is_empty() {
                self.adding.skipped.clear();
                self.found(files.iter().map(|f| (addgames::title_for(f, None), f.to_string_lossy().into_owned(), String::new(), None)).collect());
            }
        }
        if let Some(scan) = self.adding.scanning.as_ref().and_then(Later::poll) {
            self.adding.scanning = None;
            self.note.clear();
            if let Some(why) = scan.error {
                self.say(why);
                return;
            }
            self.adding.skipped = scan.skipped.into_iter().map(|s| (s.1, s.2)).collect();
            self.found(scan.games.into_iter().map(|g| (g.title, g.exe.to_string_lossy().into_owned(), g.group.unwrap_or_default(), g.cover)).collect());
        }
        if let Some(read) = self.adding.reading.as_ref().and_then(Later::poll) {
            self.adding.reading = None;
            match read {
                Ok(plan) => {
                    self.adding.ticked = ["pc".to_string()].into_iter().chain(plan["consoles"].as_object().into_iter().flat_map(|c| c.keys().cloned())).collect();
                    self.adding.plan = Some(plan);
                    self.note.clear();
                    self.go(Screen::Playnite);
                }
                Err(why) => self.say(why),
            }
        }
    }

    /// Puts the ticked games on the Custom shelf, each with the picture found in its folder.
    fn add_ticked(&mut self) {
        let (mut list, mut have) = (self.customs(), self.programs());
        let now = u128::from(rustyshelf_core::now_ms());
        let data = paths::data_dir(&self.app_root);
        let mut n = 0;
        for x in std::mem::take(&mut self.adding.list).into_iter().filter(|x| x.on && !x.have) {
            if !have.insert(x.exe.to_lowercase()) {
                continue;
            }
            // the id the Electron app gives one of several: the time in base 36, and its place among them
            let id = format!("{}-{n}", base36(now));
            if let Some(cover) = &x.cover {
                let _ = pccovers::own(&data, &format!("custom-{}", pcgames::slug(&id)), cover);
            }
            list.push(json!({ "id": id, "title": x.title, "exe": x.exe, "args": "" }));
            n += 1;
        }
        self.adding.skipped.clear();
        self.set(&["pc", "custom"], json!(list));
        self.back();
        self.say(if n > 0 { format!("Added {} to the Custom shelf", plural(n, "game")) } else { "Nothing new to add".into() });
    }

    /// The rows Custom games starts with: the ways to add games.
    pub(super) fn adding_rows(&self) -> Vec<Row> {
        let mut rows = vec![
            Row {
                label: "Add several programs".into(),
                value: "Choose…".into(),
                hint: "Pick many games' programs at once (hold Ctrl or Shift in the file window). You check their names before they're added.".into(),
                press: Some(Box::new(|s: &mut Settings| s.pick_several())),
                step: None,
            },
            Row {
                label: if self.adding.scanning.is_some() { "Looking…" } else { "Scan a folder" }.into(),
                value: "Choose folder…".into(),
                hint: "Point at the folder your games are in (like D:\\Games). Each game folder in it becomes one game, its main program picked for you; you tick which ones to add.".into(),
                press: Some(Box::new(|s: &mut Settings| s.pick(Pick::ScanGames, "Choose the folder your games are in (like D:\\Games)", true))),
                step: None,
            },
        ];
        if cfg!(windows) {
            let found = addgames::find(None, &paths::env_var);
            let known = found.is_some();
            rows.push(Row {
                label: if self.adding.reading.is_some() { "Reading Playnite's library…" } else { "Import from Playnite (experimental)" }.into(),
                value: if known { "Import…" } else { "Find…" }.into(),
                hint: "Brings your Playnite library over in one go: games you added there by hand, with their covers, onto the Custom shelf, and your emulated games' folders onto their console's shelf. Steam, Epic, GOG Galaxy and other store games are left out: RustyShelf reads those stores itself. Experimental: it couldn't be tried with every Playnite version.".into(),
                press: Some(Box::new(move |s: &mut Settings| if known { s.read_playnite(None) } else { s.pick(Pick::Playnite, "Choose Playnite's folder (the one with Playnite.DesktopApp.exe)", true) })),
                step: None,
            });
        }
        let imported = self.customs().iter().filter(|c| c["src"] == "playnite").count();
        if imported > 0 {
            rows.push(Row {
                label: "Remove what Playnite brought".into(),
                value: plural(imported, "game"),
                hint: "Takes the games imported from Playnite off the Custom shelf, and the games folders it added off their consoles. Nothing is deleted.".into(),
                press: Some(Box::new(|s: &mut Settings| s.undo_playnite())),
                step: None,
            });
        }
        rows
    }

    pub(super) fn add_games_page(&self) -> Page {
        let list = &self.adding.list;
        let ticked = list.iter().filter(|x| x.on && !x.have).count();
        let any_off = list.iter().any(|x| !x.on && !x.have);
        let mut rows = vec![
            Row {
                label: if any_off { "Select all" } else { "Deselect all" }.into(),
                hint: "Ticks or unticks every game found that isn't already on the shelf.".into(),
                press: Some(Box::new(move |s: &mut Settings| {
                    s.adding.list.iter_mut().filter(|x| !x.have).for_each(|x| x.on = any_off);
                    s.redraw = true;
                })),
                ..Row::default()
            },
            Row {
                label: "Add the ticked games".into(),
                value: if ticked > 0 { format!("Add {ticked}") } else { "Add".into() },
                hint: "Puts the ticked games on the Custom shelf. Covers are found for them like for any PC game, and each can be renamed afterwards.".into(),
                press: Some(Box::new(|s: &mut Settings| s.add_ticked())),
                step: None,
            },
        ];
        for (i, x) in list.iter().enumerate() {
            let flip = move |s: &mut Settings| {
                if let Some(x) = s.adding.list.get_mut(i).filter(|x| !x.have) {
                    x.on = !x.on;
                }
                s.redraw = true;
            };
            rows.push(Row {
                label: x.title.clone(),
                value: if x.have { "Already on the Custom shelf".into() } else { on_off(x.on) },
                hint: if x.group.is_empty() { x.exe.clone() } else { format!("{} · {}", x.group, x.exe) },
                press: Some(Box::new(flip)),
                step: Some(Box::new(move |s: &mut Settings, _| flip(s))),
            });
        }
        let mut lead = format!("Found {} · {ticked} ticked.", plural(list.len(), "game"));
        if !self.adding.skipped.is_empty() {
            let named: Vec<String> = self.adding.skipped.iter().take(6).map(|s| format!("{} ({})", s.0, s.1)).collect();
            lead = format!("{lead} {} gave no game: {}{}.", plural(self.adding.skipped.len(), "folder"), named.join(", "), if self.adding.skipped.len() > 6 { " …" } else { "" });
        }
        Page { title: "Add games".into(), lead, rows }
    }

    pub(super) fn playnite_page(&self) -> Page {
        let Some(plan) = &self.adding.plan else { return Page { title: "Playnite".into(), lead: "Nothing read yet.".into(), rows: Vec::new() } };
        let tick = |id: String, label: String, hint: String| {
            let on = self.adding.ticked.contains(&id);
            let flip = move |s: &mut Settings| {
                if !s.adding.ticked.remove(&id) {
                    s.adding.ticked.insert(id.clone());
                }
                s.redraw = true;
            };
            let again = flip.clone();
            Row { label, value: on_off(on), hint, press: Some(Box::new(flip)), step: Some(Box::new(move |s: &mut Settings, _| again(s))) }
        };
        let pc = plan["pc"].as_array().map(Vec::as_slice).unwrap_or_default();
        let there = pc.iter().filter(|g| g["there"] != false).count();
        let mut rows = vec![tick("pc".into(), format!("PC games: {there}"), format!("Games added to Playnite by hand, with their covers, onto the Custom shelf.{}", if there < pc.len() { format!(" {} more aren't installed where Playnite says.", pc.len() - there) } else { String::new() }))];
        for (id, c) in plan["consoles"].as_object().into_iter().flatten() {
            let Some(console) = systems().console(id) else { continue };
            let dirs: Vec<&str> = c["dirs"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            rows.push(tick(id.clone(), format!("{}: {} in {}", console.name, plural(c["count"].as_u64().unwrap_or(0) as usize, "game"), plural(dirs.len(), "folder")), format!("Added as games folders of {}: {}", console.name, dirs.join(", "))));
        }
        rows.push(Row {
            label: "Import".into(),
            hint: "Adds what's ticked. Remove what Playnite brought, in Custom games, takes it all off again.".into(),
            press: Some(Box::new(|s: &mut Settings| s.import_playnite())),
            ..Row::default()
        });
        let left = &plan["skipped"];
        let out: Vec<String> = [("store", "from stores RustyShelf reads itself"), ("hidden", "hidden in Playnite"), ("link", "started by a link"), ("nothing", "with nothing to start, or for a console RustyShelf hasn't")]
            .into_iter()
            .filter_map(|(k, what)| left[k].as_u64().filter(|n| *n > 0).map(|n| format!("{n} {what}")))
            .collect();
        let lead = format!("{} in Playnite's library.{}", plural(plan["total"].as_u64().unwrap_or(0) as usize, "game"), if out.is_empty() { String::new() } else { format!(" Left out: {}.", out.join(", ")) });
        Page { title: "Import from Playnite".into(), lead, rows }
    }

    fn import_playnite(&mut self) {
        let Some(plan) = self.adding.plan.take() else { return };
        let (mut list, mut have) = (self.customs(), self.programs());
        let data = paths::data_dir(&self.app_root);
        let mut favorites = self.list(&["ui", "favorites"]);
        let (mut n, mut folders) = (0, 0);
        if self.adding.ticked.contains("pc") {
            for g in plan["pc"].as_array().into_iter().flatten().filter(|g| g["there"] != false) {
                let (exe, key) = (g["exe"].as_str().unwrap_or(""), g["key"].as_str().unwrap_or(""));
                if !have.insert(exe.to_lowercase()) {
                    continue;
                }
                let id = format!("pn-{}", pcgames::slug(key).chars().take(40).collect::<String>());
                let shelf_id = format!("custom-{}", pcgames::slug(&id));
                list.push(json!({ "id": id, "title": g["title"], "exe": exe, "args": g["args"].as_str().unwrap_or(""), "src": "playnite" }));
                n += 1;
                if let Some(cover) = g["cover"].as_str() {
                    let _ = pccovers::own(&data, &shelf_id, Path::new(cover));
                }
                if g["favorite"] == true && !favorites.contains(&shelf_id) {
                    favorites.push(shelf_id);
                }
            }
        }
        self.set(&["pc", "custom"], json!(list));
        self.set(&["ui", "favorites"], json!(favorites));
        // the folders added are remembered, so they can be taken off again
        let mut added = at(&self.cfg, &["pc", "pnDirs"]).and_then(Value::as_object).cloned().unwrap_or_default();
        for (id, c) in plan["consoles"].as_object().into_iter().flatten() {
            let Some(console) = systems().console(id).filter(|_| self.adding.ticked.contains(id)) else { continue };
            let mut dirs = self.list(&["systems", id, "romDirs"]);
            for d in c["dirs"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                if !dirs.iter().any(|x| x.to_lowercase() == d.to_lowercase()) {
                    dirs.push(d.to_string());
                    let mine = added.entry(id.clone()).or_insert_with(|| json!([]));
                    if let Some(mine) = mine.as_array_mut() {
                        mine.push(json!(d));
                    }
                    folders += 1;
                }
            }
            self.set(&["systems", id, "romDirs"], json!(dirs));
            self.set(&["systems", id, "enabled"], json!(true));
            let emulator_dir = c["emulatorDir"].as_str().unwrap_or("");
            if self.text(&["systems", id, "emulator"]).is_empty() && !emulator_dir.is_empty() {
                if let Some(exe) = emulator_in(Path::new(emulator_dir), console, 2) {
                    self.set(&["systems", id, "emulator"], json!(exe.to_string_lossy()));
                }
            }
        }
        self.set(&["pc", "pnDirs"], Value::Object(added));
        self.back();
        self.say(format!("From Playnite: {} on the Custom shelf{}", plural(n, "game"), if folders > 0 { format!(", {} for your consoles", plural(folders, "games folder")) } else { String::new() }));
    }

    fn undo_playnite(&mut self) {
        let list = self.customs();
        let kept: Vec<&Value> = list.iter().filter(|c| c["src"] != "playnite").collect();
        let n = list.len() - kept.len();
        self.set(&["pc", "custom"], json!(kept));
        let mut folders = 0;
        for (id, mine) in at(&self.cfg, &["pc", "pnDirs"]).and_then(Value::as_object).cloned().unwrap_or_default() {
            let gone: HashSet<String> = mine.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_lowercase).collect();
            let dirs = self.list(&["systems", &id, "romDirs"]);
            let left: Vec<&String> = dirs.iter().filter(|d| !gone.contains(&d.to_lowercase())).collect();
            folders += dirs.len() - left.len();
            if left.len() < dirs.len() {
                self.set(&["systems", &id, "romDirs"], json!(left));
            }
        }
        self.set(&["pc", "pnDirs"], json!({}));
        self.say(format!("Removed {}{}", plural(n, "imported game"), if folders > 0 { format!(" and {}", plural(folders, "games folder")) } else { String::new() }));
    }
}

/// A console's emulator inside a folder Playnite named: one of its own program names, `left` folders deep at most.
fn emulator_in(dir: &Path, console: &Console, left: u32) -> Option<PathBuf> {
    let entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
    let named = |name: &str| console.exe_names.iter().any(|n| n.to_lowercase() == name.to_lowercase());
    entries.iter().find(|e| e.path().is_file() && named(&e.file_name().to_string_lossy())).map(|e| e.path()).or_else(|| if left > 0 { entries.iter().filter(|e| e.path().is_dir()).find_map(|e| emulator_in(&e.path(), console, left - 1)) } else { None })
}
