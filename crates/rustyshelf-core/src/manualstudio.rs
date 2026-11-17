// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/manualstudio.js, as RustyShelf has it.
//! Manual Studio: every manual found for a game side by side (the one on disk, earlier ones set aside, the console's
//! manual collections, GamesDatabase's PDF, archive.org's uploads), with language, region, pages and scan quality.
//! Use puts one in place (the manual it replaces goes to `.manual-originals/<game>/<time>`, never deleted), Undo
//! puts the previous one back, and your own PDF can go in. Kept in `data/manual-studio.json` with the Electron app.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::i18n::plural_s;
use crate::manualcoll::{self, Game};
use crate::manuals::{self, Web};
use crate::{js, spreads};

/// the most pages kept of a PDF and of an archive.org upload (main/manualstudio.js PDF_CUT, IA_CUT)
pub const PDF_CUT: u32 = 80;
pub const IA_CUT: u32 = 60;

/// One manual on offer for a game.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cand {
    /// what the screen names it by: "cur", "earlier:<folder>", "ia:<upload>", "gdb:<address>", "coll:<collection>/<file>"
    pub key: String,
    /// "current", "earlier", "archive", "gamesdatabase", "collection" or "own"
    pub source: String,
    pub label: String,
    pub title: String,
    /// two-letter language, "" when unknown
    pub lang: String,
    /// "usa", "eur", "jpn" or ""
    pub region: String,
    pub pages: Option<u32>,
    /// a scan's width (pixels), and for a PDF its bytes a page
    pub w: u32,
    pub bpp: f64,
    /// how well the search thought it matched
    pub rel: f64,
    /// the upload says pages are missing
    pub incomplete: bool,
    /// blank pages
    pub junk: u32,
    /// the pages it should have, as known when it was put in
    pub expected: Option<u32>,
    /// where the manual in use (or an earlier one) came from, by key
    pub src: Option<String>,
    pub origin: String,
    pub in_use: bool,
    pub pdf_only: bool,
    /// an official digital manual (a collection's PDF)
    pub digital: bool,
    /// its first page, on this computer
    pub thumb: Option<PathBuf>,
    /// an earlier copy's folder, or a downloaded PDF
    pub dir: Option<PathBuf>,
    pub pdf: Option<PathBuf>,
}

/// A scan's sharpness in words: by its width, or for a PDF by its bytes a page. ("sharp", "Sharp · 1600 px")
pub fn quality(w: u32, bpp: f64) -> (&'static str, String) {
    if w > 0 {
        let q = if w >= 1400 {
            ("sharp", "Sharp")
        } else if w >= 900 {
            ("good", "Good")
        } else if w >= 600 {
            ("fair", "Fair")
        } else {
            ("low", "Low-res")
        };
        return (q.0, format!("{} · {w} px", q.1));
    }
    if bpp > 0.0 {
        let q = if bpp >= 250_000.0 {
            "sharp"
        } else if bpp >= 120_000.0 {
            "good"
        } else {
            "fair"
        };
        return (q, format!("PDF · {} KB a page", (bpp / 1024.0).round()));
    }
    ("", String::new())
}

/// Whether a manual gives what a console's choice asks for ("best", "lang:xx", "region:eur", "region:jpn").
pub fn fits(c: &Cand, pref: &str) -> bool {
    let en = c.lang.is_empty() || c.lang == "en";
    match pref {
        "region:eur" => en && c.region == "eur",
        "region:jpn" => c.lang == "ja" || c.region == "jpn",
        p => match p.strip_prefix("lang:").filter(|l| l.len() == 2 && l.bytes().all(|b| b.is_ascii_lowercase())) {
            Some(l) => c.lang == l,
            None => en,
        },
    }
}

/// The page count most copies of each language agree on (two or more); a manual and its earlier backup of the same
/// download are one copy.
pub fn expect_of(list: &[Cand]) -> BTreeMap<String, u32> {
    let mut by: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    let mut once = std::collections::HashSet::new();
    for c in list.iter().filter(|c| !c.pdf_only) {
        let Some(n) = c.pages.filter(|n| *n > 0) else { continue };
        let id = c.src.clone().unwrap_or_else(|| c.key.clone());
        if !id.is_empty() && id != "cur" && !once.insert(id) {
            continue;
        }
        by.entry(if c.lang.is_empty() { "en".into() } else { c.lang.clone() }).or_default().push(n);
    }
    let mut out = BTreeMap::new();
    for (lang, ns) in by {
        let mut counts: Vec<(u32, u32)> = Vec::new();
        for n in ns {
            match counts.iter_mut().find(|x| x.0 == n) {
                Some(x) => x.1 += 1,
                None => counts.push((n, 1)),
            }
        }
        counts.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));
        if let Some(best) = counts.first().filter(|b| b.1 >= 2) {
            out.insert(lang, best.0);
        }
    }
    out
}

