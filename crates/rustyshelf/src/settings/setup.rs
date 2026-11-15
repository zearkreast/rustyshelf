// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the page and its wording follow
// Set up for me in Boxshelf's renderer/settings.js and main.js, as RustyShelf has them.
//! Settings → a console → Set up for me: the emulator set up the way the shelf expects, the RetroArch core and the
//! BIOS files it needs. Also the first run's "Set up my emulators for me" and the emulators that wait for their
//! first launch.

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use rustyshelf_core::emusetup::{self, group_name, Kind, Options, Plan, Setup, Status};
use rustyshelf_core::{js, launch, racores};

use super::*;

const PRESETS: [(&str, &str); 4] = [("auto", "For this PC"), ("fast", "Fast"), ("balanced", "Balanced"), ("pretty", "Pretty")];

fn about(group: &str) -> &'static str {
    match group {
        "controller" => "Maps your controller as player 1, when the emulator is still on keyboard-only controls. A mapping you made yourself is kept.",
        "hotkeys" => "Puts save, load, pause and screenshot on the keys the Home menu presses, so its Save and Load work in your games.",
        "bios" => "Points the emulator at one shared BIOS folder (RustyShelf\\BIOS), only when its own BIOS folder is empty.",
        "graphics" => "Sets the resolution to the Graphics preset below.",
        "quit" => "No \"are you sure?\" question when a game is closed.",
        "resume" => "RetroArch saves when you quit a game and carries on from there next time.",
        _ => "",
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// The day (UTC) of a time in ms since 1970, as "November 21, 1999".
pub(super) fn day(ms: u64) -> String {
    let z = (ms / 86_400_000) as i64 + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    format!("{} {d}, {}", MONTHS[(m - 1) as usize], yoe + era * 400 + i64::from(m <= 2))
}

fn names(groups: &[impl AsRef<str>]) -> String {
    groups.iter().map(|g| Some(group_name(g.as_ref())).filter(|n| !n.is_empty()).unwrap_or(g.as_ref())).collect::<Vec<_>>().join(", ")
}

struct Core {
    id: String,
    name: String,
    installed: bool,
}

struct Bios {
    /// None: any BIOS file will do (the emulator isn't picky about names)
    need: Option<racores::Need>,
    note: &'static str,
    ok: bool,
    files: Vec<racores::BiosFile>,
    dir: String,
}

/// What's known of a console's emulator, read when its page opens and after each thing done there.
struct Info {
    console: String,
    exe: String,
    /// it can't be set up: what the page says, and why
    no: Option<(String, String)>,
    status: Option<Status>,
    /// set up at its next launch
    pending: bool,
    preset: &'static str,
    core: Option<Core>,
    bios: Option<Bios>,
}

type Fetched = Mutex<Receiver<Result<String, String>>>;

#[derive(Default)]
pub(super) struct SetupState {
    info: Option<Info>,
    /// the page's switches, a group and whether it's on (not saved)
    on: Vec<(&'static str, bool)>,
    plan: Option<Plan>,
    /// a core being downloaded: its percent, and its name or what went wrong at the end
    core: Option<(Arc<AtomicU32>, u32, Fetched)>,
}

/// An emulator that waits for its first launch to be set up, for the thread that starts it.
pub struct AtLaunch {
    data_dir: PathBuf,
    app_root: PathBuf,
    pending: Vec<String>,
    resume: bool,
    opts: Options,
}

impl AtLaunch {
    /// Sets the emulator up if it's one that waited and its settings file is there now; the waiting list without it.
    pub fn run(&self, exe: &Path) -> Option<Vec<String>> {
        let setup = Setup { data_dir: self.data_dir.clone(), app_root: self.app_root.clone(), var: &paths::env_var };
        let root = self.app_root.clone();
        setup.at_launch(exe, &move |e: &str| launch::emu_path(e, &root), &self.pending, self.resume, &self.opts, now_ms())
    }
}

impl Settings {
    fn setup(&self) -> Setup<'static> {
        Setup { data_dir: paths::data_dir(&self.app_root), app_root: self.app_root.clone(), var: &paths::env_var }
    }

    fn options(&self) -> Options {
        let chosen = self.text(&["emuSetup", "graphics"]);
        let graphics = if chosen.is_empty() || chosen == "auto" { emusetup::preset_for(self.gpu_vendor).to_string() } else { chosen };
        Options { graphics, bios_dir: String::new() }
    }

    /// A console's emulator as the settings have it: its own, else RetroArch for a console RetroArch runs.
    pub(super) fn emulator_of(&self, id: &str) -> String {
        let own = self.text(&["systems", id, "emulator"]);
        if own.is_empty() && self.console(id).is_some_and(|c| c.retro) {
            return self.text(&["retroarch"]);
        }
        own
    }

    /// Whether a console's page offers Set up for me: its own emulator is one RustyShelf knows.
    pub(super) fn can_set_up(&self, id: &str) -> bool {
        emusetup::ON && emusetup::kind_of(&self.text(&["systems", id, "emulator"])).is_some()
    }

    pub fn at_launch(&self) -> Option<AtLaunch> {
        let pending = self.list(&["emuSetup", "pending"]);
        if !emusetup::ON || pending.is_empty() {
            return None;
        }
        Some(AtLaunch { data_dir: paths::data_dir(&self.app_root), app_root: self.app_root.clone(), pending, resume: self.flag(&["emuSetup", "resume"], false), opts: self.options() })
    }

    /// Keeps the list of emulators that wait for their first launch.
    pub fn set_pending(&mut self, pending: Vec<String>) {
        put(&mut self.cfg, &["emuSetup", "pending"], json!(pending));
        self.save();
    }

    /// The names of the emulators of the consoles switched on that RustyShelf can set up.
    pub(super) fn setup_kinds(&self) -> Vec<&'static str> {
        let mut kinds: Vec<&'static str> = Vec::new();
        for c in systems().consoles.iter().filter(|c| self.flag(&["systems", &c.id, "enabled"], true)) {
            if let Some(k) = emusetup::kind_of(&self.text(&["systems", &c.id, "emulator"])) {
                if !kinds.contains(&k.name()) {
                    kinds.push(k.name());
                }
            }
        }
        kinds
    }

    /// The first run's "Set up my emulators for me"; what it did, in a line.
    pub(super) fn auto_setup(&mut self) -> Option<String> {
        if !emusetup::ON || !self.flag(&["emuSetup", "auto"], true) || self.setup_kinds().is_empty() {
            return None;
        }
        let emulators: Vec<String> = systems().consoles.iter().filter(|c| self.flag(&["systems", &c.id, "enabled"], true)).map(|c| self.text(&["systems", &c.id, "emulator"])).collect();
        let root = self.app_root.clone();
        let r = self.setup().auto(&emulators, &move |e: &str| launch::emu_path(e, &root), &self.list(&["emuSetup", "pending"]), self.flag(&["emuSetup", "resume"], false), &self.options(), now_ms());
        self.set_pending(r.pending);
        let mut lines = Vec::new();
        if !r.done.is_empty() {
            lines.push(format!("Set up: {}.", r.done.join(", ")));
        }
        if !r.later.is_empty() {
            lines.push(format!("Set up after you play once: {}.", r.later.join(", ")));
        }
        if !r.left.is_empty() {
            lines.push(format!("Left as you had them: {} (Settings → the console → Set up for me).", r.left.join(", ")));
        }
        Some(if lines.is_empty() { "Your emulators were already set up.".into() } else { lines.join(" ") })
    }

    /// Reads what the page shows for a console's emulator. The switches start again for another emulator.
    pub(super) fn load_setup(&mut self, id: &str) {
        let emu = self.emulator_of(id);
        let exe = launch::emu_path(&emu, &self.app_root).to_string_lossy().into_owned();
        if !self.setup.info.as_ref().is_some_and(|i| i.console == id && i.exe == exe) {
            self.setup.on.clear();
            self.setup.plan = None;
        }
        let mut info = Info { console: id.to_string(), exe: exe.clone(), no: None, status: None, pending: false, preset: "", core: None, bios: None };
        let setup = self.setup();
        let status = if emu.is_empty() { None } else { setup.status(&exe, now_ms()).filter(|_| emusetup::ON) };
        match status {
            None if emu.is_empty() => info.no = Some(("Choose this console's emulator first".into(), String::new())),
            None if !emusetup::ON => info.no = Some(("RustyShelf can't set this emulator up yet".into(), "Windows only for now".into())),
            None => info.no = Some(("RustyShelf can't set this emulator up yet".into(), "RetroArch, Dolphin, DuckStation, PCSX2 and PPSSPP for now.".into())),
            Some(st) => {
                if self.setup.on.is_empty() {
                    self.setup.on = st.groups.iter().map(|g| (*g, *g != "resume")).collect();
                }
                let root = self.app_root.clone();
                info.pending = self.list(&["emuSetup", "pending"]).iter().any(|p| launch::emu_path(p, &root).to_string_lossy().to_lowercase() == exe.to_lowercase());
                info.preset = emusetup::preset_for(self.gpu_vendor);
                let console = self.console(id);
                let own = setup.bios_dir_of(st.kind, &exe);
                match st.kind {
                    Kind::RetroArch => {
                        if let (Some(c), Some(cfg)) = (console.filter(|c| c.ra_core.is_some()), Config::from_value(&self.cfg)) {
                            let sc = cfg.system(c);
                            if let Some(core) = launch::core_of(c, &sc) {
                                let name = c.ra_cores.iter().find(|(k, _)| *k == core).map_or(core.clone(), |(_, n)| n.clone());
                                info.core = Some(Core { installed: launch::retro_core(Path::new(&exe), &core).is_some(), id: core, name });
                            }
                            let mut dirs: Vec<String> = own.iter().cloned().chain([setup.shared_bios_dir()]).collect();
                            // Neo Geo's BIOS can sit with its games
                            if id == "neogeo" {
                                dirs.extend(sc.rom_dirs.iter().cloned());
                            }
                            info.bios = racores::bios(id, &dirs).map(|b| Bios { need: Some(b.need), note: b.note, ok: b.ok, files: b.files, dir: String::new() });
                        }
                    }
                    Kind::DuckStation | Kind::Pcsx2 => {
                        let wanted = |n: &str| {
                            let n = n.to_lowercase();
                            n.ends_with(".bin") && (st.kind != Kind::Pcsx2 || ["scph", "ps2", "bios"].iter().any(|p| n.starts_with(p)))
                        };
                        let any = own.as_ref().and_then(|d| std::fs::read_dir(d).ok()).is_some_and(|l| l.flatten().any(|e| wanted(&e.file_name().to_string_lossy())));
                        info.bios = Some(Bios { need: None, note: "", ok: any, files: Vec::new(), dir: String::new() });
                    }
                    _ => {}
                }
                if let Some(b) = &mut info.bios {
                    b.dir = own.filter(|d| !d.is_empty()).unwrap_or_else(|| setup.shared_bios_dir());
                }
                info.status = Some(st);
            }
        }
        self.setup.info = Some(info);
        self.redraw = true;
    }

    /// A core download's progress, and its end.
    pub(super) fn poll_core(&mut self) {
        let Some((pct, shown, rx)) = &mut self.setup.core else { return };
        let now = pct.load(Ordering::Relaxed);
        let answer = rx.get_mut().unwrap().try_recv();
        if now != *shown {
            *shown = now;
            self.redraw = true;
        }
        let said = match answer {
            Ok(Ok(name)) => format!("{name} core is ready"),
            Ok(Err(why)) => why,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => String::new(),
        };
        self.setup.core = None;
        if let Some(id) = self.setup.info.as_ref().map(|i| i.console.clone()) {
            self.load_setup(&id);
        }
        self.note = said;
        self.redraw = true;
    }

    pub(super) fn setup_page(&self, id: &str) -> Page {
        let said = |label: String, value: String, hint: String| Row { label, value, hint, ..Row::default() };
        let emu = emuname::label(&self.emulator_of(id));
        let title = format!("Set up {emu} for me");
        let lead = format!("RustyShelf sets {emu} up the way the shelf works best. Each switch is one kind of change; your files are backed up first, and Undo puts them back.");
        let Some(i) = self.setup.info.as_ref().filter(|i| i.console == id) else { return Page { title, lead, rows: vec![said("Checking the emulator…".into(), String::new(), String::new())] } };
        let Some(st) = &i.status else {
            let (head, why) = i.no.clone().unwrap_or_default();
            return Page { title, lead, rows: vec![said(head, String::new(), why)] };
        };
        let name = st.kind.name();
        let (head, under) = match &st.done {
            _ if !st.ready && i.pending => (format!("Open {name} once first"), "It's set up the next time you play.".to_string()),
            _ if !st.ready => (format!("Open {name} once first"), format!("{name} makes its settings file the first time it runs; set it up after that.")),
            Some((at, groups)) => (format!("{name} is set up"), format!("Since {}: {}.", day(*at), names(groups))),
            None if st.used => (format!("You've set {name} up yourself"), "Your settings stay as they are unless you set it up here. Everything it changes is backed up first.".to_string()),
            None => (format!("{name} isn't set up yet"), "Everything it changes is backed up first, and Undo puts it back.".to_string()),
        };
        let mut rows = vec![said(head, String::new(), under)];
        for (g, on) in &self.setup.on {
            let g = *g;
            let flip = move |s: &mut Settings| {
                if let Some(x) = s.setup.on.iter_mut().find(|x| x.0 == g) {
                    x.1 = !x.1;
                }
                s.setup.plan = None;
                s.redraw = true;
            };
            rows.push(Row { label: group_name(g).into(), value: on_off(*on), hint: about(g).into(), press: Some(Box::new(flip)), step: Some(Box::new(move |s: &mut Settings, _| flip(s))) });
        }
        if st.groups.contains(&"graphics") {
            let chosen = self.text(&["emuSetup", "graphics"]);
            let at = PRESETS.iter().position(|p| p.0 == chosen).unwrap_or(0);
            let turn = move |s: &mut Settings, d: i32| {
                let n = (at as i32 + d).rem_euclid(PRESETS.len() as i32) as usize;
                s.set(&["emuSetup", "graphics"], json!(PRESETS[n].0));
                s.setup.plan = None;
            };
            let here = PRESETS.iter().find(|p| p.0 == i.preset).map_or(String::new(), |p| format!(" ({} here)", p.1));
            rows.push(Row {
                label: "Graphics preset".into(),
                value: PRESETS[at].1.into(),
                hint: format!("Fast: the console's own resolution. Balanced: about twice that. Pretty: three to four times, for a strong graphics card. For this PC picks one from your graphics card{here}."),
                press: Some(Box::new(move |s: &mut Settings| turn(s, 1))),
                step: Some(Box::new(turn)),
            });
        }
        for (group, why) in &st.skipped {
            rows.push(said(format!("{}: left as it is", group_name(group)), String::new(), why.to_string()));
        }
        if st.ready {
            let (exe, console) = (i.exe.clone(), id.to_string());
            rows.push(Row {
                label: if st.done.is_some() { "Set up again".into() } else { format!("Set up {name}") },
                hint: format!("Applies the switches above to {name}. Your files are backed up first."),
                press: Some(Box::new(move |s: &mut Settings| {
                    let on = s.setup.on.clone();
                    let r = s.setup().apply(&exe, &on, &s.options(), now_ms());
                    s.load_setup(&console);
                    s.note = match (r.ok, r.applied.is_empty()) {
                        (true, false) => format!("{name} set up: {}", names(&r.applied)),
                        (true, true) => format!("{name} already matches"),
                        (false, _) => format!("Couldn't set up {name}: {}", if r.why.is_empty() { "unknown problem" } else { &r.why }),
                    };
                })),
                ..Row::default()
            });
            if self.setup.plan.is_none() {
                let (exe, kind) = (i.exe.clone(), st.kind);
                rows.push(Row {
                    label: "Show what will change".into(),
                    hint: "Lists every setting it would write, file by file, before you set anything up.".into(),
                    press: Some(Box::new(move |s: &mut Settings| {
                        s.setup.plan = Some(s.setup().plan(kind, &exe, &s.options()));
                        s.redraw = true;
                    })),
                    ..Row::default()
                });
            }
        }
        if st.done.is_some() {
            let (exe, console) = (i.exe.clone(), id.to_string());
            rows.push(Row {
                label: "Undo".into(),
                hint: format!("Puts {name}'s settings back exactly as they were before RustyShelf set it up."),
                press: Some(Box::new(move |s: &mut Settings| {
                    if !s.twice(&format!("undo:{console}"), "Press again to undo") {
                        return;
                    }
                    let restored = s.setup().undo(&exe);
                    s.load_setup(&console);
                    s.note = if restored.is_empty() { "Nothing to undo".into() } else { format!("{name}'s settings are back as they were") };
                })),
                ..Row::default()
            });
        }
        if let Some(core) = &i.core {
            let (exe, cid, cname) = (i.exe.clone(), core.id.clone(), core.name.clone());
            let installed = core.installed;
            rows.push(Row {
                label: format!("{} core", core.name),
                value: match &self.setup.core {
                    Some((_, pct, _)) => format!("Downloading… {pct}%"),
                    None if installed => "✓ Ready".into(),
                    None => "Get it".into(),
                },
                hint: if installed {
                    format!("RetroArch has the {} core it uses for this console.", core.name)
                } else {
                    format!("Downloads the {} core from libretro, the same place RetroArch's Online Updater uses. It's also fetched by itself the first time you play.", core.name)
                },
                press: Some(Box::new(move |s: &mut Settings| {
                    if installed || s.setup.core.is_some() {
                        return;
                    }
                    let (tx, rx) = channel();
                    let pct = Arc::new(AtomicU32::new(0));
                    let (exe, cid, cname, at) = (exe.clone(), cid.clone(), cname.clone(), pct.clone());
                    std::thread::spawn(move || {
                        let got = racores::get(&exe, &cid, &paths::env_var, &mut |p| at.store(p, Ordering::Relaxed));
                        let _ = tx.send(got.map(|_| cname));
                    });
                    s.setup.core = Some((pct, 0, Mutex::new(rx)));
                    s.redraw = true;
                })),
                step: None,
            });
        }
        if let Some(b) = &i.bios {
            let missing = match b.need {
                Some(racores::Need::One) => "Missing (one of these is enough)".to_string(),
                Some(racores::Need::Optional) => format!("Missing, optional: {}", b.note),
                _ if b.note.is_empty() => "Missing".to_string(),
                _ => format!("Missing ({})", b.note),
            };
            for f in &b.files {
                let state = match f.state {
                    racores::State::Ok => "✓ Good".to_string(),
                    racores::State::Wrong => "Wrong version: not the known good copy".to_string(),
                    racores::State::Missing => missing.clone(),
                };
                rows.push(said(if f.region.is_empty() { f.name.to_string() } else { format!("{} · {}", f.name, f.region) }, state, "A BIOS file this console's core looks for".into()));
            }
            if b.files.is_empty() {
                rows.push(said(if b.ok { "BIOS files found".into() } else { "No BIOS files yet".into() }, if b.ok { "✓ Good".into() } else { String::new() }, if b.ok { String::new() } else { format!("{name} needs a BIOS dumped from your own console.") }));
            }
            let (dir, console) = (b.dir.clone(), id.to_string());
            rows.push(Row {
                label: "Open the BIOS folder".into(),
                hint: format!("Opens the folder {name} reads BIOS files from: {}. Copy the files from your own console there, then Check again.", b.dir),
                press: Some(Box::new(move |s: &mut Settings| {
                    let _ = std::fs::create_dir_all(&dir);
                    if let Err(e) = launch::open_folder(Path::new(&dir)) {
                        s.note = e.to_string();
                        s.redraw = true;
                    }
                })),
                ..Row::default()
            });
            rows.push(Row { label: "Check again".into(), hint: "Looks in the BIOS folder again.".into(), press: Some(Box::new(move |s: &mut Settings| s.load_setup(&console))), ..Row::default() });
        }
        if let Some(plan) = &self.setup.plan {
            let on = |g: &str| self.setup.on.iter().any(|x| x.0 == g && x.1);
            let changes: Vec<_> = plan.changes.iter().filter(|c| on(c.group)).collect();
            rows.push(said("What will change".into(), if changes.is_empty() { "Nothing, with these switches".into() } else { format!("{} settings", changes.len()) }, String::new()));
            for c in changes.iter().take(60) {
                let key = if c.section.is_empty() { c.key.clone() } else { format!("[{}] {}", c.section, c.key) };
                rows.push(said(format!("{key} = {}", c.value), String::new(), format!("{} · {}", group_name(c.group), js::basename(&c.file))));
            }
            if changes.len() > 60 {
                rows.push(said(format!("…and {} more", changes.len() - 60), String::new(), String::new()));
            }
        }
        Page { title, lead, rows }
    }
}
