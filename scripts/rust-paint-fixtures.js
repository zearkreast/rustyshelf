#!/usr/bin/env node
// Writes crates/rustyshelf-core/tests/fixtures/cssbg.json: CSS backgrounds as Chromium paints them (headless, through
// Playwright), for the Rust painter's tests: every theme's backdrop, the themes' Settings tiles and the shelf's own
// layers, each over mid grey, as a coarse grid of colours.
//   node scripts/rust-fixtures.js && node scripts/rust-paint-fixtures.js
'use strict';
const fs = require('fs'), path = require('path');
const { chromium } = require('playwright');

const ROOT = path.join(__dirname, '..'), FIX = path.join(ROOT, 'crates', 'rustyshelf-core', 'tests', 'fixtures');
const CELL = 10;

const themes = JSON.parse(fs.readFileSync(path.join(FIX, 'themes.json'), 'utf8'));
const win = {}; new Function('window', fs.readFileSync(path.join(ROOT, 'renderer', 'themes-extra.js'), 'utf8'))(win);
const glow = 'hsla(258,95%,62%,0.62)';
const cases = [
  // (CRT's scan lines change colour right on the middles of the pixels, where a browser's rounding picks the side)
  ...themes.themes.map(t => ({ name: 'theme ' + t.id, width: 640, height: 360, css: themes.tints.find(x => x.id === t.id).css, ...(t.id === 'crt' ? { ties: true } : {}) })),
  ...win.BS_EXTRA_THEMES.map(t => ({ name: 'tile ' + t.id, width: 240, height: 140, css: t.swatch })),
  // the shelf's own layers (renderer/style.css: .vignette, .floor, .sel-halo, the clear plastic over a cover)
  { name: 'vignette', width: 640, height: 360, css: 'radial-gradient(ellipse 80% 70% at 50% 42%,transparent 35%,rgba(0,0,0,.5) 100%),linear-gradient(to bottom,rgba(0,0,0,.25),transparent 20%,transparent 62%,rgba(0,0,0,.5))' },
  { name: 'floor', width: 700, height: 160, css: `radial-gradient(ellipse 14% 22% at 50% 18%,${glow},transparent 75%),radial-gradient(ellipse 50% 30% at 50% 30%,rgba(0,0,0,.45),transparent 70%)` },
  { name: 'halo', width: 410, height: 430, css: `radial-gradient(closest-side,${glow},transparent)` },
  { name: 'plastic', width: 200, height: 290, css: 'linear-gradient(180deg,rgba(255,255,255,.16),rgba(255,255,255,.04) 18%,transparent 34%),linear-gradient(90deg,rgba(0,0,0,.14),transparent 10%,transparent 88%,rgba(0,0,0,.18))' },
  { name: 'corners', width: 300, height: 200, css: 'radial-gradient(circle at 20% 30%,#fff,rgba(255,0,0,.5) 40%,transparent),radial-gradient(farthest-side at 70% 60%,#0f0,transparent),conic-gradient(from 40deg at 30% 70%,#00f,#ff0 120deg,#00f)' },
];

(async () => {
  const browser = await chromium.launch();
  const pg = await browser.newPage({ viewport: { width: 800, height: 600 }, deviceScaleFactor: 1 });
  await pg.setContent('<html><body style="margin:0;background:#808080"><div id="b" style="position:absolute;left:0;top:0"></div></body></html>');
  for (const c of cases) {
    await pg.evaluate(c => { const b = document.getElementById('b'); b.style.width = c.width + 'px'; b.style.height = c.height + 'px'; b.style.background = c.css; }, c);
    const shot = (await pg.locator('#b').screenshot()).toString('base64');
    c.grid = await pg.evaluate(async ({ shot, cell }) => {
      const im = new Image(); im.src = 'data:image/png;base64,' + shot; await im.decode();
      const cv = document.createElement('canvas'); cv.width = im.width; cv.height = im.height; const x = cv.getContext('2d'); x.drawImage(im, 0, 0);
      const d = x.getImageData(0, 0, im.width, im.height).data, grid = [];
      for (let gy = 0; gy + cell <= im.height; gy += cell) for (let gx = 0; gx + cell <= im.width; gx += cell) {
        const sum = [0, 0, 0]; for (let py = gy; py < gy + cell; py++) for (let px = gx; px < gx + cell; px++) { const o = (py * im.width + px) * 4; sum[0] += d[o]; sum[1] += d[o + 1]; sum[2] += d[o + 2]; }
        grid.push(...sum.map(v => Math.round(v / (cell * cell))));
      }
      return grid;
    }, { shot, cell: CELL });
  }
  await browser.close();
  fs.writeFileSync(path.join(FIX, 'cssbg.json'), JSON.stringify({ cell: CELL, under: 128, cases }) + '\n');
  console.log('wrote', path.relative(ROOT, path.join(FIX, 'cssbg.json')), `(${cases.length} backgrounds)`);
})().catch(e => { console.error(e); process.exit(1); });
