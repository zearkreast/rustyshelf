// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from the
// sound engine of Boxshelf's renderer/sound.js, as RustyShelf has it.
//! The interface's sound engine: every cue is synthesised, nothing is a recording. A cue is a handful of voices
//! (struck tones, shaped noise, FM bells, a pad) mixed onto a dry bus and a send to one shared room, then through a
//! gentle saturation and a limiter. The Electron app builds this from WebAudio nodes; here each node's arithmetic
//! is written out, and a cue is rendered whole into a stereo buffer.

use std::collections::HashMap;
use std::f32::consts::PI;
use std::sync::{Arc, LazyLock, Mutex};

/// The level every voice goes through, so the whole set can be nudged together.
pub const TRIM: f32 = 0.27;

/// A partial of a struck tone: its frequency as a multiple of the note's, its level, and how long it rings as a
/// part of the note's length.
pub type Partial = (f32, f32, f32);

struct Rng(u64);

impl Rng {
    /// 0 up to 1.
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn noise(&mut self) -> f32 {
        self.next() * 2.0 - 1.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    Lowpass,
    Highpass,
    #[default]
    Bandpass,
}

/// WebAudio's BiquadFilterNode: `q` is in dB for a lowpass and a highpass, plain for a bandpass.
#[derive(Default)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    x: [f32; 2],
    y: [f32; 2],
}

impl Biquad {
    fn set(&mut self, kind: Filter, freq: f32, q: f32, rate: f32) {
        let f = (freq / (rate / 2.0)).clamp(0.0, 1.0);
        let (b, a0, a1, a2);
        match kind {
            Filter::Lowpass | Filter::Highpass => {
                let low = kind == Filter::Lowpass;
                if f >= 1.0 || f <= 0.0 {
                    // at the edges one passes everything and the other nothing
                    let pass = (f >= 1.0) == low;
                    (self.b, self.a) = ([if pass { 1.0 } else { 0.0 }, 0.0, 0.0], [0.0, 0.0]);
                    return;
                }
                let (theta, res) = (PI * f, 10f32.powf(q / 20.0));
                let (alpha, cosw) = (theta.sin() / (2.0 * res), theta.cos());
                let beta = if low { (1.0 - cosw) / 2.0 } else { (1.0 + cosw) / 2.0 };
                b = [beta, if low { 2.0 * beta } else { -2.0 * beta }, beta];
                (a0, a1, a2) = (1.0 + alpha, -2.0 * cosw, 1.0 - alpha);
            }
            Filter::Bandpass => {
                if f >= 1.0 || f <= 0.0 || q <= 0.0 {
                    let pass = q <= 0.0 && f > 0.0 && f < 1.0;
                    (self.b, self.a) = ([if pass { 1.0 } else { 0.0 }, 0.0, 0.0], [0.0, 0.0]);
                    return;
                }
                let w0 = PI * f;
                let alpha = w0.sin() / (2.0 * q);
                b = [alpha, 0.0, -alpha];
                (a0, a1, a2) = (1.0 + alpha, -2.0 * w0.cos(), 1.0 - alpha);
            }
        }
        self.b = [b[0] / a0, b[1] / a0, b[2] / a0];
        self.a = [a1 / a0, a2 / a0];
    }

