// Based on ZakAuMiel/boxshelf-source (https://github.com/ZakAuMiel/boxshelf-source, GPL-3.0), Zak's fork of Boxshelf by
// sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source): the page and its wording follow its
// renderer/settings.js, and the host's games asked for behind the shelf its main.js.
//! Settings → Game streaming: the host PC, Moonlight's program, the host's games, waking the host and the stream's
//! picture; and the host's games asked for behind the shelf.

use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use rustyshelf_core::moonlight::{self, App};

use super::*;

/// The host is first asked this long after the start, then every ten minutes.
const FIRST_ASK: f32 = 8.0;
const EVERY: Duration = Duration::from_secs(600);

pub(super) enum StreamAnswer {
    Found(Option<PathBuf>),
    /// Moonlight's program when it was found on the way, the host asked, and what it said
    Listed(Option<String>, String, Result<Vec<App>, String>),
    Woke(bool),
}

pub(super) struct StreamingUi {
    tx: Sender<StreamAnswer>,
    rx: Mutex<Receiver<StreamAnswer>>,
    finding: bool,
    listing: bool,
    /// the page asked for the host's games: it says how that went
    page_asked: bool,
    /// the page's last ask didn't reach the host
    unreached: bool,
    asked: Option<Instant>,
    /// Moonlight was looked for in the usual places once
    looked: bool,
}

impl Default for StreamingUi {
    fn default() -> Self {
        let (tx, rx) = channel();
        StreamingUi { tx, rx: Mutex::new(rx), finding: false, listing: false, page_asked: false, unreached: false, asked: None, looked: false }
    }
}

/// What the host's answer changes in the settings: Moonlight's program when none is set yet, and the host's apps.
/// True when something did.
fn take_answer(cfg: &mut Value, found: Option<String>, apps: Option<&[App]>) -> bool {
    let mut changed = false;
    if let Some(p) = found.filter(|_| at(cfg, &["moonlight", "path"]).and_then(Value::as_str).unwrap_or("").is_empty()) {
        put(cfg, &["moonlight", "path"], json!(p));
        changed = true;
    }
    if let Some(list) = apps.map(moonlight::apps_value).filter(|l| at(cfg, &["moonlight", "apps"]) != Some(l)) {
        put(cfg, &["moonlight", "apps"], list);
        changed = true;
    }
    changed
}

impl Settings {
    fn streaming_on(&self) -> bool {
        self.flag(&["moonlight", "enabled"], false)
    }

    /// Asks the host for its games, off the screen's thread; Moonlight is looked for first when none is set.
    fn streaming_list(&mut self) {
        if self.streaming.listing {
            return;
        }
        let (path, host, tx) = (self.text(&["moonlight", "path"]), self.text(&["moonlight", "host"]), self.streaming.tx.clone());
        let look = path.is_empty() && !self.streaming.looked;
        self.streaming.looked |= look;
        (self.streaming.listing, self.streaming.asked, self.redraw) = (true, Some(Instant::now()), true);
        std::thread::spawn(move || {
            let found = if look { moonlight::find().map(|f| f.to_string_lossy().into_owned()) } else { None };
            let apps = moonlight::list(found.as_deref().unwrap_or(&path), &host);
            let _ = tx.send(StreamAnswer::Listed(found, host, apps));
        });
    }

