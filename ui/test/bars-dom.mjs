// Toolbars and feeds (thread T13): what the address bar's list, the tab bar's list and the panel bar's list do to
// the real toolbar and tab strip (src/main.ts, view.ts, tabs.ts), driven through the same inbound messages Rust
// sends, against a small fake DOM (no browser). The row that edits them is tested in test/settings-page.mjs; the
// Rust side (the lists to the wire, a command button to its command) in src/toolbar_prefs.rs, src/chrome_host and
// src/win32/window.rs.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, '..', 'src');

function bundleSource(contents) {
  const out = esbuild.buildSync({ stdin: { contents, resolveDir: srcDir, loader: 'ts' }, bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return `data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`;
}

// ---- a small fake DOM (as test/toolbar-options-dom.mjs) ----
function matchOne(node, s) {
  const tag = /^[a-z][a-z0-9]*/.exec(s)?.[0];
  if (tag && node.tagName !== tag) return false;
  for (const cls of s.matchAll(/\.([\w-]+)/g)) if (!node.classList.contains(cls[1])) return false;
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
    this.tagName = tag; this.className = ''; this.attrs = new Map(); this.childList = []; this.parent = null; this.listeners = {};
    this.classList = new ClassList(this); this.dataset = {}; this.hidden = false; this.disabled = false; this.ownText = text; this.id = '';
    this.value = ''; this.selectionStart = 0; this.selectionEnd = 0; this.tabIndex = 0;
    const props = new Map();
    this.style = { setProperty: (k, v) => props.set(k, String(v)), getPropertyValue: (k) => props.get(k) ?? '', removeProperty: (k) => props.delete(k) };
  }
  set innerHTML(_value) { throw new TypeError('Trusted Types'); }
  get children() { return this.childList.filter((c) => c.tagName !== '#text'); }
  get textContent() { return this.ownText ?? this.childList.map((c) => c.textContent).join(''); }
  set textContent(v) { this.childList = []; this.ownText = null; if (v !== '') this.append(new Node('#text', String(v))); }
  setAttribute(k, v) { this.attrs.set(k, String(v)); if (k === 'id') this.id = String(v); if (k === 'class') this.className = String(v); }
  getAttribute(k) { return this.attrs.has(k) ? this.attrs.get(k) : null; }
  hasAttribute(k) { return this.attrs.has(k); }
  removeAttribute(k) { this.attrs.delete(k); }
  toggleAttribute(k, on) { if (on ?? !this.attrs.has(k)) this.attrs.set(k, ''); else this.attrs.delete(k); }
  addEventListener(type, cb) { (this.listeners[type] ??= []).push(cb); }
  removeEventListener() {}
  dispatch(type, extra = {}) { for (const cb of this.listeners[type] ?? []) cb({ type, target: this, preventDefault() {}, stopPropagation() {}, ...extra }); }
  click() { this.dispatch('click'); }
  append(...nodes) { for (const n of nodes) { const node = typeof n === 'string' ? new Node('#text', n) : n; this.ownText = null; if (node.parent) node.parent.childList = node.parent.childList.filter((c) => c !== node); node.parent = this; this.childList.push(node); } }
  prepend(...nodes) { for (const n of nodes.reverse()) { n.parent = this; this.childList.unshift(n); } }
  replaceChildren(...nodes) { this.childList = []; this.ownText = null; this.append(...nodes); }
  after() {}
  remove() {}
  all() { return this.childList.flatMap((c) => [c, ...c.all()]); }
  matches(s) { return s.split(',').some((x) => matchOne(this, x.trim())); }
  closest() { return null; }
  querySelectorAll(s) { return this.all().filter((n) => n.tagName !== '#text' && n.matches(s)); }
  querySelector(s) { return this.querySelectorAll(s)[0] ?? null; }
  focus() { document.activeElement = this; }
  blur() { document.activeElement = null; }
  contains(n) { return n === this || this.all().includes(n); }
  getBoundingClientRect() { return { left: 0, top: 0, right: 28, bottom: 28, width: 28, height: 28 }; }
}
const sent = [];
const listeners = [];
const root = new Node('div');
root.id = 'app';
globalThis.document = {
  documentElement: new Node('html'), body: new Node('body'), activeElement: null, title: '',
  createElement: (tag) => new Node(tag),
  createElementNS: (_ns, tag) => new Node(tag),
  createTextNode: (text) => new Node('#text', text),
  addEventListener() {}, removeEventListener() {},
  getElementById: (id) => (id === 'app' ? root : null),
  hasFocus: () => true,
};
Object.assign(globalThis, {
  window: globalThis,
  location: { search: '' },
  matchMedia: () => ({ matches: false }),
  requestAnimationFrame: () => 0,
  cancelAnimationFrame() {},
  queueMicrotask: (cb) => cb(),
  addEventListener() {}, removeEventListener() {},
  getComputedStyle: () => ({}),
  CSS: { escape: (s) => s },
  InputEvent: class InputEvent {},
  chrome: { webview: { postMessage: (wire) => sent.push(JSON.parse(wire)), addEventListener: (_t, cb) => listeners.push(cb) } },
});
globalThis.Element = Node;
globalThis.HTMLElement = Node;
globalThis.HTMLButtonElement = Node;
globalThis.HTMLInputElement = Node;
const deliver = (msg) => { for (const cb of listeners.slice()) cb({ data: JSON.stringify(msg) }); };

// main.ts starts the toolbar on import (no ?view means the toolbar), exactly as the browser does.
await import(bundleSource("import './main';"));
await new Promise(setImmediate);

const STATE = { t: 'state', scale: 1, solid: true, dark: false, back: true, forward: true, loading: false, progress: 0, security: 'secure', label: 'Secure', theme: {} };
const bar = (items, extra = {}) => ({ ...STATE, items, tools: [], extra: 0, feeds: 0, ...extra });
const nav = () => root.querySelector('.nav');
const actions = () => root.querySelector('.actions');
const pill = () => root.querySelector('.pill');
const names = (node) => node.children.map((c) => (c.classList.contains('nav-group') ? `[${names(c).join(' ')}]` : ['tb-back', 'tb-forward', 'tb-reload'].find((n) => c.classList.contains(n)) ?? c.className));
const lead = (button) => ['back', 'forward', 'reload'].find((name) => button.getAttribute('aria-label')?.toLowerCase().startsWith(name)) ?? button.getAttribute('aria-label');
const leftNames = () => nav().children.map((c) => (c.classList.contains('nav-group') ? c.children.map(lead).join('+') : lead(c)));
const rightLabels = () => actions().children.map((c) => c.getAttribute('aria-label'));
const DEFAULT = 'back,forward,reload,site-info,split,reader,extensions,downloads';

test('the default list lays the toolbar out as it always was', () => {
  deliver(bar(DEFAULT));
  assert.deepEqual(leftNames(), ['back+forward', 'reload']);
  assert.deepEqual(rightLabels(), ['Extensions', 'Downloads', 'Menu']);
  assert.equal(root.querySelector('.tb-site').hidden, false);
  assert.equal(root.querySelector('.tb-split').hidden, false);
  assert.equal(root.style.getPropertyValue('--tb-extra'), '0px');
  assert.equal(root.querySelector('.tb-feed').hidden, true);
});

test('an older host that sends no list gets every button in its usual place', () => {
  deliver({ ...STATE });
  assert.deepEqual(leftNames(), ['back+forward', 'reload']);
  assert.deepEqual(rightLabels(), ['Extensions', 'Downloads', 'Menu']);
});

test('taking Back, Forward or Reload off the list removes it, and the others keep their order', () => {
  deliver(bar('forward,reload,site-info,split,reader,extensions,downloads'));
  assert.deepEqual(leftNames(), ['forward', 'reload']);
  deliver(bar('reload,site-info,split'));
  assert.deepEqual(leftNames(), ['reload']);
  deliver(bar('site-info,split'));
  assert.deepEqual(leftNames(), []);
  deliver(bar(DEFAULT));
  assert.deepEqual(leftNames(), ['back+forward', 'reload']);
});

test('moving buttons in the list moves them on the bar', () => {
  deliver(bar('reload,back,forward,site-info,split'));
  assert.deepEqual(leftNames(), ['reload', 'back+forward']);
  deliver(bar('forward,back,reload,site-info,split'));
  assert.deepEqual(leftNames(), ['forward+back', 'reload']);
  deliver(bar('back,reload,forward,site-info,split'));
  assert.deepEqual(leftNames(), ['back', 'reload', 'forward'], 'buttons that are not neighbours are separate');
  deliver(bar(DEFAULT));
});

test('the buttons inside the address field follow the list', () => {
  deliver(bar('back,forward,reload,split'));
  assert.equal(root.querySelector('.tb-site').hidden, true);
  assert.ok(pill().classList.contains('no-site'), 'the text takes the site button’s room back');
  assert.equal(root.querySelector('.tb-split').hidden, false);
  deliver(bar('back,forward,reload,site-info'));
  assert.equal(root.querySelector('.tb-site').hidden, false);
  assert.ok(!pill().classList.contains('no-site'));
  assert.equal(root.querySelector('.tb-split').hidden, true);
  deliver(bar(DEFAULT));
});

test('reader view shows on an article only while it is on the list', () => {
  deliver({ ...bar(DEFAULT), reader: 'available' });
  assert.equal(root.querySelector('.tb-reader').hidden, false);
  deliver({ ...bar('back,forward,reload,site-info,split'), reader: 'available' });
  assert.equal(root.querySelector('.tb-reader').hidden, true, 'off the list, a readable page does not bring it back');
  deliver({ ...bar(DEFAULT), reader: 'none' });
  assert.equal(root.querySelector('.tb-reader').hidden, true);
});

test('Bookmark and New tab join the end of the bar, send their item and index, and take their room', () => {
  deliver(bar('back,forward,reload,site-info,split,bookmark,newtab,downloads,extensions', { extra: 64, bookmarked: true }));
  assert.deepEqual(rightLabels(), ['Edit this bookmark', 'New tab', 'Downloads', 'Extensions', 'Menu']);
  assert.equal(root.style.getPropertyValue('--tb-extra'), '64px', 'the group moves left by the room Rust took from the field');
  const star = root.querySelector('.tb-bookmark');
  assert.equal(star.getAttribute('aria-pressed'), 'true');
  sent.length = 0;
  star.click();
  root.querySelector('.tb-newtab').click();
  assert.deepEqual(sent, [{ v: 1, t: 'tool', b: 'address', k: 'bookmark', i: 5 }, { v: 1, t: 'tool', b: 'address', k: 'newtab', i: 6 }]);
  deliver(bar('back,forward,reload,site-info,split,bookmark,newtab', { extra: 64, bookmarked: false }));
  assert.equal(star.getAttribute('aria-pressed'), 'false');
  assert.equal(star.getAttribute('aria-label'), 'Bookmark this page');
  deliver(bar(DEFAULT));
  assert.equal(root.style.getPropertyValue('--tb-extra'), '0px');
});

test('a command button shows the command’s name, runs only through its place in the list, and goes when the item does', () => {
  const tools = [{ k: 'cmd:new-private-tab', l: 'New private tab', i: 6 }, { k: 'chain:2', l: 'Morning', i: 7 }];
  deliver(bar('back,forward,reload,site-info,split,reader,cmd:new-private-tab,chain:2', { tools, extra: 64 }));
  assert.deepEqual(rightLabels(), ['New private tab', 'Morning', 'Menu']);
  sent.length = 0;
  actions().children[0].click();
  actions().children[1].click();
  assert.deepEqual(sent, [{ v: 1, t: 'tool', b: 'address', k: 'cmd:new-private-tab', i: 6 }, { v: 1, t: 'tool', b: 'address', k: 'chain:2', i: 7 }]);
  const first = actions().children[0];
  deliver(bar('back,forward,reload,site-info,split,reader,cmd:new-private-tab,chain:2', { tools, extra: 64 }));
  assert.equal(actions().children[0], first, 'an unchanged button is kept (and keeps its focus)');
  deliver(bar('back,forward,reload,site-info,split,reader,chain:2', { tools: [{ k: 'chain:2', l: 'Morning', i: 6 }], extra: 32 }));
  assert.deepEqual(rightLabels(), ['Morning', 'Menu']);
  deliver(bar('back,forward,reload,site-info,split,reader,cmd:gone', { tools: [], extra: 32 }));
  assert.deepEqual(rightLabels(), ['Menu'], 'a command that no longer exists shows no button');
  deliver(bar(DEFAULT));
});

test('Downloads and Extensions off the list never show, and never take room', () => {
  deliver(bar('back,forward,reload,site-info,split'));
  deliver({ v: 1, t: 'dlbtn', on: true, state: 'active', n: 1, pct: 10, burst: 0 });
  deliver({ v: 1, t: 'extbtn', on: true, n: 1, name: 'One' });
  assert.equal(root.classList.contains('has-dl'), false);
  assert.equal(root.classList.contains('has-ext'), false);
  assert.equal(root.querySelector('.tb-dl'), null, 'the button is not on the bar');
  assert.equal(root.querySelector('.tb-ext'), null, 'the button is not on the bar');
  assert.deepEqual(rightLabels(), ['Menu']);
  deliver(bar(DEFAULT));
  deliver({ v: 1, t: 'dlbtn', on: true, state: 'active', n: 1, pct: 10, burst: 0 });
  assert.equal(root.querySelector('.tb-dl').hidden, false, 'back on the list, it shows again');
  assert.equal(root.classList.contains('has-dl'), true);
  deliver({ v: 1, t: 'dlbtn', on: false, state: 'idle', n: 0, pct: 0, burst: 0 });
});

test('the feed button shows only while the page offers a feed, names how many, and opens the Feeds panel', () => {
  const button = root.querySelector('.tb-feed');
  deliver(bar(DEFAULT, { feeds: 0 }));
  assert.equal(button.hidden, true);
  deliver(bar(DEFAULT, { feeds: 1 }));
  assert.equal(button.hidden, false);
  assert.equal(button.getAttribute('aria-label'), 'Subscribe to this page’s feed');
  deliver(bar(DEFAULT, { feeds: 3 }));
  assert.equal(button.getAttribute('aria-label'), 'This page offers 3 feeds');
  sent.length = 0;
  button.click();
  assert.deepEqual(sent, [{ v: 1, t: 'open', a: 'feeds' }]);
  deliver(bar(DEFAULT, { feeds: 0 }));
  assert.equal(button.hidden, true);
});

test('the toolbar CSS shifts the group by --tb-extra and keeps the slot rules the download and extensions buttons use', () => {
  const css = readFileSync(join(srcDir, 'styles', 'toolbar.css'), 'utf8').replace(/\r\n/g, '\n');
  assert.match(css, /#app\.placed \.actions \{\n  position: absolute;\n  left: var\(--menu-x\);[\s\S]*?translate: calc\(var\(--tb-extra, 0px\) \* -1\) 0;/);
  assert.match(css, /\.tb-extra \{\n  margin-inline-end: var\(--bw-space-0_5\);/);
  assert.match(css, /\.tb-feed\[hidden\] \{\n  display: none;/);
  assert.match(css, /\.pill\.no-site \.omni,/);
});