    fn run(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1] - self.a[0] * self.y[0] - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// A value sliding from `from` to `to` over `over` seconds as WebAudio's exponential ramp does, and staying there.
fn slide(from: f32, to: f32, t: f32, over: f32) -> f32 {
    if t >= over || over <= 0.0 {
        to
    } else {
        from * (to / from).powf(t / over)
    }
}

/// A level that rises in a straight line to `peak` over `attack`, stays there until `fall`, then falls away towards
/// nothing with the time constant `tau`.
fn swell_env(t: f32, attack: f32, fall: f32, tau: f32, peak: f32) -> f32 {
    if t < attack {
        peak * t / attack
    } else if t < fall {
        peak
    } else {
        peak * (-(t - fall) / tau).exp()
    }
}

/// A struck note.
#[derive(Debug, Clone, Default)]
pub struct Hit {
    pub freq: Option<f32>,
    pub dur: Option<f32>,
    pub vol: Option<f32>,
    pub at: Option<f32>,
    pub pan: Option<f32>,
    pub verb: Option<f32>,
    pub bright: Option<f32>,
    /// how far above the note its pitch starts, falling to it at once: the "thump" of something struck
    pub thump: Option<f32>,
    pub partials: Option<&'static [Partial]>,
    pub attack: Option<f32>,
    pub noise: Option<f32>,
    /// a pitch it slides to
    pub glide: Option<f32>,
    pub glide_time: Option<f32>,
    /// a soft octave below, at this level
    pub sub: Option<f32>,
    pub breath: Option<f32>,
}

/// Shaped noise: transients, whooshes, breath.
#[derive(Debug, Clone, Default)]
pub struct Air {
    pub at: Option<f32>,
    pub dur: Option<f32>,
    pub vol: Option<f32>,
    pub filter: Filter,
    pub freq: Option<f32>,
    pub sweep_to: Option<f32>,
    pub q: Option<f32>,
    pub verb: Option<f32>,
    pub pan: Option<f32>,
    pub attack: Option<f32>,
    pub hold: Option<f32>,
    /// softened noise, not white
    pub soft: bool,
}

/// A sine bent by another whose depth falls away: bright and glassy at the strike, pure as it rings.
#[derive(Debug, Clone, Default)]
pub struct Fm {
    pub freq: Option<f32>,
    pub at: Option<f32>,
    pub dur: Option<f32>,
    pub vol: Option<f32>,
    /// the bending sine's frequency as a multiple of the note's
    pub ratio: Option<f32>,
    pub index: Option<f32>,
    pub floor: Option<f32>,
    pub bright: Option<f32>,
    pub pan: Option<f32>,
    pub verb: Option<f32>,
    /// cents: adds a twin a little apart
    pub detune: Option<f32>,
    pub attack: Option<f32>,
}

/// Detuned saws through a lowpass that opens up.
#[derive(Debug, Clone, Default)]
pub struct Pad {
    pub freqs: Vec<f32>,
    pub at: Option<f32>,
    pub dur: Option<f32>,
    pub vol: Option<f32>,
    pub from: Option<f32>,
    pub to: Option<f32>,
    pub open: Option<f32>,
    pub attack: Option<f32>,
    pub hold: Option<f32>,
    pub q: Option<f32>,
    pub verb: Option<f32>,
}

/// A soft chord that fades in over a breath of air.
#[derive(Debug, Clone, Default)]
pub struct Swell {
    pub freqs: Vec<f32>,
    pub at: Option<f32>,
    pub dur: Option<f32>,
    pub vol: Option<f32>,
    pub attack: Option<f32>,
    pub verb: Option<f32>,
}

#[derive(Debug, Clone)]
pub enum Voice {
    Hit(Hit),
    Air(Air),
    Fm(Fm),
    Pad(Pad),
    Swell(Swell),
}

/// What a voice's own settings leave to the interface's voice (Aero, Melody, Music box).
#[derive(Debug, Clone, Copy)]
pub struct Tone {
    pub partials: &'static [Partial],
    pub decay: f32,
    pub bright: f32,
    pub noise: f32,
    pub thump: f32,
    pub verb: f32,
    pub attack: f32,
    pub breath: f32,
}

/// WebAudio's sawtooth against one that swings the whole way: its band-limited table is evened out by its peak,
/// which the ripple at the step raises.
const SAW: f32 = 0.846;

/// Unset, or zero where the Electron app's `||` takes the fallback for either.
fn or(v: Option<f32>, fallback: f32) -> f32 {
    v.filter(|x| *x != 0.0).unwrap_or(fallback)
}

struct Mix {
    rate: f32,
    dry: [Vec<f32>; 2],
    wet: [Vec<f32>; 2],
    rng: Rng,
}

impl Mix {
    fn reach(&mut self, n: usize) {
        for ch in self.dry.iter_mut().chain(self.wet.iter_mut()) {
            if ch.len() < n {
                ch.resize(n, 0.0);
            }
        }
    }

