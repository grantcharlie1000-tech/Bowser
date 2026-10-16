// The shield (the site information page): the blocking level for this site and the cookie notice
// switch, in a fake document. Each choice sends one `act` carrying the wanted state, choosing what
// is already in force sends nothing, an unavailable blocker offers nothing, and a private tab has no
// choice to make. The wire (ids, n) mirrors src/win32/site_info.rs.
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

const sel = (selector) => selector.split(',').map((one) => one.trim());
function matchOne(node, s) {
  const tag = /^[a-z][a-z0-9]*/.exec(s)?.[0];
  if (tag && node.tagName !== tag) return false;
  for (const cls of s.matchAll(/\.([\w-]+)/g)) if (!node.classList.contains(cls[1])) return false;
  for (const attr of s.matchAll(/\[([\w-]+)(?:="([^"]*)")?\]/g)) {
    const value = attr[1] === 'disabled' ? (node.disabled ? '' : null) : node.getAttribute(attr[1]);
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
class Node {
  constructor(tag, text = null) {
    this.tagName = tag; this.className = ''; this.attrs = new Map(); this.children = []; this.parent = null; this.listeners = {};
    this.classList = new ClassList(this); this.dataset = {}; this.hidden = false; this.disabled = false; this.tabIndex = 0; this.value = '';
    this.ownText = text; this.id = ''; this.type = ''; this.offsetHeight = 300; this.offsetWidth = 320;
    const props = new Map();
    this.style = { setProperty: (k, v) => props.set(k, v), getPropertyValue: (k) => props.get(k) ?? '' };
  }
  get textContent() { return this.ownText ?? this.children.map((c) => c.textContent).join(''); }
  set textContent(v) { for (const c of this.children) c.parent = null; this.children = []; this.ownText = null; if (v !== '') this.append(new Node('#text', String(v))); }
  setAttribute(k, v) { this.attrs.set(k, String(v)); if (k === 'id') this.id = String(v); }
  getAttribute(k) { return this.attrs.has(k) ? this.attrs.get(k) : null; }
  hasAttribute(k) { return this.attrs.has(k); }
  removeAttribute(k) { this.attrs.delete(k); }
  addEventListener(type, cb) { (this.listeners[type] ??= []).push(cb); }
  dispatch(type, event = {}) { for (const cb of this.listeners[type] ?? []) cb({ target: this, preventDefault() {}, stopPropagation() {}, key: '', ...event }); }
  click() { if (!this.disabled) this.dispatch('click'); }
  detach(node) { if (node.parent) node.parent.children = node.parent.children.filter((c) => c !== node); node.parent = this; }
  append(...nodes) { for (const n of nodes) { const node = typeof n === 'string' ? new Node('#text', n) : n; this.ownText = null; this.detach(node); this.children.push(node); } }
  prepend(node) { this.detach(node); this.children.unshift(node); }
  replaceChildren(...nodes) { for (const c of this.children) c.parent = null; this.children = []; this.ownText = null; this.append(...nodes); }
  all() { return this.children.flatMap((c) => [c, ...c.all()]); }
  matches(s) { return sel(s).some((x) => matchOne(this, x)); }
  querySelectorAll(s) { return this.all().filter((n) => n.tagName !== '#text' && n.matches(s)); }
  querySelector(s) { return this.querySelectorAll(s)[0] ?? null; }
  focus() { globalThis.document.activeElement = this; }
  contains(n) { return n === this || this.all().includes(n); }
}
const sent = [];
const timers = [];
globalThis.document = {
  documentElement: new Node('html'), body: new Node('body'), activeElement: null,
  createElement: (tag) => new Node(tag),
  createElementNS: (_ns, tag) => new Node(tag),
  createTextNode: (text) => new Node('#text', text),
  addEventListener() {}, removeEventListener() {},
};
globalThis.window = globalThis;
Object.assign(globalThis, {
  CSS: { escape: (value) => String(value) },
  matchMedia: () => ({ matches: false }),
  requestAnimationFrame: (cb) => { cb(); return 1; },
  setTimeout: (cb, ms) => { timers.push({ cb, ms }); return timers.length; },
  chrome: { webview: { postMessage: (wire) => sent.push(JSON.parse(wire)), addEventListener() {} } },
});
globalThis.Element = Node;
globalThis.HTMLElement = Node;
const logic = await bundle('siteinfo-logic.ts');
const { createSiteInfoView } = await bundle('siteinfo.ts');

const PAGE = {
  v: 1, t: 'siteinfo', tok: 9, kind: 'site-info', host: 'example.test', origin: 'https://example.test', security: 'secure', summary: 'Secure connection (HTTPS)',
  perms: [], cookies: { state: 'count', n: 2, clear: true }, blocker: { state: 'on', n: 3, level: 'trackers', notices: false }, tracking: 'balanced', zoom: 100,
  reset: false, blockedHere: false,
};
const show = (page) => {
  const root = new Node('div');
  const view = createSiteInfoView(root);
  assert.equal(view.show(page), true);
  return { root, view, page: root.children[0] };
};
const rowOf = (page, key) => page.children.find((c) => c.classList.contains('bw-siteinfo-row') && c.dataset.k === key);

test('the blocking row offers none, trackers and trackers and ads, with the level in force checked', () => {
  const { page } = show(PAGE);
  const row = rowOf(page, 'blocker');
  assert.equal(row.classList.contains('is-wide'), true, 'the long choices sit on their own line');
  assert.equal(row.querySelector('.bw-siteinfo-label').textContent, 'Blocking');
  assert.equal(row.querySelector('.bw-siteinfo-meta').textContent, 'Trackers · 3 requests blocked here');
  const group = row.querySelector('[role="radiogroup"]');
  assert.equal(group.getAttribute('aria-label'), 'Blocking on this site');
  const radios = group.querySelectorAll('[role="radio"]');
  assert.deepEqual(radios.map((r) => r.textContent), ['None', 'Trackers', 'Trackers + ads']);
  assert.deepEqual(radios.map((r) => r.getAttribute('aria-checked')), ['false', 'true', 'false']);
});

test('a choice sends the wanted level once; the level already in force sends nothing', () => {
  const { page } = show(PAGE);
  const radios = rowOf(page, 'blocker').querySelectorAll('[role="radio"]');
  sent.length = 0;
  radios[1].click();
  assert.deepEqual(sent, [], 'trackers is already in force');
  radios[2].click();
  assert.deepEqual(sent, [{ v: 1, t: 'act', tok: 9, id: logic.ACT_BLOCKER, n: 2 }]);
  assert.deepEqual(radios.map((r) => r.getAttribute('aria-checked')), ['false', 'false', 'true'], 'the choice shows at once');
  assert.equal(page.getAttribute('aria-busy'), 'true', 'and the controls wait for Bowser to answer');
  radios[0].click();
  assert.equal(sent.length, 1, 'no second request while the first is pending');
});

test('each level maps to the n Rust numbers it by', () => {
  for (const [n, level] of [[0, 'none'], [1, 'trackers'], [2, 'ads']]) {
    const at = level === 'none' ? 'trackers' : 'none';
    const { page } = show({ ...PAGE, blocker: { ...PAGE.blocker, level: at, state: at === 'none' ? 'off' : 'on' } });
    sent.length = 0;
    rowOf(page, 'blocker').querySelectorAll('[role="radio"]')[n].click();
    assert.deepEqual(sent, [{ v: 1, t: 'act', tok: 9, id: 11, n }], level);
  }
  assert.deepEqual([...logic.SHIELD_LEVELS], ['none', 'trackers', 'ads']);
  const rust = read(join(here, '..', '..', 'src', 'win32', 'site_info.rs'));
  assert.match(rust, /const ACT_BLOCKER: u32 = 11;/);
  assert.match(rust, /const ACT_NOTICES: u32 = 14;/);
  assert.match(rust, /\[crate::blocker::Level::None, crate::blocker::Level::Trackers, crate::blocker::Level::Ads\]/, 'the order Rust numbers the levels in');
});

test('the cookie notice switch sends the wanted state: hide, then show', () => {
  const shown = show(PAGE);
  const sw = rowOf(shown.page, 'notices').querySelector('[role="switch"]');
  assert.equal(rowOf(shown.page, 'notices').querySelector('.bw-siteinfo-meta').textContent, 'Shown on this site');
  assert.equal(sw.getAttribute('aria-checked'), 'false');
  sent.length = 0;
  sw.click();
  assert.deepEqual(sent, [{ v: 1, t: 'act', tok: 9, id: logic.ACT_NOTICES, n: 1 }]);
  const hidden = show({ ...PAGE, blocker: { ...PAGE.blocker, notices: true } });
  const off = rowOf(hidden.page, 'notices');
  assert.equal(off.querySelector('.bw-siteinfo-meta').textContent, 'Hidden on this site');
  assert.equal(off.querySelector('[role="switch"]').getAttribute('aria-checked'), 'true');
  sent.length = 0;
  off.querySelector('[role="switch"]').click();
  assert.deepEqual(sent, [{ v: 1, t: 'act', tok: 9, id: 14, n: 0 }]);
});

test('with no blocker nothing can be chosen, and a private tab has no choice at all', () => {
  const none = show({ ...PAGE, blocker: { state: 'unavailable', n: 0, level: 'none', notices: false } });
  const radios = rowOf(none.page, 'blocker').querySelectorAll('[role="radio"]');
  assert.equal(radios.every((r) => r.disabled), true);
  assert.equal(rowOf(none.page, 'notices'), undefined);
  assert.equal(rowOf(none.page, 'blocker').querySelector('.bw-siteinfo-meta').textContent, 'Unavailable');
  const priv = show({ ...PAGE, private: true, blocker: { state: 'on', n: 3, level: 'ads', notices: true } });
  const row = rowOf(priv.page, 'blocker');
  assert.equal(row.querySelector('[role="radiogroup"]'), null);
  assert.equal(row.classList.contains('is-wide'), false);
  assert.equal(row.querySelector('.bw-siteinfo-meta').textContent, 'Always on · 3 requests blocked here');
  assert.equal(rowOf(priv.page, 'notices'), undefined);
});

test('the arrow keys move between the levels', () => {
  const { page } = show(PAGE);
  const group = rowOf(page, 'blocker').querySelector('[role="radiogroup"]');
  const radios = group.querySelectorAll('[role="radio"]');
  radios[0].focus();
  group.dispatch('keydown', { key: 'ArrowRight' });
  assert.equal(globalThis.document.activeElement, radios[1]);
  group.dispatch('keydown', { key: 'ArrowLeft' });
  assert.equal(globalThis.document.activeElement, radios[0]);
  group.dispatch('keydown', { key: 'ArrowLeft' });
  assert.equal(globalThis.document.activeElement, radios[2], 'it wraps');
});
