//! The menu music under the shelf: written a second or so ahead on a thread of its own, and played as one sound
//! that never ends.

use std::collections::VecDeque;
use std::num::NonZero;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::path::PathBuf;
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
    /// your own music: these files, shuffled
    Own { files: Vec<PathBuf>, seed: u32 },
    /// the key the music moves to where its chord loop starts over
    Tonic(i32),
    Stop,
}

type Decoded = rodio::source::UniformSourceIterator<rodio::Decoder<std::io::BufReader<std::fs::File>>>;

/// Your own music: the files, the order they're played in, and the one that's playing.
struct Own {
    files: Vec<PathBuf>,
    order: Vec<usize>,
    at: usize,
    last: Option<usize>,
    playing: Option<Decoded>,
    /// files in a row that couldn't be played
    fails: usize,
    chance: u32,
}

impl Own {
    fn random(&mut self, below: usize) -> usize {
        self.chance ^= self.chance << 13;
        self.chance ^= self.chance >> 17;
        self.chance ^= self.chance << 5;
        self.chance as usize % below.max(1)
    }

    /// Opens the next file of the shuffled order; never the same one twice running.
    fn next(&mut self) {
        if self.at >= self.order.len() {
            self.order = (0..self.files.len()).collect();
            for i in (1..self.order.len()).rev() {
                let j = self.random(i + 1);
                self.order.swap(i, j);
            }
            if self.order.len() > 1 && Some(self.order[0]) == self.last {
                let j = 1 + self.random(self.order.len() - 1);
                self.order.swap(0, j);
            }
            self.at = 0;
        }
        let which = self.order[self.at];
        self.at += 1;
        self.last = Some(which);
        let opened = std::fs::File::open(&self.files[which]).ok().and_then(|f| rodio::Decoder::try_from(f).ok());
        self.playing = opened.map(|d| rodio::source::UniformSourceIterator::new(d, NonZero::new(2).expect("two channels"), NonZero::new(RATE).expect("a sample rate")));
    }

    /// The next samples (left, right, …); None once nothing can be played.
    fn fill(&mut self, n: usize) -> Option<Vec<f32>> {
        let mut out = Vec::with_capacity(n);
        while out.len() < n {
            match self.playing.as_mut().and_then(Iterator::next) {
                Some(s) => {
                    out.push(s);
                    self.fails = 0;
                }
                // the file is over, or it couldn't be played: the next one. Every file failing in a row: no music
                None => {
                    if self.playing.take().is_none() {
                        self.fails += 1;
                    }
                    if self.files.is_empty() || self.fails > self.files.len() {
                        return None;
                    }
                    self.next();
                }
            }
        }
        Some(out)
    }
}

