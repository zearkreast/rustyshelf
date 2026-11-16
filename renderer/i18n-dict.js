// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0). Written by Haseo
// (https://github.com/Haseosama) in their Boxshelf fork for French: the English on screen swapped, as it's drawn, for
// the chosen language's (Settings → Display → Language, localStorage "bs.lang"): French (i18n-fr.js) or Turkish
// (i18n-tr.js).
(function () {
  "use strict";
  let lang = "";
  try { lang = localStorage.getItem("bs.lang") || ""; } catch {}
  const D = { fr: window.BS_I18N_FR, tr: window.BS_I18N_TR }[lang];
  if (!D) return;

  document.documentElement.lang = lang;
  const exact = new Map(Object.entries(D.exact));
  const pats = D.patterns.map(([s, f]) => { try { return [new RegExp(s, "s"), f]; } catch { return null; } }).filter(Boolean);
  const norm = s => s.replace(/\s+/g, " ").trim();
  const cache = new Map();

  const fill = (f, m) => f.replace(/\{(\d+)(?:\?([^}]*))?\}/g, (_, n, opt) => {
    const v = m[+n]; if (v == null) return "";
    if (opt != null) return v ? opt : "";
    return exact.get(norm(v)) || v;
  });

  function tr(text) {
    if (!text || !/[A-Za-z]/.test(text)) return text;
    const lead = text.match(/^\s*/)[0], trail = text.match(/\s*$/)[0];
    const core = norm(text);
    if (!core) return text;
    let out = cache.get(core);
    if (out === undefined) {
      out = exact.get(core) || null;
      if (!out && core.length < 600) {
        for (const [re, f] of pats) { const m = re.exec(core); if (m) { out = fill(f, m); break; } }
      }
      if (cache.size > 6000) cache.clear();
      cache.set(core, out);
    }
    return out ? lead + out + trail : text;
  }
  window.BS_tr = tr;

  const SKIP = new Set(["SCRIPT", "STYLE", "TEXTAREA", "INPUT", "CODE", "PRE", "NOSCRIPT"]);
  const ATTRS = ["title", "placeholder", "aria-label", "alt"];

  function textNode(n) {
    const p = n.parentNode; if (!p || SKIP.has(p.nodeName) || (p.closest && p.closest('[translate="no"]'))) return;
    const v = n.nodeValue, t = tr(v); if (t !== v) n.nodeValue = t;
  }
  function attrs(el) {
    for (const a of ATTRS) { const v = el.getAttribute && el.getAttribute(a); if (v) { const t = tr(v); if (t !== v) el.setAttribute(a, t); } }
  }
  function walk(root) {
    if (!root) return;
    if (root.nodeType === 3) { textNode(root); return; }
    if (root.nodeType !== 1 || SKIP.has(root.nodeName)) return;
    attrs(root);
    const w = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT, null);
    let n;
    while ((n = w.nextNode())) { if (n.nodeType === 3) textNode(n); else if (!SKIP.has(n.nodeName)) attrs(n); }
  }

  let queue = new Set(), scheduled = false;
  function flush() {
    scheduled = false; const q = queue; queue = new Set();
    for (const n of q) if (n.isConnected !== false) walk(n);
  }
  const schedule = n => { queue.add(n); if (!scheduled) { scheduled = true; (window.requestAnimationFrame || setTimeout)(flush); } };

  const mo = new MutationObserver(recs => {
    for (const r of recs) {
      if (r.type === "childList") r.addedNodes.forEach(schedule);
      else if (r.type === "characterData") schedule(r.target);
      else if (r.type === "attributes") schedule(r.target);
    }
  });
  function start() {
    if (document.title) document.title = tr(document.title);
    walk(document.body || document.documentElement);
    mo.observe(document.documentElement, { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ATTRS });
  }
  if (document.body) start(); else document.addEventListener("DOMContentLoaded", start, { once: true });
})();
