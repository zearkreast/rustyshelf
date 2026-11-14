#!/usr/bin/env node
// What the Electron app makes of game names and games folders, as JSON for the Rust port's tests
// (crates/rustyshelf-core/tests/fixtures): the Rust code must give the same answers.
//   node scripts/rust-fixtures.js
'use strict';
const fs = require('fs'), os = require('os'), path = require('path');

const ROOT = path.join(__dirname, '..'), OUT = path.join(ROOT, 'crates', 'rustyshelf-core', 'tests', 'fixtures');
const Titles = require('../shared/titles');

const NAMES = [
  'Legend of Zelda, The - The Wind Waker (USA).rvz', 'Super Mario Sunshine (Europe) (En,Fr,De,Es,It).iso',
  'Final Fantasy VII (USA) (Disc 2).chd', 'Metal Gear Solid [DVD2].iso', 'Gran Turismo 2 (CD 1).bin',
  'Sonic Adventure 2 (Rev 1).gdi', 'Mario Kart Wii (Japan, Korea).wbfs', 'Some Game (en,ja).iso',
  'Pokemon Sword [0100ABF008968000][v0].nsp', 'Pokemon Sword [0100ABF008968800][v65536].nsp',
  'Zelda BOTW Update v1.6.0.nsp', 'Hall Pass.iso', 'Some Booster Pack.nsp', 'Game (UPD).xci', 'Game [DLC].iso',
  'Mario_Party_8_(USA).iso', 'Halo 3 - Copy (2).iso', 'Halo 3 - Copy.iso', 'www.romsite.com - Tekken 3.zip',
  'Metroid Prime.nkit.iso', 'Metroid Prime.nkit.rvz', 'Metroid Prime (nkit).iso', 'Game.part1.rar',
  'SXS-Cool Game Switch NSP eShop.nsp', 'Multi5 Fighter.nsp', 'A Boy and His Blob, A.zip', 'Hobbit, An.iso',
  'Simpsons, The - Hit & Run (USA).iso', 'Ape Escape (USA) (v1.1).bin', 'Rayman 2 v1.0.0.zip', '.hidden',
  'noext', 'name.', 'Tetris & Dr. Mario (USA).sfc', '[BIOS] PS1 (USA).bin', 'Crash Bandicoot - Warped (Europe) (Rev 2).cue',
  'C:\\Games\\Wii\\Xenoblade Chronicles (Europe).wbfs', '/home/me/roms/gba/Advance Wars (USA, Europe).gba',
  'Game - DLC Pack.nsp', 'DLC Quest.iso', 'Pikmin 2 [GPVE01].rvz', 'Ōkami (Japan).iso', 'Final Fantasy X-2 (USA).iso',
];

const titles = [];
for (const n of NAMES) for (const updateWords of [true, false]) {
  const t = Titles.cleanTitle(n, { updateWords });
  titles.push({ name: n, updateWords, title: t.title, region: t.region, disc: t.disc, rev: t.rev, titleId: t.titleId || null, extra: !!t.extra });
}

// The binary headers the scanner reads, as [offset, base64] patches over a zero-filled file. Every title, id and name
// below is made up.
const b64 = b => Buffer.from(b).toString('base64');
const be32 = n => { const b = Buffer.alloc(4); b.writeUInt32BE(n >>> 0); return b; };
function sfo(entries) { // param.sfo: [[key, text]]
  const keys = [], data = [], table = [];
  let ko = 0, dof = 0;
  for (const [k, v] of entries) {
    const kb = Buffer.from(k + '\0'), vb = Buffer.from(v + '\0'), e = Buffer.alloc(16);
    e.writeUInt16LE(ko, 0); e.writeUInt16LE(0x0204, 2); e.writeUInt32LE(vb.length, 4); e.writeUInt32LE(vb.length, 8); e.writeUInt32LE(dof, 12);
    keys.push(kb); data.push(vb); table.push(e); ko += kb.length; dof += vb.length;
  }
  const h = Buffer.alloc(20), keyTab = 20 + 16 * entries.length;
  h.writeUInt32LE(0x46535000, 0); h.writeUInt32LE(0x0101, 4); h.writeUInt32LE(keyTab, 8); h.writeUInt32LE(keyTab + ko, 12); h.writeUInt32LE(entries.length, 16);
  return Buffer.concat([h, ...table, ...keys, ...data]);
}
function xex(id) { const b = Buffer.alloc(0x40); b.write('XEX2', 0, 'latin1'); b.writeUInt32BE(0x40, 8); b.writeUInt32BE(1, 0x14);
  b.writeUInt32BE(0x00040006, 0x18); b.writeUInt32BE(0x20, 0x1C); b.writeUInt32BE(id, 0x2C); return b; }
