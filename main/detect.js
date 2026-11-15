// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): this file is the same as Boxshelf's
// main/detect.js apart from the app's name (RustyShelf).
// Auto-detect emulators, ROM folders, Steam and media apps.
const path = require('path');
const { execFile } = require('child_process');
const { CONSOLES, APPS, SEARCH_ROOTS, ROM_ROOT_NAMES } = require('./systems');
const { expand, isFile, isDir, readDir, findFile, appRoot } = require('./paths');
const vita3k = require('./vita3k');

// (1.82) Each folder searched is listed once, two folders deep, and every console and app looks its program names up in
// that list: the same answer as findFile (the first match, breadth first), without walking the Desktop, Downloads and
// Documents again for each of a dozen consoles while the setup window waits.
// (RustyShelf) maxDepth / maxDirs: a folder the player picks (Find emulators in a folder) is looked through deeper, but
// never more than maxDirs folders (a whole drive picked by mistake stays quick)
function makeIndex(maxDepth = 2, maxDirs = Infinity) {
  const cache = new Map();
  return dir => {
    const key = dir.toLowerCase(); let m = cache.get(key); if (m) return m;
    m = new Map(); let seq = 0, dirs = 0; const queue = [[dir, 0]];
    while (queue.length && dirs++ < maxDirs) {
      const [d, depth] = queue.shift();
      for (const ent of readDir(d)) {
        const full = path.join(d, ent.name);
        if (ent.isFile()) { const k = ent.name.toLowerCase(); if (!m.has(k)) m.set(k, { full, seq: seq++ }); }
        else if (ent.isDirectory() && depth < maxDepth && !ent.name.startsWith('.') && !SKIP_DIRS.test(ent.name)) queue.push([full, depth + 1]);
      }
    }
    cache.set(key, m); return m;
  };
}
const SKIP_DIRS = /^(\$recycle\.bin|system volume information|windows|node_modules)$/i;
// (RustyShelf) BizHawk covers many consoles: in a folder with another emulator for the console too, that one wins
const LATE = new Set(['emuhawk.exe']);
function findIn(index, dir, exeNames) {
  const late = exeNames.filter(n => LATE.has(n.toLowerCase()));
  if (late.length && late.length < exeNames.length) return findIn(index, dir, exeNames.filter(n => !LATE.has(n.toLowerCase()))) || findIn(index, dir, late);
  const m = index(dir); let best = null;
  for (const n of exeNames) {
    if (n.startsWith('re:')) { // (Linux) a pattern, for versioned AppImage names: "re:^duckstation.*\.appimage$"
      const re = new RegExp(n.slice(3), 'i');
      for (const [k, h] of m) if (re.test(k) && (!best || h.seq < best.seq)) best = h;
      continue; }
    const h = m.get(n.toLowerCase()); if (h && (!best || h.seq < best.seq)) best = h; }
  return best ? best.full : null;
}
// (Linux) SD cards and USB drives: /run/media/<user>/<card> (SteamOS, most desktops), /run/media/mmcblk0p1 (older SteamOS),
// /media/<user>/<card> (Ubuntu). Each card, and its Emulation and retrodeck folders, is searched like the home folder.
function mediaRoots() {
  if (process.platform !== 'linux') return [];
  const out = [], user = require('os').userInfo().username;
  for (const base of ['/run/media/' + user, '/run/media', '/media/' + user]) {
    for (const ent of readDir(base)) { if (!ent.isDirectory() || ent.name === user) continue; const d = path.join(base, ent.name); out.push(d, path.join(d, 'Emulation'), path.join(d, 'retrodeck')); }
  }
  return [...new Set(out)];
}
const searchRoots = () => [...SEARCH_ROOTS.map(expand), ...mediaRoots()];

function findExe(folders, exeNames, index = makeIndex()) {
  for (const f of folders) {
    const dir = expand(f);
    if (!isDir(dir)) continue;
    const hit = findIn(index, dir, exeNames);
    if (hit) return hit;
  }
  // Portable copies in common places (e.g. Desktop\Emulators\Dolphin-x64\Dolphin.exe)
  for (const root of searchRoots()) {
    if (!isDir(root)) continue;
    const hit = findIn(index, root, exeNames);
    if (hit) return hit;
  }
  return null;
}

