// Downloads in a fake document: the bowser://downloads page (landmarks, rows, what a click, a key
// or a search sends, live patching in place, Clear all, Undo, every state), the bubble (rows,
// re-posts under one tok patch rather than rebuild, dlact/size/dismiss, the auto-close timer) and
// the toolbar button (shown/hidden, ring modes, the start animation only on a burst change, the
// finish morph).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, '..', 'src');
// The UI's scenarios (every state at once); the wire as Rust writes it is checked at the end too.
const fixture = JSON.parse(readFileSync(join(here, 'fixtures', 'downloads-scenarios.json'), 'utf8'));
const rust = JSON.parse(readFileSync(join(here, 'fixtures', 'downloads-messages.json'), 'utf8'));
async function bundle(file) {
  const out = esbuild.buildSync({ entryPoints: [join(srcDir, file)], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
}

// ---- a small fake DOM ----
function matchOne(node, s) {
  const tag = /^[a-z][a-z0-9]*/.exec(s)?.[0];
  if (tag && node.tagName !== tag) return false;
  for (const cls of s.matchAll(/\.([\w-]+)/g)) if (!node.classList.contains(cls[1])) return false;
  for (const attr of s.matchAll(/\[([\w-]+)(?:="([^"]*)")?\]/g)) {
    const value = attr[1] === 'hidden' ? (node.hidden ? '' : null) : node.getAttribute(attr[1]);
    if (value === null || (attr[2] !== undefined && value !== attr[2])) return false;
  }
  const id = /#([\w-]+)/.exec(s)?.[1];
  if (id && node.id !== id) return false;
  return true;
}
class ClassList {
  constructor(owner) { this.owner = owner; }
  get set() { return new Set(this.owner.className.split(/\s+/).filter(Boolean)); }
  add(...names) { const s = this.set; names.forEach((n) => s.add(n)); this.owner.className = [...s].join(' '); }
  remove(...names) { const s = this.set; names.forEach((n) => s.delete(n)); this.owner.className = [...s].join(' '); }
  contains(n) { return this.set.has(n); }
  toggle(n, force) { const on = force ?? !this.contains(n); if (on) this.add(n); else this.remove(n); return on; }
}
const docListeners = {};
class Node {
  constructor(tag, text = null) {
    this.tagName = tag; this.className = ''; this.attrs = new Map(); this.childList = []; this.parent = null; this.listeners = {};
    this.classList = new ClassList(this); this.dataset = {}; this.hidden = false; this.disabled = false; this.tabIndex = 0; this.value = '';
    this.ownText = text; this.id = ''; this.type = ''; this.title = ''; this.dir = ''; this.scrollTop = 0; this.clientHeight = 800; this.offsetHeight = 56; this.offsetWidth = 100; this.offsetTop = 0;
    const props = new Map();
    this.styleProps = props;
    this.style = { setProperty: (k, v) => props.set(k, String(v)), getPropertyValue: (k) => props.get(k) ?? '', removeProperty: (k) => props.delete(k) };
  }
  get children() { return this.childList.filter((c) => c.tagName !== '#text'); }
  get textContent() { return this.ownText ?? this.childList.map((c) => c.textContent).join(''); }
  set textContent(v) { for (const c of this.childList) c.parent = null; this.childList = []; this.ownText = null; if (v !== '') this.append(new Node('#text', String(v))); }
  get firstChild() { return this.childList[0] ?? null; }
  get lastChild() { return this.childList[this.childList.length - 1] ?? null; }
  get nextSibling() { const p = this.parent; return p ? p.childList[p.childList.indexOf(this) + 1] ?? null : null; }
  get isConnected() { let n = this; while (n.parent) n = n.parent; return n === globalThis.__root || n === globalThis.document.body; }
  setAttribute(k, v) { this.attrs.set(k, String(v)); if (k === 'id') this.id = String(v); if (k === 'class') this.className = String(v); }
  getAttribute(k) {
    if (k === 'id') return this.id || null;
    if (k === 'class') return this.className || null;
    if (k.startsWith('data-')) { const v = this.dataset[k.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase())]; if (v !== undefined) return v; }
    return this.attrs.has(k) ? this.attrs.get(k) : null;
  }
  hasAttribute(k) { return this.getAttribute(k) !== null; }
  removeAttribute(k) { this.attrs.delete(k); }
  toggleAttribute(k, force) { const on = force ?? !this.attrs.has(k); if (on) this.attrs.set(k, ''); else this.attrs.delete(k); return on; }
  addEventListener(type, cb) { (this.listeners[type] ??= []).push(cb); }
  removeEventListener() {}
  dispatch(type, init = {}) {
    let stopped = false;
    const event = { type, target: this, defaultPrevented: false, key: '', ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, button: 0, isComposing: false, keyCode: 0,
      preventDefault() { this.defaultPrevented = true; }, stopPropagation() { stopped = true; }, ...init };
    for (let node = this; node && !stopped; node = node.parent) for (const cb of node.listeners[type] ?? []) cb(event);
    if (!stopped) for (const cb of docListeners[type] ?? []) cb(event);
    return event;
  }
  click(init) { if (!this.disabled) this.dispatch('click', init); }
  detach(node) { if (node.parent) node.parent.childList = node.parent.childList.filter((c) => c !== node); node.parent = this; }
  append(...nodes) { for (const n of nodes) { const node = typeof n === 'string' ? new Node('#text', n) : n; this.ownText = null; this.detach(node); this.childList.push(node); } }
  prepend(...nodes) { for (const n of nodes.reverse()) { this.detach(n); this.childList.unshift(n); } }
  after(node) { const p = this.parent; if (!p) return; p.detach(node); p.childList.splice(p.childList.indexOf(this) + 1, 0, node); }
  insertBefore(node, ref) { this.ownText = null; this.detach(node); const at = ref ? this.childList.indexOf(ref) : -1; if (at < 0) this.childList.push(node); else this.childList.splice(at, 0, node); return node; }
  replaceChildren(...nodes) { for (const c of this.childList) c.parent = null; this.childList = []; this.ownText = null; this.append(...nodes); }
  replaceWith(node) { const p = this.parent; if (!p) return; p.detach(node); p.childList.splice(p.childList.indexOf(this), 1, node); this.parent = null; }
  remove() { if (this.parent) this.parent.childList = this.parent.childList.filter((c) => c !== this); this.parent = null; }
  all() { return this.childList.flatMap((c) => [c, ...c.all()]); }
  matches(s) { return s.split(',').some((x) => matchOne(this, x.trim())); }
  closest(s) { for (let node = this; node; node = node.parent) if (node.tagName !== '#text' && node.matches(s)) return node; return null; }
  querySelectorAll(s) { return this.all().filter((n) => n.tagName !== '#text' && n.matches(s)); }
  querySelector(s) { return this.querySelectorAll(s)[0] ?? null; }
  focus() { globalThis.document.activeElement = this; }
  select() {}
  scrollIntoView() {}
  contains(n) { return n === this || this.all().includes(n); }
  getBoundingClientRect() { return { left: 800, top: 1, right: 828, bottom: 29, width: 28, height: 28 }; }
}
class InputNode extends Node {}
const sent = [];
const listeners = [];
globalThis.document = {
  documentElement: new Node('html'), body: new Node('body'), activeElement: null,
  createElement: (tag) => (tag === 'input' ? new InputNode(tag) : new Node(tag)),
  createElementNS: (_ns, tag) => new Node(tag),
  createTextNode: (text) => new Node('#text', text),
  addEventListener: (type, cb) => { (docListeners[type] ??= []).push(cb); },
  removeEventListener() {},
  getElementById: () => null,
};
let timers = [];
Object.assign(globalThis, {
  window: globalThis,
  matchMedia: () => ({ matches: false }),
  requestAnimationFrame: (cb) => { cb(); return 1; },
  cancelAnimationFrame() {},
  addEventListener() {}, removeEventListener() {},
  getComputedStyle: () => ({}),
  CSS: { escape: (s) => s },
  chrome: { webview: { postMessage: (wire) => sent.push(JSON.parse(wire)), addEventListener: (_t, cb) => listeners.push(cb) } },
});
// Ids are never reused, as in a browser.
let timerId = 0;
globalThis.setTimeout = (cb, ms = 0) => { timers.push({ cb, ms, id: ++timerId }); return timerId; };
globalThis.clearTimeout = (id) => { timers = timers.filter((t) => t.id !== id); };
const flush = (upTo = Infinity) => { const run = timers.filter((t) => t.ms <= upTo); timers = timers.filter((t) => t.ms > upTo); for (const t of run) t.cb(); };
globalThis.Element = Node;
globalThis.HTMLElement = Node;
globalThis.HTMLInputElement = InputNode;
globalThis.HTMLButtonElement = Node;
const deliver = (msg) => { for (const cb of listeners.slice()) cb({ data: JSON.stringify(msg) }); };

// ---- the page ----
const { startDownloadsPage } = await bundle('downloads-page.ts');
const app = new Node('div');
globalThis.__root = app;
startDownloadsPage(app);
deliver({ t: 'state', scale: 1, solid: true, dark: true, back: false, forward: false, loading: false, progress: 0, security: 'none', label: '', theme: {} });
deliver(fixture.snap);
const rowEls = () => app.querySelectorAll('.dl-row');
const rowById = (id) => rowEls().find((row) => row.dataset.id === String(id));
const nameOf = (row) => `${row.querySelector('.dl-stem').textContent}${row.querySelector('.dl-ext').textContent}`;

test('page: landmarks, a labelled section per day, a row per entry, one keyboard stop', () => {
  assert.deepEqual(sent[0], { v: 1, t: 'ready' });
  assert.equal(app.querySelector('main').getAttribute('aria-label'), 'Downloads');
  assert.equal(app.querySelector('h1').textContent, 'Downloads');
  assert.equal(app.querySelector('.dp-search-input').getAttribute('aria-label'), 'Search downloads');
  assert.ok(app.querySelector('[role="search"]'));
  const sections = app.querySelectorAll('section');
  assert.equal(sections.length, 2);
  for (const section of sections) assert.ok(app.querySelector(`#${section.getAttribute('aria-labelledby')}`));
  assert.equal(rowEls().length, 8);
  assert.equal(nameOf(rowById(4)), 'Report "<b>" v3.pdf', 'names are text');
  assert.equal(rowById(7).querySelector('.dl-status').textContent, '18.0 MB/s • 12.0 MB of 286 MB, 14 sec left');
  assert.equal(rowById(7).querySelector('.dl-host').textContent, 'static.rust-lang.org');
  assert.equal(rowById(3).querySelector('.dl-status').dataset.tone, 'warn');
  assert.equal(rowById(7).dataset.kind, 'archive');
  assert.equal(rowById(6).classList.contains('is-unknown'), true, 'no size: the bar sweeps');
  assert.equal(app.querySelector('.dp-summary').textContent, '8 downloads • 3 in progress');
  const stops = app.querySelectorAll('.dl-open').filter((b) => b.tabIndex === 0);
  assert.equal(stops.length, 1);
  for (const button of app.querySelectorAll('.dl-act')) assert.ok(button.getAttribute('aria-label'), 'icon buttons are named');
  assert.equal(rowById(1).querySelector('.dl-act[data-act="copy"]'), null, 'no address, no Copy link');
});

test('page: a click opens a finished file only; buttons send their action once', () => {
  sent.length = 0;
  rowById(4).querySelector('.dl-open').click();
  rowById(7).querySelector('.dl-open').click();
  rowById(7).querySelector('.dl-act[data-act="pause"]').click();
  rowById(7).querySelector('.dl-act[data-act="pause"]').click();
  rowById(5).querySelector('.dl-act[data-act="resume"]').click();
  rowById(3).querySelector('.dl-act[data-act="resume"]').click();
  rowById(2).querySelector('.dl-act[data-act="retry"]').click();
  rowById(4).querySelector('.dl-act[data-act="show"]').click();
  rowById(4).querySelector('.dl-act[data-act="copy"]').click();
  app.querySelector('.dp-folder').click();
  app.querySelector('.dp-settings').click();
  assert.deepEqual(sent, [
    { v: 1, t: 'dpage', a: 'open', id: 4 },
    { v: 1, t: 'dpage', a: 'pause', id: 7 },
    { v: 1, t: 'dpage', a: 'resume', id: 5 },
    { v: 1, t: 'dpage', a: 'resume', id: 3 },
    { v: 1, t: 'dpage', a: 'retry', id: 2 },
    { v: 1, t: 'dpage', a: 'show', id: 4 },
    { v: 1, t: 'dpage', a: 'copy', id: 4 },
    { v: 1, t: 'dpage', a: 'folder' },
    { v: 1, t: 'dpage', a: 'settings' },
  ]);
});

test('page: live progress patches rows in place, never rebuilding them', () => {
  const before = rowById(7);
  const fill = before.querySelector('.dl-fill');
  const zip = fixture.snap.d.rows[1];
  const rows = fixture.snap.d.rows.map((r) => (r.id === 7 ? { ...zip, got: zip.total / 2, eta: 7 } : r));
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, rows } });
  assert.equal(rowById(7), before, 'same element');
  assert.equal(rowById(7).querySelector('.dl-fill'), fill);
  assert.equal(fill.style.getPropertyValue('--dl-f'), '0.5');
  assert.match(before.querySelector('.dl-status').textContent, /143 MB of 286 MB, 7 sec left$/);
  // It finishes: same row, new state, actions swap, the line crossfades.
  const done = fixture.snap.d.rows.map((r) => (r.id === 7 ? { ...zip, state: 'done', got: zip.total, eta: -1, bps: 0 } : r));
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, rows: done, active: 2 } });
  assert.equal(rowById(7), before);
  assert.equal(before.dataset.state, 'done');
  assert.equal(before.querySelector('.dl-status').textContent, '286 MB');
  assert.ok(before.querySelector('.dl-status').classList.contains('is-swap'));
  assert.ok(before.querySelector('.dl-act[data-act="show"]'));
  assert.match(app.querySelector('.dp-sr').textContent, /rust-1\.82\.0\.zip finished downloading/);
  deliver(fixture.snap);
});

