//! The menu music under the shelf: written a second or so ahead on a thread of its own, and played as one sound
//! that never ends.

use std::collections::VecDeque;
use std::num::NonZero;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::audio::{AddAudioSource, AudioPlayer, ChannelCount, Decodable, PlaybackSettings, Sample, SampleRate, Source};
use bevy::prelude::*;
use rustyshelf_core::cues;
use rustyshelf_core::music::{self, Chain, Player, Score, Track};

use crate::loading::{LibraryRes, LibraryState};
use crate::play::Playing;
use crate::settings::Settings;
use crate::shelf::Cursor;
use crate::sound::Sounds;

const RATE: u32 = 44_100;
/// How much music is kept written ahead, in samples (both channels counted).
const AHEAD: usize = RATE as usize * 2 * 5 / 4;
const BLOCK: usize = 8192;

/// How loud the music plays right now: its level setting times the interface's volume, and how far it has stepped
/// back for a cue, until when.
struct Level {
    to: f32,
    ducked: f32,
    until: Instant,
}

struct Shared {
    /// music written ahead: left, right, left, right
    written: Mutex<VecDeque<f32>>,
    level: Mutex<Level>,
}

/// The music as Bevy's audio takes a sound.
#[derive(Asset, TypePath)]
pub struct MusicSource(Arc<Shared>);

pub struct Reader {
    shared: Arc<Shared>,
    chunk: Vec<f32>,
    at: usize,
    level: f32,
    level_to: f32,
    duck: f32,
    duck_to: f32,
}

impl Iterator for Reader {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        if self.at >= self.chunk.len() {
            self.chunk.clear();
            {
                let mut written = self.shared.written.lock().unwrap();
                let n = written.len().min(1024) & !1;
                self.chunk.extend(written.drain(..n));
            }
            // nothing written (the music is off, or held up): silence meanwhile
            if self.chunk.is_empty() {
                self.chunk.resize(512, 0.0);
            }
            self.at = 0;
            let level = self.shared.level.lock().unwrap();
            self.level_to = level.to;
            self.duck_to = if Instant::now() < level.until { level.ducked } else { 1.0 };
        }
        // once a frame: the level follows the settings gently; it steps back for a cue quickly and returns slowly
        if self.at.is_multiple_of(2) {
            const FOLLOW: f32 = 1.0 / (0.4 * RATE as f32);
            const BACK: f32 = 1.0 / (0.03 * RATE as f32);
            const RETURN: f32 = 1.0 / (0.25 * RATE as f32);
            self.level += (self.level_to - self.level) * FOLLOW;
            self.duck += (self.duck_to - self.duck) * if self.duck_to < self.duck { BACK } else { RETURN };
        }
        let s = self.chunk[self.at] * self.level * self.duck;
        self.at += 1;
        Some(Sample::from(s))
    }
}

