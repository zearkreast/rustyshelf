// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/n3ds.js, as RustyShelf has it.
//! 3DS .cia files on the shelf. A .cia is an installer, not something Azahar boots. The first time one is played it's
//! installed into Azahar's own SD card folder the way Azahar does it (the title's TMD as `00000000.tmd`, each content
//! as `<content id>.app`), and the installed game is booted; after that it boots straight away. Only unencrypted
//! .cia files can be installed like this (Azahar has the same rule).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::art::resolve;
use crate::js;
use crate::paths::is_dir;

const ZERO: &str = "00000000000000000000000000000000";

fn align(n: u64) -> u64 {
    n.div_ceil(64) * 64
}

/// A TMD's signature block (type, signature, padding) by its type; the header follows it.
fn signature_size(kind: u32) -> Option<usize> {
    match kind {
        0x10000 | 0x10003 => Some(0x240),
        0x10001 | 0x10004 => Some(0x140),
        0x10002 | 0x10005 => Some(0x80),
        _ => None,
    }
}

fn read_at(f: &mut File, pos: u64, len: usize) -> std::io::Result<Vec<u8>> {
    let mut b = vec![0u8; len];
    f.seek(SeekFrom::Start(pos))?;
    let mut got = 0;
    while got < len {
        match f.read(&mut b[got..])? {
            0 => break,
            n => got += n,
        }
    }
    b.truncate(got);
    Ok(b)
}

struct Content {
    id: u32,
    index: u16,
    kind: u16,
    size: u64,
    /// in the .cia (contents listed but not in it take no room)
    present: bool,
    at: u64,
}

struct Cia {
    title_id: u64,
    tmd: Vec<u8>,
    contents: Vec<Content>,
}

type Failure = String;

fn io(e: std::io::Error) -> Failure {
    e.to_string()
}

/// The .cia's layout and its TMD (title id, contents).
fn parse(file: &Path) -> Result<Cia, Failure> {
    let be16 = |b: &[u8], at: usize| u16::from_be_bytes([b[at], b[at + 1]]);
    let be32 = |b: &[u8], at: usize| u32::from_be_bytes(b[at..at + 4].try_into().unwrap());
    let le32 = |b: &[u8], at: usize| u64::from(u32::from_le_bytes(b[at..at + 4].try_into().unwrap()));
    let mut f = File::open(file).map_err(io)?;
    let h = read_at(&mut f, 0, 0x2020).map_err(io)?;
    if h.len() < 0x2020 {
        return Err("not a CIA".into());
    }
    let (hdr, cert, tik, tmd_size) = (le32(&h, 0), le32(&h, 8), le32(&h, 12), le32(&h, 16));
    if hdr != 0x2020 || !(0xC4..=0x100000).contains(&tmd_size) {
        return Err("not a CIA".into());
    }
    let tmd_at = align(align(align(hdr) + cert) + tik);
    let tmd = read_at(&mut f, tmd_at, tmd_size as usize).map_err(io)?;
    let body = tmd.get(..4).and_then(|k| signature_size(be32(k, 0))).ok_or("unknown TMD signature")?;
    if tmd.len() < body + 0xA0 {
        return Err("short TMD".into());
    }
    let title_id = u64::from_be_bytes(tmd[body + 0x4C..body + 0x54].try_into().unwrap());
    let count = be16(&tmd, body + 0x9E) as usize;
    let mut at = align(tmd_at + tmd_size);
    let mut contents = Vec::with_capacity(count);
    for i in 0..count {
        let o = body + 0xC4 + 64 * 0x24 + i * 0x30;
        if o + 0x30 > tmd.len() {
            return Err("short TMD".into());
        }
        let c = Content {
            id: be32(&tmd, o),
            index: be16(&tmd, o + 4),
            kind: be16(&tmd, o + 6),
            size: u64::from_be_bytes(tmd[o + 8..o + 16].try_into().unwrap()),
            present: h[0x20 + (i >> 3)] & (0x80 >> (i & 7)) != 0,
            at,
        };
        if c.present {
            at += c.size;
        }
        contents.push(c);
    }
    Ok(Cia { title_id, tmd, contents })
}

