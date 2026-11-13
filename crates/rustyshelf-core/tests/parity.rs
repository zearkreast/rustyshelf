//! The Rust port against the Electron app's own answers (tests/fixtures, made by scripts/rust-fixtures.js).

use std::cmp::Ordering;
use std::fs;
use std::path::Path;

use rustyshelf_core::config::SystemConfig;
use rustyshelf_core::scanner::scan;
use rustyshelf_core::systems::systems;
use rustyshelf_core::titles::clean_title;
use rustyshelf_core::{art::ArtLibrary, collate, config::Config};
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

#[test]
fn scans_match_the_js_scanner() {
    for case in fixture("scan.json").as_array().unwrap() {
        let id = case["console"].as_str().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let games = tmp.path().join("games");
        for f in case["files"].as_array().unwrap() {
            let p = games.join(f[0].as_str().unwrap());
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, vec![0u8; f[1].as_u64().unwrap() as usize]).unwrap();
        }
        for f in case["art"].as_array().unwrap() {
            let p = games.join(f.as_str().unwrap());
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, b"").unwrap();
        }
        let console = systems().console(id).unwrap();
        let sc = SystemConfig { rom_dirs: vec![games.to_string_lossy().into_owned()], ..Config::default().system(console) };
        let dirs = ArtLibrary::default().dirs_for(&sc, tmp.path());
        let list = scan(console, &sc, &dirs);
        assert!(list.windows(2).all(|w| collate::compare(&w[0].title, &w[1].title) != Ordering::Greater), "{id}: not in title order");

        // readdir order varies by file system and decides equal titles' order, disc order and which dump a single-disc
        // game's `discs` names: those are compared order-free
        let mut got: Vec<Value> = list
            .iter()
            .map(|g| {
                let a = &g.art;
                serde_json::json!({
                    "title": g.title, "region": g.region, "disc": g.disc, "file": rel(&games, Some(&g.file)), "ext": g.ext,
                    "sizeBytes": g.size_bytes,
                    "discs": discs(g.discs.iter().map(|d| rel(&games, Some(d))).collect()),
                    "art": {
                        "front": rel(&games, a.front.as_deref()), "back": rel(&games, a.back.as_deref()), "madeBack": a.made_back,
                        "spine": rel(&games, a.spine.as_deref()), "madeSpine": a.made_spine, "disc": rel(&games, a.disc.as_deref()),
                        "manual": a.manual.iter().map(|m| rel(&games, Some(m))).collect::<Vec<_>>(), "manualPdf": rel(&games, a.manual_pdf.as_deref()),
                    },
                })
            })
            .collect();
        let mut want: Vec<Value> = case["games"].as_array().unwrap().clone();
        for g in &mut want {
            g["discs"] = discs(g["discs"].as_array().unwrap().clone());
        }
        got.sort_by_key(key);
        want.sort_by_key(key);
        assert_eq!(serde_json::to_string_pretty(&got).unwrap(), serde_json::to_string_pretty(&want).unwrap(), "{id}");
    }
}
