// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/updates.js, as RustyShelf has it (its own releases, and a build number in a version).
//! Which release is the newest on RustyShelf's releases page, whether it's newer than this one, and putting it in
//! place of this one: the Windows zip's RustyShelf.exe, or the Linux AppImage.

use std::cmp::Ordering;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde_json::Value;

use crate::js;

const REPO: &str = "zearkreast/rustyshelf";
/// The releases page.
pub const RELEASES: &str = "https://github.com/zearkreast/rustyshelf/releases";

/// A version's four numbers, and its pre-release parts: each a number or a word.
type Version = ([u64; 4], Vec<Result<u64, String>>);

/// A version's numbers and its pre-release parts: "1.82.0-beta.1" is [1, 82, 0, 0] and ["beta", 1]. A build
/// number counts as the fourth number, written "2.12.4.284" (a release's tag) or "2.12.4+284".
fn parse(v: &str) -> Option<Version> {
    static VERSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^v?([0-9]+)(?:\.([0-9]+))?(?:\.([0-9]+))?(?:[.+]([0-9]+))?(?:-([0-9A-Za-z.-]+))?").unwrap());
    let m = VERSION.captures(js::trim(v))?;
    let n = |i: usize| m.get(i).map_or(0, |d| d.as_str().parse().unwrap_or(u64::MAX));
    let pre = m.get(5).map_or(Vec::new(), |p| p.as_str().split('.').map(|x| if !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit()) { Ok(x.parse().unwrap_or(u64::MAX)) } else { Err(x.to_string()) }).collect());
    Some(([n(1), n(2), n(3), n(4)], pre))
}

/// Whether version `a` is older than, the same as or newer than `b`. A release is newer than its own betas; two
/// versions that can't both be read count as the same.
pub fn compare(a: &str, b: &str) -> Ordering {
    let (Some((x, xp)), Some((y, yp))) = (parse(a), parse(b)) else { return Ordering::Equal };
    if x != y {
        return x.cmp(&y);
    }
    if xp.is_empty() || yp.is_empty() {
        return yp.is_empty().cmp(&xp.is_empty()).reverse();
    }
    for i in 0..xp.len().max(yp.len()) {
        match (xp.get(i), yp.get(i)) {
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(p), Some(q)) if p == q => {}
            (Some(Ok(p)), Some(Ok(q))) => return p.cmp(q),
            // a number comes before a word
            (Some(Ok(_)), Some(Err(_))) => return Ordering::Less,
            (Some(Err(_)), Some(Ok(_))) => return Ordering::Greater,
            (Some(Err(p)), Some(Err(q))) => return p.cmp(q),
        }
    }
    Ordering::Equal
}

/// A file of a release.
#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

/// The newest release, as against this version.
#[derive(Debug, Clone, PartialEq)]
pub struct Newest {
    /// its version, None when nothing is released yet
    pub latest: Option<String>,
    pub newer: bool,
    /// its page
    pub url: String,
    /// its notes, the first 2000 characters
    pub notes: String,
    pub assets: Vec<Asset>,
}

/// The newest release of a releases list (GitHub's), drafts left out, as against `current`.
pub fn newest(list: &Value, current: &str) -> Newest {
    let mut all: Vec<&Value> = list.as_array().into_iter().flatten().filter(|r| js::truthy(Some(r)) && !js::truthy(r.get("draft")) && parse(&js::string(r.get("tag_name"))).is_some()).collect();
    all.sort_by(|p, q| compare(&js::string(q.get("tag_name")), &js::string(p.get("tag_name"))));
    let best = all.first();
    let latest = best.map(|b| { let tag = js::string(b.get("tag_name")); tag.strip_prefix('v').map_or(tag.clone(), str::to_string) });
    Newest {
        newer: latest.as_ref().is_some_and(|l| compare(l, current) == Ordering::Greater),
        latest,
        url: best.map_or(RELEASES.to_string(), |b| js::string(b.get("html_url"))),
        notes: best.and_then(|b| b["body"].as_str()).map_or(String::new(), |b| js::slice(b, 2000).to_string()),
        assets: best
            .and_then(|b| b["assets"].as_array())
            .map_or(Vec::new(), |a| a.iter().map(|a| Asset { name: js::string(a.get("name")), url: js::string(a.get("browser_download_url")), size: a["size"].as_u64().unwrap_or(0) }).collect()),
    }
}

