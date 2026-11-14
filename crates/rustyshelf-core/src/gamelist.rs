// RustyShelf: the gamelist.xml that Batocera, RetroBat and EmulationStation keep in each system's games folder. A
// port of main/gamelist.js.
//! `<thumbnail>` / `<boxart>` are the box front (`<image>` is a screenshot or mix, used only when there's no box),
//! `<boxback>` the back, `<cartridge>` the disc, `<fanart>` / `<image>` the picture behind the game.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::art::resolve;
use crate::paths::is_file;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub front: Option<PathBuf>,
    pub back: Option<PathBuf>,
    pub disc: Option<PathBuf>,
    pub hero: Option<PathBuf>,
    pub manual_pdf: Option<PathBuf>,
    pub video: Option<PathBuf>,
    pub desc: String,
    pub developer: String,
    pub publisher: String,
    pub genre: String,
    /// YYYYMMDD, or as much of it as was scraped
    pub released: String,
}

static GAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<game\b[^>]*>(.*?)</game>").unwrap());
static ENTITY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)&(#x[0-9a-f]+|#[0-9]+|[0-9A-Za-z_]+);").unwrap());
const TAGS: [&str; 17] = [
    "path", "name", "image", "thumbnail", "boxart", "boxback", "cartridge", "fanart", "marquee", "wheel", "video", "manual", "desc",
    "releasedate", "developer", "publisher", "genre",
];
static TAG_RES: LazyLock<Vec<(&str, Regex)>> =
    LazyLock::new(|| TAGS.iter().map(|t| (*t, Regex::new(&format!(r"(?is)<{t}>(.*?)</{t}>")).unwrap())).collect());

fn unxml(s: &str) -> String {
    ENTITY
        .replace_all(s, |c: &regex::Captures| {
            let e = &c[1];
            let code = if let Some(h) = e.strip_prefix("#x").or_else(|| e.strip_prefix("#X")) {
                u32::from_str_radix(h, 16).ok()
            } else if let Some(d) = e.strip_prefix('#') {
                d.parse().ok()
            } else {
                return match e.to_lowercase().as_str() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => &c[0],
                }
                .to_string();
            };
            code.and_then(char::from_u32).map(String::from).unwrap_or_default()
        })
        .trim()
        .to_string()
}

fn home() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_default()
}

/// node's path.normalize
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir if matches!(out.components().next_back(), Some(Component::Normal(_))) => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

/// A path as gamelist.xml writes it ("./x.zip", "~/roms/…", "/userdata/roms/…", "images\x.png"), from its folder.
fn real(p: &str, dir: &Path) -> Option<PathBuf> {
    let s = unxml(p);
    if s.is_empty() {
        return None;
    }
    let sep = std::path::MAIN_SEPARATOR_STR;
    let drive = s.len() >= 3 && s.as_bytes()[0].is_ascii_alphabetic() && &s[1..2] == ":" && matches!(&s[2..3], "\\" | "/");
    let p = if let Some(rest) = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\")) {
        home().join(rest.replace(['/', '\\'], sep))
    } else if Path::new(&s).is_absolute() || drive {
        PathBuf::from(s.replace(['/', '\\'], sep))
    } else {
        let rest = s.strip_prefix("./").or_else(|| s.strip_prefix(".\\")).unwrap_or(&s);
        dir.join(rest.replace(['/', '\\'], sep))
    };
    Some(normalize(&p))
}

fn key(p: &Path) -> String {
    p.to_string_lossy().to_lowercase()
}

/// The games of one gamelist.xml: file (lower case) → its entry. Pictures that aren't there are left out.
pub fn parse(xml: &str, dir: &Path) -> HashMap<String, Entry> {
    let mut games = HashMap::new();
    for m in GAME.captures_iter(xml) {
        let body = &m[1];
        let mut e: HashMap<&str, String> = HashMap::new();
        for (t, re) in TAG_RES.iter() {
            if let Some(x) = re.captures(body) {
                let v = unxml(&x[1]);
                if !v.is_empty() {
                    e.insert(t, v);
                }
            }
        }
        let Some(file) = e.get("path").and_then(|p| real(p, dir)) else { continue };
        let pic = |ks: &[&str]| ks.iter().filter_map(|k| e.get(k)).filter_map(|v| real(v, dir)).find(|f| is_file(f));
        let text = |k: &str| e.get(k).cloned().unwrap_or_default();
        let manual = text("manual");
        games.insert(
            key(&file),
            Entry {
                name: text("name"),
                front: pic(&["thumbnail", "boxart"]).or_else(|| pic(&["image"])),
                back: pic(&["boxback"]),
                disc: pic(&["cartridge"]),
                hero: pic(&["fanart", "image"]),
                manual_pdf: if manual.to_lowercase().ends_with(".pdf") { pic(&["manual"]) } else { None },
                video: pic(&["video"]),
                desc: text("desc"),
                developer: text("developer"),
                publisher: text("publisher"),
                genre: text("genre"),
                released: text("releasedate").chars().take(8).collect(),
            },
        );
    }
    games
}

/// The gamelists of one scan, each folder's read once.
#[derive(Default)]
pub struct Gamelists {
    dirs: HashMap<PathBuf, HashMap<String, Entry>>,
}

impl Gamelists {
    fn for_dir(&mut self, dir: &Path) -> &HashMap<String, Entry> {
        self.dirs.entry(dir.to_path_buf()).or_insert_with(|| {
            std::fs::read(dir.join("gamelist.xml")).map(|b| parse(&String::from_utf8_lossy(&b), dir)).unwrap_or_default()
        })
    }

    /// A game file's entry: from the gamelist.xml in its folder, or in a folder above it up to the games folder (a
    /// game kept in a sub-folder is listed in the system folder's gamelist).
    pub fn lookup(&mut self, file: &Path, rom_dirs: &[String]) -> Option<Entry> {
        let roots: Vec<String> = rom_dirs.iter().map(|d| key(&resolve(Path::new(d)))).collect();
        let file = resolve(file);
        let want = key(&file);
        let mut dir = file.parent()?.to_path_buf();
        for _ in 0..4 {
            if let Some(hit) = self.for_dir(&dir).get(&want) {
                return Some(hit.clone());
            }
            if roots.contains(&key(&dir)) {
                break;
            }
            match dir.parent() {
                Some(up) => dir = up.to_path_buf(),
                None => break,
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::create_dir_all(d.join("images")).unwrap();
        std::fs::write(d.join("images/a-thumb.png"), b"").unwrap();
        std::fs::write(d.join("images/a-image.png"), b"").unwrap();
        let xml = r#"<gameList><game id="1"><path>./sub/A.zip</path><name>Alpha &amp; Omega &#233;</name>
            <thumbnail>./images/a-thumb.png</thumbnail><image>./images/a-image.png</image><boxback>./images/none.png</boxback>
            <releasedate>19910101T000000</releasedate></game><game><name>no path</name></game></gameList>"#;
        std::fs::write(d.join("gamelist.xml"), xml).unwrap();
        let mut g = Gamelists::default();
        let e = g.lookup(&d.join("sub/A.zip"), &[d.to_string_lossy().into_owned()]).unwrap();
        assert_eq!(e.name, "Alpha & Omega é");
        assert_eq!(e.front, Some(d.join("images/a-thumb.png")));
        assert_eq!(e.hero, Some(d.join("images/a-image.png")));
        assert_eq!(e.back, None);
        assert_eq!(e.released, "19910101");
        assert!(g.lookup(&d.join("B.zip"), &[]).is_none());
    }
}