/// Whether a manual is whole: Some(true) "Complete", Some(false) with the pages missing and why, None when there's
/// nothing to compare it with.
pub fn completeness(c: &Cand, expect: &BTreeMap<String, u32>) -> (Option<bool>, u32, String) {
    if c.incomplete {
        return (Some(false), 0, "Incomplete (says the upload)".into());
    }
    if c.junk > 0 {
        return (Some(false), c.junk, format!("{} blank page{}", c.junk, plural_s(c.junk)));
    }
    let want = expect.get(if c.lang.is_empty() { "en" } else { &c.lang }).copied().or(c.expected).filter(|w| *w > 0);
    match (c.pages.filter(|p| *p > 0), want) {
        (Some(p), Some(w)) if p >= w => (Some(true), 0, "Complete".into()),
        (Some(p), Some(w)) => (Some(false), w - p, format!("{} page{} missing", w - p, plural_s(w - p))),
        _ => (None, 0, String::new()),
    }
}

/// How well a manual fits the console's choice: the list's order, and what a re-pick takes.
pub fn rank(c: &Cand, pref: &str, expect: &BTreeMap<String, u32>) -> f64 {
    let en = c.lang.is_empty() || c.lang == "en";
    let mut s = if fits(c, pref) {
        30.0
    } else if en {
        10.0
    } else {
        0.0
    };
    s += c.rel.min(12.0)
        + match c.source.as_str() {
            "collection" => 3.0,
            "gamesdatabase" => 1.0,
            _ => 0.0,
        };
    if c.w > 0 {
        s += if c.w >= 1400 {
            3.0
        } else if c.w >= 900 {
            2.0
        } else if c.w < 600 {
            -2.0
        } else {
            0.0
        };
    } else if c.bpp > 0.0 {
        s += if c.bpp >= 250_000.0 {
            3.0
        } else if c.bpp >= 120_000.0 {
            1.0
        } else {
            -1.0
        };
    }
    match completeness(c, expect).0 {
        Some(false) => s -= 6.0,
        Some(true) => s += 1.0,
        None => {}
    }
    if (pref.is_empty() || pref == "best") && c.region == "usa" {
        s += 1.0;
    }
    s
}

/// The note on a card: whether it's whole, and when only its first pages are kept.
pub fn complete_text(c: &Cand, expect: &BTreeMap<String, u32>) -> String {
    let pages = c.pages.unwrap_or(0);
    let cut = if (c.pdf.is_some() || c.bpp > 0.0) && pages > PDF_CUT {
        format!("only the first {PDF_CUT} pages are kept")
    } else if c.source == "archive" && pages > IA_CUT {
        format!("only the first {IA_CUT} pages are kept")
    } else {
        String::new()
    };
    [completeness(c, expect).2, cut].into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" · ")
}

/// As the Electron app's fixtures give a candidate.
pub fn from_json(v: &Value) -> Cand {
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    Cand {
        key: s("key"),
        source: s("source"),
        label: s("label"),
        title: s("title"),
        lang: s("lang"),
        region: s("region"),
        pages: v["pages"].as_u64().map(|n| n as u32),
        w: v["w"].as_u64().unwrap_or(0) as u32,
        bpp: v["bpp"].as_f64().unwrap_or(0.0),
        rel: v["rel"].as_f64().unwrap_or(0.0),
        incomplete: v["incomplete"] == true,
        junk: v["junk"].as_u64().unwrap_or(0) as u32,
        expected: v["expected"].as_u64().map(|n| n as u32),
        src: v["src"].as_str().map(String::from),
        pdf_only: v["pdfOnly"] == true,
        pdf: v["pdf"].as_str().map(PathBuf::from),
        ..Cand::default()
    }
}

// ---------- the games, and what's on disk ----------

/// what a backup folder's manual was (for its Earlier card)
const SIDE: &str = ".rustyshelf-manual.json";
const MAX_PDF: u64 = 400 * 1_048_576;
const HIST_MAX: usize = 12;

fn is_page(name: &str, base: &str) -> bool {
    static PAGE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)-manual-[0-9]+\.(jpe?g|png|webp)$").unwrap());
    name.to_lowercase().starts_with(&format!("{base}-manual-").to_lowercase()) && PAGE.is_match(name)
}

/// A manual's pages in a folder, in page order.
pub fn pages_in(dir: &Path, base: &str) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| is_page(n, base)).collect();
    names.sort_by(|a, b| crate::collate::compare(a, b));
    names
}

fn width(f: &Path) -> u32 {
    crate::boxart::size_of(f).map_or(0, |z| z.0)
}

/// `data/manual-studio.json`: each console's choice (`prefs`), the locked games (`locks`), what each game's manual
/// is (`cur`) and the manuals it had before (`hist`), by game key.
pub struct Db {
    file: PathBuf,
    pub all: Map<String, Value>,
}

impl Db {
    pub fn load(data_dir: &Path) -> Db {
        let file = data_dir.join("manual-studio.json");
        let mut all = crate::files::read_object(&file);
        for k in ["prefs", "locks", "cur", "hist"] {
            if !all.get(k).is_some_and(Value::is_object) {
                all.insert(k.into(), json!({}));
            }
        }
        Db { file, all }
    }

    pub fn save(&self) -> Result<(), String> {
        crate::files::write_atomic(&self.file, Value::Object(self.all.clone()).to_string()).map_err(|e| e.to_string())
    }

    fn part(&mut self, k: &str) -> &mut Map<String, Value> {
        self.all.get_mut(k).and_then(Value::as_object_mut).expect("a part of the store")
    }

