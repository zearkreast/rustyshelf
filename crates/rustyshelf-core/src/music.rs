// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from the
// menu music of Boxshelf's renderer/sound.js, as RustyShelf has it.
//! The menu music: subtle home-screen music under the shelf. No beat, no bass line, no tune to hum: long chords that
//! swell in and glide into the next (a warm pad), a breath of filtered air under them, a low drone on the darker
//! ones, and now and then one soft note far off in a big room. Made here like every other sound. It sits in the
//! interface voice's key plus the shelf's, changing key only where its chord loop starts over, so the cues always
//! agree with it. The notes are chosen as it goes (seeded), so it never loops exactly.

/// A chord's notes above its root, in semitones.
fn chord(kind: &str) -> &'static [i32] {
    match kind {
        "maj9" => &[0, 4, 7, 11, 14],
        "m9" => &[0, 3, 7, 10, 14],
        "sus" => &[0, 5, 7, 10, 14],
        "m7" => &[0, 3, 7, 10],
        "maj7" => &[0, 4, 7, 11],
        _ => &[0, 4, 7, 14],
    }
}

/// The voice of the far-off notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Far {
    Glass,
    Ep,
}

/// One soft far-off note now and then: the chance of one on each eighth, its voice, its octave, the shortest gap
/// between two (seconds) and its loudness.
pub struct Notes {
    pub chance: f64,
    pub voice: Far,
    pub octave: i32,
    pub gap: f64,
    pub vel: f64,
}

pub struct Track {
    pub id: &'static str,
    pub name: &'static str,
    /// the slow pulse notes fall on, in eighths
    pub bpm: f64,
    /// bars a chord lasts
    pub bars: usize,
    /// each chord: its root above the tonic, and its kind
    pub prog: &'static [(i32, &'static str)],
    pub pad: f64,
    /// seconds a chord takes to slide into the next
    pub glide: f64,
    pub air: f64,
    pub air_hz: f64,
    pub drone: f64,
    pub notes: Option<Notes>,
    /// a slow music-box lullaby: the rhythms its two-bar phrases take, and the plan of phrases ("A" a phrase, "A2"
    /// its repeat with a new ending, "-" a rest)
    pub rhythms: &'static [usize],
    pub plan: &'static [&'static str],
}

pub static TRACKS: [Track; 3] = [
    Track {
        id: "drift",
        name: "Drift",
        bpm: 60.0,
        bars: 3,
        prog: &[(0, "maj9"), (5, "maj9"), (9, "m9"), (7, "sus")],
        pad: 1.0,
        glide: 1.6,
        air: 1.0,
        air_hz: 900.0,
        drone: 0.0,
        notes: Some(Notes { chance: 0.09, voice: Far::Glass, octave: 2, gap: 4.5, vel: 0.055 }),
        rhythms: &[],
        plan: &[],
    },
    Track {
        id: "night",
        name: "Night",
        bpm: 52.0,
        bars: 4,
        prog: &[(9, "m9"), (5, "maj7"), (0, "add9"), (7, "sus")],
        pad: 0.9,
        glide: 2.2,
        air: 0.7,
        air_hz: 520.0,
        drone: 0.65,
        notes: Some(Notes { chance: 0.07, voice: Far::Ep, octave: 1, gap: 6.0, vel: 0.05 }),
        rhythms: &[],
        plan: &[],
    },
    Track {
        id: "glow",
        name: "Glow",
        bpm: 56.0,
        bars: 2,
        prog: &[(0, "add9"), (4, "m7"), (5, "maj7"), (0, "add9")],
        pad: 0.65,
        glide: 1.2,
        air: 0.4,
        air_hz: 1200.0,
        drone: 0.6,
        notes: None,
        rhythms: &[6, 4, 7],
        plan: &["A", "-", "A2", "-"],
    },
];

/// A track by the name the settings have (`ui.music`), the names before these included; None for one that's none
/// ("off", "own").
pub fn track(name: &str) -> Option<&'static Track> {
    let id = match name {
        "calm" => "drift",
        "lounge" => "night",
        "chime" => "glow",
        other => other,
    };
    TRACKS.iter().find(|t| t.id == id)
}