    /// One voice onto the buses, placed left to right as WebAudio's StereoPanner places a mono source.
    fn add(&mut self, at: f32, sig: &[f32], verb: f32, pan: f32) {
        let start = (at.max(0.0) * self.rate).round() as usize;
        self.reach(start + sig.len());
        let x = (pan.clamp(-1.0, 1.0) + 1.0) / 2.0 * PI / 2.0;
        for (ch, gain) in [x.cos(), x.sin()].into_iter().enumerate() {
            for (i, s) in sig.iter().enumerate() {
                self.dry[ch][start + i] += s * gain;
                self.wet[ch][start + i] += s * gain * verb;
            }
        }
    }

    /// A voice that's stereo already, in the middle.
    fn add_wide(&mut self, at: f32, sig: &[Vec<f32>; 2], verb: f32) {
        let start = (at.max(0.0) * self.rate).round() as usize;
        self.reach(start + sig[0].len());
        for ch in 0..2 {
            for (i, s) in sig[ch].iter().enumerate() {
                self.dry[ch][start + i] += s;
                self.wet[ch][start + i] += s * verb;
            }
        }
    }

    fn hit(&mut self, o: &Hit, t: &Tone) {
        let rate = self.rate;
        let (at, dur, freq) = (o.at.unwrap_or(0.0), or(o.dur, t.decay), or(o.freq, 440.0));
        let vol = o.vol.unwrap_or(0.5) * TRIM;
        let (verb, pan) = (o.verb.unwrap_or(t.verb), o.pan.unwrap_or(0.0));
        let (bright, thump) = (or(o.bright, t.bright), o.thump.unwrap_or(t.thump));
        let attack = or(o.attack, if t.attack != 0.0 { t.attack } else { 0.003 });
        let parts = o.partials.unwrap_or(t.partials);
        let sub = o.sub.unwrap_or(0.0);
        let longest = parts.iter().map(|p| dur * p.2 + 0.25).fold(if sub > 0.0 { dur + 0.3 } else { 0.0 }, f32::max);
        let n = (longest * rate).ceil() as usize;
        let mut tones = vec![0.0f32; n];
        for (ratio, amp, part) in parts {
            let f = freq * ratio;
            // above hearing on the highest notes
            if f * thump.max(1.0) > 18_000.0 {
                continue;
            }
            let d = dur * part;
            let (tau, mut phase) = (d / 4.5, 0.0f32);
            let glide = o.glide.map(|g| ((g * ratio).max(30.0), or(o.glide_time, dur * 0.7)));
            for (i, out) in tones.iter_mut().enumerate().take(((d + 0.25) * rate) as usize) {
                let time = i as f32 / rate;
                let mut hz = slide(f * thump, f, time, 0.018);
                if let Some((to, by)) = glide.filter(|_| time > 0.018) {
                    hz = slide(f, to, time - 0.018, by - 0.018);
                }
                *out += phase.sin() * swell_env(time, attack, attack, tau, vol * amp);
                phase = (phase + 2.0 * PI * hz / rate) % (2.0 * PI);
            }
        }
        let mut lp = Biquad::default();
        let floor = (bright * 0.35).max(400.0);
        for (i, s) in tones.iter_mut().enumerate() {
            if i % 16 == 0 {
                lp.set(Filter::Lowpass, slide(bright, floor, i as f32 / rate, dur), 0.6, rate);
            }
            *s = lp.run(*s);
        }
        if sub > 0.0 {
            let step = 2.0 * PI * (freq / 2.0) / rate;
            for (i, out) in tones.iter_mut().enumerate().take(((dur + 0.3) * rate) as usize) {
                *out += (step * i as f32).sin() * swell_env(i as f32 / rate, 0.006, 0.006, dur / 3.0, vol * sub);
            }
        }
        self.add(at, &tones, verb, pan);
        let breath = o.breath.unwrap_or(t.breath);
        if breath > 0.0 {
            self.air(&Air {
                at: o.at,
                dur: Some(dur * 1.3),
                vol: Some(vol * breath),
                freq: Some((freq * 2.2).min(12_000.0)),
                q: Some(0.5),
                verb: Some(verb * 0.8),
                pan: o.pan,
                attack: Some((attack * 1.5).max(0.01)),
                soft: true,
                ..Air::default()
            });
        }
        let noise = o.noise.unwrap_or(t.noise);
        if noise > 0.0 {
            self.air(&Air {
                at: o.at,
                dur: Some(dur.min(0.04)),
                vol: Some(vol * noise * 2.2),
                freq: Some((freq * 5.0).min(9000.0)),
                q: Some(0.9),
                verb: Some(0.0),
                pan: o.pan,
                attack: Some(0.001),
                ..Air::default()
            });
        }
    }