test('page: keys move, Enter opens, Delete removes, the toast offers Undo', () => {
  const pdf = rowById(4).querySelector('.dl-open');
  pdf.focus();
  pdf.dispatch('keydown', { key: 'ArrowDown' });
  assert.equal(document.activeElement, rowById(3).querySelector('.dl-open'));
  rowById(3).querySelector('.dl-open').dispatch('keydown', { key: 'ArrowUp' });
  assert.equal(document.activeElement, pdf);
  sent.length = 0;
  pdf.dispatch('keydown', { key: 'Enter' });
  rowById(7).querySelector('.dl-open').dispatch('keydown', { key: 'Delete' });
  pdf.dispatch('keydown', { key: 'Delete' });
  assert.deepEqual(sent, [{ v: 1, t: 'dpage', a: 'open', id: 4 }, { v: 1, t: 'dpage', a: 'remove', id: 4 }], 'a running download is not removed');
  deliver(fixture.removed);
  assert.equal(rowById(4), undefined);
  assert.equal(document.activeElement, rowById(3).querySelector('.dl-open'), 'the next row takes the focus');
  assert.equal(app.querySelector('.dp-toast-text').textContent, 'Removed “Report "<b>" v3.pdf” from the list');
  const undo = app.querySelector('.dp-toast-undo');
  assert.equal(undo.hidden, false);
  sent.length = 0;
  undo.click();
  assert.deepEqual(sent, [{ v: 1, t: 'dpage', a: 'undo' }]);
  deliver(fixture.snap);
  assert.equal(rowEls().length, 8);
  // Left and Right step through the row's buttons.
  const exe = rowById(3).querySelector('.dl-open');
  exe.focus();
  exe.dispatch('keydown', { key: 'ArrowRight' });
  assert.equal(document.activeElement.classList.contains('dl-act'), true);
});

