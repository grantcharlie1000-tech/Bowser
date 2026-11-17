// SFW.shield — overlay creation, reveal / re-blur / release, and per-container state.
//
// Contract (DESIGN §3): apply / reveal / reblur / release / state. Extended with prepare() (batched layout
// reads), repair() (framework removed our overlay), and page-level shield helpers.
//
// What the shield never does: remove, move or resize page content. The container keeps its box; only
// `visibility` (via PAGE_CSS), `inert` on its element children, and one out-of-flow overlay child change.
SFW.shield = (function () {
  const NS_SVG = "http://www.w3.org/2000/svg";
  const states = new WeakMap(); // container -> state
  const shielded = new Set(); // containers currently in "shielded" or "revealed" state
  const hosts = new WeakMap(); // overlay host -> container
  const deps = { rootOf: null, sheet: null, css: null, onReveal: null, onReblur: null, reduced: false };
  // captured at load: a page that later patches Object.keys / Element.prototype.setAttribute cannot disable the shields
  const OKEYS = Object.keys;
  const CALL = Function.prototype.call;
  const ElP = typeof Element === "function" ? Element.prototype : null;
  const un = (proto, name) => { try { const f = proto[name]; if (typeof f === "function") return CALL.bind(f); } catch (e) { /* fall through */ } return (o, a, b) => o[name](a, b); };
  const nSet = un(ElP, "setAttribute");
  const nGet = un(ElP, "getAttribute");
  const nHas = un(ElP, "hasAttribute");
  const nDel = un(ElP, "removeAttribute");
  const SR_SHEETS = (function () { try { return Object.getOwnPropertyDescriptor(globalThis.ShadowRoot.prototype, "adoptedStyleSheets") || null; } catch (e) { return null; } })();
  const MAX_REMOVALS = 3;
  const MAX_INERT = 400;

  const TEXT = {
    stamp: "Spoiler",
    sub: "GTA VI content detected · Click to reveal",
    media: "GTA VI media hidden · Click to reveal",
    pill: " · reveal",
    pending: "Checking for GTA VI spoilers…",
    pendingPill: "Checking…",
    error: "Couldn’t verify this content",
    errorSub: "Click to reveal",
    mediaErr: "Couldn’t check image",
    labelMedia: "Reveal image that could not be checked",
    frameErr: "Embedded content couldn’t be checked",
    labelFrame: "Reveal embedded content that could not be checked",
    page: "This page may contain GTA VI spoilers · Click to view page",
    pageBtn: "View page",
    label: "Reveal GTA VI spoiler",
    labelPage: "This page may contain GTA VI spoilers. Activate to view the page",
    hide: "Hide",
    hideLabel: "Hide GTA VI spoiler again",
  };

  const VOID = new Set(["IMG", "VIDEO", "AUDIO", "CANVAS", "INPUT", "TEXTAREA", "SELECT", "IFRAME", "OBJECT", "EMBED", "SVG", "BR", "HR", "PICTURE", "SOURCE", "TRACK", "METER", "PROGRESS", "BUTTON"]);

  // own keys only: a polluted Object.prototype (Prototype.js era pages) must not leak entries into the dependency table
  function configure(opts) {
    const keys = OKEYS(opts);
    for (let i = 0; i < keys.length; i++) deps[keys[i]] = opts[keys[i]];
  }

  function fbOf(variant) {
    return variant === "pending" ? "pending" : variant === "error" ? "error" : variant === "mediaerr" ? "mediaerr" : variant === "frameerr" ? "frameerr" : "";
  }

  function el(doc, tag, cls, text) {
    const e = doc.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }

  function eyeOffIcon(doc) {
    const svg = doc.createElementNS(NS_SVG, "svg");
    svg.setAttribute("viewBox", "0 0 24 24");
    svg.setAttribute("aria-hidden", "true");
    const p = doc.createElementNS(NS_SVG, "path");
    p.setAttribute("d", "M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24");
    const l = doc.createElementNS(NS_SVG, "path");
    l.setAttribute("d", "M1 1l22 22");
    svg.appendChild(p);
    svg.appendChild(l);
    return svg;
  }

  // ---- layout reads (batched by the caller) ------------------------------------------------------------------
  // Decide which element(s) actually carry the shield for `unit`, and whether they need a containing block.
  function prepare(unit) {
    const win = unit.ownerDocument.defaultView;
    const out = [];
    const seen = new Set();
    const add = (e, pos) => { if (!seen.has(e)) { seen.add(e); out.push({ el: e, needPos: pos }); } };
    const resolve = (e, depth) => {
      let cs;
      try { cs = win.getComputedStyle(e); } catch (x) { add(e, true); return; }
      const display = cs.display;
      if ((VOID.has(e.tagName) || e.tagName === "svg") && e.parentElement && depth < 4) {
        // replaced / void elements cannot host an overlay: use the parent box
        resolve(e.parentElement, depth + 1);
        return;
      }
      if (display === "contents" && depth < 3) {
        const kids = e.children;
        if (kids.length) {
          for (let i = 0; i < kids.length; i++) if (kids[i].localName !== "sfw-shield") resolve(kids[i], depth + 1);
          return;
        }
        if (e.parentElement) { resolve(e.parentElement, depth + 1); return; }
      }
      if ((display === "table-row" || e.tagName === "TR") && e.children.length && depth < 3) {
        for (let i = 0; i < e.children.length; i++) resolve(e.children[i], depth + 1);
        return;
      }
      add(e, cs.position === "static");
    };
    resolve(unit, 0);
    return out;
  }

  // ---- overlay ----------------------------------------------------------------------------------------------
  function buildCover(doc, variant) {
    const cover = el(doc, "div", "cover");
    cover.setAttribute("data-v", variant);
    cover.setAttribute("part", "cover");
    if (variant === "notice") {
      cover.textContent = "Spoiler protection error on this page";
      return cover;
    }
    const card = el(doc, "div", "card");
    const stamp = el(doc, "div", "stamp");
    const sub = el(doc, "div", "sub");
    if (variant === "pending") {
      stamp.appendChild(el(doc, "span", "spin"));
      stamp.appendChild(doc.createTextNode(TEXT.pending));
    } else if (variant === "error") {
      stamp.textContent = TEXT.error;
      sub.textContent = TEXT.errorSub;
    } else if (variant === "mediaerr") {
      stamp.textContent = TEXT.mediaErr;
      sub.textContent = TEXT.errorSub;
    } else if (variant === "frameerr") {
      stamp.textContent = TEXT.frameErr;
      sub.textContent = TEXT.errorSub;
    } else if (variant === "page") {
      stamp.textContent = TEXT.stamp;
      sub.textContent = TEXT.page;
      card.appendChild(stamp);
      card.appendChild(sub);
      card.appendChild(el(doc, "button", "btn", TEXT.pageBtn));
      cover.appendChild(card);
      return cover;
    } else {
      stamp.textContent = TEXT.stamp;
      sub.textContent = variant === "media" ? TEXT.media : TEXT.sub;
    }
    card.appendChild(stamp);
    if (sub.textContent) card.appendChild(sub);
    cover.appendChild(card);
    const pill = el(doc, "div", "pill");
    pill.appendChild(el(doc, "i"));
    pill.appendChild(el(doc, "b", null, variant === "pending" ? TEXT.pendingPill : TEXT.stamp));
    if (variant !== "pending") pill.appendChild(el(doc, "span", null, TEXT.pill));
    cover.appendChild(pill);
    return cover;
  }

  function createOverlay(doc, variant, kind) {
    const host = doc.createElement("sfw-shield");
    host.setAttribute("data-s", "shielded");
    if (kind) host.setAttribute("data-k", kind);
    const root = SFW.hooks.attachClosed(host);
    // the setter captured at load: a page that deleted ShadowRoot.prototype.adoptedStyleSheets afterwards cannot unstyle overlays
    if (deps.sheet) {
      if (SR_SHEETS && SR_SHEETS.set) SR_SHEETS.set.call(root, [deps.sheet]);
      else if ("adoptedStyleSheets" in root) root.adoptedStyleSheets = [deps.sheet];
    }
    const cover = buildCover(doc, variant);
    root.appendChild(cover);
    const hide = el(doc, "button", "hide");
    hide.setAttribute("type", "button");
    hide.setAttribute("aria-label", TEXT.hideLabel);
    hide.hidden = true;
    hide.appendChild(eyeOffIcon(doc));
    hide.appendChild(el(doc, "span", null, TEXT.hide));
    root.appendChild(hide);
    return { host, root, cover, hide };
  }

  function armHost(st) {
    const h = st.host;
    h.setAttribute("role", "button");
    h.setAttribute("tabindex", "0");
    h.setAttribute("aria-label", st.page ? TEXT.labelPage : st.variant === "mediaerr" ? TEXT.labelMedia : st.variant === "frameerr" ? TEXT.labelFrame : TEXT.label);
    h.setAttribute("data-s", "shielded");
  }

  function disarmHost(st) {
    const h = st.host;
    h.removeAttribute("role");
    h.removeAttribute("tabindex");
    h.removeAttribute("aria-label");
    h.setAttribute("data-s", "revealed");
  }

  function wireHost(st) {
    const h = st.host;
    const stop = (e) => { e.stopPropagation(); };
    for (const t of ["pointerdown", "pointerup", "mousedown", "mouseup", "touchstart", "touchend", "mouseover", "pointerover", "mouseenter", "pointerenter", "dblclick", "contextmenu"]) {
      h.addEventListener(t, (e) => { if (st.mode === "shielded") stop(e); });
    }
    h.addEventListener("click", (e) => {
      if (st.mode !== "shielded") return;
      e.preventDefault();
      e.stopPropagation();
      if (e.isTrusted !== true) return;
      activate(st);
    });
    h.addEventListener("keydown", (e) => {
      if (st.mode !== "shielded") return;
      if (e.key === "Enter" || e.key === " " || e.key === "Spacebar") {
        e.preventDefault();
        e.stopPropagation();
        if (e.isTrusted !== true) return;
        activate(st, true);
      }
    });
    st.hide.addEventListener("click", (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.isTrusted !== true) return;
      if (st.container) reblur(st.container, true);
    });
    for (const t of ["pointerdown", "mousedown", "mouseup", "pointerup"]) st.hide.addEventListener(t, stop);
  }

  function activate(st, viaKeyboard) {
    if (st.page) revealPage(st.container.ownerDocument, viaKeyboard);
    else reveal(st.container, viaKeyboard);
  }

  // ---- element helpers ---------------------------------------------------------------------------------------
  function holdMedia(st) {
    try { nSet(st.container, "data-sfw-hold", "1"); } catch (e) { /* ignore */ }
    const list = deepAll(st.container, "video,audio");
    for (const m of list) {
      try {
        if (!st.held.includes(m)) {
          if (!nHas(m, "data-sfw-hold")) { nSet(m, "data-sfw-hold", "1"); st.held.push(m); }
        }
        // what the page had asked for is remembered: when the unit turns out CLEAN (a "Checking" cover that was only a precaution, a
        // budget overflow) the video resumes; a user reveal or a spoiler verdict never restores it
        if (!st.restore.some((r) => r.m === m)) st.restore.push({ m, autoplay: nHas(m, "autoplay"), playing: !m.paused });
        if (!m.paused) m.pause();
        m.muted = true;
        if (nHas(m, "autoplay")) { nDel(m, "autoplay"); }
      } catch (e) { /* ignore */ }
    }
  }

  // Verdict clear after a precautionary hold: give the page's media back what we took (autoplay / playback). Stays muted.
  function resumeMedia(st) {
    for (const r of st.restore) {
      try {
        if (!r.m.isConnected) continue;
        if (r.autoplay) nSet(r.m, "autoplay", "");
        if ((r.autoplay || r.playing) && r.m.paused) { const p = r.m.play(); if (p && p.catch) p.catch(function () { /* autoplay policy: the page / user decides */ }); }
      } catch (e) { /* ignore */ }
    }
    st.restore.length = 0;
  }

  function releaseMedia(st) {
    for (const m of st.held) { try { nDel(m, "data-sfw-hold"); } catch (e) { /* ignore */ } }
    st.held.length = 0;
    try { nDel(st.container, "data-sfw-hold"); } catch (e) { /* ignore */ }
  }

  // Light-DOM descendants plus open/closed shadow roots known to the runtime.
  function deepAll(root, sel, cap) {
    const out = [];
    const limit = cap || 200;
    const walk = (node, depth) => {
      if (out.length >= limit || depth > 6) return;
      const r = node.querySelectorAll ? node.querySelectorAll(sel) : [];
      for (let i = 0; i < r.length && out.length < limit; i++) out.push(r[i]);
      const all = node.querySelectorAll ? node.querySelectorAll("*") : [];
      for (let i = 0; i < all.length && i < 3000; i++) {
        const e = all[i];
        if (e.localName.indexOf("-") < 0 && !e.hasAttribute("data-sfw-sr") && !e.shadowRoot) continue;
        const sr = (deps.rootOf && deps.rootOf(e)) || e.shadowRoot;
        if (sr) walk(sr, depth + 1);
      }
    };
    walk(root, 0);
    if (deps.rootOf) { const sr = deps.rootOf(root); if (sr) walk(sr, 1); }
    return out;
  }

  function inertChildren(st) {
    const kids = st.container.children;
    if (kids.length > MAX_INERT) return;
    for (let i = 0; i < kids.length; i++) {
      const k = kids[i];
      if (k.localName === "sfw-shield" || k.inert) continue;
      try { k.inert = true; st.inerted.push(k); } catch (e) { /* ignore */ }
    }
  }

  function restoreInert(st) {
    for (const k of st.inerted) { try { k.inert = false; } catch (e) { /* ignore */ } }
    st.inerted.length = 0;
  }

  function overlayParent(container) {
    const r = deps.rootOf ? deps.rootOf(container) : container.shadowRoot;
    return r || container;
  }

  function attachHost(st) {
    const parent = overlayParent(st.container);
    parent.appendChild(st.host);
    hosts.set(st.host, st.container);
  }

  // ---- public: container shield -----------------------------------------------------------------------------
  // apply(container, {variant, fp, needPos}) -> state | null.  `variant`: spoiler | media | pending | error.
  // pending is an attribute-only cover (cheap, no shadow DOM) and is never revealable.
  function apply(container, opts) {
    opts = opts || {};
    const variant = opts.variant || "spoiler";
    const fp = opts.fp == null ? null : opts.fp;
    let st = states.get(container);
    if (st && st.mode === "revealed" && !opts.force) {
      st.fp = fp;
      return st;
    }
    const doc = container.ownerDocument;
    if (opts.needPos == null && !st) {
      try { opts.needPos = doc.defaultView.getComputedStyle(container).position === "static"; } catch (e) { opts.needPos = true; }
    }
    if (!st) {
      st = { container, mode: "shielded", variant, fp, revealedFp: null, host: null, root: null, cover: null, hide: null, held: [], inerted: [], restore: [], pos: false, removals: 0, fb: false, page: false, timer: 0 };
      states.set(container, st);
      shielded.add(container);
    } else {
      // existing shielded state: change variant if needed
      if (st.variant === variant && st.mode === "shielded") { st.fp = fp; if (!st.fb || !st.host) return st; }
      st.mode = "shielded";
      st.variant = variant;
      st.fp = fp;
    }
    nSet(container, "data-sfw", "shield");
    if (opts.needPos && !st.pos && !nHas(container, "data-sfw-pos")) { nSet(container, "data-sfw-pos", ""); st.pos = true; }
    inertChildren(st);
    holdMedia(st);
    const fallbackVariant = variant === "pending" || st.removals >= MAX_REMOVALS;
    if (fallbackVariant) {
      dropHost(st);
      st.fb = true;
      nSet(container, "data-sfw-fb", fbOf(variant));
    } else {
      nDel(container, "data-sfw-fb");
      st.fb = false;
      if (st.host) dropHost(st);
      const o = createOverlay(doc, variant, null);
      st.host = o.host; st.root = o.root; st.cover = o.cover; st.hide = o.hide;
      armHost(st);
      wireHost(st);
      attachHost(st);
    }
    return st;
  }

  function dropHost(st) {
    if (st.timer) { clearTimeout(st.timer); st.timer = 0; }
    if (st.host) {
      try { if (st.host.parentNode) st.host.parentNode.removeChild(st.host); } catch (e) { /* ignore */ }
      hosts.delete(st.host);
      st.host = null; st.root = null; st.cover = null; st.hide = null;
    }
  }

  function reveal(container, viaKeyboard) {
    const st = states.get(container);
    if (!st || st.mode !== "shielded" || st.variant === "pending") return false;
    st.mode = "revealed";
    st.revealedFp = st.fp;
    nSet(container, "data-sfw", "revealed");
    restoreInert(st);
    // media stays paused/muted; hold attributes are dropped so the page may play it again on user action
    releaseMedia(st);
    if (st.fb || !st.host) {
      nDel(container, "data-sfw-fb");
      st.fb = false;
    } else {
      disarmHost(st);
      st.hide.hidden = false;
      st.cover.classList.add("out");
      const cover = st.cover;
      const ms = deps.reduced ? 0 : 220;
      st.timer = setTimeout(() => { st.timer = 0; if (st.mode === "revealed" && cover) cover.hidden = true; }, ms);
      if (viaKeyboard) { try { st.hide.focus({ preventScroll: true }); } catch (e) { /* ignore */ } }
    }
    if (deps.onReveal) { try { deps.onReveal(container, st); } catch (e) { /* ignore */ } }
    return true;
  }

  function reblur(container, viaUser) {
    const st = states.get(container);
    if (!st || st.mode !== "revealed") return false;
    st.mode = "shielded";
    st.revealedFp = null;
    nSet(container, "data-sfw", "shield");
    inertChildren(st);
    holdMedia(st);
    if (!st.host) {
      // overlay was lost while revealed: rebuild
      st.removals = Math.min(st.removals, MAX_REMOVALS - 1);
      const o = createOverlay(container.ownerDocument, st.variant === "pending" ? "spoiler" : st.variant, null);
      st.host = o.host; st.root = o.root; st.cover = o.cover; st.hide = o.hide;
      wireHost(st);
      attachHost(st);
    }
    if (st.timer) { clearTimeout(st.timer); st.timer = 0; }
    st.cover.hidden = false;
    st.cover.classList.remove("out");
    st.hide.hidden = true;
    armHost(st);
    if (viaUser) { try { st.host.focus({ preventScroll: true }); } catch (e) { /* ignore */ } }
    if (deps.onReblur) { try { deps.onReblur(container, st); } catch (e) { /* ignore */ } }
    return true;
  }

  function release(container) {
    const st = states.get(container);
    if (!st) return false;
    dropHost(st);
    restoreInert(st);
    releaseMedia(st);
    if (st.mode === "shielded") resumeMedia(st); // never after a user reveal: that path keeps the media paused
    nDel(container, "data-sfw");
    nDel(container, "data-sfw-fb");
    if (st.pos) nDel(container, "data-sfw-pos");
    states.delete(container);
    shielded.delete(container);
    return true;
  }

  function state(container) {
    const st = states.get(container);
    if (!st) return { mode: "none", variant: null, fp: null, revealedFp: null, fallback: false };
    return { mode: st.mode, variant: st.variant, fp: st.fp, revealedFp: st.revealedFp, fallback: st.fb };
  }

  function setFp(container, fp) {
    const st = states.get(container);
    if (st) st.fp = fp;
  }

  // The page (React/Preact/hydration) removed our overlay or rewrote our attributes: bounded repair.
  function repair(container) {
    const st = states.get(container);
    if (!st) return "none";
    const hostGone = st.host && st.host.parentNode !== overlayParent(container);
    if (st.mode === "shielded") {
      if (nGet(container, "data-sfw") !== "shield") nSet(container, "data-sfw", "shield");
      if (st.pos && !nHas(container, "data-sfw-pos")) nSet(container, "data-sfw-pos", "");
      if (hostGone || (st.fb && st.variant !== "pending" && !nHas(container, "data-sfw-fb"))) {
        st.removals++;
        if (st.removals >= MAX_REMOVALS || st.fb) {
          dropHost(st);
          st.fb = true;
          nSet(container, "data-sfw-fb", fbOf(st.variant));
          return "fallback";
        }
        attachHost(st);
        return "reattached";
      }
      return "ok";
    }
    if (st.mode === "revealed") {
      if (hostGone) { st.removals++; if (st.removals <= MAX_REMOVALS) { try { attachHost(st); } catch (e) { /* ignore */ } } else dropHost(st); }
      if (nGet(container, "data-sfw") !== "revealed") nSet(container, "data-sfw", "revealed");
      return "ok";
    }
    return "ok";
  }

  function ownerOfHost(host) {
    return hosts.get(host) || null;
  }

  // Container whose shield (not reveal) state the node belongs to, for event suppression.
  function shieldedAncestor(node) {
    let n = node;
    for (let hops = 0; n && hops < 60; hops++) {
      if (n.nodeType === 1) {
        const v = n.getAttribute && nGet(n, "data-sfw");
        if (v === "shield") return n;
      }
      const p = n.parentNode;
      n = p && p.nodeType === 11 && p.host ? p.host : p;
    }
    return null;
  }

  function count() {
    return shielded.size;
  }

  function all() {
    return shielded;
  }

  function prune() {
    for (const c of shielded) if (!c.isConnected) { const st = states.get(c); if (st) { dropHost(st); releaseMedia(st); } states.delete(c); shielded.delete(c); }
  }

  // ---- page shield ------------------------------------------------------------------------------------------
  const pages = new WeakMap(); // document -> state
  function applyPage(doc, variant) {
    const root = doc.documentElement;
    if (!root) return false;
    let st = pages.get(doc);
    if (st && st.mode === "shielded" && st.host && st.host.parentNode === root) return true;
    if (!st) { st = { container: root, mode: "shielded", variant: variant || "page", host: null, root: null, cover: null, hide: null, held: [], inerted: [], page: true, removals: 0, fb: false, timer: 0, fp: null, revealedFp: null }; pages.set(doc, st); }
    st.mode = "shielded";
    nSet(root, "data-sfw-page", "shield");
    nSet(root, "data-sfw-hold", "1");
    try { for (const m of doc.querySelectorAll("video,audio")) { if (!m.paused) m.pause(); m.muted = true; } } catch (e) { /* ignore */ }
    if (st.host) dropHost(st);
    const o = createOverlay(doc, st.variant === "error" ? "page" : "page", "page");
    st.host = o.host; st.root = o.root; st.cover = o.cover; st.hide = o.hide;
    st.hide.remove();
    armHost(st);
    wireHost(st);
    root.appendChild(st.host);
    // the page overlay is activated by its button as well as by the whole cover
    return true;
  }

  function revealPage(doc, viaKeyboard) {
    const st = pages.get(doc);
    if (!st || st.mode !== "shielded") return false;
    st.mode = "revealed";
    const root = doc.documentElement;
    nDel(root, "data-sfw-page");
    nDel(root, "data-sfw-hold");
    st.cover.classList.add("out");
    const host = st.host;
    st.timer = setTimeout(() => { st.timer = 0; try { if (host.parentNode) host.parentNode.removeChild(host); } catch (e) { /* ignore */ } }, deps.reduced ? 0 : 240);
    st.host = null;
    if (deps.onReveal) { try { deps.onReveal(null, st); } catch (e) { /* ignore */ } }
    return true;
  }

  function releasePage(doc) {
    const st = pages.get(doc);
    if (!st) return;
    const root = doc.documentElement;
    if (root) { nDel(root, "data-sfw-page"); nDel(root, "data-sfw-hold"); }
    if (st.host) { try { st.host.remove(); } catch (e) { /* ignore */ } }
    pages.delete(doc);
  }

  function pageState(doc) {
    const st = pages.get(doc);
    return st ? st.mode : "none";
  }

  // Small fixed notice ("Spoiler protection error on this page"). Sheet-less fallback paints with inline style.
  function notice(doc) {
    const root = doc.documentElement || doc.body;
    if (!root) return null;
    const o = createOverlay(doc, "notice", "notice");
    o.hide.remove();
    o.host.setAttribute("role", "status");
    o.host.setAttribute("data-s", "notice");
    root.appendChild(o.host);
    return o.host;
  }

  // Small corner notice that needs no stylesheet: inline CSSOM style only (cold pages where the engine could not start).
  function emergencyNotice(doc, text) {
    const root = doc.documentElement;
    if (!root) return null;
    const d = doc.createElement("div");
    d.textContent = text;
    d.setAttribute("data-sfw-emergency", "notice");
    d.setAttribute("role", "status");
    d.style.cssText = "position:fixed;right:14px;bottom:14px;z-index:2147483647;background:#12131b;color:#ffd27a;padding:9px 13px;border-radius:12px;font:600 13px system-ui,sans-serif;box-shadow:0 6px 24px rgba(0,0,0,.35)";
    root.appendChild(d);
    return d;
  }

  // Last-resort cover that needs no stylesheet (adoptedStyleSheets unavailable): inline CSSOM style only.
  function emergencyCover(doc, text) {
    const root = doc.documentElement;
    if (!root) return null;
    const d = doc.createElement("div");
    d.textContent = text;
    d.setAttribute("data-sfw-emergency", "1");
    root.setAttribute("data-sfw-page", "shield"); // the page IS covered (DOM invariants: no spoiler outside a shield)
    const s = d.style;
    s.cssText = "position:fixed;inset:0;z-index:2147483647;background:#12131b;color:#ffb3ba;display:flex;align-items:center;justify-content:center;font:700 18px system-ui,sans-serif;text-align:center;padding:24px";
    root.appendChild(d);
    return d;
  }

  return {
    TEXT, configure, prepare, apply, reveal, reblur, release, state, setFp, repair,
    ownerOfHost, shieldedAncestor, count, all, prune,
    applyPage, revealPage, releasePage, pageState, notice, emergencyCover, emergencyNotice, deepAll,
  };
})();