/// A two-bar phrase's notes: where each starts and how long it is, in eighths (16 to a phrase).
const RHYTHMS: [&[(usize, usize)]; 8] = [
    &[(0, 3), (3, 1), (4, 4), (10, 2), (12, 4)],
    &[(2, 2), (4, 2), (6, 6), (14, 2)],
    &[(0, 2), (2, 2), (4, 2), (6, 2), (8, 8)],
    &[(1, 2), (3, 3), (8, 2), (10, 6)],
    &[(0, 6), (6, 2), (8, 8)],
    &[(3, 1), (4, 6), (12, 4)],
    &[(0, 4), (4, 4), (8, 8)],
    &[(0, 8), (8, 6)],
];
const PENTA: [i32; 5] = [0, 2, 4, 7, 9];
const STEPS: [i32; 7] = [-2, -1, -1, 0, 1, 1, 2];

/// The music's own run of chances: the same seed plays the same music (the Electron app's `rngFrom`, bit for bit).
pub struct Chance(u32);

impl Chance {
    /// 0 up to 1.
    pub fn roll(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6D2B_79F5);
        let s = self.0;
        let mut t = (s ^ (s >> 15)).wrapping_mul(1 | s);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }
}

/// What a step of the music asks to be played. Times are seconds from the music's start, notes are MIDI notes.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// the pad's four voices move to these notes: at once at the very start, gliding after
    Pad { notes: [i32; 4], at: f64, glide: bool },
    /// the pad swells towards this level
    PadLevel { level: f64, at: f64 },
    Drone { note: i32, at: f64, dur: f64, vel: f64 },
    Glass { note: i32, at: f64, vel: f64, pan: f64 },
    Ep { note: i32, at: f64, dur: f64, vel: f64, pan: f64 },
    Bell { note: i32, at: f64, vel: f64, pan: f64 },
}

/// Where each note of a two-bar phrase starts and how long it is, in eighths.
type Rhythm = &'static [(usize, usize)];

struct Phrase {
    rhythm: Rhythm,
    keep: Vec<i32>,
    notes: Vec<i32>,
}

/// The music's score, written an eighth at a time.
pub struct Score {
    pub track: &'static Track,
    chance: Chance,
    /// when the first eighth falls
    start: f64,
    step: usize,
    eighth: f64,
    tonic: i32,
    /// the tonic the next chord loop starts in
    next_tonic: i32,
    motif: Option<(Rhythm, Vec<i32>)>,
    phrase: Option<Phrase>,
    last_degree: Option<i32>,
    last_note: f64,
}

/// The tonic for a voice's root pitch and a shelf's key: the root as a pitch class, plus the key, folded into C3
/// to B3.
pub fn tonic(root: f32, key: i32) -> i32 {
    let pc = (12.0 * (f64::from(root) / 440.0).log2()).round() as i32 + 69;
    48 + (pc + key).rem_euclid(12)
}

impl Score {
    /// `start`: when the first eighth falls (seconds); `tonic`: see `tonic`.
    pub fn new(track: &'static Track, seed: u32, start: f64, tonic: i32) -> Score {
        Score { track, chance: Chance(seed), start, step: 0, eighth: 30.0 / track.bpm, tonic, next_tonic: tonic, motif: None, phrase: None, last_degree: None, last_note: -1e9 }
    }

    /// The key the music moves to where its chord loop next starts over.
    pub fn set_tonic(&mut self, tonic: i32) {
        self.next_tonic = tonic;
    }

    /// When the next eighth falls.
    pub fn next_at(&self) -> f64 {
        self.start + self.step as f64 * self.eighth
    }

    /// A pentatonic step (0 the tonic) as a note in the lullaby's octave.
    fn degree_note(&self, degree: i32) -> i32 {
        self.tonic + 24 + PENTA[degree.rem_euclid(5) as usize] + 12 * degree.div_euclid(5)
    }

    /// The nearest step that's in the chord.
    fn snap(&self, degree: i32, chord: &[i32]) -> i32 {
        let in_chord = |d: i32| chord.iter().any(|n| n.rem_euclid(12) == self.degree_note(d).rem_euclid(12));
        [0, -1, 1, -2, 2].into_iter().map(|k| degree + k).find(|d| in_chord(*d)).unwrap_or(degree)
    }

