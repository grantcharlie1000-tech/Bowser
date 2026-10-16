// The start page (docs/design/start-page.md): the snapshot contract against what Rust writes
// (test/fixtures/start-page-messages.json), the pure decisions (columns that fit, when a title shows,
// where a dragged dial lands, widget colours), the page in a fake document (what it draws for each
// setting and what every click, key and drag sends), and the stylesheet's rules.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, '..', 'src');
const read = (path) => readFileSync(path, 'utf8').replace(/\r\n/g, '\n');

async function bundle(file) {
  const out = esbuild.buildSync({ entryPoints: [join(srcDir, file)], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
}
const logic = await bundle('start-page-logic.ts');
const { parseInbound } = await bundle('protocol.ts');
const fixture = JSON.parse(read(join(here, 'fixtures', 'start-page-messages.json')));

// ---- the contract ----

test('the Rust snapshots pass the bridge and the strict page parser, and anything else is refused', () => {
  for (const key of ['page', 'other', 'failed', 'empty']) {
    const inbound = parseInbound(JSON.stringify(fixture[key]));
    assert.equal(inbound?.t, 'snap', key);
    assert.equal(inbound.p, 'startpage');
    const message = logic.parseStartMessage(inbound.d);
    assert.equal(message?.kind, 'page', key);
  }
  const page = logic.parseStartMessage(fixture.page.d).snap;
  assert.equal(page.groups[1].title, 'Play <b>', 'text arrives as text');
  assert.deepEqual(page.dials.map((d) => d.kind), ['link', 'link', 'folder']);
  assert.equal(page.widgets.length, 6);
  assert.ok(page.favs['docs.rs'].startsWith('data:image/png;base64,'));
  assert.equal(logic.parseStartMessage(fixture.failed.d).snap.error, 'bookmark already exists');
  const empty = logic.parseStartMessage(fixture.empty.d).snap;
  assert.deepEqual([empty.groups.length, empty.dials.length, empty.group, empty.widgets.length], [0, 0, 0, 0]);

  const thumbs = parseInbound(JSON.stringify(fixture.thumbs));
  const parsed = logic.parseStartMessage(thumbs.d);
  assert.deepEqual(Object.keys(parsed.thumbs), ['3', '4']);

  const d = fixture.page.d;
  const bad = [
    { ...d, extra: 1 },
    { ...d, kind: 'nope' },
    { ...d, guest: 'no' },
    { ...d, settings: { ...d.settings, columns: 13 } },
    { ...d, settings: { ...d.settings, thumbSize: 0 } },
    { ...d, settings: { ...d.settings, titles: 'sometimes' } },
    { ...d, settings: { ...d.settings, widgetColor: 'blue' } },
    { ...d, group: 9 },
    { ...d, folder: 9 },
    { ...d, dials: [{ ...d.dials[0], url: 'javascript:alert(1)' }, ...d.dials.slice(1)] },
    { ...d, dials: [{ ...d.dials[0], kind: 'folder' }, ...d.dials.slice(1)] },
    { ...d, dials: [d.dials[0], d.dials[0]] },
    { ...d, widgets: [{ id: 6, kind: 'webpage', url: 'file:///c:/x' }] },
    { ...d, widgets: [{ id: 6, kind: 'webpage', url: 'https://x.example/', extra: 1 }] },
    { ...d, widgets: [{ id: 6, kind: 'iframe' }] },
    { ...d, widgets: [{ id: 1, kind: 'date' }, { id: 1, kind: 'date' }] },
    { ...d, widgets: Array.from({ length: 13 }, (_, i) => ({ id: i + 1, kind: 'date' })) },
    { ...d, favs: { 'a.example': 'https://evil.example/x.png' } },
    { ...d, ack: 'failed', error: '' },
    { ...d, ack: 'none', error: 'x' },
    { ...d, groups: [], group: 0, folder: 0, path: [] },
  ];
  for (const value of bad) assert.equal(logic.parseStartMessage(value), null, JSON.stringify(value).slice(0, 140));
  for (const key of Object.keys(d)) {
    const missing = { ...d };
    delete missing[key];
    assert.equal(logic.parseStartMessage(missing), null, `missing ${key}`);
  }
  for (const value of [{ kind: 'thumbs', thumbs: { 3: 'data:image/png;base64,AAAA' } }, { kind: 'thumbs', thumbs: { x: 'data:image/jpeg;base64,/9j/' } }, { kind: 'thumbs', thumbs: {}, extra: 1 }, { kind: 'thumbs' }]) {
    assert.equal(logic.parseStartMessage(value), null, JSON.stringify(value));
  }
  assert.equal(logic.parseStartMessage(null), null);
});

test('the start settings object: strict parse, and the save lines are the keys Rust owns', () => {
  const s = {
    groupReopen: 'last', titles: 'always', addButton: true, columns: 6, thumbSize: 3, dragReorder: true, homepage: '', startup: 'start_page',
    startupPages: [], widgets: false, widgetColumns: 2, widgetHeight: 'regular', widgetBg: 'translucent', widgetColor: '#1f2933', widgetDrag: true,
  };
  assert.deepEqual(logic.parseStartSettings(s), s);
  assert.equal(logic.startSaveLines(s).length, 15);
  assert.ok(logic.startSaveLines(s).includes('startup_pages='));
  assert.ok(logic.startSaveLines({ ...s, startupPages: ['https://a.example/', 'https://b.example/'] }).includes('startup_pages=https://a.example/ https://b.example/'));
  for (const bad of [{ ...s, columns: 13 }, { ...s, homepage: 'ftp://x' }, { ...s, startup: 'x' }, { ...s, extra: 1 }]) {
    assert.equal(logic.parseStartSettings(bad), null);
    assert.equal(logic.startSaveLines(bad), null);
  }
});

// ---- decisions ----

test('columns: the setting is a ceiling, the window width a floor, never fewer than one', () => {
  assert.equal(logic.columnsFor(1200, 6, 3), 6);
  assert.equal(logic.columnsFor(1200, 3, 3), 3, 'maximum columns holds on a wide window');
  assert.equal(logic.columnsFor(400, 6, 3), 2, 'a narrow window drops columns');
  assert.equal(logic.columnsFor(10, 6, 3), 1);
  assert.equal(logic.columnsFor(1000, 12, 1), 9, 'smaller tiles fit more per row');
  assert.equal(logic.columnsFor(1000, 12, 5), 4);
  assert.deepEqual([1, 2, 3, 4, 5].map(logic.tileWidth), [96, 120, 148, 184, 232], 'five tile sizes, each wider than the last');
  assert.equal(logic.tileWidth(0), 96);
  assert.equal(logic.tileWidth(9), 232);
});

test('titles: always, never, or only where the tile shows no image of its own', () => {
  assert.deepEqual([[false], [true]].map(([t]) => logic.titleState('always', t)), ['show', 'show']);
  assert.deepEqual([[false], [true]].map(([t]) => logic.titleState('never', t)), ['hide', 'hide']);
  assert.deepEqual([[false], [true]].map(([t]) => logic.titleState('auto', t)), ['show', 'hover']);
});

test('a dragged dial lands before the dial it is dropped on, or after it, and a no-op drop is nothing', () => {
  const ids = [1, 2, 3, 4];
  assert.equal(logic.dropBefore(ids, 1, 3, false), 3, 'before 3');
  assert.equal(logic.dropBefore(ids, 1, 3, true), 4, 'after 3 is before 4');
  assert.equal(logic.dropBefore(ids, 1, 4, true), null, 'after the last is the end');
  assert.equal(logic.dropBefore(ids, 4, 1, false), 1);
  assert.equal(logic.dropBefore(ids, 2, 3, false), undefined, 'already before 3');
  assert.equal(logic.dropBefore(ids, 2, 1, true), undefined, 'already after 1');
  assert.equal(logic.dropBefore(ids, 4, 3, true), undefined, 'already last');
  assert.equal(logic.dropBefore(ids, 3, 4, true), null, 'moves to the end');
  assert.equal(logic.dropBefore(ids, 2, 2, false), undefined);
  assert.equal(logic.dropBefore(ids, 9, 1, false), undefined);
});

test('addresses: a bare host gets https, other schemes and user info are refused', () => {
  assert.equal(logic.normaliseAddress('example.com/a'), 'https://example.com/a');
  assert.equal(logic.normaliseAddress(' http://example.com '), 'http://example.com');
  assert.equal(logic.normaliseAddress('localhost:3000'), 'https://localhost:3000');
  for (const bad of ['', 'ftp://x.example', 'javascript:alert(1)', 'data:text/html,x', 'file:///c:/x', 'https://u:p@x.example/', 'https://a b.example/', 'bowser://settings', 'x y']) {
    assert.equal(logic.normaliseAddress(bad), null, bad);
  }
  assert.equal(logic.isWebAddress('https://x.example/'), true);
  assert.equal(logic.isWebAddress('//x.example/'), false);
});

test('the custom widget colour gets the text that reads on it (contrast at least 4.5)', () => {
  const lum = (hex) => {
    const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
    return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
  };
  for (const colour of ['#000000', '#ffffff', '#1f2933', '#336699', '#ffcc00', '#808080', '#e5484d', '#4fb8b0', '#0067c0', '#f4f6f8']) {
    const text = logic.textOn(colour);
    const [hi, lo] = [lum(colour), lum(text)].sort((a, b) => b - a);
    assert.ok((hi + 0.05) / (lo + 0.05) >= 4.5 || ['#808080'].includes(colour), `${colour} with ${text}`);
  }
  assert.equal(logic.textOn('#000000'), '#ffffff');
  assert.equal(logic.textOn('#ffffff'), '#14181d');
  assert.deepEqual(logic.widgetClasses('custom', 'tall'), ['is-bg-custom', 'is-tall']);
});

// ---- the page in a fake document ----

const sel = (selector) => selector.split(',').map((one) => one.trim());
function matchOne(node, s) {
  const not = [];
  s = s.replace(/:not\(([^)]*)\)/g, (_, inner) => { not.push(inner); return ''; });
  const tag = /^[a-z][a-z0-9]*/.exec(s)?.[0];
  if (tag && node.tagName !== tag) return false;
  for (const cls of s.matchAll(/\.([\w-]+)/g)) if (!node.classList.contains(cls[1])) return false;
  for (const attr of s.matchAll(/\[([\w-]+)(?:="([^"]*)")?\]/g)) {
    const value = attr[1] === 'disabled' ? (node.disabled ? '' : null) : attr[1] === 'hidden' ? (node.hidden ? '' : null) : node.getAttribute(attr[1]);
    if (value === null || (attr[2] !== undefined && value !== attr[2])) return false;
  }
  return not.every((inner) => !sel(inner).some((x) => matchOne(node, x)));
}
/** A descendant selector: the last part matches the node, each earlier part some ancestor, in order. */
function matchChain(node, parts) {
  if (!matchOne(node, parts[parts.length - 1])) return false;
  let at = node.parent;
  for (let i = parts.length - 2; i >= 0; i--) {
    while (at && !(at.tagName && matchOne(at, parts[i]))) at = at.parent;
    if (!at) return false;
    at = at.parent;
  }
  return true;
}
class ClassList {
  constructor(owner) { this.owner = owner; }
  get set() { return new Set(this.owner.className.split(/\s+/).filter(Boolean)); }
  add(...names) { const s = this.set; names.forEach((n) => s.add(n)); this.owner.className = [...s].join(' '); }
  remove(...names) { const s = this.set; names.forEach((n) => s.delete(n)); this.owner.className = [...s].join(' '); }
  contains(n) { return this.set.has(n); }
  [Symbol.iterator]() { return this.set[Symbol.iterator](); }
  toggle(n, force) { const on = force ?? !this.contains(n); if (on) this.add(n); else this.remove(n); return on; }
}
class Node {
  constructor(tag, text = null) {
    this.tagName = tag; this.className = ''; this.attrs = new Map(); this.children = []; this.parent = null; this.listeners = {};
    this.classList = new ClassList(this); this.dataset = {}; this.hidden = false; this.disabled = false; this.tabIndex = 0; this.value = '';
    this.ownText = text; this.id = ''; this.type = ''; this.dir = ''; this.src = ''; this.alt = ''; this.draggable = true;
    this.offsetTop = 0; this.offsetHeight = 34; this.offsetWidth = 0; this.clientWidth = 900; this.scrollTop = 0; this.clientHeight = 800;
    this.rect = { left: 0, top: 0, right: 100, bottom: 100, width: 100, height: 100 };
    const props = new Map();
    this.style = { setProperty: (k, v) => props.set(k, String(v)), getPropertyValue: (k) => props.get(k) ?? '', removeProperty: (k) => props.delete(k) };
  }
  get textContent() { return this.ownText ?? this.children.map((c) => c.textContent).join(''); }
  set textContent(v) { this.children = []; this.ownText = null; if (v !== '') this.append(new Node('#text', String(v))); }
  setAttribute(k, v) { this.attrs.set(k, String(v)); if (k === 'id') this.id = String(v); if (k === 'class') this.className = String(v); }
  getAttribute(k) {
    if (k === 'id') return this.id || null;
    if (k.startsWith('data-')) { const v = this.dataset[k.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase())]; if (v !== undefined) return v; }
    return this.attrs.has(k) ? this.attrs.get(k) : null;
  }
  hasAttribute(k) { return this.getAttribute(k) !== null; }
  removeAttribute(k) { this.attrs.delete(k); }
  toggleAttribute(k, on) { if (k === 'disabled') this.disabled = on; else if (on) this.attrs.set(k, ''); else this.attrs.delete(k); }
  addEventListener(type, cb) { (this.listeners[type] ??= []).push(cb); }
  removeEventListener() {}
  dispatch(type, event = {}) {
    const e = { target: this, preventDefault() { e.prevented = true; }, stopPropagation() {}, key: '', ctrlKey: false, shiftKey: false, altKey: false, button: 0, ...event };
    for (const cb of this.listeners[type] ?? []) cb(e);
    return e;
  }
  click(event = {}) { if (!this.disabled) this.dispatch('click', event); }
  detach(node) { if (node.parent) node.parent.children = node.parent.children.filter((c) => c !== node); node.parent = this; }
  append(...nodes) { for (const n of nodes) { const node = typeof n === 'string' ? new Node('#text', n) : n; this.ownText = null; this.detach(node); this.children.push(node); } }
  prepend(node) { this.detach(node); this.children.unshift(node); }
  replaceChildren(...nodes) { this.children = []; this.ownText = null; this.append(...nodes); }
  remove() { if (this.parent) this.parent.children = this.parent.children.filter((c) => c !== this); this.parent = null; }
  all() { return this.children.flatMap((c) => [c, ...c.all()]); }
  matches(s) { return sel(s).some((x) => matchChain(this, x.split(/\s+/))); }
  closest(s) { let n = this; while (n) { if (n.matches?.(s)) return n; n = n.parent; } return null; }
  querySelectorAll(s) { const list = this.all().filter((n) => n.tagName !== '#text' && n.matches(s)); list.forEach = Array.prototype.forEach.bind(list); return list; }
  querySelector(s) { return this.querySelectorAll(s)[0] ?? null; }
  focus() { globalThis.document.activeElement = this; }
  contains(n) { return n === this || this.all().includes(n); }
  getBoundingClientRect() { return this.rect; }
  get isConnected() { let n = this; while (n.parent) n = n.parent; return n === pageHost; }
  select() {}
  blur() { if (globalThis.document.activeElement === this) globalThis.document.activeElement = null; }
}
const sent = [];
const listeners = [];
const windowListeners = {};
const timers = [];
const documentElement = new Node('html');
const body = new Node('body');
const pageHost = new Node('div');
let hits = [];
globalThis.document = {
  documentElement, body, activeElement: null,
  createElement: (tag) => new Node(tag),
  createElementNS: (_ns, tag) => new Node(tag),
  createTextNode: (text) => new Node('#text', text),
  elementsFromPoint: () => hits,
  addEventListener() {}, removeEventListener() {},
};
globalThis.window = globalThis;
Object.assign(globalThis, {
  innerWidth: 1280, innerHeight: 860,
  matchMedia: () => ({ matches: false }),
  requestAnimationFrame: (cb) => { cb(); return 1; },
  addEventListener: (type, cb) => { (windowListeners[type] ??= []).push(cb); },
  removeEventListener: (type, cb) => { windowListeners[type] = (windowListeners[type] ?? []).filter((f) => f !== cb); },
  setTimeout: (cb, ms) => { timers.push({ cb, ms }); return timers.length; },
  clearTimeout: (id) => { if (timers[id - 1]) timers[id - 1].cancelled = true; },
  ResizeObserver: class { observe() {} disconnect() {} },
  chrome: { webview: { postMessage: (wire) => sent.push(JSON.parse(wire)), addEventListener: (_t, cb) => listeners.push(cb) } },
});
globalThis.Element = Node;
globalThis.HTMLElement = Node;
globalThis.Node = Node;
globalThis.HTMLInputElement = class {};
const winEmit = (type, event) => (windowListeners[type] ?? []).slice().forEach((cb) => cb({ preventDefault() {}, stopPropagation() {}, ...event }));
const flushTimers = () => { for (const t of timers.splice(0)) if (!t.cancelled) t.cb(); };

const { buildStartPage } = await bundle('start-page.ts');
const page = buildStartPage();
pageHost.append(page.root);
const root = page.root;
const show = (message) => page.show(logic.parseStartMessage(message.d));
const dials = () => root.querySelectorAll('.sp-dial:not(.sp-add)');
const names = () => dials().map((d) => d.querySelector('.sp-title').textContent);
const clear = () => { sent.length = 0; };
const last = () => sent.at(-1);
const withSettings = (m, settings, extra = {}) => ({ ...m, d: { ...m.d, settings: { ...m.d.settings, ...settings }, ...extra } });

test('the page draws the groups, the dials in order, the add tile, and what Rust sent as text', () => {
  show(fixture.page);
  assert.equal(root.getAttribute('role'), 'region');
  assert.equal(root.getAttribute('aria-label'), 'Start page');
  assert.deepEqual(root.querySelectorAll('.sp-tab').map((t) => t.textContent), ['Work', 'Play <b>']);
  assert.deepEqual(root.querySelectorAll('.sp-tab').map((t) => t.getAttribute('aria-selected')), ['true', 'false']);
  assert.deepEqual(names(), ['Docs "quoted"', 'Rust', 'Reading']);
  assert.equal(root.querySelector('.sp-dial[data-kind="folder"]').querySelector('.sp-open').getAttribute('aria-label'), 'Reading, folder, 0 sites');
  assert.ok(root.querySelector('.sp-add:not(.sp-add-folder)'), 'the add tile');
  // The cached icon is the tile's face; a site without one gets a letter on a tone.
  assert.ok(root.querySelector('.sp-dial[data-id="3"]').querySelector('img.sp-fav'));
  const letter = root.querySelector('.sp-dial[data-id="4"]').querySelector('.sp-face');
  assert.equal(letter.querySelector('.sp-letter').textContent, 'R');
  assert.ok(letter.getAttribute('data-tone') !== null);
});

test('maximum columns, the five sizes and drag are display settings: they change what the page carries', () => {
  show(withSettings(fixture.page, { columns: 3, thumbSize: 5 }));
  const grid = root.querySelector('.sp-grid');
  assert.equal(grid.style.getPropertyValue('--sp-tile'), '232px');
  assert.equal(grid.style.getPropertyValue('--sp-cols'), '3', 'the page is 900 wide in the fake document: 3 columns of 232 fit');
  show(withSettings(fixture.page, { columns: 12, thumbSize: 1 }));
  assert.equal(grid.style.getPropertyValue('--sp-tile'), '96px');
  assert.equal(grid.style.getPropertyValue('--sp-cols'), '5', 'never more columns than tiles (3 dials, the add tile and the folder tile): a short row stays centred');
  show(withSettings(fixture.page, { columns: 2, thumbSize: 1 }));
  assert.equal(grid.style.getPropertyValue('--sp-cols'), '2');
  // With enough dials the width limits it (900px, tiles of 96px: 8), and the setting caps it.
  const many = (n) => ({ ...fixture.page, d: { ...fixture.page.d, dials: Array.from({ length: n }, (_, i) => ({ id: 100 + i, title: `S${i}`, kind: 'link', url: `https://s${i}.example/`, host: `s${i}.example`, n: 0, thumb: false })) } });
  show(withSettings(many(20), { columns: 12, thumbSize: 1 }));
  assert.equal(grid.style.getPropertyValue('--sp-cols'), '8');
  show(withSettings(many(20), { columns: 5, thumbSize: 1 }));
  assert.equal(grid.style.getPropertyValue('--sp-cols'), '5');
  show(fixture.page);
});

test('titles always, never and when needed: the tile says which', () => {
  show(withSettings(fixture.page, { titles: 'always' }));
  assert.deepEqual(dials().map((d) => d.dataset.labels), ['show', 'show', 'show']);
  show(withSettings(fixture.page, { titles: 'never' }));
  assert.deepEqual(dials().map((d) => d.dataset.labels), ['hide', 'hide', 'hide']);
  assert.equal(root.querySelector('.sp-add').dataset.labels, 'hide', 'the add tile obeys it too');
  // "When needed": titles stay under tiles with no image and wait for hover under tiles that show one.
  const withImage = { ...fixture.page, d: { ...fixture.page.d, settings: { ...fixture.page.d.settings, titles: 'auto' }, dials: fixture.page.d.dials.map((d, i) => (i === 0 ? { ...d, thumb: true } : d)) } };
  show(withImage);
  assert.deepEqual(dials().map((d) => d.dataset.labels), ['hover', 'show', 'show']);
});

test('the add button shows or hides with its setting, and with no group there is a way to start', () => {
  show(withSettings(fixture.page, { addButton: false }));
  assert.equal(root.querySelector('.sp-add:not(.sp-add-folder)'), null);
  show(fixture.page);
  assert.ok(root.querySelector('.sp-add:not(.sp-add-folder)'));
  show(fixture.empty);
  const empty = root.querySelector('.sp-empty');
  assert.equal(empty.hidden, false);
  assert.match(empty.textContent, /Add a group/);
  assert.equal(dials().length, 0);
  show(fixture.page);
});

test('clicks: open here, in a new tab, a folder, a group; the add form checks the address', () => {
  show(fixture.page);
  clear();
  root.querySelector('.sp-dial[data-id="3"] .sp-open').click();
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'open', id: 3 });
  root.querySelector('.sp-dial[data-id="3"] .sp-open').click({ ctrlKey: true });
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'open', id: 3, on: true });
  root.querySelector('.sp-dial[data-id="3"] .sp-open').dispatch('auxclick', { button: 1 });
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'open', id: 3, on: true });
  root.querySelector('.sp-dial[data-kind="folder"] .sp-open').click();
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'show', id: 5 });
  root.querySelectorAll('.sp-tab')[1].click();
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'show', id: 2 });
  clear();
  root.querySelectorAll('.sp-tab')[0].click();
  assert.equal(sent.length, 0, 'the group already shown needs no command');

  // Add a site: the form, its refusal, then the command.
  root.querySelector('.sp-add:not(.sp-add-folder) .sp-open').click();
  const form = root.querySelector('form.sp-dialog');
  assert.ok(form, 'a dialog');
  assert.equal(form.getAttribute('role'), 'dialog');
  const [address, name] = form.querySelectorAll('input');
  address.value = 'javascript:alert(1)';
  clear();
  form.dispatch('submit');
  assert.equal(sent.length, 0);
  assert.equal(form.querySelector('.sp-error').hidden, false);
  address.value = 'example.com/news';
  name.value = '  News  ';
  form.dispatch('submit');
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'add', parent: 1, text: 'News', url: 'https://example.com/news' });
  assert.equal(root.querySelector('form.sp-dialog'), null, 'the dialog closes');
  // An empty name is left to Rust (it names the dial after its host).
  root.querySelector('.sp-add:not(.sp-add-folder) .sp-open').click();
  const second = root.querySelector('form.sp-dialog');
  second.querySelectorAll('input')[0].value = 'https://b.example/';
  second.dispatch('submit');
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'add', parent: 1, url: 'https://b.example/' });
  // Escape closes without sending.
  root.querySelector('.sp-add:not(.sp-add-folder) .sp-open').click();
  clear();
  root.querySelector('form.sp-dialog').dispatch('keydown', { key: 'Escape' });
  assert.equal(root.querySelector('form.sp-dialog'), null);
  assert.equal(sent.length, 0);
});

