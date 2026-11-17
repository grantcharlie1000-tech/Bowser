// QA harness core (agent F): loads a BUILT chrome (any dist folder) in headless Chromium through
// Playwright, behind a stand-in for the WebView2 bridge, and replays the messages Rust would post
// (fixtures/qa/scenarios.mjs). Headless Chromium on Linux is not WebView2 on Windows: no Segoe UI,
// no Mica/acrylic, no native popup windows. What it does measure is the page itself: DOM, computed
// styles, accessibility tree, layout and script timing.
//
// Exports: loadChromium(), serveDist(dir), openScenario(browser, origin, scenario, opts), ACTS,
// auditPage(page, opts) and the in-page probes it uses.
import { createServer } from 'node:http';
import { existsSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { extname, join, resolve } from 'node:path';

/** Playwright's Chromium, or null when Playwright or its browser is missing (tests then skip). */
export function loadChromium() {
  let playwright = null;
  for (const from of [import.meta.url, join(process.cwd(), 'noop.js')]) {
    try { playwright = createRequire(from)('playwright'); break; } catch { /* next */ }
  }
  if (!playwright) return null;
  const shell = '/opt/pw-browsers/chromium_headless_shell-1243/chrome-headless-shell-linux64/chrome-headless-shell';
  const explicit = process.env.BOWSER_QA_CHROMIUM || (existsSync(shell) ? shell : '');
  try { if (!explicit && !existsSync(playwright.chromium.executablePath())) return null; } catch { return null; }
  return {
    launch: () => playwright.chromium.launch({ args: ['--no-sandbox', '--font-render-hinting=none'], ...(explicit ? { executablePath: explicit } : {}) }),
  };
}

const TYPES = { '.js': 'text/javascript', '.css': 'text/css', '.html': 'text/html' };

/** Serves `dir` (a built ui/dist) on 127.0.0.1 with the production CSP as a header too. */
export async function serveDist(dir) {
  const root = resolve(dir);
  if (!existsSync(join(root, 'index.html')) || !existsSync(join(root, 'app.js'))) throw new Error(`${root} is not a built dist folder (run npm run build)`);
  const html = readFileSync(join(root, 'index.html'), 'utf8');
  const csp = html.match(/Content-Security-Policy" content="([^"]+)"/)?.[1] ?? '';
  const server = createServer((req, res) => {
    const path = decodeURIComponent(new URL(req.url, 'http://localhost').pathname).replace(/^\/+/, '') || 'index.html';
    if (path.includes('..')) { res.writeHead(404); res.end(); return; }
    const file = join(root, path);
    if (!existsSync(file)) { res.writeHead(404); res.end(); return; }
    res.setHeader('Content-Security-Policy', csp);
    res.setHeader('Cache-Control', 'no-store');
    res.setHeader('Content-Type', TYPES[extname(path)] ?? 'application/octet-stream');
    res.end(readFileSync(file));
  });
  await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
  return { origin: `http://127.0.0.1:${server.address().port}`, close: () => new Promise((ok) => server.close(ok)) };
}

// Stand-in for window.chrome.webview, installed before any page script (Playwright init scripts run
// outside the page CSP). It records every posted message with a timestamp and lets the harness
// deliver host messages as objects, as PostWebMessageAsJson does.
const BRIDGE = `(() => {
  const listeners = [];
  const sent = [];
  window.__qaSent = sent;
  window.__qaErrors = [];
  window.addEventListener('error', (e) => window.__qaErrors.push(String(e.message)));
  window.addEventListener('unhandledrejection', (e) => window.__qaErrors.push(String(e.reason)));
  window.__qaDeliver = (msg) => { for (const cb of listeners.slice()) cb({ data: msg }); };
  window.chrome = { webview: {
    addEventListener(type, cb) { if (type === 'message') listeners.push(cb); },
    removeEventListener(type, cb) { const i = listeners.indexOf(cb); if (i >= 0) listeners.splice(i, 1); },
    postMessage(wire) {
      let m = null; try { m = typeof wire === 'string' ? JSON.parse(wire) : wire; } catch { return; }
      if (m && m.t === 'chromeAction') { try { m = Object.assign(JSON.parse(m.text), { __via: 'chromeAction' }); } catch { /* keep */ } }
      sent.push(Object.assign({ __at: performance.now() }, m));
    },
  } };
})();`;

const twoFrames = () => new Promise((ok) => requestAnimationFrame(() => requestAnimationFrame(() => ok())));

/** In-page steps run after the messages are applied (scenario.act). */
export const ACTS = {
  async focusOmnibox(page) { await page.evaluate(() => document.querySelector('input.omni')?.focus()); },
  async openSubmenu(page) {
    // Keyboard path: first row with a submenu, then ArrowRight (as a person would).
    await page.evaluate(() => document.getElementById('app')?.focus());
    const has = await page.$('[aria-haspopup="menu"], [aria-haspopup="true"]');
    if (has) { await has.hover().catch(() => {}); await has.focus().catch(() => {}); await page.keyboard.press('ArrowRight'); }
    await page.waitForTimeout(350);
  },
};

/**
 * Opens one scenario: a fresh context at the given scheme/scale/size, the bridge, the page, the
 * messages. Returns { context, page, sent(), requestedSize }. `size` is [w,h] CSS px.
 */
export async function openScenario(browser, origin, scenario, { scheme = 'dark', dpr = 1, size, reducedMotion = 'no-preference', act = true } = {}) {
  const [width, height] = size ?? scenario.size.standard;
  const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: dpr, colorScheme: scheme, reducedMotion });
  const page = await context.newPage();
  await page.addInitScript(BRIDGE);
  await page.goto(`${origin}/index.html?${scenario.query}`);
  await page.waitForFunction(() => (window.__qaSent || []).some((m) => m.t === 'ready' || m.t === 'chromeReady'), null, { timeout: 15000 });
  const messages = scenario.messages(scheme, width);
  const ready = await page.evaluate(() => window.__qaSent.find((m) => m.t === 'chromeReady') ?? null);
  if (ready) {
    const id = { v: 1, view: ready.view, doc: ready.doc, rev: 1 };
    const frames = [{ ...id, t: 'chromeBegin', count: messages.length }, ...messages.map((state, i) => ({ ...id, t: 'chromePart', i, state })), { ...id, t: 'chromeCommit' }];
    await page.evaluate((all) => { for (const f of all) window.__qaDeliver(f); }, frames);
    await page.waitForFunction(() => window.__qaSent.some((m) => m.t === 'chromeApplied' || m.t === 'chromeFailed'), null, { timeout: 15000 });
    const failed = await page.evaluate(() => window.__qaSent.some((m) => m.t === 'chromeFailed'));
    if (failed) throw new Error(`${scenario.name}: the page rejected its initial bundle (chromeFailed)`);
  } else {
    await page.evaluate((all) => { for (const m of all) window.__qaDeliver(m); }, messages);
  }
  await page.evaluate(async () => { await document.fonts.ready; await new Promise((ok) => requestAnimationFrame(() => requestAnimationFrame(() => ok()))); });
  // Lazy page bundles (settings, history, start page) load with import(): wait for #app to fill.
  await page.waitForFunction(() => (document.getElementById('app')?.childElementCount ?? 0) > 0, null, { timeout: 8000 }).catch(() => {});
  await page.waitForTimeout(120);
  let requestedSize = null;
  if (scenario.fit) {
    requestedSize = await page.evaluate(() => { const s = window.__qaSent.filter((m) => m.t === 'size').at(-1); return s ? [s.w, s.h] : null; });
    if (requestedSize) {
      // A native popup is the size the page asks for, clamped to the work area (here: the scenario size).
      const w = Math.min(requestedSize[0], width), h = Math.min(requestedSize[1], height);
      await page.setViewportSize({ width: Math.max(1, w), height: Math.max(1, h) });
      await page.evaluate(twoFrames);
    }
  }
  if (act && scenario.act && ACTS[scenario.act]) await ACTS[scenario.act](page);
  await page.waitForTimeout(60);
  const errors = await page.evaluate(() => window.__qaErrors.slice());
  return { context, page, requestedSize, errors, sent: () => page.evaluate(() => window.__qaSent.slice()) };
}

// ---------------------------------------------------------------------------------------------
// In-page probes. Each is a plain function passed to page.evaluate (no closures over Node state).

/** Shared helpers, injected as a string and evaluated once per page. */
const HELPERS = `(() => {
  if (window.__qaH) return;
  const cv = document.createElement('canvas'); cv.width = cv.height = 1;
  const cx = cv.getContext('2d', { willReadFrequently: true });
  const rgba = (css) => { cx.clearRect(0, 0, 1, 1); cx.fillStyle = 'rgba(0,0,0,0)'; cx.fillStyle = css; cx.fillRect(0, 0, 1, 1); const d = cx.getImageData(0, 0, 1, 1).data; return [d[0], d[1], d[2], d[3] / 255]; };
  const over = (top, bottom) => { const a = top[3] + bottom[3] * (1 - top[3]); if (a === 0) return [0, 0, 0, 0]; return [0, 1, 2].map((i) => (top[i] * top[3] + bottom[i] * bottom[3] * (1 - top[3])) / a).concat(a); };
  const lum = (c) => { const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4); }; return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2]); };
  const ratio = (a, b) => { const x = lum(a), y = lum(b); return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05); };
  const visible = (el) => {
    if (!el.isConnected) return false;
    const r = el.getBoundingClientRect(); if (r.width < 1 || r.height < 1) return false;
    // Rendering decides, not the attribute: a [hidden] element that an author rule displays is visible.
    for (let n = el; n && n.nodeType === 1; n = n.parentElement) { const s = getComputedStyle(n); if (s.display === 'none' || s.visibility === 'hidden' || s.visibility === 'collapse') return false; if (parseFloat(s.opacity) < 0.02) return false; }
    return true;
  };
  const opacity = (el) => { let o = 1; for (let n = el; n && n.nodeType === 1; n = n.parentElement) o *= parseFloat(getComputedStyle(n).opacity); return o; };
  const sel = (el) => {
    if (!el || el.nodeType !== 1) return String(el);
    const parts = [];
    for (let n = el; n && n.nodeType === 1 && parts.length < 4; n = n.parentElement) {
      let p = n.tagName.toLowerCase();
      if (n.id) { p += '#' + n.id; parts.unshift(p); break; }
      const cls = Array.from(n.classList).filter((c) => !/^is-|^has-/.test(c)).slice(0, 2);
      if (cls.length) p += '.' + cls.join('.');
      const role = n.getAttribute('role'); if (role) p += '[role=' + role + ']';
      parts.unshift(p);
    }
    return parts.join(' > ');
  };
  // Effective background: composite ancestor background-colors until opaque. null when an image,
  // gradient or backdrop is in the way, or when the stack never becomes opaque (the window glass).
  const background = (el) => {
    const layers = [];
    for (let n = el; n && n.nodeType === 1; n = n.parentElement) {
      const s = getComputedStyle(n);
      if (s.backgroundImage && s.backgroundImage !== 'none') return { color: null, why: 'image' };
      const c = rgba(s.backgroundColor);
      if (c[3] > 0) layers.push(c);
      if (c[3] >= 0.999) break;
    }
    let acc = [0, 0, 0, 0];
    for (let i = layers.length - 1; i >= 0; i--) acc = over(layers[i], acc);
    if (acc[3] < 0.999) return { color: null, why: 'translucent', partial: acc };
    return { color: acc };
  };
  const INTERACTIVE = 'button, a[href], input:not([type=hidden]), select, textarea, summary, [role=button], [role=link], [role=menuitem], [role=menuitemcheckbox], [role=menuitemradio], [role=switch], [role=tab], [role=checkbox], [role=radio], [role=option], [role=slider], [role=combobox], [role=treeitem], [tabindex]:not([tabindex="-1"])';
  window.__qaH = { rgba, over, ratio, visible, opacity, sel, background, INTERACTIVE };
})();`;

/** Duplicate ids among all elements. */
function probeIds() {
  const seen = new Map();
  for (const el of document.querySelectorAll('[id]')) seen.set(el.id, (seen.get(el.id) ?? 0) + 1);
  return [...seen].filter(([, n]) => n > 1).map(([id, n]) => ({ id, n }));
}

/** Elements carrying the `hidden` attribute that still render (an author `display` rule beats the UA one). */
function probeHidden() {
  const H = window.__qaH;
  const out = [];
  for (const el of document.querySelectorAll('[hidden]')) {
    if (el.closest('svg') || (el.parentElement && el.parentElement.closest('[hidden]'))) continue;
    const s = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    if (s.display !== 'none' && r.width > 0 && r.height > 0) out.push({ selector: H.sel(el), display: s.display, w: Math.round(r.width), h: Math.round(r.height), name: (el.getAttribute('aria-label') || el.textContent || '').trim().slice(0, 30) });
  }
  return out;
}

/** Radio groups (ARIA or native) on screen with nothing checked: a choice whose current value is not shown. */
function probeState() {
  const H = window.__qaH;
  const out = [];
  for (const g of document.querySelectorAll('[role=radiogroup]')) {
    if (!H.visible(g)) continue;
    const radios = Array.from(g.querySelectorAll('[role=radio]'));
    if (radios.length && !radios.some((r) => r.getAttribute('aria-checked') === 'true')) out.push({ selector: H.sel(g), name: g.getAttribute('aria-label') || '', options: radios.map((r) => r.textContent.trim()).join('/') });
  }
  const named = new Map();
  for (const r of document.querySelectorAll('input[type=radio][name]')) if (H.visible(r)) named.set(r.name, [...(named.get(r.name) ?? []), r]);
  for (const [name, rs] of named) if (!rs.some((r) => r.checked)) out.push({ selector: H.sel(rs[0]), name, options: rs.map((r) => r.value).join('/') });
  return out;
}

/** Text contrast for visible text, against the composited background where determinable. */
function probeContrast() {
  const H = window.__qaH;
  const out = { checked: 0, undetermined: 0, fails: [] };
  const els = new Set();
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let t = walker.nextNode(); t; t = walker.nextNode()) if (t.textContent.trim() && t.parentElement) els.add(t.parentElement);
  for (const inp of document.querySelectorAll('input:not([type=hidden]):not([type=checkbox]):not([type=radio]):not([type=range]):not([type=color]), textarea')) els.add(inp);
  for (const el of els) {
    if (!H.visible(el) || el.closest('svg') || el.matches('input[type=color]')) continue;
    if (el.closest(':disabled, [aria-disabled="true"]')) continue; // WCAG 1.4.3 exempts inactive controls
    const isField = el.matches('input, textarea');
    const pseudo = isField && !el.value && el.placeholder ? '::placeholder' : null;
    if (isField && !el.value && !el.placeholder) continue;
    const s = getComputedStyle(el, pseudo);
    const fg = H.rgba(s.color);
    const alpha = fg[3] * H.opacity(el);
    if (alpha < 0.05) continue;
    const bg = H.background(el);
    out.checked++;
    if (!bg.color && bg.why === 'translucent') {
      // The window material (glass/Mica, or the popup's own translucent overlay) shows through: composite
      // over an approximation of Mica for the scheme and mark the result as assumed.
      const mica = matchMedia('(prefers-color-scheme: dark)').matches ? [32, 32, 32, 1] : [243, 243, 243, 1];
      bg.color = H.over(bg.partial, mica); bg.assumed = true;
    }
    if (!bg.color) { out.undetermined++; continue; }
    if (bg.assumed) out.assumed = (out.assumed ?? 0) + 1;
    const text = [fg[0], fg[1], fg[2], alpha];
    const fgc = H.over(text, bg.color);
    const r = H.ratio(fgc, bg.color);
    const px = parseFloat(s.fontSize), bold = parseInt(s.fontWeight, 10) >= 700;
    const need = px >= 24 || (bold && px >= 18.66) ? 3 : 4.5;
    if (r + 1e-6 < need) {
      const sample = isField ? (el.value || el.placeholder) : Array.from(el.childNodes).filter((n) => n.nodeType === 3).map((n) => n.textContent).join('').trim();
      out.fails.push({ selector: H.sel(el), text: sample.slice(0, 40), ratio: Math.round(r * 100) / 100, need, fg: s.color, bg: `rgb(${bg.color.slice(0, 3).map(Math.round).join(',')})`, pseudo: pseudo ?? undefined, fontSize: px, assumedBackdrop: bg.assumed || undefined });
    }
  }
  return out;
}