    /// Writes one eighth of the music.
    pub fn step(&mut self) -> Vec<Event> {
        let d = self.track;
        let s = self.step;
        self.step += 1;
        let (pos, bar, e) = (s % 8, s / 8, self.eighth);
        let t = self.start + s as f64 * e;
        let mut out = Vec::new();
        if s.is_multiple_of(8 * d.bars * d.prog.len()) {
            self.tonic = self.next_tonic;
        }
        let (root, kind) = d.prog[(bar / d.bars) % d.prog.len()];
        let shape = chord(kind);
        // the chord's notes around middle C
        let mut base = self.tonic + 12 + root;
        if base > 62 {
            base -= 12;
        }
        let keys: Vec<i32> = shape.iter().map(|i| base + i).collect();
        if pos == 0 && bar % d.bars == 0 {
            let mut low = self.tonic + root;
            if low > 57 {
                low -= 12;
            }
            out.push(Event::Pad { notes: [low + shape[0], low + shape[1], low + shape[2], low + shape[3]], at: t, glide: s > 0 });
            // each chord swells in at its own strength
            out.push(Event::PadLevel { level: 0.02 * d.pad * (0.7 + 0.3 * self.chance.roll()), at: t });
            if d.drone > 0.0 {
                let mut note = self.tonic - 12 + root;
                if note > 47 {
                    note -= 12;
                }
                out.push(Event::Drone { note, at: t, dur: e * 8.0 * d.bars as f64 + 1.5, vel: 0.05 * d.drone });
            }
        }
        if let Some(n) = &d.notes {
            // one soft note far off: a note of the chord, up high; now and then a second, a moment later
            if t - self.last_note >= n.gap && self.chance.roll() < n.chance {
                self.last_note = t;
                let pan = (self.chance.roll() - 0.5) * 0.9;
                let vel = n.vel * (0.75 + self.chance.roll() * 0.5);
                let mut far = |score: &mut Score, later: bool| {
                    let note = keys[(score.chance.roll() * keys.len() as f64) as usize] + 12 * (n.octave - 1);
                    let (at, vel) = if later { (t + e * (1.0 + (score.chance.roll() * 3.0).floor()), vel * 0.7) } else { (t, vel) };
                    out.push(match n.voice {
                        Far::Glass => Event::Glass { note, at, vel, pan },
                        Far::Ep => Event::Ep { note, at, dur: e * 6.0, vel, pan },
                    });
                };
                far(self, false);
                if self.chance.roll() < 0.3 {
                    far(self, true);
                }
            }
        }
        if !d.plan.is_empty() {
            self.lead(s, t, &keys, &mut out);
        }
        out
    }

    /// The lullaby: two-bar phrases of long notes, walking the pentatonic scale and landing on the chord.
    fn lead(&mut self, s: usize, t: f64, chord: &[i32], out: &mut Vec<Event>) {
        let d = self.track;
        let at = s % 16;
        if at == 0 {
            let which = (s / 16) % d.plan.len();
            if which == 0 {
                self.motif = None;
            }
            self.phrase = match (d.plan[which], &self.motif) {
                ("-", _) => None,
                ("A2", Some((rhythm, notes))) => Some(Phrase { rhythm, keep: notes[..notes.len().div_ceil(2)].to_vec(), notes: Vec::new() }),
                _ => Some(Phrase { rhythm: RHYTHMS[d.rhythms[(self.chance.roll() * d.rhythms.len() as f64) as usize]], keep: Vec::new(), notes: Vec::new() }),
            };
        }
        let Some(ph) = &self.phrase else { return };
        let Some(k) = ph.rhythm.iter().position(|x| x.0 == at) else { return };
        let len = ph.rhythm[k].1;
        let degree = if k < ph.keep.len() {
            ph.keep[k]
        } else {
            let from = ph.notes.last().copied().or(self.last_degree).unwrap_or(2);
            let walked = (from + STEPS[(self.chance.roll() * STEPS.len() as f64) as usize]).clamp(-1, 7);
            if len >= 4 || at == 0 || k == ph.rhythm.len() - 1 {
                self.snap(walked, chord)
            } else {
                walked
            }
        };
        let ph = self.phrase.as_mut().expect("a phrase");
        ph.notes.push(degree);
        // the first phrase of the plan is the one its repeat takes up
        if d.plan[(s / 16) % d.plan.len()] == "A" {
            self.motif = Some((ph.rhythm, ph.notes.clone()));
        }
        self.last_degree = Some(degree);
        let vel = 0.032 * (0.85 + self.chance.roll() * 0.3);
        let pan = (self.chance.roll() - 0.5) * 0.4;
        out.push(Event::Bell { note: self.degree_note(degree), at: t, vel, pan });
    }
}

