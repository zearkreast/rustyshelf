//! Settings → System → the newest release: asked of RustyShelf's releases page when the row is pressed, or a little
//! after the start with Update by itself on. Where RustyShelf can replace itself (the Windows exe, the Linux
//! AppImage) the new version is downloaded and put in place, and used from the next start; elsewhere its page opens.

use std::time::Instant;

use rustyshelf_core::updates::{self, Newest};

use super::*;

enum Said {
    Newest(Result<Newest, String>),
    Got(u64, u64),
    Installed(Result<PathBuf, String>),
}

/// What's known of the newest release, and the work on its way.
pub(super) struct Update {
    asking: Option<Mutex<Receiver<Said>>>,
    known: Option<Newest>,
    /// percent of the download, while one runs
    got: Option<u32>,
    /// the program the new version was put in as, to start
    installed: Option<PathBuf>,
    /// the start's own check, still to come
    start_check: Option<Instant>,
    /// the start's check asked: it installs and says so in a toast, and says nothing when there's nothing new
    quiet: bool,
}

impl Default for Update {
    fn default() -> Self {
        Self { asking: None, known: None, got: None, installed: None, start_check: Some(Instant::now()), quiet: false }
    }
}

const START_CHECK_AFTER: std::time::Duration = std::time::Duration::from_secs(8);

impl Settings {
    fn auto_update(&self) -> bool {
        self.flag(&["ui", "autoUpdate"], false)
    }

    /// Asks for the newest release, and installs it when `install` and it's newer.
    fn ask_newest(&mut self, install: bool) {
        if self.update.asking.is_some() {
            return;
        }
        let (tx, rx) = channel();
        self.update.asking = Some(Mutex::new(rx));
        std::thread::spawn(move || {
            let answer = updates::check(rustyshelf_core::VERSION);
            let go = install && answer.as_ref().is_ok_and(|n| n.newer && updates::installable(n));
            let newest = answer.as_ref().ok().cloned();
            let _ = tx.send(Said::Newest(answer));
            if let (true, Some(n)) = (go, newest) {
                put_in(&n, &tx);
            }
        });
        self.redraw = true;
    }

    fn install_known(&mut self) {
        let Some(n) = self.update.known.clone() else { return };
        if self.update.asking.is_some() {
            return;
        }
        let (tx, rx) = channel();
        self.update.asking = Some(Mutex::new(rx));
        self.update.got = Some(0);
        std::thread::spawn(move || put_in(&n, &tx));
        self.redraw = true;
    }

    pub(super) fn poll_update(&mut self) {
        if self.update.start_check.is_some_and(|t| t.elapsed() >= START_CHECK_AFTER) {
            self.update.start_check = None;
            updates::tidy();
            if self.auto_update() && updates::can_install() {
                self.update.quiet = true;
                self.ask_newest(true);
            }
        }
        loop {
            let Some(said) = self.update.asking.as_ref().and_then(|rx| rx.lock().unwrap().try_recv().ok()) else { return };
            let quiet = self.update.quiet;
            match said {
                Said::Newest(Ok(n)) => {
                    let installing = quiet && n.newer && updates::installable(&n);
                    if installing {
                        self.update.got = Some(0);
                    } else {
                        self.update.asking = None;
                        self.update.quiet = false;
                    }
                    if !quiet {
                        self.say(match &n.latest {
                            Some(latest) if n.newer && updates::installable(&n) => format!("RustyShelf {latest} is out. Press again to download and install it."),
                            Some(latest) if n.newer => format!("RustyShelf {latest} is out. Press again to open its page."),
                            Some(latest) => format!("The newest release is {latest}: nothing newer than this one."),
                            None => "Nothing is released yet.".into(),
                        });
                    }
                    self.update.known = Some(n);
                }
                Said::Newest(Err(why)) => {
                    (self.update.asking, self.update.quiet) = (None, false);
                    if !quiet {
                        self.say(format!("Couldn't ask for the newest release ({why}). Try again later."));
                    }
                }
                Said::Got(got, total) => {
                    let pct = (got * 100).checked_div(total).unwrap_or(0) as u32;
                    if self.update.got != Some(pct) {
                        self.update.got = Some(pct);
                        self.redraw = true;
                    }
                }
                Said::Installed(done) => {
                    (self.update.asking, self.update.quiet, self.update.got) = (None, false, None);
                    let latest = self.update.known.as_ref().and_then(|n| n.latest.clone()).unwrap_or_default();
                    let text = match done {
                        Ok(program) => {
                            self.update.installed = Some(program);
                            format!("RustyShelf {latest} is installed: it starts the next time you open RustyShelf, or now from Settings → System.")
                        }
                        Err(why) => format!("RustyShelf {latest} couldn't be installed: {why}"),
                    };
                    if self.open {
                        self.say(text);
                    } else {
                        self.toast = Some(text);
                    }
                }
            }
            self.redraw = true;
        }
    }

    pub(super) fn update_rows(&self) -> Vec<Row> {
        let mut rows = vec![self.update_row()];
        if updates::can_install() {
            rows.push(self.switch(
                "Update by itself",
                &["ui", "autoUpdate"],
                false,
                "A little after RustyShelf starts, it looks for a new version on its releases page and, when there is one, downloads and installs it. It's used from the next start.",
            ));
        }
        rows
    }

    fn update_row(&self) -> Row {
        let latest = self.update.known.as_ref().and_then(|n| n.latest.clone()).unwrap_or_default();
        if let Some(program) = self.update.installed.clone() {
            return Row {
                label: "Restart to use the new version".into(),
                value: latest,
                hint: "The new version is installed: RustyShelf starts again with it.".into(),
                press: Some(Box::new(move |_: &mut Settings| restart_into(&program))),
                step: None,
            };
        }
        let newer = self.update.known.as_ref().filter(|n| n.newer);
        let install = newer.is_some_and(updates::installable);
        let (label, value) = match (&self.update.asking, self.update.got, newer) {
            (Some(_), Some(pct), _) => ("Downloading the new version…".to_string(), format!("{pct}%")),
            (Some(_), None, _) => ("Asking for the newest release…".to_string(), String::new()),
            (None, _, Some(n)) if install => ("Install the new version".to_string(), n.latest.clone().unwrap_or_default()),
            (None, _, Some(n)) => ("Open the new release's page".to_string(), n.latest.clone().unwrap_or_default()),
            (None, _, None) => ("Check for a new version".to_string(), latest),
        };
        let page = newer.map(|n| n.url.clone());
        Row {
            label,
            value,
            hint: if updates::can_install() {
                format!("Asks RustyShelf's releases page on GitHub which version is newest, and installs a newer one when you press again. This is version {}.", rustyshelf_core::VERSION)
            } else {
                format!("Asks RustyShelf's releases page on GitHub which version is newest. This is version {}; nothing is downloaded or installed.", rustyshelf_core::VERSION)
            },
            press: Some(Box::new(move |s: &mut Settings| {
                if s.update.asking.is_some() {
                    return;
                }
                if install {
                    return s.install_known();
                }
                if let Some(page) = &page {
                    if let Err(e) = rustyshelf_core::launch::open_link(page) {
                        s.say(e.to_string());
                    }
                    return;
                }
                s.ask_newest(false);
            })),
            step: None,
        }
    }
}

/// Downloads and installs a release, saying how far it is.
fn put_in(n: &Newest, tx: &std::sync::mpsc::Sender<Said>) {
    let done = updates::install(n, &mut |got, total| {
        let _ = tx.send(Said::Got(got, total));
    });
    let _ = tx.send(Said::Installed(done));
}
