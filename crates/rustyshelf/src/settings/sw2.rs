// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main.js (sw2:builds, sw2:addBuild) and renderer/settings.js (Switch 2's emulators), as RustyShelf has them.
//! Switch 2's emulators (Eglantine and its forks): whether Play asks which one to start in, the ones added by hand or
//! taken off the list, and the build each game was last started in.

use super::*;

impl Settings {
    /// The settings as they're saved now (a build added at Play is in them before the shelf is read again).
    pub fn config_now(&self) -> Option<Config> {
        Config::from_value(&self.cfg)
    }

    /// Play shows the list of builds: Settings → Switch 2 → Emulators isn't set to one build.
    pub fn sw2_asks(&self) -> bool {
        self.flag(&["systems", "sw2", "askBuild"], true)
    }

    /// The build a game was started in comes first next time. Only a preference: it doesn't read the shelves again.
    pub fn remember_sw2_build(&mut self, file: &str, exe: &Path) {
        let mut last = at(&self.cfg, &["ui", "sw2Last"]).and_then(Value::as_object).cloned().unwrap_or_default();
        last.insert(format!("sw2:{file}"), json!(exe.to_string_lossy()));
        put(&mut self.cfg, &["ui", "sw2Last"], Value::Object(last));
        self.save();
    }

    /// One more build in the list, wherever it's kept; one taken off the list before comes back.
    pub fn add_sw2_build(&mut self, exe: &Path) -> Result<(), String> {
        if !exe.is_file() {
            return Err("That file was not found.".into());
        }
        if cfg!(windows) && !exe.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) {
            return Err("Choose the emulator's program (.exe).".into());
        }
        let Some(cfg) = self.config_now() else { return Err("The settings couldn't be read.".into()) };
        let low = nx2::low(exe);
        let hidden: Vec<String> = nx2::added(&cfg, "buildHidden").into_iter().filter(|h| nx2::low(Path::new(h)) != low).collect();
        put(&mut self.cfg, &["systems", "sw2", "buildHidden"], json!(hidden));
        let mut added = nx2::added(&cfg, "buildExes");
        if !added.iter().any(|a| nx2::low(Path::new(a)) == low) {
            added.push(exe.to_string_lossy().into_owned());
            added.truncate(20);
            put(&mut self.cfg, &["systems", "sw2", "buildExes"], json!(added));
        }
        self.save();
        Ok(())
    }

    /// The shelf's build was taken away (forks get swapped often): the newest one there is takes its place.
    pub fn replace_sw2_build(&mut self, b: &nx2::Build) {
        info!("Switch 2: shelf build gone, now {}", b.label);
        self.set(&["systems", "sw2", "emulator"], json!(b.exe.to_string_lossy()));
    }
}

/// Settings → Switch 2 → Emulators (forks): the list Play shows, looked for off the frame.
#[derive(Default)]
pub(super) struct Sw2Ui {
    list: Option<Vec<nx2::Build>>,
    looking: Option<Mutex<Receiver<Vec<nx2::Build>>>>,
    /// the list's rows take an emulator off the list instead of choosing it
    removing: bool,
}

fn day(ms: f64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64).map(|d| d.with_timezone(&chrono::Local).format("%b %-d, %Y").to_string()).unwrap_or_default()
}

fn folder_name(d: &str) -> String {
    Path::new(d).file_name().map_or_else(|| d.to_string(), |n| n.to_string_lossy().into_owned())
}

impl Settings {
    pub(super) fn load_sw2(&mut self, again: bool) {
        if self.sw2.looking.is_some() || (self.sw2.list.is_some() && !again) {
            return;
        }
        let Some(cfg) = self.config_now() else { return };
        let root = self.app_root.clone();
        let (tx, rx) = channel();
        self.sw2.looking = Some(Mutex::new(rx));
        std::thread::spawn(move || drop(tx.send(nx2::list_for(&cfg, &root, ""))));
    }

    pub(super) fn poll_sw2(&mut self) {
        let Some(list) = self.sw2.looking.as_ref().and_then(|rx| rx.lock().ok()?.try_recv().ok()) else { return };
        (self.sw2.looking, self.sw2.list, self.redraw) = (None, Some(list), true);
    }

    fn sw2_changed(&mut self) {
        self.save();
        self.sw2.list = None;
        self.load_sw2(true);
    }

