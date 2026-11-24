//! Trophies: the whole RetroAchievements account, RPCS3's trophies and the Steam achievements of the account signed in
//! here, on one screen: every game with how far it is, the last month's unlocks with how rare each is, and the
//! mastered games. A game opens its achievements, and an achievement opens on its own, with a search for how it's got.

use rustyshelf_core::steamach::{self, SteamAch};

use super::accounts::{ach_value, tier};
use super::*;

const TABS: [&str; 3] = ["Games", "Recent unlocks", "Mastered"];
/// the sources the lists can be narrowed to: (name, `src` tag)
const FROM: [(&str, &str); 4] = [("Everywhere", ""), ("RetroAchievements", "ra"), ("RPCS3", "rpcs3"), ("Steam", "steam")];

type Got = Result<Option<Value>, String>;

#[derive(Default)]
pub(super) struct Trophies {
    data: Option<Value>,
    tab: usize,
    from: usize,
    loading: Option<Later<Result<Value, String>>>,
    /// the game opened from the list: its source, id, title, and its achievements once they're here
    game: Option<(String, String, String, Option<Got>)>,
    game_rx: Option<Later<Got>>,
    /// the achievement opened: it, its game's title and release year
    pub(super) one: Option<(Value, String, Option<String>)>,
}

fn award(a: &Value) -> &'static str {
    match a.as_str() {
        Some("mastered") => "Mastered",
        Some("completed") => "Completed",
        Some("platinum") => "Platinum",
        Some("beaten-hardcore") => "Beaten",
        Some("beaten-softcore") => "Beaten (softcore)",
        _ => "",
    }
}

