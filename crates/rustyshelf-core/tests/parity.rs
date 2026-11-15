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
    // moved from Quiver 3 to Quiver 4: Quiver 4's name wins for an app only it knows
    {
        let moved = &fx["quiverMoved"];
        let t = tmp.path().to_string_lossy().into_owned();
        for (f, c) in moved["files"].as_object().unwrap() {
            let p = tmp.path().join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, c.as_str().unwrap().replace("{tmp}", &t)).unwrap();
            if f.ends_with(".exe") {
                fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let roots: Vec<String> = moved["roots"].as_array().unwrap().iter().map(|r| r.as_str().unwrap().replace("{tmp}", &t)).collect();
        let got = serde_json::to_string_pretty(&pcgames::quiver_games_in(roots).iter().map(game).collect::<Vec<_>>()).unwrap().replace(&t, "{tmp}");
        assert_eq!(got, serde_json::to_string_pretty(&moved["games"]).unwrap(), "quiver moved");
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
    assert_eq!(or_null(ra.trophies(false).unwrap()), fx["trophies"]);
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

#[test]
fn screenscraper_lookups_match_the_js() {
    use rustyshelf_core::screenscraper::{clean, is_media, Creds, Got, Rom, ScreenScraper};
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    let fx = fixture("screenscraper.json");
    for c in fx["clean"].as_array().unwrap() {
        assert_eq!(clean(c[0].as_str().unwrap()).map_or(Value::Null, Value::from), c[1], "{}", c[0]);
    }
    for c in fx["isMedia"].as_array().unwrap() {
        assert_eq!(json!(is_media(c[0].as_str().unwrap())), c[1], "{}", c[0]);
    }
    let b64 = |v: &Value| base64::engine::general_purpose::STANDARD.decode(v.as_str().unwrap_or("")).unwrap();
    let (asked, mood) = (Arc::new(Mutex::new(Vec::<String>::new())), Arc::new(Mutex::new(String::new())));
    let start = |dir: &Path, now: &str| {
        asked.lock().unwrap().clear();
        *mood.lock().unwrap() = now.to_string();
        let (site, moods, heard, mood) = (fx["site"].clone(), fx["moods"].clone(), asked.clone(), mood.clone());
        ScreenScraper::with(
            dir,
            Box::new(move |url: &str, _| {
                heard.lock().unwrap().push(url.to_string());
                let u = url::Url::parse(url).unwrap();
                let q = |k: &str| u.query_pairs().find(|p| p.0 == k).map(|p| p.1.into_owned()).filter(|v| !v.is_empty());
                let what = q("gameid").map(|id| format!("id:{id}")).or_else(|| q("romnom")).or_else(|| q("recherche")).or_else(|| q("media")).unwrap_or_default();
                let mood = mood.lock().unwrap().clone();
                let r = if mood.is_empty() { site.get(format!("{}|{what}", u.path_segments().unwrap().next_back().unwrap())) } else { moods.get(&mood) };
                match r {
                    None => Ok((404, Vec::new())),
                    Some(Value::String(s)) => Err(s == "slow"),
                    Some(r) => Ok((r["status"].as_u64().unwrap() as u16, r["text"].as_str().map_or_else(|| base64::engine::general_purpose::STANDARD.decode(r["b64"].as_str().unwrap_or("")).unwrap(), |t| t.as_bytes().to_vec()))),
                }
            }),
        )
    };
    let creds = |c: &Value| {
        let of = |k: &str, usual: &str| c.get(k).and_then(Value::as_str).unwrap_or(usual).to_string();
        Creds { user: of("user", "made up user"), pass: of("pass", "p&ss word"), dev_id: of("devId", ""), dev_pass: of("devPass", "") }
    };
    let standing = |ss: &ScreenScraper| {
        let s = ss.standing();
        json!({ "configured": s.configured, "enabled": s.enabled, "paused": s.paused, "threads": s.threads, "devLogin": s.dev_login })
    };
    let tested = |ss: &ScreenScraper| match ss.test() {
        Ok(left) => json!({ "ok": true, "left": left }),
        Err(why) => json!({ "ok": false, "error": why }),
    };

    let tmp = tempfile::tempdir().unwrap();
    let here = tmp.path().to_string_lossy().into_owned();
    for (name, bytes) in fx["files"].as_object().unwrap() {
        fs::write(tmp.path().join(name), b64(bytes)).unwrap();
    }
    fs::create_dir(tmp.path().join("Phi Folder")).unwrap();
    let games: Vec<(String, String, String, u64)> = fx["games"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| (g[0].as_str().unwrap().into(), g[1]["file"].as_str().unwrap().replace("<tmp>", &here), g[1]["title"].as_str().unwrap().into(), g[1]["sizeBytes"].as_u64().unwrap_or(0)))
        .collect();
    let rom = |n: usize| Rom { file: &games[n].1, title: &games[n].2, size_bytes: games[n].3 };
    let offered = |ss: &ScreenScraper, n: usize, part: &str, can_cut: bool| Value::from(ss.candidates(&games[n].0, &rom(n), part, false, can_cut).iter().map(|c| c.to_json()).collect::<Vec<_>>());

    let data = tempfile::tempdir().unwrap();
    let ss = start(data.path(), "");
    assert_eq!(json!([offered(&ss, 0, "front", true), tested(&ss), standing(&ss)]), fx["noAccount"]);
    ss.set_creds(Creds { user: " made up user ".into(), pass: "p&ss word".into(), ..Creds::default() });
    for (n, want) in fx["lookups"].as_array().unwrap().iter().enumerate() {
        let got = match ss.lookup(&games[n].0, &rom(n), false) {
            Ok(Some(f)) => json!({ "id": f.id, "media": f.media.iter().map(|m| m.to_json()).collect::<Vec<_>>() }),
            Ok(None) => Value::Null,
            Err(t) => json!({ "error": t.code, "words": t.words }),
        };
        assert_eq!(&got, want, "{}", games[n].1);
    }
    for c in fx["candidates"].as_array().unwrap() {
        assert_eq!(offered(&ss, c[0].as_u64().unwrap() as usize, c[1].as_str().unwrap(), c[2].as_bool().unwrap()), c[3], "{c}");
    }
    assert!(ss.candidates("nes", &rom(0), "front", true, true).is_empty() && fx["unseen"] == json!([]));
    assert_eq!(tested(&ss), fx["test"]);
    assert_eq!(standing(&ss), fx["state"]);
    for (n, d) in fx["downloads"].as_array().unwrap().iter().enumerate() {
        let dest = tmp.path().join(format!("got{n}"));
        let how = match ss.download(d[0].as_str().unwrap(), &dest) {
            Ok(Got::Ok) => "ok".to_string(),
            Ok(Got::Missing) => "missing".into(),
            Ok(Got::Busy) => "busy".into(),
            Err(why) => format!("error: {why}"),
        };
        assert_eq!(json!(how), d[1], "{}", d[0]);
        assert_eq!(fs::read(&dest).ok(), d[2].as_str().map(|_| b64(&d[2])), "{}", d[0]);
    }
    assert_eq!(json!(ss.standing().paused), fx["paused"]);
    assert_eq!(Value::from(asked.lock().unwrap().clone()), fx["asked"]);
    let kept: Value = serde_json::from_str(&fs::read_to_string(data.path().join("ss-cache.json")).unwrap().replace(&here.to_lowercase(), "<tmp>")).unwrap();
    let found: serde_json::Map<String, Value> = kept["games"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), json!([v[0], v.get(2)]))).collect();
    assert_eq!(Value::from(found), fx["cache"]["games"]);
    assert_eq!(Value::from(kept["hashes"].as_object().unwrap().values().cloned().collect::<Vec<_>>()), fx["cache"]["hashes"]);

    for t in fx["tests"].as_array().unwrap() {
        let data = tempfile::tempdir().unwrap();
        let ss = start(data.path(), t["mood"].as_str().unwrap());
        ss.set_creds(creds(&t["creds"]));
        let said = json!({ "test": tested(&ss), "state": standing(&ss), "after": ss.candidates(&games[1].0, &rom(1), "front", false, true).len(), "asked": asked.lock().unwrap().clone() });
        for k in ["test", "state", "after", "asked"] {
            assert_eq!(said[k], t[k], "{} {} {k}", t["mood"], t["creds"]);
        }
    }
}

#[test]
fn home_menu_hotkeys_match_the_js() {
    use rustyshelf_core::homemenu::{emulator_kind, hotkeys_for};
    let fx = fixture("homekeys.json");
    for k in fx["kinds"].as_array().unwrap() {
        assert_eq!(emulator_kind(k[0].as_str().unwrap(), k[1].as_str().unwrap()), k[2].as_str().unwrap(), "{k}");
    }
    for k in fx["keys"].as_array().unwrap() {
        let got = hotkeys_for(k["system"].as_str().unwrap(), k["exe"].as_str().unwrap(), k["overrides"].as_object());
        let want = &k["keys"];
        let key = |name: &str| want.get(name).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
        assert_eq!((got.pause, got.save, got.load, got.shot), (key("pause"), key("save"), key("load"), key("shot")), "{k}");
        let slot = want.get("slot").map_or(0.0, |s| s.as_f64().unwrap_or_else(|| s.as_str().unwrap().parse().unwrap()));
        assert_eq!(got.slot as f64, slot, "{k}");
    }
}

#[test]
fn home_menu_behaves_as_the_page_does() {
    use rustyshelf_core::homemenu::{Glyphs, Info, Menu, Out};
    use serde_json::json;
    const START: f64 = 1e12;
    let fx = fixture("homemenu.json");
    for (n, run) in fx["runs"].as_array().unwrap().iter().enumerate() {
        let i = &run["info"];
        let text = |k: &str| i[k].as_str().unwrap_or("").to_string();
        let list = |k: &str| i[k].as_array().map_or(Vec::new(), |l| l.iter().map(|s| s.as_str().unwrap().to_string()).collect());
        let info = Info {
            in_game: i["inGame"] == true,
            title: text("title"),
            system: text("system"),
            subtitle: text("subtitle"),
            actions: list("actions"),
            power: list("power"),
            can_save: i["canSave"] == true,
            slot: i["slot"].as_u64().unwrap_or(0) as u32,
            paused: i["paused"] == true,
            center: i["layout"] == "center",
            glyphs: match i["glyphs"].as_str() {
                Some("ps") => Glyphs::Ps,
                Some("nintendo") => Glyphs::Nintendo,
                _ => Glyphs::Xbox,
            },
            swap_ab: i["confirmButton"] == "b",
        };
        // a game is said to have started `played` ago
        let (mut now, played) = (START, i["played"].as_f64().unwrap_or(0.0));
        let started = if played > 0.0 { now - played } else { 0.0 };
        let mut menu = Menu::open(info);
        let mut acts: Vec<String> = Vec::new();
        let take = |out: Option<Out>, acts: &mut Vec<String>| {
            if let Some(Out::Close(_, action)) = out {
                acts.push(action);
            }
        };
        let shown = |menu: &Menu, acts: &[String]| {
            if !acts.is_empty() {
                return json!({ "open": false, "acts": acts });
            }
            let rows = menu.rows();
            let (title, time) = menu.heading(started, START);
            json!({ "open": true, "acts": acts, "title": title, "sys": menu.info.system, "time": time, "subhead": menu.in_power().then_some("Power"),
                "seps": rows.iter().enumerate().filter(|r| r.1.sep_before).map(|r| r.0).collect::<Vec<_>>(),
                "items": rows.iter().map(|r| json!({ "label": r.label, "sub": r.sub, "sel": r.selected, "danger": r.danger })).collect::<Vec<_>>(),
                "foot": menu.foot().iter().map(|f| json!([f.0, f.1])).collect::<Vec<_>>() })
        };
        assert_eq!(shown(&menu, &acts), run["shown"][0], "run {n}, as opened");
        for (s, step) in run["steps"].as_array().unwrap().iter().enumerate() {
            if acts.is_empty() {
                match step[0].as_str().unwrap() {
                    "btn" => take(menu.press(step[1].as_str().unwrap(), step[2] == true, now), &mut acts),
                    "paused" => menu.info.paused = step[1] == true,
                    _ => {
                        // frames a sixtieth of a second apart for as long as the page waited
                        let until = now + step[1].as_f64().unwrap();
                        while now < until && acts.is_empty() {
                            now += 1000.0 / 60.0;
                            take(menu.frame(now), &mut acts);
                        }
                    }
                }
            }
            assert_eq!(shown(&menu, &acts), run["shown"][s + 1], "run {n}, step {s}: {step}");
        }
    }
}

