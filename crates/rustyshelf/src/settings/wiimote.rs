//! Settings → Controller: the Wii remote on a DolphinBar (Windows).

use super::*;

impl Settings {
    /// Whether the Wii remote is read, and whether it's held sideways.
    pub fn wiimote(&self) -> (bool, bool) {
        (self.flag(&["ui", "wiimote"], true), self.text(&["ui", "wmHold"]) == "sideways")
    }

    /// How far the hand moves for a turn of the remote.
    pub fn wiimote_speed(&self) -> f32 {
        at(&self.cfg, &["ui", "wmSpeed"]).and_then(Value::as_f64).filter(|n| (0.8..=3.0).contains(n)).map_or(1.5, |n| n as f32)
    }

    pub(super) fn wiimote_rows(&self) -> Vec<Row> {
        if !cfg!(windows) {
            return Vec::new();
        }
        let mut rows = vec![self.switch(
            "Wii remote",
            &["ui", "wiimote"],
            true,
            "A Wii remote on a Mayflash DolphinBar, left in its Dolphin mode (4): point at the screen to move the hand, press A to pick a case. Its D-pad and buttons work like a controller, and Home opens the Home menu. Dolphin gets the remote for its games; during one, hold Home for a second for RustyShelf's Home menu.",
        )];
        if self.wiimote().0 {
            let sideways = self.wiimote().1;
            let flip = move |s: &mut Settings| s.set(&["ui", "wmHold"], json!(if sideways { "upright" } else { "sideways" }));
            rows.push(Row {
                label: "Hold the remote".into(),
                value: if sideways { "Sideways" } else { "Upright (pointing)" }.into(),
                hint: "Upright: A selects, B goes back. Sideways, like a classic pad: the D-pad turned, 2 selects, 1 goes back.".into(),
                press: Some(Box::new(flip)),
                step: Some(Box::new(move |s: &mut Settings, _| flip(s))),
            });
        }
        if self.wiimote().0 {
            let speed = self.wiimote_speed();
            let turn = move |s: &mut Settings, d: i32| {
                let n = ((speed + d as f32 * 0.1).clamp(0.8, 3.0) * 10.0).round() / 10.0;
                s.set(&["ui", "wmSpeed"], rustyshelf_core::js::num(f64::from(n)));
            };
            rows.push(Row::turning("Pointer speed", format!("{speed:.1}×"), "How far the hand moves for a turn of the remote: higher covers the screen with a smaller turn.", turn));
        }
        rows
    }
}
