// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/homemenu.js (the hotkeys and what an action sends) and renderer/home.js (the menu itself), and
// the key names of main/padwatch.js.
//! The Home menu: over a running game (Resume, Save and Load state, Screenshot, Power, Quit to shelf: only what
//! that emulator can do, by its own hotkeys) and on the shelf (Home shelf, Settings, Power).

use serde_json::{Map, Value};

use crate::js;

/// An emulator's hotkeys for the menu's actions ("f10", "shift+f1" …); `pause` is a toggle in all of them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Keys {
    pub pause: Option<String>,
    pub save: Option<String>,
    pub load: Option<String>,
    pub shot: Option<String>,
    /// the save state slot those keys use, 0 when it isn't known
    pub slot: u32,
}

/// Which emulator a program is, by its name and then by the console it's set for.
pub fn emulator_kind(system: &str, exe: &str) -> &'static str {
    let n = js::basename(exe).to_lowercase();
    let named = |names: &[&str]| names.iter().any(|x| n.contains(x));
    if n.starts_with("retroarch") {
        "retroarch"
    } else if named(&["duckstation"]) {
        "duckstation"
    } else if named(&["dolphin"]) || matches!(system, "gc" | "wii") {
        "dolphin"
    } else if named(&["pcsx2"]) || system == "ps2" {
        "pcsx2"
    } else if named(&["ppsspp"]) || system == "psp" {
        "ppsspp"
    } else if named(&["cemu"]) || system == "wiiu" {
        "cemu"
    } else if named(&["eden", "yuzu", "sudachi", "citron", "suyu", "eglantine"]) || matches!(system, "sw" | "sw2") {
        "eden"
    } else if named(&["xenia"]) || system == "x360" {
        "xenia"
    } else if named(&["shadps4"]) || system == "ps4" {
        "shadps4"
    } else if named(&["rpcs3"]) || system == "ps3" {
        "rpcs3"
    } else if named(&["azahar", "lime3ds", "citra"]) || system == "n3ds" {
        "azahar"
    } else {
        "other"
    }
}

/// Each emulator's default hotkeys. An action it has no key for is left out of the menu.
// (main/homemenu.js HOTKEYS, keep in sync)
fn defaults(kind: &str) -> Keys {
    let k = |pause: &str, save: &str, load: &str, shot: &str, slot: u32| {
        let some = |s: &str| Some(s.to_string()).filter(|s| !s.is_empty());
        Keys { pause: some(pause), save: some(save), load: some(load), shot: some(shot), slot }
    };
    match kind {
        "dolphin" => k("f10", "shift+f1", "f1", "f9", 1),
        "pcsx2" => k("space", "f1", "f3", "f8", 0),
        "ppsspp" => k("", "f2", "f4", "f12", 0),
        "eden" => k("f4", "", "", "ctrl+p", 0),
        "cemu" | "rpcs3" => k("", "", "", "f12", 0),
        "azahar" => k("f4", "ctrl+c", "ctrl+v", "ctrl+p", 0),
        // RetroArch pauses itself when it's no longer in front
        "retroarch" => k("", "f2", "f4", "f8", 0),
        "duckstation" => k("space", "f2", "f1", "f10", 0),
        _ => Keys::default(),
    }
}

/// The hotkeys for a console's emulator, with the console's own (`systems.<id>.hotkeys`) over the defaults.
pub fn hotkeys_for(system: &str, exe: &str, overrides: Option<&Map<String, Value>>) -> Keys {
    let mut keys = defaults(emulator_kind(system, exe));
    let Some(o) = overrides else { return keys };
    for (name, slot) in [("pause", &mut keys.pause), ("save", &mut keys.save), ("load", &mut keys.load), ("shot", &mut keys.shot)] {
        if let Some(v) = o.get(name) {
            *slot = js::truthy(Some(v)).then(|| js::string(Some(v)));
        }
    }
    if let Some(v) = o.get("slot") {
        let n = js::number(Some(v));
        keys.slot = if n.is_finite() && n > 0.0 { n as u32 } else { 0 };
    }
    keys
}

