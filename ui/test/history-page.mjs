// History page (docs/design/history-page.md): the snapshot contract against the Rust fixture
// (chrome_host/history_page.rs), the strict parser, day headings, the address line, search marks,
// keys, the motion and token rules of history.css, the sources' bans, and the page's DOM in a fake
// document (landmarks, labels, what a click, a key or a search sends, the states).
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
const logic = await bundle('history-page-logic.ts');
const { parseInbound } = await bundle('protocol.ts');
const fixture = JSON.parse(read(join(here, 'fixtures', 'history-page-messages.json')));

test('every Rust snapshot passes the bridge and the strict snapshot contract', () => {
  for (const [name, message] of Object.entries(fixture)) {
    const inbound = parseInbound(JSON.stringify(message));
    assert.equal(inbound?.t, 'snap', name);
    assert.equal(inbound.p, 'historypage', name);
    assert.ok(logic.parseHistorySnapshot(inbound.d), `${name}: ${JSON.stringify(message.d).slice(0, 200)}`);
  }
  const snap = logic.parseHistorySnapshot(fixture.snap.d);
  assert.equal(snap.state, 'on');
  assert.deepEqual(snap.days.map((day) => [day.label, day.date, day.count, day.entries.length]), [['Today', '2026-10-14', 2, 2], ['Yesterday', '2026-10-13', 1, 1], ['2026-10-09', '2026-10-09', 1, 1]]);
  assert.equal(snap.days[1].entries[0].title, 'News "<b>"', 'text arrives as text');
  assert.equal(snap.days[0].entries[0].host, 'rust-lang.org');
  assert.deepEqual(Object.keys(snap.favs), ['docs.rs']);
  for (const name of ['off', 'guest', 'error']) {
    const s = logic.parseHistorySnapshot(fixture[name].d);
    assert.equal(s.state, name);
    assert.equal(s.days.length, 0, `${name} lists nothing`);
  }
  assert.equal(logic.parseHistorySnapshot(fixture.removed.d).ack, 'removed');
  assert.equal(logic.parseHistorySnapshot(fixture.removed.d).undo, true);
  assert.equal(logic.parseHistorySnapshot(fixture.search.d).query, 'rust');
});

test('partial, extended or inconsistent snapshots are refused', () => {
  const d = fixture.snap.d;
  const rows = d.rows;
  const entry = rows[1];
  const bad = [
    { ...d, extra: 1 },
    { ...d, state: 'paused' },
    { ...d, ack: 'ok' },
    { ...d, more: 'no' },
    { ...d, query: 'x'.repeat(513) },
    { ...d, rows: [entry, ...rows] },
    { ...d, rows: [{ ...rows[0], count: 5 }, ...rows.slice(1)] },
    { ...d, rows: [rows[0], { ...entry, extra: 1 }, ...rows.slice(2)] },
    { ...d, rows: [rows[0], { ...entry, id: 0 }, ...rows.slice(2)] },
    { ...d, rows: [rows[0], { ...entry, id: 2 ** 32 }, ...rows.slice(2)] },
    { ...d, rows: [rows[0], { ...entry, url: 'javascript:alert(1)' }, ...rows.slice(2)] },
    { ...d, rows: [rows[0], { ...entry, clock: '25:00' }, ...rows.slice(2)] },
    { ...d, rows: [rows[0], { ...entry, title: 'x'.repeat(241) }, ...rows.slice(2)] },
    { ...d, rows: [rows[0], entry, entry, ...rows.slice(3)] },
    { ...d, rows: [{ ...rows[0], date: '2026-13-01' }, ...rows.slice(1)] },
    { ...d, favs: { 'docs.rs': 'data:image/gif;base64,AAAA' } },
    { ...d, state: 'guest' },
    { ...fixture.off.d, undo: true },
  ];
  for (const value of bad) assert.equal(logic.parseHistorySnapshot(value), null, JSON.stringify(value).slice(0, 160));
  const { favs: _f, ...missing } = d;
  assert.equal(logic.parseHistorySnapshot(missing), null);
  assert.equal(parseInbound({ v: 1, t: 'snap', p: 'historypages', d }), null, 'only the known page token');
});