test('page: Clear all asks in the page first; Escape backs out', () => {
  const clear = app.querySelector('.dp-clear');
  sent.length = 0;
  clear.click();
  assert.deepEqual(sent, [], 'the first press only asks');
  assert.equal(clear.classList.contains('is-danger'), true);
  assert.equal(app.querySelector('.dp-clear-cancel').hidden, false);
  document.activeElement = clear;
  clear.dispatch('keydown', { key: 'Escape' });
  assert.equal(clear.classList.contains('is-danger'), false);
  clear.click();
  clear.click();
  assert.deepEqual(sent, [{ v: 1, t: 'dpage', a: 'clear' }]);
  deliver(fixture.cleared);
  assert.match(app.querySelector('.dp-toast-text').textContent, /Cleared/);
  deliver(fixture.snap);
});

test('page: search is debounced, Escape clears it, and every empty state', () => {
  const input = app.querySelector('.dp-search-input');
  input.focus();
  sent.length = 0;
  input.value = 'ru';
  input.dispatch('input');
  input.value = 'rust';
  input.dispatch('input');
  flush();
  assert.deepEqual(sent.filter((m) => m.a === 'search'), [{ v: 1, t: 'dpage', a: 'search', text: 'rust' }]);
  deliver(fixture.search);
  assert.equal(rowEls().length, 1);
  assert.ok(app.querySelector('mark'), 'hits are marked');
  sent.length = 0;
  input.dispatch('keydown', { key: 'Escape' });
  assert.deepEqual(sent, [{ v: 1, t: 'dpage', a: 'search', text: '' }]);
  deliver(fixture.none);
  assert.equal(app.querySelector('.dp-empty-title').textContent, 'No matches for “zzz”');
  deliver(fixture.empty);
  assert.equal(app.querySelector('.dp-empty-title').textContent, 'No downloads yet');
  assert.equal(app.querySelector('.dp-clear').disabled, true);
  deliver(fixture.error);
  assert.equal(app.querySelector('.dp-empty-title').textContent, 'Downloads couldn’t be read');
  assert.equal(app.querySelector('.dp-search').hidden, true);
  sent.length = 0;
  app.querySelector('.dp-empty').querySelector('.bw-button').click();
  assert.deepEqual(sent, [{ v: 1, t: 'dpage', a: 'refresh' }]);
  deliver(fixture.guest);
  assert.equal(rowEls().length, 1);
  assert.equal(app.querySelector('.dp-note').hidden, false, 'a guest window says its list is forgotten');
  deliver({ v: 1, t: 'snap', p: 'downloadspage', d: { ...fixture.snap.d, extra: true } });
  assert.equal(rowEls().length, 1, 'a snapshot that breaks the contract changes nothing');
});