impl Source for Reader {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        NonZero::new(2).expect("two channels")
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::new(RATE).expect("a sample rate")
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Decodable for MusicSource {
    type Decoder = Reader;

    fn decoder(&self) -> Reader {
        Reader { shared: self.0.clone(), chunk: Vec::new(), at: 0, level: 0.0, level_to: 0.0, duck: 1.0, duck_to: 1.0 }
    }
}

enum Ask {
    Play { track: &'static Track, seed: u32, tonic: i32 },
    /// the key the music moves to where its chord loop starts over
    Tonic(i32),
    Stop,
}

/// Writes the music ahead of where it's heard.
fn write(shared: Arc<Shared>, asked: Receiver<Ask>) {
    let mut playing: Option<(Player, Chain)> = None;
    // a track that's left goes quiet over one last block
    let leave = |playing: &mut Option<(Player, Chain)>, shared: &Shared| {
        if let Some((mut player, mut chain)) = playing.take() {
            let fade: Vec<f32> = (0..BLOCK).map(|i| 1.0 - i as f32 / BLOCK as f32).collect();
            let [l, r] = chain.run(&player.fill(BLOCK), &fade, 1.0);
            shared.written.lock().unwrap().extend(l.iter().zip(&r).flat_map(|(a, b)| [*a, *b]));
        }
    };
    loop {
        loop {
            match asked.try_recv() {
                Ok(Ask::Play { track, seed, tonic }) => {
                    leave(&mut playing, &shared);
                    playing = Some((Player::new(Score::new(track, seed, 0.2, tonic), RATE, u64::from(seed), true), Chain::new(RATE)));
                }
                Ok(Ask::Tonic(t)) => {
                    if let Some((player, _)) = &mut playing {
                        player.score().set_tonic(t);
                    }
                }
                Ok(Ask::Stop) => leave(&mut playing, &shared),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let room = shared.written.lock().unwrap().len() < AHEAD;
        match &mut playing {
            Some((player, chain)) if room => {
                let [l, r] = chain.run(&player.fill(BLOCK), &[1.0; BLOCK], 1.0);
                shared.written.lock().unwrap().extend(l.iter().zip(&r).flat_map(|(a, b)| [*a, *b]));
            }
            _ => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

#[derive(Resource)]
pub struct Music {
    shared: Arc<Shared>,
    ask: Sender<Ask>,
    /// the track that's playing
    track: Option<&'static str>,
    tonic: i32,
}

impl Default for Music {
    fn default() -> Self {
        let shared = Arc::new(Shared { written: Mutex::new(VecDeque::new()), level: Mutex::new(Level { to: 0.0, ducked: 1.0, until: Instant::now() }) });
        let (ask, asked) = channel();
        let writer = shared.clone();
        std::thread::spawn(move || write(writer, asked));
        Music { shared, ask, track: None, tonic: 0 }
    }
}

pub fn plugin(app: &mut App) {
    app.add_audio_source::<MusicSource>().init_resource::<Music>().add_systems(Startup, start);
}

fn start(music: Res<Music>, mut sources: ResMut<Assets<MusicSource>>, mut commands: Commands) {
    commands.spawn((AudioPlayer(sources.add(MusicSource(music.shared.clone()))), PlaybackSettings::LOOP));
}

/// Keeps the music as the settings and the shelf have it: the track, its level, its key; quiet while a game runs
/// and before the shelf is up, and stepping back for each cue.
pub fn run(mut music: ResMut<Music>, mut sounds: ResMut<Sounds>, settings: Res<Settings>, lib: Res<LibraryRes>, cursor: Res<Cursor>, playing: Res<Playing>) {
    let prefs = settings.sound();
    let shelf = match &lib.0 {
        LibraryState::Ready(l) => l.shelves.get(cursor.shelf),
        _ => None,
    };
    let wanted = music::track(&prefs.music).filter(|_| prefs.on && prefs.music_level > 0.0 && prefs.volume > 0.0 && playing.title.is_none() && shelf.is_some());
    let tonic = music::tonic(cues::theme(&prefs.voice).root(), shelf.map_or(0, |s| cues::key_of(&s.id).0));
    if wanted.map(|t| t.id) != music.track {
        music.track = wanted.map(|t| t.id);
        music.tonic = tonic;
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos() ^ d.as_secs() as u32);
        let _ = music.ask.send(match wanted {
            Some(track) => Ask::Play { track, seed, tonic },
            None => Ask::Stop,
        });
    } else if tonic != music.tonic && wanted.is_some() {
        music.tonic = tonic;
        let _ = music.ask.send(Ask::Tonic(tonic));
    }
    let mut level = music.shared.level.lock().unwrap();
    level.to = if wanted.is_some() { prefs.music_level * prefs.volume } else { 0.0 };
    if let Some((amount, hold)) = sounds.ducked.take() {
        level.ducked = 1.0 - amount * 0.6;
        level.until = Instant::now() + Duration::from_secs_f32(hold / 1000.0);
    }
}
