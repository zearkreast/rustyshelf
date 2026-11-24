// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the pages and their wording follow
// Custom games in Boxshelf's renderer/settings.js, as RustyShelf has them.
//! Settings → Custom games: programs added by hand, each with its name, launch arguments and the shelf it sits on.

use rustyshelf_core::pcgames;

use super::*;

pub(super) fn base36(mut n: u128) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

impl Settings {
    fn custom(&self) -> Vec<Value> {
        at(&self.cfg, &["pc", "custom"]).and_then(Value::as_array).cloned().unwrap_or_default()
    }

    fn custom_field(&self, i: usize, key: &str) -> String {
        self.custom().get(i).and_then(|c| c.get(key)).and_then(Value::as_str).unwrap_or("").to_string()
    }

    fn set_custom_field(&mut self, i: usize, key: &str, value: Option<Value>) {
        let mut list = self.custom();
        let Some(Value::Object(game)) = list.get_mut(i) else { return };
        match value {
            Some(v) => {
                game.insert(key.to_string(), v);
            }
            None => {
                game.shift_remove(key);
            }
        }
        self.set(&["pc", "custom"], json!(list));
    }

    /// A program chosen in the file dialog goes on the Custom shelf, named after its file.
    pub(super) fn add_custom(&mut self, exe: &str) {
        let mut list = self.custom();
        if list.iter().any(|c| c.get("exe").and_then(Value::as_str).is_some_and(|e| e.to_lowercase() == exe.to_lowercase())) {
            self.note = "That game is already on the Custom shelf".into();
            return;
        }
        let title = pcgames::title_from_exe(exe);
        let now = u128::from(rustyshelf_core::now_ms());
        // the id the Electron app gives one: the time in base 36
        list.push(json!({ "id": base36(now), "title": title, "exe": exe, "args": "" }));
        self.set(&["pc", "custom"], json!(list));
        self.note = format!("Added {title}. Rename it here if you like.");
    }

    pub(super) fn custom_page(&self) -> Page {
        let list = self.custom();
        let mut rows = self.adding_rows();
        rows.push(Row {
            label: "Add one program".into(),
            value: "Choose…".into(),
            hint: "Choose the game's program. It goes on the Custom shelf; RustyShelf starts it directly, no emulator is involved.".into(),
            press: Some(Box::new(|s: &mut Settings| s.pick(Pick::CustomGame, "Choose the game's program", false))),
            step: None,
        });
        rows.extend(self.cover_rows());
        rows.push(Row {
            label: "Put recomps on their console's shelf".into(),
            value: on_off(self.flag(&["pc", "recompDetect"], false)),
            hint: "A game added here with no Platform, or one of Quiver's, goes on its console's shelf when LaunchBox's database (from box art downloads) has a game of exactly its name on that console and nowhere else. Ports, and games you put on PC, Ports & Recomps or a console's shelf, stay where they are; Custom counts as no Platform.".into(),
            press: Some(Box::new(|s: &mut Settings| s.toggle(&["pc", "recompDetect"], false))),
            step: Some(Box::new(|s: &mut Settings, _| s.toggle(&["pc", "recompDetect"], false))),
        });
        for (i, c) in list.iter().enumerate() {
            let text = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or("");
            let (title, shelf) = (text("title"), text("shelf"));
            let place = match shelf {
                "" => short(text("exe")),
                "pc" => "PC shelf".to_string(),
                "ports" => "Ports & Recomps".to_string(),
                id => format!("{} shelf", systems().console(id).map_or(id, |c| c.name.as_str())),
            };
            let program = if text("exe").is_empty() && !text("shellApp").is_empty() { format!("Microsoft Store app: {}", text("shellApp")) } else { format!("Program: {}", text("exe")) };
            rows.push(Row {
                label: if title.is_empty() { "Game".into() } else { title.to_string() },
                value: place,
                hint: program,
                press: Some(Box::new(move |s: &mut Settings| s.go(Screen::CustomGame(i)))),
                step: None,
            });
        }
        Page {
            title: "Custom games".into(),
            lead: "Recomps, PC ports and any other program. Each goes on the Custom shelf, or on Ports & Recomps, PC or a console's shelf if you choose its Platform. Quiver Launcher's ports and recomps go on Ports & Recomps, its other games on Custom.".into(),
            rows,
        }
    }

    /// Covers for the custom games without one: they never get one by themselves, as their names often match an
    /// unrelated game.
    fn cover_rows(&self) -> Vec<Row> {
        use crate::coverpick::Job;
        let state = self.cover_state;
        let none = |s: &mut Settings| s.note = "Every custom game has a cover already".into();
        let mut rows = vec![
            Row {
                label: "Choose covers one by one".into(),
                value: match state.missing {
                    None => "Looking…".into(),
                    Some(0) => "Every game has one".into(),
                    Some(n) => format!("{n} game{} without a cover", plural_s(n)),
                },
                hint: "Goes through every custom game without a cover of its own: SteamGridDB's covers for each (with a free SteamGridDB key, Settings → Accounts & services) or a picture of yours. A uses one, Skip moves on, B stops.".into(),
                press: Some(Box::new(move |s: &mut Settings| {
                    if state.missing == Some(0) {
                        none(s);
                    } else if s.shut() {
                        s.cover_job = Some(Job::OneByOne);
                    }
                })),
                step: None,
            },
            Row {
                label: "Use the best cover for each".into(),
                value: if state.busy { "Finding covers…" } else { "Needs a SteamGridDB key" }.into(),
                hint: "Picks SteamGridDB's top cover for every custom game without one, in one go. A recomp can share its name with another game: look them over, and Undo puts them all back.".into(),
                press: Some(Box::new(move |s: &mut Settings| {
                    if state.missing == Some(0) {
                        none(s);
                    } else if !state.busy {
                        s.cover_job = Some(Job::Best);
                    }
                })),
                step: None,
            },
        ];
        if state.undo > 0 {
            rows.push(Row {
                label: format!("Undo the {} covers just found", state.undo),
                value: "Undo".into(),
                hint: "Puts back the made-up cases those games had before.".into(),
                press: Some(Box::new(|s: &mut Settings| s.cover_job = Some(Job::Undo))),
                step: None,
            });
        }
        rows
    }