#[test]
fn exports_and_imports_match_the_js() {
    use rustyshelf_core::dataexport::{export_all, export_size, import_all, Control, Copied, Install, Target};
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::atomic::Ordering;
    let fx = fixture("dataexport.json");
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_string_lossy().into_owned();
    let at = |rel: &str| tmp.path().join(rel);
    let here = |v: &Value| -> Value { serde_json::from_str(&v.to_string().replace("<tmp>", serde_json::to_string(&root).unwrap().trim_matches('"'))).unwrap() };
    let there = |text: String| text.replace(&root, "<tmp>");
    for (rel, body) in fx["tree"].as_object().unwrap() {
        let (file, body) = (at(rel), body.as_str().unwrap());
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, if rel.ends_with("config.json") { here(&serde_json::from_str(body).unwrap()).to_string() } else { body.to_string() }).unwrap();
    }
    for d in ["out", "out2"] {
        fs::create_dir(at(d)).unwrap();
    }
    fn walk(dir: &Path, rel: &str, out: &mut serde_json::Map<String, Value>, there: &dyn Fn(String) -> String) {
        let mut entries: Vec<_> = fs::read_dir(dir).unwrap().flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            let r = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            if e.path().is_dir() {
                walk(&e.path(), &r, out, there);
            } else if name != "manifest.json" {
                let r = if name.starts_with("config-before-import-") { r.replace(&name, "config-before-import.json") } else { r };
                out.insert(r, json!(there(fs::read_to_string(e.path()).unwrap())));
            }
        }
    }
    let listing = |dir: &Path| {
        let mut out = serde_json::Map::new();
        walk(dir, "", &mut out, &there);
        Value::Object(out)
    };
    let art_dirs = |_: &str, sc: &Value| -> Vec<PathBuf> {
        let own = sc["artDir"].as_str().filter(|d| !d.is_empty()).map(PathBuf::from);
        let beside = sc["romDirs"].as_array().into_iter().flatten().filter_map(Value::as_str).flat_map(|d| [Path::new(d).join("covers"), Path::new(d).join("media").join("covers"), Path::new(d).join("boxart")]);
        own.into_iter().chain(beside).collect()
    };
    // the settings name the made-up folder four times: their size is told as it would be under the fixture's
    let longer = 4 * (root.len() as i64 - fx["tmpLength"].as_i64().unwrap());
    let sized = |bytes: u64| if bytes > 100 { bytes as i64 - longer } else { bytes as i64 };
    let copied = |c: &Copied| json!({ "copied": c.copied, "copiedBytes": sized(c.copied_bytes), "unchanged": c.unchanged });
    let merged = |mut a: Value, b: Value| {
        a.as_object_mut().unwrap().extend(b.as_object().unwrap().clone());
        a
    };
    let cfg = here(&fx["cfg"]);
    let (a, a_data, lib) = (at("A"), at("A/data"), at("Lib"));
    let install = Install { app_root: &a, data_dir: &a_data, cfg: &cfg, art_dirs: &art_dirs, art_lib: Some(&lib) };
    let control = Control::default();

    let (bytes, files) = export_size(&install);
    assert_eq!(json!({ "bytes": sized(bytes), "files": files }), fx["size"]);
    let mut steps = Vec::new();
    let r = export_all(&install, &at("out"), "9.9.9", &control, &mut |i, of, label| steps.push(json!([i, of, label])), &mut |_| {}).unwrap();
    let exported = |r: &rustyshelf_core::dataexport::Exported| merged(json!({ "files": r.files, "parts": r.parts, "updated": r.updated }), copied(&r.copied));
    let manifest = |dir: &Path| {
        let mut m: Value = serde_json::from_str(&there(fs::read_to_string(dir.join("manifest.json")).unwrap())).unwrap();
        let o = m.as_object_mut().unwrap();
        assert!(o.remove("at").is_some_and(|at| at.as_str().is_some_and(|at| at.len() == 24 && at.ends_with('Z'))));
        let first = o.remove("first").is_some();
        o.insert("first".into(), json!(first));
        m
    };
    assert_eq!(exported(&r), fx["first"]["result"]);
    assert!(r.dir.file_name().unwrap().to_string_lossy().starts_with("RustyShelf export 20") && fx["first"]["named"] == true);
    assert_eq!(Value::from(steps), fx["first"]["steps"]);
    assert_eq!(manifest(&r.dir), fx["first"]["manifest"]);
    assert_eq!(listing(&r.dir), fx["first"]["holds"]);
    // the manifest is written as the Electron app writes it
    assert!(fs::read_to_string(r.dir.join("manifest.json")).unwrap().starts_with("{\n  \"app\": \"RustyShelf\",\n  \"version\": \"9.9.9\",\n  \"at\": \""));

    let again = export_all(&install, &at("out"), "9.9.9", &control, &mut |_, _, _| {}, &mut |_| {}).unwrap();
    assert_eq!((exported(&again), &again.dir), (fx["again"].clone(), &r.dir));
    fs::write(at("A/Library/Games/covers/Phi.png"), "new cover").unwrap();
    fs::write(at("A/themes/gc/logo.png"), "changed logo").unwrap();
    let mut last = None;
    let changed = export_all(&install, &r.dir, "9.9.9", &control, &mut |_, _, _| {}, &mut |p| last = Some(p.clone())).unwrap();
    assert_eq!(exported(&changed), fx["changed"]["result"]);
    assert_eq!(manifest(&r.dir), fx["changed"]["manifest"]);
    assert_eq!(listing(&r.dir), fx["changed"]["holds"]);
    let last = last.unwrap();
    assert_eq!((last.bytes, last.files, last.copied), (last.of, last.of_files, 2));

    let stopped = Control::default();
    stopped.cancelled.store(true, Ordering::Relaxed);
    assert_eq!(json!(export_all(&install, &at("out2"), "9.9.9", &stopped, &mut |_, _, _| {}, &mut |_| {}).unwrap_err()), fx["cancelled"]);
    assert_eq!(json!(export_all(&install, &at("nowhere"), "9.9.9", &control, &mut |_, _, _| {}, &mut |_| {}).unwrap_err()), fx["noFolder"]);

    for d in ["Games/gc/covers", "Games/gc/manuals", "Games/gc/boxart", "Games/snes/covers"] {
        fs::remove_dir_all(at(d)).unwrap();
    }
    let imported = |r: &rustyshelf_core::dataexport::Imported| merged(json!({ "parts": r.parts, "skipped": r.skipped.iter().map(|s| there(s.clone())).collect::<Vec<_>>() }), copied(&r.copied));
    let (b, b_data) = (at("B"), at("B/data"));
    let b_cfg: Value = serde_json::from_str(&fs::read_to_string(at("B/data/config.json")).unwrap()).unwrap();
    let im = import_all(&r.dir, &Target { app_root: &b, data_dir: &b_data, art_dirs: &art_dirs, art_lib: None, cfg: &b_cfg }, &control, &mut |_, _, _| {}, &mut |_| {}).unwrap();
    assert_eq!(imported(&im), fx["imported"]["result"]);
    assert_eq!(listing(&b), fx["imported"]["b"]);
    assert_eq!(listing(&at("Games")), fx["imported"]["games"]);

    fs::remove_dir_all(at("Games")).unwrap();
    let (c, c_data, c_lib) = (at("C"), at("C/data"), at("LibC"));
    let c_cfg: Value = serde_json::from_str(&fs::read_to_string(at("C/data/config.json")).unwrap()).unwrap();
    let target = Target { app_root: &c, data_dir: &c_data, art_dirs: &art_dirs, art_lib: Some(&c_lib), cfg: &c_cfg };
    let im = import_all(&r.dir, &target, &control, &mut |_, _, _| {}, &mut |_| {}).unwrap();
    assert_eq!(imported(&im), fx["elsewhere"]["result"]);
    assert_eq!(listing(&c), fx["elsewhere"]["c"]);
    assert_eq!(listing(&at("Other")), fx["elsewhere"]["other"]);
    assert_eq!(listing(&c_lib), fx["elsewhere"]["lib"]);
    assert_eq!(json!(import_all(&at("Other"), &target, &control, &mut |_, _, _| {}, &mut |_| {}).unwrap_err()), fx["notOne"]);
    fs::write(at("Other/manifest.json"), r#"{"app":"Something else"}"#).unwrap();
    assert_eq!(json!(import_all(&at("Other"), &target, &control, &mut |_, _, _| {}, &mut |_| {}).unwrap_err()), fx["notOurs"]);
}

#[test]
fn update_notice_matches_the_js() {
    use rustyshelf_core::updates::{compare, newest};
    use serde_json::json;
    let fx = fixture("updates.json");
    for c in fx["compares"].as_array().unwrap() {
        let sign = match compare(c[0].as_str().unwrap(), c[1].as_str().unwrap()) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        };
        assert_eq!(json!(sign), c[2], "{} against {}", c[0], c[1]);
    }
    for c in fx["checks"].as_array().unwrap() {
        let n = newest(&c["list"], c["current"].as_str().unwrap());
        let assets: Vec<Value> = n.assets.iter().map(|a| json!({ "name": a.name, "url": a.url, "size": a.size })).collect();
        assert_eq!(json!({ "latest": n.latest, "newer": n.newer, "url": n.url, "notes": n.notes, "assets": assets }), c["newest"], "{} for {}", c["list"], c["current"]);
    }
}

#[test]
fn adding_many_games_matches_the_js() {
    use rustyshelf_core::addgames::{clean, plan, scan_folder, title_for, Playnite};
    use serde_json::json;
    let fx = fixture("addgames.json");
    for c in fx["clean"].as_array().unwrap() {
        assert_eq!(json!(clean(c[0].as_str().unwrap())), c[1], "{}", c[0]);
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_string_lossy().into_owned();
    let here = |s: &str| s.replace("<tmp>", &root);
    let there = |p: &Path| p.to_string_lossy().replace(&root, "<tmp>");
    let games = tmp.path().join("Games");
    for (rel, bytes) in fx["tree"].as_object().unwrap() {
        let file = games.join(rel);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, vec![1u8; bytes.as_u64().unwrap() as usize]).unwrap();
    }
    for t in fx["titles"].as_array().unwrap() {
        let exe = here(t[0].as_str().unwrap());
        assert_eq!(json!([title_for(Path::new(&exe), Some(&games)), title_for(Path::new(&exe), None)]), json!([t[1], t[2]]), "{}", t[0]);
    }
    let scan = scan_folder(&games, 2000);
    let found: Vec<Value> = scan
        .games
        .iter()
        .map(|g| {
            let mut o = json!({ "folder": there(&g.folder), "title": g.title, "exe": there(&g.exe), "others": g.others.iter().map(|o| there(o)).collect::<Vec<_>>() });
            if let Some(group) = &g.group {
                o["group"] = json!(group);
            }
            if let Some(cover) = &g.cover {
                o["cover"] = json!(there(cover));
            }
            o
        })
        .collect();
    assert_eq!(Value::from(found), fx["scan"]["games"]);
    let skipped: Vec<Value> = scan.skipped.iter().map(|s| json!({ "folder": there(&s.0), "title": s.1, "why": s.2 })).collect();
    assert_eq!(Value::from(skipped), fx["scan"]["skipped"]);
    assert_eq!(json!(scan_folder(&games, 3).games.len()), fx["few"]);
    assert_eq!(json!(scan_folder(&tmp.path().join("nowhere"), 2000).error), fx["gone"]["error"]);

    let library = tmp.path().join("pn").join("library");
    for file in ["g1/cover.png", "g2/bg.jpg"] {
        let file = library.join("files").join(file);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, "x").unwrap();
    }
    let planned = plan(&fx["lib"], &Playnite { app: "C:\\Playnite".into(), library, portable: false });
    let planned: Value = serde_json::from_str(&planned.to_string().replace(serde_json::to_string(&root).unwrap().trim_matches('"'), "<tmp>")).unwrap();
    assert_eq!(planned, fx["plan"]);
}

