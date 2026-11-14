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

fn fold(c: char) -> char {
    let c = match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'a',
        'ç' | 'Ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'È' | 'É' | 'Ê' | 'Ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => 'i',
        'ñ' | 'Ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => 'u',
        'ý' | 'ÿ' | 'Ý' => 'y',
        c => c,
    };
    c.to_lowercase().next().unwrap_or(c)
}

pub fn compare(a: &str, b: &str) -> Ordering {
    compare_with(a, b, true)
}

/// `localeCompare` without numeric order ("Mario 10" before "Mario 2"), as Steam's games are sorted.
pub fn compare_plain(a: &str, b: &str) -> Ordering {
    compare_with(a, b, false)
}

fn compare_with(a: &str, b: &str, numeric: bool) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if numeric && c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(&c) = it.peek().filter(|c| c.is_ascii_digit()) {
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
                let (c, d) = (fold(c), fold(d));
                let o = class(c).cmp(&class(d)).then_with(|| c.cmp(&d));
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
    }
}
