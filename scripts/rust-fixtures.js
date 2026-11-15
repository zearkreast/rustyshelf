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

// Steam folders: [file, text] with {root} for the folder they're in; cases: [given, main, hideTools] as Settings has them
const acf = (id, name, flags, dir, size = 1000) => `"AppState"\n{\n\t"appid"\t\t"${id}"\n\t"name"\t\t"${name}"\n\t"StateFlags"\t\t"${flags}"\n\t"installdir"\t\t"${dir}"\n\t"SizeOnDisk"\t\t"${size}"\n\t"LastPlayed"\t\t"7"\n}\n`;
const STEAM = {
  files: [
    ['Steam/steamapps/libraryfolders.vdf', '\uFEFF"libraryfolders"\n{\n\t"1"\n\t{\n\t\t"path"\t\t"{root}/Library"\n\t}\n\t"0"\n\t{\n\t\t"path"\t\t"{root}/Steam"\n\t}\n\t"2"\t{ "path" "{root}/Gone" }\n}\n'],
    ['Steam/steamapps/appmanifest_10.acf', acf(10, 'Zeta Run', 4, 'Zeta')],
    ['Steam/steamapps/appmanifest_11.acf', acf(11, 'Game 10', 4, 'G10')], ['Steam/steamapps/appmanifest_12.acf', acf(12, 'game 2', 4, 'G2')],
    ['Steam/steamapps/appmanifest_228980.acf', acf(228980, 'Steamworks Common Redistributables', 4, 'Common')],
    ['Steam/steamapps/appmanifest_1493710.acf', acf(1493710, 'Proton Experimental', 4, 'Proton')],
    ['Steam/steamapps/appmanifest_20.acf', acf(20, 'Alpha Soundtrack', 4, 'AlphaOST')],
    ['Steam/steamapps/appmanifest_21.acf', acf(21, 'Omega Dedicated Server', 4, 'OmegaDS')],
    ['Steam/steamapps/appmanifest_30.acf', acf(30, 'Uninstalled Thing', 1, 'Un')],
    ['Steam/steamapps/appmanifest_31.acf', acf(31, 'Queued Update', 1026, 'Queued')], ['Steam/steamapps/common/Queued/x', ''],
    ['Steam/steamapps/appmanifest_32.acf', acf(32, 'Queued Elsewhere', 1026, 'Missing')],
    ['Steam/steamapps/appmanifest_33.acf', acf(33, 'Empty Size', 0, 'Queued', 0)],
    ['Steam/steamapps/not_a_manifest.acf', acf(34, 'Not Read', 4, 'NR')],
    ['Library/steamapps/appmanifest_40.acf', acf(40, 'beta quest', 4, 'Beta', 'x')], ['Library/steamapps/appmanifest_10.acf', acf(10, 'Zeta Run Copy', 4, 'Zeta')],
    ['Steam/appcache/librarycache/40/abc/library_600x900.jpg', ''], ['Steam/appcache/librarycache/10_library_600x900_2x.jpg', ''],
    ['Steam/appcache/librarycache/10_library_600x900.jpg', ''], ['Steam/appcache/librarycache/11/library_hero.jpg', ''],
    ['Steam/userdata/1/config/localconfig.vdf', '"UserLocalConfigStore" { "Software" { "Valve" { "Steam" { "apps" { "10" { "Playtime" "125" "LastPlayed" "99" } } } } } }'],
  ],
  cases: [['Library', 'Steam', true], ['Steam', '', false], ['Steam/steamapps', '', true], ['Gone', '', true]],
};

async function steamFixtures() {
  const steam = require('../main/steam');
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-steam-')), root = path.join(tmp, 'root');
  for (const [f, t] of STEAM.files) writeFile(path.join(root, f), text(t.split('{root}').join(root.split(path.sep).join('/'))));
  const rel = p => p ? path.relative(root, p).split(path.sep).join('/') : null;
  const out = [];
  for (const [given, main, hideTools] of STEAM.cases) {
    const games = await steam.scanSteamAsync(path.join(root, given), { mainPath: main ? path.join(root, main) : '', hideTools });
    out.push({ given, main, hideTools, games: games.map(g => ({ appid: g.appid, title: g.title, sizeBytes: g.sizeBytes, dir: rel(g.dir), minutes: g.minutes,
      lastPlayed: g.lastPlayed, front: rel(g.art.front), hero: rel(g.art.hero) })) });
  }
  fs.rmSync(tmp, { recursive: true, force: true });
  return { files: STEAM.files, cases: out };
}

// PC games beyond Steam: [file, text (with {root}) | size | { b64 }], exec: the files with their execute bit set
const J = o => JSON.stringify(o);
const utf16 = t => ({ b64: Buffer.concat([Buffer.from([0xff, 0xfe]), Buffer.from(t, 'utf16le')]).toString('base64') });
const GOG_KEY = 'HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\GOG.com\\Games', HYP_KEY = 'HKEY_CURRENT_USER\\Software\\Cognosphere\\HYP\\1_0';
const regKey = (key, vals) => key + '\r\n' + vals.map(([n, t, d]) => '    ' + n + '    ' + t + (d == null ? '' : '    ' + d)).join('\r\n') + '\r\n\r\n';
const PC = {
  files: [
    ['epic/m/a.item', J({ AppName: 'Zeta', DisplayName: ' Zeta Run ', InstallLocation: '{root}/epic/Zeta', LaunchExecutable: 'bin/../Zeta.exe', CatalogNamespace: 'ns1', CatalogItemId: 'it1', AppCategories: ['public', 'games'] })],
    ['epic/m/b.item', J({ AppName: 'UE_5.1', DisplayName: 'Engine' })], ['epic/m/c.item', J({ AppName: 'Plug', DisplayName: 'Plugin', AppCategories: ['plugins'] })],
    ['epic/m/d.item', J({ AppName: 'Gone', DisplayName: 'Gone Game', InstallLocation: '{root}/epic/Nope' })], ['epic/m/e.item', J({ AppName: 'zeta', DisplayName: 'Zeta Again' })],
    ['epic/m/f.ITEM', J({ AppName: 'Alpha Two!', DisplayName: 'alpha two', bIsIncompleteInstall: false })], ['epic/m/g.item', J({ AppName: 'Half', DisplayName: 'Half', bIsIncompleteInstall: true })],
    ['epic/m/h.item', 'not json'], ['epic/m/i.item', '\uFEFF' + J({ AppName: 'Éclair & Co', DisplayName: 'Éclair', InstallLocation: '{root}/epic/Zeta' })],
    ['epic/m/j.item', J({ AppName: 77, DisplayName: 'Alpha Two' })], ['epic/m/k.item', J({ AppName: 'Eclair', DisplayName: 'Eclair', LaunchExecutable: 'x.exe' })], ['epic/m/l.txt', J({ AppName: 'Txt', DisplayName: 'Txt' })],
    ['epic/Zeta/Zeta.exe', 10],
    ['ea/Game One/__Installer/installerdata.xml', '<DiP><contentIDs><contentID>OFB-EAST:111</contentID><contentID> 222 </contentID></contentIDs><gameTitles><gameTitle locale="de_DE">Spiel Eins</gameTitle><gameTitle locale="en_US">Game &amp; One </gameTitle></gameTitles></DiP>'],
    ['ea/Game One/One.exe', 5], ['ea/Game One/Setup.exe', 50], ['ea/Game One/_Redist/big.exe', 500], ['ea/Game One/bin/x64/deep.exe', 400], ['ea/Game One/bin/x64/too/deeper.exe', 900],
    ['ea/Two/__Installer/installerdata.xml', '<contentID>OFB-EAST:111</contentID><gameTitle>Dup</gameTitle>'], ['ea/Three/__Installer/installerdata.xml', '<contentID>x.y</contentID>'],
    ['ea/Four/__Installer/installerdata.xml', '<gameTitle>No ids</gameTitle>'], ['ea/Five/__Installer/installerdata.xml', '<contentID>five</contentID><gameTitle locale="fr_FR"> Cinq &lt;5&gt; </gameTitle>'],
    ['xbox/Alpha/Content/MicrosoftGame.config', '\uFEFF<Game><Identity Name="Made.Alpha" Publisher="CN=Made &amp; Up" Version="1"/><ExecutableList><Executable Name="a.exe" Id="AlphaApp"/></ExecutableList><ShellVisuals DefaultDisplayName="Alpha &amp; Co" /></Game>'],
    ['xbox/Beta/Content/MicrosoftGame.config', utf16('<Game><Identity Name="Made.Beta" Publisher="CN=Example Corp, O=Example, C=US"/><Executable Name="b.exe"/><ShellVisuals DefaultDisplayName="ms-resource:Name"/></Game>')],
    ['xbox/GameSave/x', 0], ['xbox/Gamma/Content/MicrosoftGame.config', '<Game><Identity Publisher="x"/></Game>'],
    ['gog/Omega/omega.exe', 3], ['gog/Omega/other.exe', 9], ['gog/Sigma/sigma.exe', 3], ['gog/Tau/bin/tau.exe', 3], ['gog/Tau/unins000.exe', 30], ['gog/Phi/readme.txt', 1],
    ['hoyo/GI/sub/YuanShen.exe', 1], ['hoyo/SR/StarRail.exe', 1], ['hoyo/ZZZ/x', 1],
    ['q3/apps.json', J({ apps: [{ name: 'Zeta64', repository: 'owner/zeta64', folderName: 'Zeta64', appIconUrl: 'https://example.invalid/z.png', tags: ['n64', 5] }, { name: 'Not Installed', folderName: 'Nope' },
      { name: 'By Path', installPath: '{root}/q3/Elsewhere', project: 'P', appIconUrl: 'http://example.invalid/p.png' }, { project: 'Proj Only', folderName: 'ProjOnly' }, 'junk', null, { name: 'Listed Elsewhere', repository: 'owner/starship' }] })],
    ['q3/settings.json', J({ AppsPath: ' ', UserAppDisplayNames: { 'owner/zeta64': 'Zeta 64 Recomp' } })],
    ['q3/Apps/Zeta64/zeta.exe', 4], ['q3/Apps/Zeta64/UnityCrashHandler64.exe', 40], ['q3/Elsewhere/e.exe', 1], ['q3/Apps/ProjOnly/p.exe', 1], ['q3/Apps/Starship-v1.0.2/ship.exe', 1],
    ['q3/Apps/Loose_One.Game/l.exe', 1], ['q3/Apps/_hidden/h.exe', 1], ['q3/Apps/temp/t.exe', 1], ['q3/Apps/Empty/readme.txt', 1],
    ['q4/installs.json', J({ 'github:own/alpha': { folder: 'alpha', group: 'Ports/N64', program: 'bin/alpha.exe' }, 7: { dir: '{root}/q4/custom/seven', executables: ['seven.exe'] }, 'github:own/beta': { folder: 'beta' },
      gone: { folder: 'gone' }, notexe: { dir: '{root}/q4/custom/sh', program: 'run.sh' }, bad: 3 })],
    ['q4/library.json', J({ items: [{ id: 'github:own/alpha', overrides: { name: 'Alpha DX', cover: 'https://example.invalid/a.jpg' } }, { id: 7, local: { name: 'Seven Local' } }, { id: 'x', name: 'Gamma Loose', folder: 'gamma-loose' }] })],
    ['q4/catalog.json', J([{ id: 'github:own/beta', name: ' Beta Name ', slug: 'beta', artwork: 'https://example.invalid/b.jpg', tags: ['a', 'b'] }, { id: 'github:own/delta', name: 'Delta Port', slug: 'delta', libraryArt: { capsule: 'https://example.invalid/d.jpg' } }, { id: 'noname' }])],
    ['q4/apps/Ports/N64/alpha/bin/alpha.exe', 1], ['q4/custom/seven/seven.exe', 1], ['q4/apps/beta/b.exe', 1], ['q4/apps/Games/delta/d.exe', 1], ['q4/apps/Emulators/emu1/e.exe', 1],
    ['q4/apps/Ports/GC/gamma-loose/g.exe', 1], ['q4/apps/standalone/s.exe', 1], ['q4/custom/sh/run.sh', 1],
    ['custom/tool.exe', 1], ['custom/Some-Game_v2.exe', 1], ['custom/x.exe', 1],
    ['pcf/games/Hollow Knight/hollow_knight.exe', 10], ['pcf/games/Hollow Knight/zbig.exe', 100], ['pcf/own/Hollow Knight/mine.exe', 1], ['pcf/games/Deep One/a/b/deep.exe', 1], ['pcf/games/Deep One/a/b/c/toodeep.exe', 1],
    ['pcf/games/Native Game/start.sh', 1], ['pcf/games/Native Game/game.x86_64', { b64: b64(Buffer.from('\x7fELF', 'latin1')) }], ['pcf/games/Native Game/notelf', 4], ['pcf/games/Native Game/win.exe', 1], ['pcf/games/Native Game/run.bat', 1],
    ['pcf/games/Chosen/one.exe', 5], ['pcf/games/Chosen/two.exe', 3], ['pcf/games/Epic Install/.egstore/x', 1], ['pcf/games/Epic Install/game.exe', 1], ['pcf/games/Skipped/s.exe', 1], ['pcf/games/.hidden/h.exe', 1],
    ['pcf/games/Empty/readme.txt', 1], ['pcf/games/Öde Stadt/o.exe', 1], ['pcf/games/Half-Life/hl.exe', 1], ['pcf/games/Half Life/hl.exe', 1], ['pcf/games/Sub Elf/bin/sub_elf', { b64: b64(Buffer.from('\x7fELF', 'latin1')) }],
    ['pcf/games/Sub Elf/bin/other', { b64: b64(Buffer.from('\x7fELF', 'latin1')) }], ['pcf/games/file.txt', 1],
  ],
  exec: ['pcf/games/Native Game/start.sh', 'pcf/games/Native Game/game.x86_64', 'pcf/games/Native Game/notelf', 'pcf/games/Sub Elf/bin/sub_elf', 'pcf/games/Sub Elf/bin/other'],
  eaDirs: ['ea/Game One', 'ea/Two', 'ea/Three', 'ea/Four', 'ea/Five', 'ea/Missing'],
  gog: regKey(GOG_KEY, []) + regKey(GOG_KEY + '\\1001', [['gameName', 'REG_SZ', 'Omega Quest'], ['path', 'REG_SZ', '{root}/gog/Omega'], ['launchCommand', 'REG_SZ', '"{root}/gog/Omega/omega.exe" -fast'], ['exe', 'REG_SZ', '{root}/gog/Omega/other.exe']])
    + regKey(GOG_KEY + '\\1002', [['gameName', 'REG_SZ', 'Omega Extra'], ['path', 'REG_SZ', '{root}/gog/Omega'], ['dependsOn', 'REG_SZ', '1001']])
    + regKey(GOG_KEY + '\\1003', [['GAMETITLE', 'REG_SZ', ' Sigma '], ['path', 'REG_SZ', '{root}/gog/Sigma'], ['launchCommand', 'REG_SZ', '{root}/gog/Sigma/nope.exe'], ['exe', 'REG_SZ', '{root}/gog/Sigma/sigma.exe'], ['launchParam', 'REG_SZ', '--x'], ['empty', 'REG_SZ']])
    + regKey(GOG_KEY + '\\1004', [['gameName', 'REG_SZ', 'Tau'], ['path', 'REG_SZ', '{root}/gog/Tau']]) + regKey(GOG_KEY + '\\1005', [['gameName', 'REG_SZ', 'Nowhere'], ['path', 'REG_SZ', '{root}/gog/None']])
    + regKey(GOG_KEY + '\\1006', [['gameName', 'REG_SZ', 'Phi'], ['path', 'REG_SZ', '{root}/gog/Phi']]) + regKey(GOG_KEY + '\\extra', [['gameName', 'REG_SZ', 'Not a number'], ['path', 'REG_SZ', '{root}/gog/Tau']]),
  hoyo: regKey(HYP_KEY + '\\hk4e_global', [['GameBiz', 'REG_SZ', 'hk4e_global'], ['GameInstallPath', 'REG_SZ', '{root}/hoyo/GI/']])
    + regKey(HYP_KEY + '\\hkrpg_cn', [['GameBiz', 'REG_BINARY', Buffer.from('hkrpg_cn\0').toString('hex').toUpperCase()], ['GameInstallPath', 'REG_SZ', '{root}/hoyo/SR']])
    + regKey(HYP_KEY + '\\nap_global', [['GameBiz', 'REG_SZ', 'nap_global'], ['GameInstallPath', 'REG_SZ', '{root}/hoyo/ZZZ']]) + regKey(HYP_KEY + '\\other', [['GameBiz', 'REG_SZ', 'zzz'], ['GameInstallPath', 'REG_SZ', '{root}/hoyo/GI']])
    + regKey(HYP_KEY + '\\again', [['GameBiz', 'REG_SZ', 'HK4E_GLOBAL'], ['GameInstallPath', 'REG_SZ', '{root}/hoyo/GI']]),
  xboxAnswer: 'Made.Alpha|Made.Alpha_abc123\r\n\r\nTS-END\r\n',
  quiver: ['q3', 'q4', 'q3/Apps', ''],
  custom: [{ id: 'c1', title: ' My Tool ', exe: '{root}/custom/tool.exe', args: '-a "b c"', shelf: 'pc' }, { id: 'p1', title: 'Zeta Recomp', exe: '{root}/custom/tool.exe', shelf: 'ports', portOf: 'gc' }, { title: 'Gone', exe: '{root}/custom/gone.exe' }, { exe: '{root}/custom/Some-Game_v2.exe', shelf: 'snes' },
    { exe: '{root}/custom/x.exe', shelf: 'nope', args: 5 }, { shellApp: 'Made.Up_8abcdefgh12345!App', title: 'Store Thing' }, { shellApp: 'bad' }, null, 'x', { exe: '', shellApp: 'Made.Up_8abcdefgh12345!App2' }, { exe: '{root}/custom/tool.exe', title: 'my tool' }],
  folder: { gamesDir: 'pcf/games', ownDir: 'pcf/own', programs: { Chosen: 'two.exe', 'Deep One': 'nope.exe' }, skip: ['pcf/games/Skipped', ''] },
  publishers: ['CN=Made & Up', 'CN=Example Corp, O=Example, C=US', ''],
  exes: ['C:/x/SoH-Ship-of-Harkinian.exe', '/a/b/zeta_run.v2.exe', '/a/...', 'plain'],
};

async function pcFixtures() {
  const cp = require('child_process'), realExec = cp.execFile; let regOut = '';
  cp.execFile = (cmd, args, opts, cb) => (cmd === 'reg' ? (setImmediate(() => cb(null, regOut, '')), null) : realExec(cmd, args, opts, cb)); // (before pcgames.js takes it)
  const pcgames = require('../main/pcgames'), pcfolder = require('../main/pcfolder');
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-pc-')), root = path.join(tmp, 'root'), R = root.split(path.sep).join('/');
  const fill = t => t.split('{root}').join(R), at = r => (r ? path.join(root, r) : '');
  for (const [f, c] of PC.files) { const p = path.join(root, f); fs.mkdirSync(path.dirname(p), { recursive: true });
    fs.writeFileSync(p, typeof c === 'string' ? fill(c) : typeof c === 'number' ? Buffer.alloc(c) : Buffer.from(c.b64, 'base64')); }
  for (const f of PC.exec) fs.chmodSync(path.join(root, f), 0o755);
  const env = { ...process.env }; delete process.env.APPDATA; delete process.env.LOCALAPPDATA; // (Quiver's usual places: none here)
  const game = g => ({ id: g.id, source: g.source, title: g.title, exe: g.exe || '', args: g.args || '', uri: g.uri || '', shellApp: g.shellApp || '', dir: g.dir || '', icon: g.icon || '', tags: g.tags || [],
    shelf: g.shelf || null, native: !!g.native, programs: (g.programs || []).map(p => [p.rel, p.native]), port: !!(g.port || g.shelf === 'ports'), portOf: g.portOf || '' });
  const unroot = o => JSON.parse(JSON.stringify(o).split(R).join('{root}'));
  const asWindows = async fn => { const d = Object.getOwnPropertyDescriptor(process, 'platform'); Object.defineProperty(process, 'platform', { value: 'win32' }); try { return await fn(); } finally { Object.defineProperty(process, 'platform', d); } };
  const out = { files: PC.files, exec: PC.exec };
  out.epic = pcgames.epicGames(at('epic/m')).map(game);
  out.ea = { dirs: PC.eaDirs, games: pcgames.eaFromDirs(PC.eaDirs.map(at)).map(game) };
  regOut = fill(PC.gog);
  out.gog = { text: PC.gog, tree: [...pcgames.parseReg(regOut)], games: (await asWindows(() => pcgames.gogGames())).map(game) };
  out.hoyo = { text: PC.hoyo, games: pcgames.hoyoFromTree(pcgames.parseReg(fill(PC.hoyo))).map(game) };
  const xbox = async answer => { const data = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-pcdata-')); pcgames.init(data); pcgames._xboxReset();
    Object.assign(pcgames._xboxIo, { platform: () => 'win32', roots: () => [at('xbox')], run: async () => answer });
    const first = (await pcgames.xboxGames()).map(game); pcgames._xboxReset(); Object.assign(pcgames._xboxIo, { run: async () => '' });
    const again = (await pcgames.xboxGames()).map(game); fs.rmSync(data, { recursive: true, force: true }); return { answer, first, again }; };
  out.xbox = { scan: pcgames.xboxScan([at('xbox'), at('nope')]), cases: [await xbox(PC.xboxAnswer), await xbox('')] };
  out.quiver = PC.quiver.map(chosen => ({ chosen, games: pcgames.quiverGames(at(chosen)).map(game) }));
  // moved from Quiver 3 to Quiver 4: Settings on the old folder, Quiver 4 in AppData using its Apps folder
  { const files = { 'moved/Quiver3/QuiverLauncher.exe': 'x', 'moved/Quiver3/settings.json': '{}',
      'moved/Quiver3/apps.json': J({ apps: [{ name: 'Zeta Run Port', repository: 'MadeUp/ZetaRun', folderName: 'ZetaRun' }] }),
      'moved/Quiver3/Apps/ZetaRun/zeta.exe': 'x', 'moved/Quiver3/Apps/OmegaKart/omega.exe': 'x',
      'moved/AppData/com.quiverlauncher.app/apps-folder.json': J({ path: '{tmp}/moved/Quiver3/Apps' }),
      'moved/AppData/com.quiverlauncher.app/installs.json': J({ 'github:MadeUp/OmegaKart': { folder: 'OmegaKart', executables: ['omega.exe'] } }),
      'moved/AppData/com.quiverlauncher.app/library.json': J([{ id: 'github:MadeUp/OmegaKart' }]),
      'moved/AppData/com.quiverlauncher.app/catalog.json': J([{ id: 'github:MadeUp/OmegaKart', slug: 'omegakart', name: 'Omega Kart (Port)', tags: [] }]) };
    const T = tmp.split(path.sep).join('/'), rel = o => JSON.parse(JSON.stringify(o).split(T).join('{tmp}'));
    for (const [f, c] of Object.entries(files)) { const p = path.join(tmp, f); fs.mkdirSync(path.dirname(p), { recursive: true }); fs.writeFileSync(p, c.split('{tmp}').join(T)); if (f.endsWith('.exe')) fs.chmodSync(p, 0o755); }
    process.env.APPDATA = path.join(tmp, 'moved', 'AppData');
    const chosen = path.join(tmp, 'moved', 'Quiver3'), roots = pcgames.quiverRoots(chosen).map(r => r.split(path.sep).join('/'));
    out.quiverMoved = { files, roots: rel(roots), games: rel(pcgames.quiverGames(chosen).map(game)) };
    delete process.env.APPDATA; }
  const custom = JSON.parse(fill(J(PC.custom)));
  out.custom = { list: PC.custom, games: (await pcgames.customGames(custom)).map(game) };
  out.folder = { ...PC.folder, games: pcfolder.scan({ gamesDir: at(PC.folder.gamesDir), ownDir: at(PC.folder.ownDir), programs: PC.folder.programs, skip: PC.folder.skip.map(at) }).map(game) };
  out.publishers = PC.publishers.map(p => [p, pcgames.publisherId(p)]);
  out.exes = PC.exes.map(e => [e, pcgames.titleFromExe(e)]);
  Object.assign(process.env, env); cp.execFile = realExec;
  fs.rmSync(tmp, { recursive: true, force: true });
  return unroot(out);
}

// emulator programs, for the names shown for them (made-up builds of the emulators shared/emuname.js knows)
const EMUS = ['C:\\Emus\\selevet-eglantine-performance-6-windows-x86_64\\eglantine.exe', 'C:\\Switch 2 emu\\lichii pgo\\lichii.exe', '/home/me/Games/Nintendo Switch 2/Lichii_K-code-AI.AppImage',
  '/home/me/Games/Nintendo Switch 2/Eglantine_old.AppImage', '/home/me/Applications/dolphin-emu.AppImage', 'D:\\Emu\\Dolphin-x64\\Dolphin.exe', 'C:\\RetroArch-Win64\\retroarch.exe',
  'C:\\emus\\citron-0123456789abcdef0123456789abcdef01234567\\citron.exe', '/opt/x86_64mariotennis.eglantine.v2.1.0.AppImage', 'C:\\Tools\\My Thing\\thing.launcher.exe', 'C:\\a\\pcsx2-qt.exe',
  '/x/Ryujinx.1.2.3-linux_x64/Ryujinx.sh', 'C:\\BizHawk-2.9\\EmuHawk.exe', 'C:\\x\\citra-qt.exe', '/y/eden nightly/eden-nightly-eden.AppImage', '', 'élan.exe', 'C:\\Emu\\windowsxenia_canary.exe',
  '/z/0123456789ABCDEF0123456789abcdef01234567/cemu', 'C:\\q\\win64.exe', '/a/b.c/duckstation-qt-x64-ReleaseLTCG.exe', 'C:\\v1.2\\Lichii.v1.2.exe', 'C:\\Lichii (test) [b]\\LICHII.EXE'];

// settings files as saved by older versions, edited by hand, or damaged: what load() and normalize() make of them
const SETTINGS = [
  '{}', '[1]', '{"ui": nope', '{"ui":null,"pc":"x","systems":[],"games":[],"steam":null}', '{"ui":"str","art":5}',
  J({ ui: { ambient: true, ambientLevel: 0.03 } }), J({ ui: { soundRev: 3, ambient: true, ambientLevel: 0.03 } }), J({ ui: { soundRev: '1', ambientLevel: 0.5, theme: 'custom-my-theme', music: 'own' } }),
  J({ ui: { soundRev: null, theme: 5, soundTheme: '', music: '', bootSound: 'keys', quitComboKeys: 'start+back', quitComboChosen: true, autoCovers: false } }),
  J({ setupDone: true, retroarch: 'C:\\RetroArch\\retroarch.exe', systems: { gc: { enabled: true, emulator: 'C:\\Emu\\Dolphin.exe', romDirs: ['D:\\Games\\GC'], gameArgs: '--batch "{rom}"' }, snes: { romDirs: ['D:\\Games\\SNES'], raCore: 'snes9x' } },
    steam: { enabled: true, path: 'C:\\Steam', hideTools: false }, pc: { custom: [{ id: 'tool', title: 'Tool', exe: 'C:\\Tools\\tool.exe', args: '-x', shelf: 'pc' }], gamesDir: 'D:\\PC', programs: { Zeta: 'bin\\zeta.exe' } },
    games: { 'gc:D:\\Games\\GC\\Zeta.iso': { title: 'Zeta DX', status: 'playing' } }, ui: { theme: 'crt', favorites: ['gc:D:\\Games\\GC\\Zeta.iso'], volume: 0.35, soundRev: 4, autoCovers: true, autoManuals: false, uiScale: 1.3, sort: 'recent' },
    keys: { fanart: 'k' }, ra: { user: 'u', key: '', enabled: false } }),
  J({ setupDone: 'yes', retroarch: '  C:\\RA\\retroarch.exe  ',
    systems: { gc: { gameArgs: '-b -e "{rom}"', romDirs: 'nope', enabled: false, extra: 1 }, wii: null, snes: { romDirs: ['/roms/snes'], emulator: 5 }, madeup: { a: 1 } },
    steam: { enabled: true, path: 5, hideTools: 'x' },
    pc: { epic: 'no', quiver: false, custom: [{ exe: ' C:\\a.exe ', title: '  A  ', id: '', args: 7, shelf: 'nope', platform: 'snes', src: 'playnite', runner: '' }, { exe: 'C:\\p.exe', shelf: 'ports', portOf: 'n64' }, { exe: 'C:\\q.exe', portOf: 'nope' }, { exe: '' }, 'x', null, { exe: '/b', shelf: 'pc', platform: 'gc' }],
      pnDirs: { steam: ['/a', 5, '/b'], '12345678901234567890123': 'x' }, gamesDir: ' /games ', run: { 'pc-a': { runner: ' proton ', prefix: 'relative', env: { GOOD: 'v', '1BAD': 'v', NUM: 5 } }, 'pc-b': { prefix: '/abs' }, 'pc-c': 3 },
      programs: { A: 'bin/a.exe', B: '/abs.exe', C: '../x.exe', D: '', E: 'ok\\..\\x', 7: 'seven.exe' }, boxColor: '#12abCD', boxColors: { ps5: '#000000', nope: '#111111', ps4: 'red' }, quiverPath: null, consoleBox: 'yes', gog: 0 },
    games: { 'gc:/g/a.iso': { emulator: '  /e/dolphin  ', title: ' A   b\t c ', window: 'borderless', install: 'gone', artName: '', status: 'finished', other: 1 }, x: 'str', y: {}, 5: { title: 'five' } },
    emuSeen: ['a', 'a', '', 5, 'b'], emuSetup: { auto: 0, graphics: 'ultra', resume: true, pending: ['p', 'p'] }, apps: { stremio: { enabled: false, path: 'x' } },
    art: { styles: { gc: { preset: 'sets', region: 'eur', parts: { front: 'hd', back: 'nope', spine: 'region' }, setsHd: true, sharper: 'x' }, madeup: {}, wii: 'x', snes: { preset: 'weird', regionFirst: false }, n64: { region: 'jpn' } },
      locks: { 'gc|Game A': { front: { look: 'exact', key: 'k', at: '5' }, back: { look: 'nope' }, all: { at: 'x', sig: { front: 'hd', disc: 5 } } }, 'BAD|x': { front: { look: 'hd' } }, 'gc|': { front: { look: 'hd' } }, 'snes|Empty': {}, 'snes|\n': { disc: { look: 'special', key: '', at: -1 } } } },
    ui: { sound: false, quitComboKeys: 'start+back', theme: 'sunset', soundTheme: 'jazz', music: 'lounge', bootSound: 'boom', discordPresence: 'x', discordAppId: 'ab12-34', language: 'tr', background: 'art', volume: '0.5', ambientLevel: 9, musicLevel: null,
      previews: 'all', previewDelay: '3', previewSound: 1, clipHint: 'x', previewDirs: ['/p', '/p', '', 5], collections: [{ id: 'c1', name: '  My   games ', ids: ['a', 'a', 5] }, { id: '', name: 'x' }, { id: 'c2', name: '   ' }, 'x'],
      looks: [{ name: ' L ', set: { theme: 'crt', n: 1.5, b: true, o: {}, 'bad key': 1, long: 'x'.repeat(81) } }, { name: 5 }], openInto: 'nowhere', clock: 'time', buttons: 'ps', performance: 5, homeLayout: 'center', clock24: 'x', seamless: false,
      trailerVol: '', screenMode: 'tiny', windowBounds: { x: 10.4, y: -5.5, width: 800, height: 600.5 }, display: 2, audioOut: 5, safeArea: '95.5', attractMinutes: '61', dimMinutes: '20', uiScale: '1.15', wiimote: 'x', makeBacks: false, wmHold: 'flat',
      wmSpeed: '1.26', sw2Last: { 'sw2:/a': '/e', 'sw2:/b': 5 }, museumStyle: { walkIn: 'x', tvStand: 'cart', tvSize: 'xl', tv: 'x', tvMute: true, tvVol: 3, tvCrt: 'x' }, museumLight: 'disco', tvFavs: ['a', 'a', 5, ''], tvLast: { ads: 'x', mix: 5, other: 'y' },
      hidden: ['a', 5, 'a', {}, ''], autoHideHints: 1, boxScale: '1.234', shelfScale: { gc: '1.239', wii: 3, snes: 'x', 10: 0.5 }, gameScale: 'x', customSort: { gc: ['a', 5], wii: 'x' }, favorites: 'x', lastSeenVersion: 5, soundRev: 1, ambient: true, artRegion: 'eur', artHD: false } }),
  J({ ui: { windowBounds: { x: 1, y: 2, width: 100, height: 600 }, trailerVol: 0.25, safeArea: 80, attractMinutes: 12.5, dimMinutes: null, uiScale: 2, wmSpeed: 5, wmHold: 'sideways', wiimote: false, volume: ' ', ambientLevel: '0.01', musicLevel: 'loud', display: 'HDMI-1',
    museumStyle: { tvVol: 0.4, tvCh: 'mix', tvAnim: false }, museumLight: 'neon', boxScale: 0.5, tvLast: 'x', sw2Last: 5, previewDelay: 4, screenMode: 'window', leaveHint: true, artRegion: 'mars' }, art: { styles: { gc: {} } } }),
];
function settingsFixtures() {
  const config = require('../main/config'), dir = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-cfg-')); config.init(dir);
  const out = SETTINGS.map(text => { fs.writeFileSync(path.join(dir, 'config.json'), text); let o = null; try { o = JSON.parse(text); } catch {}
    return { text, loaded: JSON.stringify(config.load(), null, 2), normalized: o && typeof o === 'object' && !Array.isArray(o) ? JSON.stringify(config.normalize(o), null, 2) : null }; });
  fs.rmSync(dir, { recursive: true, force: true });
  return out;
}