// a disc image: the volume descriptor at 0x10000, a root table naming default.xex in the next sector, the xex after it
function iso(id) {
  const root = 0x10000 / 2048 + 1, vol = Buffer.alloc(28), ent = Buffer.alloc(14 + 11);
  vol.write('MICROSOFT*XBOX*MEDIA', 0, 'latin1'); vol.writeUInt32LE(root, 20); vol.writeUInt32LE(2048, 24);
  ent.writeUInt32LE(root + 1, 4); ent.writeUInt32LE(0x40, 8); ent[13] = 11; ent.write('default.xex', 14, 'latin1');
  return { size: (root + 2) * 2048, at: [[0x10000, b64(vol)], [root * 2048, b64(ent)], [(root + 1) * 2048, b64(xex(id))]] };
}
function stfs(type, id, name) { const b = Buffer.alloc(0x500); b.write('LIVE', 0, 'latin1'); b.writeUInt32BE(type, 0x344); b.writeUInt32BE(id, 0x360);
  const n = Buffer.from(name, 'utf16le'); n.swap16(); n.copy(b, 0x411); return { size: 0x1712, at: [[0, b64(b)]] }; }
function npdm(id) { const b = Buffer.alloc(0x100); b.write('META', 0, 'latin1'); b.writeUInt32LE(0x80, 0x70); b.write('ACI0', 0x80, 'latin1');
  b.writeBigUInt64LE(BigInt(id), 0x90); return b; }
function pkg(sfoBuf) { const h = Buffer.alloc(0x60); h.writeUInt32BE(0x7F434E54, 0); h.writeUInt32BE(1, 0x10); h.writeUInt32BE(0x20, 0x18);
  h.writeUInt32BE(0x1000, 0x20); h.writeUInt32BE(0x60, 0x30); h.writeUInt32BE(sfoBuf.length, 0x34); return Buffer.concat([h, sfoBuf]); }
const bytes = buf => ({ size: buf.length, at: [[0, b64(buf)]] });
const text = t => bytes(Buffer.from(t));

// what the Electron app downloads into its data folder, and the folder sizes it measured (relative to the games folder)
const DATA = {
  'x360-titles.json': { at: 0, titles: { '41510001': { title: 'Alpha Quest', state: 'playable' }, '41510002': { title: 'Alpha Quest Bonus Disc', state: 'ingame' },
    '41510004': { title: 'Iota Puzzle', state: '' }, '41510006': { title: 'Epsilon Drift™', state: 'playable' }, '41510007': { title: 'Omega Pack', state: 'playable' },
    '41510008': { title: 'Omega Pack Bonus', state: 'ingame' } } },
  'arcade-names.json': { v: 1, at: 0, names: { omif: 'Omicron Fighters (World 910522)', omifu: 'Omicron Fighters (USA 910206)', piblast: 'Pi Blaster / Pi Blaster: Alt' }, bios: ['rhobios'] },
};
const DIR_SIZES = { 'SR ROM': 123456789 };