/// Asks the releases page which version is newest; an error in words when it can't be asked (offline, GitHub
/// busy).
pub fn check(current: &str) -> Result<Newest, String> {
    // the list, not the "latest" release: that one skips pre-releases, and every beta is one
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=10");
    let agent = format!("RustyShelf/{current}");
    let (status, _, body) = crate::net::ask(&url, &[("User-Agent", agent.as_str()), ("Accept", "application/vnd.github+json")], Duration::from_secs(15), 2_000_000).map_err(|slow| if slow { "timed out" } else { "couldn't reach GitHub" }.to_string())?;
    if status != 200 {
        return Err(format!("GitHub answered {status}"));
    }
    Ok(newest(&serde_json::from_slice(&body).map_err(|_| "unreadable answer".to_string())?, current))
}

/// The release's file that updates this system: the Windows zip, or the Linux AppImage.
fn asset_for(assets: &[Asset]) -> Option<&Asset> {
    let end = if cfg!(windows) { "-windows.zip" } else if cfg!(target_os = "linux") && !cfg!(target_os = "android") { ".AppImage" } else { return None };
    assets.iter().find(|a| a.name.ends_with(end))
}

/// The program a new version replaces: this exe on Windows, the AppImage on Linux. None for a build run from Cargo's
/// target folder, an unpacked Linux binary, Android or any other system.
pub fn this_program() -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        std::env::current_exe().ok()?
    } else if cfg!(target_os = "linux") && !cfg!(target_os = "android") {
        PathBuf::from(std::env::var_os("APPIMAGE").filter(|a| !a.is_empty())?)
    } else {
        return None;
    };
    let names: Vec<String> = exe.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect();
    if names.windows(2).any(|w| w[0] == "target" && matches!(w[1].as_str(), "debug" | "release")) {
        return None;
    }
    Some(exe)
}

/// Whether this RustyShelf can put a new version in its own place.
pub fn can_install() -> bool {
    this_program().is_some()
}

/// Whether a release has a file this RustyShelf can update itself from.
pub fn installable(n: &Newest) -> bool {
    can_install() && asset_for(&n.assets).is_some()
}

fn beside(program: &Path, ext: &str) -> PathBuf {
    let mut name = program.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{ext}"));
    program.with_file_name(name)
}

/// What an update before this start left beside the program: the old exe Windows wouldn't let go of, a download cut
/// short.
pub fn tidy() {
    if let Some(p) = this_program() {
        for ext in ["old", "download", "new"] {
            let _ = fs::remove_file(beside(&p, ext));
        }
    }
}

fn sha256_of(path: &Path) -> Result<String, String> {
    use sha2::Digest;
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let (mut h, mut buf) = (sha2::Sha256::new(), vec![0u8; 1 << 20]);
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The file's sum in a SHA256SUMS.txt (`<hex>  <name>` lines).
fn sum_in(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (hash, file) = l.trim().split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name).then(|| hash.to_ascii_lowercase())
    })
}

