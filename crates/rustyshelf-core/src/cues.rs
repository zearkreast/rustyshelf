// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from the
// cue set of Boxshelf's renderer/sound.js and the built-in songs of its renderer/songs.js, as RustyShelf has them.
//! What the interface plays, and when: every cue as the voices it's made of. Pitches are steps of one pentatonic
//! scale per voice, so any two cues played together agree; each shelf plays in its own key. Three voices: Aero
//! (soft and airy), Melody (a felt piano playing a song one note per move), Music box (warm steel tines), and three
//! made in a style (nothing taken from any console): Disc-era, Handheld and Living room.

use crate::synth::{Air, Filter, Fm, Hit, Pad, Partial, Swell, Tone, Voice, Wave};

/// A voice of the interface.
pub struct Theme {
    pub id: &'static str,
    pub tone: Tone,
    root: f32,
}

const PENTATONIC: [i32; 5] = [0, 2, 4, 7, 9];

pub static THEMES: [Theme; 6] = [
    // almost-pure high tones with a breath of air under them, soft attacks instead of clicks, a long quiet room
    Theme {
        id: "aero",
        tone: Tone { wave: Wave::Sine, partials: &[(1.0, 1.0, 1.0), (2.0, 0.12, 0.5), (3.01, 0.04, 0.35)], decay: 0.32, bright: 9000.0, noise: 0.06, thump: 1.0, verb: 0.7, attack: 0.012, breath: 0.5 },
        // D6
        root: 1174.66,
    },
    // a soft felt piano: slightly stretched partials like real strings, a mellow hammer
    Theme {
        id: "melody",
        tone: Tone {
            wave: Wave::Sine,
            partials: &[(1.0, 1.0, 1.0), (2.002, 0.42, 0.62), (3.006, 0.15, 0.42), (4.012, 0.07, 0.3), (5.02, 0.035, 0.22), (6.03, 0.015, 0.18)],
            decay: 0.9,
            bright: 3400.0,
            noise: 0.035,
            thump: 1.0,
            verb: 0.5,
            attack: 0.003,
            breath: 0.0,
        },
        // C5
        root: 523.25,
    },
    // small steel tines plucked by a comb: a pure tone, a quiet octave and a faint shimmer that makes it metal
    Theme {
        id: "musicbox",
        tone: Tone { wave: Wave::Sine, partials: &[(1.0, 1.0, 1.0), (2.0, 0.16, 0.5), (3.0, 0.04, 0.3), (5.4, 0.05, 0.16), (8.93, 0.015, 0.1)], decay: 0.75, bright: 5200.0, noise: 0.04, thump: 1.0, verb: 0.55, attack: 0.002, breath: 0.0 },
        // G5
        root: 783.99,
    },
    // glassy FM bells over a soft pad and a big shiny room, the late-90s disc console feel
    Theme {
        id: "disc",
        tone: Tone { wave: Wave::Sine, partials: &[(1.0, 1.0, 1.0), (2.76, 0.16, 0.45), (5.4, 0.05, 0.25)], decay: 0.5, bright: 7200.0, noise: 0.02, thump: 1.0, verb: 0.8, attack: 0.005, breath: 0.2 },
        // A5
        root: 880.0,
    },
    // small and chirpy: a pulse wave with quick blips and almost no room
    Theme {
        id: "handheld",
        tone: Tone { wave: Wave::Square, partials: &[(1.0, 1.0, 1.0)], decay: 0.07, bright: 4200.0, noise: 0.0, thump: 1.0, verb: 0.06, attack: 0.002, breath: 0.0 },
        // E6
        root: 1318.51,
    },
    // warm and round: a soft triangle with a little body under it
    Theme {
        id: "living",
        tone: Tone { wave: Wave::Triangle, partials: &[(1.0, 1.0, 1.0), (2.0, 0.22, 0.6), (3.0, 0.06, 0.4)], decay: 0.32, bright: 2600.0, noise: 0.015, thump: 1.04, verb: 0.45, attack: 0.008, breath: 0.15 },
        // D5
        root: 587.33,
    },
];

impl Theme {
    /// The pitch its scale starts on.
    pub fn root(&self) -> f32 {
        self.root
    }
}

/// A voice by the name the settings have (`ui.soundTheme`), the voices before these three included; Aero for a
/// name that's none.
pub fn theme(name: &str) -> &'static Theme {
    let id = match name {
        "glass" | "wood" => "aero",
        "pulse" | "arcade" => "musicbox",
        "xylo" | "jazz" => "melody",
        other => other,
    };
    THEMES.iter().find(|t| t.id == id).unwrap_or(&THEMES[0])
}

/// A shelf's key: semitones from the voice's own, and the pentatonic colour it plays in (renderer/app.js KEYS,
/// keep in sync).
pub fn key_of(shelf: &str) -> (i32, [i32; 5]) {
    const SUS: [i32; 5] = [0, 2, 5, 7, 9];
    const MINOR: [i32; 5] = [0, 3, 5, 7, 10];
    match shelf {
        "wii" => (2, PENTATONIC),
        "wiiu" => (4, PENTATONIC),
        "sw" => (-3, SUS),
        "sw2" => (-1, SUS),
        "psp" => (-5, PENTATONIC),
        "ps3" => (-6, PENTATONIC),
        "ps4" => (-7, MINOR),
        "ps5" => (-8, PENTATONIC),
        "x360" => (-2, MINOR),
        "n3ds" => (1, PENTATONIC),
        "ps2" => (-9, PENTATONIC),
        "pc" => (-4, PENTATONIC),
        "media" => (3, SUS),
        _ => (0, PENTATONIC),
    }
}

/// A song of the Melody voice: its tune, the chord under each note, and where each bar starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Song {
    pub name: String,
    pub hue: u32,
    pub notes: Vec<i32>,
    /// the root (0 to 11) and whether it's minor
    pub chords: Vec<(i32, bool)>,
    pub bars: Vec<usize>,
    pub built_in: bool,
}

/// The built-in songs, written for the app in the style of old console soundtracks: "Chord: notes | Chord: notes".
const SONGS: [(&str, u32, &str); 8] = [
    ("Meadow Overture", 120, "C: C5 E5 G5 E5 F5 G5 | F: A5 G5 F5 E5 D5 C5 | G: D5 E5 F5 G5 A5 B5 | C: C6 G5 E5 C5 | Am: A4 C5 E5 A5 G5 E5 | F: F5 E5 D5 C5 A4 C5 | G: B4 D5 G5 F5 E5 D5 | C: C5 E5 G5 C6"),
    ("Lantern Castle", 280, "Dm: D5 F5 A5 F5 D5 A4 | Bb: Bb4 D5 F5 E5 D5 C5 | C: C5 E5 G5 F5 E5 C5 | A: C#5 E5 A5 G5 F5 E5 | Dm: D5 A5 F5 D5 | Gm: G4 Bb4 D5 G5 F5 D5 | A: E5 C#5 A4 C#5 E5 G5 | Dm: F5 E5 D5"),
    ("Tide Pool Waltz", 195, "F: A4 C5 F5 A5 G5 F5 | Dm: D5 F5 A5 G5 F5 D5 | Bb: D5 F5 Bb5 A5 G5 F5 | C: E5 G5 C6 Bb5 A5 G5 | F: F5 A5 C6 A5 F5 C5 | Gm: Bb4 D5 G5 F5 E5 D5 | C: C5 E5 G5 Bb5 A5 G5 | F: F5 C5 A4 F5"),
    ("Turbo Coast", 210, "Em: E5 G5 B5 G5 E5 B4 E5 G5 | C: C5 E5 G5 C6 B5 G5 | D: D5 F#5 A5 D6 C6 A5 | Em: B5 A5 G5 F#5 E5 | C: E5 G5 C6 E6 D6 C6 | D: A5 F#5 D5 F#5 A5 D6 | B: D#6 B5 F#5 D#5 B4 D#5 | Em: E5 G5 B5 E6"),
    ("Starlight Lullaby", 230, "G: B4 D5 G5 D5 | Em: B4 E5 G5 E5 | C: C5 E5 G5 A5 G5 E5 | D: F#5 A5 D5 F#5 | G: G5 B5 D6 B5 | C: C6 B5 A5 G5 | D: A5 G5 F#5 E5 D5 | G: G5 D5 B4 G4"),
    ("Snowcap Summit", 185, "Am: A4 E5 A5 B5 C6 B5 A5 E5 | F: F5 A5 C6 A5 F5 C5 | G: G5 B5 D6 B5 G5 D5 | E: E5 G#5 B5 E6 D6 B5 | Am: C6 A5 E5 C5 A4 C5 | Dm: D5 F5 A5 D6 C6 A5 | E: B5 G#5 E5 G#5 B5 D6 | Am: C6 B5 A5"),
    ("Clockwork Town", 40, "Bb: Bb4 D5 F5 D5 Bb4 F5 | Eb: G5 Eb5 Bb4 Eb5 G5 Bb5 | F: A5 F5 C5 F5 A5 C6 | Bb: Bb5 F5 D5 Bb4 | Gm: D5 G5 Bb5 G5 D5 Bb4 | Eb: Eb5 G5 Bb5 C6 Bb5 G5 | F: F5 A5 C6 Eb6 D6 C6 | Bb: Bb5 F5 D5 Bb5"),
    ("Ember Depths", 12, "Cm: C5 Eb5 G5 Eb5 C5 G4 | Ab: Ab4 C5 Eb5 C5 Ab4 Eb5 | Fm: F5 Ab5 C6 Ab5 F5 C5 | G: B4 D5 G5 F5 D5 B4 | Cm: G5 Eb5 C5 Eb5 G5 C6 | Ab: C6 Ab5 Eb5 Ab5 C6 Eb6 | G: D6 B5 G5 F5 D5 B4 | Cm: C5 G4 Eb5 C5"),
];