test('day headings, counts and the address line read naturally', () => {
  assert.equal(logic.longDate('2026-10-14', 2026), 'Wednesday 14 October');
  assert.equal(logic.longDate('2025-12-31', 2026), 'Wednesday 31 December 2025');
  assert.equal(logic.longDate('2026-02-30x', 2026), '');
  assert.deepEqual(logic.dayHeading({ label: 'Today', date: '2026-10-14' }, 2026), { title: 'Today', caption: 'Wednesday 14 October' });
  assert.deepEqual(logic.dayHeading({ label: 'Yesterday', date: '2026-10-13' }, 2026), { title: 'Yesterday', caption: 'Tuesday 13 October' });
  assert.deepEqual(logic.dayHeading({ label: '2026-10-09', date: '2026-10-09' }, 2026), { title: 'Friday 9 October', caption: '' });
  assert.equal(logic.pagesText(1), '1 page');
  assert.equal(logic.pagesText(12), '12 pages');
  assert.equal(logic.resultText(0, ' zz '), 'No pages match “zz”');
  assert.equal(logic.resultText(3, 'rust'), '3 pages found for “rust”');
  assert.equal(logic.resultText(3, '  '), '');
  assert.deepEqual(logic.addressParts('https://www.rust-lang.org/learn', 'rust-lang.org'), { host: 'rust-lang.org', rest: '/learn' });
  assert.deepEqual(logic.addressParts('https://example.com/', 'example.com'), { host: 'example.com', rest: '' });
  assert.deepEqual(logic.addressParts('https://a.example/x/?q=1', 'a.example'), { host: 'a.example', rest: '/x/?q=1' });
  assert.equal(logic.rowName({ title: '  ', host: 'docs.rs', url: 'https://docs.rs/' }), 'docs.rs');
  for (const host of ['a', 'example.com', 'bbc.co.uk']) {
    const tone = logic.tileTone(host);
    assert.ok(Number.isInteger(tone) && tone >= 0 && tone < 8);
    assert.equal(logic.tileTone(host), tone, 'stable');
  }
});

test('search marks follow Bowser’s one-substring match; keys move and clamp', () => {
  assert.deepEqual(logic.matchRanges('Learn Rust - Rust', 'rust'), [[6, 10], [13, 17]]);
  assert.deepEqual(logic.matchRanges('Learn Rust', ' RUST '), [[6, 10]]);
  assert.deepEqual(logic.matchRanges('Learn Rust', 'learn rust'), [[0, 10]]);
  assert.deepEqual(logic.matchRanges('abc', ''), []);
  assert.deepEqual(logic.matchRanges('İstanbul', 'i'), [], 'case folding that changes length is left unmarked');
  assert.equal(logic.moveIndex(5, -1, 'ArrowDown'), 0);
  assert.equal(logic.moveIndex(5, 4, 'ArrowDown'), 4);
  assert.equal(logic.moveIndex(5, 0, 'ArrowUp'), 0);
  assert.equal(logic.moveIndex(5, 2, 'Home'), 0);
  assert.equal(logic.moveIndex(5, 2, 'End'), 4);
  assert.equal(logic.moveIndex(50, 2, 'PageDown', 10), 12);
  assert.equal(logic.moveIndex(50, 5, 'PageUp', 10), 0);
  assert.equal(logic.moveIndex(5, 2, 'Enter'), null);
  assert.equal(logic.moveIndex(0, -1, 'ArrowDown'), null);
  assert.equal(logic.focusAfterRemoval([1, 2, 3], 2), 3);
  assert.equal(logic.focusAfterRemoval([1, 2, 3], 3), 2);
  assert.equal(logic.focusAfterRemoval([1], 1), null);
  assert.equal(logic.opensNewTab({ ctrlKey: true, metaKey: false }), true);
  assert.equal(logic.opensNewTab({ ctrlKey: false, metaKey: false, button: 1 }), true);
  assert.equal(logic.opensNewTab({ ctrlKey: false, metaKey: false, shiftKey: true }), true);
  assert.equal(logic.opensNewTab({ ctrlKey: false, metaKey: false, button: 0 }), false);
  const kinds = (s) => logic.emptyView(s)?.kind ?? null;
  assert.equal(kinds({ state: 'on', query: '', days: [] }), 'empty');
  assert.equal(kinds({ state: 'on', query: 'zz', days: [] }), 'nomatch');
  assert.equal(kinds({ state: 'on', query: '', days: [{}] }), null);
  for (const state of ['off', 'guest', 'error']) assert.equal(kinds({ state, query: '', days: [] }), state);
});

