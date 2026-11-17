//! RustyShelf without a screen: the consoles, the settings, the games folders and their box art, launching a game.
//! The Electron app's main process does the same in JS (main/*.js); the data files are shared between the two.

/// This build's version: the release's own (`0.2.0-alpha.6`, `0.2.0.3`), which CI passes in as `RUSTYSHELF_VERSION`,
/// or else the workspace's.
pub const VERSION: &str = match option_env!("RUSTYSHELF_VERSION") {
    Some(v) if !v.is_empty() => v,
    _ => env!("CARGO_PKG_VERSION"),
};

/// Milliseconds since 1970 (JS `Date.now()`).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

pub mod accountenv;
pub mod addgames;
pub mod android;
pub mod arcade;
pub mod art;
pub mod artjob;
pub mod artstudio;
pub mod bflyt;
pub mod boxart;
pub mod collate;
pub mod companion;
pub mod config;
pub mod covers;
pub mod cssbg;
pub mod cues;
pub mod customthemes;
pub mod dataexport;
pub mod detect;
pub mod dirsize;
pub mod dolphin;
pub mod drawn;
pub mod emucfg;
pub mod emuname;
pub mod emusetup;
pub mod epicdl;
pub mod files;
pub mod gamekey;
pub mod gamelist;
pub mod find;
pub mod gametdb;
pub mod gamewatch;
pub mod gx2;
pub mod hidpads;
pub mod history;
pub mod homemenu;
pub mod i18n;
pub mod igdb;
pub mod info;
pub mod keepsetup;
pub mod js;
pub mod kyty;
pub mod launch;
/// The LaunchBox Games Database index, built by the same code as the Electron app's helper.
pub use rustyshelf_imgtools::lbindex;
pub mod library;
pub mod linuxpc;
pub mod looks;
pub mod manualcoll;
pub mod manuals;
pub mod manualstudio;
pub mod moonlight;
pub mod moreshots;
pub mod music;
pub mod n3ds;
pub mod net;
pub mod nx2;
pub mod pads;
pub mod paths;
pub mod pcach;
pub mod pcbacks;
pub mod pccovers;
pub mod pcfolder;
pub mod pcgames;
pub mod pdfpages;
pub mod pkg;
pub mod presence;
pub mod provenance;
pub mod ra;
pub mod romm;
pub mod rpcs3troph;
pub mod racores;
pub mod recomps;
pub mod rocket;
pub mod raemu;
pub mod scanner;
pub mod screenscraper;
pub mod seal;
pub mod settings;
pub mod shelfview;
pub mod sfo;
pub mod soundpacks;
pub mod spreads;
pub mod steam;
pub mod steamach;
pub mod steamart;
pub mod steamdeck;
pub mod steamgrid;
pub mod storage;
pub mod synth;
pub mod systems;
pub mod themefx;
pub mod themes;
pub mod titles;
pub mod updates;
pub mod usermusic;
pub mod vdf;
pub mod vita3k;
pub mod wiimote;
pub mod wiiumanual;
pub mod wiiumanual_draw;
pub mod wikiart;
pub mod windowmode;
pub mod x360;
pub mod xeniaach;
pub mod zarchive;
pub mod zip;
