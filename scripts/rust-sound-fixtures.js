// Writes crates/rustyshelf-core/tests/fixtures/synth.json: voices rendered by the Electron app's own sound engine
// (renderer/sound.js, in a headless Chromium through Playwright), for the Rust engine's tests to be compared with;
// and cues.json: the voices every cue asks that engine for.
//   node scripts/rust-sound-fixtures.js
// Every case starts on a render quantum, after a lead-in: an offline limiter starts closed and takes a moment to open,
// and an oscillator started inside a quantum plays its first samples at the wrong pitch.
'use strict';
const fs = require('fs'), path = require('path');
const { chromium } = require('playwright');

const ROOT = path.join(__dirname, '..');
const CUES_OUT = path.join(ROOT, 'crates', 'rustyshelf-core', 'tests', 'fixtures', 'cues.json');
const OUT = path.join(ROOT, 'crates', 'rustyshelf-core', 'tests', 'fixtures', 'synth.json');
const RATE = 44100, LEAD_FRAMES = 22016, AHEAD = 264; // (the limiter's 6 ms look-ahead)
const Q = 128 / RATE; // a render quantum: a voice that starts later starts on one
const BELL = [[1, 1, 1], [2, .28, .55], [3, .1, .38], [4.16, .05, .28], [5.4, .02, .2]];

// mode: exact (nothing random in it), shape (close: a band-limited sawtooth, a level that drives the limiter),
// noise (its noise or its room differ take by take: the energy over time is compared)
const CASES = [
  { name: 'a plain struck tone', mode: 'exact', secs: .4, voices: [['hit', { freq: 2637.02, dur: .05, vol: .22, noise: 0, verb: 0, breath: 0, attack: .004 }]] },
  { name: 'a bell with a thump, to one side', mode: 'exact', secs: 1.2, voices: [['hit', { freq: 880, dur: .55, vol: .26, at: 34 * Q, attack: .002, verb: 0, pan: .3, partials: BELL, noise: 0, breath: 0, thump: 1.5 }]] },
  { name: 'a low thump with its octave below', mode: 'exact', secs: 1, voices: [['hit', { freq: 146.83, dur: .45, vol: .22, attack: .003, sub: 1, bright: 420, verb: 0, noise: 0, breath: 0 }]] },
  { name: 'a gliding tone', mode: 'exact', secs: .8, voices: [['hit', { freq: 660, dur: .3, vol: .4, glide: 330, verb: 0, noise: 0, breath: 0, pan: -.4 }]] },
  { name: 'an FM bell with a twin', mode: 'exact', secs: 1.4, voices: [['fm', { freq: 1318.5, dur: .9, vol: .22, ratio: 3.5, index: 2.2, pan: .18, verb: 0, detune: 4 }]] },
  { name: 'an FM key', mode: 'exact', secs: 1, voices: [['fm', { freq: 440, at: 17 * Q, dur: .6, vol: .3, ratio: 1, index: 3, floor: .15, verb: 0, pan: -.15 }]] },
  { name: 'a pad opening up', mode: 'shape', secs: 3.4, voices: [['pad', { freqs: [146.83, 220, 293.66], dur: 2.6, vol: .3, from: 180, to: 3200, open: 1.3, attack: 1.1, hold: 1.4, verb: 0 }]] },
  { name: 'a loud tone into the limiter', mode: 'shape', secs: 1, voices: [['hit', { freq: 523.25, dur: .5, vol: 6, verb: 0, noise: 0, breath: 0, attack: .003 }]] },
  { name: 'a whoosh', mode: 'noise', secs: .8, voices: [['air', { dur: .5, vol: .6, filter: 'bandpass', freq: 900, sweepTo: 4500, q: .5, verb: 0, attack: .12, soft: true }]] },
  { name: 'white air through a highpass', mode: 'noise', secs: .8, voices: [['air', { dur: .4, vol: .5, filter: 'highpass', freq: 3500, verb: 0, attack: .12, pan: .5 }]] },
  // (a tone's level in the room depends on the take of the room's noise, at that pitch: only noise shows the room itself)
  { name: 'a burst of noise in the room', mode: 'noise', secs: 3, voices: [['air', { dur: .06, vol: 2, filter: 'bandpass', freq: 1500, q: .3, verb: 1, attack: .002 }]] },
  { name: 'soft noise in the room, from the left', mode: 'noise', secs: 3, voices: [['air', { dur: .15, vol: 2, filter: 'lowpass', freq: 6000, q: .5, verb: .8, pan: -.5, soft: true }]] },
  { name: 'a breathy tone', mode: 'noise', secs: 1.2, voices: [['hit', { freq: 1318.5, dur: .32, vol: .5, verb: 0, breath: .5, noise: .06, attack: .012 }]] },
  { name: 'a swell', mode: 'noise', secs: 2.4, voices: [['swell', { notes: [0, 2, 3], octave: -1, freqs: [587.33, 739.99, 880], dur: 1.6, vol: .2, at: 17 * Q, attack: .45, verb: 0 }]] },
];