#[test]
fn manuals_are_matched_found_and_downloaded_as_the_js_does() {
    use rustyshelf_core::manuals::{self as m, Candidate, Got, Stores, Wanted, Web, WebError};
    use serde_json::json;
    use std::collections::BTreeSet;
    use std::sync::Mutex;
    let fx = fixture("manuals.json");
    let text = |v: &Value| v.as_str().unwrap().to_string();
    let set = |v: &Value| v.as_array().unwrap().iter().map(|w| w.as_str().unwrap().to_string()).collect::<BTreeSet<_>>();
    for n in fx["norm"].as_array().unwrap() {
        let t = n[0].as_str().unwrap();
        assert_eq!((m::norm(t), m::words(t), m::seq_nums(t)), (text(&n[1]), set(&n[2]), set(&n[3])), "{t}");
    }
    let near = |a: f64, b: &Value| (a - b.as_f64().unwrap()).abs() < 1e-9;
    for p in fx["parts"].as_array().unwrap() {
        let (a, b) = (p[0].as_str().unwrap(), p[1].as_str().unwrap());
        let (recall, noise, wrong) = m::sim_parts(a, b);
        assert!(near(recall, &p[2]) && json!(noise) == p[3] && json!(wrong) == p[4] && near(m::similarity(a, b), &p[5]), "{a} | {b}: {recall} {noise} {wrong}, wanted {p}");
    }
    for s in fx["scores"].as_array().unwrap() {
        let o = &s[3];
        let downloads = o.get("downloads").map_or(0.0, |d| d.as_f64().unwrap_or_else(|| d.as_str().unwrap().parse().unwrap()));
        let got = m::score_hit(s[0].as_str().unwrap(), s[1].as_str().unwrap(), s[2].as_str().unwrap(), o["lang"].as_str().unwrap_or(""), o["anyPlatform"] == true, downloads);
        assert!(near(got, &s[4]), "{s}: {got}");
    }
    for f in fx["foreign"].as_array().unwrap() {
        let (sys, t) = (f[0].as_str().unwrap(), f[1].as_str().unwrap());
        assert_eq!(json!([m::foreign_platform(sys, t, false), m::foreign_platform(sys, t, true)]), json!([f[2], f[3]]), "{sys}: {t}");
    }
    for s in fx["slugs"].as_array().unwrap() {
        assert_eq!(json!(m::gdb_slugs(s[0].as_str().unwrap())), s[1], "{}", s[0]);
    }
    for a in fx["alt"].as_array().unwrap() {
        assert_eq!(json!(m::alt_numbers(a[0].as_str().unwrap())), a[1], "{}", a[0]);
    }
    for p in fx["pageImages"].as_array().unwrap() {
        assert_eq!(json!(m::is_page_image(&p[0])), p[1], "{}", p[0]);
    }
    for l in fx["langs"].as_array().unwrap() {
        let raw = match &l[0] {
            Value::Array(a) => a.iter().map(|x| x.as_str().unwrap()).collect::<Vec<_>>().join(";"),
            other => other.as_str().unwrap_or("").to_string(),
        };
        assert_eq!(json!(m::lang_code(&raw, l[1].as_str().unwrap())), l[2], "{l}");
    }
    for r in fx["regions"].as_array().unwrap() {
        assert_eq!(json!(m::region_of(r[0].as_str().unwrap())), r[1], "{}", r[0]);
    }
    let list: Vec<Candidate> = fx["prefs"]["list"].as_array().unwrap().iter().map(|c| Candidate { id: text(&c["id"]), title: text(&c["title"]), language: text(&c["language"]), pages: 0, score: c["score"].as_f64().unwrap(), source: "", thumb: String::new(), url: String::new() }).collect();
    for p in fx["prefs"]["picks"].as_array().unwrap() {
        let (pref, key) = (p[0].as_str().unwrap(), p[1].as_str().unwrap());
        let pick = m::pref_pick(&list, pref, &|id| key == "gc|rejecting" && id == "a");
        assert_eq!((pick.as_ref().map(|c| c.id.as_str()), pick.as_ref().map(|c| c.score)), (p[2].as_str(), p[3].as_f64()), "{pref} for {key}");
        assert_eq!(json!(list.iter().map(|c| m::pref_match(c, pref)).collect::<Vec<_>>()), p[4], "{pref}");
    }

    // the stand-in for the sites
    struct Site<'a>(&'a Value, Mutex<usize>);
    impl Site<'_> {
        fn answer(&self, url: &str) -> Result<Vec<u8>, WebError> {
            *self.1.lock().unwrap() += 1;
            let Some(r) = self.0.get(url) else { return Err(WebError::Http(404)) };
            match r["status"].as_u64() {
                Some(code) if code != 200 => Err(WebError::Http(code as u16)),
                _ => Ok(match (r.get("json"), r["page"].as_str()) {
                    (Some(j), _) => j.to_string().into_bytes(),
                    (None, Some(letter)) => letter.repeat(3000).into_bytes(),
                    _ => r["body"].as_str().unwrap_or("").as_bytes().to_vec(),
                }),
            }
        }
    }
    impl Web for Site<'_> {
        fn json(&self, url: &str) -> Result<Value, WebError> {
            serde_json::from_slice(&self.answer(url)?).map_err(|_| WebError::Unreadable)
        }
        fn text(&self, url: &str) -> Result<String, WebError> {
            Ok(String::from_utf8(self.answer(url)?).unwrap())
        }
        fn file(&self, url: &str, dest: &Path) -> Result<bool, WebError> {
            match self.answer(url) {
                Ok(bytes) if bytes.len() >= 2048 => {
                    fs::write(dest, bytes).unwrap();
                    Ok(true)
                }
                _ => Ok(false),
            }
        }
        fn pause(&self, _: u64) {}
    }
    let site = Site(&fx["site"], Mutex::new(0));
    let listed = |found: &[Candidate]| Value::from(found.iter().map(Candidate::to_json).collect::<Vec<_>>());
    for s in fx["searches"].as_array().unwrap() {
        let got = match m::search(&site, s[0].as_str().unwrap(), s[1].as_str().unwrap()) {
            Ok(found) => listed(&found),
            Err(e) => json!({ "error": e.words() }),
        };
        assert_eq!(got, s[2], "{} {}", s[0], s[1]);
    }
    let plain = |found: &[Candidate]| Value::from(found.iter().map(|c| { let mut j = c.to_json(); let o = j.as_object_mut().unwrap(); o.remove("languageName"); o.remove("english"); j }).collect::<Vec<_>>());
    assert_eq!(plain(&m::search_archive(&site, "gc", "Zeta Run", true).unwrap()), fx["elsewhere"]);
    assert_eq!(plain(&m::search_games_database(&site, "gc", "Zeta Run").into_iter().collect::<Vec<_>>()), json!([fx["gdb"]]));

    let tmp = tempfile::tempdir().unwrap();
    let holds = |dir: &Path| -> Value {
        let mut names: Vec<String> = fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names.into_iter().map(|n| { let b = fs::read(dir.join(&n)).unwrap(); (n, json!(format!("{}{}", b[0] as char, b.len()))) }).collect::<serde_json::Map<_, _>>().into()
    };
    for f in fx["fetched"].as_array().unwrap() {
        let (id, dir) = (f[0].as_str().unwrap(), tmp.path().join(format!("f-{}", f[0].as_str().unwrap())));
        let got = match m::fetch_manual(&site, id, &dir, "Zeta Run (USA)", &|_, _| {}) {
            Ok(n) => json!(n),
            Err(why) => json!(format!("error: {why}")),
        };
        assert_eq!((got, holds(&dir)), (f[1].clone(), f[2].clone()), "{id}");
    }

    let covers = tmp.path().join("roms").join("covers");
    fs::create_dir_all(&covers).unwrap();
    fs::write(covers.join("Delta Has One (USA)-manual-01.jpg"), "page").unwrap();
    assert_eq!(m::download(&site, "gdb:https://www.gamesdatabase.org/Media/Manual/Zeta_Run_GameCube_Manual.pdf", &covers, "Beta Kart (Europe)", &|_, _| {}), Ok(Got::Pdf(covers.join("Beta Kart (Europe)-manual.pdf"))));
    assert_eq!(json!(m::download(&site, "gdb:https://example.org/x.pdf", &covers, "Gamma Force (USA)", &|_, _| {}).unwrap_err()), fx["chosen"][0]["error"]);
    assert_eq!(m::download(&site, "numbered", &covers, "Gamma Force (USA)", &|_, _| {}), Ok(Got::Pages(2)));
    assert_eq!(holds(&covers), fx["chosenHolds"]);
    for e in fs::read_dir(&covers).unwrap().flatten().filter(|e| !e.file_name().to_string_lossy().starts_with("Delta")) {
        fs::remove_file(e.path()).unwrap();
    }

    let data = tmp.path().join("data");
    fs::create_dir_all(&data).unwrap();
    let games: Vec<Wanted> = [("Alpha Tale", "Alpha Tale (USA)"), ("Beta Kart", "Beta Kart (Europe)"), ("Gamma Force", "Gamma Force (USA)")]
        .iter()
        .map(|(title, base)| Wanted { sys: "snes".into(), title: title.to_string(), file: tmp.path().join("roms").join(format!("{base}.sfc")).to_string_lossy().into_owned(), dir: covers.clone(), base: base.to_string() })
        .collect();
    let mut stores = Stores::load(&data);
    let now = 1.7e12;
    assert_eq!(json!(m::auto_fetch(&site, &games, &mut stores, now, &|| false)), json!([fx["auto"]["searched"], fx["auto"]["got"]]));
    assert_eq!(holds(&covers), fx["autoHolds"]);
    let memo: Value = serde_json::from_slice(&fs::read(data.join("manual-auto.json")).unwrap()).unwrap();
    let memo: serde_json::Map<String, Value> = memo.as_object().unwrap().iter().map(|(k, v)| { let mut v = v.clone(); assert_eq!(v.as_object_mut().unwrap().remove("at"), Some(json!(1.7e12 as i64))); (k.rsplit('/').next().unwrap().to_string(), v) }).collect();
    assert_eq!(Value::from(memo), fx["autoMemo"]);
    // looked at: nothing is asked again until it's due, a day after an error and a month after finding nothing
    let asked = *site.1.lock().unwrap();
    let mut stores = Stores::load(&data);
    assert_eq!(m::auto_fetch(&site, &games[1..], &mut stores, now + 3_600_000.0, &|| false), (0, 0));
    assert_eq!((*site.1.lock().unwrap(), fx["autoAgain"].clone()), (asked, json!([{ "searched": 0, "got": 0 }, 0])));
    assert!(!stores.due(&games[1].key(), now + 29.0 * 86_400_000.0) && stores.due(&games[1].key(), now + 31.0 * 86_400_000.0));
    assert!(!stores.due(&games[2].key(), now + 3_600_000.0) && stores.due(&games[2].key(), now + 25.0 * 3_600_000.0));
}

/// The ids, tokens and PINs of a run are random: the fixture names them `<id3>`, `<token0>`, and each is bound
/// here to this run's own where it first appears.
#[derive(Default)]
struct Bound(std::collections::HashMap<String, String>);

impl Bound {
    fn fill(&self, s: &str) -> String {
        self.0.iter().fold(s.to_string(), |s, (name, value)| s.replace(name, value))
    }

    fn same(&mut self, want: &str, got: &str) -> bool {
        let names = regex::Regex::new("<(id|token)[0-9]+>").unwrap();
        let (mut pattern, mut at, mut fresh) = (String::from("^"), 0, Vec::new());
        for m in names.find_iter(want) {
            pattern.push_str(&regex::escape(&want[at..m.start()]));
            match self.0.get(m.as_str()) {
                Some(value) => pattern.push_str(&regex::escape(value)),
                None => {
                    pattern.push_str("([A-Za-z0-9_-]+)");
                    fresh.push(m.as_str().to_string());
                }
            }
            at = m.end();
        }
        pattern.push_str(&regex::escape(&want[at..]));
        pattern.push('$');
        let Some(found) = regex::Regex::new(&pattern).unwrap().captures(got) else { return false };
        for (i, name) in fresh.into_iter().enumerate() {
            self.0.entry(name).or_insert_with(|| found[i + 1].to_string());
        }
        true
    }