// ---- the bubble ----
const { createDownloadsView } = await bundle('downloads-bubble.ts');
const popupRoot = new Node('div');
globalThis.__root = popupRoot;
const bubble = createDownloadsView(popupRoot);
const brow = (id) => popupRoot.querySelectorAll('.dl-row').find((row) => row.dataset.id === String(id));

test('bubble: rows, header, footer, natural size; a re-post patches in place', () => {
  sent.length = 0;
  timers = [];
  assert.equal(bubble.show({ ...fixture.bubble, extra: 1 }), false);
  assert.equal(bubble.show(fixture.bubble), true);
  assert.equal(popupRoot.className, 'popup-pages');
  const page = popupRoot.querySelector('.bw-dl');
  assert.equal(page.getAttribute('role'), 'dialog');
  assert.equal(popupRoot.querySelectorAll('.dl-row').length, 5);
  assert.equal(popupRoot.querySelector('.db-sub').textContent, '3 in progress');
  assert.equal(popupRoot.querySelector('.db-more').textContent, '+3 more');
  assert.equal(brow(7).querySelector('.dl-status').textContent, '12.0 MB of 286 MB, 14 sec left');
  assert.deepEqual(sent.find((m) => m.t === 'size'), { v: 1, t: 'size', tok: 41, w: 360, h: 56 });
  const row = brow(7);
  const items = fixture.bubble.items.map((e) => (e.id === 7 ? { ...e, got: e.total / 4, eta: 9 } : e));
  bubble.show({ ...fixture.bubble, items });
  assert.equal(brow(7), row, 'the same row element');
  assert.equal(row.querySelector('.dl-fill').style.getPropertyValue('--dl-f'), '0.25');
  assert.match(row.querySelector('.dl-status').textContent, /71\.5 MB of 286 MB, 9 sec left/);
  // A row that leaves the list goes; the rest stay.
  const exe = brow(3);
  bubble.show({ ...fixture.bubble, items: fixture.bubble.items.filter((e) => e.id !== 3) });
  assert.equal(brow(7), row);
  assert.ok(exe.classList.contains('is-leaving'), 'it fades where it was');
  flush();
  assert.equal(exe.parent, null);
});

