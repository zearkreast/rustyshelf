// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/scanner.js (parseSfo), as RustyShelf has it.
//! PlayStation `param.sfo`: a small key/value table with TITLE, TITLE_ID and CATEGORY.

use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SfoValue {
    Text(String),
    Int(u32),
}

pub type Sfo = HashMap<String, SfoValue>;

pub fn read(file: &Path) -> Option<Sfo> {
    parse(&std::fs::read(file).ok()?)
}

pub fn text<'a>(sfo: &'a Sfo, key: &str) -> Option<&'a str> {
    match sfo.get(key) {
        Some(SfoValue::Text(s)) => Some(s),
        _ => None,
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn zstr(b: &[u8], from: usize, max: usize) -> Option<String> {
    let s = b.get(from..max.min(b.len()))?;
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    Some(String::from_utf8_lossy(&s[..end]).into_owned())
}

pub fn parse(b: &[u8]) -> Option<Sfo> {
    if u32_at(b, 0)? != 0x4653_5000 {
        return None;
    }
    let key_tab = u32_at(b, 8)? as usize;
    let data_tab = u32_at(b, 12)? as usize;
    let n = u32_at(b, 16)? as usize;
    let mut out = Sfo::new();
    for i in 0..n {
        let e = 20 + i * 16;
        let key_off = u16_at(b, e)? as usize;
        let fmt = u16_at(b, e + 2)?;
        let len = u32_at(b, e + 4)? as usize;
        let data_off = u32_at(b, e + 12)? as usize;
        let key = zstr(b, key_tab + key_off, b.len())?;
        let at = data_tab + data_off;
        match fmt {
            0x0204 => {
                out.insert(key, SfoValue::Text(zstr(b, at, at + len)?));
            }
            0x0404 => {
                out.insert(key, SfoValue::Int(u32_at(b, at)?));
            }
            _ => {}
        }
    }
    Some(out)
}

/// Builds a param.sfo, for tests.
#[cfg(test)]
pub fn build(entries: &[(&str, &str)]) -> Vec<u8> {
    let (mut keys, mut data, mut table) = (Vec::new(), Vec::new(), Vec::new());
    for (k, v) in entries {
        let (ko, d_o) = (keys.len() as u16, data.len() as u32);
        keys.extend_from_slice(k.as_bytes());
        keys.push(0);
        data.extend_from_slice(v.as_bytes());
        data.push(0);
        let len = v.len() as u32 + 1;
        table.extend_from_slice(&ko.to_le_bytes());
        table.extend_from_slice(&0x0204u16.to_le_bytes());
        table.extend_from_slice(&len.to_le_bytes());
        table.extend_from_slice(&len.to_le_bytes());
        table.extend_from_slice(&d_o.to_le_bytes());
    }
    let key_tab = 20 + table.len() as u32;
    let data_tab = key_tab + keys.len() as u32;
    let mut b = Vec::new();
    b.extend_from_slice(&0x4653_5000u32.to_le_bytes());
    b.extend_from_slice(&0x0101u32.to_le_bytes());
    b.extend_from_slice(&key_tab.to_le_bytes());
    b.extend_from_slice(&data_tab.to_le_bytes());
    b.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    b.extend(table);
    b.extend(keys);
    b.extend(data);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let s = parse(&build(&[("TITLE", "Some Game"), ("TITLE_ID", "BLUS30001")])).unwrap();
        assert_eq!(text(&s, "TITLE"), Some("Some Game"));
        assert_eq!(text(&s, "TITLE_ID"), Some("BLUS30001"));
        assert!(parse(b"nope").is_none());
    }
}