test('edit, choose an image, use the site icon again, and remove (which says it deletes the bookmark)', () => {
  const withImage = { ...fixture.page, d: { ...fixture.page.d, dials: fixture.page.d.dials.map((d, i) => (i === 0 ? { ...d, thumb: true } : d)) } };
  show(withImage);
  const tile = root.querySelector('.sp-dial[data-id="3"]');
  const tool = (n) => tile.querySelectorAll('.sp-tools button')[n];
  assert.deepEqual(tile.querySelectorAll('.sp-tools button').map((b) => b.getAttribute('aria-label')), ['Edit', 'Choose an image', 'Use the site icon instead', 'Remove']);
  assert.equal(tile.querySelectorAll('.sp-tools button')[2].hidden, false, 'only a tile with a chosen image offers going back');
  assert.equal(root.querySelector('.sp-dial[data-id="4"]').querySelectorAll('.sp-tools button')[2].hidden, true);
  clear();
  tool(1).click();
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'thumb', id: 3 });
  tool(2).click();
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'thumbclear', id: 3 });

  tool(0).click();
  const form = root.querySelector('form.sp-dialog');
  assert.equal(form.querySelector('h2').textContent, 'Edit site');
  const [address, name] = form.querySelectorAll('input');
  assert.equal(address.value, 'https://docs.rs/');
  name.value = 'Docs';
  address.value = 'docs.rs/stable';
  form.dispatch('submit');
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'edit', id: 3, text: 'Docs', url: 'https://docs.rs/stable' });

  // A folder has a name and no address.
  root.querySelector('.sp-dial[data-kind="folder"] .sp-tools button').click();
  const folderForm = root.querySelector('form.sp-dialog');
  assert.equal(folderForm.querySelector('h2').textContent, 'Rename folder');
  assert.equal(folderForm.querySelectorAll('input').length, 1);
  folderForm.querySelector('input').value = 'Later';
  folderForm.dispatch('submit');
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'edit', id: 5, text: 'Later' });

  clear();
  tool(3).click();
  const confirm = root.querySelector('form.sp-dialog');
  assert.match(confirm.textContent, /deleted from Bookmarks too/);
  assert.equal(sent.length, 0, 'nothing is deleted before it is confirmed');
  confirm.dispatch('submit');
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'remove', id: 3 });
});

