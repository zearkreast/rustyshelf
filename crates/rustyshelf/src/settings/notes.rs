//! Settings → System → What's new (the native app's own notes, shipped with it), and the shelf tour's switch.

use super::*;

/// One version's notes; newest first in the file.
#[derive(Debug, Clone)]
pub struct Notes {
    pub version: String,
    pub name: String,
    pub date: String,
    pub title: String,
    pub highlights: Vec<String>,
    pub sections: Vec<NoteSection>,
}

#[derive(Debug, Clone)]
pub struct NoteSection {
    pub h: String,
    pub items: Vec<(String, String)>,
}

pub fn notes() -> Vec<Notes> {
    let all: Value = serde_json::from_str(include_str!("../../assets/whatsnew.json")).unwrap_or_default();
    let s = |v: &Value, k: &str| v[k].as_str().unwrap_or("").to_string();
    let list = |v: &Value| v.as_array().cloned().unwrap_or_default();
    list(&all)
        .iter()
        .map(|n| Notes {
            version: s(n, "version"),
            name: s(n, "name"),
            date: s(n, "date"),
            title: s(n, "title"),
            highlights: list(&n["highlights"]).iter().filter_map(|h| h.as_str().map(str::to_string)).collect(),
            sections: list(&n["sections"])
                .iter()
                .map(|sec| NoteSection { h: s(sec, "h"), items: list(&sec["items"]).iter().map(|i| (i[0].as_str().unwrap_or("").to_string(), i[1].as_str().unwrap_or("").to_string())).collect() })
                .collect(),
        })
        .collect()
}

impl Settings {
    pub fn language(&self) -> String {
        self.text(&["ui", "language"])
    }

    pub fn setup_done(&self) -> bool {
        self.flag(&["setupDone"], false)
    }

    pub fn tour_done(&self) -> bool {
        self.flag(&["ui", "tourDone"], false)
    }

    pub fn set_tour_done(&mut self) {
        self.set(&["ui", "tourDone"], json!(true));
    }

    pub fn open_whatsnew(&mut self) {
        self.stack = vec![(Screen::Root, 0), (Screen::System, 0)];
        self.go(Screen::WhatsNew);
        self.open = true;
        self.redraw = true;
    }

    /// Closes Settings straight back to the shelf.
    fn back_to_shelf(&mut self) {
        self.stack = vec![(Screen::Root, 0)];
        (self.open, self.just_closed, self.redraw) = (false, true, true);
        self.note.clear();
    }

    pub(super) fn notes_rows(&self) -> Vec<Row> {
        vec![
            Row {
                label: "What's new".into(),
                hint: "Every version's notes".into(),
                press: Some(Box::new(|s: &mut Settings| s.go(Screen::WhatsNew))),
                ..Row::default()
            },
            Row {
                label: "Show the tour".into(),
                hint: "A one-minute look at the basics of the shelf".into(),
                press: Some(Box::new(|s: &mut Settings| {
                    crate::tour::ASKED.store(true, std::sync::atomic::Ordering::Relaxed);
                    s.back_to_shelf();
                })),
                ..Row::default()
            },
        ]
    }

    pub(super) fn whatsnew_page(&self) -> Page {
        // a heading per version, its title with the highlights in the side card, then a row per item with what kind
        // of change it is beside it and the rest of it in the side card
        let mut rows = Vec::new();
        for (i, n) in notes().iter().enumerate() {
            rows.push(Row { label: format!("— {}{} · {} —", if i == 0 { "This version · " } else { "" }, n.name, n.date), ..Row::default() });
            rows.push(Row { label: n.title.clone(), hint: n.highlights.iter().map(|h| format!("•  {h}")).collect::<Vec<_>>().join("\n"), ..Row::default() });
            for sec in &n.sections {
                for (h, more) in &sec.items {
                    rows.push(Row { label: h.clone(), value: sec.h.clone(), hint: if more.is_empty() { h.clone() } else { more.clone() }, ..Row::default() });
                }
            }
        }
        Page { title: "What's new".into(), lead: "The native app's notes, newest first.".into(), rows }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_notes_read() {
        let n = super::notes();
        assert!(n.len() >= 3 && n.iter().all(|v| !v.version.is_empty() && !v.title.is_empty() && !v.highlights.is_empty()));
        assert!(n.windows(2).all(|w| w[0].date >= w[1].date), "newest first");
    }
}
