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
    { ...d, theme: 'dark' },
    { ...d, themeAccent: '1a2b3c' },
    { ...d, themeAccent: '#1A2B3C' },
    { ...d, themeAccent: '#abc' },
    { ...d, toolbarButtons: 'back,back' },
    { ...d, toolbarButtons: 'back,sidebar' },
    { ...d, toolbarButtons: 'back,' },
    { ...d, centredAddressBar: 'true' },
    { ...d, frameless: 0 },
    { ...d, bookmarksBar: 'sometimes' },
    { ...d, sidePanelPosition: 'top' },
    { ...d, fontSize: 'huge' },
    { ...d, uiFont: 'x'.repeat(65) },
    { ...d, uiFont: 'a\nb' },
    { ...d, defaultZoom: 24 },
    { ...d, defaultZoom: 501 },
    { ...d, defaultZoom: 100.5 },
    { ...d, showZoomIndicator: 'yes' },
    { ...d, ctrlSVerticalTabs: 1 },
    { ...d, defaultBrowser: 'maybe' },
  ];
  for (const value of bad) assert.equal(logic.parseSettingsSnapshot(value), null, JSON.stringify(value).slice(0, 120));
  for (const key of Object.keys(d)) {
    const missing = { ...d };
    delete missing[key];
    assert.equal(logic.parseSettingsSnapshot(missing), null, `missing ${key}`);
  }
});

test('browser-customizability fields: parse, draftOf and save', () => {
  const d = fixture.snap.d;
  const s = logic.parseSettingsSnapshot(d);
  assert.equal(s.theme, 'helium');
  assert.equal(s.themeAccent, '');
  assert.equal(s.toolbarButtons, 'back,forward,reload,site-info,bookmark,newtab,split,reader');
  assert.equal(s.defaultBrowser, 'yes');
  assert.ok(logic.parseSettingsSnapshot({ ...d, themeAccent: '#1a2b3c' }));
  assert.ok(logic.parseSettingsSnapshot({ ...d, toolbarButtons: '' }));
  assert.ok(logic.parseSettingsSnapshot({ ...d, toolbarButtons: 'reader,back' }), 'any order is accepted from Rust');

  const draft = logic.draftOf(s);
  for (const key of [
    'theme', 'themeAccent', 'toolbarButtons', 'centredAddressBar', 'minimalUrl', 'showLoadProgress', 'frameless', 'roundedContentFrame',
    'showHomeButton', 'bookmarksBar', 'bookmarksBarTabGroups', 'sidePanelPosition', 'tabPreviewImages', 'tabPreviewMemory', 'fontSize', 'uiFont',
    'defaultZoom', 'showZoomIndicator', 'newTabBesideActive', 'ctrlTabMru', 'shiftRightClickContext', 'ctrlShiftCCopyLink', 'ctrlSVerticalTabs',
    'pinNewTabGroups', 'splitEdgeDrop', 'blockYoutubeVideoAds',
  ]) {
    assert.deepEqual(draft[key], s[key], key);
  }
  assert.ok(!('defaultBrowser' in draft), 'defaultBrowser is read-only, never part of the draft');

  const text = logic.saveText(draft);
  for (const line of [
    'theme=helium', 'theme_accent=', 'toolbar_buttons=back,forward,reload,site-info,bookmark,newtab,split,reader', 'centred_address_bar=true',
    'minimal_url=true', 'show_load_progress=true', 'frameless=false', 'rounded_content_frame=true', 'show_home_button=true', 'bookmarks_bar=never',
    'bookmarks_bar_tab_groups=false', 'side_panel_position=left', 'tab_preview_images=true', 'tab_preview_memory=true', 'font_size=medium',
    'ui_font=', 'default_zoom=100', 'show_zoom_indicator=true', 'new_tab_beside_active=true', 'ctrl_tab_mru=true', 'shift_right_click_context=false',
    'ctrl_shift_c_copy_link=true', 'ctrl_s_vertical_tabs=false', 'pin_new_tab_groups=false', 'split_edge_drop=true', 'block_youtube_video_ads=true',
  ]) {
    const matches = text.split('\n').filter((l) => l === line);
    assert.equal(matches.length, 1, `${line} appears exactly once`);
  }
  assert.doesNotMatch(text, /defaultBrowser|default_browser/);

  for (const broken of [
    { theme: 'dark' },
    { themeAccent: '1a2b3c' },
    { themeAccent: '#ABCDEF' },
    { toolbarButtons: 'back,back' },
    { toolbarButtons: 'back,sidebar' },
    { bookmarksBar: 'sometimes' },
    { sidePanelPosition: 'top' },
    { fontSize: 'huge' },
    { uiFont: 'x'.repeat(65) },
    { defaultZoom: 24 },
    { defaultZoom: 501 },
  ]) {
    assert.equal(logic.saveText({ ...draft, ...broken }), null, JSON.stringify(broken));
  }
  assert.ok(logic.saveText({ ...draft, themeAccent: '' }));
  assert.ok(logic.saveText({ ...draft, toolbarButtons: '' }));
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
  assert.deepEqual(restart, ['allow', 'blocker', 'grouping', 'sidePanelPosition', 'tracking']);
  // Rows Bowser saves but does not act on yet say so in plain words, never "Applies after restart".
  const notYet = logic.ROWS.filter((row) => row.notYet).map((row) => row.id).sort();
  assert.deepEqual(notYet, ['bookmarksBar', 'bookmarksBarTabGroups', 'ctrlSVerticalTabs', 'ctrlShiftCCopyLink', 'frameless', 'pinNewTabGroups', 'shiftRightClickContext', 'tabPreviewImages', 'tabPreviewMemory']);
  for (const row of logic.ROWS) {
    assert.ok(!(row.notYet && row.restart), `${row.id}: one note, not two`);
    assert.doesNotMatch(row.description, /restart/i, `${row.id}: the note says it, not the description`);
  }
});

