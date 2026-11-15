// From sassiestrhihno's 1.82.0-beta.12 and beta.14 sources (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported
// from the matching in renderer/find.js, as RustyShelf has it.
//! Find a game: every shelf searched by title. The best matches first: a title that starts with what was typed,
//! then a word of it that does, then anywhere in it; capitals, accents and punctuation don't matter. With nothing
//! typed, the games played last.

use unicode_normalization::UnicodeNormalization;

/// The most matches listed, and the most games played last.
pub const MOST: usize = 40;
pub const RECENT: usize = 12;

/// A game as the search sees it.
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: String,
    pub title: String,
    /// its shelf's name
    pub shelf: String,
    /// when it was last played (milliseconds since 1970), 0 for never
    pub last: u64,
    /// taken off its shelf: listed after the games on one
    pub hidden: bool,
    folded: String,
    joined: String,
}

impl Entry {
    pub fn new(id: &str, title: &str, shelf: &str, last: u64) -> Entry {
        let folded = fold(title);
        Entry { id: id.to_string(), title: title.to_string(), shelf: shelf.to_string(), last, hidden: false, joined: folded.replace(' ', ""), folded }
    }
}

/// A title as it's compared: lower case, accents off, "&" as "and", anything that isn't a letter or a digit a
/// space ("Pokémon: Red" is "pokemon red").
pub fn fold(text: &str) -> String {
    let plain: String = text.nfd().filter(|c| !('\u{300}'..='\u{36f}').contains(c)).collect::<String>().to_lowercase().replace('&', " and ");
    plain.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

/// The games that match `typed`, best first (their places in `games`, `MOST` at most), and how many match in all.
pub fn search(games: &[Entry], typed: &str) -> (Vec<usize>, usize) {
    let n = fold(typed);
    if n.is_empty() {
        let mut played: Vec<usize> = (0..games.len()).filter(|&i| games[i].last > 0).collect();
        played.sort_by(|&a, &b| games[b].last.cmp(&games[a].last));
        played.truncate(RECENT);
        let total = played.len();
        return (played, total);
    }
    let joined = n.replace(' ', "");
    let words: Vec<&str> = n.split(' ').collect();
    let starts_a_word = |title: &str, word: &str| format!(" {title}").contains(&format!(" {word}"));
    let mut found: Vec<(u8, usize)> = Vec::new();
    for (i, g) in games.iter().enumerate() {
        let t = g.folded.as_str();
        let rank = if t == n {
            0
        } else if t.starts_with(&n) {
            1
        } else if starts_a_word(t, &n) {
            2
        } else if t.contains(&n) {
            3
        // ("spiderman" finds "Spider-Man")
        } else if g.joined.starts_with(&joined) {
            1
        } else if g.joined.contains(&joined) {
            3
        // every word typed starts a word, in any order; then every word typed is somewhere
        } else if words.len() > 1 && words.iter().all(|w| starts_a_word(t, w)) {
            4
        } else if words.len() > 1 && words.iter().all(|w| t.contains(w)) {
            5
        } else {
            continue;
        };
        found.push((rank, i));
    }
    // (as JavaScript compares strings: by their UTF-16 units)
    let units = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    found.sort_by(|a, b| a.0.cmp(&b.0).then(games[a.1].hidden.cmp(&games[b.1].hidden)).then_with(|| units(&games[a.1].folded).cmp(&units(&games[b.1].folded))).then_with(|| units(&games[a.1].shelf).cmp(&units(&games[b.1].shelf))));
    let total = found.len();
    (found.into_iter().take(MOST).map(|f| f.1).collect(), total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hidden_game_comes_after_the_ones_on_a_shelf() {
        let mut games = vec![Entry::new("a", "Zeta Run", "SNES", 0), Entry::new("b", "Zeta Run", "Genesis", 0)];
        games[0].hidden = true;
        assert_eq!(search(&games, "zeta").0, [1, 0]);
    }
}
