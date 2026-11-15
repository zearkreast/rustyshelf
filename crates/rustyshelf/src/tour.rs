//! The shelf tour: a one-minute look at the basics, a card a step, the first time the shelf is up after setup, and
//! again from Settings → System → Show the tour. Next goes on, Skip (or B / Esc) ends it for good; it never comes
//! back on its own. A new install owes it from its first start (data/tour-pending), so closing RustyShelf right after
//! setup brings it on the next start.

use std::sync::atomic::{AtomicBool, Ordering};

use bevy::prelude::*;
use rustyshelf_core::paths;
use rustyshelf_core::scanner::Source;

use crate::card::{Card, Content, Outcome};
use crate::loading::{LibraryRes, LibraryState};
use crate::play::Playing;
use crate::settings::Settings;

/// Settings → System → Show the tour was pressed.
pub static ASKED: AtomicBool = AtomicBool::new(false);

/// How long the shelf has to be still before the tour starts on its own (seconds).
const SETTLE: f32 = 2.5;

struct Step {
    k: &'static str,
    t: &'static str,
    p: &'static str,
    keys: &'static [(&'static str, &'static str)],
    /// only for a console game, which has a case to hold
    rom_only: bool,
}

const STEPS: [Step; 9] = [
    Step { k: "Welcome", t: "Your shelf, in a minute", p: "A quick look at where things are. Skip it any time; it won’t come back on its own.", keys: &[], rom_only: false },
    Step { k: "Consoles", t: "Switch consoles", p: "Each console has its own shelf. The tabs show which one you’re on.", keys: &[("L2 R2 · ↑ ↓", "Next or previous console"), ("L1 R1 · Z C", "Jump by letter")], rom_only: false },
    Step { k: "Your games", t: "Pick a game", p: "Move along the shelf with the stick, the d-pad or the arrow keys.", keys: &[("A · Enter", "Play"), ("Y · F", "Flip to the back"), ("Hold Y · H", "Favourite")], rom_only: false },
    Step { k: "Find a game", t: "Search every shelf", p: "Type a few letters of a title and pick the game from the list. The shelf goes straight to it, whichever console it’s on.", keys: &[("Hold L3 · /", "Find a game")], rom_only: false },
    Step { k: "Details", t: "Everything about a game", p: "Play it, make it a favourite, read its manual and see its screenshots.", keys: &[("X · I", "Open Details")], rom_only: false },
    Step { k: "Inspect", t: "Hold the case", p: "Turn it round and look at it close up.", keys: &[("R3 · V", "Inspect")], rom_only: true },
    Step { k: "Shelf menu", t: "The shelf menu", p: "The order of every shelf, which games it shows, and the chosen game.", keys: &[("Start · G", "Shelf menu")], rom_only: false },
    Step { k: "Adding games", t: "Add more games", p: "Consoles: point RustyShelf at a games folder. PC games and Custom games: add anything else, one at a time or a whole folder.", keys: &[("S", "Settings")], rom_only: false },
    Step { k: "Done", t: "That’s the basics", p: "Watch this one again from Settings → System → Show the tour.", keys: &[("Hold View · M", "Museum")], rom_only: false },
];

#[derive(Resource, Default)]
pub struct Tour {
    /// the steps of this run, and the one shown
    steps: Vec<usize>,
    at: Option<usize>,
    owed: Option<bool>,
    still_since: Option<f32>,
}

fn pending_file() -> std::path::PathBuf {
    paths::data_dir(&paths::app_root()).join("tour-pending")
}

fn show(card: &mut Card, step: &Step, last: bool) {
    card.show(Content {
        kick: step.k.into(),
        title: step.t.into(),
        sub: step.p.into(),
        items: Vec::new(),
        keys: step.keys.iter().map(|(k, w)| (k.to_string(), w.to_string())).collect(),
        buttons: if last { vec!["Done".into()] } else { vec!["Next".into(), "Skip the tour".into()] },
    });
}

#[allow(clippy::too_many_arguments)]
pub fn run(time: Res<Time>, lib: Res<LibraryRes>, cursor: Res<crate::shelf::Cursor>, mut tour: ResMut<Tour>, mut card: ResMut<Card>, mut settings: ResMut<Settings>, playing: Res<Playing>) {
    let now = time.elapsed_secs();
    if let Some(at) = tour.at {
        let Some(o) = card.take() else { return };
        let next = at + 1;
        if o == Outcome::Picked(0) && next < tour.steps.len() {
            tour.at = Some(next);
            show(&mut card, &STEPS[tour.steps[next]], next + 1 == tour.steps.len());
        } else {
            tour.at = None;
            settings.set_tour_done();
            let _ = std::fs::remove_file(pending_file());
        }
        return;
    }
    let owed = *tour.owed.get_or_insert_with(|| {
        if !settings.setup_done() {
            let _ = std::fs::write(pending_file(), "1");
        }
        !settings.setup_done() || pending_file().exists()
    });
    let asked = ASKED.load(Ordering::Relaxed);
    if !crate::whatsnew::quiet(&lib, &settings, &playing, &card) || (!asked && (!owed || !settings.setup_done() || settings.tour_done())) {
        tour.still_since = None;
        return;
    }
    if !asked && now - *tour.still_since.get_or_insert(now) < SETTLE {
        return;
    }
    ASKED.store(false, Ordering::Relaxed);
    let LibraryState::Ready(l) = &lib.0 else { return };
    let rom = l.shelves.get(cursor.shelf).and_then(|s| s.games.get(cursor.game)).is_some_and(|g| g.source == Source::Rom);
    tour.steps = (0..STEPS.len()).filter(|&i| rom || !STEPS[i].rom_only).collect();
    tour.at = Some(0);
    show(&mut card, &STEPS[tour.steps[0]], tour.steps.len() == 1);
}
