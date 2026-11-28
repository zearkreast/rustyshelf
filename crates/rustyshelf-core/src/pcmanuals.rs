//! A PC game's manual, read like a console game's: one the player gave it (Settings → Box art), the PDF that came
//! in the game's own folder (GOG's Manual.pdf, a Steam game's manual folder), or the manual Steam keeps for the
//! game (its store's /manual/<appid> page sends you to the PDF). Each is cut into pages in `data/pc-covers`,
//! under the game's id, the way `pccovers::manual` reads them; a game's own file is copied there first, never
//! changed where it is.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

use crate::art::Art;
use crate::library::{Library, ShelfKind};
use crate::pccovers;
use crate::scanner::Source;

/// Steam games looked at in one run at most, and how long a game Steam has no manual for waits to be asked again.
const STEAM_RUN: usize = 40;
const STEAM_RETRY_MS: f64 = 30.0 * 24.0 * 3600.0 * 1000.0;
const STEAM_FILE: &str = "steam-manuals.json";

/// The manuals already tried this session: one that wouldn't turn into pages isn't read again on every scan.
static TRIED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Gives a PC game the manual it has: pages or a PDF in `data/pc-covers`, else a PDF in its folder.
pub fn attach(data: &Path, id: &str, folder: Option<&Path>, art: &mut Art) {
    if !art.manual.is_empty() || art.manual_pdf.is_some() {
        return;
    }
    let (pages, pdf) = pccovers::manual(data, id);
    art.manual_pdf = pdf.or_else(|| folder.filter(|_| pages.is_empty()).and_then(in_folder));
    art.manual = pages;
}

/// A manual that came with a game: a PDF named like one (manual, handbook) in its folder, or any PDF in a folder of
/// its named Manual(s) or Docs.
pub fn in_folder(dir: &Path) -> Option<PathBuf> {
    let pdfs = |d: &Path| -> Vec<PathBuf> {
        let mut list: Vec<PathBuf> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf"))).collect();
        list.sort();
        list
    };
    let named = |p: &Path| p.file_stem().is_some_and(|s| {
        let s = s.to_string_lossy().to_lowercase();
        s.contains("manual") || s.contains("handbook")
    });
    if let Some(f) = pdfs(dir).into_iter().find(|p| named(p)) {
        return Some(f);
    }
    let mut subs: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    subs.sort();
    subs.into_iter()
        .filter(|d| d.file_name().is_some_and(|n| matches!(n.to_string_lossy().to_lowercase().as_str(), "manual" | "manuals" | "docs" | "doc" | "documentation")))
        .find_map(|d| {
            let all = pdfs(&d);
            all.iter().find(|p| named(p)).or(all.first()).cloned()
        })
}

/// Whether a redirect from Steam's /manual/ page is to the manual (a PDF in the game's manuals), not the store.
fn steam_pdf(location: &str) -> bool {
    let path = location.split('?').next().unwrap_or("").to_lowercase();
    path.contains("/manuals/") && path.ends_with(".pdf")
}