    fn alike(&mut self, want: &Value, got: &Value) -> bool {
        match (want, got) {
            (Value::String(a), Value::String(b)) => self.same(a, b),
            (Value::Array(a), Value::Array(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| self.alike(x, y)),
            (Value::Object(a), Value::Object(b)) => a.len() == b.len() && a.iter().all(|(k, x)| b.get(k).is_some_and(|y| self.alike(x, y))),
            (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
            _ => want == got,
        }
    }
}

struct PhoneWorld {
    library: std::sync::Arc<Value>,
    achievements: Value,
    hidden: Vec<String>,
    glyphs: (String, bool),
    home_menu: bool,
    play_ok: bool,
    running: Option<rustyshelf_core::companion::Running>,
    calls: Vec<Value>,
}

struct PhoneHost(std::sync::Mutex<PhoneWorld>);

impl rustyshelf_core::companion::Host for PhoneHost {
    fn library(&self) -> std::sync::Arc<Value> {
        self.0.lock().unwrap().library.clone()
    }
    fn hidden(&self) -> Vec<String> {
        self.0.lock().unwrap().hidden.clone()
    }
    fn glyphs(&self) -> (String, bool) {
        self.0.lock().unwrap().glyphs.clone()
    }
    fn home_menu(&self) -> bool {
        self.0.lock().unwrap().home_menu
    }
    fn running(&self) -> Option<rustyshelf_core::companion::Running> {
        self.0.lock().unwrap().running.clone()
    }
    fn play(&self, shelf: &str, id: &str, title: &str) -> bool {
        let mut w = self.0.lock().unwrap();
        w.calls.push(serde_json::json!(["play", shelf, id, title]));
        w.play_ok
    }
    fn quit(&self) {
        let mut w = self.0.lock().unwrap();
        w.calls.push(serde_json::json!(["quit"]));
        w.running = None;
    }
    fn input(&self, input: rustyshelf_core::companion::Input) {
        use rustyshelf_core::companion::Input;
        let told = match input {
            Input::Button(b, d) => serde_json::json!({ "b": b, "d": d }),
            Input::Key(key) => serde_json::json!({ "key": key }),
            Input::Text(text) => serde_json::json!({ "text": text }),
        };
        self.0.lock().unwrap().calls.push(serde_json::json!(["input", told]));
    }
    fn achievements(&self, _shelf: &str, title: &str) -> Option<Value> {
        self.0.lock().unwrap().achievements.get(title).cloned()
    }
    fn has_achievements(&self, shelf: &str) -> bool {
        shelf == "snes"
    }
    fn version(&self) -> String {
        "9.9.9".into()
    }
}

#[test]
fn phone_companion_answers_as_the_js_does() {
    use rustyshelf_core::companion::{self, Companion, Request, Running};
    use rustyshelf_core::js;
    use sha2::Digest;
    use std::sync::{Arc, Mutex};
    let f = fixture("companion.json");
    for q in f["qr"].as_array().unwrap() {
        let text = q["text"].as_str().unwrap();
        match (companion::qr_matrix(text), q.get("rows")) {
            (Ok(m), Some(rows)) => {
                let got: Vec<Value> = m.iter().map(|r| Value::String(r.iter().map(|d| if *d { '1' } else { '0' }).collect())).collect();
                assert_eq!(&Value::Array(got), rows, "the QR code of {text:?}");
            }
            (Err(e), None) => assert_eq!(e, q["error"].as_str().unwrap()),
            (got, _) => panic!("the QR code of {text:?}: {:?}", got.map(|m| m.len())),
        }
    }
    for a in f["private"].as_array().unwrap() {
        assert_eq!(companion::is_private(a[0].as_str().unwrap()), a[1].as_bool().unwrap(), "{}", a[0]);
    }
    for n in f["names"].as_array().unwrap() {
        assert_eq!(companion::phone_name(n[0].as_str().unwrap(), n[1].as_str().unwrap()), n[2].as_str().unwrap(), "{n}");
    }
    for u in f["files"].as_array().unwrap() {
        assert_eq!(companion::file_of(u[0].as_str().unwrap()).as_deref(), u[1].as_str(), "{}", u[0]);
    }

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_string_lossy().into_owned();
    fs::write(tmp.path().join("zeta front.png"), base64::engine::general_purpose::STANDARD.decode(f["png"].as_str().unwrap()).unwrap()).unwrap();
    let library: Value = serde_json::from_str(&f["library"].to_string().replace("<encdir>", &js::encode_uri_component(&dir)).replace("<dir>", &dir)).unwrap();
    let host = Arc::new(PhoneHost(Mutex::new(PhoneWorld { library: Arc::new(library), achievements: f["achievements"].clone(), hidden: Vec::new(), glyphs: ("xbox".into(), false), home_menu: true, play_ok: true, running: None, calls: Vec::new() })));
    let phone = Companion::new(tmp.path(), host.clone());
    phone.start_on(Some("127.0.0.1".into()));
    assert!(phone.state().on, "{}", phone.state().error);

    let mut bound = Bound::default();
    for (i, step) in f["steps"].as_array().unwrap().iter().enumerate() {
        host.0.lock().unwrap().calls.clear();
        if let Some(set) = step.get("set") {
            let mut w = host.0.lock().unwrap();
            if let Some(ok) = set.get("playOk") {
                w.play_ok = ok.as_bool().unwrap();
            }
            if let Some(r) = set.get("running") {
                let part = |k: &str| r[k].as_str().unwrap_or("").to_string();
                w.running = Some(Running { id: part("id"), title: part("title"), kind: part("kind") });
            }
            if let Some(h) = set.get("hidden") {
                w.hidden = h.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect();
            }
            if let Some(g) = set.get("glyphs") {
                w.glyphs = (g["style"].as_str().unwrap().to_string(), js::truthy(g.get("swap")));
            }
            if let Some(m) = set.get("homeMenu") {
                w.home_menu = m.as_bool().unwrap();
            }
            continue;
        }
        if step.get("renew").is_some() {
            phone.renew_pin();
            continue;
        }
        if let Some(who) = step.get("forget") {
            phone.forget(who.as_str().unwrap());
            continue;
        }
        let pin = phone.state().pin;
        let fill = |s: &str| bound.fill(s).replace("<pin>", &pin).replace("<notpin>", &format!("{:06}", (pin.parse::<u32>().unwrap() + 1) % 1_000_000));
        let body = step.get("body").and_then(Value::as_str);
        let mut headers: std::collections::HashMap<String, String> = [("host".to_string(), "127.0.0.1:18730".to_string())].into();
        if body.is_some() {
            headers.insert("content-type".into(), "application/json".into());
        }
        for (name, value) in step.get("headers").and_then(Value::as_object).into_iter().flatten() {
            match value.as_str() {
                Some(v) => headers.insert(name.clone(), fill(v)),
                None => headers.remove(name),
            };
        }
        let req = Request { ip: step["ip"].as_str().unwrap_or("192.168.1.50").into(), method: step["method"].as_str().unwrap_or("GET").into(), url: fill(step["url"].as_str().unwrap()), headers, body: body.map_or(Vec::new(), |b| fill(b).into_bytes()) };
        let (got, want) = (phone.handle(&req), &step["answer"]);
        let what = format!("step {i}: {} {}", req.method, req.url);
        let Some(got) = got else {
            assert!(want.is_null(), "{what}: no answer, the JS gave {want}");
            continue;
        };
        assert_eq!(got.status, want["status"].as_u64().unwrap() as u16, "{what}: {}", String::from_utf8_lossy(&got.body));
        let want_head = want["headers"].as_array().unwrap();
        assert_eq!(got.headers.len(), want_head.len(), "{what}: {:?}", got.headers);
        for ((name, value), w) in got.headers.iter().zip(want_head) {
            assert!(name == w[0].as_str().unwrap() && bound.same(w[1].as_str().unwrap(), value), "{what}: {name}: {value}, the JS gave {w}");
        }
        match want.get("json") {
            Some(json) => {
                let said: Value = serde_json::from_slice(&got.body).unwrap();
                assert!(bound.alike(json, &said), "{what}: {said}\nthe JS gave {json}");
            }
            None => assert_eq!(sha2::Sha256::digest(&got.body).iter().map(|b| format!("{b:02x}")).collect::<String>(), want["sha"].as_str().unwrap(), "{what}"),
        }
        let calls = Value::Array(host.0.lock().unwrap().calls.clone());
        assert_eq!(&calls, step.get("calls").unwrap_or(&serde_json::json!([])), "{what}");
    }
    let state = phone.state();
    assert_eq!((state.phones.len() as u64, state.pin.len() as u64), (f["state"]["phones"].as_u64().unwrap(), f["state"]["pin"].as_u64().unwrap()));
    let stored: Value = serde_json::from_slice(&fs::read(tmp.path().join("companion.json")).unwrap()).unwrap();
    let mut keys: Vec<&String> = stored.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(serde_json::json!(keys), f["stored"]);
    phone.stop();
}

#[test]
fn phone_companion_serves_the_home_network() {
    use rustyshelf_core::companion::{Companion, Request};
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};
    let tmp = tempfile::tempdir().unwrap();
    let big = tmp.path().join("tall.png");
    image::RgbImage::from_pixel(300, 400, image::Rgb([200, 40, 40])).save(&big).unwrap();
    let library = serde_json::json!([{ "id": "snes", "name": "Super Shelf", "kind": "console", "games": [{ "id": "snes:Zeta Run.sfc", "title": "Zeta Run", "kind": "rom", "art": { "front": big.to_string_lossy() } }] }]);
    let host = Arc::new(PhoneHost(Mutex::new(PhoneWorld { library: Arc::new(library), achievements: Value::Null, hidden: Vec::new(), glyphs: ("xbox".into(), false), home_menu: true, play_ok: true, running: None, calls: Vec::new() })));
    let phone = Companion::new(tmp.path(), host);
    phone.start_on(Some("127.0.0.1".into()));
    let state = phone.state();
    assert!(state.on && state.pin.len() == 6 && phone.pairing_link().unwrap() == format!("{}/#pin={}", state.url, state.pin), "{state:?}");

    let over_the_wire = |request: String| {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", state.port)).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut answer = Vec::new();
        stream.read_to_end(&mut answer).unwrap();
        answer
    };
    let host_line = format!("Host: 127.0.0.1:{}", state.port);
    let page = String::from_utf8_lossy(&over_the_wire(format!("GET / HTTP/1.1\r\n{host_line}\r\n\r\n"))).into_owned();
    assert!(page.starts_with("HTTP/1.1 200 OK\r\n") && page.contains("Content-Security-Policy: default-src 'self'") && page.contains("<html"), "{}", &page[..page.len().min(400)]);
    let body = format!("{{\"pin\":\"{}\",\"name\":\"Hall\"}}", state.pin);
    let paired = String::from_utf8_lossy(&over_the_wire(format!("POST /api/pair HTTP/1.1\r\n{host_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()))).into_owned();
    assert!(paired.starts_with("HTTP/1.1 200 OK\r\n") && paired.contains("\"name\":\"Hall\""), "{paired}");
    let cookie = paired.lines().find_map(|l| l.strip_prefix("Set-Cookie: ")).unwrap().split(';').next().unwrap().to_string();
    assert_ne!(phone.state().pin, state.pin, "a PIN pairs one phone");
    let huge = format!("POST /api/input HTTP/1.1\r\n{host_line}\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nContent-Length: 20000\r\n\r\n{}", "x".repeat(20000));
    assert!(String::from_utf8_lossy(&over_the_wire(huge)).starts_with("HTTP/1.1 413 "));

    // a picture taller than asked for goes out smaller, as a JPEG
    let ask = |url: &str| phone.handle(&Request { ip: "192.168.1.50".into(), method: "GET".into(), url: url.into(), headers: [("host".to_string(), "192.168.1.2:8730".to_string()), ("cookie".to_string(), cookie.clone())].into(), body: Vec::new() }).unwrap();
    let all: Value = serde_json::from_slice(&ask("/api/library").body).unwrap();
    let cover = all["shelves"][0]["games"][0]["c"].as_str().unwrap().to_string();
    let small = ask(&format!("/img/{cover}?h=100"));
    assert!(small.headers.iter().any(|h| h.0 == "Content-Type" && h.1 == "image/jpeg"));
    let made = image::load_from_memory(&small.body).unwrap();
    assert_eq!((made.width(), made.height()), (135, 180));
    assert_eq!(ask(&format!("/img/{cover}")).body, fs::read(&big).unwrap());

    // more than a remote needs at once is turned away
    let mut refused = false;
    for _ in 0..2000 {
        refused |= ask("/api/me").status == 429;
    }
    assert!(refused);
    assert!(phone.handle(&Request { ip: "8.8.8.8".into(), method: "GET".into(), url: "/".into(), ..Default::default() }).is_none());
    phone.stop();
    assert!(!phone.state().on && phone.state().pin.is_empty());
}

#[test]
fn steam_achievements_and_storage_match_the_js() {
    use rustyshelf_core::{pcach, storage};
    let f = fixture("pcach.json");
    let decode = |v: &Value| base64::engine::general_purpose::STANDARD.decode(v.as_str().unwrap()).unwrap();
    // a number can be past what JSON keeps whole
    fn alike(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
            (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| alike(p, q)),
            (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().zip(y).all(|((k, p), (l, q))| k == l && alike(p, q)),
            _ => a == b,
        }
    }
    for (i, blob) in f["blobs"].as_array().unwrap().iter().enumerate() {
        match pcach::parse_bin(&decode(&blob["bytes"])) {
            Ok(kv) => assert!(alike(&kv.to_json(), &blob["parsed"]), "blob {i}: {}, the JS read {}", kv.to_json(), blob["parsed"]),
            Err(e) => assert!(blob["parsed"].is_null() && (blob["error"] == "range" || blob["error"] == e.as_str()), "blob {i}: {e}"),
        }
    }
    let tmp = tempfile::tempdir().unwrap();
    let stats = tmp.path().join("appcache/stats");
    fs::create_dir_all(&stats).unwrap();
    fs::create_dir_all(tmp.path().join("config")).unwrap();
    for (name, bytes) in f["files"].as_object().unwrap() {
        fs::write(stats.join(name), decode(bytes)).unwrap();
    }
    let users = tmp.path().join("config/loginusers.vdf");
    for a in f["accounts"].as_array().unwrap() {
        match a["text"].as_str() {
            Some(text) => fs::write(&users, text).unwrap(),
            None => fs::remove_file(&users).unwrap(),
        }
        assert_eq!(pcach::account_id(tmp.path()), a["id"].as_str().unwrap(), "{}", a["text"]);
    }
    fs::write(&users, f["users"].as_str().unwrap()).unwrap();
    for p in f["progress"].as_array().unwrap() {
        let got = pcach::steam_progress(tmp.path(), p[0].as_str().unwrap()).unwrap_or(Value::Null);
        assert_eq!(got, p[1], "the achievements of app {}", p[0]);
    }
    assert!(pcach::steam_progress(Path::new(""), "4242").is_none() && f["noSteam"].is_null());

    let s = &f["storage"];
    let dir = tmp.path().to_string_lossy().into_owned();
    let here = |v: &Value| v.as_str().unwrap().replace("<dir>", &dir);
    for d in ["a/Zeta Run", "a/Omega Quest", "a/deep/Sigma", "b/Tau Racers", "c d/Phi", "elsewhere/Rho"] {
        fs::create_dir_all(tmp.path().join("games").join(d)).unwrap();
    }
    fs::write(tmp.path().join("games/a/file.txt"), "x").unwrap();
    let games: Vec<storage::Entry> = s["games"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| storage::Entry { id: here(&g["id"]), title: here(&g["title"]), source: g["source"].as_str().unwrap_or("").into(), dir: here(&g["dir"]), bytes: g["bytes"].as_u64().unwrap() })
        .collect();
    let room = |root: &str| {
        let r = s["room"].as_object().unwrap().iter().find(|(k, _)| k.replace("<dir>", &dir) == root).map(|(_, v)| v);
        (r.and_then(|r| r["free"].as_f64()).map(|n| n as u64), r.and_then(|r| r["total"].as_f64()).map(|n| n as u64))
    };
    let drives: Vec<Value> = storage::summary(&games, Some(&here(&s["mounts"])), &room)
        .iter()
        .map(|d| {
            let games: Vec<Value> = d.games.iter().map(|g| serde_json::json!({ "id": g.id, "title": g.title, "source": g.source, "dir": g.dir.replace(&dir, "<dir>"), "bytes": g.bytes })).collect();
            serde_json::json!({ "root": d.root.replace(&dir, "<dir>"), "games": games, "gamesBytes": d.games_bytes, "free": d.free, "total": d.total })
        })
        .collect();
    assert!(alike(&Value::Array(drives.clone()), &s["summary"]["drives"]), "{}\nthe JS gave {}", Value::Array(drives), s["summary"]["drives"]);
    for m in f["mountOf"].as_array().unwrap() {
        assert_eq!(storage::mount_of(m[0].as_str().unwrap(), Some(m[1].as_str().unwrap_or(""))), m[2].as_str().unwrap(), "{}", m[0]);
    }
}

#[test]
fn controllers_are_told_apart_as_in_the_js() {
    use rustyshelf_core::pads::{self, Glyphs, Real};
    let f = fixture("pads.json");
    let style = |s: &Value| match s.as_str().unwrap() {
        "ps" => Glyphs::Ps,
        "nintendo" => Glyphs::Nintendo,
        "xbox" => Glyphs::Xbox,
        other => panic!("{other}"),
    };
    let real = |list: &Value| -> Vec<Real> { list.as_array().unwrap().iter().map(|r| Real { vendor: r["vendor"].as_str().unwrap().into(), product: r["product"].as_str().unwrap().into(), style: style(&r["style"]) }).collect() };
    for i in f["info"].as_array().unwrap() {
        let (id, want) = (i[0].as_str().unwrap(), &i[1]);
        let got = pads::pad_info(id);
        assert_eq!((got.style, got.name.as_str(), got.vendor.as_str(), got.product.as_str(), got.wrapped), (style(&want["style"]), want["name"].as_str().unwrap(), want["vendor"].as_str().unwrap(), want["product"].as_str().unwrap(), want["virtual"].as_bool().unwrap()), "{id}");
    }
    let ids: Vec<&str> = f["ids"].as_array().unwrap().iter().map(|i| i.as_str().unwrap()).collect();
    for s in f["styles"].as_array().unwrap() {
        let list = real(&s["list"]);
        for (id, want) in ids.iter().zip(s["pads"].as_array().unwrap()) {
            assert_eq!((pads::style_of(id, &list), pads::pad_name(id, &list)), (style(&want[0]), want[1].as_str().unwrap().to_string()), "{id} with {}", s["list"]);
        }
    }
    for w in f["words"].as_array().unwrap() {
        let (st, swap) = (style(&w["style"]), w["swap"].as_bool().unwrap());
        for c in w["caps"].as_array().unwrap() {
            assert_eq!(pads::cap(c[0].as_str().unwrap(), st, false), c[1].as_str().unwrap(), "{} on {st:?}", c[0]);
        }
        assert_eq!((pads::button_word(true, st, swap), pads::button_word(false, st, swap)), (w["ok"].as_str().unwrap(), w["back"].as_str().unwrap()), "{st:?} {swap}");
        for p in w["pad"].as_array().unwrap() {
            assert_eq!(pads::pad_word(p[0].as_str().unwrap(), st), p[1].as_str().unwrap());
        }
        // with the right button confirming, the button that confirms is drawn as that one
        assert_eq!((pads::cap("x", st, true), pads::cap("o", st, true), pads::cap("tri", st, true)), (pads::cap("o", st, false), pads::cap("x", st, false), pads::cap("tri", st, false)));
    }
    for d in f["devices"].as_array().unwrap() {
        assert_eq!(pads::parse(d["text"].as_str().unwrap()), real(&d["pads"]), "{}", d["text"]);
    }
}

#[test]
fn manual_collections_and_spreads_match_the_js() {
    use image::{Rgb, RgbImage};
    use rustyshelf_core::manualcoll::{self as coll, Game};
    use rustyshelf_core::manuals::{Stores, Web, WebError};
    use rustyshelf_core::spreads::{self, Split};
    use serde_json::json;
    use std::sync::Mutex;
    let f = fixture("manualcoll.json");
    // numbers by their value, objects by their keys
    fn same(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
            (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
            (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, p)| y.get(k).is_some_and(|q| same(p, q))),
            _ => a == b,
        }
    }
    let tmp = tempfile::tempdir().unwrap();
    let stamp = regex::Regex::new(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9-]+$").unwrap();
    let tree = |dir: &Path| -> Vec<String> {
        fn walk(d: &Path, rel: String, stamp: &regex::Regex, out: &mut Vec<String>) {
            for e in fs::read_dir(d).into_iter().flatten().flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if e.path().is_dir() {
                    let shown = if stamp.is_match(&name) { "<stamp>".to_string() } else { name };
                    walk(&e.path(), if rel.is_empty() { shown } else { format!("{rel}/{shown}") }, stamp, out);
                } else {
                    out.push(if rel.is_empty() { name } else { format!("{rel}/{name}") });
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, String::new(), &stamp, &mut out);
        out.sort();
        out
    };

    // each page is drawn with its number in its green, red on its left half and blue on its right
    for s in f["spreads"].as_array().unwrap() {
        let base = s["base"].as_str().unwrap();
        let dir = tmp.path().join("sp").join(base);
        fs::create_dir_all(&dir).unwrap();
        let mut whole = Vec::new();
        for (i, p) in s["pages"].as_array().unwrap().iter().enumerate() {
            let (n, w, h) = (p[0].as_str().unwrap(), p[1].as_u64().unwrap() as u32, p[2].as_u64().unwrap() as u32);
            let green = 20 * i as u8 + 20;
            let file = dir.join(format!("{base}-manual-{n}"));
            RgbImage::from_fn(w, h, |x, _| if x < w / 2 { Rgb([200, green, 0]) } else { Rgb([0, green, 200]) }).save(&file).unwrap();
            whole.push((n.to_string(), fs::read(&file).unwrap()));
        }
        fs::write(dir.join(format!("{base}-front.png")), "not a page").unwrap();
        fs::write(dir.join(format!("{base}-manual-x.png")), "not numbered").unwrap();
        let opts = Split { scan: s["opts"]["scan"] == true, min_w: s["opts"]["minW"].as_u64().unwrap_or(0) as u32, discard: s["opts"]["discard"] == true };
        assert_eq!(spreads::split_manual(&dir, base, opts).unwrap() as u64, s["n"].as_u64().unwrap(), "{base}");
        let mut now = serde_json::Map::new();
        let mut names: Vec<String> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.contains("-manual-") && !n.ends_with("-x.png") && !n.starts_with('.')).collect();
        names.sort();
        for name in names {
            let bytes = fs::read(dir.join(&name)).unwrap();
            let what = match whole.iter().find(|w| w.1 == bytes) {
                Some((n, _)) => format!("whole:{n}"),
                None => {
                    let half = image::load_from_memory(&bytes).unwrap().into_rgb8();
                    let px = half.get_pixel(half.width() / 2, half.height() / 2).0;
                    let page = ((f32::from(px[1]) - 20.0) / 20.0).round() as usize;
                    let of = &s["pages"][page];
                    assert_eq!(u64::from(half.width()), of[1].as_u64().unwrap() / 2, "{base} {name}");
                    format!("{}:{}", if px[0] > px[2] { "left" } else { "right" }, of[0].as_str().unwrap())
                }
            };
            now.insert(name, json!(what));
        }
        assert_eq!(Value::Object(now), s["now"], "{base}");
        assert_eq!(json!(tree(&dir.join(".manual-originals"))), s["kept"], "{base}");
        // (the fixture's halves aren't pictures: a cut manual's width is only known here)
        if s["n"] == 0 {
            assert_eq!(u64::from(spreads::median_width(&dir, base)), s["median"].as_u64().unwrap(), "{base}");
        }
    }
    for z in f["isSpread"].as_array().unwrap() {
        let size = z[0].as_array().map(|z| (z[0].as_u64().unwrap() as u32, z[1].as_u64().unwrap() as u32));
        assert_eq!(spreads::is_spread(size), z[1].as_bool().unwrap(), "{}", z[0]);
    }

    for c in f["clean"].as_array().unwrap() {
        let got = coll::clean(c[0].as_str().unwrap());
        assert_eq!(json!({ "title": got.title, "tags": got.tags, "titleId": got.title_id, "disc": got.disc }), c[1], "{}", c[0]);
    }
    for (sys, cols) in f["collections"].as_object().unwrap() {
        let got: Vec<Value> = coll::collections(sys).iter().map(|c| json!({ "id": c.id, "folder": c.folder, "names": c.names, "digital": c.digital })).collect();
        assert_eq!(&Value::Array(got), cols, "{sys}");
    }
    assert!(coll::collections("wiiu").is_empty());
    for k in f["keys"].as_array().unwrap() {
        let (title, sq, set) = coll::name_keys(k[0].as_str().unwrap(), k[1].as_str().unwrap());
        let got: serde_json::Map<String, Value> = [("title", title.map(Value::from)), ("sq", sq.map(Value::from)), ("set", set.map(Value::from))].into_iter().filter_map(|(name, v)| Some((name.to_string(), v?))).collect();
        assert_eq!(Value::Object(got), k[2], "{} as {}", k[0], k[1]);
    }

    struct Site<'a>(&'a Value, Mutex<usize>);
    impl Site<'_> {
        fn answer(&self, url: &str) -> Result<Vec<u8>, WebError> {
            *self.1.lock().unwrap() += 1;
            let Some(r) = self.0.get(url) else { return Err(WebError::Http(404)) };
            match r["status"].as_u64() {
                Some(code) if code != 200 => Err(WebError::Http(code as u16)),
                _ => Ok(match (r.get("json"), r["png"].as_array()) {
                    (Some(j), _) => j.to_string().into_bytes(),
                    (None, Some(size)) => page(size[0].as_u64().unwrap() as u32, size[1].as_u64().unwrap() as u32),
                    _ => r["body"].as_str().unwrap_or("").as_bytes().to_vec(),
                }),
            }
        }
    }
    impl Web for Site<'_> {
        fn json(&self, url: &str) -> Result<Value, WebError> {
            serde_json::from_slice(&self.answer(url)?).map_err(|_| WebError::Unreadable)
        }
        fn text(&self, url: &str) -> Result<String, WebError> {
            Ok(String::from_utf8(self.answer(url)?).unwrap())
        }
        fn file(&self, url: &str, dest: &Path) -> Result<bool, WebError> {
            match self.answer(url) {
                Ok(bytes) => {
                    fs::write(dest, bytes).unwrap();
                    Ok(true)
                }
                Err(_) => Ok(false),
            }
        }
        fn pause(&self, _: u64) {}
    }
    // a picture's header with its size, as the fixture's pages are
    fn page(w: u32, h: u32) -> Vec<u8> {
        let mut b = vec![0u8; 3000];
        b[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        b[8..12].copy_from_slice(&13u32.to_be_bytes());
        b[12..16].copy_from_slice(b"IHDR");
        b[16..20].copy_from_slice(&w.to_be_bytes());
        b[20..24].copy_from_slice(&h.to_be_bytes());
        b
    }
    let site = Site(&f["site"], Mutex::new(0));
    let asked = || *site.1.lock().unwrap();
    let data = tmp.path().join("data");
    let cache = data.join("boxart-cache");
    let dir = tmp.path().to_string_lossy().into_owned();
    let here = |v: &Value| -> Value { serde_json::from_str(&v.to_string().replace("<tmp>", &dir)).unwrap() };
    // (a game's key is its file in lower case)
    let here_low = |v: &Value| -> Value { serde_json::from_str(&v.to_string().replace("<tmp>", &dir.to_lowercase())).unwrap() };
    let there = |v: &Value| -> Value { serde_json::from_str(&v.to_string().replace(&dir, "<tmp>").replace(&dir.to_lowercase(), "<tmp>")).unwrap() };
    let mut indexes = Vec::new();
    for (col, want) in coll::collections("gc").iter().zip(f["indexes"].as_array().unwrap()) {
        let ix = coll::index(&site, &cache, col, 1e12).unwrap();
        let mut got = serde_json::to_value(&ix).unwrap();
        got["at"] = json!(0);
        assert!(same(&got, want), "{}: {got}\nthe JS listed {want}", col.id);
        indexes.push(ix);
    }
    let mut cached: Vec<String> = fs::read_dir(&cache).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    cached.sort();
    assert_eq!(json!(cached), f["cached"]);
    // listed within the month: not asked for again; older, it is
    let before = asked();
    coll::index(&site, &cache, &coll::collections("gc")[0], 1e12 + 29.0 * 86_400_000.0).unwrap();
    assert_eq!(asked(), before);
    assert_eq!(coll::index(&site, &cache, &coll::collections("gc")[0], 1e12 + 31.0 * 86_400_000.0).unwrap().at, 1e12 + 31.0 * 86_400_000.0);
    assert_eq!(asked(), before + 1);
    fs::write(cache.join("ia-NintendoGameCubeManuals.json"), serde_json::to_string(&indexes[0]).unwrap()).unwrap();

    let game = |g: &Value| Game { sys: "gc".into(), system: "GameCube".into(), title: g["title"].as_str().unwrap().into(), title_id: g["titleId"].as_str().map(String::from), file: g["file"].as_str().unwrap().into(), region: g["region"].as_str().unwrap_or("").into(), ..Game::default() };
    for m in f["matches"].as_array().unwrap() {
        let got: Vec<Vec<&str>> = indexes.iter().map(|ix| coll::matches(&ix.entries, &game(&m[0])).iter().map(|e| e.name.as_str()).collect()).collect();
        assert_eq!(json!(got), m[1], "{}", m[0]);
    }
    let sets: Vec<coll::Entry> = serde_json::from_value(f["sets"]["entries"].clone()).unwrap();
    for m in f["sets"]["games"].as_array().unwrap() {
        let got: Vec<&str> = coll::matches(&sets, &game(&m[0])).iter().map(|e| e.name.as_str()).collect();
        assert_eq!(json!(got), m[1], "{}", m[0]);
    }
    let pages: Vec<Value> = coll::scan_pages(&site, &coll::collections("gc")[1], &indexes[1], &indexes[1].entries[0]).unwrap().into_iter().map(|(full, half)| json!({ "full": full, "half": half })).collect();
    assert_eq!(Value::Array(pages), f["pages"]);

    // a run over a made-up GameCube folder
    let covers = tmp.path().join("roms/covers");
    fs::create_dir_all(&covers).unwrap();
    for (title, sizes) in f["had"].as_object().unwrap() {
        for (i, z) in sizes.as_array().unwrap().iter().enumerate() {
            fs::write(covers.join(format!("{title}-manual-{:02}.jpg", i + 1)), page(z[0].as_u64().unwrap() as u32, z[1].as_u64().unwrap() as u32)).unwrap();
        }
    }
    fs::write(data.join("manual-rejects.json"), here_low(&f["rejects"]).to_string()).unwrap();
    fs::write(data.join("manual-studio.json"), here_low(&f["studio"]).to_string()).unwrap();
    let games: Vec<Game> = here(&f["games"]).as_array().unwrap().iter().map(|g| { let g2 = game(g); Game { dir: covers.clone(), base: Path::new(&g2.file).file_stem().unwrap().to_string_lossy().into_owned(), ..g2 } }).collect();
    // (within the month of the collections' listings above)
    let clock = Mutex::new(1e12 + 2.0 * 86_400_000.0);
    let now = || { let mut t = clock.lock().unwrap(); *t += 1000.0; *t };
    let before = asked();
    let stores = Stores::load(&data);
    let mut changes: Vec<Value> = coll::run(&site, &data, &games, &stores, &now, &|_| false, &|| false).iter().map(|c| json!({ "title": c.title, "upgraded": c.upgraded })).collect();
    changes.sort_by_key(|c| c["title"].as_str().unwrap().to_string());
    assert_eq!(Value::Array(changes.clone()), f["report"]);
    assert_eq!(changes.len() as u64, f["done"].as_u64().unwrap());
    assert_eq!((asked() - before) as u64, f["asked"].as_u64().unwrap());
    let memo = || -> Value {
        let kept: Value = serde_json::from_slice(&fs::read(data.join("manual-collections.json")).unwrap()).unwrap();
        there(&Value::Object(kept.as_object().unwrap().iter().map(|(k, v)| { let mut v = v.clone(); for gone in ["at", "altTried", "render"] { v.as_object_mut().unwrap().remove(gone); } (k.clone(), v) }).collect()))
    };
    assert!(same(&memo(), &f["memo"]), "{}\nthe JS kept {}", memo(), f["memo"]);
    assert_eq!(json!(tree(&covers)), f["tree"]);
    // nothing is asked for again (the Electron app downloads a manual it then keeps its own over, every start)
    let before = asked();
    assert!(coll::run(&site, &data, &games, &stores, &now, &|_| false, &|| false).is_empty() && f["doneAgain"] == 0);
    assert_eq!(asked(), before);
    let mut kept = coll::Memo::load(&data);
    let mut recent: Vec<Value> = kept.recent(30).into_iter().map(|mut r| { r.as_object_mut().unwrap().remove("at"); r }).collect();
    recent.sort_by_key(|r| r["title"].as_str().unwrap().to_string());
    assert!(same(&Value::Array(recent.clone()), &f["recent"]), "{recent:?}");
    let key = |title: &str| format!("gc|{}", tmp.path().join(format!("roms/{title}.iso")).to_string_lossy().to_lowercase());
    assert_eq!(json!([kept.forget(&key("Phi (USA)"), now()), kept.forget("gc|nothing", now())]), f["forgot"]);
    assert!(same(&memo(), &f["memoAfter"]), "{}\nthe JS kept {}", memo(), f["memoAfter"]);

    // "wrong manual": the copy goes aside and is never brought again; the next best takes its place
    for w in f["wrong"].as_array().unwrap() {
        let g = games.iter().find(|g| g.base == w[0].as_str().unwrap()).unwrap();
        let r = rustyshelf_core::manuals::reject_manual(&site, &data, g, &now).unwrap();
        assert_eq!(json!({ "moved": r.moved, "got": r.got, "from": r.from, "pdf": r.pdf, "offline": r.offline }), w[1], "{}", w[0]);
    }
    assert_eq!(json!(tree(&covers)), f["wrongTree"]);
    assert!(same(&memo(), &f["wrongMemo"]), "{}\nthe JS kept {}", memo(), f["wrongMemo"]);
    let read = |name: &str| -> Value { there(&serde_json::from_slice(&fs::read(data.join(name)).unwrap()).unwrap()) };
    assert_eq!(read("manual-rejects.json"), f["wrongRejects"]);
    let mut auto = read("manual-auto.json");
    for v in auto.as_object_mut().unwrap().values_mut() {
        v.as_object_mut().unwrap().remove("at");
    }
    assert!(same(&auto, &f["wrongAuto"]), "{auto}\nthe JS kept {}", f["wrongAuto"]);

    // a console's choice and a game's lock, as the Electron app's Manual Studio keeps them
    let mut stores = Stores::load(&data);
    assert!(stores.set_pref("gc", "lang:xx").is_err());
    stores.set_pref("gc", "lang:fr").unwrap();
    stores.set_lock(&key("Sigma, The (USA)"), true, true, 5.0);
    let kept = Stores::load(&data);
    assert_eq!((kept.pref("gc").as_str(), kept.pref("wii").as_str()), ("lang:fr", "best"));
    assert!(kept.is_locked(&key("Sigma, The (USA)")) && kept.locked(&key("Sigma, The (USA)")) && kept.is_locked(&key("Locked Game (USA)")) && !kept.is_locked(&key("Phi (USA)")));
    // (the games that found nothing under the old choice are looked up again)
    assert_eq!(read("manual-auto.json").as_object().unwrap().len(), 1);
    assert_eq!(read("manual-studio.json")["locks"][key("Sigma, The (USA)").replace(&dir.to_lowercase(), "<tmp>")], json!({ "at": 5, "src": "cur" }));
    stores.set_pref("gc", "best").unwrap();
    stores.set_lock(&key("Sigma, The (USA)"), false, true, 6.0);
    assert!(read("manual-studio.json")["prefs"].as_object().unwrap().is_empty() && !Stores::load(&data).is_locked(&key("Sigma, The (USA)")));

    // a console set to another language is left alone
    let before = asked();
    let _ = fs::remove_file(data.join("manual-collections.json"));
    assert!(coll::run(&site, &data, &games, &Stores::load(&data), &now, &|sys| sys == "gc", &|| false).is_empty() && asked() == before);
}

#[test]
fn art_studio_ranks_and_lists_as_the_js_does() {
    use rustyshelf_core::artstudio::{self as studio, Cand, Ctx, Sources};
    use rustyshelf_core::covers::LibretroNames;
    use rustyshelf_core::lbindex::ShardFile;
    use serde_json::json;
    let f = fixture("artstudio.json");
    for s in f["styles"].as_array().unwrap() {
        let style = studio::style_for(&s["cfg"], "gc");
        assert_eq!(style, s["style"], "{}", s["cfg"]);
        assert_eq!(studio::style_for(&s["cfg"], "wii"), s["other"], "{}", s["cfg"]);
        let looks: Vec<String> = studio::PARTS.iter().map(|p| studio::look_for(&style, p)).collect();
        assert_eq!(json!(looks), s["looks"], "{}", s["style"]);
        let regions: Vec<Vec<String>> = ["authentic", "hd", "region:jpn", "region:xx", "special"].iter().map(|l| ["USA", "Japan", "Europe, Australia", ""].iter().map(|r| studio::region_for_look(&style, l, r)).collect()).collect();
        assert_eq!(json!(regions), s["regions"], "{}", s["style"]);
    }
    for r in f["gameRegions"].as_array().unwrap() {
        assert_eq!(studio::game_region(r[0].as_str().unwrap()), r[1].as_str().unwrap(), "{}", r[0]);
    }
    let text = |v: &Value| v.as_str().unwrap_or("").to_string();
    let cands: Vec<Cand> = f["cands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| Cand {
            source: text(&c["source"]),
            region: text(&c["region"]),
            edition: text(&c["edition"]),
            kind: text(&c["kind"]),
            fan: c["fan"] == true,
            cut: c["cut"].as_str().map(String::from),
            exact: c["exact"] == true,
            rank: c["rank"].as_f64().unwrap_or(0.0),
            w: c["w"].as_u64().map(|w| w as u32),
            h: c["h"].as_u64().map(|h| h as u32),
            in_use: c["inUse"] == true,
            ..Cand::default()
        })
        .collect();
    for r in f["ranks"].as_array().unwrap() {
        let ctx = Ctx { style: &r["ctx"]["style"], game_region: r["ctx"]["game"]["region"].as_str().unwrap_or(""), sharp: r["ctx"]["sharp"].as_bool() };
        for (c, want) in cands.iter().zip(r["scores"].as_array().unwrap()) {
            let got = studio::scores(c, &ctx);
            for look in ["authentic", "hd", "region", "special"] {
                assert_eq!(got[look].as_f64(), want[look].as_f64(), "{look} of {c:?} under {}", r["ctx"]);
            }
        }
        for by in r["by"].as_array().unwrap() {
            let look = by[0].as_str().unwrap();
            for (c, want) in cands.iter().zip(by[1].as_array().unwrap()) {
                assert!((studio::rank_score(c, look, &ctx) - want.as_f64().unwrap()).abs() < 1e-9, "{look} of {c:?} under {}: {} for {want}", r["ctx"], studio::rank_score(c, look, &ctx));
            }
            let order: Vec<usize> = studio::rank_list(&cands, look, &ctx).iter().map(|c| cands.iter().position(|x| std::ptr::eq(x, *c)).unwrap()).collect();
            assert_eq!(json!(order), by[2], "{look} under {}", r["ctx"]);
        }
    }

