//! Accounts: RetroAchievements (the account's name and web API key, whether each console's emulator is signed in
//! to it, and a game's achievements in Details), the player's SteamGridDB and Steam Web API keys and their
//! ScreenScraper account.

use std::sync::Arc;

use rustyshelf_core::ra::Ra;
use rustyshelf_core::screenscraper::{Creds, ScreenScraper};
use rustyshelf_core::steamach::SteamAch;
use rustyshelf_core::steamgrid::SteamGrid;
use rustyshelf_core::{raemu, seal};

use super::*;

/// What came back from the site, asked off the frame.
pub(super) enum RaAnswer {
    Tested(Result<String, String>),
    /// a game's achievements, by its id
    Progress(String, Result<Option<Value>, String>),
}

impl Settings {
    /// The account RetroAchievements is read through: its name and web API key, both empty when it's switched off.
    /// The key is opened from the settings where the Electron app sealed it.
    pub fn ra_account(&self) -> (String, String) {
        if !self.flag(&["ra", "enabled"], true) {
            return Default::default();
        }
        let plain = self.text(&["ra", "key"]);
        let key = if plain.is_empty() { self.seal_key.and_then(|k| seal::unseal(&self.text(&["ra", "keyEnc"]), &k)).unwrap_or_default() } else { plain };
        (self.text(&["ra", "user"]), key)
    }

    pub fn ra(&self) -> Arc<Ra> {
        self.ra.clone()
    }

    /// Keeps the web API key: sealed as the Electron app seals it where that can be done, else as it is.
    fn set_ra_key(&mut self, key: String) {
        let sealed = self.seal_key.filter(|_| !key.is_empty()).and_then(|k| seal::seal(&key, &k, seal::nonce()));
        match sealed {
            Some(s) => {
                self.set(&["ra", "key"], json!(""));
                self.set(&["ra", "keyEnc"], json!(s));
            }
            None => {
                if let Some(ra) = self.cfg.get_mut("ra").and_then(Value::as_object_mut) {
                    ra.remove("keyEnc");
                }
                self.set(&["ra", "key"], json!(key));
            }
        }
    }

    /// One of the player's own API keys ("steamgriddb" …), opened from where the Electron app sealed it.
    pub fn api_key(&self, name: &str) -> String {
        let plain = self.text(&["keys", name]);
        let key = if plain.trim().is_empty() { self.seal_key.and_then(|k| seal::unseal(&self.text(&["keysEnc", name]), &k)).unwrap_or_default() } else { plain };
        key.trim().to_string()
    }

    /// Keeps an API key as `set_ra_key` keeps RetroAchievements'.
    fn set_api_key(&mut self, name: &str, key: String) {
        let sealed = self.seal_key.filter(|_| !key.is_empty()).and_then(|k| seal::seal(&key, &k, seal::nonce()));
        match sealed {
            Some(s) => {
                self.set(&["keys", name], json!(""));
                self.set(&["keysEnc", name], json!(s));
            }
            None => {
                if let Some(enc) = self.cfg.get_mut("keysEnc").and_then(Value::as_object_mut) {
                    enc.remove(name);
                }
                self.set(&["keys", name], json!(key));
            }
        }
    }

    fn ask_ra(&mut self, work: impl FnOnce(&Ra) -> RaAnswer + Send + 'static) {
        let (ra, (user, key)) = (self.ra.clone(), self.ra_account());
        let (tx, rx) = std::sync::mpsc::channel();
        self.ra_answer = Some(Mutex::new(rx));
        std::thread::spawn(move || {
            ra.set_credentials(&user, &key);
            let _ = tx.send(work(&ra));
        });
    }

    /// What the site answered since the last frame.
    pub(super) fn poll_ra(&mut self) {
        let Some(answer) = self.ra_answer.as_ref().and_then(|rx| rx.lock().ok()?.try_recv().ok()) else { return };
        self.ra_answer = None;
        match answer {
            RaAnswer::Tested(Ok(said)) => self.say(said),
            RaAnswer::Tested(Err(why)) => self.say(why),
            RaAnswer::Progress(id, got) => self.achievements = Some((id, got)),
        }
        self.redraw = true;
    }