fn pitch_class(letter: u8, mark: Option<u8>) -> i32 {
    let base = match letter {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        _ => 11,
    };
    base + match mark {
        Some(b'#') => 1,
        Some(b'b') => -1,
        _ => 0,
    }
}

/// "C#5" as a MIDI note.
fn midi_of(n: &str) -> Option<i32> {
    let b = n.as_bytes();
    let letter = *b.first().filter(|l| (b'A'..=b'G').contains(l))?;
    let mark = b.get(1).copied().filter(|m| *m == b'#' || *m == b'b');
    let octave: i32 = n[1 + usize::from(mark.is_some())..].parse().ok().filter(|o| (-9..=9).contains(o))?;
    Some(12 * (octave + 1) + pitch_class(letter, mark))
}

/// "Bbm" as a root and whether it's minor ("maj" isn't).
fn chord_of(c: &str) -> (i32, bool) {
    let b = c.as_bytes();
    let Some(letter) = b.first().copied().filter(|l| (b'A'..=b'G').contains(l)) else { return (0, false) };
    let mark = b.get(1).copied().filter(|m| *m == b'#' || *m == b'b');
    let rest = &c[1 + usize::from(mark.is_some())..];
    ((pitch_class(letter, mark) + 12) % 12, rest.starts_with('m') && !rest.starts_with("maj"))
}

fn parse(name: &str, hue: u32, tune: &str) -> Song {
    let mut song = Song { name: name.to_string(), hue, notes: Vec::new(), chords: Vec::new(), bars: Vec::new(), built_in: true };
    for bar in tune.split('|') {
        let (chord, body) = bar.split_once(':').unwrap_or((bar, ""));
        let chord = chord_of(chord.trim());
        song.bars.push(song.notes.len());
        for m in body.split_whitespace().filter_map(midi_of) {
            song.notes.push(m);
            song.chords.push(chord);
        }
    }
    song
}

pub fn built_in_songs() -> Vec<Song> {
    SONGS.iter().map(|(name, hue, tune)| parse(name, *hue, tune)).collect()
}

/// A standard MIDI file as a song: the top melody line (one note per onset) as the tune, and the bass under it as
/// the chords. The melody is the part whose notes sit highest and come most often (a tune, not a pad); the drums'
/// channel is left out.
pub fn parse_midi(d: &[u8], name: &str) -> Result<Song, &'static str> {
    const CUT: &str = "The MIDI file is cut short.";
    const NO_TUNE: &str = "No tune found in this MIDI file.";
    struct Note {
        t: u32,
        ch: u8,
        n: i32,
        track: usize,
    }
    let u16_at = |o: usize| d.get(o..o + 2).map(|b| u32::from(u16::from_be_bytes([b[0], b[1]])));
    let u32_at = |o: usize| d.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize);
    if d.len() < 14 || &d[..4] != b"MThd" {
        return Err("Not a MIDI file.");
    }
    let (tracks, div) = (u16_at(10).ok_or(CUT)? as usize, u16_at(12).ok_or(CUT)?);
    if div & 0x8000 != 0 {
        return Err("This MIDI timing format isn't supported.");
    }
    let mut p = 8usize.saturating_add(u32_at(4).ok_or(CUT)?);
    let mut events: Vec<Note> = Vec::new();
    for track in 0..tracks {
        if p.saturating_add(8) > d.len() || &d[p..p + 4] != b"MTrk" {
            break;
        }
        let len = u32_at(p + 4).ok_or(CUT)?;
        let mut q = p + 8;
        let end = d.len().min(q.saturating_add(len));
        p = q.saturating_add(len);
        let (mut t, mut running) = (0u32, 0u8);
        // a number of any length, seven bits to a byte
        let number = |q: &mut usize| -> Result<u32, &'static str> {
            let mut v = 0u32;
            loop {
                let b = *d.get(*q).ok_or(CUT)?;
                *q += 1;
                v = (v << 7) | u32::from(b & 0x7f);
                if b & 0x80 == 0 || *q >= end {
                    return Ok(v);
                }
            }
        };
        while q < end {
            t = t.wrapping_add(number(&mut q)?);
            let mut status = *d.get(q).ok_or(CUT)?;
            // a data byte: the status before it still stands
            if status < 0x80 {
                status = running;
            } else {
                q += 1;
                if status < 0xf0 {
                    running = status;
                }
            }
            let kind = status & 0xf0;
            if status == 0xff {
                q += 1;
                let len = number(&mut q)? as usize;
                q = q.saturating_add(len);
            } else if status == 0xf0 || status == 0xf7 {
                let len = number(&mut q)? as usize;
                q = q.saturating_add(len);
            } else if kind == 0x90 || kind == 0x80 {
                let (n, v) = (*d.get(q).ok_or(CUT)?, *d.get(q + 1).ok_or(CUT)?);
                q += 2;
                if kind == 0x90 && v > 0 {
                    events.push(Note { t, ch: status & 15, n: i32::from(n), track });
                }
            } else if kind == 0xc0 || kind == 0xd0 {
                q += 1;
            } else if (0xa0..=0xe0).contains(&kind) {
                q += 2;
            } else {
                break;
            }
            if events.len() > 60_000 {
                break;
            }
        }
    }
    // channel 10 is drums
    let tones: Vec<&Note> = events.iter().filter(|e| e.ch != 9).collect();
    if tones.len() < 8 {
        return Err(NO_TUNE);
    }
    let mut parts: Vec<(usize, Vec<&Note>)> = Vec::new();
    for e in &tones {
        let key = e.track * 16 + usize::from(e.ch);
        match parts.iter_mut().find(|p| p.0 == key) {
            Some(p) => p.1.push(e),
            None => parts.push((key, vec![e])),
        }
    }
    let mut best: Option<(usize, f64)> = None;
    for (key, part) in &parts {
        if part.len() < 8 {
            continue;
        }
        let mut onsets: Vec<u32> = part.iter().map(|e| e.t).collect();
        onsets.sort_unstable();
        onsets.dedup();
        let mean = part.iter().map(|e| f64::from(e.n)).sum::<f64>() / part.len() as f64;
        let score = onsets.len() as f64 * ((mean - 40.0).max(0.0) / 30.0).powf(1.5);
        if best.is_none_or(|b| score > b.1) {
            best = Some((*key, score));
        }
    }
    let (best, _) = best.ok_or(NO_TUNE)?;
    let in_best = |e: &Note| e.track * 16 + usize::from(e.ch) == best;
    // one note per onset: the highest at each moment
    let mut line: Vec<(u32, i32)> = Vec::new();
    for e in tones.iter().filter(|e| in_best(e)) {
        match line.iter_mut().find(|l| l.0 == e.t) {
            Some(l) => l.1 = l.1.max(e.n),
            None => line.push((e.t, e.n)),
        }
    }
    line.sort_by_key(|l| l.0);
    line.truncate(600);
    // the bass under each note: the lowest other note sounding at it or just before
    let mut others: Vec<&&Note> = tones.iter().filter(|e| !in_best(e)).collect();
    others.sort_by_key(|e| e.t);
    let (mut low_from, mut bass): (usize, Option<i32>) = (0, None);
    let mut chords = Vec::new();
    for (t, n) in &line {
        while low_from < others.len() && i64::from(others[low_from].t) < i64::from(*t) - i64::from(div) {
            low_from += 1;
        }
        if let Some(low) = others[low_from..].iter().take_while(|e| e.t <= *t).map(|e| e.n).min() {
            bass = Some(low);
        }
        chords.push((bass.unwrap_or(*n) % 12, false));
    }
    // into a comfortable piano range: the middle of the tune near E5
    let mean = line.iter().map(|l| f64::from(l.1)).sum::<f64>() / line.len() as f64;
    let shift = 12 * crate::js::round((76.0 - mean) / 12.0) as i32;
    let notes = line.iter().map(|l| (l.1 + shift).clamp(48, 96)).collect();
    let (mut bars, mut last_bar) = (Vec::new(), -1.0f64);
    for (i, (t, _)) in line.iter().enumerate() {
        // four beats to a bar
        let bar = (f64::from(*t) / f64::from(div * 4)).floor();
        if bar != last_bar {
            bars.push(i);
            last_bar = bar;
        }
    }
    // (the first UTF-16 unit of each character, as the Electron app's `charCodeAt(0)` gives it)
    let hue = name.chars().fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(u32::from(c.encode_utf16(&mut [0; 2])[0]))) % 360;
    Ok(Song { name: if name.is_empty() { "Song".into() } else { name.to_string() }, hue, notes, chords, bars, built_in: false })
}