/** WCAG 2.2 2.5.8 target size (24x24 CSS px) with the spacing exception. Inline links are exempt. */
function probeTargets() {
  const H = window.__qaH;
  const targets = Array.from(document.querySelectorAll(H.INTERACTIVE)).filter((el) => H.visible(el) && !el.matches(':disabled') && el.getAttribute('aria-disabled') !== 'true' && !el.matches('input[type=text], input[type=search], input:not([type]), textarea'));
  // Nested interactive elements: measure the outermost.
  const outer = targets.filter((el) => !targets.some((o) => o !== el && o.contains(el) && !o.matches('[role=listbox], [role=menu], [role=tablist], [role=tree], [role=grid]') && o.matches('button, a[href], [role=button], [role=link], [role=menuitem], [role=tab], [role=option], [role=treeitem]')));
  const boxes = outer.map((el) => ({ el, r: el.getBoundingClientRect() }));
  const fails = [];
  for (const { el, r } of boxes) {
    if (r.width >= 23.99 && r.height >= 23.99) continue;
    if (el.matches('a[href]') && el.parentElement && Array.from(el.parentElement.childNodes).some((n) => n.nodeType === 3 && n.textContent.trim())) continue;
    const cx = r.left + r.width / 2, cy = r.top + r.height / 2;
    // Spacing exception: a 24px circle on its centre touches no other target or other small target's circle.
    const clash = boxes.find(({ el: o, r: q }) => {
      if (o === el || o.contains(el) || el.contains(o)) return false;
      const dx = Math.max(q.left - cx, 0, cx - q.right), dy = Math.max(q.top - cy, 0, cy - q.bottom);
      if (Math.hypot(dx, dy) < 12) return true;
      if (q.width < 24 || q.height < 24) return Math.hypot(cx - (q.left + q.width / 2), cy - (q.top + q.height / 2)) < 24;
      return false;
    });
    if (!clash) continue;
    fails.push({ selector: H.sel(el), w: Math.round(r.width * 10) / 10, h: Math.round(r.height * 10) / 10, near: H.sel(clash.el), name: (el.getAttribute('aria-label') || el.textContent || '').trim().slice(0, 30) });
  }
  return { checked: boxes.length, fails };
}