    pub(super) fn accounts_page(&self) -> Page {
        let (user, key) = self.ra_account();
        let typed_key = |s: &mut Settings| {
            s.edit(
                "Web API key",
                "From retroachievements.org → Settings → Keys. It's kept encrypted for this Windows user where the Electron app has run; a copied RustyShelf folder doesn't carry a usable key.",
                String::new(),
                Box::new(|s: &mut Settings, v: String| s.set_ra_key(v.trim().to_string())),
            );
        };
        let mut rows = vec![
            self.switch("RetroAchievements", &["ra", "enabled"], true, "Shows your progress on the shelves and in Details. Achievements are earned inside the emulator, signed in there."),
            self.typed("Username", &["ra", "user"], "Your name on retroachievements.org."),
            Row { label: "Web API key".into(), value: if key.is_empty() { "(none)".into() } else { "••••••••".into() }, hint: "Your web API key (not your password): retroachievements.org → Settings → Keys.".into(), press: Some(Box::new(typed_key)), step: None },
            Row {
                label: "Check the account".into(),
                hint: "Asks RetroAchievements for your profile with this name and key.".into(),
                press: Some(Box::new(move |s: &mut Settings| {
                    let (user, key) = (user.clone(), key.clone());
                    s.say("Asking RetroAchievements…".into());
                    s.ask_ra(move |ra| RaAnswer::Tested(ra.test(&user, &key).map(|p| format!("Signed in as {} · {} points", p["user"].as_str().unwrap_or(""), p["points"]))));
                })),
                ..Row::default()
            },
        ];
        // each console's emulator: whether achievements will work in it
        let mut real = self.cfg.clone();
        if let Some(systems) = real.get_mut("systems").and_then(Value::as_object_mut) {
            for sc in systems.values_mut() {
                if let Some(e) = sc.get("emulator").and_then(Value::as_str).filter(|e| !e.is_empty()) {
                    sc["emulator"] = json!(rustyshelf_core::launch::emu_path(e, &self.app_root).to_string_lossy());
                }
            }
        }
        for r in raemu::check(&real, &paths::env_var) {
            let name = systems().console(&r.system).map_or(r.system.clone(), |c| c.name.clone());
            let state = match r.state {
                "ok" => format!("Ready{}", if r.user.is_empty() { String::new() } else { format!(" ({})", r.user) }),
                "off" => "Off in the emulator".into(),
                "signed-out" => "Not signed in".into(),
                "other-user" => "Another account".into(),
                "no-emulator" => "No emulator".into(),
                "unsupported" => "Not covered".into(),
                _ => "Unknown".into(),
            };
            rows.push(Row { label: format!("{name} · {}", r.emulator), value: state, hint: r.detail, ..Row::default() });
        }
        let sgdb = self.api_key("steamgriddb");
        let typed_sgdb = |s: &mut Settings| {
            s.edit(
                "SteamGridDB key",
                "Free: sign in at steamgriddb.com, then Preferences → API. Kept encrypted like the RetroAchievements key.",
                String::new(),
                Box::new(|s: &mut Settings, v: String| s.set_api_key("steamgriddb", v.trim().to_string())),
            );
        };
        rows.push(Row {
            label: "SteamGridDB key".into(),
            value: if sgdb.is_empty() { "(none)".into() } else { "••••••••".into() },
            hint: "Community-made posters. With box art downloads on, PC games, ports and recomps that have no cover get one (their titles are sent to SteamGridDB). Optional.".into(),
            press: Some(Box::new(typed_sgdb)),
            step: None,
        });
        rows.push(Row {
            label: "Check the SteamGridDB key".into(),
            hint: "Asks SteamGridDB whether it takes this key.".into(),
            press: Some(Box::new(move |s: &mut Settings| {
                let (key, dir) = (sgdb.clone(), paths::data_dir(&s.app_root));
                s.say("Asking SteamGridDB…".into());
                let (tx, rx) = std::sync::mpsc::channel();
                s.ra_answer = Some(Mutex::new(rx));
                std::thread::spawn(move || {
                    let said = SteamGrid::new(&dir).test(&key).map(|()| "SteamGridDB: the key works".to_string()).map_err(|why| format!("SteamGridDB: {why}"));
                    let _ = tx.send(RaAnswer::Tested(said));
                });
            })),
            ..Row::default()
        });
        let steamweb = self.api_key("steamweb");
        let typed_steam = |s: &mut Settings| {
            s.edit(
                "Steam Web API key",
                "Free: steamcommunity.com/dev/apikey (any domain name will do). Kept encrypted like the RetroAchievements key.",
                String::new(),
                Box::new(|s: &mut Settings, v: String| s.set_api_key("steamweb", v.trim().to_string())),
            );
        };
        rows.push(Row {
            label: "Steam achievements".into(),
            value: if steamweb.is_empty() { "(none)".into() } else { "••••••••".into() },
            hint: "Your own Steam Web API key: the Trophies screen then shows your Steam achievements beside RetroAchievements, for the account signed in to Steam on this PC. The key is sent only to api.steampowered.com. Optional.".into(),
            press: Some(Box::new(typed_steam)),
            step: None,
        });
        rows.push(Row {
            label: "Check the Steam key".into(),
            hint: "Asks Steam for the profile of the account signed in here, with this key.".into(),
            press: Some(Box::new(move |s: &mut Settings| {
                let (key, dir, given) = (steamweb.clone(), paths::data_dir(&s.app_root), s.text(&["steam", "path"]));
                s.say("Asking Steam…".into());
                let (tx, rx) = std::sync::mpsc::channel();
                s.ra_answer = Some(Mutex::new(rx));
                std::thread::spawn(move || {
                    let r = SteamAch::new(&dir, &key, rustyshelf_core::steam::main_root(&given)).test(&key);
                    let said = if r["ok"] == true { Ok(format!("Steam: the key works · {}", r["user"].as_str().unwrap_or(""))) } else { Err(format!("Steam: {}", r["error"].as_str().unwrap_or("not accepted"))) };
                    let _ = tx.send(RaAnswer::Tested(said));
                });
            })),
            ..Row::default()
        });
        let ss = Creds { user: self.api_key("ssUser"), pass: self.api_key("ssPass"), dev_id: self.api_key("ssDevId"), dev_pass: self.api_key("ssDevPass") };
        for (label, name, hint, secret) in [
            ("ScreenScraper username", "ssUser", "Your screenscraper.fr username. Box scans by region for the art sources that use it. Optional, kept encrypted.", false),
            ("ScreenScraper password", "ssPass", "Your screenscraper.fr password. Kept encrypted, sent only to ScreenScraper.", true),
            ("ScreenScraper developer ID", "ssDevId", "Only if ScreenScraper gave you a developer login of your own (devid). Left blank, your username is used.", false),
            ("ScreenScraper developer password", "ssDevPass", "The developer password (devpassword) that goes with your developer ID. Kept encrypted.", true),
        ] {
            let now = self.api_key(name);
            let typed = move |s: &mut Settings| {
                let old = if secret { String::new() } else { s.api_key(name) };
                s.edit(label, hint, old, Box::new(move |s: &mut Settings, v: String| s.set_api_key(name, v.trim().to_string())));
            };
            rows.push(Row { label: label.into(), value: if now.is_empty() { "(none)".into() } else if secret { "••••••••".into() } else { now }, hint: hint.into(), press: Some(Box::new(typed)), step: None });
        }
        rows.push(Row {
            label: "Check the ScreenScraper account".into(),
            hint: "Signs in and shows how many lookups the account has left today. ScreenScraper only answers apps registered with it: without a developer login it says so.".into(),
            press: Some(Box::new(move |s: &mut Settings| {
                let (creds, dir) = (ss.clone(), paths::data_dir(&s.app_root));
                s.say("Asking ScreenScraper…".into());
                let (tx, rx) = std::sync::mpsc::channel();
                s.ra_answer = Some(Mutex::new(rx));
                std::thread::spawn(move || {
                    let site = ScreenScraper::new(&dir);
                    site.set_creds(creds);
                    let said = match site.test() {
                        Ok(Some(left)) => Ok(format!("ScreenScraper: working · {left} lookups left today")),
                        Ok(None) => Ok("ScreenScraper: working".to_string()),
                        Err(why) => Err(format!("ScreenScraper: {why}")),
                    };
                    let _ = tx.send(RaAnswer::Tested(said));
                });
            })),
            ..Row::default()
        });
        Page { title: "Accounts".into(), lead: "RetroAchievements: your progress on the shelves that it covers. SteamGridDB: posters for PC games without a cover. Steam: your achievements on the Trophies screen. ScreenScraper: box scans by region.".into(), rows }
    }