// a home folder with emulators and games folders in it, for what setup finds by itself ({home} = that folder; the app's
// own folder is {home}/app). Only the folders under the home and the app are searched, so the answer is this tree's alone
const DETECT = {
  files: ['app/Emulators/Dolphin/dolphin-emu', 'Applications/Eden-v1.AppImage', 'Applications/DuckStation-x64.AppImage', '.local/bin/retroarch', 'Emulators/flycast/flycast',
    'Emulators/retro/RetroArch-Linux.AppImage', 'Desktop/Emus/deep/gopher64', 'Desktop/Emus/deep/deeper/RMG', 'Desktop/node_modules/citron', 'Desktop/.hidden/citron', 'Desktop/stremio',
    'Games/vita3k/Vita3K', ['Games/vita3k/config.yml', 'other: 1\npref-path: "../vitadata"  \n'], 'Games/vitadata/ux0/app/PCSE00000/eboot.bin', '.local/share/Vita3K/Vita3K/ux0/app/x',
    ['.local/share/Vita3K/Vita3K/config.yml', "pref-path: '{home}/Elsewhere'\n"], 'Elsewhere/ux0/app/x',
    'app/ROMS/gc/x', 'app/roms/GameCube/x', 'Games/roms/SNES/x', 'Emulation/roms/n64/x', 'Games/wii/x', 'Downloads/isos/PSX/x', 'Documents/games/Dreamcast/x', 'Documents/other/snes/x',
    'Portable/a/b/c/d/dolphin-emu', 'Portable/a/b/c/d/e/citron', 'Portable/x/retroarch.AppImage', 'Portable/x/y/duckstation-qt'],
  systems: ['gc', 'psvita', 'n64', 'nope'],
};
async function detectFixtures() {
  const { CONSOLES, APPS } = require('../main/systems'), paths = require('../main/paths'), detect = require('../main/detect');
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-home-')), was = { HOME: process.env.HOME, XDG: process.env.XDG_DATA_HOME, root: paths.appRoot() };
  for (const f of DETECT.files) { const [rel, text] = Array.isArray(f) ? f : [f, '']; const p = path.join(home, rel); fs.mkdirSync(path.dirname(p), { recursive: true }); fs.writeFileSync(p, text.split('{home}').join(home)); }
  process.env.HOME = home; delete process.env.XDG_DATA_HOME; paths.setAppRoot(path.join(home, 'app'));
  const own = d => /^(\{app\}|%HOME%)/.test(d), kept = [...CONSOLES.map(c => [c, 'emulatorFolders']), ...APPS.map(a => [a, 'folders'])].map(([o, k]) => [o, k, o[k]]);
  for (const [o, k, v] of kept) o[k] = v.filter(own);
  const all = await detect.detectAll(), out = { files: DETECT.files, all: { systems: all.systems, apps: all.apps, retroarch: all.retroarch },
    systems: DETECT.systems.map(id => [id, detect.detectSystem(id)]), inFolder: detect.emulatorsIn(path.join(home, 'Portable')), inNothing: detect.emulatorsIn(path.join(home, 'nope')) };
  for (const [o, k, v] of kept) o[k] = v;
  process.env.HOME = was.HOME; if (was.XDG != null) process.env.XDG_DATA_HOME = was.XDG; paths.setAppRoot(was.root);
  fs.rmSync(home, { recursive: true, force: true });
  return JSON.parse(JSON.stringify(out).split(home).join('{home}'));
}

// emulator settings files in a home folder ({home}), and what each step makes of them: [what, ...its arguments]
const GD = n => 'Paths\\gamedirs\\' + n;
const EMUCFG = {
  files: [
    ['eden/eden', ''], ['eden/user/config/qt-config.ini', '[Core]\r\nx=1\r\n[UI]\r\nconfirmClose=true\r\nconfirmStop = 0\r\nother=1\r\n[Zeta]\r\nconfirmClose=true\r\n'],
    ['apps/citron.AppImage', ''], ['.config/citron/qt-config.ini', '\uFEFF[UI]\n' + GD('size=5') + '\n' + [1, 2, 3, 4].map(n => GD(n + '\\path=/g' + n)).join('\n') + '\n' + GD('4\\deep_scan=false') + '\n' + GD('5\\path=/g5') + '\nconfirmClose=false\n[Other]\n' + GD('9\\path=/x') + '\n'],
    ['yuzu/yuzu.exe', ''], ['yuzu/user/config/qt-config.ini', '[Core]\nx=1'], ['wide/eden', ''], ['wide/user/config/qt-config.ini', { b64: Buffer.concat([Buffer.from([0xff, 0xfe]), Buffer.from('[UI]\n', 'utf16le')]).toString('base64') }],
    ['ansi/suyu', ''], ['ansi/user/config/qt-config.ini', { b64: Buffer.from('[UI]\npath=C:\\Jeux\\\xc9ditions\n', 'latin1').toString('base64') }], ['none/eden', ''],
    ['ini/b.ini', '; note\n[Core]\nKey=Old\n Other = 1 \nKey2 = same\n[Next]\nKey=x\n'], ['ini/c.ini', '[A]\r\nx = 1'], ['ini/d.ini', '[A]\nx = 1\n\n'],
    ['pcsx2/pcsx2-qt', ''], ['pcsx2/portable.ini', ''], ['pcsx2/inis/PCSX2.ini', '[UI]\nConfirmShutdown = true\nx = 1\n'], ['Documents/PCSX2/inis/PCSX2.ini', '[Other]\ny = 2\n'], ['plain/pcsx2-qt', ''],
    ['OneDrive - Made Up/Dokumente/x', ''], ['OneDrive/x', ''], ['OneDriveNot/x', ''],
    ['rpcs3/rpcs3', ''], ['rpcs3/GuiConfigs/CurrentSettings.ini', '[Meta]\nconfirmationBoxExitGame = true\n[x]\nconfirmationBoxExitGame=true\n'], ['rpcs3b/rpcs3', ''], ['rpcs3b/GuiConfigs/CurrentSettings.ini', '[Other]\na=1'],
    ['rpcs3c/rpcs3', ''], ['rpcs3c/GuiConfigs/CurrentSettings.ini', '[Meta]\nconfirmationBoxExitGame=false\n'],
    ['dol/Dolphin.exe', ''], ['dol/portable.txt', ''], ['dol/User/Config/Hotkeys.ini', '[Hotkeys]\r\nDevice = XInput/0/Gamepad\r\nGeneral/Stop = `Back`&`Button B`\r\nGeneral/Open = `F2`\r\n'],
    ['dol/User/Config/WiimoteNew.ini', '[Wiimote1]\r\nDevice = SDL/0/Pro Controller\r\n[Wiimote2]\r\nDevice = XInput/0/Gamepad\r\n[Wiimote3]\r\nDevice = DInput/0/Keyboard Mouse\r\n[Wiimote4]\r\nDevice = SDL/0/Pro Controller\r\n'],
    ['dol/User/Config/GCPadNew.ini', '[GCPad1]\nDevice = WGInput/0/Made Up Pad\n'],
    ['dol2/Dolphin.exe', ''], ['dol2/portable.txt', ''], ['dol2/User/Config/Hotkeys.ini', '[Hotkeys]\nDevice = SDL/0/Main Pad\nGeneral/Stop = `F1` | (`SDL/1/Other:Thumb L`&`SDL/1/Other:Thumb R`) | (`Ctrl` & (`Q`|`W`))\n'],
    ['dol2/User/Config/GCPadNew.ini', '[GCPad1]\nDevice = SDL/1/Other\n[GCPad2]\nDevice = SDL/0/Main Pad\n'],
    ['dol3/Dolphin.exe', ''], ['dol3/portable.txt', ''], ['dol3/User/Config/Hotkeys.ini', '[Other]\nx = 1\n'], ['dol3/User/Config/WiimoteNew.ini', '[Wiimote1]\nDevice = SDL/2/Third\n'],
    ['dol4/Dolphin.exe', ''], ['dol4/portable.txt', ''], ['dol4/User/Config/WiimoteNew.ini', '[Wiimote1]\nDevice = SDL/2/Third\n'],
    ['ra/retroarch.exe', ''], ['ra/retroarch.cfg', 'x = "1"\n libretro_directory = ":\\cores2"\n'], ['ra2/retroarch.exe', ''], ['ra2/retroarch.cfg', 'libretro_directory = "default"\n'], ['ra3/retroarch.exe', ''],
    ['ra4/retroarch.exe', ''], ['ra4/retroarch.cfg', 'libretro_directory = "/abs/$&/cores"\n'], ['bios/scph5501.bin', 'not it'], ['bios/dc/dc_boot.bin', 'x'], ['bios2/neogeo.zip', ''], ['bios2/scph5502.bin', 'nor this'],
    ['dol/User/GC/EUR/IPL.bin', ''], ['Documents/Dolphin Emulator/GC/USA/IPL.bin', ''], ['loose/Dolphin.exe', ''], ['loose/User/GC/JAP/IPL.bin', ''],
    ['duck/duckstation-qt-x64-ReleaseLTCG.exe', ''], ['duck/portable.txt', ''], ['duck/settings.ini', '[Main]\nConfirmPowerOff = true\n[Pad1]\nType = AnalogController\nUp = Keyboard/W\n[BIOS]\nSearchDirectory = mybios\n'], ['duck/mybios/scph1001.bin', ''],
    ['duck2/duckstation.exe', ''], ['duck2/portable.txt', ''], ['duck2/settings.ini', '\uFEFF[Pad1]\r\nType = X\r\nCross = SDL-1/A\r\nCross = Keyboard/X\r\n=odd\r\n'], ['none2/duckstation.exe', ''],
    ['psp/PPSSPPWindows64.exe', ''], ['psp/memstick/PSP/SYSTEM/ppsspp.ini', '[Graphics]\nInternalResolution = 1\n'], ['app/BIOS/readme.txt', ''], ['app/BIOS/SCPH-39001.bin', ''], ['app/BIOS/ps2-0230a.bin', ''], ['app/data/x', ''],
    ['dol5/Dolphin.exe', ''], ['dol5/portable.txt', ''], ['dol5/User/Config/Hotkeys.ini', '[Hotkeys]\nDevice = XInput/0/Gamepad\nGeneral/Stop = (`Start`&`Back`) | (`SDL/0/Gone:Start`&`SDL/0/Gone:Back`)\n'], ['dol5/User/Config/GCPadNew.ini', '[GCPad1]\nDevice = XInput/0/Gamepad\n'],
  ],
  steps: [['lists', 'pcsx2/pcsx2-qt'], ['lists', 'plain/pcsx2-qt'], ['lists', ''],
    ['quietEden', 'eden/eden'], ['quietEden', 'eden/eden'], ['quietEden', 'apps/citron.AppImage'], ['quietEden', 'apps/citron.AppImage'], ['afterExit', 'sw', 'apps/citron.AppImage'], ['afterExit', 'gc', 'apps/citron.AppImage'],
    ['quietEden', 'yuzu/yuzu.exe'], ['quietEden', 'wide/eden'], ['quietEden', 'ansi/suyu'], ['quietEden', 'none/eden'],
    ['setIni', 'ini/a.ini', 'New', { K: 'v', 'K 2': 'w' }], ['setIni', 'ini/b.ini', 'Core', { Key: 'New', Other: '1', Key2: 'same', Added: 'yes' }], ['setIni', 'ini/b.ini', 'Core', { Key: 'New' }],
    ['setIni', 'ini/c.ini', 'B', { y: '2' }], ['setIni', 'ini/d.ini', 'B', { y: '2' }], ['setIni', 'ini/d.ini', 'A', { x: '3' }],
    ['pcsx2NoConfirm', 'pcsx2/pcsx2-qt'], ['pcsx2NoConfirm', 'plain/pcsx2-qt'], ['rpcs3NoConfirm', 'rpcs3/rpcs3'], ['rpcs3NoConfirm', 'rpcs3b/rpcs3'], ['rpcs3NoConfirm', 'rpcs3c/rpcs3'], ['rpcs3NoConfirm', 'none/rpcs3'],
    ['dolphinQuitPads', 'dol/Dolphin.exe', 'l3+r3'], ['dolphinQuitPads', 'dol/Dolphin.exe', 'back+b'], ['dolphinQuitPads', 'dol/Dolphin.exe', 'nope'], ['dolphinQuitPads', 'dol2/Dolphin.exe', 'lb+rb+start'],
    ['dolphinQuitPads', 'dol3/Dolphin.exe', 'start+back'], ['dolphinQuitPads', 'dol4/Dolphin.exe', 'l3+r3'], ['dolphinQuitPads', 'dol5/Dolphin.exe', 'l3+r3'],
    ['coresDir', 'ra/retroarch.exe'], ['coresDir', 'ra2/retroarch.exe'], ['coresDir', 'ra3/retroarch.exe'], ['coresDir', 'ra4/retroarch.exe'],
    ['bios', 'psx', ['bios', '', 'bios2']], ['bios', 'neogeo', ['bios', 'bios2']], ['bios', 'dreamcast', ['bios']], ['bios', 'atarilynx', ['bios']], ['bios', 'snes', ['bios']],
    ['findIpl', 'dol/Dolphin.exe'], ['findIpl', 'loose/Dolphin.exe'], ['findIpl', ''], ['ensureIpl', 'loose/Dolphin.exe', 'EUR'], ['ensureIpl', 'dol/Dolphin.exe', 'nope'], ['findIpl', 'loose/Dolphin.exe'],
    ['restoreBackups', { gc: 'dol/Dolphin.exe', wii: 'dol2/Dolphin.exe', sw: 'eden/eden', sw2: 'apps/citron.AppImage', ps2: 'pcsx2/pcsx2-qt', ps3: 'rpcs3/rpcs3', x360: '' }],
    ['setupPlan', 'ra/retroarch.exe', {}], ['setupStatus', 'ra/retroarch.exe'], ['setupApply', 'ra/retroarch.exe', {}, {}], ['setupApply', 'ra/retroarch.exe', { resume: true, quit: false }, { graphics: 'pretty' }], ['setupStatus', 'ra/retroarch.exe'],
    ['setupUndo', 'ra/retroarch.exe'], ['setupUndo', 'ra/retroarch.exe'], ['setupPlan', 'ra3/retroarch.exe', { graphics: 'fast' }], ['setupApply', 'ra3/retroarch.exe', { bios: false }, {}], ['setupUndo', 'ra3/retroarch.exe'],
    ['setupPlan', 'duck/duckstation-qt-x64-ReleaseLTCG.exe', { graphics: 'fast' }], ['setupApply', 'duck/duckstation-qt-x64-ReleaseLTCG.exe', {}, { graphics: 'fast' }], ['setupStatus', 'duck/duckstation-qt-x64-ReleaseLTCG.exe'],
    ['setupPlan', 'duck2/duckstation.exe', { graphics: 'weird' }], ['setupApply', 'duck2/duckstation.exe', { controller: false, hotkeys: false, quit: false, graphics: false }, {}], ['setupUndo', 'duck2/duckstation.exe'],
    ['setupPlan', 'pcsx2/pcsx2-qt', {}], ['setupApply', 'pcsx2/pcsx2-qt', {}, { graphics: 'pretty' }], ['setupPlan', 'dol/Dolphin.exe', {}], ['setupApply', 'dol/Dolphin.exe', {}, {}], ['setupApply', 'dol3/Dolphin.exe', {}, { graphics: 'pretty' }],
    ['setupStatus', 'dol3/Dolphin.exe'], ['setupUndo', 'dol3/Dolphin.exe'], ['setupApply', 'psp/PPSSPPWindows64.exe', {}, { graphics: 'pretty' }], ['setupPlan', 'eden/eden', {}], ['setupApply', 'eden/eden', {}, {}], ['setupStatus', 'eden/eden'],
    ['setupPlan', 'none2/duckstation.exe', {}], ['setupApply', 'none2/duckstation.exe', {}, {}], ['setupStatus', 'none2/duckstation.exe'], ['setupApply', 'gone/pcsx2.exe', {}, {}], ['setupUndo', 'duck/duckstation-qt-x64-ReleaseLTCG.exe']],
};
async function emucfgFixtures() {
  const emucfg = require('../main/emucfg'), racores = require('../main/racores'), dolphin = require('../main/dolphin'), emusetup = require('../main/emusetup');
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-emucfg-')), was = { ...process.env }, at = r => (r ? path.join(home, r) : '');
  for (const [rel, c] of EMUCFG.files) { const p = path.join(home, rel); fs.mkdirSync(path.dirname(p), { recursive: true }); fs.writeFileSync(p, typeof c === 'string' ? c : Buffer.from(c.b64, 'base64')); }
  process.env.HOME = home; for (const k of ['XDG_CONFIG_HOME', 'APPDATA', 'LOCALAPPDATA', 'USERPROFILE', 'BS_DOCUMENTS', 'BS_LINUX']) delete process.env[k];
  emusetup.init({ dataDir: path.join(home, 'app', 'data'), appRoot: path.join(home, 'app') }); const realNow = Date.now; Date.now = () => 1700000000000; // (what's written of the time is the same each run)
  const plan = p => ({ ok: p.ok, why: p.why || '', changes: p.changes.map(c => ({ group: c.group, file: c.file, cfg: c.fmt === 'cfg', section: c.section || '', key: c.key, value: c.value })), skipped: (p.skipped || []).map(x => [x.group, x.why]), groups: p.groups || [] });
  const snap = () => { const o = {}; const walk = d => { for (const e of fs.readdirSync(d, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : 1))) { const p = path.join(d, e.name);
    if (e.isDirectory()) walk(p); else { const b = fs.readFileSync(p), t = b.toString('utf8'); o[path.relative(home, p)] = Buffer.from(t, 'utf8').equals(b) ? t : { b64: b.toString('base64') }; } } }; walk(home); return o; };
  let before = snap(); const steps = [];
  for (const step of EMUCFG.steps) {
    const [what, a, b, c] = step; let result;
    if (what === 'lists') result = { pcsx2: emucfg.pcsx2Inis(at(a), process.env, true), docs: emucfg.docsDirs(process.env), dolphin: emucfg.dolphinUserDirs(at(a), process.env) };
    else if (what === 'quietEden') result = emucfg.quietEden(at(a));
    else if (what === 'afterExit') result = emucfg.afterExit(a, at(b)) || null;
    else if (what === 'setIni') result = emucfg.setIni(at(a), b, c);
    else if (what === 'coresDir') result = racores.coresDir(at(a));
    else if (what === 'bios') result = await racores.bios(a, b.map(at));
    else if (what === 'findIpl') result = dolphin.findIpl(at(a));
    else if (what === 'ensureIpl') result = dolphin.ensureIplFolder(at(a), b);
    else if (what === 'setupPlan') { const k = emusetup.kindOf(at(a)); result = k ? plan(emusetup.plan(k, at(a), b)) : null; }
    else if (what === 'setupApply') { const r = emusetup.apply(at(a), b, c); result = { ok: r.ok, why: r.why || '', applied: r.applied || [], skipped: (r.skipped || []).map(x => [x.group, x.why]), files: r.files || [] }; }
    else if (what === 'setupUndo') result = emusetup.undo(at(a)).restored;
    else if (what === 'setupStatus') { const r = emusetup.status(at(a)); result = r.supported ? { kind: r.kind, name: r.name, ready: r.ready, file: r.file, used: r.used, done: r.done, groups: r.groups, skipped: r.skipped.map(x => [x.group, x.why]), biosDir: r.biosDir } : null; }
    else if (what === 'restoreBackups') result = emucfg.restoreBackups(Object.fromEntries(Object.entries(a).map(([k, v]) => [k, at(v)])));
    else result = emucfg[what](at(a), b);
    const now = snap(), changed = {};
    for (const k of new Set([...Object.keys(before), ...Object.keys(now)])) if (JSON.stringify(before[k]) !== JSON.stringify(now[k])) changed[k] = k in now ? now[k] : null;
    steps.push({ step, result: result === undefined ? null : result, changed }); before = now;
  }
  Date.now = realNow; for (const k of Object.keys(process.env)) if (!(k in was)) delete process.env[k]; Object.assign(process.env, was);
  fs.rmSync(home, { recursive: true, force: true });
  return JSON.parse(JSON.stringify({ files: EMUCFG.files, steps }).split(home).join('{home}').split(home.toLowerCase()).join('{home}')); // (a key in emu-setup.json is the program in lower case)
}

// 3DS .cia files (made up: [signature type, title id, [content id, index, type, size, in the file, NCCH flags | null]]) in
// a home folder where Azahar has run once, and what playing each one does
function cia(sigType, titleId, contents, cut = 0) {
  const sig = { 0x10004: 0x140, 0x10002: 0x80, 0x10003: 0x240 }[sigType] || 0x140, tmd = Buffer.alloc(sig + 0xC4 + 64 * 0x24 + contents.length * 0x30);
  tmd.writeUInt32BE(sigType, 0); tmd.writeBigUInt64BE(BigInt(titleId), sig + 0x4C); tmd.writeUInt16BE(contents.length, sig + 0x9E);
  const h = Buffer.alloc(0x2020); h.writeUInt32LE(0x2020, 0); h.writeUInt32LE(0x40, 8); h.writeUInt32LE(0x10, 12); h.writeUInt32LE(tmd.length, 16);
  const bodies = [];
  contents.forEach(([id, index, type, size, present, flags], i) => { const o = sig + 0xC4 + 64 * 0x24 + i * 0x30; tmd.writeUInt32BE(id, o); tmd.writeUInt16BE(index, o + 4); tmd.writeUInt16BE(type, o + 6); tmd.writeBigUInt64BE(BigInt(size), o + 8);
    if (!present) return; h[0x20 + (i >> 3)] |= 0x80 >> (i & 7); const b = Buffer.alloc(size, 0x41 + i); if (flags != null && size >= 0x200) { b.write('NCCH', 0x100, 'latin1'); b[0x18F] = flags; } bodies.push(b); });
  const pad = (b, to) => Buffer.concat([b, Buffer.alloc(to - b.length)]), al = n => Math.ceil(n / 64) * 64;
  const head = pad(pad(pad(h, al(0x2020)), al(al(0x2020) + 0x40)), al(al(al(0x2020) + 0x40) + 0x10));
  const all = Buffer.concat([pad(Buffer.concat([head, tmd]), al(head.length + tmd.length)), ...bodies]);
  return all.subarray(0, all.length - cut);
}
const N3DS = {
  cias: [['good.cia', cia(0x10004, '0x0004000000ABC100', [[0x10, 0, 0, 0x300, true, 4], [0x11, 1, 0, 0x40, false, null], [0x12, 2, 0, 0x90, true, null]])],
    ['dlc.CIA', cia(0x10002, '0x0004008C00ABC100', [[0x20, 0, 0, 0x200, true, 4], [0x21, 1, 0x4000, 0x50, true, null]])],
    ['enc.cia', cia(0x10003, '0x0004000000ABC200', [[0x30, 0, 1, 0x200, true, 4]])], ['inner.cia', cia(0x10004, '0x0004000000ABC300', [[0x40, 0, 0, 0x200, true, 0]])],
    ['nogame.cia', cia(0x10004, '0x0004000000ABC400', [[0x50, 0, 0, 0x200, false, 4], [0x51, 1, 0, 0x40, true, null]])], ['notncch.cia', cia(0x10004, '0x0004000000ABC500', [[0x60, 0, 0, 0x200, true, null]])],
    ['cut.cia', cia(0x10004, '0x0004000000ABC600', [[0x70, 0, 0, 0x300, true, 4], [0x71, 1, 0, 0x90, true, null]], 0x20)], ['junk.cia', Buffer.alloc(0x3000, 7)], ['short.cia', Buffer.alloc(0x100)],
    ['badsig.cia', cia(0x99999, '0x0004000000ABC700', [[0x80, 0, 0, 0x200, true, 4]])], ['game.3ds', Buffer.alloc(8)]],
  plays: [['good.cia', 'emus/azahar'], ['good.cia', 'emus/azahar'], ['dlc.CIA', 'emus/azahar'], ['enc.cia', 'emus/azahar'], ['enc.cia', 'emus/citra-qt'], ['inner.cia', 'emus/azahar'], ['inner.cia', 'emus/lime3ds'],
    ['nogame.cia', 'emus/azahar'], ['notncch.cia', 'emus/azahar'], ['cut.cia', 'emus/azahar'], ['junk.cia', 'emus/azahar'], ['short.cia', 'emus/azahar'], ['badsig.cia', 'emus/azahar'], ['game.3ds', 'emus/azahar'], ['good.cia', 'emus/citra-qt']],
  dirs: ['.local/share/azahar-emu', '.local/share/lime3ds-emu', 'elsewhere'], files: [['.config/lime3ds-emu/qt-config.ini', '[Data%20Storage]\r\nuse_custom_storage=true\r\nsdmc_directory={home}/elsewhere/sd/\r\n']],
};
async function n3dsFixtures() {
  const n3ds = require('../main/n3ds');
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-3ds-')), was = process.env.HOME; process.env.HOME = home;
  for (const d of N3DS.dirs) fs.mkdirSync(path.join(home, d), { recursive: true });
  for (const [f, t] of N3DS.files) { fs.mkdirSync(path.dirname(path.join(home, f)), { recursive: true }); fs.writeFileSync(path.join(home, f), t.split('{home}').join(home)); }
  fs.mkdirSync(path.join(home, 'roms'), { recursive: true }); for (const [n, b] of N3DS.cias) fs.writeFileSync(path.join(home, 'roms', n), b);
  const snap = () => { const o = {}; const walk = d => { for (const e of fs.readdirSync(d, { withFileTypes: true })) { const p = path.join(d, e.name); if (e.isDirectory()) walk(p); else o[path.relative(home, p)] = require('crypto').createHash('sha1').update(fs.readFileSync(p)).digest('hex'); } }; walk(home); return o; };
  let before = snap(); const plays = [];
  for (const [file, exe] of N3DS.plays) { const status = []; let result;
    try { result = { boot: await n3ds.bootFile(path.join(home, 'roms', file), path.join(home, exe), t => status.push(t)) }; } catch (e) { result = { error: e.message }; }
    const now = snap(), changed = {}; for (const k of Object.keys(now).sort()) if (before[k] !== now[k]) changed[k] = now[k];
    plays.push({ file, exe, result, status, changed }); before = now; }
  process.env.HOME = was; fs.rmSync(home, { recursive: true, force: true });
  return JSON.parse(JSON.stringify({ cias: N3DS.cias.map(([n, b]) => [n, b.toString('base64')]), dirs: N3DS.dirs, files: N3DS.files, plays }).split(home).join('{home}'));
}

// a list in the shape of FinalBurn Neo's (made-up sets), for the names kept from it
function arcadeDat() {
  const L = ['<?xml version="1.0"?>', '<datafile>', '\t<header><name>Made up</name><description>Not a game</description></header>'];
  for (let i = 1; i <= 110; i++) L.push(`\t<game name="set${i}" sourcefile="x.cpp">`, `\t\t<description>Made Up Game ${i} (World)</description>`, '\t\t<year>1990</year>', '\t</game>');
  L.push('\t<game name="madebios" isbios="yes">', '\t\t<description>Made Up BIOS</description>', '\t</game>', '\t<game isbios="no" name="1942x" cloneof="set1"><description>  Nineteen &amp; Forty-Two &#233;&#x41; &lt;set&gt; &quot;x&quot; &apos;y&apos; &nope; </description>',
    '\t<game name="UPPER">', '\t\t<comment>x</comment>', '\t\t<description>Upper Set</description>', '\t<game cloneof="x">', '\t\t<description>No name</description>', '\t<game name="77">\r', '\t\t<description>Seventy Seven</description>\r',
    '\t<game name="set5">', '\t\t<description>Set Five Again</description>', '\t<gamelist name="notagame">', '\t\t<description>Still set5?</description>', '</datafile>');
  return L.join('\n');
}
async function arcadeFixtures() {
  const { Readable } = require('stream'), arcadenames = require('../main/arcadenames'), dat = arcadeDat();
  const t = await arcadenames.parse(Readable.from([dat.slice(0, 5000), dat.slice(5000)]));
  let none = null; try { await arcadenames.parse(Readable.from(['<game name="a">\n<description>A</description>'])); } catch (e) { none = e.message; }
  return { dat, names: t.names, bios: t.bios, none };
}

