//! RustyShelf, the native app: the games of every console from the Electron app's settings, on a 3D shelf.
// Bevy systems take their data as arguments, and queries are types
#![allow(clippy::too_many_arguments, clippy::type_complexity)]
// (the desktop's launching, emulator setup and boot slide aren't used on Android yet)
#![cfg_attr(target_os = "android", allow(dead_code, unused_variables, unused_mut))]

#[cfg(target_os = "android")]
mod android_pad;
#[cfg(target_os = "android")]
mod android_saf;
mod appicon;
mod background;
mod boot;
mod card;
mod coverpick;
mod cheevos;
mod deck;
mod dialog;
mod frames;
mod motion;
mod home;
mod hud;
mod idle;
mod loading;
mod music;
mod osk;
mod padwatch;
mod place;
mod phone;
mod play;
mod reader;
mod romm;
mod rumble;
mod settings;
mod shelf;
mod sound;
mod touch;
mod tour;
mod whatsnew;
mod wiimote;

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

/// The app on Android: GameActivity loads librustyshelf.so and calls this.
#[cfg(target_os = "android")]
#[bevy::prelude::bevy_main]
fn main() {
    // the app's own storage holds data/ (settings, art, history); HTTPS trusts the system's certificates
    if let Some(dir) = bevy::android::ANDROID_APP.get().and_then(|a| a.internal_data_path()) {
        // SAFETY: set before any other thread is started
        unsafe { std::env::set_var("RUSTYSHELF_ROOT", dir) };
    }
    if std::env::var_os("SSL_CERT_DIR").is_none() {
        unsafe { std::env::set_var("SSL_CERT_DIR", "/system/etc/security/cacerts") };
    }
    rustyshelf_core::android::set_lister(android_saf::list);
    run();
}

/// Android's controller and Back key in Bevy's input, and its games started in emulator apps (nothing elsewhere).
fn android_input(app: &mut App) {
    #[cfg(target_os = "android")]
    app.add_systems(PreUpdate, (android_pad::feed.before(bevy::input::InputSystems), android_pad::back_is_esc.after(bevy::input::InputSystems)));
    #[cfg(target_os = "android")]
    app.add_systems(Update, android_saf::poll);
    #[cfg(not(target_os = "android"))]
    let _ = app;
}

/// The settings written in the background are on disk before the app ends or Android sets it aside.
fn settings_on_disk(mut exit: MessageReader<AppExit>, mut life: MessageReader<bevy::window::AppLifecycle>) {
    let ending = exit.read().count() > 0;
    let leaving = life.read().any(|l| matches!(l, bevy::window::AppLifecycle::WillSuspend));
    if ending || leaving {
        rustyshelf_core::settings::flush();
    }
}

pub fn run() {
    paths::quiet_drive_errors();
    // before anything reads the settings: a waiting change put in, a new version's safety copy
    rustyshelf_core::keepsetup::at_start(&paths::app_root(), env!("CARGO_PKG_VERSION"), &paths::env_var);
    std::thread::spawn(|| rustyshelf_core::emucfg::init(&paths::env_var));
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "RustyShelf".into(),
                        mode: window_mode(),
                        resolution: (1280, 720).into(),
                        ..default()
                    }),
                    ..default()
                })
        )
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
        .init_resource::<hud::Wordmarks>()
        .init_resource::<settings::Settings>()
        .init_resource::<home::Home>()
        .init_resource::<reader::Reader>()
        .init_resource::<idle::Idle>()
        .init_resource::<romm::Sync>()
        .init_resource::<wiimote::Remote>()
        .init_resource::<card::Card>()
        .init_resource::<deck::DeckHint>()
        .init_resource::<whatsnew::WhatsNew>()
        .init_resource::<tour::Tour>()
        .init_resource::<rumble::Rumble>()
        .init_resource::<coverpick::CoverPick>()
        .add_plugins((music::plugin, frames::plugin))
        .add_systems(Last, settings_on_disk)
        .add_systems(Startup, (shelf::setup, hud::setup, boot::setup, home::setup, reader::setup, phone::setup, idle::setup, card::setup, wiimote::setup, deck::log_start).chain())
        .init_resource::<background::Background>()
        .add_systems(Update, (appicon::set, background::run, rumble::send, place::keep, settings::apply_scale))
        .add_systems(PostUpdate, (settings::motion::measure, settings::panels::measure, coverpick::scroll).after(bevy::ui::UiSystems::Layout))
        .add_systems(PreUpdate, phone::keys.before(bevy::input::InputSystems))
        .add_plugins(android_input)
        .add_systems(PreUpdate, touch::run.after(bevy::input::InputSystems))
        .add_systems(
            Update,
            (
                loading::poll_library,
                loading::recheck_folders,
                (romm::run, wiimote::run, wiimote::pointer),
                play::watch_game,
                phone::run,
                idle::run,
                home::run,
                (card::run, coverpick::run, coverpick::animate, play::pick_build, whatsnew::run, deck::run, tour::run).chain(),
                reader::run,
                shelf::inspect,
                settings::input,
                play::navigate,
                sound::run,
                music::run,
                (shelf::measure, shelf::aside).chain(),
                shelf::build_shelf,
                (shelf::apply_covers, shelf::reveal).chain(),
                shelf::arrange,
                (shelf::customs, shelf::backdrop, shelf::moving).chain(),
                (cheevos::run, hud::update, hud::top, hud::hints, settings::draw, settings::motion::animate, settings::motion::fade, settings::panels::draw, boot::run, home::draw).chain(),
            )
                .chain(),
        )
        .run();
}