test('save sends every profile setting exactly once in the order Rust expects', () => {
  const s = logic.parseSettingsSnapshot(fixture.snap.d);
  const draft = logic.draftOf(s);
  assert.equal(logic.saveText(draft), 'tab_layout=horizontal\nsidebar_width=260\nsleep_enabled=true\nsleep_after_secs=300\nblocker_enabled=true\nblocker_allow=example.com,news.example\nautomatic_tab_grouping=true\nefficiency_mode=auto\ntracking_prevention=balanced\nsearch_engine=optimised\ntheme=helium\ntheme_accent=\ntoolbar_buttons=back,forward,reload,site-info,bookmark,newtab,split,reader\ncentred_address_bar=true\nminimal_url=true\nshow_load_progress=true\nframeless=false\nrounded_content_frame=true\nshow_home_button=true\nbookmarks_bar=never\nbookmarks_bar_tab_groups=false\nside_panel_position=left\ntab_preview_images=true\ntab_preview_memory=true\nfont_size=medium\nui_font=\ndefault_zoom=100\nshow_zoom_indicator=true\nnew_tab_beside_active=true\nctrl_tab_mru=true\nshift_right_click_context=false\nctrl_shift_c_copy_link=true\nctrl_s_vertical_tabs=false\npin_new_tab_groups=false\nsplit_edge_drop=true\nblock_youtube_video_ads=true');
  const none = logic.saveText({ ...draft, searchEngine: null });
  assert.doesNotMatch(none, /search_engine/);
  assert.doesNotMatch(logic.saveText(draft), /auto_update|history|efficiencyActive/);
  for (const layout of logic.TAB_LAYOUTS) assert.match(logic.saveText({ ...draft, tabLayout: layout }), new RegExp(`^tab_layout=${layout}\n`));
  for (const broken of [{ sidebarWidth: 999 }, { sleepAfterSecs: 10 }, { blockerAllow: ['a b'] }, { tabLayout: 'sideways' }, { efficiencyMode: 'max' }, { searchEngine: 'bing' }]) {
    assert.equal(logic.saveText({ ...draft, ...broken }), null, JSON.stringify(broken));
  }
  assert.equal(logic.saveText({ ...draft, blockerAllow: Array.from({ length: 300 }, (_, i) => `site${i}.example`) }), null, 'over the MAX_TEXT cap');
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
  assert.ok(logic.matchRows('youtube').has('blockYoutubeVideoAds'));
  assert.ok(logic.matchRows('frameless').has('frameless'));
  assert.ok(logic.matchRows('accent colour').has('themeAccent'));
  assert.ok(logic.matchRows('default browser').has('defaultBrowser'));
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
  assert.equal(app.querySelectorAll('.sp-nav-item').length, 9);
  assert.equal(app.querySelectorAll('section').length, 9);
  for (const section of app.querySelectorAll('section')) assert.ok(section.getAttribute('aria-labelledby'));
  // The Privacy dashboard is one block standing for the `dashboard` row.
  const rows = app.querySelectorAll('.sp-row, .sp-dash').map((row) => row.dataset.row);
  assert.deepEqual(rows, logic.ROWS.map((row) => row.id));
  for (const control of app.querySelectorAll('[role="switch"], [role="radiogroup"]')) {
    assert.ok(control.getAttribute('aria-labelledby') || control.getAttribute('aria-label'), control.className);
  }
  assert.equal(app.querySelector('[role="status"]')?.getAttribute('aria-live'), 'polite');
  assert.equal(app.querySelector('.sp-avatar').textContent, 'W');
  assert.equal(app.querySelector('.sp-tile[aria-checked="true"]')?.querySelector('.sp-tile-name').textContent, 'Toolbar above tabs');
});

