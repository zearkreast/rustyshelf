//! Trophies: the whole RetroAchievements account and the Steam achievements of the account signed in here, on one
//! screen: every game with how far it is, the last month's unlocks with how rare each is, and the mastered games.

use rustyshelf_core::steamach::{self, SteamAch};

use super::*;

const TABS: [&str; 3] = ["Games", "Recent unlocks", "Mastered"];
/// the sources the lists can be narrowed to: (name, `src` tag)
const FROM: [(&str, &str); 3] = [("Everywhere", ""), ("RetroAchievements", "ra"), ("Steam", "steam")];

#[derive(Default)]
pub(super) struct Trophies {
    data: Option<Value>,
    tab: usize,
    from: usize,
    loading: Option<Mutex<Receiver<Result<Value, String>>>>,
}

fn award(a: &Value) -> &'static str {
    match a.as_str() {
        Some("mastered") => "Mastered",
        Some("completed") => "Completed",
        Some("beaten-hardcore") => "Beaten",
        Some("beaten-softcore") => "Beaten (softcore)",
        _ => "",
    }
}

fn mastered(g: &&Value) -> bool {
    matches!(g["award"].as_str(), Some("mastered" | "completed"))
}

fn day(v: &Value) -> &str {
    v.as_str().map_or("", |d| d.get(..10).unwrap_or(d))
}