// games folders: [console, [[file, size or { size, at: [[offset, base64]] }]], art files]
const TREES = [
  ['gc', [['Metroid Prime (USA).iso', 100], ['Metroid Prime (USA).rvz', 50], ['Tales of Symphonia (USA) (Disc 1).rvz', 10], ['Tales of Symphonia (USA) (Disc 2).rvz', 10],
    ['Tales of Symphonia (USA) (Disc 2).iso', 30], ['Pikmin 2 (Europe).rvz', 5], ['Pikmin 2 (USA).rvz', 5], ['RPG/Skies of Arcadia Legends (USA).gcz', 7],
    ['a/b/c/d/e/Too Deep (USA).iso', 1], ['a/b/c/d/Deep Enough (USA).iso', 1], ['.hidden/Hidden Game.iso', 1], ['updates/Patch.iso', 1], ['notes.txt', 1],
    ['Legend of Zelda, The - The Wind Waker (USA).rvz', 9]],
    ['covers/Metroid Prime (USA).png', 'covers/Metroid Prime (USA)-back.jpg', 'covers/Metroid Prime (USA)-spinemade.png',
      'covers/Tales of Symphonia.png', 'covers/Pikmin 2 (USA)-manual-02.jpg', 'covers/Pikmin 2 (USA)-manual-10.jpg', 'covers/Pikmin 2 (USA)-manual-1.jpg',
      'media/covers/Skies of Arcadia Legends (USA)-manual.pdf', 'boxart/The Legend of Zelda - The Wind Waker-disc.webp']],
  ['sw', [['Pokemon Sword [0100ABF008968000][v0].nsp', 20], ['Pokemon Sword [0100ABF008968800][v65536].nsp', 2], ['Zelda Update v1.6.0.nsp', 1],
    ['Hall Pass.xci', 3], ['Game [UPD].nsp', 1], ['DLC/Extra.nsp', 1]], []],
  ['psx', [['Final Fantasy VII (USA) (Disc 1).chd', 3], ['Final Fantasy VII (USA) (Disc 2).chd', 3], ['Final Fantasy VII (USA) (Disc 3).chd', 3],
    ['Ape Escape (USA).cue', 1], ['Mario 10.chd', 1], ['Mario 2.chd', 1], ['mario 1.chd', 1]], []],
  // disc images named by their title ids; a disc 2 joins its disc 1 by the file name, or in the same folder
  // (games whose file names say nothing, "DVD1.iso", are a tree of their own: a disc 2 joins the last of them)
  ['x360', [['Alpha Quest/DVD1.iso', iso(0x41510001)], ['Alpha Quest/DVD2.iso', iso(0x41510002)], ['Omega Pack/Omega Pack.iso', iso(0x41510007)],
    ['Omega Pack/Omega Pack Bonus (Disc 2).iso', iso(0x41510008)], ['Gamma Run (USA).iso', 4], ['Kappa Saga (Disc 2).iso', 2], ['Kappa Saga (Disc 1).iso', 2],
    ['Lone Extra (Disc 2).iso', 2]],
    ['covers/Alpha Quest.png']],
  // extracted games (default.xex, its helpers and data beside it), generic disc names, a generic title id
  ['x360', [['Epsilon/default.xex', bytes(xex(0x41510006))], ['Epsilon/helper.xex', 1], ['Epsilon/sub/Other Game.iso', 1],
    ['Delta Force Lite/default.xex', bytes(xex(0xFFED2000))], ['Zeta/DVD.iso', iso(0x41510009)], ['DVD.iso', iso(0x4151000A)]],
    ['covers/Epsilon.png']],
  // Games on Demand / Arcade packages: no extension, in a content-type folder; a save and a package's data folder aren't games
  ['x360', [['Content/0000000000000000/41510003/00007000/0123456789ABCDEF0123456789ABCDEF01234567', stfs(0x7000, 0x41510003, 'Theta  Saga')],
    ['Content/0000000000000000/41510004/000D0000/89ABCDEF0123456789ABCDEF0123456789ABCDEF', stfs(0xD0000, 0x41510004, '')],
    ['Content/0000000000000000/41510005/000D0000/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA', stfs(0xD0000, 0x41510005, '')],
    ['Content/0000000000000000/41510003/00000001/BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB', stfs(0x1, 0x41510003, 'Theta Save')],
    ['Content/0000000000000000/41510003/00007000/0123456789ABCDEF0123456789ABCDEF01234567.data/Data0000.iso', 1],
    ['Content/0000000000000000/41510003/00007000/notes', 1]], []],
  // unpacked games (base + updates, versions only), a packed copy inside one, packed games
  ['sw2', [['SR ROM/base/exefs/main', 1], ['SR ROM/base/exefs/main.npdm', bytes(npdm(0x0400C3F00006E000n))], ['SR ROM/updates/131072/exefs/main', 1],
    ['SR ROM/updates/131072/exefs/main.npdm', 1], ['SR ROM/base/game.nsp', 5], ['Group/OtherGame_v1.0.2 [BASE]/200/exefs/main', 1],
    ['Group/OtherGame_v1.0.2 [BASE]/200/exefs/main.npdm', 1], ['Group/OtherGame_v1.0.2 [BASE]/30/exefs/main', 1], ['Group/OtherGame_v1.0.2 [BASE]/30/exefs/main.npdm', 1],
    ['Packed.Thing_v1.0.0 [0100000000001000].nsp', 7], ['Upsilon [0400C3F00006E000].xci', 3], ['Upsilon Update.nsp', 1]],
    ['covers/SR ROM.png']],
  // installed folders and .pkg packages: one installed already, a patch, and two only as packages
  ['ps4', [['Lambda Game/eboot.bin', 1], ['Lambda Game/sce_sys/param.sfo', bytes(sfo([['TITLE', 'Lambda  Game™'], ['TITLE_ID', 'CUSA99901'], ['CATEGORY', 'gd']]))],
    ['Lambda Game/sce_sys/icon0.png', 1], ['Lambda Game/sce_sys/pic1.png', 1],
    ['pkgs/Lambda.Game_CUSA99901_v1.00.pkg', 3], ['pkgs/[Site]-Mu.Racer_CUSA99902_v1.00_UP.pkg', 4],
    ['pkgs/nu.pkg', bytes(pkg(sfo([['TITLE', 'Nu Story'], ['TITLE_ID', 'CUSA99903'], ['CATEGORY', 'gd']])))],
    ['pkgs/nu-patch.pkg', bytes(pkg(sfo([['TITLE', 'Nu Story'], ['TITLE_ID', 'CUSA99903'], ['CATEGORY', 'gp']])))]], []],
  ['ps5', [['Xi Game/eboot.bin', 1], ['Xi Game/sce_sys/param.json', text(JSON.stringify({ titleId: 'PPSA99901', localizedParameters: { defaultLanguage: 'fr-FR', 'en-US': { titleName: 'Xi  Game' } } }))],
    ['Xi Game/sce_sys/pic0.png', 1], ['PPSA99902/eboot.bin', 1], ['PPSA99902/sce_sys/param.json', text('{')]], []],
  // set names from the arcade list; BIOS sets, from the list or without it
  ['arcade', [['omif.zip', 10], ['omifu.zip', 20], ['piblast.zip', 3], ['rhobios.zip', 1], ['neogeo.zip', 1], ['sigmaset.zip', 2]], []],
  // a gamelist.xml (Batocera, RetroBat): its names and pictures where there's no art of RustyShelf's own
  ['snes', [['Sigma (USA).sfc', 1], ['sub/Tau.sfc', 1], ['Phi (USA).sfc', 1],
    ['gamelist.xml', text('<gameList>\n <game><path>./Sigma (USA).sfc</path><name>Sigma &amp; Friends</name><thumbnail>./images/sigma.png</thumbnail>' +
      '<boxback>./images/none.png</boxback><fanart>./images/sigma-fan.png</fanart></game>\n <game id="2"><path>./sub/Tau.sfc</path><name>Tau™  Deluxe</name>' +
      '<image>./images/tau.png</image><manual>./manuals/tau.pdf</manual></game>\n <game><path>./Phi (USA).sfc</path><name>Phi Again</name>' +
      '<thumbnail>./images/phi.png</thumbnail></game>\n</gameList>\n')]],
    ['images/sigma.png', 'images/sigma-fan.png', 'images/tau.png', 'images/phi.png', 'manuals/tau.pdf', 'covers/Phi (USA).png']],
];

