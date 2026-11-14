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
