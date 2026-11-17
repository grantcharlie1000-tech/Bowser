// Source of the page-side probe served as /__probe.js by live-hidden.mjs. Three parts, concatenated:
//   PRE  - before the engine's hooks are consulted: tracks every attachShadow() root (closed ones too) and exposes a fake
//          `window.__sfw.SFW.hooks.rootOf` so the PROBE from test/e2e/lib.mjs (reused verbatim as BASE) can descend into them.
//   BASE - `PROBE` from test/e2e/lib.mjs: rAF + task-boundary DOM invariant scans of its own document, plus window.__audit().
//   EXT  - everything Bowser-live specific (see below).
// The release bundle exposes no hooks of its own (that is why PRE exists). The engine's one page-visible marker is
// window[Symbol.for("sfw.bowser.guard")] === true, used as the "engine injected into this realm" evidence.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));

// lib.mjs imports playwright (not installed for this suite), so a dynamic import is attempted first and the template
// literal is otherwise cooked out of the file text. Either way the string is the one lib.mjs defines.
async function loadBase() {
  try { return (await import("../e2e/lib.mjs")).PROBE; } catch (e) { /* fall back to the file text */ }
  const src = readFileSync(join(HERE, "..", "e2e", "lib.mjs"), "utf8");
  const a = src.indexOf("export const PROBE = `");
  const b = src.indexOf("`;\n\nexport async function instrumentedContext");
  if (a < 0 || b < 0) throw new Error("PROBE not found in e2e/lib.mjs");
  const raw = src.slice(a + "export const PROBE = `".length, b);
  return new Function("return `" + raw + "`")();
}
export const BASE = await loadBase();

export const PRE = `(() => {
  try {
    const closedMap = new WeakMap();
    window.__liveRoots = new Set();
    window.__liveClosedMap = closedMap;
    const orig = Element.prototype.attachShadow;
    Element.prototype.attachShadow = function (init) {
      const r = orig.call(this, init);
      try { closedMap.set(this, r); window.__liveRoots.add(r); if (window.__liveArmRoot) window.__liveArmRoot(r); } catch (e) { /* ignore */ }
      return r;
    };
    if (!window.__sfw) window.__sfw = { SFW: { hooks: { rootOf: (e) => closedMap.get(e) || e.shadowRoot || null } } };
  } catch (e) { /* ignore */ }
})();`;