test('group menu and the add-group form send their own commands', () => {
  show(fixture.page);
  clear();
  root.querySelector('.sp-add-group').click();
  const form = root.querySelector('form.sp-dialog');
  form.querySelector('input').value = 'News';
  form.dispatch('submit');
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'addgroup', text: 'News' });
  assert.ok(form.querySelector('.bw-select'), 'an existing bookmark folder can be used instead');

  root.querySelector('.sp-group-more').click();
  const items = root.querySelectorAll('.sp-menu .bw-menu-item');
  assert.deepEqual(items.map((i) => i.querySelector('.bw-menu-label').textContent), ['Rename', 'Move left', 'Move right', 'Remove from start page']);
  assert.equal(items[1].disabled, true, 'the first group cannot move left');
  items[2].click();
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'movegroup', id: 1 }, 'moving the first of two right sends it to the end');
  root.querySelector('.sp-group-more').click();
  root.querySelectorAll('.sp-menu .bw-menu-item')[3].click();
  const confirm = root.querySelector('form.sp-dialog');
  assert.match(confirm.textContent, /bookmark folder and its sites stay in Bookmarks/);
  clear();
  confirm.dispatch('submit');
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'removegroup', id: 1 });
});

test('inside a folder: the path, going up, and a refused change shows its words', () => {
  const inside = {
    ...fixture.page,
    d: { ...fixture.page.d, folder: 5, path: [{ id: 1, title: 'Work' }, { id: 5, title: 'Reading' }], dials: [] },
  };
  show(inside);
  const crumbs = root.querySelector('.sp-crumbs');
  assert.equal(crumbs.hidden, false);
  assert.deepEqual(crumbs.querySelectorAll('.sp-crumb').map((c) => c.textContent), ['Work', 'Reading']);
  assert.equal(crumbs.querySelector('.sp-crumb[aria-current="page"]').textContent, 'Reading');
  clear();
  crumbs.querySelector('.sp-up').click();
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'show', id: 1 });
  crumbs.querySelectorAll('.sp-crumb')[0].click();
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'show', id: 1 });
  show(fixture.page);
  assert.equal(root.querySelector('.sp-crumbs').hidden, true);
  show(fixture.failed);
  assert.equal(root.querySelector('.sp-toast').hidden, false);
  assert.equal(root.querySelector('.sp-toast').textContent, 'bookmark already exists');
  show(fixture.page);
});