    fn air(&mut self, o: &Air) {
        let rate = self.rate;
        let dur = or(o.dur, 0.2);
        let (attack, hold) = (or(o.attack, 0.004), o.hold.unwrap_or(0.0));
        let (peak, from, q) = (o.vol.unwrap_or(0.1) * TRIM, or(o.freq, 1200.0), or(o.q, 0.8));
        let n = ((dur + 0.05) * rate).ceil() as usize;
        let (mut f, mut smooth) = (Biquad::default(), 0.0f32);
        let mut sig = vec![0.0f32; n];
        for (i, out) in sig.iter_mut().enumerate() {
            let time = i as f32 / rate;
            if i % 16 == 0 {
                f.set(o.filter, o.sweep_to.map_or(from, |to| slide(from, to, time, dur)), q, rate);
            }
            let white = self.rng.noise();
            smooth += (white - smooth) * 0.2;
            *out = f.run(if o.soft { smooth } else { white }) * swell_env(time, attack, attack + hold, dur / 3.0, peak);
        }
        self.add(o.at.unwrap_or(0.0), &sig, o.verb.unwrap_or(0.2), o.pan.unwrap_or(0.0));
    }

    fn fm(&mut self, o: &Fm) {
        let rate = self.rate;
        let (dur, f) = (or(o.dur, 1.2), or(o.freq, 440.0));
        let vol = o.vol.unwrap_or(0.3) * TRIM;
        let twins: Vec<f32> = match o.detune.filter(|d| *d != 0.0) {
            Some(d) => vec![-d, d],
            None => vec![0.0],
        };
        let n = ((dur + 0.3) * rate).ceil() as usize;
        let mut sig = vec![0.0f32; n];
        for cents in &twins {
            let ff = f * 2f32.powf(cents / 1200.0);
            // the bending sine stays in hearing, so high notes have no grit
            let mut ratio = or(o.ratio, 3.5);
            while ff * ratio > 12_000.0 && ratio > 1.0 {
                ratio -= 1.0;
            }
            let depth = o.index.unwrap_or(2.5) * ff;
            let rest = depth * o.floor.unwrap_or(0.08);
            let (tau, attack, level) = (or(o.bright, 0.18) * dur, or(o.attack, 0.002), vol / twins.len() as f32);
            let (mut phase, mut bend) = (0.0f32, 0.0f32);
            for (i, out) in sig.iter_mut().enumerate() {
                let time = i as f32 / rate;
                let amount = if time < 0.005 { depth } else { rest + (depth - rest) * (-(time - 0.005) / tau).exp() };
                *out += phase.sin() * swell_env(time, attack, attack, dur / 4.2, level);
                phase = (phase + 2.0 * PI * (ff + amount * bend.sin()) / rate) % (2.0 * PI);
                bend = (bend + 2.0 * PI * ff * ratio / rate) % (2.0 * PI);
            }
        }
        self.add(o.at.unwrap_or(0.0), &sig, o.verb.unwrap_or(0.4), o.pan.unwrap_or(0.0));
    }

