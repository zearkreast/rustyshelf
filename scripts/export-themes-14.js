#!/usr/bin/env node
// Writes crates/rustyshelf-core/data/themes-14.json: the pictures and patterns Boxshelf 1.82.0-beta.14's new themes are
// made of (renderer/themes-extra.js at sassiestrhihno/boxshelf-source@8c6b0ab, read from git: the Electron app here is
// frozen before them), and the theme list in that version's order. The Rust app writes the colours round them.
//   node scripts/export-themes-14.js           write the file, and the parity fixture tests/fixtures/themes14.json
//   node scripts/export-themes-14.js --check   exit 1 when it's out of date
'use strict';
const fs = require('fs'), path = require('path'), { execFileSync } = require('child_process');

const ROOT = path.join(__dirname, '..'), OUT = path.join(ROOT, 'crates', 'rustyshelf-core', 'data', 'themes-14.json');
const UPSTREAM = '8c6b0ab';
const PARTS = ['NEB_STARS', 'DUSK_STARS', 'ABYSS_SNOW', 'EMBER_STILL', 'HOLO_GRID', 'SF_STARS', 'SF_STREAKS', 'RAIN_GLASS', 'SAKURA_BRANCH', 'SNOW_HILLS', 'SNOW_STILL',
  'EH', 'EH_STARS', 'GL_FAR', 'GL_NEAR', 'GL_STARS', 'NC_SKY', 'DS_SNOW', 'ST_SKY', 'FF_LAND', 'FF_STARS', 'DN_DUNES', 'DN_MOON', 'DN_STARS'];
const src = execFileSync('git', ['show', `${UPSTREAM}:renderer/themes-extra.js`], { cwd: ROOT, encoding: 'utf8', maxBuffer: 16 << 20 });
const hooked = src.replace('window.BS_EXTRA_THEMES = [', `window.BS_THEME_PARTS = { ${PARTS.join(', ')}, PRISM_POOLS };\n  window.BS_EXTRA_THEMES = [`);
if (hooked === src) throw new Error('themes-extra.js: the theme list was not found');
// (Prism draws on a canvas it makes as the file loads: a stand-in that draws nothing)
const canvas = () => ({ width: 0, height: 0, getContext: () => new Proxy({}, { get: () => () => ({ addColorStop() {} }) }) });
const win = {}; new Function('window', 'document', hooked)(win, { createElement: canvas });
const text = JSON.stringify({ from: `sassiestrhihno/boxshelf-source@${UPSTREAM} renderer/themes-extra.js`, parts: win.BS_THEME_PARTS, retired: win.BS_THEME_RETIRED || {},
  extra: win.BS_EXTRA_THEMES.map(t => ({ id: t.id, name: t.name, moves: typeof t.fx === 'function' || !!t.css })) }, null, 1) + '\n';
// the parity fixture: every extra theme's backdrop and glow at a few hues, as that version paints them
const FIXTURE = path.join(ROOT, 'crates', 'rustyshelf-core', 'tests', 'fixtures', 'themes14.json');
const tints = [];
for (const t of win.BS_EXTRA_THEMES) for (const sh of [258, 0, 351.6, -12.5, 374.25]) tints.push({ id: t.id, sh, halo: typeof t.halo === 'function' ? t.halo(sh) : sh, css: t.tint(sh, { hue: 40 }, .5) });
const fixture = JSON.stringify({ themes: win.BS_EXTRA_THEMES.map(t => t.id), retired: win.BS_THEME_RETIRED, tints }) + '\n';
if (process.argv.includes('--check')) {
  const now = fs.existsSync(OUT) ? fs.readFileSync(OUT, 'utf8') : '';
  if (now !== text || !fs.existsSync(FIXTURE) || fs.readFileSync(FIXTURE, 'utf8') !== fixture) { console.error('themes-14.json is out of date: run node scripts/export-themes-14.js'); process.exit(1); }
} else { fs.writeFileSync(OUT, text); fs.writeFileSync(FIXTURE, fixture); console.log('wrote', path.relative(ROOT, OUT), 'and', path.relative(ROOT, FIXTURE)); }
