// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/racores.js, as RustyShelf has it.
//! RetroArch cores and BIOS files. A console's core is downloaded from the libretro buildbot (the same files
//! RetroArch's own Online Updater gets) into the cores folder RetroArch reads; Windows only, Linux builds come with
//! their own updater. The BIOS files a core wants for a console are checked against libretro's System.dat
//! fingerprints: good, wrong version, or missing.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use md5::{Digest, Md5};
use regex::Regex;

use crate::js;
use crate::paths::is_file;

/// The folder RetroArch loads cores from: retroarch.cfg's `libretro_directory` (":" is RetroArch's own folder), else
/// `<RetroArch>\cores`.
pub fn cores_dir(exe: &str, var: &dyn Fn(&str) -> Option<String>) -> String {
    static DIR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?m)^\s*libretro_directory\s*=\s*"([^"]*)""#).unwrap());
    static OWN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^:[\\/]?").unwrap());
    let dir = js::dirname(exe);
    let appdata = var("APPDATA").unwrap_or_default();
    for f in [js::join(&[&dir, "retroarch.cfg"]), js::join(&[&appdata, "RetroArch", "retroarch.cfg"])] {
        let Ok(t) = std::fs::read(&f).map(|b| String::from_utf8_lossy(&b).into_owned()) else { continue };
        if let Some(m) = DIR.captures(&t).map(|m| m[1].to_string()).filter(|m| !m.is_empty() && m != "default") {
            return OWN.replace(&m, regex::NoExpand(&format!("{dir}{}", std::path::MAIN_SEPARATOR))).into_owned();
        }
        break;
    }
    js::join(&[&dir, "cores"])
}

fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let mut c = std::process::Command::new(cmd);
    c.args(args).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut c, 0x0800_0000); // CREATE_NO_WINDOW
    let out = c.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("{cmd} failed"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Downloads a core and gives its file. The zip must hold just that one .dll: anything else is refused, nothing is
/// unpacked. `on_pct`: how far the download is.
pub fn get(exe: &str, core: &str, var: &dyn Fn(&str) -> Option<String>, on_pct: &mut dyn FnMut(u32)) -> Result<PathBuf, String> {
    const URL_BASE: &str = "https://buildbot.libretro.com/nightly/windows/x86_64/latest/";
    if !cfg!(windows) {
        return Err("get it from RetroArch's Online Updater".into());
    }
    if core.is_empty() || !core.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err("unknown core".into());
    }
    let tmp = std::env::temp_dir().join(format!("rs-core-{}-{core}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let result = (|| {
        let (zip, want) = (tmp.join(format!("{core}.zip")), format!("{core}_libretro.dll"));
        crate::net::download(&format!("{URL_BASE}{want}.zip"), &zip, Duration::from_secs(300), on_pct)?;
        if !std::fs::read(&zip).is_ok_and(|b| b.starts_with(b"PK")) {
            return Err("the download wasn't a core".to_string());
        }
        let zip_s = zip.to_string_lossy().into_owned();
        let list: Vec<String> = run("tar", &["-tf", &zip_s])?.lines().map(|l| js::trim(l).to_string()).filter(|l| !l.is_empty()).collect();
        if list != [want.clone()] {
            return Err("the download held something other than the core".into());
        }
        let unpack = tmp.join("x");
        std::fs::create_dir_all(&unpack).map_err(|e| e.to_string())?;
        run("tar", &["-xf", &zip_s, "-C", &unpack.to_string_lossy()])?;
        let got = unpack.join(&want);
        if !is_file(&got) {
            return Err("the core wasn't in the download".into());
        }
        let dir = cores_dir(exe, var);
        let dest = js::join(&[&dir, &want]);
        let part = format!("{dest}.part");
        std::fs::create_dir_all(&dir).and_then(|()| std::fs::copy(&got, &part)).and_then(|_| std::fs::rename(&part, &dest)).map_err(|e| {
            let _ = std::fs::remove_file(&part);
            format!("RetroArch's cores folder can't be written ({e}): use RetroArch's Online Updater instead")
        })?;
        Ok(PathBuf::from(dest))
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

// ---------- BIOS ----------

/// How much a console needs its BIOS files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// any one of them (one per region)
    One,
    /// this file
    Required,
    /// better with it
    Optional,
}

struct Wanted {
    console: &'static str,
    need: Need,
    note: &'static str,
    /// name (in the BIOS folder), region, its MD5 in libretro's System.dat (None: any file of that name)
    files: &'static [(&'static str, &'static str, Option<&'static str>)],
}