/// Citra and Lime3DS (Azahar's predecessors) keep the same layout under their own names.
fn emu_name(exe: &str) -> &'static str {
    let n = js::basename(exe).to_lowercase();
    if n.contains("citra") {
        "Citra"
    } else if n.contains("lime3ds") {
        "Lime3DS"
    } else {
        "Azahar"
    }
}

/// The SD card folder its settings move it to, if they do.
fn custom_sdmc(ini: &str) -> Option<String> {
    static CUSTOM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?imR)^use_custom_storage\s*=\s*true\s*$").unwrap());
    static DIR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?imR)^sdmc_directory\s*=\s*(.+?)\s*$").unwrap());
    let ini = String::from_utf8_lossy(&std::fs::read(ini).ok()?).into_owned();
    let dir = DIR.captures(&ini).filter(|_| CUSTOM.is_match(&ini))?[1].to_string();
    (!dir.starts_with('@')).then_some(dir)
}

/// Azahar's SD card. Windows: in a `user` folder next to the program (portable), otherwise in `%APPDATA%\Azahar`.
/// Linux: in `~/.local/share/azahar-emu` (the Flatpak: `~/.var/app/org.azahar_emu.Azahar/data/…`), its settings in
/// `~/.config/azahar-emu`; Azahar has to have run once.
fn sdmc_dir(exe: &str, var: &dyn Fn(&str) -> Option<String>) -> Result<String, Failure> {
    let get = |k: &str| var(k).filter(|v| !v.is_empty());
    let name = emu_name(exe);
    let abs = |p: &str| resolve(Path::new(p)).to_string_lossy().into_owned();
    if cfg!(target_os = "linux") {
        let home = get("HOME").unwrap_or_default();
        let folder = format!("{}-emu", name.to_lowercase());
        let flat = js::join(&[&home, ".var", "app", "org.azahar_emu.Azahar"]);
        let pairs = [
            (js::join(&[&home, ".local", "share", &folder]), js::join(&[&home, ".config", &folder])),
            (js::join(&[&flat, "data", &folder]), js::join(&[&flat, "config", &folder])),
        ];
        let Some((data, config)) = pairs.into_iter().find(|(d, _)| is_dir(Path::new(d))) else {
            return Err(format!("Open {name} once on its own first (it makes its folders then), then press Play again."));
        };
        return Ok(custom_sdmc(&js::join(&[&config, "qt-config.ini"])).map_or_else(|| js::join(&[&data, "sdmc"]), |d| abs(&d)));
    }
    let beside = Some(js::join(&[&js::dirname(exe), "user"])).filter(|u| !exe.is_empty() && is_dir(Path::new(u)));
    let user = beside.unwrap_or_else(|| {
        let appdata = get("APPDATA").unwrap_or_else(|| js::join(&[&get("USERPROFILE").unwrap_or_default(), "AppData", "Roaming"]));
        js::join(&[&appdata, name])
    });
    Ok(custom_sdmc(&js::join(&[&user, "config", "qt-config.ini"])).map_or_else(|| js::join(&[&user, "sdmc"]), |d| abs(&d.replace('/', std::path::MAIN_SEPARATOR_STR))))
}

fn content_dir(sdmc: &str, cia: &Cia) -> String {
    let (hi, lo) = (format!("{:08x}", cia.title_id >> 32), format!("{:08x}", cia.title_id & 0xffff_ffff));
    let dir = js::join(&[sdmc, "Nintendo 3DS", ZERO, ZERO, "title", &hi, &lo, "content"]);
    // DLC: Azahar keeps those one folder down
    if cia.contents.get(1).is_some_and(|c| c.kind & 0x4000 != 0) {
        return js::join(&[&dir, "00000000"]);
    }
    dir
}

fn app_name(c: &Content) -> String {
    format!("{:08x}.app", c.id)
}

