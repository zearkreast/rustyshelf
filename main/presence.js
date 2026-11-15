// RustyShelf's Discord Rich Presence: while a game runs, Discord shows "Playing <title>" with the shelf's name, and
// clears it when the game closes. Talks to Discord's own app over its local socket (no library, nothing sent anywhere
// else). Needs a Discord application id (discord.com/developers → New application): the id is the one the player (or
// the maker) puts in Settings → Display → Discord. Off unless it's switched on and an id is set; if Discord isn't running,
// it says nothing and tries again later.
'use strict';
const net = require('net'), path = require('path');

let clientId = '', on = false, sock = null, ready = false;
let want = null, retryT = null, retries = 0, log = () => {};
const OP = { HANDSHAKE: 0, FRAME: 1 };

function socketPaths() {
  const i = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
  if (process.platform === 'win32') return i.map(n => `\\\\.\\pipe\\discord-ipc-${n}`);
  const dirs = [process.env.XDG_RUNTIME_DIR, process.env.TMPDIR, '/tmp'].filter(Boolean);
  const extra = [];
  for (const d of dirs) extra.push(d); // (Flatpak and Snap put the socket one folder further down)
  const list = [];
  for (const d of new Set(extra)) for (const n of i) { list.push(path.join(d, `discord-ipc-${n}`)); }
  for (const d of new Set(extra)) for (const n of [0]) { list.push(path.join(d, 'app', 'com.discordapp.Discord', `discord-ipc-${n}`)); list.push(path.join(d, 'snap.discord', `discord-ipc-${n}`)); }
  return list;
}
function frame(op, obj) { const body = Buffer.from(JSON.stringify(obj)), head = Buffer.alloc(8); head.writeInt32LE(op, 0); head.writeInt32LE(body.length, 4); return Buffer.concat([head, body]); }

function tryPaths(list, done) {
  let k = 0;
  const next = () => {
    if (k >= list.length) return done(null);
    const p = list[k++], s = net.createConnection(p);
    s.once('connect', () => { s.removeAllListeners('error'); done(s); });
    s.once('error', () => { s.destroy(); next(); });
  };
  next();
}
function connect() {
  if (sock || !on || !clientId) return;
  tryPaths(socketPaths(), s => {
    if (!s) return scheduleRetry();
    sock = s; ready = false; let buf = Buffer.alloc(0);
    s.on('data', d => { buf = Buffer.concat([buf, d]); while (buf.length >= 8) { const len = buf.readInt32LE(4); if (buf.length < 8 + len) break; let msg = {}; try { msg = JSON.parse(buf.slice(8, 8 + len).toString('utf8')); } catch {} buf = buf.slice(8 + len); if (msg.evt === 'READY') { ready = true; retries = 0; flush(); } } });
    const drop = () => { if (sock === s) { sock = null; ready = false; } try { s.destroy(); } catch {} scheduleRetry(); };
    s.on('error', drop); s.on('close', drop);
    s.write(frame(OP.HANDSHAKE, { v: 1, client_id: clientId }));
  });
}
function scheduleRetry() {
  if (!on || retryT) return;
  retries++; const ms = Math.min(60000, 5000 * retries); // (Discord not running: a quiet retry, longer each time)
  retryT = setTimeout(() => { retryT = null; connect(); }, ms); if (retryT.unref) retryT.unref();
}
function send(activity) {
  want = activity; if (!sock || !ready) return connect();
  flush();
}
function flush() {
  if (!sock || !ready) return;
  const args = { pid: process.pid, activity: want || null };
  try { sock.write(frame(OP.FRAME, { cmd: 'SET_ACTIVITY', args, nonce: String(Date.now()) })); } catch { /* the drop handler reconnects */ }
}

// Settings: on / off and the application id. Called on start and whenever the settings change.
function configure({ enabled, appId, logger } = {}) {
  if (logger) log = logger;
  const id = String(appId || '').replace(/\D/g, '').slice(0, 25);
  const nowOn = !!enabled && id.length > 0;
  if (id !== clientId && sock) { try { sock.destroy(); } catch {} sock = null; ready = false; }
  clientId = id; on = nowOn;
  if (!on) { if (retryT) { clearTimeout(retryT); retryT = null; } if (sock) { try { flush(); sock.destroy(); } catch {} sock = null; ready = false; } want = null; }
  else if (want) connect();
}
// RustyShelf's icon, where Discord can fetch it: the picture when there's no cover, and the small one beside a cover
const ICON = 'https://raw.githubusercontent.com/zearkreast/rustyshelf/rustyshelf/renderer/app-icon.png';
// A game started: its title, the shelf (or store) it's from and the time it began; `image` (an https address) is its
// cover. Keep in sync with crates/rustyshelf-core/src/presence.rs activity
function setGame(title, shelf, image) {
  if (!on) return;
  const t = String(title || 'a game').slice(0, 120), img = /^https:\/\/\S+$/.test(String(image || '')) && String(image).length <= 256 ? String(image) : '';
  send({ type: 0, details: t, state: String(shelf || 'RustyShelf').slice(0, 120), timestamps: { start: Math.floor(Date.now() / 1000) },
    assets: img ? { large_image: img, large_text: t, small_image: ICON, small_text: 'RustyShelf' } : { large_image: ICON, large_text: 'RustyShelf' }, instance: false });
}
// The game closed (or the shelf is back): nothing shown
function clear() { if (!on) return; want = null; if (sock && ready) flush(); }

module.exports = { configure, setGame, clear, socketPaths, _frame: frame };
