// Page translation, the HTML-chrome half: the toolbar `translate` pill in the state
// message, the `translate` open target, the Translate button (hidden / offered / on, its names and
// what a click sends), the `translate` glyph, and the translation members of the settings snapshot.
// The Settings page's own controls are exercised in test/settings-page.mjs (it owns the fake page).
// Runs the real view and main code against a small fake DOM (no browser needed).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, '..', 'src');
const read = (path) => readFileSync(path, 'utf8').replace(/\r\n/g, '\n');
const rust = JSON.parse(read(join(here, 'fixtures', 'rust-messages.json')));
const settingsFixture = JSON.parse(read(join(here, 'fixtures', 'settings-messages.json')));

function bundleSource(contents) {
  const out = esbuild.buildSync({ stdin: { contents, resolveDir: srcDir, loader: 'ts' }, bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return `data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`;
}

// ---- a small fake DOM ----
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
    const props = new Map();
    this.style = { setProperty: (k, v) => props.set(k, String(v)), getPropertyValue: (k) => props.get(k) ?? '', removeProperty: (k) => props.delete(k) };
  }
  // Under the strict page's Trusted Types this sink throws; main.ts probes it before it will start.
  set innerHTML(_value) { throw new TypeError('Trusted Types'); }
  get children() { return this.childList.filter((c) => c.tagName !== '#text'); }
  get textContent() { return this.ownText ?? this.childList.map((c) => c.textContent).join(''); }
  set textContent(v) { this.childList = []; this.ownText = null; if (v !== '') this.append(new Node('#text', String(v))); }
  setAttribute(k, v) { this.attrs.set(k, String(v)); if (k === 'id') this.id = String(v); if (k === 'class') this.className = String(v); }
  getAttribute(k) { return this.attrs.has(k) ? this.attrs.get(k) : null; }
  hasAttribute(k) { return this.attrs.has(k); }
  removeAttribute(k) { this.attrs.delete(k); }
  addEventListener(type, cb) { (this.listeners[type] ??= []).push(cb); }
  removeEventListener() {}
  click() { for (const cb of this.listeners.click ?? []) cb({ type: 'click', target: this, preventDefault() {}, stopPropagation() {} }); }
  append(...nodes) { for (const n of nodes) { const node = typeof n === 'string' ? new Node('#text', n) : n; this.ownText = null; node.parent = this; this.childList.push(node); } }
  prepend(...nodes) { for (const n of nodes.reverse()) { n.parent = this; this.childList.unshift(n); } }
  replaceChildren(...nodes) { this.childList = []; this.ownText = null; this.append(...nodes); }
  all() { return this.childList.flatMap((c) => [c, ...c.all()]); }
  matches(s) { return s.split(',').some((x) => matchOne(this, x.trim())); }
  querySelectorAll(s) { return this.all().filter((n) => n.tagName !== '#text' && n.matches(s)); }
  querySelector(s) { return this.querySelectorAll(s)[0] ?? null; }
  focus() {}
  contains(n) { return n === this || this.all().includes(n); }
  getBoundingClientRect() { return { left: 0, top: 0, right: 28, bottom: 28, width: 28, height: 28 }; }
}
const sent = [];
const wires = [];
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
  hasFocus: () => false,
};
Object.assign(globalThis, {
  window: globalThis,
  location: { search: '' },
  matchMedia: () => ({ matches: false }),
  requestAnimationFrame: () => 0,
  cancelAnimationFrame() {},
  addEventListener() {}, removeEventListener() {},
  getComputedStyle: () => ({}),
  CSS: { escape: (s) => s },
  chrome: { webview: { postMessage: (wire) => { wires.push(wire); sent.push(JSON.parse(wire)); }, addEventListener: (_t, cb) => listeners.push(cb) } },
});
globalThis.Element = Node;
globalThis.HTMLElement = Node;
globalThis.HTMLButtonElement = Node;
globalThis.HTMLInputElement = Node;
const deliver = (msg) => { for (const cb of listeners.slice()) cb({ data: JSON.stringify(msg) }); };

