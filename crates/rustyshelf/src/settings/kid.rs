// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's renderer/settings.js and renderer/app.js (Kid mode).
//! Kid mode: a PIN keeps Settings (and hiding, box art and Exit) for grown-ups, and only the shelves ticked show.
//! `ui.kid = { on, pin (a hash of the PIN, never the PIN), hidden: [shelf ids] }`, as the Electron app keeps it.
//! While it's on, Settings opens on a PIN page; the right PIN opens the rest until Settings closes.

use super::*;

/// What a kid may still use while Settings is locked: the game's Details, its achievements, Find, the shelf's menu.
fn kid_free(screen: &Screen) -> bool {
    matches!(screen, Screen::Details | Screen::Achievements | Screen::Find | Screen::Shelf)
}

/// The PIN as it's kept (renderer/settings.js kidHash: FNV-1a over "bs-kid|" and the PIN; keep in sync).
pub fn kid_hash(pin: &str) -> String {
    let mut h: u32 = 2_166_136_261;
    for c in format!("bs-kid|{pin}").chars() {
        let mut unit = [0u16; 2];
        h ^= u32::from(c.encode_utf16(&mut unit)[0]);
        h = h.wrapping_mul(16_777_619);
    }
    format!("{h:08x}")
}

/// Whether Kid mode is on (it needs a PIN to be), from the settings' `ui`.
pub fn kid_mode_on(ui: Option<&Value>) -> bool {
    let k = ui.and_then(|u| u.get("kid"));
    k.is_some_and(|k| k["on"] == true && k["pin"].as_str().is_some_and(|p| !p.is_empty()))
}

/// The shelves kids don't see, from the settings' `ui`.
pub fn kid_hidden(ui: Option<&Value>) -> Vec<String> {
    if !kid_mode_on(ui) {
        return Vec::new();
    }
    ui.and_then(|u| u["kid"]["hidden"].as_array()).map_or(Vec::new(), |h| h.iter().filter_map(|v| v.as_str().map(String::from)).collect())
}

impl Settings {
    pub fn kid_on(&self) -> bool {
        kid_mode_on(self.cfg.get("ui"))
    }

    /// Kid mode is on and the PIN hasn't been given since Settings opened.
    pub(super) fn kid_locked(&self) -> bool {
        self.kid_on() && !self.kid_open
    }

    /// The page shown in Settings' place while it's locked; None when `screen` is one a kid may use.
    pub(super) fn kid_lock_page(&self, screen: &Screen) -> Option<Page> {
        if !self.kid_locked() || kid_free(screen) {
            return None;
        }
        let rows = vec![
            Row {
                label: "PIN".into(),
                hint: "The PIN chosen when Kid mode was turned on.".into(),
                press: Some(Box::new(|s: &mut Settings| s.edit("PIN", "Enter the PIN", String::new(), Box::new(|s: &mut Settings, v: String| s.kid_try(&v))))),
                ..Row::default()
            },
            Row { label: "Forgot the PIN?".into(), hint: "Close RustyShelf, open config.json in its data folder and delete the \"kid\" part; Kid mode is off when it starts again".into(), ..Row::default() },
        ];
        Some(Page { title: "Kid mode".into(), lead: "Kid mode is on. Enter the PIN to change settings.".into(), rows })
    }

    fn kid_try(&mut self, pin: &str) {
        if kid_hash(pin.trim()) != self.text(&["ui", "kid", "pin"]) {
            self.cues.push(("error", 1));
            self.say("That's not the PIN".into());
            return;
        }
        self.kid_open = true;
        self.cues.push(("open", 1));
        self.say("Unlocked until Settings closes".into());
    }