    fn pad(&mut self, o: &Pad) {
        let rate = self.rate;
        let dur = or(o.dur, 2.0);
        let (attack, fall) = (or(o.attack, dur * 0.4), or(o.hold, dur * 0.55));
        let level = o.vol.unwrap_or(0.2) * TRIM / o.freqs.len().max(1) as f32 / 2.0;
        let n = ((dur + 1.0) * rate).ceil() as usize;
        let mut sig = [vec![0.0f32; n], vec![0.0f32; n]];
        for fr in &o.freqs {
            for cents in [-7.0f32, 7.0] {
                let step = fr * 2f32.powf(cents / 1200.0) / rate;
                let x = (if cents > 0.0 { 0.4f32 } else { -0.4 } + 1.0) / 2.0 * PI / 2.0;
                // from its middle, going up, as WebAudio's starts
                let (left, right, mut phase) = (x.cos(), x.sin(), 0.5f32);
                for i in 0..n {
                    // a sawtooth with its step rounded off, as a band-limited one is
                    let mut saw = 2.0 * phase - 1.0;
                    if phase < step {
                        let p = phase / step;
                        saw -= p + p - p * p - 1.0;
                    } else if phase > 1.0 - step {
                        let p = (phase - 1.0) / step;
                        saw -= p * p + p + p + 1.0;
                    }
                    let s = saw * SAW * swell_env(i as f32 / rate, attack, fall.max(attack), dur / 5.0, level);
                    sig[0][i] += s * left;
                    sig[1][i] += s * right;
                    phase = (phase + step) % 1.0;
                }
            }
        }
        let (from, to, open, q) = (or(o.from, 300.0), or(o.to, 3500.0), or(o.open, dur * 0.6), or(o.q, 1.2));
        for ch in sig.iter_mut() {
            let mut lp = Biquad::default();
            for (i, s) in ch.iter_mut().enumerate() {
                if i % 16 == 0 {
                    lp.set(Filter::Lowpass, slide(from, to, i as f32 / rate, open), q, rate);
                }
                *s = lp.run(*s);
            }
        }
        self.add_wide(o.at.unwrap_or(0.0), &sig, o.verb.unwrap_or(0.7));
    }

    fn swell(&mut self, o: &Swell) {
        let rate = self.rate;
        let dur = or(o.dur, 1.2);
        let level = o.vol.unwrap_or(0.18) * TRIM / o.freqs.len().max(1) as f32;
        let attack = or(o.attack, dur * 0.35);
        let n = ((dur + 0.6) * rate).ceil() as usize;
        let mut sig = [vec![0.0f32; n], vec![0.0f32; n]];
        for (k, fr) in o.freqs.iter().enumerate() {
            // the first in the middle, the rest to one side and the other
            let pan = if k == 0 { 0.0f32 } else if k % 2 == 1 { 0.35 } else { -0.35 };
            let x = (pan + 1.0) / 2.0 * PI / 2.0;
            let step = 2.0 * PI * fr / rate;
            for i in 0..n {
                let s = (step * i as f32).sin() * swell_env(i as f32 / rate, attack, (dur * 0.55).max(attack), dur / 5.0, level);
                sig[0][i] += s * x.cos();
                sig[1][i] += s * x.sin();
            }
        }
        self.add_wide(o.at.unwrap_or(0.0), &sig, o.verb.unwrap_or(0.6));
        self.air(&Air { at: o.at, dur: Some(dur), vol: Some(0.05), filter: Filter::Lowpass, freq: Some(500.0), sweep_to: Some(2600.0), q: Some(0.4), verb: Some(0.6), attack: Some(dur * 0.4), soft: true, ..Air::default() });
    }
}

// ---------- the room ----------

/// An in-place FFT of a power-of-two length; `inverse` leaves the result unscaled.
fn fft(re: &mut [f32], im: &mut [f32], inverse: bool) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = 2.0 * std::f64::consts::PI / len as f64 * if inverse { 1.0 } else { -1.0 };
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f64, 0.0f64);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (xr, xi) = (re[b] as f64 * cr - im[b] as f64 * ci, re[b] as f64 * ci + im[b] as f64 * cr);
                re[b] = (re[a] as f64 - xr) as f32;
                im[b] = (im[a] as f64 - xi) as f32;
                re[a] = (re[a] as f64 + xr) as f32;
                im[a] = (im[a] as f64 + xi) as f32;
                (cr, ci) = (cr * wr - ci * wi, cr * wi + ci * wr);
            }
        }
        len <<= 1;
    }
}