impl Settings {
    /// Asks both sites (again, when `force`) and opens the screen.
    pub(super) fn open_trophies(&mut self, force: bool) {
        if self.trophies.loading.is_some() {
            return;
        }
        if !force && self.trophies.data.is_some() {
            self.trophies.tab = 0;
            if self.stack.last().is_none_or(|p| p.0 != Screen::Trophies) {
                self.go(Screen::Trophies);
            }
            return;
        }
        let (ra, (user, key)) = (self.ra.clone(), self.ra_account());
        let steam = SteamAch::new(&paths::data_dir(&self.app_root), &self.api_key("steamweb"), rustyshelf_core::steam::main_root(&self.text(&["steam", "path"])));
        if user.is_empty() && !steam.enabled() {
            if self.stack.last().is_some_and(|p| p.0 == Screen::Trophies) {
                self.stack.pop();
            }
            if self.stack.is_empty() {
                self.stack.push((Screen::Accounts, 0));
            }
            self.say("Sign in to RetroAchievements, or add a Steam Web API key, under Accounts first.".into());
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            ra.set_credentials(&user, &key);
            let mut errs = Vec::new();
            let (r, s) = std::thread::scope(|sc| {
                let r = sc.spawn(|| ra.trophies(force));
                let s = sc.spawn(|| steam.trophies(force, now_s()));
                (r.join().unwrap_or(Ok(None)), s.join().unwrap_or(Ok(None)))
            });
            let mut keep = |got: Result<Option<Value>, String>| got.unwrap_or_else(|e| {
                errs.push(e);
                None
            });
            let (r, s) = (keep(r), keep(s));
            let _ = tx.send(steamach::combine(r, s, &errs));
        });
        self.trophies.loading = Some(Mutex::new(rx));
        if !force {
            self.trophies.tab = 0;
        }
        if self.stack.last().is_none_or(|p| p.0 != Screen::Trophies) {
            self.go(Screen::Trophies);
        }
        self.redraw = true;
    }

    fn trophy_from(&mut self, by: i32) {
        self.trophies.from = (self.trophies.from as i32 + by).rem_euclid(FROM.len() as i32) as usize;
        self.cues.push(("tick", by));
        self.redraw = true;
    }

    fn trophy_tab(&mut self, tab: usize) {
        if tab != self.trophies.tab {
            self.trophies.tab = tab;
            self.cues.push(("tick", 1));
            self.redraw = true;
        }
    }

    pub(super) fn poll_trophies(&mut self) {
        let Some(got) = self.trophies.loading.as_ref().and_then(|rx| rx.lock().unwrap().try_recv().ok()) else { return };
        self.trophies.loading = None;
        match got {
            Ok(d) => {
                if let Some(w) = d["warn"].as_str().filter(|w| !w.is_empty()) {
                    self.say(w.to_string());
                }
                self.trophies.data = Some(d);
            }
            Err(why) => {
                self.say(why);
                if self.trophies.data.is_none() && self.stack.last().is_some_and(|p| p.0 == Screen::Trophies) {
                    self.back();
                }
            }
        }
        self.redraw = true;
    }

    pub(super) fn trophies_page(&self) -> Page {
        self.trophies_listing().0
    }

    /// The page, and each row's picture (a game's icon, an achievement's badge).
    pub(super) fn trophies_listing(&self) -> (Page, Vec<Option<String>>) {
        let t = &self.trophies;
        let Some(d) = &t.data else {
            return (Page { title: "Trophies".into(), lead: "Loading your trophies…".into(), rows: Vec::new() }, Vec::new());
        };
        let empty = Vec::new();
        let src = &d["sources"];
        let both = src["ra"] == true && src["steam"] == true;
        let from = if both { FROM[t.from % FROM.len()].1 } else { "" };
        let mine = |x: &&Value| from.is_empty() || x["src"] == from;
        let games: Vec<&Value> = d["games"].as_array().unwrap_or(&empty).iter().filter(mine).collect();
        let recent: Vec<&Value> = d["recent"].as_array().unwrap_or(&empty).iter().filter(mine).collect();
        let done: Vec<&Value> = games.iter().copied().filter(mastered).collect();
        let counts = [games.len(), recent.len(), done.len()];
        let mut rows = Vec::new();
        if both {
            rows.push(Row {
                label: "From".into(),
                value: FROM[t.from % FROM.len()].0.into(),
                hint: "◀ ▶ Everywhere · RetroAchievements (your emulators) · Steam".into(),
                press: Some(Box::new(|s: &mut Settings| s.trophy_from(1))),
                step: Some(Box::new(|s: &mut Settings, by| s.trophy_from(by))),
            });
        }
        rows.extend([
            Row {
                label: "Show".into(),
                value: format!("{} {}", TABS[t.tab], counts[t.tab]),
                hint: "◀ ▶ Games · Recent unlocks · Mastered".into(),
                press: Some(Box::new(|s: &mut Settings| s.trophy_tab((s.trophies.tab + 1) % TABS.len()))),
                step: Some(Box::new(|s: &mut Settings, by| s.trophy_tab((s.trophies.tab as i32 + by).clamp(0, TABS.len() as i32 - 1) as usize))),
            },
            Row { label: "Reload".into(), hint: "Asks RetroAchievements and Steam again now.".into(), press: Some(Box::new(|s: &mut Settings| s.open_trophies(true))), ..Row::default() },
        ]);
        let list = match t.tab {
            1 => &recent,
            2 => &done,
            _ => &games,
        };
        let mut pics: Vec<Option<String>> = vec![None; rows.len()];
        for x in list {
            let pic = if t.tab == 1 { [&x["badge"], &x["fallback"]] } else { [&x["icon"], &Value::Null] };
            pics.push(pic.iter().find_map(|p| p.as_str().filter(|u| u.starts_with("https://"))).map(String::from));
            let text = |k: &str| x[k].as_str().unwrap_or("").to_string();
            rows.push(if t.tab == 1 {
                let rarity = x["rarity"].as_f64().map_or(String::new(), |r| format!(" · {r}% of players"));
                Row {
                    label: text("title"),
                    value: if x["src"] == "steam" { "Steam".into() } else { format!("{} pts", x["points"]) },
                    hint: format!("{}{} · {}{}{rarity}", text("game"), if text("desc").is_empty() { String::new() } else { format!(" · {}", text("desc")) }, day(&x["date"]), if x["hardcore"] == true { " · hardcore" } else { "" }),
                    ..Row::default()
                }
            } else {
                let aw = award(&x["award"]);
                let last = day(&x["last"]);
                Row {
                    label: text("title"),
                    value: format!("{}{}%", if aw.is_empty() { String::new() } else { format!("{aw} · ") }, x["pct"]),
                    hint: format!("{} · {} / {}{}", text("console"), x["earned"], x["total"], if last.is_empty() { String::new() } else { format!(" · {last}") }),
                    ..Row::default()
                }
            });
        }
        if list.is_empty() {
            let none = match t.tab {
                1 => "No unlocks in the last 30 days. Go earn some!",
                2 => "No mastered game yet. Games shows how close you are.",
                _ => "No game with achievements on this account yet.",
            };
            rows.push(Row { label: none.into(), ..Row::default() });
        }
        let mut who = Vec::new();
        if src["ra"] == true {
            let soft = d["softcore"].as_f64().filter(|s| *s != 0.0).map_or(String::new(), |s| format!(" · {s} softcore"));
            who.push(format!("{} RetroAchievements points{soft}", d["points"].as_f64().unwrap_or(0.0)));
        }
        if src["steam"] == true {
            who.push(match src["steamUser"].as_str().filter(|u| !u.is_empty()) {
                Some(u) => format!("Steam · {u}"),
                None => "Steam".into(),
            });
        }
        let tot = &d["totals"];
        let lead = format!(
            "{}{}  ·  {} unlocked · {} games · {} mastered · {} beaten · {}% average",
            who.join("  ·  "),
            if t.loading.is_some() { " · reloading…" } else { "" },
            tot["unlocked"],
            tot["games"],
            tot["mastered"],
            tot["beaten"],
            tot["avg"]
        );
        let user = d["user"].as_str().filter(|u| !u.is_empty()).unwrap_or("RetroAchievements");
        pics.resize(rows.len(), None);
        (Page { title: format!("Trophies · {user}"), lead, rows }, pics)
    }
}

fn now_s() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}