test('Downloads: the location and Change, the ask switch, each its own command', () => {
  const row = app.querySelector('.sp-row[data-row="downloadDir"]');
  assert.equal(row.querySelector('.sp-path').textContent, 'C:\\Users\\Work\\Downloads');
  assert.equal(row.querySelector('.sp-path').dir, 'ltr');
  const ask = app.querySelector('.sp-row[data-row="downloadAsk"]').querySelector('[role="switch"]');
  assert.equal(ask.getAttribute('aria-checked'), 'false');
  sent.length = 0;
  row.querySelector('.bw-button').click();
  ask.click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'downloadfolder' }, { v: 1, t: 'set', a: 'downloadask', on: true }]);
  assert.ok(!sent.some((m) => m.a === 'save'), 'never part of save');
  assert.doesNotMatch(logic.saveText(logic.draftOf(logic.parseSettingsSnapshot(fixture.snap.d))), /download/);
  deliver(fixture.snap);
  const s = logic.parseSettingsSnapshot(fixture.guest.d);
  assert.equal(s.downloadDir, '');
  assert.equal(s.downloadAsk, true);
  for (const bad of [{ downloadDir: 7 }, { downloadDir: 'C:\\x\n' }, { downloadDir: 'x'.repeat(521) }, { downloadAsk: 'no' }]) {
    assert.equal(logic.parseSettingsSnapshot({ ...fixture.snap.d, ...bad }), null, JSON.stringify(bad));
  }
});

test('rows that do nothing yet say so; a group of only such rows says it once', () => {
  const flagOf = (id) => app.querySelector(`.sp-row[data-row="${id}"]`).querySelector('.sp-flag');
  assert.equal(flagOf('frameless').textContent, logic.NOT_YET_NOTE);
  assert.equal(flagOf('tabPreviewImages').textContent, logic.NOT_YET_NOTE);
  assert.equal(flagOf('pinNewTabGroups').textContent, logic.NOT_YET_NOTE);
  assert.equal(flagOf('blocker').textContent, logic.RESTART_NOTE);
  assert.equal(flagOf('centredAddressBar'), null);
  // Bookmarks bar and Shortcuts hold only such rows: one note under the heading, none on the rows.
  for (const id of ['bookmarksBar', 'bookmarksBarTabGroups', 'shiftRightClickContext', 'ctrlShiftCCopyLink']) assert.equal(flagOf(id), null, id);
  const notes = app.querySelectorAll('.sp-group-note');
  assert.equal(notes.length, 2);
  assert.ok(notes.every((note) => note.textContent === logic.NOT_YET_GROUP_NOTE));
  const frameless = app.querySelector('.sp-row[data-row="frameless"]').querySelector('[role="switch"]');
  assert.equal(frameless.getAttribute('aria-describedby'), 'sp-d-frameless sp-f-frameless', 'the note is read with the control');
  assert.ok(app.querySelectorAll('.sp-flag').every((flag) => !/Applies after restart/.test(flag.textContent) || !logic.ROWS.find((r) => `sp-f-${r.id}` === flag.id)?.notYet));
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

test('the toolbar checklist toggles one id and the reset button restores the full catalog', () => {
  deliver(fixture.snap);
  const toolbarRow = app.querySelector('.sp-row[data-row="toolbarButtons"]');
  const switches = toolbarRow.querySelectorAll('[role="switch"]');
  assert.equal(switches.length, 8);
  assert.ok(switches.every((sw) => sw.getAttribute('aria-checked') === 'true'), 'the default set is every button, all on');
  sent.length = 0;
  switches[0].click();
  assert.equal(sent.length, 1);
  assert.match(sent[0].text, /toolbar_buttons=forward,reload,site-info,bookmark,newtab,split,reader\n/);
  const reset = toolbarRow.querySelector('.sp-corner').querySelector('button');
  sent.length = 0;
  reset.click();
  assert.match(sent[0].text, /toolbar_buttons=back,forward,reload,site-info,bookmark,newtab,split,reader\n/);
});

test('the theme reset button clears both theme and accent in one save', () => {
  deliver(fixture.snap);
  const themeRow = app.querySelector('.sp-row[data-row="themeAccent"]');
  const accent = themeRow.querySelector('input');
  accent.value = '#abcdef';
  accent.dispatch('change');
  const reset = themeRow.querySelector('button');
  sent.length = 0;
  reset.click();
  assert.equal(sent.length, 1);
  assert.match(sent[0].text, /theme=helium\n/);
  assert.match(sent[0].text, /theme_accent=\n/);
});

test('default browser status reflects the snapshot and the button sends a bare setdefault', () => {
  deliver(fixture.snap);
  const row = app.querySelector('.sp-row[data-row="defaultBrowser"]');
  assert.equal(row.querySelector('.sp-desc').textContent, 'Bowser is your default browser.');
  sent.length = 0;
  row.querySelector('button').click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'setdefault' }]);
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, defaultBrowser: 'no' } });
  assert.equal(row.querySelector('.sp-desc').textContent, 'Bowser is not your default browser.');
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, defaultBrowser: 'unknown' } });
  assert.equal(row.querySelector('.sp-desc').textContent, 'Bowser could not check whether it is the default browser.');
});