// EXT(CFG): CFG = { fx, label, origin, run, role, meta }
function EXT(CFG) {
  "use strict";
  const W = window;
  if (W.__liveProbe) return;
  const GUARD = Symbol.for("sfw.bowser.guard");
  const SEL = '.sentinel,.sbox,img[src*="magenta"],img[src*="gta6-ending"],canvas.sentinel,[data-sentinel]';
  const T0 = performance.now();
  const id = Math.random().toString(36).slice(2, 8) + Date.now().toString(36).slice(-4);
  const closedMap = W.__liveClosedMap || new WeakMap();
  const knownRoots = W.__liveRoots || new Set();
  const rootOf = (e) => { try { return closedMap.get(e) || e.shadowRoot || null; } catch (x) { return null; } };
  const isTop = (() => { try { return W === W.top; } catch (x) { return false; } })();
  const L = (W.__liveProbe = {
    id, cfg: CFG, top: isTop, errors: [],
    mo: { batches: 0, scans: 0, unsafe: 0, leak: 0, exposed: 0, skipped: 0, maxScanMs: 0, fUnsafe: 0, fLeak: 0, fExposed: 0 },
    raf: { scans: 0, unsafe: 0, leak: 0, exposed: 0, fUnsafe: 0, fLeak: 0, fExposed: 0 },
    task: { scans: 0, unsafe: 0, leak: 0, exposed: 0, fUnsafe: 0, fLeak: 0, fExposed: 0 },
    frames: { scanned: 0, blind: {}, maxDepth: 0 },
    popups: [], examples: [], maxSeen: 0, firstSeenAt: null, firstShieldAt: null, restored: null,
  });
  const note = (e) => { if (L.errors.length < 6) L.errors.push(String(e && e.message || e)); };
  const sendLog = [];

  // ---- shield / rendering predicates (same rules as lib.mjs PROBE, generalised to any document) ----
  function shieldedNode(n) {
    for (let x = n, i = 0; x && i < 120; i++) {
      if (x.nodeType === 1) {
        const v = x.getAttribute("data-sfw");
        if (v && v.split(" ").indexOf("shield") >= 0) return true;
        const od = x.ownerDocument;
        if (od && x === od.documentElement && (x.getAttribute("data-sfw-page") || "").indexOf("shield") >= 0) return true;
      }
      const p = x.parentNode; x = p && p.nodeType === 11 ? p.host : p;
    }
    return false;
  }
  const rendered = (e) => {
    const w = (e.ownerDocument && e.ownerDocument.defaultView) || W;
    const cs = w.getComputedStyle(e);
    if (cs.display === "none" || cs.visibility !== "visible" || +cs.opacity === 0) return false;
    const r = e.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  };
  // Is the (already shielded and rendered) element actually covered? topmost element at its centre must not be itself/inside it.
  function exposedThroughShield(e) {
    try {
      const d = e.ownerDocument; const r = e.getBoundingClientRect();
      const x = r.left + r.width / 2, y = r.top + Math.min(r.height / 2, 20);
      const w = d.defaultView; if (x < 0 || y < 0 || x >= w.innerWidth || y >= w.innerHeight) return false;
      const top = d.elementFromPoint(x, y);
      if (!top) return false;
      for (let n = top, i = 0; n && i < 60; i++) { if (n === e) return true; const p = n.parentNode; n = p && p.nodeType === 11 ? p.host : p; }
      return false;
    } catch (x2) { return false; }
  }
  function visit(root, out, full) {
    root.querySelectorAll(SEL).forEach((e) => out.push(e));
    if (full) root.querySelectorAll("*").forEach((e) => { const r = rootOf(e); if (r) visit(r, out, full); });
  }
  // Scan one document (frame host element given for a nested document: a shield on, or hiding, the frame element counts).
  function scanDoc(doc, hostEl, acc) {
    const list = [];
    let full = true;
    try { full = doc.getElementsByTagName("*").length < 20000; } catch (x) { /* keep full */ }
    visit(doc, list, full);
    knownRoots.forEach((r) => { try { if (r.host && r.host.ownerDocument === doc && r.host.isConnected) visit(r, list, false); } catch (x) { /* ignore */ } });
    const seen = new Set(); let unsafe = 0, leak = 0, exposed = 0, total = 0;
    const hostShield = hostEl ? shieldedNode(hostEl) : false;
    const hostRendered = hostEl ? rendered(hostEl) : true;
    for (const e of list) {
      if (seen.has(e)) continue; seen.add(e);
      if (!e.isConnected) continue;
      total++;
      if (!hostRendered || !rendered(e)) continue;
      const sh = hostShield || shieldedNode(e);
      if (sh) { leak++; if (!hostShield && exposedThroughShield(e)) exposed++; } else unsafe++;
      if ((!sh || (!hostShield && exposedThroughShield(e))) && L.examples.length < 6) L.examples.push({ t: Math.round(performance.now() - T0), shielded: sh, cls: String(e.className && e.className.baseVal != null ? e.className.baseVal : e.className).slice(0, 30), tag: e.localName, text: (e.textContent || "").slice(0, 24), doc: String(doc.location && doc.location.href).slice(0, 60) });
    }
    acc.total += total; acc.unsafe += unsafe; acc.leak += leak; acc.exposed += exposed;
  }
  function frameDocs(doc, depth, out) {
    let frames = [];
    try { frames = Array.from(doc.querySelectorAll("iframe,frame,object,embed")); } catch (x) { /* ignore */ }
    knownRoots.forEach((r) => { try { if (r.host && r.host.ownerDocument === doc && r.host.isConnected) frames.push(...r.querySelectorAll("iframe")); } catch (x) { /* ignore */ } });
    for (const f of frames) {
      let cd = null, cw = null;
      try { cw = f.contentWindow; cd = f.contentDocument; } catch (x) { cd = null; }
      if (!cd) {
        const key = (f.localName || "frame") + ":" + (f.hasAttribute("sandbox") ? "sandbox[" + f.getAttribute("sandbox") + "]" : "") + (f.hasAttribute("srcdoc") ? "srcdoc" : (f.getAttribute("src") || "").slice(0, 28));
        L.frames.blind[key] = 1;
        continue;
      }
      out.push({ doc: cd, host: f, depth: depth + 1, win: cw });
      if (depth + 1 > L.frames.maxDepth) L.frames.maxDepth = depth + 1;
      if (depth < 5) frameDocs(cd, depth + 1, out);
    }
  }
  function popupDocs(out) {
    for (const p of L.popups) {
      try {
        if (!p.w || p.w.closed) continue;
        const d = p.w.document; // throws when cross-origin
        if (p.w.__liveProbe && p.w.__liveProbe !== L) continue; // has its own probe
        out.push({ doc: d, host: null, depth: 1, win: p.w, popup: true });
      } catch (x) { /* inaccessible */ }
    }
  }
  const armed = new WeakSet();
  const mo = new MutationObserver(() => { L.mo.batches++; checkpoint("mo"); });
  function arm(node) {
    if (!node || armed.has(node)) return;
    try { mo.observe(node, { subtree: true, childList: true, attributes: true, characterData: true }); armed.add(node); } catch (x) { /* ignore */ }
  }
  W.__liveArmRoot = (r) => arm(r);
  let throttleUntil = 0;
  function scanAll(kind) {
    const acc = { total: 0, unsafe: 0, leak: 0, exposed: 0 };
    const facc = { total: 0, unsafe: 0, leak: 0, exposed: 0 };
    scanDoc(document, null, acc);
    const fr = []; frameDocs(document, 0, fr); popupDocs(fr);
    for (const f of fr) {
      L.frames.scanned++;
      try { if (!armed.has(f.doc)) arm(f.doc); } catch (x) { /* ignore */ }
      try { if (f.win && f.win.__liveProbe && f.win.__liveProbe !== L) continue; scanDoc(f.doc, f.host, facc); } catch (x) { note(x); }
    }
    knownRoots.forEach((r) => { if (!armed.has(r)) arm(r); });
    acc.total += facc.total; acc.fUnsafe = facc.unsafe; acc.fLeak = facc.leak; acc.fExposed = facc.exposed;
    return acc;
  }
  function checkpoint(kind) {
    const bucket = L[kind];
    const t = performance.now();
    if (kind === "mo" && throttleUntil > t) { bucket.skipped++; return; }
    let s;
    try { s = scanAll(kind); } catch (x) { note(x); return; }
    const dt = performance.now() - t;
    if (kind === "mo") { bucket.scans++; if (dt > bucket.maxScanMs) bucket.maxScanMs = dt; if (dt > 25) throttleUntil = t + dt * 3; }
    else bucket.scans++;
    if (s.total > L.maxSeen) L.maxSeen = s.total;
    if (s.total && L.firstSeenAt == null) L.firstSeenAt = Math.round(performance.now() - T0);
    if (s.unsafe) bucket.unsafe++;
    if (s.leak) bucket.leak++;
    if (s.exposed) bucket.exposed++;
    if (s.fUnsafe) bucket.fUnsafe++;
    if (s.fLeak) bucket.fLeak++;
    if (s.fExposed) bucket.fExposed++;
  }
  arm(document);
  function rafLoop() { checkpoint("raf"); requestAnimationFrame(rafLoop); }
  try { requestAnimationFrame(rafLoop); } catch (x) { note(x); }
  (function taskLoop() {
    const mc = new MessageChannel();
    mc.port1.onmessage = () => { if (L.stopTasks) return; checkpoint("task"); setTimeout(() => mc.port2.postMessage(0), 0); };
    mc.port2.postMessage(0);
  })();

  // ---- popups: wrap window.open, inject a probe into same-origin popups (about:blank written by the opener has no server-injected one) ----
  const SRC = W.__liveSrc; // {pre, base, ext} strings, defined by the served script
  function inject(pw, label) {
    try {
      if (!pw || pw.closed || pw.__liveProbe) return;
      void pw.document; // same-origin check
      if (!SRC) return;
      pw.eval(SRC.pre + SRC.base + "(" + SRC.ext + ")(" + JSON.stringify(Object.assign({}, CFG, { label, role: "popup" })) + ")");
    } catch (x) { /* cross-origin or navigating */ }
  }
  try {
    const origOpen = W.open;
    W.open = function () {
      const w = origOpen.apply(this, arguments);
      const rec = { w, args: Array.from(arguments).map(String).slice(0, 2), at: Math.round(performance.now() - T0), created: !!w };
      L.popups.push(rec);
      if (w) {
        inject(w, "popup-inject");
        let n = 0;
        const iv = setInterval(() => { n++; if (!w || w.closed || n > 400) { clearInterval(iv); return; } inject(w, "popup-inject"); }, 25);
      }
      return w;
    };
  } catch (x) { note(x); }

  // ---- reporting ----
  function engineEvidence(w, d) {
    const q = (s) => { try { return d.querySelectorAll(s).length; } catch (x) { return -1; } };
    let guard = false; try { guard = !!w[GUARD]; } catch (x) { /* ignore */ }
    let pageShield = false; try { pageShield = (d.documentElement.getAttribute("data-sfw-page") || "").indexOf("shield") >= 0; } catch (x) { /* ignore */ }
    let inRoots = 0; const list = [];
    try { knownRoots.forEach((r) => { try { if (r.host && r.host.ownerDocument === d) inRoots += r.querySelectorAll('[data-sfw~="shield"]').length; } catch (x2) { /* ignore */ } }); } catch (x) { /* ignore */ }
    try { Array.from(d.querySelectorAll('[data-sfw~="shield"]')).slice(0, 6).forEach((e) => list.push(e.localName + (e.id ? "#" + e.id : "") + "." + String(e.className).slice(0, 20) + "[" + e.getAttribute("data-sfw") + "]")); } catch (x) { /* ignore */ }
    const n = q('[data-sfw~="shield"]') + inRoots + (pageShield ? 1 : 0);
    if (n && L.firstShieldAt == null) L.firstShieldAt = Math.round(performance.now() - T0);
    return { guard, shields: n, shieldList: list, pageShield, htmlAttrs: (() => { try { return d.documentElement.getAttributeNames().filter((a) => a.indexOf("data-sfw") === 0).map((a) => a + "=" + d.documentElement.getAttribute(a)); } catch (x) { return []; } })(), bodyAttrs: (() => { try { return d.body ? d.body.getAttributeNames().filter((a) => a.indexOf("data-sfw") === 0).map((a) => a + "=" + d.body.getAttribute(a)) : []; } catch (x) { return []; } })(), sfwShieldEls: q("sfw-shield"), sfwAttrs: q("[data-sfw]"), fb: q("[data-sfw-fb]"), mc: q("[data-sfw-mc]"), hold: q("[data-sfw-hold]"), defer: q("[data-sfw-defer]") };
  }
  function snapshot(final) {
    const base = W.__probe || {};
    let audit = null; try { audit = W.__audit && W.__audit(); } catch (x) { note(x); }
    let ev = {}; try { ev = engineEvidence(W, document); } catch (x) { note(x); }
    const popups = L.popups.map((p) => { let st = { created: p.created, args: p.args, closed: false, guard: null, shields: null, sentinelsRendered: null, unshielded: null }; try { st.closed = !!p.w.closed; if (!st.closed) { const e = engineEvidence(p.w, p.w.document); st.guard = e.guard; st.shields = e.shields; const list = Array.from(p.w.document.querySelectorAll(SEL)); st.sentinelsRendered = list.filter(rendered).length; st.unshielded = list.filter((e2) => rendered(e2) && !shieldedNode(e2)).length; st.ownProbe = !!(p.w.__liveProbe); } } catch (x) { st.inaccessible = true; } return st; });
    let missInfo = []; try { Array.from(document.querySelectorAll("[data-expect=shield]")).filter((e) => !shieldedNode(e) && !e.querySelector('[data-sfw~="shield"]')).slice(0, 3).forEach((e) => { const cs = getComputedStyle(e); const r = e.getBoundingClientRect(); missInfo.push({ tag: e.localName, attrs: e.getAttributeNames().filter((a) => a.indexOf("data-sfw") === 0).map((a) => a + "=" + e.getAttribute(a)), parentAttrs: e.parentElement ? e.parentElement.getAttributeNames().filter((a) => a.indexOf("data-sfw") === 0) : [], display: cs.display, vis: cs.visibility, op: cs.opacity, cv: cs.contentVisibility, rect: [Math.round(r.top), Math.round(r.height), Math.round(r.width)], rendered: rendered(e) }); }); } catch (x) { /* ignore */ }
    let units = []; try { units = Array.from(document.querySelectorAll("[data-unit]")).map((e) => ({ id: e.getAttribute("data-unit"), shielded: shieldedNode(e) || !!e.querySelector('[data-sfw~="shield"]') })); } catch (x) { /* ignore */ }
    let title = ""; try { title = document.title; } catch (x) { /* ignore */ }
    return {
      id, fx: CFG.fx, label: CFG.label, role: CFG.role || null, run: CFG.run, href: String(location.href).slice(0, 140), top: isTop, hasOpener: !!W.opener,
      t: Math.round(performance.now() - T0), final: !!final, title: String(title).slice(0, 120), vis: document.visibilityState,
      engine: ev, base: { frames: base.frames | 0, unsafe: base.unsafe | 0, leak: base.leak | 0, taskScans: base.taskScans | 0, taskUnsafe: base.taskUnsafe | 0, taskLeak: base.taskLeak | 0, maxSeen: base.maxSeen | 0, examples: (base.examples || []).slice(0, 3) },
      mo: L.mo, raf: L.raf, task: L.task, frames: { scanned: L.frames.scanned, blind: Object.keys(L.frames.blind), maxDepth: L.frames.maxDepth },
      popups, examples: L.examples, maxSeen: L.maxSeen, firstSeenAt: L.firstSeenAt, firstShieldAt: L.firstShieldAt, restored: L.restored,
      audit, missInfo, units, errors: L.errors, ua: isTop ? navigator.userAgent.slice(0, 120) : undefined,
      extra: L.extra || undefined,
    };
  }
  function send(final) {
    let body;
    try { body = JSON.stringify(snapshot(final)); } catch (x) { note(x); return; }
    const url = CFG.origin + "/__r";
    let ok = false;
    try { ok = navigator.sendBeacon(url, body); } catch (x) { /* ignore */ }
    if (!ok) { try { fetch(url, { method: "POST", body, keepalive: true, mode: "no-cors" }).catch(() => {}); } catch (x) { /* ignore */ } }
  }
  L.send = send; L.checkpoint = checkpoint;
  setInterval(() => send(false), 500);
  addEventListener("pagehide", (ev) => { L.lastPersisted = !!(ev && ev.persisted); send(false); });
  addEventListener("pageshow", (ev) => { if (ev.persisted) { L.restored = "bfcache"; checkpoint("raf"); } });
  addEventListener("visibilitychange", () => send(false));
  send(false);

  const meta = CFG.meta || {};
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const nextUrl = () => CFG.origin + "/__next?after=" + encodeURIComponent(CFG.fx) + "&run=" + encodeURIComponent(CFG.run);
  const hop = () => { L.stopTasks = true; location.href = nextUrl(); };

  // ---- popup pages. Bowser opens a script popup as a separate tab (the opener's window.open returns null). The popup tab reports,
  // then either takes over the suite (meta.popupHandoff: the opener tab is left in the background) or closes itself. ----
  if (CFG.role === "popup" || (isTop && W.opener)) {
    const handoff = !!meta.popupHandoff && CFG.label !== "popup-inject";
    setTimeout(() => { send(true); setTimeout(() => { if (handoff) hop(); else { try { W.close(); } catch (x) { /* ignore */ } } }, 400); }, CFG.popupMs || meta.popupMs || 3200);
  }

  // ---- the suite driver (top document of the suite tab only) ----
  const driver = isTop && !W.opener && CFG.role !== "popup" && CFG.role !== "passive" && CFG.driver !== false;
  if (!driver) return;
  const stageKey = "sfwLive:" + CFG.run + ":" + CFG.fx;
  let stage = null; try { stage = sessionStorage.getItem(stageKey); } catch (x) { /* ignore */ }

  async function loaded() {
    if (document.readyState === "complete") return;
    await Promise.race([new Promise((r) => addEventListener("load", r, { once: true })), sleep(20000)]);
  }
  const SCEN = {
    async scroll() { for (let i = 0; i < 10; i++) { window.scrollBy(0, 450); await sleep(120); } window.scrollTo(0, 0); },
    async virtual() { for (let i = 0; i < 14; i++) { const vp = document.getElementById("vp"); if (vp) vp.scrollTop = i * 150; await sleep(100); } },
    async spa() { if (window.go) { window.go("gta 6 ending"); await sleep(300); window.go("gta 6 ending spoiler"); await sleep(300); } },
    async sentinels() { for (const e of Array.from(document.querySelectorAll(".sentinel"))) { try { e.scrollIntoView({ block: "center" }); } catch (x) { /* ignore */ } await sleep(350); } window.scrollTo(0, 0); await sleep(300); },
    async hover() {
      for (const e of Array.from(document.querySelectorAll("a[data-hover],[data-expect=shield]")).slice(0, 6)) {
        for (const t of ["pointerover", "mouseover", "mouseenter", "mousemove", "pointermove"]) { try { e.dispatchEvent(new MouseEvent(t, { bubbles: true, clientX: 20, clientY: 20 })); } catch (x) { /* ignore */ } }
        await sleep(120);
      }
    },
  };
  async function runActions(actions) {
    for (const a of actions || []) {
      try {
        if (a.click) { const el = document.querySelector(a.click); if (el) el.click(); else L.errors.push("no " + a.click); }
        else if (a.wait) await sleep(a.wait);
        else if (a.eval) (0, eval)(a.eval);
      } catch (x) { note("action: " + (x && x.message)); }
    }
  }
  async function settle(extra) {
    await sleep(Math.max(300, (meta.settle || 400)) + (extra || 0));
    await Promise.race([new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))), sleep(600)]);
  }
  function finish() { send(true); if (meta.stay) { setTimeout(() => { try { fetch(nextUrl()); } catch (x) { /* ignore */ } }, 350); return; } setTimeout(hop, 350); }
  if ((meta.passivePaths || []).some((q) => location.pathname.endsWith(q))) return;
  addEventListener("pageshow", (ev) => { if (ev.persisted && meta.stage === "bfcache") { L.extra = { stage: "after-back", persisted: true }; L.restored = "bfcache"; (async () => { await settle(); checkpoint("raf"); clearTimeout(hard); finish(); })(); } });
  const hard = setTimeout(() => { L.errors.push("hard timeout"); finish(); }, (meta.maxMs || 30000));

  (async () => {
    try {
      await loaded();
      const plan = meta.stage || null;
      if (plan === "reload") {
        if (!stage) {
          try { sessionStorage.setItem(stageKey, "reloaded"); } catch (x) { /* ignore */ }
          await settle(); L.extra = { stage: "before-reload" }; send(false); location.reload(); return;
        }
        L.extra = { stage: "after-reload" };
      } else if (plan === "bfcache") {
        if (!stage) {
          try { sessionStorage.setItem(stageKey, "away"); } catch (x) { /* ignore */ }
          await settle(); L.extra = { stage: "before-away" }; send(false);
          location.href = "/bw/bw-away.html?__role=passive&back=1"; return;
        }
        L.extra = { stage: "after-back", navType: (performance.getEntriesByType("navigation")[0] || {}).type };
        if (!L.restored) L.restored = "reloaded";
      } else if (plan === "cache") {
        if (!stage) {
          try { sessionStorage.setItem(stageKey, "second"); } catch (x) { /* ignore */ }
          await settle(); L.extra = { stage: "first-visit", fromCache: false }; send(false);
          location.href = "/bw/bw-away.html?__role=passive&forward=" + encodeURIComponent(location.pathname + location.search); return;
        }
        const nav = performance.getEntriesByType("navigation")[0] || {};
        L.extra = { stage: "second-visit", transferSize: nav.transferSize, encodedBodySize: nav.encodedBodySize, navType: nav.type };
      }
      await runActions(meta.actions);
      if (meta.popupWait) { try { fetch(CFG.origin + "/__popupclick?fx=" + encodeURIComponent(CFG.fx)); } catch (x) { /* ignore */ } }
      if (meta.scenario && SCEN[meta.scenario]) await SCEN[meta.scenario]();
      if (meta.js) { try { await (0, eval)(meta.js); } catch (x) { note(x); } }
      if (meta.sw) { // service worker staging is done by the fixture page itself; just record controller state
        L.extra = Object.assign(L.extra || {}, { swController: !!(navigator.serviceWorker && navigator.serviceWorker.controller) });
      }
      await settle(meta.streamMs || 0);
      // final DOM snapshot of any late frames
      checkpoint("raf");
      clearTimeout(hard);
      if (meta.popupWait) {
        let seen = false;
        for (let i = 0; i < 24 && !seen; i++) { try { seen = (await (await fetch(CFG.origin + "/__popupseen?fx=" + encodeURIComponent(CFG.fx))).json()).seen; } catch (x) { /* ignore */ } if (!seen) await sleep(500); }
        L.extra = Object.assign(L.extra || {}, { popupTabSeen: seen });
        if (seen) { send(true); return; } // the popup tab drives the rest of the suite
      }
      finish();
    } catch (x) { note(x); clearTimeout(hard); finish(); }
  })();
}

export const EXT_SRC = EXT.toString();

// What /__probe.js serves. `W.__liveSrc` lets a running probe inject itself into popups it opened.
export function probeScript(cfg) {
  const pre = PRE, base = BASE;
  const ext = EXT_SRC;
  return `window.__liveSrc = ${JSON.stringify({ pre, base, ext })};\n${pre}\n${base}\n(${ext})(${JSON.stringify(cfg)});\n`;
}
