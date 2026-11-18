// SFW.runtime — observers, unit resolution, classification scheduling and page guards (DESIGN §5, §10).
//
// Zero-flash invariants this file maintains:
//  1. Everything that can change what is painted happens synchronously inside a MutationObserver callback
//     (a microtask that runs before the next rendering opportunity) or inside a requestAnimationFrame callback
//     (which runs before style/layout/paint of that frame).
//  2. Every failure path ends protective: budget overflow -> `pending` cover, exception -> `error` cover,
//     engine failure -> page shield (hot / high-risk hosts) or a visible notice.
//  3. `pending` is only ever resolved by a completed classification, never by a timer.
SFW.runtime = (function () {
  const BUDGET_MS = 12; // per MutationObserver callback
  const IDLE_BUDGET_MS = 8; // per drain slice for pending units
  const ITEM_TEXT_CAP = 1500; // generic item (li/article/...) larger than this falls back to the block
  const LADDER_TEXT_CAP = 1200; // ancestor blocks evaluated for text split across sibling elements
  const LADDER_LEVELS = 4;
  const MAX_TEXT = 200000; // per unit: at most this many characters are extracted (head + tail when one text node exceeds it)
  const MAX_QUEUE = 60000;
  const WIN_CHARS = 12000; // long texts are classified in windows of this size (+ overlap), cooperatively across slices
  const WIN_OVERLAP = 500;
  const SLICE_MS = 10; // budget of a continuation slice (the first slice of a MutationObserver callback uses BUDGET_MS)
  const EXPAND_AHEAD = 384; // units resolved before they are decided (keeps the cover for the rest cheap)
  const EVAL_BATCH = 192;
  const PENDING_MAX = 12; // leftover units up to this many get their own "Checking" cover; the rest are hidden by a marker on each unit
  const UNIT_MARK_MAX = 1500; // leftover units hidden one by one up to this many, grouped under their parents beyond
  const GROUP_MIN = 48; // this many outstanding nodes under one parent: the parent itself is covered
  const MARK_MAX = 2500;
  const VISIT_MAX = 30000; // element/text nodes visited per unit at most (a unit with more is pathological; its nested blocks are units too)
  const ENTITIES = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " ", ndash: "-", mdash: "-", hellip: "...", rsquo: "'", lsquo: "'", ldquo: '"', rdquo: '"' };
  const CRUDE_WORDS = ["gta", "grand theft", "grandtheft", "vice city", "vicecity", "leonida", "rockstar"]; // crude hotness test, no RegExp
  const RAF_WATCH_MS = 2000; // rAF safety pass keeps running this long after load / a big insertion (DESIGN §5.1 step 3)
  const BIG_INSERT = 1500; // a callback that delivers more added nodes than this only covers them (O(1)) and continues in slices
  // An unverifiable embedded frame (sandboxed without scripts: the engine cannot run inside it) is covered in Strict on
  // cold pages too once it is large enough to show one line of text; only tracking-pixel sized frames stay clear.
  const FRAME_MIN_W = 48;
  const FRAME_MIN_H = 16;
  const TITLE_GUARD = "Spoiler-protected page";
  const LAUNCH = Date.UTC(2026, 10, 19);
  const MO_ATTRS = ["alt", "title", "aria-label", "aria-description", "placeholder", "href", "src", "srcset", "poster", "srcdoc", "sandbox", "data-sfw-sr", "data-sfw"];
  // tag tables are filled with add(): a (broken) Set polyfill that ignores its iterable argument must not leave them empty
  function mkSet(list) {
    const s = new Set();
    for (let i = 0; i < list.length; i++) s.add(list[i]);
    return s;
  }
  const BLOCK = mkSet(["P", "DIV", "LI", "H1", "H2", "H3", "H4", "H5", "H6", "BLOCKQUOTE", "PRE", "TD", "TH", "DD", "DT", "SECTION", "ARTICLE", "FIGURE", "FIGCAPTION", "TR", "ASIDE", "HEADER", "FOOTER", "MAIN", "UL", "OL", "TABLE", "DETAILS", "SUMMARY", "FORM", "FIELDSET", "NAV", "BODY", "HTML", "ADDRESS", "DL", "CAPTION"]);
  const SKIP = mkSet(["SCRIPT", "STYLE", "NOSCRIPT", "TEMPLATE", "SFW-SHIELD", "HEAD", "TITLE", "META", "LINK"]);
  const MEDIA_TAGS = mkSet(["IMG", "VIDEO", "PICTURE", "CANVAS", "OBJECT", "EMBED"]);
  const LISTISH = mkSet(["LI", "TR", "DD", "DT", "BLOCKQUOTE", "FIGURE"]);
  const ATTR_TEXT = ["alt", "title", "aria-label", "aria-description", "placeholder"];
  // inert() (handleRecords): elements whose arrival or removal can change what a unit says even when empty
  const INERT_DENY = mkSet(["a", "area", "iframe", "frame", "frameset", "input", "textarea", "select", "option", "button", "image", "svg", "math", "slot", "template", "source", "track", "audio", "br", "wbr", "html", "head", "body", "title", "meta", "link", "base"]);
  const INERT_ATTR_DENY = ["alt", "title", "aria-label", "aria-description", "placeholder", "href", "src", "srcset", "data-src", "poster", "srcdoc", "sandbox", "value", "data-sfw", "data-sfw-sr", "data-sfw-hold", "data-sfw-defer"];
  const HTML_NS = "http://www.w3.org/1999/xhtml";
  // media OCR (host.ocr contract, DESIGN §5.3)
  const OCR_MIN_W = 96; // rendered AND natural size must reach this, else the media is "small" and follows the v1 rule
  const OCR_MIN_H = 64;
  const OCR_MARGIN = "150% 150% 150% 150%"; // IntersectionObserver rootMargin: request OCR within ~1.5 viewports
  const OCR_FLIGHT = 3; // concurrent host requests per frame, unless the host says it can take more (host.ocr.flight)
  const OCR_FLIGHT_MAX = 16;
  const OCR_WATCHDOG_MS = 60000; // the host must settle sooner (it enforces its own timeout); this only backs it up
  const OCR_ERR_TTL_MS = 20000; // unavailable/error results are cached briefly, then retried once
  const OCR_CACHE_MAX = 1500;
  const OCR_LOAD_FALLBACK_MS = 600; // src changed but no load event arrived (identical URL): re-check anyway
  const OCR_SEL = 'img,video[poster],[style*="background-image"]';
  const BG_URL = /background(?:-image)?\s*:[^;]*?url\(\s*(["']?)(.*?)\1\s*\)/i;

  // ---- builtins captured at bundle load ---------------------------------------------------------------------------
  // Every platform primitive the hot paths need is taken once, here, through Function.prototype.call.bind (so later
  // patches of Element.prototype.*, String.prototype.*, MutationObserver, ... by page scripts cannot disable the engine).
  // A primitive that is missing or not callable at this point degrades to a late-bound lookup instead of throwing.
  const G = globalThis;
  const CALL = Function.prototype.call;
  function uncurry(proto, name) {
    try {
      const f = proto && proto[name];
      if (typeof f === "function") return CALL.bind(f);
    } catch (e) { /* fall through */ }
    return function (o, a, b, c) { return o[name](a, b, c); };
  }
  const ElP = G.Element && G.Element.prototype;
  const NodeP = G.Node && G.Node.prototype;
  const DocP = G.Document && G.Document.prototype;
  const StrP = String.prototype;
  const ArrP = Array.prototype;
  const nGetAttr = uncurry(ElP, "getAttribute");
  const nHasAttr = uncurry(ElP, "hasAttribute");
  const nSetAttr = uncurry(ElP, "setAttribute");
  const nRemoveAttr = uncurry(ElP, "removeAttribute");
  const nClosest = uncurry(ElP, "closest");
  const nMatches = uncurry(ElP, "matches");
  const nQS = uncurry(ElP, "querySelector");
  const nQSA = uncurry(ElP, "querySelectorAll");
  const nGBCR = uncurry(ElP, "getBoundingClientRect");
  const nGetRoot = uncurry(NodeP, "getRootNode");
  const nCreateEl = uncurry(DocP, "createElement");
  const nSlice = uncurry(StrP, "slice");
  const nIndexOf = uncurry(StrP, "indexOf");
  const nTrim = uncurry(StrP, "trim");
  const nCharCode = uncurry(StrP, "charCodeAt");
  const nReTest = uncurry(RegExp.prototype, "test");
  const nReExec = uncurry(RegExp.prototype, "exec");
  const aPush = uncurry(ArrP, "push");
  const aSlice = uncurry(ArrP, "slice");
  const aIndexOf = uncurry(ArrP, "indexOf");
  const aConcat = uncurry(ArrP, "concat");
  const aJoin = uncurry(ArrP, "join");
  const aFilter = uncurry(ArrP, "filter");
  const aSort = uncurry(ArrP, "sort");
  const nGetCS = typeof G.getComputedStyle === "function" ? G.getComputedStyle.bind(G) : null;
  const MO_CTOR = typeof G.MutationObserver === "function" ? G.MutationObserver : null;
  const MO_OBSERVE = uncurry(MO_CTOR && MO_CTOR.prototype, "observe");
  const MO_TAKE = uncurry(MO_CTOR && MO_CTOR.prototype, "takeRecords");
  const MO_DISCONNECT = uncurry(MO_CTOR && MO_CTOR.prototype, "disconnect");
  const T_SET = typeof G.setTimeout === "function" ? G.setTimeout.bind(G) : null;
  const T_CLEAR = typeof G.clearTimeout === "function" ? G.clearTimeout.bind(G) : null;
  const RAF = typeof G.requestAnimationFrame === "function" ? G.requestAnimationFrame.bind(G) : null;
  const PERF_NOW = G.performance && typeof G.performance.now === "function" ? G.performance.now.bind(G.performance) : null;
  const DATE_NOW = Date.now.bind(Date);
  const MC_CTOR = typeof G.MessageChannel === "function" ? G.MessageChannel : null;
  const SCHED_POST = G.scheduler && typeof G.scheduler.postTask === "function" ? G.scheduler.postTask.bind(G.scheduler) : null;
  const DOC_SHEETS = (function (p) {
    // adoptedStyleSheets accessors captured before a page can delete or replace them
    const d = {};
    try { d.doc = Object.getOwnPropertyDescriptor(G.Document.prototype, "adoptedStyleSheets") || null; } catch (e) { d.doc = null; }
    try { d.root = Object.getOwnPropertyDescriptor(G.ShadowRoot.prototype, "adoptedStyleSheets") || null; } catch (e) { d.root = null; }
    return d;
  })();
  const SHEET_CTOR = typeof G.CSSStyleSheet === "function" ? G.CSSStyleSheet : null;
  const SHEET_REPLACE = uncurry(SHEET_CTOR && SHEET_CTOR.prototype, "replaceSync");
  function getSheets(target) {
    const d = target && target.nodeType === 11 ? DOC_SHEETS.root : DOC_SHEETS.doc;
    try { if (d && d.get) { const v = d.get.call(target); if (v) return v; } } catch (e) { /* ignore */ }
    try { const v = target.adoptedStyleSheets; if (v) return v; } catch (e) { /* ignore */ }
    return null;
  }
  function setSheets(target, list) {
    const d = target && target.nodeType === 11 ? DOC_SHEETS.root : DOC_SHEETS.doc;
    if (d && d.set) { d.set.call(target, list); return; }
    target.adoptedStyleSheets = list;
  }
  // Whitespace test by character code (no RegExp: a page that replaces RegExp.prototype.test / exec must not turn every text node
  // into "blank"). Same set as /\s/: ASCII space characters, NBSP, Unicode space separators, line terminators, BOM.
  function isSpaceCode(c) {
    return c === 32 || (c >= 9 && c <= 13) || c === 160 || c === 5760 || (c >= 8192 && c <= 8202) || c === 8232 || c === 8233 || c === 8239 || c === 8287 || c === 12288 || c === 65279;
  }
  function isBlank(s) {
    for (let i = 0; i < s.length; i++) if (!isSpaceCode(nCharCode(s, i))) return false;
    return true;
  }
  // next node in document order inside `root` (never leaves it); `skip` = do not descend into n
  function nextNode(n, root, skip) {
    if (!skip) { const c = n.firstChild; if (c) return c; }
    while (n && n !== root) {
      const s = n.nextSibling;
      if (s) return s;
      n = n.parentNode;
    }
    return null;
  }

  // ---- pure helpers (exported for unit tests) --------------------------------------------------------------
  function fnv1a(s) {
    let h = 0x811c9dc5;
    for (let i = 0; i < s.length; i++) {
      h ^= s.charCodeAt(i);
      h = Math.imul(h, 0x01000193);
    }
    return h >>> 0;
  }
  function fingerprint(s) {
    return fnv1a(s).toString(36) + "." + s.length.toString(36);
  }
  function slugText(href) {
    if (!href) return "";
    let p = String(href);
    const q = p.indexOf("#");
    if (q >= 0) p = p.slice(0, q);
    p = p.replace(/^[a-z][a-z0-9+.-]*:\/\/[^/?#]*/i, ""); // drop scheme + host
    if (/^(javascript|data|mailto|tel|blob):/i.test(p)) return "";
    try { p = decodeURIComponent(p); } catch (e) { /* keep raw */ }
    return p.replace(/[-_+/.=&?%]+/g, " ").trim();
  }
  function srcWords(src) {
    if (!src || /^data:/i.test(src)) return "";
    let p = String(src).split(/[?#]/)[0];
    const i = p.lastIndexOf("/");
    p = i >= 0 ? p.slice(i + 1) : p;
    p = p.replace(/\.[a-z0-9]{2,5}$/i, "");
    try { p = decodeURIComponent(p); } catch (e) { /* keep raw */ }
    p = p.replace(/[-_+.]+/g, " ").trim();
    return /[a-z]{4,}/i.test(p) ? p : "";
  }
  function isBlockTag(tag) {
    return BLOCK.has(tag) || tag.indexOf("-") > 0;
  }

  // ---- embedded frame helpers (pure; exported for unit tests) ------------------------------------------------
  function hasToken(list, tok) {
    const a = String(list).toLowerCase().split(/\s+/);
    return aIndexOf(a, tok) >= 0;
  }

  function dataUrlHtml(src) {
    const m = /^\s*data:text\/html(;[^,]*)?,/i.exec(src);
    if (!m) return null;
    let body = nSlice(src, m[0].length, m[0].length + 400000);
    try {
      if (m[1] && /;base64/i.test(m[1])) {
      const bin = globalThis.atob(body);
      try { return new globalThis.TextDecoder().decode(Uint8Array.from(bin, (c) => c.charCodeAt(0))); } catch (e) { return bin; }
    }
      return decodeURIComponent(body);
    } catch (e) { return body; }
  }

  function decodeEntities(t) {
    if (nIndexOf(t, "&") < 0) return t;
    return t.replace(/&(#x[0-9a-f]{1,6}|#[0-9]{1,7}|[a-z]{2,8});/gi, (m, g) => {
      if (g.charCodeAt(0) === 35) {
        const cp = g.charCodeAt(1) === 120 || g.charCodeAt(1) === 88 ? parseInt(g.slice(2), 16) : parseInt(g.slice(1), 10);
        if (cp > 0 && cp < 0x110000) { try { return String.fromCodePoint(cp); } catch (e) { return " "; } }
        return " ";
      }
      const v = ENTITIES[g.toLowerCase()];
      return v !== undefined ? v : m;
    });
  }

  // Cheap HTML-to-text tokenizer for srcdoc / data: HTML (no DOMParser, nothing is ever parsed into the live document):
  // text between tags, the words in alt / title / aria-label / placeholder / value attributes and in src / href file names;
  // script and style bodies and comments are dropped. Linear, bounded (400k characters).
  function srcdocText(html) {
    html = String(html);
    if (html.length > 400000) html = nSlice(html, 0, 200000) + " " + nSlice(html, html.length - 200000);
    const out = [];
    const n = html.length;
    let i = 0;
    let lower = null;
    const pushText = (t) => { if (t && !isBlank(t)) aPush(out, decodeEntities(t)); };
    while (i < n) {
      const lt = nIndexOf(html, "<", i);
      if (lt < 0) { pushText(nSlice(html, i)); break; }
      if (lt > i) pushText(nSlice(html, i, lt));
      if (html.substr(lt, 4) === "<!--") { const e = nIndexOf(html, "-->", lt + 4); i = e < 0 ? n : e + 3; continue; }
      const c = nCharCode(html, lt + 1);
      if (!((c >= 65 && c <= 90) || (c >= 97 && c <= 122) || c === 47 || c === 33 || c === 63)) { pushText("<"); i = lt + 1; continue; }
      const lim = lt + 8000 < n ? lt + 8000 : n;
      let j = lt + 1;
      let q = 0;
      while (j < lim) {
        const ch = nCharCode(html, j);
        if (q) { if (ch === q) q = 0; } else if (ch === 34 || ch === 39) q = ch; else if (ch === 62) break;
        j++;
      }
      const tag = nSlice(html, lt + 1, j);
      const m = /^(\/?)([a-zA-Z][a-zA-Z0-9-]*)/.exec(tag);
      let skipTo = -1;
      if (m && !m[1]) {
        const name = m[2].toLowerCase();
        const re = /\b(?:alt|title|aria-label|aria-description|placeholder|value|label)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))/gi;
        let am;
        while ((am = re.exec(tag))) { const v = am[1] !== undefined ? am[1] : am[2] !== undefined ? am[2] : am[3]; if (v) pushText(v); }
        const sm = /\b(?:src|href|data-src|poster)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))/i.exec(tag);
        if (sm) { const v = sm[1] || sm[2] || sm[3]; if (v && !/^data:/i.test(v)) { const w = name === "a" ? slugText(v) : srcWords(v); if (w) pushText(w); } }
        if (name === "script" || name === "style") {
          if (lower === null) lower = html.toLowerCase();
          const e = nIndexOf(lower, "</" + name, j);
          skipTo = e < 0 ? n : e;
        }
      }
      i = skipTo >= 0 ? skipTo : (j < lim ? j + 1 : lim);
    }
    return aJoin(out, " ");
  }


  function crudeHot(s) {
    try {
      const t = String(s || "").toLowerCase();
      for (let i = 0; i < CRUDE_WORDS.length; i++) if (nIndexOf(t, CRUDE_WORDS[i]) >= 0) return true;
    } catch (e) { /* ignore */ }
    return false;
  }

  function now() {
    return PERF_NOW ? PERF_NOW() : DATE_NOW();
  }

  // A value that changed only in its numbers: a like / view / reply counter, a relative time, a video clock ("12 Likes" ->
  // "13 Likes", "5h" -> "6h", "0:31" -> "0:32"). Feeds tick these many times a second and every tick used to re-read and
  // re-classify the whole post. Posters cannot change a text in place (only the site's own script can), and the guard
  // in counterTick() keeps any number that could complete a franchise name ("GTA 5" -> "GTA 6") on the full path.
  const COUNTER_MAX = 80;
  const DIGITS = /\d+(?:[.,:]\d+)*/g;
  function counterOnly(a, b) {
    if (typeof a !== "string" || typeof b !== "string" || a === b) return false;
    if (a.length > COUNTER_MAX || b.length > COUNTER_MAX) return false;
    const ma = a.replace(DIGITS, "#"), mb = b.replace(DIGITS, "#");
    return ma === mb && nIndexOf(mb, "#") >= 0;
  }
  function counterTick(a, b, context) {
    return counterOnly(a, b) && !crudeHot(b) && !crudeHot(context) && nIndexOf(String(context || "").toLowerCase(), "\u0433\u0442\u0430") < 0;
  }

  // ---- start -------------------------------------------------------------------------------------------------
  // start() never throws into the page. If even the engine's own state cannot be built (a page replaced Map / WeakMap /
  // MutationObserver with something broken before us) the last resort is a visible notice, upgraded to a cover as soon as the
  // page shows GTA VI content (judged by plain substring tests), and the host is told.
  function start(host) {
    try {
      return startEngine(host);
    } catch (e) {
      return lastResort(host, e);
    }
  }

  function lastResort(host, e) {
    const ctrl = { stop() {}, stats() { return { errors: 1, shields: 0, pending: 0, lastError: String(e && e.message || e).slice(0, 200), moMode: "none" }; } };
    try { if (host && host.report) host.report("error.start", 1); } catch (x) { /* ignore */ }
    try {
      const doc = G.document;
      const html = () => doc.documentElement;
      let shown = false, covered = false;
      const text = () => {
        const root = doc.body || doc.documentElement;
        if (!root) return "";
        let out = "";
        let n = nextNode(root, root, false);
        let visits = 0;
        while (n && out.length < 100000 && visits++ < 60000) {
          let skip = false;
          if (n.nodeType === 3) out += " " + n.data;
          else if (n.nodeType === 1 && SKIP.has(n.tagName)) skip = true;
          n = nextNode(n, root, skip);
        }
        return out;
      };
      const mk = (cover) => {
        const r = html();
        if (!r) return;
        const d = doc.createElement("div");
        d.textContent = cover ? "Spoiler Firewall could not start on this page. Content hidden to protect you from GTA VI spoilers." : "Spoiler protection error on this page";
        nSetAttr(d, "data-sfw-emergency", cover ? "1" : "notice");
        d.style.cssText = cover
          ? "position:fixed;inset:0;z-index:2147483647;background:#12131b;color:#ffb3ba;display:flex;align-items:center;justify-content:center;font:700 18px system-ui,sans-serif;text-align:center;padding:24px"
          : "position:fixed;right:14px;bottom:14px;z-index:2147483647;background:#12131b;color:#ffd27a;padding:9px 13px;border-radius:12px;font:600 13px system-ui,sans-serif";
        r.appendChild(d);
        if (cover) nSetAttr(r, "data-sfw-page", "shield");
      };
      const check = () => {
        if (covered) return true;
        let hot = false;
        try { hot = crudeHot(String(G.location.href) + " " + (doc.title || "") + " " + text()); } catch (x) { /* ignore */ }
        if (hot) { covered = true; mk(true); }
        return covered;
      };
      const tick = (n) => {
        if (covered || n > 600) return;
        const again = () => { if (!check()) tick(n + 1); };
        if (n < 240 && RAF) { try { RAF(again); return; } catch (x) { /* fall through */ } }
        if (T_SET) T_SET(again, n < 280 ? 250 : 1000);
      };
      const first = () => { if (!shown && html()) { shown = true; if (!check()) mk(false); tick(0); } else if (!shown && T_SET) T_SET(first, 25); };
      first();
      if (MO_CTOR) { try { const m = new MO_CTOR(() => { if (!covered) check(); }); MO_OBSERVE(m, doc, { childList: true, subtree: true, characterData: true }); } catch (x) { /* ignore */ } }
    } catch (x) { /* nothing left to do */ }
    return ctrl;
  }

  function startEngine(host) {
    const win = globalThis;
    let doc = win.document; // rebound when the window gets a new document (initial about:blank of a popup, see rebind())
    const loc = win.location;
    const S = SFW;
    const stats = {
      batches: 0, units: 0, shields: 0, pending: 0, errors: 0, maxBatchMs: 0, totalMs: 0, samples: [],
      firstShieldAt: 0, rafCaught: 0, rafChecks: 0, records: 0, charDataRecords: 0, roots: 0, repairs: 0, fallbacks: 0, urlChanges: 0,
    };
    let cfg = normalizeConfig(host && host.config);
    let configPending = !!(host && host.configReady && typeof host.configReady.then === "function");
    let adapter = null;
    let mo = null;
    let booted = false;
    let stopped = false;
    let pageSheet = null, shieldSheet = null, quarSheet = null;
    let sheetsOk = false;
    let opaqueRule = false;
    let pageHot = 0;
    let liveNow = false; // the page is a live stream and the adapter has an exempt region (checkLive)
    let pageWant = false; // the URL or title alone calls for protection (evalPage): the page is hot, its content masked item by item
    let lastHref = loc.href;
    let pageRevealedKey = null;
    let titleOriginal = null;
    let titleRevealedFor = null;
    let launchWindow = false;
    let rafId = 0;
    let drainScheduled = false;
    let workTimer = 0;
    let detectionBroken = false; // the classifier does not work on this page (canary failed): never shield units for it on cold pages
    let moMode = "mo"; // "mo" | "poll" (MutationObserver missing/dead: rAF + timer rescans)
    let failShown = false;
    let coverShown = false;
    let healthAt = -1e9, healthN = 0; // classifier health check (see healthCheck)
    let failMo = null; // observer used only while protection is down on a page judged cold
    let pollChanged = false, pollIdle = 0; // poll mode: did the last scan change anything (tight loop) or not (back off)
    let sliceDeadline = Infinity;
    let pollTimer = 0, pollNext = 0;
    let swapWatch = false;
    let swapPoll = 0, swapPollUntil = 0;
    let slicePageCtx = false;
    let addedBurst = 0; // nodes added in the records of the current callback
    let sliceOcr = false;
    const sliceCtxRoots = new Set();
    let ucache = new Map(); // per slice: element -> resolved unit | null
    let tlcache = new Map(); // per slice: element -> text length (capped)
    const chunks = []; // newly inserted subtree roots waiting for expansion: {target, nodes, i, cover}
    let chunkHead = 0;
    let expandCur = null; // resumable depth-first walk of the current root: {root, node}
    const unitQ = new Set(); // resolved units waiting for a decision
    const freshUnits = new Set(); // units in unitQ whose content changed (new / edited text), not only re-checked
    const structOnly = new Set(); // queued nodes touched only as the parent of added / removed children (touchStruct)
    const inertSeen = new WeakSet(); // elements that arrived inert and have had no record since (handleRecords, inert())
    let noteFresh = true;
    const marks = new Set(); // elements carrying data-sfw-defer (region cover for work that did not fit in the budget)
    const longJobs = new Map(); // unit -> windowed classification in progress
    let overflow = false;
    let titleDirty = true;
    let warm = false;
    let pageCtxDirty = true;
    const shieldTimes = [];
    const queue = new Set();
    const repairSet = new Set();
    const pendingSet = new Set();
    const threadWait = new Map();
    const recent = new Set();
    let rafWatchUntil = 0; // the rAF re-check of touched units runs while loading and until this time (2 s after load / a big insertion)
    const U = new WeakMap(); // unit -> {fp, cfp, dec, targets}
    const NONTEXT_SEL = "img,video,picture,canvas,svg,iframe,embed,object,audio,[style*='background-image'],[style*='background:']"; // holdsNonText()
    const BG_CHILDREN = 12; // hasBackgroundMedia(): direct children whose computed background-image is read
    const bgMemo = new WeakMap(); // item -> {n: child count, v}
    const ESCALATE_SEL = "article,[role=article],li,figure,blockquote,[role=listitem]"; // escalationTarget()
    const ESC_MAX_CHILDREN = 8; // a card has few children; an article of paragraphs does not
    // Declared up here, not next to their functions below: the first scan runs while startEngine is still being
    // evaluated (a synchronous observer callback), and a const read before its line throws (temporal dead zone).
    const SEARCH_NAMES = mkSet(["q", "query", "search", "search_query", "p", "keywords", "text", "s"]);
    const SEARCH_UI = "[role=search],yt-searchbox,ytd-searchbox,form#search-form,form[action*=search],form[action*=results],[data-testid=SearchBox_Search_Input]";
    const SEARCH_UI_TEXT_MAX = 400; // a search box carries little text; a page-wide <form action="/search.aspx"> is not one
    const escalated = new Set(); // generic cards shielded whole because a block inside them is a spoiler
    // shield targets that give "local heat" to their siblings: spoiler verdicts on the unit's own content only. Pending,
    // error and frame covers say nothing about the content, and a shield that itself came from neighbour heat must not
    // pass it on (that chained across every sibling with an icon, up to whole page shells)
    const heatSrc = new WeakSet();
    const threads = new WeakMap(); // root -> {resolved, hot, first, shield, key}
    const seenRoots = new WeakSet();
    const rootList = new Set();
    const cleanups = [];
    // media OCR state (see "media OCR gate" below)
    const mrec = new WeakMap(); // media element -> record
    const unitInfo = new WeakMap(); // unit -> {recs:Set, plain:[]}
    const ocrVer = new WeakMap(); // unit -> bumped whenever one of its OCR results changes
    const mediaVer = new WeakMap(); // unit -> bumped whenever one of its media elements is added or changed
    const liveRecs = new Set(); // records not yet resolved (held / queued / running)
    const failRecs = new Set(); // records covered by a "couldn't check image" shield
    const ocrCache = new Map(); // url key -> {status, text, at}
    const ocrFlight = new Map(); // url key -> Set(record) waiting for the request in flight
    const ocrQueue = new Set(); // records waiting for a request slot
    const ocrEvalSet = new Set(); // units to re-decide after OCR results arrived
    let ocrFlying = 0;
    let ocrIO = null, ocrRO = null, ocrBgMo = null;
    let ocrPumpTimer = 0, ocrEvalTimer = 0;
    let lastOcrOn = false;
    stats.ocr = { ok: 0, unavailable: 0, error: 0, skipped: 0, requests: 0, dedupe: 0, cacheHits: 0, small: 0, released: 0, failShields: 0, ocrShields: 0, maxFlight: 0, retries: 0 };
    if (host && host.kind === "test") { stats.trace = []; stats.ocrLog = []; stats.slow = []; }
    const ctrl = { stop, stats: getStats };
    if (host && host.kind === "test") ctrl.explain = (el) => { const p = decide(el, !isOwn(el)); const ex = extract(el, isOwn(el), isDeep(el)); return { dec: p.dec, hot: p.hot, fp: p.fp, deep: isDeep(el), text: ex.text.slice(0, 300), unit: (resolveUnit(el) === el) }; };

    function noteError(e) {
      // test host only: remember the exception message + top frame (never page text) so harness failures are diagnosable
      if (host && host.kind === "test") { try { stats.lastError = String(e && e.stack || e).slice(0, 400); } catch (x) { /* ignore */ } }
    }

    function report(code, n) {
      try { if (host && host.report) host.report(code, n || 1); } catch (e) { /* ignore */ }
    }

    // ---- boot ---------------------------------------------------------------------------------------------
    let lexVersionSeen = S.lexicon.version;
    try {
      boot();
    } catch (e) {
      stats.errors++;
      fatal(e);
    }
    if (host && typeof host.onConfigChange === "function") {
      try { host.onConfigChange(onConfig); } catch (e) { /* ignore */ }
    }
    if (configPending) {
      try { host.configReady.then((c) => { if (c) cfg = normalizeConfig(c); onConfigReady(); }, () => { cfg = normalizeConfig(cfg); onConfigReady(); }); } catch (e) { onConfigReady(); }
    }
    return ctrl;

    function onConfigReady() {
      configPending = false;
      lexVersionSeen = S.lexicon.version;
      if (!cfg.enabled) { teardown(); return; }
      if (!booted && !stopped) { try { boot(); } catch (e) { fatal(e); } return; }
      if (booted) { applyReducedEffects(); ocrSync(); ocrKick(); rescanAll(); }
    }

    function onConfig(c) {
      const was = cfg;
      cfg = normalizeConfig(c);
      configPending = false;
      if (!cfg.enabled) { teardown(); return; }
      if (!booted && !stopped) { try { boot(); } catch (e) { fatal(e); } return; }
      const ocrChanged = ocrSync();
      // a term-feed update (lexicon.extend) changes the version; the version is part of every unit fingerprint, so a
      // rescan re-classifies text that was already on the page against the added terms
      const lexChanged = lexVersionSeen !== S.lexicon.version;
      lexVersionSeen = S.lexicon.version;
      if (ocrChanged || lexChanged || was.mode !== cfg.mode || was.reducedEffects !== cfg.reducedEffects) { applyReducedEffects(); ocrKick(); rescanAll(); }
    }

    function boot() {
      if (booted || stopped || !cfg.enabled) return;
      adapter = S.adapters.forLocation(loc, doc);
      launchWindow = Math.abs(DATE_NOW() - LAUNCH) <= 56 * 864e5;
      makeSheets();
      S.shield.configure({
        sheet: shieldSheet, rootOf, reduced: cfg.reducedEffects,
        onReveal: onShieldReveal, onReblur: onShieldReblur,
      });
      applyReducedEffects();
      booted = true;
      lastOcrOn = ocrOn();
      // start-up canary: a classifier or observer that a page patched into uselessness must be noticed now, not trusted
      runCanary();
      const opts = { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: MO_ATTRS, attributeOldValue: true, characterDataOldValue: true };
      ctrl.opts = opts;
      if (moMode === "mo") {
        mo = new MO_CTOR(onMutations);
        MO_OBSERVE(mo, doc, opts);
      } else {
        mo = null;
        report("degraded.poll");
      }
      if (S.hooks && S.hooks.subscribe) cleanups.push(S.hooks.subscribe(onShadowAttached));
      installGuards();
      // pay the one-off lexicon / regex compilation now (document_start, nothing to protect yet), not inside the first batch
      try { S.classify.text("gta 6 warm-up", { hot: 0, source: "text" }); S.classify.url("https://example.invalid/?q=warm-up"); } catch (e) { /* ignore */ }
      recomputeUrlContext();
      if (detectionBroken) { fail(new Error("classifier unavailable"), "error.detect"); }
      else evalPage();
      if (doc.readyState === "loading") scheduleRaf();
      if (doc.documentElement) {
        // injected late (document already has content): scan everything now
        scanSubtree(doc.documentElement, true);
        findExistingRoots(doc.documentElement);
      }
      runWork(now(), BUDGET_MS);
      if (moMode === "poll") startPoll();
      armSwapWatch();
      schedule();
    }

    // ---- start-up canary + degraded modes ---------------------------------------------------------------------
    function runCanary() {
      detectionBroken = false;
      try {
        const r = S.classify.text("GTA 6 ending spoiler leaked", { hot: 0, source: "text" });
        detectionBroken = !(r && (r.level === "spoiler" || r.level === "mention"));
      } catch (e) { detectionBroken = true; noteError(e); }
      let ok = false;
      try {
        if (MO_CTOR) {
          const probe = nCreateEl(doc, "i");
          const m = new MO_CTOR(function () {});
          MO_OBSERVE(m, probe, { attributes: true });
          nSetAttr(probe, "x", "1");
          const recs = MO_TAKE(m);
          MO_DISCONNECT(m);
          ok = !!recs && recs.length === 1;
        }
      } catch (e) { ok = false; noteError(e); }
      moMode = ok ? "mo" : "poll";
    }

    // The classifier is tested again while the page runs: a script that patches RegExp / String builtins after the engine started
    // must not switch detection off silently. Every run for the first dozen slices (the patch is usually the page's first
    // script), then every 1.5 s. A failure is handled like a failure to start (fail closed on hot content, notice elsewhere).
    function healthCheck(t) {
      if (detectionBroken || !booted) return;
      if (t - healthAt < (healthN < 12 ? 0 : 1500)) return;
      healthAt = t;
      healthN++;
      let ok = false;
      try {
        const r = S.classify.text("GTA 6 ending spoiler leaked " + (healthN % 1000), { hot: 0, source: "text" });
        ok = !!r && (r.level === "spoiler" || r.level === "mention");
      } catch (e) { ok = false; noteError(e); }
      if (ok) return;
      detectionBroken = true;
      fail(new Error("classifier stopped working"), "error.detect");
      checkHotContent(); // content that is already on the page is judged now
    }

    // MutationObserver unusable: rescan the document once per frame (and on a timer for hidden/throttled frames), at a
    // rate that never costs more than ~20% of a core. Not zero-flash for everything, but never silent either.
    function pollOnce() {
      const t0 = now();
      if (!booted || stopped || moMode !== "poll" || t0 < pollNext) return;
      pollChanged = false;
      try {
        checkDoc();
        if (doc.documentElement) {
          scanSubtree(doc.documentElement, true);
          runWork(t0, BUDGET_MS);
        }
      } catch (e) { stats.errors++; noteError(e); }
      pollIdle = pollChanged ? 0 : pollIdle < 400 ? pollIdle + 1 : pollIdle;
      const dt = now() - t0;
      pollNext = now() + Math.max(dt * 4, pollIdle < 8 ? 0 : pollIdle < 60 ? 40 : 200);
    }

    function startPoll() {
      if (pollTimer || stopped) return;
      pollTimer = 1;
      const frame = () => {
        if (!booted || stopped || moMode !== "poll") { pollTimer = 0; return; }
        pollOnce();
        if (RAF) { try { RAF(frame); return; } catch (e) { /* fall through */ } }
        if (T_SET) T_SET(frame, 16);
      };
      if (RAF) { try { RAF(frame); } catch (e) { /* ignore */ } }
      // a frame that never fires (hidden tab, rAF stubbed): the timer keeps the scan alive
      const keep = () => {
        if (!booted || stopped || moMode !== "poll") return;
        pollOnce();
        T_SET(keep, 0);
      };
      if (T_SET) T_SET(keep, 0);
    }

    // ---- document swap (popups / new windows) ---------------------------------------------------------------------
    // The initial about:blank document of a window.open() popup is replaced by a new Document object on the SAME Window
    // (the injected script runs once, in the old one). Observers and listeners bound to the old document are dead, so the
    // engine must follow win.document. Detection: wrappers on Document.prototype members (fires synchronously, before the
    // opener's first write/append lands) plus a short timer poll.
    function armSwapWatch() {
      if (swapWatch || stopped) return;
      let prone = false;
      try { prone = loc.protocol === "about:" && (win.opener != null || win.parent !== win); } catch (e) { prone = true; }
      if (!prone) return;
      swapWatch = true;
      try { if (S.hooks && S.hooks.watchDocument) S.hooks.watchDocument(win, () => doc, onDocSwap); } catch (e) { /* ignore */ }
      swapPollUntil = now() + 6000;
      const poll = () => {
        swapPoll = 0;
        if (stopped) return;
        try { checkDoc(); } catch (e) { /* ignore */ }
        if (now() < swapPollUntil && T_SET) swapPoll = T_SET(poll, 0);
      };
      if (T_SET) swapPoll = T_SET(poll, 0);
    }

    function onDocSwap(nd) {
      try { rebind(nd); } catch (e) { stats.errors++; noteError(e); fail(e, "error.rebind"); }
    }

    function checkDoc() {
      const d = win.document;
      if (d && d !== doc && d.nodeType === 9) rebind(d);
    }

    function rebind(nd) {
      if (stopped || !nd || nd === doc) return;
      // forget everything bound to the old document; the Window (and our closure) lives on
      try { if (mo) MO_DISCONNECT(mo); } catch (e) { /* ignore */ }
      mo = null;
      try { if (failMo) MO_DISCONNECT(failMo); } catch (e) { /* ignore */ }
      failMo = null;
      for (const f of cleanups.splice(0)) { try { f(); } catch (e) { /* ignore */ } }
      ocrShutdown();
      queue.clear(); unitQ.clear(); freshUnits.clear(); structOnly.clear(); pendingSet.clear(); threadWait.clear(); repairSet.clear(); recent.clear(); chunks.length = 0; chunkHead = 0; expandCur = null; marks.clear(); longJobs.clear();
      rootList.clear();
      doc = nd;
      booted = false;
      pageSheet = shieldSheet = quarSheet = null;
      sheetsOk = false;
      lastHref = loc.href;
      titleOriginal = null;
      titleDirty = true;
      pageCtxDirty = true;
      failShown = false;
      coverShown = false;
      boot();
      report("rebind");
    }

    // Constructable stylesheets only (never <style>: page CSP / Trusted Types could block it silently, DESIGN §5.1). When the
    // platform has none (a page removed CSSStyleSheet before us) start fails and fail() decides: page shield or notice.
    // The accessors are the ones captured at load, so a later delete of adoptedStyleSheets does not matter.
    function makeSheets() {
      if (!SHEET_CTOR || !SHEET_REPLACE || !DOC_SHEETS.doc || !DOC_SHEETS.doc.set || !DOC_SHEETS.doc.get) throw new Error("no constructable stylesheets");
      pageSheet = new SHEET_CTOR();
      SHEET_REPLACE(pageSheet, S.styles.PAGE_CSS);
      shieldSheet = new SHEET_CTOR();
      SHEET_REPLACE(shieldSheet, S.styles.SHIELD_CSS);
      if (adapter.highRisk) {
        quarSheet = new SHEET_CTOR();
        SHEET_REPLACE(quarSheet, S.styles.QUARANTINE_CSS);
      }
      adoptInto(doc);
      sheetsOk = true;
    }

    function adoptInto(target) {
      const cur = getSheets(target);
      if (!cur) return;
      const add = [];
      if (pageSheet && aIndexOf(cur, pageSheet) < 0) add[add.length] = pageSheet;
      if (quarSheet && aIndexOf(cur, quarSheet) < 0) add[add.length] = quarSheet;
      if (add.length) setSheets(target, aConcat([], cur, add));
    }

    function applyReducedEffects() {
      S.shield.configure({ reduced: cfg.reducedEffects });
      if (!pageSheet) return;
      try {
        // rule 0 is toggled; removing/inserting keeps the sheet small and avoids re-parsing PAGE_CSS
        const rules = pageSheet.cssRules;
        const first = rules.length && rules[0].cssText.indexOf("--sfw-bf") >= 0 && rules[0].cssText.indexOf("--sfw-dur") >= 0;
        if (cfg.reducedEffects && !first) { pageSheet.insertRule(S.styles.REDUCED_RULE, 0); opaqueRule = false; }
        else if (!cfg.reducedEffects && first) pageSheet.deleteRule(0);
      } catch (e) { /* ignore */ }
    }

    function setOpaque(on) {
      if (!pageSheet || cfg.reducedEffects || on === opaqueRule) return;
      try {
        if (on) pageSheet.insertRule(S.styles.OPAQUE_RULE, 0);
        else pageSheet.deleteRule(0);
        opaqueRule = on;
      } catch (e) { /* ignore */ }
    }

    function teardown() {
      // protection switched off by an explicit user action
      if (!booted) return;
      ocrShutdown();
      for (const c of Array.from(S.shield.all())) S.shield.release(c);
      S.shield.releasePage(doc);
      restoreTitle();
      try { if (mo) MO_DISCONNECT(mo); } catch (e) { /* ignore */ }
      for (const f of cleanups.splice(0)) { try { f(); } catch (e) { /* ignore */ } }
      for (const t of [doc].concat(Array.from(rootList))) {
        try { const cur = getSheets(t); if (cur) setSheets(t, aFilter(cur, (s) => s !== pageSheet && s !== quarSheet)); } catch (e) { /* ignore */ }
      }
      clearMarks();
      booted = false;
      queue.clear(); unitQ.clear(); freshUnits.clear(); structOnly.clear(); chunks.length = 0; chunkHead = 0; expandCur = null; longJobs.clear(); pendingSet.clear(); threadWait.clear();
    }

    function stop() {
      stopped = true;
      teardown();
    }

    function getStats() {
      const s = aSort(aSlice(stats.samples), (a, b) => a - b);
      return {
        batches: stats.batches, units: stats.units, shields: stats.shields, shielded: S.shield.count(), pending: pendingSet.size,
        errors: stats.errors, maxBatchMs: stats.maxBatchMs, meanBatchMs: stats.batches ? stats.totalMs / stats.batches : 0,
        p95BatchMs: s.length ? s[Math.min(s.length - 1, Math.floor(s.length * 0.95))] : 0,
        trace: stats.trace || null, slow: stats.slow || null, firstShieldAt: stats.firstShieldAt, lastError: stats.lastError || null, rafCaught: stats.rafCaught, rafChecks: stats.rafChecks, records: stats.records,
        charDataRecords: stats.charDataRecords, counterSkips: stats.counterSkips | 0, inertSkips: stats.inertSkips | 0, roots: stats.roots, repairs: stats.repairs, fallbacks: stats.fallbacks,
        pageHot, adapter: adapter && adapter.id, urlChanges: stats.urlChanges, titleGuarded: titleOriginal != null,
        pageShield: S.shield.pageState(doc), mode: cfg.mode, warm, moMode, detectionBroken, marks: marks.size, workLeft: workLeft(),
        ocr: Object.assign({}, stats.ocr, { on: lastOcrOn, live: liveRecs.size, queued: ocrQueue.size, flight: ocrFlying, cached: ocrCache.size, failed: failRecs.size }), ocrLog: stats.ocrLog || null,
      };
    }

    // ---- failure handling ----------------------------------------------------------------------------------
    // A failure to start (or a classifier that does not work on this page) is never silent: hot / high-risk pages get the
    // page shield (fail closed), every other page a small visible notice, and the host is told either way.
    function fatal(e) { fail(e, "error.start"); }

    // Hot or high-risk: a failure fails CLOSED here. When the classifier itself is what failed, hotness is judged by a crude
    // substring test (it cannot be asked) so that a broken classifier never turns an ordinary page into a page shield.

    function isProtective() {
      let protective = false;
      try { protective = !!(adapter && adapter.highRisk) || S.adapters.forLocation(loc, doc).highRisk; } catch (x) { protective = true; }
      if (detectionBroken) {
        let t = "";
        try { t = doc.title || ""; } catch (x) { /* ignore */ }
        if (crudeHot(loc.href + " " + t)) protective = true;
      } else {
        try { if (adapter && adapter.hotUrl(loc.href)) protective = true; } catch (x) { protective = true; }
      }
      if (pageHot > 0) protective = true;
      return protective;
    }

    function fail(e, code) {
      noteError(e);
      report(code || "error.start");
      if (failShown) return;
      failShown = true;
      showFailure(0);
    }

    function showFailure(tries) {
      try {
        const root = doc.documentElement;
        if (!root) {
          if (tries < 400 && T_SET) T_SET(() => showFailure(tries + 1), 25);
          return;
        }
        if (isProtective()) coverPage();
        else {
          if (sheetsOk) S.shield.notice(doc);
          else S.shield.emergencyNotice(doc, "Spoiler protection error on this page");
          // an ordinary page keeps the small notice, unless GTA VI content shows up on it while protection is down
          watchHotContent(0);
        }
      } catch (x) { /* nothing left to do */ }
    }

    function coverPage() {
      if (coverShown) return;
      coverShown = true;
      try {
        if (sheetsOk) { if (S.shield.pageState(doc) !== "shielded") S.shield.applyPage(doc, "error"); }
        else showEmergency("Spoiler Firewall could not start on this page. Content hidden to protect you from GTA VI spoilers.");
      } catch (e) { coverShown = false; }
    }

    // Visible text of the page (scripts / styles / templates excluded), capped; read with a plain depth-first walk.
    function pageText(cap) {
      const root = doc.body || doc.documentElement;
      if (!root) return "";
      const out = [];
      let len = 0;
      let n = nextNode(root, root, false);
      let visits = 0;
      while (n && len < cap && visits++ < 60000) {
        let skip = false;
        if (n.nodeType === 3) { const d = n.data; if (d && !isBlank(d)) { aPush(out, d); len += d.length; } }
        else if (n.nodeType === 1 && (SKIP.has(n.tagName) || n.localName === "sfw-shield")) skip = true;
        n = nextNode(n, root, skip);
      }
      return aJoin(out, " ");
    }

    function contentHot() {
      const t = pageText(100000);
      if (!t) return false;
      if (detectionBroken) return crudeHot(t);
      for (let i = 0; i < t.length; i += WIN_CHARS - WIN_OVERLAP) {
        try {
          const r = S.classify.text(nSlice(t, i, i + WIN_CHARS), { hot: 0, source: "text" });
          if (r && (r.level === "spoiler" || r.level === "mention" || (r.flags && (r.flags.franchise || r.flags.entity)))) return true;
        } catch (e) { return crudeHot(t); }
      }
      return false;
    }

    // Protection is down on a page that was judged cold: look again now and then; the moment it carries GTA VI content,
    // fail closed (full cover). Timer based (the observer may be exactly what is broken).
    function watchHotContent(n) {
      if (stopped || coverShown || n > 500) return;
      if (n === 0 && !failMo && MO_CTOR) {
        // an observer of its own (start may have failed before the engine's was created): content is looked at in the
        // microtask that delivers it, before it can paint
        let calls = 0;
        try {
          failMo = new MO_CTOR(() => {
            if (stopped || coverShown || (booted && !failShown) || ++calls > 3000) { try { MO_DISCONNECT(failMo); } catch (e) { /* ignore */ } failMo = null; return; }
            checkHotContent();
          });
          MO_OBSERVE(failMo, doc, { childList: true, subtree: true, characterData: true });
        } catch (e) { failMo = null; }
      }
      const tick = () => {
        if (stopped || coverShown || (booted && !failShown)) return; // recovered (rebind / config boot) or covered
        if (!checkHotContent()) watchHotContent(n + 1);
      };
      // first ~4 s: every frame, before it paints (zero-flash for static pages); afterwards on a slow timer
      if (n < 240 && RAF) { try { RAF(tick); return; } catch (e) { /* fall through */ } }
      if (T_SET) T_SET(tick, n < 280 ? 250 : 1000);
    }

    // true when it covered the page
    function checkHotContent() {
      try { if (contentHot()) { coverPage(); return true; } } catch (e) { /* ignore */ }
      return false;
    }

    function showEmergency(text) {
      if (doc.documentElement) S.shield.emergencyCover(doc, text);
    }

    // ---- shadow roots ----------------------------------------------------------------------------------------
    function rootOf(el) {
      if (!el || el.nodeType !== 1) return null;
      try {
        const r = (S.hooks && S.hooks.rootOf(el)) || el.shadowRoot;
        if (r) return r;
        if (host && host.privileged && host.privileged.openOrClosedShadowRoot && nHasAttr(el, "data-sfw-sr")) return host.privileged.openOrClosedShadowRoot(el) || null;
      } catch (e) { /* ignore */ }
      return null;
    }

    function onShadowAttached(el, root) {
      if (!booted) return;
      try { observeRoot(root, el); } catch (e) { report("error.root"); }
    }

    function discoverRoot(el) {
      const r = rootOf(el);
      if (r) observeRoot(r, el);
    }

    function observeRoot(r, hostEl) {
      if (seenRoots.has(r)) return;
      seenRoots.add(r);
      rootList.add(r);
      stats.roots++;
      try { adoptInto(r); } catch (e) { /* ignore */ }
      if (mo) MO_OBSERVE(mo, r, ctrl.opts);
      // content may already exist (declarative shadow DOM, late discovery)
      scanSubtree(r);
      if (hostEl) touch(hostEl);
      scheduleRaf();
    }

    function findExistingRoots(from) {
      const tw = doc.createTreeWalker(from, 1);
      let n = from;
      while (n) {
        if (n.nodeType === 1) discoverRoot(n);
        n = tw.nextNode();
      }
    }

    // ---- work collection ---------------------------------------------------------------------------------------
    // Pipeline (all of it budgeted, see runWork): MutationObserver records -> `chunks` (subtree roots to expand) and `queue`
    // (touched nodes) -> units (`unitQ`) -> decisions. Work that does not fit in the budget never waits uncovered: leftover
    // units get a per-unit "Checking" cover (few) or their regions get a data-sfw-defer marker (many), and the rest finishes in
    // continuation slices. Markers and pending covers are lifted only by completed work, never by a timer.
    function touch(n) {
      if (queue.size >= MAX_QUEUE) { overflow = true; return; }
      queue.add(n);
      structOnly.delete(n);
    }

    // The parent of added / removed children: the children themselves are queued as new content (chunks), so the parent's
    // own text did not change. A unit re-checked only for this, and judged clear before, needs no cover while it waits
    // (a page that keeps re-rendering would otherwise blink every container it touches).
    function touchStruct(n) {
      if (queue.has(n)) return;
      if (queue.size >= MAX_QUEUE) { overflow = true; return; }
      queue.add(n);
      structOnly.add(n);
    }

    // `cover`: the content is new to the engine, so whatever does not fit in a budget is hidden until it has been classified.
    function scanSubtree(rootNode, cover) {
      if (!rootNode) return;
      pushRoot(rootNode.nodeType === 11 ? rootNode.host || rootNode : rootNode, rootNode, cover !== false);
    }

    function pushRoot(target, node, cover) {
      const last = chunkHead < chunks.length ? chunks[chunks.length - 1] : null;
      if (last && last.single && last.target === target && last.cover === cover && last.i === 0) { aPush(last.nodes, node); return; }
      aPush(chunks, { target, nodes: [node], i: 0, cover, single: true });
    }

    function pushList(target, list, cover) {
      if (!list.length) return;
      if (list.length === 1) { pushRoot(target, list[0], cover); return; }
      aPush(chunks, { target, nodes: list, i: 0, cover, single: false });
    }

    function workLeft() {
      return queue.size > 0 || chunkHead < chunks.length || !!expandCur || unitQ.size > 0;
    }

    function elOf(n) {
      if (!n) return null;
      if (n.nodeType === 1) return n;
      return n.parentElement || (n.parentNode && n.parentNode.host) || null;
    }

    // Region covers (data-sfw-defer, see PAGE_CSS): O(1) per region, independent of how many nodes it holds.
    function markEl(el) {
      if (!el || el.nodeType !== 1 || marks.has(el)) return;
      if (marks.size >= MARK_MAX) el = doc.documentElement || el;
      if (!el || marks.has(el)) return;
      try { nSetAttr(el, "data-sfw-defer", ""); marks.add(el); } catch (e) { /* ignore */ }
    }

    function underMark(el) {
      if (!marks.size) return false;
      for (let n = el, hops = 0; n && hops < 60; hops++) {
        if (marks.has(n)) return true;
        n = parentOf(n);
      }
      return false;
    }

    function clearMarks() {
      if (!marks.size) return;
      for (const el of marks) { try { nRemoveAttr(el, "data-sfw-defer"); } catch (e) { /* ignore */ } }
      marks.clear();
    }

    // The budget ran out: everything not decided yet must stay hidden. Called once at the end of a slice.
    function coverLeft() {
      let hit = false;
      // 1. subtrees not expanded yet (new content only; rescans of content that is already visible are not hidden)
      if (expandCur && expandCur.cover) { markEl(elOf(expandCur.root)); hit = true; }
      for (let ci = chunkHead; ci < chunks.length; ci++) {
        const ch = chunks[ci];
        if (!ch.cover) continue;
        const rem = ch.nodes.length - ch.i;
        if (rem <= 0) continue;
        hit = true;
        if (rem >= GROUP_MIN && ch.target && ch.target.nodeType === 1) { markEl(ch.target); continue; }
        for (let k = ch.i; k < ch.nodes.length; k++) markEl(elOf(ch.nodes[k]));
      }
      // 2. touched nodes (attributes / character data changed) not resolved yet
      if (queue.size) {
        if (queue.size > MARK_MAX) markEl(doc.documentElement);
        else for (const n of queue) { if (!structOnly.has(n)) markEl(elOf(n)); } // a parent touched for its children: they are covered by step 1
        hit = true;
      }
      // 3. resolved units waiting for a decision: a few get their own cover, the rest are covered by region
      if (unitQ.size) {
        hit = true;
        let k = 0;
        const rest = [];
        for (const u of unitQ) {
          if (!u.isConnected || pendingSet.has(u) || underMark(u)) continue; // already hidden by a region marker: no per-slice DOM churn
          // A page root is classified on its own text only (its blocks are units of their own), yet a cover over it hides the
          // whole page: pages that keep appending to <body> flickered whole-page "Checking" covers for seconds. It is covered
          // only while text sits directly in it.
          if (isPageRoot(u)) {
            if (hasDirectText(u)) markEl(u);
            continue;
          }
          // judged clear before and re-checked only because children came or went (those are covered as new content of
          // their own): it stays visible. App shells re-render constantly; covering them blinked whole regions.
          if (!freshUnits.has(u)) { const prev = U.get(u); if (prev && prev.dec === "clear") continue; }
          if (k < PENDING_MAX) { k++; try { applyPlan({ u, dec: "pending", fp: "p", cfp: "p", hot: 0, budget: true }, [{ el: u, needPos: true }]); } catch (e) { markEl(u); } }
          else aPush(rest, u);
        }
        if (rest.length && rest.length <= UNIT_MARK_MAX) {
          for (let i = 0; i < rest.length; i++) markEl(rest[i]); // one attribute per unit: cheap, layout untouched, no overlay
        } else if (rest.length) {
          const per = new Map();
          for (let i = 0; i < rest.length; i++) { const p = parentOf(rest[i]); if (p) per.set(p, (per.get(p) || 0) + 1); }
          for (let i = 0; i < rest.length; i++) { const p = parentOf(rest[i]); if (p && p.nodeType === 1 && per.get(p) >= GROUP_MIN) markEl(p); else markEl(rest[i]); }
        }
      }
      return hit;
    }

    // Resolve a touched node to the unit that must be (re)classified, with the page / thread context hits that go with it.
    function noteNode(n) {
      const el = elOf(n);
      if (!el) return;
      if (isHeadOrTitle(el)) { slicePageCtx = true; return; }
      const ctxSel = adapter.context || "";
      const hit = ctxSel ? closestFlat(el, ctxSel) : null;
      if (hit || el.localName === "h1" || nClosest(el, "h1")) slicePageCtx = true;
      const tr = adapter.thread ? closestFlat(el, adapter.thread) : null;
      if (tr && adapter.threadActive) sliceCtxRoots.add(tr);
      const u = resolveUnitCached(el);
      if (u) {
        unitQ.add(u);
        if (noteFresh) freshUnits.add(u);
        if (sliceOcr && isMediaEl(el)) mediaVer.set(u, (mediaVer.get(u) | 0) + 1);
      }
    }

    function resolveUnitCached(el) {
      if (ucache.has(el)) return ucache.get(el);
      const u = resolveUnit(el);
      ucache.set(el, u);
      return u;
    }

    function drainTouched(t0, budget) {
      let k = 0;
      for (const n of queue) {
        queue.delete(n);
        noteFresh = !structOnly.delete(n);
        noteNode(n);
        noteFresh = true;
        if ((++k & 31) === 0 && now() - t0 > budget) return false;
      }
      return true;
    }

    // Depth-first expansion of the queued subtree roots into units, resumable at any node. Stops when the budget is used up
    // or enough units are waiting for a decision.
    function expand(t0, budget) {
      try { expandRun(t0, budget); } finally { noteFresh = true; }
    }

    function expandRun(t0, budget) {
      let steps = 0;
      for (;;) {
        if (!expandCur) {
          if (chunkHead >= chunks.length) { chunks.length = 0; chunkHead = 0; return; }
          const ch = chunks[chunkHead];
          if (ch.i >= ch.nodes.length) { chunkHead++; continue; }
          const root = ch.nodes[ch.i++];
          if (!root || !root.isConnected) continue;
          expandCur = { root, node: root, cover: ch.cover, lastParent: null, started: false };
        }
        const ec = expandCur;
        noteFresh = ec.cover !== false; // a rescan of content already on screen is not new content
        const root = ec.root;
        let cur = ec.node;
        const rootIsText = root.nodeType === 3;
        while (cur) {
          let skip = false;
          if (cur.nodeType === 3) {
            const p = cur.parentNode;
            if (p !== ec.lastParent && !isBlank(cur.data)) { noteNode(cur); ec.lastParent = p; }
          } else if (cur.nodeType === 1) {
            if (cur.localName === "sfw-shield" || SKIP.has(cur.tagName)) skip = true;
            else discoverShadowAndTouch(cur);
          }
          cur = rootIsText ? null : nextNode(cur, root, skip);
          if (cur && (++steps & 63) === 0 && (now() - t0 > budget || unitQ.size >= EXPAND_AHEAD)) { ec.node = cur; return; }
        }
        expandCur = null;
        if (unitQ.size >= EXPAND_AHEAD || now() - t0 > budget) return;
      }
    }

    function discoverShadowAndTouch(el) {
      const r = (S.hooks && S.hooks.rootOf(el)) || el.shadowRoot || (el.hasAttributes() && nHasAttr(el, "data-sfw-sr") ? rootOf(el) : null);
      if (r) observeRoot(r, el);
      const tag = el.tagName;
      if (MEDIA_TAGS.has(tag) || tag === "IFRAME") { noteNode(el); return; }
      if (!el.hasAttributes()) return;
      if (nHasAttr(el, "alt") || nHasAttr(el, "title") || nHasAttr(el, "aria-label") || nHasAttr(el, "placeholder") || nHasAttr(el, "poster") || nHasAttr(el, "data-sfw-sr")) noteNode(el);
      else if (el.localName === "a" && nHasAttr(el, "href")) noteNode(el);
      else if (nHasAttr(el, "style") && sliceOcr && /background-image/i.test(nGetAttr(el, "style"))) noteNode(el);
    }

    // Inert elements: no child nodes, no attribute extract() reads, not media / a frame / a form field / a custom element,
    // no shadow root. Adding or removing one cannot change any unit's text, media or frames, yet games and animations
    // add and remove hundreds a second (bullets, particles, ripples, cursors), and each one re-read the whole parent
    // container twice (callback + frame). A removal counts as inert only for an element that was inert when it arrived
    // and has had no record since (`inertSeen`): the removed node's state now says nothing about what it held while
    // attached (a page could remove a text-bearing element and empty it in the same task).
    function inert(n) {
      if (!n || n.nodeType !== 1 || n.firstChild) return false;
      const ln = n.localName;
      if (!ln || ln.indexOf("-") > 0 || INERT_DENY.has(ln) || MEDIA_TAGS.has(n.tagName)) return false;
      if (n.namespaceURI && n.namespaceURI !== HTML_NS) return false; // svg / math: images and text of their own
      if (rootOf(n)) return false;
      if (n.hasAttributes()) {
        for (let i = 0; i < INERT_ATTR_DENY.length; i++) if (nHasAttr(n, INERT_ATTR_DENY[i])) return false;
        if (nHasAttr(n, "style") && /background|content|url\(/i.test(nGetAttr(n, "style") || "")) return false;
        if (!adapter) return false;
        if (adapter.media && nMatches(n, adapter.media)) return false;
        if (adapter.own && nMatches(n, adapter.own)) return false;
        if (adapter.attrs) for (let i = 0; i < adapter.attrs.length; i++) if (nHasAttr(n, adapter.attrs[i])) return false;
      }
      return true;
    }
    function inertList(list, added) {
      for (let k = 0; k < list.length; k++) {
        const n = list[k];
        if (added) { if (!inert(n)) return false; inertSeen.add(n); }
        else if (!inertSeen.has(n) || !inert(n)) return false;
      }
      return true;
    }

    function handleRecords(records) {
      stats.records += records.length;
      for (let i = 0; i < records.length; i++) {
        const r = records[i];
        const t = r.target;
        inertSeen.delete(t); // anything recorded on an element ends its inert status for good
        if (r.type === "attributes") {
          const a = r.attributeName;
          if (a === "data-sfw") { repairSet.add(t); continue; }
          if (a === "data-sfw-sr") { discoverRoot(t); touch(t); continue; }
          if (a !== "href" && a !== "src" && a !== "srcset" && a !== "poster" && a !== "srcdoc" && a !== "sandbox" && counterTick(r.oldValue, nGetAttr(t, a), "")) { stats.counterSkips = (stats.counterSkips | 0) + 1; continue; }
          touch(t);
        } else if (r.type === "characterData") {
          stats.charDataRecords++;
          if (!titleDirty && t.parentNode && t.parentNode.nodeName === "TITLE") titleDirty = true;
          if (counterTick(r.oldValue, t.data, nearText(t))) { stats.counterSkips = (stats.counterSkips | 0) + 1; continue; }
          touch(t);
        } else {
          const rem = r.removedNodes;
          const add0 = r.addedNodes;
          // a text node swapped for another that differs only in its numbers (textContent = "13 Likes")
          if (rem.length === 1 && add0.length === 1 && rem[0].nodeType === 3 && add0[0].nodeType === 3 && t.nodeType === 1 && t.nodeName !== "TITLE" && counterTick(rem[0].data, add0[0].data, nearText(add0[0]))) { stats.counterSkips = (stats.counterSkips | 0) + 1; continue; }
          // only inert elements came or went (see inert()): nothing any unit says has changed
          if ((t.nodeType === 1 || (t.nodeType === 11 && t.host)) && t !== doc.head && t.nodeName !== "TITLE" && (rem.length || add0.length) && inertList(add0, true) && inertList(rem, false)) { stats.inertSkips = (stats.inertSkips | 0) + 1; continue; }
          for (let k = 0; k < rem.length; k++) {
            if (rem[k].localName === "sfw-shield") repairSet.add(t.nodeType === 11 ? t.host : t);
          }
          const add = r.addedNodes;
          addedBurst += add.length;
          if (add.length) pushList(t.nodeType === 11 && t.host ? t.host : t, add, true);
          if (t.nodeType === 9) titleDirty = true;
          if (t.nodeType === 1) { touchStruct(t); if (t.nodeName === "TITLE" || t === doc.head) titleDirty = true; }
          else if (t.nodeType === 11 && t.host) touchStruct(t.host);
        }
      }
    }

    // the text around a changed text node (its parent and grandparent, capped): what a number could combine with
    function nearText(n) {
      try {
        const p = n.parentNode;
        const g = p && p.parentNode && p.parentNode.nodeType === 1 ? p.parentNode : p;
        const t = g && g.textContent;
        return t ? (t.length > 400 ? nSlice(t, 0, 400) : t) : "";
      } catch (e) { return "gta"; } // unreadable: never skip
    }

    // ---- MutationObserver entry ------------------------------------------------------------------------------
    function onMutations(records) {
      const t0 = now();
      let heavy = false;
      try {
        if (!booted) return;
        checkUrl();
        checkLive();
        addedBurst = 0;
        handleRecords(records);
        heavy = addedBurst > BIG_INSERT;
        if (heavy) rafWatchUntil = now() + RAF_WATCH_MS;
        if (failShown && !coverShown && !isProtective()) checkHotContent(); // protection is down on a page judged cold: look at what just arrived
        if (stats.trace) stats.trace.push("BATCH " + records.map((r) => r.type[0] + ":" + r.target.nodeName + ":" + (r.addedNodes ? r.addedNodes.length : 0) + (r.attributeName ? ":" + r.attributeName : "")).join(",") + " rec=" + records.length + " q=" + queue.size + " rs=" + doc.readyState);
        // a huge insertion: cover it (region marker, O(1)) and leave the classification to continuation slices, so the callback
        // itself stays tiny and the page's own next task is not queued behind the engine
        runWork(t0, heavy ? 2 : BUDGET_MS);
      } catch (e) {
        stats.errors++;
        noteError(e);
        report("error.batch");
        try { protectQueue(); } catch (x) { /* ignore */ }
      }
      try { if (mo) MO_TAKE(mo); } catch (e) { /* ignore */ }
      noteBatch(now() - t0);
      if (heavy && workLeft()) scheduleWork(3);
      schedule();
    }

    function noteBatch(dt) {
      stats.batches++;
      stats.totalMs += dt;
      if (dt > stats.maxBatchMs) stats.maxBatchMs = dt;
      if (stats.samples.length < 4000) stats.samples.push(dt);
    }

    function flushPending() {
      try {
        if (!mo) return;
        const pend = MO_TAKE(mo);
        if (pend.length) handleRecords(pend);
      } catch (e) { /* ignore */ }
    }

    function protectQueue() {
      // an exception escaped the batch: every queued unit ends in an error shield, never in a clear state
      const todo = new Set();
      for (const n of queue) { const el = elOf(n); const u = el && resolveUnit(el); if (u) todo.add(u); }
      for (const u of unitQ) todo.add(u);
      for (const u of todo) {
        if (detectionBroken && !isProtective()) continue;
        try { applyPlan({ u, dec: "error", fp: "e", cfp: "e", hot: 0 }, null); } catch (e) { /* ignore */ }
      }
      queue.clear(); structOnly.clear();
      unitQ.clear();
      try { coverLeft(); } catch (e) { /* ignore */ }
    }

    function schedule() {
      if (recent.size) scheduleRaf();
      if (workLeft()) scheduleWork();
    }

    function scheduleRaf() {
      if (rafId || !booted || !RAF) return;
      rafId = RAF(onFrame);
    }

    // Continuation slices: a plain timer task, so the page's own tasks (input, timers, rendering) interleave with the engine's
    // remaining work instead of waiting behind it.
    function scheduleWork(delay) {
      if (workTimer || !booted || !T_SET) return;
      workTimer = T_SET(slice, delay || 0);
    }

    function slice() {
      workTimer = 0;
      if (!booted) return;
      const t0 = now();
      try {
        flushPending();
        runWork(t0, SLICE_MS);
      } catch (e) { stats.errors++; noteError(e); report("error.drain"); try { protectQueue(); } catch (x) { /* ignore */ } }
      try { if (mo) MO_TAKE(mo); } catch (e) { /* ignore */ }
      noteBatch(now() - t0);
      if (workLeft()) scheduleWork();
    }

    // rAF safety net (DESIGN §10): not a rescan. Re-checks (a) units processed since the last frame, in case a
    // parser append grew a text node without queueing a record, and (b) shadow roots attached by the streaming
    // parser without any record (declarative shadow DOM).
    function onFrame() {
      rafId = 0;
      if (!booted) return;
      const t0 = now();
      try {
        flushPending();
        if (doc.readyState === "loading") parserPathScan();
        const list = Array.from(recent);
        recent.clear();
        stats.rafChecks += list.length;
        const units = [];
        for (const u of list) {
          if (!u.isConnected) continue;
          const c = U.get(u);
          if (!c) continue;
          aPush(units, u);
          if (u.hasChildNodes()) discoverShadowIn(u);
        }
        sliceDeadline = t0 + BUDGET_MS;
        ucache = new Map(); tlcache = new Map();
        const r = evaluate(units, t0, BUDGET_MS, true);
        stats.rafCaught += r.changed;
        for (const u of r.left) unitQ.add(u);
        checkUrl();
        checkLive();
        if (workLeft()) runWork(t0, BUDGET_MS);
      } catch (e) { stats.errors++; noteError(e); report("error.raf"); }
      try { if (mo) MO_TAKE(mo); } catch (e) { /* ignore */ }
      if (doc.readyState === "loading" || recent.size) scheduleRaf();
      if (workLeft()) scheduleWork();
    }

    // Declarative shadow roots are attached by the streaming parser to an element that is still OPEN (on the parser's
    // stack), without any mutation record. Open elements are exactly the last-child chain from <html>: check each
    // one every frame while the document is loading.
    function parserPathScan() {
      let n = doc.documentElement;
      for (let i = 0; n && i < 64; i++) {
        const r = rootOf(n);
        if (r) observeRoot(r, n);
        n = n.lastElementChild;
      }
    }

    function discoverShadowIn(u) {
      // streamed declarative shadow roots appear without records: look at the unit's last descendants
      const r = rootOf(u);
      if (r) observeRoot(r, u);
      let n = u.lastElementChild;
      for (let i = 0; n && i < 8; i++) { const rr = rootOf(n); if (rr) observeRoot(rr, n); n = n.lastElementChild; }
    }

    // ---- processing --------------------------------------------------------------------------------------------
    // One budgeted slice of work. Returns nothing; leaves whatever did not fit covered and scheduled.
    function runWork(t0, budget) {
      if (!booted) return;
      healthCheck(t0);
      sliceDeadline = t0 + budget;
      if (sheetsOk) { try { adoptInto(doc); } catch (e) { /* ignore */ } } // a page that replaced document.adoptedStyleSheets must not drop our sheets
      doRepairs();
      if (overflow) {
        overflow = false;
        queue.clear(); structOnly.clear();
        chunks.length = 0; chunkHead = 0; expandCur = null;
        scanSubtree(doc.documentElement, true);
      }
      ucache = new Map();
      tlcache = new Map();
      slicePageCtx = false;
      sliceCtxRoots.clear();
      sliceOcr = ocrOn();
      let guard = 0;
      const ph = stats.slow ? { touched: 0, expand: 0, ctx: 0, decide: 0, cover: 0 } : null;
      let tp = t0;
      const lap = (k) => { if (ph) { const t = now(); ph[k] += t - tp; tp = t; } };
      while (workLeft() && guard++ < 1000) {
        drainTouched(t0, budget); lap("touched");
        expand(t0, budget); lap("expand");
        applyContext(); lap("ctx");
        if (unitQ.size) decideUnits(t0, budget);
        lap("decide");
        if (now() - t0 > budget) break;
      }
      if (titleDirty) { titleDirty = false; evalPage(); }
      lap("ctx");
      if (workLeft()) coverLeft();
      else {
        clearMarks();
        if (!pendingSet.size) longJobs.clear();
      }
      lap("cover");
      if (ph) { const tot = now() - t0; if (tot > 40) stats.slow.push({ tot: Math.round(tot), ph: JSON.stringify(ph), q: queue.size, ch: chunks.length - chunkHead, uq: unitQ.size, marks: marks.size }); }
    }

    // Page / thread context changes discovered while resolving units: re-evaluate everything they influence.
    function applyContext() {
      if (slicePageCtx) { slicePageCtx = false; pageCtxDirty = true; }
      if (pageCtxDirty) {
        pageCtxDirty = false;
        if (recomputePageHot()) { rescanAll(false); }
      }
      if (sliceCtxRoots.size) {
        const roots = Array.from(sliceCtxRoots);
        sliceCtxRoots.clear();
        for (const r of roots) {
          if (refreshThread(r)) {
            // thread context changed: everything inside is re-evaluated, waiters resume
            const w = threadWait.get(r);
            if (w) { for (const u of w) unitQ.add(u); threadWait.delete(r); }
            rescanInside(r, unitQ);
          }
        }
      }
    }

    function decideUnits(t0, budget) {
      while (unitQ.size) {
        if (now() - t0 > budget) return;
        const batch = [];
        for (const u of unitQ) { aPush(batch, u); unitQ.delete(u); if (batch.length >= EVAL_BATCH) break; }
        const r = evaluate(batch, t0, budget);
        if (r.left.length) { for (const u of r.left) unitQ.add(u); return; }
      }
    }

    function isHeadOrTitle(el) {
      return el.localName === "title" || el === doc.head || (doc.head && doc.head.contains(el));
    }

    function rescanInside(r, units) {
      let n = r.firstChild;
      let count = 0;
      while (n && count < 5000) {
        if (n.nodeType === 1 && (n.localName === "sfw-shield" || SKIP.has(n.tagName))) { n = nextNode(n, r, true); continue; }
        const el = n.nodeType === 1 ? n : n.parentElement;
        if (el) { const u = resolveUnitCached(el); if (u) units.add(u); }
        n = nextNode(n, r, false);
        count++;
      }
    }

    // Re-evaluate everything already on the page (config change, page became hot, URL change). Content that is already
    // visible is not hidden while it waits for its turn (cover = false).
    function rescanAll(run) {
      if (!booted || !doc.documentElement) return;
      // new content that was still waiting for its first look keeps its cover: the rescan that replaces it is covered too
      let hadCover = !!(expandCur && expandCur.cover) || queue.size > 0;
      for (let ci = chunkHead; ci < chunks.length && !hadCover; ci++) if (chunks[ci].cover && chunks[ci].i < chunks[ci].nodes.length) hadCover = true;
      queue.clear(); structOnly.clear();
      chunks.length = 0; chunkHead = 0; expandCur = null;
      scanSubtree(doc.documentElement, hadCover);
      for (const r of rootList) { if (r.host && r.host.isConnected) scanSubtree(r, hadCover); }
      if (run !== false) { flushPending(); runWork(now(), BUDGET_MS); try { if (mo) MO_TAKE(mo); } catch (e) { /* ignore */ } schedule(); }
    }

    function doRepairs() {
      for (const c of repairSet) {
        if (!c || c.nodeType !== 1) continue;
        const before = S.shield.state(c);
        if (before.mode === "none") continue;
        const r = S.shield.repair(c);
        stats.repairs++;
        if (r === "fallback") stats.fallbacks++;
      }
      repairSet.clear();
    }

    // ---- unit resolution ---------------------------------------------------------------------------------------
    function parentOf(n) {
      const p = n.parentNode;
      if (!p) return null;
      return p.nodeType === 11 ? p.host || null : p;
    }

    function closestFlat(el, sel) {
      let n = el;
      for (let hops = 0; n && hops < 40; hops++) {
        if (n.nodeType === 1) {
          const c = nClosest(n, sel);
          if (c) return c;
        }
        const r = nGetRoot(n);
        n = r && r.nodeType === 11 ? r.host : null;
      }
      return null;
    }

    function imageDoc() {
      try { return /^image\//i.test(doc.contentType || ""); } catch (e) { return false; }
    }

    function isPageRoot(el) {
      return !!el && (el.tagName === "BODY" || el.tagName === "HTML");
    }

    function isSearchField(e) {
      try {
        if (e.type === "search" || nGetAttr(e, "role") === "searchbox") return true;
        if (SEARCH_NAMES.has(String(nGetAttr(e, "name") || "").toLowerCase())) return true;
        if (adapter.context && nMatches(e, adapter.context)) return true;
        return !!nClosest(e, "[role=search],form[action*=search],form[action*=results]");
      } catch (x) { return false; }
    }

    // The site's own search box: what the user typed (and the site echoes back inside the box) is never masked. Suggestion
    // rows are content and stay judged (they sit in a listbox, often inside the same component).
    function smallSearchRoot(r) {
      return !!r && textLen(r, SEARCH_UI_TEXT_MAX) <= SEARCH_UI_TEXT_MAX;
    }
    function isSearchUi(el) {
      try { return smallSearchRoot(closestFlat(el, SEARCH_UI)) && !closestFlat(el, "[role=listbox],[role=option]"); } catch (e) { return false; }
    }
    function maybeSearchUiRoot(el) {
      const tg = el.tagName;
      return tg === "FORM" || tg === "YT-SEARCHBOX" || tg === "YTD-SEARCHBOX" || (el.hasAttribute("role") && nGetAttr(el, "role") === "search");
    }

    // Page chrome: what is not content. On a site with an adapter, content is its items (videos, posts, comments ...) and
    // generic list / article items; everything else (app shell, navigation, toolbars) is chrome. Anywhere, a large container
    // is chrome for its own text (its blocks are units of their own).
    function isChrome(u, deep) {
      if (isOwn(u) || imageDoc()) return false;
      if (!deep) return true;
      if (!adapter.own) return false;
      return !(adapter.generic && nMatches(u, adapter.generic));
    }

    // Non-blank text nodes straight under el (bounded): what a page-root unit actually holds of its own.
    function hasDirectText(el) {
      let n = el.firstChild;
      for (let i = 0; n && i < 2000; i++, n = n.nextSibling) if (n.nodeType === 3 && !isBlank(n.data)) return true;
      return !!n; // too many children to look at: assume text
    }

    function blockOf(el) {
      let n = el;
      while (n && n.nodeType === 1) {
        if (isBlockTag(n.tagName)) return n;
        n = parentOf(n);
      }
      return null;
    }

    // Resolve a touched element to the unit that is classified: adapter item (innermost) -> generic item that is
    // small enough -> nearest block. Returns null for nodes that are never content (our overlay, head, ...).
    function resolveUnit(el) {
      if (!el || el.nodeType !== 1) return null;
      if (el.localName === "sfw-shield") return null;
      if (isHeadOrTitle(el)) return null;
      if (adapter.own) {
        const own = closestFlat(el, adapter.own);
        if (own) return own;
      }
      const b = blockOf(el);
      if (!b) return null;
      // an element straight under <body> / <html> (an iframe, an inline span ...) is a unit of its own: the page root is
      // a unit only for text sitting directly in it, so one element can never hide the whole page
      if (isPageRoot(b) && el !== b && !imageDoc()) { // an image document (i.imgur.com/x.png) keeps <body> as its unit
        let top = el;
        for (let p = parentOf(top); p && p !== b && p.nodeType === 1; p = parentOf(top)) top = p;
        return SKIP.has(top.tagName) ? null : top;
      }
      if (adapter.generic) {
        const g = closestFlat(b, adapter.generic);
        if (g && useGenericItem(g)) return g;
      }
      return b;
    }

    // A generic item (li / article / ...) becomes the unit only when it is small AND behaves like a card: list-ish tags,
    // containers with media (so a shielded caption also hides its picture), or containers with at most two children.
    // A long article made of many paragraphs is shielded per paragraph instead.
    function useGenericItem(g) {
      if (g.tagName === "BODY" || g.tagName === "HTML") return false;
      if (textLen(g, ITEM_TEXT_CAP) > ITEM_TEXT_CAP) return false;
      if (LISTISH.has(g.tagName) || nGetAttr(g, "role") === "listitem" || nGetAttr(g, "role") === "option") return true;
      if (g.children.length <= 2) return true;
      try { if (nQS(g, "img,video,picture,canvas,svg image")) return true; } catch (e) { /* ignore */ }
      return hasBackgroundMedia(g);
    }

    // A picture drawn as a CSS background (common on social sites) is media too: an inline style anywhere in the item, or a
    // computed background-image on one of its first direct children (bounded, and memoised per child count).
    function hasBackgroundMedia(g) {
      try { if (nQS(g, "[style*='background-image'],[style*='background:']")) return true; } catch (e) { /* ignore */ }
      if (!nGetCS) return false;
      const kids = g.children;
      const memo = bgMemo.get(g);
      if (memo && memo.n === kids.length) return memo.v;
      let v = false;
      for (let i = 0; i < kids.length && i < BG_CHILDREN && !v; i++) {
        try { const bi = nGetCS(kids[i]).backgroundImage; v = !!bi && bi !== "none"; } catch (e) { /* ignore */ }
      }
      bgMemo.set(g, { n: kids.length, v });
      return v;
    }

    // Escalation: a block that is shielded as a spoiler inside a small generic card (article / li / figure / blockquote /
    // listitem within ITEM_TEXT_CAP) covers the whole card, so its siblings (pictures, links, embeds, captions) are covered
    // too. Done in the same evaluate() pass (the ladder rounds), so there is no frame in between. A long article made of many
    // paragraphs is above the cap and stays shielded per paragraph.
    function escalationTarget(u) {
      if (!adapter.generic || isOwn(u)) return null;
      const p = parentOf(u);
      if (!p || p.nodeType !== 1) return null;
      const g = closestFlat(p, ESCALATE_SEL);
      if (!g || g === u || g.tagName === "BODY" || g.tagName === "HTML" || nHasAttr(g, "data-sfw")) return null;
      if (!coverWhole(g)) return null;
      return g;
    }
    // A small item: few element children and little text. A long article is never covered whole, also not while it is
    // still streaming in (it grows past these limits and the escalation is dropped).
    function smallItem(g) {
      return g.children.length <= ESC_MAX_CHILDREN && textLen(g, ITEM_TEXT_CAP) <= ITEM_TEXT_CAP;
    }
    // Something that is not a text block (a picture, video, embed, CSS background picture or a direct link): what a shielded
    // block cannot cover on its own. An article made only of paragraphs keeps deciding paragraph by paragraph.
    function holdsNonText(g) {
      try { if (nQS(g, NONTEXT_SEL) || nQS(g, ":scope > a[href]")) return true; } catch (e) { /* ignore */ }
      return hasBackgroundMedia(g);
    }
    // Cover the whole item: while the document is still loading, every small item holding a shielded block is covered (its
    // pictures may not have been parsed yet, and zero-flash comes first); once loaded, only one that really holds something
    // besides text blocks. onReadyState() re-decides the escalated items when loading ends, which releases the others.
    function coverWhole(g) {
      return smallItem(g) && (doc.readyState !== "complete" || holdsNonText(g));
    }
    function hasInnerShield(u) {
      try { return !!nQS(u, "[data-sfw~='shield']:not(sfw-shield)"); } catch (e) { return false; }
    }

    // Text length with early exit: never walks more than ~cap characters of a huge ancestor (memoised for the slice).
    function textLen(e, cap) {
      cap = cap || 1e9;
      const hit = tlcache.get(e);
      if (hit && (hit.n <= hit.cap || cap <= hit.cap)) return hit.n;
      let n = 0;
      const c = e.firstChild;
      if (c && !c.nextSibling && c.nodeType === 3) n = c.data.length; // the common leaf: one text node
      else {
        let t = nextNode(e, e, false);
        let visits = 0;
        while (t && n <= cap && visits++ < 4000) {
          if (t.nodeType === 3) n += t.data.length;
          t = nextNode(t, e, false);
        }
        if (t && n <= cap) n = cap + 1; // walked the node limit without reaching the end: treat as large
      }
      tlcache.set(e, { n, cap });
      return n;
    }

    // Deep units (adapter items, small blocks) are classified on their whole subtree; large blocks only on their
    // own text plus inline children, because nested blocks are units of their own.
    function isDeep(u) {
      if (isOwn(u)) return true;
      if (u.tagName === "BODY" || u.tagName === "HTML") return false;
      return textLen(u, ITEM_TEXT_CAP) <= ITEM_TEXT_CAP;
    }

    function isOwn(u) {
      return !!(adapter.own && nMatches(u, adapter.own));
    }

    // ---- text extraction ---------------------------------------------------------------------------------------
    function extract(u, own, deep) {
      const parts = [];
      const keys = []; // block key per part (text nodes only; others get a unique key)
      const bk = new Map();
      const blockKey = (el) => {
        let n = el;
        const trail = [];
        let k;
        while (n && n.nodeType === 1) {
          if (bk.has(n)) { k = bk.get(n); break; }
          aPush(trail, n);
          if (isBlockTag(n.tagName) || n.localName === "br") { k = n; break; }
          n = parentOf(n);
        }
        if (!k) k = u;
        for (const t of trail) bk.set(t, k);
        return k;
      };
      const lead = []; // whitespace-only text node seen right before part i (kept for the tight view)
      let wsBefore = false;
      let len = 0;
      let media = false;
      let frames = null; // embedded frames the engine cannot look into (sandboxed without allow-scripts)
      let sig = "";
      const mediaSel = adapter.media || "";
      const noMediaSel = adapter.mediaIgnore || ""; // profile pictures / emoji images: present in every post, not a picture post
      const ownSel = adapter.own || ""; // nested adapter items are units of their own (never part of a container's text)
      const prev = U.get(u);
      const mine = prev && prev.targets && prev.targets.length ? mkSet(prev.targets) : null;
      const custom = adapter.attrs && adapter.attrs.length ? adapter.attrs : null;
      const wantBg = ocrOn(); // OCR gates inline background images too; without a host.ocr they behave exactly as in v1
      const add = (v) => { aPush(parts, v); aPush(keys, null); len += v.length; };
      const walkRoot = (rootNode, depth) => {
        const n = rootNode.nodeType === 1 ? rootNode : null;
        const handleEl = (e) => {
          const tag = e.localName;
          if (!media && (MEDIA_TAGS.has(e.tagName) || tag === "image" || (mediaSel && nMatches(e, mediaSel))) && !(noMediaSel && nMatches(e, noMediaSel))) media = true;
          if (e.hasAttributes()) {
            if (!media && wantBg && nHasAttr(e, "style") && /background-image/i.test(nGetAttr(e, "style")) && !(noMediaSel && nMatches(e, noMediaSel))) media = true;
            for (let i = 0; i < ATTR_TEXT.length; i++) {
              const v = nGetAttr(e, ATTR_TEXT[i]);
              if (v) add(v);
            }
            if (tag === "a") { const h = nGetAttr(e, "href"); if (h) { const s = slugText(h); if (s) add(s); } }
            else if (tag === "img" || tag === "source") {
              const s = srcWords(nGetAttr(e, "src") || nGetAttr(e, "data-src") || ""); if (s) add(s);
            } else if (tag === "video") {
              const p = srcWords(nGetAttr(e, "poster") || ""); if (p) add(p);
            } else if (tag === "iframe") {
              const fi = frameInfo(e);
              for (let i = 0; i < fi.texts.length; i++) add(fi.texts[i]);
              if (fi.sig) sig += fi.sig + "|";
              if (fi.opaque && !fi.readable) { media = true; (frames || (frames = [])).push(e); }
            }
            if (custom && tag.indexOf("-") > 0) {
              for (let i = 0; i < custom.length; i++) {
                const v = nGetAttr(e, custom[i]);
                if (v) add(custom[i] === "permalink" || custom[i] === "content-href" ? slugText(v) : v);
              }
            }
          } else if (tag === "iframe") {
            // no attributes at all: an unsandboxed, empty frame; nothing to read
          }
          // the user's own query in a search box is not content: it still makes the page hot (context), but it must never
          // mask the search bar itself, or a search for GTA VI leaves no way to search for something else
          if ((tag === "input" || tag === "textarea") && e.value && e.type !== "password" && !isSearchField(e)) add(e.value);
          if (depth < 5) {
            const sr = rootOf(e);
            if (sr && e !== rootNode) walkRoot(sr, depth + 1);
          }
        };
        if (n) handleEl(n);
        let visits = 0;
        let cur = nextNode(rootNode, rootNode, false);
        while (cur && len < MAX_TEXT && visits++ < VISIT_MAX) {
          if (cur.nodeType === 3) {
            const d = cur.data;
            if (d) {
              if (isBlank(d)) wsBefore = true;
              else {
                lead[parts.length] = wsBefore; wsBefore = false;
                const bkey = blockKey(cur.parentNode.nodeType === 1 ? cur.parentNode : parentOf(cur));
                if (d.length > MAX_TEXT) {
                  // one giant text node: its head and its tail (windowed classification covers each), never the whole thing
                  const half = MAX_TEXT >> 1;
                  aPush(parts, nSlice(d, 0, half)); aPush(keys, bkey);
                  aPush(parts, nSlice(d, d.length - half)); aPush(keys, null);
                  len += MAX_TEXT;
                } else { aPush(parts, d); aPush(keys, bkey); len += d.length; }
              }
            }
            cur = nextNode(cur, rootNode, false);
          } else if (cur.nodeType === 1) {
            let skip = false;
            const tg = cur.tagName;
            if (SKIP.has(tg)) skip = true;
            else if (cur !== u) {
              if (nHasAttr(cur, "data-sfw") && !(mine && mine.has(cur))) skip = true; // nested shielded / revealed unit (not our own targets)
              else if (ownSel && nMatches(cur, ownSel)) skip = true; // nested unit: classified on its own
              else if (maybeSearchUiRoot(cur) && nMatches(cur, SEARCH_UI) && smallSearchRoot(cur)) skip = true; // the search box: the user's own query
              else if (!deep && isBlockTag(tg)) skip = true;
            }
            if (!skip) handleEl(cur);
            cur = nextNode(cur, rootNode, skip);
          } else cur = nextNode(cur, rootNode, false);
        }
        // the unit itself may be a shadow host whose content lives in its root
        if (n && depth < 5) { const sr = rootOf(n); if (sr && sr !== rootNode) walkRoot(sr, depth + 1); }
      };
      walkRoot(u, 0);
      // \`text\` joins every text node with a space (never raw textContent concatenation); \`tight\` joins nodes that share a
      // block with nothing, so a word split by inline markup ("G<b>T</b>A") is still seen whole.
      let tight = parts.length > 1 ? parts[0] : null;
      if (tight != null) { for (let i = 1; i < parts.length; i++) tight += (keys[i] && keys[i] === keys[i - 1] ? (lead[i] ? " " : "") : " ") + parts[i]; }
      const text = aJoin(parts, " ");
      return { text, tight: tight !== text ? tight : null, media, frames, sig };
    }

    // ---- embedded frames ------------------------------------------------------------------------------------------
    // An iframe whose sandbox lacks allow-scripts cannot run this engine (injected scripts do not run there), and a parent
    // cannot read an opaque-origin document. What the parent CAN read is the srcdoc attribute (or a data: URL), the title /
    // name / aria attributes and the URL words; those are classified as part of the unit. A frame the engine cannot look into
    // at all (src only) is "unverifiable": see frameVerdict().
    function frameInfo(e) {
      const texts = [];
      const sb = nGetAttr(e, "sandbox");
      const opaque = sb !== null && !hasToken(sb, "allow-scripts");
      let sig = "";
      let readable = false; // the frame's whole text is in the parent's hands (srcdoc / data:text/html), so it is judged, not "unverifiable"
      const srcdoc = nGetAttr(e, "srcdoc");
      if (srcdoc !== null) {
        readable = true;
        const t = srcdocText(srcdoc);
        if (t) aPush(texts, t);
        sig = "d" + fingerprint(srcdoc);
      } else {
        const src = nGetAttr(e, "src");
        if (src) {
          sig = "s" + fingerprint(src);
          const dt = dataUrlHtml(src);
          if (dt != null) { readable = true; const t = srcdocText(dt); if (t) aPush(texts, t); }
          else { const sl = slugText(src); if (sl) aPush(texts, sl); }
        }
      }
      const nm = nGetAttr(e, "name");
      if (nm) aPush(texts, nm);
      return { texts, opaque, readable, sig: (opaque ? "o" : "") + sig };
    }



    // Layout-reading verdict for the unverifiable frames of one unit (the only synchronous layout read the frame rule makes).
    // Only frames big enough to show text count. strict: any such frame; lockdown: every such frame; balanced: hot context only. Returns "shield" | "clear" | "cfg" (decision differs between modes while the config is loading).
    function frameVerdict(frames, hot) {
      const big = () => {
        for (const f of frames) {
          try { const r = nGBCR(f); if (r.width >= FRAME_MIN_W && r.height >= FRAME_MIN_H) return true; } catch (e) { return true; }
        }
        return false;
      };
      // a frame too small to show a line of text (tracking / ad / messaging frames are 0x0 or hidden) cannot leak anything:
      // covering its block hid whole pages (a hidden about:blank frame straight under YouTube's <body>)
      if (!big()) return "clear";
      const per = (mode) => {
        if (mode === "lockdown") return "shield";
        if (hot >= 1) return "shield";
        return mode === "balanced" ? "clear" : "shield";
      };
      if (!configPending) return per(cfg.mode);
      const v = [per("lockdown"), per("strict"), per("balanced")];
      return v[0] === v[1] && v[1] === v[2] ? v[0] : "cfg";
    }



    // ---- context ----------------------------------------------------------------------------------------------
    function contextText(els, limit) {
      const out = [];
      let n = 0;
      for (const e of els) {
        if (n++ >= limit) break;
        if (e.value && typeof e.value === "string") out.push(e.value);
        const t = e.textContent;
        if (t) out.push(t.length > 600 ? t.slice(0, 600) : t);
        if (adapter.attrs) for (const a of adapter.attrs) { const v = e.getAttribute && nGetAttr(e, a); if (v) out.push(v); }
      }
      return out.join(" ");
    }

    function recomputePageHot() {
      const href = loc.href;
      let hot = 0;
      try { hot = adapter.hotUrl(href) | 0; } catch (e) { hot = 0; }
      let ctx = "";
      try {
        if (doc.title) ctx += doc.title + " ";
        const og = doc.head && nQS(doc.head, "meta[property='og:title'],meta[name='description'],meta[property='og:description']");
        if (og) ctx += nGetAttr(og, "content") + " ";
        const h1 = doc.querySelectorAll("h1");
        ctx += contextText(Array.prototype.slice.call(h1, 0, 3), 3) + " ";
        if (adapter.context) {
          const els = Array.prototype.filter.call(doc.querySelectorAll(adapter.context), (e) => !(adapter.own && closestFlat(e, adapter.own)) && !(adapter.threadActive && adapter.thread && closestFlat(e, adapter.thread)));
          ctx += contextText(els, 6);
        }
      } catch (e) { /* ignore */ }
      if (ctx.trim() && S.classify.isHotText(ctx)) hot = 2;
      if (pageWant) hot = 2; // the URL or title alone is a spoiler / mention (evalPage)
      const changed = hot !== pageHot;
      pageHot = hot;
      return changed;
    }

    function recomputeUrlContext() {
      adapter = S.adapters.forLocation(loc, doc);
      adapter.threadActive = threadActiveFor(adapter);
      recomputePageHot();
      pageCtxDirty = false;
    }

    function threadActiveFor(a) {
      const def = S.adapters.definition ? S.adapters.definition(a.id) : null;
      if (!a.thread) return false;
      if (!def || !def.threadUrl) return true;
      try { return def.threadUrl.test(loc.pathname + loc.search); } catch (e) { return false; }
    }

    // Thread root: its context = context-selector matches inside it + its first adapter item (the post).
    function refreshThread(root) {
      let ti = threads.get(root);
      if (!ti) { ti = { resolved: false, hot: 0, first: null, shield: false }; threads.set(root, ti); }
      let text = "";
      let first = null;
      try {
        // the head post defines the thread only where the root really is one thread (adapter.threadFirst): the first item
        // of a feed container is just whichever post is on top, and must not heat every other post below it
        if (adapter.own && adapter.threadFirst) first = nQS(root, adapter.own);
        const tctx = adapter.threadContext || adapter.context;
        const els = tctx ? Array.prototype.slice.call(nQSA(root, tctx), 0, 6) : [];
        text = contextText(els, 6);
        if (first) text += " " + extract(first, true, true).text.slice(0, 2000);
        else if (root.shadowRoot) text += "";
        const attrText = adapter.attrs ? adapter.attrs.map((a) => nGetAttr(root, a)).filter(Boolean).join(" ") : "";
        text += " " + attrText;
      } catch (e) { /* ignore */ }
      const resolved = !!(first || text.trim());
      const hot = text.trim() && S.classify.isHotText(text) ? 2 : 0;
      const firstU = first ? U.get(first) : null;
      const sh = !!(firstU && firstU.dec === "shield");
      const changed = ti.resolved !== resolved || ti.hot !== hot || ti.shield !== sh;
      ti.resolved = resolved; ti.hot = hot; ti.first = first; ti.shield = sh;
      return changed;
    }

    function threadOf(u) {
      if (!adapter.thread || !adapter.threadActive) return null;
      const r = closestFlat(u, adapter.thread);
      if (!r || r === u) return null;
      let ti = threads.get(r);
      if (!ti) { refreshThread(r); ti = threads.get(r); }
      return { root: r, ti };
    }

    function isWarm() {
      const t = now();
      while (shieldTimes.length && t - shieldTimes[0] > 30000) shieldTimes.shift();
      return shieldTimes.length >= 3;
    }

    // ---- decision ----------------------------------------------------------------------------------------------
    // Long unit text is classified in windows (WIN_CHARS + overlap), cooperatively: a window that runs past the slice deadline
    // leaves the unit pending (covered) and the next slice continues where this one stopped. Returns "shield" | "clear" |
    // "cfg", or null while unfinished.
    function windowed(u, text, key, one) {
      let job = longJobs.get(u);
      if (!job || job.key !== key) { job = { key, pos: 0, res: "clear" }; longJobs.set(u, job); }
      const step = WIN_CHARS - WIN_OVERLAP;
      while (job.pos < text.length) {
        const d = one(nSlice(text, job.pos, job.pos + WIN_CHARS));
        if (d === "shield") { longJobs.delete(u); return "shield"; }
        if (d === "cfg") job.res = "cfg";
        job.pos += step;
        if (job.pos < text.length && now() > sliceDeadline) return null;
      }
      longJobs.delete(u);
      return job.res;
    }

    // Live stream on a site with an exempt region (YouTube): the player is never checked or covered while the stream is live.
    // Checked on every mutation batch (one querySelector): the player's class flips with no other attribute we observe, and
    // YouTube's player updates its own DOM (time display, badges) many times a second.
    function checkLive() {
      if (!adapter || !adapter.exempt || !adapter.exemptWhen) { liveNow = false; return; }
      let live = false;
      try { live = !!doc.querySelector(adapter.exemptWhen); } catch (e) { live = false; }
      if (live !== liveNow) { liveNow = live; rescanAll(false); }
    }
    function isExempt(u) {
      return liveNow && !!adapter.exempt && u.nodeType === 1 && !!closestFlat(u, adapter.exempt);
    }

    function decide(u, generic) {
      if (isExempt(u)) return { u, dec: "clear", fp: "live|" + S.lexicon.version, cfp: "live", hot: 0, generic, nb: false, media: false, ocr: false, strong: false, warmOnly: false, frameErr: false };
      // A small card (article / li / figure ...) that holds a shielded block is covered whole, whether it was escalated to or is
      // itself the unit: its own text excludes the nested shielded block, which would otherwise release the card around it.
      if (escalated.has(u) || (adapter.generic && u.nodeType === 1 && !isOwn(u) && nMatches(u, ESCALATE_SEL))) {
        if (hasInnerShield(u) && coverWhole(u) && !detectionBroken) {
          const efp = "esc|" + cfg.mode + "|" + S.lexicon.version;
          const ec = U.get(u);
          if (ec && ec.fp === efp && ec.dec === "shield" && ec.targets.every((t) => S.shield.state(t).mode !== "none")) return { u, dec: "shield", same: true, fp: efp, cfp: efp, hot: Math.max(pageHot, 1), generic };
          return { u, dec: "shield", fp: efp, cfp: efp, hot: Math.max(pageHot, 1), generic, nb: false, media: true, ocr: false, strong: false, warmOnly: false, frameErr: false };
        }
        escalated.delete(u);
      }
      const deep = isDeep(u);
      const ex = extract(u, isOwn(u), deep);
      const text = ex.text;
      // OCR results of this unit's media (cache hits included) become part of what the unit says
      let ocrTexts = null;
      let ocrKey = "";
      if (ocrOn()) {
        if (ex.media) {
          const ui = ensureRecs(u);
          for (const rec of ui.recs) {
            const r = rec.res || cacheGet(rec.key);
            if (r && r.status === "ok" && r.text && !isBlank(r.text)) (ocrTexts || (ocrTexts = [])).push(r.text);
          }
          ocrKey = "|o" + (ocrVer.get(u) | 0) + "." + (mediaVer.get(u) | 0);
        } else ocrKey = "|O";
      }
      const cfp = fingerprint(ocrTexts ? text + "\u0001" + aJoin(ocrTexts, "\u0001") : (ex.sig ? text + "\u0002" + ex.sig : text));
      let hot = pageHot;
      let reply = false;
      let wait = null;
      const th = threadOf(u);
      if (th) {
        if (th.ti.hot > hot) hot = th.ti.hot;
        reply = th.ti.shield && th.ti.first !== u;
        if (!th.ti.resolved && adapter.highRisk && doc.readyState !== "complete" && th.ti.first !== u) wait = th.root;
      }
      // Feed warmth (>= 3 recent GTA shields) does NOT heat other posts. During the launch window every feed is "warm" within
      // seconds, and treating it as GTA context shielded any post that said "dying", "the end", "story" or "last" (and re-ran
      // the whole page on every warm/cold flip). Posts are judged on their own words; real GTA context comes from the page,
      // the thread head or a shielded neighbour block.
      const warmOnly = false;
      // local heat: a plain block next to a shielded sibling inside a small parent (the rest of a spoiler that was
      // split across sibling blocks) is evaluated as if inside a spoiler thread
      const nb = !isOwn(u) && hasShieldedNeighbor(u);
      if (nb) { if (hot < 1) hot = 1; reply = true; }
      const fp = cfp + "|" + hot + (reply ? "r" : "") + (nb ? "n" : "") + (warmOnly ? "w" : "") + "|" + cfg.mode + (deep ? "d" : "s") + ocrKey + "|" + S.lexicon.version;
      const c = U.get(u);
      if (wait) return { u, dec: "pending", wait, fp, cfp, hot };
      if (c && c.fp === fp && c.dec !== "pending" && c.dec !== "error") {
        if (c.dec === "clear" || c.targets.every((t) => S.shield.state(t).mode !== "none")) return { u, dec: c.dec, same: true, fp, cfp, hot, generic };
      }
      let dec;
      let strong = false;
      let frameErr = false;
      let nbText = false;
      if (detectionBroken) dec = "clear"; // the classifier does not work here (canary): page shield / notice already say so
      else if (!text.trim() && !ex.media) dec = "clear";
      else if (!isOwn(u) && isSearchUi(u)) dec = "clear"; // the search box itself (see SEARCH_UI)
      else {
        // Page chrome (app shells, navigation, toolbars) is masked only for what its own words say: the page's heat and the
        // "picture in hot context" rule are for content. Without this a hot page (a search for GTA VI) hid YouTube's whole app
        // shell for a weak cue in its footer links, and its guide drawer for its icons.
        const chrome = isChrome(u, deep);
        const uHot = chrome ? 0 : hot;
        const cctx = { hot: uHot, source: "text", reply: reply && !chrome, launchWindow: !!(adapter.highRisk && launchWindow) };
        // ocrDone is always true here: media is held by the OCR gate (gateMedia), not by the policy's "hold" verdict
        const polMedia = ex.media && !warmOnly && !chrome;
        const opts = { mode: cfg.mode, hot: uHot, kind: !text.trim() && !warmOnly && !chrome ? "media" : "item", hasMedia: polMedia, highRisk: adapter.highRisk, ocrDone: true };
        const oneWith = (t, cx, o) => {
          const res = S.classify.text(t, cx);
          let d = S.policy.decide(res, o);
          if (d === "shield" && res && res.flags && (res.flags.franchise || res.flags.entity)) strong = true;
          if (configPending) {
            const all = S.policy.decideAll(res, o);
            const vals = [all.lockdown, all.strict, all.balanced];
            d = vals.some((v) => v !== vals[0]) ? "cfg" : vals[0];
          }
          if (d === "hold") d = o.hot > 0 ? "shield" : "clear";
          return d;
        };
        const one = (t) => oneWith(t, cctx, opts);
        if (text.length > WIN_CHARS + WIN_CHARS / 2) {
          const r = windowed(u, text, cfp, one);
          if (r === null) return { u, dec: "pending", more: true, fp, cfp, hot };
          dec = r;
        } else dec = one(text);
        if (ex.tight && dec !== "shield" && ex.tight.length <= WIN_CHARS * 2) { const d2 = one(ex.tight); if (d2 === "shield" || (d2 === "cfg" && dec === "clear")) dec = d2; }
        if (ocrTexts && dec !== "shield") {
          // the text found inside the images, on its own (source "ocr") and glued to the unit's own text
          const octx = { hot: uHot, source: "ocr", reply: cctx.reply, launchWindow: cctx.launchWindow };
          const mopts = { mode: cfg.mode, hot: uHot, kind: "media", hasMedia: !chrome, highRisk: adapter.highRisk, ocrDone: true };
          let od = dec;
          for (const t of ocrTexts) {
            const d = oneWith(t, octx, mopts);
            if (d === "shield") { od = "shield"; break; }
            if (d === "cfg") od = "cfg";
          }
          if (od !== "shield" && text.length <= WIN_CHARS * 2) { const dc = one(text + "\n" + aJoin(ocrTexts, "\n")); if (dc === "shield" || (dc === "cfg" && od === "clear")) od = dc; }
          if (od === "shield" || od === "cfg") dec = od;
        }
        // frames the engine cannot look into: cover them where the context calls for it (see frameVerdict)
        if (dec === "clear" && ex.frames && !isPageRoot(u)) { // frames straight under <body> are units of their own
          const fv = frameVerdict(ex.frames, uHot);
          if (fv === "shield") { dec = "shield"; frameErr = true; strong = false; }
          else if (fv === "cfg") dec = "cfg";
        }
        if (dec === "cfg") return { u, dec: "pending", cfgWait: true, fp, cfp, hot };
        if (dec !== "shield" && dec !== "clear" && dec !== "pending") dec = "shield";
        // a neighbour-heat shield passes the heat on only when its words carry the spoiler, not merely a picture or an icon
        if (nb && dec === "shield" && !frameErr && text.trim()) {
          nbText = !ex.media || oneWith(text, cctx, Object.assign({}, opts, { hasMedia: false, kind: "item" })) === "shield";
        }
      }
      return { u, dec, fp, cfp, hot, generic, nb, nbText, media: ex.media, ocr: !!ocrTexts, strong: dec === "shield" && strong, warmOnly, frameErr };
    }

    // Evaluate units: batched decisions first, then one layout-read pass, then the writes.
    // Evaluate units in rounds so that containers are classified AFTER their member units have been applied
    // (nested shielded units are excluded from a container's text): inner units, then outer units, then up to
    // LADDER_LEVELS rounds of small ancestor blocks (spoilers split across sibling elements).
    // Returns {changed, left}: \`left\` = units that did not fit in the budget (the caller keeps them queued and covered).
    function evaluate(units, t0, budget, fromFrame) {
      const set = mkSet(units);
      const outer = new Set();
      for (const u of units) {
        let p = parentOf(u);
        for (let i = 0; p && i < 80; i++) { if (set.has(p)) outer.add(p); p = parentOf(p); }
      }
      const inner = outer.size ? aFilter(units, (u) => !outer.has(u)) : units;
      const ctx = { processed: 0, changed: 0, any: false, left: [] };
      let ladderNext = new Set();
      const run = (list, collect) => {
        const r = runRound(list, t0, budget, fromFrame, ctx);
        if (collect) for (const l of r) ladderNext.add(l);
      };
      run(inner, true);
      if (outer.size) run(Array.from(outer), true);
      for (let lvl = 0; lvl < LADDER_LEVELS && ladderNext.size && !ctx.left.length; lvl++) {
        const cur = Array.from(ladderNext);
        ladderNext = new Set();
        run(cur, true);
      }
      for (const l of ladderNext) { if (ctx.left.length) ctx.left.push(l); } // ladder candidates that did not run: re-queued with the rest
      const n = S.shield.count();
      if (n > 20 && !opaqueRule) setOpaque(true);
      else if (n <= 12 && opaqueRule) setOpaque(false);
      return ctx;
    }

    // One batched round: decide all, one layout-read pass, then writes. Returns ancestor blocks to evaluate next.
    function runRound(units, t0, budget, fromFrame, ctx) {
      const plans = [];
      const next = [];
      for (const u of units) {
        if (!u.isConnected) { pendingSet.delete(u); freshUnits.delete(u); continue; }
        if (ctx.processed > 0 && now() - t0 > budget) { aPush(ctx.left, u); continue; }
        ctx.processed++;
        let plan;
        try {
          plan = decide(u, !isOwn(u));
        } catch (e) {
          stats.errors++;
          noteError(e);
          report("error.classify");
          plan = { u, dec: detectionBroken && !isProtective() ? "clear" : "error", fp: "e", cfp: "e", hot: 0 };
        }
        if (host.kind === "test" && stats.trace) { if (stats.trace.length < 400) stats.trace.push(u.localName + "[" + (u.textContent || "").trim().slice(0, 14) + "]:" + plan.dec + (plan.same ? ":same" : "") + ":h" + plan.hot + (plan.nb ? "n" : "") + (u.id ? "#" + u.id : "") + (plan.wait ? ":wait" : "") + (fromFrame ? ":raf" : "")); }
        if (fromFrame && !plan.same) ctx.changed++;
        if (plan.dec !== "pending") freshUnits.delete(u);
        aPush(plans, plan);
        if (plan.more) aPush(ctx.left, u); // windowed classification unfinished: covered now, continued next slice
        if (plan.dec === "clear" && plan.generic && (!plan.same || fromFrame)) {
          const b = ladderParent(u);
          if (b) aPush(next, b);
        }
      }
      const preps = new Map();
      for (const p of plans) {
        if (p.same || p.dec === "clear" || (p.dec === "pending" && !p.budget && p.wait)) continue;
        const c = U.get(p.u);
        if (c && c.targets && c.targets.length && c.targets.every((t) => t.isConnected && S.shield.state(t).mode !== "none")) continue;
        if (p.dec === "pending") { preps.set(p.u, [{ el: p.u, needPos: true }]); continue; }
        try { preps.set(p.u, S.shield.prepare(p.u)); } catch (e) { preps.set(p.u, [{ el: p.u, needPos: true }]); }
      }
      for (const p of plans) {
        try { applyPlan(p, preps.get(p.u)); } catch (e) {
          stats.errors++;
          noteError(e);
          report("error.apply");
          try { applyPlan({ u: p.u, dec: "error", fp: "e", cfp: "e", hot: 0 }, [{ el: p.u, needPos: true }]); } catch (x) { /* ignore */ }
        }
      }
      stats.units += plans.length;
      // the rAF safety pass is for parser appends (DESIGN §5.1 step 3): while loading and for 2 s after load or a large
      // insertion. After that every change arrives as a record; re-checking every touched unit on the next frame only
      // doubled the cost of pages that change every frame (games, animations, live tickers).
      if (!fromFrame && (doc.readyState !== "complete" || now() < rafWatchUntil)) { if (recent.size > 5000) recent.clear(); for (const p of plans) recent.add(p.u); }
      // siblings of units that just became shielded are re-evaluated with local heat (same round budget)
      for (const p of plans) {
        if (!p.generic || p.same) continue;
        if (p.dec === "shield") {
          const g = p.fp.slice(0, 4) === "esc|" ? null : escalationTarget(p.u);
          if (g) { escalated.add(g); aPush(next, g); }
          for (const sib of [p.u.previousElementSibling, p.u.nextElementSibling]) if (sib && sib.nodeType === 1 && !nHasAttr(sib, "data-sfw") && sibEligible(p.u, sib)) aPush(next, sib);
        } else if (p.dec === "clear" && !p.nb && hasShieldedNeighbor(p.u)) aPush(next, p.u);
      }
      return next;
    }

    function sibEligible(u, sib) {
      const par = parentOf(u);
      return !!par && par.nodeType === 1 && par.tagName !== "BODY" && par.tagName !== "HTML" && textLen(par, LADDER_TEXT_CAP) <= LADDER_TEXT_CAP && !SKIP.has(sib.tagName);
    }

    function hasShieldedNeighbor(u) {
      const a = u.previousElementSibling, b = u.nextElementSibling;
      const hit = (e) => e && heatSrc.has(e) && nGetAttr(e, "data-sfw") === "shield" && e.localName !== "sfw-shield";
      if (!(hit(a) || hit(b))) return false;
      return sibEligible(u, a || b);
    }

    function ladderParent(u) {
      const p = parentOf(u);
      if (!p || p.nodeType !== 1) return null;
      const b = isBlockTag(p.tagName) ? p : blockOf(p);
      if (!b || b.tagName === "BODY" || b.tagName === "HTML") return null;
      if (nHasAttr(b, "data-sfw")) return null;
      if (textLen(b, LADDER_TEXT_CAP) > LADDER_TEXT_CAP) return null;
      return b;
    }

    function applyPlan(plan, prep) {
      const u = plan.u;
      const c = U.get(u);
      if (plan.same) return;
      pollChanged = true;
      if (plan.dec === "clear") {
        if (c && c.targets) for (const t of c.targets) { S.shield.release(t); heatSrc.delete(t); }
        heatSrc.delete(u);
        if (S.shield.state(u).mode !== "none") S.shield.release(u);
        U.set(u, { fp: plan.fp, cfp: plan.cfp, dec: "clear", targets: [] });
        pendingSet.delete(u);
        if (adapter.highRisk) { const cold = plan.hot === 0 || plan.warmOnly; if (cold && ocrOn()) gateMedia(u); else markMedia(u, cold); }
        return;
      }
      if (plan.dec === "shield" && ocrOn()) { quiesce(u); if (plan.ocr) stats.ocr.ocrShields++; }
      if (plan.wait) {
        let set = threadWait.get(plan.wait);
        if (!set) { set = new Set(); threadWait.set(plan.wait, set); }
        set.add(u);
      } else if (plan.cfgWait) {
        stats.pending++; // resumed by the config-ready rescan
      } else if (plan.dec === "pending") {
        pendingSet.add(u);
        stats.pending++;
      } else pendingSet.delete(u);
      const variant = plan.dec === "pending" ? "pending" : plan.dec === "error" ? "error" : plan.frameErr ? "frameerr" : "spoiler";
      let targets = c && c.targets && c.targets.length && c.targets.every((t) => t.isConnected) ? c.targets.map((t) => ({ el: t, needPos: null })) : null;
      if (!targets) targets = prep || [{ el: u, needPos: true }];
      const els = [];
      const givesHeat = variant === "spoiler" && (!plan.nb || !!plan.nbText);
      for (const t of targets) {
        const el = t.el;
        els.push(el);
        if (givesHeat) heatSrc.add(el); else heatSrc.delete(el);
        const s = S.shield.state(el);
        if (s.mode === "revealed") {
          if (plan.dec === "shield" && s.revealedFp === plan.cfp) continue; // user revealed exactly this content
          S.shield.reblur(el);
        }
        if (variant === "spoiler" && s.mode === "shielded" && s.variant === "spoiler") { S.shield.setFp(el, plan.cfp); continue; }
        // a unit that went none -> shield, or pending -> shield (a budget overflow covered it first), is a new shield
        const isNew = s.mode === "none" || (s.mode === "shielded" && s.variant === "pending");
        S.shield.apply(el, { variant, fp: plan.cfp, needPos: t.needPos });
        if (isNew && variant !== "pending") onNewShield(plan);
      }
      U.set(u, { fp: plan.fp, cfp: plan.cfp, dec: plan.dec, targets: els });
      // thread root shield state feeds reply context
      if (adapter.thread && adapter.threadActive) {
        const r = closestFlat(u, adapter.thread);
        if (r) { const ti = threads.get(r); if (ti && ti.first === u) { const sh = plan.dec === "shield"; if (ti.shield !== sh) { ti.shield = sh; pendingThreadRescan(r); } } }
      }
    }

    function pendingThreadRescan(r) {
      const units = new Set();
      rescanInside(r, units);
      for (const u of units) { const c = U.get(u); if (c) c.fp = "stale"; touch(u); }
    }

    function onNewShield(plan) {
      stats.shields++;
      if (!stats.firstShieldAt) stats.firstShieldAt = now();
      report("shield");
      // only a shield that carried a franchise / strong-entity signal makes the feed warm: weak-name or generic-spoiler shields
      // ("Jason dies at the end of Friday the 13th") must not turn ordinary posts into collateral
      if (adapter.highRisk && plan && plan.strong) {
        const was = warm;
        shieldTimes.push(now());
        warm = isWarm();
        if (warm && !was) report("feed.warm"); // statistics only (see decide: warmth is not context)
      }
    }

    // Media quarantine release: only when the owning unit is clear and the context is cold.
    function markMedia(u, ok, skip) {
      if (!adapter.media || !quarSheet) return;
      let list;
      try { list = S.shield.deepAll(u, adapter.media, 400); } catch (e) { return; }
      const self = u.matches && nMatches(u, adapter.media) ? [u] : [];
      for (const m of self.concat(list)) {
        try {
          if (ok && skip && skip(m)) continue; // tracked by the OCR gate: released only when its result is in
          if (ok) { if (nGetAttr(m, "data-sfw-m") !== "ok") nSetAttr(m, "data-sfw-m", "ok"); }
          else if (nHasAttr(m, "data-sfw-m")) nRemoveAttr(m, "data-sfw-m");
        } catch (e) { /* ignore */ }
      }
    }

    function onShieldReveal(container, st) {
      report("reveal");
      if (st && st.page) {
        pageRevealedKey = loc.href;
        restoreTitle(true);
        return;
      }
      if (container && adapter.highRisk) {
        markMedia(container, true);
        // images the OCR gate holds (backgrounds, posters, images whose check failed) show once the user reveals them
        const ui = unitInfo.get(container);
        if (ui) for (const rec of ui.recs) { rec.st = "done"; hidePending(rec); liveRecs.delete(rec); ocrQueue.delete(rec); setOk(rec.m, false); }
        for (const rec of failRecs) if (rec.shieldEls && rec.shieldEls.indexOf(container) >= 0) { setOk(rec.m, false); rec.st = "done"; }
      }
    }

    function onShieldReblur(container) {
      report("reblur");
    }

    // ---- media OCR gate ----------------------------------------------------------------------------------------
    // Contract (DESIGN §3, §5.3): host.ocr.request({url, kind, width, height, naturalWidth, naturalHeight}) resolves to
    // {status:"ok", text} | {status:"unavailable"|"error"|"skipped"} and may take seconds.
    //
    // Zero-flash for media is NOT done here: it is the CSS quarantine (QUARANTINE_CSS: media is visibility:hidden until
    // it carries data-sfw-m="ok"). This gate only decides WHEN a unit's media may be released:
    //   * only units that decide "clear" in a cold context reach it (hot / flagged units are shielded, media included);
    //   * eligible media (img, video[poster], inline background-image) stay quarantined until the host settles;
    //   * the OCR text is folded into the unit's own decision (decide()): shield the whole unit or release the media;
    //   * failures never release on their own: strict/lockdown keep a click-to-reveal "couldn't check" cover.
    // Nothing here runs inside the MutationObserver callback except bookkeeping; requests, results and releases happen
    // from IntersectionObserver callbacks, timers and promise continuations.
    function ocrHost() {
      const o = host && host.ocr;
      return o && typeof o.request === "function" ? o : null;
    }

    // OCR gating applies on high-risk hosts (the only place media is quarantined). While the host config is still
    // loading, a host that may enable OCR (`host.ocrMaybe`) is treated as if it did: media is held, never released early.
    function ocrOn() {
      if (!booted || !adapter || !adapter.highRisk || !quarSheet || adapter.ocr === false) return false;
      return !!ocrHost() || (configPending && !!(host && host.ocrMaybe));
    }

    function ocrSync() {
      const on = ocrOn();
      if (on === lastOcrOn) return false;
      lastOcrOn = on;
      if (!on) ocrShutdown();
      return true;
    }

    function ocrKickRec(rec) {
      if (!ocrIO) return;
      try { ocrIO.unobserve(rec.m); ocrIO.observe(rec.m); } catch (e) { /* ignore */ }
    }

    function ocrKick() {
      if (!lastOcrOn) return;
      for (const rec of liveRecs) ocrKickRec(rec);
      schedulePump();
    }

    function ocrShutdown() {
      for (const rec of Array.from(liveRecs)) {
        hidePending(rec);
        rec.st = "held";
        rec.obs = false;
        try { if (ocrIO) ocrIO.unobserve(rec.m); if (ocrRO) ocrRO.unobserve(rec.m); } catch (e) { /* ignore */ }
      }
      liveRecs.clear();
      ocrQueue.clear();
      for (const rec of Array.from(failRecs)) unfailRec(rec);
      failRecs.clear();
      ocrEvalSet.clear();
      if (ocrBgMo) { try { ocrBgMo.disconnect(); } catch (e) { /* ignore */ } ocrBgMo = null; }
    }

    function isMediaEl(el) {
      if (MEDIA_TAGS.has(el.tagName)) return true;
      return el.hasAttribute && nHasAttr(el, "style") && /background-image/i.test(nGetAttr(el, "style"));
    }

    function urlKey(url) {
      return url.length > 300 ? url.slice(0, 40) + "#" + url.length + "." + fnv1a(url).toString(36) : url;
    }

    // What to OCR for a media element: {kind, url} or null (nothing to read: no src yet, gradient background, ...).
    function mediaDesc(m) {
      const tag = m.localName;
      let raw = "";
      let kind = "img";
      if (tag === "img") raw = m.currentSrc || nGetAttr(m, "src") || "";
      else if (tag === "video") { raw = nGetAttr(m, "poster") || ""; kind = "poster"; }
      else { const mm = BG_URL.exec(nGetAttr(m, "style") || ""); raw = mm ? mm[2] : ""; kind = "bg"; }
      raw = raw.trim();
      if (!raw) return null;
      try { return { kind, url: new win.URL(raw, doc.baseURI).href }; } catch (e) { return null; }
    }

    // currentSrc lags behind a src/srcset change (the browser re-selects asynchronously), so attribute changes are detected
    // from the attributes themselves and force a fresh look (needLoad) even while currentSrc still shows the old URL.
    function attrSig(m) {
      const t = m.localName;
      if (t === "img") return (nGetAttr(m, "src") || "") + " | " + (nGetAttr(m, "srcset") || "");
      if (t === "video") return nGetAttr(m, "poster") || "";
      return "";
    }

    function cacheGet(key) {
      if (!key) return null;
      const c = ocrCache.get(key);
      if (!c) return null;
      if ((c.status === "error" || c.status === "unavailable") && now() - c.at > OCR_ERR_TTL_MS) { ocrCache.delete(key); return null; }
      return c;
    }

    function cachePut(key, res) {
      if (ocrCache.size >= OCR_CACHE_MAX) ocrCache.delete(ocrCache.keys().next().value);
      ocrCache.set(key, { status: res.status, text: res.text || "", at: now() });
    }

    function normalizeRes(r) {
      if (r && typeof r === "object") {
        if (r.status === "ok" && typeof r.text === "string") return { status: "ok", text: r.text.length > 40000 ? r.text.slice(0, 40000) : r.text };
        if (r.status === "unavailable" || r.status === "skipped") return { status: r.status };
      }
      return { status: "error" };
    }

    function setOk(m, fade) {
      try {
        if (nGetAttr(m, "data-sfw-m") !== "ok") {
          if (fade && !cfg.reducedEffects) {
            nSetAttr(m, "data-sfw-fade", "");
            m.addEventListener("animationend", () => { try { nRemoveAttr(m, "data-sfw-fade"); } catch (e) { /* ignore */ } }, { once: true });
          }
          nSetAttr(m, "data-sfw-m", "ok");
        }
        const p = m.parentElement;
        if (p && p.localName === "picture" && nGetAttr(p, "data-sfw-m") !== "ok") nSetAttr(p, "data-sfw-m", "ok");
      } catch (e) { /* ignore */ }
    }

    function removeOk(m) {
      try {
        if (nHasAttr(m, "data-sfw-m")) nRemoveAttr(m, "data-sfw-m");
        if (nHasAttr(m, "data-sfw-fade")) nRemoveAttr(m, "data-sfw-fade");
        const p = m.parentElement;
        if (p && p.localName === "picture" && nHasAttr(p, "data-sfw-m")) nRemoveAttr(p, "data-sfw-m");
      } catch (e) { /* ignore */ }
    }

    // Records and the media of one unit. Media without anything to read (`plain`) is released like in v1.
    function ensureRecs(u) {
      let ui = unitInfo.get(u);
      if (!ui) { ui = { recs: new Set(), plain: [] }; unitInfo.set(u, ui); }
      let list = [];
      try { list = S.shield.deepAll(u, OCR_SEL, 60); } catch (e) { /* ignore */ }
      if (u.matches && nMatches(u, OCR_SEL) && list.indexOf(u) < 0) list.unshift(u);
      const recs = new Set();
      const plain = [];
      for (const m of list) {
        if (m.localName === "sfw-shield") continue;
        const d = mediaDesc(m);
        if (!d && m.localName !== "img") { plain.push(m); continue; }
        let rec = mrec.get(m);
        if (!rec) rec = newRec(m, u, d);
        else {
          rec.units.add(u);
          const sig = attrSig(m);
          if (sig !== rec.sig || (!rec.needLoad && (d ? d.url : "") !== rec.url)) { rec.sig = sig; ocrReset(rec, d, true); }
        }
        recs.add(rec);
      }
      ui.recs = recs;
      ui.plain = plain;
      return ui;
    }

    function newRec(m, u, d) {
      const rec = {
        m, units: new Set([u]), kind: d ? d.kind : "img", url: d ? d.url : "", key: d ? urlKey(d.url) : "", st: "held", res: null,
        wrap: null, wrapPos: false, shieldEls: null, sig: attrSig(m), obs: false, inRange: false, needLoad: false, retried: false, req: null, timer: 0, retryTimer: 0, ro: false, loadFn: null,
      };
      mrec.set(m, rec);
      removeOk(m); // released by v1 / before OCR was on / for another URL: hold it again
      if (m.localName === "img") {
        rec.loadFn = () => onMediaLoad(rec);
        m.addEventListener("load", rec.loadFn);
        m.addEventListener("error", rec.loadFn);
      } else if (rec.kind === "bg") {
        if (!ocrBgMo && typeof win.MutationObserver === "function") ocrBgMo = new MO_CTOR(onBgMutations);
        try { if (ocrBgMo) ocrBgMo.observe(m, { attributes: true, attributeFilter: ["style"] }); } catch (e) { /* ignore */ }
      }
      return rec;
    }

    // The URL (or src attribute) behind a record changed: everything learned about the old one is void.
    function ocrReset(rec, d, needLoad) {
      hidePending(rec);
      ocrQueue.delete(rec);
      removeOk(rec.m);
      unfailRec(rec);
      T_CLEAR(rec.timer);
      T_CLEAR(rec.retryTimer);
      rec.res = null;
      rec.retried = false;
      rec.req = null;
      rec.kind = d ? d.kind : rec.kind;
      // needLoad: the attributes changed but currentSrc still names the OLD image until the browser re-selects and loads
      // the new one, so no URL is trusted (nothing may be answered from the cache) until the load event / fallback timer
      rec.needLoad = !!needLoad && rec.m.localName === "img" && !!d;
      rec.url = d && !rec.needLoad ? d.url : "";
      rec.key = d && !rec.needLoad ? urlKey(d.url) : "";
      rec.st = "held";
      if (rec.needLoad) {
        rec.timer = T_SET(() => { rec.timer = 0; if (rec.needLoad) { rec.needLoad = false; ocrAdvance(rec); } }, OCR_LOAD_FALLBACK_MS);
      }
      if (!d && rec.m.localName !== "img") { liveRecs.delete(rec); return; }
      liveRecs.add(rec);
      ocrKickRec(rec);
    }

    function onMediaLoad(rec) {
      if (!booted || !ocrOn() || mrec.get(rec.m) !== rec) return;
      rec.needLoad = false;
      T_CLEAR(rec.timer);
      const d = mediaDesc(rec.m);
      if (d && d.url !== rec.url) { ocrReset(rec, d, false); ocrRegister(rec); return; }
      if (rec.st === "held") ocrAdvance(rec);
    }

    function onBgMutations(records) {
      if (!booted || !ocrOn()) return;
      for (const r of records) {
        const rec = mrec.get(r.target);
        if (!rec || rec.kind !== "bg") continue;
        const d = mediaDesc(rec.m);
        if ((d ? d.url : "") !== rec.url) { ocrReset(rec, d, false); ocrRegister(rec); }
      }
    }

    // A decision for this record's units has to be redone (new result, new URL): bump and evaluate soon.
    function ocrRegister(rec) {
      for (const u of Array.from(rec.units)) {
        if (!u.isConnected) { rec.units.delete(u); continue; }
        ocrVer.set(u, (ocrVer.get(u) | 0) + 1);
        ocrEvalSet.add(u);
      }
      if (!ocrEvalTimer) ocrEvalTimer = T_SET(flushOcrEval, 0);
    }

    function flushOcrEval() {
      ocrEvalTimer = 0;
      if (!booted) return;
      const units = Array.from(ocrEvalSet).filter((u) => u.isConnected);
      ocrEvalSet.clear();
      try {
        flushPending();
        if (units.length) {
          const t0 = now();
          sliceDeadline = t0 + BUDGET_MS * 2;
          ucache = new Map(); tlcache = new Map();
          const r = evaluate(units, t0, BUDGET_MS * 2);
          for (const u of r.left) unitQ.add(u);
        }
      } catch (e) { stats.errors++; noteError(e); report("error.ocr"); }
      try { if (mo) MO_TAKE(mo); } catch (e) { /* ignore */ }
      if (workLeft()) scheduleWork();
    }

    // Called from applyPlan for a unit that decided "clear" in a cold context: release or hold each of its media.
    function gateMedia(u) {
      const ui = unitInfo.get(u);
      markMedia(u, true, (m) => {
        if (mrec.has(m)) return true;
        if (m.localName === "picture") { const i = nQS(m, "img"); return !!(i && mrec.has(i)); }
        return false;
      });
      if (!ui) return;
      for (const m of ui.plain) setOk(m);
      for (const rec of ui.recs) {
        if (!rec.m.isConnected) continue;
        let r = rec.res;
        if (!r) { r = cacheGet(rec.key); if (r) stats.ocr.cacheHits++; }
        if (r && rec.url) { rec.res = r; applyResolved(rec, r); } else holdRec(rec);
      }
    }

    function holdRec(rec) {
      removeOk(rec.m);
      if (rec.st === "done") rec.st = "held"; // the result was dropped while the unit was shielded: look again
      liveRecs.add(rec);
      if (!rec.obs) {
        rec.obs = true;
        const io = getIO();
        if (io) { try { io.observe(rec.m); } catch (e) { /* ignore */ } } else T_SET(() => ocrAdvance(rec), 0);
      } else if (rec.st === "held") ocrKickRec(rec);
    }

    // The unit decided clear, so a result is a release or a failure cover: ok -> release; skipped -> release unless
    // lockdown; unavailable / error -> release only in balanced, otherwise a click-to-reveal "couldn't check" cover.
    function applyResolved(rec, r) {
      hidePending(rec);
      ocrQueue.delete(rec);
      liveRecs.delete(rec);
      rec.st = "done";
      if (rec.obs) { rec.obs = false; try { if (ocrIO) ocrIO.unobserve(rec.m); if (ocrRO) ocrRO.unobserve(rec.m); } catch (e) { /* ignore */ } }
      const mode = cfg.mode;
      // skipped = the host could not read it (animated, multi-frame, undecodable): Strict keeps it held with the "couldn't check
      // image" cover like unavailable / error (it can carry text a still frame would show); only Balanced releases it.
      // Local data:/blob: images the engine never asks about (r.local) follow the older rule: released unless Lockdown.
      const release = r.status === "ok" || (r.status === "skipped" ? (r.local ? mode !== "lockdown" : mode === "balanced") : mode === "balanced");
      if (release) {
        unfailRec(rec);
        setOk(rec.m, !r.small);
        stats.ocr.released++;
      } else failShield(rec);
    }

    function onlyChildOf(p, n) {
      for (let c = p.firstChild; c; c = c.nextSibling) {
        if (c === n) continue;
        if (c.nodeType === 3) { if (/\S/.test(c.data)) return false; continue; }
        if (c.nodeType === 1 && c.localName !== "sfw-shield") return false;
      }
      return true;
    }

    // The smallest box that holds only this media (up to 3 levels), so a cover can sit on the picture alone.
    function isolatedWrap(rec) {
      const m = rec.m;
      const tag = m.localName;
      if (tag !== "img" && tag !== "video") return nHasAttr(m, "data-sfw") ? null : m;
      let n = tag === "img" && m.parentElement && m.parentElement.localName === "picture" ? m.parentElement : m;
      let best = null;
      for (let i = 0; i < 3; i++) {
        if (rec.units.has(n)) break;
        const p = parentOf(n);
        if (!p || p.nodeType !== 1 || p.tagName === "BODY" || p.tagName === "HTML" || !onlyChildOf(p, n)) break;
        let inline = false;
        try { inline = nGetCS(p).display === "inline"; } catch (e) { /* ignore */ }
        if (!inline) best = p;
        n = p;
      }
      return best;
    }

    function showPending(rec) {
      if (rec.wrap) return;
      const w = isolatedWrap(rec);
      if (!w || nHasAttr(w, "data-sfw")) return;
      try {
        nSetAttr(w, "data-sfw-mc", "");
        S.shield.setTheme(w, S.shield.theme(w));
        if (nGetCS(w).position === "static") { nSetAttr(w, "data-sfw-mcpos", ""); rec.wrapPos = true; }
        rec.wrap = w;
      } catch (e) { /* ignore */ }
    }

    function hidePending(rec) {
      const w = rec.wrap;
      if (!w) return;
      rec.wrap = null;
      try { nRemoveAttr(w, "data-sfw-mc"); nRemoveAttr(w, "data-sfw-theme"); if (rec.wrapPos) nRemoveAttr(w, "data-sfw-mcpos"); } catch (e) { /* ignore */ }
      rec.wrapPos = false;
    }

    function failShield(rec) {
      if (rec.shieldEls && rec.shieldEls.some((el) => S.shield.state(el).mode !== "none")) { failRecs.add(rec); return; } // covered, or revealed by the user
      let tgt = isolatedWrap(rec);
      if (!tgt || nHasAttr(tgt, "data-sfw")) {
        const b = blockOf(rec.m);
        tgt = b && b.tagName !== "BODY" && b.tagName !== "HTML" ? b : Array.from(rec.units).find((u) => u.isConnected) || null;
      }
      if (!tgt || nHasAttr(tgt, "data-sfw")) return; // already inside a shield
      const els = [];
      try {
        for (const p of S.shield.prepare(tgt)) { S.shield.apply(p.el, { variant: "mediaerr", fp: null, needPos: p.needPos }); els.push(p.el); }
      } catch (e) { stats.errors++; noteError(e); report("error.ocr"); return; }
      rec.shieldEls = els;
      failRecs.add(rec);
      stats.ocr.failShields++;
      if (rec.res && !rec.retried) rec.retryTimer = T_SET(() => ocrRetry(rec), OCR_ERR_TTL_MS + 500);
    }

    function unfailRec(rec) {
      if (rec.shieldEls) {
        for (const el of rec.shieldEls) { try { if (S.shield.state(el).variant === "mediaerr") S.shield.release(el); } catch (e) { /* ignore */ } }
        rec.shieldEls = null;
      }
      failRecs.delete(rec);
    }

    // Transient failures (host warming up, service worker restarting) get one more try after the negative cache expires.
    function ocrRetry(rec) {
      rec.retryTimer = 0;
      if (!booted || !ocrOn() || mrec.get(rec.m) !== rec || rec.retried || !rec.res) return;
      if (rec.res.status !== "error" && rec.res.status !== "unavailable") return;
      if (rec.shieldEls && rec.shieldEls.some((el) => S.shield.state(el).mode === "revealed")) return;
      const d = mediaDesc(rec.m);
      ocrCache.delete(rec.key);
      ocrReset(rec, d, false);
      rec.retried = true;
      stats.ocr.retries++;
      ocrRegister(rec);
    }

    // A unit that is shielded for another reason needs no image check: drop the queue entries and the pending veil.
    function quiesce(u) {
      const ui = unitInfo.get(u);
      if (!ui) return;
      for (const rec of ui.recs) {
        if (rec.st === "wait") { ocrQueue.delete(rec); rec.st = "held"; }
        hidePending(rec);
      }
    }

    function getIO() {
      if (ocrIO) return ocrIO;
      if (typeof win.IntersectionObserver !== "function") return null;
      try { ocrIO = new win.IntersectionObserver(onOcrIntersect, { rootMargin: OCR_MARGIN }); } catch (e) { return null; }
      return ocrIO;
    }

    function onOcrIntersect(entries) {
      if (!booted) return;
      try {
        for (const e of entries) {
          const rec = mrec.get(e.target);
          if (!rec) continue;
          rec.inRange = e.isIntersecting;
          if (e.isIntersecting) { if (rec.st === "held") ocrAdvance(rec); }
          else if (rec.st === "wait") { ocrQueue.delete(rec); hidePending(rec); rec.st = "held"; }
        }
      } catch (e) { stats.errors++; noteError(e); report("error.ocr"); }
    }

    function watchSize(rec) {
      if (rec.ro) return;
      if (!ocrRO && typeof win.ResizeObserver === "function") {
        try {
          ocrRO = new win.ResizeObserver((es) => {
            for (const e of es) {
              const r = mrec.get(e.target);
              if (r && r.st === "held" && e.contentRect.width > 0 && e.contentRect.height > 0) ocrAdvance(r);
            }
          });
        } catch (e) { ocrRO = null; }
      }
      if (ocrRO) { rec.ro = true; try { ocrRO.observe(rec.m); } catch (e) { /* ignore */ } }
    }

    // `rec` entered the OCR window: measure it, then release it (too small), answer from the cache, or queue a request.
    function ocrAdvance(rec) {
      if (rec.st !== "held" || !booted || !ocrOn()) return;
      const m = rec.m;
      if (!m.isConnected || mrec.get(m) !== rec) return;
      if (ocrIO && !rec.inRange) return; // outside the ~1.5-viewport window: stays held until it scrolls near
      if (rec.needLoad) return; // src changed: wait for the new image's load event (or the fallback timer)
      const d = mediaDesc(m);
      if (d && d.url !== rec.url) ocrReset(rec, d, false);
      if (!rec.url) return; // no src yet (lazy image): stays held until one arrives
      const isImg = m.localName === "img";
      if (isImg) {
        if (rec.needLoad || !m.complete) return; // the load listener brings us back
        if (m.naturalWidth === 0) { finishSmall(rec); return; } // broken image: only its alt text can show, and that was classified
      }
      if (shieldedAt(m)) return; // inside a shield for another reason: no point in reading it
      const r = nGBCR(m);
      const w = r.width, h = r.height;
      if (!(w > 0 && h > 0)) { watchSize(rec); return; }
      const nw = isImg ? m.naturalWidth : 0, nh = isImg ? m.naturalHeight : 0;
      if (w < OCR_MIN_W || h < OCR_MIN_H || (nw && nw < OCR_MIN_W) || (nh && nh < OCR_MIN_H)) { finishSmall(rec); return; }
      const c = cacheGet(rec.key);
      if (c) { stats.ocr.cacheHits++; rec.res = c; resolveRec(rec); return; }
      if (!/^(?:https?|data|blob):/i.test(rec.url)) { rec.res = { status: "skipped", local: true }; resolveRec(rec); return; }
      rec.req = { width: Math.round(w), height: Math.round(h), naturalWidth: nw, naturalHeight: nh };
      showPending(rec);
      rec.st = "wait";
      ocrQueue.add(rec);
      schedulePump();
    }

    // Too small to carry readable text: the v1 rule applies (released; the unit's text and alt were already classified).
    function finishSmall(rec) {
      stats.ocr.small++;
      hidePending(rec);
      ocrQueue.delete(rec);
      liveRecs.delete(rec);
      rec.res = { status: "ok", text: "", small: true };
      rec.st = "done";
      if (rec.obs) { rec.obs = false; try { if (ocrIO) ocrIO.unobserve(rec.m); if (ocrRO) ocrRO.unobserve(rec.m); } catch (e) { /* ignore */ } }
      setOk(rec.m, false);
    }

    function resolveRec(rec) {
      hidePending(rec);
      ocrQueue.delete(rec);
      liveRecs.delete(rec);
      rec.st = "done";
      ocrRegister(rec);
    }

    function schedulePump() {
      if (ocrPumpTimer) return;
      ocrPumpTimer = T_SET(() => { ocrPumpTimer = 0; try { pump(); } catch (e) { stats.errors++; noteError(e); report("error.ocr"); } }, 0);
    }

    // Nearest to the viewport first (fully visible ones before anything off-screen, then nearest to the centre).
    function nextRec() {
      let best = null, bs = Infinity;
      const vh = win.innerHeight || 800, vw = win.innerWidth || 1200;
      for (const rec of Array.from(ocrQueue)) {
        if (!rec.m.isConnected) { ocrQueue.delete(rec); liveRecs.delete(rec); continue; }
        const r = nGBCR(rec.m);
        const dy = r.bottom < 0 ? -r.bottom : r.top > vh ? r.top - vh : 0;
        const dx = r.right < 0 ? -r.right : r.left > vw ? r.left - vw : 0;
        const score = (dy + dx) * 100000 + Math.abs((r.top + r.bottom) / 2 - vh / 2);
        if (score < bs) { bs = score; best = rec; }
      }
      return best;
    }

    function pump() {
      if (!booted || !ocrOn()) return;
      const h = ocrHost();
      if (!h) return; // host config still loading: ocrKick() resumes once it is there
      // A host with its own queue (Bowser's native worker) takes more at once: with only 3 in flight, each answer waits
      // a full drain round trip before the next image is even asked for, and thumbnails stay held for seconds.
      const fl = typeof h.flight === "number" && h.flight >= 1 ? Math.min(OCR_FLIGHT_MAX, h.flight | 0) : OCR_FLIGHT;
      while (ocrFlying < fl && ocrQueue.size) {
        const rec = nextRec();
        if (!rec) break;
        ocrQueue.delete(rec);
        startRequest(rec, h);
      }
    }

    function startRequest(rec, h) {
      const key = rec.key;
      const waiters = ocrFlight.get(key);
      rec.st = "run";
      if (waiters) { waiters.add(rec); stats.ocr.dedupe++; return; } // same URL already in flight: share the answer
      const set = new Set([rec]);
      ocrFlight.set(key, set);
      ocrFlying++;
      if (ocrFlying > stats.ocr.maxFlight) stats.ocr.maxFlight = ocrFlying;
      stats.ocr.requests++;
      if (stats.ocrLog) stats.ocrLog.push(rec.url);
      let done = false;
      let wd = 0;
      const finish = (raw) => {
        if (done) return;
        done = true;
        T_CLEAR(wd);
        ocrFlying--;
        ocrFlight.delete(key);
        const res = normalizeRes(raw);
        stats.ocr[res.status]++;
        report("ocr." + res.status);
        cachePut(key, res);
        for (const w of set) {
          if (mrec.get(w.m) === w && w.key === key && w.st === "run") {
            w.res = res;
            resolveRec(w);
          }
        }
        schedulePump();
      };
      wd = T_SET(() => finish({ status: "error" }), OCR_WATCHDOG_MS);
      let p;
      try { p = h.request(Object.assign({ url: rec.url, kind: rec.kind }, rec.req)); } catch (e) { p = Promise.reject(e); }
      Promise.resolve(p).then(finish, () => finish({ status: "error" }));
    }

    // ---- page level ----------------------------------------------------------------------------------------------
    function pageWants() {
      const href = loc.href;
      if (detectionBroken) { let t = ""; try { t = doc.title || ""; } catch (e) { /* ignore */ } return crudeHot(href + " " + t); }
      let shieldIt = false;
      try {
        const ures = S.classify.url(href);
        const tres = S.classify.text(doc.title || "", { source: "title", hot: pageHot });
        for (const r of [ures, tres]) {
          const opts = { mode: cfg.mode, hot: pageHot, kind: "page", hasMedia: false, highRisk: adapter.highRisk, ocrDone: true };
          let d = S.policy.decide(r, opts);
          if (configPending) { const all = S.policy.decideAll(r, opts); if (all.lockdown === "shield" || all.strict === "shield" || all.balanced === "shield") d = "shield"; }
          if (d === "shield") shieldIt = true;
        }
      } catch (e) { shieldIt = true; stats.errors++; report("error.page"); }
      return shieldIt;
    }

    function evalPage() {
      if (!booted) return;
      // A risky URL or title makes the whole page hot: its content is masked item by item (hot context shields anything with
      // a GTA / story signal, and its media). The page itself is never covered for it: a full-page cover also hid the site's
      // own search box, navigation and buttons (a YouTube search for "gta 6" could not even be changed).
      const want = pageWants();
      if (want !== pageWant) {
        pageWant = want;
        if (recomputePageHot()) rescanAll(false);
      }
      // Item masking needs a working engine: with the MutationObserver unusable (rescan polling, not zero-flash) or the
      // classifier broken, new content can paint before it is judged. Then a hot page fails closed with the page cover.
      if (moMode === "poll" || detectionBroken) {
        if (want && pageRevealedKey !== loc.href) {
          if (doc.documentElement && S.shield.pageState(doc) !== "shielded") S.shield.applyPage(doc, "page");
        } else if (!want && S.shield.pageState(doc) === "shielded" && !coverShown) S.shield.releasePage(doc);
      }
      guardTitle();
    }

    function guardTitle() {
      let t;
      try { t = doc.title; } catch (e) { return; }
      if (t === TITLE_GUARD) return;
      if (titleRevealedFor === loc.href) return;
      if (!t) return;
      let bad = false;
      try {
        const r = S.classify.text(t, { source: "title", hot: pageHot });
        bad = r.level === "mention" || r.level === "spoiler";
      } catch (e) { bad = true; }
      if (bad) {
        titleOriginal = t;
        try { doc.title = TITLE_GUARD; } catch (e) { /* ignore */ }
      }
    }

    function restoreTitle(forUser) {
      if (titleOriginal != null) {
        try { doc.title = titleOriginal; } catch (e) { /* ignore */ }
        titleOriginal = null;
      }
      if (forUser) titleRevealedFor = loc.href;
    }

    function checkUrl() {
      if (loc.href === lastHref) return;
      lastHref = loc.href;
      stats.urlChanges++;
      pageRevealedKey = pageRevealedKey === loc.href ? pageRevealedKey : null;
      titleRevealedFor = null;
      recomputeUrlContext();
      checkLive();
      evalPage();
      rescanAll(false);
    }

    // ---- guards ------------------------------------------------------------------------------------------------------
    function installGuards() {
      const add = (target, type, fn, opts) => {
        target.addEventListener(type, fn, opts);
        cleanups.push(() => target.removeEventListener(type, fn, opts));
      };
      const cap = { capture: true };
      for (const t of ["mouseover", "pointerover", "mouseenter", "pointerenter", "mousedown", "pointerdown", "mouseup", "pointerup", "touchstart", "touchend", "dblclick", "auxclick"]) add(win, t, suppress, cap);
      add(win, "click", onClick, cap);
      add(doc, "copy", onCopy, cap);
      add(doc, "cut", onCopy, cap);
      add(win, "input", () => { pageCtxDirty = true; if (!rafId) scheduleRaf(); }, cap);
      add(win, "popstate", urlEvent, cap);
      add(win, "hashchange", urlEvent, cap);
      add(win, "sfw:nav", urlEvent, cap);
      add(doc, "readystatechange", onReadyState, cap);
      add(doc, "DOMContentLoaded", () => { if (moMode === "poll") { pollNext = 0; pollOnce(); } }, cap);
      add(win, "load", onReadyState, cap);
      add(doc, "visibilitychange", () => { if (!doc.hidden) { flushPending(); if (workLeft()) runWork(now(), BUDGET_MS); schedule(); } }, cap);
    }

    function urlEvent() {
      try {
        checkUrl();
        flushPending();
        if (workLeft()) runWork(now(), BUDGET_MS);
        if (mo) MO_TAKE(mo);
        schedule();
      } catch (e) { stats.errors++; report("error.nav"); }
    }

    function onReadyState() {
      if (!booted) return;
      if (doc.readyState === "complete" || doc.readyState === "interactive") {
        // streaming finished: unresolved thread roots resolve with whatever context exists (cold if none)
        for (const [r, set] of Array.from(threadWait)) {
          const ti = threads.get(r);
          if (ti && doc.readyState === "complete") { ti.resolved = true; }
          if (doc.readyState === "complete") { threadWait.delete(r); for (const u of set) touch(u); }
        }
        if (doc.readyState === "complete") rafWatchUntil = now() + RAF_WATCH_MS;
        if (doc.readyState === "complete") for (const g of Array.from(escalated)) { if (g.isConnected) touch(g); else escalated.delete(g); }
        flushPending();
        runWork(now(), BUDGET_MS);
        try { if (mo) MO_TAKE(mo); } catch (e) { /* ignore */ }
        schedule();
      }
    }

    function shieldedAt(t) {
      if (!t || t.nodeType !== 1) return null;
      return closestFlat(t, '[data-sfw="shield"]');
    }

    // Hover cards, hover-autoplay and framework handlers must never fire for events that originate in a shield.
    function suppress(ev) {
      try {
        const c = shieldedAt(ev.target);
        if (c) ev.stopImmediatePropagation();
      } catch (e) { /* ignore */ }
    }

    function onClick(ev) {
      try {
        const t = ev.target;
        const c = shieldedAt(t);
        if (c) {
          const st = S.shield.state(c);
          const onOverlay = t.localName === "sfw-shield";
          if (ev.isTrusted === true && st.mode === "shielded" && (onOverlay || (st.fallback && st.variant !== "pending"))) S.shield.reveal(c);
          ev.preventDefault();
          ev.stopImmediatePropagation();
          return;
        }
        // page overlay (fixed, appended to <html>)
        if (t && t.localName === "sfw-shield" && nGetAttr(t, "data-k") === "page") {
          if (ev.isTrusted === true) S.shield.revealPage(doc);
          ev.preventDefault();
          ev.stopImmediatePropagation();
        }
      } catch (e) { /* ignore */ }
    }

    function onCopy(ev) {
      try {
        const sel = doc.getSelection();
        if (!sel || sel.rangeCount === 0 || sel.isCollapsed) return;
        const pageSh = S.shield.pageState(doc) === "shielded";
        const ranges = [];
        for (let i = 0; i < sel.rangeCount; i++) ranges.push(sel.getRangeAt(i));
        let hit = pageSh;
        if (!hit) {
          for (const c of S.shield.all()) {
            if (!c.isConnected || S.shield.state(c).mode !== "shielded") continue;
            for (const r of ranges) { if (r.intersectsNode(c)) { hit = true; break; } }
            if (hit) break;
          }
        }
        if (!hit) return;
        const out = [];
        if (!pageSh) {
          for (const r of ranges) {
            const root = r.commonAncestorContainer;
            const walker = doc.createTreeWalker(root.nodeType === 3 ? root.parentNode : root, 4);
            let n = root.nodeType === 3 ? root : walker.nextNode();
            while (n) {
              if (r.intersectsNode(n) && !shieldedAt(n.parentElement)) {
                let s = n.data;
                if (n === r.endContainer) s = s.slice(0, r.endOffset);
                if (n === r.startContainer) s = s.slice(r.startOffset);
                if (s.trim()) out.push(s);
              }
              n = root.nodeType === 3 ? null : walker.nextNode();
            }
          }
        }
        ev.clipboardData.setData("text/plain", out.join(" "));
        ev.preventDefault();
        report("copy.guard");
      } catch (e) { /* ignore */ }
    }
  }

  function normalizeConfig(c) {
    c = c || {};
    return {
      enabled: c.enabled !== false,
      mode: c.mode === "lockdown" || c.mode === "balanced" ? c.mode : "strict",
      reducedEffects: !!c.reducedEffects,
      version: c.version || "",
      debugReports: !!c.debugReports,
    };
  }

  return { start, fnv1a, fingerprint, slugText, srcWords, isBlockTag, normalizeConfig, srcdocText, dataUrlHtml, decodeEntities, hasToken };
})();