test('an unreadable settings file disables the new controls too, except the setdefault button', () => {
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, readable: false } });
  const toolbarRow = app.querySelector('.sp-row[data-row="toolbarButtons"]');
  for (const sw of toolbarRow.querySelectorAll('[role="switch"]')) assert.equal(sw.disabled, true);
  assert.equal(toolbarRow.querySelector('.sp-corner').querySelector('button').disabled, true);
  const themeRow = app.querySelector('.sp-row[data-row="themeAccent"]');
  assert.equal(themeRow.querySelector('input').disabled, true);
  assert.equal(themeRow.querySelector('button').disabled, true);
  assert.equal(app.querySelector('.sp-row[data-row="uiFont"]').querySelector('input').disabled, true);
  assert.equal(app.querySelector('.sp-row[data-row="defaultZoom"]').querySelector('input').disabled, true);
  assert.equal(app.querySelector('.sp-row[data-row="centredAddressBar"]').querySelector('[role="switch"]').disabled, true);
  assert.equal(app.querySelector('.sp-row[data-row="fontSize"]').querySelectorAll('[role="radio"]').every((b) => b.disabled), true);
  // A registry action, not a settings.txt value: stays enabled like "Open settings file" and "Reset settings".
  assert.equal(app.querySelector('.sp-row[data-row="defaultBrowser"]').querySelector('button').disabled, false);
  deliver(fixture.snap);
});

test('the Privacy dashboard is part of the page: its snapshot fills the section and its commands go out as priv', () => {
  globalThis.CSS ??= { escape: (value) => String(value) };
  const privacy = JSON.parse(readFileSync(join(here, 'fixtures', 'privacy-snapshot.json'), 'utf8'));
  const dash = app.querySelector('.sp-dash');
  assert.equal(dash.dataset.row, 'dashboard');
  deliver(privacy);
  const headings = dash.querySelectorAll('.sp-group-h').map((h) => h.textContent);
  for (const title of ['This site', 'Site permissions', 'Blocked requests', 'Clear browsing data']) assert.ok(headings.includes(title), title);
  assert.equal(dash.querySelectorAll('.sp-card').length, headings.length, 'every dashboard group is a settings card');
  const clear = dash.querySelectorAll('button').find((b) => b.textContent.includes('Clear statistics'));
  assert.ok(clear, 'the statistics button is on the page');
  sent.length = 0;
  clear.click();
  assert.deepEqual(sent, [{ v: 1, t: 'priv', a: 'clear-stats' }]);
});