    pub fn cur(&self, key: &str) -> Option<&Value> {
        self.all["cur"].get(key).filter(|v| v.is_object())
    }

    pub fn can_undo(&self, key: &str) -> bool {
        self.all["hist"].get(key).and_then(Value::as_array).is_some_and(|h| !h.is_empty())
    }

    pub fn locked(&self, key: &str) -> bool {
        js::truthy(self.all["locks"].get(key))
    }

    pub fn pref(&self, sys: &str) -> String {
        self.all["prefs"].get(sys).and_then(Value::as_str).filter(|p| !p.is_empty()).unwrap_or("best").to_string()
    }

    /// The manual that was there goes on the game's list of earlier ones; `meta` is what's there now.
    fn remember(&mut self, key: &str, keep: Option<&Path>, meta: Value, now: f64) {
        let (cur, lock) = (self.all["cur"].get(key).cloned().unwrap_or(Value::Null), self.all["locks"].get(key).cloned().unwrap_or(Value::Null));
        let h = self.part("hist").entry(key.to_string()).or_insert_with(|| json!([]));
        if let Some(h) = h.as_array_mut() {
            h.push(json!({ "dir": keep.map(|k| k.to_string_lossy().into_owned()), "cur": cur, "lock": lock, "at": js::num(now) }));
            while h.len() > HIST_MAX {
                h.remove(0);
            }
        }
        let mut meta = meta;
        meta["at"] = js::num(now);
        self.part("cur").insert(key.into(), meta);
    }
}

/// What the manual in use is, as far as RustyShelf knows: picked here, brought by a collection, or by the search.
pub fn cur_meta(data_dir: &Path, db: &Db, g: &Game) -> Option<Value> {
    let key = g.key();
    if let Some(c) = db.cur(&key) {
        return Some(c.clone());
    }
    let memo = manualcoll::Memo::load(data_dir);
    if let Some(src) = memo.seen.get(&key).and_then(|c| c["src"].as_str()).filter(|s| !s.is_empty()) {
        return Some(match src.strip_prefix("gdb/") {
            Some(url) => json!({ "src": format!("gdb:{url}"), "label": "GamesDatabase", "lang": "en", "region": "", "origin": "auto" }),
            None => json!({ "src": format!("coll:{src}"), "label": format!("Collection · {}", src.split('/').next().unwrap_or("")), "lang": "en", "region": "usa", "origin": "auto" }),
        });
    }
    let stores = manuals::Stores::load(data_dir);
    if let Some(a) = stores.auto.get(&key).filter(|a| js::truthy(a.get("got"))) {
        let got = js::string(a.get("got"));
        let title = a["title"].as_str().unwrap_or("");
        return Some(json!({ "src": if got.starts_with("gdb:") { got.clone() } else { format!("ia:{got}") }, "label": title, "lang": "en", "region": manuals::region_of(title), "origin": "auto" }));
    }
    (g.sys == "wiiu").then(|| json!({ "src": "builtin", "label": "Built-in e-manual", "lang": "", "region": "", "origin": "auto" }))
}

/// The manual in use, as a card.
pub fn current_cand(data_dir: &Path, db: &Db, g: &Game) -> Option<Cand> {
    let pages = pages_in(&g.dir, &g.base);
    let pdf = g.dir.join(format!("{}-manual.pdf", g.base));
    let pdf = pdf.is_file().then_some(pdf);
    if pages.is_empty() && pdf.is_none() {
        return None;
    }
    let mut ws: Vec<u32> = pages.iter().take(6).map(|f| width(&g.dir.join(f))).filter(|w| *w >= 300).collect();
    ws.sort_unstable();
    let meta = cur_meta(data_dir, db, g).unwrap_or(json!({}));
    let s = |k: &str| meta[k].as_str().unwrap_or("").to_string();
    Some(Cand {
        key: "cur".into(),
        source: "current".into(),
        label: Some(s("label")).filter(|l| !l.is_empty()).unwrap_or_else(|| "The manual on disk".into()),
        lang: s("lang"),
        region: s("region"),
        pages: Some(pages.len() as u32).filter(|n| *n > 0),
        w: ws.get(ws.len() / 2).copied().unwrap_or(0),
        thumb: pages.first().map(|f| g.dir.join(f)),
        in_use: true,
        pdf_only: pages.is_empty(),
        src: meta["src"].as_str().map(String::from),
        expected: meta["expected"].as_u64().map(|n| n as u32),
        origin: s("origin"),
        ..Cand::default()
    })
}