/// Copies one content out of the .cia, through a `.part` file.
fn copy_range(src: &Path, dst: &str, at: u64, size: u64, on_bytes: &mut dyn FnMut(u64)) -> Result<(), Failure> {
    let tmp = format!("{dst}.part");
    let mut copy = || -> std::io::Result<u64> {
        let mut r = File::open(src)?;
        r.seek(SeekFrom::Start(at))?;
        let mut w = File::create(&tmp)?;
        let mut buf = vec![0u8; 4 << 20];
        let mut done = 0;
        while done < size {
            let want = buf.len().min((size - done) as usize);
            let n = r.read(&mut buf[..want])?;
            if n == 0 {
                break;
            }
            w.write_all(&buf[..n])?;
            done += n as u64;
            on_bytes(n as u64);
        }
        Ok(done)
    };
    let result = match copy() {
        // a .cia cut short ends early without an error: a short content would be "installed", fail to boot, and be
        // copied again on every launch
        Ok(n) if n != size => Err("This .cia is incomplete (the file ends early; it may not have finished downloading).".to_string()),
        Ok(_) => std::fs::rename(&tmp, dst).map_err(io),
        Err(e) if e.kind() == std::io::ErrorKind::StorageFull => Err("There isn't enough free space on the drive to install this game.".to_string()),
        Err(e) => Err(io(e)),
    };
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// The file Azahar should boot for this game: the game itself, or for a .cia the installed copy, installed first if
/// it isn't yet. `on_status`: what the launch screen says meanwhile.
pub fn boot_file(file: &Path, exe: &str, var: &dyn Fn(&str) -> Option<String>, on_status: &mut dyn FnMut(&str)) -> Result<PathBuf, Failure> {
    if !file.extension().is_some_and(|e| e.eq_ignore_ascii_case("cia")) {
        return Ok(file.to_path_buf());
    }
    let cia = parse(file)?;
    let emu = emu_name(exe);
    // Citra and Lime3DS can run encrypted games themselves, given the 3DS's keys
    let keys = emu != "Azahar";
    if cia.contents.iter().any(|c| c.present && c.kind & 1 != 0) {
        return Err(if keys {
            format!("This .cia is encrypted: install it once inside {emu} (File → Install CIA), then play it from here.")
        } else {
            "This .cia is encrypted, so Azahar can't use it. Use a decrypted copy of the game.".into()
        });
    }
    let Some(main) = cia.contents.iter().find(|c| c.index == 0).or(cia.contents.first()).filter(|c| c.present) else {
        return Err("This .cia has no game inside (it may be an update or DLC: install it inside Azahar).".into());
    };
    // the game inside can still be encrypted when the .cia around it isn't: NCCH header flags, byte 7 bit 0x4 = no
    // encryption
    let ncch = File::open(file).and_then(|mut f| read_at(&mut f, main.at, 0x200)).map_err(io)?;
    if ncch.len() < 0x200 || &ncch[0x100..0x104] != b"NCCH" {
        return Err("This .cia doesn't look like a 3DS game Azahar can use.".into());
    }
    if ncch[0x18F] & 0x4 == 0 && !keys {
        return Err("This game is still encrypted inside its .cia, so Azahar can't install it. Use a decrypted copy of the game.".into());
    }
    let dir = content_dir(&sdmc_dir(exe, var)?, &cia);
    let inside = |c: &Content| js::join(&[&dir, &app_name(c)]);
    let boot = PathBuf::from(inside(main));
    let have = |c: &Content| Path::new(&inside(c)).metadata().is_ok_and(|m| m.len() == c.size);
    let installed = std::fs::read_dir(&dir).is_ok_and(|d| d.flatten().any(|e| e.file_name().to_string_lossy().to_lowercase().ends_with(".tmd")));
    if installed && cia.contents.iter().filter(|c| c.present).all(have) {
        return Ok(boot);
    }
    std::fs::create_dir_all(&dir).map_err(io)?;
    let todo: Vec<&Content> = cia.contents.iter().filter(|c| c.present).collect();
    let total = todo.iter().map(|c| c.size).sum::<u64>().max(1);
    let (mut got, mut shown) = (0u64, None);
    on_status(&format!("Installing into {emu} · 0%"));
    for c in todo {
        if have(c) {
            got += c.size;
            continue;
        }
        copy_range(file, &inside(c), c.at, c.size, &mut |n| {
            got += n;
            let p = got * 100 / total;
            if shown != Some(p) {
                shown = Some(p);
                on_status(&format!("Installing into {emu} · {p}%"));
            }
        })?;
    }
    // written last, through a temporary name: an interrupted install isn't taken as a finished one
    let tmd = js::join(&[&dir, "00000000.tmd"]);
    let part = format!("{tmd}.part");
    std::fs::write(&part, &cia.tmd).and_then(|()| std::fs::rename(&part, &tmd)).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        io(e)
    })?;
    on_status(&format!("Starting in {emu}"));
    Ok(boot)
}