test('history.css: transform and opacity motion only, finite, off under reduced motion, tokens only', () => {
  const css = read(join(srcDir, 'styles', 'history.css'));
  assert.match(css.slice(0, 600), /Surface: HISTORY PAGE.*Owner:/s);
  assert.doesNotMatch(css, /var\(--(bg|fill|border|focus-border|text|placeholder|completion|selection|hover|progress)\)/, 'host inputs are read through --bw-* tokens');
  assert.doesNotMatch(css, /infinite/);
  for (const block of css.matchAll(/@keyframes [\w-]+ \{([\s\S]*?)\n\}/g)) {
    for (const prop of block[1].matchAll(/^\s+([a-z-]+):/gm)) assert.ok(['transform', 'opacity'].includes(prop[1]), `keyframes animate ${prop[1]}`);
  }
  for (const rule of css.matchAll(/transition:\s*([^;]+);/g)) {
    for (const part of rule[1].split(',')) {
      const prop = part.trim().split(/\s+/)[0];
      assert.ok(['transform', 'opacity', 'background-color', 'border-color', 'color'].includes(prop), `transition on ${prop}`);
    }
  }
  const reduced = css.slice(css.lastIndexOf('@media (prefers-reduced-motion: reduce)'));
  for (const selector of ['.hp-mark.is-turn svg path', '.hp-row.is-new', '.hp-row.is-new::after', '.hp-row.is-gliding', '.hp-row.is-leaving', '.hp-toast']) assert.ok(reduced.includes(selector), selector);
  assert.match(read(join(srcDir, 'styles', 'app.css')), /@import "\.\/history\.css" layer\(surfaces\);/);
});

