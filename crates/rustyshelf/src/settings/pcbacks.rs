// Based on ZakAuMiel/boxshelf-source (https://github.com/ZakAuMiel/boxshelf-source, GPL-3.0), Zak's fork of Boxshelf by
// sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source): the rows and their wording follow the back of
// the box choices of its renderer/app.js cover picker.
//! A PC game's back of the box from Details: a picture of yours, LaunchBox's, or the made-up one again; and
//! LaunchBox's PC games downloaded from Settings → PC games, only when asked for.

use std::sync::Arc;

use rustyshelf_core::pcbacks::{self, Got, Want};
use rustyshelf_core::{boxart, pccovers};

use super::*;

#[derive(Default)]
pub(super) struct PcBacksUi {
    /// LaunchBox's database downloaded again with its PC games: what it's doing, and how it ended
    pub(super) get: Option<(Arc<Mutex<String>>, Mutex<Receiver<Result<(), String>>>)>,
    /// "Back from LaunchBox": the game's id and title, and what came back
    looking: Option<(String, String, Mutex<Receiver<Got>>)>,
}

impl Settings {
    /// Details' rows for the back of a PC game's box, unless it's on a console's shelf.
    pub(super) fn pc_back_rows(&self) -> Vec<Row> {
        let Some(c) = self.chosen.as_ref().filter(|c| matches!(c.game.source, Source::Steam { .. } | Source::Pc(_)) && systems().console(&c.console).is_none()) else { return Vec::new() };
        let (data, id) = (paths::data_dir(&self.app_root), &c.game.id);
        let mut rows = vec![Row { label: "Back of the box…".into(), hint: "A picture of yours".into(), press: Some(Box::new(|s: &mut Settings| s.pick(Pick::OwnBack, "Choose the back of the box", false))), ..Row::default() }];
        // a console box's case never wears LaunchBox's back
        if c.game.box_plat.is_none() {
            rows.push(Row {
                label: "Back from LaunchBox".into(),
                value: if self.pc_backs.looking.is_some() { "Looking…".into() } else { String::new() },
                hint: "Found by name".into(),
                press: Some(Box::new(|s: &mut Settings| s.launchbox_back())),
                step: None,
            });
        }
        let launchbox = c.game.box_plat.is_none() && !pccovers::made_up_back(&data, id) && pccovers::auto_back(&data, id).is_some();
        if pccovers::back(&data, id).is_some() || launchbox {
            rows.push(Row { label: "Made-up back".into(), hint: "Remove your back".into(), press: Some(Box::new(|s: &mut Settings| s.made_up_back())), ..Row::default() });
        }
        rows
    }

    /// Settings → PC games: LaunchBox's database downloaded again with its PC games in, asking twice before it starts.
    pub(super) fn launchbox_pc_row(&self) -> Row {
        let cache = paths::data_dir(&self.app_root).join("boxart-cache");
        let value = if self.pc_backs.get.is_some() {
            "Downloading…"
        } else if boxart::has_pc_games(&cache) {
            "Added"
        } else {
            "About 110 MB"
        };
        Row {
            label: "Add LaunchBox's PC games".into(),
            value: value.into(),
            hint: "LaunchBox's free database again, with its PC games in, for the backs of their boxes: a download of about 110 MB".into(),
            press: Some(Box::new(|s: &mut Settings| s.add_launchbox_pc())),
            step: None,
        }
    }

    fn add_launchbox_pc(&mut self) {
        let cache = paths::data_dir(&self.app_root).join("boxart-cache");
        if self.pc_backs.get.is_some() || self.launchbox_downloading() {
            self.say("LaunchBox's database is downloading already".into());
            return;
        }
        if boxart::has_pc_games(&cache) {
            self.say("LaunchBox's PC games are in already".into());
            return;
        }
        if self.asked != "lbpc-sure" {
            if self.twice("lbpc", "This downloads LaunchBox's database again: about 110 MB. Press again to go on") {
                self.asked = "lbpc-sure".into();
                self.say("Press once more to start the download".into());
            }
            return;
        }
        self.asked.clear();
        let (tx, rx) = channel();
        let stage = Arc::new(Mutex::new("Starting the download…".to_string()));
        let at = stage.clone();
        std::thread::spawn(move || {
            let _ = std::fs::create_dir_all(&cache);
            let _ = tx.send(boxart::with_pc_games(&cache, &mut |s| *at.lock().unwrap() = s.to_string()).map(|_| ()));
        });
        self.pc_backs.get = Some((stage, Mutex::new(rx)));
        self.say("LaunchBox's PC games: downloading…".into());
    }

    /// A picture of the player's own as the chosen game's back.
    pub(super) fn own_back(&mut self, file: &Path) {
        let Some((id, title)) = self.chosen.as_ref().map(|c| (c.game.id.clone(), c.game.title.clone())) else { return };
        match pccovers::own_back(&paths::data_dir(&self.app_root), &id, file) {
            Ok(f) => {
                self.back_now(&id, Some(f));
                self.say(format!("New back for {title}. Flip the case to see it"));
            }
            Err(why) => self.say(why),
        }
    }