    pub(super) fn custom_game_page(&self, i: usize) -> Page {
        let (title, args, shelf) = (self.custom_field(i, "title"), self.custom_field(i, "args"), self.custom_field(i, "shelf"));
        let mut shelves: Vec<(String, String)> = vec![("custom".into(), "Custom".into()), ("pc".into(), "PC".into()), ("ports".into(), "Ports & Recomps".into())];
        shelves.extend(systems().consoles.iter().map(|c| (c.id.clone(), c.name.clone())));
        let on = shelves.iter().position(|x| x.0 == shelf).unwrap_or(0);
        let name = shelves[on].1.clone();
        let turn = move |s: &mut Settings, d: i32| {
            let n = (on as i32 + d).rem_euclid(shelves.len() as i32) as usize;
            // the Custom shelf is no "shelf" at all, as the Electron app writes it
            s.set_custom_field(i, "shelf", Some(shelves[n].0.clone()).filter(|id| id != "custom").map(Value::from));
        };
        let (t2, a2) = (title.clone(), args.clone());
        const NAME: &str = "The name shown on its shelf.";
        const ARGS: &str = "Passed to the program when it starts, for example -dx12. Put quotes around anything with spaces.";
        const PLACE: &str = "Which shelf it sits on: Custom, Ports & Recomps (native PC versions of console games), PC (with your Steam and store games) or a console's, beside its games. RustyShelf starts the program itself either way.";
        let port_of = self.custom_field(i, "portOf");
        let mut froms: Vec<(String, String)> = vec![(String::new(), "Not a port".into())];
        froms.extend(systems().consoles.iter().map(|c| (c.id.clone(), c.name.clone())));
        let at = froms.iter().position(|x| x.0 == port_of).unwrap_or(0);
        let from_name = froms[at].1.clone();
        let turn_from = move |s: &mut Settings, d: i32| {
            let n = (at as i32 + d).rem_euclid(froms.len() as i32) as usize;
            s.set_custom_field(i, "portOf", Some(froms[n].0.clone()).filter(|id| !id.is_empty()).map(Value::from));
        };
        let mut rows = vec![
            Row {
                label: "Game name".into(),
                value: title.clone(),
                hint: NAME.into(),
                press: Some(Box::new(move |s: &mut Settings| {
                    s.edit("Game name", NAME, t2.clone(), Box::new(move |s: &mut Settings, v: String| s.set_custom_field(i, "title", Some(json!(v.chars().take(120).collect::<String>())))));
                })),
                step: None,
            },
            Row {
                label: "Launch arguments".into(),
                value: if args.is_empty() { "(none)".into() } else { args.clone() },
                hint: ARGS.into(),
                press: Some(Box::new(move |s: &mut Settings| {
                    s.edit("Launch arguments", ARGS, a2.clone(), Box::new(move |s: &mut Settings, v: String| s.set_custom_field(i, "args", Some(json!(v)))));
                })),
                step: None,
            },
            Row::turning("Platform", name, PLACE, turn),
        ];
        if shelf == "ports" {
            rows.push(Row::turning("Port of", from_name, "The console this program is a port or recompilation of.", turn_from));
        }
        rows.extend([
            Row {
                label: "Remove game".into(),
                hint: "Takes it off its shelf. Nothing is deleted.".into(),
                press: Some(Box::new(move |s: &mut Settings| {
                    if !s.twice(&format!("custom:{i}"), "Press again to remove it") {
                        return;
                    }
                    let mut list = s.custom();
                    if i < list.len() {
                        list.remove(i);
                    }
                    s.set(&["pc", "custom"], json!(list));
                    s.back();
                    s.note = "Removed from its shelf (nothing was deleted)".into();
                })),
                ..Row::default()
            },
        ]);
        let exe = self.custom_field(i, "exe");
        Page { title: if title.is_empty() { "Game".into() } else { title }, lead: format!("Program: {exe}"), rows }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recomps_go_on_their_console_s_shelf_only_once_switched_on() {
        let r = std::env::temp_dir().join(format!("rustyshelf-custom-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&r);
        let mut s = Settings { wizard: false, stack: vec![(Screen::Custom, 0)], app_root: r.clone(), ..Settings::default() };
        let row = |s: &Settings| s.custom_page().rows.into_iter().find(|r| r.label == "Put recomps on their console's shelf").unwrap();
        assert_eq!(row(&s).value, on_off(false));
        (row(&s).press.unwrap())(&mut s);
        assert_eq!((row(&s).value, &s.cfg["pc"]["recompDetect"]), (on_off(true), &json!(true)));
        let _ = std::fs::remove_dir_all(&r);
    }
}
