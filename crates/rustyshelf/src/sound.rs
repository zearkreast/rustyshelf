//! The interface's sounds: a cue's voices are rendered on a thread of their own, and played once they're ready. A
//! sound pack's moments are its own recordings, decoded on that thread once and kept; a cue the pack has no file for
//! plays Aero's.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings};
use bevy::prelude::*;
use rustyshelf_core::cues;
use rustyshelf_core::i18n::plural_s;
use rustyshelf_core::soundpacks::{self, Pack};
use rustyshelf_core::synth::{self, Tone, Voice};

use crate::loading::{LibraryRes, LibraryState};
use crate::settings::Settings;
use crate::shelf::Cursor;

/// The sample rate cues are rendered at.
const RATE: u32 = 44_100;

enum Job {
    Cue { voices: Vec<Voice>, tone: Tone, level: f32, volume: f32, seed: u64 },
    /// a pack's file, at the volume
    File(PathBuf, f32),
}

/// A pack's file decoded, as two channels at its own rate, and the level that evens it out (renderer/sound.js
/// packLoad: half of full scale at its peak, at most four times as loud, through the cues' trim).
fn decode(file: &PathBuf) -> Option<(u32, Vec<f32>, Vec<f32>, f32)> {
    use rodio::Source;
    let d = rodio::Decoder::try_from(std::fs::File::open(file).ok()?).ok()?;
    let (rate, ch) = (d.sample_rate().get(), usize::from(d.channels().get()));
    // at most 20 s of it
    let samples: Vec<f32> = d.take(rate as usize * ch * 20).collect();
    let (mut left, mut right) = (Vec::with_capacity(samples.len() / ch), Vec::with_capacity(samples.len() / ch));
    for frame in samples.chunks(ch) {
        left.push(frame[0]);
        right.push(*frame.get(1).unwrap_or(&frame[0]));
    }
    let peak = left.iter().step_by(2).fold(0.0f32, |m, s| m.max(s.abs()));
    let gain = (0.5 / peak.max(0.02)).min(4.0) * synth::TRIM * 1.6;
    Some((rate, left, right, gain))
}

#[derive(Resource)]
pub struct Sounds {
    /// where RustyShelf lives, for the packs' files
    root: std::path::PathBuf,
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
    /// the voice chosen, the consoles given one of their own, the packs there are, and the pack playing
    voice: String,
    by_shelf: HashMap<String, String>,
    packs: Vec<Pack>,
    pack: Option<Pack>,
    last_pack_tick: f64,
    /// the menu music is asked to step back for the cue just played: how far (0 to 1), and for how long (ms)
    pub ducked: Option<(f32, f32)>,
}

impl Default for Sounds {
    fn default() -> Self {
        let (jobs, asked) = channel::<Job>();
        let (tx, done) = channel();
        std::thread::spawn(move || {
            let mut decoded: HashMap<PathBuf, Option<(u32, Vec<f32>, Vec<f32>, f32)>> = HashMap::new();
            for job in asked {
                let wav = match job {
                    Job::Cue { voices, tone, level, volume, seed } => synth::render(&voices, &tone, level, RATE, volume, seed).wav(),
                    Job::File(file, volume) => {
                        let Some((rate, left, right, gain)) = decoded.entry(file.clone()).or_insert_with(|| decode(&file)) else { continue };
                        let k = *gain * volume;
                        synth::Stereo { rate: *rate, left: left.iter().map(|s| s * k).collect(), right: right.iter().map(|s| s * k).collect() }.wav()
                    }
                };
                if tx.send(wav).is_err() {
                    return;
                }
            }
        });
        let mut state = cues::Sound::default();
        state.set_songs(rustyshelf_core::usermusic::songs(&rustyshelf_core::paths::app_root()).0, true, 0.0);
        Sounds { root: rustyshelf_core::paths::app_root(), state, on: true, volume: 0.7, started: Instant::now(), seed: 1, jobs, done: Mutex::new(done), later: Vec::new(), shelf: String::new(), voice: String::new(), by_shelf: HashMap::new(), packs: Vec::new(), pack: None, last_pack_tick: 0.0, ducked: None }
    }
}