/// Downloads the release's file for this system, checks it against the release's SHA256SUMS.txt and puts it in place
/// of this program, which runs on until restarted. `on` hears the bytes got and the size.
pub fn install(n: &Newest, on: &mut dyn FnMut(u64, u64)) -> Result<PathBuf, String> {
    let program = this_program().ok_or("This RustyShelf can't update itself: get the new version from its page.")?;
    let asset = asset_for(&n.assets).ok_or("The release has no download for this system.")?;
    let sums = n.assets.iter().find(|a| a.name == "SHA256SUMS.txt").ok_or("The release has no SHA256SUMS.txt to check the download with.")?;
    let (status, body) = crate::net::get(&sums.url, Duration::from_secs(30), 1 << 20, "RustyShelf").map_err(|e| format!("Couldn't get the release's sums ({e})."))?;
    let want = (status == 200).then(|| sum_in(&String::from_utf8_lossy(&body), &asset.name)).flatten().ok_or("The release's sums don't list its download.")?;

    let download = beside(&program, "download");
    let fail = |why: String| {
        let _ = fs::remove_file(&download);
        why
    };
    crate::net::download_sized(&asset.url, &download, Duration::from_secs(30 * 60), on).map_err(|e| fail(format!("The download failed ({e}).")))?;
    if sha256_of(&download).map_err(fail)? != want {
        return Err(fail("The download doesn't match the release's sums.".into()));
    }

    if cfg!(windows) {
        let new = beside(&program, "new");
        let unpacked = (|| -> Result<(), String> {
            let mut zip = File::open(&download).map_err(|e| e.to_string())?;
            let entries = crate::zip::entries_of(&mut zip).map_err(|e| format!("{e:?}"))?;
            let exe = entries.iter().find(|e| e.name.replace('\\', "/").eq_ignore_ascii_case("RustyShelf/RustyShelf.exe")).ok_or("no RustyShelf.exe in the zip")?;
            let mut out = File::create(&new).map_err(|e| e.to_string())?;
            crate::zip::unpack(&mut zip, exe, &mut out).map_err(|e| e.to_string())?;
            out.sync_all().map_err(|e| e.to_string())
        })();
        let _ = fs::remove_file(&download);
        if let Err(why) = unpacked {
            let _ = fs::remove_file(&new);
            return Err(format!("The download couldn't be unpacked ({why})."));
        }
        // a running exe can't be overwritten, but it can be renamed
        let old = beside(&program, "old");
        let _ = fs::remove_file(&old);
        fs::rename(&program, &old).map_err(|e| {
            let _ = fs::remove_file(&new);
            format!("RustyShelf.exe couldn't be replaced ({e}).")
        })?;
        if let Err(e) = fs::rename(&new, &program) {
            let _ = fs::rename(&old, &program);
            let _ = fs::remove_file(&new);
            return Err(format!("RustyShelf.exe couldn't be replaced ({e})."));
        }
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&download, fs::Permissions::from_mode(0o755)).map_err(|e| fail(e.to_string()))?;
        }
        fs::rename(&download, &program).map_err(|e| fail(format!("The AppImage couldn't be replaced ({e}).")))?;
    }
    Ok(program)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_alpha_sees_the_next_alpha_and_a_build_the_next_build() {
        let list = |tag: &str| json!([{ "tag_name": tag, "html_url": "u", "body": "", "assets": [] }]);
        assert!(newest(&list("v0.2.0-alpha.7"), "0.2.0-alpha.6").newer);
        assert!(!newest(&list("v0.2.0-alpha.6"), "0.2.0-alpha.6").newer);
        assert!(newest(&list("v0.2.0.4"), "0.2.0.3").newer);
        assert!(newest(&list("v0.2.0.0"), "0.2.0-alpha.6").newer);
    }

    #[test]
    fn a_sum_is_found_by_its_file_name() {
        let sums = "ab12  RustyShelf-0.5.6-windows.zip\nCD34 *RustyShelf-0.5.6-linux.AppImage\n";
        assert_eq!(sum_in(sums, "RustyShelf-0.5.6-windows.zip").as_deref(), Some("ab12"));
        assert_eq!(sum_in(sums, "RustyShelf-0.5.6-linux.AppImage").as_deref(), Some("cd34"));
        assert_eq!(sum_in(sums, "RustyShelf-0.5.6-android.apk"), None);
    }
}