/** Content outside the viewport that no scroll container can reach (popups and menus). */
function probeClip() {
  const H = window.__qaH;
  const W = window.innerWidth, Hh = window.innerHeight;
  const fails = [];
  const scrollable = (n) => { const s = getComputedStyle(n); return /(auto|scroll)/.test(s.overflowY + s.overflowX) && (n.scrollHeight > n.clientHeight + 1 || n.scrollWidth > n.clientWidth + 1); };
  for (const el of document.querySelectorAll('#app *')) {
    if (el.closest('svg') && el.tagName.toLowerCase() !== 'svg') continue;
    if (el.childElementCount > 0 && !el.matches(H.INTERACTIVE) && el.tagName.toLowerCase() !== 'svg') continue; // leaves and controls
    if (!H.visible(el)) continue;
    const r = el.getBoundingClientRect();
    const out = r.right > W + 1 || r.bottom > Hh + 1 || r.left < -1 || r.top < -1;
    if (!out) continue;
    let reach = false;
    // Reachable only through a scroller that is itself on screen (a scroller cut by the viewport is not).
    for (let n = el.parentElement; n && n !== document.body; n = n.parentElement) {
      if (!scrollable(n)) continue;
      const q = n.getBoundingClientRect();
      if (q.bottom <= Hh + 1 && q.right <= W + 1 && q.top >= -1 && q.left >= -1) { reach = true; break; }
    }
    if (reach) continue;
    // Clipped by an overflow:hidden ancestor that is itself inside the viewport: visually cut, report too.
    fails.push({ selector: H.sel(el), rect: [r.left, r.top, r.right, r.bottom].map(Math.round), viewport: [W, Hh] });
  }
  const doc = document.scrollingElement;
  return { fails: fails.slice(0, 20), count: fails.length, docOverflow: { x: doc.scrollWidth - W, y: doc.scrollHeight - Hh } };
}

