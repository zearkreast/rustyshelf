//! RustyShelf without a screen: the consoles, the settings, the games folders and their box art, launching a game.
//! The Electron app's main process does the same in JS (main/*.js); the data files are shared between the two.

pub mod arcade;
pub mod art;
pub mod collate;
pub mod config;
pub mod detect;
pub mod dirsize;
pub mod dolphin;
pub mod emucfg;
pub mod emuname;
pub mod emusetup;
pub mod gamelist;
pub mod history;
pub mod js;
pub mod kyty;
pub mod launch;
pub mod library;
pub mod n3ds;
pub mod net;
pub mod nx2;
pub mod paths;
pub mod pccovers;
pub mod pcfolder;
pub mod pcgames;
pub mod pkg;
pub mod racores;
pub mod scanner;
pub mod settings;
pub mod sfo;
pub mod steam;
pub mod steamart;
pub mod systems;
pub mod titles;
pub mod vdf;
pub mod vita3k;
pub mod x360;
