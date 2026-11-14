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