test('bubble: buttons and the footer send dlact; Escape dismisses', () => {
  bubble.show(fixture.bubble);
  sent.length = 0;
  brow(7).querySelector('.dl-act[data-act="pause"]').click();
  brow(7).querySelector('.dl-act[data-act="pause"]').click();
  brow(5).querySelector('.dl-act[data-act="resume"]').click();
  brow(4).querySelector('.dl-open').click();
  brow(7).querySelector('.dl-open').click();
  popupRoot.querySelector('.db-foot').click();
  popupRoot.querySelector('.db-folder').click();
  brow(4).querySelector('.dl-open').dispatch('keydown', { key: 'Delete' });
  assert.deepEqual(sent, [
    { v: 1, t: 'dlact', tok: 41, id: 7, n: 0 },
    { v: 1, t: 'dlact', tok: 41, id: 5, n: 1 },
    { v: 1, t: 'dlact', tok: 41, id: 4, n: 3 },
    { v: 1, t: 'dlact', tok: 41, id: 0, n: 8 },
    { v: 1, t: 'dlact', tok: 41, id: 0, n: 9 },
    { v: 1, t: 'dlact', tok: 41, id: 4, n: 5 },
  ]);
  // Arrow keys walk the rows and the footer.
  brow(7).querySelector('.dl-open').focus();
  brow(7).querySelector('.dl-open').dispatch('keydown', { key: 'ArrowDown' });
  assert.equal(document.activeElement, brow(6).querySelector('.dl-open'));
  brow(6).querySelector('.dl-open').dispatch('keydown', { key: 'End' });
  assert.equal(document.activeElement, popupRoot.querySelector('.db-foot'));
  sent.length = 0;
  popupRoot.querySelector('.db-foot').dispatch('keydown', { key: 'Escape' });
  assert.deepEqual(sent, [{ v: 1, t: 'dismiss', tok: 41 }]);
});

