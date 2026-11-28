// Ported from MicaelTek's boxshelf-source (https://github.com/MicaelTek/boxshelf-source, GPL-3.0), the pc-bulk-covers
// branch's Covers rows (renderer/settings.js).
//! Settings → PC → Covers: the best front cover for every PC game at once (`rustyshelf_core::pcbulk`), with its
//! progress, a Cancel, and Undo for the last run.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rustyshelf_core::pcbulk::{self, Progress, Wanted};

use super::*;

#[derive(Default)]
pub(super) struct PcBulk {
    /// the run going on: whether it's every game, how far it is, its stop, and its end
    running: Option<(bool, Arc<Mutex<Progress>>, Arc<AtomicBool>, Later<Progress>)>,
    /// the last run's end, and whether it was every game
    last: Option<(bool, Progress)>,
    /// how far it was when the page was last drawn
    shown: (usize, usize),
}

impl Settings {
    /// The PC games of a library (the PC, Custom and Ports shelves'), for a run.
    pub(super) fn pc_wanted(shelves: &[rustyshelf_core::library::Shelf]) -> Vec<Wanted> {
        shelves
            .iter()
            .filter(|sh| matches!(sh.kind, ShelfKind::Pc))
            .flat_map(|sh| sh.games.iter())
            .filter_map(|g| {
                let appid = match &g.source {
                    Source::Steam { appid, .. } => Some(appid.clone()),
                    Source::Pc(_) => None,
                    _ => return None,
                };
                Some(Wanted { id: g.id.clone(), appid, title: g.title.clone(), has_front: g.art.front.is_some() })
            })
            .collect()
    }

    fn start_pc_bulk(&mut self, all: bool) {
        if self.pc_bulk.running.is_some() {
            return;
        }
        let (games, data) = (self.pc_games.clone(), paths::data_dir(&self.app_root));
        let (progress, stop) = (Arc::new(Mutex::new(Progress::default())), Arc::new(AtomicBool::new(false)));
        let (p, st) = (progress.clone(), stop.clone());
        let job = Later::spawn(move || {
            let sg = rustyshelf_core::steamgrid::SteamGrid::from_settings(&data);
            pcbulk::run(&data, &games, all, sg.as_ref(), &st, &|now: &Progress| *p.lock().unwrap() = now.clone())
        });
        self.pc_bulk.running = Some((all, progress, stop, job));
        self.redraw = true;
    }

    /// The run's progress since the last frame, and its end.
    pub(super) fn poll_pc_bulk(&mut self) {
        let Some((all, progress, _, job)) = self.pc_bulk.running.as_ref() else { return };
        let now = progress.lock().unwrap().clone();
        if (now.done, now.updated) != self.pc_bulk.shown {
            self.pc_bulk.shown = (now.done, now.updated);
            self.redraw = true;
        }
        let Some(end) = job.poll() else { return };
        let all = *all;
        self.pc_bulk.running = None;
        self.note = match end.updated {
            0 => "No PC cover needed changing.".into(),
            n => format!("{n} PC cover{} changed.", plural_s(n)),
        };
        if end.updated > 0 {
            crate::loading::STALE.store(true, Ordering::Relaxed);
        }
        self.pc_bulk.last = Some((all, end));
        self.redraw = true;
    }

    fn pc_bulk_value(&self, all: bool) -> String {
        match (&self.pc_bulk.running, &self.pc_bulk.last) {
            (Some((a, p, _, _)), _) if *a == all => {
                let p = p.lock().unwrap();
                format!("{}/{} · {} changed · Cancel", p.done, p.total, p.updated)
            }
            (Some(_), _) => "Busy with the other one".into(),
            (None, Some((a, p))) if *a == all => format!("Done · {} changed, {} kept", p.updated, p.skipped + p.failed),
            _ => String::new(),
        }
    }

    pub(super) fn pc_bulk_rows(&self) -> Vec<Row> {
        if self.pc_games.is_empty() {
            return Vec::new();
        }
        let press = |all: bool| -> Press {
            Box::new(move |s: &mut Settings| match s.pc_bulk.running.as_ref().map(|r| (r.0, r.2.clone())) {
                Some((a, stop)) if a == all => {
                    stop.store(true, Ordering::Relaxed);
                    s.note = "Stopping after the games being looked at…".into();
                }
                Some(_) => {}
                None => s.start_pc_bulk(all),
            })
        };
        let mut rows = vec![
            Row {
                label: "Download missing PC covers".into(),
                value: self.pc_bulk_value(false),
                hint: "A front cover for every PC game that has none, the best match picked by itself: Steam's own, Steam's store, or SteamGridDB's posters (with its key). Press again to stop.".into(),
                press: Some(press(false)),
                step: None,
            },
            Row {
                label: "Refresh all PC covers".into(),
                value: self.pc_bulk_value(true),
                hint: "Looks again at every PC game's cover and keeps the best one found, covers you chose included. Undo puts them back.".into(),
                press: Some(press(true)),
                step: None,
            },
        ];
        let n = pcbulk::can_undo(&paths::data_dir(&self.app_root));
        if n > 0 && self.pc_bulk.running.is_none() {
            rows.push(Row {
                label: "Undo the last cover download".into(),
                value: format!("{n} cover{}", plural_s(n)),
                hint: "Puts back the covers the last run replaced.".into(),
                press: Some(Box::new(|s: &mut Settings| {
                    let n = pcbulk::undo(&paths::data_dir(&s.app_root));
                    s.note = format!("{n} cover{} put back.", plural_s(n));
                    s.pc_bulk.last = None;
                    crate::loading::STALE.store(true, Ordering::Relaxed);
                })),
                step: None,
            });
        }
        rows
    }
}