/** Transitions and animations longer than 83 ms on anything but opacity (run under reduced motion). */
function probeMotion() {
  const H = window.__qaH;
  const ms = (v) => v.split(',').map((x) => (x.trim().endsWith('ms') ? parseFloat(x) : parseFloat(x) * 1000));
  const fails = [];
  for (const el of document.querySelectorAll('*')) {
    for (const pseudo of [null, '::before', '::after']) {
      const s = getComputedStyle(el, pseudo);
      const props = s.transitionProperty.split(',').map((p) => p.trim());
      const durs = ms(s.transitionDuration);
      props.forEach((p, i) => {
        const d = durs[i % durs.length];
        if (d > 83.5 && p !== 'opacity' && p !== 'none') fails.push({ kind: 'transition', selector: H.sel(el) + (pseudo ?? ''), property: p, ms: d });
      });
      // Status animations keep moving, slower, under reduced motion on purpose: spinners (components.css
      // .bw-spinner, sidebar.css .side-spin) and the unknown-size download sweep (downloads.css). They
      // carry state, not decoration, so they are not findings.
      const STATUS = ['bw-spin', 'dl-sweep'];
      if (s.animationName !== 'none' && !s.animationName.split(',').every((n) => STATUS.includes(n.trim()))) {
        const ad = ms(s.animationDuration);
        s.animationName.split(',').forEach((n, i) => { const d = ad[i % ad.length]; if (d > 83.5) fails.push({ kind: 'animation', selector: H.sel(el) + (pseudo ?? ''), property: n.trim(), ms: d }); });
      }
    }
  }
  for (const a of document.getAnimations()) {
    const t = a.effect?.getTiming?.(); const d = typeof t?.duration === 'number' ? t.duration : 0;
    const kf = a.effect?.getKeyframes?.() ?? [];
    const props = new Set(kf.flatMap((k) => Object.keys(k).filter((x) => !['offset', 'easing', 'composite', 'computedOffset'].includes(x))));
    if (d > 83.5 && [...props].some((p) => p !== 'opacity') && !(a instanceof CSSTransition) && !(a instanceof CSSAnimation)) fails.push({ kind: 'web-animation', selector: H.sel(a.effect?.target), property: [...props].join('+'), ms: d });
  }
  // Collapse duplicates by selector shape.
  const seen = new Set();
  return fails.filter((f) => { const k = f.kind + f.selector.replace(/:nth.*$/, '') + f.property; if (seen.has(k)) return false; seen.add(k); return true; });
}