    /// The build Play always starts in, when it doesn't ask.
    fn sw2_fixed(&self) -> Option<(String, String)> {
        let exe = self.text(&["systems", "sw2", "emulator"]);
        if self.sw2_asks() || exe.is_empty() {
            return None;
        }
        let low = nx2::low(Path::new(&exe));
        let label = self.sw2.list.iter().flatten().find(|b| nx2::low(&b.exe) == low).map_or_else(|| emuname::label(&exe), |b| b.label.clone());
        Some((exe, label))
    }

    fn sw2_added(&self, key: &str) -> Vec<String> {
        self.config_now().map(|c| nx2::added(&c, key)).unwrap_or_default()
    }

    /// The Switch 2 page's row that opens the list.
    pub(super) fn sw2_row(&self) -> Row {
        let hint = match &self.sw2.list {
            None => "Looking for emulators…".to_string(),
            Some(l) if l.is_empty() => "None yet · add one here".into(),
            Some(l) => format!("{} emulator{} · add or remove here", l.len(), plural_s(l.len())),
        };
        Row {
            label: "Emulators (forks)".into(),
            value: self.sw2_fixed().map_or_else(|| "Ask each time".into(), |(_, label)| label),
            hint: format!("{hint}. Your Switch 2 emulators (Eglantine and its forks): add one, remove one, or choose which one Play starts."),
            press: Some(Box::new(|s: &mut Settings| s.go(Screen::Sw2Builds))),
            step: None,
        }
    }

    pub(super) fn sw2_page(&self) -> Page {
        let fixed = self.sw2_fixed();
        let shelf = nx2::low(Path::new(&self.text(&["systems", "sw2", "emulator"])));
        let mut rows = vec![Row {
            label: "Ask each time".into(),
            value: if fixed.is_some() { "Choose".into() } else { "✓ Chosen".into() },
            hint: "Play shows the list of builds, starting on the one you last used for that game.".into(),
            press: Some(Box::new(|s: &mut Settings| {
                put(&mut s.cfg, &["systems", "sw2", "askBuild"], json!(true));
                s.cues.push(("tick", 1));
                s.save();
            })),
            step: None,
        }];
        rows.push(Row {
            label: "+ Add an emulator".into(),
            value: "Browse…".into(),
            hint: "Choose one emulator's program (eglantine.exe, lichii.exe, orchid.exe…), wherever it is. It joins this list and the one Play shows.".into(),
            press: Some(Box::new(|s: &mut Settings| s.pick(Pick::Sw2Build, "Choose a Switch 2 emulator (eglantine.exe, lichii.exe, orchid.exe…)", false))),
            step: None,
        });
        let removing = self.sw2.removing;
        for b in self.sw2.list.iter().flatten() {
            let low = nx2::low(&b.exe);
            let on = fixed.as_ref().is_some_and(|(exe, _)| nx2::low(Path::new(exe)) == low);
            let first = fixed.is_none() && shelf == low;
            let (exe, label) = (b.exe.clone(), b.label.clone());
            rows.push(Row {
                label: b.label.clone(),
                value: if removing { "Remove".into() } else if on { "✓ Always".into() } else { "Always use".into() },
                hint: format!("{}{}{} · {}", day(b.date), if b.nup { " · opens .nup" } else { "" }, if first { " · Play offers it first" } else { "" }, short(&b.exe.to_string_lossy())),
                press: Some(Box::new(move |s: &mut Settings| {
                    if !s.sw2.removing {
                        put(&mut s.cfg, &["systems", "sw2", "askBuild"], json!(false));
                        s.cues.push(("tick", 1));
                        return s.set(&["systems", "sw2", "emulator"], json!(exe.to_string_lossy()));
                    }
                    if s.twice(&format!("sw2r:{}", exe.display()), "Press again to remove it") {
                        s.sw2_remove(&exe, &label);
                    }
                })),
                step: None,
            });
        }
        if self.sw2.list.as_ref().is_some_and(|l| !l.is_empty()) {
            rows.push(Row {
                label: "Remove an emulator".into(),
                value: if removing { "Done".into() } else { "Choose…".into() },
                hint: "Switches the list above to Remove: press an emulator twice to take it off the list. Nothing is deleted.".into(),
                press: Some(Box::new(|s: &mut Settings| {
                    s.sw2.removing = !s.sw2.removing;
                    s.cues.push(("tick", 1));
                    s.redraw = true;
                })),
                step: None,
            });
        }
        let hidden = self.sw2_added("buildHidden").len();
        if hidden > 0 {
            rows.push(Row {
                label: format!("Show removed emulators ({hidden})"),
                value: "Show".into(),
                hint: "Puts every emulator you removed back on the list.".into(),
                press: Some(Box::new(|s: &mut Settings| {
                    put(&mut s.cfg, &["systems", "sw2", "buildHidden"], json!([]));
                    s.sw2_changed();
                    s.say("Removed emulators are back on the list".into());
                })),
                step: None,
            });
        }
        rows.push(Row {
            label: "Add a folder of emulators".into(),
            value: "Browse…".into(),
            hint: "A folder holding several emulators (your forks): every one inside it is listed. RustyShelf looks four folders deep.".into(),
            press: Some(Box::new(|s: &mut Settings| s.pick(Pick::Sw2Folder, "Choose a folder of Switch 2 emulators", true))),
            step: None,
        });
        for d in self.sw2_added("buildDirs") {
            let d2 = d.clone();
            rows.push(Row {
                label: format!("Folder · {}", folder_name(&d)),
                value: "Remove".into(),
                hint: format!("Emulators in this folder are listed. Remove takes the folder off the list; nothing is deleted. {}", short(&d)),
                press: Some(Box::new(move |s: &mut Settings| {
                    if s.twice(&format!("sw2d:{d2}"), "Press again to remove this folder") {
                        let dirs: Vec<String> = s.sw2_added("buildDirs").into_iter().filter(|y| *y != d2).collect();
                        put(&mut s.cfg, &["systems", "sw2", "buildDirs"], json!(dirs));
                        s.sw2_changed();
                        s.say("Folder removed from the list (nothing was deleted)".into());
                    }
                })),
                step: None,
            });
        }
        let lead = match &self.sw2.list {
            None => "Looking for your emulators…".to_string(),
            Some(l) if l.is_empty() => "No Switch 2 emulators yet: add one below.".into(),
            Some(l) => format!("Your Switch 2 emulators ({}). The same list Play shows. Ask each time shows it at Play; choose one to always start in it. A game's own choice (Details → Emulator) still comes first.", l.len()),
        };
        Page { title: "Emulators (forks)".into(), lead, rows }
    }

