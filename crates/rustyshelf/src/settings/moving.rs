//! Export everything and Import everything (Settings → System): the settings, the downloads and the art kept
//! next to the games, carried to another PC or a second install.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustyshelf_core::art::ArtLibrary;
use rustyshelf_core::dataexport::{self, Control, Install, Progress, Target};

use super::*;

/// An export or import on its way, on a thread of its own.
pub(super) struct Moving {
    what: &'static str,
    control: Arc<Control>,
    progress: Arc<Mutex<Progress>>,
    done: Mutex<Receiver<Result<String, String>>>,
}

/// What the System page keeps of exports and imports.
#[derive(Default)]
pub(super) struct Moves {
    running: Option<Moving>,
    /// how big an export would be: its bytes and its files, once measured
    size: Option<(u64, u64)>,
    measuring: Option<Mutex<Receiver<(u64, u64)>>>,
    /// an import is in: RustyShelf starts again on the imported settings
    restart_at: Option<Instant>,
}

fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn left(secs: u64) -> String {
    match secs {
        0..=89 => format!("{secs} s left"),
        90..=5399 => format!("{} min left", (secs + 30) / 60),
        _ => format!("{} h {} min left", secs / 3600, secs % 3600 / 60),
    }
}

impl Settings {
    /// The install's folders and its art folders, for a thread: `with(app root, data folder, settings, each
    /// console's art folders, the art library)`.
    fn on_install<T: Send + 'static>(&self, with: impl FnOnce(&Path, &Path, &Value, &dyn Fn(&str, &Value) -> Vec<PathBuf>, Option<&Path>) -> T + Send + 'static) -> Receiver<T> {
        let (cfg, app_root) = (self.cfg.clone(), self.app_root.clone());
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let config = Config::from_value(&cfg).unwrap_or_default();
            let art = ArtLibrary::from_config(&config, &app_root);
            let art_dirs = |id: &str, _: &Value| systems().console(id).map_or(Vec::new(), |c| art.dirs_for(&config.system(c), &app_root));
            let _ = tx.send(with(&app_root, &paths::data_dir(&app_root), &cfg, &art_dirs, art.root()));
        });
        rx
    }

    fn start_move(&mut self, what: &'static str, folder: PathBuf) {
        if self.moves.running.is_some() {
            return;
        }
        // an export copies the file, an import replaces it: the settings are on disk first
        rustyshelf_core::settings::flush();
        let (control, progress) = (Arc::new(Control::default()), Arc::new(Mutex::new(Progress::default())));
        let (asked, told) = (control.clone(), progress.clone());
        let done = self.on_install(move |app_root, data_dir, cfg, art_dirs, art_lib| {
            let mut say = |p: &Progress| *told.lock().unwrap() = p.clone();
            if what == "export" {
                let install = Install { app_root, data_dir, cfg, art_dirs, art_lib };
                dataexport::export_all(&install, &folder, rustyshelf_core::VERSION, &asked, &mut |_, _, _| {}, &mut say).map(|r| {
                    let all = format!("{} in {} files", crate::hud::size(r.bytes), count(r.files));
                    if r.updated { format!("Export brought up to date: {} new or changed files copied. {all}, in {}", count(r.copied.copied), r.dir.display()) } else { format!("Exported {all} to {}", r.dir.display()) }
                })
            } else {
                let target = Target { app_root, data_dir, art_dirs, art_lib, cfg };
                dataexport::import_all(&folder, &target, &asked, &mut |_, _, _| {}, &mut say).map(|r| {
                    let nowhere = if r.skipped.is_empty() { String::new() } else { format!(" No place here for: {}.", r.skipped.join("; ")) };
                    format!("Imported {} files.{nowhere} RustyShelf starts again on the imported settings…", count(r.copied.copied))
                })
            }
        });
        self.moves.running = Some(Moving { what, control, progress, done: Mutex::new(done) });
        self.redraw = true;
    }

    /// Exports into the folder chosen (or brings the export in it up to date). The settings on disk are the ones
    /// exported: they're written as they change.
    pub(super) fn export_to(&mut self, dest: &Path) {
        self.start_move("export", dest.to_path_buf());
    }

    pub(super) fn import_from(&mut self, src: &Path) {
        self.start_move("import", src.to_path_buf());
    }

    /// What an export or import has done since the last frame; and how big an export would be, measured once the
    /// System page is up.
    pub(super) fn poll_moves(&mut self) {
        if self.moves.restart_at.is_some_and(|at| Instant::now() >= at) {
            rustyshelf_core::settings::flush();
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).args(std::env::args_os().skip(1)).spawn();
            }
            std::process::exit(0);
        }
        if let Some(size) = self.moves.measuring.as_ref().and_then(|rx| rx.lock().unwrap().try_recv().ok()) {
            (self.moves.size, self.moves.measuring, self.redraw) = (Some(size), None, true);
        }
        let on_page = self.open && self.stack.last().is_some_and(|p| p.0 == Screen::System);
        if on_page && self.moves.size.is_none() && self.moves.measuring.is_none() && self.moves.running.is_none() {
            let size = self.on_install(|app_root, data_dir, cfg, art_dirs, art_lib| dataexport::export_size(&Install { app_root, data_dir, cfg, art_dirs, art_lib }));
            self.moves.measuring = Some(Mutex::new(size));
        }
        let Some(m) = &self.moves.running else { return };
        let ended = m.done.lock().unwrap().try_recv();
        match ended {
            Ok(result) => {
                let import = m.what == "import";
                (self.moves.running, self.moves.size) = (None, None);
                if import && result.is_ok() {
                    self.moves.restart_at = Some(Instant::now() + Duration::from_millis(2500));
                }
                self.say(result.unwrap_or_else(|why| why));
            }
            Err(TryRecvError::Disconnected) => self.moves.running = None,
            // the row follows the copy
            Err(TryRecvError::Empty) if on_page => self.redraw = true,
            Err(TryRecvError::Empty) => {}
        }
    }

    pub(super) fn moving_rows(&self) -> Vec<Row> {
        let Some(m) = &self.moves.running else {
            let size = self.moves.size.map_or("measuring…".to_string(), |(bytes, files)| format!("{} · {} files", crate::hud::size(bytes), count(files)));
            return vec![
                Row {
                    label: "Export everything".into(),
                    value: size,
                    hint: "Settings, downloads (art, manuals, the box art database) and the art kept next to your games, into a folder you choose: for another PC or a second install. A folder that already holds an export is brought up to date.".into(),
                    press: Some(Box::new(|s: &mut Settings| s.pick(Pick::ExportTo, "Choose where the export goes", true))),
                    step: None,
                },
                Row {
                    label: "Import everything".into(),
                    hint: "Copies an export back: nothing is deleted, files of the same name are replaced, and the settings there are now are kept as a backup file. RustyShelf then starts again.".into(),
                    press: Some(Box::new(|s: &mut Settings| s.pick(Pick::ImportFrom, "Choose the RustyShelf export folder", true))),
                    ..Row::default()
                },
            ];
        };
        let p = m.progress.lock().unwrap().clone();
        let paused = m.control.paused.load(Ordering::Relaxed);
        let (doing, noun) = if m.what == "export" { ("Exporting", "export") } else { ("Importing", "import") };
        let percent = (p.bytes * 100).checked_div(p.of).unwrap_or(0);
        let mut value = format!("{} of {} · {} files", crate::hud::size(p.bytes), crate::hud::size(p.of), count(p.files));
        if let Some(eta) = p.eta.filter(|_| !paused) {
            value = format!("{value} · {}", left(eta));
        }
        let (control, stop) = (m.control.clone(), m.control.clone());
        vec![
            Row {
                label: if paused { format!("{doing} paused at {percent}%") } else { format!("{doing}… {percent}%") },
                value,
                hint: if paused { "Press to go on".into() } else { format!("{}Press to pause", if p.label.is_empty() { String::new() } else { format!("{} · ", p.label) }) },
                press: Some(Box::new(move |s: &mut Settings| {
                    control.paused.fetch_xor(true, Ordering::Relaxed);
                    s.redraw = true;
                })),
                step: None,
            },
            Row {
                label: format!("Cancel the {noun}"),
                hint: "Stops between two files. What's copied so far stays; the next export into the same folder goes on from it.".into(),
                press: Some(Box::new(move |_: &mut Settings| stop.cancelled.store(true, Ordering::Relaxed))),
                ..Row::default()
            },
        ]
    }
}
