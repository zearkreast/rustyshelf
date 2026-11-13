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

// games folders: [console, [[file, size]], art files]
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
];

function scanFixtures() {
  require('../main/systems');
  const { CONSOLES } = require('../main/systems'), scanner = require('../main/scanner');
  const out = [];
  for (const [id, files, art] of TREES) {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-')), games = path.join(tmp, 'games');
    for (const [f, size] of files) { const p = path.join(games, f); fs.mkdirSync(path.dirname(p), { recursive: true }); fs.writeFileSync(p, Buffer.alloc(size)); }
    for (const f of art) { const p = path.join(games, f); fs.mkdirSync(path.dirname(p), { recursive: true }); fs.writeFileSync(p, ''); }
    const rel = p => p ? path.relative(games, p).split(path.sep).join('/') : null;
    const list = scanner.scanSystem(CONSOLES.find(c => c.id === id), { romDirs: [games] }, true);
    out.push({ console: id, files, art, games: list.map(g => ({
      title: g.title, region: g.region, disc: g.disc, file: rel(g.file), ext: g.ext, sizeBytes: g.sizeBytes, discs: (g.discs || []).map(rel),
      art: { front: rel(g.art.front), back: rel(g.art.back), madeBack: g.art.madeBack, spine: rel(g.art.spine), madeSpine: g.art.madeSpine,
        disc: rel(g.art.disc), manual: g.art.manual.map(rel), manualPdf: rel(g.art.manualPdf) },
    })) });
    fs.rmSync(tmp, { recursive: true, force: true });
  }
  return out;
}

fs.mkdirSync(OUT, { recursive: true });
fs.writeFileSync(path.join(OUT, 'titles.json'), JSON.stringify(titles, null, 1) + '\n');
fs.writeFileSync(path.join(OUT, 'scan.json'), JSON.stringify(scanFixtures(), null, 1) + '\n');
console.log('wrote', path.relative(ROOT, OUT));
