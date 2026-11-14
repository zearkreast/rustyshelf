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
  custom: [{ id: 'c1', title: ' My Tool ', exe: '{root}/custom/tool.exe', args: '-a "b c"', shelf: 'pc' }, { title: 'Gone', exe: '{root}/custom/gone.exe' }, { exe: '{root}/custom/Some-Game_v2.exe', shelf: 'snes' },
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
    shelf: g.shelf || null, native: !!g.native, programs: (g.programs || []).map(p => [p.rel, p.native]) });
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
    pc: { epic: 'no', quiver: false, custom: [{ exe: ' C:\\a.exe ', title: '  A  ', id: '', args: 7, shelf: 'nope', platform: 'snes', src: 'playnite', runner: '' }, { exe: '' }, 'x', null, { exe: '/b', shelf: 'pc', platform: 'gc' }],
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
    API_GetUserCompletionProgress: { Count: 2, Total: 2, Results: [{ GameID: 11, NumAwarded: 20, NumAwardedHardcore: 10, HighestAwardKind: 'beaten-softcore' }, { GameID: 16, NumAwarded: '5', NumAwardedHardcore: 5, HighestAwardKind: 'mastered' }] },
    API_GetUserProfile: { User: 'Made Up', TotalPoints: 1234, TotalSoftcorePoints: 56, UserPic: '/UserPic/MadeUp.png', RichPresenceMsg: '' },
    API_GetUserRecentAchievements: [{ AchievementID: 901, Title: 'First', Description: 'Do the first thing', Points: 5, BadgeName: '0901', GameTitle: 'Zeta Run', HardcoreMode: 1, Date: '2026-01-01 09:00:00' }, { AchievementID: 902, Title: 'Second', Description: '', Points: 10, BadgeName: '0902', GameTitle: 'Zeta Run', HardcoreMode: 0, Date: '2026-01-02 10:00:00' }],
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
    out.asked = asked; return out;
  } finally { https.get = real; fs.rmSync(tmp, { recursive: true, force: true }); }
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
steamFixtures().then(async st => {
  fs.writeFileSync(path.join(OUT, 'steam.json'), JSON.stringify(st, null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'pc.json'), JSON.stringify(await pcFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'ra.json'), JSON.stringify(await raFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'raemu.json'), JSON.stringify(await raemuFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'gametdb.json'), JSON.stringify(await gametdbFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'steamgrid.json'), JSON.stringify(await steamgridFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'place.json'), JSON.stringify(await placeFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'arcade.json'), JSON.stringify(await arcadeFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'emucfg.json'), JSON.stringify(await emucfgFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'n3ds.json'), JSON.stringify(await n3dsFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'detect.json'), JSON.stringify(await detectFixtures(), null, 1) + '\n');
  console.log('wrote', path.relative(ROOT, OUT));
  process.exit(0); // (the folder-size measure it queued would keep the process waiting)
});