    let shard = |plat: &str| -> Option<ShardFile> { serde_json::from_value(json!({ "v": 1, "plat": plat, "games": f["index"]["games"].get(plat)?, "images": f["index"]["images"] })).ok() };
    let libretro = LibretroNames::new(f["libretro"].as_array().unwrap().iter().map(text).collect());
    for a in f["candidates"].as_array().unwrap() {
        let sys = a["sys"].as_str().unwrap();
        let shard = rustyshelf_core::covers::launchbox_platform(sys).and_then(shard);
        let from = Sources { launchbox: shard.as_ref(), libretro: (sys == "gc").then_some(&libretro), gametdb: None, disc_id: a["discId"].as_str().map(String::from), region: a["region"].as_str().unwrap(), wiki: None };
        for part in studio::PARTS {
            let got: Vec<Value> = studio::candidates(sys, a["game"]["title"].as_str().unwrap(), Path::new(a["game"]["file"].as_str().unwrap()), part, &from).iter().map(Cand::to_json).collect();
            let want = a["parts"][part].as_array().unwrap();
            assert_eq!(got.len(), want.len(), "{sys} {} {part}: {got:?}", a["game"]["title"]);
            for (g, w) in got.iter().zip(want) {
                assert_eq!(g, w, "{sys} {} {part}", a["game"]["title"]);
            }
        }
    }
}