    fn made_up_back(&mut self) {
        let Some((id, title)) = self.chosen.as_ref().map(|c| (c.game.id.clone(), c.game.title.clone())) else { return };
        match pccovers::reset_back(&paths::data_dir(&self.app_root), &id) {
            Ok(()) => {
                self.back_now(&id, None);
                self.say(format!("{title} has its made-up back again"));
            }
            Err(why) => self.say(why),
        }
    }

    fn launchbox_back(&mut self) {
        let Some(want) = self.chosen.as_ref().filter(|c| c.game.box_plat.is_none()).and_then(|c| Want::of(&c.game)).filter(|_| self.pc_backs.looking.is_none()) else { return };
        let (data, region) = (paths::data_dir(&self.app_root), self.art_region());
        let (id, title) = (want.id.clone(), want.title.clone());
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(pcbacks::fetch(&data, &data.join("boxart-cache"), &want, &region));
        });
        self.pc_backs.looking = Some((id, title, Mutex::new(rx)));
        self.say("Looking for the back of the box…".into());
    }

    /// The back the chosen game has now, until the shelves are read again when Settings closes.
    fn back_now(&mut self, id: &str, back: Option<PathBuf>) {
        if let Some(c) = self.chosen.as_mut().filter(|c| c.game.id == id) {
            (c.game.art.back, c.game.art.made_back) = (back, false);
        }
        self.changed = true;
        self.redraw = true;
    }

    /// What the back looked up and LaunchBox's download came to since the last frame.
    pub(super) fn poll_pc_backs(&mut self) {
        let looked = self.pc_backs.looking.as_ref().map(|(_, _, rx)| rx.lock().unwrap().try_recv());
        match looked {
            Some(Ok(got)) => {
                let (id, title, _) = self.pc_backs.looking.take().expect("a back looked up");
                match got {
                    Got::Back(f, _) => {
                        self.back_now(&id, Some(f));
                        self.say(format!("New back for {title}. Flip the case to see it"));
                    }
                    Got::Miss => self.say("LaunchBox has no back for this name. Try a picture of yours.".into()),
                    Got::NotReady => self.say("LaunchBox's PC games aren't here yet: Settings → PC games and apps → Add LaunchBox's PC games".into()),
                }
            }
            Some(Err(TryRecvError::Disconnected)) => self.pc_backs.looking = None,
            _ => {}
        }
        let Some((stage, rx)) = &self.pc_backs.get else { return };
        let ended = match rx.lock().unwrap().try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Disconnected) => Some(Err("it stopped".into())),
            Err(TryRecvError::Empty) => None,
        };
        let said = format!("LaunchBox's PC games: {}", stage.lock().unwrap());
        if ended.is_none() && self.note != said && self.stack.last().is_some_and(|p| p.0 == Screen::Pc) {
            self.say(said);
        }
        let Some(ended) = ended else { return };
        self.pc_backs.get = None;
        match ended {
            Ok(()) => {
                self.changed |= self.flag(&["pc", "autoBacks"], false);
                self.say("LaunchBox's PC games are in".into());
            }
            Err(why) => self.say(format!("LaunchBox's PC games didn't download: {why}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rustyshelf-pcbacks-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn pc_game(id: &str) -> Game {
        Game { id: id.into(), title: "Zeta".into(), source: Source::Pc(Box::new(rustyshelf_core::pcgames::PcGame { id: id.into(), source: "custom", title: "Zeta".into(), ..Default::default() })), ..Game::default() }
    }

    #[test]
    fn a_pc_game_s_back_rows_off_a_console_s_shelf_only() {
        let r = root("rows");
        let mut s = Settings { wizard: false, stack: vec![(Screen::Details, 0)], app_root: r.clone(), ..Settings::default() };
        let labels = |s: &Settings| s.pc_back_rows().into_iter().map(|r| r.label).collect::<Vec<_>>();
        s.chosen = Some(Chosen { console: "custom".into(), emulator: String::new(), game: pc_game("custom-zeta"), plays: None });
        assert_eq!(labels(&s), ["Back of the box…", "Back from LaunchBox"]);
        std::fs::create_dir_all(r.join("data/pc-covers")).unwrap();
        std::fs::write(r.join("data/pc-covers/custom-zeta.back.jpg"), b"x").unwrap();
        assert_eq!(labels(&s).last().map(String::as_str), Some("Made-up back"));
        s.made_up_back();
        assert_eq!(labels(&s).len(), 2);
        assert!(s.changed && pccovers::made_up_back(&r.join("data"), "custom-zeta"));
        s.chosen.as_mut().unwrap().game.box_plat = Some("ps5".into());
        assert_eq!(labels(&s), ["Back of the box…"], "in a console box");
        s.launchbox_back();
        assert!(s.pc_backs.looking.is_none());
        s.chosen.as_mut().unwrap().console = "snes".into();
        assert!(labels(&s).is_empty());
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn the_download_asks_twice_and_starts_over_after_a_move() {
        let mut s = Settings { wizard: false, stack: vec![(Screen::Pc, 0)], app_root: root("get"), ..Settings::default() };
        s.add_launchbox_pc();
        assert_eq!(s.asked, "lbpc");
        s.add_launchbox_pc();
        assert_eq!((s.asked.as_str(), s.note.as_str()), ("lbpc-sure", "Press once more to start the download"));
        s.asked.clear();
        s.add_launchbox_pc();
        assert!(s.asked == "lbpc" && s.pc_backs.get.is_none());
    }
}
