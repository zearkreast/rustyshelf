//! The Rust port against the Electron app's own answers (tests/fixtures, made by scripts/rust-fixtures.js).

use std::cmp::Ordering;
use std::fs;
use std::path::Path;

use base64::Engine;

use rustyshelf_core::config::SystemConfig;
use rustyshelf_core::scanner::{scan, ScanData};
use rustyshelf_core::systems::systems;
use rustyshelf_core::titles::clean_title;
use rustyshelf_core::art::{resolve, ArtLibrary};
use rustyshelf_core::{collate, config::Config};
use serde_json::Value;

fn fixture(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    serde_json::from_str(&fs::read_to_string(p).unwrap()).unwrap()
}

#[test]
fn titles_match_the_js_rules() {
    let mut wrong = Vec::new();
    for case in fixture("titles.json").as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let t = clean_title(name, case["updateWords"].as_bool().unwrap());
        let got = (
            t.title.as_str(),
            t.region.as_str(),
            t.disc as u64,
            t.rev.as_str(),
            t.title_id.as_deref(),
            t.extra,
        );
        let want = (
            case["title"].as_str().unwrap(),
            case["region"].as_str().unwrap(),
            case["disc"].as_u64().unwrap(),
            case["rev"].as_str().unwrap(),
            case["titleId"].as_str(),
            case["extra"].as_bool().unwrap(),
        );
        if got != want {
            wrong.push(format!("{name:?} (updateWords {}): got {got:?}, want {want:?}", case["updateWords"]));
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

fn rel(games: &Path, p: Option<&Path>) -> Value {
    match p {
        Some(p) => Value::String(p.strip_prefix(games).unwrap().to_string_lossy().replace('\\', "/")),
        None => Value::Null,
    }
}

fn discs(mut d: Vec<Value>) -> Value {
    if d.len() <= 1 {
        return Value::from(d.len());
    }
    d.sort_by_key(|v| v.as_str().unwrap_or("").to_string());
    Value::from(d)
}

fn key(g: &Value) -> (String, String) {
    (g["title"].as_str().unwrap().to_string(), g["region"].as_str().unwrap().to_string())
}

/// A file of the fixture: a size (zero-filled), or `{ size, at: [[offset, base64]] }`.
fn write(p: &Path, spec: &Value) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    let mut b = vec![0u8; spec.as_u64().or_else(|| spec["size"].as_u64()).unwrap() as usize];
    for patch in spec["at"].as_array().into_iter().flatten() {
        let at = patch[0].as_u64().unwrap() as usize;
        let data = base64::engine::general_purpose::STANDARD.decode(patch[1].as_str().unwrap()).unwrap();
        b[at..at + data.len()].copy_from_slice(&data);
    }
    fs::write(p, b).unwrap();
}

#[test]
fn scans_match_the_js_scanner() {
    let fx = fixture("scan.json");
    for case in fx["trees"].as_array().unwrap() {
        let id = case["console"].as_str().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let games = tmp.path().join("games");
        for f in case["files"].as_array().unwrap() {
            write(&games.join(f[0].as_str().unwrap()), &f[1]);
        }
        for f in case["art"].as_array().unwrap() {
            write(&games.join(f.as_str().unwrap()), &Value::from(0));
        }
        let data_dir = tmp.path().join("data");
        fs::create_dir_all(&data_dir).unwrap();
        for (name, j) in fx["data"].as_object().unwrap() {
            fs::write(data_dir.join(name), j.to_string()).unwrap();
        }
        let sizes: serde_json::Map<String, Value> = fx["dirSizes"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(rel, bytes)| (resolve(&games.join(rel)).to_string_lossy().into_owned(), serde_json::json!({ "bytes": bytes, "mtime": 0, "at": 0 })))
            .collect();
        fs::write(data_dir.join("dir-sizes.json"), Value::Object(sizes).to_string()).unwrap();

        let console = systems().console(id).unwrap();
        let sc = SystemConfig { rom_dirs: vec![games.to_string_lossy().into_owned()], ..Config::default().system(console) };
        let dirs = ArtLibrary::default().dirs_for(&sc, tmp.path());
        let list = scan(console, &sc, &dirs, &ScanData::load(&data_dir));
        assert!(list.windows(2).all(|w| collate::compare(&w[0].title, &w[1].title) != Ordering::Greater), "{id}: not in title order");

        // the fixtures are made on Linux, where folders are walked in name order. On Windows it's the order the file system
        // keeps, which decides equal titles' order, disc order and which dump a single-disc game's `discs` names: there
        // those are compared order-free
        let mut got: Vec<Value> = list
            .iter()
            .map(|g| {
                let a = &g.art;
                serde_json::json!({
                    "title": g.title, "region": g.region, "disc": g.disc, "file": rel(&games, Some(&g.file)), "ext": g.ext,
                    "sizeBytes": g.size_bytes,
                    "discs": g.discs.iter().map(|d| rel(&games, Some(d))).collect::<Vec<_>>(),
                    "titleId": g.title_id, "notInstalled": g.not_installed, "compat": g.compat,
                    "art": {
                        "front": rel(&games, a.front.as_deref()), "back": rel(&games, a.back.as_deref()), "madeBack": a.made_back,
                        "spine": rel(&games, a.spine.as_deref()), "madeSpine": a.made_spine, "disc": rel(&games, a.disc.as_deref()),
                        "manual": a.manual.iter().map(|m| rel(&games, Some(m))).collect::<Vec<_>>(), "manualPdf": rel(&games, a.manual_pdf.as_deref()),
                        "hero": rel(&games, g.hero.as_deref()),
                    },
                })
            })
            .collect();
        let mut want: Vec<Value> = case["games"].as_array().unwrap().clone();
        if cfg!(windows) {
            for g in got.iter_mut().chain(want.iter_mut()) {
                g["discs"] = discs(g["discs"].as_array().unwrap().clone());
            }
            got.sort_by_key(key);
            want.sort_by_key(key);
        }
        assert_eq!(serde_json::to_string_pretty(&got).unwrap(), serde_json::to_string_pretty(&want).unwrap(), "{id}");
    }
}

#[test]
fn steam_matches_the_js_scan() {
    let fx = fixture("steam.json");
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let slashed = root.to_string_lossy().replace('\\', "/");
    for f in fx["files"].as_array().unwrap() {
        let p = root.join(f[0].as_str().unwrap());
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, f[1].as_str().unwrap().replace("{root}", &slashed)).unwrap();
    }
    let rel = |p: &Option<std::path::PathBuf>| match p {
        Some(p) => Value::String(p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/")),
        None => Value::Null,
    };
    for case in fx["cases"].as_array().unwrap() {
        let main = case["main"].as_str().unwrap();
        let main = (!main.is_empty()).then(|| root.join(main).to_string_lossy().into_owned());
        let games = rustyshelf_core::steam::scan(&root.join(case["given"].as_str().unwrap()).to_string_lossy(), main.as_deref(), case["hideTools"].as_bool().unwrap());
        let got: Vec<Value> = games
            .iter()
            .map(|g| {
                serde_json::json!({ "appid": g.appid, "title": g.title, "sizeBytes": g.size_bytes, "dir": rel(&g.dir), "minutes": g.minutes,
                    "lastPlayed": g.last_played, "front": rel(&g.front), "hero": rel(&g.hero) })
            })
            .collect();
        // a size that isn't a number is NaN in JS (null in the fixture), none here
        let want: Vec<Value> = case["games"]
            .as_array()
            .unwrap()
            .iter()
            .cloned()
            .map(|mut g| {
                if g["sizeBytes"].is_null() {
                    g["sizeBytes"] = Value::from(0);
                }
                g
            })
            .collect();
        assert_eq!(serde_json::to_string_pretty(&got).unwrap(), serde_json::to_string_pretty(&want).unwrap(), "{case}");
    }
}

// the PC games folder picks Linux programs on Linux, and the fixture was written there
#[cfg(target_os = "linux")]
#[test]
fn pc_games_match_the_js_lists() {
    use rustyshelf_core::pcgames::{self, PcGame};
    use std::os::unix::fs::PermissionsExt;
    let fx = fixture("pc.json");
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let r = root.to_string_lossy().into_owned();
    let fill = |t: &str| t.replace("{root}", &r);
    let at = |rel: &str| if rel.is_empty() { String::new() } else { format!("{r}/{rel}") };
    for f in fx["files"].as_array().unwrap() {
        let p = root.join(f[0].as_str().unwrap());
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        match &f[1] {
            Value::String(t) => fs::write(&p, fill(t)).unwrap(),
            Value::Number(n) => fs::write(&p, vec![0u8; n.as_u64().unwrap() as usize]).unwrap(),
            o => fs::write(&p, base64::engine::general_purpose::STANDARD.decode(o["b64"].as_str().unwrap()).unwrap()).unwrap(),
        }
    }
    for f in fx["exec"].as_array().unwrap() {
        fs::set_permissions(root.join(f.as_str().unwrap()), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let game = |g: &PcGame| {
        serde_json::json!({ "id": g.id, "source": g.source, "title": g.title, "exe": g.exe, "args": g.args, "uri": g.uri, "shellApp": g.shell_app, "dir": g.dir,
            "icon": g.icon, "tags": g.tags, "shelf": g.shelf, "native": g.native, "programs": g.programs })
    };
    let same = |what: &str, got: &[PcGame], want: &Value| {
        let got = serde_json::to_string_pretty(&got.iter().map(game).collect::<Vec<_>>()).unwrap().replace(&r, "{root}");
        assert_eq!(got, serde_json::to_string_pretty(want).unwrap(), "{what}");
    };
    let strs = |v: &Value| -> Vec<String> { v.as_array().unwrap().iter().map(|s| at(s.as_str().unwrap())).collect() };

    same("epic", &pcgames::epic_games(&root.join("epic/m")), &fx["epic"]);
    same("ea", &pcgames::ea_from_dirs(&strs(&fx["ea"]["dirs"])), &fx["ea"]["games"]);

    let tree = pcgames::parse_reg(&fill(fx["gog"]["text"].as_str().unwrap()));
    let got: Vec<Value> = tree.iter().map(|(k, v)| serde_json::json!([k, v.iter().collect::<std::collections::BTreeMap<_, _>>()])).collect();
    let want: Vec<Value> = fx["gog"]["tree"].as_array().unwrap().iter().map(|e| serde_json::json!([e[0], e[1].as_object().unwrap().iter().collect::<std::collections::BTreeMap<_, _>>()])).collect();
    assert_eq!(serde_json::to_string_pretty(&got).unwrap().replace(&r, "{root}"), serde_json::to_string_pretty(&want).unwrap(), "registry");
    same("gog", &pcgames::gog_from_tree(&tree), &fx["gog"]["games"]);
    same("hoyo", &pcgames::hoyo_from_tree(&pcgames::parse_reg(&fill(fx["hoyo"]["text"].as_str().unwrap()))), &fx["hoyo"]["games"]);

    let scanned = pcgames::xbox_scan(&[at("xbox"), at("nope")]);
    let got: Vec<Value> = scanned.iter().map(|f| serde_json::json!({ "name": f.name, "publisher": f.publisher, "appId": f.app_id, "title": f.title, "dir": f.dir })).collect();
    assert_eq!(serde_json::to_string_pretty(&got).unwrap().replace(&r, "{root}"), serde_json::to_string_pretty(&fx["xbox"]["scan"]).unwrap(), "xbox folders");
    for case in fx["xbox"]["cases"].as_array().unwrap() {
        let data = tempfile::tempdir().unwrap();
        let answer = case["answer"].as_str().unwrap().to_string();
        let found = pcgames::xbox_scan(&[at("xbox")]);
        same("xbox", &pcgames::xbox_games_with(data.path(), found.clone(), |_| answer, 1_000_000), &case["first"]);
        same("xbox again", &pcgames::xbox_games_with(data.path(), found, |_| String::new(), 1_000_001), &case["again"]);
    }

    // Quiver's usual places: none here, as when the fixture was written
    std::env::remove_var("APPDATA");
    std::env::remove_var("LOCALAPPDATA");
    for case in fx["quiver"].as_array().unwrap() {
        same("quiver", &pcgames::quiver_games(&at(case["chosen"].as_str().unwrap())), &case["games"]);
    }

    let list: Value = serde_json::from_str(&fill(&fx["custom"]["list"].to_string())).unwrap();
    same("custom", &pcgames::custom_games(Some(&list)), &fx["custom"]["games"]);

    let f = &fx["folder"];
    let games = rustyshelf_core::pcfolder::scan(&at(f["gamesDir"].as_str().unwrap()), &at(f["ownDir"].as_str().unwrap()), Some(&f["programs"]), &strs(&f["skip"]));
    same("folder", &games, &f["games"]);

    for p in fx["publishers"].as_array().unwrap() {
        assert_eq!(pcgames::publisher_id(p[0].as_str().unwrap()), p[1].as_str().unwrap());
    }
    for e in fx["exes"].as_array().unwrap() {
        assert_eq!(pcgames::title_from_exe(e[0].as_str().unwrap()), e[1].as_str().unwrap());
    }
}

#[test]
fn emulator_names_match_the_js_ones() {
    use rustyshelf_core::emuname;
    for case in fixture("emunames.json").as_array().unwrap() {
        let p = case["path"].as_str().unwrap();
        let (emu, build) = emuname::parts(p);
        let got = serde_json::json!({ "path": p, "label": emuname::label(p), "parts": { "emu": emu, "build": build }, "known": emuname::known_name(p) });
        assert_eq!(&got, case);
    }
}

// the defaults hold this platform's consoles, and the fixture was written on Linux
#[cfg(target_os = "linux")]
#[test]
fn settings_match_the_js_rules() {
    use rustyshelf_core::{js, settings};
    // the first lines that differ say more than two whole files
    let same = |what: &str, got: &str, want: &str| {
        let at = got.lines().zip(want.lines()).position(|(g, w)| g != w).unwrap_or(got.lines().count().min(want.lines().count()));
        let near = |t: &str| t.lines().skip(at.saturating_sub(3)).take(8).collect::<Vec<_>>().join("\n");
        assert!(got == want, "{what}, line {}:\n--- got\n{}\n--- want\n{}", at + 1, near(got), near(want));
    };
    for (i, case) in fixture("settings.json").as_array().unwrap().iter().enumerate() {
        let text = case["text"].as_str().unwrap();
        let loaded = settings::read(text).unwrap_or_else(settings::fallback);
        same(&format!("loading settings {i}"), &js::stringify_pretty(&loaded), case["loaded"].as_str().unwrap());
        if let Some(want) = case["normalized"].as_str() {
            let got = settings::normalize(&serde_json::from_str(text).unwrap()).unwrap();
            same(&format!("normalizing settings {i}"), &js::stringify_pretty(&got), want);
        }
    }
}

// the consoles' program names are this platform's, and the fixture was written on Linux
#[cfg(target_os = "linux")]
#[test]
fn setup_finds_what_the_js_finds() {
    use rustyshelf_core::detect::{Detector, Found};
    let fx = fixture("detect.json");
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().to_string_lossy().into_owned();
    for f in fx["files"].as_array().unwrap() {
        let (rel, text) = match f {
            Value::String(rel) => (rel.as_str(), ""),
            pair => (pair[0].as_str().unwrap(), pair[1].as_str().unwrap()),
        };
        let p = tmp.path().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, text.replace("{home}", &home)).unwrap();
    }
    // only the folders under the home and the app, as when the fixture was written
    let mut sys = systems().clone();
    let own = |d: &String| d.starts_with("{app}") || d.starts_with("%HOME%");
    sys.consoles.iter_mut().for_each(|c| c.emulator_folders.retain(own));
    sys.apps.iter_mut().for_each(|a| a.folders.retain(own));
    let det = Detector::with(&sys, &tmp.path().join("app"), |k| (k.eq_ignore_ascii_case("HOME")).then(|| home.clone()), Vec::new());

    let found = |f: &Found| serde_json::json!({ "emulator": f.emulator, "emulators": f.emulators, "romDirs": f.rom_dirs });
    let same = |what: &str, got: Value, want: &Value| assert_eq!(serde_json::to_string_pretty(&got).unwrap().replace(&home, "{home}"), serde_json::to_string_pretty(want).unwrap(), "{what}");
    let all = det.detect_all();
    let systems: serde_json::Map<String, Value> = all.systems.iter().map(|(id, f)| (id.clone(), found(f))).collect();
    let apps: serde_json::Map<String, Value> = all.apps.iter().map(|(id, p)| (id.clone(), serde_json::json!({ "path": p }))).collect();
    same("everything", serde_json::json!({ "systems": systems, "apps": apps, "retroarch": all.retroarch }), &fx["all"]);
    for case in fx["systems"].as_array().unwrap() {
        let id = case[0].as_str().unwrap();
        same(id, det.detect_system(id).map_or(Value::Null, |f| found(&f)), &case[1]);
    }
    for (dir, key) in [("Portable", "inFolder"), ("nope", "inNothing")] {
        let (hits, retroarch) = det.emulators_in(&format!("{home}/{dir}"));
        let hits: serde_json::Map<String, Value> = hits.into_iter().map(|(id, p)| (id, Value::from(p))).collect();
        same(key, serde_json::json!({ "systems": hits, "retroarch": retroarch }), &fx[key]);
    }
}

/// Every file under a folder: its text, or `{ b64 }` when it isn't UTF-8.
#[cfg(target_os = "linux")]
fn snapshot(root: &Path) -> std::collections::BTreeMap<String, Value> {
    fn walk(d: &Path, root: &Path, out: &mut std::collections::BTreeMap<String, Value>) {
        for e in fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                let b = fs::read(&p).unwrap();
                let v = match String::from_utf8(b) {
                    Ok(t) => Value::from(t),
                    Err(e) => serde_json::json!({ "b64": base64::engine::general_purpose::STANDARD.encode(e.as_bytes()) }),
                };
                out.insert(p.strip_prefix(root).unwrap().to_string_lossy().into_owned(), v);
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(root, root, &mut out);
    out
}

// the settings folders looked in are Linux's, and the fixture was written there
#[cfg(target_os = "linux")]
#[test]
fn emulator_settings_change_as_in_the_js() {
    use rustyshelf_core::emucfg;
    let fx = fixture("emucfg.json");
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().to_string_lossy().into_owned();
    for f in fx["files"].as_array().unwrap() {
        let p = tmp.path().join(f[0].as_str().unwrap());
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        match &f[1] {
            Value::String(t) => fs::write(&p, t).unwrap(),
            o => fs::write(&p, base64::engine::general_purpose::STANDARD.decode(o["b64"].as_str().unwrap()).unwrap()).unwrap(),
        }
    }
    let var = |k: &str| (k == "HOME").then(|| home.clone());
    let at = |rel: &Value| match rel.as_str().unwrap() {
        "" => String::new(),
        rel => format!("{home}/{rel}"),
    };
    let mut before = snapshot(tmp.path());
    for (i, case) in fx["steps"].as_array().unwrap().iter().enumerate() {
        let step = &case["step"];
        let result = match step[0].as_str().unwrap() {
            "lists" => serde_json::json!({ "pcsx2": emucfg::pcsx2_inis(&at(&step[1]), &var, true), "docs": emucfg::docs_dirs(&var), "dolphin": emucfg::dolphin_user_dirs(&at(&step[1]), &var) }),
            "quietEden" => Value::from(emucfg::quiet_eden(&at(&step[1]), &var)),
            "afterExit" => {
                emucfg::after_exit(step[1].as_str().unwrap(), &at(&step[2]), &var);
                Value::Null
            }
            "setIni" => {
                let want: Vec<(&str, &str)> = step[3].as_object().unwrap().iter().map(|(k, v)| (k.as_str(), v.as_str().unwrap())).collect();
                Value::from(emucfg::set_ini(&at(&step[1]), step[2].as_str().unwrap(), &want).unwrap())
            }
            "pcsx2NoConfirm" => Value::from(emucfg::pcsx2_no_confirm(&at(&step[1]), &var)),
            "rpcs3NoConfirm" => Value::from(emucfg::rpcs3_no_confirm(&at(&step[1]))),
            "dolphinQuitPads" => Value::from(emucfg::dolphin_quit_pads(&at(&step[1]), step[2].as_str().unwrap(), &var)),
            "coresDir" => Value::from(rustyshelf_core::racores::cores_dir(&at(&step[1]), &var)),
            "bios" => {
                use rustyshelf_core::racores::{bios, Need, State};
                let dirs: Vec<String> = step[2].as_array().unwrap().iter().map(&at).collect();
                bios(step[1].as_str().unwrap(), &dirs).map_or(Value::Null, |b| {
                    let files: Vec<Value> = b
                        .files
                        .iter()
                        .map(|f| {
                            let state = match f.state {
                                State::Ok => "ok",
                                State::Wrong => "wrong",
                                State::Missing => "missing",
                            };
                            serde_json::json!({ "name": f.name, "region": f.region, "state": state, "where": f.at })
                        })
                        .collect();
                    let need = match b.need {
                        Need::One => "one",
                        Need::Required => "req",
                        Need::Optional => "opt",
                    };
                    serde_json::json!({ "need": need, "note": b.note, "ok": b.ok, "files": files })
                })
            }
            "findIpl" => {
                let ipl = rustyshelf_core::dolphin::find_ipl(&at(&step[1]), &var);
                let found: Vec<Value> = ipl.found.iter().map(|(r, f)| serde_json::json!({ "region": r, "file": f })).collect();
                serde_json::json!({ "found": found, "userDir": ipl.user_dir, "usaFolder": ipl.usa_folder })
            }
            "ensureIpl" => Value::from(rustyshelf_core::dolphin::ensure_ipl_folder(&at(&step[1]), step[2].as_str().unwrap(), &var).unwrap()),
            what @ ("setupPlan" | "setupApply" | "setupUndo" | "setupStatus") => {
                use rustyshelf_core::emusetup::{kind_of, Options, Setup};
                let setup = Setup { data_dir: tmp.path().join("app/data"), app_root: tmp.path().join("app"), var: &var };
                let exe = at(&step[1]);
                let opts = |o: &Value| Options { graphics: o["graphics"].as_str().unwrap_or("").to_string(), bios_dir: String::new() };
                let pairs = |l: &[(&str, &str)]| serde_json::json!(l.iter().map(|(g, why)| serde_json::json!([g, why])).collect::<Vec<_>>());
                match what {
                    "setupPlan" => kind_of(&exe).map_or(Value::Null, |k| {
                        let p = setup.plan(k, &exe, &opts(&step[2]));
                        let changes: Vec<Value> = p.changes.iter().map(|c| serde_json::json!({ "group": c.group, "file": c.file, "cfg": c.cfg, "section": c.section, "key": c.key, "value": c.value })).collect();
                        serde_json::json!({ "ok": p.ok, "why": p.why, "changes": changes, "skipped": pairs(&p.skipped), "groups": p.groups })
                    }),
                    "setupApply" => {
                        let on: Vec<(&str, bool)> = step[2].as_object().unwrap().iter().map(|(k, v)| (k.as_str(), v.as_bool().unwrap())).collect();
                        let r = setup.apply(&exe, &on, &opts(&step[3]), 1_700_000_000_000);
                        serde_json::json!({ "ok": r.ok, "why": r.why, "applied": r.applied, "skipped": pairs(&r.skipped), "files": r.files })
                    }
                    "setupUndo" => Value::from(setup.undo(&exe)),
                    _ => setup.status(&exe, 1_700_000_000_000).map_or(Value::Null, |st| {
                        serde_json::json!({ "kind": st.kind.id(), "name": st.kind.name(), "ready": st.ready, "file": st.file, "used": st.used,
                            "done": st.done.map(|(at, groups)| serde_json::json!({ "at": at, "groups": groups })), "groups": st.groups, "skipped": pairs(&st.skipped), "biosDir": st.bios_dir })
                    }),
                }
            }
            "restoreBackups" => {
                let emus: Vec<(String, String)> = step[1].as_object().unwrap().iter().map(|(k, v)| (k.clone(), at(v))).collect();
                Value::from(emucfg::restore_backups(&emus, &var))
            }
            other => panic!("unknown step {other}"),
        };
        let now = snapshot(tmp.path());
        let mut changed = serde_json::Map::new();
        for k in before.keys().chain(now.keys()).collect::<std::collections::BTreeSet<_>>() {
            if before.get(k) != now.get(k) {
                changed.insert(k.clone(), now.get(k).cloned().unwrap_or(Value::Null));
            }
        }
        let got = serde_json::json!({ "result": result, "changed": changed.into_iter().collect::<std::collections::BTreeMap<_, _>>() });
        let want = serde_json::json!({ "result": case["result"], "changed": case["changed"].as_object().unwrap().iter().collect::<std::collections::BTreeMap<_, _>>() });
        // a key in emu-setup.json is the program in lower case
        let got = serde_json::to_string_pretty(&got).unwrap().replace(&home, "{home}").replace(&home.to_lowercase(), "{home}");
        assert_eq!(got, serde_json::to_string_pretty(&want).unwrap(), "step {i}: {step}");
        before = now;
    }
}

// Azahar's folders are looked for where Linux keeps them, and the fixture was written there
#[cfg(target_os = "linux")]
#[test]
fn cia_files_install_as_in_the_js() {
    let fx = fixture("n3ds.json");
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().to_string_lossy().into_owned();
    for d in fx["dirs"].as_array().unwrap() {
        fs::create_dir_all(tmp.path().join(d.as_str().unwrap())).unwrap();
    }
    for f in fx["files"].as_array().unwrap() {
        let p = tmp.path().join(f[0].as_str().unwrap());
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, f[1].as_str().unwrap().replace("{home}", &home)).unwrap();
    }
    fs::create_dir_all(tmp.path().join("roms")).unwrap();
    for c in fx["cias"].as_array().unwrap() {
        fs::write(tmp.path().join("roms").join(c[0].as_str().unwrap()), base64::engine::general_purpose::STANDARD.decode(c[1].as_str().unwrap()).unwrap()).unwrap();
    }
    let var = |k: &str| (k == "HOME").then(|| home.clone());
    let sizes = |root: &Path| -> std::collections::BTreeMap<String, Vec<u8>> {
        fn walk(d: &Path, root: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
            for e in fs::read_dir(d).unwrap().flatten() {
                match e.path() {
                    p if p.is_dir() => walk(&p, root, out),
                    p => drop(out.insert(p.strip_prefix(root).unwrap().to_string_lossy().into_owned(), fs::read(&p).unwrap())),
                }
            }
        }
        let mut out = std::collections::BTreeMap::new();
        walk(root, root, &mut out);
        out
    };
    let mut before = sizes(tmp.path());
    for (i, play) in fx["plays"].as_array().unwrap().iter().enumerate() {
        let mut status: Vec<String> = Vec::new();
        let file = tmp.path().join("roms").join(play["file"].as_str().unwrap());
        let result = match rustyshelf_core::n3ds::boot_file(&file, &format!("{home}/{}", play["exe"].as_str().unwrap()), &var, &mut |t| status.push(t.to_string())) {
            Ok(boot) => serde_json::json!({ "boot": boot.to_string_lossy().replace(&home, "{home}") }),
            Err(e) => serde_json::json!({ "error": e }),
        };
        let now = sizes(tmp.path());
        use sha1::Digest;
        let hex = |b: &[u8]| sha1::Sha1::digest(b).iter().map(|x| format!("{x:02x}")).collect::<String>();
        let changed: serde_json::Map<String, Value> = now.iter().filter(|(k, v)| before.get(*k) != Some(*v)).map(|(k, v)| (k.clone(), Value::from(hex(v)))).collect();
        assert_eq!((&result, serde_json::json!(status), &Value::Object(changed)), (&play["result"], play["status"].clone(), &play["changed"]), "play {i}");
        before = now;
    }
}

#[test]
fn arcade_names_match_the_js_list() {
    let fx = fixture("arcade.json");
    let (names, bios) = rustyshelf_core::arcade::parse_dat(fx["dat"].as_str().unwrap()).unwrap();
    let got: std::collections::BTreeMap<&str, &str> = names.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let want: std::collections::BTreeMap<&str, &str> = fx["names"].as_object().unwrap().iter().map(|(k, v)| (k.as_str(), v.as_str().unwrap())).collect();
    assert_eq!(got, want);
    assert_eq!(serde_json::json!(bios), fx["bios"]);
    assert!(rustyshelf_core::arcade::parse_dat("<game name=\"a\">\n<description>A</description>").is_none());
}

#[test]
fn box_art_matches_as_in_the_js() {
    use rustyshelf_core::covers::*;
    use rustyshelf_core::lbindex::{norm, Game, GameImage};
    let fx = fixture("art.json");
    let text = |v: &Value| v.as_str().unwrap().to_string();
    for c in fx["norms"].as_array().unwrap() {
        assert_eq!(norm(c[0].as_str().unwrap()), c[1].as_str().unwrap(), "norm {}", c[0]);
    }
    for c in fx["lookups"].as_array().unwrap() {
        assert_eq!(serde_json::json!(lookup_names(c[0].as_str().unwrap())), c[1], "lookup names {}", c[0]);
    }

    let entries: Vec<(String, usize)> = fx["list"].as_array().unwrap().iter().map(|e| (text(&e[0]), e[1].as_u64().unwrap() as usize)).collect();
    let normed: Vec<String> = entries.iter().map(|e| norm(&e.0)).collect();
    let names: Vec<&str> = normed.iter().map(String::as_str).collect();
    let games: Vec<Game> = entries.iter().enumerate().map(|(i, e)| serde_json::from_value(serde_json::json!({ "id": (100 + i).to_string(), "name": e.0, "n": normed[i] })).unwrap()).collect();
    let image = |kind: &str, region: &str, file: &str| GameImage { r#type: kind.into(), region: region.into(), file: file.into() };
    let images: std::collections::HashMap<String, Vec<GameImage>> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| ((100 + i).to_string(), (0..e.1).map(|k| image(if k % 2 == 1 { "Disc" } else { "Box - Front" }, "", "x.jpg")).chain([image("Clear Logo", "", "l.png")]).collect()))
        .collect();
    let art = |i: usize| entries[i].1;
    for c in fx["titles"].as_array().unwrap() {
        let t = c["title"].as_str().unwrap();
        let at = |i: Option<usize>| i.map_or(Value::Null, Value::from);
        let lb = launchbox_match(&games, &images, t).map(|g| games.iter().position(|x| x.id == g.id).unwrap());
        let got = serde_json::json!({ "title": t, "loose": at(match_title(&names, t, &art, false)), "exact": at(match_title(&names, t, &art, true)),
            "plain": at(match_title(&names, t, &|_| 1, false)), "name": at(match_name(&names, t, &art)), "launchbox": at(lb) });
        assert_eq!(&got, c, "matching {t}");
    }

    let imgs: Vec<GameImage> = fx["images"].as_array().unwrap().iter().map(|i| image(i[0].as_str().unwrap(), i[1].as_str().unwrap(), i[2].as_str().unwrap())).collect();
    for c in fx["picks"].as_array().unwrap() {
        let files: Vec<&str> = pick_images(&imgs, c["type"].as_str().unwrap(), c["region"].as_str().unwrap()).iter().map(|i| i.file.as_str()).collect();
        assert_eq!(serde_json::json!(files), c["files"], "picking {}", c["type"]);
    }

    let lr = LibretroNames::new(fx["libretro"].as_array().unwrap().iter().map(text).collect());
    for c in fx["games"].as_array().unwrap() {
        let (file, title, region) = (Path::new(c["file"].as_str().unwrap()), c["title"].as_str().unwrap(), c["region"].as_str().unwrap());
        assert_eq!(serde_json::json!(lr.all(file, title)), c["all"], "every cover of {title}");
        for want in c["found"].as_array().unwrap() {
            assert_eq!(serde_json::json!(lr.find(file, title, region, want[0].as_str().unwrap())), want[1], "the cover of {} for {}", c["file"], want[0]);
        }
    }

    for c in fx["editions"].as_array().unwrap() {
        let (kind, word) = edition(c[0].as_str().unwrap());
        assert_eq!(serde_json::json!({ "edition": kind, "name": word }), c[1]);
    }
    for (key, of) in [("lbRegions", lb_region as fn(&str) -> &'static str), ("lrNames", lr_region), ("tdb", tdb_region)] {
        for c in fx[key].as_array().unwrap() {
            assert_eq!(of(c[0].as_str().unwrap()), c[1].as_str().unwrap(), "{key} {}", c[0]);
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    for d in fx["discs"].as_array().unwrap() {
        let mut b = vec![0u8; 0x8010];
        for patch in d["at"].as_array().unwrap() {
            let (o, data) = (patch[0].as_u64().unwrap() as usize, base64::engine::general_purpose::STANDARD.decode(patch[1].as_str().unwrap()).unwrap());
            b[o..o + data.len()].copy_from_slice(&data);
        }
        let f = tmp.path().join(d["name"].as_str().unwrap());
        fs::write(&f, if d["short"].as_bool().unwrap() { &b[..4] } else { &b[..] }).unwrap();
        assert_eq!(serde_json::json!(disc_id(&f)), d["id"], "disc id of {}", d["name"]);
    }

    let listed: Vec<String> = fx["replaced"]["names"].as_array().unwrap().iter().map(text).collect();
    for c in fx["replaced"]["asks"].as_array().unwrap() {
        let found = replaced_for(&listed, c["base"].as_str().unwrap(), c["suffix"].as_str().unwrap());
        assert_eq!(serde_json::json!(found), c["found"], "replaced pictures of {}{}", c["base"], c["suffix"]);
    }
}

// a picture's type in capitals is another file only where names keep their case, as where the fixture was written
#[cfg(target_os = "linux")]
#[test]
fn pictures_go_in_place_as_in_the_js() {
    use rustyshelf_core::boxart::{place_art, size_of};
    let fx = fixture("place.json");
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    for f in fx["files"].as_array().unwrap() {
        let p = dir.join(f[0].as_str().unwrap());
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, f[1].as_str().unwrap()).unwrap();
    }
    // the stamps in replaced\ are the time
    let stamp = regex::Regex::new(r"\((?:[013-9][0-9]{3}|2[1-9][0-9]{2}|20[03-9][0-9]|202[1-9])(?:-[0-9]+){6}(-[0-9]+)?\)").unwrap();
    let rel = |f: &Path| stamp.replace_all(&f.strip_prefix(dir).unwrap().to_string_lossy(), "(STAMP$1)").into_owned();
    let snapshot = || {
        fn walk(d: &Path, out: &mut Vec<std::path::PathBuf>) {
            for e in fs::read_dir(d).unwrap().flatten() {
                match e.path() {
                    p if p.is_dir() => walk(&p, out),
                    p => out.push(p),
                }
            }
        }
        let mut files = Vec::new();
        walk(dir, &mut files);
        files.iter().map(|f| (rel(f), String::from_utf8_lossy(&fs::read(f).unwrap()).into_owned())).collect::<std::collections::BTreeMap<_, _>>()
    };
    for case in fx["steps"].as_array().unwrap() {
        let s = &case["step"];
        let result = match place_art(dir, s[0].as_str().unwrap(), s[1].as_str().unwrap(), &dir.join(s[2].as_str().unwrap()), s[3].as_str().unwrap()) {
            Ok((dest, moved)) => serde_json::json!({ "dest": rel(&dest), "moved": moved.iter().map(|m| rel(m)).collect::<Vec<_>>() }),
            Err(_) => serde_json::json!({ "error": true }),
        };
        assert_eq!((&result, serde_json::json!(snapshot())), (&case["result"], case["files"].clone()), "placing {s}");
    }
    for s in fx["sizes"].as_array().unwrap() {
        let f = dir.join(s[0].as_str().unwrap());
        fs::write(&f, base64::engine::general_purpose::STANDARD.decode(s[1].as_str().unwrap()).unwrap()).unwrap();
        assert_eq!(serde_json::json!(size_of(&f).map(|z| [z.0, z.1])), s[2], "the size of {}", s[0]);
    }
}

#[test]
fn songs_match_the_js() {
    use base64::Engine;
    use rustyshelf_core::cues::{built_in_songs, parse_midi, Song};
    let fx = fixture("songs.json");
    let plain = |s: &Song| serde_json::json!({ "name": s.name, "hue": s.hue, "notes": s.notes, "chords": s.chords, "bars": s.bars, "builtIn": s.built_in });
    let ours: Vec<Value> = built_in_songs().iter().map(plain).collect();
    assert_eq!(Value::from(ours), fx["builtIn"]);
    let mut read = 0;
    for m in fx["midi"].as_array().unwrap() {
        let bytes = base64::engine::general_purpose::STANDARD.decode(m["bytes"].as_str().unwrap()).unwrap();
        let name = m["name"].as_str().unwrap();
        let got = parse_midi(&bytes, name).ok().map_or(Value::Null, |s| plain(&s));
        assert_eq!(got, m["song"], "{name}");
        read += usize::from(!got.is_null());
    }
    assert!(read >= 3, "only {read} of the fixture's files are songs");
}

#[test]
fn window_mode_matches_the_js() {
    use rustyshelf_core::windowmode::{mode_of, ps_script};
    let fx = fixture("windowmode.json");
    for s in fx["scripts"].as_array().unwrap() {
        let got = ps_script(s["pid"].as_u64().unwrap() as u32, s["dir"].as_str().unwrap(), s["seconds"].as_u64().unwrap() as u32);
        assert_eq!(got, s["script"].as_str().unwrap());
    }
    let cfg = Config::from_value(&serde_json::json!({ "games": fx["games"] })).unwrap();
    for m in fx["modes"].as_array().unwrap() {
        assert_eq!(mode_of(&cfg, m[0].as_str().unwrap()), m[1].as_str().unwrap(), "{}", m[0]);
    }
}

#[test]
fn game_info_matches_the_js_lookup() {
    use rustyshelf_core::info::InfoCache;
    let fx = fixture("info.json");
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("launchbox-index.json"), fx["index"].to_string()).unwrap();
    let mut cache = InfoCache::load(tmp.path());
    for a in fx["asks"].as_array().unwrap() {
        let got = cache.get(a[0].as_str().unwrap(), a[1].as_str().unwrap()).map_or(Value::Null, |i| serde_json::to_value(i).unwrap());
        // a field that's null is as good as not there, and 3.0 is 3
        let plain = |v: &Value| -> Option<std::collections::BTreeMap<String, String>> {
            let fields = v.as_object()?.iter().filter(|(k, x)| !x.is_null() || *k == "bg" || *k == "logo");
            Some(fields.map(|(k, x)| (k.clone(), x.as_f64().map_or(x.to_string(), |n| n.to_string()))).collect())
        };
        assert_eq!(plain(&got), plain(&a[2]), "the info of {} on {}", a[1], a[0]);
    }
    // what was found is kept, and read back without the index
    cache.flush();
    fs::remove_file(tmp.path().join("launchbox-shards.json")).unwrap();
    let mut again = InfoCache::load(tmp.path());
    assert_eq!(again.get("gc", "Zeta Run").and_then(|i| i.dev), Some("Made Up".to_string()));
    assert_eq!(again.get("gc", "Nothing Here"), None);
}

#[test]
fn themes_match_the_js() {
    use rustyshelf_core::themes;
    let fx = fixture("themes.json");
    let ours: Vec<Value> = themes::themes().iter().map(|t| serde_json::json!({ "id": t.id, "name": t.name })).collect();
    assert_eq!(Value::from(ours), fx["themes"]);
    for (old, now) in fx["alias"].as_object().unwrap() {
        assert_eq!(themes::id_of(old), now.as_str().unwrap());
    }
    assert_eq!(themes::id_of("no such theme"), "spectrum");
    for t in fx["tints"].as_array().unwrap() {
        let (id, sh, hue, song) = (t["id"].as_str().unwrap(), t["sh"].as_f64().unwrap(), t["hue"].as_f64().unwrap(), t["song"].as_f64());
        assert_eq!(themes::tint(id, sh, hue, song), t["css"].as_str().unwrap(), "{id} at {sh}");
        assert_eq!(themes::halo(id, sh, song), t["halo"].as_f64().unwrap(), "{id} at {sh}");
    }
}

#[test]
fn find_matches_the_js() {
    use rustyshelf_core::find::{fold, search, Entry};
    let fx = fixture("find.json");
    let games: Vec<Entry> = fx["games"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            assert_eq!(fold(g["title"].as_str().unwrap()), g["folded"].as_str().unwrap(), "{}", g["title"]);
            Entry::new(g["id"].as_str().unwrap(), g["title"].as_str().unwrap(), g["shelf"].as_str().unwrap(), g["last"].as_u64().unwrap())
        })
        .collect();
    for s in fx["searches"].as_array().unwrap() {
        let typed = s["typed"].as_str().unwrap();
        let (found, total) = search(&games, typed);
        let ids: Vec<Value> = found.iter().map(|&i| Value::from(games[i].id.clone())).collect();
        assert_eq!(Value::from(ids), s["found"], "{typed:?}");
        assert_eq!(total as u64, s["total"].as_u64().unwrap(), "{typed:?}");
    }
}

#[test]
fn custom_themes_match_the_js() {
    use rustyshelf_core::customthemes as ct;
    let fx = fixture("customthemes.json");
    for s in fx["slugs"].as_array().unwrap() {
        assert_eq!(ct::slug(s[0].as_str().unwrap()), s[1].as_str().unwrap(), "{}", s[0]);
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = ct::root(tmp.path());
    ct::ensure_examples(&root);
    for (file, text) in fx["examples"].as_object().unwrap() {
        assert_eq!(std::fs::read_to_string(root.join(file)).unwrap_or_default(), text.as_str().unwrap(), "{file}");
    }
    for (dir, files) in fx["folders"].as_object().unwrap() {
        std::fs::create_dir_all(root.join(dir)).unwrap();
        for (name, text) in files.as_object().unwrap() {
            std::fs::write(root.join(dir).join(name), text.as_str().unwrap()).unwrap();
        }
    }
    let ours: Vec<Value> = ct::list(&root, &["gc", "ps2", "home", "media"])
        .iter()
        .map(|t| {
            let file = |p: &std::path::Path| p.file_name().unwrap().to_string_lossy().into_owned();
            let shelves: serde_json::Map<String, Value> = t.shelves.iter().map(|(k, v)| (k.clone(), Value::from(file(v)))).collect();
            serde_json::json!({ "id": t.id, "name": t.name, "folder": t.folder, "bg": file(&t.bg), "shelves": shelves, "shade": t.shade, "blur": t.blur, "glow": t.glow, "accent": t.accent })
        })
        .collect();
    for (ours, theirs) in ours.iter().zip(fx["list"].as_array().unwrap()) {
        for key in ["id", "name", "folder", "bg", "shelves", "blur", "accent"] {
            assert_eq!(ours[key], theirs[key], "{key} of {}", theirs["folder"]);
        }
        assert_eq!((ours["shade"].as_f64(), ours["glow"].as_f64()), (theirs["shade"].as_f64(), theirs["glow"].as_f64()), "{}", theirs["folder"]);
    }
    assert_eq!(ours.len(), fx["list"].as_array().unwrap().len());
    let src = tmp.path().join("Sea_side-view.PNG");
    std::fs::write(&src, "x").unwrap();
    let made: Vec<Value> = [("", ""), ("  A: name? ", ""), ("", "")].iter().map(|(name, _)| Value::from(ct::create(&root, &src, name).unwrap())).collect();
    assert_eq!(Value::from(made), fx["made"]);
    for dir in fx["after"].as_array().unwrap() {
        assert!(root.join(dir.as_str().unwrap()).join("background.png").is_file(), "{dir}");
    }
    let first = fx["after"][0].as_str().unwrap();
    let (ours, theirs): (Value, Value) = (serde_json::from_str(&std::fs::read_to_string(root.join(first).join("theme.json")).unwrap()).unwrap(), serde_json::from_str(fx["json"].as_str().unwrap()).unwrap());
    assert_eq!(ours, theirs);
    assert!(ct::create(&root, &tmp.path().join("notes.txt"), "").is_err());
}

#[test]
fn retroachievements_match_the_js() {
    use rustyshelf_core::ra::{match_game, norm, Ra};
    use std::sync::{Arc, Mutex};
    let fx = fixture("ra.json");
    for n in fx["norms"].as_array().unwrap() {
        assert_eq!(norm(n[0].as_str().unwrap()), n[1].as_str().unwrap(), "{}", n[0]);
    }
    let tmp = tempfile::tempdir().unwrap();
    let (site, asked) = (fx["site"].clone(), Arc::new(Mutex::new(Vec::<String>::new())));
    let heard = asked.clone();
    let ra = Ra::with(
        tmp.path(),
        Box::new(move |url: &str| {
            heard.lock().unwrap().push(url.to_string());
            let name = url.split("/API/").nth(1).and_then(|r| r.split(".php").next()).unwrap_or("");
            site.get(name).cloned().ok_or_else(|| format!("no {name}"))
        }),
    );
    assert!(!ra.enabled() && ra.game_list("gc").unwrap().is_empty());
    ra.set_credentials(" Made Up ", "k e&y/1");
    let list = ra.game_list("gc").unwrap();
    for m in fx["matches"].as_array().unwrap() {
        let title = m[0].as_str().unwrap();
        assert_eq!(match_game(&list, title).map(|g| g.id), m[1].as_u64(), "{title}");
    }
    let or_null = |v: Option<Value>| v.unwrap_or(Value::Null);
    assert_eq!(or_null(ra.progress("gc", "Zeta Run", false).unwrap()), fx["progress"]);
    assert_eq!(or_null(ra.progress("gc", "Nothing Like It", false).unwrap()), fx["none"]);
    assert_eq!(or_null(ra.progress("sw", "Zeta Run", false).unwrap()), fx["other"]);
    let titles: Vec<String> = fx["matches"].as_array().unwrap().iter().map(|m| m[0].as_str().unwrap().to_string()).collect();
    assert_eq!(ra.shelf_progress("gc", &titles).unwrap(), fx["shelf"]);
    assert_eq!(or_null(ra.profile().unwrap()), fx["profile"]);
    assert_eq!(Value::from(ra.recent(12.2).unwrap()), fx["recent"]);
    assert_eq!(ra.test("Other One", "x").unwrap(), fx["test"]);
    assert_eq!(Value::from(asked.lock().unwrap().clone()), fx["asked"]);
    // asked again within the cache's time: nothing new goes to the site
    ra.progress("gc", "Zeta Run", false).unwrap();
    ra.profile().unwrap();
    assert_eq!(asked.lock().unwrap().len(), fx["asked"].as_array().unwrap().len());
    assert!(ra.test("", "x").is_err());
}

#[test]
fn emulators_retroachievements_standing_matches_the_js() {
    use rustyshelf_core::raemu;
    let fx = fixture("raemu.json");
    let ini: Value = raemu::parse_ini(fx["ini"][0].as_str().unwrap().trim_start_matches('\u{feff}')).into_iter().map(|(k, v)| (k, Value::from(v.into_iter().map(|(k, v)| (k, Value::from(v))).collect::<serde_json::Map<_, _>>()))).collect::<serde_json::Map<_, _>>().into();
    assert_eq!(ini, fx["ini"][1]);
    let cfg: Value = raemu::parse_cfg(fx["cfg"][0].as_str().unwrap()).into_iter().map(|(k, v)| (k, Value::from(v))).collect::<serde_json::Map<_, _>>().into();
    assert_eq!(cfg, fx["cfg"][1]);
    for k in fx["kinds"].as_array().unwrap() {
        assert_eq!(raemu::kind_of(k[0].as_str().unwrap()), k[1].as_str(), "{}", k[0]);
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_string_lossy().into_owned();
    for (rel, text) in fx["files"].as_object().unwrap() {
        let file = tmp.path().join(rel);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text.as_str().unwrap()).unwrap();
    }
    let home = format!("{root}/home");
    let var = |name: &str| match name {
        "USERPROFILE" | "HOME" => Some(home.clone()),
        "LOCALAPPDATA" => Some(format!("{home}/AppData/Local")),
        "APPDATA" => Some(format!("{home}/AppData/Roaming")),
        _ => None,
    };
    for c in fx["checks"].as_array().unwrap() {
        let cfg: Value = serde_json::from_str(&c["cfg"].to_string().replace("<tmp>", &root)).unwrap();
        let rows: Vec<Value> = raemu::check(&cfg, &var).iter().map(|r| serde_json::json!({ "system": r.system, "emulator": r.emulator, "state": r.state, "user": r.user, "detail": r.detail })).collect();
        assert_eq!(Value::from(rows), c["rows"]);
    }
}

#[test]
fn gametdb_matches_the_js() {
    use base64::Engine;
    use rustyshelf_core::gametdb;
    let fx = fixture("gametdb.json");
    assert_eq!(fx["read"], true);
    let xml = fx["xml"].as_str().unwrap();
    for zip in ["stored", "deflated"] {
        let bytes = base64::engine::general_purpose::STANDARD.decode(fx[zip].as_str().unwrap()).unwrap();
        assert_eq!(gametdb::xml_of(&bytes).as_deref(), Some(xml), "{zip}");
    }
    assert_eq!(gametdb::xml_of(b"not a zip"), None);
    let mut maps = std::collections::HashMap::new();
    for (console, types) in [("sw", &["Switch"][..]), ("n3ds", &["3DS", "New3DS"][..])] {
        let (map, n) = gametdb::parse(xml, types);
        assert_eq!(Value::Object(map.clone()), fx["parsed"][console]["map"], "{console}");
        assert_eq!(n as u64, fx["parsed"][console]["n"].as_u64().unwrap());
        maps.insert(console, map);
    }
    for c in fx["covers"].as_array().unwrap() {
        let console = c["console"].as_str().unwrap();
        let names: Vec<String> = c["names"].as_array().unwrap().iter().map(|n| n.as_str().unwrap().to_string()).collect();
        let prefer: Vec<&str> = c["prefer"].as_array().unwrap().iter().map(|p| p.as_str().unwrap()).collect();
        let found: Vec<Value> = gametdb::full_covers(maps.get(console).unwrap_or(&serde_json::Map::new()), console, &names, &prefer).iter().map(|f| serde_json::json!({ "url": f.url, "id": f.id, "region": f.region, "kind": f.kind })).collect();
        assert_eq!(Value::from(found), c["found"], "{names:?}");
    }
    assert!(gametdb::has("sw") && gametdb::has("n3ds") && !gametdb::has("sw2"));
}

#[test]
fn steamgriddb_lookups_match_the_js() {
    use rustyshelf_core::steamgrid::{Game, Reply, SteamGrid};
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    let fx = fixture("steamgrid.json");
    let asked = Arc::new(Mutex::new(Vec::<String>::new()));
    let start = |dir: &Path| {
        asked.lock().unwrap().clear();
        let (site, heard) = (fx["site"].clone(), asked.clone());
        SteamGrid::with(
            dir,
            Box::new(move |url: &str, key: &str| {
                let path = url.strip_prefix("https://www.steamgriddb.com/api/v2").unwrap().to_string();
                let at = site.get(&path);
                heard.lock().unwrap().push(path);
                let status = match key {
                    "offline-key" => return Err(false),
                    "slow-key" => return Err(true),
                    "bad-key" => 401,
                    "k500" => 500,
                    _ => at.map_or(404, |r| r["status"].as_u64().unwrap() as u16),
                };
                let body = if key == "junk-key" { b"<html>".to_vec() } else { at.map_or(Vec::new(), |r| r["body"].to_string().into_bytes()) };
                Ok(Reply { status, retry_after: at.and_then(|r| r["retry"].as_str()).map(str::to_string), body })
            }),
        )
    };
    let names: Vec<Vec<String>> = fx["games"].as_array().unwrap().iter().map(|g| g["names"].as_array().map_or(Vec::new(), |n| n.iter().map(|s| s.as_str().unwrap().to_string()).collect())).collect();
    let game = |i: usize| Game { appid: fx["games"][i]["appid"].as_str().unwrap_or(""), title: fx["games"][i]["title"].as_str().unwrap(), names: &names[i] };
    let posters = |sg: &SteamGrid, i: usize| Value::from(sg.posters(&game(i)).iter().map(|p| p.to_json()).collect::<Vec<_>>());

    let tmp = tempfile::tempdir().unwrap();
    let sg = start(tmp.path());
    assert!(!sg.enabled());
    assert_eq!(posters(&sg, 0), fx["nokey"]);
    sg.set_key(" made-up-key ");
    for i in 0..names.len() {
        assert_eq!(posters(&sg, i), fx["posters"][i], "{}", fx["games"][i]["title"]);
    }
    let ids: Vec<u64> = [0, 4, 1].iter().map(|i| sg.game_id(&game(*i)).unwrap()).collect();
    assert_eq!(Value::from(ids), fx["ids"]);
    let logos: Vec<Value> = [1, 0, 3].iter().map(|i| sg.logos(&game(*i)).iter().map(|l| json!({ "url": l.url, "ext": ".png", "w": l.w, "h": l.h })).collect()).collect();
    assert_eq!(Value::from(logos), fx["logos"]);
    for t in fx["tests"].as_array().unwrap() {
        let said = match sg.test(t[0].as_str().unwrap()) {
            Ok(()) => json!({ "ok": true }),
            Err(why) => json!({ "ok": false, "error": why }),
        };
        assert_eq!(said, t[1], "{}", t[0]);
    }
    assert_eq!(Value::from(asked.lock().unwrap().clone()), fx["asked"]);
    let kept: Value = serde_json::from_slice(&std::fs::read(tmp.path().join("sgdb-cache.json")).unwrap()).unwrap();
    let column = |m: &Value, n: usize| Value::from(m.as_object().unwrap().iter().map(|(k, v)| (k.clone(), v[n].clone())).collect::<serde_json::Map<_, _>>());
    assert_eq!(column(&kept["ids"], 0), fx["cache"]["ids"]);
    assert_eq!(column(&kept["lists"], 1), fx["cache"]["lists"]);

    // what was kept answers after a new start: nothing is asked again
    let again = start(tmp.path());
    again.set_key("made-up-key");
    assert_eq!(posters(&again, 1), fx["posters"][1]);
    assert_eq!(again.game_id(&game(2)).unwrap(), 10);
    assert!(asked.lock().unwrap().is_empty());

    let tmp = tempfile::tempdir().unwrap();
    let sg = start(tmp.path());
    sg.set_key("made-up-key");
    let slow = Game { title: "Slow Game", ..Game::default() };
    let said = json!([sg.posters(&slow).len(), sg.posters(&game(1)).len(), sg.test("another-key").unwrap_err()]);
    assert_eq!(said, json!([fx["paused"][0].as_array().unwrap().len(), fx["paused"][1].as_array().unwrap().len(), fx["paused"][2]["error"]]));
    assert_eq!(Value::from(asked.lock().unwrap().clone()), fx["pausedAsked"]);

    let tmp = tempfile::tempdir().unwrap();
    let sg = start(tmp.path());
    sg.set_key("bad-key");
    assert!(sg.posters(&game(1)).is_empty() && !sg.enabled() && sg.posters(&game(0)).is_empty());
    assert_eq!(fx["refused"], json!([[], false, []]));
    assert_eq!(Value::from(asked.lock().unwrap().clone()), fx["refusedAsked"]);
}
