//! Accounts: RetroAchievements. The account's name and web API key, whether each console's emulator is signed in
//! to it, and a game's achievements in Details.

use std::sync::Arc;

use rustyshelf_core::ra::Ra;
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
        Page { title: "Accounts".into(), lead: "RetroAchievements: your progress on the shelves that it covers.".into(), rows }
    }

    /// Asks for the chosen game's achievements (again, when `force`).
    pub(super) fn load_achievements(&mut self, force: bool) {
        let Some(c) = &self.chosen else { return };
        let (shelf, title, id) = (c.console.clone(), c.game.title.clone(), c.game.id.clone());
        self.achievements = None;
        self.ask_ra(move |ra| RaAnswer::Progress(id, ra.progress(&shelf, &title, force)));
    }

    /// The chosen game's achievements in a line, for Details; None on a shelf RetroAchievements doesn't cover
    /// or without an account.
    pub(super) fn achievements_line(&self) -> Option<String> {
        let c = self.chosen.as_ref()?;
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

    pub(super) fn achievements_page(&self) -> Page {
        let title = self.chosen.as_ref().map_or(String::new(), |c| c.game.title.clone());
        let mut rows = vec![Row { label: "Ask again".into(), hint: "Reads your progress from RetroAchievements now.".into(), press: Some(Box::new(|s: &mut Settings| s.load_achievements(true))), ..Row::default() }];
        let lead = match self.achievements.as_ref().map(|a| &a.1) {
            None => "Asking RetroAchievements…".into(),
            Some(Err(why)) => why.clone(),
            Some(Ok(None)) => "RetroAchievements has no set for this game.".into(),
            Some(Ok(Some(p))) => {
                for a in p["achievements"].as_array().map(Vec::as_slice).unwrap_or_default() {
                    let earned = a["earned"] == true;
                    rows.push(Row {
                        label: format!("{} {}", if earned { "★" } else { "☆" }, a["title"].as_str().unwrap_or("")),
                        value: format!("{}{}", a["points"], if a["hardcore"] == true { " · hardcore" } else { "" }),
                        hint: [a["desc"].as_str().unwrap_or("").to_string(), a["date"].as_str().map_or(String::new(), |d| format!("Earned {d}")), a["rarity"].as_u64().map_or(String::new(), |r| format!("{r}% of players have it"))]
                            .into_iter()
                            .filter(|t| !t.is_empty())
                            .collect::<Vec<_>>()
                            .join(" · "),
                        ..Row::default()
                    });
                }
                format!("{} of {} earned · {} of {} points", p["earned"], p["total"], p["pointsEarned"], p["points"])
            }
        };
        Page { title: format!("Achievements · {title}"), lead, rows }
    }
}