/// After a scan: the PC shelves' manuals turned into pages, and the Steam games without one asked of Steam (a few
/// dozen a run, each again a month later). True when a manual changed.
pub fn upkeep(lib: &Library, data: &Path) -> bool {
    let dir = data.join("pc-covers");
    let mut changed = false;
    let mut steam = Vec::new();
    for g in lib.shelves.iter().filter(|s| matches!(s.kind, ShelfKind::Pc)).flat_map(|s| &s.games) {
        if !g.art.manual.is_empty() {
            continue;
        }
        if let Some(pdf) = &g.art.manual_pdf {
            changed |= to_pages(pdf, &dir, &g.id);
        } else if let Source::Steam { appid, .. } = &g.source {
            steam.push((g.id.clone(), appid.clone()));
        }
    }
    if steam.is_empty() {
        return changed;
    }
    let file = data.join(STEAM_FILE);
    let mut seen = crate::files::read_object(&file);
    let now = crate::now_ms() as f64;
    let due = |appid: &str, seen: &serde_json::Map<String, Value>| seen.get(appid).is_none_or(|v| now - crate::js::number(v.get("at")) > STEAM_RETRY_MS);
    let mut asked = 0;
    for (id, appid) in steam {
        if asked >= STEAM_RUN || !due(&appid, &seen) {
            continue;
        }
        asked += 1;
        let Ok(location) = crate::net::redirect(&format!("https://store.steampowered.com/manual/{appid}/"), Duration::from_secs(20)) else { break };
        let found = location.filter(|l| steam_pdf(l));
        if let Some(url) = &found {
            let pdf = dir.join(format!("{}-manual.pdf", pccovers::safe_id(&id)));
            if std::fs::create_dir_all(&dir).is_err() || crate::net::download(url, &pdf, Duration::from_secs(120), &mut |_| {}).is_err() {
                let _ = std::fs::remove_file(&pdf);
                continue;
            }
            to_pages(&pdf, &dir, &id);
            changed = true;
        }
        seen.insert(appid, json!({ "at": now, "found": found.is_some() }));
    }
    let _ = crate::files::write_atomic(&file, Value::Object(seen).to_string());
    changed
}

/// Cuts a PDF into the game's pages in `data/pc-covers`, once a session. A PDF of the game's own is copied there
/// first (turning it into pages takes the PDF away). True when there are pages now.
fn to_pages(pdf: &Path, dir: &Path, id: &str) -> bool {
    let base = pccovers::safe_id(id);
    {
        let mut tried = TRIED.lock().unwrap();
        if !tried.get_or_insert_with(HashSet::new).insert(format!("{id}|{}", pdf.display())) {
            return false;
        }
    }
    let ours = dir.join(format!("{base}-manual.pdf"));
    if pdf != ours.as_path() && std::fs::create_dir_all(dir).and_then(|()| std::fs::copy(pdf, &ours)).is_err() {
        return false;
    }
    crate::manuals::pdf_to_pages(&ours, dir, &base).is_ok_and(|n| n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manual_is_found_in_the_game_s_folder_and_not_mistaken() {
        let d = std::env::temp_dir().join(format!("rs-pcman-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let game = d.join("Zeta");
        std::fs::create_dir_all(game.join("Docs")).unwrap();
        std::fs::write(game.join("EULA.pdf"), b"%PDF").unwrap();
        assert_eq!(in_folder(&game), None);
        std::fs::write(game.join("Docs").join("zeta.pdf"), b"%PDF").unwrap();
        assert_eq!(in_folder(&game), Some(game.join("Docs").join("zeta.pdf")));
        std::fs::write(game.join("Zeta Manual.PDF"), b"%PDF").unwrap();
        assert_eq!(in_folder(&game), Some(game.join("Zeta Manual.PDF")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn only_a_redirect_to_the_manual_counts() {
        assert!(steam_pdf("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/245390/manuals/IHNMAIMS Manual.pdf?t=1773355242"));
        assert!(!steam_pdf("https://store.steampowered.com/"));
        assert!(!steam_pdf("https://store.steampowered.com/app/620/manuals.html"));
    }

    #[test]
    fn a_game_s_own_pdf_is_kept_where_it_is() {
        let d = std::env::temp_dir().join(format!("rs-pcman-own-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (game, covers) = (d.join("Zeta"), d.join("pc-covers"));
        std::fs::create_dir_all(&game).unwrap();
        let own = game.join("Manual.pdf");
        std::fs::write(&own, b"not really a pdf").unwrap();
        assert!(!to_pages(&own, &covers, "gog-zeta"));
        assert!(own.is_file(), "the game's file is untouched");
        assert!(covers.join("gog-zeta-manual.pdf").is_file(), "a copy is ours to read, or open as a PDF");
        let _ = std::fs::remove_dir_all(&d);
    }
}