#[test]
fn art_studio_lists_and_places_pictures_as_the_js_does() {
    use rustyshelf_core::artstudio::{self as studio, Cand, GameArt, Sources, Studio};
    use rustyshelf_core::covers::LibretroNames;
    use rustyshelf_core::lbindex::ShardFile;
    use serde_json::json;
    let f = fixture("artstudio.json");
    let tmp = tempfile::tempdir().unwrap();
    let (data, roms) = (tmp.path().join("data"), tmp.path().join("roms"));
    let dir = tmp.path().to_string_lossy().into_owned();
    let decode = |v: &Value| base64::engine::general_purpose::STANDARD.decode(v.as_str().unwrap()).unwrap();
    let place = |file: &Path, bytes: Vec<u8>| {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, bytes).unwrap();
    };
    fs::create_dir_all(&data).unwrap();
    place(&roms.join("Zeta Run (USA).iso"), b"rom".to_vec());
    for (name, bytes) in f["local"]["pictures"].as_object().unwrap() {
        place(&roms.join(name), decode(bytes));
    }
    let st = Studio::new(&data);
    for (key, kept) in f["local"]["cached"].as_object().unwrap() {
        let (url, cut) = key.rsplit_once('|').unwrap();
        let c = Cand { url: url.into(), cut: Some(cut.to_string()).filter(|c| !c.is_empty()), ext: kept[0].as_str().unwrap().into(), ..Cand::default() };
        place(&st.cache_path(&c), decode(&kept[1]));
    }
    let now = 2e12;
    let missing: serde_json::Map<String, Value> = f["local"]["missing"].as_object().unwrap().iter().map(|(url, days)| (url.clone(), json!(now - days.as_f64().unwrap() * 86_400_000.0))).collect();
    fs::write(data.join("art-missing.json"), Value::Object(missing).to_string()).unwrap();

    let shard: ShardFile = serde_json::from_value(json!({ "v": 1, "plat": "Nintendo GameCube", "games": f["index"]["games"]["Nintendo GameCube"], "images": f["index"]["images"] })).unwrap();
    let libretro = LibretroNames::new(f["libretro"].as_array().unwrap().iter().map(|n| n.as_str().unwrap().to_string()).collect());
    let file = roms.join("Zeta Run (USA).iso");
    let mut g = GameArt { sys: "gc".into(), title: "Zeta Run".into(), file: file.clone(), region: "USA".into(), dir: roms.join("covers"), base: "Zeta Run (USA)".into(), current: [Some(roms.join("covers/Zeta Run (USA).png")), None, None, None] };
    let rel = |p: &Path| p.to_string_lossy().replace(&dir, "<tmp>");
    let list = |g: &GameArt, part: &str| {
        let from = Sources { launchbox: Some(&shard), libretro: Some(&libretro), gametdb: None, disc_id: rustyshelf_core::covers::disc_id(&g.file), region: "usa", wiki: None };
        st.list_for(g, part, studio::candidates(&g.sys, &g.title, &g.file, part, &from), now)
    };
    // (a picture on this computer is keyed by where it is: named here by its file instead)
    let stampless = |name: &str| -> String {
        let stamp = regex::Regex::new(r"\(([0-9]{4}(?:-[0-9]+){6})(-[0-9]+)?\)").unwrap();
        stamp.replace_all(name, |m: &regex::Captures| if m[1].starts_with("2025-0") || m[1].starts_with("2026-0") { m[0].to_string() } else { format!("(STAMP{})", m.get(2).map_or("", |x| x.as_str())) }).into_owned()
    };
    let name = |key: &str, list: &[Cand]| match list.iter().find(|c| c.key == key).and_then(|c| c.file.as_ref()) {
        Some(file) => format!("local:{}", stampless(&file.strip_prefix(&roms).unwrap().to_string_lossy())),
        None => key.to_string(),
    };
    for part in studio::PARTS {
        let got = list(&g, part);
        let shown: Vec<&str> = studio::shown(&got).map(|c| c.key.as_str()).collect();
        let view: Vec<Value> = got
            .iter()
            .map(|c| json!({ "key": name(&c.key, &got), "source": c.source, "label": c.label, "w": c.w, "h": c.h, "bytes": c.bytes, "cached": c.cached, "inUse": c.in_use, "dup": c.dup.as_ref().map(|d| name(d, &got)), "guess": c.guess, "file": c.file.as_deref().map(rel), "shown": shown.contains(&c.key.as_str()) }))
            .collect();
        let want = f["lists"][part].as_array().unwrap();
        assert_eq!(view.len(), want.len(), "{part}: {view:?}");
        for (v, w) in view.iter().zip(want) {
            assert_eq!(v, w, "{part}");
        }
    }
    assert_eq!(studio::lock_key("gc", &file).replace(&dir.to_lowercase(), "<tmp>"), f["lockKey"].as_str().unwrap());

    // (a picture replaced during this run carries its own stamp, and today's date)
    let holds = |roms: &Path| -> Value {
        fn walk(d: &Path, out: &mut Vec<std::path::PathBuf>) {
            for e in fs::read_dir(d).into_iter().flatten().flatten() {
                if e.path().is_dir() { walk(&e.path(), out) } else { out.push(e.path()) }
            }
        }
        let mut files = Vec::new();
        walk(&roms.join("covers"), &mut files);
        let mut map: Vec<(String, String)> = files.iter().map(|p| { let b = fs::read(p).unwrap(); (stampless(&p.strip_prefix(roms).unwrap().to_string_lossy()), format!("{}:{}", b.len(), b[b.len() - 1])) }).collect();
        map.sort();
        Value::Object(map.into_iter().map(|(k, v)| (k, json!(v))).collect())
    };
    let day = |label: &str| match label.strip_prefix("Earlier · ") {
        Some(when) if !["5 Jan 2026", "30 Sept 2025", "1 Feb 2026"].contains(&when) => "Earlier · TODAY".to_string(),
        _ => label.to_string(),
    };
    for h in f["heads"].as_array().unwrap() {
        let got = studio::image_info(&decode(&h[0])).map(|(ext, size)| json!({ "ext": ext, "w": size.map(|z| z.0), "h": size.map(|z| z.1) }));
        assert_eq!(json!(got), h[1], "{}", h[0]);
    }
    for (i, s) in f["steps"].as_array().unwrap().iter().enumerate() {
        let part = s["part"].as_str().unwrap();
        let offered = list(&g, part);
        let chosen = offered.iter().find(|c| Some(name(&c.key, &offered).as_str()) == s["key"].as_str()).cloned();
        let error = match s["what"].as_str().unwrap() {
            "use" => match chosen {
                Some(c) => st.apply(&mut g, part, &c, now + i as f64).err(),
                None => Some("That picture is no longer offered. Open the list again.".to_string()),
            },
            _ => st.undo(&mut g, part, now + i as f64).err(),
        };
        assert_eq!(json!(error), s["error"], "step {i}: {} {part}", s["what"]);
        assert_eq!(holds(&roms), s["holds"], "step {i}: {} {part}", s["what"]);
        let front: Vec<Value> = studio::shown(&list(&g, "front")).map(|c| json!([c.source, day(&c.label), c.in_use])).collect();
        assert_eq!(Value::Array(front), s["front"], "step {i}: {} {part}", s["what"]);
    }

    // a picture of the player's own: copied in, the old one kept; what isn't a picture, or is too small, is refused
    let mine = tmp.path().join("mine.png");
    image::RgbImage::from_pixel(300, 420, image::Rgb([10, 200, 90])).save(&mine).unwrap();
    let before = g.current[0].clone().unwrap();
    st.own(&mut g, "front", &mine, false, now + 100.0).unwrap();
    assert!(mine.is_file() && fs::read(g.current[0].as_ref().unwrap()).unwrap() == fs::read(&mine).unwrap() && g.current[0].as_ref().unwrap().ends_with("covers/Zeta Run (USA).png"));
    assert!(!before.is_file() || before == *g.current[0].as_ref().unwrap());
    let in_use = g.current[0].clone().unwrap();
    st.own(&mut g, "front", &in_use, false, now + 101.0).unwrap();
    let small = tmp.path().join("small.png");
    image::RgbImage::from_pixel(40, 300, image::Rgb([0, 0, 0])).save(&small).unwrap();
    assert_eq!(st.own(&mut g, "front", &small, false, now + 102.0).unwrap_err(), "That picture is too small (40 × 300).");
    st.own(&mut g, "spine", &small, false, now + 103.0).unwrap();
    assert_eq!(st.own(&mut g, "spine", &mine, true, now + 104.0).unwrap_err(), "For a spine cut, pick a picture of the whole box opened flat: back, spine and front side by side.");
    let text = tmp.path().join("notes.png");
    fs::write(&text, "not a picture ".repeat(40)).unwrap();
    assert_eq!(st.own(&mut g, "back", &text, false, now + 105.0).unwrap_err(), "That file isn't a PNG, JPEG or WebP picture.");
    assert_eq!(st.own(&mut g, "back", &tmp.path().join("gone.png"), false, now + 106.0).unwrap_err(), "That file can't be opened.");
    assert!(g.current[1].is_none());
}

