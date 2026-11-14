// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/kyty.js (readParam, gameDir), as RustyShelf has it.
//! PS5 games: folders with eboot.bin and sce_sys/param.json. Kyty is handed the folder, not the eboot.bin.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// The game folder for a shelf file (…/Game/eboot.bin → …/Game).
pub fn game_dir(file: &Path) -> PathBuf {
    let is_eboot = file.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("eboot.bin"));
    match file.parent() {
        Some(p) if is_eboot => p.to_path_buf(),
        _ => file.to_path_buf(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// in the game's default language, else US English
    pub title: String,
    pub title_id: String,
}

/// sce_sys/param.json of the game a shelf file (or folder) belongs to.
pub fn read_param(file: &Path) -> Option<Param> {
    let text = std::fs::read_to_string(game_dir(file).join("sce_sys").join("param.json")).ok()?;
    let j: Value = serde_json::from_str(&text).ok()?;
    let l = j.get("localizedParameters");
    let lang = l.and_then(|l| l.get("defaultLanguage")).and_then(Value::as_str).unwrap_or("en-US");
    let name = |lang: &str| l.and_then(|l| l.get(lang)).and_then(|o| o.get("titleName")).filter(|v| truthy(v)).map(text_of);
    let title = name(lang).or_else(|| name("en-US")).unwrap_or_default();
    let title_id = j.get("titleId").filter(|v| truthy(v)).map(text_of).unwrap_or_default();
    Some(Param { title: title.split_whitespace().collect::<Vec<_>>().join(" "), title_id: title_id.trim().to_string() })
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null | Value::Bool(false) => false,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        _ => true,
    }
}

/// JS String(v)
fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("sce_sys")).unwrap();
        std::fs::write(
            t.path().join("sce_sys/param.json"),
            r#"{"titleId":"PPSA01234 ","localizedParameters":{"defaultLanguage":"ja-JP","ja-JP":{"titleName":""},"en-US":{"titleName":"Some  Game"}}}"#,
        )
        .unwrap();
        let p = read_param(&t.path().join("eboot.bin")).unwrap();
        assert_eq!((p.title.as_str(), p.title_id.as_str()), ("Some Game", "PPSA01234"));
        assert_eq!(game_dir(&t.path().join("EBOOT.BIN")), t.path());
    }
}
