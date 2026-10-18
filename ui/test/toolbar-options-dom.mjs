// Wave-1 fixes: what the address bar, search field and toolbar options do to the real toolbar
// (src/main.ts and src/view.ts), driven through the same inbound messages Rust sends, against a small
// fake DOM (no browser). The messages come from the Rust-written fixture (address-messages.json).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, '..', 'src');
const fixture = JSON.parse(readFileSync(join(here, 'fixtures', 'address-messages.json'), 'utf8'));

function bundleSource(contents) {
  const out = esbuild.buildSync({ stdin: { contents, resolveDir: srcDir, loader: 'ts' }, bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return `data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`;
}

// ---- a small fake DOM (as test/translate-ui.mjs, with what text fields need) ----
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
    this.value = ''; this.selectionStart = 0; this.selectionEnd = 0;
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
  append(...nodes) { for (const n of nodes) { const node = typeof n === 'string' ? new Node('#text', n) : n; this.ownText = null; node.parent = this; this.childList.push(node); } }
  prepend(...nodes) { for (const n of nodes.reverse()) { n.parent = this; this.childList.unshift(n); } }
  replaceChildren(...nodes) { this.childList = []; this.ownText = null; this.append(...nodes); }
  after() {}
  remove() {}
  all() { return this.childList.flatMap((c) => [c, ...c.all()]); }
  matches(s) { return s.split(',').some((x) => matchOne(this, x.trim())); }
  closest() { return null; }
  querySelectorAll(s) { return this.all().filter((n) => n.tagName !== '#text' && n.matches(s)); }
  querySelector(s) { return this.querySelectorAll(s)[0] ?? null; }
  focus() { if (document.activeElement === this) return; document.activeElement?.dispatch?.('blur'); document.activeElement = this; this.dispatch('focus'); }
  blur() { if (document.activeElement !== this) return; document.activeElement = null; this.dispatch('blur'); }
  select() { this.selectionStart = 0; this.selectionEnd = this.value.length; }
  setSelectionRange(start, end) { this.selectionStart = start; this.selectionEnd = end; }
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

const viewMod = await import(bundleSource("export { applyState, buildView, renderDisplay } from './view';"));
// main.ts starts the toolbar on import (no ?view means the toolbar), exactly as the browser does.
await import(bundleSource("import './main';"));
await new Promise(setImmediate);

const omni = root.querySelector('.omni');
const search = root.querySelector('.omni-search');
const searchInput = root.querySelector('.omni-search-input');
const tile = root.querySelector('.tb-tile');
const historyButton = root.querySelector('.tb-hist');
const addr = (patch) => ({ ...fixture.addr, ...patch });
const sfield = (patch) => ({ ...fixture.sfield, ...patch });

test('the toolbar was built by main.ts', () => {
  for (const node of [omni, search, searchInput, tile, historyButton]) assert.ok(node);
});

test('search field mode: off hides the field, button shows the engine button, field shows the box', () => {
  deliver(sfield({ mode: 'off' }));
  assert.equal(search.hidden, true);
  deliver(sfield({ mode: 'button' }));
  assert.equal(search.hidden, false);
  assert.ok(search.classList.contains('is-button'));
  deliver(sfield({ mode: 'field' }));
  assert.equal(search.hidden, false);
  assert.ok(!search.classList.contains('is-button'));
});

test('the engine icon shows only with "Show the engine icon" on', () => {
  deliver(sfield({ icon: false }));
  assert.equal(tile.hidden, true);
  deliver(sfield({ icon: true }));
  assert.equal(tile.hidden, false);
  assert.equal(tile.textContent !== '', true, 'the engine monogram');
});

test('Enter searches with the chosen engine, and keeps the typed text only with "Keep the text" on', () => {
  for (const keep of [false, true]) {
    deliver(sfield({ mode: 'field', keep }));
    searchInput.value = 'rust lifetimes';
    sent.length = 0;
    searchInput.dispatch('keydown', { key: 'Enter', altKey: false, shiftKey: false, ctrlKey: false, metaKey: false, isComposing: false, keyCode: 13 });
    const searched = sent.find((m) => m.a === 'search');
    assert.ok(searched, JSON.stringify(sent));
    assert.equal(searched.text, 'rust lifetimes');
    assert.equal(searched.id, fixture.sfield.engine, 'the engine the field shows');
    assert.equal(searchInput.value, keep ? 'rust lifetimes' : '', `keep ${keep}`);
  }
});

test('the typed-history button shows only with its option on', () => {
  deliver(addr({ hbtn: true }));
  assert.equal(historyButton.hidden, false);
  deliver(addr({ hbtn: false }));
  assert.equal(historyButton.hidden, true);
});

test('select on activation holds for keyboard focus as well as a click', () => {
  omni.blur();
  searchInput.blur();
  omni.value = 'https://a.example/path';
  deliver(addr({ select: true }));
  omni.focus();
  assert.deepEqual([omni.selectionStart, omni.selectionEnd], [0, omni.value.length], 'on: Tab selects the address');
  omni.blur();
  deliver(addr({ select: false }));
  omni.setSelectionRange(0, 0);
  omni.focus();
  assert.deepEqual([omni.selectionStart, omni.selectionEnd], [omni.value.length, omni.value.length], 'off: the caret goes after the address');
  // An address the user has edited keeps its caret when focus comes back.
  omni.blur();
  deliver(addr({ select: true }));
  omni.focus();
  omni.dispatch('input');
  omni.setSelectionRange(3, 3);
  document.activeElement = null;
  omni.dispatch('focus');
  assert.deepEqual([omni.selectionStart, omni.selectionEnd], [3, 3]);
  omni.blur();
});

test('select on click: a first click on the untouched address selects it, only with the option on', () => {
  for (const select of [true, false]) {
    omni.blur();
    omni.value = 'https://a.example/';
    deliver(addr({ select }));
    omni.setSelectionRange(5, 5);
    omni.dispatch('pointerdown');
    omni.focus();
    omni.dispatch('pointerup');
    const all = omni.selectionStart === 0 && omni.selectionEnd === omni.value.length;
    assert.equal(all, select, `select ${select}`);
  }
  omni.blur();
});

test('renderDisplay: the unfocused address emphasises the host, the base domain with highlight on', () => {
  const view = viewMod.buildView(new Node('div'));
  const hosts = () => view.display.querySelectorAll('.u-host').map((n) => n.textContent);
  assert.equal(viewMod.renderDisplay(view, 'https://www.example.co.uk/a?b', { full: false, highlight: false, base: '' }), true);
  assert.equal(hosts().length, 1);
  assert.ok(hosts()[0].includes('example.co.uk'));
  viewMod.renderDisplay(view, 'https://www.example.co.uk/a', { full: false, highlight: true, base: 'example.co.uk' });
  assert.deepEqual(hosts(), ['example.co.uk'], 'highlight: only the base domain is emphasised');
  assert.equal(viewMod.renderDisplay(view, 'not an address', { full: false, highlight: false, base: '' }), false, 'plain text has no display form');
});

const STATE = { t: 'state', scale: 1, solid: true, dark: true, back: false, forward: false, loading: true, progress: 40, security: 'secure', label: 'Secure', zoom: 125, theme: {} };

test('show_load_progress: the bar shows while loading only with the option on', () => {
  const view = viewMod.buildView(new Node('div'));
  viewMod.applyState(view, { ...STATE, showProgress: true });
  assert.equal(view.progress.style.opacity, '1');
  viewMod.applyState(view, { ...STATE, showProgress: false });
  assert.equal(view.progress.style.opacity, '0');
  assert.ok(!view.pill.classList.contains('is-loading'));
});

test('show_zoom_indicator: the zoom chip shows at 125% only with the option on', () => {
  const view = viewMod.buildView(new Node('div'));
  viewMod.applyState(view, { ...STATE, showZoom: true });
  assert.equal(view.zoom.hidden, false);
  viewMod.applyState(view, { ...STATE, showZoom: false });
  assert.equal(view.zoom.hidden, true);
  viewMod.applyState(view, { ...STATE, zoom: 100, showZoom: true });
  assert.equal(view.zoom.hidden, true, 'never at 100%');
});