/// The manuals set aside before (`.manual-originals/<game>/<time>`), the newest first, each once, and none that's
/// the manual in use.
pub fn earlier_cands(g: &Game) -> Vec<Cand> {
    let root = g.dir.join(".manual-originals").join(&g.base);
    let mut ds: Vec<String> = fs::read_dir(&root).into_iter().flatten().flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    ds.sort_by(|a, b| b.cmp(a));
    let size = |f: &Path| fs::metadata(f).map_or(0, |m| m.len());
    let sig = |dir: &Path, pages: &[String]| format!("{}:{}:{}", pages.len(), size(&dir.join(&pages[0])), size(&dir.join(&pages[pages.len() - 1])));
    let cur = pages_in(&g.dir, &g.base);
    let mut seen: std::collections::HashSet<String> = if cur.is_empty() { Default::default() } else { [sig(&g.dir, &cur)].into() };
    let mut out = Vec::new();
    for d in ds.iter().take(30) {
        if out.len() >= 10 {
            break;
        }
        let full = root.join(d);
        let pages = pages_in(&full, &g.base);
        if pages.is_empty() || !seen.insert(sig(&full, &pages)) {
            continue;
        }
        let side = crate::files::read_json(&full.join(SIDE)).unwrap_or(json!({}));
        let when = if d.len() >= 10 && d.as_bytes()[4] == b'-' { format!(" ({}/{}/{})", &d[8..10], &d[5..7], &d[..4]) } else { String::new() };
        let label = side["label"].as_str().filter(|l| !l.is_empty()).map_or(String::new(), |l| format!("{l} · "));
        out.push(Cand {
            key: format!("earlier:{d}"),
            source: "earlier".into(),
            label: format!("{label}Earlier{when}"),
            lang: side["lang"].as_str().unwrap_or("").into(),
            region: side["region"].as_str().unwrap_or("").into(),
            pages: Some(pages.len() as u32),
            w: width(&full.join(&pages[0])),
            thumb: Some(full.join(&pages[0])),
            src: side["src"].as_str().map(String::from),
            dir: Some(full),
            ..Cand::default()
        });
    }
    out
}

// ---------- the copies on the web ----------

/// Where a candidate's first page and PDF are kept (`data/manual-cand`).
fn cache_name(data_dir: &Path, key: &str, ext: &str) -> PathBuf {
    let h = crate::files::hex(&<sha1::Sha1 as sha1::Digest>::digest(key.as_bytes()));
    data_dir.join("manual-cand").join(format!("{}{ext}", &h[..24]))
}

fn is_pdf(f: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 5];
    fs::File::open(f).and_then(|mut x| x.read_exact(&mut head)).is_ok() && &head == b"%PDF-"
}

/// What `Use` needs to fetch a copy that isn't on this computer.
#[derive(Debug, Clone)]
pub enum Remote {
    Archive(String),
    Pdf(String),
    Scans { col: &'static manualcoll::Collection, entry: Box<manualcoll::Entry>, ix: Box<manualcoll::Index> },
}

/// Every copy the web has for a game: the console's collections (three entries each at most), GamesDatabase's PDF
/// and archive.org's uploads; the ones the manual in use came from left out.
pub fn search(web: &dyn Web, data_dir: &Path, g: &Game, skip: &[String], now: f64) -> (Vec<(Cand, Remote)>, bool) {
    let mut found: Vec<(Cand, Remote)> = Vec::new();
    let mut add = |c: Cand, r: Remote| {
        if !skip.contains(&c.key) && !found.iter().any(|x| x.0.key == c.key) {
            found.push((c, r));
        }
    };
    let cache = crate::paths::boxart_cache(data_dir);
    for col in manualcoll::collections(&g.sys) {
        let Ok(ix) = manualcoll::index(web, &cache, col, now) else { continue };
        for e in manualcoll::matches(&ix.entries, g).into_iter().take(3) {
            let tags = e.tags.join(" ");
            let lt = e.tags.iter().find(|t| t.len() >= 5 && t.split(',').all(|x| x.len() == 2 && x.as_bytes()[0].is_ascii_uppercase() && x.as_bytes()[1].is_ascii_lowercase()));
            let lang = lt.map_or("", |t| manuals::lang_code(&t[..2].to_lowercase(), ""));
            static USA: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"\bUSA\b|^U$|World").unwrap());
            static EUR: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"Europe|^E$|\bUK\b").unwrap());
            static JPN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"Japan|^J$").unwrap());
            let region = if USA.is_match(&tags) {
                "usa"
            } else if EUR.is_match(&tags) {
                "eur"
            } else if JPN.is_match(&tags) {
                "jpn"
            } else {
                "usa"
            };
            let c = Cand {
                key: format!("coll:{}/{}", col.id, e.name),
                source: "collection".into(),
                rel: 12.0,
                title: e.title.clone(),
                label: format!("{}{}{}", if col.digital { "Official digital manual · " } else { "Collection scan · " }, e.title, if e.disc > 1 { format!(" (disc {})", e.disc) } else { String::new() }),
                lang: if !lang.is_empty() { lang.into() } else if region == "jpn" { "ja".into() } else { "en".into() },
                region: region.into(),
                digital: col.digital,
                ..Cand::default()
            };
            let remote = if !col.digital && e.jp2 { Remote::Scans { col, entry: Box::new(e.clone()), ix: Box::new(ix.clone()) } } else { Remote::Pdf(format!("https://archive.org/download/{}/{}", js::encode_uri_component(col.id), e.name.split('/').map(js::encode_uri_component).collect::<Vec<_>>().join("/"))) };
            add(c, remote);
        }
    }
    if let Some(h) = manuals::search_games_database(web, &g.sys, &g.title) {
        let url = h.id.trim_start_matches("gdb:").to_string();
        let c = Cand { key: format!("gdb:{url}"), source: "gamesdatabase".into(), rel: 9.5, label: format!("GamesDatabase · {}", h.title), lang: "en".into(), region: Some(manuals::region_of(&h.title)).filter(|r| !r.is_empty()).unwrap_or("usa").into(), title: h.title, ..Cand::default() };
        add(c, Remote::Pdf(url));
    }
    let mut offline = false;
    match manuals::search_archive(web, &g.sys, &g.title, false) {
        Ok(list) => {
            for h in list {
                let lang = Some(manuals::lang_code(&h.language, &h.title)).filter(|l| !l.is_empty()).unwrap_or(if h.language.is_empty() { "en" } else { "" });
                let c = Cand { key: format!("ia:{}", h.id), source: "archive".into(), rel: h.score, label: format!("Internet Archive · {}", h.title), lang: lang.into(), region: manuals::region_of(&h.title).into(), pages: Some(h.pages).filter(|p| *p > 0), title: h.title, ..Cand::default() };
                add(c, Remote::Archive(h.id));
            }
        }
        Err(_) => offline = true,
    }
    (found, offline)
}

