//! The downloads against the real servers. Not part of a normal test run:
//!   cargo test -p rustyshelf-core --test network -- --ignored

#[test]
#[ignore = "needs the Internet"]
fn lists_download() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(rustyshelf_core::arcade::fetch(tmp.path()), Ok(true));
    assert_eq!(rustyshelf_core::arcade::fetch(tmp.path()), Ok(false));
    assert!(rustyshelf_core::arcade::Names::load(tmp.path()).is_some());
    assert_eq!(rustyshelf_core::x360::refresh(tmp.path()), Ok(true));
    assert_eq!(rustyshelf_core::x360::refresh(tmp.path()), Ok(false));
    let size = |f: &str| std::fs::metadata(tmp.path().join(f)).unwrap().len();
    assert!(size("arcade-names.json") > 100_000 && size("x360-titles.json") > 10_000);
}

#[test]
#[ignore = "needs the Internet"]
fn steam_art_downloads() {
    use rustyshelf_core::steamart::{fill, Wanted};
    let tmp = tempfile::tempdir().unwrap();
    // an app with art on Steam's CDN, and a number no app has
    let games = [Wanted { appid: "620".into(), has_front: false, has_hero: false }, Wanted { appid: "4294967000".into(), has_front: false, has_hero: false }];
    let (covers, other) = fill(tmp.path(), &games);
    assert_eq!((covers, other), (1, 2));
    let there = |f: &str| tmp.path().join(f).exists();
    assert!(there("steam-art/620_cover.jpg") && there("steam-art/620_hero.jpg") && there("steam-art/4294967000.missing") && there("steam-art/4294967000_hero.missing"));
    let info: serde_json::Value = serde_json::from_slice(&std::fs::read(tmp.path().join("steam-info.json")).unwrap()).unwrap();
    assert_eq!((info["620"]["ok"].as_bool(), info["4294967000"]["ok"].as_bool()), (Some(true), Some(false)));
    assert!(info["620"]["release"].as_str().is_some_and(|r| r.len() == 10) && info["620"]["shots"].as_array().is_some_and(|s| !s.is_empty()));
    assert_eq!(fill(tmp.path(), &games), (0, 0));

    let titles = [info["620"]["name"].as_str().unwrap().to_string(), "zzqx no such game zzqx".to_string()];
    assert_eq!(rustyshelf_core::pcgames::match_steam(tmp.path(), &titles), 1);
    assert_eq!(rustyshelf_core::pcgames::steam_ids(tmp.path()).get(&titles[0].to_lowercase()).map(String::as_str), Some("620"));
    assert_eq!(rustyshelf_core::pcgames::match_steam(tmp.path(), &titles), 0);
}

#[test]
#[ignore = "needs the Internet, and downloads the 110 MB LaunchBox database"]
fn box_art_downloads() {
    use rustyshelf_core::art::ArtLibrary;
    use rustyshelf_core::boxart::{art_file, download, libretro_names, ready_index, size_of, Run, Wanted};
    use rustyshelf_core::config::Config;
    use rustyshelf_core::scanner::{scan, ScanData};
    use rustyshelf_core::systems::systems;
    let tmp = tempfile::tempdir().unwrap();
    let (root, cache) = (tmp.path().to_path_buf(), tmp.path().join("data/boxart-cache"));
    let console = systems().console("snes").unwrap();
    // two games named as libretro's list names them, whatever it holds today
    let names = libretro_names(&cache, "snes").expect("libretro's list");
    assert!(!names.is_empty() && cache.join("libretro-snes.json").exists());
    let listed: serde_json::Value = serde_json::from_slice(&std::fs::read(cache.join("libretro-snes.json")).unwrap()).unwrap();
    let roms = root.join("roms");
    std::fs::create_dir_all(&roms).unwrap();
    for n in listed["names"].as_array().unwrap().iter().filter_map(|n| n.as_str()).filter(|n| n.contains("(USA)") && !n.contains(['/', '_'])).take(2) {
        std::fs::write(roms.join(format!("{n}.sfc")), b"x").unwrap();
    }
    let cfg = Config::parse(&serde_json::json!({ "systems": { "snes": { "romDirs": [roms.to_string_lossy()] } } }).to_string()).unwrap();
    let (sc, art) = (cfg.system(console), ArtLibrary::from_config(&cfg, &root));
    let games = scan(console, &sc, &art.dirs_for(&sc, &root), &ScanData::default());
    assert_eq!(games.len(), 2);
    let stages = std::sync::Mutex::new(Vec::new());
    let (wanted, off) = ([Wanted { console, config: &sc, games: games.iter().collect() }], std::sync::atomic::AtomicBool::new(false));
    let run = Run { art: &art, app_root: &root, cache_dir: &cache, kinds: &["front", "back"], region: "auto" };
    let report = download(&run, &wanted, &|stage, done, total| stages.lock().unwrap().push(format!("{stage} {done}/{total}")), &off);
    println!("{report:?}\n{:?}", stages.lock().unwrap().iter().filter(|s| !s.contains('%')).collect::<Vec<_>>());
    assert_eq!((report.launchbox_error.clone(), report.systems[0].front), (None, 2));
    assert!(ready_index(&cache).is_some() && !cache.join("Metadata.xml").exists() && !cache.join("Metadata.zip").exists());
    for g in &games {
        let front = art_file(&roms.join("covers"), &rustyshelf_core::art::art_base(&g.file), "").expect("a front");
        assert!(size_of(&front).is_some_and(|z| z.0 > 50 && z.1 > 50));
    }
    // nothing is fetched twice
    let again = download(&Run { kinds: &["front"], ..run }, &wanted, &|_, _, _| {}, &off);
    assert_eq!((again.systems[0].front, again.systems[0].had), (0, 2));
}