(async () => {
  let src = fs.readFileSync(path.join(ROOT, 'renderer', 'sound.js'), 'utf8');
  // the voices are private to the engine: this copy hands them out, and takes a function where a cue's name goes
  src = src.replace('return { warm,', 'return { _v: { hit, air, fm, pad, swell }, warm,').replace('function voiceFor(name) {', "function voiceFor(name) { if (typeof name === 'function') return name;");
  if (!src.includes('_v: {') || !src.includes("typeof name === 'function'")) throw new Error('renderer/sound.js changed: the hooks no longer fit');
  const browser = await chromium.launch(), page = await browser.newPage();
  await page.setContent('<html></html>'); await page.addScriptTag({ content: src });
  const cases = [];
  for (const c of CASES) {
    const r = await page.evaluate(async ({ voices, secs, RATE, LEAD_FRAMES, AHEAD, from }) => {
      const lead = (LEAD_FRAMES - .01) / RATE;
      const buf = await window.Sound.renderDemo('aero', [['none', {}, lead], [() => voices.forEach(([k, o]) => window.Sound._v[k](o)), {}, 0]], lead + secs);
      const ch = [0, 1].map(i => buf.getChannelData(i).slice(LEAD_FRAMES + AHEAD));
      const peaks = (d, step) => { const out = []; for (let i = 0; i + step <= d.length; i += step) { let m = 0; for (let j = i; j < i + step; j++) m = Math.max(m, Math.abs(d[j])); out.push(+m.toFixed(6)); } return out; };
      const rms = (d, step) => { const out = []; for (let i = 0; i + step <= d.length; i += step) { let s = 0; for (let j = i; j < i + step; j++) s += d[j] * d[j]; out.push(+Math.sqrt(s / step).toFixed(6)); } return out; };
      return { peaks: ch.map(d => peaks(d, 441)), rms: ch.map(d => rms(d, 2205)), samples: Array.from(ch[0].slice(from, from + 600)).map(x => +x.toFixed(6)) };
    }, { voices: c.voices, secs: c.secs, RATE, LEAD_FRAMES, AHEAD, from: Math.round((c.voices[0][1].at || 0) * RATE) });
    // peaks: the loudest sample of each 10 ms, left and right; rms: the level of each 50 ms; samples: the left's first 600 from where the first voice starts
    cases.push({ name: c.name, mode: c.mode, voices: c.voices, peaks: r.peaks, rms: r.rms, samples: c.mode === 'exact' ? r.samples : undefined });
  }
  fs.writeFileSync(OUT, JSON.stringify({ rate: RATE, cases }) + '\n');
  console.log('wrote', path.relative(ROOT, OUT), `(${cases.length} cases)`);

  // ---- the cues: the voices each one asks for, in every voice of the interface and a few shelves' keys ----
  // (this copy of the engine only notes what a cue asks for, and takes chance out of it: the middle of every random pick)
  const rec = src
    .replace('function hit(o) {', "function hit(o) { if (window.__rec) { window.__rec.push(['hit', o]); return; }")
    .replace('function air(o) {', "function air(o) { if (window.__rec) { window.__rec.push(['air', o]); return; }")
    .replace('function fm(o) {', "function fm(o) { if (window.__rec) { window.__rec.push(['fm', o]); return; }")
    .replace('function pad(o) {', "function pad(o) { if (window.__rec) { window.__rec.push(['pad', o]); return; }")
    .replace('function swell(o) {', "function swell(o) { if (window.__rec) { window.__rec.push(['swell', Object.assign({}, o, { freqs: (o.notes || [0]).map(n => deg(n, o.octave || 0)) })]); return; }");
  if (rec.split('window.__rec.push').length !== 6) throw new Error('renderer/sound.js changed: the hooks no longer fit');
  const songs = fs.readFileSync(path.join(ROOT, 'renderer', 'songs.js'), 'utf8');
  const keys = /const KEYS=(\{.*?\});/.exec(fs.readFileSync(path.join(ROOT, 'renderer', 'app.js'), 'utf8'));
  if (!keys) throw new Error("renderer/app.js changed: the shelves' keys weren't found");
  const KEYS = new Function('return ' + keys[1])();
  const ONE = ['open', 'back', 'go', 'flip', 'caseOpen', 'caseClose', 'discOut', 'page', 'hold', 'error', 'toast', 'unlock', 'boot', 'lidClick', 'seat', 'whir', 'lamp', 'museum', 'slide', 'slideBack', 'lift', 'discIn', 'pedestal', 'arrive', 'panel', 'nothing'];
  // moving along the shelf quickly, back slowly, after a pause; changing shelf slowly and in a flick; then every other cue
  const STEPS = [...Array(7).fill(['tick', 1, .05]), ...Array(3).fill(['tick', -1, .3]), ['tick', 1, 3], ['tick', 1, .12], ['tick', 0, 1],
    ['shelf', 1, 1], ['shelf', 1, .2], ['shelf', -1, .3], ['shelf', -1, 3], ...Array(30).fill(['tick', 1, .2]), ...ONE.map(c => [c, 0, 1])];
  const runs = [];
  for (const theme of ['aero', 'melody', 'musicbox', 'glass', 'jazz']) for (const shelf of theme.length > 5 || theme === 'aero' ? ['gc', 'sw', 'ps4', 'media', 'ps2'] : ['gc', 'x360'])
    runs.push({ theme, shelf, boot: 'chime', steps: STEPS });
  for (const boot of ['crystal', 'pulse', 'horizon', 'keys']) runs.push({ theme: 'aero', shelf: 'wii', boot, steps: [['boot', 0, 1]] });
  for (const run of runs) {
    const pg = await browser.newPage(); await pg.setContent('<html></html>');
    await pg.addScriptTag({ content: 'Math.random = () => .5;' + songs + rec });
    run.cues = await pg.evaluate(async ({ theme, key, boot, steps }) => {
      window.Sound.set(Object.assign({ boot }, key || { key: 0 }));
      const out = []; let at = 0; const seq = [];
      for (const [cue, dir, gap] of steps) { const t = at; seq.push([() => { window.__rec = []; const before = window.__rec; window.Sound.play(cue, { dir }); out.push({ cue, dir, at: t, voices: before }); window.__rec = null; }, {}, gap]); at += gap; }
      await window.Sound.renderDemo(theme, seq, .05);
      return JSON.parse(JSON.stringify(out));
    }, { theme: run.theme, key: KEYS[run.shelf], boot: run.boot, steps: run.steps });
    delete run.steps;
    await pg.close();
  }
  await browser.close();
  fs.writeFileSync(CUES_OUT, JSON.stringify({ runs }) + '\n');
  console.log('wrote', path.relative(ROOT, CUES_OUT), `(${runs.length} runs)`);
})().catch(e => { console.error(e); process.exit(1); });
