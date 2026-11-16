// The pretend RomM server (fake-romm.js), run for tests/romm.rs: prints its address, then
// answers one JSON line per command read from stdin.
'use strict';
const path = require('path');
const fake = require(path.join(__dirname, 'fake-romm.js'));
const readline = require('readline');

(async () => {
  const f = await fake.start();
  const S = f.state;
  process.stdout.write(JSON.stringify({ url: f.url }) + '\n');
  const rl = readline.createInterface({ input: process.stdin });
  rl.on('line', line => {
    const c = JSON.parse(line);
    let out = true;
    if (c.op === 'set') S[c.key] = c.value;
    else if (c.op === 'corrupt') { if (c.on) S.corrupt.add(c.id); else S.corrupt.delete(c.id); }
    else if (c.op === 'touch') S.touch(c.id, c.fields || {});
    else if (c.op === 'remove') S.remove(c.id);
    else if (c.op === 'add') out = S.add(c.rom).id;
    else if (c.op === 'clear') S.log.length = 0;
    else if (c.op === 'log') out = S.requests(c.prefix || '');
    else if (c.op === 'zip') out = fake.makeZip(c.entries.map(e => ({ name: e.name, data: Buffer.from(e.data) }))).toString('base64');
    process.stdout.write(JSON.stringify(out) + '\n');
  });
  rl.on('close', () => f.stop().then(() => process.exit(0)));
})();