    /// Asks for the chosen game's achievements (again, when `force`).
    pub(super) fn load_achievements(&mut self, force: bool) {
        let Some(c) = &self.chosen else { return };
        let (shelf, title, id) = (c.console.clone(), c.game.title.clone(), c.game.id.clone());
        self.achievements = None;
        if let Some(appid) = self.steam_app().map(String::from) {
            let given = self.text(&["steam", "path"]);
            let (tx, rx) = std::sync::mpsc::channel();
            self.ra_answer = Some(Mutex::new(rx));
            std::thread::spawn(move || {
                let root = rustyshelf_core::steam::main_root(&given);
                let _ = tx.send(RaAnswer::Progress(id, Ok(root.and_then(|r| rustyshelf_core::pcach::steam_progress(&r, &appid)))));
            });
            return;
        }
        self.ask_ra(move |ra| RaAnswer::Progress(id, ra.progress(&shelf, &title, force)));
    }

    /// The chosen game's achievements in a line, for Details; None on a shelf RetroAchievements doesn't cover
    /// or without an account.
    pub(super) fn achievements_line(&self) -> Option<String> {
        let c = self.chosen.as_ref()?;
        if self.steam_app().is_some() {
            // (a Steam game with none kept here has no row at all)
            let p = self.achievements.as_ref().filter(|a| a.0 == c.game.id)?.1.as_ref().ok()?.as_ref()?;
            return Some(format!("{} of {}{}", p["earned"], p["total"], if p["mastered"] == "completed" { " · Completed" } else { "" }));
        }
        rustyshelf_core::ra::console_id(&c.console)?;
        if self.ra_account().1.is_empty() {
            return None;
        }
        Some(match self.achievements.as_ref().filter(|a| a.0 == c.game.id) {
            None => "…".into(),
            Some((_, Ok(Some(p)))) => format!("{} of {} · {} of {} points{}", p["earned"], p["total"], p["pointsEarned"], p["points"], if p["mastered"] == "mastered" { " · Mastered" } else { "" }),
            Some((_, Ok(None))) => "No set for this game".into(),
            Some((_, Err(_))) => "Couldn't be read".into(),
        })
    }