test('bubble: an auto-opened bubble closes itself unless the pointer is on it', () => {
  sent.length = 0;
  timers = [];
  bubble.show(fixture.bubbleAuto);
  assert.notEqual(document.activeElement, popupRoot.querySelector('.bw-dl'), 'it never takes the keyboard');
  flush(6000);
  flush(100);
  assert.deepEqual(sent.filter((m) => m.t === 'dismiss'), [{ v: 1, t: 'dismiss', tok: 42 }]);
  // A fresh opening: the pointer rests on it, so it stays; leaving arms a shorter timer.
  sent.length = 0;
  timers = [];
  bubble.show({ ...fixture.bubbleAuto, tok: 44 });
  const page = popupRoot.querySelector('.bw-dl');
  page.dispatch('pointerenter');
  flush(6000);
  flush(100);
  assert.deepEqual(sent.filter((m) => m.t === 'dismiss'), []);
  page.dispatch('pointerleave');
  flush(3000);
  flush(100);
  assert.deepEqual(sent.filter((m) => m.t === 'dismiss'), [{ v: 1, t: 'dismiss', tok: 44 }]);
  // A user-opened bubble never closes on a timer.
  sent.length = 0;
  timers = [];
  bubble.show({ ...fixture.bubble, tok: 45 });
  flush();
  assert.deepEqual(sent.filter((m) => m.t === 'dismiss'), []);
  bubble.show(fixture.bubbleEmpty);
  assert.equal(popupRoot.querySelector('.db-empty').hidden, false);
});

