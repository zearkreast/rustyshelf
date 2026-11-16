// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/updates.js, as RustyShelf has it (its own releases, and a build number in a version).
//! Which release is the newest on RustyShelf's releases page, and whether it's newer than this one. Nothing is
//! downloaded: it's a notice, with the page to get it from.

use std::cmp::Ordering;
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
}
