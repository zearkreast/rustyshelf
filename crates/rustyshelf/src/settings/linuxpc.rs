//! (Linux) Settings → PC games: the Wine / Proton that Windows games run with, their prefixes, the Linux games' own
//! home, and the Epic Games account, library and downloads (through legendary).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rustyshelf_core::epicdl::{Downloads, Hooks, Item, State};
use rustyshelf_core::linuxpc::{self, Owned};

use super::*;

/// Epic's own sign-in page; the page it ends on shows the code legendary signs in with.
const EPIC_LOGIN: &str = "https://www.epicgames.com/id/login?redirectUrl=https%3A%2F%2Fwww.epicgames.com%2Fid%2Fapi%2Fredirect%3FclientId%3D34a02cf8f4414e29b15921876da36f9a%26responseType%3Dcode";

pub(super) enum EpicAnswer {
    Library(Result<Vec<Owned>, String>),
    SignedIn(Result<String, String>),
    SignedOut,
    Legendary(Result<PathBuf, String>),
    Uninstalled(String, bool),
}

#[derive(Default)]
pub(super) struct EpicUi {
    downloads: Option<Downloads>,
    /// what the downloads have to say since the last frame (an install finished or failed)
    said: Arc<Mutex<Vec<String>>>,
    changed: Arc<AtomicBool>,
    answer: Option<Mutex<std::sync::mpsc::Receiver<EpicAnswer>>>,
    /// the account's library as Epic last answered; None until it's asked for
    library: Option<Result<Vec<Owned>, String>>,
    busy: Option<&'static str>,
    /// Play pressed on a game that isn't installed: its app name, and until when a second press installs it
    offered: Option<(String, f32)>,
}

/// "45.0% · 5.00 MiB/s · 00:00:12 left"
fn pct(i: &Item) -> String {
    let mut t = format!("{:.1}%", i.pct);
    if !i.speed.is_empty() {
        t += &format!(" · {}", i.speed);
    }
    if !i.eta.is_empty() {
        t += &format!(" · {} left", i.eta);
    }
    t
}

fn state_text(i: &Item) -> String {
    match i.state {
        State::Installing => pct(i),
        State::Queued => "Waiting for its turn".into(),
        State::Paused => format!("Paused · {:.1}%", i.pct),
        State::Failed => format!("Failed · {}", if i.error.is_empty() { "the install stopped" } else { &i.error }),
        State::Cancelled => format!("Stopped · {:.1}%", i.pct),
        State::Done => "Done".into(),
    }
}

/// The code legendary signs in with: the page's whole text pasted, or the code alone.
fn sign_in_code(pasted: &str) -> String {
    let t = pasted.trim();
    serde_json::from_str::<Value>(t).ok().and_then(|j| j.get("authorizationCode").and_then(Value::as_str).map(str::to_string)).unwrap_or_else(|| t.trim_matches('"').to_string())
}

impl Settings {
    fn legendary(&self) -> Option<PathBuf> {
        linuxpc::legendary_bin(&self.text(&["pc", "legendaryPath"]), &self.app_root)
    }

    fn epic_dir(&self) -> PathBuf {
        linuxpc::epic_install_dir(&self.text(&["pc", "epicDir"]), &self.text(&["pc", "gamesDir"]), &self.app_root)
    }

    /// The download manager, made on first use; installs go on while Settings is closed.
    fn epic_downloads(&mut self) -> Downloads {
        if let Some(d) = &self.epic.downloads {
            return d.clone();
        }
        let (custom, root) = (self.text(&["pc", "legendaryPath"]), self.app_root.clone());
        let (said, changed, changed2) = (self.epic.said.clone(), self.epic.changed.clone(), self.epic.changed.clone());
        let d = Downloads::new(
            &paths::data_dir(&self.app_root),
            Hooks {
                bin: Box::new(move || linuxpc::legendary_bin(&custom, &root)),
                on_change: Box::new(move |_| changed.store(true, Ordering::Relaxed)),
                on_settled: Box::new(move |i| {
                    changed2.store(true, Ordering::Relaxed);
                    crate::loading::READ_AGAIN.store(true, Ordering::Relaxed);
                    let text = match i.state {
                        State::Done if i.update => format!("{} is up to date", i.title),
                        State::Done => format!("{} is installed: it's on the PC shelf", i.title),
                        State::Failed => format!("{}: {}", i.title, if i.error.is_empty() { "the install stopped" } else { &i.error }),
                        _ => return,
                    };
                    said.lock().unwrap().push(text);
                }),
            },
        );
        self.epic.downloads = Some(d.clone());
        d
    }