/// The room's tail: 2.2 s of softened noise falling away, a take for each ear, at the level WebAudio's convolver
/// plays an impulse at.
fn impulse(rate: f32) -> Arc<[Vec<f32>; 2]> {
    type Made = HashMap<u32, Arc<[Vec<f32>; 2]>>;
    static MADE: LazyLock<Mutex<Made>> = LazyLock::new(Default::default);
    let mut made = MADE.lock().unwrap();
    made.entry(rate as u32)
        .or_insert_with(|| {
            let n = (rate * 2.2).floor() as usize;
            let mut rng = Rng(0x5eed_0fa4_00d1);
            let mut ir = [vec![0.0f32; n], vec![0.0f32; n]];
            for ch in ir.iter_mut() {
                let mut smooth = 0.0f32;
                for (i, s) in ch.iter_mut().enumerate() {
                    smooth += (rng.noise() - smooth) * 0.35;
                    *s = smooth * (1.0 - i as f32 / n as f32).powf(3.6) * (1.0 - 0.4 * (-(i as f32) / (rate * 0.004)).exp());
                }
            }
            // the convolver evens impulses out by their power
            let power = (ir.iter().flatten().map(|s| (s * s) as f64).sum::<f64>() / (2 * n) as f64).sqrt().max(0.000_125);
            let scale = (1.0 / power * 0.001_25 * 44_100.0 / rate as f64) as f32;
            for s in ir.iter_mut().flatten() {
                *s *= scale;
            }
            Arc::new(ir)
        })
        .clone()
}

fn convolve(sig: &[f32], ir: &[f32]) -> Vec<f32> {
    let size = (sig.len() + ir.len()).next_power_of_two();
    let load = |x: &[f32]| {
        let (mut re, mut im) = (vec![0.0f32; size], vec![0.0f32; size]);
        re[..x.len()].copy_from_slice(x);
        fft(&mut re, &mut im, false);
        (re, im)
    };
    let ((mut ar, mut ai), (br, bi)) = (load(sig), load(ir));
    for i in 0..size {
        (ar[i], ai[i]) = (ar[i] * br[i] - ai[i] * bi[i], ar[i] * bi[i] + ai[i] * br[i]);
    }
    fft(&mut ar, &mut ai, true);
    ar.truncate(sig.len() + ir.len());
    ar.iter_mut().for_each(|s| *s /= size as f32);
    ar
}

/// The room every cue shares: a few early reflections placed left and right, and a soft diffuse tail.
fn room(wet: &[Vec<f32>; 2], rate: f32) -> [Vec<f32>; 2] {
    const TAPS: [(f32, f32, f32); 4] = [(0.011, 0.5, -0.6), (0.019, 0.38, 0.6), (0.029, 0.3, -0.25), (0.041, 0.22, 0.35)];
    let ir = impulse(rate);
    let n = wet[0].len() + ir[0].len();
    let mut out = [vec![0.0f32; n], vec![0.0f32; n]];
    for (delay, gain, pan) in TAPS {
        let d = (delay * rate).round() as usize;
        // WebAudio's panner on a stereo signal: the side it leans away from is folded into the other
        let x = if pan <= 0.0 { pan + 1.0 } else { pan } * PI / 2.0;
        let (gl, gr) = (x.cos(), x.sin());
        for i in 0..wet[0].len() {
            let (l, r) = (wet[0][i] * gain, wet[1][i] * gain);
            let (ol, or_) = if pan <= 0.0 { (l + r * gl, r * gr) } else { (l * gl, r + l * gr) };
            out[0][i + d] += ol;
            out[1][i + d] += or_;
        }
    }
    for ch in 0..2 {
        let mut hc = Biquad::default();
        hc.set(Filter::Lowpass, 3600.0, 1.0, rate);
        let soft: Vec<f32> = wet[ch].iter().map(|s| hc.run(*s)).collect();
        for (o, s) in out[ch].iter_mut().zip(convolve(&soft, &ir[ch])) {
            *o += s * 0.5;
        }
    }
    out
}

// ---------- the limiter (WebAudio's DynamicsCompressorNode: threshold -10 dB, knee 4 dB, ratio 12, 2 ms, 90 ms) ----------

const THRESHOLD_DB: f32 = -10.0;
const KNEE_DB: f32 = 4.0;
const RATIO: f32 = 12.0;
const ATTACK: f32 = 0.002;
const RELEASE: f32 = 0.09;