/// main/racores.js BIOS, keep in sync.
const BIOS: &[Wanted] = &[
    Wanted {
        console: "psx",
        need: Need::One,
        note: "",
        files: &[
            ("scph5501.bin", "USA", Some("490f666e1afb15b7362b406ed1cea246")),
            ("scph5500.bin", "Japan", Some("8dd7d5296a650fac7319bce665a6a53c")),
            ("scph5502.bin", "Europe", Some("32736f17079d0b2b7024407c39bd3050")),
        ],
    },
    Wanted { console: "saturn", need: Need::One, note: "", files: &[("mpr-17933.bin", "USA & Europe", Some("3240872c70984b6cbfda1586cab68dbe")), ("sega_101.bin", "Japan", Some("85ec9ca47d8f6807718151cbcca8b964"))] },
    Wanted {
        console: "segacd",
        need: Need::One,
        note: "",
        files: &[
            ("bios_CD_U.bin", "USA", Some("2efd74e3232ff260e371b99f84024f7f")),
            ("bios_CD_E.bin", "Europe", Some("e66fa1dc5820d254611fdcdba0662372")),
            ("bios_CD_J.bin", "Japan", Some("278a9397d192149e84e820ac621a8edd")),
        ],
    },
    Wanted { console: "atarilynx", need: Need::Required, note: "", files: &[("lynxboot.img", "", Some("fcd403db69f54290b51035d82f835e7b"))] },
    Wanted { console: "pcengine", need: Need::Optional, note: "only CD games need it", files: &[("syscard3.pce", "", Some("38179df8f4ac870017db21ebcbf53114"))] },
    Wanted { console: "gba", need: Need::Optional, note: "games run without it", files: &[("gba_bios.bin", "", Some("a860e8c0b6d573d191e4ec7db1b1e4f6"))] },
    Wanted { console: "dreamcast", need: Need::Optional, note: "Flycast runs most games without it", files: &[("dc/dc_boot.bin", "", Some("e10c53c2f8b90bab96ead2d368858623"))] },
    Wanted { console: "neogeo", need: Need::Required, note: "in the games folder or the BIOS folder", files: &[("neogeo.zip", "", None)] },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Ok,
    /// a file of that name that isn't the one the core wants
    Wrong,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiosFile {
    pub name: &'static str,
    pub region: &'static str,
    pub state: State,
    pub at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bios {
    pub need: Need,
    pub note: &'static str,
    pub ok: bool,
    pub files: Vec<BiosFile>,
}

fn md5_of(f: &str) -> Option<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(f).ok()?;
    let (mut h, mut buf) = (Md5::new(), vec![0u8; 64 << 10]);
    loop {
        match file.read(&mut buf).ok()? {
            0 => break,
            n => h.update(&buf[..n]),
        }
    }
    Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The BIOS state for one console, looking in each of `dirs` (the first good file wins). None: the console needs
/// none.
pub fn bios(console_id: &str, dirs: &[String]) -> Option<Bios> {
    let t = BIOS.iter().find(|w| w.console == console_id)?;
    let mut files = Vec::new();
    for (name, region, sum) in t.files {
        let (mut at, mut state) = (None, State::Missing);
        for d in dirs.iter().filter(|d| !d.is_empty()) {
            let f = js::join(&[&[d.as_str()], name.split('/').collect::<Vec<_>>().as_slice()].concat());
            if !is_file(Path::new(&f)) {
                continue;
            }
            // nothing that big is a BIOS: not worth hashing
            let size = Path::new(&f).metadata().map_or(0, |m| m.len());
            state = match sum {
                None => State::Ok,
                Some(_) if size > 8 << 20 => State::Wrong,
                Some(sum) if md5_of(&f).as_deref() == Some(sum) => State::Ok,
                Some(_) => State::Wrong,
            };
            at = Some(f);
            if state == State::Ok {
                break;
            }
        }
        files.push(BiosFile { name, region, state, at });
    }
    let good = |f: &BiosFile| f.state == State::Ok;
    let ok = match t.need {
        Need::One => files.iter().any(good),
        Need::Required => files.iter().all(good),
        Need::Optional => true,
    };
    Some(Bios { need: t.need, note: t.note, ok, files })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("gba_bios.bin");
        std::fs::write(&f, b"abc").unwrap();
        assert_eq!(md5_of(&f.to_string_lossy()).as_deref(), Some("900150983cd24fb0d6963f7d28e17f72"));
        let b = bios("gba", &[String::new(), t.path().to_string_lossy().into_owned()]).unwrap();
        assert_eq!((b.ok, b.files[0].state, b.files[0].at.is_some()), (true, State::Wrong, true));
        assert_eq!(bios("snes", &[]), None);
    }
}