/// The menu's actions over a game with these hotkeys.
pub fn actions_for(k: &Keys) -> Vec<&'static str> {
    let mut a = vec!["resume"];
    for (name, key) in [("save", &k.save), ("load", &k.load), ("shot", &k.shot)] {
        if key.is_some() {
            a.push(name);
        }
    }
    a.extend(["power", "quit"]);
    a
}

/// What an action chosen over a game sends to the emulator, in order ("wait150": a pause of that many
/// milliseconds): its own key, then the pause key again when the game was paused for the menu.
pub fn keys_for(action: &str, k: &Keys, paused: bool) -> Vec<String> {
    let own = match action {
        "save" => &k.save,
        "load" => &k.load,
        "shot" => &k.shot,
        _ => &None,
    };
    let mut keys: Vec<String> = own.iter().cloned().collect();
    if let Some(pause) = k.pause.as_ref().filter(|_| paused) {
        if !keys.is_empty() {
            keys.push("wait150".into());
        }
        keys.push(pause.clone());
    }
    keys
}

/// What's said once an action's keys were sent (`ok`) or couldn't be; None when there's nothing to say.
pub fn toast_for(action: &str, k: &Keys, paused: bool, ok: bool) -> Option<String> {
    let done = match action {
        "save" => "State saved",
        "load" => "State loaded",
        "shot" => "Screenshot saved",
        _ => "",
    };
    if !ok {
        return match action {
            "shot" => Some("Couldn't take the screenshot".into()),
            "save" => Some("Couldn't save the state".into()),
            "load" => Some("Couldn't load the state".into()),
            _ if paused && k.pause.is_some() => Some("Couldn't unpause the game".into()),
            _ => None,
        };
    }
    if done.is_empty() {
        return None;
    }
    Some(if action != "shot" && k.slot > 0 { format!("{done} · slot {}", k.slot) } else { done.to_string() })
}

/// A hotkey as Windows' virtual keys: the modifiers to hold, and the key to tap. None for a name that isn't one.
pub fn virtual_keys(spec: &str) -> Option<(Vec<u8>, u8)> {
    fn vk(name: &str) -> Option<u8> {
        Some(match name {
            "esc" => 0x1b,
            "enter" => 0x0d,
            "space" => 0x20,
            "tab" => 0x09,
            "shift" => 0x10,
            "ctrl" => 0x11,
            "alt" => 0x12,
            _ => {
                let b = name.as_bytes();
                match b {
                    [c @ (b'a'..=b'z')] => c - 32,
                    [c @ (b'0'..=b'9')] => *c,
                    [b'f', digits @ ..] if !digits.is_empty() && digits[0] != b'0' => match name[1..].parse::<u8>() {
                        Ok(n @ 1..=12) => 0x6f + n,
                        _ => return None,
                    },
                    _ => return None,
                }
            }
        })
    }
    let lower = spec.to_lowercase();
    let parts: Vec<&str> = lower.split('+').collect();
    let (last, mods) = parts.split_last()?;
    Some((mods.iter().filter_map(|m| vk(m)).collect(), vk(last)?))
}