/// Writes the music ahead of where it's heard.
fn write(shared: Arc<Shared>, asked: Receiver<Ask>) {
    let mut playing: Option<(Player, Chain)> = None;
    let mut own: Option<Own> = None;
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
                Ok(Ask::Own { files, seed }) => {
                    leave(&mut playing, &shared);
                    own = Some(Own { files, order: Vec::new(), at: 0, last: None, playing: None, fails: 0, chance: seed | 1 });
                }
                Ok(Ask::Play { track, seed, tonic }) => {
                    own = None;
                    leave(&mut playing, &shared);
                    playing = Some((Player::new(Score::new(track, seed, 0.2, tonic), RATE, u64::from(seed), true), Chain::new(RATE)));
                }
                Ok(Ask::Tonic(t)) => {
                    if let Some((player, _)) = &mut playing {
                        player.score().set_tonic(t);
                    }
                }
                Ok(Ask::Stop) => {
                    own = None;
                    leave(&mut playing, &shared);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let room = shared.written.lock().unwrap().len() < AHEAD;
        if let Some(files) = own.as_mut().filter(|_| room) {
            match files.fill(BLOCK * 2) {
                Some(samples) => shared.written.lock().unwrap().extend(samples),
                None => own = None,
            }
            continue;
        }
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
    /// the track that's playing ("own": your music)
    track: Option<&'static str>,
    tonic: i32,
    /// your music was asked to be read again
    pub reload: bool,
}

impl Default for Music {
    fn default() -> Self {
        let shared = Arc::new(Shared { written: Mutex::new(VecDeque::new()), level: Mutex::new(Level { to: 0.0, ducked: 1.0, until: Instant::now() }) });
        let (ask, asked) = channel();
        let writer = shared.clone();
        std::thread::spawn(move || write(writer, asked));
        Music { shared, ask, track: None, tonic: 0, reload: false }
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
pub fn run(mut music: ResMut<Music>, mut sounds: ResMut<Sounds>, mut settings: ResMut<Settings>, lib: Res<LibraryRes>, cursor: Res<Cursor>, playing: Res<Playing>) {
    if std::mem::take(&mut settings.reload_music) {
        let n = rustyshelf_core::usermusic::music_files(&rustyshelf_core::paths::app_root()).len();
        settings.say(format!("{n} track{} found", if n == 1 { "" } else { "s" }));
        music.reload = true;
    }
    let prefs = settings.sound();
    let shelf = match &lib.0 {
        LibraryState::Ready(l) => l.shelves.get(cursor.shelf),
        _ => None,
    };
    let heard = prefs.on && prefs.music_level > 0.0 && prefs.volume > 0.0 && playing.title.is_none() && shelf.is_some();
    let track = music::track(&prefs.music).filter(|_| heard);
    let wanted = track.map(|t| t.id).or((heard && prefs.music == "own").then_some("own"));
    let tonic = music::tonic(cues::theme(&prefs.voice).root(), shelf.map_or(0, |s| cues::key_of(&s.id).0));
    if wanted != music.track || (std::mem::take(&mut music.reload) && wanted == Some("own")) {
        music.track = wanted;
        music.tonic = tonic;
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos() ^ d.as_secs() as u32);
        let _ = music.ask.send(match (track, wanted) {
            (Some(track), _) => Ask::Play { track, seed, tonic },
            (None, Some(_)) => Ask::Own { files: rustyshelf_core::usermusic::music_files(&rustyshelf_core::paths::app_root()).into_iter().map(|(_, f)| f).collect(), seed },
            (None, None) => Ask::Stop,
        });
    } else if tonic != music.tonic && track.is_some() {
        music.tonic = tonic;
        let _ = music.ask.send(Ask::Tonic(tonic));
    }
    let mut level = music.shared.level.lock().unwrap();
    // (your own files are recordings, quieter by ear than the app's music at the same setting)
    level.to = match wanted {
        Some("own") => (prefs.music_level * 1.4 * prefs.volume).min(1.0),
        Some(_) => prefs.music_level * prefs.volume,
        None => 0.0,
    };
    // the app's own music steps back for a cue; yours plays on
    if let Some((amount, hold)) = sounds.ducked.take().filter(|_| track.is_some()) {
        level.ducked = 1.0 - amount * 0.6;
        level.until = Instant::now() + Duration::from_secs_f32(hold / 1000.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_your_files_and_skips_what_it_cannot() {
        let dir = std::env::temp_dir().join(format!("rustyshelf-music-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // a second of a made-up tone, as a WAV file at another sample rate; and a file that's no audio at all
        let tone: Vec<f32> = (0..22_050).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let wav = rustyshelf_core::synth::Stereo { rate: 22_050, left: tone.clone(), right: tone }.wav();
        std::fs::write(dir.join("tone.wav"), wav).unwrap();
        std::fs::write(dir.join("broken.mp3"), b"not music").unwrap();
        let own = |files: &[&str]| Own { files: files.iter().map(|f| dir.join(f)).collect(), order: Vec::new(), at: 0, last: None, playing: None, fails: 0, chance: 7 };

        let mut both = own(&["broken.mp3", "tone.wav"]);
        // three seconds: the tone comes round again and again, the broken file never stops it
        let heard = both.fill(RATE as usize * 2 * 3).expect("music");
        let loud = heard.iter().filter(|s| s.abs() > 0.1).count();
        assert!(loud > heard.len() / 2, "{loud} of {} samples carry the tone", heard.len());

        assert!(own(&["broken.mp3"]).fill(1000).is_none());
        assert!(own(&[]).fill(1000).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
