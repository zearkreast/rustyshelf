// (a release build on Windows opens no console window beside the app)
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    rustyshelf::run();
}