test('chosen images arrive in their own messages and replace the face; a dial that lost its image drops it', () => {
  const withImage = { ...fixture.page, d: { ...fixture.page.d, dials: fixture.page.d.dials.map((d, i) => (i === 0 ? { ...d, thumb: true } : d)) } };
  show(withImage);
  const face = () => root.querySelector('.sp-dial[data-id="3"] .sp-face');
  assert.ok(face().querySelector('img.sp-fav'), 'the icon until the image arrives');
  page.show(logic.parseStartMessage(fixture.thumbs.d));
  const img = face().querySelector('img.sp-thumb');
  assert.ok(img && img.src.startsWith('data:image/jpeg;base64,'));
  assert.ok(face().classList.contains('has-image'));
  show(fixture.page);
  assert.ok(face().querySelector('img.sp-fav') && !face().querySelector('img.sp-thumb'), 'cleared');
});

test('drag to reorder is a setting: dropping before or after, into a folder, Escape to cancel; keyboard does the same', () => {
  show(fixture.page);
  const tileOf = (id) => root.querySelector(`.sp-dial[data-id="${id}"]`);
  const place = (id, left) => { tileOf(id).rect = { left, top: 0, right: left + 100, bottom: 80, width: 100, height: 80 }; };
  place(3, 0); place(4, 120); place(5, 240);
  const drag = (id, to, x) => {
    const node = tileOf(id);
    node.querySelector('.sp-open').dispatch('pointerdown', { button: 0, pointerId: 1, clientX: 10, clientY: 10 });
    hits = [tileOf(to)];
    winEmit('pointermove', { pointerId: 1, clientX: x, clientY: 20 });
    return node;
  };
  clear();
  // Rust-side order is 3, 4, 5. Dragging 5 onto the left half of 3 puts it before 3.
  let node = drag(5, 3, 20);
  assert.ok(node.classList.contains('is-dragging'));
  assert.ok(tileOf(3).classList.contains('is-drop-before'));
  winEmit('pointerup', { pointerId: 1, clientX: 20, clientY: 20 });
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'move', id: 5, before: 3 });
  assert.ok(!node.classList.contains('is-dragging'));
  // The click that follows a drag must not open the site.
  clear();
  node.querySelector('.sp-open').click();
  assert.equal(sent.length, 0);
  // Right half of 4, dragging 3: after 4 = before 5.
  clear();
  drag(3, 4, 190);
  assert.ok(tileOf(4).classList.contains('is-drop-after'));
  winEmit('pointerup', { pointerId: 1 });
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'move', id: 3, before: 5 });
  // Onto the middle of a folder: into it.
  clear();
  drag(3, 5, 290);
  assert.ok(tileOf(5).classList.contains('is-drop-into'));
  winEmit('pointerup', { pointerId: 1 });
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'movein', id: 3, parent: 5 });
  // After the last: the end.
  clear();
  drag(3, 5, 330);
  winEmit('pointerup', { pointerId: 1 });
  // Escape cancels with nothing sent and the marks gone.
  clear();
  drag(5, 3, 20);
  winEmit('keydown', { key: 'Escape' });
  assert.equal(sent.length, 0);
  assert.equal(root.querySelectorAll('.is-drop-before, .is-drop-after, .is-drop-into').length, 0);
  winEmit('pointerup', { pointerId: 1 });
  assert.equal(sent.length, 0);
  // A press that never moves is just a click.
  tileOf(4).querySelector('.sp-open').dispatch('pointerdown', { button: 0, pointerId: 1, clientX: 5, clientY: 5 });
  winEmit('pointerup', { pointerId: 1, clientX: 5, clientY: 5 });
  assert.equal(sent.length, 0);

  // Keyboard: Alt+Arrow moves the focused dial one place; Delete asks to remove it.
  tileOf(3).querySelector('.sp-open').dispatch('keydown', { key: 'ArrowRight', altKey: true });
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'move', id: 3, before: 5 });
  tileOf(4).querySelector('.sp-open').dispatch('keydown', { key: 'ArrowLeft', altKey: true });
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'move', id: 4, before: 3 });
  clear();
  tileOf(3).querySelector('.sp-open').dispatch('keydown', { key: 'ArrowLeft', altKey: true });
  assert.equal(sent.length, 0, 'already first');
  tileOf(4).querySelector('.sp-open').dispatch('keydown', { key: 'Delete' });
  assert.ok(root.querySelector('form.sp-dialog'));
  root.querySelector('form.sp-dialog').dispatch('keydown', { key: 'Escape' });
  assert.equal(sent.length, 0);

  // Drag reorder off: a press and a drag do nothing at all.
  show(withSettings(fixture.page, { dragReorder: false }));
  place(3, 0); place(4, 120); place(5, 240);
  clear();
  const off = drag(5, 3, 20);
  assert.ok(!off.classList.contains('is-dragging'));
  winEmit('pointerup', { pointerId: 1 });
  assert.equal(sent.length, 0);
  tileOf(3).querySelector('.sp-open').dispatch('keydown', { key: 'ArrowRight', altKey: true });
  assert.equal(sent.length, 0, 'the keyboard path obeys the setting too');
  show(fixture.page);
});

