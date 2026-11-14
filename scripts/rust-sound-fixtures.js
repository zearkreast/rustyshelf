// Writes crates/rustyshelf-core/tests/fixtures/synth.json: voices rendered by the Electron app's own sound engine
// (renderer/sound.js, in a headless Chromium through Playwright), for the Rust engine's tests to be compared with;
// cues.json: the voices every cue asks that engine for; music.json: what the menu music asks to be played; and
// drawn.json: the covers renderer/covers.js draws for games without box art.
//   node scripts/rust-sound-fixtures.js
// Every case starts on a render quantum, after a lead-in: an offline limiter starts closed and takes a moment to open,
// and an oscillator started inside a quantum plays its first samples at the wrong pitch.
'use strict';
const fs = require('fs'), path = require('path');
const { chromium } = require('playwright');

const ROOT = path.join(__dirname, '..');
const MUSIC_OUT = path.join(ROOT, 'crates', 'rustyshelf-core', 'tests', 'fixtures', 'music.json');
const DRAWN_OUT = path.join(ROOT, 'crates', 'rustyshelf-core', 'tests', 'fixtures', 'drawn.json');
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
  fs.writeFileSync(CUES_OUT, JSON.stringify({ runs }) + '\n');
  console.log('wrote', path.relative(ROOT, CUES_OUT), `(${runs.length} runs)`);

  // ---- the menu music's score: what each track asks to be played over a few minutes, for a seed and a key ----
  const score = src
    .replace('function mEp(m, f, t, dur, vel, pan) {', "function mEp(m, f, t, dur, vel, pan) { if (window.__mus) { window.__mus.push(['ep', f, t, dur, vel, pan]); return; }")
    .replace('function mBell(m, f, t, vel, pan) {', "function mBell(m, f, t, vel, pan) { if (window.__mus) { window.__mus.push(['bell', f, t, vel, pan]); return; }")
    .replace('function mGlass(m, f, t, vel, pan) {', "function mGlass(m, f, t, vel, pan) { if (window.__mus) { window.__mus.push(['glass', f, t, vel, pan]); return; }")
    .replace('function mDrone(m, f, t, dur, vel) {', "function mDrone(m, f, t, dur, vel) { if (window.__mus) { window.__mus.push(['drone', f, t, dur, vel]); return; }")
    .replace('function padTo(m, notes, t) {', "function padTo(m, notes, t) { if (window.__mus) { window.__mus.push(['pad', notes.slice(), t, m.step > 1]); return; }")
    .replace('m.pad.out.gain.setTargetAtTime(m.padBase * (.7 + .3 * m.rng()), t, 2.2);', "if (window.__mus) window.__mus.push(['level', m.padBase * (.7 + .3 * m.rng()), t]); else m.pad.out.gain.setTargetAtTime(m.padBase * (.7 + .3 * m.rng()), t, 2.2);");
  if (score.split('window.__mus.push').length !== 7) throw new Error('renderer/sound.js changed: the music hooks no longer fit');
  const scores = [];
  for (const [track, seed, shelf, theme] of [['drift', 1, 'gc', 'aero'], ['night', 7, 'ps4', 'aero'], ['glow', 3, 'sw', 'musicbox'], ['glow', 123456789, 'wii', 'melody'], ['drift', -5, 'media', 'aero'], ['lounge', 99, 'ps2', 'aero']]) {
    const pg = await browser.newPage(); await pg.setContent('<html></html>'); await pg.addScriptTag({ content: score });
    const events = await pg.evaluate(async ({ track, seed, key, theme }) => {
      window.Sound.set(Object.assign({ theme }, key || { key: 0 })); window.__mus = [];
      await window.Sound.renderMusic(track, 240, seed, .4);
      return window.__mus;
    }, { track, seed, key: KEYS[shelf], theme });
    scores.push({ track, seed, shelf, theme, events });
    await pg.close();
  }
  // ---- and its sound: half a minute of each track as the engine renders it, by its level every quarter of a second ----
  const sounds = [];
  for (const [track, seed, shelf] of [['drift', 1, 'gc'], ['night', 7, 'ps4'], ['glow', 3, 'sw']]) {
    const pg = await browser.newPage(); await pg.setContent('<html></html>'); await pg.addScriptTag({ content: src });
    const rms = await pg.evaluate(async ({ track, seed, key }) => {
      window.Sound.set(key || { key: 0 });
      const buf = await window.Sound.renderMusic(track, 30, seed, .4), step = 11025;
      return [0, 1].map(c => { const d = buf.getChannelData(c), out = []; for (let i = 0; i + step <= d.length; i += step) { let q = 0; for (let j = i; j < i + step; j++) q += d[j] * d[j]; out.push(+Math.sqrt(q / step).toFixed(6)); } return out; });
    }, { track, seed, key: KEYS[shelf] });
    sounds.push({ track, seed, shelf, rms });
    await pg.close();
  }
  fs.writeFileSync(MUSIC_OUT, JSON.stringify({ start: .05, seconds: 240, scores, sounds }) + '\n');

  // ---- the covers drawn for games without box art (renderer/covers.js), as a coarse grid of their colours ----
  const looks = JSON.parse(fs.readFileSync(path.join(ROOT, 'crates', 'rustyshelf-core', 'data', 'looks.json'), 'utf8')).style;
  const face = (family, weight, file) => `@font-face{font-family:"${family}";font-weight:${weight};src:url(data:font/woff2;base64,${fs.readFileSync(path.join(ROOT, 'renderer', 'fonts', file)).toString('base64')})}`;
  const css = [face('Lilita One', 400, 'lilita-one-latin-400-normal.woff2'), face('Bungee', 400, 'bungee-latin-400-normal.woff2'), face('Audiowide', 400, 'audiowide-latin-400-normal.woff2'),
    face('Lexend', 400, 'lexend-latin-400-normal.woff2'), face('Lexend', 500, 'lexend-latin-500-normal.woff2'), face('Lexend', 700, 'lexend-latin-700-normal.woff2'), face('Lexend', 800, 'lexend-latin-800-normal.woff2')].join('');
  const pg = await browser.newPage(); await pg.setContent(`<html><head><style>${css}</style></head><body></body></html>`);
  await pg.addScriptTag({ content: fs.readFileSync(path.join(ROOT, 'renderer', 'covers.js'), 'utf8') });
  // made-up titles, picked so that every scene and every title font comes up
  const TITLES = ['Zeta Run', 'Omega Quest: The Long Way Home', 'Beta Kart', 'Sigma', 'Tau Racers Deluxe Edition', 'Phi and Rho', 'Alpha Tale', 'Gamma Force', 'Delta Squadron 2', 'Kappa Nights', 'Lambda', 'Iota Garden', 'Theta Drift', 'Mu Stadium'];
  const wanted = [...TITLES.map((title, i) => ({ look: ['gc', 'snes', 'n3ds', 'pc', 'sw', 'ps2', 'psx', 'gba'][i % 8], title, region: i % 3 ? '' : 'usa', side: 'front' })),
    { look: 'media', title: 'Made Up Player', region: '', side: 'front', scene: 'media' },
    { look: 'gc', title: 'Zeta Run', side: 'back', facts: [['Region', 'USA'], ['File', 'ISO · 1.4 GB'], ['Emulator', 'Dolphin']], blurb: 'A made-up game about running, written to try the back of a drawn cover on for size.' },
    { look: 'snes', title: 'Omega Quest: The Long Way Home', side: 'back', facts: [['Region', 'Europe']], blurb: '' },
    // the shelf's own cases: Surprise me, and Add games on a shelf without any
    { look: 'gc', title: 'Surprise me', side: 'surprise' }, { look: 'snes', title: 'Surprise me', side: 'surprise' }, { look: 'pc', title: 'Add games', side: 'add' }, { look: 'ps2', title: 'Add games', side: 'add' }];
  const covers = await pg.evaluate(async ({ wanted, looks }) => {
    await Promise.all(['400 20px "Lilita One"', '400 20px Bungee', '400 20px Audiowide', '400 20px Lexend', '500 20px Lexend', '700 20px Lexend', '800 20px Lexend'].map(f => document.fonts.load(f)));
    const out = [];
    for (const w of wanted) {
      const seed = window.Covers.hash(w.title), game = { title: w.title, seed, hue: seed % 360, region: w.region, scene: w.scene };
      const url = w.side === 'back' ? window.Covers.back(looks[w.look], game, w.facts, w.blurb) : w.side === 'surprise' ? window.Covers.randomFront(looks[w.look])
        : w.side === 'add' ? window.Covers.setupFront(looks[w.look]) : window.Covers.front(looks[w.look], game);
      const im = new Image(); im.src = url; await im.decode();
      const c = document.createElement('canvas'); c.width = im.width; c.height = im.height; const x = c.getContext('2d'); x.drawImage(im, 0, 0);
      const d = x.getImageData(0, 0, im.width, im.height).data, cell = 20, grid = [];
      for (let gy = 0; gy + cell <= im.height; gy += cell) for (let gx = 0; gx + cell <= im.width; gx += cell) {
        const sum = [0, 0, 0]; for (let py = gy; py < gy + cell; py++) for (let px = gx; px < gx + cell; px++) { const o = (py * im.width + px) * 4; sum[0] += d[o]; sum[1] += d[o + 1]; sum[2] += d[o + 2]; }
        grid.push(sum.map(v => Math.round(v / (cell * cell))));
      }
      out.push({ ...w, width: im.width, height: im.height, cell, grid });
    }
    return out;
  }, { wanted, looks });
  await browser.close();
  fs.writeFileSync(DRAWN_OUT, JSON.stringify({ covers }) + '\n');
  console.log('wrote', path.relative(ROOT, DRAWN_OUT), `(${covers.length} covers)`);
  console.log('wrote', path.relative(ROOT, MUSIC_OUT), `(${scores.length} scores)`);
})().catch(e => { console.error(e); process.exit(1); });