test('the page never changes its own URL, sends only its own family and names no store it must not touch', () => {
  const page = read(join(srcDir, 'history-page.ts'));
  for (const banned of [/location\.(hash|href|assign|replace)/, /history\.(pushState|replaceState)/, /\.href\s*=/, /window\.open/, /createElement\('a'\)|el\('a'/, /innerHTML|insertAdjacentHTML|outerHTML/]) {
    assert.doesNotMatch(page, banned);
  }
  assert.deepEqual([...page.matchAll(/sendPanel\('([a-z]+)'/g)].map((m) => m[1]), ['hpage']);
  assert.deepEqual([...page.matchAll(/\bsend\((\{[^)]*\})\)/g)].map((m) => m[1]), ["{ v: 1, t: 'ready' }"]);
  for (const file of ['history-page.ts', 'history-page-logic.ts']) {
    const text = read(join(srcDir, file)).toLowerCase();
    for (const term of [['pass', 'words'].join(''), ['va', 'ult'].join(''), ['creden', 'tial'].join('')]) assert.ok(!text.includes(term), `${file}: ${term}`);
  }
});

// ---- the page in a fake document ----

function matchOne(node, s) {
  const tag = /^[a-z][a-z0-9]*/.exec(s)?.[0];
  if (tag && node.tagName !== tag) return false;
  for (const cls of s.matchAll(/\.([\w-]+)/g)) if (!node.classList.contains(cls[1])) return false;
  for (const attr of s.matchAll(/\[([\w-]+)(?:="([^"]*)")?\]/g)) {
    const value = attr[1] === 'hidden' ? (node.hidden ? '' : null) : node.getAttribute(attr[1]);
    if (value === null || (attr[2] !== undefined && value !== attr[2])) return false;
  }
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
    this.tagName = tag; this.className = ''; this.attrs = new Map(); this.children = []; this.parent = null; this.listeners = {};
    this.classList = new ClassList(this); this.dataset = {}; this.hidden = false; this.disabled = false; this.tabIndex = 0; this.value = '';
    this.ownText = text; this.id = ''; this.type = ''; this.scrollTop = 0; this.clientHeight = 800; this.offsetHeight = 52; this.offsetWidth = 100;
    const props = new Map();
    this.style = { setProperty: (k, v) => props.set(k, v), getPropertyValue: (k) => props.get(k) ?? '' };
  }
  get textContent() { return this.ownText ?? this.children.map((c) => c.textContent).join(''); }
  set textContent(v) { for (const c of this.children) c.parent = null; this.children = []; this.ownText = null; if (v !== '') this.append(new Node('#text', String(v))); }
  get firstChild() { return this.children[0] ?? null; }
  get nextSibling() { const p = this.parent; return p ? p.children[p.children.indexOf(this) + 1] ?? null : null; }
  setAttribute(k, v) { this.attrs.set(k, String(v)); if (k === 'id') this.id = String(v); if (k === 'class') this.className = String(v); }
  getAttribute(k) {
    if (k === 'id') return this.id || null;
    if (k.startsWith('data-')) { const v = this.dataset[k.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase())]; if (v !== undefined) return v; }
    return this.attrs.has(k) ? this.attrs.get(k) : null;
  }
  hasAttribute(k) { return this.getAttribute(k) !== null; }
  removeAttribute(k) { this.attrs.delete(k); if (k.startsWith('data-')) delete this.dataset[k.slice(5)]; }
  addEventListener(type, cb) { (this.listeners[type] ??= []).push(cb); }
  removeEventListener() {}
  /** Bubbles from this node to the document, like a DOM event. */
  dispatch(type, init = {}) {
    let stopped = false;
    const event = { type, target: this, defaultPrevented: false, key: '', ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, button: 0, isComposing: false, keyCode: 0,
      preventDefault() { this.defaultPrevented = true; }, stopPropagation() { stopped = true; }, ...init };
    for (let node = this; node && !stopped; node = node.parent) for (const cb of node.listeners[type] ?? []) cb(event);
    if (!stopped) for (const cb of docListeners[type] ?? []) cb(event);
    return event;
  }
  click(init) { if (!this.disabled) this.dispatch('click', init); }
  detach(node) { if (node.parent) node.parent.children = node.parent.children.filter((c) => c !== node); node.parent = this; }
  append(...nodes) { for (const n of nodes) { const node = typeof n === 'string' ? new Node('#text', n) : n; this.ownText = null; this.detach(node); this.children.push(node); } }
  insertBefore(node, ref) { this.ownText = null; this.detach(node); const at = ref ? this.children.indexOf(ref) : -1; if (at < 0) this.children.push(node); else this.children.splice(at, 0, node); return node; }
  replaceChildren(...nodes) { for (const c of this.children) c.parent = null; this.children = []; this.ownText = null; this.append(...nodes); }
  remove() { if (this.parent) this.parent.children = this.parent.children.filter((c) => c !== this); this.parent = null; }
  all() { return this.children.flatMap((c) => [c, ...c.all()]); }
  matches(s) { return s.split(',').some((x) => matchOne(this, x.trim())); }
  closest(s) { for (let node = this; node; node = node.parent) if (node.tagName !== '#text' && node.matches(s)) return node; return null; }
  querySelectorAll(s) { return this.all().filter((n) => n.tagName !== '#text' && n.matches(s)); }
  querySelector(s) { return this.querySelectorAll(s)[0] ?? null; }
  focus() { globalThis.document.activeElement = this; }
  select() {}
  scrollIntoView() {}
  contains(n) { return n === this || this.all().includes(n); }
  getBoundingClientRect() { return { left: 0, top: 0, right: 0, bottom: 0, width: 0, height: 0 }; }
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
};
const timers = [];
Object.assign(globalThis, {
  window: globalThis,
  matchMedia: () => ({ matches: false }),
  requestAnimationFrame: (cb) => { cb(); return 1; },
  addEventListener() {}, removeEventListener() {},
  chrome: { webview: { postMessage: (wire) => sent.push(JSON.parse(wire)), addEventListener: (_t, cb) => listeners.push(cb) } },
});
// Timers run when the test says so (the search debounce, the toast).
globalThis.setTimeout = (cb) => { timers.push(cb); return timers.length; };
globalThis.clearTimeout = () => {};
const flush = () => { while (timers.length) timers.shift()(); };
globalThis.Element = Node;
globalThis.HTMLElement = Node;
globalThis.HTMLInputElement = InputNode;
const { startHistoryPage } = await bundle('history-page.ts');
const app = new Node('div');
startHistoryPage(app);
const deliver = (msg) => { for (const cb of listeners) cb({ data: JSON.stringify(msg) }); };
deliver({ t: 'state', scale: 1, pillx: 0, pillw: 0, menux: 0, solid: true, dark: true, back: false, forward: false, loading: false, split: false, bookmarked: false, progress: 0, security: 'none', label: '', theme: {} });
deliver(fixture.snap);
const rowEls = () => app.querySelectorAll('.hp-row');
const idOf = (row) => Number(row.dataset.id);