fn db(x: f32) -> f32 {
    20.0 * x.log10()
}

fn lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

struct Curve {
    k: f32,
    threshold: f32,
    knee_end: f32,
    knee_end_db: f32,
    knee_end_out_db: f32,
}

impl Curve {
    fn knee(&self, x: f32, k: f32) -> f32 {
        if x < self.threshold {
            x
        } else {
            self.threshold + (1.0 - (-k * (x - self.threshold)).exp()) / k
        }
    }

    fn new() -> Curve {
        let mut c = Curve { k: 5.0, threshold: lin(THRESHOLD_DB), knee_end_db: THRESHOLD_DB + KNEE_DB, knee_end: lin(THRESHOLD_DB + KNEE_DB), knee_end_out_db: 0.0 };
        // the knee's bend, found so that the curve leaves the knee at the ratio's slope
        let (mut low, mut high) = (0.1f32, 10_000.0f32);
        for _ in 0..15 {
            let (x, x2) = (c.knee_end, c.knee_end * 1.001);
            let slope = (db(c.knee(x2, c.k)) - db(c.knee(x, c.k))) / (db(x2) - db(x));
            if slope < 1.0 / RATIO {
                high = c.k;
            } else {
                low = c.k;
            }
            c.k = (low * high).sqrt();
        }
        c.knee_end_out_db = db(c.knee(c.knee_end, c.k));
        c
    }

    /// A level after the curve.
    fn shape(&self, x: f32) -> f32 {
        if x < self.knee_end {
            self.knee(x, self.k)
        } else {
            lin(self.knee_end_out_db + (db(x) - self.knee_end_db) / RATIO)
        }
    }
}

/// The limiter's release in frames against how far it has to recover: the weights of four points along it, for each
/// power of the way there.
const RELEASE_CURVE: [[f64; 4]; 5] = [
    [0.999_999_999_999_999_8, 1.843_221_968_432_392_3e-16, -1.937_339_435_167_642_3e-16, 8.824_516_011_816_245e-18],
    [-1.578_832_035_284_588_8, 2.330_583_703_207_428_6, -0.914_119_420_484_042_9, 0.162_367_752_561_203_2],
    [0.533_414_286_910_642_4, -1.272_736_789_213_631, 0.925_885_604_220_751_2, -0.186_563_101_917_762_26],
    [0.087_834_631_382_072_34, -0.169_416_296_792_562_2, 0.085_880_579_515_952_72, -0.004_298_914_105_462_83],
    [-0.042_416_883_008_123_074, 0.111_569_382_798_760_2, -0.097_646_763_252_658_72, 0.028_494_263_462_021_576],
];