    /// What came back since the last frame; then the host is asked again when it's due, while streaming is on and
    /// nothing is being played. A list that changed is saved from here, the settings' one writer, and the shelf is
    /// read again behind it.
    pub(super) fn poll_streaming(&mut self, since_start: f32, playing: bool) {
        loop {
            let got = self.streaming.rx.lock().unwrap().try_recv();
            let Ok(answer) = got else { break };
            self.redraw = true;
            match answer {
                StreamAnswer::Found(Some(p)) => {
                    self.streaming.finding = false;
                    self.set(&["moonlight", "path"], json!(p.to_string_lossy()));
                    self.say("Found Moonlight".into());
                }
                StreamAnswer::Found(None) => {
                    self.streaming.finding = false;
                    self.say("Moonlight isn't installed in the usual places: choose it above".into());
                }
                StreamAnswer::Woke(sent) => self.say(if sent { "Wake-up sent: give the host a few seconds" } else { "Couldn't send it: check the MAC address" }.into()),
                StreamAnswer::Listed(found, host, apps) => {
                    self.streaming.listing = false;
                    let told = std::mem::take(&mut self.streaming.page_asked);
                    // another host was named meanwhile: its own list is asked for next
                    if host != self.text(&["moonlight", "host"]) {
                        continue;
                    }
                    let changed = take_answer(&mut self.cfg, found, apps.as_deref().ok());
                    match apps {
                        Ok(_) => {
                            self.streaming.unreached = false;
                            if told {
                                let n = at(&self.cfg, &["moonlight", "apps"]).and_then(Value::as_array).map_or(0, Vec::len);
                                self.say(format!("{n} games from {host}"));
                            }
                        }
                        Err(why) if told => {
                            self.streaming.unreached = true;
                            self.say(why);
                        }
                        Err(why) => info!("Game streaming: {why}"),
                    }
                    // not `set`: that reads the whole library again when Settings closes
                    if changed {
                        self.save();
                        crate::loading::READ_AGAIN.store(true, Ordering::Relaxed);
                    }
                }
            }
        }
        let due = self.streaming.asked.map_or(since_start >= FIRST_ASK, |t| t.elapsed() >= EVERY);
        if due && !playing && self.streaming_on() && !self.text(&["moonlight", "host"]).is_empty() {
            self.streaming_list();
        }
    }