    fn epic_ask(&mut self, busy: &'static str, work: impl FnOnce(&std::path::Path) -> EpicAnswer + Send + 'static) {
        let data = paths::data_dir(&self.app_root);
        let (tx, rx) = std::sync::mpsc::channel();
        self.epic.answer = Some(Mutex::new(rx));
        self.epic.busy = Some(busy);
        self.redraw = true;
        std::thread::spawn(move || drop(tx.send(work(&data))));
    }

    fn epic_library_load(&mut self, force: bool) {
        if self.epic.busy.is_some() || (self.epic.library.is_some() && !force) {
            return;
        }
        let Some(bin) = self.legendary() else {
            self.epic.library = Some(Err("legendary not found: Settings → PC games → Get legendary.".into()));
            return;
        };
        self.epic.library = None;
        self.epic_ask("library", move |data| EpicAnswer::Library(linuxpc::epic_library(&bin, data)));
    }

    /// What the downloads and legendary answered since the last frame.
    pub(super) fn poll_epic(&mut self) {
        if self.epic.changed.swap(false, Ordering::Relaxed) && self.stack.last().is_some_and(|p| matches!(p.0, Screen::Pc | Screen::EpicLibrary | Screen::EpicDownloads)) {
            self.redraw = true;
        }
        let said: Vec<String> = std::mem::take(&mut *self.epic.said.lock().unwrap());
        if let Some(last) = said.last() {
            if self.open {
                self.say(last.clone());
            } else {
                self.toast = Some(last.clone());
            }
            self.epic.library = None;
        }
        if self.epic.library.is_none() && self.epic.busy.is_none() && self.stack.last().is_some_and(|p| p.0 == Screen::EpicLibrary) {
            self.epic_library_load(false);
        }
        let got = match self.epic.answer.as_ref().map(|rx| rx.lock().unwrap().try_recv()) {
            Some(Ok(a)) => a,
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                (self.epic.answer, self.epic.busy) = (None, None);
                return;
            }
            _ => return,
        };
        (self.epic.answer, self.epic.busy) = (None, None);
        self.redraw = true;
        match got {
            EpicAnswer::Library(l) => self.epic.library = Some(l),
            EpicAnswer::SignedIn(Ok(name)) => {
                self.epic.library = None;
                self.changed = true;
                self.say(format!("Signed in to Epic Games as {name}"));
            }
            EpicAnswer::SignedIn(Err(e)) => self.say(e),
            EpicAnswer::SignedOut => {
                self.epic.library = None;
                self.changed = true;
                self.say("Signed out of Epic Games".into());
            }
            EpicAnswer::Legendary(Ok(_)) => self.say("legendary is ready: sign in to Epic Games".into()),
            EpicAnswer::Legendary(Err(e)) => self.say(e),
            EpicAnswer::Uninstalled(title, ok) => {
                self.epic.library = None;
                self.changed = true;
                self.say(if ok { format!("{title} uninstalled") } else { format!("Couldn't uninstall {title}") });
            }
        }
    }

    /// A folder typed in, "" for the default.
    fn folder_row(&self, label: &'static str, path: &'static [&'static str], empty: String, hint: &'static str) -> Row {
        let now = self.text(path);
        Row {
            label: label.into(),
            value: if now.is_empty() { empty } else { short(&now) },
            hint: hint.into(),
            press: Some(Box::new(move |s: &mut Settings| {
                let now = s.text(path);
                s.edit(label, hint, now, Box::new(move |s: &mut Settings, v: String| s.set(path, json!(v.trim()))));
            })),
            step: Some(Box::new(move |s: &mut Settings, d| {
                if d < 0 {
                    s.set(path, json!(""));
                }
            })),
        }
    }

    /// Play on an Epic game that isn't installed: the first press says so, a second one within a few seconds puts it
    /// in the downloads. What to tell the player.
    pub fn epic_offer(&mut self, app_name: &str, title: &str, now: f32) -> String {
        if !self.epic.offered.take().is_some_and(|(app, until)| app == app_name && now <= until) {
            self.epic.offered = Some((app_name.to_string(), now + 6.0));
            return format!("{title} isn't installed yet. Press Play again to install it into {}.", short(&self.epic_dir().to_string_lossy()));
        }
        let dl = self.epic_downloads();
        let busy = dl.current().map(|j| j.title);
        match dl.add(app_name, title, &self.epic_dir().to_string_lossy(), false) {
            Ok(i) if i.state != State::Queued || busy.is_none() => format!("Installing {title}… Settings → PC games → Downloads shows how far it is."),
            Ok(_) => format!("{title} will start after {}", busy.unwrap_or_default()),
            Err(e) => e,
        }
    }

    /// The PC games page's Linux rows.
    pub(super) fn linux_pc_rows(&self) -> Vec<Row> {
        let runners = linuxpc::runners(&self.app_root);
        let chosen = self.text(&["pc", "runner"]);
        let picked = linuxpc::pick_runner(&chosen, &runners);
        let mut choices: Vec<(String, String)> = vec![(String::new(), "Best one found".into())];
        choices.extend(runners.iter().map(|r| (r.id.clone(), if r.proton { r.name.clone() } else { format!("{} (Wine)", r.name) })));
        let at = choices.iter().position(|c| c.0 == chosen).unwrap_or(0);
        let now = match (&choices[at], &picked) {
            ((id, _), Some(p)) if id.is_empty() => format!("Best one found · {}", p.name),
            ((_, name), _) => name.clone(),
        };
        let turn = cycle(&["pc", "runner"], choices.iter().map(|c| json!(c.0)).collect(), at);
        let found = if runners.is_empty() {
            "No Wine or Proton found: install Wine or GE-Proton.".to_string()
        } else {
            format!("{} found{}", runners.len(), if linuxpc::umu_run().is_some() { " · Proton runs through umu-run" } else { " · umu-run not found: Proton runs through its own script" })
        };
        let mut rows = vec![
            Row::turning(
                "Run Windows games with",
                now,
                format!("Windows games (Epic's, the PC games folder's, the programs on the Custom shelf) run through this. Proton (GE-Proton, UMU-Proton) suits most games: it brings DXVK and VKD3D. {found}"),
                turn,
            ),
            self.folder_row("Wine prefixes folder", &["pc", "prefixDir"], short(&linuxpc::prefix_root("", &self.app_root).to_string_lossy()), "Each Windows game gets a Wine prefix of its own (its own little Windows: saves, settings), a folder named after the game, in this folder. Empty: RustyShelf's own Prefixes folder."),
            self.folder_row("Game data folder", &["pc", "homeDir"], "Your home folder".into(), "Linux games get this folder as their home: their saves and settings go there, kept apart from yours. Empty: your own home folder."),
        ];
        let bin = self.legendary();
        if bin.is_none() {
            rows.push(Row {
                label: "Get legendary".into(),
                value: if self.epic.busy == Some("legendary") { "Downloading…".into() } else { "Not found · Get".into() },
                hint: "Epic games need legendary, a small open-source Epic Games client. RustyShelf downloads its official release from GitHub into its tools folder and checks it before using it.".into(),
                press: Some(Box::new(|s: &mut Settings| {
                    if s.epic.busy.is_none() {
                        let root = s.app_root.clone();
                        s.epic_ask("legendary", move |_| EpicAnswer::Legendary(linuxpc::get_legendary(&root)));
                    }
                })),
                step: None,
            });
            rows.push(self.folder_row("Or choose legendary", &["pc", "legendaryPath"], "(none)".into(), "A legendary program you have already."));
        }
        let account = linuxpc::epic_account(&paths::data_dir(&self.app_root));
        rows.push(Row {
            label: "Epic Games account".into(),
            value: match (&account, self.epic.busy) {
                (_, Some("signin")) => "Signing in…".into(),
                (Some(name), _) => format!("Signed in as {name} · Sign out"),
                (None, _) => "Not signed in · Sign in".into(),
            },
            hint: "Sign in with Epic's own page, in your browser: when it shows a line of text with authorizationCode, copy all of it and paste it here. RustyShelf keeps only legendary's sign-in; your password never reaches it.".into(),
            press: Some(Box::new(move |s: &mut Settings| {
                let Some(bin) = s.legendary() else { return s.say("Get legendary first (just above)".into()) };
                if s.epic.busy.is_some() {
                    return;
                }
                if linuxpc::epic_account(&paths::data_dir(&s.app_root)).is_some() {
                    if s.twice("epicout", "Enter again to sign out of Epic Games.") {
                        s.epic_ask("signout", move |data| {
                            linuxpc::epic_sign_out(&bin, data);
                            EpicAnswer::SignedOut
                        });
                    }
                    return;
                }
                if let Err(e) = rustyshelf_core::launch::open_link(EPIC_LOGIN) {
                    return s.say(e.to_string());
                }
                s.edit(
                    "Epic sign-in code",
                    "Sign in on the page that opened. It ends on a line of text with authorizationCode: copy all of it (or just the code) and paste it here (Ctrl+V).",
                    String::new(),
                    Box::new(move |s: &mut Settings, v: String| {
                        let (code, bin) = (sign_in_code(&v), bin.clone());
                        s.epic_ask("signin", move |data| EpicAnswer::SignedIn(linuxpc::epic_sign_in(&bin, data, &code)));
                    }),
                );
            })),
            step: None,
        });
        if account.is_some() {
            let running = self.epic.downloads.as_ref().and_then(Downloads::current);
            rows.push(Row {
                label: "Epic library".into(),
                value: running.as_ref().map_or_else(|| "Install, update and uninstall".into(), |j| format!("{} {} · {}", if j.update { "Updating" } else { "Installing" }, j.title, pct(j))),
                hint: "Every game on your Epic account. Installed games are on the PC shelf; installs and updates run in the background, one at a time.".into(),
                press: Some(Box::new(|s: &mut Settings| {
                    s.epic_downloads();
                    s.epic_library_load(false);
                    s.go(Screen::EpicLibrary)
                })),
                step: None,
            });
            let list = self.epic.downloads.as_ref().map(Downloads::list).unwrap_or_default();
            let live: Vec<&Item> = list.iter().filter(|i| i.state.live()).collect();
            rows.push(Row {
                label: "Downloads".into(),
                value: if live.is_empty() { "Nothing downloading".into() } else { format!("{} in the list{}", live.len(), live.iter().find(|i| i.state == State::Installing).map(|i| format!(" · {}", pct(i))).unwrap_or_default()) },
                hint: "The Epic downloads, one at a time. Pause stops a download without losing what's done: Resume goes on from there. A game asked for while another downloads waits its turn.".into(),
                press: Some(Box::new(|s: &mut Settings| {
                    s.epic_downloads();
                    s.go(Screen::EpicDownloads)
                })),
                step: None,
            });
            rows.push(self.folder_row("Epic install folder", &["pc", "epicDir"], short(&self.epic_dir().to_string_lossy()), "Where new Epic games are installed. Empty: the PC games folder when there's one, else RustyShelf's own Epic Games folder."));
        }
        rows
    }

    pub(super) fn epic_library_page(&self) -> Page {
        let mut rows = Vec::new();
        let list = self.epic.downloads.as_ref().map(Downloads::list).unwrap_or_default();
        let current = self.epic.downloads.as_ref().and_then(Downloads::current);
        let mine = |app: &str| list.iter().find(|i| i.app_name == app && i.state.live()).cloned();
        if let Some(j) = current.clone() {
            let id = j.id;
            rows.push(Row {
                label: format!("{} {}", if j.update { "Updating" } else { "Installing" }, j.title),
                value: format!("{} · Pause", pct(&j)),
                hint: "It keeps going while you play or browse the shelf. Pause stops it without losing what's done; the Downloads page resumes it.".into(),
                press: Some(Box::new(move |s: &mut Settings| {
                    s.epic_downloads().pause(id);
                })),
                step: None,
            });
        }
        match &self.epic.library {
            None => rows.push(Row { label: "Asking Epic for your library…".into(), ..Row::default() }),
            Some(Err(e)) => {
                rows.push(Row { label: e.clone(), ..Row::default() });
                rows.push(Row { label: "Try again".into(), value: "Retry".into(), hint: "Asks Epic for the library again.".into(), press: Some(Box::new(|s: &mut Settings| s.epic_library_load(true))), step: None });
            }
            Some(Ok(games)) => {
                let busy = current.map(|j| j.title);
                let dir = short(&self.epic_dir().to_string_lossy());
                let go = move |g: &Owned, update: bool, busy: Option<String>| -> Press {
                    let (app, title) = (g.app_name.clone(), g.title.clone());
                    Box::new(move |s: &mut Settings| {
                        let dl = s.epic_downloads();
                        if dl.list().iter().any(|i| i.app_name == app && i.state.live()) {
                            return s.say(format!("{title} is already in the downloads"));
                        }
                        let base = s.epic_dir().to_string_lossy().into_owned();
                        match dl.add(&app, &title, &base, update) {
                            Ok(_) => s.say(match &busy {
                                Some(b) => format!("{title} will start after {b}"),
                                None => format!("{} {title}…", if update { "Updating" } else { "Installing" }),
                            }),
                            Err(e) => s.say(e),
                        }
                    })
                };
                for g in games.iter().filter(|g| g.installed) {
                    let f = go(g, true, busy.clone());
                    rows.push(Row { label: g.title.clone(), value: mine(&g.app_name).map_or_else(|| "Installed · Update".into(), |i| state_text(&i)), hint: "Checks Epic for an update and installs it.".into(), press: Some(Box::new(move |s: &mut Settings| f(s))), step: None });
                    let (app, title) = (g.app_name.clone(), g.title.clone());
                    rows.push(Row {
                        label: format!("Uninstall {}", g.title),
                        value: "Uninstall".into(),
                        hint: "Deletes the game's files. Its Wine prefix (saves, settings) is kept.".into(),
                        press: Some(Box::new(move |s: &mut Settings| {
                            let Some(bin) = s.legendary() else { return };
                            if s.epic.busy.is_none() && s.twice(&format!("epicun:{app}"), "Enter again to uninstall it.") {
                                let (app, title) = (app.clone(), title.clone());
                                s.say(format!("Uninstalling {title}…"));
                                s.epic_ask("uninstall", move |data| EpicAnswer::Uninstalled(title, linuxpc::epic_uninstall(&bin, data, &app).unwrap_or(false)));
                            }
                        })),
                        step: None,
                    });
                }
                for g in games.iter().filter(|g| !g.installed) {
                    let f = go(g, false, busy.clone());
                    rows.push(Row { label: g.title.clone(), value: mine(&g.app_name).map_or_else(|| "Install".into(), |i| state_text(&i)), hint: format!("Downloads it into {dir}. It joins the PC shelf when it's done."), press: Some(Box::new(move |s: &mut Settings| f(s))), step: None });
                }
            }
        }
        Page { title: "Epic library".into(), lead: "Every game on your Epic account. Installed games are on the PC shelf; installs and updates run in the background, one at a time.".into(), rows }
    }

    pub(super) fn epic_downloads_page(&self) -> Page {
        let list = self.epic.downloads.as_ref().map(Downloads::list).unwrap_or_default();
        let mut rows = Vec::new();
        if list.is_empty() {
            rows.push(Row { label: "No downloads".into(), hint: "Install or update a game from the Epic library and it shows up here, with its progress.".into(), ..Row::default() });
        }
        for i in &list {
            let id = i.id;
            let (label, value): (String, String) = match i.state {
                State::Installing | State::Queued => ("Pause".into(), state_text(i)),
                State::Done => (String::new(), state_text(i)),
                _ => ("Resume".into(), state_text(i)),
            };
            let st = i.state;
            rows.push(Row {
                label: format!("{}{}", if i.update { "Update · " } else { "" }, i.title),
                value: if label.is_empty() { value } else { format!("{value} · {label}") },
                hint: state_text(i),
                press: Some(Box::new(move |s: &mut Settings| {
                    let dl = s.epic_downloads();
                    match st {
                        State::Installing | State::Queued => {
                            dl.pause(id);
                        }
                        State::Done => {}
                        _ => {
                            dl.resume(id);
                        }
                    }
                    s.redraw = true;
                })),
                step: None,
            });
            if matches!(i.state, State::Queued | State::Paused) {
                rows.push(Row {
                    label: format!("Move up {}", i.title),
                    value: "Up".into(),
                    hint: "Moves it ahead of the downloads before it.".into(),
                    press: Some(Box::new(move |s: &mut Settings| {
                        s.epic_downloads().move_by(id, -1);
                        s.redraw = true;
                    })),
                    step: None,
                });
            }
            if i.state != State::Done {
                rows.push(Row {
                    label: format!("Remove {}", i.title),
                    value: "Remove".into(),
                    hint: "Takes it off the list. What it downloaded so far stays, so a later install goes on from there.".into(),
                    press: Some(Box::new(move |s: &mut Settings| {
                        if s.twice(&format!("epicdl:{id}"), "Enter again to remove it.") {
                            s.epic_downloads().remove(id);
                            s.redraw = true;
                        }
                    })),
                    step: None,
                });
            }
        }
        if list.iter().any(|i| matches!(i.state, State::Done | State::Failed | State::Cancelled)) {
            rows.push(Row {
                label: "Clear finished".into(),
                value: "Clear".into(),
                hint: "Takes the finished, failed and stopped ones off the list.".into(),
                press: Some(Box::new(|s: &mut Settings| {
                    s.epic_downloads().clear_done();
                    s.redraw = true;
                })),
                step: None,
            });
        }
        Page { title: "Downloads".into(), lead: "The Epic downloads, one at a time. Pause stops a download without losing what's done: Resume goes on from there. A game asked for while another downloads waits its turn.".into(), rows }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_code_from_the_page() {
        assert_eq!(super::sign_in_code(r#"{"warning":"x","redirectUrl":"https://y","authorizationCode":"abc123def456","exchangeCode":null}"#), "abc123def456");
        assert_eq!(super::sign_in_code("  \"abc123def456\" "), "abc123def456");
    }
}