/// A cue: its voices, the level of the whole (the boot's own volume), and how the menu music steps back for it:
/// how far (0 to 1) and for how long (ms). None: it doesn't.
pub struct Cue {
    pub voices: Vec<Voice>,
    pub level: f32,
    pub duck: Option<(f32, f32)>,
}

/// Twelve places of the figure quick steps ride: up to four above home, down to two below.
const TRI: [i32; 12] = [0, 1, 2, 3, 4, 3, 2, 1, 0, -1, -2, -1];

fn tri(n: f64) -> i32 {
    TRI[(n.round() as i64).rem_euclid(TRI.len() as i64) as usize]
}

const BELL: &[Partial] = &[(1.0, 1.0, 1.0), (2.0, 0.28, 0.55), (3.0, 0.1, 0.38), (4.16, 0.05, 0.28), (5.4, 0.02, 0.2)];
const SONG_MINUTES: f64 = 3.0;

fn hit(freq: f32) -> Hit {
    Hit::default().freq(freq)
}

fn air(filter: Filter, soft: bool) -> Air {
    Air { filter, soft, ..Air::default() }
}

fn fm(freq: f32) -> Fm {
    Fm::default().freq(freq)
}

/// A MIDI note's pitch.
fn mf(m: i32) -> f32 {
    440.0 * 2f32.powf((m - 69) as f32 / 12.0)
}

/// The interface's sound as it stands: its voice, the shelf's key, and where the scrolling figures and the song are.
pub struct Sound {
    theme: &'static Theme,
    key: i32,
    scale: [i32; 5],
    /// the boot picked in Settings, and its own volume
    boot: String,
    boot_level: f32,
    last_tick: f64,
    tick_run: f64,
    tick_var: usize,
    last_shelf: f64,
    shelf_run: i32,
    songs: Vec<Song>,
    /// your own songs, and whether the built-in ones play along with them
    own_songs: Vec<Song>,
    order: Vec<usize>,
    order_at: usize,
    song: Option<usize>,
    song_idx: usize,
    song_at: f64,
    song_loops: u32,
    last_note: f64,
    seed: u64,
    /// nothing left to chance: every random pick is the middle one
    steady: bool,
    out: Vec<Voice>,
}

impl Default for Sound {
    fn default() -> Self {
        Sound {
            theme: &THEMES[0],
            key: 0,
            scale: PENTATONIC,
            boot: "chime".into(),
            boot_level: 1.0,
            last_tick: 0.0,
            tick_run: 0.0,
            tick_var: 0,
            last_shelf: 0.0,
            shelf_run: 0,
            songs: built_in_songs(),
            own_songs: Vec::new(),
            order: Vec::new(),
            order_at: 0,
            song: None,
            song_idx: 0,
            song_at: 0.0,
            song_loops: 0,
            last_note: 0.0,
            seed: 0x2545_f491_4f6c_dd1d,
            steady: false,
            out: Vec::new(),
        }
    }
}

impl Sound {
    pub fn set_theme(&mut self, name: &str) {
        self.theme = theme(name);
    }