function writeFile(p, spec) {
  fs.mkdirSync(path.dirname(p), { recursive: true });
  if (typeof spec === 'number') return fs.writeFileSync(p, Buffer.alloc(spec));
  const b = Buffer.alloc(spec.size); for (const [at, data] of spec.at) Buffer.from(data, 'base64').copy(b, at); fs.writeFileSync(p, b);
}

function scanFixtures() {
  const { CONSOLES } = require('../main/systems'), scanner = require('../main/scanner');
  const tmps = TREES.map(() => fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-')));
  // the data folder is read once per run, so it holds every tree's folder sizes
  const data = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-data-')), sizes = {};
  for (const tmp of tmps) for (const [rel, bytes] of Object.entries(DIR_SIZES)) sizes[path.resolve(tmp, 'games', rel)] = { bytes, mtime: 0, at: 0 };
  for (const [f, j] of Object.entries(DATA)) fs.writeFileSync(path.join(data, f), JSON.stringify(j));
  fs.writeFileSync(path.join(data, 'dir-sizes.json'), JSON.stringify(sizes));
  require('../main/x360').init(data); require('../main/arcadenames').init(data); require('../main/dirsize').init({ dataDir: data });
  const out = [];
  TREES.forEach(([id, files, art], i) => {
    const tmp = tmps[i], games = path.join(tmp, 'games');
    for (const [f, spec] of files) writeFile(path.join(games, f), spec);
    for (const f of art) writeFile(path.join(games, f), 0);
    const rel = p => p ? path.relative(games, p).split(path.sep).join('/') : null;
    const list = scanner.scanSystem(CONSOLES.find(c => c.id === id), { romDirs: [games] }, true);
    out.push({ console: id, files, art, games: list.map(g => ({
      title: g.title, region: g.region, disc: g.disc, file: rel(g.file), ext: g.ext, sizeBytes: g.sizeBytes, discs: (g.discs || []).map(rel),
      titleId: g.titleId || null, notInstalled: !!g.notInstalled, compat: g.compat || null,
      art: { front: rel(g.art.front), back: rel(g.art.back), madeBack: g.art.madeBack, spine: rel(g.art.spine), madeSpine: g.art.madeSpine,
        disc: rel(g.art.disc), manual: g.art.manual.map(rel), manualPdf: rel(g.art.manualPdf), hero: rel(g.art.hero) },
    })) });
  });
  for (const tmp of [...tmps, data]) fs.rmSync(tmp, { recursive: true, force: true });
  return { data: DATA, dirSizes: DIR_SIZES, trees: out };
}

fs.mkdirSync(OUT, { recursive: true });
fs.writeFileSync(path.join(OUT, 'titles.json'), JSON.stringify(titles, null, 1) + '\n');
fs.writeFileSync(path.join(OUT, 'scan.json'), JSON.stringify(scanFixtures(), null, 1) + '\n');
console.log('wrote', path.relative(ROOT, OUT));
process.exit(0); // (the folder-size measure it queued would keep the process waiting)
