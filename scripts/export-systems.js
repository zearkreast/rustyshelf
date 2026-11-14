#!/usr/bin/env node
// Writes main/systems.js's consoles and apps as JSON for the Rust app (crates/rustyshelf-core/data), once for Windows
// and once for Linux (systems.js rewrites its paths and programs for Linux when it's loaded there); with them the box
// art sources' names, each shelf's looks and the themes' pictures.
//   node scripts/export-systems.js           write the files
//   node scripts/export-systems.js --check   exit 1 when they're out of date
'use strict';
const fs = require('fs'), path = require('path'), { execFileSync } = require('child_process');

const ROOT = path.join(__dirname, '..'), OUT = path.join(ROOT, 'crates', 'rustyshelf-core', 'data');
const dump = platform => execFileSync(process.execPath, ['-e', `
  Object.defineProperty(process, 'platform', { value: ${JSON.stringify(platform)} });
  const s = require(${JSON.stringify(path.join(ROOT, 'main', 'systems.js'))});
  process.stdout.write(JSON.stringify({ consoles: s.CONSOLES, apps: s.APPS, searchRoots: s.SEARCH_ROOTS, romRootNames: s.ROM_ROOT_NAMES }, null, 1) + '\\n');
`], { encoding: 'utf8' });

// the box art sources' names for each console (main/covers.js): libretro's thumbnail folders, LaunchBox's platforms, and
// the platforms a PC game's console box is looked up in
const artSources = () => {
  const covers = require(path.join(ROOT, 'main', 'covers.js')), src = fs.readFileSync(path.join(ROOT, 'main', 'covers.js'), 'utf8');
  const libretro = new Function('return ' + /const LIBRETRO = (\{[\s\S]*?\});\n/.exec(src)[1])();
  return JSON.stringify({ libretro, launchbox: covers.LAUNCHBOX, pcBoxPlats: covers.PC_BOX_PLATS }, null, 1) + '\n';
};

// each shelf's case and colours (renderer/app.js STYLE and WORDMARKS, with the extra shelves of renderer/systems-extra.js)
const looks = () => {
  const app = fs.readFileSync(path.join(ROOT, 'renderer', 'app.js'), 'utf8');
  const STYLE = new Function('return ' + /\nconst STYLE=(\{[\s\S]*?\n\});\n/.exec(app)[1])();
  const WORDMARKS = new Function('return ' + /\nconst WORDMARKS=(\{.*?\});\n/.exec(app)[1])();
  const win = {}; new Function('window', fs.readFileSync(path.join(ROOT, 'renderer', 'systems-extra.js'), 'utf8'))(win);
  STYLE.xsx = STYLE.xboxone; STYLE.xone = STYLE.xboxone; STYLE.all = STYLE.home; WORDMARKS.all = ['All Games', 'Whole library'];
  Object.assign(STYLE, win.BS_SYSTEMS_EXTRA.style || {}); Object.assign(WORDMARKS, win.BS_SYSTEMS_EXTRA.wordmarks || {});
  return JSON.stringify({ style: STYLE, wordmarks: WORDMARKS }, null, 1) + '\n';
};

// the pictures and patterns the extra themes' backdrops are made of (renderer/themes-extra.js): they never change, so
// the Rust app takes them as they are and only writes the colours round them
const themes = () => {
  const src = fs.readFileSync(path.join(ROOT, 'renderer', 'themes-extra.js'), 'utf8');
  const hooked = src.replace('window.BS_EXTRA_THEMES = [', 'window.BS_THEME_PARTS = { CUBE, BLUE, SWIRL, FOREST, CRT_BEZEL, VHS_NOISE, SYN_GRID, SYN_SUN };\n  window.BS_EXTRA_THEMES = [');
  if (hooked === src) throw new Error('themes-extra.js: the theme list was not found');
  const win = {}; new Function('window', hooked)(win);
  return JSON.stringify({ parts: win.BS_THEME_PARTS, extra: win.BS_EXTRA_THEMES.map(t => ({ id: t.id, name: t.name, moves: typeof t.fx === 'function' })) }, null, 1) + '\n';
};

let stale = false;
for (const [platform, name] of [['win32', 'systems-windows.json'], ['linux', 'systems-linux.json'], ['', 'art-sources.json'], ['', 'looks.json'], ['', 'themes.json']]) {
  const file = path.join(OUT, name), text = platform ? dump(platform) : name === 'looks.json' ? looks() : name === 'themes.json' ? themes() : artSources();
  const old = fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : '';
  if (old === text) continue;
  if (process.argv.includes('--check')) { console.error(`${name} is out of date: run node scripts/export-systems.js`); stale = true; continue; }
  fs.mkdirSync(OUT, { recursive: true }); fs.writeFileSync(file, text); console.log('wrote', path.relative(ROOT, file));
}
process.exit(stale ? 1 : 0);