    pub fn theme(&self) -> &'static Theme {
        self.theme
    }

    /// The shelf the cues are tuned to.
    pub fn set_shelf(&mut self, shelf: &str) {
        (self.key, self.scale) = key_of(shelf);
    }

    /// Settings → Sounds: the boot ("chime", "crystal", "pulse", "horizon", "keys") and its volume, 0 to 1.
    pub fn set_boot(&mut self, boot: &str, level: f32) {
        self.boot = boot.to_string();
        self.boot_level = level.clamp(0.0, 1.0);
    }

    /// Your own songs for the Melody voice (the Songs folder's), and whether the built-in ones play along. With
    /// none of your own the built-in ones play whatever the switch says.
    pub fn set_songs(&mut self, own: Vec<Song>, with_built_in: bool, now: f64) {
        self.own_songs = own;
        let was = self.song().cloned();
        self.songs = if with_built_in || self.own_songs.is_empty() { built_in_songs() } else { Vec::new() };
        self.songs.extend(self.own_songs.iter().cloned());
        self.order.clear();
        self.order_at = 0;
        // the song that was playing carries on if it's still there
        self.song = was.and_then(|w| self.songs.iter().position(|s| *s == w));
        if self.song.is_none() {
            self.next_song(now);
        }
    }

    /// Whether the built-in songs play along with your own.
    pub fn with_built_in(&mut self, with: bool, now: f64) {
        let own = std::mem::take(&mut self.own_songs);
        self.set_songs(own, with, now);
    }

    /// The song the Melody voice is playing.
    pub fn song(&self) -> Option<&Song> {
        self.song.map(|i| &self.songs[i])
    }

    /// Takes chance out of the cues (the small differences from one tick to the next, the songs' order), for
    /// comparing them.
    #[doc(hidden)]
    pub fn steady(&mut self) {
        self.steady = true;
    }

    /// 0 up to 1.
    fn random(&mut self) -> f32 {
        if self.steady {
            return 0.5;
        }
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 40) as f32 / (1u64 << 24) as f32
    }

    /// The n-th step of the scale (below home too), `octave` octaves away, as a pitch.
    fn deg(&self, n: i32, octave: i32) -> f32 {
        let len = self.scale.len() as i32;
        let step = self.scale[n.rem_euclid(len) as usize];
        self.theme.root * 2f32.powf((self.key + step + 12 * (n.div_euclid(len) + octave)) as f32 / 12.0)
    }

    fn d(&self, n: i32) -> f32 {
        self.deg(n, 0)
    }

    fn put(&mut self, v: impl Into<Voice>) {
        self.out.push(v.into());
    }

    /// A soft chord of scale steps that fades in over a breath of air.
    fn swell(&mut self, notes: &[i32], octave: i32, dur: f32, vol: f32, at: f32, attack: f32) {
        let freqs = notes.iter().map(|n| self.deg(*n, octave)).collect();
        self.put(Swell { freqs, ..Swell::default() }.dur(dur).vol(vol).at(at).attack(attack));
    }

    /// Each quick step walks one place along the figure in the direction of the move; held scrolling goes by time
    /// (a place every 160 ms or so), and a pause brings it home.
    fn walk(&mut self, dir: i32, now: f64) -> i32 {
        let gap = now - self.last_tick;
        self.last_tick = now;
        self.tick_run = if gap < 2500.0 { self.tick_run + if dir < 0 { -1.0 } else { 1.0 } * (gap / 160.0).min(1.0) } else { 0.0 };
        tri(self.tick_run)
    }

    /// Flicking through shelves quickly: a short bright step instead of the whole shelf phrase each time.
    fn shelf_quick(&mut self, dir: i32, now: f64) -> bool {
        let quick = now - self.last_shelf < 600.0;
        self.shelf_run = if now - self.last_shelf < 2500.0 { self.shelf_run + if dir < 0 { -1 } else { 1 } } else { 0 };
        self.last_shelf = now;
        if !quick {
            return false;
        }
        let step = tri(f64::from(self.shelf_run));
        let pan = (self.random() - 0.5) * 0.3;
        self.put(hit(self.d(step)).dur(0.1).vol(0.3).verb(0.4).pan(pan).attack(0.006).breath(0.25).duck(0.15));
        self.put(hit(self.d(4 + step)).dur(0.14).vol(0.18).at(0.05).verb(0.5).attack(0.01).breath(0.25).duck(0.0));
        true
    }

    // ---------- the boots, each in the shelf's own key ----------

    fn thump_low(&mut self, at: f32, vol: f32) {
        self.put(hit(self.deg(0, -2)).dur(0.45).vol(vol).at(at).attack(0.003).under(1.0).bright(420.0).verb(0.2).noise(0.0).breath(0.0).duck(0.0));
    }

    fn boot(&mut self) {
        match self.boot.as_str() {
            // a fast glassy run up the scale that blooms into a ringing chord
            "crystal" => {
                self.thump_low(0.0, 0.16);
                for i in 0..5 {
                    self.put(fm(self.deg(i, 1)).at(0.1 + i as f32 * 0.075).dur(0.9).vol(0.22).ratio(3.5).index(2.2).pan((i - 2) as f32 * 0.18).verb(0.45));
                }
                for (n, o, v, p) in [(5, 1, 0.3, 0.0), (7, 1, 0.2, 0.25), (3, 0, 0.2, -0.25)] {
                    self.put(fm(self.deg(n, o)).at(0.5).dur(2.4).vol(v).ratio(3.5).index(1.6).pan(p).verb(0.6).detune(4.0));
                }
                self.put(fm(self.deg(9, 1)).at(0.95).dur(1.0).vol(0.06).ratio(5.0).index(1.0).pan(0.4).verb(0.8));
            }
            // two quick notes and a punchy chord, short and snappy like a power-on
            "pulse" => {
                self.thump_low(0.0, 0.26);
                self.put(fm(self.d(3)).at(0.02).dur(0.35).vol(0.3).ratio(1.0).index(3.0).verb(0.25).pan(-0.15));
                self.put(fm(self.d(5)).at(0.16).dur(0.35).vol(0.3).ratio(1.0).index(3.0).verb(0.25).pan(0.15));
                for (i, (n, o)) in [(5, 0), (7, 0), (8, 0), (5, 1)].into_iter().enumerate() {
                    self.put(fm(self.deg(n, o)).at(0.32).dur(1.5).vol(0.2).ratio(1.0).index(2.2).floor(0.15).pan((i as f32 - 1.5) * 0.2).verb(0.45).detune(6.0));
                }
                self.put(fm(self.deg(5, 1)).at(0.34).dur(1.3).vol(0.12).ratio(3.5).index(1.2).verb(0.6));
            }
            // a deep swell opens up and lands on a bright chord with a shimmering tail
            "horizon" => {
                let freqs = vec![self.deg(0, -1), self.deg(3, -1), self.deg(5, -1)];
                self.put(Pad { freqs, ..Pad::default() }.dur(2.6).vol(0.3).from(180.0).to(3200.0).open(1.3).attack(1.1).hold(1.4));
                self.thump_low(1.25, 0.24);
                for (n, o, v, p) in [(0, 1, 0.26, 0.0), (3, 1, 0.18, -0.3), (5, 1, 0.2, 0.3), (7, 1, 0.1, 0.1)] {
                    self.put(fm(self.deg(n, o)).at(1.25).dur(2.2).vol(v).ratio(3.5).index(2.0).pan(p).verb(0.65).detune(3.0));
                }
                for (i, n) in [9, 10, 12].into_iter().enumerate() {
                    self.put(fm(self.deg(n, 1)).at(1.55 + i as f32 * 0.11).dur(0.7).vol(0.05).ratio(4.0).index(1.0).pan((i as f32 - 1.0) * 0.4).verb(0.9));
                }
            }
            // a warm electric-piano chord rolled upwards with a bell on top
            "keys" => {
                self.thump_low(0.0, 0.14);
                for (i, n) in [0, 2, 3, 5, 7].into_iter().enumerate() {
                    self.put(fm(self.d(n)).at(0.08 + i as f32 * 0.06).dur(2.0).vol(0.2).ratio(1.0).index(1.8).floor(0.2).pan((i as f32 - 2.0) * 0.15).verb(0.5).detune(5.0));
                }
                self.put(fm(self.deg(5, 1)).at(0.45).dur(1.8).vol(0.16).ratio(3.5).index(1.8).verb(0.6));
                self.put(fm(self.deg(7, 1)).at(0.62).dur(1.4).vol(0.09).ratio(3.5).index(1.4).verb(0.7).pan(0.3));
            }
            _ => self.boot_chime(),
        }
    }

    /// The boot you know from the first note: a soft power-on thump, a breath of air sweeping up, four clear bell
    /// notes rising and a bright open chord that rings out over a warm pad, with two tiny sparkles at the top.
    fn boot_chime(&mut self) {
        let bell = |h: Hit| Hit { partials: Some(BELL), ..h }.noise(0.0).breath(0.0);
        self.put(hit(self.deg(0, -2)).dur(0.5).vol(0.2).attack(0.004).under(0.8).bright(500.0).verb(0.25).noise(0.0).breath(0.0).duck(0.0));
        self.put(air(Filter::Highpass, true).dur(0.7).vol(0.06).freq(1800.0).sweep_to(7000.0).q(0.5).verb(0.5).attack(0.25));
        for (i, (n, t)) in [(0, 0.14), (3, 0.29), (4, 0.44)].into_iter().enumerate() {
            self.put(bell(hit(self.deg(n, 1)).dur(0.55).vol(0.26).at(t).attack(0.002).verb(0.45).pan((i as f32 - 1.0) * 0.22)).duck(0.0));
        }
        // the landing: the octave on top, the third above it, the fifth underneath
        self.put(bell(hit(self.deg(5, 1)).dur(2.1).vol(0.26).at(0.62).attack(0.002).verb(0.6).pan(0.05)));
        self.put(bell(hit(self.deg(7, 1)).dur(1.8).vol(0.15).at(0.65).attack(0.003).verb(0.65).pan(0.25)));
        self.put(bell(hit(self.d(3)).dur(1.9).vol(0.16).at(0.64).attack(0.004).verb(0.6).pan(-0.25)));
        self.swell(&[0, 2, 3], -1, 2.2, 0.1, 0.55, 0.35);
        for (n, t, p) in [(10, 1.02, 0.3), (12, 1.16, -0.3)] {
            self.put(bell(hit(self.deg(n, 1)).dur(0.5).vol(0.07).at(t).attack(0.002).verb(0.8).pan(p)).duck(0.0));
        }
    }

    // ---------- the cues every voice shares unless it has its own ----------

    fn shared(&mut self, name: &str, dir: i32, now: f64) {
        let d = |s: &Sound, n: i32| s.d(n);
        match name {
            // one box: short, pitched, climbs and tightens as you scroll faster
            "tick" => {
                let n = self.walk(dir, now);
                let pan = (self.random() - 0.5) * 0.15;
                self.put(hit(d(self, 5 + n)).dur(0.075 - n.abs() as f32 * 0.005).vol(0.5).verb(0.14).pan(pan).thump(1.5));
            }
            // changing shelf: a quick airy whoosh and a low-high pair, wide in stereo
            "shelf" => {
                if self.shelf_quick(dir, now) {
                    return;
                }
                self.put(air(Filter::Bandpass, true).dur(0.24).vol(0.3).freq(500.0).sweep_to(3200.0).q(0.7).verb(0.45));
                self.put(hit(d(self, 0)).dur(0.24).vol(0.32).at(0.02).pan(-0.3));
                self.put(hit(d(self, 4)).dur(0.34).vol(0.3).at(0.1).pan(0.3));
            }
            "open" => {
                self.put(hit(d(self, 3)).dur(0.18).vol(0.32));
                self.put(hit(d(self, 7)).dur(0.3).vol(0.3).at(0.075).pan(0.2));
            }
            "back" => {
                self.put(hit(d(self, 3)).dur(0.14).vol(0.3));
                self.put(hit(d(self, 0)).dur(0.26).vol(0.28).at(0.07).pan(-0.2).bright(3000.0));
            }
            // Play: a rising three-note figure that lands on the octave, with air swelling under it
            "go" => {
                for (i, n) in [0, 2, 5].into_iter().enumerate() {
                    self.put(hit(d(self, n)).dur(0.3 + i as f32 * 0.1).vol(0.36).at(i as f32 * 0.095).pan((i as f32 - 1.0) * 0.25));
                }
                self.swell(&[0, 2, 5], -1, 1.1, 0.14, 0.1, 0.3);
            }
            "flip" => {
                self.put(air(Filter::Bandpass, false).dur(0.2).vol(0.3).freq(1200.0).sweep_to(350.0).q(1.0).verb(0.2));
                self.put(hit(d(self, -2)).dur(0.12).vol(0.34).at(0.11).noise(0.5).thump(1.2));
            }
            "caseOpen" => {
                self.put(hit(self.deg(9, 1)).dur(0.045).vol(0.5).noise(1.2).verb(0.08).thump(1.6));
                self.put(hit(d(self, -5)).dur(0.3).vol(0.22).at(0.1).bright(1600.0).under(0.4));
            }
            "caseClose" => {
                self.put(hit(d(self, -5)).dur(0.11).vol(0.55).noise(0.8).bright(1500.0).under(0.7).thump(1.1));
                self.put(hit(d(self, 8)).dur(0.04).vol(0.3).noise(1.2).at(0.06).verb(0.05));
            }
            "discOut" => {
                self.put(hit(d(self, 4)).dur(0.2).vol(0.26));
                self.put(hit(d(self, 9)).dur(0.34).vol(0.22).at(0.07).pan(0.25));
                self.put(air(Filter::Highpass, false).dur(0.4).vol(0.12).freq(3500.0).verb(0.5).attack(0.12));
            }
            "page" => {
                self.put(air(Filter::Bandpass, false).dur(0.14).vol(0.45).freq(2200.0).sweep_to(600.0).q(0.6).verb(0.12).pan(0.1));
                self.put(hit(d(self, 6)).dur(0.05).vol(0.25).noise(0.8).verb(0.05));
            }
            "hold" => self.put(hit(d(self, 10)).dur(0.04).vol(0.3).noise(0.0).verb(0.0)),
            "error" => {
                self.put(Hit { partials: Some(&[(1.0, 1.0, 1.0), (1.5, 0.35, 0.8)]), ..hit(d(self, -7)) }.dur(0.22).vol(0.36).bright(1100.0).thump(0.9));
                self.put(hit(d(self, -8)).dur(0.3).vol(0.28).at(0.12).bright(900.0).thump(0.9));
            }
            "toast" => {
                self.put(hit(d(self, 8)).dur(0.14).vol(0.34).verb(0.3).pan(0.4));
                self.put(hit(d(self, 10)).dur(0.2).vol(0.26).at(0.05).verb(0.4).pan(0.4));
            }
            // an achievement: a quick upward sparkle landing on a held high note, lots of room
            "unlock" => {
                for (i, n) in [0, 2, 4, 5, 7].into_iter().enumerate() {
                    self.put(hit(self.deg(n, 1)).dur(0.35).vol(0.28).at(i as f32 * 0.06).verb(0.55).pan((i as f32 - 2.0) * 0.18));
                }
                self.put(Hit { partials: Some(&[(1.0, 1.0, 1.0), (2.0, 0.15, 0.6)]), ..hit(self.deg(9, 1)) }.dur(0.9).vol(0.2).at(0.34).verb(0.75));
                self.swell(&[0, 4], 0, 1.4, 0.12, 0.25, 0.4);
            }
            "boot" => self.boot(),
            // the insert animation's foley
            "lidClick" => self.put(hit(d(self, 7)).dur(0.045).vol(0.6).noise(1.2).verb(0.08).thump(1.7)),
            "seat" => self.put(hit(d(self, -9)).dur(0.24).vol(0.4).bright(700.0).noise(0.4).under(0.8).thump(1.1)),
            "whir" => {
                self.put(air(Filter::Bandpass, false).dur(1.1).vol(0.2).freq(110.0).sweep_to(1100.0).q(2.5).verb(0.3).attack(0.2).hold(0.35));
                self.put(air(Filter::Bandpass, false).dur(1.0).vol(0.08).freq(2400.0).sweep_to(5200.0).q(3.0).verb(0.3).attack(0.35).hold(0.2));
            }
            // the museum: a picture lamp switching on, a case sliding on wood and back, lifting one, a disc on its hub
            "lamp" => self.put(hit(self.deg(-9, -1)).dur(0.12).vol(0.05).bright(700.0).noise(0.25).under(0.4).attack(0.004).breath(0.0).verb(0.5).duck(0.0)),
            // the museum opening: a slow warm chord blooming up with a breath of air, like lights coming up
            "museum" => {
                const WARM: &[Partial] = &[(1.0, 1.0, 1.0), (2.0, 0.32, 0.6), (3.0, 0.1, 0.4), (4.2, 0.05, 0.3)];
                for i in 0..8 {
                    let f = i as f32;
                    self.put(Hit { partials: Some(WARM), ..hit(d(self, i)) }.dur(0.32 + f * 0.02).vol(0.24).at(f * 0.072 - f * f * 0.0015).verb(0.55).pan((f - 3.5) * 0.09).attack(0.003).breath(0.0).noise(0.0).duck(0.2));
                }
                // a soft landing: the run resolves down onto a warm mid chord
                self.put(Hit { partials: Some(WARM), ..hit(d(self, 5)) }.dur(1.5).vol(0.18).at(0.6).verb(0.85).attack(0.02).pan(0.12));
                self.put(Hit { partials: Some(WARM), ..hit(d(self, 2)) }.dur(1.5).vol(0.14).at(0.62).verb(0.85).attack(0.03).pan(-0.12));
                self.put(hit(self.deg(0, -1)).dur(1.6).vol(0.12).at(0.64).verb(0.8).attack(0.04).breath(0.3));
                self.swell(&[-3, 0, 4], -1, 2.0, 0.14, 0.5, 0.2);
            }
            "slide" => {
                self.put(air(Filter::Bandpass, true).dur(0.22).vol(0.16).freq(900.0).sweep_to(1600.0).q(0.9).verb(0.35).attack(0.02));
                self.put(hit(self.deg(-5, -1)).dur(0.08).vol(0.1).at(0.16).noise(0.4).breath(0.0).verb(0.3));
            }
            "slideBack" => {
                self.put(air(Filter::Bandpass, true).dur(0.2).vol(0.1).freq(1300.0).sweep_to(800.0).q(0.9).verb(0.3).attack(0.015));
                self.put(hit(self.deg(-7, -1)).dur(0.07).vol(0.07).at(0.15).noise(0.4).breath(0.0).verb(0.25));
            }
            "lift" => {
                self.put(air(Filter::Bandpass, true).dur(0.26).vol(0.12).freq(700.0).sweep_to(1500.0).q(0.8).verb(0.3).attack(0.03));
                self.put(hit(d(self, 2)).dur(0.12).vol(0.07).at(0.14).verb(0.3).attack(0.01).breath(0.2));
            }
            "discIn" => {
                self.put(hit(d(self, 7)).dur(0.1).vol(0.14).noise(0.3).attack(0.004).breath(0.0));
                self.put(hit(d(self, 0)).dur(0.18).vol(0.12).at(0.08).attack(0.006).thump(1.3).breath(0.0));
            }
            // the console's pedestal: a short warm three-note rise, like a system powering up
            "pedestal" => {
                for (i, n) in [0, 4, 7].into_iter().enumerate() {
                    self.put(hit(d(self, n)).dur(0.5 + i as f32 * 0.1).vol(0.12).at(i as f32 * 0.09).verb(0.7).attack(0.006).breath(0.2).pan((i as f32 - 1.0) * 0.15).duck(0.1));
                }
                self.put(hit(self.deg(0, -1)).dur(1.2).vol(0.07).at(0.25).verb(0.9).attack(0.05).breath(0.3));
            }
            // the quiet settle when a motion lands
            "arrive" => self.put(hit(d(self, 0)).dur(0.1).vol(0.05).verb(0.3).attack(0.01).breath(0.1).duck(0.0)),
            "panel" => {
                self.put(air(Filter::Bandpass, true).dur(0.2).vol(0.07).freq(700.0).sweep_to(2200.0).q(0.6).verb(0.4));
                self.put(hit(d(self, 2)).dur(0.16).vol(0.08).at(0.1).attack(0.01).duck(0.05));
            }
            _ => {}
        }
    }

    // ---------- Aero: fewer notes, closer intervals, breath instead of attack ----------

    fn aero(&mut self, name: &str, dir: i32, now: f64) -> bool {
        match name {
            // one of three slightly different takes, a tiny bit off pitch each time
            "tick" => {
                let n = self.walk(dir, now);
                self.tick_var = (self.tick_var + 1) % 3;
                let v = self.tick_var;
                let jitter = 1.0 + (self.random() - 0.5) * 0.01;
                let pan = (self.random() - 0.5) * 0.2;
                let parts: &'static [Partial] = [&[(1.0, 1.0, 1.0), (2.0, 0.12, 0.5)][..], &[(1.0, 1.0, 1.0), (2.01, 0.1, 0.5), (3.98, 0.03, 0.3)], &[(1.0, 1.0, 1.0), (1.99, 0.14, 0.45)]][v];
                let v = v as f32;
                self.put(Hit { partials: Some(parts), ..hit(self.d(2 + n) * jitter) }.dur(0.085 + v * 0.006).vol(0.34 + v * 0.015).verb(0.35).pan(pan).attack(0.005 + v * 0.002).breath(0.3 + v * 0.05).duck(0.15));
            }
            "shelf" => {
                if self.shelf_quick(dir, now) {
                    return true;
                }
                self.put(air(Filter::Bandpass, true).dur(0.5).vol(0.16).freq(900.0).sweep_to(4500.0).q(0.5).verb(0.7).attack(0.12));
                self.put(hit(self.d(0)).dur(0.5).vol(0.2).at(0.05).pan(-0.2).attack(0.03));
                self.put(hit(self.d(4)).dur(0.6).vol(0.16).at(0.16).pan(0.2).attack(0.04));
            }
            "open" => {
                self.put(hit(self.d(2)).dur(0.26).vol(0.32).attack(0.008));
                self.put(hit(self.d(4)).dur(0.42).vol(0.26).at(0.06).attack(0.02).pan(0.15));
            }
            "back" => {
                self.put(hit(self.d(2)).dur(0.2).vol(0.3).attack(0.008).bright(4000.0));
                self.put(hit(self.d(-1)).dur(0.36).vol(0.22).at(0.06).bright(2600.0).pan(-0.15));
            }
            "go" => {
                self.put(hit(self.d(0)).dur(0.9).vol(0.3).attack(0.02).verb(0.8));
                self.put(hit(self.d(4)).dur(1.1).vol(0.26).at(0.22).attack(0.05).verb(0.85).pan(0.2));
                self.swell(&[0, 4], -2, 1.6, 0.2, 0.05, 0.45);
                self.put(air(Filter::Bandpass, true).dur(1.2).vol(0.16).freq(1500.0).sweep_to(7000.0).q(0.4).verb(0.8).attack(0.5));
            }
            "flip" => {
                self.put(air(Filter::Bandpass, true).dur(0.28).vol(0.26).freq(2500.0).sweep_to(500.0).q(0.7).verb(0.5).attack(0.02));
                self.put(hit(self.d(-3)).dur(0.18).vol(0.2).at(0.12).attack(0.01).breath(0.3));
            }
            "caseOpen" => {
                self.put(hit(self.d(6)).dur(0.06).vol(0.32).noise(0.9).verb(0.2).attack(0.002).breath(0.0));
                self.put(hit(self.deg(-5, -1)).dur(0.4).vol(0.22).at(0.08).bright(1600.0).under(0.4).attack(0.01));
            }
            "caseClose" => {
                self.put(hit(self.deg(-5, -1)).dur(0.14).vol(0.32).noise(0.5).bright(1400.0).under(0.6).attack(0.003).breath(0.0));
                self.put(hit(self.d(6)).dur(0.04).vol(0.14).noise(1.0).at(0.06).verb(0.1).breath(0.0));
            }
            "discOut" => {
                self.put(hit(self.d(4)).dur(0.36).vol(0.2).attack(0.01));
                self.put(hit(self.d(7)).dur(0.6).vol(0.15).at(0.09).attack(0.03).pan(0.25));
                self.put(air(Filter::Highpass, true).dur(0.6).vol(0.1).freq(4000.0).verb(0.7).attack(0.15));
            }
            "page" => self.put(air(Filter::Bandpass, true).dur(0.18).vol(0.4).freq(2600.0).sweep_to(700.0).q(0.5).verb(0.25).pan(0.1)),
            "hold" => self.put(hit(self.d(6)).dur(0.05).vol(0.22).noise(0.0).verb(0.1).breath(0.0).attack(0.004)),
            "error" => {
                self.put(Hit { partials: Some(&[(1.0, 1.0, 1.0), (1.5, 0.3, 0.8)]), ..hit(self.d(-6)) }.dur(0.3).vol(0.34).bright(1300.0).attack(0.006).breath(0.2));
                self.put(hit(self.d(-6)).dur(0.3).vol(0.26).at(0.16).bright(1200.0).attack(0.006));
            }
            "toast" => self.put(hit(self.d(5)).dur(0.3).vol(0.26).verb(0.6).pan(0.4).attack(0.01)),
            "unlock" => {
                for (i, n) in [0, 4, 7].into_iter().enumerate() {
                    self.put(hit(self.d(n)).dur(0.7).vol(0.2).at(i as f32 * 0.11).verb(0.8).pan((i as f32 - 1.0) * 0.25).attack(0.02));
                }
                self.put(hit(self.d(9)).dur(1.4).vol(0.18).at(0.4).verb(0.9).attack(0.06));
                self.swell(&[0, 4], -1, 1.8, 0.14, 0.3, 0.5);
            }
            "lidClick" => self.put(hit(self.d(7)).dur(0.045).vol(0.26).noise(1.0).verb(0.1).attack(0.002).breath(0.0).thump(1.6)),
            "seat" => self.put(hit(self.deg(-9, -1)).dur(0.34).vol(0.2).bright(700.0).noise(0.0).under(0.7).attack(0.008).breath(0.0).thump(1.05)),
            "arrive" => self.put(hit(self.deg(0, -1)).dur(0.12).vol(0.04).verb(0.5).attack(0.012).breath(0.2).duck(0.0)),
            "panel" => {
                self.put(air(Filter::Bandpass, true).dur(0.26).vol(0.07).freq(800.0).sweep_to(2800.0).q(0.5).verb(0.6).attack(0.04));
                self.put(hit(self.deg(2, -1)).dur(0.22).vol(0.07).at(0.12).attack(0.015).breath(0.3).duck(0.05));
            }
            _ => return false,
        }
        true
    }

    // ---------- Melody: a song played one note per move ----------

    /// Moves on to another song (every few minutes, a new shelf's loops run out): never the same twice running.
    pub fn next_song(&mut self, now: f64) {
        if self.songs.is_empty() {
            return;
        }
        if self.order_at >= self.order.len() || self.order.len() != self.songs.len() {
            self.order = (0..self.songs.len()).collect();
            for i in (1..self.order.len()).rev() {
                let j = (self.random() * (i + 1) as f32) as usize;
                self.order.swap(i, j.min(i));
            }
            if self.order.len() > 1 && Some(self.order[0]) == self.song {
                self.order.rotate_left(1);
            }
            self.order_at = 0;
        }
        self.song = Some(self.order[self.order_at]);
        self.order_at += 1;
        self.song_idx = 0;
        self.song_loops = 0;
        self.song_at = now;
    }

    fn cur_song(&mut self, now: f64) -> usize {
        if self.song.is_none() {
            self.next_song(now);
        }
        self.song.unwrap_or(0)
    }

    fn chord_at(&mut self, i: usize, now: f64) -> (i32, bool) {
        let s = self.cur_song(now);
        let chords = &self.songs[s].chords;
        chords.get(i.min(chords.len().saturating_sub(1))).copied().unwrap_or((0, false))
    }

    /// The chord under the tune right now, as piano notes around middle C.
    fn triad(&mut self, octave: i32, now: f64) -> [i32; 3] {
        let (root, minor) = self.chord_at(self.song_idx.saturating_sub(1), now);
        let r = 48 + 12 * octave + root;
        [r, r + if minor { 3 } else { 4 }, r + 7]
    }

    /// A piano note, placed left to right by its pitch.
    fn piano(&mut self, m: i32, vol: f32, dur: f32, at: f32) -> Hit {
        let _ = &self;
        hit(mf(m)).dur(dur).attack(0.003).verb(0.5).pan(((m - 72) as f32 / 30.0).clamp(-0.5, 0.5)).duck(0.1).vol(vol * 0.45).at(at)
    }

    /// The bass when a bar changes: the chord's root low and soft, with its octave.
    fn bass_for(&mut self, i: usize, now: f64) {
        let (root, _) = self.chord_at(i, now);
        let r = 36 + root + if root < 5 { 12 } else { 0 };
        let low = self.piano(r, 0.15, 1.9, 0.0).bright(1100.0).verb(0.55).attack(0.006).noise(0.0).pan(-0.1);
        self.put(low);
        let high = self.piano(r + 12, 0.08, 1.6, 0.01).bright(1500.0).attack(0.008).noise(0.0);
        self.put(high);
    }

    fn melody(&mut self, name: &str, now: f64) -> bool {
        match name {
            "tick" => {
                // held scrolling sends a tick every 45 ms or so: the tune keeps its own pace
                if now - self.last_note < 110.0 {
                    return true;
                }
                let gap = now - self.last_note;
                self.last_note = now;
                let s = self.cur_song(now);
                // a few minutes on one song: it changes at the next bar, so the switch lands on a downbeat
                if now - self.song_at > SONG_MINUTES * 60_000.0 && self.songs[s].bars.contains(&self.song_idx) {
                    self.next_song(now);
                }
                let s = self.cur_song(now);
                if self.song_idx >= self.songs[s].notes.len() {
                    self.song_idx = 0;
                    self.song_loops += 1;
                    if self.song_loops >= 3 {
                        self.next_song(now);
                    }
                }
                let s = self.cur_song(now);
                let i = self.song_idx;
                self.song_idx += 1;
                let bar = self.songs[s].bars.contains(&i);
                // quick runs play lighter, a slow deliberate step a little fuller; the first note of a bar leans in
                let vol = if gap < 200.0 { 0.22 } else { 0.28 } + if bar { 0.04 } else { 0.0 } + (self.random() - 0.5) * 0.02;
                let note = self.piano(self.songs[s].notes[i], vol, if gap < 200.0 { 0.7 } else { 1.05 }, 0.0);
                self.put(note);
                if bar || i == 0 {
                    self.bass_for(i, now);
                }
            }
            // a new shelf starts a new song: its opening chord rolls in under the first notes
            "shelf" => {
                let quick = now - self.last_shelf < 600.0;
                self.last_shelf = now;
                let s = self.cur_song(now);
                let first = self.songs[s].notes[0];
                if quick {
                    let note = self.piano(first + 12, 0.3, 0.5, 0.0);
                    self.put(note);
                    return true;
                }
                self.last_note = 0.0;
                self.song_idx = 0;
                let (root, minor) = self.songs[s].chords[0];
                let r = 48 + root;
                for (k, m) in [r, r + if minor { 3 } else { 4 }, r + 7, r + 12].into_iter().enumerate() {
                    let note = self.piano(m, 0.22, 1.4, k as f32 * 0.045).bright(2600.0);
                    self.put(note);
                }
                let note = self.piano(first, 0.32, 1.2, 0.2);
                self.put(note);
            }
            "open" => {
                let [a, b, c] = self.triad(1, now);
                for (m, at, vol, dur) in [(a, 0.0, 0.18, 0.8), (b, 0.05, 0.17, 0.8), (c, 0.1, 0.2, 1.0)] {
                    let note = self.piano(m, vol, dur, at);
                    self.put(note);
                }
            }
            "back" => {
                let [a, _, c] = self.triad(1, now);
                let note = self.piano(c, 0.2, 0.6, 0.0);
                self.put(note);
                let note = self.piano(a, 0.18, 0.9, 0.07).bright(2400.0);
                self.put(note);
            }
            "go" => {
                let [a, b, c] = self.triad(0, now);
                for (k, m) in [a, b, c, a + 12, b + 12, c + 12, a + 24].into_iter().enumerate() {
                    let note = self.piano(m, 0.2 - k as f32 * 0.01, 1.6, k as f32 * 0.055).verb(0.65);
                    self.put(note);
                }
                let note = self.piano(a - 12, 0.16, 2.2, 0.02).bright(1200.0).verb(0.6);
                self.put(note);
                self.swell(&[0, 4], -1, 1.4, 0.1, 0.2, 0.4);
            }
            "flip" => {
                let [_, b, _] = self.triad(1, now);
                self.put(air(Filter::Bandpass, true).dur(0.22).vol(0.16).freq(2000.0).sweep_to(500.0).q(0.6).verb(0.4));
                let note = self.piano(b, 0.14, 0.6, 0.1);
                self.put(note);
            }
            "toast" => {
                let [_, b, c] = self.triad(1, now);
                let note = self.piano(c + 12, 0.15, 0.9, 0.0).verb(0.6);
                self.put(note);
                let note = self.piano(b + 12, 0.12, 0.9, 0.06).verb(0.6);
                self.put(note);
            }
            "error" => {
                let [a, _, _] = self.triad(0, now);
                let note = self.piano(a + 1, 0.42, 0.5, 0.0).bright(1800.0);
                self.put(note);
                let note = self.piano(a, 0.38, 0.8, 0.12).bright(1600.0);
                self.put(note);
            }
            "unlock" => {
                let [a, b, c] = self.triad(1, now);
                for (k, m) in [a, b, c, a + 12, c, a + 12, b + 12, c + 12].into_iter().enumerate() {
                    let note = self.piano(m, 0.18, 1.2, k as f32 * 0.07).verb(0.7);
                    self.put(note);
                }
                let note = self.piano(a + 24, 0.15, 2.0, 0.6).verb(0.8);
                self.put(note);
            }
            "museum" => {
                let [a, b, c] = self.triad(0, now);
                for (k, m) in [a, b, c, a + 12, b + 12, c + 12, a + 24].into_iter().enumerate() {
                    let note = self.piano(m, 0.15, 1.8, k as f32 * 0.09).verb(0.75);
                    self.put(note);
                }
            }
            "pedestal" => {
                let [a, b, c] = self.triad(1, now);
                for (k, m) in [a, b, c].into_iter().enumerate() {
                    let note = self.piano(m, 0.12, 1.2, k as f32 * 0.1).verb(0.7);
                    self.put(note);
                }
            }
            "hold" => {
                let s = self.cur_song(now);
                let note = self.piano(self.songs[s].notes[0] + 12, 0.1, 0.25, 0.0);
                self.put(note);
            }
            _ => return false,
        }
        true
    }

    // ---------- Music box ----------

    /// One tine, placed left to right by its step.
    fn tine(&self, n: i32, vol: f32, dur: f32, at: f32) -> Hit {
        hit(self.d(n)).dur(dur).pan((n as f32 * 0.05).clamp(-0.4, 0.4)).duck(0.1).vol(vol * 0.48).at(at)
    }

    fn music_box(&mut self, name: &str, dir: i32, now: f64) -> bool {
        match name {
            "tick" => {
                let n = self.walk(dir, now);
                let vol = 0.3 + (self.random() - 0.5) * 0.03;
                let pan = (self.random() - 0.5) * 0.25;
                self.put(hit(self.d(2 + n)).dur(0.55).pan(pan).duck(0.1).vol(vol * 0.48));
            }
            "shelf" => {
                if self.shelf_quick(dir, now) {
                    return true;
                }
                for (n, vol, dur, at) in [(0, 0.26, 0.9, 0.0), (4, 0.24, 1.0, 0.09), (7, 0.2, 1.2, 0.18)] {
                    self.put(self.tine(n, vol, dur, at));
                }
            }
            "open" => {
                self.put(self.tine(2, 0.28, 0.6, 0.0));
                self.put(self.tine(4, 0.26, 0.8, 0.07));
            }
            "back" => {
                self.put(self.tine(4, 0.26, 0.5, 0.0));
                self.put(self.tine(1, 0.24, 0.8, 0.07).bright(3800.0));
            }
            "go" => {
                for (i, n) in [0, 2, 4, 7].into_iter().enumerate() {
                    self.put(self.tine(n, 0.26, 1.0, i as f32 * 0.08));
                }
                self.put(self.tine(9, 0.26, 1.8, 0.34).verb(0.7));
                self.put(self.tine(0, 0.16, 2.0, 0.34).verb(0.7).freq(self.deg(0, -1)));
                self.swell(&[0, 4], -1, 1.4, 0.1, 0.3, 0.4);
            }
            "flip" => {
                self.put(air(Filter::Bandpass, true).dur(0.2).vol(0.14).freq(2400.0).sweep_to(700.0).q(0.6).verb(0.4));
                self.put(self.tine(-1, 0.2, 0.5, 0.1));
            }
            "toast" => {
                self.put(self.tine(7, 0.22, 0.8, 0.0).verb(0.65));
                self.put(self.tine(9, 0.18, 0.9, 0.07).verb(0.65));
            }
            "error" => {
                self.put(self.tine(-2, 0.26, 0.6, 0.0).bright(3000.0));
                self.put(self.tine(-3, 0.24, 0.8, 0.14).bright(2800.0));
            }
            "unlock" => {
                for (i, n) in [0, 2, 4, 7, 9, 7, 9, 11].into_iter().enumerate() {
                    self.put(self.tine(n, 0.22, 1.1, i as f32 * 0.075).verb(0.7));
                }
                self.put(self.tine(14, 0.2, 2.0, 0.62).verb(0.8));
            }
            "museum" => {
                for (i, n) in [0, 2, 4, 7, 9, 11, 9, 7].into_iter().enumerate() {
                    self.put(self.tine(n, 0.18, 1.2, i as f32 * 0.1).verb(0.7));
                }
                self.put(self.tine(4, 0.16, 2.0, 0.85).verb(0.8));
            }
            "hold" => self.put(self.tine(9, 0.14, 0.2, 0.0)),
            _ => return false,
        }
        true
    }

    // ---------- Disc-era, Handheld, Living room ----------

    /// Disc-era's glassy FM bell: a sine bent by a sine, bright at the strike.
    fn bell_note(&self, n: i32) -> Fm {
        fm(self.d(n)).dur(0.5).vol(0.2).ratio(3.5).index(1.4).floor(0.1).verb(0.6).attack(0.003)
    }

    fn disc_era(&mut self, name: &str, dir: i32, now: f64) -> bool {
        match name {
            "tick" => {
                let n = self.walk(dir, now);
                let pan = (self.random() - 0.5) * 0.25;
                self.put(self.bell_note(2 + n).dur(0.2).vol(0.22).index(1.0).pan(pan).verb(0.45));
            }
            "shelf" => {
                if self.shelf_quick(dir, now) {
                    return true;
                }
                self.put(air(Filter::Bandpass, true).dur(0.45).vol(0.12).freq(700.0).sweep_to(5200.0).q(0.5).verb(0.8).attack(0.1));
                self.put(self.bell_note(0).at(0.03).dur(0.6).vol(0.16).pan(-0.25));
                self.put(self.bell_note(4).at(0.13).dur(0.8).vol(0.14).pan(0.25));
            }
            "open" => {
                self.put(self.bell_note(2).dur(0.4).vol(0.2));
                self.put(self.bell_note(4).at(0.06).dur(0.7).vol(0.17).pan(0.2).detune(4.0));
            }
            "back" => {
                self.put(self.bell_note(4).dur(0.3).vol(0.18));
                self.put(self.bell_note(1).at(0.07).dur(0.55).vol(0.16).pan(-0.2).index(0.8));
            }
            "go" => {
                for (i, n) in [0, 2, 4, 7].into_iter().enumerate() {
                    self.put(self.bell_note(n).at(i as f32 * 0.08).dur(1.1).vol(0.17).pan((i as f32 - 1.5) * 0.2).detune(3.0));
                }
                let freqs = vec![self.deg(0, -2), self.deg(2, -2), self.deg(4, -2)];
                self.put(Pad { freqs, ..Pad::default() }.dur(2.0).vol(0.22).from(250.0).to(2600.0).open(0.9).attack(0.5).hold(1.0));
            }
            "flip" => {
                self.put(air(Filter::Bandpass, true).dur(0.26).vol(0.2).freq(2800.0).sweep_to(600.0).q(0.6).verb(0.5));
                self.put(self.bell_note(-2).at(0.1).dur(0.3).vol(0.12));
            }
            "error" => {
                self.put(fm(self.d(-6)).dur(0.35).vol(0.24).ratio(1.41).index(2.5).floor(0.3).verb(0.35));
                self.put(fm(self.d(-7)).at(0.15).dur(0.45).vol(0.2).ratio(1.41).index(2.2).floor(0.3).verb(0.35));
            }
            "toast" => {
                self.put(self.bell_note(7).dur(0.6).vol(0.15).pan(0.4).verb(0.7));
                self.put(self.bell_note(9).at(0.07).dur(0.8).vol(0.12).pan(0.4).verb(0.75));
            }
            "unlock" => {
                for (i, n) in [0, 2, 4, 7, 9].into_iter().enumerate() {
                    self.put(self.bell_note(n).at(i as f32 * 0.07).dur(0.9).vol(0.15).pan((i as f32 - 2.0) * 0.18).verb(0.7));
                }
                self.put(self.bell_note(12).at(0.4).dur(1.6).vol(0.13).verb(0.85).detune(4.0));
            }
            "hold" => self.put(self.bell_note(9).dur(0.12).vol(0.08).verb(0.3)),
            _ => return false,
        }
        true
    }

    /// Handheld's blip: a pulse wave, short, with next to no room.
    fn chirp(&self, n: i32) -> Hit {
        const ONE: &[Partial] = &[(1.0, 1.0, 1.0)];
        Hit { partials: Some(ONE), wave: Some(Wave::Square), ..hit(self.d(n)) }.dur(0.06).vol(0.2).bright(3800.0).verb(0.05).thump(0.94).noise(0.0).breath(0.0).attack(0.002).duck(0.1)
    }

    fn handheld(&mut self, name: &str, dir: i32, now: f64) -> bool {
        match name {
            "tick" => {
                let n = self.walk(dir, now);
                let pan = (self.random() - 0.5) * 0.2;
                self.put(self.chirp(2 + n).dur(0.035).vol(0.3).pan(pan));
            }
            "shelf" => {
                if self.shelf_quick(dir, now) {
                    return true;
                }
                self.put(self.chirp(0).dur(0.05));
                self.put(self.chirp(2).at(0.05).dur(0.05));
                self.put(self.chirp(4).at(0.1).dur(0.08));
            }
            "open" => {
                self.put(self.chirp(2).dur(0.05));
                self.put(self.chirp(5).at(0.055).dur(0.09));
            }
            "back" => {
                self.put(self.chirp(5).dur(0.05));
                self.put(self.chirp(2).at(0.055).dur(0.09).bright(3000.0));
            }
            "go" => {
                for (i, n) in [0, 2, 4, 5, 7].into_iter().enumerate() {
                    self.put(self.chirp(n).at(i as f32 * 0.055).dur(0.07).vol(0.18));
                }
                self.put(self.chirp(10).at(0.3).dur(0.3).vol(0.17));
            }
            "flip" => self.put(self.chirp(3).dur(0.04).glide(self.d(0)).glide_time(0.04)),
            "error" => {
                self.put(self.chirp(-5).dur(0.12).vol(0.22).bright(1800.0));
                self.put(self.chirp(-6).at(0.13).dur(0.16).vol(0.22).bright(1600.0));
            }
            "toast" => {
                self.put(self.chirp(7).dur(0.05));
                self.put(self.chirp(9).at(0.06).dur(0.1));
            }
            "unlock" => {
                for (i, n) in [0, 2, 4, 7, 9, 12].into_iter().enumerate() {
                    self.put(self.chirp(n).at(i as f32 * 0.05).dur(0.06).vol(0.16));
                }
                self.put(self.chirp(14).at(0.32).dur(0.35).vol(0.15));
            }
            "hold" => self.put(self.chirp(9).dur(0.025).vol(0.1)),
            _ => return false,
        }
        true
    }

    /// Living room's warm round note.
    fn round(&self, n: i32) -> Hit {
        hit(self.d(n)).dur(0.3).vol(0.3).attack(0.008)
    }

    fn living_room(&mut self, name: &str, dir: i32, now: f64) -> bool {
        match name {
            "tick" => {
                let n = self.walk(dir, now);
                let pan = (self.random() - 0.5) * 0.2;
                self.put(self.round(2 + n).dur(0.11).vol(0.26).pan(pan).verb(0.3).duck(0.12));
            }
            "shelf" => {
                if self.shelf_quick(dir, now) {
                    return true;
                }
                self.put(air(Filter::Bandpass, true).dur(0.3).vol(0.1).freq(500.0).sweep_to(1800.0).q(0.6).verb(0.5));
                self.put(self.round(0).at(0.03).dur(0.4).vol(0.24).pan(-0.2).under(0.3));
                self.put(self.round(4).at(0.12).dur(0.5).vol(0.22).pan(0.2));
            }
            "open" => {
                self.put(self.round(2).dur(0.28).vol(0.28));
                self.put(self.round(4).at(0.07).dur(0.45).vol(0.24).pan(0.15));
            }
            "back" => {
                self.put(self.round(3).dur(0.22).vol(0.26));
                self.put(self.round(0).at(0.07).dur(0.4).vol(0.22).pan(-0.15).bright(1800.0));
            }
            "go" => {
                for (i, n) in [0, 2, 4].into_iter().enumerate() {
                    let i = i as f32;
                    self.put(self.round(n).at(i * 0.1).dur(0.7 + i * 0.15).vol(0.26).pan((i - 1.0) * 0.2).under(0.3));
                }
                self.swell(&[0, 2, 4], -1, 1.4, 0.16, 0.15, 0.35);
            }
            "flip" => {
                self.put(air(Filter::Bandpass, true).dur(0.2).vol(0.14).freq(1400.0).sweep_to(450.0).q(0.7).verb(0.3));
                self.put(self.round(-2).at(0.1).dur(0.16).vol(0.2));
            }
            "error" => {
                self.put(self.round(-6).dur(0.3).vol(0.3).bright(1200.0).under(0.4));
                self.put(self.round(-7).at(0.14).dur(0.4).vol(0.26).bright(1000.0));
            }
            "toast" => self.put(self.round(5).dur(0.35).vol(0.22).verb(0.55).pan(0.35)),
            "unlock" => {
                for (i, n) in [0, 2, 4, 7].into_iter().enumerate() {
                    self.put(self.round(n).at(i as f32 * 0.09).dur(0.6).vol(0.22).verb(0.6).pan((i as f32 - 1.5) * 0.2));
                }
                self.put(self.round(9).at(0.4).dur(1.2).vol(0.18).verb(0.7));
            }
            "hold" => self.put(self.round(7).dur(0.05).vol(0.14)),
            _ => return false,
        }
        true
    }

    /// A cue's voices at `now` (ms, any clock that only goes forward): the scrolling cues change with how fast they
    /// come. `dir`: which way a move went (-1 back, 1 on), for "tick" and "shelf". No voices for a name that's no
    /// cue, or a note the Melody voice lets pass.
    pub fn cue(&mut self, name: &str, dir: i32, now: f64) -> Cue {
        self.out.clear();
        let own = match self.theme.id {
            "melody" => self.melody(name, now) || self.aero(name, dir, now),
            "musicbox" => self.music_box(name, dir, now),
            "disc" => self.disc_era(name, dir, now) || self.aero(name, dir, now),
            "handheld" => self.handheld(name, dir, now),
            "living" => self.living_room(name, dir, now) || self.aero(name, dir, now),
            _ => self.aero(name, dir, now),
        };
        if !own {
            self.shared(name, dir, now);
        }
        // each struck note asks the music back; the last one's word stands
        let duck = self.out.iter().rev().find_map(|v| match v {
            Voice::Hit(h) => Some((h.duck.unwrap_or(0.35), h.dur.filter(|d| *d != 0.0).unwrap_or(self.theme.tone.decay) * 1000.0 + 150.0)),
            _ => None,
        });
        Cue { voices: std::mem::take(&mut self.out), level: if name == "boot" { self.boot_level } else { 1.0 }, duck }
    }
}