fn fetch_pdf(web: &dyn Web, data_dir: &Path, key: &str, url: &str) -> Option<PathBuf> {
    let pdf = cache_name(data_dir, key, ".pdf");
    if !pdf.is_file() {
        fs::create_dir_all(pdf.parent()?).ok()?;
        if !web.file(url, &pdf).unwrap_or(false) {
            return None;
        }
    }
    if !is_pdf(&pdf) {
        let _ = fs::remove_file(&pdf);
        return None;
    }
    Some(pdf)
}

/// Checks a copy can really be had and fetches its first page (a PDF is downloaded whole, so Use doesn't wait for
/// it again): false when it isn't to be shown at all.
pub fn verify(web: &dyn Web, data_dir: &Path, c: &mut Cand, r: &Remote) -> bool {
    static BORROW: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)borrow|inlibrary|printdisabled").unwrap());
    static ZIP: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)_jp2\.zip$").unwrap());
    static IMG: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)\.(jpe?g|png)$").unwrap());
    static THUMB: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)__ia_thumb|_thumb\.").unwrap());
    static GAPS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)incomplete|missing pages|pages missing").unwrap());
    let jpg = cache_name(data_dir, &c.key, ".jpg");
    if fs::create_dir_all(data_dir.join("manual-cand")).is_err() {
        return false;
    }
    let page1 = |url: &str| jpg.is_file() || web.file(url, &jpg).unwrap_or(false);
    match r {
        Remote::Archive(id) => {
            let Ok(meta) = web.json(&format!("https://archive.org/metadata/{}", js::encode_uri_component(id))) else { return false };
            let files: Vec<Value> = meta["files"].as_array().cloned().unwrap_or_default();
            let md = &meta["metadata"];
            let name = |f: &Value| f["name"].as_str().unwrap_or("").to_string();
            let jp2 = files.iter().any(|f| ZIP.is_match(&name(f)));
            if BORROW.is_match(&js::string(md.get("collection"))) && !jp2 {
                return false;
            }
            let mut imgs: Vec<String> = files.iter().map(name).filter(|n| IMG.is_match(n) && !THUMB.is_match(n)).collect();
            imgs.sort_by(|a, b| crate::collate::compare(a, b));
            if !jp2 && imgs.is_empty() {
                return false;
            }
            let count = js::number(md.get("imagecount"));
            c.pages = if count > 0.0 { Some(count as u32) } else if jp2 { c.pages } else { Some(imgs.len() as u32) };
            let lang = manuals::lang_code(&js::string(md.get("language")), &c.title);
            if !lang.is_empty() {
                c.lang = lang.into();
            }
            let region = manuals::region_of(&[c.title.clone(), js::string(md.get("description")), js::string(md.get("subject"))].join(" "));
            if !region.is_empty() {
                c.region = region.into();
            }
            if GAPS.is_match(&format!("{} {}", c.title, js::string(md.get("description")))) {
                c.incomplete = true;
            }
            let enc = js::encode_uri_component;
            let first = if jp2 { format!("https://archive.org/download/{}/page/n0_w1600.jpg", enc(id)) } else { format!("https://archive.org/download/{}/{}", enc(id), enc(&imgs[0])) };
            if !page1(&first) {
                return false;
            }
        }
        Remote::Scans { col, entry, ix } => {
            let Ok(urls) = manualcoll::scan_pages(web, col, ix, entry) else { return false };
            let Some((full, half)) = urls.first() else { return false };
            c.pages = Some(urls.len() as u32);
            if !full.as_ref().is_some_and(|u| page1(u)) && !page1(half) {
                return false;
            }
        }
        Remote::Pdf(url) => {
            let Some(pdf) = fetch_pdf(web, data_dir, &c.key, url) else { return false };
            let Ok(bytes) = fs::read(&pdf) else { return false };
            if !jpg.is_file() {
                let Some(first) = crate::pdfpages::pages(&bytes, 900, 1, false).ok().and_then(|p| p.into_iter().next()) else { return false };
                if fs::write(&jpg, first).is_err() {
                    return false;
                }
            }
            let n = crate::pdfpages::count(&bytes).unwrap_or(0) as u32;
            c.pages = Some(n).filter(|n| *n > 0).or(c.pages);
            c.bpp = if n > 0 { bytes.len() as f64 / f64::from(n) } else { 0.0 };
            c.pdf = Some(pdf);
            c.thumb = Some(jpg);
            return true;
        }
    }
    c.w = width(&jpg);
    c.thumb = Some(jpg);
    true
}