// ---- the toolbar button ----
const { buildView } = await bundle('view.ts');
const bar = new Node('div');
globalThis.__root = bar;
const view = buildView(bar);
const button = view.downloads.button;

test('toolbar: hidden until on; ring modes; the start animation only on a burst change', () => {
  assert.equal(button.hidden, true);
  assert.equal(button.parent, bar.querySelector('.actions'));
  assert.equal(button.nextSibling, view.menu, 'between the pill and the menu');
  view.downloads.apply({ t: 'dlbtn', ...fixture.dlbtnActive });
  assert.equal(button.hidden, false);
  assert.ok(bar.classList.contains('has-dl'));
  assert.equal(button.dataset.ring, 'progress');
  assert.equal(button.querySelector('.tb-dl-arc').style.getPropertyValue('stroke-dashoffset'), '55');
  assert.equal(button.classList.contains('is-burst'), false, 'never on the first message');
  assert.equal(button.getAttribute('aria-label'), 'Downloads, 2 downloads in progress, 45%');
  view.downloads.apply({ t: 'dlbtn', ...fixture.dlbtnActive, pct: 50 });
  assert.equal(button.classList.contains('is-burst'), false);
  view.downloads.apply({ t: 'dlbtn', ...fixture.dlbtnSweep });
  assert.equal(button.classList.contains('is-burst'), true, 'burst 3 -> 4');
  assert.equal(button.dataset.ring, 'sweep');
  view.downloads.apply({ t: 'dlbtn', ...fixture.dlbtnPaused });
  assert.equal(button.dataset.ring, 'paused');
  sent.length = 0;
  button.click();
});

test('toolbar: finishing morphs, then rests unseen; failed tone; hiding fades first', () => {
  timers = [];
  view.downloads.apply({ t: 'dlbtn', ...fixture.dlbtnDone });
  assert.ok(button.classList.contains('is-finish'));
  assert.equal(button.dataset.ring, 'progress', 'the ring closes');
  assert.equal(button.querySelector('.tb-dl-arc').style.getPropertyValue('stroke-dashoffset'), '0');
  flush();
  assert.equal(button.classList.contains('is-finish'), false);
  assert.equal(button.dataset.ring, 'none');
  assert.equal(button.dataset.state, 'done');
  view.downloads.apply({ t: 'dlbtn', ...fixture.dlbtnFailed });
  assert.equal(button.dataset.state, 'failed');
  assert.equal(button.classList.contains('is-finish'), false, 'only a running download finishing morphs');
  view.downloads.apply({ t: 'dlbtn', ...fixture.dlbtnHidden });
  assert.ok(button.classList.contains('is-out'));
  assert.equal(button.hidden, false, 'it fades before it goes');
  flush();
  assert.equal(button.hidden, true);
  assert.equal(bar.classList.contains('has-dl'), false);
});

test('every message Rust writes renders: page rows, bubble rows, the button', () => {
  for (const [name, message] of Object.entries(rust)) {
    if (message.t === 'downloads') {
      assert.equal(bubble.show(message), true, name);
      assert.equal(popupRoot.querySelectorAll('.dl-row').filter((row) => !row.classList.contains('is-leaving')).length, message.items.length, name);
    } else if (message.t === 'snap') {
      deliver(message);
      const count = message.d.rows.filter((row) => 'id' in row).length;
      assert.equal(rowEls().length, count, name);
    } else {
      view.downloads.apply({ ...message });
      assert.equal(button.hidden || button.classList.contains('is-out'), !message.on, name);
    }
  }
});