// ---------- the music's sound ----------

use std::f32::consts::PI;

use crate::synth::{makeup, saturate, Biquad, Filter, Rng, RoomStream, Stereo};

/// How much of the level setting (0 to 1) reaches the music's bus, and how much of the music goes to the room.
const GAIN: f32 = 0.4;
const ROOM: f32 = 0.4;

fn pitch(note: i32) -> f32 {
    440.0 * 2f32.powf((note - 69) as f32 / 12.0)
}

fn cents(c: f32) -> f32 {
    2f32.powf(c / 1200.0)
}

/// A note that's sounding: partials (a multiple of its pitch, a level, a detune in cents), its level over time, and
/// where it sits left to right (None: in the middle, as it is).
struct Tone {
    start: f64,
    end: f64,
    freq: f32,
    partials: &'static [(f32, f32, f32)],
    phases: [f32; 3],
    kind: Kind,
    vel: f32,
    dur: f32,
    pan: Option<f32>,
    /// the electric piano's bending sine
    bend: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Drone,
    Glass,
    Ep,
    Bell,
}

impl Tone {
    /// Its level `t` seconds after it starts.
    fn level(&self, t: f32) -> f32 {
        let v = self.vel;
        match self.kind {
            // breathing in over a few seconds, and out at the next chord
            Kind::Drone => {
                if t < self.dur {
                    v * (t / 2.5).min(1.0)
                } else {
                    v * (-(t - self.dur) / 1.2).exp()
                }
            }
            // a soft attack and a long ring
            Kind::Glass => {
                if t < 0.05 {
                    v * t / 0.05
                } else if t < 0.06 {
                    v
                } else {
                    v * (-(t - 0.06) / 1.6).exp()
                }
            }
            // down to half in a moment, then a slow fall once it's let go
            Kind::Ep => {
                let held = |t: f32| v * 0.5 + v * 0.5 * (-(t - 0.02) / 0.4).exp();
                if t < 0.02 {
                    v * t / 0.02
                } else if t < self.dur {
                    held(t)
                } else {
                    held(self.dur) * (-(t - self.dur) / 0.8).exp()
                }
            }
            // plucked and left to ring
            Kind::Bell => {
                if t < 0.004 {
                    v * t / 0.004
                } else if t < 0.01 {
                    v
                } else {
                    v * (-(t - 0.01) / 0.55).exp()
                }
            }
        }
    }
}

/// The music as sound: the score's notes played as they fall due, a block at a time. What comes out is the music's
/// own bus, before its level and the room.
pub struct Player {
    rate: f32,
    score: Score,
    frame: u64,
    /// the pad: four voices of two triangles a little apart, gliding to each new chord
    pad_freq: [f32; 4],
    pad_to: [f32; 4],
    pad_phase: [[f32; 2]; 4],
    pad_filter: Biquad,
    pad_level: f32,
    pad_level_to: f32,
    /// the air: a few seconds of softened noise, looped, through a filter that slowly breathes
    air: [Vec<f32>; 2],
    air_at: usize,
    air_filter: [Biquad; 2],
    tones: Vec<Tone>,
    /// events written, waiting for their time
    waiting: Vec<Event>,
    /// the music comes in over its first seconds
    fade_in: bool,
}

impl Player {
    /// `fade_in`: the music comes in over three seconds, as it does when it starts under the shelf.
    pub fn new(score: Score, rate: u32, seed: u64, fade_in: bool) -> Player {
        let rate = rate as f32;
        let mut rng = Rng(seed | 1);
        let n = (rate * 4.0) as usize;
        let mut air = [vec![0.0f32; n], vec![0.0f32; n]];
        if score.track.air > 0.0 {
            for ch in air.iter_mut() {
                let mut smooth = 0.0f32;
                for s in ch.iter_mut() {
                    smooth += (rng.noise() - smooth) * 0.02;
                    *s = smooth * 6.0;
                }
            }
        }
        let mut pad_filter = Biquad::default();
        pad_filter.set(Filter::Lowpass, 850.0, 0.3, rate);
        Player {
            rate,
            score,
            frame: 0,
            pad_freq: [220.0; 4],
            pad_to: [220.0; 4],
            pad_phase: [[0.0; 2]; 4],
            pad_filter,
            pad_level: 0.0,
            pad_level_to: 0.0,
            air,
            air_at: 0,
            air_filter: Default::default(),
            tones: Vec::new(),
            waiting: Vec::new(),
            fade_in,
        }
    }

