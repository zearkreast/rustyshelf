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
    ['restoreBackups', { gc: 'dol/Dolphin.exe', wii: 'dol2/Dolphin.exe', sw: 'eden/eden', sw2: 'apps/citron.AppImage', ps2: 'pcsx2/pcsx2-qt', ps3: 'rpcs3/rpcs3', x360: '' }]],
};
function emucfgFixtures() {
  const emucfg = require('../main/emucfg');
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'rs-fix-emucfg-')), was = { ...process.env }, at = r => (r ? path.join(home, r) : '');
  for (const [rel, c] of EMUCFG.files) { const p = path.join(home, rel); fs.mkdirSync(path.dirname(p), { recursive: true }); fs.writeFileSync(p, typeof c === 'string' ? c : Buffer.from(c.b64, 'base64')); }
  process.env.HOME = home; for (const k of ['XDG_CONFIG_HOME', 'APPDATA', 'USERPROFILE', 'BS_DOCUMENTS', 'BS_LINUX']) delete process.env[k];
  const snap = () => { const o = {}; const walk = d => { for (const e of fs.readdirSync(d, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : 1))) { const p = path.join(d, e.name);
    if (e.isDirectory()) walk(p); else { const b = fs.readFileSync(p), t = b.toString('utf8'); o[path.relative(home, p)] = Buffer.from(t, 'utf8').equals(b) ? t : { b64: b.toString('base64') }; } } }; walk(home); return o; };
  let before = snap(); const steps = [];
  for (const step of EMUCFG.steps) {
    const [what, a, b, c] = step; let result;
    if (what === 'lists') result = { pcsx2: emucfg.pcsx2Inis(at(a), process.env, true), docs: emucfg.docsDirs(process.env), dolphin: emucfg.dolphinUserDirs(at(a), process.env) };
    else if (what === 'quietEden') result = emucfg.quietEden(at(a));
    else if (what === 'afterExit') result = emucfg.afterExit(a, at(b)) || null;
    else if (what === 'setIni') result = emucfg.setIni(at(a), b, c);
    else if (what === 'restoreBackups') result = emucfg.restoreBackups(Object.fromEntries(Object.entries(a).map(([k, v]) => [k, at(v)])));
    else result = emucfg[what](at(a), b);
    const now = snap(), changed = {};
    for (const k of new Set([...Object.keys(before), ...Object.keys(now)])) if (JSON.stringify(before[k]) !== JSON.stringify(now[k])) changed[k] = k in now ? now[k] : null;
    steps.push({ step, result: result === undefined ? null : result, changed }); before = now;
  }
  for (const k of Object.keys(process.env)) if (!(k in was)) delete process.env[k]; Object.assign(process.env, was);
  fs.rmSync(home, { recursive: true, force: true });
  return JSON.parse(JSON.stringify({ files: EMUCFG.files, steps }).split(home).join('{home}'));
}

fs.mkdirSync(OUT, { recursive: true });
fs.writeFileSync(path.join(OUT, 'emunames.json'), JSON.stringify(EMUS.map(p => { const n = require('../shared/emuname'); return { path: p, label: n.label(p), parts: n.parts(p), known: n.knownName(p) }; }), null, 1) + '\n');
fs.writeFileSync(path.join(OUT, 'settings.json'), JSON.stringify(settingsFixtures(), null, 1) + '\n');
fs.writeFileSync(path.join(OUT, 'emucfg.json'), JSON.stringify(emucfgFixtures(), null, 1) + '\n');
fs.writeFileSync(path.join(OUT, 'titles.json'), JSON.stringify(titles, null, 1) + '\n');
fs.writeFileSync(path.join(OUT, 'scan.json'), JSON.stringify(scanFixtures(), null, 1) + '\n');
steamFixtures().then(async st => {
  fs.writeFileSync(path.join(OUT, 'steam.json'), JSON.stringify(st, null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'pc.json'), JSON.stringify(await pcFixtures(), null, 1) + '\n');
  fs.writeFileSync(path.join(OUT, 'detect.json'), JSON.stringify(await detectFixtures(), null, 1) + '\n');
  console.log('wrote', path.relative(ROOT, OUT));
  process.exit(0); // (the folder-size measure it queued would keep the process waiting)
});
