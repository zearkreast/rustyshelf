//! RustyShelf, the native app: the games of every console from the Electron app's settings, on a 3D shelf.
// Bevy systems take their data as arguments, and queries are types
#![allow(clippy::too_many_arguments, clippy::type_complexity)]
// (a release build on Windows opens no console window beside the app)
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod arcade;
mod boot;
mod cheevos;
mod home;
mod hud;
mod idle;
mod loading;
mod museum;
mod music;
mod osk;
mod padwatch;
mod phone;
mod play;
mod reader;
mod romm;
mod settings;
mod shelf;
mod sound;

use bevy::prelude::*;
use bevy::window::{MonitorSelection, WindowMode};
use rustyshelf_core::config::Config;
use rustyshelf_core::paths;

/// Fullscreen unless `--windowed` or Settings → Display says otherwise, as in the Electron app.
fn window_mode() -> WindowMode {
    if std::env::args().any(|a| a == "--windowed") {
        return WindowMode::Windowed;
    }
    let cfg = Config::load(&paths::data_dir(&paths::app_root())).unwrap_or_default();
    let full = cfg.raw().get("ui").and_then(|u| u.get("startFullscreen")).and_then(|v| v.as_bool()).unwrap_or(true);
    if full {
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    } else {
        WindowMode::Windowed
    }
}

fn main() {
    paths::quiet_drive_errors();
    std::thread::spawn(|| rustyshelf_core::emucfg::init(&paths::env_var));
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "RustyShelf".into(),
                mode: window_mode(),
                resolution: (1280, 720).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(loading::start_library())
        .init_resource::<loading::Refresh>()
        .init_resource::<loading::Base>()
        .init_resource::<loading::Reshow>()
        .insert_resource(loading::ArtLoader::start())
        .init_resource::<shelf::Cursor>()
        .init_resource::<shelf::Covers>()
        .init_resource::<shelf::Stage>()
        .init_resource::<shelf::Lit>()
        .init_resource::<shelf::Backdrop>()
        .init_resource::<shelf::Moving>()
        .init_resource::<shelf::Customs>()
        .init_resource::<shelf::Inspect>()
        .init_resource::<play::Playing>()
        .init_resource::<play::Plays>()
        .init_resource::<play::Surprise>()
        .init_resource::<play::Discord>()
        .init_resource::<sound::Sounds>()
        .init_resource::<play::Repeat>()
        .init_resource::<hud::Toast>()
        .init_resource::<boot::Slide>()
        .init_resource::<cheevos::Cheevos>()
        .init_resource::<hud::Prompts>()
        .init_resource::<settings::Settings>()
        .init_resource::<home::Home>()
        .init_resource::<reader::Reader>()
        .init_resource::<idle::Idle>()
        .init_resource::<museum::Museum>()
        .init_resource::<romm::Sync>()
        .add_plugins(music::plugin)
        .add_systems(Startup, (shelf::setup, hud::setup, boot::setup, home::setup, reader::setup, phone::setup, idle::setup, museum::setup).chain())
        .add_systems(PreUpdate, phone::keys.before(bevy::input::InputSystems))
        .add_systems(
            Update,
            (
                loading::poll_library,
                loading::recheck_folders,
                romm::run,
                play::watch_game,
                phone::run,
                idle::run,
                home::run,
                (reader::run, museum::run).chain(),
                shelf::inspect,
                settings::input,
                play::navigate,
                sound::run,
                music::run,
                shelf::measure,
                shelf::build_shelf,
                (shelf::apply_covers, shelf::reveal).chain(),
                shelf::arrange,
                (shelf::customs, shelf::backdrop, shelf::moving).chain(),
                (cheevos::run, hud::update, hud::top, hud::hints, settings::draw, boot::run, home::draw).chain(),
            )
                .chain(),
        )
        .run();
}