#[test]
fn wikiart_matches_electron() {
    use rustyshelf_core::wikiart;
    let fx = fixture("wikiart.json");
    let dir = tempfile::tempdir().unwrap();
    let now = 1_800_000_000_000.0;
    let answers: serde_json::Map<String, Value> = fx["answers"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), serde_json::json!([now - v[0].as_f64().unwrap() * 86_400_000.0, v[1]]))).collect();
    wikiart::keep(dir.path(), &answers).unwrap();
    let no_web = |w: &str| -> Result<Value, String> { panic!("asked the web for {w}") };
    for c in fx["finds"].as_array().unwrap() {
        let names: Vec<String> = c["names"].as_array().unwrap().iter().map(|n| n.as_str().unwrap().to_string()).collect();
        let found = wikiart::find(dir.path(), c["sys"].as_str().unwrap(), &names, c["part"].as_str().unwrap(), c["region"].as_str().unwrap(), now, &no_web);
        let got = found.map_or(Value::Null, |f| serde_json::json!({ "url": f.url, "name": f.name, "ext": f.ext }));
        assert_eq!(got, c["found"], "{c}");
    }
    for r in fx["regions"].as_array().unwrap() {
        let got: Vec<i32> = ["usa", "eur", "jpn", ""].iter().map(|w| wikiart::region_score(r[0].as_str().unwrap(), w)).collect();
        assert_eq!(serde_json::json!(got), r[1], "{r}");
    }
    let old = serde_json::json!({ "zeta run": [now - 8.0 * 86_400_000.0, []] });
    wikiart::keep(dir.path(), old.as_object().unwrap()).unwrap();
    let asked = std::cell::Cell::new(0);
    let web = |_: &str| -> Result<Value, String> {
        asked.set(asked.get() + 1);
        Ok(serde_json::json!({ "query": { "pages": { "1": { "title": "File:Zeta Run box NA.png", "imageinfo": [{ "url": "https://mario.wiki.gallery/images/z.png", "mime": "image/png", "width": 600, "height": 900 }] } } } }))
    };
    let found = wikiart::find(dir.path(), "sw2", &["Zeta Run".into()], "front", "usa", now, &web).unwrap();
    assert_eq!((found.ext.as_str(), asked.get()), ("png", 1));
    wikiart::find(dir.path(), "sw2", &["Zeta Run".into()], "front", "usa", now + 1000.0, &web).unwrap();
    assert_eq!(asked.get(), 1, "a week's answer is kept");
    let wiki = |part: &str| wikiart::find(dir.path(), "sw2", &["Zeta Run".into()], part, "usa", now, &no_web);
    let from = rustyshelf_core::artstudio::Sources { wiki: Some(&wiki), region: "usa", ..Default::default() };
    let got: Vec<Value> = rustyshelf_core::artstudio::candidates("sw2", "Zeta Run", Path::new("/roms/Zeta Run.nsp"), "front", &from).iter().map(rustyshelf_core::artstudio::Cand::to_json).collect();
    assert_eq!(got.len(), 1);
    assert_eq!((&got[0]["source"], &got[0]["label"], &got[0]["type"], &got[0]["ext"]), (&serde_json::json!("mariowiki"), &serde_json::json!("Super Mario Wiki · Zeta Run box NA"), &serde_json::json!("Box - Front"), &serde_json::json!(".png")));
    assert!(rustyshelf_core::artstudio::candidates("switch", "Zeta Run", Path::new("/roms/Zeta Run.nsp"), "front", &from).is_empty());
}

