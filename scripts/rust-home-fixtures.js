#!/usr/bin/env node
// Writes crates/rustyshelf-core/tests/fixtures/homemenu.json: the Home menu (renderer/home.html) as Chromium shows
// it (headless, through Playwright) while buttons are pressed, for the Rust menu's tests: its rows, the one chosen,
// the prompts, and what each run ends with.
//   node scripts/rust-home-fixtures.js
'use strict';
const fs = require('fs'), path = require('path');
const { chromium } = require('playwright');

const ROOT = path.join(__dirname, '..'), OUT = path.join(ROOT, 'crates', 'rustyshelf-core', 'tests', 'fixtures', 'homemenu.json');
const MIN = 60000;
const game = { inGame: true, title: 'Zeta Run', system: 'GameCube', actions: ['resume', 'save', 'load', 'shot', 'power', 'quit'], power: ['sleep', 'restart', 'shutdown'], canSave: true, slot: 1, paused: true, played: 125.4 * MIN };
const shelf = { inGame: false, title: 'Home', system: 'RustyShelf', subtitle: '42 games', actions: ['shelfhome', 'museum', 'settings', 'power'], power: ['sleep', 'restart', 'shutdown', 'quitapp'] };
const b = (name, down = true) => ['btn', name, down];
// each run: what the menu is opened with, then buttons (name, going down or up), waits (ms) and the game's pause landing
const runs = [
  { info: game, steps: [b('down'), b('down'), b('up'), b('up'), b('up'), b('up'), b('a'), b('down'), b('down'), b('down'), b('up'), b('a'), ['wait', 400], b('a', false), b('down'), b('a'), b('down'), b('a'), ['wait', 400], b('a', false), ['wait', 100], b('a'), ['wait', 1400]] },
  { info: { ...game, actions: ['resume', 'shot', 'power', 'quit'], canSave: false, slot: 0, paused: false, confirmButton: 'b', glyphs: 'nintendo', played: 20000 }, steps: [b('down'), b('down'), b('b'), b('a'), b('down'), b('up'), b('home')] },
  { info: { ...shelf, glyphs: 'ps', layout: 'center' }, steps: [b('right'), b('down'), b('left'), b('ok'), b('up'), b('left'), b('down'), b('cancel'), b('start')] },
  { info: shelf, steps: [b('down'), b('down'), b('a')] },
  { info: { ...shelf, layout: 'center' }, steps: [b('left'), b('a'), b('down'), b('down'), b('down'), b('a'), ['wait', 500], b('down'), ['wait', 700], b('up'), b('a'), ['wait', 1400]] },
  { info: { inGame: true, played: 59.6 * MIN }, steps: [b('down'), b('up'), b('b')] },
  { info: { inGame: true, actions: ['resume', 'power', 'quit'], played: 31000 }, steps: [b('down'), b('a'), ['paused', true], b('cancel'), ['paused', false], b('ok')] },
  { info: { ...game, slot: 0, played: 600 * MIN }, steps: [b('down'), b('a')] },
  { info: { ...game, played: 59.4 * MIN }, steps: [b('down'), b('down'), b('down'), b('ok')] },
  { info: { ...game, played: 60.4 * MIN, confirmButton: 'b', glyphs: 'ps' }, steps: [b('down'), b('down'), b('down'), b('down'), b('b'), b('b'), ['wait', 1400]] },
];

(async () => {
  const browser = await chromium.launch();
  const pg = await browser.newPage({ viewport: { width: 1280, height: 720 } });
  await pg.addInitScript(() => {
    window.__acts = [];
    window.home = { onOpen: f => { window.__open = f; }, onButton: f => { window.__button = f; }, onToast: () => {}, onPaused: f => { window.__paused = f; }, act: a => window.__acts.push(a) };
  });
  await pg.goto('file://' + path.join(ROOT, 'renderer', 'home.html'));
  const snap = () => pg.evaluate(() => {
    const open = document.body.classList.contains('open'), text = id => document.getElementById(id).textContent;
    if (!open) return { open, acts: [...window.__acts] };
    const kids = [...document.getElementById('items').children], items = kids.filter(k => k.classList.contains('item'));
    return { open, acts: [...window.__acts], title: text('title'), sys: text('sys'), time: text('time'), subhead: (kids.find(k => k.classList.contains('subhead')) || {}).textContent || null,
      seps: kids.map((k, i) => k.classList.contains('sep') ? items.indexOf(kids[i + 1]) : -1).filter(i => i >= 0),
      items: items.map(k => ({ label: k.children[1].textContent, sub: (k.querySelector('.sub') || {}).textContent || '', sel: k.classList.contains('sel'), danger: k.classList.contains('danger') })),
      foot: [...document.getElementById('foot').children].map(s => [s.firstChild.textContent, s.lastChild.textContent]) };
  });
  for (const run of runs) {
    const { played, ...info } = run.info;
    await pg.evaluate(({ info, played }) => { window.__acts.length = 0; window.__open({ ...info, ...(played ? { startedAt: Date.now() - played } : {}) }); }, { info, played });
    await pg.waitForFunction(() => document.body.classList.contains('open'));
    run.shown = [await snap()];
    for (const [what, a, down] of run.steps) {
      if (what === 'btn') await pg.evaluate(({ btn, down }) => window.__button({ btn, down }), { btn: a, down });
      else if (what === 'paused') await pg.evaluate(p => window.__paused(p), a);
      else await pg.waitForTimeout(a);
      run.shown.push(await snap());
    }
  }
  await browser.close();
  fs.writeFileSync(OUT, JSON.stringify({ runs }, null, 1) + '\n');
  console.log('wrote', path.relative(ROOT, OUT));
})().catch(e => { console.error(e); process.exit(1); });
