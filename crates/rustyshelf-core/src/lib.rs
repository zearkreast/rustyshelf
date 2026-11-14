//! RustyShelf without a screen: the consoles, the settings, the games folders and their box art, launching a game.
//! The Electron app's main process does the same in JS (main/*.js); the data files are shared between the two.

pub mod arcade;
pub mod art;
pub mod boxart;
pub mod collate;
pub mod config;
pub mod covers;
pub mod cues;
pub mod detect;
pub mod dirsize;
pub mod dolphin;
pub mod emucfg;
pub mod emuname;
pub mod emusetup;
pub mod gamelist;
pub mod gamewatch;
pub mod history;
pub mod info;
pub mod js;
pub mod kyty;
pub mod launch;
/// The LaunchBox Games Database index, built by the same code as the Electron app's helper.
#[path = "../../img-tools/src/lbindex.rs"]
#[allow(clippy::all, dead_code)]
pub mod lbindex;
pub mod library;
pub mod looks;
pub mod music;
pub mod n3ds;
pub mod net;
pub mod nx2;
pub mod paths;
pub mod pccovers;
pub mod pcfolder;
pub mod pcgames;
pub mod pkg;
pub mod presence;
pub mod racores;
pub mod scanner;
pub mod settings;
pub mod shelfview;
pub mod sfo;
pub mod steam;
pub mod steamart;
pub mod synth;
pub mod systems;
pub mod titles;
pub mod usermusic;
pub mod vdf;
pub mod vita3k;
pub mod windowmode;
pub mod x360;