test('the page renders landmarks, a labelled section per day and a row per entry', () => {
  assert.deepEqual(sent[0], { v: 1, t: 'ready' });
  assert.equal(app.querySelector('main')?.getAttribute('aria-label'), 'History');
  assert.equal(app.querySelector('h1').textContent, 'History');
  assert.ok(app.querySelector('[role="search"]'));
  assert.equal(app.querySelector('.hp-search-input').getAttribute('aria-label'), 'Search history');
  const sections = app.querySelectorAll('section');
  assert.equal(sections.length, 3);
  for (const section of sections) assert.ok(app.querySelector(`#${section.getAttribute('aria-labelledby')}`), 'labelled by its heading');
  assert.deepEqual(app.querySelectorAll('.hp-day-title').map((n) => n.textContent), ['Today', 'Yesterday', 'Friday 9 October']);
  assert.equal(rowEls().length, 4);
  assert.equal(app.querySelector('.hp-name').textContent, 'Learn Rust');
  assert.equal(app.querySelector('.hp-host').textContent, 'rust-lang.org');
  assert.equal(app.querySelector('.hp-rest').textContent, '/learn');
  for (const button of app.querySelectorAll('.hp-act')) assert.ok(button.getAttribute('aria-label'), 'icon buttons are named');
  // One keyboard stop in the list: the first row.
  const stops = app.querySelectorAll('.hp-open').filter((b) => b.tabIndex === 0);
  assert.equal(stops.length, 1);
  assert.equal(stops[0].closest('.hp-row'), rowEls()[0]);
  // The cached favicon is an image; a site without one gets its letter.
  assert.equal(app.querySelectorAll('img').length, 1);
  assert.equal(rowEls()[0].querySelector('.hp-fav').textContent, 'R');
});

test('a click opens here, Ctrl or middle click and the row button open a new tab', () => {
  const [first, second] = rowEls();
  sent.length = 0;
  first.querySelector('.hp-open').click();
  second.querySelector('.hp-open').click({ ctrlKey: true });
  second.querySelector('.hp-open').dispatch('auxclick', { button: 1 });
  first.querySelector('.hp-act').click();
  assert.deepEqual(sent, [
    { v: 1, t: 'hpage', a: 'open', id: idOf(first) },
    { v: 1, t: 'hpage', a: 'newtab', id: idOf(second) },
    { v: 1, t: 'hpage', a: 'newtab', id: idOf(second) },
    { v: 1, t: 'hpage', a: 'newtab', id: idOf(first) },
  ]);
});

