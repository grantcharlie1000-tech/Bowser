// Settings page (docs/design/settings-page.md): the snapshot and settingsgo contracts against the
// Rust fixture, the exact-match save text, the section/row index and search, the words that depend
// on state, the motion and token rules of settings.css, and the page's DOM in a fake document
// (landmarks, labels, switches, what a click sends).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');
const srcDir = join(here, '..', 'src');
const read = (path) => readFileSync(path, 'utf8').replace(/\r\n/g, '\n');

async function bundle(file) {
  const out = esbuild.buildSync({ entryPoints: [join(srcDir, file)], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
}
const logic = await bundle('settings-page-logic.ts');
const { parseInbound } = await bundle('protocol.ts');
const fixture = JSON.parse(read(join(here, 'fixtures', 'settings-messages.json')));

test('the Rust snapshots pass the strict snapshot contract and anything else is refused', () => {
  for (const message of [fixture.snap, fixture.guest]) {
    const inbound = parseInbound(JSON.stringify(message));
    assert.equal(inbound?.t, 'snap');
    assert.ok(logic.parseSettingsSnapshot(inbound.d), JSON.stringify(message.d));
  }
  const d = fixture.snap.d;
  const ok = logic.parseSettingsSnapshot(d);
  assert.equal(ok.profile, 'Work "<b>"', 'text arrives as text');
  assert.deepEqual(ok.blockerAllow, ['example.com', 'news.example']);
  const guest = logic.parseSettingsSnapshot(fixture.guest.d);
  assert.equal(guest.guest, true);
  assert.equal(guest.historyEnabled, false);
  assert.equal(guest.searchFallback, 'custom');
  const bad = [
    { ...d, extra: 1 },
    { ...d, sidebarWidth: 179 },
    { ...d, sidebarWidth: 361 },
    { ...d, tabLayout: 'toolbar-top' },
    { ...d, sleepAfterSecs: 29 },
    { ...d, searchEngine: 'bing' },
    { ...d, searchEngine: 'google', searchFallback: 'custom' },
    { ...d, efficiencyMode: 'sometimes' },
    { ...d, trackingPrevention: 'off' },
    { ...d, blockerAllow: ['bad site'] },
    { ...d, blockerAllow: ['a,b'] },
    { ...d, update: 'soon' },
    { ...d, checkResult: 'maybe' },
    { ...d, ack: 'ok' },
    { ...d, guest: true, historyEnabled: true },
    { ...d, profile: 'x'.repeat(65) },
    { ...d, autoUpdate: 'yes' },
  ];
  for (const value of bad) assert.equal(logic.parseSettingsSnapshot(value), null, JSON.stringify(value).slice(0, 120));
  const { readable: _r, ...missing } = d;
  assert.equal(logic.parseSettingsSnapshot(missing), null);
});

test('settingsgo carries one known section or the top, nothing else', () => {
  assert.deepEqual(parseInbound(JSON.stringify(fixture.go)), { t: 'settingsgo', s: 'privacy' });
  assert.deepEqual(parseInbound(JSON.stringify(fixture.goTop)), { t: 'settingsgo', s: '' });
  for (const s of logic.SETTINGS_SECTIONS) assert.deepEqual(parseInbound({ v: 1, t: 'settingsgo', s }), { t: 'settingsgo', s });
  for (const raw of [{ v: 1, t: 'settingsgo', s: 'Privacy' }, { v: 1, t: 'settingsgo', s: 'nope' }, { v: 1, t: 'settingsgo' }, { v: 1, t: 'settingsgo', s: 'about', x: 1 }, { v: 2, t: 'settingsgo', s: '' }]) {
    assert.equal(parseInbound(raw), null, JSON.stringify(raw));
  }
});

test('sections match the Rust routes, in page order, and every row belongs to one', () => {
  const rust = read(join(root, 'src', 'internal_page.rs'));
  const keys = [...rust.matchAll(/Self::\w+ => "([a-z]+)",/g)].map((m) => m[1]);
  assert.deepEqual(keys, [...logic.SETTINGS_SECTIONS]);
  assert.deepEqual(logic.SECTIONS.map((s) => s.key), [...logic.SETTINGS_SECTIONS]);
  const ids = new Set();
  for (const row of logic.ROWS) {
    assert.ok(logic.isSection(row.section), row.id);
    assert.ok(!ids.has(row.id), `duplicate ${row.id}`);
    ids.add(row.id);
  }
  for (const section of logic.SETTINGS_SECTIONS) assert.ok(logic.ROWS.some((row) => row.section === section), `${section} has rows`);
  // The restart notes match what Rust applies live (chrome_host/settings.rs, settings_page.rs run()).
  const restart = logic.ROWS.filter((row) => row.restart).map((row) => row.id).sort();
  assert.deepEqual(restart, ['allow', 'blocker', 'grouping', 'tracking']);
});

test('save sends every profile setting exactly once in the order Rust expects', () => {
  const s = logic.parseSettingsSnapshot(fixture.snap.d);
  const draft = logic.draftOf(s);
  assert.equal(logic.saveText(draft), 'tab_layout=horizontal\nsidebar_width=260\nsleep_enabled=true\nsleep_after_secs=300\nblocker_enabled=true\nblocker_allow=example.com,news.example\nautomatic_tab_grouping=true\nefficiency_mode=auto\ntracking_prevention=balanced\nsearch_engine=optimised');
  const none = logic.saveText({ ...draft, searchEngine: null });
  assert.doesNotMatch(none, /search_engine/);
  assert.doesNotMatch(logic.saveText(draft), /auto_update|history|efficiencyActive/);
  for (const layout of logic.TAB_LAYOUTS) assert.match(logic.saveText({ ...draft, tabLayout: layout }), new RegExp(`^tab_layout=${layout}\n`));
  for (const broken of [{ sidebarWidth: 999 }, { sleepAfterSecs: 10 }, { blockerAllow: ['a b'] }, { tabLayout: 'sideways' }, { efficiencyMode: 'max' }, { searchEngine: 'bing' }]) {
    assert.equal(logic.saveText({ ...draft, ...broken }), null, JSON.stringify(broken));
  }
  assert.equal(logic.saveText({ ...draft, blockerAllow: Array.from({ length: 300 }, (_, i) => `site${i}.example`) }), null, 'over the 2048 text cap');
});

test('search matches every word across label, description, keywords and section; marks are merged', () => {
  assert.equal(logic.matchRows('  '), null);
  const sleep = logic.matchRows('sleep');
  for (const id of ['sleep', 'sleepAfter', 'efficiency']) assert.ok(sleep.has(id), id);
  assert.ok(!sleep.has('engine'));
  assert.deepEqual([...logic.matchRows('duckduckgo')], ['engine']);
  assert.ok(logic.matchRows('TAB layout').has('layout'));
  assert.ok(logic.matchRows('privacy cookies').has('dashboard'), 'section title plus keyword');
  assert.ok(logic.matchRows('netflix').has('media'));
  assert.equal(logic.matchRows('zzzz').size, 0);
  assert.ok(logic.matchRows('café').size === 0);
  assert.equal(logic.fold('  Café  Au   Lait '), 'cafe au lait');
  assert.deepEqual(logic.highlightRanges('Sleep hidden tabs that sleep', 'sleep'), [[0, 5], [23, 28]]);
  assert.deepEqual(logic.highlightRanges('abcdef', 'abc cde'), [[0, 5]]);
  assert.deepEqual(logic.highlightRanges('abc', ''), []);
  assert.equal(logic.resultCountText(0, ' zz '), 'No settings match “zz”');
  assert.equal(logic.resultCountText(1, 'a'), '1 setting found');
  assert.equal(logic.resultCountText(3, 'a'), '3 settings found');
});

test('values: sleep delays, sites for the allow list and the update card', () => {
  assert.equal(logic.durationLabel(30), '30 seconds');
  assert.equal(logic.durationLabel(60), '1 minute');
  assert.equal(logic.durationLabel(14400), '4 hours');
  assert.equal(logic.durationLabel(604800), '7 days');
  assert.equal(logic.durationLabel(90), '90 seconds');
  assert.deepEqual(logic.sleepOptions(300), [...logic.SLEEP_PRESETS]);
  assert.ok(logic.sleepOptions(450).includes(450), 'a hand-edited value stays selectable');
  assert.equal(logic.normaliseSite(' https://WWW.Example.com:8080/path?q '), 'example.com');
  assert.equal(logic.normaliseSite('news.example.org'), 'news.example.org');
  for (const bad of ['', 'a b', 'exa,mple.com', '-bad.example', 'http://', 'a..b']) assert.equal(logic.normaliseSite(bad), null, bad);
  assert.deepEqual(logic.addSite(['a.example'], 'B.example'), { list: ['a.example', 'b.example'] });
  assert.ok('error' in logic.addSite(['a.example'], 'a.example'));
  assert.ok('error' in logic.addSite(Array.from({ length: logic.MAX_ALLOW }, (_, i) => `s${i}.example`), 'new.example'));
  const base = { update: 'idle', updateVersion: '', lastCheck: 'none', checkResult: 'none', version: '0.4.2', autoUpdate: true };
  assert.equal(logic.updateView({ ...base, update: 'unavailable' }).action, null);
  assert.equal(logic.updateView({ ...base, update: 'checking' }).busy, true);
  const ready = logic.updateView({ ...base, update: 'ready', updateVersion: '0.4.3' });
  assert.equal(ready.action, 'restart');
  assert.match(ready.title, /0\.4\.3/);
  assert.equal(logic.updateView({ ...base, checkResult: 'failed' }).tone, 'warn');
  assert.equal(logic.updateView({ ...base, checkResult: 'uptodate' }).title, 'Bowser is up to date.');
  assert.equal(logic.updateView({ ...base, lastCheck: 'uptodate' }).action, 'check');
  assert.match(logic.updateView({ ...base, autoUpdate: false }).detail, /off/);
});

test('settings.css: transform and opacity motion only, finite, off under reduced motion, tokens only', () => {
  const css = read(join(srcDir, 'styles', 'settings.css'));
  assert.match(css.slice(0, 600), /Surface: SETTINGS PAGE.*Owner:/s);
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
  for (const selector of ['.sp-mark.is-turn svg', '.sp-card.is-flash::after', '.sp-toast', '.sp-tile:active']) assert.ok(reduced.includes(selector), selector);
  const menu = read(join(srcDir, 'styles', 'menu.css'));
  assert.match(menu, /\.html-menu-row\[data-icon="gear"\]\.selected:not\(:disabled\) \.html-menu-lead svg \{\n\s+transform: rotate\(60deg\);/);
});

test('the page never changes its own URL and names no store it must not touch', () => {
  const page = read(join(srcDir, 'settings-page.ts'));
  for (const banned of [/location\.(hash|href|assign|replace)/, /history\.(pushState|replaceState)/, /\.href\s*=/, /window\.open/, /createElement\('a'\)|el\('a'/]) {
    assert.doesNotMatch(page, banned);
  }
  // Mirrors src/chrome_host/guard.rs for the two new sources.
  for (const file of ['settings-page.ts', 'settings-page-logic.ts']) {
    const text = read(join(srcDir, file)).toLowerCase();
    for (const term of [['pass', 'words'].join(''), ['va', 'ult'].join(''), ['creden', 'tial'].join('')]) assert.ok(!text.includes(term), `${file}: ${term}`);
  }
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
    this.ownText = text; this.id = ''; this.type = ''; this.offsetTop = 0; this.offsetHeight = 34; this.offsetWidth = 0;
    this.scrollTop = 0; this.clientHeight = 800; this.scrollHeight = 4000;
    const props = new Map();
    this.style = { setProperty: (k, v) => props.set(k, v), getPropertyValue: (k) => props.get(k) ?? '' };
  }
  get textContent() { return this.ownText ?? this.children.map((c) => c.textContent).join(''); }
  set textContent(v) { this.children = []; this.ownText = null; if (v !== '') this.append(new Node('#text', String(v))); }
  get childElementCount() { return this.children.filter((c) => c.tagName !== '#text').length; }
  get lastChild() { return this.children[this.children.length - 1] ?? null; }
  get lastElementChild() { return [...this.children].reverse().find((c) => c.tagName !== '#text') ?? null; }
  setAttribute(k, v) { this.attrs.set(k, String(v)); if (k === 'id') this.id = String(v); }
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
  dispatch(type, event = {}) { for (const cb of this.listeners[type] ?? []) cb({ target: this, preventDefault() {}, key: '', ...event }); }
  click() { if (!this.disabled) this.dispatch('click'); }
  detach(node) { if (node.parent) node.parent.children = node.parent.children.filter((c) => c !== node); node.parent = this; }
  append(...nodes) { for (const n of nodes) { const node = typeof n === 'string' ? new Node('#text', n) : n; this.ownText = null; this.detach(node); this.children.push(node); } }
  prepend(node) { this.detach(node); this.children.unshift(node); }
  after(node) { const p = this.parent; p.detach(node); p.children.splice(p.children.indexOf(this) + 1, 0, node); }
  replaceWith(node) { const p = this.parent; const at = p.children.indexOf(this); p.detach(node); p.children.splice(at, 1, node); this.parent = null; }
  replaceChildren(...nodes) { this.children = []; this.ownText = null; this.append(...nodes); }
  remove() { if (this.parent) this.parent.children = this.parent.children.filter((c) => c !== this); this.parent = null; }
  all() { return this.children.flatMap((c) => [c, ...c.all()]); }
  matches(s) { return sel(s).some((x) => matchOne(this, x)); }
  querySelectorAll(s) { return this.all().filter((n) => n.tagName !== '#text' && n.matches(s)); }
  querySelector(s) { return this.querySelectorAll(s)[0] ?? null; }
  focus() { globalThis.document.activeElement = this; }
  scrollTo(o) { this.scrollTop = o.top ?? 0; }
  scrollIntoView() {}
  contains(n) { return n === this || this.all().includes(n); }
  getBoundingClientRect() { return { left: 0, top: 0, right: 0, bottom: 0, width: 0, height: 0 }; }
}
const sent = [];
const listeners = [];
const documentElement = new Node('html');
const body = new Node('body');
globalThis.document = {
  documentElement, body, activeElement: null,
  createElement: (tag) => new Node(tag),
  createElementNS: (_ns, tag) => new Node(tag),
  createTextNode: (text) => new Node('#text', text),
  addEventListener() {}, removeEventListener() {},
};
globalThis.window = globalThis;
Object.assign(globalThis, {
  innerWidth: 1280, innerHeight: 860,
  matchMedia: () => ({ matches: false }),
  requestAnimationFrame: (cb) => { cb(); return 1; },
  addEventListener() {}, removeEventListener() {},
  IntersectionObserver: class { observe() {} disconnect() {} },
  chrome: { webview: { postMessage: (wire) => sent.push(JSON.parse(wire)), addEventListener: (_t, cb) => listeners.push(cb) } },
});
globalThis.Element = Node;
globalThis.HTMLElement = Node;
globalThis.HTMLInputElement = class {};
const { startSettingsPage } = await bundle('settings-page.ts');
const app = new Node('div');
startSettingsPage(app);
const deliver = (msg) => { for (const cb of listeners) cb({ data: JSON.stringify(msg) }); };
deliver(fixture.snap);

test('the page renders landmarks, every section and row, and labelled controls', () => {
  assert.deepEqual(sent[0], { v: 1, t: 'ready' });
  assert.ok(app.querySelector('nav')?.getAttribute('aria-label'), 'nav landmark');
  assert.equal(app.querySelector('main')?.getAttribute('aria-label'), 'Settings');
  assert.ok(app.querySelector('[role="search"]'));
  assert.equal(app.querySelectorAll('.sp-nav-item').length, 8);
  assert.equal(app.querySelectorAll('section').length, 8);
  for (const section of app.querySelectorAll('section')) assert.ok(section.getAttribute('aria-labelledby'));
  const rows = app.querySelectorAll('.sp-row').map((row) => row.dataset.row);
  assert.deepEqual(rows, logic.ROWS.map((row) => row.id));
  for (const control of app.querySelectorAll('[role="switch"], [role="radiogroup"]')) {
    assert.ok(control.getAttribute('aria-labelledby') || control.getAttribute('aria-label'), control.className);
  }
  assert.equal(app.querySelector('[role="status"]')?.getAttribute('aria-live'), 'polite');
  assert.equal(app.querySelector('.sp-avatar').textContent, 'W');
  assert.equal(app.querySelector('.sp-tile[aria-checked="true"]')?.querySelector('.sp-tile-name').textContent, 'Toolbar above tabs');
});

test('a change saves the whole draft; a toggle answer shows "Saved"', () => {
  sent.length = 0;
  const vertical = app.querySelectorAll('.sp-tile')[3];
  vertical.click();
  assert.equal(sent.length, 1);
  assert.equal(sent[0].t, 'set');
  assert.equal(sent[0].a, 'save');
  assert.match(sent[0].text, /^tab_layout=vertical\n/);
  const history = app.querySelector('.sp-row[data-row="history"]').querySelector('[role="switch"]');
  history.click();
  assert.deepEqual(sent[1], { v: 1, t: 'set', a: 'history', on: false });
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, tabLayout: 'vertical', ack: 'saved' } });
  assert.equal(app.querySelector('.sp-toast').textContent, 'Saved');
  const nav = app.querySelectorAll('.sp-nav-item')[2];
  sent.length = 0;
  nav.click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'section', text: 'privacy' }]);
  assert.equal(nav.getAttribute('aria-current'), 'true');
});

test('search hides what does not match and settingsgo moves without echoing', () => {
  const input = app.querySelector('.sp-search-input');
  input.value = 'img';
  input.dispatch('input');
  const shown = app.querySelectorAll('.sp-row').filter((row) => !row.hidden).map((row) => row.dataset.row);
  assert.deepEqual(shown, ['engine']);
  assert.ok(app.querySelector('mark'), 'the hit is marked');
  assert.equal(app.querySelector('.sp-sr').textContent, '1 setting found');
  input.value = '';
  input.dispatch('input');
  assert.equal(app.querySelectorAll('.sp-row').filter((row) => row.hidden).length, 0);
  sent.length = 0;
  deliver(fixture.go);
  assert.deepEqual(sent, [], 'a section Bowser asked for is not sent back');
  assert.equal(app.querySelector('.sp-nav-item[aria-current="true"]').dataset.section, 'privacy');
});

test('a guest cannot switch history on', () => {
  deliver(fixture.guest);
  const history = app.querySelector('.sp-row[data-row="history"]').querySelector('[role="switch"]');
  assert.equal(history.disabled, true);
  assert.equal(history.getAttribute('aria-checked'), 'false');
  sent.length = 0;
  history.click();
  assert.deepEqual(sent, []);
});
