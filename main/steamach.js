// Steam achievements for the Trophies screen, beside RetroAchievements. Needs the player's own free Steam Web API
// key (Settings → Accounts & services → Steam achievements, kept encrypted like the other keys). The account is the one
// signed in to Steam on this PC (config\loginusers.vdf, MostRecent), so there is nothing else to type. The key is sent
// only to api.steampowered.com and never logged; error messages never carry the URL.
// What comes back, in the Trophies screen's shapes: every played game that has achievements (the most recently played
// first, at most MAX_GAMES), the last 30 days of unlocks with their icon and how many Steam players have them.
// Cached on disk under data\steam-ach: a game's progress 10 minutes, its icons a week, world percentages a day.
const fs = require('fs');
const path = require('path');
const https = require('https');
const vdf = require('./vdf');

const API = 'https://api.steampowered.com';
const MAX_GAMES = 60, RECENT_DAYS = 30, CONCURRENCY = 4;
let dir = null, log = () => {}, getKey = () => '', getSteamPath = () => '';
const mem = new Map(), inflight = new Map();

function init(o) {
  dir = path.join(o.dataDir, 'steam-ach'); try { fs.mkdirSync(dir, { recursive: true }); } catch {}
  log = o.log || log; getKey = o.key || getKey; getSteamPath = o.steamPath || getSteamPath;
}
const key = () => String(getKey() || '').trim();
function enabled() { return /^[0-9A-Fa-f]{32}$/.test(key()); }

function getJson(url) {
  return new Promise((resolve, reject) => {
    const req = https.get(url, { headers: { 'User-Agent': 'RustyShelf' } }, res => {
      let b = ''; res.setEncoding('utf8'); res.on('data', c => b += c);
      res.on('end', () => {
        let j = null; try { j = JSON.parse(b); } catch {}
        if (res.statusCode === 401 || res.statusCode === 403 && !j) return reject(new Error('Steam didn\'t accept the Web API key. Check it in Settings → Accounts & services.'));
        if (res.statusCode === 429) return reject(new Error('Steam is busy (too many requests). Try again in a minute.'));
        if (j) return resolve({ status: res.statusCode, j }); // 400 / 403 with a body: "no stats", "profile is not public"
        reject(new Error(`Steam didn't answer (HTTP ${res.statusCode}).`)); // never the URL: it carries the key
      });
      res.on('error', reject);
    });
    req.on('error', e => reject(new Error('Steam couldn\'t be reached (' + e.code + ').'))); req.setTimeout(20000, () => req.destroy(Object.assign(new Error('Steam took too long to answer.'), { code: 'timeout' })));
  });
}
const q = o => new URLSearchParams(o).toString();
function cached(name, maxAge, fetch) {
  name = String(name).replace(/[^A-Za-z0-9_-]/g, '_'); const f = path.join(dir, name + '.json');
  const m = mem.get(name); if (m && Date.now() - m.at < maxAge) return Promise.resolve(m.data);
  try { const j = JSON.parse(fs.readFileSync(f, 'utf8')); if (Date.now() - j.at < maxAge) { mem.set(name, j); return Promise.resolve(j.data); } } catch {}
  if (inflight.has(name)) return inflight.get(name);
  const p = fetch().then(data => { const j = { at: Date.now(), data }; mem.set(name, j); try { fs.writeFileSync(f, JSON.stringify(j)); } catch {} return data; })
    .catch(e => { try { const j = JSON.parse(fs.readFileSync(f, 'utf8')); if (j && j.data !== undefined) return j.data; } catch {} throw e; }) // offline: the last copy
    .finally(() => inflight.delete(name));
  inflight.set(name, p); return p;
}

// The account signed in to Steam here: loginusers.vdf's MostRecent user, else the newest userdata folder (its name is
// the 32-bit account id; the 64-bit id is that plus 76561197960265728)
function steamId() {
  const root = getSteamPath(); if (!root) return null;
  try {
    const d = vdf.parse(fs.readFileSync(path.join(root, 'config', 'loginusers.vdf'), 'utf8').replace(/^﻿/, ''));
    const users = vdf.get(d, 'users') || {}; let best = null, bt = -1;
    for (const [id, u] of Object.entries(users)) { if (!/^7656\d{13}$/.test(id)) continue; const t = String(vdf.get(u, 'MostRecent')) === '1' ? Infinity : +(vdf.get(u, 'Timestamp') || 0); if (t > bt) { bt = t; best = id; } }
    if (best) return best;
  } catch {}
  try {
    let best = null, bt = 0;
    for (const n of fs.readdirSync(path.join(root, 'userdata'))) { if (!/^\d+$/.test(n) || n === '0') continue; try { const t = fs.statSync(path.join(root, 'userdata', n, 'config', 'localconfig.vdf')).mtimeMs; if (t > bt) { bt = t; best = n; } } catch {} }
    if (best) return (76561197960265728n + BigInt(best)).toString();
  } catch {}
  return null;
}

