//! Settings → Controller: the Wii remote on a DolphinBar (Windows).

use super::*;

impl Settings {
    /// Whether the Wii remote is read, and whether it's held sideways.
    pub fn wiimote(&self) -> (bool, bool) {
        (self.flag(&["ui", "wiimote"], true), self.text(&["ui", "wmHold"]) == "sideways")
    }

    pub(super) fn wiimote_rows(&self) -> Vec<Row> {
        if !cfg!(windows) {
            return Vec::new();
        }
        let mut rows = vec![self.switch(
            "Wii remote",
            &["ui", "wiimote"],
            true,
            "A Wii remote on a Mayflash DolphinBar, left in its Dolphin mode (4): its D-pad and buttons work like a controller, and Home opens the Home menu. Dolphin gets the remote for its games; during one, hold Home for a second for RustyShelf's Home menu.",
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
        rows
    }
}