    pub(super) fn streaming_page(&self) -> Page {
        let on = self.streaming_on();
        let mut rows = vec![Row {
            label: "Show the Game streaming shelf".into(),
            value: on_off(on),
            hint: "Adds a Game streaming shelf with the games your host PC offers. Playing one opens Moonlight fullscreen; quitting it brings you back to the shelf.".into(),
            press: Some(Box::new(|s: &mut Settings| s.streaming_switch())),
            step: Some(Box::new(|s: &mut Settings, _| s.streaming_switch())),
        }];
        if on {
            let (host, path, mac) = (self.text(&["moonlight", "host"]), self.text(&["moonlight", "path"]), self.text(&["moonlight", "mac"]));
            let typed = |label: &'static str, key: &'static str, value: &str, hint: &'static str| Row {
                label: label.into(),
                value: if value.is_empty() { "Not set".into() } else { value.to_string() },
                hint: hint.into(),
                press: Some(Box::new(move |s: &mut Settings| {
                    let now = s.text(&["moonlight", key]);
                    s.edit(
                        label,
                        hint,
                        now,
                        Box::new(move |s: &mut Settings, v: String| {
                            s.set(&["moonlight", key], json!(v.trim()));
                            if key == "host" {
                                s.streaming.asked = None;
                            }
                        }),
                    );
                })),
                step: None,
            };
            rows.push(typed("Host PC", "host", &host, "The name or IP address of the PC that runs Sunshine, as Moonlight shows it (like 192.168.1.20)."));
            rows.push(Row {
                label: "Moonlight".into(),
                value: if path.is_empty() { "Not found".into() } else { short(&path) },
                hint: "Found automatically. Point to Moonlight (Moonlight.exe, moonlight, or its Flatpak or AppImage) if it's somewhere unusual.".into(),
                press: Some(Box::new(|s: &mut Settings| s.pick(Pick::Moonlight, "Choose Moonlight", false))),
                step: None,
            });
            rows.push(Row {
                label: "Find Moonlight".into(),
                value: if self.streaming.finding { "Searching…" } else { "Search" }.into(),
                hint: "Looks in the usual install places: the installer's folder, your packages, the Flatpak and ~/Applications.".into(),
                press: Some(Box::new(|s: &mut Settings| {
                    if !s.streaming.finding {
                        (s.streaming.finding, s.redraw) = (true, true);
                        let tx = s.streaming.tx.clone();
                        std::thread::spawn(move || drop(tx.send(StreamAnswer::Found(moonlight::find()))));
                    }
                })),
                step: None,
            });
            if !host.is_empty() {
                let n = at(&self.cfg, &["moonlight", "apps"]).and_then(Value::as_array).map_or(0, Vec::len);
                rows.push(Row {
                    label: "Get the games from the host".into(),
                    value: if self.streaming.listing {
                        "Asking…".into()
                    } else if self.streaming.unreached {
                        "Couldn't reach it".into()
                    } else if n > 0 {
                        format!("{n} app{} on the host", if n == 1 { "" } else { "s" })
                    } else {
                        "Refresh".into()
                    },
                    hint: "Asks the host PC for its list of games now (it's also asked by itself every few minutes). Wake it first if it's asleep.".into(),
                    press: Some(Box::new(|s: &mut Settings| {
                        (s.streaming.page_asked, s.streaming.unreached) = (true, false);
                        s.streaming_list();
                    })),
                    step: None,
                });
            }
            rows.push(Row { label: "— Waking the host —".into(), ..Row::default() });
            rows.push(typed("Host's MAC address", "mac", &mac, "Its network card's address (like AA:BB:CC:DD:EE:FF), to wake it with Wake-on-LAN before a game. Leave empty if it's always on."));
            if !mac.is_empty() {
                rows.push(Row {
                    label: "Wake the host now".into(),
                    value: "Wake".into(),
                    hint: "Sends the wake-up packet to the host PC. Wake-on-LAN has to be turned on in its BIOS and network card settings.".into(),
                    press: Some(Box::new(|s: &mut Settings| {
                        let (mac, tx) = (s.text(&["moonlight", "mac"]), s.streaming.tx.clone());
                        std::thread::spawn(move || drop(tx.send(StreamAnswer::Woke(moonlight::wake(&mac)))));
                    })),
                    step: None,
                });
            }
            rows.push(Row { label: "— Stream —".into(), ..Row::default() });
            let size = vec![(json!(""), "Moonlight's setting"), (json!("1280x720"), "720p"), (json!("1920x1080"), "1080p"), (json!("2560x1440"), "1440p"), (json!("3840x2160"), "4K")];
            rows.push(self.choose("Resolution", &["moonlight", "resolution"], size, json!(""), "The picture size the host sends. Moonlight's setting: whatever you chose in Moonlight's own window."));
            let fps = vec![(json!(0), "Moonlight's setting"), (json!(30), "30 fps"), (json!(60), "60 fps"), (json!(90), "90 fps"), (json!(120), "120 fps")];
            rows.push(self.choose("Frame rate", &["moonlight", "fps"], fps, json!(0), "Frames per second the host sends."));
            rows.push(self.switch("Close the game on the host after", &["moonlight", "quitAfter"], false, "When you quit the stream, the game is closed on the host PC too, instead of staying open there."));
        }
        Page {
            title: "Game streaming".into(),
            lead: "Games from your other PC, streamed with Moonlight: each app your Sunshine host offers gets a case on a Game streaming shelf. Pair the PC once in Moonlight's own window first.".into(),
            rows,
        }
    }

    /// The shelf on or off; turned on, the host is asked for its games at once.
    fn streaming_switch(&mut self) {
        self.toggle(&["moonlight", "enabled"], false);
        self.streaming.asked = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_new_list_or_a_first_moonlight_changes_the_settings() {
        let mut cfg = rustyshelf_core::settings::normalize(&json!({ "moonlight": { "enabled": true, "host": "pc" } })).unwrap();
        let apps = [App { name: " Elden Ring ".into(), id: 2, ..App::default() }];
        assert!(take_answer(&mut cfg, Some("/m/moonlight".into()), Some(&apps)));
        assert_eq!((cfg["moonlight"]["path"].clone(), cfg["moonlight"]["apps"].clone()), (json!("/m/moonlight"), json!([{ "name": "Elden Ring", "id": 2, "hidden": false, "boxart": "" }])));
        assert!(!take_answer(&mut cfg, Some("/other/moonlight".into()), Some(&apps)), "the same list, and a program already set");
        assert_eq!(cfg["moonlight"]["path"], "/m/moonlight");
        assert!(!take_answer(&mut cfg, None, None), "the host didn't answer: its last list stays");
        assert!(take_answer(&mut cfg, None, Some(&[App { name: "Hades".into(), hidden: true, ..App::default() }])));
        assert_eq!(cfg["moonlight"]["apps"], json!([{ "name": "Hades", "id": 0, "hidden": true, "boxart": "" }]));
    }
}