async function summary(sid, age) {
  return cached(`player-${sid}`, age, async () => {
    const r = await getJson(`${API}/ISteamUser/GetPlayerSummaries/v2/?${q({ key: key(), steamids: sid })}`);
    const p = r.j && r.j.response && r.j.response.players && r.j.response.players[0]; if (!p) throw new Error('Steam sent no profile for this account.');
    return { name: String(p.personaname || ''), pic: p.avatarfull || p.avatarmedium || null, visible: p.communityvisibilitystate === 3 };
  });
}
async function owned(sid, age) {
  return cached(`owned-${sid}`, age, async () => {
    const r = await getJson(`${API}/IPlayerService/GetOwnedGames/v1/?${q({ key: key(), steamid: sid, include_appinfo: 1, include_played_free_games: 1 })}`);
    const g = r.j && r.j.response && r.j.response.games;
    if (!Array.isArray(g)) throw new Error('Steam sent no game list. In Steam, set Profile → Privacy → Game details to Public, then reload.');
    const flagged = g.some(x => x.has_community_visible_stats); // the stats flag, when Steam sends it, skips games without achievements; without it, every played game is asked
    return g.filter(x => x.playtime_forever > 0 && (!flagged || x.has_community_visible_stats)).map(x => ({ appid: x.appid, name: String(x.name || ''), last: +x.rtime_last_played || 0,
      icon: x.img_icon_url ? `https://media.steampowered.com/steamcommunity/public/images/apps/${x.appid}/${x.img_icon_url}.jpg` : null }));
  });
}
// one game's achievements for the account; null when the game has none (or keeps them hidden)
async function player(sid, appid, age) {
  return cached(`game-${appid}-${sid}`, age, async () => {
    const r = await getJson(`${API}/ISteamUserStats/GetPlayerAchievements/v1/?${q({ key: key(), steamid: sid, appid, l: 'english' })}`);
    const ps = r.j && r.j.playerstats;
    if (!ps || ps.success === false) { const e = String(ps && ps.error || ''); if (/not public/i.test(e)) throw Object.assign(new Error('Your Steam game details are private. In Steam, set Profile → Privacy → Game details to Public, then reload.'), { priv: true }); return null; }
    return (ps.achievements || []).map(a => ({ api: a.apiname, title: String(a.name || a.apiname), desc: String(a.description || ''), earned: a.achieved === 1, at: +a.unlocktime || 0 }));
  });
}
async function icons(appid) {
  return cached(`schema-${appid}`, 7 * 864e5, async () => {
    const r = await getJson(`${API}/ISteamUserStats/GetSchemaForGame/v2/?${q({ key: key(), appid, l: 'english' })}`);
    const list = r.j && r.j.game && r.j.game.availableGameStats && r.j.game.availableGameStats.achievements || [];
    const out = {}; for (const a of list) out[a.name] = a.icon || null; return out;
  }).catch(() => ({}));
}
async function world(appid) {
  return cached(`world-${appid}`, 864e5, async () => {
    const r = await getJson(`${API}/ISteamUserStats/GetGlobalAchievementPercentagesForApp/v2/?${q({ gameid: appid })}`);
    const list = r.j && r.j.achievementpercentages && r.j.achievementpercentages.achievements || [];
    const out = {}; for (const a of list) out[a.name] = +a.percent; return out;
  }).catch(() => ({}));
}
// Steam's own API names media.steampowered.com / steamcdn-a.akamaihd.net, which RustyShelf's page doesn't load from (its
// image list allows *.steamstatic.com); the same files are on cdn.akamai.steamstatic.com
const cdn = u => u ? String(u).replace(/^https?:\/\/[^/]+(\/steamcommunity\/)/, 'https://cdn.akamai.steamstatic.com$1') : null;
const iso = t => t ? new Date(t * 1000).toISOString().replace('T', ' ').slice(0, 19) : null;
async function pool(items, n, fn) { const out = new Array(items.length); let i = 0; await Promise.all(Array.from({ length: Math.min(n, items.length) }, async () => { while (i < items.length) { const k = i++; out[k] = await fn(items[k]); } })); return out; }