// box art matching: made-up titles against made-up lists, as main/covers.js answers
const ART = {
  norms: ['The Legend of Zélda III & IV: X’s', 'Zeta Run – Definitive Edition + DLC', 'An Ōmega  Quest: A Tale (of the) VIII', 'ÆON ß 2', '  ', 'Mega Zeta X', 'Zeta XIX: The II', "Rock 'n' Run", 'I V X', '1942: An The A', 'Pokémon™ Zeta®'],
  lookups: ['Zeta Run – Definitive Edition + DLC', 'Zeta Run: Game of the Year Edition', 'Zeta Run GOTY', 'Zeta Run - Nintendo Switch 2 Edition', 'Zeta Run + DLC + All DLC', 'Definitive Edition', 'Zeta & ', 'Zeta Deluxe', 'Zeta Run Complete Edition with DLC', '',
    'Zeta: The Run - Deluxe Edition (Europe)', 'Zeta Run Deluxe Editions'],
  list: [['Zeta Run', 2], ['Zeta Run', 5], ['Zeta Run 2', 3], ['Zeta Run: Omega Deluxe', 1], ['Finding Zeta', 1], ['Alpha & Beta', 0], ['Alpha / Beta', 2], ['Sigma and Tau at the Rio 2016 Olympic Games', 1], ['Omega Quest IV', 1],
    ['Omega Quest', 0], ['Pixar Tau Racers', 1], ['Mega Zeta X', 1], ['Phi: The Long Subtitle of Many Words', 1], ['Kart Phi Deluxe 8', 1], ['', 3], ['Rho', 0], ['Rho Returns', 0], ['The Rho', 1]],
  titles: ['Zeta Run', 'zetarun', 'ZETA RUN!!', 'Zeta Run 3', 'Zeta Run Omega', 'Zeta Run: Omega', 'Disney-Pixar Finding Zeta', "Disney's Finding Zeta", 'Alpha and Beta', 'Alpha Beta', 'Sigma & Tau Rio 2016', 'Sigma and Tau Rio 2012', 'Omega Quest 4',
    'Omega Quest V', 'Omega Quest: Definitive Edition', 'Tau Racers', 'Nick Tau Racers', 'Mega Zeta', 'Mega Zeta X2', 'Phi Long Subtitle Many Words', 'Phi', 'Phi Kart 8 Deluxe', 'Deluxe Kart Phi', 'DVD', 'game', 'Xbox 360 Game 4d5307e6', '!!!', '', 'Rho', 'Rho Returns Again',
    'Sega Rho', 'EA Sports EA Rho', 'Nothing Like It', 'Zeta', 'Run Zeta'],
  images: [['Box - Front', 'Japan', 'f/jp.jpg'], ['Box - Front', 'Europe', 'f/eu.jpg'], ['Box - Front', '', 'f/none.jpg'], ['Box - Front', 'North America', 'f/na.jpg'], ['Box - Front', 'Germany', 'f/de.jpg'], ['Box - Front', ' United Kingdom ', 'f/uk space.jpg'],
    ['Box - Front', 'World', 'f/world.jpg'], ['Box - Front', 'Canada', ''], ['Box - Back', 'North America', 'b/na.jpg'], ['Box - Back', 'United Kingdom', 'b/uk.jpg'], ['Box - Back', 'Brazil', 'b/br.jpg'], ['Box - Back', 'Japan', 'b/jp.jpg'], ['Disc', 'Europe', 'd/eu.png'],
    ['Fanart - Box - Back', 'Australia', 'fb/au.jpg'], ['Box - Spine', 'USA', 's/usa.jpg']],
  picks: [['Box - Front', 'auto'], ['Box - Front', 'eur'], ['Box - Front', 'jpn'], ['Box - Front', 'usa'], ['Box - Back', ''], ['Box - Back', 'jpn'], ['Fanart - Box - Back', 'eur'], ['Disc', 'usa'], ['Box - Spine', 'usa'], ['Clear Logo', 'usa']],
  libretro: ['Zeta Run (USA)', 'Zeta Run (Europe) (En,Fr,De)', 'Zeta Run (Japan)', 'Zeta Run (USA) (Rev 1)', 'Omega Quest_ The Tale (Europe)', 'Omega Quest_ The Tale (USA, Europe)', 'Tau Racers (World)', 'Sigma (Japan) (Player\'s Choice)', 'Phi & Rho (USA)', 'Phi _ Rho (Europe)',
    'Legend of Rho, The (USA) (Collector\'s Edition)', 'Alpha (UK)', 'Beta (Germany)', 'Gamma (Asia)'],
  games: [['/roms/gc/Zeta Run (USA).iso', 'Zeta Run', 'USA'], ['/roms/gc/Zeta Run (Europe) (En,Fr,De).rvz', 'Zeta Run', 'Europe'], ['/roms/gc/zeta run.iso', 'zeta run', ''], ['/roms/gc/Zeta Run (Korea).iso', 'Zeta Run', 'Korea'], ['/roms/gc/Omega Quest: The Tale (Europe).iso', 'Omega Quest: The Tale', 'Europe'],
    ['/roms/gc/Omega Quest - The Tale - Definitive Edition.iso', 'Omega Quest - The Tale - Definitive Edition', ''], ['/roms/gc/Tau Racers.iso', 'Tau Racers', ''], ['/roms/gc/Sigma.iso', 'Sigma', ''], ['/roms/gc/Phi & Rho (USA).iso', 'Phi & Rho', 'USA'], ['/roms/gc/The Legend of Rho.iso', 'The Legend of Rho', ''],
    ['/roms/gc/Unknown Thing.iso', 'Unknown Thing', ''], ['/roms/ps4/Alpha/eboot.bin', 'Alpha', ''], ['/roms/gc/Gamma (Asia).iso', 'Gamma', 'Asia'], ['/roms/gc/Beta.iso', 'Beta', 'Japan']],
  regions: ['auto', 'usa', 'eur', 'jpn'],
  editions: ["Zeta (USA) (Player's Choice)", 'Zeta [Greatest Hits]', "Collector's Edition", 'Limited Deluxe', 'Platinum Special', 'plain', '', 'Nintendo Selects', 'hitsville', 'Best Price!'],
  lbRegions: ['North America', ' usa ', 'Europe', 'PAL', 'Japan', 'World', 'world', 'Brazil', '', 'Oceania', 'United Kingdom'],
  lrNames: ['Zeta (USA)', 'Zeta (Europe, Australia)', 'Zeta (Japan) (Rev 1)', 'Zeta (World)', 'Zeta (En,Fr) (USA, Europe)', 'Zeta (Asia)', 'Zeta', 'Zeta (U.S.A.)', 'Zeta (Japanese)', 'Zeta (PAL)', 'Zeta (Canada, World)'],
  tdb: ['US', 'JA', 'KO', 'EN', 'FR', 'AU'],
  discs: [['a.iso', [[0, 'GZTE01']]], ['b.rvz', [[0, 'RVZ\x01'], [0x58, 'RZTP01']]], ['c.wia', [[0, 'WIA\x01'], [0x58, 'SZTJ8P']]], ['d.ciso', [[0, 'CISO'], [0x8000, 'DZTE01']]], ['e.wbfs', [[0, 'WBFS'], [0x200, 'RZTK01']]], ['f.iso', [[0, 'XZTE01']]],
    ['g.iso', [[0, 'gzte01']]], ['h.gcz', [[0, '\x01\xc0\x0b\xb1']]], ['i.iso', [[0, 'GZ']]], ['j.wbfs', [[0, 'WBFS']]]],
  replaced: { names: ['Zeta (2026-01-02-03-04-05-678).png', 'Zeta (2026-01-02-03-04-05-678-2).PNG', 'zeta (2025-12-31-23-59-59-999).jpeg', 'Zeta-back (2026-01-02-03-04-05-678).jpg', 'Zeta (2026-01-02).png', 'Zeta (2026-01-02-03-04-05-67).webp', 'Zeta (2026-1-2-3-4-5-6).jpg',
    'Zeta (2026-01-02-03-04-05-678).gif', 'Other (2026-01-02-03-04-05-678).png', 'Zeta (x).png'], asks: [['Zeta', ''], ['Zeta', '-back'], ['zeta', ''], ['Other', '-spine']] },
};
function artFixtures() {
  const covers = require('../main/covers'), tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-art-'));
  const list = ART.list.map(([name, art], i) => ({ id: String(100 + i), name, n: covers.norm(name), art })), idx = { games: { 'Nintendo GameCube': list }, images: {} };
  for (const g of list) idx.images[g.id] = Array.from({ length: g.art }, (_, k) => ({ type: k % 2 ? 'Disc' : 'Box - Front', region: '', file: 'x.jpg' })).concat([{ type: 'Clear Logo', region: '', file: 'l.png' }]);
  const imgs = ART.images.map(([type, region, file]) => ({ type, region, file })), pos = g => (g ? list.indexOf(g) : null);
  const out = { norms: ART.norms.map(s => [s, covers.norm(s)]), lookups: ART.lookups.map(t => [t, covers.lookupNames(t)]), list: ART.list,
    titles: ART.titles.map(t => ({ title: t, loose: pos(covers.matchTitle(list, t, g => g.art)), exact: pos(covers.matchTitle(list, t, g => g.art, true)), plain: pos(covers.matchTitle(list, t)), name: pos(covers.matchName(list, t, g => g.art)), launchbox: pos(covers.launchboxMatch(idx, 'gc', { title: t })) })),
    images: ART.images, picks: ART.picks.map(([type, region]) => ({ type, region, files: covers.pickImages(imgs, type, region).map(i => i.file) })),
    libretro: ART.libretro, games: ART.games.map(([file, title, region]) => ({ file, title, region, all: covers.libretroAll(ART.libretro, { file, title, region }), found: ART.regions.map(r => [r, covers.libretroMatch(ART.libretro, { file, title, region }, r)]) })),
    editions: ART.editions.map(t => [t, covers.edition(t)]), lbRegions: ART.lbRegions.map(r => [r, covers.lbRegion(r)]), lrNames: ART.lrNames.map(n => [n, covers.lrRegion(n)]), tdb: ART.tdb.map(r => [r, covers.tdbRegion(r)]),
    discs: ART.discs.map(([name, at]) => { const b = Buffer.alloc(0x8010); for (const [o, t] of at) b.write(t, o, 'latin1'); const f = path.join(tmp, name); fs.writeFileSync(f, name === 'i.iso' ? b.subarray(0, 4) : b); return { name, at: at.map(([o, t]) => [o, Buffer.from(t, 'latin1').toString('base64')]), short: name === 'i.iso', id: covers.discId(f) }; }),
    replaced: { names: ART.replaced.names, asks: ART.replaced.asks.map(([base, suffix]) => ({ base, suffix, found: covers.replacedFor('/art', base, suffix, ART.replaced.names).map(r => [path.basename(r.file), r.stamp]) })) } };
  fs.rmSync(tmp, { recursive: true, force: true });
  return out;
}