// ---------- putting a manual in place ----------

fn stage_dir(tag: &str) -> Result<PathBuf, String> {
    let d = std::env::temp_dir().join(format!("rustyshelf-manual-studio-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

/// A PDF's pages into `stage`; the count, and whether only its first PDF_CUT came out.
fn pdf_into(pdf: &Path, stage: &Path, base: &str) -> Result<(usize, bool), String> {
    let bytes = fs::read(pdf).map_err(|e| e.to_string())?;
    let pages = crate::pdfpages::pages(&bytes, 1600, PDF_CUT as usize, false)?;
    for (i, p) in pages.iter().enumerate() {
        fs::write(stage.join(format!("{base}-manual-{:02}.jpg", i + 1)), p).map_err(|e| e.to_string())?;
    }
    if pages.is_empty() {
        return Err("the PDF could not be turned into pages".into());
    }
    let cut = pages.len() >= PDF_CUT as usize && crate::pdfpages::count(&bytes).is_none_or(|n| n > pages.len());
    Ok((pages.len(), cut))
}

/// The old manual aside (a new `.manual-originals` folder with a note of what it was), the new pages in: a failure
/// anywhere puts everything back. The backup folder (None when there was no manual) and the pages put in.
fn swap_in(g: &Game, stage: &Path, prev: Option<&Value>) -> Result<(Option<PathBuf>, usize), String> {
    let pages = pages_in(stage, &g.base);
    if pages.is_empty() {
        return Err("No pages came out of that manual.".into());
    }
    fs::create_dir_all(&g.dir).map_err(|e| e.to_string())?;
    let olds = manualcoll::manual_files(&g.dir, &g.base);
    let keep = if olds.is_empty() { None } else { Some(spreads::backup_dir(&g.dir, &g.base).map_err(|e| e.to_string())?) };
    let (mut moved, mut placed) = (Vec::new(), Vec::new());
    let put = (|| -> std::io::Result<()> {
        for f in &olds {
            move_file(&g.dir.join(f), &keep.as_ref().expect("a backup folder").join(f))?;
            moved.push(f.clone());
        }
        for (i, f) in pages.iter().enumerate() {
            let d = g.dir.join(format!("{}-manual-{:02}{}", g.base, i + 1, js::extname(f).to_lowercase()));
            move_file(&stage.join(f), &d)?;
            placed.push(d);
        }
        Ok(())
    })();
    if let Err(e) = put {
        for d in &placed {
            let _ = fs::remove_file(d);
        }
        for f in &moved {
            let _ = move_file(&keep.as_ref().expect("a backup folder").join(f), &g.dir.join(f));
        }
        let _ = keep.as_ref().map(fs::remove_dir);
        return Err(format!("Couldn't put the manual in place ({e}). Is a page open in another program? The old one was kept."));
    }
    if let Some(k) = &keep {
        let _ = fs::write(k.join(SIDE), prev.cloned().unwrap_or(json!({ "label": "Manual from before" })).to_string());
    }
    Ok((keep, placed.len()))
}

fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::rename(from, to).or_else(|_| fs::copy(from, to).and_then(|_| fs::remove_file(from)))
}

/// What a manual put in from a card is on record as.
fn meta_of(c: &Cand) -> Value {
    static EARLIER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"(^| · )Earlier.*$").unwrap());
    let earlier = c.source == "earlier";
    let label = EARLIER.replace(&c.label, "").into_owned();
    json!({ "src": if earlier { c.src.clone().unwrap_or_else(|| c.key.clone()) } else { c.key.clone() }, "label": if earlier && label.is_empty() { "Your manual from before".into() } else { label },
        "lang": c.lang, "region": c.region, "expected": c.pages, "origin": "studio" })
}