    /// The chosen game's Steam app, when Steam starts it.
    fn steam_app(&self) -> Option<&str> {
        match &self.chosen.as_ref()?.game.source {
            Source::Steam { appid, .. } => Some(appid),
            _ => None,
        }
    }

    pub(super) fn achievements_page(&self) -> Page {
        const SORTS: [&str; 3] = ["Order", "Rarity", "Points"];
        let title = self.chosen.as_ref().map_or(String::new(), |c| c.game.title.clone());
        let steam = self.steam_app().is_some();
        let again = if steam { "Reads your progress again from what Steam keeps on this PC." } else { "Reads your progress from RetroAchievements now." };
        let sort = self.ach_sort % SORTS.len();
        let mut rows = vec![Row { label: "Ask again".into(), hint: again.into(), press: Some(Box::new(|s: &mut Settings| s.load_achievements(true))), ..Row::default() }];
        let lead = match self.achievements.as_ref().map(|a| &a.1) {
            None if steam => "Reading Steam's files…".into(),
            None => "Asking RetroAchievements…".into(),
            Some(Err(why)) => why.clone(),
            Some(Ok(None)) if steam => "Steam keeps no achievements for this game on this PC yet.".into(),
            Some(Ok(None)) => "RetroAchievements has no set for this game.".into(),
            Some(Ok(Some(p))) => {
                let all: Vec<&Value> = p["achievements"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
                if !steam {
                    rows.push(Row {
                        label: "Sort".into(),
                        value: SORTS[sort].into(),
                        hint: "◀ ▶ Order (story first), Rarity (the rarest first) or Points.".into(),
                        press: Some(Box::new(|s: &mut Settings| s.sort_achievements(1))),
                        step: Some(Box::new(|s: &mut Settings, by| s.sort_achievements(by))),
                    });
                }
                for (label, list) in achievement_sections(&all, sort) {
                    if list.is_empty() {
                        continue;
                    }
                    rows.push(Row { label: format!("— {label} · {} —", list.len()), ..Row::default() });
                    rows.extend(list.into_iter().map(|a| achievement_row(a, steam)));
                }
                if steam {
                    format!("{} of {} earned, as Steam keeps them on this PC", p["earned"], p["total"])
                } else {
                    format!("{} of {} earned · {} of {} points", p["earned"], p["total"], p["pointsEarned"], p["points"])
                }
            }
        };
        Page { title: format!("Achievements · {title}"), lead, rows }
    }

    fn sort_achievements(&mut self, by: i32) {
        self.ach_sort = (self.ach_sort as i32 + by).rem_euclid(3) as usize;
        self.redraw = true;
    }
}

/// How rare an achievement is, from the share of players who have it.
fn tier(rarity: Option<f64>) -> Option<&'static str> {
    Some(match rarity? {
        r if r < 3.0 => "Ultra rare",
        r if r < 10.0 => "Rare",
        r if r < 40.0 => "Uncommon",
        _ => "Common",
    })
}