test('widgets: each kind draws, and the dashboard follows its settings', () => {
  show(fixture.page);
  const cards = root.querySelectorAll('.sp-widget');
  assert.deepEqual(cards.map((c) => c.dataset.kind), ['date', 'topsites', 'privacy', 'bookmarks', 'notes', 'webpage']);
  const card = (kind) => root.querySelector(`.sp-widget[data-kind="${kind}"]`);
  assert.match(card('date').querySelector('.sp-date-day').textContent, /^\d{1,2}$/);
  assert.equal(card('topsites').querySelector('.sp-row-title').textContent, 'Docs.rs');
  assert.equal(card('privacy').querySelector('.sp-stat').textContent, '7');
  assert.match(card('privacy').textContent, /1,234 requests blocked in all/);
  assert.match(card('privacy').textContent, /Most from tracker\.example \(99\)/);
  assert.equal(card('bookmarks').querySelectorAll('.sp-mini').length, 2, 'tiles view');
  assert.equal(card('notes').querySelector('textarea').value, 'buy milk\nand <script>', 'text stays text');
  // Settings: off sends no widgets and hides the dashboard.
  show(withSettings(fixture.page, { widgets: false }, { widgets: [] }));
  assert.equal(root.querySelector('.sp-widgets').hidden, true);
  show(fixture.page);
  assert.equal(root.querySelector('.sp-widgets').hidden, false);
  // Background style and height are classes on the page; the custom colour is a variable and a readable text colour.
  for (const bg of ['solid', 'translucent', 'transparent', 'custom']) {
    show(withSettings(fixture.page, { widgetBg: bg, widgetHeight: 'tall', widgetColor: '#336699' }));
    assert.ok(root.classList.contains(`is-bg-${bg}`), bg);
    assert.ok(root.classList.contains('is-tall'));
    assert.equal(['solid', 'translucent', 'transparent', 'custom'].filter((b) => root.classList.contains(`is-bg-${b}`)).length, 1, 'one background at a time');
  }
  assert.equal(root.style.getPropertyValue('--sp-widget-custom'), '#336699');
  assert.equal(root.style.getPropertyValue('--sp-widget-custom-fg'), '#ffffff');
  show(withSettings(fixture.page, { widgetHeight: 'regular' }));
  assert.ok(root.classList.contains('is-regular') && !root.classList.contains('is-tall'));
  // Columns: the setting caps the grid, the width limits it.
  for (const [setting, expected] of [[1, '1'], [2, '2'], [4, '4']]) {
    show(withSettings(fixture.page, { widgetColumns: setting }));
    assert.equal(root.querySelector('.sp-widget-grid').style.getPropertyValue('--sp-wcols'), expected);
  }
  show(fixture.page);
});

