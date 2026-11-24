//! Settings → PC games → Storage: the installed PC games with a folder, by drive, with the room each drive has.

use rustyshelf_core::storage::{self, Drive, Entry};

use super::*;

#[derive(Default)]
pub(super) struct Storage {
    /// the PC games with a folder, as the library has them
    folders: Vec<Entry>,
    drives: Option<Vec<Drive>>,
    counting: Option<Later<Vec<Drive>>>,
}

/// A size as the Electron app writes it: "12.3 GB", "640 MB".
fn size(bytes: u64) -> String {
    const MB: f64 = 1_048_576.0;
    let b = bytes as f64;
    if b >= 1024.0 * MB {
        format!("{:.1} GB", b / (1024.0 * MB))
    } else if b >= MB {
        format!("{} MB", (b / MB).round())
    } else {
        "0 MB".into()
    }
}

impl Settings {
    /// Keeps the PC games that have a folder, from the library as it was read.
    pub(super) fn note_folders(&mut self, games: &[&Game]) {
        self.storage.folders = games
            .iter()
            .filter(|g| !g.not_installed)
            .filter_map(|g| {
                let (source, dir) = match &g.source {
                    Source::Steam { .. } => ("steam", g.file.to_string_lossy().into_owned()),
                    Source::Pc(pc) if pc.installed != Some(false) && matches!(pc.source, "epic" | "gog" | "hoyoplay" | "folder") => (pc.source, pc.dir.clone()),
                    _ => return None,
                };
                (!dir.is_empty()).then(|| Entry { id: g.id.clone(), title: g.title.clone(), source: source.to_string(), dir, bytes: g.size_bytes })
            })
            .collect();
    }

    /// Looks at the folders and the drives off the frame; again when `again`.
    pub(super) fn load_storage(&mut self, again: bool) {
        if self.storage.counting.is_some() || (self.storage.drives.is_some() && !again) {
            return;
        }
        let games = self.storage.folders.clone();
        self.storage.counting = Some(Later::spawn(move || storage::summary(&games, None, &storage::space)));
    }

    pub(super) fn poll_storage(&mut self) {
        let Some(drives) = self.storage.counting.as_ref().and_then(Later::poll) else { return };
        (self.storage.counting, self.storage.drives, self.redraw) = (None, Some(drives), true);
    }

    pub(super) fn storage_page(&self) -> Page {
        let title = "Storage".to_string();
        let Some(drives) = &self.storage.drives else { return Page { title, lead: "Looking at the games' folders…".into(), rows: Vec::new() } };
        let mut rows = Vec::new();
        for d in drives {
            let room = match (d.free, d.total) {
                (Some(free), Some(total)) => format!("{} free of {} · ", size(free), size(total)),
                _ => String::new(),
            };
            rows.push(Row { label: d.root.clone(), value: format!("{room}games {}", size(d.games_bytes)), hint: format!("{} game{} on this drive", d.games.len(), plural_s(d.games.len())), ..Row::default() });
            for g in &d.games {
                let store = g.source.chars().next().map_or(String::new(), |c| format!(" · {}{}", c.to_uppercase(), &g.source[c.len_utf8()..]));
                rows.push(Row { label: format!("    {}", g.title), value: if g.bytes > 0 { size(g.bytes) } else { "…".into() }, hint: format!("{}{store}", g.dir), ..Row::default() });
            }
        }
        let lead = if drives.is_empty() {
            "No installed PC game with a folder yet. Steam, Epic, GOG, HoYoPlay and PC games folder games show up here once they're installed.".into()
        } else {
            "The PC games that are installed, with the room they take on each drive, the biggest first.".into()
        };
        rows.push(Row { label: "Count again".into(), hint: "Looks at the folders and the drives again. A folder's size is measured behind the shelf and kept for a week.".into(), press: Some(Box::new(|s: &mut Settings| s.load_storage(true))), ..Row::default() });
        Page { title, lead, rows }
    }
}