/// Puts a card's manual in place of the game's: downloaded (or copied, for an earlier one), turned into pages, cut
/// into single pages. Never a lock: only the Lock switch is. The pages put in, and whether only the first ones
/// were kept.
pub fn apply(web: &dyn Web, data_dir: &Path, g: &Game, c: &Cand, r: Option<&Remote>, now: f64) -> Result<(usize, bool), String> {
    let stage = stage_dir(&format!("{:x}", now as u64))?;
    let done = (|| -> Result<(usize, bool), String> {
        let mut cut = false;
        match (c.source.as_str(), r) {
            ("earlier", _) => {
                let dir = c.dir.as_ref().ok_or("That earlier manual isn't there any more.")?;
                for f in pages_in(dir, &g.base) {
                    fs::copy(dir.join(&f), stage.join(&f)).map_err(|e| e.to_string())?;
                }
            }
            (_, Some(Remote::Archive(id))) => {
                manuals::fetch_manual(web, id, &stage, &g.base, &|_, _| {})?;
                cut = c.pages.is_some_and(|p| p > IA_CUT);
            }
            (_, Some(Remote::Scans { col, entry, ix })) => {
                manualcoll::fetch_to(web, col, ix, entry, &stage, &g.base, &|| false)?;
            }
            (_, Some(Remote::Pdf(url))) => {
                let pdf = c.pdf.clone().filter(|p| p.is_file()).or_else(|| fetch_pdf(web, data_dir, &c.key, url)).ok_or("the PDF could not be downloaded")?;
                cut = pdf_into(&pdf, &stage, &g.base)?.1;
            }
            _ => return Err("That version isn't in the list any more. Open the game again.".into()),
        }
        let scan = c.source == "collection" && !c.digital;
        let _ = spreads::split_manual(&stage, &g.base, spreads::Split { scan, min_w: 0, discard: true });
        let mut db = Db::load(data_dir);
        let prev = cur_meta(data_dir, &db, g);
        let (keep, n) = swap_in(g, &stage, prev.as_ref())?;
        let mut meta = meta_of(c);
        if cut {
            meta["expected"] = json!(n);
        }
        let key = g.key();
        db.remember(&key, keep.as_deref(), meta, now);
        // an earlier copy put back is moved, not copied: it leaves the Earlier list (Undo still has the one it replaced)
        if let (Some(dir), true) = (&c.dir, c.source == "earlier") {
            if keep.as_ref() != Some(dir) {
                let _ = fs::remove_dir_all(dir);
                let gone = dir.to_string_lossy().to_lowercase();
                if let Some(h) = db.part("hist").get_mut(&key).and_then(Value::as_array_mut) {
                    h.retain(|e| e["dir"].as_str().is_none_or(|d| d.to_lowercase() != gone));
                }
            }
        }
        db.save()?;
        Ok((n, cut))
    })();
    let _ = fs::remove_dir_all(&stage);
    done
}

/// The previous manual back; the one in use goes aside in its turn (so this can be undone by choosing it under
/// Earlier).
pub fn undo(data_dir: &Path, g: &Game) -> Result<(), String> {
    let mut db = Db::load(data_dir);
    let key = g.key();
    let last = db.all["hist"][key.as_str()].as_array().and_then(|h| h.last()).cloned().ok_or("Nothing to undo")?;
    let from_dir = last["dir"].as_str().map(PathBuf::from);
    let pop = |db: &mut Db| {
        if let Some(h) = db.part("hist").get_mut(&key).and_then(Value::as_array_mut) {
            h.pop();
        }
    };
    if let Some(d) = &from_dir {
        if manualcoll::manual_files(d, &g.base).is_empty() {
            pop(&mut db);
            db.save()?;
            return Err("The earlier manual isn't there any more.".into());
        }
    }
    let olds = manualcoll::manual_files(&g.dir, &g.base);
    let keep = if olds.is_empty() { None } else { Some(spreads::backup_dir(&g.dir, &g.base).map_err(|e| e.to_string())?) };
    let from = from_dir.as_ref().map(|d| manualcoll::manual_files(d, &g.base)).unwrap_or_default();
    let (mut moved, mut back) = (Vec::new(), Vec::new());
    let put = (|| -> std::io::Result<()> {
        for f in &olds {
            move_file(&g.dir.join(f), &keep.as_ref().expect("a backup folder").join(f))?;
            moved.push(f.clone());
        }
        fs::create_dir_all(&g.dir)?;
        for f in &from {
            move_file(&from_dir.as_ref().expect("an earlier folder").join(f), &g.dir.join(f))?;
            back.push(f.clone());
        }
        Ok(())
    })();
    if let Err(e) = put {
        for f in &back {
            let _ = move_file(&g.dir.join(f), &from_dir.as_ref().expect("an earlier folder").join(f));
        }
        for f in &moved {
            let _ = move_file(&keep.as_ref().expect("a backup folder").join(f), &g.dir.join(f));
        }
        let _ = keep.as_ref().map(fs::remove_dir);
        return Err(format!("Couldn't undo ({e}). Nothing was changed."));
    }
    if let Some(k) = &keep {
        let _ = fs::write(k.join(SIDE), cur_meta(data_dir, &db, g).unwrap_or(json!({ "label": "Manual from before" })).to_string());
    }
    if let Some(d) = &from_dir {
        let _ = fs::remove_file(d.join(SIDE));
        // (only when it's empty now)
        let _ = fs::remove_dir(d);
    }
    pop(&mut db);
    match last.get("cur").filter(|c| c.is_object()) {
        Some(c) => {
            db.part("cur").insert(key.clone(), c.clone());
        }
        None => {
            db.part("cur").remove(&key);
        }
    }
    db.save()
}

