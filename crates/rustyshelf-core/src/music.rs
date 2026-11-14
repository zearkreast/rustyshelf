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

    /// The clock ran on without the music (the app was held up): it carries on from the next bar, not with a pile
    /// of late notes.
    pub fn catch_up(&mut self, now: f64) {
        if self.next_at() < now - 0.1 {
            self.step = ((now + 0.15 - self.start) / (8.0 * self.eighth)).ceil() as usize * 8;
        }
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