/** Snapshot of the visual focus treatment of one element (self, ::before/::after, three ancestors). */
function focusLook(el) {
  const pick = (s) => [s.outlineStyle !== 'none' && parseFloat(s.outlineWidth) > 0 ? `${s.outlineStyle} ${s.outlineWidth} ${s.outlineColor}` : '', s.boxShadow, s.borderTopColor + s.borderBottomColor, s.backgroundColor, s.textDecorationLine, s.opacity].join('|');
  const parts = [pick(getComputedStyle(el)), pick(getComputedStyle(el, '::before')), pick(getComputedStyle(el, '::after'))];
  // :focus-within treatments up to three levels up (the omnibox pill lifts around its field).
  for (let n = el.parentElement, i = 0; n && i < 3; n = n.parentElement, i++) parts.push(pick(getComputedStyle(n)));
  const s = getComputedStyle(el);
  return { look: parts.join('#'), outline: s.outlineStyle !== 'none' && parseFloat(s.outlineWidth) > 0, shadow: s.boxShadow !== 'none' };
}

/**
 * Accessible names from Chromium's own accessibility tree (CDP), for every interactive role.
 * Returns failures with a selector for the DOM node.
 */
async function axNames(page) {
  const cdp = await page.context().newCDPSession(page);
  try {
    await cdp.send('Accessibility.enable');
    const { nodes } = await cdp.send('Accessibility.getFullAXTree');
    const ROLES = new Set(['button', 'link', 'textbox', 'searchbox', 'combobox', 'checkbox', 'radio', 'switch', 'tab', 'menuitem', 'menuitemcheckbox', 'menuitemradio', 'option', 'slider', 'spinbutton', 'treeitem', 'listbox', 'menu', 'dialog', 'img', 'PopUpButton', 'ToggleButton']);
    const fails = [];
    let checked = 0;
    for (const n of nodes) {
      if (n.ignored) continue;
      const role = n.role?.value;
      if (!ROLES.has(role)) continue;
      if (role === 'img' || role === 'menu' || role === 'listbox' || role === 'dialog') {
        // Containers: only report when they are a named landmark-ish element with no name at all.
        if (role === 'img' || n.name?.value) continue;
      }
      checked++;
      if ((n.name?.value ?? '').trim()) continue;
      let selector = '?';
      if (n.backendDOMNodeId) {
        try {
          const { object } = await cdp.send('DOM.resolveNode', { backendNodeId: n.backendDOMNodeId });
          const r = await cdp.send('Runtime.callFunctionOn', { objectId: object.objectId, functionDeclaration: 'function () { return window.__qaH ? window.__qaH.sel(this) : this.tagName; }', returnByValue: true });
          selector = r.result.value;
          const vis = await cdp.send('Runtime.callFunctionOn', { objectId: object.objectId, functionDeclaration: 'function () { return window.__qaH ? window.__qaH.visible(this) : true; }', returnByValue: true });
          if (!vis.result.value) { checked--; continue; }
        } catch { /* keep '?' */ }
      }
      fails.push({ role, selector });
    }
    return { checked, fails };
  } finally { await cdp.detach().catch(() => {}); }
}

