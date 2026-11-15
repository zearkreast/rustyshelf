//! The room the PC games take (Settings → PC games → Storage): every installed PC game that has a folder, with
//! its size, grouped by the drive it's on, with each drive's free space. The sizes are the ones already known
//! (Steam's own, or a folder's as measured behind the shelf); nothing is walked here.
//!
//! The idea of a storage screen comes from the Playnite "Storage" add-on (MIT) as the TyroShelf fork has it;
//! this is written for RustyShelf's own games and settings.

use std::collections::HashSet;
use std::path::Path;

use crate::{art, collate, js};

/// A game with a folder, and its size as it's known (0 when it isn't yet).
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: String,
    pub title: String,
    pub source: String,
    pub dir: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Drive {
    pub root: String,
    /// in bytes; None where the system doesn't say
    pub free: Option<u64>,
    pub total: Option<u64>,
    pub games_bytes: u64,
    /// the biggest first
    pub games: Vec<Entry>,
}

/// The mount a folder is on: on Windows its drive; on Linux the longest mount in `/proc/self/mounts` (or
/// `mounts`, its text) that holds it.
pub fn mount_of(p: &str, mounts: Option<&str>) -> String {
    let b = p.as_bytes();
    if b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/') {
        return format!("{}\\", &p[..2]);
    }
    if cfg!(windows) {
        let full = std::path::absolute(p).unwrap_or_else(|_| p.into());
        let root: std::path::PathBuf = full.components().take_while(|c| matches!(c, std::path::Component::Prefix(_) | std::path::Component::RootDir)).collect();
        return if root.as_os_str().is_empty() { full.to_string_lossy().into_owned() } else { root.to_string_lossy().into_owned() };
    }
    let full = art::resolve(Path::new(p)).to_string_lossy().into_owned();
    let text = mounts.map(str::to_string).or_else(|| std::fs::read_to_string("/proc/self/mounts").ok()).unwrap_or_default();
    let mut best = "/".to_string();
    for line in text.split('\n') {
        let Some(point) = line.split(' ').nth(1) else { continue };
        let point = point.replace("\\040", " ").replace("\\011", "\t").replace("\\134", "\\");
        let within = if point.ends_with('/') { point.clone() } else { format!("{point}/") };
        if (full == point || full.starts_with(&within)) && point.len() > best.len() {
            best = point;
        }
    }
    best
}

pub fn space(root: &str) -> Room {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetDiskFreeSpaceExW(dir: *const u16, free_to_caller: *mut u64, total: *mut u64, free: *mut u64) -> i32;
        }
        let wide: Vec<u16> = std::ffi::OsStr::new(root).encode_wide().chain(Some(0)).collect();
        let (mut free, mut total) = (0u64, 0u64);
        // SAFETY: a zero-ended path and two numbers to fill; the third isn't asked for
        let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, std::ptr::null_mut()) };
        if ok != 0 { (Some(free), Some(total)) } else { (None, None) }
    }
    #[cfg(not(windows))]
    {
        // POSIX `df`, in 1024-byte blocks: Filesystem, blocks, used, available, capacity, mount
        let out = std::process::Command::new("df").args(["-Pk", "--", root]).output().ok().filter(|o| o.status.success());
        let numbers: Option<Vec<u64>> = out.and_then(|o| String::from_utf8_lossy(&o.stdout).lines().nth(1).map(|l| l.split_whitespace().skip(1).take(3).filter_map(|n| n.parse().ok()).collect()));
        match numbers.as_deref() {
            Some([blocks, _, free]) => (Some(free * 1024), Some(blocks * 1024)),
            _ => (None, None),
        }
    }
}

/// A drive's free room and its size, in bytes, where the system says.
pub type Room = (Option<u64>, Option<u64>);

/// The games by drive: the drive whose games take the most first, each drive's games the biggest first. A game
/// with no folder, or one that isn't there, is left out; so is a folder seen before.
pub fn summary(games: &[Entry], mounts: Option<&str>, space: &dyn Fn(&str) -> Room) -> Vec<Drive> {
    let mut drives: Vec<Drive> = Vec::new();
    let mut seen = HashSet::new();
    for g in games.iter().filter(|g| !g.dir.is_empty()) {
        let dir = art::resolve(Path::new(&g.dir)).to_string_lossy().into_owned();
        if seen.contains(&dir.to_lowercase()) || !Path::new(&dir).is_dir() {
            continue;
        }
        seen.insert(dir.to_lowercase());
        let root = mount_of(&dir, mounts);
        let at = drives.iter().position(|d| d.root == root).unwrap_or_else(|| {
            drives.push(Drive { root, free: None, total: None, games_bytes: 0, games: Vec::new() });
            drives.len() - 1
        });
        let title = if g.title.is_empty() { js::basename(&dir) } else { g.title.clone() };
        drives[at].games.push(Entry { id: g.id.clone(), title: js::slice(&title, 120).to_string(), source: g.source.clone(), dir, bytes: g.bytes });
        drives[at].games_bytes += g.bytes;
    }
    for d in &mut drives {
        (d.free, d.total) = space(&d.root);
        d.games.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| collate::compare_exact(&a.title, &b.title)));
    }
    drives.sort_by(|a, b| b.games_bytes.cmp(&a.games_bytes).then_with(|| collate::compare_exact(&a.root, &b.root)));
    drives
}