// pictures put in place over older ones (the stamps in replaced\ are the time: written here as STAMP), and picture
// sizes read from made-up headers
function jpegHead(w, h, pad) { const apps = []; for (let left = pad; left > 0; left -= 60000) { const n = Math.min(left, 60000), app = Buffer.alloc(4 + n); app[0] = 0xff; app[1] = 0xe1; app.writeUInt16BE(n + 2, 2); apps.push(app); }
  const sof = Buffer.from([0xff, 0xc0, 0, 17, 8, h >> 8, h & 255, w >> 8, w & 255, 3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
  return Buffer.concat([Buffer.from([0xff, 0xd8]), ...apps, Buffer.from([0xff, 0xc4, 0, 4, 0, 0]), sof, Buffer.alloc(32)]); }
function pngHead(w, h) { const b = Buffer.alloc(40); Buffer.from('\x89PNG\r\n\x1a\n', 'latin1').copy(b); b.writeUInt32BE(13, 8); b.write('IHDR', 12, 'latin1'); b.writeUInt32BE(w, 16); b.writeUInt32BE(h, 20); return b; }
const PLACE = {
  files: [['Zeta.png', 'A'], ['Zeta.JPG', 'B'], ['Zeta-back.jpg', 'back'], ['Zeta.incoming.png', 'C'], ['replaced/Zeta (2020-01-01-00-00-00-000).png', 'A'], ['replaced/Zeta (2019-01-01-00-00-00-000).jpg', 'older'], ['replaced/Zeta-back (2020-01-01-00-00-00-000).jpg', 'A'],
    ['Zeta-back.incoming.jpg', 'D'], ['Omega.webp', 'W'], ['Omega.tmp.webp', 'X'], ['Tau-spine.incoming.jpg', 'T']],
  steps: [['Zeta', '', 'Zeta.incoming.png', '.png'], ['Zeta', '-back', 'Zeta-back.incoming.jpg', '.jpg'], ['Omega', '', 'Omega.webp', '.webp'], ['Omega', '', 'Omega.tmp.webp', '.webp'], ['Tau', '-spine', 'Tau-spine.incoming.jpg', '.jpg'], ['Gone', '', 'nope.png', '.png']],
  sizes: [['a.png', pngHead(640, 480)], ['b.jpg', jpegHead(1200, 1700, 10)], ['c.jpg', jpegHead(300, 200, 5000)], ['d.webp', Buffer.from('RIFF\0\0\0\0WEBPVP8 ', 'latin1')], ['e.png', pngHead(1, 1).subarray(0, 20)], ['f.jpg', Buffer.from([0xff, 0xd8, 0xff, 0xd9])], ['g.jpg', jpegHead(9, 9, 70000)]],
};
async function placeFixtures() {
  const covers = require('../main/covers'), dir = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-place-'));
  for (const [f, t] of PLACE.files) { fs.mkdirSync(path.dirname(path.join(dir, f)), { recursive: true }); fs.writeFileSync(path.join(dir, f), t); }
  const stamp = s => s.replace(/\((?!20(?:19|20)-)\d{4}(?:-\d+){6}(-\d+)?\)/g, '(STAMP$1)'), rel = f => stamp(path.relative(dir, f));
  const snap = () => { const o = {}; const walk = d => { for (const e of fs.readdirSync(d, { withFileTypes: true })) { const f = path.join(d, e.name); if (e.isDirectory()) walk(f); else o[rel(f)] = fs.readFileSync(f, 'latin1'); } }; walk(dir); return Object.fromEntries(Object.entries(o).sort()); };
  const steps = [];
  for (const [base, suffix, src, ext] of PLACE.steps) { let result; try { const r = await covers.placeArt(dir, base, suffix, path.join(dir, src), ext); result = { dest: rel(r.dest), moved: r.moved.map(rel) }; } catch (e) { result = { error: true }; }
    steps.push({ step: [base, suffix, src, ext], result, files: snap() }); }
  const sizes = PLACE.sizes.map(([n, b]) => { fs.writeFileSync(path.join(dir, n), b); return [n, b.toString('base64'), covers.sizeOf(path.join(dir, n))]; });
  fs.rmSync(dir, { recursive: true, force: true });
  return { files: PLACE.files, steps, sizes };
}

// a small LaunchBox index (made-up games) and what the game info lookup gives for each title
const INFO = {
  index: { v: 8, at: 1, games: { 'Nintendo GameCube': [
    { id: '1', name: 'Zeta Run', n: 'zeta run', overview: 'A run.', release: '1999-11-21T00:00:00-08:00', year: '1999', dev: 'Made Up', pub: 'Made Up', genres: 'Racing; Sports', esrb: 'E - Everyone', players: '1-4', coop: 'true', rating: '3.456', wiki: 'https://example.invalid/w', video: 'https://example.invalid/v' },
    { id: '2', name: 'Zeta Run', n: 'zeta run' }, { id: '3', name: 'Omega Quest: The Tale', n: 'omega quest tale', year: '2001', rating: '0' }, { id: '4', name: 'Tau Racers Deluxe', n: 'tau racers deluxe', genres: 'Racing' }, { id: '5', name: 'Sigma', n: 'sigma', rating: 'x' }],
    'Nintendo Wii': [] },
    images: { 1: [{ type: 'Box - Front', region: '', file: 'f/1.jpg' }, { type: 'Screenshot - Game Title', region: '', file: 's/title 1.jpg' }, { type: 'Screenshot - Gameplay', region: '', file: 's/play1.jpg' }, { type: 'Screenshot - Gameplay', region: '', file: 's/play2.jpg' },
      { type: 'Fanart - Background', region: '', file: 'bg/1.jpg' }, { type: 'Clear Logo', region: '', file: 'logo/1.png' }], 2: [{ type: 'Box - Front', region: '', file: 'f/2.jpg' }], 4: [{ type: 'Disc', region: '', file: 'd/4.png' }] } },
  asks: [['gc', 'Zeta Run'], ['gc', 'ZetaRun'], ['gc', 'zeta run!'], ['gc', 'Omega Quest - The Tale'], ['gc', 'OmegaQuestTale'], ['gc', 'Tau Racers'], ['gc', 'Sigma'], ['gc', 'Nothing Here'], ['wii', 'Zeta Run'], ['snes', 'Zeta Run'], ['nope', 'Zeta Run'], ['gc', 'Zeta Run - Definitive Edition']],
};
function infoFixtures() {
  const covers = require('../main/covers'), data = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-info-'));
  fs.mkdirSync(path.join(data, 'boxart-cache')); fs.writeFileSync(path.join(data, 'boxart-cache', 'launchbox-index.json'), JSON.stringify(INFO.index));
  covers.init(data);
  const out = { index: INFO.index, asks: INFO.asks.map(([sys, title]) => [sys, title, covers.gameInfo(sys, title)]) };
  fs.rmSync(data, { recursive: true, force: true });
  return JSON.parse(JSON.stringify(out));
}

fs.mkdirSync(OUT, { recursive: true });
fs.writeFileSync(path.join(OUT, 'emunames.json'), JSON.stringify(EMUS.map(p => { const n = require('../shared/emuname'); return { path: p, label: n.label(p), parts: n.parts(p), known: n.knownName(p) }; }), null, 1) + '\n');
fs.writeFileSync(path.join(OUT, 'settings.json'), JSON.stringify(settingsFixtures(), null, 1) + '\n');
fs.writeFileSync(path.join(OUT, 'info.json'), JSON.stringify(infoFixtures(), null, 1) + '\n');

// a PC game's window mode: the helper's command for made-up games, and the mode read from the settings
function windowFixtures() {
  const wm = require('../main/windowmode');
  const scripts = [[4242, 'D:\\Games\\Made Up Port', 28800], [7, "C:\\It's Here", 60]].map(([pid, dir, seconds]) => ({ pid, dir, seconds, script: wm.psScript(pid, dir, seconds) }));
  const games = { a: { window: 'borderless' }, b: { window: 'windowed' }, c: { window: 'tiny' }, d: { title: 'No Mode' } };
  return { scripts, modes: ['a', 'b', 'c', 'd', 'e'].map(id => [id, wm.modeOf({ games }, id)]), games };
}
fs.writeFileSync(path.join(OUT, 'windowmode.json'), JSON.stringify(windowFixtures(), null, 1) + '\n');

// the Melody voice's songs: the built-in ones as they're read from their text, and MIDI files (made here, with made-up
// tunes) as songs, or as the files they aren't
function songFixtures() {
  global.window = global.window || {};
  require('../renderer/songs.js');
  const Songs = global.window.Songs;
  const vlq = n => { const out = [n & 0x7f]; while ((n >>= 7)) out.unshift((n & 0x7f) | 0x80); return out; };
  const u16 = n => [n >> 8 & 255, n & 255], u32 = n => [n >>> 24 & 255, n >>> 16 & 255, n >>> 8 & 255, n & 255];
  const chunk = (id, bytes) => [...Buffer.from(id), ...u32(bytes.length), ...bytes];
  const file = (div, tracks) => Buffer.from([...chunk('MThd', [...u16(1), ...u16(tracks.length), ...u16(div)]), ...tracks.flatMap(t => chunk('MTrk', t))]);
  // a tune over a held chord's top notes, with running status, a name, a tempo, a program change and a pitch bend
  const tune = [72, 74, 76, 79, 76, 74, 72, 67, 69, 72, 76, 81, 79, 76, 74, 72, 71, 74, 79, 83];
  const melody = [...vlq(0), 0xff, 0x03, 4, ...Buffer.from('Lead'), ...vlq(0), 0xc0, 5, ...vlq(0), 0xe0, 0, 64];
  tune.forEach((n, i) => { melody.push(...vlq(i ? 0 : 0), ...(i ? [] : [0x90]), n, 90, ...(i % 4 === 0 ? [...vlq(0), n - 5, 80] : []), ...vlq(240), n, 0, ...(i % 4 === 0 ? [...vlq(0), n - 5, 0] : [])); });
  melody.push(...vlq(0), 0xff, 0x2f, 0);
  const bass = [...vlq(0), 0xff, 0x51, 3, 7, 161, 32];
  [48, 53, 55, 45, 50].forEach(n => bass.push(...vlq(0), 0x91, n, 70, ...vlq(960), 0x81, n, 0));
  bass.push(...vlq(0), 0xf0, ...vlq(3), 1, 2, 0xf7, ...vlq(0), 0xff, 0x2f, 0);
  const drums = []; for (let i = 0; i < 40; i++) drums.push(...vlq(i ? 120 : 0), 0x99, 36 + (i % 3), 100, ...vlq(0), 0x89, 36 + (i % 3), 0);
  drums.push(...vlq(0), 0xff, 0x2f, 0);
  const good = file(480, [melody, bass, drums]);
  const few = file(480, [[...vlq(0), 0x90, 60, 90, ...vlq(480), 60, 0, ...vlq(0), 0xff, 0x2f, 0]]);
  const cases = [['Made_Up Theme', good], ['', good], ['Çok kısa 🎵', few], ['not midi', Buffer.from('RIFF....WAVEfmt ')], ['film timing', file(0xe728, [melody])], ['cut short', good.subarray(0, 60)], ['no beat', file(0, [melody, bass])]];
  const plain = s => ({ name: s.name, hue: s.hue, notes: s.notes, chords: s.chords.map(c => [c.root, !!c.minor]), bars: s.bars, builtIn: !!s.builtIn });
  return {
    builtIn: Songs.builtIn.map(plain),
    midi: cases.map(([name, bytes]) => { let song = null; try { song = plain(Songs.parseMidi(new Uint8Array(bytes), name)); } catch {} return { name, bytes: bytes.toString('base64'), song }; }),
  };
}
// ---- the backdrop every theme paints (renderer/app.js THEMES, renderer/themes-extra.js): its CSS, and the glow's hue ----
function themeFixtures() {
  const app = fs.readFileSync(path.join(ROOT, 'renderer', 'app.js'), 'utf8'), covers = fs.readFileSync(path.join(ROOT, 'renderer', 'covers.js'), 'utf8');
  const hsl = /\nconst hsl=.*\n/.exec(covers)[0], block = /\nconst THEMES=\{[\s\S]*?\n\};\n/.exec(app)[0], alias = /\nconst THEME_ALIAS=.*\n/.exec(app)[0];
  const win = {}; new Function('window', fs.readFileSync(path.join(ROOT, 'renderer', 'themes-extra.js'), 'utf8'))(win);
  const { THEMES, THEME_ALIAS } = new Function('window', 'song', `${hsl}const songHue=sh=>song.hue==null?sh:song.hue;${block}${alias}
    for(const t of window.BS_EXTRA_THEMES)THEMES[t.id]={name:t.name,halo:typeof t.halo==="function"?t.halo:sh=>sh,tint:t.tint};return {THEMES,THEME_ALIAS}`)(win, themeFixtures.song);
  const tints = [];
  for (const id of Object.keys(THEMES)) for (const [sh, hue, song] of [[258, 40, null], [0, 359, null], [351.6, 200, 96], [-12.5, 0, null], [374.25, 120.5, 300]]) {
    themeFixtures.song.hue = song;
    tints.push({ id, sh, hue, song, halo: THEMES[id].halo(sh, .5), css: THEMES[id].tint(sh, { hue }, .5) });
  }
  return { themes: Object.keys(THEMES).map(id => ({ id, name: THEMES[id].name })), alias: THEME_ALIAS, tints };
}
// ---- Find a game (renderer/find.js): what each thing typed finds among made-up titles, best first ----
function findFixtures() {
  const src = fs.readFileSync(path.join(ROOT, 'renderer', 'find.js'), 'utf8');
  const norm = /\n  const norm = .*\n/.exec(src)[0], search = /\n  function search\(q\) \{[\s\S]*?\n  \}\n/.exec(src)[0];
  const { S, run, fold } = new Function(`const S = { list: null, total: 0 }, MAX = 40, RECENT = 12;${norm}${search}return { S, run: search, fold: norm };`)();
  const games = [['Spider-Man 2', 'PS2', 5], ['Spider-Man: Web of Shadows', 'Wii', 0], ['The Amazing Spider-Man', 'PS3', 9], ['Pokémon: Red', 'Game Boy', 3], ['Pokemon Blue', 'Game Boy', 0], ['Mario & Luigi: Superstar Saga', 'GBA', 2],
    ['Super Mario Sunshine', 'GameCube', 8], ['Mario Kart: Double Dash!!', 'GameCube', 0], ['Mario Kart Wii', 'Wii', 1], ['Ōkami', 'PS2', 0], ['R-Type Final', 'PS2', 0], ['Final Fantasy X-2', 'PS2', 4], ['FINAL FANTASY XII', 'PS2', 0],
    ['Zeta Run', 'SNES', 0], ['Zeta Run', 'GameCube', 6], ['Run, Zeta, Run!', 'SNES', 0], ['2064: Read Only Memories', 'PC', 0], ['Half-Life 2', 'PC', 7], ['A Hat in Time', 'PC', 0], ['日本語のゲーム', 'PS2', 0], ['', 'PC', 0]]
    .map(([title, sys, last], i) => ({ id: 'g' + i, title, sys, last }));
  for (const g of games) { g.n = fold(g.title); g.c = g.n.replace(/ /g, ''); }
  S.list = games;
  const typed = ['', '  ', 'spider', 'Spiderman', 'SPIDER MAN', 'man spider', 'mario', 'kart mario', 'pokemon', 'Pokémon r', 'okami', 'final', 'x2', 'x-2', 'zeta run', 'run zeta', 'r', '2', 'hat', 'a hat', 'luigi and', '&', 'ゲーム', 'nothing here', 'ar'];
  return { games: games.map(g => ({ id: g.id, title: g.title, shelf: g.sys, last: g.last, folded: g.n })), searches: typed.map(q => { const found = run(q).map(g => g.id); return { typed: q, found, total: S.total }; }) };
}
fs.writeFileSync(path.join(OUT, 'find.json'), JSON.stringify(findFixtures(), null, 1) + '\n');
// ---- custom themes (main/customthemes.js): the example themes it writes, and how it reads a folder of themes ----
function customThemeFixtures() {
  const ct = require('../main/customthemes');
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-ct-')), root = path.join(tmp, 'Custom themes');
  ct.init({ root, cache: path.join(tmp, 'cache'), systems: ['gc', 'ps2', 'home', 'media'] });
  ct.ensureExamples();
  const examples = {};
  for (const d of fs.readdirSync(root).sort()) { const f = path.join(root, d); if (fs.statSync(f).isDirectory()) for (const n of fs.readdirSync(f)) examples[d + '/' + n] = fs.readFileSync(path.join(f, n), 'utf8'); else examples[d] = fs.readFileSync(f, 'utf8'); }
  // made-up themes: one with every setting, one with a single picture and a broken theme.json, two folders with the
  // same plain name, one without a picture, one with a picture for a shelf
  const put = (dir, files) => { fs.mkdirSync(path.join(root, dir), { recursive: true }); for (const [n, t] of Object.entries(files)) fs.writeFileSync(path.join(root, dir, n), t); };
  const folders = {
    'My Théme!': { 'background.png': 'x', 'theme.json': JSON.stringify({ name: '  Evening  ', accent: '#AABBCC', shade: 2, blur: true, glow: '0.5' }) },
    'my theme': { 'only.webp': 'x', 'theme.json': '{ not json' },
    'My_Theme': { 'Background.JPG': 'x', 'gc.png': 'x', 'PS2.jpg': 'x', 'wii.png': 'x', 'notes.txt': 'x', 'theme.json': JSON.stringify({ accent: 'red', shade: -1, glow: 'x', blur: 'yes' }) },
    'No picture': { 'theme.json': '{}' }, 'Two pictures': { 'a.png': 'x', 'b.png': 'x' }, '10 Stars': { 'background.svg': '<svg/>' }, '9 Stars': { 'background.svg': '<svg/>', 'theme.json': '[1]' },
  };
  for (const [d, f] of Object.entries(folders)) put(d, f);
  const list = ct.list().map(t => ({ id: t.id, name: t.name, folder: t.folder, bg: path.basename(t.bg), shelves: Object.fromEntries(Object.entries(t.sys).map(([k, v]) => [k, path.basename(v)])), shade: t.shade, blur: t.blur, glow: t.glow,
    accent: t.accent === '#6b8cff' ? null : t.accent })); // (without a picture decoder the JS gives its stand-in colour: none, here)
  const src = path.join(tmp, 'Sea_side-view.PNG'); fs.writeFileSync(src, 'x');
  const made = [ct.create(src, ''), ct.create(src, '  A: name? '), ct.create(src, '')];
  const after = fs.readdirSync(root).sort().filter(d => !(d in folders) && !(d in examples) && !Object.keys(examples).some(k => k.startsWith(d + '/')));
  const json = fs.readFileSync(path.join(root, after[0], 'theme.json'), 'utf8');
  fs.rmSync(tmp, { recursive: true, force: true });
  return { examples, folders, list, made, after, json, slugs: ['My Théme!', '  A_b--c  ', 'ÀÉÎ õü', '日本', 'x'.repeat(50)].map(n => [n, ct._slug(n)]) };
}
fs.writeFileSync(path.join(OUT, 'customthemes.json'), JSON.stringify(customThemeFixtures(), null, 1) + '\n');
themeFixtures.song = { hue: null };
fs.writeFileSync(path.join(OUT, 'themes.json'), JSON.stringify(themeFixtures()) + '\n');
fs.writeFileSync(path.join(OUT, 'songs.json'), JSON.stringify(songFixtures()) + '\n');
fs.writeFileSync(path.join(OUT, 'art.json'), JSON.stringify(artFixtures(), null, 1) + '\n');
fs.writeFileSync(path.join(OUT, 'titles.json'), JSON.stringify(titles, null, 1) + '\n');
fs.writeFileSync(path.join(OUT, 'scan.json'), JSON.stringify(scanFixtures(), null, 1) + '\n');
// ---- RetroAchievements (main/ra.js) against a stand-in for the site: made-up games, sets and an account ----
async function raFixtures() {
  const https = require('https'), { EventEmitter } = require('events');
  const site = {
    API_GetGameList: [{ ID: 11, Title: 'Zeta Run', NumAchievements: 30, Points: 400, ImageIcon: '/Images/000011.png' }, { ID: 12, Title: 'Zeta Run 2: The Long Way', NumAchievements: 12, Points: 90, ImageIcon: '/Images/000012.png' },
      { ID: 13, Title: 'Omega Quest: Dawn of the Kings', NumAchievements: 55, Points: 620, ImageIcon: '/Images/000013.png' }, { ID: 14, Title: '~Hack~ Sigma\'s Tale', NumAchievements: 8, Points: 50, ImageIcon: '/Images/000014.png' },
      { ID: 15, Title: 'Pokémon & Friends | Pocket Friends', NumAchievements: 20, Points: 200, ImageIcon: '/Images/000015.png' }, { ID: 16, Title: 'The Legend of Tau', NumAchievements: 5, Points: 25, ImageIcon: '/Images/000016.png' },
      { ID: 17, Title: 'Kart Racers Deluxe Turbo Edition', NumAchievements: 9, Points: 70, ImageIcon: '/Images/000017.png' }, { ID: 18, Title: 'Alpha III', NumAchievements: 3, Points: 15, ImageIcon: '/Images/000018.png' }],
    API_GetGameInfoAndUserProgress: { ID: 11, Title: 'Zeta Run', ConsoleName: 'GameCube', ImageIcon: '/Images/000011.png', ImageBoxArt: '/Images/box11.png', NumDistinctPlayers: 200, UserCompletion: '66.67%', UserCompletionHardcore: '33.33%', HighestAwardKind: 'beaten-softcore',
      Achievements: { 902: { ID: 902, Title: 'Second', Description: 'Do the second thing', Points: '10', BadgeName: '0902', DisplayOrder: 2, NumAwarded: 50, DateEarned: '2026-01-02 10:00:00', type: 'progression' },
        '901': { ID: 901, Title: 'First', Description: 'Do the first thing', Points: 5, BadgeName: '0901', DisplayOrder: 1, NumAwarded: 133, DateEarned: '2026-01-01 09:00:00', DateEarnedHardcore: '2026-01-01 09:00:00', Type: 'progression' },
        903: { ID: 903, Title: 'Last', Description: 'Do the last thing', Points: 25, BadgeName: '0903', DisplayOrder: 2, NumAwarded: 0, Type: null }, 7: { ID: 7, Title: 'Unordered', Description: '', Points: 1, BadgeName: '0007', NumAwarded: 3 } } },
    API_GetUserCompletionProgress: { Count: 3, Total: 3, Results: [{ GameID: 11, Title: 'Zeta Run', ConsoleName: 'GameCube', ImageIcon: '/Images/000011.png', MaxPossible: 30, NumAwarded: 20, NumAwardedHardcore: 10, HighestAwardKind: 'beaten-softcore', MostRecentAwardedDate: '2026-01-02T10:00:00+00:00' },
      { GameID: 16, Title: 'The Legend of Tau', ConsoleName: 'GameCube', MaxPossible: '5', NumAwarded: '5', NumAwardedHardcore: 5, HighestAwardKind: 'mastered', MostRecentAwardedDate: '2025-12-30T08:00:00+00:00' }, { GameID: 19, Title: 'Empty Set', MaxPossible: 0, NumAwarded: 0 }] },
    API_GetUserProfile: { User: 'Made Up', TotalPoints: 1234, TotalSoftcorePoints: 56, UserPic: '/UserPic/MadeUp.png', RichPresenceMsg: '' },
    API_GetUserRecentAchievements: [{ AchievementID: 901, Title: 'First', Description: 'Do the first thing', Points: 5, BadgeName: '0901', GameTitle: 'Zeta Run', GameID: 11, ConsoleName: 'GameCube', HardcoreMode: 1, Date: '2026-01-01 09:00:00' }, { AchievementID: 902, Title: 'Second', Description: '', Points: 10, BadgeName: '0902', GameTitle: 'Zeta Run', HardcoreMode: 0, Date: '2026-01-02 10:00:00' }],
  };
  const asked = [], real = https.get;
  https.get = (u, _opts, cb) => { asked.push(u); const name = /API\/(\w+)\.php/.exec(u)[1], res = new EventEmitter(); res.statusCode = 200; res.setEncoding = () => {}; res.resume = () => {};
    setImmediate(() => { cb(res); res.emit('data', JSON.stringify(site[name])); res.emit('end'); }); const req = new EventEmitter(); req.setTimeout = () => {}; req.destroy = () => {}; return req; };
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-ra-')), ra = require('../main/ra');
  try {
    ra.init(tmp); ra.setCredentials(' Made Up ', 'k e&y/1');
    const titles = ['Zeta Run', 'zeta run!', 'Zeta Run 2', 'Zeta Run 3', 'Omega Quest', 'Dawn of the Kings Omega Quest', 'Sigmas Tale', 'Pokemon and Friends', 'Legend of Tau, The', 'Kart Racers Deluxe Turbo', 'Kart Racers', 'Alpha', 'Alpha III', 'Nothing Like It', ''];
    const list = (await ra.progress('gc', 'nothing', false), JSON.parse(fs.readFileSync(path.join(tmp, 'ra', 'list-16.json'), 'utf8')).data);
    const out = { site, norms: [...titles, ...site.API_GetGameList.map(g => g.Title), 'Ōkami: The “Great” Game ~Demo~ II', 'A an the THE', "Ain't & isn’t"].map(t => [t, ra.norm(t)]),
      matches: titles.map(t => { const g = ra.matchGame(list, t); return [t, g ? g.id : null]; }),
      progress: await ra.progress('gc', 'Zeta Run', false), none: await ra.progress('gc', 'Nothing Like It', false), other: await ra.progress('sw', 'Zeta Run', false),
      shelf: await ra.shelfProgress('gc', titles), profile: await ra.profile(), recent: await ra.recent(12.2), test: await ra.test('Other One', 'x') };
    out.trophies = await ra.trophies(false);
    out.asked = asked; return out;
  } finally { https.get = real; fs.rmSync(tmp, { recursive: true, force: true }); }
}
// ---- the interface in another language (renderer/i18n-dict.js): texts through the French and Turkish dictionaries ----
function i18nFixtures(lang) {
  const vm = require('vm'), read = f => fs.readFileSync(path.join(__dirname, '..', 'renderer', f), 'utf8');
  const doc = { documentElement: { lang: '' }, body: null, title: '', addEventListener() {} };
  const win = { localStorage: { getItem: () => lang }, document: doc, MutationObserver: class { observe() {} }, NodeFilter: {}, requestAnimationFrame: () => 0 };
  win.window = win; vm.createContext(win);
  vm.runInContext(read('i18n-fr.js'), win); vm.runInContext(read('i18n-tr.js'), win); vm.runInContext(read('i18n-dict.js'), win);
  const D = lang === 'tr' ? win.BS_I18N_TR : win.BS_I18N_FR, keys = Object.keys(D.exact);
  // a sentence each pattern matches: its groups filled with made-up words or a known text
  const fill = ['Zeta Run', 'Omega Quest', keys[0], ''];
  const made = D.patterns.map(([src], i) => src.replace(/^\^|\$$/g, '').replace(/\\s\+/g, ' ').replace(/\((?:\.\*\?|\.\+\?|[^()]*)\)/g, () => fill[i++ % fill.length]).replace(/\\(.)/g, '$1'));
  const texts = [...keys.filter((_, i) => i % 7 === 0), ...made, '  Add games  ', 'Add\n  games', '12:30', '', 'Not a known text at all', keys[3].toUpperCase()];
  return { lang, cases: texts.map(t => [t, win.BS_tr(t)]) };
}
// ---- the museum's rooms and the arcade wing (shared/museum-layout.js, shared/arcade-plan.js): made-up libraries
// big enough to split rooms, segments and walls ----
function museumPlanFixtures() {
  const ML = require('../shared/museum-layout.js'), AP = require('../shared/arcade-plan.js');
  const genres = ['Racing', 'Fighting', 'Puzzle', 'Shooter', 'Platform', '', 'Beat \'em Up', 'Sports', 'racing ', 'Role-Playing'];
  const lib = [];
  const add = (sys, n, g) => { for (let i = 0; i < n; i++) lib.push({ id: `${sys}:${String(i).padStart(4, '0')}`, systemId: sys, primaryGenre: g ? g(i) : genres[i % genres.length], sourceOrder: lib.length }); };
  add('x360', 1250); add('gc', 90); add('nes', 45); add('psx', 30); add('megadrive', 12); add('xone', 3); add('pcengine', 5); add('mystery', 2);
  add('arcade', 340, i => genres[i % 7]); add('pc', 20); add('dos', 7); add('custom', 3);
  for (const s of ['sw', 'snes', 'n64', 'gb', 'gbc', 'gba', 'nds', 'n3ds', 'wii', 'wiiu', 'sw2', 'ps2', 'ps3']) add(s, 75);
  lib.push({ ...lib[3] }); lib.push({ id: 'none:1', systemId: null, primaryGenre: 'Puzzle' });
  const plain = p => ({ rooms: p.rooms.map(r => ({ id: r.id, bucket: r.bucketKey || '', group: r.groupKey, kind: r.kind, title: r.title, part: r.part, parts: r.parts, gameIds: r.gameIds, theme: r.themeKey,
    segments: r.segments.map(s => ({ id: s.id, system: s.systemId, gameIds: s.gameIds, wall: s.wallId, row: s.row, kind: s.kind })) })), byGame: Object.fromEntries(p.byGame) });
  const arcadeLib = lib.filter(e => e.systemId === 'arcade').concat([{ id: 'arcade:extra', systemId: 'arcade', primaryGenre: 'Driving' }]);
  const arcade = cap => { const a = AP.planArcade(arcadeLib, cap == null ? undefined : { cap }); return { cap: a.cap, corridorEnd: a.corridorEnd, walls: a.walls,
    rooms: a.rooms.map(r => ({ id: r.id, genre: r.genreKey, title: r.title, plain: r.plain, gameIds: r.gameIds, x: r.x, z: r.z, door: r.door, cabinet: r.cabinetKind, slots: r.slots })) }; };
  return { lib, manufacturer: plain(ML.planMuseum(lib, { grouping: 'manufacturer' })), genre: plain(ML.planMuseum(lib, { grouping: 'genre' })),
    systemRooms: ['x360', 'xsx', 'gc', 'mystery', 'nothing'].map(s => [s, ML.roomsForSystem(ML.planMuseum(lib, { grouping: 'manufacturer' }), s)]),
    arcade: [null, 20, 30, 5, 99, 26.6].map(c => [c, arcade(c)]) };
}
// ---- Brazilian Portuguese (renderer/i18n.js and i18n-pt-data.js, Thanks2111's): its own matching, templates with
// plurals, capitals and fragments of longer texts ----
function i18nPtFixtures() {
  const vm = require('vm'), read = f => fs.readFileSync(path.join(__dirname, '..', 'renderer', f), 'utf8');
  const doc = { documentElement: { setAttribute() {} }, title: '', addEventListener() {}, createTreeWalker: () => ({ nextNode: () => null }) };
  const win = { localStorage: { getItem: () => 'pt' }, document: doc, MutationObserver: class { observe() {} }, NodeFilter: {} };
  win.window = win; vm.createContext(win);
  vm.runInContext(read('i18n-pt-data.js'), win); vm.runInContext(read('i18n.js'), win);
  const D = win.__BS_PT, keys = Object.keys(D.d), tr = win.__bsI18n.tr;
  // each template's text with its groups filled: words, a number, a plural's two forms
  const fills = [['Zeta Run', '3', ''], ['Omega Quest', '1', 's'], [keys[5], '12', 's']];
  const made = D.t.flatMap(([src], i) => fills.slice(0, i % 3 === 0 ? 3 : 1).map(f => { let g = 0; return src.replace(/\((?:\.\*\?|\.\+\?|\[[^\]]*\][+*]|s\|)\)|\(s\|\)/g, m => /s\|/.test(m) ? f[2] : /\\d|\d/.test(m) ? f[1] : f[(g++) % 2 === 0 ? 0 : 0]).replace(/\\(.)/g, '$1'); }));
  const long = keys.filter(k => k.length >= 8 && /\s/.test(k)).slice(0, 40);
  const texts = [...keys.filter((_, i) => i % 9 === 0), ...made, ...keys.filter((_, i) => i % 41 === 0).map(k => k.toUpperCase()),
    ...long.map((k, i) => `Zeta ${i}: ${k} and more`), 'Page 3 of 12', '  Home  ', 'Home\n shelf', '', '12:30', 'Not a known text at all', tr(keys[0])];
  return { lang: 'pt', cases: texts.map(t => [t, tr(t)]) };
}
// ---- Steam achievements for the Trophies screen (main/steamach.js), over a made-up Steam API ----
async function steamachFixtures() {
  const https = require('https'), { EventEmitter } = require('events');
  const NOW = 1767261600, KEY = 'ABCDEF0123456789abcdef0123456789';
  const ok = j => ({ s: 200, j });
  const site = {
    GetPlayerSummaries: ok({ response: { players: [{ personaname: 'Made Up', avatarfull: 'https://avatars.example/made-up.jpg', communityvisibilitystate: 3 }] } }),
    GetOwnedGames: ok({ response: { games: [
      { appid: 10, name: 'Zeta Run', playtime_forever: 50, rtime_last_played: NOW - 100, img_icon_url: 'abc10', has_community_visible_stats: true },
      { appid: 20, name: 'Omega Quest', playtime_forever: 5, rtime_last_played: NOW - 5000, img_icon_url: 'abc20', has_community_visible_stats: true },
      { appid: 30, name: 'Never Played', playtime_forever: 0, has_community_visible_stats: true },
      { appid: 40, name: 'No Stats Flag', playtime_forever: 3, rtime_last_played: NOW - 9000 },
      { appid: 50, name: 'Alpha Tale', playtime_forever: 9, rtime_last_played: NOW - 200, has_community_visible_stats: true }] } }),
    'GetPlayerAchievements-10': ok({ playerstats: { success: true, achievements: [
      { apiname: 'A1', name: 'First Steps', description: 'Start', achieved: 1, unlocktime: NOW - 3600 },
      { apiname: 'A2', name: '', description: 'Old one', achieved: 1, unlocktime: NOW - 86400 * 40 },
      { apiname: 'A3', name: 'Later', achieved: 0, unlocktime: 0 }] } }),
    'GetPlayerAchievements-20': ok({ playerstats: { success: true, achievements: [
      { apiname: 'B1', name: 'All', description: 'Every one', achieved: 1, unlocktime: NOW - 7200 }, { apiname: 'B2', name: 'Done', achieved: 1, unlocktime: NOW - 7300 }] } }),
    'GetPlayerAchievements-50': { s: 400, j: { playerstats: { success: false, error: 'Requested app has no stats' } } },
    'GetSchemaForGame-10': ok({ game: { availableGameStats: { achievements: [{ name: 'A1', icon: 'https://steamcdn-a.akamaihd.net/steamcommunity/public/images/apps/10/a1.jpg' }, { name: 'A2' }] } } }),
    'GetSchemaForGame-20': { s: 500, j: null },
    'GetGlobalAchievementPercentagesForApp-10': ok({ achievementpercentages: { achievements: [{ name: 'A1', percent: '12.34' }] } }),
    'GetGlobalAchievementPercentagesForApp-20': ok({ achievementpercentages: { achievements: [{ name: 'B1', percent: 0.01 }, { name: 'B2', percent: 99.96 }] } }),
  };
  const priv = { 'GetPlayerAchievements-10': { s: 403, j: { playerstats: { success: false, error: 'Profile is not public' } } }, 'GetPlayerAchievements-20': { s: 403, j: { playerstats: { success: false, error: 'Profile is not public' } } } };
  let now = site; const asked = [], real = https.get, realNow = Date.now;
  https.get = (u, _opts, cb) => { asked.push(u.replace(KEY, 'KEY')); const url = new URL(u), name = url.pathname.split('/')[2], id = url.searchParams.get('appid') || url.searchParams.get('gameid');
    const a = now[`${name}-${id}`] || now[name] || { s: 404, j: null }, res = new EventEmitter(); res.statusCode = a.s; res.setEncoding = () => {};
    setImmediate(() => { cb(res); res.emit('data', a.j === null ? 'not json' : JSON.stringify(a.j)); res.emit('end'); }); const req = new EventEmitter(); req.setTimeout = () => {}; req.destroy = () => {}; return req; };
  Date.now = () => NOW * 1000;
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-steamach-'));
  const put = (rel, text) => { const f = path.join(tmp, rel); fs.mkdirSync(path.dirname(f), { recursive: true }); fs.writeFileSync(f, text); };
  put('steam/config/loginusers.vdf', '﻿"users"\n{\n\t"76561197960265999"\n\t{\n\t\t"AccountName"\t"other"\n\t\t"MostRecent"\t"0"\n\t\t"Timestamp"\t"1700000000"\n\t}\n\t"76561197960265851"\n\t{\n\t\t"AccountName"\t"madeup"\n\t\t"MostRecent"\t"1"\n\t\t"Timestamp"\t"1600000000"\n\t}\n\t"123"\n\t{\n\t}\n}\n');
  put('bare/userdata/0/config/localconfig.vdf', ''); put('bare/userdata/77/config/localconfig.vdf', ''); put('bare/userdata/notanid/config/localconfig.vdf', '');
  const load = dataDir => { delete require.cache[require.resolve('../main/steamach')]; const m = require('../main/steamach'); m.init({ dataDir, key: () => ' ' + KEY + ' ', steamPath: () => path.join(tmp, 'steam') }); return m; };
  try {
    const sa = load(path.join(tmp, 'data'));
    const out = { now: NOW, key: KEY, site, priv, ids: { main: sa.steamId(), bare: (sa.init({ dataDir: path.join(tmp, 'data'), steamPath: () => path.join(tmp, 'bare') }), sa.steamId()) } };
    sa.init({ dataDir: path.join(tmp, 'data'), steamPath: () => path.join(tmp, 'steam') });
    out.trophies = await sa.trophies(false);
    out.tests = [await sa.test(KEY), await sa.test('short'), await sa.test(' ' + KEY.toLowerCase() + ' ')];
    out.asked = asked.slice();
    now = { ...site, ...priv };
    const p = load(path.join(tmp, 'data2'));
    out.private = await p.trophies(false).then(() => null, e => e.message);
    const ra = { user: 'Made Up', pic: 'https://media.retroachievements.org/UserPic/MadeUp.png', points: 1234, softcore: 56,
      games: [{ id: 11, title: 'Zeta Run', console: 'GameCube', total: 30, earned: 20, pct: 67, award: 'beaten-softcore', last: '2026-01-02T10:00:00+00:00' }, { id: 16, title: 'Tau', console: 'GameCube', total: 5, earned: 5, pct: 100, award: 'mastered', last: null }],
      recent: [{ id: 901, title: 'First', date: '2026-01-01 09:00:00', rarity: 66.5 }] };
    out.combined = { both: sa.combine(ra, out.trophies, []), steam: sa.combine(null, out.trophies, ['RetroAchievements: down']), ra: sa.combine({ ...ra, user: '', pic: null }, null, []), ra2: ra };
    try { sa.combine(null, null, ['why']); } catch (e) { out.combined.none = e.message; }
    try { sa.combine(null, null, []); } catch (e) { out.combined.nothing = e.message; }
    return out;
  } finally { https.get = real; Date.now = realNow; fs.rmSync(tmp, { recursive: true, force: true }); }
}
// ---- whether each emulator is signed in to RetroAchievements (main/raemu.js), over made-up emulator folders ----
async function raemuFixtures() {
  const raemu = require('../main/raemu'), ra = require('../main/ra');
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-raemu-')), home = path.join(tmp, 'home');
  const put = (rel, text) => { const f = path.join(tmp, rel); fs.mkdirSync(path.dirname(f), { recursive: true }); fs.writeFileSync(f, text); return f; };
  const files = {
    'dolphin/Dolphin.exe': '', 'dolphin/portable.txt': '', 'dolphin/User/Config/RetroAchievements.ini': '\ufeff[Achievements]\r\nEnabled = True\r\nUsername = Made Up\r\nApiToken = secret\r\nHardcoreEnabled = true\r\n',
    'ppsspp/PPSSPPWindows64.exe': '', 'ppsspp/memstick/PSP/SYSTEM/ppsspp.ini': '[Achievements]\nAchievementsEnable = True\nAchievementsUserName = other\n', 'ppsspp/memstick/PSP/SYSTEM/ppsspp_retroachievements.dat': 'x',
    'pcsx2/pcsx2-qt.exe': '', 'pcsx2/portable.ini': '', 'pcsx2/inis/PCSX2.ini': '[UI]\nx=1\n[Achievements]\nEnabled = false\nUsername = made up\nToken = t\n',
    'duck/duckstation-qt-x64-ReleaseLTCG.exe': '', 'duck/portable.txt': '', 'duck/settings.ini': '; a note\n[Cheevos]\nEnabled = true\nUsername = Made Up\n# no token\n',
    'ra/retroarch.exe': '', 'ra/retroarch.cfg': 'cheevos_enable = "true"\ncheevos_username = "MADE UP"\ncheevos_token = "t"\ncheevos_hardcore_mode_enable = "false"\nbad line "x" y\n spaced_key   =   plain value  \n',
    'fly/flycast.exe': '', 'fly/emu.cfg': '[achievements]\nEnabled = yes\nUserName = Made Up\nToken = abc\nHardcoreMode = on\n',
    'empty/Dolphin.exe': '',
  };
  const at = Object.fromEntries(Object.entries(files).map(([rel, text]) => [rel, put(rel, text)]));
  const env = { USERPROFILE: home, HOME: home, LOCALAPPDATA: path.join(home, 'AppData', 'Local'), APPDATA: path.join(home, 'AppData', 'Roaming') };
  const cfgs = [
    { ra: { user: 'made up' }, systems: { gc: { emulator: at['dolphin/Dolphin.exe'] }, wii: { emulator: at['dolphin/Dolphin.exe'] }, psp: { emulator: at['ppsspp/PPSSPPWindows64.exe'] }, ps2: { emulator: at['pcsx2/pcsx2-qt.exe'] },
      psx: { emulator: at['duck/duckstation-qt-x64-ReleaseLTCG.exe'] }, snes: { emulator: at['ra/retroarch.exe'] }, dreamcast: { emulator: at['fly/flycast.exe'] }, sw: { emulator: at['ra/retroarch.exe'] }, nes: { emulator: path.join(tmp, 'other', 'mesen.exe') } } },
    { systems: { gc: { emulator: at['empty/Dolphin.exe'] }, ps2: { emulator: at['ra/retroarch.exe'] }, gba: { emulator: at['fly/flycast.exe'] } } },
    {},
  ];
  const rel = s => String(s).split(tmp).join('<tmp>');
  const checks = [];
  for (const cfg of cfgs) checks.push({ cfg: JSON.parse(rel(JSON.stringify(cfg))), rows: await raemu.check(cfg, ra.CONSOLE_ID, env) });
  const ini = '\ufeff; top\nloose = 1\n[ Section One ]\nKey = a = b\n=nokey\n[]\n#x\n[two]\r\nK2=\r\n', cfgText = files['ra/retroarch.cfg'];
  fs.rmSync(tmp, { recursive: true, force: true });
  return { files, checks, ini: [ini, raemu.parseIni(ini)], cfg: [cfgText, raemu.parseCfg(cfgText)], kinds: ['C:\\Emu\\Dolphin.exe', '/opt/pcsx2-qt', 'RetroArch-Win64/retroarch.exe', 'x/my-retroarch.exe', 'flycast-x64.exe', 'mesen.exe', ''].map(e => [e, raemu.kindOf(e)]) };
}
// ---- GameTDB (main/gametdb.js): a made-up database in a zip, the games read from it, and a game's full covers ----
async function gametdbFixtures() {
  const zlib = require('zlib'), gametdb = require('../main/gametdb'), { norm } = require('../main/lbindex');
  const game = (name, type, id, region, title) => `<game name="${name}"><id>${id}</id><type>${type}</type><region>${region}</region><locale lang="EN">\n  <title>${title}</title></locale><locale lang="JA"><title>x</title></locale></game>`;
  const xml = '<?xml version="1.0"?><datafile>' + [game('Zeta Run (USA)', 'Switch', 'AAAAA', 'NTSC-U', 'Zeta Run'), game('Zeta Run (EUR)', 'Switch', 'AAAAB', 'PAL', 'Zeta Run'), game('Omega &amp; Quest', 'Switch', 'BBBB', 'NTSC-J', 'Omega &amp; Quest: The &quot;Long&quot; Way'),
    game('Shop title', 'eShop', 'CCCCC', 'NTSC-U', 'Shop Only'), game('No id', 'Switch', 'bad', 'PAL', 'No Id'), game('Everywhere', 'Switch', 'DDDDDD', '', 'Sigma'), game('Korea', 'Switch', 'EEEEE', 'KOR', 'Tau Racers'),
    '<game name="No English"><id>FFFFF</id><type>Switch</type><region>PAL</region><locale lang="FR"><title>Sans</title></locale></game>', game('3DS one', '3DS', 'GGGG', 'PAL', 'Phi and Rho'), game('New one', 'New3DS', 'HHHH', 'NTSC-U', 'Alpha Tale')].join('\n') + '</datafile>';
  // a zip of one file, stored or deflated
  const zip = deflate => { const name = Buffer.from('switchtdb.xml'), raw = Buffer.from(xml), data = deflate ? zlib.deflateRawSync(raw) : raw;
    const local = Buffer.alloc(30); local.writeUInt32LE(0x04034b50, 0); local.writeUInt16LE(deflate ? 8 : 0, 8); local.writeUInt32LE(data.length, 18); local.writeUInt32LE(raw.length, 22); local.writeUInt16LE(name.length, 26);
    const central = Buffer.alloc(46); central.writeUInt32LE(0x02014b50, 0); central.writeUInt16LE(deflate ? 8 : 0, 10); central.writeUInt32LE(data.length, 20); central.writeUInt32LE(raw.length, 24); central.writeUInt16LE(name.length, 28); central.writeUInt32LE(0, 42);
    const before = Buffer.concat([local, name, data]), dir = Buffer.concat([central, name]), end = Buffer.alloc(22); end.writeUInt32LE(0x06054b50, 0); end.writeUInt16LE(1, 8); end.writeUInt16LE(1, 10); end.writeUInt32LE(dir.length, 12); end.writeUInt32LE(before.length, 16);
    return Buffer.concat([before, dir, end]); };
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-tdb-'));
  gametdb.init({ dir: tmp, norm, download: async () => false });
  const parsed = { sw: gametdb._t.parse(xml, /^Switch$/), n3ds: gametdb._t.parse(xml, /^(3DS|New3DS)$/) };
  for (const id of ['sw', 'n3ds']) fs.writeFileSync(path.join(tmp, `gametdb-${id}.json`), JSON.stringify({ v: 1, at: Date.now(), map: parsed[id].map }));
  const asks = [['sw', ['Zeta Run'], []], ['sw', ['Nothing', 'zeta run!'], ['EN', 'JA']], ['sw', ['Omega and Quest: The Long Way'], ['JA']], ['sw', ['Sigma'], ['EN']], ['sw', ['Tau Racers'], []], ['sw', ['Shop Only'], []], ['n3ds', ['Phi & Rho'], ['FR']], ['n3ds', ['Alpha Tale'], []], ['gc', ['Zeta Run'], []]];
  const covers = []; for (const [id, names, prefer] of asks) covers.push({ console: id, names, prefer, found: await gametdb.fullCovers(id, names, prefer) });
  fs.rmSync(tmp, { recursive: true, force: true });
  return { xml, stored: zip(false).toString('base64'), deflated: zip(true).toString('base64'), read: gametdb._t.xmlOf(zip(true)) === xml && gametdb._t.xmlOf(zip(false)) === xml, parsed, covers };
}
// ---- SteamGridDB (main/steamgrid.js) against a stand-in for the site: made-up games, posters, logos and keys ----
async function steamgridFixtures() {
  const steamgrid = require('../main/steamgrid'), covers = require('../main/covers');
  const cdn = 'https://cdn2.steamgriddb.com', grid = (id, more = {}) => ({ id, url: `${cdn}/grid/${id}.png`, thumb: `${cdn}/thumb/${id}.jpg`, width: 600, height: 900, style: 'alternate', author: { name: 'Made Up Artist' }, score: 3, mime: 'image/png', nsfw: false, humor: false, ...more });
  const logo = (n, more = {}) => ({ id: n, url: `${cdn}/logo/${n}.png`, width: 800, height: 310, nsfw: false, humor: false, ...more });
  const ok = data => ({ status: 200, body: { success: true, data } }), find = t => '/search/autocomplete/' + encodeURIComponent(t);
  const q = '?dimensions=600x900,342x482,660x930&styles=alternate,white_logo,no_logo,material,blurred&nsfw=false&humor=false&epilepsy=false', lq = '?types=static&nsfw=false&humor=false&epilepsy=false';
  const site = {
    '/games/steam/4242': ok({ id: 77, name: 'Zeta Run' }),
    ['/grids/steam/4242' + q]: ok([grid(1001), grid(1002, { nsfw: true }), grid(1003, { humor: true }), grid(1004, { width: 920, height: 430 }), grid(1005, { url: 'http://cdn2.steamgriddb.com/grid/1005.png' }),
      grid(1006, { url: 'https://steamgriddb.com.example.org/grid/1006.png' }), grid('1007', { url: `${cdn}/grid/1007.JPEG?x=1`, thumb: 'https://example.org/thumb.jpg', width: '342', height: '482', style: 'material', author: null, score: undefined }),
      grid(1008, { url: `${cdn}/file/1008`, mime: 'image/webp', style: 'unknown_style', author: { name: 'A very long name of somebody who made this poster' } }), grid(0), grid(1010, { width: 512, height: 512, style: 'no_logo', mime: '' }), null, grid(1011, { url: `${cdn}/a b.png` })]),
    [find('Omega Quest')]: ok([{ id: 5, name: 'Omega Quest II', release_date: 1e9 }, { id: 6, name: 'Omega Quest' }, { id: 0, name: 'Omega Quest' }, null, { id: 7 }]),
    ['/grids/game/6' + q]: ok(Array.from({ length: 45 }, (_, i) => grid(2000 + i, { style: ['white_logo', 'blurred', 'alternate'][i % 3], score: 45 - i }))),
    ['/logos/game/6' + lq]: ok([logo(1), logo(2, { url: `${cdn}/logo/2.jpg` }), logo(3, { nsfw: true }), logo(4, { width: 0 }), ...Array.from({ length: 14 }, (_, i) => logo(10 + i, { width: 400 + i }))]),
    [find("Sigma's Tale: Deluxe Edition")]: ok([{ id: 9, name: 'Sigma Tales' }]), [find("Sigma's Tale")]: ok([{ id: 10, name: "Sigma's Tale" }, { id: 11, name: "Sigma's Tale 2" }]), ['/grids/game/10' + q]: ok([grid(3001)]),
    [find('Nothing Like It')]: ok([]), [find('Tau Racers')]: { status: 200, body: { success: false, errors: ['nothing'] } }, [find('Phi & Rho: Ünï/2')]: ok([{ id: 12, name: 'Phi and Rho Uni 2' }]), ['/grids/game/12' + q]: ok({ not: 'a list' }),
    [find('Slow Game')]: { status: 429, retry: '5', body: {} }, [find('zelda')]: ok([]),
  };
  const asked = []; let key = '';
  const fetch = async (url, o) => {
    const k = o.headers.Authorization.replace(/^Bearer /, ''), p = url.replace('https://www.steamgriddb.com/api/v2', ''); asked.push(p);
    if (k === 'offline-key') throw new Error('fetch failed'); if (k === 'slow-key') throw Object.assign(new Error('x'), { name: 'TimeoutError' });
    const r = k === 'bad-key' ? { status: 401 } : k === 'k500' ? { status: 500 } : site[p] || { status: 404 };
    return { status: r.status, ok: r.status >= 200 && r.status < 300, headers: { get: n => n === 'retry-after' ? r.retry || null : null }, json: async () => { if (k === 'junk-key') throw new Error('bad'); return r.body; } };
  };
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-sgdb-')), start = () => { asked.length = 0; steamgrid.init({ key: () => key, dataDir: fs.mkdtempSync(path.join(tmp, 'd')), fetch }); };
  const game = (title, appid) => appid ? { appid, title } : { title, names: covers.lookupNames(title) };
  const games = [game('Zeta Run', '4242'), game('Omega Quest'), game("Sigma's Tale: Deluxe Edition"), game('Nothing Like It'), game('Gone', '999'), game(''), game('Tau Racers'), game('Phi & Rho: Ünï/2'), game('Omega Quest')];
  try {
    const out = { site, games };
    start(); out.nokey = await steamgrid.posters(games[0], covers.sgMatch);
    key = ' made-up-key '; out.posters = []; for (const g of games) out.posters.push(await steamgrid.posters(g, covers.sgMatch));
    out.ids = []; for (const g of [games[0], games[4], games[1]]) out.ids.push(await steamgrid.gameId(g, covers.sgMatch));
    out.logos = []; for (const g of [games[1], games[0], games[3]]) out.logos.push(await steamgrid.logos(g, covers.sgMatch));
    out.tests = []; for (const k of ['', 'bad-key', 'k500', 'offline-key', 'slow-key', 'junk-key', 'another-key']) out.tests.push([k, await steamgrid.test(k)]);
    const c = steamgrid._t.cacheGet(); out.cache = { ids: Object.fromEntries(Object.entries(c.ids).map(([k, v]) => [k, v[0]])), lists: Object.fromEntries(Object.entries(c.lists).map(([k, v]) => [k, v[1]])) };
    out.asked = [...asked];
    // a pause the site asks for: nothing more is sent
    start(); out.paused = [await steamgrid.posters(game('Slow Game'), covers.sgMatch), await steamgrid.posters(games[1], covers.sgMatch), await steamgrid.test('another-key')]; out.pausedAsked = [...asked];
    // a key the site refuses isn't sent again
    start(); key = 'bad-key'; out.refused = [await steamgrid.posters(games[1], covers.sgMatch), steamgrid.enabled(), await steamgrid.posters(games[0], covers.sgMatch)]; out.refusedAsked = [...asked];
    return out;
  } finally { fs.rmSync(tmp, { recursive: true, force: true }); }
}
// ---- ScreenScraper (main/screenscraper.js) against a stand-in for the site: a made-up account, games and pictures ----
async function screenscraperFixtures() {
  const ss = require('../main/screenscraper'), covers = require('../main/covers');
  const cdn = 'https://neoclone.screenscraper.fr/api2/mediaJeu.php', media = (type, region, format, more = {}) => ({ type, parent: 'jeu', region, format, url: `${cdn}?devid=d&devpassword=dp&softname=x&ssid=u&sspassword=p&systemeid=4&jeuid=1&media=${type}(${region})`, ...more });
  const user = { maxthreads: '2', maxrequestspermin: '6000', requeststoday: '5', maxrequestsperday: '20000' };
  const ok = (r, ssuser = user) => ({ status: 200, text: JSON.stringify({ response: { ssuser, ...r } }) });
  const png = Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47]), Buffer.alloc(80, 7)]).toString('base64');
  const site = {
    'jeuInfos.php|Zeta Run (USA).sfc': ok({ jeu: { id: '101', medias: [media('box-2D', 'us', 'png'), media('box-2D', 'eu', 'JPG'), media('box-2D-back', 'cus', 'jpeg'), media('box-2D-side', 'jp', 'webp'), media('box-texture', 'wor', 'png'), media('support-2D', 'ss', ''),
      media('box-2D', 'kr', 'png', { url: 'http://www.screenscraper.fr/medias/4/101/box-2D(kr).png' }), media('box-2D', 'us', 'png', { parent: 'rom' }), media('screenshot', 'us', 'png'), media('box-2D', 'us', 'png', { url: 'https://example.org/mediaJeu.php?media=box-2D' }),
      media('box-2D', 'A Very Long Region', 'png', { parent: undefined }), null, media('box-2D', 'fr', 'png', { url: '' })] } }),
    'jeuInfos.php|Omega Quest.sfc': { status: 404, text: '' },
    'jeuRecherche.php|Omega Quest': ok({ jeux: [{ id: '201', noms: [{ region: 'us', text: 'Omega Quest II' }] }, { id: '202', noms: [{ text: 'Omega no Tabi' }, { text: 'Omega Quest' }], medias: [] }, { id: 0, noms: [{ text: 'Omega Quest' }] }, { id: 203, noms: 'Omega Quest' }] }),
    'jeuInfos.php|id:202': ok({ jeu: { id: 202, medias: [media('box-2D', 'wor', 'png')] } }),
    'jeuInfos.php|Nothing Like It.sfc': { status: 200, text: 'Erreur : Rom/Iso/Dossier non trouvée !' }, 'jeuRecherche.php|Nothing Like It': ok({ jeux: [] }),
    'jeuInfos.php|Tau Racers.sfc': { status: 400, text: 'Erreur : Champ crc, md5 ou sha1 erroné' }, 'jeuRecherche.php|Tau Racers': ok({ jeux: [{ id: 301, noms: [{ text: 'Tau Racers' }], medias: [media('support-2D', 'us', 'png')] }] }),
    'jeuInfos.php|Sigma (USA).iso': ok({ jeu: { id: '0' } }), 'jeuRecherche.php|Sigma': ok({}),
    'jeuInfos.php|Phi Folder': ok({ jeu: { id: 'abc', medias: [media('support-2D', 'eu', 'png')] } }),
    'jeuInfos.php|Rho.sfc': { status: 200, text: 'not json at all' },
    'ssuserInfos.php|': ok({}),
    'mediaJeu.php|box-2D(us)': { status: 200, b64: png }, 'mediaJeu.php|box-2D(eu)': { status: 200, b64: Buffer.from('NOMEDIA').toString('base64') }, 'mediaJeu.php|support-2D(ss)': { status: 500, b64: '' }, 'mediaJeu.php|box-texture(wor)': { status: 429, b64: '' },
  };
  // what the site says however it's asked, by the name of a mood
  const moods = { refusedDev: { status: 403, text: 'Erreur de login : Vérifier vos identifiants développeur !' }, refused: { status: 403, text: 'Erreur de login' }, loginWords: { status: 200, text: 'Erreur de login : Vérifier vos identifiants !' }, quota: { status: 430, text: '' }, quotaWords: { status: 200, text: 'Votre quota de scrape est dépassé' },
    closed: { status: 401, text: '' }, down: { status: 423, text: '' }, blocked: { status: 426, text: '' }, rate: { status: 429, text: '' }, unknown: { status: 431, text: '' }, broken: { status: 500, text: '' }, teapot: { status: 418, text: '' }, junk: { status: 200, text: '{"response":' },
    empty: { status: 200, text: '{"response":{}}' }, usedUp: ok({}, { ...user, requeststoday: 20000 }), few: ok({}, { maxthreads: 99, maxrequestspermin: 'x', requeststoday: '7' }), offline: 'offline', slow: 'slow' };
  const asked = []; let mood = '', creds = {};
  const fetch = async url => {
    asked.push(url); const u = new URL(url), file = u.pathname.split('/').pop(), q = u.searchParams;
    const r = mood ? moods[mood] : site[file + '|' + (q.get('gameid') ? 'id:' + q.get('gameid') : q.get('romnom') || q.get('recherche') || q.get('media') || '')] || { status: 404, text: '' };
    if (r === 'offline') throw new Error('fetch failed'); if (r === 'slow') throw Object.assign(new Error('x'), { name: 'AbortError' });
    return { status: r.status, ok: r.status >= 200 && r.status < 300, text: async () => r.text, arrayBuffer: async () => Buffer.from(r.b64 || '', 'base64') };
  };
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-ss-')), rel = v => JSON.parse(JSON.stringify(v).split(JSON.stringify(tmp).slice(1, -1)).join('<tmp>').split(JSON.stringify(tmp.toLowerCase()).slice(1, -1)).join('<tmp>'));
  const start = () => { asked.length = 0; mood = ''; ss.init({ creds: () => creds, dataDir: fs.mkdtempSync(path.join(tmp, 'd')), fetch }); };
  const disc = Buffer.alloc(64); disc.write('GSGE01', 0, 'latin1');
  const files = { 'Zeta Run (USA).sfc': Buffer.from('zeta run rom bytes').toString('base64'), 'Omega Quest.sfc': Buffer.from('omega').toString('base64'), 'Nothing Like It.sfc': Buffer.from('n').toString('base64'), 'Tau Racers.sfc': Buffer.from('tau racers').toString('base64'),
    'Sigma (USA).iso': disc.toString('base64'), 'Rho.sfc': Buffer.from('rho').toString('base64'), 'Empty.sfc': '' };
  for (const [n, b] of Object.entries(files)) fs.writeFileSync(path.join(tmp, n), Buffer.from(b, 'base64')); fs.mkdirSync(path.join(tmp, 'Phi Folder'));
  const at = n => path.join(tmp, n), game = (n, title, more = {}) => ({ file: n ? at(n) : '', title, ...more });
  const games = [['snes', game('Zeta Run (USA).sfc', 'Zeta Run')], ['snes', game('Omega Quest.sfc', 'Omega Quest')], ['snes', game('Nothing Like It.sfc', 'Nothing Like It')], ['snes', game('Tau Racers.sfc', 'Tau Racers')], ['gc', game('Sigma (USA).iso', 'Sigma')],
    ['sw', game('Phi Folder', 'Phi', { sizeBytes: 12345 })], ['pc', game('Zeta Run (USA).sfc', 'Zeta Run')], ['snes', game('', 'Zeta Run')], ['snes', game('Gone.sfc', '')], ['snes', game('Zeta Run (USA).sfc', 'Zeta Run')], ['snes', game('Rho.sfc', 'Rho')], ['snes', game('Empty.sfc', '')]];
  const match = (list, g) => covers.matchName(list.map(x => ({ ...x, n: covers.norm(x.name) })), g.title), opts = { match, discId: covers.discId };
  try {
    const out = { site, moods, files, games: rel(games) };
    out.clean = ['http://neoclone.screenscraper.fr/api2/mediaJeu.php?devid=d&devpassword=dp&softname=x&ssid=u&sspassword=p&systemeid=4&jeuid=1&media=box-2D(us)', 'https://WWW.ScreenScraper.fr:8443/a b/c.png?SSID=1&x=a%20b+c&Output=xml', 'https://screenscraper.fr/x.png?ssid=1&devid=2',
      'https://screenscraper.fr/x.png?a=1%20b&c=d+e', 'https://screenscraper.fr/x.png', 'http://www.screenscraper.fr:80/x?q=ü&ssid=1#frag', 'https://screenscraper.fr.example.org/x.png', 'https://notscreenscraper.fr/x.png', 'ftp://screenscraper.fr/x.png', 'screenscraper.fr/x.png', '',
      'https://screenscraper.fr/' + 'a'.repeat(1480), '  https://screenscraper.fr/x.png?a=b&sspassword=  '].map(u => [u, ss.clean(u)]);
    out.isMedia = ['https://neoclone.screenscraper.fr/api2/mediaJeu.php?media=box-2D(us)', 'https://screenscraper.fr/api2/MEDIAJEU.PHP', 'https://screenscraper.fr/medias/x.png', 'https://example.org/mediaJeu.php', 'nothing'].map(u => [u, ss.isMedia(u)]);
    start(); out.noAccount = [await ss.candidates('snes', games[0][1], 'front', opts), await ss.test(), ss.state()];
    creds = { user: ' made up user ', pass: 'p&ss word' };
    out.lookups = []; for (const [sys, g] of games) { let r; try { r = await ss._t.lookup(sys, g, opts); } catch (e) { r = { error: e.code, words: e.message }; } out.lookups.push(r); }
    out.candidates = []; for (const [n, part, canCut] of [[0, 'front', true], [0, 'back', true], [0, 'spine', true], [0, 'spine', false], [0, 'disc', true], [0, 'manual', true], [1, 'front', true], [3, 'disc', true], [5, 'disc', true], [2, 'front', true]]) out.candidates.push([n, part, canCut, await ss.candidates(games[n][0], games[n][1], part, { ...opts, canCut })]);
    out.unseen = await ss.candidates('nes', games[0][1], 'front', { ...opts, cacheOnly: true });
    out.test = await ss.test(); out.state = ss.state();
    const m = out.lookups[0].media, dl = async (u, n) => { const dest = at(n); let r; try { r = await ss.download(u, dest); } catch (e) { r = 'error: ' + e.message; } return [u, r, fs.existsSync(dest) ? fs.readFileSync(dest).toString('base64') : null]; };
    out.downloads = [await dl('https://screenscraper.fr/medias/x.png', 'a.png'), await dl(m[0].url, 'b.png'), await dl(m[1].url, 'c.jpg'), await dl(m[5].url, 'd.png'), await dl(m[4].url, 'e.png'), await dl(m[0].url, 'f.png')];
    out.paused = ss.state().paused;
    const c = ss._t.cacheGet(); out.cache = rel({ games: Object.fromEntries(Object.entries(c.games).map(([k, v]) => [k, [v[0], v[2] === undefined ? null : v[2]]])), hashes: Object.values(c.hashes) });
    out.asked = [...asked];
    out.tests = [];
    for (const [md, cr] of [['refusedDev', {}], ['refusedDev', { devId: 'dev', devPass: 'dp' }], ['refused', { devPass: 'dp' }], ['loginWords', {}], ['quota', {}], ['quotaWords', {}], ['closed', {}], ['down', {}], ['blocked', {}], ['rate', {}], ['unknown', {}], ['broken', {}], ['teapot', {}], ['junk', {}],
      ['empty', {}], ['usedUp', {}], ['few', {}], ['offline', {}], ['slow', {}], ['', { user: '', pass: '', devId: ' dev ', devPass: 'dp' }], ['', { user: 'only a name' }]]) {
      creds = { user: 'made up user', pass: 'p&ss word', ...cr }; start(); mood = md;
      const t = await ss.test(), st = ss.state(), after = await ss.candidates('snes', games[1][1], 'front', opts);
      out.tests.push({ mood: md, creds: cr, test: t, state: st, after: after.length, asked: [...asked] });
    }
    return out;
  } finally { fs.rmSync(tmp, { recursive: true, force: true }); }
}
// ---- the Home menu's hotkeys (main/homemenu.js): which emulator a program is, and its keys with a console's own over them ----
function homekeysFixtures() {
  const hm = require('../main/homemenu');
  const exes = ['C:\\Emu\\Dolphin.exe', '/opt/retroarch/retroarch', 'RetroArch-Win64\\retroarch.exe', 'my-retroarch.exe', 'duckstation-qt-x64-ReleaseLTCG.exe', 'pcsx2-qt.exe', 'PPSSPPWindows64.exe', 'Cemu.exe', 'eden.exe', 'yuzu.exe', 'Ryujinx.exe', 'xenia_canary.exe',
    'shadPS4.exe', 'rpcs3.exe', 'azahar.exe', 'citra-qt.exe', 'mesen.exe', ''];
  const kinds = []; for (const sys of ['snes', 'gc', 'wii', 'ps2', 'psp', 'wiiu', 'sw', 'sw2', 'x360', 'ps4', 'ps3', 'n3ds', 'pc', 'media']) for (const exe of exes) kinds.push([sys, exe, hm.emulatorKind(sys, exe)]);
  const overrides = [undefined, {}, { pause: 'p' }, { save: '', load: null, shot: 'ctrl+shift+s', slot: 3 }, { pause: '', slot: 0 }, { slot: '2', other: 'x' }];
  const keys = []; for (const [sys, exe] of [['gc', 'Dolphin.exe'], ['ps2', 'pcsx2-qt.exe'], ['psp', ''], ['sw', 'eden.exe'], ['wiiu', 'Cemu.exe'], ['x360', ''], ['ps3', 'rpcs3.exe'], ['n3ds', 'azahar.exe'], ['snes', 'retroarch.exe'], ['psx', 'duckstation-qt.exe'], ['pc', 'game.exe']])
    for (const o of overrides) keys.push({ system: sys, exe, overrides: o === undefined ? null : o, keys: hm.hotkeysFor(sys, exe, o) });
  return { kinds, keys };
}
// ---- Export everything / Import everything (main/dataexport.js) over a made-up install: what an export holds, an
// update of it, and what an import puts where ----
async function dataexportFixtures() {
  const dx = require('../main/dataexport');
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-dx-')), at = (...p) => path.join(tmp, ...p);
  const artDirs = sc => [sc && sc.artDir, ...((sc && sc.romDirs) || []).flatMap(d => [path.join(d, 'covers'), path.join(d, 'media', 'covers'), path.join(d, 'boxart')])].filter(Boolean);
  const cfg = { systems: { gc: { romDirs: ['<tmp>/Games/gc'] }, psx: { romDirs: ['<tmp>/Games/psx-gone'] }, snes: { romDirs: ['<tmp>/Games/snes'], artDir: '<tmp>/A/Library/own' } } };
  const tree = { 'A/data/config.json': JSON.stringify(cfg), 'A/data/boxart-cache/launchbox-made-up.json': '{"big":1}', 'A/data/Local Storage/leveldb/000003.log': 'ui', 'A/data/Cache/Cache_Data/f_000001': 'x', 'A/data/Cookies': 'x', 'A/data/Cookies-journal': 'x',
    'A/data/legendary/user.json': '{"token":"made up"}', 'A/data/rustyshelf.log': 'x', 'A/data/companion.json': '{"pair":"made up"}', 'A/data/Preferences': 'x', 'A/data/part.tmp': 'x', 'A/data/pc-covers/zeta.jpg': 'cover', 'A/data/sub/Cache/kept.bin': 'kept',
    'A/Library/Games/covers/Zeta Run.png': 'cover', 'A/Library/own/Omega Quest.png': 'own', 'A/themes/gc/logo.png': 'logo', 'A/Custom themes/mine/theme.json': '{}', 'A/Songs/tune.mid': 'notes', 'A/Other/left.txt': 'not exported',
    'Lib/gc/covers/Sigma.png': 'in the library', 'Games/gc/Zeta Run (USA).iso': 'game', 'Games/gc/covers/Zeta Run (USA).png': 'beside', 'Games/gc/manuals/Zeta Run (USA)/1.jpg': 'page', 'Games/gc/boxart/Tau.png': 'boxart', 'Games/snes/Omega Quest.sfc': 'game',
    'Games/snes/covers/Omega Quest.png': 'snes beside', 'B/data/config.json': '{"systems":{}}', 'Other/gc/keep.txt': 'x', 'C/data/config.json': JSON.stringify({ systems: { gc: { romDirs: ['<tmp>/Other/gc'] } } }) };
  const here = v => JSON.parse(JSON.stringify(v).split('<tmp>').join(JSON.stringify(tmp).slice(1, -1))), there = v => JSON.parse(JSON.stringify(v).split(JSON.stringify(tmp).slice(1, -1)).join('<tmp>'));
  for (const [rel, body] of Object.entries(tree)) { fs.mkdirSync(path.dirname(at(rel)), { recursive: true }); fs.writeFileSync(at(rel), rel.endsWith('config.json') ? JSON.stringify(here(JSON.parse(body))) : body); }
  for (const d of ['out', 'out2']) fs.mkdirSync(at(d));
  // every file under a folder, with what's in it (a settings backup under one name, whenever it was made)
  const listing = root => { const out = {}; const walk = (d, rel) => { for (const e of fs.readdirSync(d, { withFileTypes: true }).sort((a, b) => a.name < b.name ? -1 : 1)) { const f = path.join(d, e.name), r = rel ? rel + '/' + e.name : e.name;
    if (e.isDirectory()) walk(f, r); else if (e.name !== 'manifest.json') out[r.replace(/config-before-import-[^/]*\.json$/, 'config-before-import.json')] = there(fs.readFileSync(f, 'utf8')); } }; walk(root, ''); return out; };
  const plain = r => { const { dir, bytes, from, ...rest } = r; return there(rest); };
  const base = { appRoot: at('A'), dataDir: at('A', 'data'), cfg: here(cfg), artDirsFor: artDirs, artLib: at('Lib'), version: '9.9.9' };
  try {
    // (tmpLength: the settings name this made-up folder four times, so their size goes with its name's length)
    const out = { tree, cfg, tmpLength: tmp.length };
    out.size = await dx.exportSize(base);
    const steps = [], r = await dx.exportAll({ ...base, dest: at('out'), onStep: st => steps.push([st.i, st.of, st.label]) });
    const manifest = () => { const { at: _when, first, ...m } = JSON.parse(fs.readFileSync(path.join(r.dir, 'manifest.json'), 'utf8')); return there({ ...m, first: !!first }); };
    out.first = { result: plain(r), named: /^RustyShelf export \d{4}-\d\d-\d\d \d\d-\d\d$/.test(path.basename(r.dir)), steps, manifest: manifest(), holds: listing(r.dir) };
    out.again = plain(await dx.exportAll({ ...base, dest: at('out') }));
    fs.writeFileSync(at('A/Library/Games/covers/Phi.png'), 'new cover'); fs.writeFileSync(at('A/themes/gc/logo.png'), 'changed logo');
    out.changed = { result: plain(await dx.exportAll({ ...base, dest: r.dir })), manifest: manifest(), holds: listing(r.dir) };
    let said = ''; try { await dx.exportAll({ ...base, dest: at('out2'), control: { cancelled: true } }); } catch (e) { said = e.message; } out.cancelled = said;
    try { await dx.exportAll({ ...base, dest: at('nowhere') }); } catch (e) { out.noFolder = e.message; }
    // a second PC: the same games folder, the art beside the games gone
    for (const d of ['Games/gc/covers', 'Games/gc/manuals', 'Games/gc/boxart', 'Games/snes/covers']) fs.rmSync(at(d), { recursive: true });
    const im = await dx.importAll({ src: r.dir, appRoot: at('B'), dataDir: at('B', 'data'), artDirsFor: artDirs, artLib: '', readCfg: () => JSON.parse(fs.readFileSync(at('B/data/config.json'), 'utf8')) });
    out.imported = { result: plain(im), b: listing(at('B')), games: listing(at('Games')) };
    // a third: no such games folder; its GameCube games are somewhere else, its other consoles nowhere
    fs.rmSync(at('Games'), { recursive: true });
    const c0 = JSON.parse(fs.readFileSync(at('C/data/config.json'), 'utf8'));
    const im2 = await dx.importAll({ src: r.dir, appRoot: at('C'), dataDir: at('C', 'data'), artDirsFor: artDirs, artLib: at('LibC'), readCfg: () => c0 });
    out.elsewhere = { result: plain(im2), c: listing(at('C')), other: listing(at('Other')), lib: fs.existsSync(at('LibC')) ? listing(at('LibC')) : null };
    try { await dx.importAll({ src: at('Other'), appRoot: at('C'), dataDir: at('C', 'data'), artDirsFor: artDirs }); } catch (e) { out.notOne = e.message; }
    fs.writeFileSync(at('Other/manifest.json'), '{"app":"Something else"}');
    try { await dx.importAll({ src: at('Other'), appRoot: at('C'), dataDir: at('C', 'data'), artDirsFor: artDirs }); } catch (e) { out.notOurs = e.message; }
    return out;
  } finally { fs.rmSync(tmp, { recursive: true, force: true }); }
}
// ---- the update notice (main/updates.js): which of two versions is newer, and the newest of made-up releases ----
async function updatesFixtures() {
  const https = require('https'), { EventEmitter } = require('events'), updates = require('../main/updates');
  const versions = ['1.82.0', 'v1.82.0', '1.82', '1', '1.82.0-beta.1', '1.82.0-beta.2', '1.82.0-beta.10', '1.82.0-beta', '1.82.0-alpha.3', '1.82.0-rc.1', '1.82.0-beta.1.x', '1.82.0-1', '1.82.0-x', '2.12.4.284', '2.12.4+284', '2.12.4+9', '2.12.4', '2.12.5-beta.1',
    ' 1.9.0 ', '1.9.0+0', '0.1.0', '10.0', '1.10.0', '1.9.10', 'nonsense', '', 'v2', '1.2.3.4-beta.1', '1.2.3-beta-x.1'];
  const compares = []; for (const a of versions) for (const b of versions) compares.push([a, b, Math.sign(updates.compare(a, b))]);
  const rel = (tag, more = {}) => ({ tag_name: tag, html_url: 'https://github.com/zearkreast/rustyshelf/releases/tag/' + tag, body: 'Notes for ' + tag, draft: false, assets: [{ name: `RustyShelf-${tag}.exe`, browser_download_url: `https://example.org/${tag}.exe`, size: 1234 }], ...more });
  const lists = [[rel('v1.9.0.5'), rel('v1.9.0.12'), rel('1.9.0-beta.3'), rel('v2.0.0', { draft: true }), rel('not a version'), null, rel('v1.9.0.12', { body: 'second of the same' })], [rel('v1.8.0', { body: 'ü'.repeat(2100), assets: null })],
    [], { message: 'rate limited' }, [rel('1.9.0-beta.2', { body: null }), rel('1.9.0-beta.10')], [rel('0.1.0')]];
  const real = https.get, checks = [];
  try {
    for (const list of lists) for (const current of ['1.9.0+7', '0.1.0', '1.9.0-beta.5']) {
      https.get = (u, _opts, cb) => { const res = new EventEmitter(); res.statusCode = 200; res.setEncoding = () => {}; res.resume = () => {};
        setImmediate(() => { cb(res); res.emit('data', JSON.stringify(list)); res.emit('end'); }); const req = new EventEmitter(); req.setTimeout = () => {}; req.destroy = () => {}; return req; };
      const { checkedAt, current: _c, ...r } = await updates.check(current, true); checks.push({ list, current, newest: r });
    }
  } finally { https.get = real; }
  return { compares, checks };
}
// ---- Add games (main/addgames.js): a made-up folder of games scanned, names made into titles, and a made-up
// Playnite library turned into what's added ----
async function addgamesFixtures() {
  const ag = require('../main/addgames');
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-add-')), there = v => JSON.parse(JSON.stringify(v).split(JSON.stringify(tmp).slice(1, -1)).join('<tmp>'));
  // each file with as many bytes as its number (the biggest program of a folder is the game)
  const tree = { 'Zeta Run/ZetaRun.exe': 50, 'Zeta Run/unins000.exe': 900, 'Zeta Run/tools/packer.exe': 800, 'Zeta Run/cover.png': 5, 'Omega_Quest.v1.2.3-GOG/bin/x64/game.exe': 30, 'Omega_Quest.v1.2.3-GOG/launcher.exe': 10, 'Sigma (v2.1)/Sigma.exe': 20,
    '[Repack] Tau Racers/Binaries/Win64/TauRacers-Win64-Shipping.exe': 70, '[Repack] Tau Racers/Binaries/Win64/CrashReportClient.exe': 300, 'Racing/Phi Kart/phikart.exe': 41, 'Racing/Phi Kart/editor.exe': 40, 'Racing/Phi Kart/server.exe': 39, 'Racing/Phi Kart/extra.exe': 38,
    'Racing/Phi Kart/fifth.exe': 37, 'Racing/Rho Rally/Game/rho.exe': 42, 'Racing/Rho Rally/art/rho.jpg': 3, '2D games/Retro/Alpha Tale/alpha.exe': 11, '2D games/Retro/Beta Kart/beta.exe': 12, '2D games/Gamma Force/gf.exe': 13, 'Only Installer/setup_only.exe': 9,
    'Zipped/game.zip': 8, 'Empty Thing/readme.txt': 2, '.hidden/secret.exe': 7, 'redist/vc.exe': 6, 'loose.exe': 14, 'Loose_Game-v1.0.exe': 15, 'notes.txt': 1, 'bin/thing.exe': 16, '___/prog.exe': 17, 'One Game Only/Deep/Deeper/one.exe': 18 };
  for (const [rel, n] of Object.entries(tree)) { const f = path.join(tmp, 'Games', rel); fs.mkdirSync(path.dirname(f), { recursive: true }); fs.writeFileSync(f, Buffer.alloc(n, 1)); }
  const names = ['Omega_Quest.v1.2.3-GOG', 'Sigma (v2.1)', '[Repack] Tau Racers', 'Zeta-Run_(GOG)', 'Phi.Kart.Build 1234', 'Rho [www.example.org]', 'v2 Alpha', 'Beta 2', 'Gamma (1.0b) Force', 'repack', '', '___', 'Tau  Racers   Deluxe', 'Kart v1.2.3a final', 'build7 thing', 'Über-Spiel'];
  const exes = ['Games/Zeta Run/ZetaRun.exe', 'Games/Omega/bin/x64/game.exe', 'Games/x64/game.exe', 'Games/Tau/Binaries/Win64/Tau-Win64-Shipping.exe', 'Games/prog.exe', 'Games/___/prog.exe', 'elsewhere/bin/Sigma_Tale.v2.exe'].map(e => path.join(tmp, e));
  // a Playnite library: every name, folder and id made up
  const EMPTY = '00000000-0000-0000-0000-000000000000', play = (Type, more = {}) => ({ IsPlayAction: true, Type, ...more });
  const lib = { platforms: [{ Id: 'p-gc', SpecificationId: 'nintendo_gamecube', Name: 'Nintendo GameCube' }, { Id: 'P-SNES', Name: 'Super Nintendo (SNES)' }, { Id: 'p-pc', SpecificationId: 'pc_windows', Name: 'PC (Windows)' }, { _id: 'p-odd', Name: 'Something Else' }, { Id: 'p-ds', Name: 'Handheld DS' }],
    emulators: [{ Id: 'e-dolphin', InstallDir: '{PlayniteDir}\\Emulation\\Dolphin' }, { Id: 'e-none' }],
    games: [{ Id: 'g1', Name: 'Zeta Run', PluginId: EMPTY, InstallDirectory: 'D:\\Games\\Zeta Run\\', GameActions: [play(0, { Path: '{InstallDir}\\zeta.exe', Arguments: '-w {InstallDirName} {PlayniteDir}' })], CoverImage: 'g1\\cover.png', BackgroundImage: 'https://example.org/bg.jpg', Favorite: true },
      { Id: 'g2', Name: 'Omega Quest', InstallDirectory: 'E:/Omega', GameActions: [{ Type: 'File', Path: 'bin/../bin/omega.exe' }], CoverImage: 'g2/missing.png', BackgroundImage: 'g2/bg.jpg' },
      { Id: 'g3', Name: 'Store Game', PluginId: 'ABCDEF01-2345-6789-abcd-ef0123456789', GameActions: [play(0, { Path: 'x.exe' })] }, { Name: 'Hidden One', Hidden: true, GameActions: [play(0, { Path: 'h.exe' })] },
      { Id: 'g5', Name: 'Link Game', GameActions: [play(1, { Path: 'steam://run/1' })] }, { Id: 'g5b', Name: 'Store Link', PluginId: 'ABCDEF01-2345-6789-abcd-ef0123456789', GameActions: [play('URL', { Path: 'x://y' })] },
      { Id: 'g6', Name: 'Sigma', PlatformIds: ['P-GC'], GameActions: [play(2, { EmulatorId: 'E-DOLPHIN' })], Roms: [{ Path: '{InstallDir}\\Sigma.iso' }], InstallDirectory: 'F:\\Roms\\GC' },
      { Id: 'g7', Name: 'Tau', PlatformIds: ['p-gc'], GameActions: [play('Emulator', { EmulatorId: 'e-none' })], Roms: [{ Path: 'f:\\roms\\gc\\Tau.iso' }] }, { Id: 'g7b', Name: 'Phi', PlatformIds: ['nowhere', 'p-gc'], Roms: [{ Path: 'G:/More/gc/Phi.rvz' }, { Path: 'ignored' }] },
      { Name: 'Rho', PlatformIds: ['p-snes'], Roms: [{ Path: 'D:\\snes\\rho.sfc' }] }, { Name: 'Odd', PlatformIds: ['p-odd'], GameActions: [play('Emulator')], Roms: [{ Path: 'x' }] }, { Name: 'Pc Rom', PlatformIds: ['p-pc'], Roms: [{ Path: 'a' }] },
      { Id: 'g11' }, null, { Name: 'No Rom', PlatformIds: ['p-gc'], GameActions: [play(2)] }, { Name: 'Kart DS', PlatformIds: ['p-ds'], Roms: [{ Path: 'rom.nds' }] },
      { _id: 'g14', Name: 'A very long name '.repeat(10), InstallDirectory: '', GameActions: [play(0, { Path: 'C:\\Long\\long.exe', Arguments: 'x'.repeat(1100) }), { Type: 0, Path: 'other.exe' }] },
      { Name: 'alpha tale', GameActions: [play(0, { Path: 'D:\\a\\alpha.exe' })], Favorite: 0 }, { Name: 'Two Actions', GameActions: [{ Type: 0, Path: 'one.exe' }, { Type: 0, Path: 'two.exe' }] }] };
  const found = { app: 'C:\\Playnite', library: path.join(tmp, 'pn', 'library') };
  fs.mkdirSync(path.join(found.library, 'files', 'g1'), { recursive: true }); fs.writeFileSync(path.join(found.library, 'files', 'g1', 'cover.png'), 'x'); fs.mkdirSync(path.join(found.library, 'files', 'g2')); fs.writeFileSync(path.join(found.library, 'files', 'g2', 'bg.jpg'), 'x');
  try {
    return there({ tree, scan: await ag.scanFolderAsync(path.join(tmp, 'Games')), few: (await ag.scanFolderAsync(path.join(tmp, 'Games'), 3)).games.length, gone: await ag.scanFolderAsync(path.join(tmp, 'nowhere')),
      clean: names.map(n => [n, ag.clean(n)]), titles: exes.map(e => [e, ag.titleFor(e, path.join(tmp, 'Games')), ag.titleFor(e)]), lib, plan: ag.plan(lib, found) });
  } finally { fs.rmSync(tmp, { recursive: true, force: true }); }
}
// ---- manuals (main/manuals.js): how a manual's title is matched to a game's, the searches and the download against
// a stand-in for the sites (made-up games and uploads), and the automatic pick ----
async function manualsFixtures() {
  const https = require('https'), { EventEmitter } = require('events'), { PassThrough } = require('stream'), M = require('../main/manuals');
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-man-')), there = v => JSON.parse(JSON.stringify(v).split(JSON.stringify(tmp).slice(1, -1)).join('<tmp>'));
  const titles = ['Zeta Run', 'Zeta Run 2', 'Zeta Run II: The Long Way', 'The Legend of Omega & Sigma', "Tau's Racers", 'Omega Quest - Dawn of the Kings', 'Phi Party 9 (USA)', 'Call of Rho 7 Black Ops', 'Sigma', 'A', 'Zeta Run + Zeta Run Resort', 'Final Omega XIII-2', 'Alpha 10', '', 'An Ode of the Tau'];
  const uploads = ['Zeta Run (GameCube) Manual', 'Zeta Run 2 - Instruction Booklet', 'Zeta Run Strategy Guide', 'Zeta Run III manual', 'Nintendo GameCube Zeta Run USA manual scan 2004', 'Zeta Run PS2 manual', 'Zeta Run Xbox 360 Manual', 'Legend of Omega and Sigma, The - Manual (Wii)',
    'Tau s Racers Game Boy Color', 'Tau Racers promotional poster', 'Sigma Chronicles: The Other Game Collection Manual', 'Phi Party 9 Wii U manual', 'Phi Party 8', 'Call of Rho Black Ops manual', 'Zeta Run Mode d\'emploi', 'Zeta Run (PlayStation Portable)', 'Zeta Run PlayStation 2', 'Zeta Run playstation',
    'Zeta Run Neo Geo Pocket Color', 'Zeta Run neo-geo pocket', 'Zeta Run Neo Geo', 'Zeta Run game boy advance', 'Zeta Run gameboy', 'Zeta Run xbox one', 'Zeta Run Xbox', 'Zeta Run Wii', 'Zeta Run WiiU', 'Zeta Run service manual', 'Zeta Run 1999 catalogue no 12345', ''];
  const systems = ['gc', 'wii', 'wiiu', 'ps2', 'psx', 'psp', 'x360', 'xbox', 'gb', 'gbc', 'neogeo', 'ngp', 'nds', 'sw2', 'nowhere'];
  const out = { norm: titles.concat(uploads).map(t => [t, M.norm(t), [...M.words(t)], [...M.seqNums(t)]]), parts: [], scores: [], foreign: [] };
  for (const a of titles) for (const b of uploads) { const p = M.simParts(a, b); out.parts.push([a, b, p.recall, p.noise, !!p.wrongSequel, M.similarity(a, b)]); }
  for (const sys of ['gc', 'wii', 'ps2', 'gb']) for (const t of uploads) for (const o of [{}, { lang: 'fre', downloads: 250 }, { lang: 'english', anyPlatform: true, downloads: '9000' }]) out.scores.push([sys, 'Zeta Run', t, o, M.scoreHit(sys, 'Zeta Run', t, o)]);
  for (const sys of systems) for (const t of uploads) out.foreign.push([sys, t, M.foreignPlatform(sys, t, false), M.foreignPlatform(sys, t, true)]);
  out.slugs = ["Tau's Racers (USA)", 'The Legend of Omega - Dawn', 'A Sigma & Rho Tale', 'Omega Quest, The - Second Part', 'Zeta Run!', '--Phi--', 'Ünï Çode', 'the'].map(t => [t, M.gdbSlugs(t)]);
  out.alt = ['Zeta Run III', 'Zeta Run 3', 'Final Omega XIII: Part IV', 'Alpha 14', 'Phi 2 and 10', 'Nothing here', 'vii lowercase'].map(t => [t, M.altNumbers(t)]);
  out.pageImages = [{ name: 'scan_01.jpg', source: 'original', format: 'JPEG', size: '200000' }, { name: 'scan_01_thumb.jpg', source: 'original', size: 200000 }, { name: '__ia_thumb.jpg' }, { name: 'x.PNG', source: 'derivative' }, { name: 'tile.jpg', format: 'Item Tile' },
    { name: 'small.jpg', size: 9000 }, { name: 'huge.png', size: 90 * 1048576 }, { name: 'nosize.jpeg' }, { name: 'book.pdf', source: 'original' }, { name: 'thumbs/x.jpg' }, { name: 'a/.thumbs/x.jpg' }, null, { name: 'zero.jpg', size: 0 }].map(f => [f, M.isPageImage(f)]);
  out.langs = [['eng', ''], ['English;French', ''], [['fre', 'eng'], ''], ['', 'Zeta Run Mode d\'emploi'], ['', 'Zeta Run Bedienungsanleitung'], ['', 'Zeta Run manuale'], ['', 'Manual de instrucciones'], ['', '取扱説明書'], ['xx', 'Plain'], ['JPN', ''], [undefined, 'Notice']].map(([r, t]) => [r === undefined ? null : r, t, M.langCode(r, t)]);
  out.regions = ['Zeta Run (PAL)', 'Zeta Run NTSC-J', 'Zeta Run USA', 'Zeta Run (Europe) (USA)', 'Zeta Run', 'us', 'Australia', 'ntsc', 'bus'].map(t => [t, M.regionOf(t)]);
  const cand = (id, title, language, score) => ({ id, title, language, score, english: !language || /^(eng|en)/.test(language) });
  const list = [cand('a', 'Zeta Run Manual', 'eng', 9), cand('b', 'Zeta Run Mode d\'emploi', 'fre', 5.5), cand('c', 'Zeta Run (PAL) Manual', '', 8), cand('d', 'Zeta Run Japan', 'jpn', 2), cand('e', 'Zeta Run Anleitung', 'ger', 2.5)];
  fs.writeFileSync(path.join(tmp, 'rejects.json'), JSON.stringify({ 'gc|rejecting': ['a'] })); M.setRejectStore(path.join(tmp, 'rejects.json'));
  out.prefs = { list, picks: [] };
  for (const pref of ['best', '', 'lang:fr', 'lang:de', 'lang:it', 'region:eur', 'region:jpn', 'nonsense']) for (const key of ['gc|plain', 'gc|rejecting']) { const p = M.prefPick(list, pref, key); out.prefs.picks.push([pref, key, p ? p.id : null, p ? p.score : null, list.map(c => M.prefMatch(c, pref))]); }

  // the sites: an address → what it answers (page: a file of that letter, 3000 times)
  const ia = q => `https://archive.org/advancedsearch.php?q=${encodeURIComponent(q)}&fl[]=identifier&fl[]=title&fl[]=language&fl[]=imagecount&fl[]=downloads&rows=12&output=json`;
  const q1 = (t, w) => `(${JSON.stringify(t)}) AND mediatype:texts AND (manual OR instruction OR booklet)` + (w ? ` AND (${w.map(x => JSON.stringify(x)).join(' OR ')})` : ''), q2 = t => `title:(${JSON.stringify(t)}) AND mediatype:texts AND (manual OR instruction OR booklet)`;
  const GC = ['gamecube', 'game cube', 'gcn', 'ngc'], docs = d => ({ json: { response: { docs: d } } }), doc = (identifier, title, more = {}) => ({ identifier, title, language: 'eng', imagecount: 12, downloads: 100, ...more });
  const gdb = 'https://www.gamesdatabase.org/game/nintendo-gamecube/', dl = 'https://archive.org/download/';
  const site = {
    [ia(q1('Zeta Run', GC))]: docs([doc('zeta-run-gc', 'Zeta Run (GameCube) Manual'), doc('zeta-guide', 'Zeta Run Strategy Guide'), doc('zeta-fr', 'Zeta Run Mode d\'emploi', { language: ['fre', 'eng'], downloads: 900 }), doc('zeta-ps2', 'Zeta Run PS2 manual', { imagecount: '30' }), doc('zeta-3', 'Zeta Run III manual'),
      doc('zeta-none', 'Zeta Run instruction booklet', { language: undefined, imagecount: undefined, downloads: 2000 }), doc('other', 'Something Else Entirely')]),
    [gdb + 'zeta-run']: { body: '<html><a href="/Media/Manual/Zeta_Run_GameCube_Manual.pdf">Manual</a></html>' },
    [ia(q1('Omega Quest', GC))]: docs([]), [ia(q2('Omega Quest'))]: docs([doc('omega-any', 'Omega Quest Manual', { language: 'English' })]), [gdb + 'omega-quest']: { body: 'Game Not Found' }, [gdb + 'omega-quest-']: { body: '<html>a page with no manual</html>' },
    [ia(q1('Sigma III', GC))]: docs([]), [ia(q1('Sigma 3', GC))]: docs([doc('sigma-3', 'Sigma 3 Instruction Manual')]),
    [ia(q1('Tau Racers + Tau Racers Resort', GC))]: docs([]), [ia(q1('Tau Racers', GC))]: docs([doc('tau', 'Tau Racers Manual (GameCube)')]),
    [ia(q1('Busy Game', GC))]: { status: 503 }, [ia(q1('Refused Game', GC))]: { status: 400 }, [ia(q1('Junk Game', GC))]: { body: 'not json' }, [ia(q1('Odd Answer', GC))]: { json: { nothing: true } }, [ia(q1('Slow Down', GC))]: { status: 429 },
    [ia(q1('Pdf Only', GC))]: { status: 503 }, [gdb + 'pdf-only']: { body: '<A HREF="https://www.gamesdatabase.org/Media/SYSTEM/Nintendo_GameCube/Manual/formated/Pdf_Only_PC_Game_Manual.PDF">x</A>' },
    [ia(q1('Phi "Quoted" Kart', GC).replace('\\"', '').replace('\\"', ''))]: docs([doc('phi', 'Phi Quoted Kart manual')]),
    [ia(q1('Zeta Run', null))]: docs([doc('zeta-wii', 'Zeta Run Wii Manual'), doc('zeta-pc', 'Zeta Run PC manual'), doc('zeta-fr', 'Zeta Run manuel', { language: 'fre' })]),
    // uploads to download
    'https://archive.org/metadata/counted': { json: { server: 'ia800.example.org', dir: '/1/items/counted', metadata: { imagecount: '3' }, files: [{ name: 'counted_jp2.zip' }, { name: 'counted.pdf' }] } },
    [dl + 'counted/page/n0_w1600.jpg']: { page: 'A' }, [dl + 'counted/page/n1_w1600.jpg']: { status: 500 }, [dl + 'counted/page/n2_w1600.jpg']: { page: 'C' },
    'https://ia800.example.org/BookReader/BookReaderImages.php?zip=%2F1%2Fitems%2Fcounted%2Fcounted_jp2.zip&file=counted_jp2%2Fcounted_0001.jp2&id=counted&scale=2&rotate=0': { page: 'B' },
    'https://archive.org/metadata/numbered': { json: { server: 's', dir: '/d', metadata: {}, files: [{ name: 'Numbered Scan_jp2.zip' }, { name: 'Numbered Scan_page_numbers.json' }] } }, [dl + 'numbered/Numbered%20Scan_page_numbers.json']: { json: { pages: [{}, {}] } },
    [dl + 'numbered/page/n0_w1600.jpg']: { page: 'D' }, [dl + 'numbered/page/n1_w1600.jpg']: { page: 'E' },
    'https://archive.org/metadata/scanned': { json: { server: 's', dir: '/d', files: [{ name: 'scanned_jp2.zip' }, { name: 'scanned_scandata.xml' }] } }, [dl + 'scanned/scanned_scandata.xml']: { body: '<book>' + '<page leafNum="0">x</page>\n<page\tleafNum="1"></page>'.padEnd(2100, ' ') + '</book>' },
    [dl + 'scanned/page/n0_w1600.jpg']: { page: 'F' }, [dl + 'scanned/page/n1_w1600.jpg']: { page: 'G' },
    'https://archive.org/metadata/guessed': { json: { server: 's', dir: '/d', files: [{ name: 'guessed_jp2.zip' }] } }, ...Object.fromEntries(Array.from({ length: 40 }, (_, i) => [dl + `guessed/page/n${i}_w1600.jpg`, { page: 'HIJ'[Math.min(i, 2)] }])),
    'https://archive.org/metadata/loose': { json: { files: [{ name: 'page 10.jpg', source: 'original', size: 30000 }, { name: 'page 2.PNG', source: 'original', size: 30000 }, { name: 'page 1.jpg', source: 'original', size: 30000 }, { name: '__ia_thumb.jpg' }, { name: 'page 3_thumb.jpg', source: 'original' }, { name: 'notes.txt' }] } },
    [dl + 'loose/page%201.jpg']: { page: 'K' }, [dl + 'loose/page%202.PNG']: { page: 'L' }, [dl + 'loose/page%2010.jpg']: { page: 'M' },
    'https://archive.org/metadata/broken': { json: { server: 's', dir: '/d', metadata: { imagecount: 3 }, files: [{ name: 'broken_jp2.zip' }] } }, [dl + 'broken/page/n0_w1600.jpg']: { page: 'N' }, [dl + 'broken/page/n2_w1600.jpg']: { body: 'tiny' },
    'https://archive.org/metadata/gone': { status: 404 },
    'https://www.gamesdatabase.org/Media/Manual/Zeta_Run_GameCube_Manual.pdf': { page: 'P' },
    // for the automatic pick: one game with a confident match, one with a poor one, one that can't be asked about
    [ia(q1('Alpha Tale', ['snes', 'super nintendo', 'super famicom']))]: docs([doc('counted', 'Alpha Tale Manual (SNES)')]),
    [ia(q1('Beta Kart', ['snes', 'super nintendo', 'super famicom']))]: docs([doc('beta-guide', 'Beta Kart Strategy Guide')]),
    [ia(q1('Gamma Force', ['snes', 'super nintendo', 'super famicom']))]: { status: 503 },
  };
  const asked = [], real = https.get;
  https.get = (url, opts, cb) => { asked.push(url); const r = site[url] || { status: 404 }, req = new EventEmitter(); req.setTimeout = () => {}; req.destroy = e => { if (e) req.emit('error', e); };
    setImmediate(() => { const res = new PassThrough(); res.statusCode = r.status || 200; res.headers = {}; cb(res); res.end(r.json ? JSON.stringify(r.json) : r.page ? r.page.repeat(3000) : r.body || ''); }); return req; };
  const holds = dir => { try { return Object.fromEntries(fs.readdirSync(dir).sort().map(f => { const b = fs.readFileSync(path.join(dir, f)); return [f, b.toString('latin1', 0, 1) + b.length]; })); } catch { return {}; } };
  try {
    out.site = site;
    out.searches = []; for (const [sys, title] of [['gc', 'Zeta Run'], ['gc', 'Omega Quest'], ['gc', 'Sigma III'], ['gc', 'Tau Racers + Tau Racers Resort'], ['gc', 'Busy Game'], ['gc', 'Refused Game'], ['gc', 'Junk Game'], ['gc', 'Odd Answer'], ['gc', 'Slow Down'], ['gc', 'Pdf Only'], ['gc', 'Phi "Quoted" Kart'], ['gc', 'Nothing At All'], ['pc', 'Zeta Run']]) {
      let r; try { r = await M.search(sys, title); } catch (e) { r = { error: e.message }; } out.searches.push([sys, title, r]); }
    out.elsewhere = await M.searchArchive('gc', 'Zeta Run', { anyPlatform: true });
    out.gdb = await M.searchGamesDatabase('gc', 'Zeta Run');
    out.fetched = []; for (const id of ['counted', 'numbered', 'scanned', 'guessed', 'loose', 'broken', 'gone']) { const dir = path.join(tmp, 'f-' + id); let r; try { r = await M.fetchManual(id, dir, 'Zeta Run (USA)', () => {}); } catch (e) { r = 'error: ' + e.message; } out.fetched.push([id, r, holds(dir)]); }
    // chosen by hand: an upload, GamesDatabase's PDF, and an address that isn't one of its PDFs
    const { CONSOLES } = require('../main/systems'), roms = path.join(tmp, 'roms'), sys = [{ def: CONSOLES.find(c => c.id === 'snes'), cfg: { romDirs: [roms], enabled: true } }];
    fs.mkdirSync(roms); for (const n of ['Alpha Tale (USA).sfc', 'Beta Kart (Europe).sfc', 'Gamma Force (USA).sfc', 'Delta Has One (USA).sfc']) fs.writeFileSync(path.join(roms, n), 'rom');
    fs.mkdirSync(path.join(roms, 'covers')); fs.writeFileSync(path.join(roms, 'covers', 'Delta Has One (USA)-manual-01.jpg'), 'page');
    out.chosen = there(await M.downloadChosen(sys, [{ systemId: 'snes', file: path.join(roms, 'Beta Kart (Europe).sfc'), id: 'gdb:https://www.gamesdatabase.org/Media/Manual/Zeta_Run_GameCube_Manual.pdf' }, { systemId: 'snes', file: path.join(roms, 'Gamma Force (USA).sfc'), id: 'gdb:https://example.org/x.pdf' },
      { systemId: 'snes', file: path.join(roms, 'Gamma Force (USA).sfc'), id: 'numbered' }], () => {}));
    out.chosenHolds = holds(path.join(roms, 'covers'));
    for (const f of fs.readdirSync(path.join(roms, 'covers'))) if (!f.startsWith('Delta')) fs.unlinkSync(path.join(roms, 'covers', f));
    M.setStore(path.join(tmp, 'auto.json'));
    out.auto = await M.autoFetch(sys); out.autoHolds = holds(path.join(roms, 'covers'));
    out.autoMemo = there(Object.fromEntries(Object.entries(JSON.parse(fs.readFileSync(path.join(tmp, 'auto.json'), 'utf8'))).map(([k, v]) => { const { at, ...rest } = v; return [k.split('/').pop(), rest]; })));
    asked.length = 0; out.autoAgain = [await M.autoFetch(sys), asked.length];
    return out;
  } finally { https.get = real; fs.rmSync(tmp, { recursive: true, force: true }); }
}
// The phone companion: QR codes, addresses, names, and a run of requests through the Electron app's own handler.
// Ids, tokens and PINs are random: the Rust test maps the ones written here to its own as they first appear.
async function companionFixtures() {
  const C = require('../main/companion'), { Readable, Writable } = require('stream'), url = require('url');
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-phone-'));
  const out = {};
  const long = n => Array.from({ length: n }, (_, i) => 'http://192.168.1.20:8730/#pin=0123456789'[i % 40]).join('');
  out.qr = ['HELLO', 'http://192.168.1.20:8730/#pin=123456', 'Zeta Run é ✓', ...[20, 50, 70, 100, 115, 150, 180, 213, 214].map(long)].map(text => { try { return { text, rows: C.qrMatrix(text).map(r => r.map(x => (x ? '1' : '0')).join('')) }; } catch (e) { return { text, error: e.message }; } });
  out.private = ['10.0.0.1', '172.16.5.4', '172.32.0.1', '172.15.0.1', '192.168.1.50', '192.169.1.1', '127.0.0.1', '8.8.8.8', '::1', '::ffff:192.168.1.9', '::ffff:8.8.8.8', '::ffff:1234.1.1.1', 'fe80::1%eth0', 'FE9A::2', 'fec0::1', '2001:db8::1', '300.1.1.1', '10.0.0.256', '', '10.0.0', 'localhost', '169.254.1.1'].map(a => [a, C.isPrivate(a)]);
  out.names = [['Kitchen <b>phone</b>!', ''], ['  ', 'Mozilla/5.0 (iPhone; CPU iPhone OS like Mac OS X)'], ['', 'Mozilla/5.0 (iPad; CPU OS like Mac OS X)'], ['', 'Mozilla/5.0 (Linux; Android 14; Tablet Q Build/AB1.2) Mobile'], ['', 'Mozilla/5.0 (Linux; Android 14) Mobile'], ['', 'Mozilla/5.0 (X11; Linux x86_64)'], ['x'.repeat(60), ''], ["Ümit’s (old) phone_2.0-b", ''], ['📱', 'Android; ; A very long made-up model name that goes on and on Build/1']].map(([g, ua]) => [g, ua, C._test.phoneName(g, ua)]);
  out.files = ['bsfile://local/' + encodeURIComponent('/x/Zeta Run.png') + '?v=3', 'bsfile://local/%E0%A4%A', 'file:///x/a%20b.png?x=1', 'https://example.invalid/a.png', ''].map(u => [u, C._test.fileOf(u)]);

  const U = f => url.pathToFileURL(path.join(tmp, f)).href;
  const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==', 'base64');
  fs.writeFileSync(path.join(tmp, 'zeta front.png'), PNG);
  const lib = [
    { id: 'home', name: 'Home', kind: 'home', games: [{ id: 'h1', title: 'Rho', kind: 'rom' }] },
    { id: 'snes', name: 'Super Shelf', kind: 'console', games: [
      { id: 'snes:Zeta Run.sfc', title: 'Zeta Run', kind: 'rom', file: 'Zeta Run.sfc', region: 'USA', sizeBytes: 524288, art: { front: U('zeta front.png'), back: U('zeta-back.jpg'), manual: [U('m1.png'), U('m2.png'), U('notes.svg')], manualPdf: U('zeta.pdf') }, info: { overview: 'A made-up game.', year: 1993, dev: 'Nobody', genres: ['Platform'], players: '', coop: null, extra: 'left out' } },
      { id: 'snes:Omega Quest.sfc', title: 'Omega Quest', kind: 'rom', file: 'Omega Quest.sfc', art: { icon: 'bsfile://local/' + encodeURIComponent(path.join(tmp, 'omega.webp')) + '?v=2', manualPdf: U('omega.pdf') } },
      { id: 'snes:Sigma.sfc', title: 'Sigma', kind: 'rom', file: 'Sigma.sfc' },
      { id: 'tile', title: 'Surprise me', kind: 'tile' }, null] },
    { id: 'steam', name: '', kind: 'pc', games: [{ id: 'steam-4242', title: 'Tau Racers', kind: 'steam', appid: 4242, art: { front: U('tau.svg'), icon: U('tau.gif'), hero: U('tau-hero.jpg') } }, { id: 'app-phi', title: 'Phi', kind: 'app' }] },
    { id: 'empty', name: 'Empty', kind: 'console', games: [] }, { nothing: true }, null];
  out.library = JSON.parse(JSON.stringify(lib).split(encodeURIComponent(tmp)).join('<encdir>').split(tmp).join('<dir>'));
  out.achievements = { 'Zeta Run': { earned: '3', total: 10.4, points: 55, pointsEarned: -2, mastered: 0 }, 'Omega Quest': { total: 0 } };

  const world = { running: null, hidden: [], glyphs: { style: 'xbox', swap: false }, homeMenu: true, playOk: true };
  let calls = [];
  C.init({ dataDir: tmp, webDir: path.join(ROOT, 'renderer', 'companion'), fontDir: path.join(ROOT, 'renderer', 'fonts'), version: '9.9.9', lanAddress: () => '127.0.0.1',
    library: () => lib, hidden: () => world.hidden, glyphs: () => world.glyphs, homeMenu: () => world.homeMenu, running: () => world.running, raFor: s => s === 'snes',
    play: async g => { calls.push(['play', g.shelf, g.id, g.title]); return world.playOk; }, quit: async () => { calls.push(['quit']); world.running = null; },
    input: i => calls.push(['input', i]), achievements: async (s, t) => out.achievements[t] || null });
  await C.set({ enabled: true, port: 18730 });
  if (!C._test.pinNow()) throw new Error('the companion did not start: ' + C.state().error);
  const sha = b => require('crypto').createHash('sha256').update(b).digest('hex');
  const fill = t => { const pin = C._test.pinNow(); return String(t).split('<pin>').join(pin).split('<notpin>').join(String((+pin + 1) % 1e6).padStart(6, '0')); };
  const ask = step => new Promise(done => {
    const req = Readable.from(step.body != null ? [Buffer.from(fill(step.body))] : []); req.method = step.method || 'GET'; req.url = step.url;
    req.headers = { host: '127.0.0.1:18730', ...(step.body != null && !(step.headers && 'content-type' in step.headers) ? { 'content-type': 'application/json' } : {}), ...(step.headers || {}) };
    for (const k of Object.keys(req.headers)) if (req.headers[k] == null) delete req.headers[k];
    req.socket = { remoteAddress: step.ip || '192.168.1.50', destroy() { done(null); } };
    const parts = [], res = new Writable({ write(c, e, cb) { parts.push(c); cb(); } }); res.headersSent = false;
    res.writeHead = (code, h) => { res.headersSent = true; res.code = code; res.head = h; };
    res.on('finish', () => { const b = Buffer.concat(parts), isJson = /json/.test(res.head['Content-Type']) && !/manifest/.test(res.head['Content-Type']);
      done({ status: res.code, headers: Object.entries(res.head).map(([k, v]) => [k, String(v)]), ...(isJson ? { json: JSON.parse(b.toString()) } : { sha: sha(b) }) }); });
    C._test.handle(req, res).catch(e => done({ error: String(e) }));
  });
  const tokens = [], ids = {};
  const cookie = n => ({ cookie: 'bs_phone=' + tokens[n] });
  const steps = [];
  const run = async step => { calls = []; const answer = step.set || step.renew || step.forget ? undefined : await ask(step);
    if (step.set) Object.assign(world, step.set); if (step.renew) C.renewPin(); if (step.forget) C.forget(step.forget);
    const c = answer && answer.headers.find(h => h[0] === 'Set-Cookie'), t = c && /^bs_phone=([\w-]{20,})/.exec(c[1]); if (t) tokens.push(t[1]);
    steps.push({ ...step, ...(answer !== undefined ? { answer } : {}), ...(calls.length ? { calls } : {}) }); return answer && answer.json; };
  const post = (u, body, headers) => ({ method: 'POST', url: u, body: typeof body === 'string' ? body : JSON.stringify(body), ...(headers ? { headers } : {}) });

  await run({ url: '/', ip: '8.8.8.8' });
  await run({ url: '/', headers: { host: 'rustyshelf.local:18730' } });
  await run({ url: '/', headers: { host: null } });
  for (const u of ['/', '/app.js', '/app.css', '/icon.svg', '/manifest.json', '/font/lexend-600.woff2', '/nothing', '/font/nope.woff2', '/api/library', '/img/x', '']) await run({ url: u });
  await run({ url: '/?x=1', ip: '::ffff:10.0.0.7', headers: { host: '[fe80::1%eth0]:18730' } });
  await run(post('/', {}));
  await run(post('/api/pair', { pin: '<pin>' }, { origin: 'http://evil.example' }));
  await run(post('/api/pair', { pin: '<pin>' }, { 'content-type': 'text/plain' }));
  await run(post('/api/pair', { pin: '<pin>', name: 'x'.repeat(1100) }));
  await run(post('/api/pair', '{"pin":'));
  await run(post('/api/pair', '[1]'));
  await run(post('/api/pair', { pin: '<notpin>' }));
  await run(post('/api/pair', { pin: ' <pin> ', name: 'Kitchen <b>phone</b>!' }, { origin: 'http://127.0.0.1:18730', 'content-type': 'Application/JSON; charset=utf-8', 'user-agent': 'Mozilla/5.0 (iPhone; CPU iPhone OS like Mac OS X)' }));
  await run(post('/api/pair', { pin: '<pin>' }, { 'user-agent': 'Mozilla/5.0 (Linux; Android 14; Tablet Q Build/AB1.2) Mobile' }));
  await run({ url: '/api/me', headers: { cookie: 'bs_phone=' + 'A'.repeat(43) } });
  await run({ url: '/api/me', headers: { cookie: 'bs_phone=short' } });
  await run({ url: '/api/me', headers: { cookie: 'theme=dark; bs_phone=' + tokens[0] + '; x=y' } });
  await run({ url: '/api/me', headers: cookie(1) });
  await run({ url: '/api/status', headers: cookie(0) });
  const all = await run({ url: '/api/library', headers: cookie(0) });
  for (const s of all.shelves) for (const g of s.games) ids[g.t] = g.id;
  for (const t of ['Zeta Run', 'Omega Quest', 'Sigma', 'Tau Racers', 'Phi']) { await run({ url: '/api/game?id=' + ids[t], headers: cookie(0) }); await run({ url: '/api/ach?id=' + ids[t], headers: cookie(0) }); }
  const zeta = await run({ url: '/api/game?id=' + ids['Zeta Run'] + '&x=1', headers: cookie(0) });
  for (const u of ['/api/game', '/api/game?id=gNope', '/api/ach?id=gNope', '/img/' + zeta.cover, '/img/' + zeta.cover + '?h=200', '/img/' + zeta.cover + '?h=12', '/img/' + zeta.cover + '?h=many', '/img/' + zeta.back, '/img/' + zeta.manual[0] + '?h=1080', '/img/iNope', '/api/nothing']) await run({ url: u, headers: cookie(0) });
  await run({ method: 'PUT', url: '/api/me', headers: cookie(0) });
  await run({ method: 'DELETE', url: '/', headers: cookie(0) });
  for (const [u, b] of [['/api/input', { b: 'a', d: true }], ['/api/input', { b: 'home' }], ['/api/input', { b: 'select', d: 1 }], ['/api/input', {}], ['/api/input', { b: 'lb', d: 'yes' }],
    ['/api/text', { key: 'Enter' }], ['/api/text', { key: 'Tab' }], ['/api/text', { key: 'Escape', text: 'no' }], ['/api/text', { text: 'Zeta\u0007 Run\n' }], ['/api/text', { text: '' }], ['/api/text', {}],
    ['/api/text', { text: 'é✓'.repeat(150) }], ['/api/text', { text: 12 }], ['/api/text', { key: 5, text: 'five' }], ['/api/nothing', {}], ['/api/play', { id: 'gNope' }], ['/api/play', {}], ['/api/play', { id: ids['Zeta Run'] }], ['/api/quit', {}]]) await run(post(u, b, cookie(0)));
  await run(post('/api/input', { b: 'a' }, { ...cookie(0), 'content-type': 'text/plain' }));
  await run(post('/api/input', { b: 'a', pad: 'x'.repeat(5000) }, cookie(0)));
  await run(post('/api/input', { b: 'a' }));
  await run({ set: { playOk: false } });
  await run(post('/api/play', { id: ids['Phi'] }, cookie(1)));
  await run({ set: { playOk: true, running: { id: 'snes:Zeta Run.sfc', title: 'not this', kind: '' } } });
  await run({ url: '/api/status', headers: cookie(0) });
  await run(post('/api/play', { id: ids['Tau Racers'] }, cookie(0)));
  await run(post('/api/play', { id: ids['Tau Racers'], quit: true }, cookie(0)));
  for (const running of [{ id: 'steam-4242', kind: 'steam' }, { id: 'snes:Omega Quest.sfc', title: '', kind: 'rom' }, { id: 'elsewhere', title: 'Rho', kind: 'app' }, { id: '', title: 'Sigma' }]) { await run({ set: { running } }); await run({ url: '/api/status', headers: cookie(1) }); }
  await run(post('/api/quit', {}, cookie(1)));
  await run({ set: { hidden: ['snes:Sigma.sfc', 'app-phi'], glyphs: { style: 'ps', swap: true }, homeMenu: false } });
  await run({ url: '/api/library', headers: cookie(0) });
  await run({ url: '/api/game?id=' + ids['Sigma'], headers: cookie(0) });
  await run({ url: '/api/status', headers: cookie(0) });
  await run({ set: { hidden: [], glyphs: { style: 'weird', swap: 0 } } });
  await run({ url: '/api/library', headers: cookie(1) });
  await run(post('/api/unpair', {}, cookie(1)));
  await run({ url: '/api/me', headers: cookie(1) });
  for (let i = 0; i < 5; i++) await run(post('/api/pair', { pin: '<notpin>' }));
  await run(post('/api/pair', { pin: '<pin>' }));
  await run({ renew: true });
  for (let i = 0; i < 4; i++) await run(post('/api/pair', { pin: i % 2 ? '' : '12ab' }));
  await run(post('/api/pair', { pin: '<pin>', name: 'Third' }));
  await run({ url: '/api/me', headers: cookie(2) });
  await run({ forget: 'all' });
  await run({ url: '/api/me', headers: cookie(0) });
  const seen = [], walk = v => { if (typeof v === 'string') { if (/^[gi][\w-]{22}$/.test(v) && !seen.includes(v)) seen.push(v); } else if (v && typeof v === 'object') Object.values(v).forEach(walk); };
  for (const s of steps) if (s.answer && s.answer.json) walk(s.answer.json);
  let told = JSON.stringify(steps); tokens.forEach((t, i) => { told = told.split(t).join(`<token${i}>`); }); seen.forEach((d, i) => { told = told.split(d).join(`<id${i}>`); });
  out.steps = JSON.parse(told); out.png = b64(PNG);
  const st = C.state(); out.state = { on: st.on, url: st.url, port: st.port, wantPort: st.wantPort, phones: st.phones.length, pin: st.pin.length };
  out.stored = Object.keys(JSON.parse(fs.readFileSync(path.join(tmp, 'companion.json'), 'utf8'))).sort();
  await C.stop(); fs.rmSync(tmp, { recursive: true, force: true });
  return out;
}
// Steam's cached achievements (binary KeyValues files as Steam writes them) and the PC games' storage summary.
function kv(o) { // an object as binary KeyValues: a string is a text, a whole number an int32, { $: [type, bytes] } anything else
  const parts = [];
  for (const [k, v] of Object.entries(o)) {
    const key = Buffer.concat([Buffer.from(k), Buffer.from([0])]);
    if (typeof v === 'string') parts.push(Buffer.from([1]), key, Buffer.from(v), Buffer.from([0]));
    else if (typeof v === 'number') { const n = Buffer.alloc(4); n.writeInt32LE(v | 0); parts.push(Buffer.from([2]), key, n); }
    else if (v && v.$) parts.push(Buffer.from([v.$[0]]), key, Buffer.from(v.$[1]));
    else parts.push(Buffer.from([0]), key, kv(v), Buffer.from([8]));
  }
  return Buffer.concat(parts);
}
async function pcachFixtures() {
  const pcach = require('../main/pcach'), storage = require('../main/storage');
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-pcach-')), stats = path.join(tmp, 'appcache', 'stats'), out = {};
  fs.mkdirSync(stats, { recursive: true }); fs.mkdirSync(path.join(tmp, 'config'));
  const f32 = n => { const b = Buffer.alloc(4); b.writeFloatLE(n); return [...b]; }, u64 = n => { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(n)); return [...b]; }, i64 = n => { const b = Buffer.alloc(8); b.writeBigInt64LE(BigInt(n)); return [...b]; };
  const wide = t => [...Buffer.from(t, 'utf16le'), 0, 0];
  const files = {};
  const put = (name, buf) => { fs.writeFileSync(path.join(stats, name), buf); files[name] = b64(buf); };
  const ach = (name, bit, display) => ({ name, ...(bit == null ? {} : { bit }), display });
  // 4242: two stats, languages, hidden ones, grey icons, a bit named by its key, one past the 32 bits
  put('UserGameStatsSchema_4242.bin', kv({ 4242: { GameName: 'Tau Racers', Version: '3', Stats: {
    10: { type: '4', bits: { 1: ach('ACH_SECOND', 1, { name: { german: 'Zweite', english: 'Second Lap' }, desc: { english: 'Finish a second lap' }, hidden: '0', icon: 'second.jpg', icon_gray: 'second_gray.jpg' }),
      0: ach('ACH_FIRST', 0, { Name: 'First Lap', desc: 'Finish a lap', icon: 'first.jpg', icon_gray: 'first_gray.jpg' }),
      31: ach('ACH_LAST', 31, { name: { french: 'Dernier' }, desc: { french: 'Le dernier' }, hidden: 1, icon: 'last.jpg' }),
      7: ach('ACH_SECRET', 7, { name: { english: 'Secret Road' }, desc: { english: 'Find the hidden road' }, hidden: '1', icon_gray: 'secret_gray.jpg', icon: 'secret.jpg' }) } },
    2: { type: 'INT', default: '0' },
    3: { Type: '4', Bits: { 5: { name: 'ACH_KEYED', display: { name: {}, desc: {} } }, 40: ach('ACH_FAR', 40, { name: { english: 'Far Out' }, desc: '' }), 2: ach('', 2, { desc: { english: 'No name at all' } }) } } } } }));
  put('UserGameStats_1234_4242.bin', kv({ cache: { crc: 99, PendingChanges: 0, 10: { data: (1 << 0) | (1 << 7) | (1 << 31), AchievementTimes: { 0: 943171200, 7: 0, 31: 1700000000 } }, 3: { data: 1 << 5, achievementtimes: { 5: 1262304000 } } } }));
  // 500: everything earned, the root not under its app id; 600: nobody's progress; 700: one other account's; 800: two others'
  put('UserGameStatsSchema_500.bin', kv({ other: { gamename: { english: 'Phi' }, stats: { 1: { bits: { 0: ach('A', 0, { name: 'Alpha', desc: 'One', icon: 'a.jpg' }), 1: ach('B', 1, { name: 'Beta', desc: 'Two', icon: 'b.jpg', hidden: '1' }) } } } } }));
  put('UserGameStats_1234_500.bin', kv({ cache: { 1: { data: 3, AchievementTimes: { 0: 1, 1: 86399 } } } }));
  const one = kv({ 0: { stats: { 1: { bits: { 0: ach('A', 0, { name: 'Alpha', desc: 'One', icon: 'a.jpg', icon_gray: 'a_gray.jpg' }) } } } } });
  for (const id of [600, 700, 800]) put(`UserGameStatsSchema_${id}.bin`, one);
  put('UserGameStats_77_700.bin', kv({ cache: { 1: { data: 1, AchievementTimes: { 0: 1500000000 } } } }));
  put('UserGameStats_77_800.bin', kv({ cache: { 1: { data: 1 } } })); put('UserGameStats_78_800.bin', kv({ cache: { 1: { data: 1 } } }));
  put('UserGameStatsSchema_900.bin', kv({ 900: { gamename: 'Rho', stats: { 1: { type: 'INT' } } } }));
  put('UserGameStatsSchema_901.bin', Buffer.concat([kv({ 901: { stats: {} } }), Buffer.from([9]), Buffer.from('odd\0')]));
  out.files = files;
  out.blobs = [kv({ a: 'text', b: -5, c: { $: [3, f32(1.5)] }, d: { $: [7, u64('18446744073709551615')] }, e: { $: [10, i64(-9)] }, f: { $: [5, wide('wide é')] }, g: { h: { i: 'deep' }, 2: 'two', 1: 'one' }, a2: { $: [4, [1, 0, 0, 0]] }, a3: { $: [6, [2, 0, 0, 0]] } }),
    Buffer.concat([kv({ a: 'kept' }), Buffer.from([11]), kv({ b: 'after the end' })]), Buffer.from([1, 97, 0, 98]), Buffer.from([2, 97, 0, 1, 2]), Buffer.from([12, 97, 0]), Buffer.alloc(0), kv({ a: 'one', A: 'two', a: 'three' })]
    .map(b => { let parsed = null, error = null; try { parsed = pcach.parseBin(b); } catch (e) { error = e instanceof RangeError ? 'range' : e.message; } return { bytes: b64(b), parsed, error }; });
  const users = [
    '"users"\n{\n\t"76561197960266962"\n\t{\n\t\t"AccountName"\t"one"\n\t\t"MostRecent"\t"0"\n\t\t"Timestamp"\t"1700000000"\n\t}\n\t"76561197960267000"\n\t{\n\t\t"mostrecent"\t"1"\n\t\t"Timestamp"\t"5"\n\t}\n}\n',
    '"users"\n{\n\t"76561197960266962"\n\t{\n\t\t"Timestamp"\t"1700000000"\n\t}\n\t"76561197960267000"\n\t{\n\t\t"Timestamp"\t"1800000000"\n\t}\n\t"76561197960267001"\n\t{\n\t\t"Timestamp"\t"soon"\n\t}\n}\n',
    '"users"\n{\n\t"123"\n\t{\n\t\t"MostRecent"\t"1"\n\t}\n}\n', '"users"\n{\n}\n', 'nothing here', null];
  out.accounts = users.map(text => { const f = path.join(tmp, 'config', 'loginusers.vdf'); if (text == null) fs.rmSync(f, { force: true }); else fs.writeFileSync(f, text); return { text, id: pcach.accountId(tmp) }; });
  out.users = users[0]; fs.writeFileSync(path.join(tmp, 'config', 'loginusers.vdf'), users[0]);
  out.progress = ['4242', '500', '600', '700', '800', '900', '901', '1000', '', '42x', ' 4242'].map(id => [id, pcach.steamProgress(tmp, id)]);
  out.noSteam = pcach.steamProgress('', '4242');

  const root = path.join(tmp, 'games'), rel = t => t.split(tmp).join('<dir>');
  for (const d of ['a/Zeta Run', 'a/Omega Quest', 'a/deep/Sigma', 'b/Tau Racers', 'c d/Phi', 'elsewhere/Rho']) fs.mkdirSync(path.join(root, d), { recursive: true });
  fs.writeFileSync(path.join(root, 'a', 'file.txt'), 'x');
  const mounts = ['proc /proc proc rw 0 0', `dev ${root}/a ext4 rw 0 0`, `dev ${root}/a/deep ext4 rw 0 0`, `dev ${root}/b ext4 rw 0 0`, `dev ${root}/c\\040d ext4 rw 0 0`, 'short', `dev ${root}/a/de ext4 rw 0 0`].join('\n');
  const games = [{ id: 'g1', title: 'Zeta Run', source: 'steam', dir: path.join(root, 'a', 'Zeta Run'), bytes: 3000 }, { id: 'g2', title: 'Omega Quest', source: 'epic', dir: path.join(root, 'a', 'Omega Quest') + '/', bytes: 3000 },
    { id: 'g3', title: '', source: 'folder', dir: path.join(root, 'a', 'x', '..', 'deep', 'Sigma'), bytes: 9000 }, { id: 'g4', title: 'Tau Racers', dir: path.join(root, 'b', 'Tau Racers'), bytes: 500 },
    { id: 'g5', title: 'Phi ' + 'x'.repeat(130), source: 'gog', dir: path.join(root, 'c d', 'Phi'), bytes: 0 }, { id: 'g6', title: 'Rho', source: 'hoyoplay', dir: path.join(root, 'elsewhere', 'Rho'), bytes: 7000 },
    { id: 'g7', title: 'Again', source: 'steam', dir: path.join(root, 'a', 'ZETA RUN'), bytes: 1 }, { id: 'g8', title: 'A file', source: 'steam', dir: path.join(root, 'a', 'file.txt'), bytes: 1 },
    { id: 'g9', title: 'Gone', source: 'steam', dir: path.join(root, 'gone'), bytes: 1 }, { id: 'g10', title: 'No folder', source: 'steam', dir: '', bytes: 1 }, { id: 'g11', title: 'omega quest', source: 'epic', dir: path.join(root, 'a'), bytes: 3000 }];
  const room = { [root + '/a']: { free: 1e9, total: 5e9 }, [root + '/b']: { free: 0, total: 1e6 }, '/': { free: 123456789, total: 987654321 } };
  out.storage = { mounts: rel(mounts), games: JSON.parse(rel(JSON.stringify(games))), room: JSON.parse(rel(JSON.stringify(room))),
    summary: JSON.parse(rel(JSON.stringify(await storage.summary(games, { mountsText: mounts, statfs: async r => room[r] || { free: null, total: null } })))) };
  out.mountOf = [['C:\\Games\\Zeta Run', null], ['d:/games/x', null], ['C:\\', null], ['/x/y', 'a /x/y b\nc /x d\ne /x/yz f'], ['/x/yz/q', 'a /x/y b\nc /x d\ne /x/yz/ f'], ['/other', 'a /x b'], ['/x', 'a /x b']].map(([p, m]) => [p, m, storage.mountOf(p, m == null ? '' : m)]);
  fs.rmSync(tmp, { recursive: true, force: true });
  return out;
}
// Controllers: what a pad is and the prompts it gets (the pad parts of renderer/app.js, run on their own) and the
// real controllers among Windows' devices (main/padsense.js).
function padsFixtures() {
  const src = fs.readFileSync(path.join(ROOT, 'renderer', 'app.js'), 'utf8');
  const cut = (from, to) => { const a = src.indexOf(from), b = src.indexOf(to, a + from.length); if (a < 0 || b < 0) throw new Error('renderer/app.js no longer has: ' + from); return src.slice(a, b); };
  const line = from => cut(from, '\n');
  const code = ['let STYLE="xbox";const PADMAP={swap:false},glyphStyle=()=>STYLE,window={};', line('const XB_MAP='), line('const PS_WORD='), line('const NT_MAP='), line('function btnWord(which)'), line('function padWord(k)'),
    cut('const PAD_MODELS=', 'window.BS_padInfo=padInfo;'), 'const padSee={list:[],ver:0};', line('function padSeeReal()'), line('function padStyleOf(id)'), line('function padSeeName(id)'), line('function padName_(id)'),
    'return {XB_MAP,PS_WORD,NT_MAP,btnWord,padWord,padInfo,padSee,padStyleOf,padName_,set:(s,w)=>{STYLE=s;PADMAP.swap=w}};'].join('\n');
  const A = new Function(code)(), padsense = require('../main/padsense'), out = {};
  const ids = ['DualSense Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)', 'Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 09cc)', 'Wireless Controller (Vendor: 054c Product: ffff)',
    'Pro Controller (STANDARD GAMEPAD Vendor: 057e Product: 2009)', 'Joy-Con (L) (Vendor: 057e Product: 2006)', 'Xbox 360 Controller (XInput STANDARD GAMEPAD)', 'xinput', 'Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 0b13)',
    '8BitDo Pro 2 (Vendor: 2dc8 Product: 6003)', '8BitDo Pro Controller for Switch (Vendor: 2dc8 Product: 6003)', 'HORI Pad for Nintendo Switch (Vendor: 0f0d Product: 00c1)', 'Some PS4 pad (Vendor: 0f0d Product: 0001)',
    'Wireless Controller', 'My wireless controller', 'Steam Deck (Vendor: 28de Product: 1205)', 'Steam pad (Vendor: 28DE Product: 9999)', 'Generic USB Joystick (Vendor: 0079 Product: 0006)', 'Xbox pad of some kind', '',
    'Odd (vendor 054C then product 0CE6)', 'XInput pad (Vendor: 1234 Product: 5678)', 'Pokken Tournament pad'];
  const lists = [[], [{ vendor: '054c', product: '0ce6', style: 'ps' }], [{ vendor: '054c', product: '0ce6', style: 'ps' }, { vendor: '054c', product: '09cc', style: 'ps' }],
    [{ vendor: '054c', product: '0ce6', style: 'ps' }, { vendor: '045e', product: '0b13', style: 'xbox' }], [{ vendor: '057e', product: '2009', style: 'nintendo' }], [{ vendor: '057e', product: '2009', style: 'nintendo' }, { vendor: '054c', product: '0ce6', style: 'ps' }],
    [{ vendor: '2dc8', product: '6003', style: 'xbox' }], [{ vendor: '0f0d', product: '0001', style: 'ps' }]];
  out.info = ids.map(id => [id, A.padInfo(id)]);
  out.styles = lists.map(list => { A.padSee.list = list; return { list, pads: ids.map(id => [A.padStyleOf(id), A.padName_(id)]) }; });
  out.ids = ids;
  const KEYS = ['x', 'o', 'sq', 'tri', 'L1', 'R1', 'L2', 'R2', 'L3', 'R3', '☰', '⧉', 'other'], SHAPE = { x: '✕', o: '○', sq: '□', tri: '△' };
  out.words = ['xbox', 'ps', 'nintendo'].flatMap(style => [false, true].map(swap => { A.set(style, swap);
    return { style, swap, caps: KEYS.map(k => [k, style === 'xbox' ? A.XB_MAP[k] || k : style === 'nintendo' ? A.NT_MAP[k] || k : SHAPE[k] || A.PS_WORD[k] || k]), ok: A.btnWord('ok'), back: A.btnWord('back'), pad: ['L1', 'R1', 'L3', 'menu', 'guide', 'other'].map(k => [k, A.padWord(k)]) }; }));
  const devices = [
    ['USB\\VID_054C&PID_0CE6\\5&1&0&1|USB|USB Composite Device', 'HID\\VID_054C&PID_0CE6&MI_03\\7&1|HIDClass|HID-compliant game controller', 'USB\\VID_054C&PID_0E45\\A1|USB|PULSE headset', 'HID\\VID_054C&PID_0E45&MI_03\\8&2|HIDClass|HID-compliant consumer control device'],
    ['USB\\VID_045E&PID_0B12\\3032|XboxComposite|Xbox Gaming Device', 'USB\\VID_045E&PID_07A5\\6&1|USB|Microsoft mouse receiver', 'HID\\VID_045E&PID_0745\\7|Mouse|HID-compliant mouse', 'USB\\VID_045E&PID_02FF\\9|USB|Xbox compatible controller'],
    ['ROOT\\SYSTEM\\0001|System|Nefarius Virtual Gamepad Emulation Bus', 'USB\\VID_045E&PID_028E\\01|XnaComposite|Xbox 360 Controller for Windows', 'BTHENUM\\{00001124-0000-1000-8000-00805f9b34fb}_VID&0002054c_PID&09cc\\8&1|HIDClass|Wireless Controller', 'HID\\{00001124}_VID&0002054C_PID&09CC&Col01\\9|HIDClass|Bluetooth HID gamepad'],
    ['USB\\VID_045E&PID_028E\\01|XnaComposite|Xbox 360 Controller for Windows'],
    ['BTHENUM\\x_VID&0002057e_PID&2009\\1|HIDClass|Pro Controller', 'BTHENUM\\x_VID&0002057e_PID&0306\\2|HIDClass|Nintendo RVL-CNT-01 game controller', 'USB\\VID_057E&PID_0330\\3|HIDClass|Wii remote controller', 'USB\\VID_28DE&PID_11FF\\4|HIDClass|Steam virtual gamepad', 'USB\\VID_28DE&PID_1205\\5|HIDClass|Steam Deck controller', 'USB\\VID_2DC8&PID_6003\\6|HIDClass|8BitDo game controller'],
    ['', 'nothing|at|all', 'USB\\VID_054C|USB|no product']];
  out.devices = devices.map(l => { const text = l.join('\r\n'); return { text, pads: padsense.parse(text) }; });
  out.devices.push({ text: devices[2].join('\n') + '\n', pads: padsense.parse(devices[2].join('\n') + '\n') });
  return out;
}
// Manual spreads cut into pages, and the manual collections on the Internet Archive (a made-up one, answered here).
async function manualcollFixtures() {
  const https = require('https'), { EventEmitter } = require('events'), { PassThrough } = require('stream');
  const M = require('../main/manuals'), C = require('../main/manualcoll'), spreads = require('../main/spreads');
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-coll-')), out = {};
  // (a game's key is its file in lower case)
  const there = v => JSON.parse(JSON.stringify(v).split(JSON.stringify(tmp).slice(1, -1)).join('<tmp>').split(JSON.stringify(tmp.toLowerCase()).slice(1, -1)).join('<tmp>'));
  const page = (w, h) => Buffer.concat([pngHead(w, h), Buffer.alloc(2960)]);
  const tree = dir => { const o = []; const walk = (d, rel) => { for (const e of fs.readdirSync(d, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : 1))) { const r = rel ? rel + '/' + e.name : e.name;
    if (e.isDirectory()) walk(path.join(d, e.name), /^\d{4}-\d\d-\d\dT[\d-]+$/.test(e.name) ? rel + '/<stamp>' : r); else o.push(r); } }; try { walk(dir, ''); } catch {} return o.sort(); };

  // ---- spreads: each manual is its pages' sizes; a cut half says which page it was and which side
  const manuals = {
    'scanned open': { pages: [['01.png', 2000, 1400], ['02.png', 2000, 1400], ['03.jpg', 1000, 1400], ['04.png', 2000, 1400]] },
    'upright': { pages: [['01.png', 1000, 1400], ['02.png', 1000, 1400]] },
    'one poster': { pages: [['01.png', 1000, 1400], ['02.png', 2000, 1400], ['03.png', 1000, 1400], ['04.png', 1000, 1400]] },
    'sideways': { pages: [['01.png', 1400, 1000], ['02.png', 1400, 1000], ['03.png', 1400, 1000]] },
    'two sideways': { pages: [['01.png', 1400, 1000], ['02.png', 1400, 1000]] },
    'screens': { pages: [['01.png', 640, 480], ['02.png', 640, 480]] },
    'small spreads': { pages: [['01.png', 1000, 700], ['02.png', 1000, 700], ['03.png', 500, 700]] },
    'squarish scan': { pages: [['01.png', 1650, 1400], ['02.png', 1650, 1400]], opts: { scan: true } },
    'squarish no scan': { pages: [['01.png', 1650, 1400], ['02.png', 1650, 1400]] },
    'scan discarded': { pages: [['1.png', 2000, 1400], ['2.png', 1000, 1400], ['10.png', 2000, 1400]], opts: { scan: true, discard: true } },
    'too narrow for repair': { pages: [['01.png', 2000, 1400], ['02.png', 2000, 1400]], opts: { minW: 2400 } },
    'wide enough for repair': { pages: [['01.png', 2000, 1400], ['02.png', 2000, 1400]], opts: { minW: 1800 } },
    'too wide': { pages: [['01.png', 2400, 1400], ['02.png', 2400, 1400]] },
    'nothing': { pages: [] },
  };
  out.spreads = [];
  for (const [base, m] of Object.entries(manuals)) {
    const dir = path.join(tmp, 'sp', base); fs.mkdirSync(dir, { recursive: true });
    for (const [n, w, h] of m.pages) fs.writeFileSync(path.join(dir, `${base}-manual-${n}`), Buffer.concat([/\.jpg$/.test(n) ? jpegHead(w, h, 0) : pngHead(w, h), Buffer.from('whole:' + n)]));
    fs.writeFileSync(path.join(dir, `${base}-front.png`), 'not a page'); fs.writeFileSync(path.join(dir, `${base}-manual-x.png`), 'not numbered');
    const crop = async (file, side) => Buffer.from(side + ':' + path.basename(file).split('-manual-')[1]);
    const n = await spreads.splitManual(dir, base, crop, m.opts || {});
    const now = Object.fromEntries(fs.readdirSync(dir).filter(f => f.includes('-manual-') && !f.endsWith('-x.png')).sort().map(f => { const t = fs.readFileSync(path.join(dir, f)).toString('latin1'), k = t.lastIndexOf('whole:'); return [f, k >= 0 ? t.slice(k) : t]; }));
    out.spreads.push({ base, pages: m.pages, opts: m.opts || {}, n, now, kept: tree(path.join(dir, '.manual-originals')), median: spreads.medianWidth(dir, base) });
  }
  out.isSpread = [[1400, 1000], [1250, 1000], [1251, 1000], [1620, 1000], [1619, 1000], [1000, 1400], null].map(z => [z, spreads.isSpread(z)]);

  // ---- names
  out.clean = ['Legend of Omega, The - The Sigma Waker (USA)', 'Zeta Run (Europe) (En,Fr,De) (v1.1)', 'Tau_Racers_(USA)_(Disc 2)', 'Phi Party [4D5307E6] (World)', 'Rho, A (Japan) (4d5307e6)', 'Alpha, An - Beta', 'Plain', 'Omega Quest, The', '(USA)', 'Gamma [b] Force (Disc 12) (Disc 3)', 'A, The, The'].map(t => [t, C.clean(t)]);
  out.keys = [['ProfessorZetaAndTheOmegaFutureusa', 'squash'], ['ZetaRun2Skiesusaenfres', 'squash'], ['Wii Games/Wii_Zeta_Run_Brawl', 'loose'], ['Nintendo 64 Games/Manual_Nintendo64_OmegaKart64_EN', 'loose'], ['DS Games/NTR_NSMB_Single_Sheet', 'loose'], ['GBA Games/AGB_LoZ_Sigma_Cap_EN', 'loose'],
    ['x/DKC2_e-manual_digital', 'loose'], ['Gamecube Games/GCN_Tau_Racers_Double_Dash', 'loose'], ['2020bb', 'set'], ['Sets/MSLUG', 'set'], ['Zeta Run (USA)', ''], ['usa', 'squash'], ['Europeeur', 'squash']].map(([stem, mode]) => [stem, mode, C._entryKeys(stem, mode)]);

  // ---- the Internet Archive, as far as this goes
  const md = id => `https://archive.org/metadata/${id}`, pdf = (name, size = 4e6) => ({ name, size: String(size) }), zip = name => ({ name: name.replace(/\.pdf$/, '_jp2.zip') });
  const both = (name, size) => [pdf(name, size), zip(name)];
  const listing = (col, stem, n) => ({ [`https://archive.org/download/${col}/${stem.split('/').map(encodeURIComponent).join('/')}_jp2.zip/`]: { body: '<html>' + Array.from({ length: n }, (_, i) => { const f = `${path.posix.basename(stem)}_jp2/${path.posix.basename(stem)}_${String(i).padStart(4, '0')}.jp2`;
    return `<a href="//archive.org/download/${col}/${encodeURIComponent(stem + '_jp2.zip')}/${encodeURIComponent(f)}">x</a> <a href="?${encodeURIComponent(f)}&ext=jpg">jpg</a>`; }).reverse().join('\n') + '</html>' } });
  const full = (server, dir, col, stem, i) => `https://${server}/BookReader/BookReaderImages.php?zip=${encodeURIComponent(dir + '/' + stem + '_jp2.zip')}&file=${encodeURIComponent(`${path.posix.basename(stem)}_jp2/${path.posix.basename(stem)}_${String(i).padStart(4, '0')}.jp2`)}&id=${encodeURIComponent(col)}&scale=1&rotate=0`;
  const half = (col, stem, i) => `https://archive.org/download/${col}/${stem.split('/').map(encodeURIComponent).join('/')}_jp2.zip/${encodeURIComponent(`${path.posix.basename(stem)}_jp2/${path.posix.basename(stem)}_${String(i).padStart(4, '0')}.jp2`)}&ext=jpg`;
  const GC = 'NintendoGameCubeManuals', GC2 = 'GCN-GBA-manuals', HI = 'Nintendo-Hi-Res-Manuals-2026', F2 = 'Gamecube/Game Manuals/';
  const site = {
    [md(GC)]: { json: { server: 'ia9.example.org', dir: '/7/items/gc', d2: 'ia8.example.org', files: [...both('Zeta Run (USA).pdf'), ...both('Zeta Run (Japan).pdf'), ...both('Zeta Run 2 (USA).pdf'), ...both('Omega Quest (Europe) (En,Fr,De).pdf'), ...both('Omega Quest (USA) (v1.1).pdf'), ...both('Sigma, The (USA).pdf'),
      pdf('Tau Racers (USA).pdf'), ...both('Rho (USA).pdf'), ...both('Alpha Tale (USA).pdf'), ...both('Blurry Game (USA).pdf'), ...both('Locked Game (USA).pdf'), pdf('Zeta Run (USA)_text.pdf'), { name: 'Zeta Run (USA)_djvu.txt' }, ...both('Legend of Gamma, The - The Long Way (USA) (Disc 2).pdf'), ...both('Legend of Gamma, The - The Long Way (USA).pdf'),
      ...both('Disney Pixar Delta Cars (USA).pdf'), ...both('Epsilon & Friends (USA).pdf')] } },
    [md(GC2)]: { json: { d1: 'ia7.example.org', dir: '/3/items/gcn', files: [...both(F2 + 'GCN_Alpha Tale (USA).pdf'), ...both(F2 + 'Kappa Storm (USA).pdf'), ...both('GBA/Game Manuals/Zeta Run (USA).pdf')] } },
    [md(HI)]: { json: { server: 'ia6.example.org', dir: '/1/items/hi', files: [...both('Gamecube Games/GCN_Lambda_Golf_EN.pdf'), ...both('Gamecube Games/Nintendo_GameCube_Health_and_Safety.pdf'), ...both('Wii Games/Wii_Lambda_Golf.pdf'), ...both('Gamecube Games/DK_Jungle_Mu.pdf')] } },
    ...listing(GC, 'Zeta Run (USA)', 3), ...listing(GC, 'Omega Quest (USA) (v1.1)', 2), ...listing(GC, 'Sigma, The (USA)', 2), ...listing(GC, 'Rho (USA)', 3), ...listing(GC, 'Blurry Game (USA)', 2), ...listing(GC2, F2 + 'GCN_Alpha Tale (USA)', 2), ...listing(HI, 'Gamecube Games/GCN_Lambda_Golf_EN', 2),
    ...Object.fromEntries([0, 1, 2].map(i => [full('ia9.example.org', '/7/items/gc', GC, 'Zeta Run (USA)', i), { png: [1200, 1700] }])),
    [full('ia9.example.org', '/7/items/gc', GC, 'Omega Quest (USA) (v1.1)', 0)]: { status: 500 }, [half(GC, 'Omega Quest (USA) (v1.1)', 0)]: { png: [1200, 1700] }, [full('ia9.example.org', '/7/items/gc', GC, 'Omega Quest (USA) (v1.1)', 1)]: { png: [1210, 1700] },
    ...Object.fromEntries([0, 1].map(i => [full('ia9.example.org', '/7/items/gc', GC, 'Sigma, The (USA)', i), { png: [1200, 1700] }])),
    [full('ia9.example.org', '/7/items/gc', GC, 'Rho (USA)', 0)]: { png: [1200, 1700] }, [full('ia9.example.org', '/7/items/gc', GC, 'Rho (USA)', 2)]: { png: [1200, 1700] },
    ...Object.fromEntries([0, 1].map(i => [full('ia9.example.org', '/7/items/gc', GC, 'Blurry Game (USA)', i), { png: [400, 600] }])),
    ...Object.fromEntries([0, 1].map(i => [full('ia7.example.org', '/3/items/gcn', GC2, F2 + 'GCN_Alpha Tale (USA)', i), { png: [1300, 1800] }])),
    ...Object.fromEntries([0, 1].map(i => [full('ia6.example.org', '/1/items/hi', HI, 'Gamecube Games/GCN_Lambda_Golf_EN', i), { png: [2800, 3600] }])),
    // for "wrong manual": the European Omega Quest, and an upload the search finds for Lambda Golf
    ...listing(GC, 'Omega Quest (Europe) (En,Fr,De)', 2), ...Object.fromEntries([0, 1].map(i => [full('ia9.example.org', '/7/items/gc', GC, 'Omega Quest (Europe) (En,Fr,De)', i), { png: [1100, 1600] }])),
    [`https://archive.org/advancedsearch.php?q=${encodeURIComponent('("Lambda Golf") AND mediatype:texts AND (manual OR instruction OR booklet) AND ("gamecube" OR "game cube" OR "gcn" OR "ngc")')}&fl[]=identifier&fl[]=title&fl[]=language&fl[]=imagecount&fl[]=downloads&rows=12&output=json`]:
      { json: { response: { docs: [{ identifier: 'lambda-upload', title: 'Lambda Golf (GameCube) Manual', language: 'eng', imagecount: 2, downloads: 100 }] } } },
    'https://archive.org/metadata/lambda-upload': { json: { files: [{ name: 'page 2.png', source: 'original', size: 30000 }, { name: 'page 1.png', source: 'original', size: 30000 }] } },
    'https://archive.org/download/lambda-upload/page%201.png': { png: [1500, 2100] }, 'https://archive.org/download/lambda-upload/page%202.png': { png: [1500, 2100] },
  };
  const asked = [], real = https.get;
  https.get = (url, opts, cb) => { asked.push(url); const r = site[url] || { status: 404 }, req = new EventEmitter(); req.setTimeout = () => {}; req.destroy = e => { if (e) req.emit('error', e); };
    setImmediate(() => { const res = new PassThrough(); res.statusCode = r.status || 200; res.headers = {}; cb(res); res.end(r.json ? JSON.stringify(r.json) : r.png ? page(r.png[0], r.png[1]) : r.body || ''); }); return req; };
  try {
    out.site = site;
    const data = path.join(tmp, 'data'); fs.mkdirSync(data);
    C.init({ cacheDir: path.join(data, 'boxart-cache'), memoFile: path.join(data, 'manual-collections.json') });
    const { COLLECTIONS } = C;
    out.collections = Object.fromEntries(Object.entries(COLLECTIONS).map(([k, v]) => [k, v.map(c => ({ id: c.id, folder: c.folder || '', names: c.names || '', digital: !!c.digital }))]));
    const strip = ix => ({ ...ix, at: 0, entries: ix.entries.map(e => Object.fromEntries(Object.entries(e).filter(([k]) => k[0] !== '_'))) });
    out.indexes = []; const ixs = {};
    for (const col of COLLECTIONS.gc) { const ix = await C.index(col); ixs[col.id] = ix; out.indexes.push(strip(ix)); }
    out.cached = fs.readdirSync(path.join(data, 'boxart-cache')).sort();
    const game = (title, more = {}) => ({ title, file: `/roms/${title}.iso`, ...more });
    const games = [game('Zeta Run'), game('Zeta Run', { region: 'Japan' }), game('Zeta Run 2 (USA)'), game('Omega Quest'), game('The Sigma'), game('Sigma, The'), game('Legend of Gamma, The - The Long Way'), game('The Legend of Gamma: The Long Way (USA) (Disc 2)'),
      game('Delta Cars'), game('Disney-Pixar Delta Cars'), game('Epsilon and Friends'), game('Epsilon Friends'), game('Zeta'), game('Run Zeta'), game('Zeta Runs'), game('Nothing Like It'), game('Alpha Tale'), game('Kappa Storm'), game('Lambda Golf'), game('Lambda Golf 2'),
      game('Donkey Kong Jungle Mu'), game('DK: Jungle Mu'), game('Jungle'), game(''), game('Rho', { titleId: 'abcd1234' })];
    out.matches = games.map(g => [g, COLLECTIONS.gc.map(col => C.matches(ixs[col.id].entries, g).map(e => e.name))]);
    const sets = [{ name: 'a/2020bb.pdf', stem: 'a/2020bb', title: '2020bb', tags: [], mode: 'set', set: '2020bb' }, { name: 'mslug.pdf', stem: 'mslug', title: 'mslug', tags: [], mode: 'set', set: 'mslug' }];
    out.sets = { entries: sets, games: [{ title: 'Zeta Ball', file: 'C:\\roms\\2020BB.zip' }, { title: 'mslug', file: '/roms/other.zip' }, { title: 'x', file: '' }].map(g => [g, C.matches(sets, g).map(e => e.name)]) };
    out.pages = there(await C.jp2Pages(COLLECTIONS.gc[1], ixs[GC2], ixs[GC2].entries[0]));

    // ---- a run over a made-up GameCube folder
    const { CONSOLES } = require('../main/systems'), roms = path.join(tmp, 'roms'), covers = path.join(roms, 'covers');
    fs.mkdirSync(covers, { recursive: true });
    const titles = ['Zeta Run (USA)', 'Omega Quest (USA)', 'Sigma, The (USA)', 'Tau Racers (USA)', 'Phi (USA)', 'Rho (USA)', 'Alpha Tale (USA)', 'Blurry Game (USA)', 'Locked Game (USA)', 'Lambda Golf (USA)'];
    for (const t of titles) fs.writeFileSync(path.join(roms, t + '.iso'), 'rom');
    const has = (t, sizes) => sizes.forEach(([w, h], i) => fs.writeFileSync(path.join(covers, `${t}-manual-${String(i + 1).padStart(2, '0')}.jpg`), page(w, h)));
    out.had = { 'Omega Quest (USA)': [[800, 1100], [800, 1100]], 'Sigma, The (USA)': [[2600, 3600], [2600, 3600], [2600, 3600]], 'Blurry Game (USA)': [[900, 1300], [900, 1300]], 'Locked Game (USA)': [[500, 700]] };
    for (const [t, sizes] of Object.entries(out.had)) has(t, sizes);
    const key = t => 'gc|' + path.join(roms, t + '.iso').toLowerCase();
    const rejects = { [key('Alpha Tale (USA)')]: ['coll:' + GC + '/Alpha Tale (USA).pdf'] }, studio = { locks: { [key('Locked Game (USA)')]: true } };
    out.rejects = there(rejects); out.studio = there(studio);
    fs.writeFileSync(path.join(data, 'manual-rejects.json'), JSON.stringify(rejects)); M.setRejectStore(path.join(data, 'manual-rejects.json'));
    fs.writeFileSync(path.join(data, 'manual-studio.json'), JSON.stringify(studio));
    const sys = [{ def: CONSOLES.find(c => c.id === 'gc'), cfg: { romDirs: [roms], enabled: true } }];
    out.games = there(require('../main/scanner').scanSystem(sys[0].def, sys[0].cfg).map(g => ({ title: g.title, file: g.file, region: g.region || '', titleId: g.titleId || null })));
    const before = asked.length, report = [];
    // (a PDF renderer, as both apps have one: the made-up site has no PDF to draw)
    const renderPdf = async () => [];
    out.done = await C.run(sys, { report, renderPdf, locked: (id, f) => /Locked Game/.test(f) });
    const memo = () => there(Object.fromEntries(Object.entries(JSON.parse(fs.readFileSync(path.join(data, 'manual-collections.json'), 'utf8'))).map(([k, v]) => { const { at, altTried, render, ...rest } = v; return [k, rest]; })));
    out.report = report.sort((a, b) => (a.title < b.title ? -1 : 1)); out.memo = memo(); out.tree = tree(covers); out.asked = asked.length - before;
    const again = asked.length; out.doneAgain = await C.run(sys, { report: [], renderPdf, locked: (id, f) => /Locked Game/.test(f) }); out.askedAgain = asked.length - again;
    out.recent = C.recent(30).map(r => { const { at, ...rest } = r; return rest; }).sort((a, b) => (a.title < b.title ? -1 : 1));
    out.forgot = [C.forget(key('Phi (USA)')), C.forget('gc|nothing')]; out.memoAfter = memo();
    // "wrong manual": the copy goes aside and is never brought again; the next best takes its place
    M.setStore(path.join(data, 'manual-auto.json'));
    out.wrong = [];
    for (const t of ['Zeta Run (USA)', 'Alpha Tale (USA)', 'Omega Quest (USA)', 'Lambda Golf (USA)', 'Phi (USA)']) {
      const file = path.join(roms, t + '.iso'), collSrc = C.forget(key(t));
      const r = await M.rejectManual(sys, 'gc', file, { collSrc, tryFirst: (def, sc, g) => C.fetchFor(def, sc, g, {}) });
      out.wrong.push([t, { moved: r.moved, got: r.got, from: r.from || null, pdf: !!r.pdf, offline: !!r.offline }]);
    }
    out.wrongTree = tree(covers); out.wrongMemo = memo();
    out.wrongRejects = there(JSON.parse(fs.readFileSync(path.join(data, 'manual-rejects.json'), 'utf8')));
    out.wrongAuto = there(Object.fromEntries(Object.entries(JSON.parse(fs.readFileSync(path.join(data, 'manual-auto.json'), 'utf8'))).map(([k, v]) => { const { at, ...rest } = v; return [k, rest]; })));
    return out;
  } finally { https.get = real; fs.rmSync(tmp, { recursive: true, force: true }); }
}
// The Art Studio's looks and ranking, and the candidates the sources give for a game's part (a made-up LaunchBox index,
// libretro list and disc id; nothing is asked of the web).
async function artstudioFixtures() {
  const covers = require('../main/covers'), A = require('../main/artstudio'), out = {};
  const styles = [{}, { preset: 'hd' }, { preset: 'region', region: 'eur' }, { preset: 'region', region: 'xx' }, { preset: 'special', sharper: false }, { preset: 'sets', region: 'jpn' }, { preset: 'sets', setsHd: true },
    { preset: 'mix', region: 'eur', parts: { front: 'hd', back: 'region', spine: 'region:jpn', disc: 'nothing' } }, { preset: 'authentic', region: 'jpn', regionFirst: true }, { preset: 'weird' }];
  const cfgs = [{ ui: {} }, { ui: { artRegion: 'eur', artHD: false } }];
  out.styles = [];
  for (const cfg of cfgs) for (const st of styles) { const c = { ...cfg, art: { styles: { gc: st } } }, style = A.styleFor(c, 'gc');
    out.styles.push({ cfg: c, style, other: A.styleFor(c, 'wii'), looks: A.PARTS.map(p => A.lookFor(style, p)), regions: ['authentic', 'hd', 'region:jpn', 'region:xx', 'special'].map(l => ['USA', 'Japan', 'Europe, Australia', ''].map(r => A.regionForLook(style, l, { region: r }))) }); }
  out.gameRegions = ['USA', 'Europe', 'Japan', 'USA, Europe', 'Canada', 'World', 'Germany', 'Australia', 'Korea', '', 'UK', 'Latin America', 'japan, usa'].map(r => [r, A.gameRegion({ region: r })]);
  // candidates as the sources' lists would give them, and a few as the picker has them once downloaded
  const C = (source, more = {}) => ({ source, region: '', edition: 'original', kind: 'scan', ...more });
  const cands = [C('launchbox', { region: 'usa' }), C('launchbox', { region: 'eur', kind: 'clean' }), C('launchbox', { region: 'jpn', fan: true, kind: 'clean' }), C('launchbox', { region: 'world', cut: 'spine' }), C('launchbox', { edition: 'special', region: 'usa' }),
    C('launchbox', { edition: 'reprint', region: 'usa' }), C('launchbox', { edition: 'odd' }), C('libretro', { kind: 'clean', region: 'usa', exact: true }), C('libretro', { kind: 'clean', region: 'eur' }), C('gametdb', { kind: 'hd', region: 'usa' }),
    C('gametdb', { region: 'jpn', cut: 'back' }), C('gametdb', { region: 'eur' }), C('steamgriddb', { rank: 0, w: 600, h: 900 }), C('steamgriddb', { rank: 7, w: 2000, h: 3000 }), C('steamgriddb', { rank: 60 }), C('current', { inUse: true, w: 800, h: 1100 }),
    C('earlier', { w: 3000, h: 2000 }), C('made', { kind: 'clean' }), C('mariowiki'), C('launchbox', { region: 'usa', w: 1500, h: 2100 }), C('launchbox', { region: 'usa', w: 0, h: 500 })];
  out.cands = cands;
  const ctxs = [{ style: A.styleFor({ ui: {} }, 'gc'), game: { region: 'USA' } }, { style: A.styleFor({ ui: {} }, 'gc'), game: { region: 'Japan' } }, { style: A.styleFor({ ui: { artRegion: 'eur', artHD: false } }, 'gc'), game: { region: 'USA' } },
    { style: A.styleFor({ ui: {}, art: { styles: { gc: { preset: 'hd', region: 'jpn', sharper: false } } } }, 'gc'), game: {} }, { style: A.styleFor({ ui: {} }, 'gc'), game: { region: 'Europe' }, sharp: false }, { style: A.styleFor({ ui: { artHD: false } }, 'gc'), game: {}, sharp: true }];
  const LOOKS = ['authentic', 'hd', 'region:usa', 'region:eur', 'region:jpn', 'special', 'region:xx', ''];
  out.ranks = ctxs.map(ctx => ({ ctx, scores: cands.map(c => A.scores(c, ctx)), by: LOOKS.map(l => [l, cands.map(c => A.rankScore(c, l, ctx)), A.rankList(cands, l, ctx).map(c => cands.indexOf(c))]) }));

  const im = (type, region, file) => ({ type, region, file });
  const games = [{ id: '1', name: 'Zeta Run' }, { id: '2', name: 'Omega Quest' }, { id: '3', name: 'Tau Racers' }].map(g => ({ ...g, n: covers.norm(g.name) }));
  const index = { games: { 'Nintendo GameCube': games, 'Nintendo Wii': [games[0]] }, images: {
    1: [im('Box - Front', 'North America', 'Nintendo GameCube/Box - Front/Zeta Run-01.jpg'), im('Box - Front', 'Europe', 'Nintendo GameCube/Box - Front/Zeta Run-02.png'), im('Box - Front', 'Japan', 'Nintendo GameCube/Box - Front/Zeta Run (Japan) é.jpg'),
      im('Box - Front - Reconstructed', 'World', 'a/Zeta Run-r.webp'), im('Fanart - Box - Front', '', 'a/Zeta Run-fan.jpg'), im('Steam Poster', '', 'a/poster.jpg'), im('Box - Front', 'Brazil', 'a/odd.gif'), im('Box - Front', 'North America', ''),
      im('Box - Back', 'United Kingdom', 'a/back-uk.jpg'), im('Box - Back - Reconstructed', '', 'a/back-r.jpg'), im('Fanart - Box - Back', 'Oceania', 'a/back-f.jpg'), im('Box - Spine', 'North America', 'a/spine.jpg'), im('Box - Full', 'North America', 'a/full.png'), im('Box - Full', 'Europe', 'a/full-eu.jpg'),
      im('Disc', 'North America', 'a/disc.png'), im('Cart - Front', 'World', 'a/cart.png'), im('Fanart - Cart - Front', '', 'a/cart-f.png'), im('Fanart - Disc', 'Japan', 'a/disc-f.png'), im('Clear Logo', '', 'a/logo.png'), im('Box - 3D', '', 'a/3d.png')],
    2: [im('Box - Front', 'North America', 'b/omega.jpg'), im('Box - Front', 'North America', 'b/omega.jpg')], 3: [] } };
  covers._setIndex(index); covers.setSpineCutter(async () => null);
  const lr = ['Zeta Run (USA)', 'Zeta Run (Europe) (En,Fr,De)', "Zeta Run (USA) (Player's Choice)", 'Zeta Run (Japan) (Limited Edition)', 'Zeta Run 2 (USA)', 'Omega Quest (USA)', 'Tau Racers Deluxe (World)', 'Zeta Run_ The Long Way (USA)'];
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-studio-')); covers.init(tmp); fs.writeFileSync(path.join(tmp, 'boxart-cache', 'libretro-gc.json'), JSON.stringify({ at: Date.now(), names: lr }));
  out.index = index; out.libretro = lr;
  const asks = [['gc', { title: 'Zeta Run', file: '/roms/Zeta Run (USA).iso' }, 'GZRE01', 'auto'], ['gc', { title: 'Zeta Run', file: '/roms/zeta.iso' }, 'GZRP01', 'eur'], ['gc', { title: 'Zeta Run', file: '/roms/Zeta Run (Japan) (Limited Edition).iso' }, 'GZRJ01', 'jpn'],
    ['gc', { title: 'Omega Quest', file: '/roms/Omega Quest (USA).iso' }, null, 'usa'], ['gc', { title: 'Omega Quest & Zeta Run', file: '/roms/both.iso' }, null, 'auto'], ['gc', { title: 'Zeta Run + Nothing', file: '/roms/x.iso' }, null, 'auto'], ['gc', { title: 'Tau Racers', file: '/roms/Tau Racers Deluxe (World).iso' }, 'GTRX01', 'auto'],
    ['gc', { title: 'Nothing At All', file: '/roms/n.iso' }, null, 'auto'], ['wii', { title: 'Zeta Run', file: '/roms/z.wbfs' }, 'RZRK01', 'auto'], ['wiiu', { title: 'Zeta Run', file: '/roms/z.wua' }, 'AZRD01', 'eur'], ['snes', { title: 'Zeta Run', file: '/roms/z.sfc' }, null, 'auto'], ['gc', { title: 'Zeta Run: The Long Way', file: '/roms/Zeta Run: The Long Way (USA).iso' }, null, 'auto']];
  out.candidates = [];
  for (const [sys, game, discId, region] of asks) out.candidates.push({ sys, game, discId, region, parts: Object.fromEntries(await Promise.all(A.PARTS.map(async p => [p, await covers.candidates(sys, game, p, { discId, region, wiki: false })]))) });
  // ---- a game's list with what's on this computer, and a picture put in use (the Electron app's own cands, use, undo)
  const { CONSOLES } = require('../main/systems'), scanner = require('../main/scanner'), paths = require('../main/paths');
  const data = path.join(tmp, 'data'), roms = path.join(tmp, 'roms'), art = path.join(roms, 'covers'), cand = path.join(data, 'art-cand');
  for (const d of [data, path.join(art, 'replaced'), cand]) fs.mkdirSync(d, { recursive: true });
  const file = path.join(roms, 'Zeta Run (USA).iso'); fs.writeFileSync(file, 'rom');
  const pic = (w, h, n) => Buffer.concat([pngHead(w, h), Buffer.alloc(2000 + n, n)]);
  const sha1 = t => require('crypto').createHash('sha1').update(t).digest('hex');
  const LB = 'https://images.launchbox-app.com/';
  const pictures = { 'covers/Zeta Run (USA).png': pic(600, 840, 1), 'covers/replaced/Zeta Run (USA) (2026-01-05-10-11-12-123).png': pic(500, 700, 2), 'covers/replaced/Zeta Run (USA) (2025-09-30-01-02-03-004).jpg': pic(1000, 1400, 3),
    'covers/replaced/Zeta Run (USA)-spine (2026-02-01-00-00-00-000).jpg': pic(60, 840, 4), 'covers/replaced/Other Game (2026-01-05-10-11-12-123).png': pic(1, 1, 5) };
  const cached = { [LB + 'Nintendo%20GameCube/Box%20-%20Front/Zeta%20Run-01.jpg|']: ['.jpg', pic(1000, 1400, 3)], [LB + 'Nintendo%20GameCube/Box%20-%20Front/Zeta%20Run-02.png|']: ['.png', pic(600, 840, 1)], [LB + 'a/full.png|spine']: ['.jpg', pic(70, 900, 6)],
    'https://thumbnails.libretro.com/Nintendo%20-%20GameCube/Named_Boxarts/Zeta%20Run%20(USA).png|': ['.png', Buffer.from('not a picture at all, but kept all the same'.repeat(60))] };
  for (const [f, b] of Object.entries(pictures)) fs.writeFileSync(path.join(roms, f), b);
  for (const [k, [ext, b]] of Object.entries(cached)) fs.writeFileSync(path.join(cand, sha1(k).slice(0, 24) + ext), b);
  const day = 864e5, missing = { [LB + 'a/Zeta%20Run-fan.jpg']: Date.now() - day, [LB + 'a/poster.jpg']: Date.now() - 8 * day };
  fs.writeFileSync(path.join(data, 'art-missing.json'), JSON.stringify(missing));
  out.local = { pictures: Object.fromEntries(Object.entries(pictures).map(([f, b]) => [f, b64(b)])), cached: Object.fromEntries(Object.entries(cached).map(([k, [ext, b]]) => [k, [ext, b64(b)]])), missing: Object.fromEntries(Object.entries(missing).map(([u, t]) => [u, Math.round((Date.now() - t) / day)])) };
  covers.init(data); fs.writeFileSync(path.join(data, 'boxart-cache', 'libretro-gc.json'), JSON.stringify({ at: Date.now(), names: lr }));
  const cfg = { ui: {}, art: { styles: {}, locks: {} }, systems: {} }, def = CONSOLES.find(c => c.id === 'gc'), sc = { romDirs: [roms], enabled: true };
  A.init({ dataDir: data, log() {}, getCfg: () => cfg, setArt: a => { cfg.art = a; }, consoles: () => [{ def, sc }], gameFile: (id, f) => ({ file: f }), scan: async (d, c) => scanner.scanSystem(d, c), placed() {}, changed() {}, send() {}, fileUrl: f => f, pruneDelay: 1e9, migrateDelay: 1e9 });
  const rel = v => JSON.parse(JSON.stringify(v === undefined ? null : v).split(JSON.stringify(tmp).slice(1, -1)).join('<tmp>').split(JSON.stringify(tmp.toLowerCase()).slice(1, -1)).join('<tmp>'));
  const stampless = f => f.replace(/\((?!20(?:25|26)-0)\d{4}(?:-\d+){6}(-\d+)?\)/g, '(STAMP$1)');
  const holds = () => { const o = {}; const walk = d => { for (const e of fs.readdirSync(d, { withFileTypes: true })) { const f = path.join(d, e.name); if (e.isDirectory()) walk(f); else { const b = fs.readFileSync(f); o[stampless(path.relative(roms, f))] = b.length + ':' + b[b.length - 1]; } } }; walk(art); return Object.fromEntries(Object.entries(o).sort()); };
  const game = () => scanner.scanSystem(def, sc).find(g => g.file === file);
  // (a picture on this computer is keyed by where it is: named here by its file instead)
  const name = (c, list) => { const of = k => { const x = list.find(y => y.key === k); return x && x.file ? 'local:' + stampless(path.relative(roms, x.file)) : k; }; return [of(c.key), c.dup ? of(c.dup) : null]; };
  const view = (c, i, list) => rel({ key: name(c, list)[0], source: c.source, label: c.label || '', w: c.w || null, h: c.h || null, bytes: c.bytes || 0, cached: !!c.cached, inUse: !!c.inUse, dup: name(c, list)[1], guess: !!c.guess, file: c.file || null, shown: !c.dup && !(c.guess && !c.cached && !c.inUse) });
  const raw = async part => { const g = { sysId: 'gc', def, sc, game: game(), dir: paths.artDirFor(sc, file), base: paths.artBase(file), key: A.lockKey('gc', file) }; return (await A._t.listFor(g, part, { fresh: true, lrWait: 200 })).list; };
  const listed = async part => (await raw(part)).map(view);
  out.lists = {}; for (const p of A.PARTS) out.lists[p] = await listed(p);
  out.lockKey = rel(A.lockKey('gc', file));
  const steps = [['use', 'front', 'launchbox', 'Box scan · USA'], ['use', 'front', 'earlier', 'Earlier · 5 Jan 2026'], ['undo', 'front'], ['use', 'front', 'nothing', ''], ['use', 'spine', 'launchbox', 'Cut from full box · USA'], ['undo', 'spine'], ['undo', 'back'], ['undo', 'spine'], ['undo', 'spine']];
  out.steps = [];
  for (const [what, part, source, label] of steps) { let error = null, key = null;
    if (what === 'use') { const list = await raw(part), c = list.find(x => x.source === source && x.label === label); key = c ? name(c, list)[0] : 'zzzz';
      try { await A.use({ systemId: 'gc', file, part, key: c ? c.key : 'zzzz' }); } catch (e) { error = e.message; } }
    else { try { await A.undo({ systemId: 'gc', file, part }); } catch (e) { error = e.message; } }
    const locks = JSON.parse(JSON.stringify(cfg.art.locks || {})); for (const e of Object.values(locks)) for (const l of Object.values(e)) { delete l.at; if (/^[ce]/.test(l.key || '')) l.key = 'local'; }
    // (a picture replaced during this run carries today's date)
    const day = l => (/^Earlier · /.test(l) && !['5 Jan 2026', '30 Sept 2025', '1 Feb 2026'].includes(l.slice(10)) ? 'Earlier · TODAY' : l);
    out.steps.push({ what, part, key, error, holds: holds(), locks: rel(locks), front: (await listed('front')).filter(c => c.shown).map(c => [c.source, day(c.label), c.inUse]) }); }
  const riff = (kind, rest) => Buffer.concat([Buffer.from('RIFF\0\0\0\0WEBP' + kind, 'latin1'), Buffer.from(rest), Buffer.alloc(16)]);
  out.heads = [pngHead(640, 480), pngHead(640, 480).subarray(0, 23), jpegHead(300, 200, 0), Buffer.from([0xff, 0xd8, 0x00, 0x00]), riff('VP8X', [0, 0, 0, 0, 10, 0, 0, 0, 0x1f, 0x03, 0x00, 0x57, 0x02, 0x00]), riff('VP8 ', [0, 0, 0, 0, 0, 0, 0, 0x9d, 0x01, 0x2a, 0x20, 0xc3, 0x58, 0x82]),
    riff('VP8 ', [0, 0, 0, 0, 0, 0, 0, 0x9d, 0x01, 0x2b, 0x20, 0x03, 0x58, 0x02]), riff('VP8L', [0, 0, 0, 0, 0x2f, 0x1f, 0xc3, 0x95, 0x00]), riff('VP8L', [0, 0, 0, 0, 0x2e, 0x1f, 0xc3, 0x95, 0x00]), riff('ALPH', [0]), Buffer.from('GIF89a' + ' '.repeat(40), 'latin1'), Buffer.alloc(0)]
    .map(b => [b64(b), A._t.imageInfo(b)]);
  covers._setIndex(undefined); fs.rmSync(tmp, { recursive: true, force: true });
  return out;
}
async function wikiartFixtures() {
  const W = require('../main/wikiart'), { norm } = require('../main/lbindex'), out = {};
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-wiki-'));
  const G = 'https://mario.wiki.gallery/images/', f = (name, w, h, more = {}) => ({ name, url: G + encodeURIComponent(name.replace(/ /g, '_')), mime: /\.png$/i.test(name) ? 'image/png' : 'image/jpeg', w, h, ...more });
  const zeta = [f('Zeta Run Nintendo Switch 2 Edition NA box art.jpg', 800, 1300), f('Zeta Run Nintendo Switch 2 Edition Box EUR.png', 900, 1450), f('Zeta Run Nintendo Switch 2 Edition JP box art.jpg', 700, 1200),
    f('Zeta Run Nintendo Switch 2 Edition NA back.jpg', 800, 1300), f('Zeta Run Nintendo Switch 2 Edition EUR back.jpg', 800, 1300), f('Zeta Run Nintendo Switch 2 Edition box art prerelease.jpg', 800, 1300),
    f('Zeta Run Nintendo Switch 2 Edition 2 box art.jpg', 800, 1300), f('Zeta Run Nintendo Switch 2 Edition Extra box art.jpg', 800, 1300), f('Zeta Run Nintendo Switch 2 Edition + Omega Park box art.jpg', 820, 1310),
    f('Zeta Run Nintendo Switch 2 Edition wide box art.jpg', 1300, 800), f('Zeta Run Nintendo Switch 2 Edition tiny box.jpg', 100, 200), f('Zeta Run Nintendo Switch 2 Edition logo box.png', 800, 1300),
    f('Zeta Run Nintendo Switch 2 Edition KR box art.webp', 800, 1300, { mime: '' }), f('Zeta Run Nintendo Switch 2 Edition box art.gif', 800, 1300, { mime: 'image/gif' }),
    { name: 'Zeta Run Nintendo Switch 2 Edition AU box art.jpg', url: 'https://elsewhere.example/a.jpg', mime: 'image/jpeg', w: 900, h: 1500 }];
  const tau = [f('Tau Racers Box.jpg', 600, 1000), f('Tau Racers Switch 2 box art.jpg', 600, 1000), f('Tau Racers back.jpg', 600, 1000)];
  const now = Date.now();
  const answers = { [norm('Zeta Run: Nintendo Switch 2 Edition')]: [now - 864e5, zeta], [norm('Tau Racers')]: [now - 2 * 864e5, tau], [norm('Phi Rho')]: [now, []] };
  fs.writeFileSync(path.join(tmp, 'wikiart.json'), JSON.stringify({ v: 2, q: answers }));
  W.init({ dir: tmp, log() {}, norm });
  out.answers = Object.fromEntries(Object.entries(answers).map(([k, [at, l]]) => [k, [Math.round((now - at) / 864e5), l]]));
  out.finds = [];
  const asks = [['sw2', ['Zeta Run: Nintendo Switch 2 Edition', 'Zeta Run'], 'front', 'usa'], ['sw2', ['Zeta Run: Nintendo Switch 2 Edition'], 'front', 'eur'], ['sw2', ['Zeta Run: Nintendo Switch 2 Edition'], 'front', 'jpn'],
    ['sw2', ['Zeta Run: Nintendo Switch 2 Edition'], 'back', 'usa'], ['sw2', ['Zeta Run: Nintendo Switch 2 Edition'], 'back', 'eur'], ['sw2', ['Zeta Run: Nintendo Switch 2 Edition'], 'spine', 'usa'],
    ['sw2', ['Tau Racers'], 'front', 'usa'], ['sw2', ['Tau Racers'], 'back', ''], ['sw2', ['Phi Rho'], 'front', 'usa'], ['sw2', ['Phi'], 'front', 'usa'], ['switch', ['Tau Racers'], 'front', 'usa'], ['sw2', [], 'front', 'usa']];
  for (const [sys, names, part, region] of asks) out.finds.push({ sys, names, part, region, found: await W.find(sys, names, part, region) });
  out.regions = ['Zeta NA box', 'Zeta US box', 'Zeta Box EUR', 'Zeta AUS box', 'Zeta JP box', 'Zeta box', 'Zeta North America box', 'Zeta na eur box'].map(n => [n, ['usa', 'eur', 'jpn', ''].map(r => W._t.regionScore(n, r))]);
  fs.rmSync(tmp, { recursive: true, force: true });
  return out;
}
async function provenanceFixtures() {
  const A = require('../main/artstudio'), covers = require('../main/covers'), out = {};
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-prov-')), data = path.join(tmp, 'data'), art = path.join(tmp, 'roms', 'covers');
  for (const d of [data, art]) fs.mkdirSync(d, { recursive: true });
  const T = 1767225600000; // 1 Jan 2026
  const files = { 'auto.png': [10, 0], 'own.png': [11, 0], 'poster.png': [12, 0], 'legacy.png': [13, 0], 'hand.png': [14, 0], 'touched.png': [15, 0], 'late.png': [16, 1500], 'later.png': [17, 2500], 'grown.png': [18, 0] };
  for (const [f, [n, late]] of Object.entries(files)) { const p = path.join(art, f); fs.writeFileSync(p, Buffer.alloc(100 + n, n)); fs.utimesSync(p, new Date(T), new Date(T + late)); }
  const k = f => path.resolve(path.join(art, f)).toLowerCase(), size = f => 100 + files[f][0];
  const prov = { v: 1, at: T, files: { [k('auto.png')]: ['auto', size('auto.png'), T], [k('own.png')]: ['own', size('own.png'), T], [k('poster.png')]: ['poster', size('poster.png'), T], [k('touched.png')]: ['studio', size('touched.png'), T - 5000],
    [k('late.png')]: ['auto', size('late.png'), T], [k('later.png')]: ['auto', size('later.png'), T], [k('grown.png')]: ['auto', 1, T], [k('gone.png')]: ['auto', 1, T] }, base: { [k('legacy.png')]: [size('legacy.png'), T], [k('hand.png')]: [1, T] } };
  fs.writeFileSync(path.join(data, 'art-provenance.json'), JSON.stringify(prov));
  A.init({ dataDir: data, log() {}, getCfg: () => ({ ui: {}, art: { styles: {}, locks: {} } }), setArt() {}, consoles: () => [], pruneDelay: 1e9, migrateDelay: 1e9 });
  const rel = v => JSON.parse(JSON.stringify(v).split(tmp.toLowerCase()).join('<tmp>'));
  out.prov = rel(prov); out.files = files; out.t = T;
  const names = [...Object.keys(files), 'gone.png'];
  out.origins = names.map(f => { const p = path.join(art, f); return [f, A.originOf(p), A.mayReplace(p, false), A.mayReplace(p, true), A.isPoster(p)]; });
  A.copied(path.join(art, 'auto.png'), path.join(art, 'hand.png')); A.copied(path.join(art, 'own.png'), path.join(art, 'touched.png')); A.copied(path.join(art, 'legacy.png'), path.join(art, 'late.png'));
  out.after = rel(A._t.provGet());
  fs.rmSync(tmp, { recursive: true, force: true });
  return out;
}
// a module's inner function, for a fixture: its file compiled again with that name exported
function inner(rel, names) {
  const Module = require('module'), file = path.join(ROOT, rel);
  const m = new Module(file, module); m.filename = file; m.paths = Module._nodeModulePaths(path.dirname(file));
  m._compile(fs.readFileSync(file, 'utf8') + '\nmodule.exports.__inner = { ' + names.join(', ') + ' };', file);
  return m.exports;
}
function artsetsFixtures() {
  const A = inner('main/artstudio.js', ['buildSets']), out = {};
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-sets-'));
  let cfg = { ui: {}, art: { styles: {}, locks: {} } };
  A.init({ dataDir: tmp, log() {}, getCfg: () => cfg, setArt() {}, consoles: () => [], pruneDelay: 1e9, migrateDelay: 1e9 });
  let n = 0;
  const C = (source, region, more = {}) => ({ key: 'k' + (++n), url: `https://example.invalid/${source}/${n}.jpg`, ext: '.jpg', source, region, edition: 'original', kind: 'scan', label: `${source} ${region}`, ...more });
  const wrapEu = 'https://example.invalid/launchbox/full-eu.jpg', wrapUs = 'https://example.invalid/gametdb/full-us.jpg';
  const L = {
    front: [C('current', 'usa', { file: '/x.png' }), C('launchbox', 'usa', { w: 1000, h: 1400 }), C('launchbox', 'eur'), C('launchbox', 'jpn', { fan: true }), C('gametdb', 'usa', { kind: 'hd' }), C('gametdb', 'eur'), C('libretro', 'usa', { kind: 'clean' }), C('screenscraper', 'jpn'), C('steamgriddb', ''), C('launchbox', 'world'), C('launchbox', 'usa', { dup: 'k2' })],
    back: [C('launchbox', 'usa'), C('launchbox', 'eur', { url: wrapEu, cut: 'back' }), C('gametdb', 'usa', { url: wrapUs, cut: 'back' }), C('gametdb', 'eur', { guess: true }), C('screenscraper', 'jpn'), C('earlier', 'usa', { file: '/y.png' })],
    spine: [C('launchbox', 'usa'), C('launchbox', 'eur'), C('launchbox', 'eur', { url: wrapEu, cut: 'spine' }), C('gametdb', 'usa', { url: wrapUs, cut: 'spine' }), C('gametdb', 'eur', { guess: true, cached: true }), C('screenscraper', 'jpn'), C('launchbox', '')],
    disc: [C('launchbox', 'usa'), C('launchbox', 'world'), C('gametdb', 'usa', { kind: 'clean', guess: true }), C('gametdb', '', {}), C('screenscraper', 'jpn'), C('launchbox', 'eur', { fan: true })],
  };
  out.lists = L;
  out.cases = [];
  for (const [ui, region] of [[{}, 'USA'], [{}, 'Europe'], [{ artRegion: 'jpn' }, 'Japan'], [{ artHD: false }, '']]) {
    cfg = { ui, art: { styles: {}, locks: {} } };
    const sets = A.__inner.buildSets({ sysId: 'gc', game: { region } }, JSON.parse(JSON.stringify(L)));
    out.cases.push({ ui, region, sets: sets.map(s => ({ id: s.id, label: s.label, region: s.region, source: s.source, hd: s.hd, same: s.same, parts: ['front', 'back', 'spine', 'disc'].map(p => s.parts[p].key) })) });
  }
  fs.rmSync(tmp, { recursive: true, force: true });
  return out;
}
function manualstudioFixtures() {
  const S = inner('main/manualstudio.js', ['quality']), T = S._t, out = {};
  out.quality = [[0, 0], [1600, 0], [1400, 0], [1399, 0], [900, 0], [899, 0], [600, 0], [599, 0], [0, 300000], [0, 250000], [0, 249999], [0, 120000], [0, 50000], [0, 512.4]].map(([w, b]) => [w, b, S.__inner.quality(w, b)]);
  const C = (key, more) => ({ key, source: 'archive', label: key, title: key, lang: 'en', region: 'usa', pages: 24, w: 1000, rel: 8, ...more });
  const lists = [
    [C('cur', { source: 'current', src: 'ia:a' }), C('earlier:x', { source: 'earlier', src: 'ia:a' }), C('ia:a'), C('ia:b'), C('ia:c', { pages: 20 }), C('ia:d', { lang: 'fr', pages: 30 }), C('ia:e', { lang: 'fr', pages: 30 }), C('ia:f', { lang: '', pages: 22 })],
    [C('ia:a', { pages: 10 }), C('ia:b', { pages: 12 }), C('gdb:x', { source: 'gamesdatabase', pages: 12, pdfOnly: true })],
    [C('ia:a', { pages: 12 }), C('ia:b', { pages: 10 }), C('ia:c', { pages: 12 }), C('ia:d', { pages: 10 })],
    [],
  ];
  out.expect = lists.map(l => [l, T.expectOf(l)]);
  const cands = [C('ia:a'), C('ia:b', { pages: 20 }), C('ia:c', { incomplete: true }), C('ia:d', { junk: 1 }), C('ia:e', { junk: 3 }), C('ia:f', { pages: null }), C('ia:g', { lang: 'fr', pages: 30 }), C('ia:h', { lang: 'ja', region: 'jpn', pages: 18 }),
    C('ia:i', { lang: 'de', expected: 40, pages: 39 }), C('coll:x', { source: 'collection', rel: 12, w: 1600 }), C('gdb:y', { source: 'gamesdatabase', rel: 9.5, w: 0, bpp: 300000, pdf: '/x.pdf', pages: 90 }), C('gdb:z', { source: 'gamesdatabase', rel: 15, w: 0, bpp: 130000, region: 'eur' }),
    C('ia:j', { w: 500, region: 'eur' }), C('ia:k', { w: 650, region: '' }), C('ia:l', { w: 0, bpp: 50000 }), C('ia:m', { pages: 70 }), C('ia:n', { lang: '', region: 'eur' })];
  const expect = { en: 24, fr: 30 };
  out.cands = cands; out.expectUsed = expect;
  out.ranks = ['best', 'region:eur', 'region:jpn', 'lang:fr', 'lang:de', '', 'weird'].map(pref => [pref, cands.map(c => [T.rank(c, pref, expect), T.completeness(c, expect)])]);
  return out;
}
// the reading corner's listing (main/library.js): each item's place under the reading folder, title, series and type,
// and the ids of a few full paths
function readingFixtures() {
  const paths = require('../main/paths'), library = require('../main/library');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-read-'));
  const files = ['Magazines/Nintendo_Power/Issue_002.pdf', 'Magazines/Nintendo_Power/Issue_010.cbz', 'Magazines/Nintendo_Power/Issue 10 scans/01.jpg', 'Magazines/Nintendo_Power/Issue 10 scans/02.jpg',
    'Magazines/EGM/1993/EGM.v2.pdf', 'Magazines/one-image/cover.png', 'Magazines/notes.txt', 'Magazines/.hidden.pdf', 'Guides/Zelda Guide.cbr', 'Guides/a/b/c/d/Too deep.pdf', 'Guides/a/b/c/Deep enough.pdf'];
  for (const f of files) { fs.mkdirSync(path.dirname(path.join(root, f)), { recursive: true }); fs.writeFileSync(path.join(root, f), 'x'); }
  paths.setReadingLibrary(root);
  const items = library.scan().map(i => ({ rel: path.relative(root, i.path).split(path.sep).join('/'), kind: i.kind, type: i.type, title: i.title, series: i.series, pages: i.pages || null }));
  paths.setReadingLibrary('');
  fs.rmSync(root, { recursive: true, force: true });
  const ids = ['/home/x/RustyShelf/Magazines/Nintendo Power/Issue 001.pdf', '/Mixed/CASE/Guides/Zelda.CBZ'].map(p => {
    let id = null; library.setCacheRoot(os.tmpdir()); try { const st = fs.statSync; fs.statSync = () => ({ mtimeMs: 0 }); id = path.basename(library.coverPath(p)).slice(0, 16); fs.statSync = st; } catch {} return [p, id];
  });
  return { files, items, ids };
}
steamFixtures().then(async st => {
  fs.writeFileSync(path.join(OUT, 'steam.json'), JSON.stringify(st, null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'pc.json'), JSON.stringify(await pcFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'ra.json'), JSON.stringify(await raFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'museumplan.json'), JSON.stringify(museumPlanFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'reading.json'), JSON.stringify(readingFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'i18n.json'), JSON.stringify([...['fr', 'tr'].map(i18nFixtures), i18nPtFixtures()], null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'steamach.json'), JSON.stringify(await steamachFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'raemu.json'), JSON.stringify(await raemuFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'gametdb.json'), JSON.stringify(await gametdbFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'steamgrid.json'), JSON.stringify(await steamgridFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'dataexport.json'), JSON.stringify(await dataexportFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'addgames.json'), JSON.stringify(await addgamesFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'manuals.json'), JSON.stringify(await manualsFixtures()) + '\n');
  fs.writeFileSync(path.join(OUT, 'companion.json'), JSON.stringify(await companionFixtures()) + '\n');
  fs.writeFileSync(path.join(OUT, 'pcach.json'), JSON.stringify(await pcachFixtures()) + '\n');
  fs.writeFileSync(path.join(OUT, 'pads.json'), JSON.stringify(padsFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'manualcoll.json'), JSON.stringify(await manualcollFixtures()) + '\n');
  fs.writeFileSync(path.join(OUT, 'provenance.json'), JSON.stringify(await provenanceFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'artsets.json'), JSON.stringify(artsetsFixtures()) + '\n');
  fs.writeFileSync(path.join(OUT, 'manualstudio.json'), JSON.stringify(manualstudioFixtures()) + '\n');
  fs.writeFileSync(path.join(OUT, 'wikiart.json'), JSON.stringify(await wikiartFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'artstudio.json'), JSON.stringify(await artstudioFixtures()) + '\n');
  fs.writeFileSync(path.join(OUT, 'updates.json'), JSON.stringify(await updatesFixtures()) + '\n');
  fs.writeFileSync(path.join(OUT, 'homekeys.json'), JSON.stringify(homekeysFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'screenscraper.json'), JSON.stringify(await screenscraperFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'place.json'), JSON.stringify(await placeFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'arcade.json'), JSON.stringify(await arcadeFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'emucfg.json'), JSON.stringify(await emucfgFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'n3ds.json'), JSON.stringify(await n3dsFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'detect.json'), JSON.stringify(await detectFixtures(), null, 1) + '\n');
  console.log('wrote', path.relative(ROOT, OUT));
  process.exit(0); // (the folder-size measure it queued would keep the process waiting)
});