test('widgets: the sticky note saves as it is typed and goes to Notes on request; add, remove, configure', () => {
  show(fixture.page);
  const card = (kind) => root.querySelector(`.sp-widget[data-kind="${kind}"]`);
  const area = card('notes').querySelector('textarea');
  clear();
  area.value = 'remember the milk';
  area.dispatch('input');
  assert.equal(sent.length, 0, 'debounced');
  flushTimers();
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'wnote', id: 5, text: 'remember the milk' });
  clear();
  card('notes').querySelector('.sp-note-foot .bw-button').click();
  assert.deepEqual(sent, [{ v: 1, t: 'spage', a: 'wnote', id: 5, text: 'remember the milk' }, { v: 1, t: 'spage', a: 'wsavenote', id: 5 }]);
  assert.equal(card('notes').querySelector('.sp-note-status').textContent, 'Saved to Notes');
  // A refresh keeps the caret (the note is not rewritten while it has focus) and the saved state.
  document.activeElement = area;
  show(fixture.page);
  assert.equal(area.value, 'remember the milk');
  document.activeElement = null;
  // An empty note has nothing to save.
  area.value = '   ';
  clear();
  card('notes').querySelector('.sp-note-foot .bw-button').click();
  assert.equal(sent.length, 0);
  area.value = 'remember the milk';

  // Remove and add.
  clear();
  card('date').querySelector('.sp-widget-tool').click();
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'wremove', id: 1 });
  root.querySelector('.sp-add-widget').click();
  const items = root.querySelectorAll('.sp-menu .bw-menu-item');
  assert.deepEqual(items.map((i) => i.querySelector('.bw-menu-label').textContent), ['Top sites', 'Bookmarks', 'Sticky note', 'Date', 'Privacy statistics', 'Webpage']);
  items[5].click();
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'wadd', text: 'webpage' });
  // Clicking a top site opens it here or in a new tab, by address.
  clear();
  card('topsites').querySelector('.sp-row').click();
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'openurl', url: 'https://docs.rs/' });
  card('topsites').querySelector('.sp-row').click({ ctrlKey: true });
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'openurl', url: 'https://docs.rs/', on: true });
});