/// How long a game has been played, as the menu says it: "1 min", "59 min", "2 h", "2 h 5 min".
pub fn played_for(ms: f64) -> String {
    let m = js::round(ms / 60_000.0).max(1.0) as u64;
    match (m / 60, m % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// Whose letters the pad's buttons carry.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Glyphs {
    #[default]
    Xbox,
    Ps,
    Nintendo,
}

/// What the menu is opened with.
#[derive(Debug, Clone, Default)]
pub struct Info {
    pub in_game: bool,
    pub title: String,
    pub system: String,
    /// the line under the title on the shelf; over a game it's how long it has been played
    pub subtitle: String,
    pub actions: Vec<String>,
    pub power: Vec<String>,
    pub can_save: bool,
    pub slot: u32,
    pub paused: bool,
    /// the centred card instead of the panel at the side
    pub center: bool,
    pub glyphs: Glyphs,
    /// the right face button confirms (Settings → Controller)
    pub swap_ab: bool,
}

/// What a press led to.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    /// a sound to play, and which way a move went
    Cue(&'static str, i32),
    /// the menu closed with this action: first its sound, then what to do
    Close(&'static str, String),
    /// the running game's achievements are to be read (again, now, when true)
    Ach(bool),
}

/// A row of the menu as it's drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: String,
    pub label: String,
    pub sub: String,
    pub selected: bool,
    /// needs the button held, so a stray press never does it
    pub danger: bool,
    /// a line is drawn above it
    pub sep_before: bool,
}

/// how long a held action's button is held, in milliseconds
const HOLD_MS: f64 = 900.0;

fn needs_hold(id: &str) -> bool {
    matches!(id, "quit" | "restart" | "shutdown" | "quitapp")
}

pub fn label(id: &str) -> &str {
    match id {
        "resume" => "Resume",
        "save" => "Save state",
        "load" => "Load state",
        "shot" => "Screenshot",
        "quit" => "Quit to shelf",
        "power" => "Power",
        "shelfhome" => "Home shelf",
        "museum" => "Museum",
        "settings" => "Settings",
        "desktop" => "Go to desktop",
        "sleep" => "Sleep",
        "restart" => "Restart",
        "shutdown" => "Shut down",
        "quitapp" => "Quit RustyShelf",
        "back" => "Back",
        "ach" => "Achievements",
        other => other,
    }
}

/// The open menu: its rows, the one chosen, and a held button's progress.
#[derive(Debug, Clone)]
pub struct Menu {
    pub info: Info,
    main: Vec<String>,
    power: Vec<String>,
    items: Vec<String>,
    in_power: bool,
    sel: usize,
    main_sel: usize,
    /// since when the button is held on a row that needs it, how full its bar was drawn, and whether a full bar
    /// has been on screen
    hold: Option<(f64, f64, bool)>,
    /// the Achievements item's name ("Achievements", or "Trophies" for a PS3 game)
    ach_label: String,
    /// on the Achievements page: the one chosen, of how many
    ach: Option<(usize, usize)>,
}

impl Menu {
    pub fn open(info: Info) -> Menu {
        let list = |l: &[String], usual: &[&str]| if l.is_empty() { usual.iter().map(|s| s.to_string()).collect() } else { l.to_vec() };
        let (main, power) = (list(&info.actions, &["resume", "quit"]), list(&info.power, &["sleep", "restart", "shutdown"]));
        Menu { items: main.clone(), main, power, info, in_power: false, sel: 0, main_sel: 0, hold: None, ach_label: String::new(), ach: None }
    }

    /// The running game has achievements: their item goes in before Power, the row chosen staying chosen.
    pub fn add_ach(&mut self, label: &str) {
        if self.main.iter().any(|i| i == "ach") {
            return;
        }
        let at = self.main.iter().position(|i| i == "power").unwrap_or(self.main.len());
        self.main.insert(at, "ach".into());
        self.ach_label = label.to_string();
        if self.in_power || self.ach.is_some() {
            if self.main_sel >= at {
                self.main_sel += 1;
            }
        } else {
            self.items = self.main.clone();
            if self.sel >= at {
                self.sel += 1;
            }
        }
    }

    /// On the Achievements page: the one chosen.
    pub fn ach(&self) -> Option<usize> {
        self.ach.map(|a| a.0)
    }

    /// The page's list came: `n` of them, `first` chosen.
    pub fn ach_loaded(&mut self, n: usize, first: usize) {
        if self.ach.is_some() {
            self.ach = Some((first.min(n.saturating_sub(1)), n));
        }
    }

    pub fn in_power(&self) -> bool {
        self.in_power
    }

    /// The face button that confirms ("ok") or goes back, as the pad in hand prints it.
    pub fn glyph(&self, ok: bool) -> &'static str {
        let bottom = ok != self.info.swap_ab;
        match (self.info.glyphs, bottom) {
            (Glyphs::Ps, true) => "✕",
            (Glyphs::Ps, false) => "○",
            (Glyphs::Nintendo, true) => "B",
            (Glyphs::Nintendo, false) => "A",
            (Glyphs::Xbox, true) => "A",
            (Glyphs::Xbox, false) => "B",
        }
    }

    fn sub_for(&self, id: &str) -> String {
        let i = &self.info;
        match id {
            "restart" | "shutdown" if i.in_game => format!("closes the game · hold {}", self.glyph(true)),
            _ if needs_hold(id) => format!("hold {}", self.glyph(true)),
            "save" | "load" if i.slot > 0 => format!("slot {}", i.slot),
            "sleep" if i.in_game => if i.can_save { "saves, then sleeps" } else if i.paused { "game stays paused" } else { "game keeps running" }.into(),
            "resume" if i.in_game && !i.paused => "game is still running".into(),
            "power" | "ach" => "›".into(),
            _ => String::new(),
        }
    }

    pub fn rows(&self) -> Vec<Row> {
        self.items
            .iter()
            .enumerate()
            .map(|(n, id)| Row { label: if id == "ach" && !self.ach_label.is_empty() { self.ach_label.clone() } else { label(id).to_string() }, sub: self.sub_for(id), selected: n == self.sel, danger: needs_hold(id), sep_before: !self.in_power && n > 0 && matches!(id.as_str(), "power" | "quit"), id: id.clone() })
            .collect()
    }

    /// The prompts under the menu: a button, and what it does.
    pub fn foot(&self) -> Vec<(&'static str, &'static str)> {
        let leave = if self.info.in_game { "Resume" } else { "Close" };
        if self.ach.is_some() {
            return vec![(self.glyph(true), "Ask again"), (self.glyph(false), "Back"), ("↕", "Move")];
        }
        vec![(self.glyph(true), "Select"), (self.glyph(false), if self.in_power { "Back" } else { leave }), ("↕", "Move"), (if self.info.glyphs == Glyphs::Nintendo { "Home" } else { "Guide" }, leave)]
    }

    /// The title, and the line under it at `now` for a game started at `started` (both in milliseconds).
    pub fn heading(&self, started: f64, now: f64) -> (String, String) {
        let title = if !self.info.title.is_empty() { self.info.title.clone() } else if self.info.in_game { "Now playing".into() } else { "Home".into() };
        (title, if started > 0.0 { format!("Playing for {}", played_for(now - started)) } else { self.info.subtitle.clone() })
    }

    /// How full the held button's bar is, 0 to 1.
    pub fn held(&self) -> f64 {
        self.hold.map_or(0.0, |h| h.1)
    }

    fn step(&mut self, by: i32) -> Option<Out> {
        if self.items.is_empty() {
            return None;
        }
        let n = self.items.len() as i32;
        self.sel = (self.sel as i32 + by).rem_euclid(n) as usize;
        self.hold = None;
        Some(Out::Cue("tick", by.signum()))
    }

    fn choose(&mut self) -> Option<Out> {
        self.hold = None;
        match self.items.get(self.sel)?.as_str() {
            "power" => {
                (self.main_sel, self.in_power, self.sel) = (self.sel, true, 0);
                self.items = self.power.iter().cloned().chain(["back".to_string()]).collect();
                Some(Out::Cue("open", 0))
            }
            "back" => self.leave_power(),
            "ach" => {
                (self.main_sel, self.ach) = (self.sel, Some((0, 0)));
                Some(Out::Ach(false))
            }
            id => Some(close(id)),
        }
    }

    fn leave_power(&mut self) -> Option<Out> {
        (self.in_power, self.sel, self.hold) = (false, self.main_sel, None);
        self.items = self.main.clone();
        Some(Out::Cue("back", 0))
    }

    /// A button going down or coming up at `now` (ms): "up", "down", "left", "right", "a", "b", "start", "home",
    /// and "ok" / "cancel", which select and go back whichever face button confirms.
    pub fn press(&mut self, button: &str, down: bool, now: f64) -> Option<Out> {
        let button = match button {
            "ok" => "a",
            "cancel" => "b",
            "a" if self.info.swap_ab => "b",
            "b" if self.info.swap_ab => "a",
            other => other,
        };
        if let Some((sel, n)) = self.ach {
            let by = match button {
                "up" if down => -1,
                "down" if down => 1,
                _ => 0,
            };
            return match button {
                _ if by != 0 && n > 0 => {
                    let to = (sel as i32 + by).clamp(0, n as i32 - 1) as usize;
                    self.ach = Some((to, n));
                    (to != sel).then_some(Out::Cue("tick", by))
                }
                "a" if down => Some(Out::Ach(true)),
                "b" if down => {
                    (self.ach, self.sel) = (None, self.main_sel);
                    Some(Out::Cue("back", 0))
                }
                "home" | "start" if down => Some(close("resume")),
                _ => None,
            };
        }
        let grid = self.info.center && !self.in_power;
        match button {
            "up" if down => self.step(if grid { -3 } else { -1 }),
            "left" if down && grid => self.step(-1),
            "down" if down => self.step(if grid { 3 } else { 1 }),
            "right" if down && grid => self.step(1),
            "a" if self.items.get(self.sel).is_some_and(|id| needs_hold(id)) => {
                self.hold = down.then_some((now, 0.0, false));
                None
            }
            "a" if down => self.choose(),
            "b" if down && self.in_power => self.leave_power(),
            "b" | "home" | "start" if down => Some(close("resume")),
            _ => None,
        }
    }

    /// A frame at `now` (ms) while a button is held. The bar is the timer: it never jumps more than a step a
    /// frame, and the action only goes once a full bar has been on screen.
    pub fn frame(&mut self, now: f64) -> Option<Out> {
        let (since, shown, full) = self.hold?;
        if full {
            return self.choose();
        }
        let p = ((now - since) / HOLD_MS).min(shown + 0.08).min(1.0);
        self.hold = Some((since, p, p >= 1.0));
        None
    }
}