// (RustyShelf, its discussions #10) every copy of a console's emulators found, not only the first: Settings lists them so
// another one (Flycast for the Dreamcast, Gopher64 for the N64) is one press away. At most 8, in the order they're found.
function findAllExe(folders, exeNames, index = makeIndex()) {
  const out = [], add = h => { if (h && !out.some(x => x.toLowerCase() === h.toLowerCase())) out.push(h); };
  for (const dir of [...folders.map(expand), ...searchRoots()]) { if (out.length >= 8 || !isDir(dir)) continue; for (const n of exeNames) add(findIn(index, dir, [n])); }
  return out.slice(0, 8);
}
// Look for "<root>\ROMS\<system>" style folders.
function findRomDirs(sys) {
  const found = new Set();
  const roots = [appRoot(), ...searchRoots()];
  for (const root of roots) {
    if (!isDir(root)) continue;
    const candidates = [root];
    for (const ent of readDir(root)) {
      if (ent.isDirectory() && ROM_ROOT_NAMES.includes(ent.name.toLowerCase())) candidates.push(path.join(root, ent.name));
    }
    for (const c of candidates) {
      for (const ent of readDir(c)) {
        if (ent.isDirectory() && sys.romFolderNames.includes(ent.name.toLowerCase())) found.add(path.join(c, ent.name));
      }
    }
  }
  return [...found];
}

function regQuery(key, value) {
  return new Promise(resolve => {
    if (process.platform !== 'win32') return resolve(null);
    execFile('reg', ['query', key, '/v', value], { windowsHide: true }, (err, out) => {
      if (err) return resolve(null);
      const m = String(out).match(new RegExp(value + '\\s+REG_\\w+\\s+(.+)', 'i'));
      resolve(m ? m[1].trim() : null);
    });
  });
}

