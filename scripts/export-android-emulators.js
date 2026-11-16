#!/usr/bin/env node
// Writes the Android app's emulator apps and how each is started (android-app/web-extras/android-systems.js) as JSON for
// the Rust app (crates/rustyshelf-core/data/android-emulators.json); a name pattern is kept as its source text.
//   node scripts/export-android-emulators.js           write the file
//   node scripts/export-android-emulators.js --check   exit 1 when it's out of date
'use strict';
const fs = require('fs'), path = require('path'), vm = require('vm');

const ROOT = path.join(__dirname, '..');
const OUT = path.join(ROOT, 'crates', 'rustyshelf-core', 'data', 'android-emulators.json');
const ctx = { window: {} };
vm.runInNewContext(fs.readFileSync(path.join(ROOT, 'android-app', 'web-extras', 'android-systems.js'), 'utf8'), ctx);
const E = ctx.window.BS_ANDROID.EMULATORS;
const out = Object.values(E).map(e => ({
  id: e.id, name: e.name, packages: e.packages || [], match: e.match ? e.match.source : '', matchCase: !!(e.match && !e.match.ignoreCase),
  launch: e.launch, variants: e.variants || {}, stores: e.stores || []
}));
const text = JSON.stringify(out, null, 1) + '\n';
if (process.argv.includes('--check')) {
  const now = fs.existsSync(OUT) ? fs.readFileSync(OUT, 'utf8') : '';
  if (now !== text) { console.error('android-emulators.json is out of date: run node scripts/export-android-emulators.js'); process.exit(1); }
} else fs.writeFileSync(OUT, text);