fn close(action: &str) -> Out {
    Out::Close(if matches!(action, "resume" | "quit") { "back" } else { "open" }, action.to_string())
}

/// The Achievements page's summary of a game's progress (as RetroAchievements, Xenia's or RPCS3's files give it):
/// what's unlocked, the share unlocked in percent, and the points line ending with where it's from.
pub fn ach_summary(p: &Value, src: &str) -> (String, u32, String) {
    let n = |k: &str| p.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let (earned, total) = (n("earned"), n("total"));
    let pct = if total > 0.0 { (100.0 * earned / total).round() as u32 } else { 0 };
    let mastered = p.get("mastered").and_then(Value::as_str).filter(|m| !m.is_empty());
    let state = match mastered {
        Some("mastered") => "Mastered".to_string(),
        Some("platinum") => "Platinum".to_string(),
        Some("completed") => "Completed".to_string(),
        _ => format!("{earned} of {total} unlocked"),
    };
    let points = match p.get("unit").and_then(Value::as_str) {
        Some("G") => format!("{} / {} G", n("pointsEarned"), n("points")),
        Some("trophy") => p.get("grades").and_then(Value::as_str).filter(|g| !g.is_empty()).map_or_else(|| format!("{earned} / {total} trophies"), str::to_string),
        _ => format!("{} / {} points", n("pointsEarned"), n("points")),
    };
    let line = format!("{}{points} · {}", if mastered.is_some() { format!("{earned} of {total} unlocked · ") } else { String::new() }, src);
    (state, pct, line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_achievements_item_and_page() {
        let mut m = Menu::open(Info { in_game: true, actions: ["resume", "save", "power", "quit"].map(String::from).to_vec(), ..Info::default() });
        m.press("down", true, 0.0);
        m.press("down", true, 0.0);
        m.add_ach("Trophies");
        let rows = m.rows();
        assert_eq!(rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["resume", "save", "ach", "power", "quit"]);
        // the row that was chosen (Power) still is; the item is named for the game's kind
        assert!(rows[3].selected);
        assert_eq!((rows[2].label.as_str(), rows[2].sub.as_str()), ("Trophies", "›"));
        m.press("up", true, 0.0);
        assert_eq!(m.press("a", true, 0.0), Some(Out::Ach(false)));
        m.ach_loaded(3, 1);
        assert_eq!(m.ach(), Some(1));
        m.press("down", true, 0.0);
        assert_eq!(m.press("down", true, 0.0), None);
        assert_eq!(m.ach(), Some(2));
        assert_eq!(m.press("a", true, 0.0), Some(Out::Ach(true)));
        assert_eq!(m.press("b", true, 0.0), Some(Out::Cue("back", 0)));
        assert_eq!(m.ach(), None);
        assert!(m.rows()[2].selected);
    }

    #[test]
    fn an_achievements_summary() {
        let ra = serde_json::json!({ "earned": 3, "total": 12, "points": 100, "pointsEarned": 25 });
        assert_eq!(ach_summary(&ra, "RetroAchievements"), ("3 of 12 unlocked".into(), 25, "25 / 100 points · RetroAchievements".into()));
        let x = serde_json::json!({ "earned": 2, "total": 2, "points": 30, "pointsEarned": 30, "unit": "G", "mastered": "completed" });
        assert_eq!(ach_summary(&x, "Xenia"), ("Completed".into(), 100, "2 of 2 unlocked · 30 / 30 G · Xenia".into()));
        let ps = serde_json::json!({ "earned": 1, "total": 3, "unit": "trophy", "grades": "1 gold" });
        assert_eq!(ach_summary(&ps, "RPCS3").2, "1 gold · RPCS3");
    }

    #[test]
    fn an_action_sends_its_key_then_unpauses() {
        let k = hotkeys_for("gc", "Dolphin.exe", None);
        assert_eq!(actions_for(&k), ["resume", "save", "load", "shot", "power", "quit"]);
        assert_eq!(actions_for(&hotkeys_for("x360", "xenia.exe", None)), ["resume", "power", "quit"]);
        assert_eq!(keys_for("save", &k, true), ["shift+f1", "wait150", "f10"]);
        assert_eq!(keys_for("save", &k, false), ["shift+f1"]);
        assert_eq!(keys_for("resume", &k, true), ["f10"]);
        assert!(keys_for("resume", &k, false).is_empty());
        // no pause key: nothing to undo
        assert_eq!(keys_for("shot", &hotkeys_for("psp", "", None), true), ["f12"]);
        assert_eq!(toast_for("save", &k, true, true).as_deref(), Some("State saved · slot 1"));
        assert_eq!(toast_for("shot", &k, true, true).as_deref(), Some("Screenshot saved"));
        assert_eq!(toast_for("load", &hotkeys_for("ps2", "", None), true, true).as_deref(), Some("State loaded"));
        assert_eq!(toast_for("resume", &k, true, true), None);
        assert_eq!(toast_for("load", &k, false, false).as_deref(), Some("Couldn't load the state"));
        assert_eq!(toast_for("resume", &k, true, false).as_deref(), Some("Couldn't unpause the game"));
        assert_eq!(toast_for("resume", &k, false, false), None);
    }

    #[test]
    fn hotkeys_are_named_as_the_helper_names_them() {
        assert_eq!(virtual_keys("f10"), Some((vec![], 0x79)));
        assert_eq!(virtual_keys("Shift+F1"), Some((vec![0x10], 0x70)));
        assert_eq!(virtual_keys("ctrl+alt+p"), Some((vec![0x11, 0x12], 0x50)));
        assert_eq!(virtual_keys("space"), Some((vec![], 0x20)));
        assert_eq!(virtual_keys("7"), Some((vec![], 0x37)));
        assert_eq!(virtual_keys("f12"), Some((vec![], 0x7b)));
        // a modifier that isn't one is left out; a key that isn't one sends nothing
        assert_eq!(virtual_keys("win+f1"), Some((vec![], 0x70)));
        for bad in ["f13", "f0", "f01", "pause", "", "shift+", "é"] {
            assert_eq!(virtual_keys(bad), None, "{bad}");
        }
    }
}