async function findSteam() {
  // (Linux) Steam's own folder (the one with steamapps): native, the Steam Deck's, or the Flatpak's
  if (process.platform === 'linux') {
    for (const p of ['%HOME%/.steam/steam', '%HOME%/.local/share/Steam', '%HOME%/.var/app/com.valvesoftware.Steam/.local/share/Steam'].map(expand))
      if (isDir(path.join(p, 'steamapps'))) { try { return require('fs').realpathSync(p); } catch { return p; } }
    return null;
  }
  const reg = await regQuery('HKCU\\Software\\Valve\\Steam', 'SteamPath');
  const options = [reg && reg.replace(/\//g, '\\'), expand('%ProgramFiles(x86)%\\Steam'), expand('%ProgramFiles%\\Steam'), 'C:\\Steam', 'D:\\Steam'];
  for (const p of options) if (p && isFile(path.join(p, 'steam.exe'))) return p;
  return null;
}

// (Haseosama's fork) the drives there are (Windows), for the search to ask which to look through
function listDrives() {
  if (process.platform !== 'win32') return [];
  const out = [], sys = String(process.env.SystemDrive || 'C:').replace(':', '').toUpperCase();
  for (let c = 65; c <= 90; c++) {
    const l = String.fromCharCode(c);
    try { require('fs').readdirSync(l + ':\\'); out.push({ letter: l, path: l + ':\\', system: l === sys }); } catch {}
  }
  return out;
}

// (Haseosama's fork) the drives chosen, looked through five folders deep, nearest the top first: emulators by their program's
// name, and games folders by a console's folder name, only somewhere a folder above says roms / games / emulation (a
// "psx" folder anywhere else is someone's photos). Each drive stops after 400,000 folders or three minutes.
const DRIVE_SKIP = new Set(['windows', '$recycle.bin', 'system volume information', 'node_modules', 'appdata', 'programdata', 'winsxs', 'recovery', 'perflogs', '$windows.~bt', 'msocache', '.git']);
const ROMISH = ['rom', 'game', 'iso', 'emul', 'retro', 'batocera', 'recalbox', 'jeux'];
async function scanDrives(drives) {
  const names = new Map(), pats = [], romNames = new Map();
  const add = (m, k, id) => { k = k.toLowerCase(); if (!m.has(k)) m.set(k, []); m.get(k).push(id); };
  for (const s of CONSOLES) {
    for (const n of s.exeNames || []) { if (!n.startsWith('re:')) add(names, n, s.id); else try { pats.push([new RegExp(n.slice(3), 'i'), s.id]); } catch {} }
    for (const n of s.romFolderNames || []) add(romNames, n, s.id);
  }
  const systems = {}, get = id => (systems[id] = systems[id] || { emulator: null, romDirs: [] });
  let steps = 0;
  for (const dr of drives || []) {
    const root = String(dr || '').replace(/[\\/]*$/, '') + '\\';
    if (!/^[A-Za-z]:\\$/.test(root) || !isDir(root)) continue;
    const queue = [[root, 0, false]], t0 = Date.now();
    let head = 0;
    while (head < queue.length && head < 400000 && Date.now() - t0 < 180000) {
      const [d, depth, romish] = queue[head]; queue[head++] = null;
      if (++steps % 300 === 0) await new Promise(r => setImmediate(r));
      for (const ent of readDir(d)) {
        const full = path.join(d, ent.name), low = ent.name.toLowerCase();
        if (ent.isFile()) {
          for (const id of names.get(low) || []) { const e = get(id); if (!e.emulator) e.emulator = full; }
          if (low.endsWith('.exe')) for (const [re, id] of pats) if (re.test(low)) { const e = get(id); if (!e.emulator) e.emulator = full; }
        } else if (ent.isDirectory() && !DRIVE_SKIP.has(low) && !low.startsWith('$')) {
          const ids = romNames.get(low);
          if (ids && romish) { for (const id of ids) { const e = get(id); if (!e.romDirs.includes(full)) e.romDirs.push(full); } continue; }
          if (depth < 5) queue.push([full, depth + 1, romish || ROMISH.some(w => low.includes(w))]);
        }
      }
    }
  }
  return systems;
}

async function detectAll(drives) {
  const systems = {}, index = makeIndex();
  let foundRetroarch = '';
  for (const s of CONSOLES) {
    const emu = findExe(s.emulatorFolders, s.exeNames, index);
    if (!foundRetroarch && s.retro && emu && /retroarch/i.test(path.basename(emu))) foundRetroarch = emu;
    systems[s.id] = { emulator: emu, emulators: emu ? findAllExe(s.emulatorFolders, s.exeNames, index) : [], romDirs: withOwnDirs(s, emu, findRomDirs(s)) };
  }
  if (Array.isArray(drives) && drives.length) {
    for (const [id, e] of Object.entries(await scanDrives(drives))) {
      const s = systems[id]; if (!s) continue;
      if (e.emulator && !s.emulators.some(x => x.toLowerCase() === e.emulator.toLowerCase())) s.emulators.push(e.emulator);
      if (!s.emulator && e.emulator) s.emulator = e.emulator;
      for (const d of e.romDirs) if (!s.romDirs.some(x => x.toLowerCase() === d.toLowerCase())) s.romDirs.push(d);
    }
  }
  const apps = {};
  for (const a of APPS) apps[a.id] = { path: findExe(a.folders, a.exeNames, index) };
  return { systems, apps, steam: { path: await findSteam() }, appRoot: appRoot(), retroarch: foundRetroarch };
}

// (RustyShelf) games an emulator keeps in its own folders: Vita3K's installed games (ux0\app)
function withOwnDirs(s, emu, dirs) { if (s.id === 'psvita') for (const d of vita3k.appDirs(emu)) if (!dirs.includes(d)) dirs.push(d); return dirs; }
// one console's emulator and games folders (a console added in an update, found once as setup would have)
function detectSystem(id) { const s = CONSOLES.find(c => c.id === id); if (!s) return null; const index = makeIndex(), emu = findExe(s.emulatorFolders, s.exeNames, index); return { emulator: emu, emulators: emu ? findAllExe(s.emulatorFolders, s.exeNames, index) : [], romDirs: withOwnDirs(s, emu, findRomDirs(s)) }; }

// (RustyShelf, asked for in Boxshelf's discussions) portable emulators kept anywhere: the emulator programs in a folder
// the player picks (four folders deep), per console: { systems: { <console id>: program } }
function emulatorsIn(dir) {
  const systems = {}; if (!dir || !isDir(dir)) return { systems, retroarch: '' };
  const index = makeIndex(4, 4000);
  let retroarch = '';
  for (const s of CONSOLES) {
    if (s.standalone || !(s.exeNames || []).length) continue;
    const hit = findIn(index, dir, s.exeNames);
    if (hit) {
      systems[s.id] = hit;
      if (!retroarch && s.retro && /retroarch/i.test(path.basename(hit))) retroarch = hit;
    }
  }
  return { systems, retroarch };
}

// (Haseosama's fork) a folder with one folder of games per console (RetroBat's roms: gc, wii, ps2 …), up to three folders
// down: { systems: { <console id>: [folders] } }. A console's folder isn't looked into.
function romFoldersIn(dir) {
  const systems = {}; if (!dir || !isDir(dir)) return { systems };
  const byName = new Map();
  for (const s of CONSOLES) for (const n of s.romFolderNames || []) { const k = n.toLowerCase(); if (!byName.has(k)) byName.set(k, []); byName.get(k).push(s.id); }
  const queue = [[dir, 0]];
  let seen = 0;
  while (queue.length && seen++ < 4000) {
    const [d, depth] = queue.shift();
    for (const ent of readDir(d)) {
      if (!ent.isDirectory() || ent.name.startsWith('.')) continue;
      const full = path.join(d, ent.name), ids = byName.get(ent.name.toLowerCase());
      if (ids) { for (const id of ids) { const l = systems[id] = systems[id] || []; if (!l.includes(full)) l.push(full); } continue; }
      if (depth < 2 && !SKIP_DIRS.test(ent.name)) queue.push([full, depth + 1]);
    }
  }
  return { systems };
}

module.exports = { detectAll, detectSystem, findSteam, emulatorsIn, findAllExe, romFoldersIn, listDrives };
