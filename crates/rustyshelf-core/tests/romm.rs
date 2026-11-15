//! The RomM client against the Electron app's pretend server (linux/ci/fake-romm.js, through romm-server.js), as
//! linux/ci/romm-test.js runs main/romm.js against it. Skipped where node isn't installed.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use rustyshelf_core::romm::{map_slug, Error, Phase, Poll, Romm, Timeouts};
use serde_json::{json, Value};
use sha1::Digest;

struct Fake {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    url: String,
}

impl Fake {
    fn start() -> Option<Fake> {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/romm-server.js");
        let mut child = Command::new("node").arg(script).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().ok()?;
        let input = child.stdin.take()?;
        let mut output = BufReader::new(child.stdout.take()?);
        let mut line = String::new();
        output.read_line(&mut line).ok()?;
        let url = serde_json::from_str::<Value>(&line).ok()?["url"].as_str()?.to_string();
        Some(Fake { child, input, output, url })
    }

    fn cmd(&mut self, c: Value) -> Value {
        writeln!(self.input, "{c}").unwrap();
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }

    fn set(&mut self, key: &str, value: Value) {
        self.cmd(json!({ "op": "set", "key": key, "value": value }));
    }

    fn log(&mut self, prefix: &str) -> Vec<Value> {
        self.cmd(json!({ "op": "log", "prefix": prefix })).as_array().cloned().unwrap_or_default()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn sha1_of(p: &Path) -> String {
    sha1::Sha1::digest(std::fs::read(p).unwrap()).iter().map(|b| format!("{b:02x}")).collect()
}

fn pages(log: &[Value]) -> Vec<&Value> {
    log.iter().filter(|l| l["query"]["with_rom_id_index"] != "true").collect()
}

#[test]
fn against_the_pretend_server() {
    let Some(mut fake) = Fake::start() else {
        eprintln!("node isn't installed: the RomM test is skipped");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let (data, roms) = (tmp.path().join("data"), tmp.path().join("roms"));
    let ms = Duration::from_millis;
    let t = Timeouts { api: ms(1500), page: ms(1500), cover: ms(1500), connect: ms(1500), idle: ms(1500) };
    let romm = Romm::new(&data);
    romm.set_time_scale(0.01);
    romm.set_timeouts(t);
    assert_eq!(romm.set_server(&format!("{}/api/", fake.url), false).unwrap(), fake.url);

    // pairing
    let none = romm.library(&mut |_| {}, None);
    assert_eq!((none.error, none.count), (Some(Error::NoToken), 0));
    let t0 = Instant::now();
    let st = romm.pair_start("RustyShelf test").unwrap();
    assert_eq!(st.user_code, "WDJB-MJHT");
    assert_eq!(st.qr_url, format!("{}/pair?user_code=WDJB-MJHT", fake.url));
    let init = fake.log("/api/auth/device/init");
    assert_eq!(init.len(), 1);
    assert!(matches!(romm.pair_poll(), Poll::Pending { wait, .. } if wait > 0));
    assert!(fake.log("/api/auth/device/token").is_empty(), "a poll before the interval asks nothing");
    let Poll::Signed { token } = romm.pair_wait(None) else { panic!("not signed in") };
    assert!(token.starts_with("rmm_"));
    let polls: Vec<f64> = fake.log("/api/auth/device/token").iter().map(|l| l["t"].as_f64().unwrap()).collect();
    assert_eq!(polls.len(), 3);
    // the interval is 5 s, slow_down adds 5 s (scaled 0.01)
    assert!(polls[1] - polls[0] >= 95.0 && polls[2] - polls[1] >= 95.0, "{polls:?}");
    assert!(t0.elapsed() >= ms(240));
    fake.set("deny", json!(true));
    romm.pair_start("").unwrap();
    assert_eq!(romm.pair_wait(None), Poll::Denied);
    fake.set("deny", json!(false));

    assert_eq!(romm.pair_with_code("0000-0000"), Err(Error::BadCode));
    assert!(romm.pair_with_code("1234 5678").unwrap().starts_with("rmm_"));
    let test = romm.test();
    assert_eq!((test.version.as_deref(), test.outcome), (Some("5.4.1"), Ok(4)));
    romm.set_token(Some("rmm_wrong"));
    assert_eq!(romm.test().outcome, Err(Error::Auth));
    romm.set_token(Some(&token));

    // platform -> console
    for (s, c) in [("ngc", Some("gc")), ("n64", Some("n64")), ("genesis-slash-megadrive", Some("megadrive")), ("psx", Some("psx")), ("ps", Some("psx")), ("dc", Some("dreamcast")), ("3ds", Some("n3ds")), ("switch", Some("sw")), ("xbox360", Some("x360")), ("sms", Some("mastersystem")), ("turbografx16--1", Some("pcengine")), ("lynx", Some("atarilynx")), ("philips-cd-i", None)] {
        assert_eq!(map_slug(s, "").as_deref(), c, "{s}");
    }
    assert_eq!(map_slug("something-new", "GameCube").as_deref(), Some("gc"));
    assert_eq!(map_slug("x", "super famicom").as_deref(), Some("snes"));

    // first sync
    fake.cmd(json!({ "op": "clear" }));
    let mut prog = 0;
    let lib = romm.library(&mut |_| prog += 1, None);
    let n = |l: &rustyshelf_core::romm::Shown, c: &str| l.consoles.get(c).map_or(0, Vec::len);
    assert!(!lib.offline && lib.error.is_none(), "{:?}", lib.error);
    assert_eq!((n(&lib, "nes"), n(&lib, "snes"), n(&lib, "gc"), lib.count), (12, 10, 6, 28));
    assert_eq!((lib.skipped.platforms, lib.skipped.games, lib.skipped.slugs.clone()), (1, 2, vec!["philips-cd-i".to_string()]));
    let g1 = lib.consoles["nes"].iter().find(|g| g.romm_id == 1).unwrap().clone();
    assert_eq!((g1.id.as_str(), g1.title.as_str(), g1.file.as_deref()), ("romm-1", "NES Game 1", Some("NES Game 1 (USA).nes")));
    assert!(g1.size.unwrap() > 0 && g1.files.len() == 1 && g1.files[0].sha1.is_some() && g1.summary.is_some());
    assert_eq!((g1.released.as_deref(), g1.genres.as_deref(), g1.regions[0].as_str(), g1.cover.as_ref()), (Some("1990-01-02"), Some("Action"), Some("USA"), None));
    assert_eq!(g1.cover_url.as_deref(), Some(format!("{}/assets/romm/resources/roms/1/1/cover/big.png?ts=1767225600000", fake.url).as_str()));
    let g2 = lib.consoles["nes"].iter().find(|g| g.romm_id == 2).unwrap();
    assert!(g2.cover_url.as_ref().unwrap().starts_with(&format!("{}/assets/romm/resources/roms/1/2/cover/big.png?ts=", fake.url)));
    let multi = lib.consoles["gc"].iter().find(|g| g.multi).unwrap().clone();
    assert_eq!((multi.romm_id, multi.files.len()), (28, 2));
    let log = fake.log("/api/roms");
    let p1 = pages(&log);
    assert!(p1.len() == 3 && p1.iter().all(|l| l["query"].get("updated_after").is_none() && l["query"]["with_total"] == "true" && l["query"]["limit"] == "1000"), "{p1:?}");
    assert!(prog >= 3);
    assert!(data.join("romm-library.json").exists());

    // only what changed
    fake.cmd(json!({ "op": "touch", "id": 2, "fields": { "name": "NES Game 2 Deluxe" } }));
    fake.cmd(json!({ "op": "touch", "id": 15, "fields": { "summary": "A new summary." } }));
    fake.cmd(json!({ "op": "remove", "id": 7 }));
    fake.cmd(json!({ "op": "add", "rom": { "platform_id": 2, "name": "SNES Game New", "fs_name": "SNES Game New (USA).sfc" } }));
    fake.cmd(json!({ "op": "clear" }));
    let lib2 = romm.library(&mut |_| {}, None);
    let log = fake.log("/api/roms");
    let p2 = pages(&log);
    let items: i64 = p2.iter().map(|l| l["items"].as_i64().unwrap_or(0)).sum();
    assert!(p2.len() == 3 && p2.iter().all(|l| l["query"]["updated_after"].is_string()) && items == 3, "{p2:?}");
    assert_eq!(lib2.consoles["nes"].iter().find(|g| g.romm_id == 2).unwrap().title, "NES Game 2 Deluxe");
    assert_eq!(lib2.consoles["snes"].iter().find(|g| g.romm_id == 15).unwrap().summary.as_deref(), Some("A new summary."));
    assert!(n(&lib2, "snes") == 11 && lib2.consoles["snes"].iter().any(|g| g.title == "SNES Game New"));
    assert!(n(&lib2, "nes") == 11 && !lib2.consoles["nes"].iter().any(|g| g.romm_id == 7));
    assert!(log.iter().any(|l| l["query"]["with_rom_id_index"] == "true" && l["query"]["limit"] == "1"));
    fake.cmd(json!({ "op": "clear" }));
    let lib3 = romm.library(&mut |_| {}, None);
    let log = fake.log("/api/roms");
    assert_eq!((lib3.fetched, lib3.count, pages(&log).iter().map(|l| l["items"].as_i64().unwrap_or(0)).sum::<i64>()), (0, 28, 0));

    // offline and stuck
    fake.set("offline", json!(true));
    let off = romm.library(&mut |_| {}, None);
    assert!(off.offline && off.count == 28 && n(&off, "nes") == 11);
    fake.set("offline", json!(false));
    fake.set("hang", json!(true));
    let t = Instant::now();
    let hung = romm.library(&mut |_| {}, None);
    assert!(hung.offline && hung.count == 28 && t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    fake.set("hang", json!(false));
    fake.set("offline", json!(true));
    let again = Romm::new(&data);
    again.set_server(&fake.url, false).unwrap();
    assert_eq!(again.cached().count, 28, "the saved copy after a restart");
    fake.set("offline", json!(false));

    // covers
    let all: Vec<_> = romm.library(&mut |_| {}, None).consoles.into_values().flatten().collect();
    let with_cover = all.iter().filter(|g| g.cover_url.is_some()).count();
    fake.cmd(json!({ "op": "clear" }));
    let cv1 = romm.covers(&all, 100, 3, None);
    let reqs = fake.log("/assets/");
    assert!(with_cover > 15 && cv1.downloaded == with_cover && reqs.len() == with_cover, "{} of {with_cover}", cv1.downloaded);
    assert!(cv1.paths.values().all(|p| p.exists() && p.parent().unwrap().ends_with("romm-covers")));
    assert!(reqs.iter().all(|l| l["auth"] == false), "covers get no token");
    fake.cmd(json!({ "op": "clear" }));
    let cv2 = romm.covers(&all, 100, 3, None);
    assert!(cv2.downloaded == 0 && fake.log("/assets/").is_empty() && cv2.paths.len() == with_cover);
    let cover = romm.cached().consoles["nes"].iter().find(|g| g.romm_id == 1).unwrap().cover.clone().unwrap();
    assert!(cover.ends_with("romm-covers/1.png"));
    let more: Vec<_> = all.iter().map(|g| rustyshelf_core::romm::Game { romm_id: g.romm_id + 1000, id: format!("x{}", g.id), ..g.clone() }).collect();
    assert_eq!(romm.covers(&more, 2, 3, None).downloaded, 2);

    // downloads
    let nes = roms.join("nes");
    let game1 = lib2.consoles["nes"].iter().find(|g| g.romm_id == 1).unwrap();
    let mut steps = Vec::new();
    let d1 = romm.download(game1, &nes, &mut |p| steps.push(p), None).unwrap();
    assert_eq!(d1.file, nes.join("NES Game 1 (USA).nes"));
    assert_eq!((std::fs::metadata(&d1.file).unwrap().len(), d1.verified.as_deref()), (game1.size.unwrap(), Some("sha1")));
    assert_eq!(sha1_of(&d1.file), *game1.files[0].sha1.as_ref().unwrap());
    let dl: Vec<_> = steps.iter().filter(|s| s.phase == Phase::Download).collect();
    assert!(dl.len() >= 2 && dl.last().unwrap().received == game1.size.unwrap() && dl.windows(2).all(|w| w[1].received >= w[0].received));
    assert!(steps.iter().any(|s| s.phase == Phase::Verify));
    assert!(romm.download(game1, &nes, &mut |_| {}, None).unwrap().already);

    let big = lib2.consoles["nes"].iter().find(|g| g.romm_id == 5).unwrap();
    fake.set("chunkDelay", json!(8));
    let stop = AtomicBool::new(false);
    let size = big.size.unwrap();
    let cut = romm.download(big, &nes, &mut |p| if p.phase == Phase::Download && p.received > size * 3 / 10 { stop.store(true, std::sync::atomic::Ordering::Relaxed) }, Some(&stop));
    let part = nes.join(format!("{}.part", big.file.as_ref().unwrap()));
    let part_size = std::fs::metadata(&part).map_or(0, |m| m.len());
    assert_eq!(cut, Err(Error::Aborted));
    assert!(part_size > 0 && part_size < size && !nes.join(big.file.as_ref().unwrap()).exists(), "{part_size}");
    fake.set("chunkDelay", json!(0));
    fake.cmd(json!({ "op": "clear" }));
    let resumed = romm.download(big, &nes, &mut |_| {}, None).unwrap();
    let content = fake.log("/api/roms/5/content/");
    assert!(content.len() == 1 && content[0]["range"] == format!("bytes={part_size}-") && resumed.resumed, "{content:?}");
    assert_eq!(sha1_of(&resumed.file), *big.files[0].sha1.as_ref().unwrap());

    for (id, algo) in [(2, "sha1"), (3, "crc"), (4, "md5")] {
        let g = lib2.consoles["nes"].iter().find(|x| x.romm_id == id).unwrap();
        fake.cmd(json!({ "op": "corrupt", "id": id, "on": true }));
        let e = romm.download(g, &nes, &mut |_| {}, None);
        assert!(matches!(e, Err(Error::HashMismatch(_))), "{algo}: {e:?}");
        assert!(!nes.join(g.file.as_ref().unwrap()).exists() && !nes.join(format!("{}.part", g.file.as_ref().unwrap())).exists());
        fake.cmd(json!({ "op": "corrupt", "id": id, "on": false }));
        assert_eq!(romm.download(g, &nes, &mut |_| {}, None).unwrap().verified.as_deref(), Some(algo));
    }

    let gc = roms.join("gc");
    let md = romm.download(&multi, &gc, &mut |_| {}, None).unwrap();
    let folder = gc.join(multi.file.as_ref().unwrap());
    assert!(md.extracted && md.dir.as_ref() == Some(&folder) && md.file == folder.join("GC Game 6 (USA).cue") && md.files.len() == 2);
    assert_eq!(std::fs::metadata(folder.join("GC Game 6 (USA).bin")).unwrap().len(), 150_000);
    assert!(!gc.join(format!("{}.zip", multi.file.as_ref().unwrap())).exists() && !gc.join(format!("{}.unpacking", multi.file.as_ref().unwrap())).exists());
    assert_eq!(md.verified.as_deref(), Some("files"));
    assert_eq!(sha1_of(&folder.join("GC Game 6 (USA).bin")), *multi.files[1].sha1.as_ref().unwrap());
    std::fs::remove_dir_all(&folder).unwrap();
    fake.cmd(json!({ "op": "corrupt", "id": 28, "on": true }));
    assert!(matches!(romm.download(&multi, &gc, &mut |_| {}, None), Err(Error::HashMismatch(_))));
    assert!(!folder.exists());
    fake.cmd(json!({ "op": "corrupt", "id": 28, "on": false }));

    // a zip entry outside the folder
    use base64::Engine;
    let evil = tmp.path().join("evil.zip");
    let z = fake.cmd(json!({ "op": "zip", "entries": [{ "name": "../escape.txt", "data": [120] }] }));
    std::fs::write(&evil, base64::engine::general_purpose::STANDARD.decode(z.as_str().unwrap()).unwrap()).unwrap();
    let e = rustyshelf_core::romm::unzip(&evil, &tmp.path().join("evilout"), &|_| None, &mut |_| {}, None);
    assert!(matches!(e, Err(Error::ZipBad(_))) && !tmp.path().join("escape.txt").exists());

    // sign out
    romm.forget();
    let gone = romm.library(&mut |_| {}, None);
    assert_eq!((gone.error, gone.count), (Some(Error::NoToken), 0));
    assert!(!data.join("romm-library.json").exists() && !data.join("romm-covers").exists());
}