fn mastered(g: &&Value) -> bool {
    matches!(g["award"].as_str(), Some("mastered" | "completed" | "platinum"))
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
        let ps3 = self.own_ach_emulator("ps3");
        if user.is_empty() && !steam.enabled() && ps3.is_none() {
            if self.stack.last().is_some_and(|p| p.0 == Screen::Trophies) {
                self.stack.pop();
            }
            if self.stack.is_empty() {
                self.stack.push((Screen::Accounts, 0));
            }
            self.say("Sign in to RetroAchievements, add a Steam Web API key under Accounts, or set up RPCS3 for PS3 first.".into());
            return;
        }
        self.trophies.loading = Some(Later::spawn(move || {
            ra.set_credentials(&user, &key);
            let mut errs = Vec::new();
            let (r, s) = std::thread::scope(|sc| {
                let r = sc.spawn(|| ra.trophies(force));
                let s = sc.spawn(|| steam.trophies(force, rustyshelf_core::js::date_now() / 1000.0));
                (r.join().unwrap_or(Ok(None)), s.join().unwrap_or(Ok(None)))
            });
            let mut keep = |got: Result<Option<Value>, String>| got.unwrap_or_else(|e| {
                errs.push(e);
                None
            });
            let (r, s) = (keep(r), keep(s));
            let (games, recent) = ps3.map(|exe| rustyshelf_core::rpcs3troph::all(&exe, rustyshelf_core::now_ms() as i64, 30)).unwrap_or_default();
            match steamach::combine(r, s, &errs) {
                Err(_) if !games.is_empty() => Ok(steamach::add_rpcs3(None, games, recent)),
                Ok(d) => Ok(steamach::add_rpcs3(Some(d), games, recent)),
                Err(e) => Err(e),
            }
        }));
        if !force {
            self.trophies.tab = 0;
        }
        if self.stack.last().is_none_or(|p| p.0 != Screen::Trophies) {
            self.go(Screen::Trophies);
        }
        self.redraw = true;
    }

    /// The sources this account's lists have, Everywhere first.
    fn trophy_froms(&self) -> Vec<(&'static str, &'static str)> {
        let src = self.trophies.data.as_ref().map(|d| &d["sources"]);
        FROM.into_iter().filter(|f| f.1.is_empty() || src.is_some_and(|s| s[f.1] == true)).collect()
    }

    fn trophy_from(&mut self, by: i32) {
        let n = self.trophy_froms().len().max(1);
        self.trophies.from = (self.trophies.from as i32 + by).rem_euclid(n as i32) as usize;
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

    /// A game from the list, opened on its achievements.
    fn open_trophy_game(&mut self, src: String, id: String, title: String) {
        let data = paths::data_dir(&self.app_root);
        let got = match src.as_str() {
            "ra" => {
                let (ra, (user, key)) = (self.ra.clone(), self.ra_account());
                let Ok(n) = id.parse::<u64>() else { return };
                Later::spawn(move || {
                    ra.set_credentials(&user, &key);
                    ra.game(n, false)
                })
            }
            "steam" => {
                let steam = SteamAch::new(&data, &self.api_key("steamweb"), rustyshelf_core::steam::main_root(&self.text(&["steam", "path"])));
                let appid = id.trim_start_matches("steam-").to_string();
                Later::spawn(move || steam.game(&appid, false))
            }
            _ => {
                let Some(exe) = self.own_ach_emulator("ps3") else { return };
                let set = id.clone();
                Later::spawn(move || Ok(rustyshelf_core::rpcs3troph::set(&exe, &set, rustyshelf_core::now_ms() as i64)))
            }
        };
        self.trophies.game = Some((src, id, title, None));
        self.trophies.game_rx = Some(got);
        self.go(Screen::TrophyGame);
    }

    /// One achievement on its own page.
    pub(super) fn open_achievement(&mut self, a: Value, game: String, year: Option<String>) {
        self.trophies.one = Some((a, game, year));
        self.go(Screen::AchievementOne);
    }

    pub(super) fn poll_trophies(&mut self) {
        if let Some(got) = self.trophies.game_rx.as_ref().and_then(Later::poll) {
            self.trophies.game_rx = None;
            if let Some(g) = self.trophies.game.as_mut() {
                g.3 = Some(got);
            }
            self.redraw = true;
        }
        let Some(got) = self.trophies.loading.as_ref().and_then(Later::poll) else { return };
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
        let froms = self.trophy_froms();
        let both = froms.len() > 2;
        let from = if both { froms[t.from % froms.len()].1 } else { "" };
        let mine = |x: &&Value| from.is_empty() || x["src"] == from;
        let games: Vec<&Value> = d["games"].as_array().unwrap_or(&empty).iter().filter(mine).collect();
        let recent: Vec<&Value> = d["recent"].as_array().unwrap_or(&empty).iter().filter(mine).collect();
        let done: Vec<&Value> = games.iter().copied().filter(mastered).collect();
        let counts = [games.len(), recent.len(), done.len()];
        let mut rows = Vec::new();
        if both {
            rows.push(Row {
                label: "From".into(),
                value: froms[t.from % froms.len()].0.into(),
                hint: format!("◀ ▶ {}", froms.iter().map(|f| f.0).collect::<Vec<_>>().join(" · ")),
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
            pics.push(pic.iter().find_map(|p| p.as_str().filter(|u| !u.is_empty())).map(String::from));
            let text = |k: &str| x[k].as_str().unwrap_or("").to_string();
            rows.push(if t.tab == 1 {
                let a = (*x).clone();
                let game = text("game");
                Row {
                    label: text("title"),
                    value: ach_value(x),
                    hint: format!("{}{} · {}{}", text("game"), if text("desc").is_empty() { String::new() } else { format!(" · {}", text("desc")) }, day(&x["date"]), if x["hardcore"] == true { " · hardcore" } else { "" }),
                    press: Some(Box::new(move |s: &mut Settings| s.open_achievement(a.clone(), game.clone(), None))),
                    step: None,
                }
            } else {
                let aw = award(&x["award"]);
                let last = day(&x["last"]);
                let (src, id, title) = (text("src"), rustyshelf_core::js::string(x.get("id")), text("title"));
                Row {
                    label: text("title"),
                    value: format!("{}{}%", if aw.is_empty() { String::new() } else { format!("{aw} · ") }, x["pct"]),
                    hint: format!("{} · {} / {}{}", text("console"), x["earned"], x["total"], if last.is_empty() { String::new() } else { format!(" · {last}") }),
                    press: Some(Box::new(move |s: &mut Settings| s.open_trophy_game(src.clone(), id.clone(), title.clone()))),
                    step: None,
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
        if src["rpcs3"] == true {
            who.push("RPCS3".into());
        }
        let tot = &d["totals"];
        let lead = format!(
            "{}{}{}{} unlocked · {} games · {} mastered · {} beaten · {}% average",
            who.join("  ·  "),
            if t.loading.is_some() { " · reloading…" } else { "" },
            if who.is_empty() { "" } else { "  ·  " },
            tot["unlocked"],
            tot["games"],
            tot["mastered"],
            tot["beaten"],
            tot["avg"]
        );
        let title = match d["user"].as_str().filter(|u| !u.is_empty()) {
            Some(user) => format!("Trophies · {user}"),
            None => "Trophies".into(),
        };
        pics.resize(rows.len(), None);
        (Page { title, lead, rows }, pics)
    }
}

impl Settings {
    /// A game opened from Trophies: its achievements, each opening on its own.
    pub(super) fn trophy_game_listing(&self) -> (Page, Vec<Option<String>>) {
        let Some((src, _, title, got)) = &self.trophies.game else { return (Page { title: "Achievements".into(), lead: String::new(), rows: Vec::new() }, Vec::new()) };
        let lead = match got {
            None => "Loading…".to_string(),
            Some(Err(why)) => why.clone(),
            Some(Ok(None)) => "Nothing kept for this game.".to_string(),
            Some(Ok(Some(p))) => summary(p, src),
        };
        let (rows, pics) = match got {
            Some(Ok(Some(p))) => {
                let year = p["year"].as_str().map(String::from);
                let name = p["title"].as_str().filter(|t| !t.is_empty()).unwrap_or(title).to_string();
                self.ach_listing(p, &name, year)
            }
            _ => (Vec::new(), Vec::new()),
        };
        (Page { title: title.clone(), lead, rows }, pics)
    }

    /// One achievement: its picture, when it was earned, how rare it is, its points, and a search for how it's got.
    pub(super) fn achievement_listing(&self) -> (Page, Vec<Option<String>>) {
        let Some((a, game, year)) = &self.trophies.one else { return (Page { title: String::new(), lead: String::new(), rows: Vec::new() }, Vec::new()) };
        let text = |k: &str| a[k].as_str().unwrap_or("").to_string();
        let earned = a["earned"] == true || (a.get("earned").is_none() && a["date"].is_string());
        let mut rows = vec![Row { label: text("title"), value: if earned { "Earned".into() } else { "Not earned yet".into() }, hint: text("desc"), ..Row::default() }];
        let mut pics = vec![a["badge"].as_str().filter(|b| !b.is_empty()).map(String::from)];
        if let Some(d) = a["date"].as_str().filter(|d| !d.is_empty()) {
            rows.push(Row { label: "Earned on".into(), value: day(&a["date"]).to_string(), hint: d.to_string(), ..Row::default() });
        }
        if let Some(r) = a["rarity"].as_f64() {
            rows.push(Row { label: "Rarity".into(), value: format!("{r}%"), hint: format!("{r}% of players have it{}", tier(Some(r)).map_or(String::new(), |t| format!(" · {t}"))), ..Row::default() });
        }
        if let Some(g) = a["grade"].as_str() {
            rows.push(Row { label: "Grade".into(), value: g.into(), ..Row::default() });
        } else if let Some(n) = a["points"].as_f64().filter(|n| *n > 0.0) {
            rows.push(Row { label: "Points".into(), value: n.to_string(), ..Row::default() });
        }
        rows.push(Row { label: "Game".into(), value: match year { Some(y) => format!("{game} ({y})"), None => game.clone() }, ..Row::default() });
        let mut query = format!("how to get {} in {game}", text("title"));
        if let Some(y) = year {
            query.push_str(&format!(" ({y})"));
        }
        let url = format!("https://www.youtube.com/results?{}", rustyshelf_core::js::query(&[("search_query", query.clone())]));
        rows.push(Row {
            label: "How to get it".into(),
            value: "YouTube".into(),
            hint: format!("Searches YouTube for \"{query}\" in your browser."),
            press: Some(Box::new(move |s: &mut Settings| match rustyshelf_core::launch::open_link(&url) {
                Ok(()) => s.say("Opened YouTube in your browser".into()),
                Err(e) => s.say(format!("Couldn't open the browser: {e}")),
            })),
            step: None,
        });
        pics.resize(rows.len() - 1, None);
        pics.push(Some(super::badge::YOUTUBE.into()));
        (Page { title: text("title"), lead: text("desc"), rows }, pics)
    }
}

/// How far a game is, for the top of its page.
fn summary(p: &Value, src: &str) -> String {
    match src {
        "rpcs3" => format!("{} of {} earned · {}", p["earned"], p["total"], p["grades"].as_str().unwrap_or("")),
        "steam" if p["points"].as_f64().is_some_and(|n| n > 0.0) => format!("{} of {} earned · {} of {} points (steamhunters.com)", p["earned"], p["total"], p["pointsEarned"], p["points"]),
        "steam" => format!("{} of {} earned", p["earned"], p["total"]),
        _ => format!("{} of {} earned · {} of {} points", p["earned"], p["total"], p["pointsEarned"], p["points"]),
    }
}
