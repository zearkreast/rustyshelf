//! Settings → RomM: the server's address, pairing (a QR for the phone, or an 8-digit code), the connection test, a
//! sync now, whether games not on this device show, and signing out.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rustyshelf_core::romm::{origin_of, Error, PairStart, Poll, Setup, Shown, Test};

use super::*;

pub(super) enum Answer {
    Started(Result<PairStart, Error>),
    Pairing(Poll),
    Code(Result<String, Error>),
    Tested(Test),
    Synced(Shown),
}

#[derive(Default)]
pub(super) struct RommUi {
    answer: Option<Mutex<std::sync::mpsc::Receiver<Answer>>>,
    /// the code and the page to approve it on, while a pairing waits
    pair: Option<(String, String)>,
    pairing: bool,
    cancel: Arc<AtomicBool>,
    busy: Option<&'static str>,
    test: Option<String>,
}

impl Settings {
    fn romm_setup(&self) -> Setup {
        let cfg = rustyshelf_core::config::Config::parse(&self.cfg.to_string()).unwrap_or_default();
        Setup::from_config(&cfg, self.seal_key.as_ref())
    }

    /// The page the QR leads to, while a pairing waits on the RomM page.
    pub(super) fn romm_link(&self) -> Option<String> {
        self.romm.pair.as_ref().filter(|_| self.stack.last().is_some_and(|p| p.0 == Screen::Romm)).map(|p| p.1.clone())
    }

    fn romm_ask(&mut self, busy: &'static str, work: impl FnOnce(&rustyshelf_core::romm::Romm, &std::sync::mpsc::Sender<Answer>) + Send + 'static) {
        let (client, data) = (self.romm_setup(), paths::data_dir(&self.app_root));
        let (tx, rx) = std::sync::mpsc::channel();
        self.romm.answer = Some(Mutex::new(rx));
        self.romm.busy = Some(busy);
        self.redraw = true;
        std::thread::spawn(move || work(&client.client(&data), &tx));
    }

    fn romm_signed_in(&mut self, token: &str) {
        let origin = origin_of(&self.text(&["romm", "server"]));
        self.set_api_key("rommToken", token.to_string());
        self.set_api_key("rommTokenServer", origin);
        self.changed = true;
        self.toast = Some("Paired with RomM".into());
    }