    /// Settings → System → Kid mode.
    pub(super) fn kid_page(&self) -> Page {
        let on = self.kid_on() || self.flag(&["ui", "kid", "on"], false);
        let has_pin = !self.text(&["ui", "kid", "pin"]).is_empty();
        let hidden = self.list(&["ui", "kid", "hidden"]);
        let mut rows = vec![
            Row {
                label: "Kid mode".into(),
                value: on_off(on),
                hint: "Only the shelves ticked below show, and Settings, hiding, box art and Exit need the PIN. Playing, Details, manuals and the museum work as usual.".into(),
                press: Some(Box::new(move |s: &mut Settings| s.kid_switch(!on))),
                step: Some(Box::new(move |s: &mut Settings, _| s.kid_switch(!on))),
            },
            Row {
                label: "PIN".into(),
                value: if has_pin { "Set · press to change it" } else { "Choose one before turning Kid mode on" }.into(),
                hint: "4 to 8 digits. RustyShelf keeps only a scrambled copy, never the PIN itself.".into(),
                press: Some(Box::new(|s: &mut Settings| s.edit("PIN", "4 to 8 digits", String::new(), Box::new(|s: &mut Settings, v: String| s.kid_set_pin(&v))))),
                step: None,
            },
        ];
        let shelves = self.kid_shelves();
        let shown = shelves.iter().filter(|(id, _)| !hidden.contains(id)).count();
        rows.push(Row { label: format!("— Shelves kids can see · {shown} of {} —", shelves.len()), ..Row::default() });
        for (id, name) in shelves {
            let see = !hidden.contains(&id);
            let (a, b) = (id.clone(), id.clone());
            rows.push(Row {
                label: name.clone(),
                value: on_off(see),
                hint: format!("On: the {name} shelf shows while Kid mode is on."),
                press: Some(Box::new(move |s: &mut Settings| s.kid_shelf(&a))),
                step: Some(Box::new(move |s: &mut Settings, _| s.kid_shelf(&b))),
            });
        }
        rows.push(Row { label: "Forgot the PIN?".into(), hint: "Close RustyShelf, open config.json in its data folder and delete the \"kid\" part".into(), ..Row::default() });
        Page { title: "Kid mode".into(), lead: "A PIN for Settings, and only the shelves you choose.".into(), rows }
    }

    /// The shelves Kid mode can hide: the consoles that are on, the PC, Custom, and Media when an app is set up.
    fn kid_shelves(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = systems().consoles.iter().filter(|c| self.cfg["systems"][&c.id]["enabled"] != false).map(|c| (c.id.clone(), c.name.clone())).collect();
        if self.cfg["steam"]["enabled"] != false {
            out.push(("pc".into(), "PC".into()));
        }
        out.push(("custom".into(), "Custom".into()));
        if self.cfg["apps"].as_object().is_some_and(|a| a.values().any(|a| a["enabled"] != false && a["path"].as_str().is_some_and(|p| !p.is_empty()))) {
            out.push(("media".into(), "Media".into()));
        }
        out
    }

    fn kid_switch(&mut self, on: bool) {
        if on && self.text(&["ui", "kid", "pin"]).is_empty() {
            self.say("Choose a PIN first, just below".into());
            return;
        }
        self.set(&["ui", "kid", "on"], json!(on));
        if !on {
            self.kid_open = false;
        }
        self.rearranged = true;
    }

    fn kid_set_pin(&mut self, pin: &str) {
        let pin = pin.trim();
        if !(4..=8).contains(&pin.len()) || !pin.bytes().all(|b| b.is_ascii_digit()) {
            self.say("A PIN is 4 to 8 digits".into());
            return;
        }
        self.set(&["ui", "kid", "pin"], json!(kid_hash(pin)));
        if self.cfg["ui"]["kid"]["hidden"].as_array().is_none() {
            self.set(&["ui", "kid", "hidden"], json!([]));
        }
        self.kid_open = true;
        let said = if self.kid_on() { "PIN changed" } else { "PIN set: now turn Kid mode on" };
        self.say(said.into());
    }

    fn kid_shelf(&mut self, id: &str) {
        let mut hidden = self.list(&["ui", "kid", "hidden"]);
        match hidden.iter().position(|h| h == id) {
            Some(i) => {
                hidden.remove(i);
            }
            None => hidden.push(id.to_string()),
        }
        self.set(&["ui", "kid", "hidden"], json!(hidden));
        self.rearranged = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pin_is_kept_as_the_electron_app_keeps_it() {
        // renderer/settings.js kidHash("1234"), worked out in node
        assert_eq!(kid_hash("1234"), "035eaa69");
        let ui = json!({ "kid": { "on": true, "pin": kid_hash("1234"), "hidden": ["ps2"] } });
        assert!(kid_mode_on(Some(&ui)));
        assert_eq!(kid_hidden(Some(&ui)), ["ps2"]);
        assert!(kid_hidden(Some(&json!({ "kid": { "on": true, "pin": "", "hidden": ["ps2"] } }))).is_empty());
        assert!(!kid_mode_on(None));
    }
}
