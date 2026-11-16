// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's renderer/app.js (deckHint, deckAdd), as RustyShelf has it.
//! Steam Deck in Desktop Mode, RustyShelf not started by Steam: the Deck's controller is a keyboard and mouse there, so
//! B is Esc and most emulators quit their game on it. Said once on the shelf, with the way out: start RustyShelf from
//! Steam (Game Mode), which Add RustyShelf to Steam sets up.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;

use bevy::prelude::*;
use rustyshelf_core::paths;
use rustyshelf_core::steamdeck::{self, Refusal};

use crate::card::{Card, Content, Outcome};
use crate::hud::Toast;
use crate::loading::LibraryRes;
use crate::play::Playing;
use crate::settings::Settings;

const ICON: &[u8] = include_bytes!("../../../renderer/app-icon.png");

/// Adds RustyShelf to Steam off the frame (Steam closes and opens again): what to say once it's done. `card`: asked from
/// the shelf's card, which words it for a Deck.
pub fn add(card: bool) -> Receiver<String> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let info = steamdeck::info(&paths::env_var);
        let home = PathBuf::from(paths::env_var("HOME").unwrap_or_default());
        let data = paths::data_dir(&paths::app_root());
        let r = steamdeck::add_to_steam(&paths::env_var, &home, &info.exe, Some(ICON), &data, true);
        if let Ok(a) = &r {
            a.notes.iter().for_each(|n| info!("{n}"));
        }
        let why = |e: Refusal| match (e, card) {
            (Refusal::NoSteam, true) => "Steam hasn't been set up on this Deck yet",
            (Refusal::NoSteam, false) => "Steam hasn't been set up here yet",
            (Refusal::NoProgram, _) => "RustyShelf's program wasn't found",
            (Refusal::SteamWouldNotClose, true) => "Steam didn't close: close it and try again in Settings → System",
            (Refusal::SteamWouldNotClose, false) => "Steam didn't close: close it yourself and try again",
            (Refusal::GameMode, _) => "it's already in Steam",
            (Refusal::WriteFailed, _) => "Steam's list couldn't be written",
            (Refusal::NotLinux | Refusal::SteamRunning, _) => "",
        };
        let said = match r {
            Ok(a) if card && a.updated => "RustyShelf's Steam entry is up to date. Start it from Steam (Game Mode)".to_string(),
            Ok(_) if card => "RustyShelf is in Steam now: start it from your Library in Game Mode (Non-Steam)".into(),
            Ok(_) => "RustyShelf is in Steam: start it from your Library (Non-Steam), in Game Mode on a Steam Deck".into(),
            Err(e) => {
                let w = why(e);
                format!("Couldn't add RustyShelf to Steam{}{}", if w.is_empty() { "" } else { ": " }, w) + if card { "." } else { "" }
            }
        };
        let _ = tx.send(said);
    });
    rx
}

/// Where RustyShelf runs, in the log (Linux).
pub fn log_start() {
    let d = steamdeck::info(&paths::env_var);
    if d.linux {
        info!("Linux start: {} | started by Steam: {} | Game Mode: {}", if d.deck { "Steam Deck" } else { "PC" }, d.steam, d.game_mode);
    }
}

#[derive(Resource, Default)]
pub struct DeckHint {
    checked: bool,
    /// the card is up; and whether its first button adds RustyShelf to Steam
    showing: Option<bool>,
    adding: Option<Mutex<Receiver<String>>>,
}

pub fn run(time: Res<Time>, lib: Res<LibraryRes>, mut hint: ResMut<DeckHint>, mut card: ResMut<Card>, mut settings: ResMut<Settings>, playing: Res<Playing>, mut toast: ResMut<Toast>) {
    let now = time.elapsed_secs();
    if let Some(said) = hint.adding.as_ref().and_then(|rx| rx.lock().unwrap().try_recv().ok()) {
        hint.adding = None;
        (toast.text, toast.until) = (said, now + 8.0);
    }
    if let Some(can_add) = hint.showing {
        let Some(o) = card.take() else { return };
        hint.showing = None;
        match o {
            Outcome::Picked(0) if can_add => {
                (toast.text, toast.until) = ("Adding RustyShelf to Steam (Steam closes and opens again)…".into(), now + 30.0);
                hint.adding = Some(Mutex::new(add(true)));
            }
            Outcome::Picked(1) => (toast.text, toast.until) = ("You can add it any time in Settings → System.".into(), now + 5.0),
            _ => {}
        }
        return;
    }
    if hint.checked || now < 11.0 || !crate::whatsnew::quiet(&lib, &settings, &playing, &card) {
        return;
    }
    hint.checked = true;
    if !settings.setup_done() || settings.deck_hint_seen() {
        return;
    }
    let d = steamdeck::info(&paths::env_var);
    if !d.deck || d.steam || d.game_mode {
        return;
    }
    settings.set_deck_hint_seen();
    let ready = d.in_steam || !d.can_add;
    let mut steps = vec!["Switch to Game Mode and start RustyShelf from your Library (Non-Steam)".to_string(), "To leave a game, hold Select + Start for a moment".into()];
    if !ready {
        steps = vec!["Add RustyShelf to Steam (the button below does it for you)".into(), "Switch to Game Mode and start it from your Library (Non-Steam)".into(), steps.pop().unwrap()];
    }
    card.show(Content {
        kick: "Steam Deck".into(),
        title: "Start RustyShelf from Steam for your games' controls".into(),
        sub: "Started outside Steam, the Deck's controller works as a keyboard and mouse here, so B is the Esc key and most emulators quit their game on it. Started from Steam, B is just B.".into(),
        items: steps,
        buttons: if ready { vec!["Got it".into()] } else { vec!["Add RustyShelf to Steam".into(), "Not now".into()] },
        ..default()
    });
    hint.showing = Some(!ready);
}
