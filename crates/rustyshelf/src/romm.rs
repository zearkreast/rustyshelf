//! RomM behind the shelf: a sync a little after the shelf is up (the saved copy shows meanwhile), and the shelf read
//! again when it brought something new.

use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;
use std::thread;

use bevy::prelude::*;
use rustyshelf_core::paths;

use crate::loading::{Base, Refresh};

/// Seconds after the start before the first sync, as the Electron app waits.
const FIRST_SYNC: f32 = 20.0;

#[derive(Resource, Default)]
pub struct Sync {
    started: bool,
    running: Option<Mutex<Receiver<bool>>>,
    /// the shelves are to be read again once nothing else reads them
    pending: bool,
}

pub fn run(time: Res<Time>, base: Res<Base>, mut sync: ResMut<Sync>, mut refresh: ResMut<Refresh>) {
    let Some(lib) = base.0.as_ref() else { return };
    let got = sync.running.as_ref().map(|rx| rx.lock().unwrap().try_recv());
    match got {
        Some(Ok(changed)) => {
            sync.running = None;
            sync.pending |= changed;
        }
        Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => sync.running = None,
        _ => {}
    }
    sync.pending |= crate::loading::READ_AGAIN.swap(false, std::sync::atomic::Ordering::Relaxed);
    if sync.pending && crate::loading::read_again(&mut refresh, &lib.app_root) {
        sync.pending = false;
    }
    let due = !sync.started && time.elapsed_secs() >= FIRST_SYNC;
    if sync.running.is_some() || !due {
        return;
    }
    sync.started = true;
    let (cfg, root) = (lib.config.clone(), lib.app_root.clone());
    let (tx, rx) = channel();
    thread::spawn(move || {
        let data = paths::data_dir(&root);
        let changed = rustyshelf_core::romm::sync_for_shelf(&cfg, rustyshelf_core::seal::key(&paths::env_var).as_ref(), &data);
        let _ = tx.send(changed);
    });
    sync.running = Some(Mutex::new(rx));
}
