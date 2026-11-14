//! The shelf's title order: what `Intl.Collator(undefined, { sensitivity: 'base', numeric: true })` gives in the
//! Electron app, close enough for game titles. Case doesn't count, digit runs compare as numbers ("Mario 2" before
//! "Mario 10"), and spaces and punctuation come before digits, digits before letters.

use std::cmp::Ordering;

fn class(c: char) -> u8 {
    if c.is_alphabetic() {
        2
    } else if c.is_ascii_digit() {
        1
    } else {
        0
    }
}

/// Where a space or a punctuation mark sorts among the others, as ICU's root collation has them.
fn punct_rank(c: char) -> u32 {
    const ORDER: &str = "_-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";
    if c.is_whitespace() {
        return 0;
    }
    match ORDER.find(c) {
        Some(i) => 1 + i as u32,
        None => 100 + c as u32,
    }
}

/// A title as the collator compares it at its first level: accents off, case off, ligatures spelt out.
fn folded(s: &str) -> Vec<char> {
    use unicode_normalization::UnicodeNormalization;
    let mut out = Vec::with_capacity(s.len());
    // compatibility forms too, as ICU compares them: ™ as TM, ﬁ as fi, ² as 2, full-width letters as plain ones
    for c in s.nfkd() {
        if ('\u{300}'..='\u{36f}').contains(&c) {
            continue;
        }
        match c {
            'Œ' | 'œ' => out.extend(['o', 'e']),
            'Æ' | 'æ' => out.extend(['a', 'e']),
            'ß' => out.extend(['s', 's']),
            'Þ' | 'þ' => out.extend(['t', 'h']),
            'Ł' | 'ł' => out.push('l'),
            'Ø' | 'ø' => out.push('o'),
            'Đ' | 'đ' | 'Ð' | 'ð' => out.push('d'),
            'ı' => out.push('i'),
            c => out.extend(c.to_lowercase()),
        }
    }
    out
}

pub fn compare(a: &str, b: &str) -> Ordering {
    compare_with(a, b, true)
}

/// `localeCompare` without numeric order ("Mario 10" before "Mario 2"), as Steam's games are sorted.
pub fn compare_plain(a: &str, b: &str) -> Ordering {
    compare_with(a, b, false)
}

/// `localeCompare` as it is: titles that only differ by an accent or by case still have an order, the plain letter
/// first, then the small one.
pub fn compare_exact(a: &str, b: &str) -> Ordering {
    use unicode_normalization::UnicodeNormalization;
    let marks = |s: &str| s.nfd().filter(|c| ('\u{300}'..='\u{36f}').contains(c)).collect::<Vec<char>>();
    let upper = |s: &str| s.chars().map(char::is_uppercase).collect::<Vec<bool>>();
    compare_plain(a, b).then_with(|| marks(a).cmp(&marks(b))).then_with(|| upper(a).cmp(&upper(b)))
}

fn compare_with(a: &str, b: &str, numeric: bool) -> Ordering {
    let (a, b) = (folded(a), folded(b));
    let (mut x, mut y) = (a.iter().copied().peekable(), b.iter().copied().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if numeric && c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::iter::Copied<std::slice::Iter<char>>>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(|c| c.is_ascii_digit()) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (n, m) = (take(&mut x), take(&mut y));
                let (n, m) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
                let o = n.len().cmp(&m.len()).then_with(|| n.cmp(m));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(c), Some(d)) => {
                let o = class(c).cmp(&class(d)).then_with(|| if class(c) == 0 { punct_rank(c).cmp(&punct_rank(d)) } else { c.cmp(&d) });
                if o != Ordering::Equal {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order() {
        let mut v = vec!["mario 10", "Mario 2", "Zelda", "Éclair", "1942", "Ape"];
        v.sort_by(|a, b| compare(a, b));
        assert_eq!(v, ["1942", "Ape", "Éclair", "Mario 2", "mario 10", "Zelda"]);
        v.sort_by(|a, b| compare_plain(a, b));
        assert_eq!(v, ["1942", "Ape", "Éclair", "mario 10", "Mario 2", "Zelda"]);
        // as node's localeCompare orders them
        let mut v = vec!["Zuma", "Šachy", "S.T.A.L.K.E.R.: X", "S!X", "S:X", "Portal 2", "Outlast", "Ōkami HD", "Œuvre", "Ärger"];
        v.sort_by(|a, b| compare_plain(a, b));
        assert_eq!(v, ["Ärger", "Œuvre", "Ōkami HD", "Outlast", "Portal 2", "S:X", "S!X", "S.T.A.L.K.E.R.: X", "Šachy", "Zuma"]);
        let mut v = vec!["Foo™", "Foob", "Foo", "ﬁx", "fiz", "Pokemon2", "Pokémon²", "Pokemon 2"];
        v.sort_by(|a, b| compare_plain(a, b));
        assert_eq!(v, ["ﬁx", "fiz", "Foo", "Foob", "Foo™", "Pokemon 2", "Pokemon2", "Pokémon²"]);
        let mut v = vec!["A$b", "A#b", "A&b", "A/b", "A(b", "A?b", "A;b", "A,b", "A-b", "A_b", "A b"];
        v.sort_by(|a, b| compare_plain(a, b));
        assert_eq!(v, ["A b", "A_b", "A-b", "A,b", "A;b", "A?b", "A(b", "A/b", "A&b", "A#b", "A$b"]);
        let want = [
            "Ärger", "Final Fantasy X", "Final Fantasy X-2", "Final Fantasy XII", "Ł", "Lz", "mario 2", "Mario 10", "Mario_2", "Mario-2",
            "Mario: 2", "Œuvre", "Ōkami HD", "S:X", "S.T.A.L.K.E.R.: X", "Šachy 9", "Šachy 10", "Zuma 2",
        ];
        let mut v = want.to_vec();
        v.reverse();
        v.sort_by(|a, b| compare(a, b));
        assert_eq!(v, want);
    }
}