impl Sounds {
    /// Plays a cue; `dir` is which way a move went (-1 back, 1 on), for "tick" and "shelf".
    pub fn play(&mut self, name: &str, dir: i32) {
        if !self.on || self.volume <= 0.0 {
            return;
        }
        let now = self.started.elapsed().as_secs_f64() * 1000.0;
        if let Some(file) = self.pack.as_ref().and_then(|p| Some((p, p.file_for(soundpacks::moment_of(name)?)?))).and_then(|(p, f)| soundpacks::file_of(&self.root, p, f)) {
            // held scrolling sends a tick every few ms: one recording at a time
            if name == "tick" && now - self.last_pack_tick < 40.0 {
                return;
            }
            self.last_pack_tick = now;
            let _ = self.jobs.send(Job::File(file, self.volume));
            return;
        }
        let cue = self.state.cue(name, dir, now);
        if cue.voices.is_empty() {
            return;
        }
        if cue.duck.is_some() {
            self.ducked = cue.duck;
        }
        self.seed = self.seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let _ = self.jobs.send(Job::Cue { voices: cue.voices, tone: self.state.theme().tone, level: cue.level, volume: self.volume, seed: self.seed });
    }

    /// The music file of the pack playing on this shelf, when it has one.
    pub fn pack_music(&self) -> Option<std::path::PathBuf> {
        let p = self.pack.as_ref()?;
        soundpacks::file_of(&self.root, p, p.file_for("music")?)
    }

    /// The sound the shelf plays: a voice, or a pack (and Aero for what it has no file for).
    fn choose(&mut self) {
        let sound = soundpacks::sound_for(&self.shelf, &self.voice, &self.by_shelf, &self.packs);
        self.pack = soundpacks::pack_id_of(&sound).and_then(|id| self.packs.iter().find(|p| p.id == id)).cloned();
        self.state.set_theme(if self.pack.is_some() { "aero" } else { &sound });
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
        sounds.packs = soundpacks::list(&rustyshelf_core::paths::app_root());
        sounds.choose();
        sounds.state.set_boot(&s.boot, s.boot_volume);
        let now = sounds.started.elapsed().as_secs_f64() * 1000.0;
        sounds.state.with_built_in(s.built_in_songs, now);
    }
    if std::mem::take(&mut settings.reload_songs) {
        let (songs, skipped) = rustyshelf_core::usermusic::songs(&rustyshelf_core::paths::app_root());
        let found = format!("{} song{} loaded", songs.len(), plural_s(songs.len()));
        settings.say(if skipped.is_empty() { found } else { format!("{found} · {} couldn't be read", skipped.len()) });
        let (with, now) = (settings.sound().built_in_songs, sounds.started.elapsed().as_secs_f64() * 1000.0);
        sounds.state.set_songs(songs, with, now);
    }
    if let LibraryState::Ready(l) = &lib.0 {
        if let Some(shelf) = l.shelves.get(cursor.shelf).filter(|s| s.id != sounds.shelf) {
            sounds.shelf = shelf.id.clone();
            sounds.state.set_shelf(&shelf.id);
            sounds.choose();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pack_s_recording_is_decoded_and_evened_out() {
        let dir = std::env::temp_dir().join(format!("rustyshelf-sound-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("click.wav");
        let tone: Vec<f32> = (0..2205).map(|i| (i as f32 * 0.1).sin() * 0.1).collect();
        std::fs::write(&file, synth::Stereo { rate: 22_050, left: tone.clone(), right: tone }.wav()).unwrap();
        let (rate, left, right, gain) = decode(&file).unwrap();
        assert_eq!((rate, left.len(), right.len()), (22_050, 2205, 2205));
        // a quiet recording is brought up to half of full scale (at most four times)
        assert!((gain - 4.0 * synth::TRIM * 1.6).abs() < 1e-3);
        assert!(decode(&dir.join("missing.ogg")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