const protocol = await import(bundleSource("export * from './protocol';"));
const viewMod = await import(bundleSource("export { applyState, applyTranslate, buildView } from './view';"));
const logic = await import(bundleSource("export * from './settings-page-logic';"));
const menu = await import(bundleSource("export * from './menu-logic';"));
// main.ts starts the toolbar on import (no ?view means the toolbar), exactly as the browser does.
await import(bundleSource("import './main';"));

const STATE = { t: 'state', scale: 1, solid: true, dark: true, back: false, forward: false, loading: false, progress: 0, security: 'secure', label: 'Secure', theme: {} };
const stateMsg = (patch, drop = []) => {
  const message = { ...STATE, ...patch };
  for (const key of drop) delete message[key];
  return message;
};
const parse = (patch, drop = []) => protocol.parseInbound(JSON.stringify(stateMsg(patch, drop)));

test('state: the translate pill parses for none, available and on', () => {
  for (const pill of ['none', 'available', 'on']) {
    const msg = parse({ translate: pill });
    assert.equal(msg?.t, 'state', pill);
    assert.equal(msg.translate, pill);
  }
});

test('state: a bad translate value rejects the whole message', () => {
  for (const bad of ['', 'On', 'yes', 'reader', true, 1, null, ['on'], {}]) {
    assert.equal(parse({ translate: bad }), null, JSON.stringify(bad));
  }
});

test('state: a message without translate (an older host) parses and means hidden', () => {
  const msg = parse({});
  assert.equal(msg?.t, 'state');
  assert.equal('translate' in msg, false);
  const view = viewMod.buildView(new Node('div'));
  viewMod.applyState(view, msg);
  assert.equal(view.translate.hidden, true);
});

test('state: translate and reader are separate members and each is validated', () => {
  const msg = parse({ reader: 'available', translate: 'on' });
  assert.deepEqual([msg.reader, msg.translate], ['available', 'on']);
  assert.equal(parse({ reader: 'bogus', translate: 'on' }), null);
  assert.equal(parse({ reader: 'on', translate: 'bogus' }), null);
});

test("the Rust fixture's state message carries translate right after reader", () => {
  const keys = Object.keys(rust.state);
  assert.equal(keys.indexOf('translate'), keys.indexOf('reader') + 1, 'order on the wire');
  assert.equal(protocol.parseInbound(rust.state).translate, 'none');
});

test('toolbar: the translate button follows the reader button in the pill, hidden by default', () => {
  const view = viewMod.buildView(new Node('div'));
  const at = (name) => view.pill.children.findIndex((c) => c.classList.contains(name));
  assert.equal(at('tb-translate'), at('tb-reader') + 1);
  assert.equal(at('tb-split'), at('tb-translate') + 1);
  assert.equal(view.translate.tagName, 'button');
  assert.equal(view.translate.hidden, true);
  assert.equal(view.translate.getAttribute('aria-label'), 'Translate page');
  assert.equal(view.translate.getAttribute('data-tip'), 'Translate this page');
  assert.equal(view.translate.children.length, 1, 'one glyph');
});

test('toolbar: hidden for none, offered for available, pressed and "Show original" for on', () => {
  const view = viewMod.buildView(new Node('div'));
  viewMod.applyState(view, parse({ translate: 'none' }));
  assert.equal(view.translate.hidden, true);

  viewMod.applyState(view, parse({ translate: 'available' }));
  assert.equal(view.translate.hidden, false);
  assert.equal(view.translate.getAttribute('aria-pressed'), 'false');
  assert.equal(view.translate.getAttribute('aria-label'), 'Translate page');
  assert.equal(view.translate.getAttribute('data-tip'), 'Translate this page');

  viewMod.applyState(view, parse({ translate: 'on' }));
  assert.equal(view.translate.hidden, false);
  assert.equal(view.translate.getAttribute('aria-pressed'), 'true');
  assert.equal(view.translate.getAttribute('aria-label'), 'Show original');

  viewMod.applyState(view, parse({ translate: 'available' }));
  assert.equal(view.translate.getAttribute('aria-pressed'), 'false');
  assert.equal(view.translate.getAttribute('aria-label'), 'Translate page', 'the names come back when the translation is turned off');

  viewMod.applyState(view, parse({ translate: 'none' }));
  assert.equal(view.translate.hidden, true);
  viewMod.applyState(view, parse({ translate: 'on' }));
  viewMod.applyState(view, parse({}));
  assert.equal(view.translate.hidden, true, 'a message without the member hides the button again');
});

