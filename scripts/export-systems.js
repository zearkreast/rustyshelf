#!/usr/bin/env node
// Writes main/systems.js's consoles and apps as JSON for the Rust app (crates/rustyshelf-core/data), once for Windows
// and once for Linux (systems.js rewrites its paths and programs for Linux when it's loaded there); with them the box
// art sources' names and each shelf's looks.
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

let stale = false;
for (const [platform, name] of [['win32', 'systems-windows.json'], ['linux', 'systems-linux.json'], ['', 'art-sources.json'], ['', 'looks.json']]) {
  const file = path.join(OUT, name), text = platform ? dump(platform) : name === 'looks.json' ? looks() : artSources();
  const old = fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : '';
  if (old === text) continue;
  if (process.argv.includes('--check')) { console.error(`${name} is out of date: run node scripts/export-systems.js`); stale = true; continue; }
  fs.mkdirSync(OUT, { recursive: true }); fs.writeFileSync(file, text); console.log('wrote', path.relative(ROOT, file));
}
process.exit(stale ? 1 : 0);
