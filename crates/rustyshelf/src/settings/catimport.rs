// Ported from MicaelTek's boxshelf-source (https://github.com/MicaelTek/boxshelf-source, GPL-3.0), the pc-categories
// branch's Steam and Playnite import (renderer/settings.js), as shelves of your own here.
//! Settings → Collections → Import Steam collections / Import Playnite categories (`rustyshelf_core::pccats`): each
//! becomes a collection, made or, imported again, brought up to date; Steam's Favorites join the favourites.

use rustyshelf_core::pccats::{self, Group, Have};

use super::*;

#[derive(Default)]
pub(super) struct CatImport {
    reading: Option<Later<Result<Value, String>>>,
}

impl Settings {
    fn haves(&self) -> Vec<Have<'_>> {
        self.pc_games.iter().map(|g| Have { id: &g.id, appid: g.appid.as_deref(), title: &g.title }).collect()
    }

    /// The import rows, under Make a new shelf.
    pub(super) fn import_rows(&self) -> Vec<Row> {
        let mut rows = vec![Row {
            label: "Import Steam collections".into(),
            hint: "Your Steam library's collections become shelves of your own here (dynamic ones and Hidden are left out), and its Favorites your favorites. Importing again brings them up to date; Steam isn't changed.".into(),
            press: Some(Box::new(|s: &mut Settings| s.import_steam_collections())),
            ..Row::default()
        }];
        if cfg!(windows) {
            rows.push(Row {
                label: "Import Playnite categories".into(),
                value: if self.cat_import.reading.is_some() { "Reading…".into() } else { String::new() },
                hint: "Playnite's categories become shelves of your own, with the games of them that are on your shelves. Importing again brings them up to date; Playnite isn't changed.".into(),
                press: Some(Box::new(|s: &mut Settings| s.import_playnite_categories())),
                ..Row::default()
            });
        }
        rows
    }

    fn import_steam_collections(&mut self) {
        let Some(root) = rustyshelf_core::steam::main_root(&self.text(&["steam", "path"])) else {
            return self.say("Steam wasn't found. Set its folder in Settings → PC.".into());
        };
        let Some(text) = pccats::steam_files(&root).iter().find_map(|f| std::fs::read_to_string(f).ok()) else {
            return self.say("Steam has no collections on this PC yet (they're kept once you sign in to Steam here).".into());
        };
        let (groups, favorites) = pccats::steam(&text, &self.haves());
        self.take_groups(groups, favorites, "Steam");
    }

    fn import_playnite_categories(&mut self) {
        if self.cat_import.reading.is_some() {
            return;
        }
        let Some(found) = rustyshelf_core::addgames::find(None, &paths::env_var) else {
            return self.say("Playnite wasn't found.".into());
        };
        self.cat_import.reading = Some(Later::spawn(move || rustyshelf_core::addgames::read(&found)));
        self.redraw = true;
    }

    pub(super) fn poll_cat_import(&mut self) {
        let Some(read) = self.cat_import.reading.as_ref().and_then(Later::poll) else { return };
        self.cat_import.reading = None;
        match read {
            Ok(lib) => {
                let groups = pccats::playnite(&lib, &self.haves());
                self.take_groups(groups, Vec::new(), "Playnite");
            }
            Err(why) => self.say(why),
        }
        self.redraw = true;
    }

    /// Makes each group a collection, or brings the one imported before up to date; adds the favourites.
    fn take_groups(&mut self, groups: Vec<Group>, favorites: Vec<String>, from: &str) {
        let mut list = self.collections();
        let (mut made, mut updated, mut full) = (0, 0, 0);
        for g in groups.into_iter().filter(|g| !g.ids.is_empty()) {
            let room = list.len() < collections::MOST;
            match list.iter_mut().find(|c| c["id"] == g.id.as_str()) {
                Some(c) => {
                    c["name"] = json!(g.name);
                    c["ids"] = json!(g.ids);
                    updated += 1;
                }
                None if !room => full += 1,
                None => {
                    list.push(json!({ "id": g.id, "name": g.name, "ids": g.ids }));
                    made += 1;
                }
            }
        }
        if made + updated > 0 {
            self.set(&["ui", "collections"], json!(list));
        }
        let mut favs = self.list(&["ui", "favorites"]);
        let before = favs.len();
        for f in favorites {
            if !favs.contains(&f) {
                favs.push(f);
            }
        }
        let added = favs.len() - before;
        if added > 0 {
            self.set(&["ui", "favorites"], json!(favs));
        }
        let mut said = vec![format!("{from}: {made} shelves made, {updated} brought up to date")];
        if added > 0 {
            said.push(format!("{added} favorites added"));
        }
        if full > 0 {
            said.push(format!("{full} left out (that's the most collections there can be)"));
        }
        self.note = if made + updated + added == 0 { format!("{from}: nothing to import with games that are on your shelves.") } else { format!("{}.", said.join(", ")) };
        self.redraw = true;
    }
}