test('toolbar: the translate and reader buttons do not affect each other', () => {
  const view = viewMod.buildView(new Node('div'));
  viewMod.applyState(view, parse({ reader: 'on', translate: 'available' }));
  assert.equal(view.reader.hidden, false);
  assert.equal(view.reader.getAttribute('aria-pressed'), 'true');
  assert.equal(view.translate.getAttribute('aria-pressed'), 'false');
  viewMod.applyState(view, parse({ reader: 'none', translate: 'on' }));
  assert.equal(view.reader.hidden, true);
  assert.equal(view.translate.hidden, false);
});

test('clicking the toolbar translate button posts exactly {"v":1,"t":"open","a":"translate"}', () => {
  const button = root.querySelector('.tb-translate');
  assert.ok(button, 'main.ts built the toolbar');
  deliver(stateMsg({ translate: 'available' }));
  assert.equal(button.hidden, false);
  wires.length = 0;
  button.click();
  assert.deepEqual(wires, ['{"v":1,"t":"open","a":"translate"}']);
  // The reader button keeps its own target.
  wires.length = 0;
  root.querySelector('.tb-reader').click();
  assert.deepEqual(wires, ['{"v":1,"t":"open","a":"reader"}']);
});

test('the translate glyph is in the icon family and the menu may use it', () => {
  const svg = read(join(here, '..', '..', 'icons', 'extra', 'translate.svg'));
  assert.match(svg, /viewBox="0 0 24 24"/);
  assert.match(read(join(srcDir, 'icons.gen.ts')), /\btranslate\b/);
  assert.equal(menu.MENU_ICONS.filter((name) => name === 'translate').length, 1);
});