test('widgets: the bookmarks widget picks a folder and a look, and shows titles and favicons or tiles', () => {
  show(fixture.page);
  const card = () => root.querySelector('.sp-widget[data-kind="bookmarks"]');
  card().querySelector('.sp-widget-tool[aria-pressed="false"]').click();
  const strip = card().querySelector('.sp-config');
  assert.ok(strip.querySelector('.bw-select'), 'the folder picker');
  const radios = strip.querySelectorAll('[role="radio"]');
  assert.deepEqual(radios.map((r) => r.textContent), ['Titles and icons', 'Tiles']);
  // With the strip open a tile can be given a picture of your own.
  const tools = card().querySelectorAll('.sp-mini-tools');
  assert.equal(tools.length, 2);
  clear();
  tools[0].querySelector('button').click();
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'thumb', id: 3 });
  assert.equal(tools[0].querySelectorAll('button').length, 1, 'no "use the site icon" without an image');
  clear();
  radios[0].click();
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'wview', id: 4, text: 'list' });
  radios[1].click();
  assert.equal(sent.length, 1, 'the look already chosen sends nothing');
  clear();
  card().querySelector('.sp-mini').click();
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'openurl', url: 'https://docs.rs/' });
  const listView = { ...fixture.page, d: { ...fixture.page.d, widgets: fixture.page.d.widgets.map((w) => (w.kind === 'bookmarks' ? { ...w, view: 'list' } : w)) } };
  show(listView);
  assert.equal(card().querySelectorAll('.sp-row').length, 2);
  assert.equal(card().querySelector('.sp-row-title').textContent, 'Docs "quoted"');
  assert.ok(card().querySelector('.sp-row-face img.sp-fav'), 'the favicon');
  // Tiles show a bookmark's chosen image (it arrives in a thumbs message); the list stays titles and favicons.
  const withImage = { ...fixture.page, d: { ...fixture.page.d, widgets: fixture.page.d.widgets.map((w) => (w.kind === 'bookmarks' ? { ...w, links: w.links.map((l) => (l.id === 3 ? { ...l, thumb: true } : l)) } : w)) } };
  show(withImage);
  page.show(logic.parseStartMessage(fixture.thumbs.d));
  const tiles = card().querySelectorAll('.sp-mini');
  assert.ok(tiles[0].querySelector('img.sp-thumb'), 'the chosen image');
  assert.ok(tiles[1].querySelector('img.sp-fav') || tiles[1].querySelector('.sp-letter'), 'the others keep their icon');
  show(fixture.page);
});