fn kind(a: &Value) -> &'static str {
    match a["type"].as_str().map(str::to_lowercase).as_deref() {
        Some("progression") => "Story",
        Some("win_condition") => "Beats the game",
        Some("missable") => "Missable",
        _ => "",
    }
}

/// The list in sections, as the Electron app's wall: what you've unlocked first (newest first), then the rest by
/// story and missable (Order), by rarity tier (Rarity) or by points.
fn achievement_sections<'a>(all: &[&'a Value], sort: usize) -> Vec<(&'static str, Vec<&'a Value>)> {
    let mut got: Vec<&Value> = all.iter().copied().filter(|a| a["earned"] == true).collect();
    got.sort_by(|a, b| b["date"].as_str().unwrap_or("").cmp(a["date"].as_str().unwrap_or("")));
    let mut rest: Vec<&Value> = all.iter().copied().filter(|a| a["earned"] != true).collect();
    let rarity = |a: &Value| a["rarity"].as_f64().unwrap_or(101.0);
    match sort {
        1 => rest.sort_by(|a, b| rarity(a).total_cmp(&rarity(b))),
        2 => rest.sort_by(|a, b| b["points"].as_f64().unwrap_or(0.0).total_cmp(&a["points"].as_f64().unwrap_or(0.0))),
        _ => {}
    }
    let mut out = vec![("Your trophies", got)];
    match sort {
        0 => {
            let (story, others): (Vec<&Value>, Vec<&Value>) = rest.into_iter().partition(|a| matches!(kind(a), "Story" | "Beats the game"));
            let (miss, others): (Vec<&Value>, Vec<&Value>) = others.into_iter().partition(|a| kind(a) == "Missable");
            let split = !story.is_empty() || !miss.is_empty();
            out.extend([("Story", story), ("Missable", miss), (if split { "Everything else" } else { "Still to unlock" }, others)]);
        }
        1 => {
            for t in ["Ultra rare", "Rare", "Uncommon", "Common"] {
                out.push((t, rest.iter().copied().filter(|a| tier(a["rarity"].as_f64()) == Some(t)).collect()));
            }
            out.push(("No rarity yet", rest.into_iter().filter(|a| a["rarity"].as_f64().is_none()).collect()));
        }
        _ => out.push(("Still to unlock", rest)),
    }
    out
}

fn achievement_row(a: &Value, steam: bool) -> Row {
    let earned = a["earned"] == true;
    let rarity = a["rarity"].as_f64();
    let value = if steam { if earned { "Earned" } else { "" }.into() } else { format!("{}{}", a["points"], if a["hardcore"] == true { " · hardcore" } else { "" }) };
    let hint = [
        a["desc"].as_str().unwrap_or("").to_string(),
        kind(a).to_string(),
        a["date"].as_str().map_or(String::new(), |d| format!("Earned {d}")),
        rarity.map_or(String::new(), |r| format!("{r}% of players have it")),
        tier(rarity).map_or(String::new(), String::from),
    ]
    .into_iter()
    .filter(|t| !t.is_empty())
    .collect::<Vec<_>>()
    .join(" · ");
    let mark = match tier(rarity) {
        Some("Ultra rare") => "◆ ",
        Some("Rare") => "◇ ",
        _ => "",
    };
    Row { label: format!("{} {mark}{}", if earned { "★" } else { "☆" }, a["title"].as_str().unwrap_or("")), value, hint, ..Row::default() }
}
