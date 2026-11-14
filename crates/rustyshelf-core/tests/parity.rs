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