    /// What RomM answered since the last frame.
    pub(super) fn poll_romm(&mut self) {
        // a pairing left waiting when its page closed is stopped, not shown stale on the next visit
        if self.romm.pairing && !self.romm.cancel.load(Ordering::Relaxed) && !(self.open && self.stack.last().is_some_and(|p| p.0 == Screen::Romm)) {
            self.romm.cancel.store(true, Ordering::Relaxed);
        }
        loop {
            let got = match self.romm.answer.as_ref().map(|rx| rx.lock().unwrap().try_recv()) {
                Some(Ok(a)) => a,
                Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                    self.romm.answer = None;
                    self.romm.busy = None;
                    return;
                }
                _ => return,
            };
            self.redraw = true;
            match got {
                Answer::Started(Ok(p)) => self.romm.pair = Some((p.user_code, p.qr_url)),
                Answer::Started(Err(e)) => {
                    self.romm.pairing = false;
                    self.say(if e == Error::Unsupported { "This RomM is older than 5.0: pair with a code instead (below).".into() } else { format!("RomM didn't answer: {}", e.code()) });
                }
                Answer::Pairing(p) => {
                    (self.romm.pair, self.romm.pairing) = (None, false);
                    match p {
                        Poll::Signed { token } => {
                            self.romm_signed_in(&token);
                            self.say("Paired. Your RomM games are on their way to the shelves.".into());
                        }
                        Poll::Denied => self.say("Pairing was turned down in RomM.".into()),
                        Poll::Expired => self.say("The code ran out: press Pair for a new one.".into()),
                        Poll::Cancelled => self.say("Pairing stopped".into()),
                        Poll::Pending { .. } => {}
                    }
                }
                Answer::Code(Ok(token)) => {
                    self.romm_signed_in(&token);
                    self.say("Paired with RomM".into());
                }
                Answer::Code(Err(e)) => self.say(format!("RomM didn't accept that code{}", if e == Error::BadCode { String::new() } else { format!(": {}", e.code()) })),
                Answer::Tested(t) => {
                    self.romm.test = Some(match t.outcome {
                        Ok(n) => format!("Connected{}{}", t.version.map(|v| format!(" · RomM {v}")).unwrap_or_default(), format_args!(" · {n} platforms")),
                        Err(Error::Auth | Error::NoToken) => "Not paired, or the pairing was removed".into(),
                        Err(e) => format!("Not reachable: {}", e.code()),
                    });
                }
                Answer::Synced(lib) => {
                    self.changed = true;
                    self.say(match &lib.error {
                        None => format!("RomM: {} game{}", lib.count, if lib.count == 1 { "" } else { "s" }),
                        Some(_) if lib.offline => format!("RomM: {} game{} (the saved list: the server isn't reachable)", lib.count, if lib.count == 1 { "" } else { "s" }),
                        Some(e) => format!("RomM: {}", e.code()),
                    });
                }
            }
            if !self.romm.pairing {
                self.romm.busy = None;
            }
        }
    }

    /// The RomM row's state on the Accounts page.
    pub(super) fn romm_state(&self) -> String {
        let setup = self.romm_setup();
        if setup.server.trim().is_empty() {
            return "Not set up".into();
        }
        if !setup.on() {
            return "Not paired yet".into();
        }
        let lib = setup.client(&paths::data_dir(&self.app_root)).cached();
        format!("{} game{}", lib.count, if lib.count == 1 { "" } else { "s" })
    }

    pub(super) fn romm_page(&self) -> Page {
        let setup = self.romm_setup();
        let (server, paired) = (setup.server.clone(), setup.on());
        let mut rows = vec![Row {
            label: "Server address".into(),
            value: if server.is_empty() { "Not set".into() } else { server.clone() },
            hint: "Your RomM server's address as you open it in a browser, like http://192.168.1.20:8080.".into(),
            press: Some(Box::new(move |s: &mut Settings| {
                s.edit(
                    "Server address",
                    "Your RomM server's address as you open it in a browser, like http://192.168.1.20:8080.",
                    server.clone(),
                    Box::new(|s: &mut Settings, v: String| {
                        // the sign-in belongs to the server it was paired with
                        let bound = s.api_key("rommTokenServer");
                        let bound = if bound.is_empty() { origin_of(&s.text(&["romm", "server"])) } else { bound };
                        let o = origin_of(v.trim());
                        if !s.api_key("rommToken").is_empty() && (o.is_empty() || o != bound) {
                            s.set_api_key("rommToken", String::new());
                            s.set_api_key("rommTokenServer", String::new());
                        }
                        s.set(&["romm", "server"], json!(v.trim()));
                    }),
                );
            })),
            step: None,
        }];
        if !setup.server.trim().is_empty() {
            let pairing = self.romm.pairing;
            rows.push(Row {
                label: "Pair RustyShelf with RomM".into(),
                value: if pairing { "Cancel" } else if paired { "Pair again" } else { "Pair" }.into(),
                hint: "Shows a QR code: scan it with your phone, sign in to RomM there and approve RustyShelf. No password is typed here, and RomM keeps RustyShelf signed in. Needs RomM 5.0 or newer (older: Pair with a code, below).".into(),
                press: Some(Box::new(move |s: &mut Settings| {
                    if s.romm.pairing {
                        s.romm.cancel.store(true, Ordering::Relaxed);
                        return;
                    }
                    s.romm.cancel = Arc::new(AtomicBool::new(false));
                    s.romm.pairing = true;
                    let cancel = s.romm.cancel.clone();
                    s.say("Asking RomM for a code…".into());
                    s.romm_ask("pair", move |r, tx| {
                        let started = r.pair_start("RustyShelf");
                        let ok = started.is_ok();
                        let _ = tx.send(Answer::Started(started));
                        if ok {
                            let _ = tx.send(Answer::Pairing(r.pair_wait(Some(&cancel))));
                        }
                    });
                })),
                step: None,
            });
            if let Some((code, url)) = &self.romm.pair {
                rows.push(Row { label: "Enter this code".into(), value: code.clone(), hint: format!("Scan the code with your phone's camera, or open {url} and enter {code}. Sign in to RomM there and approve RustyShelf."), ..Row::default() });
            }
            rows.push(Row {
                label: "Pair with a code".into(),
                value: "8-digit code".into(),
                hint: "In RomM: your profile → Client API tokens → Pair, then type the 8-digit code here (RomM 4.8 and newer).".into(),
                press: Some(Box::new(|s: &mut Settings| {
                    s.edit(
                        "Pair with a code",
                        "In RomM: your profile → Client API tokens → Pair, then type the 8-digit code here (RomM 4.8 and newer).",
                        String::new(),
                        Box::new(|s: &mut Settings, v: String| {
                            let code: String = v.chars().filter(char::is_ascii_digit).take(12).collect();
                            if code.len() >= 6 {
                                s.romm_ask("code", move |r, tx| drop(tx.send(Answer::Code(r.pair_with_code(&code)))));
                            }
                        }),
                    );
                })),
                step: None,
            });
            rows.push(Row {
                label: "Test the connection".into(),
                value: if self.romm.busy == Some("test") { "Checking…".into() } else { self.romm.test.clone().unwrap_or_else(|| "Test".into()) },
                hint: "Checks the address and the pairing.".into(),
                press: Some(Box::new(|s: &mut Settings| {
                    if s.romm.busy.is_none() {
                        s.romm_ask("test", |r, tx| drop(tx.send(Answer::Tested(r.test()))));
                    }
                })),
                step: None,
            });
        }
        if paired {
            rows.push(Row {
                label: "Look for new games now".into(),
                value: if self.romm.busy == Some("sync") { "Looking…".into() } else { "Sync".into() },
                hint: "RustyShelf checks RomM for new and changed games by itself a little after it starts; this does it now.".into(),
                press: Some(Box::new(|s: &mut Settings| {
                    if s.romm.busy.is_none() {
                        s.romm_ask("sync", |r, tx| {
                            let lib = r.library(&mut |_| {}, None);
                            let all: Vec<_> = lib.consoles.values().flatten().cloned().collect();
                            r.covers(&all, 500, 3, None);
                            let _ = tx.send(Answer::Synced(lib));
                        });
                    }
                })),
                step: None,
            });
            rows.push(self.switch("Show games that aren't on this device", &["romm", "cloud"], true, "Off: only RomM games you've already downloaded show on the shelves."));
        }
        if setup.server.trim().get(..8).is_some_and(|p| p.eq_ignore_ascii_case("https://")) {
            rows.push(self.switch("Skip the HTTPS certificate check", &["romm", "skipTls"], false, "Only for a home server with its own (self-signed) certificate. Leave off otherwise."));
        }
        if paired {
            rows.push(Row {
                label: "Sign out of RomM".into(),
                value: "Sign out".into(),
                hint: "Forgets the pairing and RomM's list of games on this device. Games you downloaded stay where they are.".into(),
                press: Some(Box::new(|s: &mut Settings| {
                    if s.twice("romm-out", "Enter again to sign out of RomM.") {
                        s.romm_setup().client(&paths::data_dir(&s.app_root)).forget();
                        s.set_api_key("rommToken", String::new());
                        s.set_api_key("rommTokenServer", String::new());
                        s.romm.test = None;
                        s.changed = true;
                        s.say("Signed out of RomM".into());
                    }
                })),
                step: None,
            });
        }
        Page {
            title: "RomM".into(),
            lead: "Your own RomM server's games join your shelves, with RustyShelf's full box art, manuals and achievements. A game that isn't on this device yet has a cloud on its case and is downloaded into its console's games folder when you press Play.".into(),
            rows,
        }
    }
}