    pub fn score(&mut self) -> &mut Score {
        &mut self.score
    }

    fn start(&mut self, e: Event) {
        const DRONE: &[(f32, f32, f32)] = &[(1.0, 1.0, 0.0), (2.0, 0.25, 4.0), (1.0, 0.5, -3.0)];
        const GLASS: &[(f32, f32, f32)] = &[(1.0, 1.0, 0.0), (2.001, 0.1, 0.0), (3.003, 0.03, 0.0)];
        const BELL: &[(f32, f32, f32)] = &[(1.0, 1.0, 0.0), (2.0, 0.16, 0.0), (5.4, 0.05, 0.0)];
        const EP: &[(f32, f32, f32)] = &[(1.0, 1.0, 0.0)];
        let tone = |start: f64, len: f64, note: i32, partials, kind, vel: f64, dur: f64, pan: Option<f64>| Tone {
            start,
            end: start + len,
            freq: pitch(note),
            partials,
            phases: [0.0; 3],
            kind,
            vel: vel as f32,
            dur: dur as f32,
            pan: pan.map(|p| p as f32),
            bend: 0.0,
        };
        match e {
            Event::Pad { notes, glide, .. } => {
                for (i, n) in notes.into_iter().enumerate() {
                    self.pad_to[i] = pitch(n);
                    if !glide {
                        self.pad_freq[i] = self.pad_to[i];
                    }
                }
            }
            Event::PadLevel { level, .. } => self.pad_level_to = level as f32,
            Event::Drone { note, at, dur, vel } => self.tones.push(tone(at, dur + 6.0, note, DRONE, Kind::Drone, vel, dur, None)),
            Event::Glass { note, at, vel, pan } => self.tones.push(tone(at, 7.0, note, GLASS, Kind::Glass, vel, 0.0, Some(pan))),
            Event::Ep { note, at, dur, vel, pan } => self.tones.push(tone(at, dur + 4.0, note, EP, Kind::Ep, vel, dur, Some(pan))),
            Event::Bell { note, at, vel, pan } => self.tones.push(tone(at, 2.4, note, BELL, Kind::Bell, vel, 0.0, Some(pan))),
        }
    }

