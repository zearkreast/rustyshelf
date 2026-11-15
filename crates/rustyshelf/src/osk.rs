// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from the
// on-screen keyboard in Boxshelf's renderer/settings.js, as RustyShelf has it.
//! The on-screen keyboard a text setting is typed on with a pad: three layers of keys and a row of actions.

const SMALL: [&str; 4] = ["1234567890", "qwertyuiop", "asdfghjkl'", "zxcvbnm-.⌫"];
const CAPITALS: [&str; 4] = ["1234567890", "QWERTYUIOP", "ASDFGHJKL\"", "ZXCVBNM_:⌫"];
const SIGNS: [&str; 4] = ["!@#$%^&*()", "\\/:;_=+{}~", "[]<>|?,`'\"", "€£¥§°·…-.⌫"];
const ACTIONS: [&str; 7] = ["Shift", "#+=", "Space", "Paste", "Clear", "Cancel", "Done"];

/// What pressing a key does.
#[derive(Debug, PartialEq, Eq)]
pub enum Hit {
    Type(char),
    Back,
    /// what's on the clipboard
    Paste,
    Clear,
    Cancel,
    Done,
    /// to this layer: 0 small letters, 1 capitals, 2 signs
    Layer(usize),
}

/// The keys of a layer, row by row, the actions last.
pub fn rows(layer: usize) -> Vec<Vec<String>> {
    let keys = [SMALL, CAPITALS, SIGNS][layer.min(2)];
    let mut out: Vec<Vec<String>> = keys.iter().map(|r| r.chars().map(String::from).collect()).collect();
    out.push(ACTIONS.iter().map(|a| a.to_string()).collect());
    out
}

/// The key a step lands on from (row, column): sideways goes round its row, up and down keep the place across rows
/// of different lengths.
pub fn step(layer: usize, at: (usize, usize), dx: i32, dy: i32) -> (usize, usize) {
    let rows = rows(layer);
    let (row, col) = (at.0.min(rows.len() - 1), at.1);
    let n = rows[row].len();
    let col = col.min(n - 1);
    if dx != 0 {
        return (row, (col as i32 + dx).rem_euclid(n as i32) as usize);
    }
    let next = row as i32 + dy;
    if dy == 0 || next < 0 || next >= rows.len() as i32 {
        return (row, col);
    }
    let m = rows[next as usize].len();
    let across = (col as f64 + 0.5) / n as f64;
    (next as usize, ((across * m as f64).floor() as usize).min(m - 1))
}

/// What the key at (row, column) does. A capital is one letter: the layer goes back to small letters after it.
pub fn press(layer: usize, at: (usize, usize)) -> Vec<Hit> {
    let rows = rows(layer);
    let Some(key) = rows.get(at.0).and_then(|r| r.get(at.1)) else { return Vec::new() };
    match key.as_str() {
        "⌫" => vec![Hit::Back],
        "Space" => vec![Hit::Type(' ')],
        "Paste" => vec![Hit::Paste],
        "Clear" => vec![Hit::Clear],
        "Cancel" => vec![Hit::Cancel],
        "Done" => vec![Hit::Done],
        "Shift" => vec![Hit::Layer(if layer == 1 { 0 } else { 1 })],
        "#+=" => vec![Hit::Layer(if layer == 2 { 0 } else { 2 })],
        k => {
            let mut hits: Vec<Hit> = k.chars().next().map(Hit::Type).into_iter().collect();
            if layer == 1 {
                hits.push(Hit::Layer(0));
            }
            hits
        }
    }
}

/// Text from the clipboard as it goes into a one-line setting: its lines joined, nothing that can't be typed. None
/// when there's no text on it.
pub fn clipboard() -> Option<String> {
    let text = arboard::Clipboard::new().ok()?.get_text().ok()?;
    let line: String = text.split(['\r', '\n']).map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ").chars().filter(|c| !c.is_control()).collect();
    Some(line).filter(|l| !l.is_empty())
}

/// What was typed as it's kept: trimmed, and the quotes off a path pasted in quotes.
pub fn settled(text: &str) -> String {
    let t = text.trim();
    match t.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        // the Electron app takes the outer quotes off anything, which breaks options like `"-a" "{rom}"`
        Some(inner) if !inner.contains('"') && !inner.contains('\n') => inner.to_string(),
        _ => t.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_and_presses() {
        assert_eq!(rows(0)[2][9], "'");
        assert_eq!(step(0, (0, 9), 1, 0), (0, 0));
        assert_eq!(step(0, (0, 0), -1, 0), (0, 9));
        assert_eq!(step(0, (0, 3), 0, -1), (0, 3));
        // ten keys over seven actions and back
        assert_eq!(step(0, (3, 9), 0, 1), (4, 6));
        assert_eq!(step(0, (3, 4), 0, 1), (4, 3));
        assert_eq!(step(0, (4, 3), 0, -1), (3, 5));
        assert_eq!(step(0, (4, 6), 0, 1), (4, 6));
        assert_eq!(press(0, (1, 0)), vec![Hit::Type('q')]);
        assert_eq!(press(1, (1, 0)), vec![Hit::Type('Q'), Hit::Layer(0)]);
        assert_eq!(press(2, (1, 0)), vec![Hit::Type('\\')]);
        assert_eq!(press(0, (3, 9)), vec![Hit::Back]);
        assert_eq!(press(0, (4, 0)), vec![Hit::Layer(1)]);
        assert_eq!(press(1, (4, 0)), vec![Hit::Layer(0)]);
        assert_eq!(press(2, (4, 1)), vec![Hit::Layer(0)]);
        assert_eq!(press(0, (4, 2)), vec![Hit::Type(' ')]);
        assert_eq!(press(0, (4, 3)), vec![Hit::Paste]);
        assert_eq!(press(0, (4, 6)), vec![Hit::Done]);
    }

    #[test]
    fn keeps_what_was_typed() {
        assert_eq!(settled("  -b -e \"{rom}\" "), "-b -e \"{rom}\"");
        assert_eq!(settled("\"C:\\Games\\a b.exe\""), "C:\\Games\\a b.exe");
        assert_eq!(settled("\"-a\" \"{rom}\""), "\"-a\" \"{rom}\"");
        assert_eq!(settled("\""), "\"");
    }
}