/** Keyboard Tab walk: each stop's focus treatment compared focused vs. unfocused. */
async function focusWalk(page, maxStops = 60) {
  await page.evaluate(() => { document.activeElement?.blur?.(); window.focus(); });
  const stops = [];
  let prev = null;
  for (let i = 0; i < maxStops; i++) {
    await page.keyboard.press('Tab');
    await page.waitForTimeout(25);
    // Settle focus transitions (a control revealed on :focus-within fades in over 83 ms).
    await page.evaluate(() => { for (const a of document.getAnimations()) { try { if (a.effect?.getComputedTiming?.().endTime !== Infinity) a.finish(); } catch { /* infinite */ } } });
    const info = await page.evaluate((prevIndex) => {
      const H = window.__qaH;
      const el = document.activeElement;
      window.__qaStops ??= [];
      let prevAfter = null;
      if (prevIndex !== null) { const p = window.__qaStops[prevIndex]; if (p && p.isConnected && p !== el) prevAfter = window.__qaFocusLook(p).look; }
      if (!el || el === document.body || el === document.documentElement) return { body: true, prevAfter };
      if (el.tagName === 'IFRAME') return { body: true, prevAfter, iframe: true }; // focus moved into a frame: its own document
      const idx = window.__qaStops.indexOf(el);
      const r = el.getBoundingClientRect();
      const look = window.__qaFocusLook(el);
      let index = idx;
      if (idx < 0) { window.__qaStops.push(el); index = window.__qaStops.length - 1; }
      return { index, seen: idx >= 0, selector: H.sel(el), name: (el.getAttribute('aria-label') || el.textContent || '').trim().slice(0, 30), focusVisible: el.matches(':focus-visible'), visible: H.visible(el) && r.right > 0 && r.bottom > 0 && r.left < innerWidth && r.top < innerHeight, ...look, prevAfter };
    }, prev === null ? null : prev.index);
    if (prev && info.prevAfter !== null) prev.changes = prev.look !== info.prevAfter;
    if (info.body) { if (stops.length) break; continue; }
    if (info.seen) break;
    stops.push(info);
    prev = info;
  }
  // The last stop: compare against blurred.
  if (prev && prev.changes === undefined) {
    const after = await page.evaluate((i) => { const el = window.__qaStops[i]; el.blur(); return window.__qaFocusLook(el).look; }, prev.index);
    prev.changes = prev.look !== after;
  }
  return stops.map(({ selector, name, focusVisible, visible, outline, shadow, changes }) => ({ selector, name, focusVisible, visible, outline, shadow, changes: !!changes }));
}

