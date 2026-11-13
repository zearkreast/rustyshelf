//! RustyShelf without a screen: the consoles, the settings, the games folders and their box art, launching a game.
//! The Electron app's main process does the same in JS (main/*.js); the data files are shared between the two.

pub mod art;
pub mod collate;
pub mod config;
pub mod launch;
pub mod library;
pub mod paths;
pub mod scanner;
pub mod sfo;
pub mod systems;
pub mod titles;