#[test]
fn provenance_matches_electron() {
    use rustyshelf_core::provenance;
    let fx = fixture("provenance.json");
    let tmp = tempfile::tempdir().unwrap();
    let (data, art) = (tmp.path().join("data"), tmp.path().join("roms").join("covers"));
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(&art).unwrap();
    let t = fx["t"].as_u64().unwrap();
    for (f, v) in fx["files"].as_object().unwrap() {
        let n = v[0].as_u64().unwrap() as u8;
        let p = art.join(f);
        fs::write(&p, vec![n; 100 + n as usize]).unwrap();
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_millis(t + v[1].as_u64().unwrap());
        fs::File::options().write(true).open(&p).unwrap().set_modified(at).unwrap();
    }
    let root = tmp.path().to_string_lossy().to_lowercase();
    let put = |v: &Value| -> Value { serde_json::from_str(&v.to_string().replace("<tmp>", &root)).unwrap() };
    let back = |v: Value| -> Value { serde_json::from_str(&v.to_string().replace(&root, "<tmp>")).unwrap() };
    fs::write(data.join("art-provenance.json"), put(&fx["prov"]).to_string()).unwrap();
    provenance::init(&data);
    for o in fx["origins"].as_array().unwrap() {
        let p = art.join(o[0].as_str().unwrap());
        let got = serde_json::json!([o[0], provenance::origin_of(&p), provenance::may_replace(&p, false), provenance::may_replace(&p, true), provenance::is_poster(&p)]);
        assert_eq!(&got, o);
    }
    provenance::copied(&art.join("auto.png"), &art.join("hand.png"));
    provenance::copied(&art.join("own.png"), &art.join("touched.png"));
    provenance::copied(&art.join("legacy.png"), &art.join("late.png"));
    provenance::flush();
    let mut saved: Value = serde_json::from_slice(&fs::read(data.join("art-provenance.json")).unwrap()).unwrap();
    // (other tests place pictures at the same time: theirs are on record too)
    for m in ["files", "base"] {
        saved[m].as_object_mut().unwrap().retain(|k, _| k.starts_with(&root));
    }
    assert_eq!(back(saved).to_string(), fx["after"].to_string());
    // the first look is made once: a picture with the very bytes of one the Art Studio downloaded is RustyShelf's
    let other = tempfile::tempdir().unwrap();
    let (d2, cand) = (other.path().join("data"), other.path().join("data").join("art-cand"));
    fs::create_dir_all(&cand).unwrap();
    let (a, b) = (other.path().join("a.png"), other.path().join("b.png"));
    fs::write(&a, b"one picture").unwrap();
    fs::write(&b, b"another one").unwrap();
    fs::write(cand.join("0123.png"), b"one picture").unwrap();
    provenance::init(&d2);
    assert!(!provenance::looked());
    assert_eq!(provenance::first_look(&[a.clone(), b.clone(), b.clone()], &cand, 5), (1, 1));
    assert_eq!((provenance::origin_of(&a), provenance::origin_of(&b), provenance::may_replace(&b, true)), (Some("rustyshelf"), Some("legacy"), true));
    assert_eq!(provenance::first_look(&[a], &cand, 6), (0, 0));
    let kept: Value = serde_json::from_slice(&fs::read(d2.join("art-provenance.json")).unwrap()).unwrap();
    assert_eq!(kept["at"], 5);
}

#[test]
fn art_sets_match_electron() {
    use rustyshelf_core::artstudio::{style_for, Cand};
    let fx = fixture("artsets.json");
    let cand = |v: &Value| Cand {
        key: v["key"].as_str().unwrap().into(),
        url: v["url"].as_str().unwrap().into(),
        ext: v["ext"].as_str().unwrap().into(),
        source: v["source"].as_str().unwrap().into(),
        region: v["region"].as_str().unwrap().into(),
        edition: v["edition"].as_str().unwrap().into(),
        kind: v["kind"].as_str().unwrap().into(),
        label: v["label"].as_str().unwrap().into(),
        w: v["w"].as_u64().map(|n| n as u32),
        h: v["h"].as_u64().map(|n| n as u32),
        fan: v["fan"] == true,
        guess: v["guess"] == true,
        cached: v["cached"] == true,
        dup: v["dup"].as_str().map(String::from),
        cut: v["cut"].as_str().map(String::from),
        file: v["file"].as_str().map(std::path::PathBuf::from),
        ..Cand::default()
    };
    let lists: [Vec<Cand>; 4] = ["front", "back", "spine", "disc"].map(|p| fx["lists"][p].as_array().unwrap().iter().map(cand).collect());
    for c in fx["cases"].as_array().unwrap() {
        let style = style_for(&serde_json::json!({ "ui": c["ui"] }), "gc");
        let got: Vec<Value> = rustyshelf_core::artjob::build_sets(&style, c["region"].as_str().unwrap(), &lists)
            .iter()
            .map(|s| serde_json::json!({ "id": s.id, "label": s.label, "region": s.region, "source": s.source, "hd": s.hd, "same": s.same, "parts": s.parts.iter().map(|p| p.key.clone()).collect::<Vec<_>>() }))
            .collect();
        assert_eq!(Value::from(got), c["sets"], "{}", c["ui"]);
    }
}

#[test]
fn manual_studio_ranks_as_electron() {
    use rustyshelf_core::manualstudio as ms;
    let fx = fixture("manualstudio.json");
    for q in fx["quality"].as_array().unwrap() {
        let (k, label) = ms::quality(q[0].as_u64().unwrap() as u32, q[1].as_f64().unwrap());
        assert_eq!(serde_json::json!({ "q": k, "label": label }), q[2], "{q}");
    }
    for e in fx["expect"].as_array().unwrap() {
        let list: Vec<ms::Cand> = e[0].as_array().unwrap().iter().map(ms::from_json).collect();
        assert_eq!(ms::expect_json(&ms::expect_of(&list)), e[1]);
    }
    let cands: Vec<ms::Cand> = fx["cands"].as_array().unwrap().iter().map(ms::from_json).collect();
    let expect: std::collections::BTreeMap<String, u32> = fx["expectUsed"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_u64().unwrap() as u32)).collect();
    for r in fx["ranks"].as_array().unwrap() {
        let pref = r[0].as_str().unwrap();
        for (c, want) in cands.iter().zip(r[1].as_array().unwrap()) {
            let (complete, missing, text) = ms::completeness(c, &expect);
            assert_eq!(ms::rank(c, pref, &expect), want[0].as_f64().unwrap(), "{pref} {}", c.key);
            assert_eq!(serde_json::json!({ "complete": complete, "missing": missing, "text": text }), want[1], "{}", c.key);
        }
    }
}

#[test]
fn steam_achievements_match_electron() {
    use rustyshelf_core::steamach::{combine, steam_id, SteamAch};
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    let fx = fixture("steamach.json");
    let tmp = tempfile::tempdir().unwrap();
    let put = |rel: &str, text: &str| {
        let f = tmp.path().join(rel);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, text).unwrap();
    };
    put("steam/config/loginusers.vdf", "\u{feff}\"users\"\n{\n\t\"76561197960265999\"\n\t{\n\t\t\"AccountName\"\t\"other\"\n\t\t\"MostRecent\"\t\"0\"\n\t\t\"Timestamp\"\t\"1700000000\"\n\t}\n\t\"76561197960265851\"\n\t{\n\t\t\"AccountName\"\t\"madeup\"\n\t\t\"MostRecent\"\t\"1\"\n\t\t\"Timestamp\"\t\"1600000000\"\n\t}\n\t\"123\"\n\t{\n\t}\n}\n");
    for d in ["0", "77", "notanid"] {
        put(&format!("bare/userdata/{d}/config/localconfig.vdf"), "");
    }
    assert_eq!(Value::from(steam_id(&tmp.path().join("steam"))), fx["ids"]["main"]);
    assert_eq!(Value::from(steam_id(&tmp.path().join("bare"))), fx["ids"]["bare"]);
    let key = fx["key"].as_str().unwrap().to_string();
    let site_of = |site: Value, asked: Arc<Mutex<Vec<String>>>, key: String| -> rustyshelf_core::steamach::Fetch {
        Box::new(move |u: &str| {
            asked.lock().unwrap().push(u.replace(&key, "KEY"));
            let name = u.split('/').nth(4).unwrap_or("");
            let id = u.split(['?', '&']).find_map(|p| p.strip_prefix("appid=").or_else(|| p.strip_prefix("gameid="))).unwrap_or("");
            let a = site.get(format!("{name}-{id}")).or_else(|| site.get(name)).cloned().unwrap_or(json!({ "s": 404, "j": null }));
            Ok((a["s"].as_u64().unwrap() as u16, Some(a["j"].clone()).filter(|j| !j.is_null())))
        })
    };
    let asked = Arc::new(Mutex::new(Vec::new()));
    let now = fx["now"].as_f64().unwrap();
    let root = Some(tmp.path().join("steam"));
    let sa = SteamAch::with(&tmp.path().join("data"), &format!(" {key} "), root.clone(), site_of(fx["site"].clone(), asked.clone(), key.clone()));
    let got = sa.trophies(false, now).unwrap().unwrap();
    assert_eq!(got, fx["trophies"]);
    let tests: Vec<Value> = [key.clone(), "short".into(), format!(" {} ", key.to_lowercase())].iter().map(|k| sa.test(k)).collect();
    assert_eq!(Value::from(tests), fx["tests"]);
    // the games are asked four at a time, so in any order
    let mut a = asked.lock().unwrap().clone();
    let mut want: Vec<String> = fx["asked"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    a.sort();
    want.sort();
    assert_eq!(a, want);
    // asked again within the cache's time: only the icons that failed are asked again
    sa.trophies(false, now).unwrap();
    assert_eq!(asked.lock().unwrap()[want.len()..], ["https://api.steampowered.com/ISteamUserStats/GetSchemaForGame/v2/?key=KEY&appid=20&l=english"]);
    let mut site = fx["site"].clone();
    for (k, v) in fx["priv"].as_object().unwrap() {
        site[k] = v.clone();
    }
    let p = SteamAch::with(&tmp.path().join("data2"), &key, root, site_of(site, Arc::default(), key.clone()));
    assert_eq!(Value::from(p.trophies(false, now).err()), fx["private"]);
    let c = &fx["combined"];
    let ra = c["ra2"].clone();
    let mut ra_blank = ra.clone();
    (ra_blank["user"], ra_blank["pic"]) = (json!(""), Value::Null);
    assert_eq!(combine(Some(ra.clone()), Some(got.clone()), &[]).unwrap(), c["both"]);
    assert_eq!(combine(None, Some(got), &["RetroAchievements: down".into()]).unwrap(), c["steam"]);
    assert_eq!(combine(Some(ra_blank), None, &[]).unwrap(), c["ra"]);
    assert_eq!(Value::from(combine(None, None, &["why".into()]).err()), c["none"]);
    assert_eq!(Value::from(combine(None, None, &[]).err()), c["nothing"]);
}
