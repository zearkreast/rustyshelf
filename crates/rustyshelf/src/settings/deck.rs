// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's renderer/settings.js (deckRows), as RustyShelf has it.
//! Settings → System → Add RustyShelf to Steam (Linux, outside Game Mode, where it's in Steam already), and the
//! Steam Deck card's once-only flag (`ui.deckHint`).

use rustyshelf_core::steamdeck;

use super::*;

#[derive(Default)]
pub(super) struct DeckUi {
    info: Option<steamdeck::Info>,
    adding: Option<Mutex<Receiver<String>>>,
}

impl Settings {
    pub fn deck_hint_seen(&self) -> bool {
        self.flag(&["ui", "deckHint"], false)
    }

    pub fn set_deck_hint_seen(&mut self) {
        put(&mut self.cfg, &["ui", "deckHint"], json!(true));
        self.save();
    }

    pub(super) fn load_deck(&mut self) {
        self.deck.info = Some(steamdeck::info(&paths::env_var));
    }

    pub(super) fn poll_deck(&mut self) {
        let Some(said) = self.deck.adding.as_ref().and_then(|rx| rx.lock().ok()?.try_recv().ok()) else { return };
        self.deck.adding = None;
        self.load_deck();
        self.say(said);
    }

    pub(super) fn deck_rows(&self) -> Vec<Row> {
        let Some(d) = self.deck.info.as_ref().filter(|d| d.can_add && !d.game_mode) else { return Vec::new() };
        let busy = self.deck.adding.is_some();
        vec![Row {
            label: if busy { "Adding RustyShelf to Steam…" } else { "Add RustyShelf to Steam" }.into(),
            value: if busy { String::new() } else if d.in_steam { "Update".into() } else { "Add".into() },
            hint: format!(
                "{}. Adds RustyShelf to Steam's Library as a non-Steam game, so you can start it from Steam and Game Mode. Started from Steam, the controller works as a gamepad in your games (outside Steam on a Steam Deck, B is the Esc key, and most emulators quit their game on it). Steam closes and opens again while it's added; its old list is backed up first.",
                if d.in_steam { "In Steam already" } else if d.deck { "Recommended on a Steam Deck" } else { "Not in Steam yet" }
            ),
            press: Some(Box::new(|s: &mut Settings| {
                if s.deck.adding.is_none() {
                    s.deck.adding = Some(Mutex::new(crate::deck::add(false)));
                    s.redraw = true;
                }
            })),
            step: None,
        }]
    }
}