// The Trophies screen's Steam half: { user, pic, games, recent } or null without a key
async function trophies(force) {
  if (!enabled()) return null;
  const sid = steamId(); if (!sid) throw new Error('RustyShelf couldn\'t find the account signed in to Steam on this PC.');
  const age = force ? 0 : 10 * 60e3;
  const [me, list] = await Promise.all([summary(sid, force ? 0 : 864e5).catch(() => null), owned(sid, force ? 0 : 60 * 60e3)]);
  const pick = list.slice().sort((a, b) => b.last - a.last).slice(0, MAX_GAMES);
  let priv = null;
  const per = await pool(pick, CONCURRENCY, g => player(sid, g.appid, age).catch(e => { if (e.priv) priv = e; else log('Steam achievements:', g.name, '-', e.message); return null; }));
  if (priv && !per.some(Boolean)) throw priv;
  const games = [], recent = [], since = Date.now() / 1000 - RECENT_DAYS * 86400;
  pick.forEach((g, i) => {
    const a = per[i]; if (!a || !a.length) return;
    const earned = a.filter(x => x.earned), last = earned.reduce((m, x) => Math.max(m, x.at), 0), pct = Math.round(100 * earned.length / a.length);
    games.push({ id: 'steam-' + g.appid, appid: g.appid, title: g.name, console: 'Steam', icon: cdn(g.icon), total: a.length, earned: earned.length, pct, award: pct === 100 ? 'mastered' : null, last: iso(last), src: 'steam' });
    for (const x of earned) if (x.at >= since) recent.push({ id: g.appid + ':' + x.api, api: x.api, appid: g.appid, title: x.title, desc: x.desc, points: null, badge: null, game: g.name, console: 'Steam', hardcore: false, date: iso(x.at), rarity: null, src: 'steam' });
  });
  recent.sort((a, b) => String(b.date).localeCompare(String(a.date)));
  const top = recent.slice(0, 60);
  for (const appid of [...new Set(top.map(r => r.appid))].slice(0, 8)) { // icons and world percentages for the games with recent unlocks only
    const [ic, w] = await Promise.all([icons(appid), world(appid)]);
    for (const r of top) if (r.appid === appid) { r.badge = cdn(ic[r.api]); if (w[r.api] != null) r.rarity = Math.max(0.1, Math.round(w[r.api] * 10) / 10); }
  }
  for (const r of top) { const g = games.find(x => x.appid === r.appid); r.fallback = g && g.icon || null; if (!r.badge) r.badge = r.fallback; } // some newer games' achievement icons are gone from Steam's servers: the game's icon instead
  return { user: me && me.name || '', pic: me && me.pic || null, games, recent: top };
}
// Settings' "Test the key": the profile of the account signed in here
async function test(k) {
  const was = getKey; getKey = () => k;
  try {
    if (!enabled()) return { ok: false, error: 'A Steam Web API key is 32 letters and digits.' };
    const sid = steamId(); if (!sid) return { ok: false, error: 'No Steam account found on this PC (sign in to Steam once).' };
    const r = await getJson(`${API}/ISteamUser/GetPlayerSummaries/v2/?${q({ key: key(), steamids: sid })}`);
    const p = r.j && r.j.response && r.j.response.players && r.j.response.players[0];
    return p ? { ok: true, user: String(p.personaname || '') } : { ok: false, error: 'Steam answered without this account.' };
  } catch (e) { return { ok: false, error: e.message }; } finally { getKey = was; }
}

// RetroAchievements' and Steam's halves on one screen; either alone is enough. errs: why a side asked didn't answer
const trDate = s => s ? String(s).replace('T', ' ').slice(0, 19) : '';
function combine(r, s, errs) {
  if (!r && !s) throw new Error(errs[0] || 'Neither RetroAchievements nor Steam answered.');
  const games = [...(r ? r.games.map(g => ({ ...g, src: 'ra' })) : []), ...(s ? s.games : [])].map(g => ({ ...g, last: trDate(g.last) })).sort((a, b) => b.last.localeCompare(a.last));
  const recent = [...(r ? r.recent.map(a => ({ ...a, src: 'ra' })) : []), ...(s ? s.recent : [])].map(a => ({ ...a, date: trDate(a.date) })).sort((a, b) => b.date.localeCompare(a.date)).slice(0, 80);
  const tot = k => games.filter(g => k.includes(g.award)).length;
  return { user: r && r.user || s && s.user || '', pic: r && r.pic || s && s.pic || null, points: r ? r.points : 0, softcore: r ? r.softcore : 0, games, recent,
    sources: { ra: !!r, steam: !!s, steamUser: s ? s.user : '' }, warn: errs.length ? errs[0] : '',
    totals: { games: games.length, unlocked: games.reduce((n, g) => n + g.earned, 0), mastered: tot(['mastered', 'completed']), beaten: tot(['beaten-hardcore', 'beaten-softcore']),
      avg: games.length ? Math.round(games.reduce((n, g) => n + g.pct, 0) / games.length) : 0 } };
}

module.exports = { init, enabled, trophies, test, steamId, combine };