/**
 * Runs the automated checks on an opened page. `checks` picks: names, ids, hidden, state, contrast, targets,
 * clip, focus, motion. Returns a plain object (JSON-safe).
 */
export async function auditPage(page, checks = ['names', 'ids', 'hidden', 'state', 'contrast', 'targets']) {
  await page.evaluate(HELPERS);
  await page.evaluate(`window.__qaFocusLook = ${focusLook.toString()};`);
  const out = {};
  if (checks.includes('names')) out.names = await axNames(page);
  if (checks.includes('ids')) out.ids = await page.evaluate(probeIds);
  if (checks.includes('hidden')) out.hidden = await page.evaluate(probeHidden);
  if (checks.includes('state')) out.state = await page.evaluate(probeState);
  if (checks.includes('contrast')) out.contrast = await page.evaluate(probeContrast);
  if (checks.includes('targets')) out.targets = await page.evaluate(probeTargets);
  if (checks.includes('clip')) out.clip = await page.evaluate(probeClip);
  if (checks.includes('motion')) out.motion = await page.evaluate(probeMotion);
  if (checks.includes('focus')) {
    const stops = await focusWalk(page);
    out.focus = { stops: stops.length, fails: stops.filter((s) => !s.changes || !s.visible).map((s) => ({ ...s, why: !s.visible ? 'focus on invisible element' : 'no visible focus change' })) };
  }
  return out;
}