test('toolbar css: the translate button hides with `hidden` and drops out of a narrow pill with the reader button', () => {
  const css = read(join(srcDir, 'styles', 'toolbar.css'));
  assert.match(css, /\.tb-translate\[hidden\] \{\n\s+display: none;/);
  assert.match(css, /@container \(max-width: 190px\) \{\n\s+\.tb-split,\n\s+\.tb-reader,\n\s+\.tb-translate \{\n\s+display: none;/);
});

// ---- the settings snapshot ----

const snap = () => structuredClone(settingsFixture.snap.d);

test('settings snapshot: the Rust fixture carries the translate members after downloadAsk', () => {
  const d = snap();
  const keys = Object.keys(d);
  const from = keys.indexOf('downloadAsk');
  assert.deepEqual(keys.slice(from, from + 5), ['downloadAsk', 'translateOffer', 'translateAuto', 'translateTarget', 'translateTargets']);
  const s = logic.parseSettingsSnapshot(d);
  assert.deepEqual([s.translateOffer, s.translateAuto, s.translateTarget], [true, false, '']);
  assert.deepEqual(s.translateTargets.slice(0, 4), ['en', 'es', 'fr', 'de']);
  assert.ok(s.translateTargets.length > 40 && s.translateTargets.length <= 64, String(s.translateTargets.length));
  for (const code of s.translateTargets) assert.notEqual(logic.languageName(code), code, code + ' has an English name');
  assert.ok(logic.parseSettingsSnapshot(settingsFixture.guest.d));
  assert.ok(logic.parseSettingsSnapshot({ ...d, translateTarget: 'fr', translateAuto: true, translateOffer: false }));
  assert.ok(logic.parseSettingsSnapshot({ ...d, translateTargets: [] }));
  assert.ok(logic.parseSettingsSnapshot({ ...d, translateTargets: ['fil', 'en'] }), 'three letters are fine');
  assert.ok(logic.parseSettingsSnapshot({ ...d, translateTargets: Array.from({ length: 64 }, () => 'en') }), '64 entries are the limit');
});

test('settings snapshot: wrong types and bad language codes are refused', () => {
  const d = snap();
  const bad = [
    { translateOffer: 'yes' }, { translateOffer: 1 }, { translateOffer: null },
    { translateAuto: 'no' }, { translateAuto: 0 },
    { translateTarget: 7 }, { translateTarget: null }, { translateTarget: 'EN' }, { translateTarget: 'e' }, { translateTarget: 'engl' },
    { translateTarget: 'e1' }, { translateTarget: 'en-GB' }, { translateTarget: ' en' }, { translateTarget: 'en\n' },
    { translateTargets: 'en' }, { translateTargets: null }, { translateTargets: {} }, { translateTargets: [7] }, { translateTargets: [null] },
    { translateTargets: [['en']] }, { translateTargets: ['EN'] }, { translateTargets: ['e'] }, { translateTargets: ['engl'] },
    { translateTargets: ['en', ''] }, { translateTargets: ['en', 'de-DE'] }, { translateTargets: ['en', 'd3'] }, { translateTargets: ['é'] },
    { translateTargets: Array.from({ length: 65 }, () => 'en') },
  ];
  for (const patch of bad) assert.equal(logic.parseSettingsSnapshot({ ...d, ...patch }), null, JSON.stringify(patch).slice(0, 80));
  for (const key of ['translateOffer', 'translateAuto', 'translateTarget', 'translateTargets']) {
    const missing = { ...d };
    delete missing[key];
    assert.equal(logic.parseSettingsSnapshot(missing), null, `missing ${key}`);
  }
  assert.equal(logic.parseSettingsSnapshot({ ...d, translateExtra: 1 }), null);
});

test('settings snapshot: the translate settings never ride in the Save-all text', () => {
  const s = logic.parseSettingsSnapshot(snap());
  const draft = logic.draftOf(s);
  for (const key of ['translateOffer', 'translateAuto', 'translateTarget', 'translateTargets']) assert.equal(key in draft, false, key);
  assert.doesNotMatch(logic.saveText(draft), /translat/i);
});

test('settings rows: three translation rows with the agreed words, in a real section, searchable', () => {
  const byId = (id) => logic.ROWS.find((row) => row.id === id);
  assert.equal(byId('translateOffer').label, 'Offer translate');
  assert.equal(byId('translateOffer').description, '');
  assert.match(byId('translateOffer').keywords, /Google Translate/);
  assert.equal(byId('translateAuto').label, 'Auto-translate');
  assert.equal(byId('translateAuto').description, '');
  assert.match(byId('translateAuto').keywords, /Google Translate/);
  assert.equal(byId('translateTarget').label, 'Translate to');
  for (const id of ['translateOffer', 'translateAuto', 'translateTarget']) {
    assert.ok(logic.isSection(byId(id).section), id);
    assert.ok(logic.matchRows('translate translation language foreign page').has(id), id);
    assert.ok(!byId(id).restart && !byId(id).notYet, `${id} is applied at once`);
  }
});

test('settings: "Translate into" options are the Windows language first, then each code with its English name', () => {
  const options = logic.translateTargetOptions(['en', 'es', 'fr', 'de', 'ja', 'fil', 'zh']);
  assert.deepEqual(options, [
    { value: '', label: 'My Windows language' },
    { value: 'en', label: 'English' }, { value: 'es', label: 'Spanish' }, { value: 'fr', label: 'French' }, { value: 'de', label: 'German' },
    { value: 'ja', label: 'Japanese' }, { value: 'fil', label: 'Filipino' }, { value: 'zh', label: 'Chinese' },
  ]);
  assert.deepEqual(logic.translateTargetOptions(['xx', 'qqq']).slice(1), [{ value: 'xx', label: 'xx' }, { value: 'qqq', label: 'qqq' }], 'an unknown code shows the code');
  assert.deepEqual(logic.translateTargetOptions([]), [{ value: '', label: 'My Windows language' }]);
  assert.equal(logic.languageName('constructor'), 'constructor', 'prototype names are not languages');
});