    /// Takes a found build off the list (`buildHidden`) or forgets an added one (`buildExes`); nothing is deleted.
    fn sw2_remove(&mut self, exe: &Path, label: &str) {
        let low = nx2::low(exe);
        let added = self.sw2_added("buildExes");
        if added.iter().any(|a| nx2::low(Path::new(a)) == low) {
            let kept: Vec<String> = added.into_iter().filter(|a| nx2::low(Path::new(a)) != low).collect();
            put(&mut self.cfg, &["systems", "sw2", "buildExes"], json!(kept));
        } else {
            let mut hidden = at(&self.cfg, &["systems", "sw2", "buildHidden"]).and_then(Value::as_array).cloned().unwrap_or_default();
            if !hidden.iter().filter_map(Value::as_str).any(|h| nx2::low(Path::new(h)) == low) {
                hidden.push(json!(exe.to_string_lossy()));
                let n = hidden.len();
                hidden.drain(..n.saturating_sub(50));
                put(&mut self.cfg, &["systems", "sw2", "buildHidden"], Value::Array(hidden));
            }
        }
        // it was the one Play always started: Play asks again
        if !self.sw2_asks() && nx2::low(Path::new(&self.text(&["systems", "sw2", "emulator"]))) == low {
            put(&mut self.cfg, &["systems", "sw2", "askBuild"], json!(true));
        }
        self.sw2_changed();
        self.say(format!("{label} removed from the list (nothing was deleted)"));
    }

    pub(super) fn sw2_picked(&mut self, folder: bool, path: &Path) {
        let p = path.to_string_lossy().into_owned();
        if folder {
            let mut dirs = self.sw2_added("buildDirs");
            if dirs.contains(&p) {
                return self.say("That folder is already in the list".into());
            }
            dirs.push(p.clone());
            dirs.truncate(20);
            put(&mut self.cfg, &["systems", "sw2", "buildDirs"], json!(dirs));
            self.sw2_changed();
            return self.say(format!("Looking for emulators in {}", folder_name(&p)));
        }
        let low = nx2::low(path);
        let was_hidden = self.sw2_added("buildHidden").iter().any(|h| nx2::low(Path::new(h)) == low);
        let listed = self.sw2_added("buildExes").iter().any(|a| nx2::low(Path::new(a)) == low) || self.sw2.list.iter().flatten().any(|b| nx2::low(&b.exe) == low);
        if !was_hidden && listed {
            return self.say("That emulator is already in the list".into());
        }
        match self.add_sw2_build(path) {
            Ok(()) => {
                self.sw2_changed();
                self.say("Emulator added".into());
            }
            Err(e) => self.say(e),
        }
    }
}
