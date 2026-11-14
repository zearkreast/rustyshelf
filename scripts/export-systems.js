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
  // each shelf's lettering colours (the .wm-<id> rules of the renderer's style sheets; "" is a shelf without its own)
  const wm = {};
  for (const f of fs.readdirSync(path.join(ROOT, 'renderer')).filter(f => f.endsWith('.css')).sort()) {
    const css = fs.readFileSync(path.join(ROOT, 'renderer', f), 'utf8');
    for (const m of css.matchAll(/(?:^|\n|\})((?:\.wm(?:-[a-z0-9]+)?,?)+)\{--wm-grad:(linear-gradient\([^;]*\));--wm-glow:([^;]*);--wm-tagc:([^;}]*)\}/g))
      for (const sel of m[1].split(',')) wm[sel.replace(/^\.wm-?/, '')] = { grad: m[2], glow: m[3], tag: m[4] };
  }
  // and its own letters, where it has them (.wm-<id> .wm-main): the font, how far apart, and how large for the window
  // ("size": least, hundredths of the window's width, most); the rest are set as .sys-word sets them
  const plain = { font: 'Lexend', weight: 800, spacing: 0, size: [22, 2.6, 38], upper: true, italic: false };
  for (const id of Object.keys(wm)) Object.assign(wm[id], plain);
  for (const f of fs.readdirSync(path.join(ROOT, 'renderer')).filter(f => f.endsWith('.css') && f !== 'bootslide.css').sort()) {
    const css = fs.readFileSync(path.join(ROOT, 'renderer', f), 'utf8');
    for (const m of css.matchAll(/(?:^|\n|\})((?:\.wm-[a-z0-9]+ \.wm-main,?)+)\{([^}]*)\}/g)) {
      const d = Object.fromEntries(m[2].split(';').filter(Boolean).map(x => [x.slice(0, x.indexOf(':')).trim(), x.slice(x.indexOf(':') + 1).trim()]));
      const size = /clamp\(([\d.]+)px,([\d.]+)vw,([\d.]+)px\)/.exec(d['font-size'] || '');
      for (const sel of m[1].split(',')) {
        const id = /\.wm-([a-z0-9]+)/.exec(sel)[1]; wm[id] = wm[id] || { ...wm[''] };
        if (d['font-family']) wm[id].font = /'([^']+)'/.exec(d['font-family'])[1];
        if (d['font-weight']) wm[id].weight = +d['font-weight'];
        if (d['letter-spacing']) wm[id].spacing = parseFloat(d['letter-spacing']);
        if (size) wm[id].size = [+size[1], +size[2], +size[3]];
        if (d['text-transform'] === 'none') wm[id].upper = false;
        if (d['font-style'] === 'italic') wm[id].italic = true;
      }
    }
  }
  // the boot slide's colours for the consoles that have their own (renderer/bootslide.css .ins-slide.bs-<id>, and the
  // gradient their name is lettered in there); "" is what the rest start from
  const bs = fs.readFileSync(path.join(ROOT, 'renderer', 'bootslide.css'), 'utf8'), boot = {};
  const vars = text => Object.fromEntries([...text.matchAll(/--bs-([a-z-]+):((?:[^;()}]|\([^()]*(?:\([^()]*\)[^()]*)*\))*)/g)].map(m => [m[1], m[2].trim()]));
  boot[''] = vars(/\.ins-slide\{[^}]*\}/.exec(bs)[0]);
  for (const m of bs.matchAll(/\.ins-slide\.bs-([a-z0-9]+)\{([^}]*)\}/g)) boot[m[1]] = { ...(boot[m[1]] || {}), ...vars(m[2]) };
  for (const m of bs.matchAll(/\.bs-([a-z0-9]+) \.bs-word \.wm-main\{background:([^;}]*(?:\([^)]*\))?[^;}]*)/g)) if (boot[m[1]]) boot[m[1]].word = m[2].trim();
  return JSON.stringify({ style: STYLE, wordmarks: WORDMARKS, lettering: wm, boot }, null, 1) + '\n';
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