/// Runs both channels through the limiter in place. It starts settled, as it is in an app that has been making
/// sound; its 6 ms of look-ahead is taken back off the front.
fn limit(sig: &mut [Vec<f32>; 2], rate: f32) {
    let curve = Curve::new();
    let makeup = (1.0 / curve.shape(1.0)).powf(0.6);
    let attack_frames = ATTACK.max(0.001) * rate;
    let release_frames = RELEASE * rate;
    let sat_release_frames = 0.0025 * rate;
    // the release is quicker the less there is to recover: a curve through four points
    let ys = [release_frames * 0.09, release_frames * 0.16, release_frames * 0.42, release_frames * 0.98].map(f64::from);
    let [ka, kb, kc, kd, ke] = RELEASE_CURVE.map(|row| row.iter().zip(ys).map(|(c, y)| c * y).sum::<f64>() as f32);
    let ahead = (0.006 * rate) as usize;
    let n = sig[0].len();
    let (mut detector, mut gain, mut max_attack) = (1.0f32, 1.0f32, -1.0f32);
    let mut gains = vec![1.0f32; n];
    let mut rate_now = 1.0f32;
    for i in 0..n {
        if i % 32 == 0 {
            let wanted = detector.asin() / (PI / 2.0);
            let diff = db(gain / wanted);
            if wanted > gain {
                max_attack = -1.0;
                let x = 0.25 * (if diff.is_nan() { -1.0 } else { diff }.clamp(-12.0, 0.0) + 12.0);
                let frames = ka + kb * x + kc * x * x + kd * x * x * x + ke * x * x * x * x;
                rate_now = lin(5.0 / frames);
            } else {
                let diff = if diff.is_nan() { 1.0 } else { diff };
                if max_attack == -1.0 || max_attack < diff {
                    max_attack = diff;
                }
                rate_now = 1.0 - (0.25 / max_attack.max(0.5)).powf(1.0 / attack_frames);
            }
        }
        let wanted = detector.asin() / (PI / 2.0);
        let level = sig[0][i].abs().max(sig[1][i].abs());
        let attenuation = if level <= 0.0001 { 1.0 } else { curve.shape(level) / level };
        let fall = lin((-db(attenuation)).max(2.0) / sat_release_frames) - 1.0;
        detector = (detector + (attenuation - detector) * if attenuation > detector { fall } else { 1.0 }).min(1.0);
        if rate_now < 1.0 {
            gain += (wanted - gain) * rate_now;
        } else {
            gain = (gain * rate_now).min(1.0);
        }
        gains[i] = makeup * (PI / 2.0 * gain).sin();
    }
    for ch in sig.iter_mut() {
        for i in 0..n {
            // the level was read `ahead` samples before the sound it's applied to
            ch[i] *= gains[(i + ahead).min(n - 1)];
        }
    }
}

/// A rendered cue.
pub struct Stereo {
    pub rate: u32,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

/// Renders voices into a buffer: mixed, sent through the room, saturated a little, limited, at `volume` (0 to 1).
/// `seed` picks the noise; the same seed gives the same buffer.
pub fn render(voices: &[Voice], tone: &Tone, rate: u32, volume: f32, seed: u64) -> Stereo {
    let mut mix = Mix { rate: rate as f32, dry: [Vec::new(), Vec::new()], wet: [Vec::new(), Vec::new()], rng: Rng(seed | 1) };
    for v in voices {
        match v {
            Voice::Hit(o) => mix.hit(o, tone),
            Voice::Air(o) => mix.air(o),
            Voice::Fm(o) => mix.fm(o),
            Voice::Pad(o) => mix.pad(o),
            Voice::Swell(o) => mix.swell(o),
        }
    }
    let mut out = room(&mix.wet, mix.rate);
    // gentle saturation gives the pure tones a little body without sounding distorted
    let body = 1.15f32;
    for ch in 0..2 {
        for (i, o) in out[ch].iter_mut().enumerate() {
            let x = (*o * 0.9 + mix.dry[ch].get(i).copied().unwrap_or(0.0)).clamp(-1.0, 1.0);
            *o = (x * body).tanh() / body.tanh();
        }
    }
    limit(&mut out, mix.rate);
    // the tail is cut where it can't be heard any more
    let quiet = |ch: &Vec<f32>| ch.iter().rposition(|s| s.abs() > 2e-5).map_or(0, |i| i + 1);
    let keep = quiet(&out[0]).max(quiet(&out[1]));
    let [mut left, mut right] = out;
    left.truncate(keep);
    right.truncate(keep);
    left.iter_mut().chain(right.iter_mut()).for_each(|s| *s *= volume);
    Stereo { rate, left, right }
}

impl Stereo {
    /// As a 16-bit WAV file.
    pub fn wav(&self) -> Vec<u8> {
        let n = self.left.len() as u32;
        let mut out = Vec::with_capacity(44 + n as usize * 4);
        out.extend(b"RIFF");
        out.extend((36 + n * 4).to_le_bytes());
        out.extend(b"WAVEfmt ");
        out.extend(16u32.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(2u16.to_le_bytes());
        out.extend(self.rate.to_le_bytes());
        out.extend((self.rate * 4).to_le_bytes());
        out.extend(4u16.to_le_bytes());
        out.extend(16u16.to_le_bytes());
        out.extend(b"data");
        out.extend((n * 4).to_le_bytes());
        for (l, r) in self.left.iter().zip(&self.right) {
            for s in [l, r] {
                out.extend(((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
            }
        }
        out
    }
}