test('keys: arrows move, Enter opens, Delete removes and the toast offers Undo', () => {
  const [first, second, third] = rowEls();
  first.querySelector('.hp-open').focus();
  first.querySelector('.hp-open').dispatch('keydown', { key: 'ArrowDown' });
  assert.equal(document.activeElement, second.querySelector('.hp-open'));
  sent.length = 0;
  second.querySelector('.hp-open').dispatch('keydown', { key: 'Enter', ctrlKey: true });
  assert.deepEqual(sent, [{ v: 1, t: 'hpage', a: 'newtab', id: idOf(second) }]);
  sent.length = 0;
  second.querySelector('.hp-open').dispatch('keydown', { key: 'Delete' });
  assert.deepEqual(sent, [{ v: 1, t: 'hpage', a: 'remove', id: idOf(second) }]);
  // Bowser answers: the row is gone, the next one has the keyboard, the toast offers Undo.
  const d = fixture.snap.d;
  const rows = [{ ...d.rows[0], count: 1 }, d.rows[1], ...d.rows.slice(3)];
  deliver({ ...fixture.snap, d: { ...d, rows, undo: true, ack: 'removed' } });
  assert.equal(rowEls().length, 3);
  assert.equal(document.activeElement, third.querySelector('.hp-open'));
  assert.match(app.querySelector('.hp-toast-text').textContent, /^Removed “serde - Rust”$/);
  const undo = app.querySelector('.hp-toast-undo');
  assert.equal(undo.hidden, false);
  sent.length = 0;
  undo.click();
  assert.deepEqual(sent, [{ v: 1, t: 'hpage', a: 'undo' }]);
  deliver(fixture.snap);
  assert.equal(rowEls().length, 4);
  // ArrowUp on the first row goes back to the search field.
  rowEls()[0].querySelector('.hp-open').focus();
  rowEls()[0].querySelector('.hp-open').dispatch('keydown', { key: 'ArrowUp' });
  assert.equal(document.activeElement, app.querySelector('.hp-search-input'));
});

test('search is debounced to one command, / focuses it and Escape clears it', () => {
  const input = app.querySelector('.hp-search-input');
  document.activeElement = rowEls()[0].querySelector('.hp-open');
  const slash = rowEls()[0].querySelector('.hp-open').dispatch('keydown', { key: '/' });
  assert.equal(slash.defaultPrevented, true);
  assert.equal(document.activeElement, input);
  sent.length = 0;
  input.value = 'ru';
  input.dispatch('input');
  input.value = 'rust';
  input.dispatch('input');
  flush();
  assert.deepEqual(sent.filter((m) => m.a === 'search').at(-1), { v: 1, t: 'hpage', a: 'search', text: 'rust' });
  deliver(fixture.search);
  assert.equal(rowEls().length, 1);
  assert.ok(app.querySelector('mark'), 'the hits are marked');
  assert.equal(app.querySelector('.hp-sr').textContent, '1 page found for “rust”');
  assert.equal(app.querySelector('.hp-search-clear').hidden, false);
  sent.length = 0;
  input.dispatch('keydown', { key: 'Escape' });
  assert.equal(input.value, '');
  assert.deepEqual(sent, [{ v: 1, t: 'hpage', a: 'search', text: '' }]);
  deliver(fixture.none);
  assert.equal(app.querySelector('.hp-empty-title').textContent, 'No matches for “zzz”');
  deliver(fixture.snap);
  assert.equal(app.querySelector('.hp-empty').hidden, true);
});

test('Clear browsing data, history off, guest and unreadable states', () => {
  sent.length = 0;
  app.querySelector('.hp-clear-data').click();
  assert.deepEqual(sent, [{ v: 1, t: 'hpage', a: 'privacy' }]);
  deliver(fixture.off);
  assert.equal(rowEls().length, 0);
  assert.equal(app.querySelector('.hp-empty-title').textContent, 'History is off');
  assert.equal(app.querySelector('.hp-search').hidden, true);
  sent.length = 0;
  app.querySelector('.hp-empty').querySelector('.bw-button').click();
  assert.deepEqual(sent, [{ v: 1, t: 'hpage', a: 'settings' }]);
  deliver(fixture.guest);
  assert.equal(app.querySelector('.hp-empty-title').textContent, 'Guest windows never save history');
  assert.equal(app.querySelector('.hp-clear-data').hidden, true);
  assert.equal(app.querySelector('.hp-empty').querySelector('.bw-button'), null);
  deliver(fixture.error);
  sent.length = 0;
  app.querySelector('.hp-empty').querySelector('.bw-button').click();
  assert.deepEqual(sent, [{ v: 1, t: 'hpage', a: 'refresh' }]);
  assert.match(app.querySelector('.hp-toast-text').textContent, /could not be changed/);
  deliver(fixture.empty);
  assert.equal(app.querySelector('.hp-empty-title').textContent, 'No history yet');
  // A snapshot that breaks the contract changes nothing.
  deliver({ v: 1, t: 'snap', p: 'historypage', d: { ...fixture.snap.d, extra: true } });
  assert.equal(app.querySelector('.hp-empty-title').textContent, 'No history yet');
});