    /// The next `n` frames of the music's bus.
    pub fn fill(&mut self, n: usize) -> [Vec<f32>; 2] {
        let rate = self.rate;
        let d = self.score.track;
        let mut out = [vec![0.0f32; n], vec![0.0f32; n]];
        let (glide, swell) = (1.0 - (-1.0 / (d.glide as f32 / 3.0 * rate)).exp(), 1.0 - (-1.0 / (2.2 * rate)).exp());
        let at_time = |e: &Event| match e {
            Event::Pad { at, .. } | Event::PadLevel { at, .. } | Event::Drone { at, .. } | Event::Glass { at, .. } | Event::Ep { at, .. } | Event::Bell { at, .. } => *at,
        };
        for i in 0..n {
            let now = self.frame as f64 / f64::from(rate);
            // the eighths that fall due are written, and what they ask for starts at its time
            while self.score.next_at() <= now {
                let written = self.score.step();
                self.waiting.extend(written);
            }
            if self.waiting.iter().any(|e| at_time(e) <= now) {
                let (due, later): (Vec<Event>, Vec<Event>) = std::mem::take(&mut self.waiting).into_iter().partition(|e| at_time(e) <= now);
                self.waiting = later;
                for e in due {
                    self.start(e);
                }
            }
            // the pad
            let mut pad = 0.0f32;
            for v in 0..4 {
                self.pad_freq[v] += (self.pad_to[v] - self.pad_freq[v]) * glide;
                for (k, apart) in [-6.0f32, 6.0].into_iter().enumerate() {
                    let p = &mut self.pad_phase[v][k];
                    pad += if *p < 0.25 { 4.0 * *p } else if *p < 0.75 { 2.0 - 4.0 * *p } else { 4.0 * *p - 4.0 };
                    *p = (*p + self.pad_freq[v] * cents(apart) / rate) % 1.0;
                }
            }
            self.pad_level += (self.pad_level_to - self.pad_level) * swell;
            let pad = self.pad_filter.run(pad) * self.pad_level;
            let (mut left, mut right) = (pad, pad);
            // the air
            if d.air > 0.0 {
                if self.frame.is_multiple_of(64) {
                    let hz = d.air_hz as f32 * (1.0 + 0.35 * (2.0 * PI * 0.05 * now as f32).sin());
                    for f in self.air_filter.iter_mut() {
                        f.set(Filter::Bandpass, hz, 0.4, rate);
                    }
                }
                let level = 0.05 * d.air as f32;
                left += self.air_filter[0].run(self.air[0][self.air_at]) * level;
                right += self.air_filter[1].run(self.air[1][self.air_at]) * level;
                self.air_at = (self.air_at + 1) % self.air[0].len();
            }
            // the notes
            for t in self.tones.iter_mut() {
                let age = (now - t.start) as f32;
                let level = t.level(age);
                let mut s = 0.0f32;
                if t.kind == Kind::Ep {
                    // a sine bent by a sine at the same pitch: the bark fades in a moment
                    let depth = t.freq * 0.08 + (t.freq * (0.6 + t.vel * 4.0) - t.freq * 0.08) * (-age / 0.25).exp();
                    s = t.phases[0].sin();
                    t.phases[0] = (t.phases[0] + 2.0 * PI * (t.freq + depth * t.bend.sin()) / rate) % (2.0 * PI);
                    t.bend = (t.bend + 2.0 * PI * t.freq / rate) % (2.0 * PI);
                } else {
                    for (k, (ratio, amp, apart)) in t.partials.iter().enumerate() {
                        s += t.phases[k].sin() * amp;
                        t.phases[k] = (t.phases[k] + 2.0 * PI * t.freq * ratio * cents(*apart) / rate) % (2.0 * PI);
                    }
                }
                let s = s * level;
                match t.pan {
                    Some(pan) => {
                        let x = (pan.clamp(-1.0, 1.0) + 1.0) / 2.0 * PI / 2.0;
                        left += s * x.cos();
                        right += s * x.sin();
                    }
                    None => {
                        left += s;
                        right += s;
                    }
                }
            }
            let come_in = if self.fade_in { (now as f32 / 3.0).min(1.0) } else { 1.0 };
            out[0][i] = left * come_in;
            out[1][i] = right * come_in;
            self.frame += 1;
            if self.frame.is_multiple_of(4096) {
                let now = self.frame as f64 / f64::from(rate);
                self.tones.retain(|t| t.end > now);
            }
        }
        out
    }
}

/// The music from its bus to the speakers: its level, some of it through the room, the lift the limiter gives quiet
/// sound, the interface's volume.
pub struct Chain {
    room: RoomStream,
    lift: f32,
}

impl Chain {
    pub fn new(rate: u32) -> Chain {
        Chain { room: RoomStream::new(rate as f32), lift: makeup() }
    }

    /// `level`: the music's level setting (0 to 1), for each frame of the block; `volume`: the interface's.
    pub fn run(&mut self, bus: &[Vec<f32>; 2], level: &[f32], volume: f32) -> [Vec<f32>; 2] {
        let amb: [Vec<f32>; 2] = [0, 1].map(|ch| bus[ch].iter().zip(level).map(|(s, l)| s * l * GAIN).collect());
        let wet: [Vec<f32>; 2] = [0, 1].map(|ch| amb[ch].iter().map(|s| s * ROOM).collect());
        let room = self.room.run(&wet);
        [0, 1].map(|ch| amb[ch].iter().zip(&room[ch]).map(|(dry, r)| (dry + saturate(r * 0.9)) * self.lift * volume).collect())
    }
}

/// Some seconds of a track from its start, as the Electron app's `renderMusic` gives them (for comparing the two).
pub fn render(track: &'static Track, seed: u32, tonic: i32, seconds: f32, level: f32, volume: f32, rate: u32) -> Stereo {
    let mut player = Player::new(Score::new(track, seed, 0.05, tonic), rate, u64::from(seed) + 17, false);
    let mut chain = Chain::new(rate);
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let total = (seconds * rate as f32) as usize;
    while left.len() < total {
        let n = (total - left.len()).min(16_384);
        let [l, r] = chain.run(&player.fill(n), &vec![level; n], volume);
        left.extend(l);
        right.extend(r);
    }
    Stereo { rate, left, right }
}