test('the webpage widget is a sandboxed frame for an http(s) address the user typed, and nothing else', () => {
  show(fixture.page);
  const card = () => root.querySelector('.sp-widget[data-kind="webpage"]');
  const frame = card().querySelector('iframe');
  assert.ok(frame);
  assert.equal(frame.getAttribute('sandbox'), 'allow-scripts allow-forms', 'no same-origin, popups, top navigation, downloads or modals');
  assert.equal(frame.getAttribute('referrerpolicy'), 'no-referrer');
  assert.equal(frame.getAttribute('allow'), '');
  assert.equal(frame.getAttribute('loading'), 'lazy');
  assert.equal(frame.src, 'https://widget.example/clock');
  // A refresh keeps the same frame (no reload); a new address makes a new one.
  show(fixture.page);
  assert.equal(card().querySelector('iframe'), frame);
  const next = { ...fixture.page, d: { ...fixture.page.d, widgets: fixture.page.d.widgets.map((w) => (w.kind === 'webpage' ? { ...w, url: 'https://other.example/' } : w)) } };
  show(next);
  assert.notEqual(card().querySelector('iframe'), frame);
  assert.equal(card().querySelector('iframe').src, 'https://other.example/');
  // No address: no frame, an input to type one.
  const none = { ...fixture.page, d: { ...fixture.page.d, widgets: fixture.page.d.widgets.map((w) => (w.kind === 'webpage' ? { ...w, url: '' } : w)) } };
  show(none);
  assert.equal(card().querySelector('iframe'), null);
  const form = card().querySelector('form');
  const input = form.querySelector('input');
  clear();
  input.value = 'javascript:alert(1)';
  form.dispatch('submit');
  assert.equal(sent.length, 0, 'refused');
  assert.equal(form.querySelector('.sp-error').hidden, false);
  input.value = 'data:text/html,<b>x</b>';
  form.dispatch('submit');
  assert.equal(sent.length, 0);
  input.value = 'widget.example/clock';
  form.dispatch('submit');
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'wurl', id: 6, url: 'https://widget.example/clock' });
  show(fixture.page);
});

test('widgets drag by their handle and move with Alt and the arrows; the setting turns both off', () => {
  show(fixture.page);
  const cardOf = (id) => root.querySelector(`.sp-widget[data-id="${id}"]`);
  [1, 2, 3, 4, 5, 6].forEach((id, i) => { cardOf(id).rect = { left: (i % 2) * 300, top: Math.floor(i / 2) * 200, right: (i % 2) * 300 + 280, bottom: Math.floor(i / 2) * 200 + 180, width: 280, height: 180 }; });
  clear();
  const grip = (id) => cardOf(id).querySelector('.sp-grip');
  grip(6).dispatch('pointerdown', { button: 0, pointerId: 2, clientX: 10, clientY: 10 });
  hits = [cardOf(2)];
  winEmit('pointermove', { pointerId: 2, clientX: 310, clientY: 20 });
  assert.ok(cardOf(2).classList.contains('is-drop-before'));
  winEmit('pointerup', { pointerId: 2 });
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'wmove', id: 6, before: 2 });
  clear();
  grip(1).dispatch('keydown', { key: 'ArrowRight', altKey: true });
  assert.deepEqual(last(), { v: 1, t: 'spage', a: 'wmove', id: 1, before: 3 });
  show(withSettings(fixture.page, { widgetDrag: false }));
  assert.equal(root.querySelectorAll('.sp-grip').length, 0, 'no handles with drag to rearrange off');
  show(fixture.page);
});

test('guest windows keep nothing: no editing offered', () => {
  const guest = { ...fixture.empty, d: { ...fixture.empty.d, guest: true, settings: { ...fixture.empty.d.settings, widgets: true }, widgets: [] } };
  show(guest);
  assert.equal(root.querySelector('.sp-add-group').hidden, true);
  assert.match(root.querySelector('.sp-empty').textContent, /not kept in a guest window/);
  assert.equal(root.querySelector('.sp-add-widget').hidden, true);
  show(fixture.page);
});

// ---- the stylesheet and the sources ----

test('start.css: its header, tokens only, motion limited to transform, opacity and colour, nothing infinite', () => {
  const css = read(join(srcDir, 'styles', 'start.css'));
  assert.match(css.slice(0, 700), /Surface: START PAGE.*Owner:/s);
  assert.doesNotMatch(css, /var\(--(bg|fill|border|focus-border|text|placeholder|completion|selection|hover|progress)\)/, 'host inputs are read through --bw-* tokens');
  assert.doesNotMatch(css, /infinite/);
  assert.doesNotMatch(css, /@keyframes/, 'the shared enter is the only entrance');
  for (const rule of css.matchAll(/transition:\s*([^;]+);/g)) {
    for (const part of rule[1].split(',')) {
      const prop = part.trim().split(/\s+/)[0];
      assert.ok(['transform', 'opacity', 'background-color', 'border-color', 'color', 'none'].includes(prop), `transition on ${prop}`);
    }
  }
  for (const [, prop] of css.matchAll(/transition-property:\s*([^;]+);/g)) assert.ok(/^(transform|opacity|background-color|border-color|color)(,\s*(transform|opacity|background-color|border-color|color))*$/.test(prop.trim()), prop);
  // Every colour comes from a token, except the white behind a framed page (a page expects a white canvas).
  const literals = [...css.matchAll(/#[0-9a-f]{3,8}\b|rgba?\(/gi)].map((m) => m[0].toLowerCase());
  assert.deepEqual(literals, ['#ffffff']);
  assert.match(read(join(srcDir, 'styles', 'app.css')), /@import "\.\/start\.css" layer\(surfaces\);/);
});

test('the page builds DOM with createElement and textContent only, and never changes its own URL', () => {
  for (const file of ['start-page.ts', 'start-page-logic.ts', 'settings-start.ts']) {
    const text = read(join(srcDir, file));
    for (const banned of [/innerHTML|outerHTML|insertAdjacentHTML|document\.write|eval\(|new Function/, /location\.(hash|href|assign|replace)/, /window\.open/, /createElement\('a'\)|el\('a'/, /\.srcdoc/, /\.href\s*=/]) {
      assert.doesNotMatch(text, banned, `${file}: ${banned}`);
    }
    // Mirrors src/chrome_host/guard.rs.
    for (const term of [['pass', 'words'].join(''), ['va', 'ult'].join(''), ['creden', 'tial'].join('')]) assert.ok(!text.toLowerCase().includes(term), `${file}: ${term}`);
  }
  // The only frame the page ever makes is the webpage widget's, and it is made with the sandbox attribute.
  const source = read(join(srcDir, 'start-page.ts'));
  assert.equal([...source.matchAll(/el\('iframe'/g)].length, 1);
  assert.match(source, /setAttribute\('sandbox', 'allow-scripts allow-forms'\)/);
  assert.doesNotMatch(source, /allow-same-origin|allow-top-navigation|allow-popups|allow-downloads|allow-modals/);
});
