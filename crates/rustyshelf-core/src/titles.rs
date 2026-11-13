// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): a Rust port of
// Boxshelf's shared/titles.js, as RustyShelf has it.
//! A game's file name as a clean shelf title.
//!
//! JS regexes are ASCII for `\b`, `\w` and `\d`; the patterns here spell that out (`(?-u:\b)`, `[0-9]`) so titles come
//! out exactly as the Electron app makes them.

use std::sync::LazyLock;

use regex::{Captures, Regex};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanTitle {
    pub title: String,
    pub region: String,
    /// 0 when the name doesn't say
    pub disc: u32,
    pub rev: String,
    /// a Switch title id from a `[0100…]` tag, upper case
    pub title_id: Option<String>,
    /// an update or DLC package, not a game
    pub extra: bool,
}

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
    };
}

re!(EXT, r"\.[^.]+$");
re!(DUMP_SUFFIX, r"(?i)\.?\s*(?-u:\b)(nkit|wia|rvz|gcz|ciso|iso|wbfs|part[0-9]+)\s*$");
re!(DUMP_TAG, r"(?i)\s*\((nkit|wbfs|iso)\)");
// v65536: a Switch update package's version
re!(BIG_VERSION, r"(?i)(^|[^a-z0-9])v[0-9]{5,}");
re!(TAG, r"\s*[(\[]([^)\]]*)[)\]]");
re!(TITLE_ID, r"(?i)^[0-9A-F]{16}$");
re!(TAG_EXTRA, r"(?i)^(upd|update|dlc|patch)$");
re!(DISC, r"(?i)^(?:dis[ck]|dvd|cd)\s*([0-9]+)");
re!(REV, r"(?i)^rev\s*[0-9A-Za-z_]+");
re!(REGION, r"(?i)^(usa|europe|japan|world|canada|australia|korea|china|asia|germany|france|spain|italy|uk|brazil|netherlands|sweden|en|ja|fr|de|es|it)([,\s].*)?$");
re!(LANGUAGE, r"(?i)^(en|ja|fr|de|es|it)(?-u:\b)");
re!(UNDERSCORES, r"_+");
re!(COPY, r"(?i)\s+-\s+copy(\s*\([0-9]+\))?\s*$");
re!(WEBSITE, r"(?i)(?-u:\b)(www\.)?[0-9A-Za-z_-]+\.(com|net|org|io|to|cc)(?-u:\b)");
re!(VERSION, r"(?i)(?-u:\b)v[0-9]+(\.[0-9]+)*(?-u:\b)");
re!(DUMP_WORDS, r"(?i)(?-u:\b)(switch|xci|nsp|nsz|base game|eshop|repack|multi[0-9]*)(?-u:\b)");
re!(SXS, r"(?i)(?-u:\b)sxs-");
re!(SPACES, r"\s{2,}");
re!(TRAILING, r"[\s-]+$");
re!(ARTICLE, r"(?i)^(.+?),\s+(The|A|An)(\s+-\s+.*)?$");
re!(EXTRA_WORDS, r"(?i)(?-u:\b)(update|dlc|patch|booster|pass)(?-u:\b)");
re!(EXTRA_TAIL, r"(?i)(?-u:\b)(update|dlc|patch)(?-u:\b).*$");

/// The file name with any folders taken off (either slash).
pub fn base_name(s: &str) -> &str {
    s.rsplit(['/', '\\']).next().unwrap_or(s)
}

/// `update_words`: the bare words update / dlc / patch / booster / pass mark an update package. Only the Switch shelves
/// want that ("Hall Pass" is a game elsewhere); bracket tags like "(Update)" count everywhere.
pub fn clean_title(file_name: &str, update_words: bool) -> CleanTitle {
    let name = base_name(file_name);
    let base = EXT.replace(name, "");
    let base = DUMP_SUFFIX.replace(&base, "");
    let base = DUMP_TAG.replace_all(&base, "").into_owned();

    let mut meta = CleanTitle::default();
    let big_version = BIG_VERSION.is_match(&base);
    let mut tag_extra = false;
    let base = TAG
        .replace_all(&base, |c: &Captures| {
            let t = c[1].trim();
            if TITLE_ID.is_match(t) {
                meta.title_id = Some(t.to_uppercase());
            }
            if TAG_EXTRA.is_match(t) {
                tag_extra = true;
            }
            if let Some(d) = DISC.captures(t) {
                meta.disc = d[1].parse().unwrap_or(0);
            } else if REV.is_match(t) {
                meta.rev = t.to_string();
            } else if meta.region.is_empty() && REGION.is_match(t) && !LANGUAGE.is_match(t) {
                meta.region = t.split(',').next().unwrap_or("").trim().to_string();
            }
            " "
        })
        .into_owned();

    let base = UNDERSCORES.replace_all(&base, " ");
    let base = COPY.replace(&base, "");
    let base = WEBSITE.replace_all(&base, "");
    let base = VERSION.replace_all(&base, " ");
    let base = DUMP_WORDS.replace_all(&base, " ");
    let base = SXS.replace(&base, "");
    let base = SPACES.replace_all(&base, " ");
    let base = TRAILING.replace(base.trim(), "").into_owned();
    let mut base = ARTICLE
        .replace(&base, |c: &Captures| {
            format!("{} {}{}", &c[2], &c[1], c.get(3).map_or("", |m| m.as_str()))
        })
        .into_owned();

    let odd_id = meta.title_id.as_deref().is_some_and(|id| !id.ends_with("000"));
    meta.extra = (update_words && EXTRA_WORDS.is_match(&base)) || odd_id || big_version || tag_extra;
    if meta.extra && update_words {
        base = EXTRA_TAIL.replace(&base, "").trim().to_string();
    }
    meta.title = if base.is_empty() { name.to_string() } else { base };
    meta
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_become_metadata() {
        let t = clean_title("Legend of Zelda, The - Wind Waker (USA) (Disc 2).rvz", false);
        assert_eq!(t.title, "The Legend of Zelda - Wind Waker");
        assert_eq!(t.region, "USA");
        assert_eq!(t.disc, 2);
    }

    #[test]
    fn update_words_only_where_asked() {
        assert!(!clean_title("Hall Pass.iso", false).extra);
        assert!(clean_title("Game Update v1.0.1.nsp", true).extra);
        assert!(clean_title("Game [DLC].iso", false).extra);
    }
}