/// Your own PDF as the game's manual: copied (never moved or changed), turned into pages, put in. Whether only its
/// first pages were kept.
pub fn own(data_dir: &Path, g: &Game, src: &Path, now: f64) -> Result<(usize, bool), String> {
    let meta = fs::metadata(src).map_err(|_| "That file isn't there any more.".to_string())?;
    if !meta.is_file() {
        return Err("That isn't a file.".into());
    }
    if meta.len() < 1024 {
        return Err("That PDF is empty.".into());
    }
    if meta.len() > MAX_PDF {
        return Err("That PDF is over 400 MB: too big for a game manual.".into());
    }
    let tmp = stage_dir(&format!("own-{:x}", now as u64))?;
    let done = (|| -> Result<(usize, bool), String> {
        let copy = tmp.join("own.pdf");
        fs::copy(src, &copy).map_err(|e| e.to_string())?;
        if !is_pdf(&copy) {
            return Err("That file isn't a PDF.".into());
        }
        let stage = tmp.join("pages");
        fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
        let (_, cut) = pdf_into(&copy, &stage, &g.base)?;
        let _ = spreads::split_manual(&stage, &g.base, spreads::Split { scan: false, min_w: 0, discard: true });
        let pages = fs::read(&copy).ok().and_then(|b| crate::pdfpages::count(&b));
        let mut db = Db::load(data_dir);
        let prev = cur_meta(data_dir, &db, g);
        let (keep, n) = swap_in(g, &stage, prev.as_ref())?;
        let name = src.file_stem().map_or(String::new(), |s| s.to_string_lossy().into_owned());
        let expected = if cut { Some(n) } else { pages };
        db.remember(&g.key(), keep.as_deref(), json!({ "src": "own", "label": format!("Your PDF · {name}"), "lang": "", "region": "", "expected": expected, "origin": "own" }), now);
        db.save()?;
        Ok((n, cut))
    })();
    let _ = fs::remove_dir_all(&tmp);
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoWeb;
    impl Web for NoWeb {
        fn json(&self, _: &str) -> Result<Value, manuals::WebError> {
            panic!("asked the web")
        }
        fn text(&self, _: &str) -> Result<String, manuals::WebError> {
            panic!("asked the web")
        }
        fn file(&self, _: &str, _: &Path) -> Result<bool, manuals::WebError> {
            panic!("asked the web")
        }
    }

    #[test]
    fn your_own_pdf_goes_in_and_undo_brings_the_old_manual_back() {
        let tmp = tempfile::tempdir().unwrap();
        let (data, covers) = (tmp.path().join("data"), tmp.path().join("covers"));
        fs::create_dir_all(&covers).unwrap();
        let g = Game { sys: "gc".into(), system: "GameCube".into(), title: "Zeta Run".into(), title_id: None, file: "/roms/Zeta Run (USA).iso".into(), region: "USA".into(), dir: covers.clone(), base: "Zeta Run (USA)".into() };
        let page = |n: u8| {
            let mut b = Vec::new();
            image::codecs::jpeg::JpegEncoder::new(&mut b).encode_image(&image::RgbImage::from_pixel(700, 1000, image::Rgb([n, 0, 0]))).unwrap();
            b
        };
        for i in 1..=2 {
            fs::write(covers.join(format!("Zeta Run (USA)-manual-0{i}.jpg")), page(i)).unwrap();
        }
        let old = fs::read(covers.join("Zeta Run (USA)-manual-01.jpg")).unwrap();
        let pdf = tmp.path().join("Mine.pdf");
        fs::write(&pdf, crate::pdfpages::tests::pdf_of(3)).unwrap();
        assert_eq!(own(&data, &g, &pdf, 1.0).unwrap(), (3, false));
        assert!(pdf.is_file(), "your file stays where it is");
        assert_eq!(pages_in(&covers, &g.base).len(), 3);
        let db = Db::load(&data);
        assert_eq!((db.cur(&g.key()).unwrap()["origin"].as_str(), db.cur(&g.key()).unwrap()["label"].as_str(), db.can_undo(&g.key())), (Some("own"), Some("Your PDF · Mine"), true));
        // the old manual is an earlier copy now, and the current one is listed as what's in use
        let earlier = earlier_cands(&g);
        assert_eq!((earlier.len(), earlier[0].pages), (1, Some(2)));
        let cur = current_cand(&data, &db, &g).unwrap();
        assert_eq!((cur.pages, cur.label.as_str(), cur.in_use), (Some(3), "Your PDF · Mine", true));
        undo(&data, &g).unwrap();
        assert_eq!(pages_in(&covers, &g.base).len(), 2);
        assert_eq!(fs::read(covers.join("Zeta Run (USA)-manual-01.jpg")).unwrap(), old);
        assert!(Db::load(&data).cur(&g.key()).is_none() && !Db::load(&data).can_undo(&g.key()));
        // the PDF's pages went aside in their turn: put back from Earlier
        let back = earlier_cands(&g).into_iter().find(|c| c.pages == Some(3)).unwrap();
        assert_eq!(apply(&NoWeb, &data, &g, &back, None, 2.0).unwrap(), (3, false));
        assert_eq!(pages_in(&covers, &g.base).len(), 3);
        assert!(earlier_cands(&g).iter().all(|c| c.pages != Some(3)), "an earlier copy put back leaves the list");
        assert!(undo(&data, &g).is_ok());
        assert!(own(&data, &g, &tmp.path().join("gone.pdf"), 3.0).is_err());
        fs::write(tmp.path().join("fake.pdf"), vec![b'x'; 4096]).unwrap();
        assert_eq!(own(&data, &g, &tmp.path().join("fake.pdf"), 4.0).unwrap_err(), "That file isn't a PDF.");
    }
}
