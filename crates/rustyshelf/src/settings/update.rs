//! Settings → System → the newest release: asked of RustyShelf's releases page when the row is pressed. Nothing is
//! downloaded; the page is opened to get it from.

use rustyshelf_core::updates::{self, Newest};

use super::*;

/// What's known of the newest release, and the question on its way.
#[derive(Default)]
pub(super) struct Update {
    asking: Option<Mutex<Receiver<Result<Newest, String>>>>,
    known: Option<Newest>,
}

impl Settings {
    pub(super) fn poll_update(&mut self) {
        let Some(answer) = self.update.asking.as_ref().and_then(|rx| rx.lock().unwrap().try_recv().ok()) else { return };
        self.update.asking = None;
        match answer {
            Ok(n) => {
                self.say(match &n.latest {
                    Some(latest) if n.newer => format!("RustyShelf {latest} is out. Press again to open its page."),
                    Some(latest) => format!("The newest release is {latest}: nothing newer than this one."),
                    None => "Nothing is released yet.".into(),
                });
                self.update.known = Some(n);
            }
            Err(why) => self.say(format!("Couldn't ask for the newest release ({why}). Try again later.")),
        }
    }

    pub(super) fn update_row(&self) -> Row {
        let newer = self.update.known.as_ref().filter(|n| n.newer);
        let (label, value) = match (&self.update.asking, newer) {
            (Some(_), _) => ("Asking for the newest release…".to_string(), String::new()),
            (None, Some(n)) => ("Open the new release's page".to_string(), n.latest.clone().unwrap_or_default()),
            (None, None) => ("Check for a new version".to_string(), self.update.known.as_ref().and_then(|n| n.latest.clone()).unwrap_or_default()),
        };
        let page = newer.map(|n| n.url.clone());
        Row {
            label,
            value,
            // the releases are the Electron app's until this one has its own
            hint: format!("Asks RustyShelf's releases page on GitHub which version is newest. This is the Rust rewrite, {}; nothing is downloaded or installed.", rustyshelf_core::VERSION),
            press: Some(Box::new(move |s: &mut Settings| {
                if let Some(page) = &page {
                    if let Err(e) = rustyshelf_core::launch::open_link(page) {
                        s.say(e.to_string());
                    }
                    return;
                }
                if s.update.asking.is_some() {
                    return;
                }
                let (tx, rx) = channel();
                s.update.asking = Some(Mutex::new(rx));
                std::thread::spawn(move || {
                    let _ = tx.send(updates::check(rustyshelf_core::VERSION));
                });
                s.redraw = true;
            })),
            step: None,
        }
    }
}
