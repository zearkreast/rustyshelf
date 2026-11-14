//! RustyShelf, the native app: the games of every console from the Electron app's settings, on a 3D shelf.
// Bevy systems take their data as arguments, and queries are types
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod boot;
mod hud;
mod loading;
mod music;
mod osk;
mod padwatch;
mod play;
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
        .init_resource::<play::Playing>()
        .init_resource::<play::Plays>()
        .init_resource::<play::Surprise>()
        .init_resource::<play::Discord>()
        .init_resource::<sound::Sounds>()
        .init_resource::<play::Repeat>()
        .init_resource::<hud::Toast>()
        .init_resource::<boot::Slide>()
        .init_resource::<hud::Prompts>()
        .init_resource::<settings::Settings>()
        .add_plugins(music::plugin)
        .add_systems(Startup, (shelf::setup, hud::setup, boot::setup).chain())
        .add_systems(
            Update,
            (
                loading::poll_library,
                play::watch_game,
                settings::input,
                play::navigate,
                sound::run,
                music::run,
                shelf::measure,
                shelf::build_shelf,
                (shelf::apply_covers, shelf::reveal).chain(),
                shelf::arrange,
                (shelf::customs, shelf::backdrop, shelf::moving).chain(),
                (hud::update, hud::top, hud::hints, settings::draw, boot::run).chain(),
            )
                .chain(),
        )
        .run();
}
