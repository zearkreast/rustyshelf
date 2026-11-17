//! Accounts: RetroAchievements (the account's name and web API key, whether each console's emulator is signed in
//! to it, and a game's achievements in Details), the player's SteamGridDB and Steam Web API keys and their
//! ScreenScraper account. Xbox 360 achievements and PS3 trophies come from Xenia's and RPCS3's own files instead.

use std::sync::Arc;

use rustyshelf_core::ra::Ra;
use rustyshelf_core::screenscraper::{Creds, ScreenScraper};
use rustyshelf_core::steamach::SteamAch;
use rustyshelf_core::steamgrid::SteamGrid;
use rustyshelf_core::{accountenv, raemu, rpcs3troph, seal, xeniaach};

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
        (self.ra_user(), self.ra_key())
    }

    fn ra_user(&self) -> String {
        self.account_env.get("ra.user").cloned().unwrap_or_else(|| self.text(&["ra", "user"]))
    }

    fn ra_key(&self) -> String {
        if let Some(k) = self.account_env.get("ra.key") {
            return k.clone();
        }
        let plain = self.text(&["ra", "key"]);
        if plain.is_empty() { self.seal_key.and_then(|k| seal::unseal(&self.text(&["ra", "keyEnc"]), &k)).unwrap_or_default() } else { plain }
    }

    /// The accounts in `config.json` that `data/.env` doesn't have yet are written there, then it's read.
    pub(super) fn keep_accounts_in_env(&mut self) {
        let data = paths::data_dir(&self.app_root);
        self.account_env = accountenv::read(&data);
        for (name, _) in accountenv::NAMES {
            if self.account_env.contains_key(name) {
                continue;
            }
            let v = match name {
                "ra.user" => self.text(&["ra", "user"]),
                "ra.key" => self.ra_key(),
                _ => self.api_key(name),
            };
            if !v.trim().is_empty() && accountenv::write(&data, name, &v).is_ok() {
                self.account_env.insert(name.to_string(), v.trim().to_string());
            }
        }
    }

    /// Writes one account value to `data/.env` too; a failure is said.
    fn env_keep(&mut self, name: &str, value: &str) {
        match accountenv::write(&paths::data_dir(&self.app_root), name, value) {
            Ok(()) if value.trim().is_empty() => {
                self.account_env.remove(name);
            }
            Ok(()) => {
                self.account_env.insert(name.to_string(), value.trim().to_string());
            }
            Err(e) => self.say(format!("Couldn't write the accounts file (.env): {e}")),
        }
    }

    fn set_ra_user(&mut self, user: String) {
        self.set(&["ra", "user"], json!(user));
        self.env_keep("ra.user", &user);
    }

    pub fn ra(&self) -> Arc<Ra> {
        self.ra.clone()
    }

    /// Keeps the web API key: sealed as the Electron app seals it where that can be done, else as it is.
    fn set_ra_key(&mut self, key: String) {
        self.env_keep("ra.key", &key);
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
        if let Some(v) = self.account_env.get(name) {
            return v.clone();
        }
        let plain = self.text(&["keys", name]);
        let key = if plain.trim().is_empty() { self.seal_key.and_then(|k| seal::unseal(&self.text(&["keysEnc", name]), &k)).unwrap_or_default() } else { plain };
        key.trim().to_string()
    }

    /// Keeps an API key as `set_ra_key` keeps RetroAchievements'.
    pub(super) fn set_api_key(&mut self, name: &str, key: String) {
        self.env_keep(name, &key);
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

    /// Settings → Accounts: one row per service, each opening its own page.
    pub(super) fn accounts_page(&self) -> Page {
        let set = |v: &str| if v.is_empty() { "Not set up".to_string() } else { "Ready".to_string() };
        let open = |label: &str, value: String, hint: &str, page: &'static str| Row { label: label.into(), value, hint: hint.into(), press: Some(Box::new(move |s: &mut Settings| s.go(Screen::Account(page)))), step: None };
        let (user, key) = self.ra_account();
        let ra = if !self.flag(&["ra", "enabled"], true) { "Off".to_string() } else if user.is_empty() || key.is_empty() { "Not set up".into() } else { user };
        let ss = self.api_key("ssUser");
        let rows = vec![
            open("RetroAchievements", ra, "Your progress on the shelves it covers, and the emulators signed in to it.", "ra"),
            open("Steam", set(&self.api_key("steamweb")), "A Steam Web API key: your Steam achievements on the Trophies screen.", "steam"),
            open("SteamGridDB", set(&self.api_key("steamgriddb")), "A key for community-made posters, for PC games without a cover.", "sgdb"),
            open("IGDB", set(&self.api_key("igdbId")), "A Twitch app's Client ID and Secret: more screenshots and game info.", "igdb"),
            open("ScreenScraper", if ss.is_empty() { "Not set up".into() } else { ss }, "A screenscraper.fr account: box scans by region.", "ss"),
            Row {
                label: "RomM".into(),
                value: self.romm_state(),
                hint: "Your RomM server's games on your shelves: RustyShelf's full box art and manuals for them, and a game not on this device yet is downloaded when you press Play.".into(),
                press: Some(Box::new(|s: &mut Settings| s.go(Screen::Romm))),
                step: None,
            },
        ];
        Page { title: "Accounts".into(), lead: "Each service's sign-in or keys. They're kept in the data folder's .env file as well, in plain text, so it can be edited by hand or carried to another PC.".into(), rows }
    }

    /// One service's page under Accounts.
    pub(super) fn account_page(&self, which: &str) -> Page {
        match which {
            "ra" => self.ra_page(),
            "emus" => self.emus_page(),
            "steam" => self.steam_page(),
            "sgdb" => self.sgdb_page(),
            "igdb" => self.igdb_page(),
            _ => self.ss_page(),
        }
    }

    /// Of the consoles RetroAchievements or their emulator's own files cover, how many are ready, and how many.
    fn emulators_ready(&self) -> (usize, usize) {
        let rows = self.emus_page().rows;
        let counted: Vec<&Row> = rows.iter().filter(|r| !r.label.ends_with("pop-up") && !matches!(r.value.as_str(), "Not covered" | "No emulator")).collect();
        (counted.iter().filter(|r| r.value.starts_with("Ready")).count(), counted.len())
    }

    fn ra_page(&self) -> Page {
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
            {
                let now = self.ra_user();
                let old = now.clone();
                Row {
                    label: "Username".into(),
                    value: if now.is_empty() { "(none)".into() } else { now },
                    hint: "Your name on retroachievements.org.".into(),
                    press: Some(Box::new(move |s: &mut Settings| s.edit("Username", "Your name on retroachievements.org.", old.clone(), Box::new(|s: &mut Settings, v: String| s.set_ra_user(v.trim().to_string()))))),
                    step: None,
                }
            },
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
        let (ready, all) = self.emulators_ready();
        rows.push(Row {
            label: "Emulators".into(),
            value: if all == 0 { "No emulator".into() } else { format!("{ready} of {all} ready") },
            hint: "Whether each console's emulator is signed in to RetroAchievements, and Xbox 360's and PS3's own achievements.".into(),
            press: Some(Box::new(|s: &mut Settings| s.go(Screen::Account("emus")))),
            step: None,
        });
        Page { title: "RetroAchievements".into(), lead: "Achievements are earned inside the emulator, signed in there; RustyShelf reads your progress with your name and web API key.".into(), rows }
    }

    fn emus_page(&self) -> Page {
        // each console's emulator: whether achievements will work in it
        let mut rows = Vec::new();
        let mut real = self.cfg.clone();
        real["ra"]["user"] = json!(self.ra_user());
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
        // Xenia's and RPCS3's own achievements, with the switch for each one's pop-up in game
        let own: [(&str, &str, &str); 2] = [("x360", "Xenia", "Xbox 360"), ("ps3", "RPCS3", "PS3")];
        for (system, emulator, name) in own {
            let Some(exe) = self.own_ach_emulator(system) else { continue };
            let (state, popups, detail) = if system == "x360" { xeniaach::status(&exe, &paths::env_var) } else { rpcs3troph::status(&exe) };
            let value = match state {
                "ok" => "Ready",
                "signed-out" => "Not signed in",
                _ => "Unknown",
            };
            let hint = if detail.is_empty() { format!("{emulator} keeps the {} on this PC: no account needed.", if system == "ps3" { "trophies" } else { "achievements" }) } else { detail };
            rows.push(Row { label: format!("{name} · {emulator}"), value: value.into(), hint, ..Row::default() });
            if state == "ok" {
                rows.push(Row {
                    label: format!("{emulator}'s own pop-up"),
                    value: if popups { "On" } else { "Off" }.into(),
                    hint: format!("{emulator} shows an achievement as it's earned. Close {emulator} first: it writes its settings back when it quits."),
                    press: Some(Box::new(move |s: &mut Settings| {
                        let Some(exe) = s.own_ach_emulator(system) else { return };
                        let done = if system == "x360" { xeniaach::set_popups(&exe, !popups, &paths::env_var) } else { rpcs3troph::set_popups(&exe, !popups) };
                        match done {
                            Ok(_) => s.say(format!("{emulator}'s pop-up: {}", if popups { "Off" } else { "On" })),
                            Err(why) => s.say(why),
                        }
                    })),
                    step: None,
                });
            }
        }
        Page { title: "Emulators".into(), lead: "Whether achievements will work in each console's emulator. Xenia and RPCS3 keep their own, on this PC.".into(), rows }
    }

    fn steam_page(&self) -> Page {
        let mut rows = Vec::new();
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
            label: "Steam Web API key".into(),
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
        Page { title: "Steam".into(), lead: "Your Steam achievements on the Trophies screen, for the account signed in to Steam on this PC.".into(), rows }
    }

    fn sgdb_page(&self) -> Page {
        let mut rows = Vec::new();
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
        Page { title: "SteamGridDB".into(), lead: "Community-made posters for PC games, ports and recomps without a cover.".into(), rows }
    }

    fn igdb_page(&self) -> Page {
        let mut rows = Vec::new();
        let (igdb_id, igdb_secret) = (self.api_key("igdbId"), self.api_key("igdbSecret"));
        for (label, key, hint, secret) in [
            ("IGDB Client ID", "igdbId", "From a free Twitch developer app (dev.twitch.tv → Your Console → Applications → Register, any name, OAuth redirect http://localhost). IGDB then fills in what LaunchBox's database is missing about a game. Kept encrypted.", false),
            ("IGDB Client Secret", "igdbSecret", "The same app's Client Secret (New Secret). Kept encrypted, sent only to Twitch.", true),
        ] {
            let now = self.api_key(key);
            rows.push(Row {
                label: label.into(),
                value: if now.is_empty() { "(none)".into() } else if secret { "••••••••".into() } else { now.clone() },
                hint: hint.into(),
                press: Some(Box::new(move |s: &mut Settings| s.edit(label, hint, String::new(), Box::new(move |s: &mut Settings, v: String| s.set_api_key(key, v.trim().to_string()))))),
                step: None,
            });
        }
        rows.push(Row {
            label: "Check IGDB".into(),
            hint: "Asks Twitch for an app token with this Client ID and Secret.".into(),
            press: Some(Box::new(move |s: &mut Settings| {
                let c = rustyshelf_core::igdb::Igdb::new(&igdb_id, &igdb_secret);
                s.say("Asking Twitch…".into());
                let (tx, rx) = std::sync::mpsc::channel();
                s.ra_answer = Some(Mutex::new(rx));
                std::thread::spawn(move || {
                    let _ = tx.send(RaAnswer::Tested(c.test().map(|()| "IGDB: the keys work".to_string()).map_err(|why| format!("IGDB: {why}"))));
                });
            })),
            ..Row::default()
        });
        Page { title: "IGDB".into(), lead: "What LaunchBox's database is missing about a game, and more screenshots. Free with a Twitch developer app.".into(), rows }
    }

    fn ss_page(&self) -> Page {
        let mut rows = Vec::new();
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
        Page { title: "ScreenScraper".into(), lead: "Box scans by region for the art sources that use it.".into(), rows }
    }

    /// The emulator a console with achievements of its own uses (Xenia for Xbox 360, on Windows; RPCS3 for PS3),
    /// when one is set up.
    pub(super) fn own_ach_emulator(&self, system: &str) -> Option<std::path::PathBuf> {
        if !matches!(system, "ps3" | "x360") || (system == "x360" && !cfg!(windows)) {
            return None;
        }
        let exe = self.text(&["systems", system, "emulator"]);
        (!exe.is_empty()).then(|| rustyshelf_core::launch::emu_path(&exe, &self.app_root))
    }

    /// More screenshots for the chosen console game when it has fewer than 6: asked once a run, off the frame
    /// (ScreenScraper with the account, libretro's thumbnails, IGDB with keys).
    pub(super) fn load_more_shots(&mut self) {
        let Some(c) = self.chosen.as_ref().filter(|c| c.game.source == Source::Rom && !c.game.file.as_os_str().is_empty()) else { return };
        let base: Vec<String> = c.game.info.as_ref().map(|i| i.shots.clone()).unwrap_or_default();
        if base.len() >= 6 || !self.more_shots() || !self.shots_asked.insert(c.game.id.clone()) {
            return;
        }
        let (sys, file, title, region, id) = (c.console.clone(), c.game.file.clone(), c.game.title.clone(), c.game.region.clone(), c.game.id.clone());
        let (data, art_region) = (paths::data_dir(&self.app_root), self.art_region());
        let (tx, rx) = std::sync::mpsc::channel();
        self.shots_answer = Some(Mutex::new(rx));
        std::thread::spawn(move || {
            use rustyshelf_core::moreshots::{self, Source};
            let (ss, ig) = (ScreenScraper::from_settings(&data), rustyshelf_core::igdb::Igdb::from_settings(&data));
            let cache = data.join("boxart-cache");
            let lr = moreshots::Libretro { cache_dir: &cache, region: &art_region };
            let (ss, ig) = (ss.as_ref().map(moreshots::Ss), ig.as_ref().map(|client| moreshots::Igdb { client, data_dir: &data }));
            let sources: Vec<&dyn Source> = [ss.as_ref().map(|s| s as &dyn Source), Some(&lr as &dyn Source), ig.as_ref().map(|s| s as &dyn Source)].into_iter().flatten().collect();
            let mut index = moreshots::Index::load(&data);
            let lb = |u: &str| u.starts_with("https://images.launchbox-app.com/").then(|| rustyshelf_core::net::get(u, std::time::Duration::from_secs(30), 32 << 20, "RustyShelf").ok()).flatten().filter(|r| r.0 == 200).map(|r| r.1);
            moreshots::top_up(&mut index, &sys, &moreshots::Game { file: &file, title: &title, region: &region }, &base, &sources, &lb);
            let _ = tx.send((id, index.list(&sys, &file, &title, &base)));
        });
    }

    /// The chosen game's screenshots, since the last frame; kept for when its Details opens again.
    pub(super) fn poll_more_shots(&mut self) {
        let Some((id, shots)) = self.shots_answer.as_ref().and_then(|rx| rx.lock().ok()?.try_recv().ok()) else { return };
        self.shots_answer = None;
        if let Some(c) = self.chosen.as_mut().filter(|c| c.game.id == id && c.game.info.as_ref().map_or(0, |i| i.shots.len()) < shots.len()) {
            c.game.info.get_or_insert_with(Default::default).shots = shots.clone();
            self.redraw = true;
        }
        self.more_shots.insert(id, shots);
    }

    /// Asks IGDB for what the chosen console game's info is missing, when there are keys and it's due; what's kept
    /// from before fills the gaps without asking.
    pub(super) fn load_igdb(&mut self) {
        let Some(c) = self.chosen.as_ref().filter(|c| c.game.source == Source::Rom && rustyshelf_core::igdb::gaps(c.game.info.as_ref())) else { return };
        let (sys, file, title, id, mut info) = (c.console.clone(), c.game.file.clone(), c.game.title.clone(), c.game.id.clone(), c.game.info.clone().unwrap_or_default());
        let data = paths::data_dir(&self.app_root);
        let (tx, rx) = std::sync::mpsc::channel();
        self.igdb_answer = Some(Mutex::new(rx));
        std::thread::spawn(move || {
            let client = rustyshelf_core::igdb::Igdb::from_settings(&data);
            if rustyshelf_core::igdb::lookup_info(client.as_ref(), &data, &sys, &file, &title, &mut info) {
                let _ = tx.send((id, info));
            }
        });
    }

    /// IGDB's answer for the chosen game, since the last frame.
    pub(super) fn poll_igdb(&mut self) {
        let Some((id, info)) = self.igdb_answer.as_ref().and_then(|rx| rx.lock().ok()?.try_recv().ok()) else { return };
        self.igdb_answer = None;
        if let Some(c) = self.chosen.as_mut().filter(|c| c.game.id == id) {
            c.game.info = Some(info);
            self.redraw = true;
        }
    }

    /// Asks for the chosen game's achievements (again, when `force`).
    pub(super) fn load_achievements(&mut self, force: bool) {
        let Some(c) = &self.chosen else { return };
        let (shelf, title, id) = (c.console.clone(), c.game.title.clone(), c.game.id.clone());
        self.achievements = None;
        if let Some(exe) = self.own_ach_emulator(&shelf) {
            let (file, title_id, cache) = (c.game.file.clone(), c.game.title_id.clone().unwrap_or_default(), paths::data_dir(&self.app_root).join("trophy-packs"));
            let (tx, rx) = std::sync::mpsc::channel();
            self.ra_answer = Some(Mutex::new(rx));
            std::thread::spawn(move || {
                let got = if shelf == "x360" { xeniaach::progress(&exe, &title_id, &paths::env_var) } else { rpcs3troph::progress(&exe, &title, &file, Some(&cache), rustyshelf_core::now_ms() as i64) };
                let _ = tx.send(RaAnswer::Progress(id, Ok(got)));
            });
            return;
        }
        if let Some(appid) = self.steam_app().map(String::from) {
            let (given, data) = (self.text(&["steam", "path"]), paths::data_dir(&self.app_root));
            let (tx, rx) = std::sync::mpsc::channel();
            self.ra_answer = Some(Mutex::new(rx));
            std::thread::spawn(move || {
                let root = rustyshelf_core::steam::main_root(&given);
                let mut got = root.as_deref().and_then(|r| rustyshelf_core::pcach::steam_progress_named(r, &appid));
                if let Some(p) = got.as_mut() {
                    let site = SteamAch::new(&data, "", root);
                    let (rarity, points) = (site.rarity(&appid), site.hunter_points(&appid));
                    let mut total = (0i64, 0i64);
                    for a in p["achievements"].as_array_mut().into_iter().flatten() {
                        let api = a["api"].as_str().unwrap_or("").to_string();
                        if let Some(r) = rarity.get(&api) {
                            a["rarity"] = json!(((r * 10.0).round() / 10.0).max(0.1));
                        }
                        if let Some(n) = points.get(&api).map(|n| n.round() as i64) {
                            a["points"] = json!(n);
                            total.0 += n;
                            if a["earned"] == true {
                                total.1 += n;
                            }
                        }
                    }
                    if !points.is_empty() {
                        (p["points"], p["pointsEarned"]) = (json!(total.0), json!(total.1));
                    }
                }
                let _ = tx.send(RaAnswer::Progress(id, Ok(got)));
            });
            return;
        }
        self.ask_ra(move |ra| RaAnswer::Progress(id, ra.progress(&shelf, &title, force)));
    }

    /// Where a running game's achievements are read from for the Home menu: Xenia's or RPCS3's files, or
    /// RetroAchievements with an account. None for a console none of them covers.
    pub fn ach_from(&self, console: &str) -> Option<AchFrom> {
        if let Some(exe) = self.own_ach_emulator(console) {
            return Some(AchFrom::Own { x360: console == "x360", exe, cache: paths::data_dir(&self.app_root).join("trophy-packs") });
        }
        rustyshelf_core::ra::console_id(console)?;
        let (user, key) = self.ra_account();
        (!user.is_empty() && !key.is_empty()).then(|| AchFrom::Ra { ra: self.ra.clone(), user, key })
    }

    /// The chosen game's achievements in a line, for Details; None on a shelf RetroAchievements doesn't cover
    /// or without an account.
    pub(super) fn achievements_line(&self) -> Option<String> {
        let c = self.chosen.as_ref()?;
        if self.steam_app().is_some() {
            // (a Steam game with none kept here has no row at all)
            let p = self.achievements.as_ref().filter(|a| a.0 == c.game.id)?.1.as_ref().ok()?.as_ref()?;
            let pts = p["points"].as_f64().filter(|n| *n > 0.0).map_or(String::new(), |n| format!(" · {} of {n} points", p["pointsEarned"]));
            return Some(format!("{} of {}{pts}{}", p["earned"], p["total"], if p["mastered"] == "completed" { " · Completed" } else { "" }));
        }
        if self.own_ach_emulator(&c.console).is_some() {
            let ps3 = c.console == "ps3";
            return Some(match self.achievements.as_ref().filter(|a| a.0 == c.game.id) {
                None => "…".into(),
                Some((_, Ok(Some(p)))) if ps3 => format!("{} of {} · {}", p["earned"], p["total"], p["grades"].as_str().unwrap_or("")),
                Some((_, Ok(Some(p)))) => format!("{} of {} · {} of {} G{}", p["earned"], p["total"], p["pointsEarned"], p["points"], if p["mastered"] == "completed" { " · Completed" } else { "" }),
                Some((_, Ok(None))) if ps3 => "No trophies found yet".into(),
                Some((_, Ok(None))) => "Nothing recorded yet".into(),
                Some((_, Err(_))) => "Couldn't be read".into(),
            });
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
        self.achievements_listing().0
    }

    /// The page, and each row's picture (an achievement's badge).
    pub(super) fn achievements_listing(&self) -> (Page, Vec<Option<String>>) {
        let mut pics: Vec<Option<String>> = Vec::new();
        let title = self.chosen.as_ref().map_or(String::new(), |c| c.game.title.clone());
        let steam = self.steam_app().is_some();
        let own = self.chosen.as_ref().filter(|c| self.own_ach_emulator(&c.console).is_some()).map(|c| if c.console == "ps3" { "RPCS3" } else { "Xenia" });
        let again = match own {
            Some(emu) => format!("Reads your progress again from what {emu} keeps on this PC."),
            None if steam => "Reads your progress again from what Steam keeps on this PC.".into(),
            None => "Reads your progress from RetroAchievements now.".into(),
        };
        let mut rows = vec![Row { label: "Ask again".into(), hint: again, press: Some(Box::new(|s: &mut Settings| s.load_achievements(true))), ..Row::default() }];
        let lead = match self.achievements.as_ref().map(|a| &a.1) {
            None if own.is_some() || steam => format!("Reading {}'s files…", own.unwrap_or("Steam")),
            Some(Ok(None)) if own == Some("Xenia") => "Xenia has nothing for this game yet: it records achievements once the game has run with a profile signed in.".into(),
            Some(Ok(None)) if own.is_some() => "No trophies found: RPCS3 has none for this game yet, and the game has no trophy pack of its own.".into(),
            None => "Asking RetroAchievements…".into(),
            Some(Err(why)) => why.clone(),
            Some(Ok(None)) if steam => "Steam keeps no achievements for this game on this PC yet.".into(),
            Some(Ok(None)) => "RetroAchievements has no set for this game.".into(),
            Some(Ok(Some(p))) => {
                let year = self.chosen.as_ref().and_then(|c| c.game.info.as_ref()?.year()).map(|y| y.to_string());
                let (more, more_pics) = self.ach_listing(p, &title, year);
                pics.resize(rows.len(), None);
                rows.extend(more);
                pics.extend(more_pics);
                if own == Some("RPCS3") {
                    format!("{} of {} earned · {}", p["earned"], p["total"], p["grades"].as_str().unwrap_or(""))
                } else if own.is_some() {
                    format!("{} of {} earned · {} of {} G", p["earned"], p["total"], p["pointsEarned"], p["points"])
                } else if steam && p["points"].as_f64().is_some_and(|n| n > 0.0) {
                    format!("{} of {} earned · {} of {} points (steamhunters.com)", p["earned"], p["total"], p["pointsEarned"], p["points"])
                } else if steam {
                    format!("{} of {} earned, as Steam keeps them on this PC", p["earned"], p["total"])
                } else {
                    format!("{} of {} earned · {} of {} points", p["earned"], p["total"], p["pointsEarned"], p["points"])
                }
            }
        };
        pics.resize(rows.len(), None);
        (Page { title: format!("Achievements · {title}"), lead, rows }, pics)
    }

    /// An achievement list's rows: the sort, then the sections, each achievement opening on its own; with each row's
    /// picture.
    pub(super) fn ach_listing(&self, p: &Value, game: &str, year: Option<String>) -> (Vec<Row>, Vec<Option<String>>) {
        const SORTS: [&str; 3] = ["Order", "Rarity", "Points"];
        let sort = self.ach_sort % SORTS.len();
        let all: Vec<&Value> = p["achievements"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
        let mut rows = Vec::new();
        if all.iter().any(|a| a["rarity"].is_number() || a["points"].as_f64().is_some_and(|n| n > 0.0)) {
            rows.push(Row {
                label: "Sort".into(),
                value: SORTS[sort].into(),
                hint: "◀ ▶ Order (story first), Rarity (the rarest first) or Points.".into(),
                press: Some(Box::new(|s: &mut Settings| s.sort_achievements(1))),
                step: Some(Box::new(|s: &mut Settings, by| s.sort_achievements(by))),
            });
        }
        let mut pics = vec![None; rows.len()];
        for (label, list) in achievement_sections(&all, sort) {
            if list.is_empty() {
                continue;
            }
            rows.push(Row { label: format!("— {label} · {} —", list.len()), ..Row::default() });
            pics.push(None);
            for a in list {
                let mut row = achievement_row(a);
                let (a2, g, y) = (a.clone(), game.to_string(), year.clone());
                row.press = Some(Box::new(move |s: &mut Settings| s.open_achievement(a2.clone(), g.clone(), y.clone())));
                rows.push(row);
                pics.push(a["badge"].as_str().filter(|b| !b.is_empty()).map(String::from));
            }
        }
        (rows, pics)
    }

    fn sort_achievements(&mut self, by: i32) {
        self.ach_sort = (self.ach_sort as i32 + by).rem_euclid(3) as usize;
        self.redraw = true;
    }
}

/// How rare an achievement is, from the share of players who have it.
pub(super) fn tier(rarity: Option<f64>) -> Option<&'static str> {
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

/// An achievement's value on the right of its row: how rare it is, then its points (or a trophy's grade).
pub(super) fn ach_value(a: &Value) -> String {
    let rarity = a["rarity"].as_f64().map(|r| format!("{r}%"));
    let worth = match a["grade"].as_str() {
        Some(g) => Some(g.to_string()),
        None => a["points"].as_f64().filter(|n| *n > 0.0).map(|n| format!("{n} pts")),
    };
    [rarity, worth].into_iter().flatten().collect::<Vec<_>>().join(" · ")
}

fn achievement_row(a: &Value) -> Row {
    let earned = a["earned"] == true;
    let rarity = a["rarity"].as_f64();
    let hint = [
        a["desc"].as_str().unwrap_or("").to_string(),
        kind(a).to_string(),
        a["date"].as_str().map_or(String::new(), |d| format!("Earned {d}")),
        rarity.map_or(String::new(), |r| format!("{r}% of players have it")),
        tier(rarity).map_or(String::new(), String::from),
        if a["hardcore"] == true { "hardcore".into() } else { String::new() },
    ]
    .into_iter()
    .filter(|t| !t.is_empty())
    .collect::<Vec<_>>()
    .join(" · ");
    Row { label: format!("{} {}", if earned { "★" } else { "☆" }, a["title"].as_str().unwrap_or("")), value: ach_value(a), hint, ..Row::default() }
}

/// Where a running game's achievements come from (`Settings::ach_from`).
pub enum AchFrom {
    Own { x360: bool, exe: std::path::PathBuf, cache: std::path::PathBuf },
    Ra { ra: Arc<Ra>, user: String, key: String },
}

/// The running game as its achievements are looked up: its console, title, file and Xbox 360 title id.
pub struct AchGame {
    pub console: String,
    pub title: String,
    pub file: std::path::PathBuf,
    pub title_id: String,
}

impl AchFrom {
    pub fn label(&self) -> &'static str {
        match self {
            AchFrom::Own { x360: false, .. } => "Trophies",
            _ => "Achievements",
        }
    }

    pub fn source(&self) -> &'static str {
        match self {
            AchFrom::Own { x360: true, .. } => "Xenia",
            AchFrom::Own { .. } => "RPCS3",
            AchFrom::Ra { .. } => "RetroAchievements",
        }
    }

    fn own(x360: bool, exe: &std::path::Path, cache: &std::path::Path, g: &AchGame) -> Option<Value> {
        if x360 {
            let id = g.title_id.len() == 8 && g.title_id.chars().all(|c| c.is_ascii_hexdigit());
            return id.then(|| xeniaach::progress(exe, &g.title_id, &paths::env_var)).flatten();
        }
        rpcs3troph::progress(exe, &g.title, &g.file, Some(cache), rustyshelf_core::now_ms() as i64)
    }

    /// Whether the game has any: from the files on this PC, or RetroAchievements' game list as last downloaded.
    pub fn has_any(&self, g: &AchGame) -> bool {
        match self {
            AchFrom::Own { x360, exe, cache } => Self::own(*x360, exe, cache, g).is_some_and(|p| p["total"].as_u64().unwrap_or(0) > 0),
            AchFrom::Ra { ra, user, key } => {
                ra.set_credentials(user, key);
                ra.has_set(&g.console, &g.title)
            }
        }
    }

    /// The game's progress; RetroAchievements' kept copy is used when it's no older than `max_age_ms`.
    pub fn read(&self, g: &AchGame, max_age_ms: u64) -> Result<Option<Value>, String> {
        match self {
            AchFrom::Own { x360, exe, cache } => Ok(Self::own(*x360, exe, cache, g)),
            AchFrom::Ra { ra, user, key } => {
                ra.set_credentials(user, key);
                ra.progress_within(&g.console, &g.title, max_age_ms)
            }
        }
    }
}
