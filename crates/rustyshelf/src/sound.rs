//! The interface's sounds: a cue's voices are rendered on a thread of their own, and played once they're ready.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings};
use bevy::prelude::*;
use rustyshelf_core::cues;
use rustyshelf_core::synth::{self, Tone, Voice};

use crate::loading::{LibraryRes, LibraryState};
use crate::settings::Settings;
use crate::shelf::Cursor;

/// The sample rate cues are rendered at.
const RATE: u32 = 44_100;

struct Job {
    voices: Vec<Voice>,
    tone: Tone,
    level: f32,
    volume: f32,
    seed: u64,
}

#[derive(Resource)]
pub struct Sounds {
    state: cues::Sound,
    on: bool,
    volume: f32,
    started: Instant,
    seed: u64,
    jobs: Sender<Job>,
    /// cues rendered, as WAV files
    done: Mutex<Receiver<Vec<u8>>>,
    /// cues to play later: when (seconds), and which
    later: Vec<(f32, &'static str)>,
    /// the shelf the cues are tuned to
    shelf: String,
    /// the voice chosen, and the consoles given one of their own
    voice: String,
    by_shelf: std::collections::HashMap<String, String>,
    /// the menu music is asked to step back for the cue just played: how far (0 to 1), and for how long (ms)
    pub ducked: Option<(f32, f32)>,
}

impl Default for Sounds {
    fn default() -> Self {
        let (jobs, asked) = channel::<Job>();
        let (tx, done) = channel();
        std::thread::spawn(move || {
            for job in asked {
                let out = synth::render(&job.voices, &job.tone, job.level, RATE, job.volume, job.seed);
                if tx.send(out.wav()).is_err() {
                    return;
                }
            }
        });
        let mut state = cues::Sound::default();
        state.set_songs(rustyshelf_core::usermusic::songs(&rustyshelf_core::paths::app_root()).0, true, 0.0);
        Sounds { state, on: true, volume: 0.7, started: Instant::now(), seed: 1, jobs, done: Mutex::new(done), later: Vec::new(), shelf: String::new(), voice: String::new(), by_shelf: Default::default(), ducked: None }
    }
}

impl Sounds {
    /// Plays a cue; `dir` is which way a move went (-1 back, 1 on), for "tick" and "shelf".
    pub fn play(&mut self, name: &str, dir: i32) {
        if !self.on || self.volume <= 0.0 {
            return;
        }
        let cue = self.state.cue(name, dir, self.started.elapsed().as_secs_f64() * 1000.0);
        if cue.voices.is_empty() {
            return;
        }
        if cue.duck.is_some() {
            self.ducked = cue.duck;
        }
        self.seed = self.seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let _ = self.jobs.send(Job { voices: cue.voices, tone: self.state.theme().tone, level: cue.level, volume: self.volume, seed: self.seed });
    }

    /// Plays a cue `after` seconds from `now`.
    pub fn play_later(&mut self, name: &'static str, now: f32, after: f32) {
        self.later.push((now + after, name));
    }
}

/// Hands the cues that are ready to the audio device, plays the ones whose time has come and what Settings asked
/// for, and keeps the voice, the volume and the shelf's key as the settings and the shelf have them.
pub fn run(mut sounds: ResMut<Sounds>, mut settings: ResMut<Settings>, lib: Res<LibraryRes>, cursor: Res<Cursor>, time: Res<Time>, mut assets: ResMut<Assets<AudioSource>>, mut commands: Commands) {
    if std::mem::take(&mut settings.sound_changed) {
        let s = settings.sound();
        sounds.on = s.on;
        sounds.volume = s.volume;
        (sounds.voice, sounds.by_shelf) = (s.voice.clone(), s.by_shelf.clone());
        let voice = sounds.by_shelf.get(&sounds.shelf).unwrap_or(&sounds.voice).clone();
        sounds.state.set_theme(&voice);
        sounds.state.set_boot(&s.boot, s.boot_volume);
        let now = sounds.started.elapsed().as_secs_f64() * 1000.0;
        sounds.state.with_built_in(s.built_in_songs, now);
    }
    if std::mem::take(&mut settings.reload_songs) {
        let (songs, skipped) = rustyshelf_core::usermusic::songs(&rustyshelf_core::paths::app_root());
        let found = format!("{} song{} loaded", songs.len(), if songs.len() == 1 { "" } else { "s" });
        settings.say(if skipped.is_empty() { found } else { format!("{found} · {} couldn't be read", skipped.len()) });
        let (with, now) = (settings.sound().built_in_songs, sounds.started.elapsed().as_secs_f64() * 1000.0);
        sounds.state.set_songs(songs, with, now);
    }
    if let LibraryState::Ready(l) = &lib.0 {
        if let Some(shelf) = l.shelves.get(cursor.shelf).filter(|s| s.id != sounds.shelf) {
            sounds.shelf = shelf.id.clone();
            sounds.state.set_shelf(&shelf.id);
            let voice = sounds.by_shelf.get(&shelf.id).unwrap_or(&sounds.voice).clone();
            sounds.state.set_theme(&voice);
        }
    }
    for (name, dir) in std::mem::take(&mut settings.cues) {
        sounds.play(name, dir);
    }
    let now = time.elapsed_secs();
    let due: Vec<&'static str> = sounds.later.iter().filter(|(at, _)| *at <= now).map(|(_, name)| *name).collect();
    sounds.later.retain(|(at, _)| *at > now);
    for name in due {
        sounds.play(name, 1);
    }
    let ready: Vec<Vec<u8>> = sounds.done.lock().unwrap().try_iter().collect();
    for wav in ready {
        commands.spawn((AudioPlayer(assets.add(AudioSource { bytes: Arc::from(wav) })), PlaybackSettings::DESPAWN));
    }
}
