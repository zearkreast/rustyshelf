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
