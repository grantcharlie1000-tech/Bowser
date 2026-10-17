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
  assert.equal(s.toolbarButtons, 'back,forward,reload,site-info,split,reader,extensions,downloads');
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
    'theme=helium', 'theme_accent=', 'toolbar_buttons=back,forward,reload,site-info,split,reader,extensions,downloads', 'centred_address_bar=true',
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
  // The blocking level and the sites it skips apply at once (App::apply_live_settings reaches the blocker).
  // The side panel position is read each time a panel opens (side_panel.rs), so it needs no restart.
  assert.deepEqual(restart, ['grouping', 'tracking']);
  // Rows Bowser saves but does not act on yet say so in plain words, never "Applies after restart".
  // --- toolbars and feeds (thread T13) --- the toolbar editor acts on the HTML toolbar, tab strip and rail, so only
  // the frameless window is left that saves a choice and does nothing.
  const notYet = logic.ROWS.filter((row) => row.notYet).map((row) => row.id).sort();
  assert.deepEqual(notYet, ['frameless']);
  for (const row of logic.ROWS) {
    assert.ok(!(row.notYet && row.restart), `${row.id}: one note, not two`);
    assert.doesNotMatch(row.description, /restart/i, `${row.id}: the note says it, not the description`);
  }
});

test('save sends every profile setting exactly once in the order Rust expects', () => {
  const s = logic.parseSettingsSnapshot(fixture.snap.d);
  const draft = logic.draftOf(s);
  assert.equal(logic.saveText(draft), 'tab_layout=horizontal\nsidebar_width=260\nsleep_enabled=true\nsleep_after_secs=300\nblocker_enabled=true\nblocker_allow=example.com,news.example\nautomatic_tab_grouping=true\nefficiency_mode=auto\ntracking_prevention=balanced\nsearch_engine=optimised\ntheme=helium\ntheme_accent=\ntoolbar_buttons=back,forward,reload,site-info,split,reader,extensions,downloads\ncentred_address_bar=true\nminimal_url=true\nshow_load_progress=true\nframeless=false\nrounded_content_frame=true\nshow_home_button=true\nbookmarks_bar=never\nbookmarks_bar_tab_groups=false\nside_panel_position=left\ntab_preview_images=true\ntab_preview_memory=true\nfont_size=medium\nui_font=\ndefault_zoom=100\nshow_zoom_indicator=true\nnew_tab_beside_active=true\nctrl_tab_mru=true\nshift_right_click_context=false\nctrl_shift_c_copy_link=true\nctrl_s_vertical_tabs=false\npin_new_tab_groups=false\nsplit_edge_drop=true\nblock_youtube_video_ads=true\nstart_group_reopen=last\nstart_titles=always\nstart_add_button=true\nstart_columns=6\nstart_thumb_size=3\nstart_drag_reorder=true\nhomepage=\nstartup=start_page\nstartup_pages=\nstart_widgets=false\nstart_widget_columns=2\nstart_widget_height=regular\nstart_widget_bg=translucent\nstart_widget_color=#1f2933\nstart_widget_drag=true\ntab_bar_visible=true\naddress_bar_position=top\naddress_bar_visible=true\nshow_title_bar=false\nstatus_bar=overlay\nui_density=regular\ncompact_menus=false\nui_zoom=100\nlayout_saved=');
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
  assert.deepEqual([...logic.matchRows('duckduckgo')], ['searchDefault']);
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
// --- settings slim --- the disclosures remember their state here; Privacy was left open last time.
const stored = new Map([['bowser.settings.more', JSON.stringify(['privacy'])]]);
globalThis.localStorage = { getItem: (k) => stored.get(k) ?? null, setItem: (k, v) => stored.set(k, String(v)), removeItem: (k) => stored.delete(k) };
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
  // --- settings slim --- twelve areas in the nav, each holding the Rust sections it merges, then
  // "Not active yet" and one sub-page per big editor.
  assert.equal(app.querySelectorAll('.sp-nav-item').length, 12);
  assert.equal(app.querySelectorAll('section.sp-part').length, 16);
  assert.deepEqual(app.querySelectorAll('section[data-area]').map((s) => s.dataset.area), [...logic.AREAS.map((a) => a.key), 'later']);
  assert.deepEqual(app.querySelectorAll('section.sp-page').map((s) => s.dataset.page).sort(), Object.keys(logic.PAGE_ROWS).sort());
  for (const section of app.querySelectorAll('section')) assert.ok(section.getAttribute('aria-labelledby') || section.getAttribute('aria-label'));
  // The Privacy dashboard, the Extensions block, the shortcut list and the chain list each stand for one row.
  // Every row is drawn exactly once, wherever it sits now.
  const rows = app.querySelectorAll('.sp-row, .sp-dash, .sp-ext, .sp-kb, .sp-chains').map((row) => row.dataset.row).filter((id) => id !== undefined);
  assert.deepEqual([...rows].sort(), logic.ROWS.map((row) => row.id).sort());
  for (const control of app.querySelectorAll('[role="switch"], [role="radiogroup"]')) {
    assert.ok(control.getAttribute('aria-labelledby') || control.getAttribute('aria-label'), control.className);
  }
  assert.equal(app.querySelector('.sp-sr[role="status"]')?.getAttribute('aria-live'), 'polite');
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
  assert.doesNotMatch(logic.saveText(logic.draftOf(logic.parseSettingsSnapshot(fixture.snap.d))), /download_dir|ask_download|downloadDir/);
  deliver(fixture.snap);
  const s = logic.parseSettingsSnapshot(fixture.guest.d);
  assert.equal(s.downloadDir, '');
  assert.equal(s.downloadAsk, true);
  for (const bad of [{ downloadDir: 7 }, { downloadDir: 'C:\\x\n' }, { downloadDir: 'x'.repeat(521) }, { downloadAsk: 'no' }]) {
    assert.equal(logic.parseSettingsSnapshot({ ...fixture.snap.d, ...bad }), null, JSON.stringify(bad));
  }
});

test('Translation: two switches and a language select, each its own command, applied at once', () => {
  const rowOf = (id) => app.querySelector(`.sp-row[data-row="${id}"]`);
  const offer = rowOf('translateOffer').querySelector('[role="switch"]');
  const auto = rowOf('translateAuto').querySelector('[role="switch"]');
  assert.equal(offer.getAttribute('aria-checked'), 'true');
  assert.equal(auto.getAttribute('aria-checked'), 'false');
  assert.equal(rowOf('translateOffer').querySelector('.sp-label').textContent, 'Offer to translate pages in other languages');
  assert.equal(rowOf('translateAuto').querySelector('.sp-label').textContent, 'Translate automatically on sites set to always translate');
  assert.equal(rowOf('translateOffer').querySelector('.sp-desc').textContent, 'Shows a button in the toolbar when a page is in another language. Nothing is sent until you click it. Pages are translated by Google Translate, so their text is sent to Google.');
  assert.equal(rowOf('translateAuto').querySelector('.sp-desc').textContent, 'Sites set to always translate are translated as soon as they load, which sends their text to Google Translate.');

  sent.length = 0;
  offer.click();
  auto.click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'translateoffer', on: false }, { v: 1, t: 'set', a: 'translateauto', on: true }]);
  offer.click();
  assert.deepEqual(sent.at(-1), { v: 1, t: 'set', a: 'translateoffer', on: true });
  assert.ok(!sent.some((m) => m.a === 'save'), 'never part of save');

  // The select: "My Windows language" first, then one option per code with its English name.
  const select = rowOf('translateTarget').querySelector('.bw-select-button');
  assert.equal(select.getAttribute('aria-label'), 'Translate into');
  assert.equal(select.getAttribute('aria-labelledby'), 'sp-l-translateTarget');
  assert.equal(select.querySelector('.bw-select-value').textContent, 'My Windows language');
  select.click();
  const options = rowOf('translateTarget').querySelectorAll('[role="option"]');
  const codes = fixture.snap.d.translateTargets;
  assert.deepEqual(options.map((o) => o.querySelector('.bw-menu-label').textContent).slice(0, 5), ['My Windows language', 'English', 'Spanish', 'French', 'German']);
  assert.equal(options.length, codes.length + 1, 'one option per language Rust lists');
  sent.length = 0;
  options[3].click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'translatetarget', text: 'fr' }]);
  assert.equal(select.querySelector('.bw-select-value').textContent, 'French');
  select.click();
  rowOf('translateTarget').querySelectorAll('[role="option"]')[0].click();
  assert.deepEqual(sent.at(-1), { v: 1, t: 'set', a: 'translatetarget', text: '' }, 'the Windows language sends an empty text');

  // The next snapshot is the truth: it moves every control, and a new list of codes rebuilds the select.
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, translateOffer: false, translateAuto: true, translateTarget: 'de' } });
  assert.equal(offer.getAttribute('aria-checked'), 'false');
  assert.equal(auto.getAttribute('aria-checked'), 'true');
  assert.equal(rowOf('translateTarget').querySelector('.bw-select-value').textContent, 'German');
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, translateTarget: 'xx', translateTargets: ['en', 'xx'] } });
  const rebuilt = rowOf('translateTarget').querySelector('.bw-select-button');
  assert.equal(rebuilt.querySelector('.bw-select-value').textContent, 'xx', 'an unknown code shows the code');
  rebuilt.click();
  assert.deepEqual(rowOf('translateTarget').querySelectorAll('[role="option"]').map((o) => o.querySelector('.bw-menu-label').textContent), ['My Windows language', 'English', 'xx']);
  rebuilt.click();

  // An unreadable settings file disables all three, like the other rows.
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, readable: false } });
  assert.equal(offer.disabled, true);
  assert.equal(auto.disabled, true);
  assert.equal(rowOf('translateTarget').querySelector('.bw-select-button').disabled, true);
  deliver(fixture.snap);
  assert.equal(offer.disabled, false);
  assert.equal(rowOf('translateTarget').querySelector('.bw-select-button').disabled, false);
});

test('rows that do nothing yet say so; a group of only such rows says it once', () => {
  const flagOf = (id) => app.querySelector(`.sp-row[data-row="${id}"]`).querySelector('.sp-flag');
  assert.equal(flagOf('frameless').textContent, logic.NOT_YET_NOTE);
  assert.equal(flagOf('tabPreviewImages'), null, 'the tab preview rows act now: the hover card honours them');
  assert.equal(flagOf('grouping').textContent, logic.RESTART_NOTE);
  assert.equal(flagOf('blocker'), null, 'the blocking level applies at once');
  assert.equal(flagOf('centredAddressBar'), null);
  // The bookmarks bar rows (T2) and the shortcut rows (T4) act now: no note on any of them.
  for (const id of ['bookmarksBar', 'bookmarksBarTabGroups', 'pinNewTabGroups', 'shiftRightClickContext', 'ctrlShiftCCopyLink']) assert.equal(flagOf(id), null, id);
  // With both of those groups acting, no group is left made only of such rows, so no group note shows.
  assert.equal(app.querySelectorAll('.sp-group-note').length, 0);
  const frameless = app.querySelector('.sp-row[data-row="frameless"]').querySelector('[role="switch"]');
  assert.equal(frameless.getAttribute('aria-describedby'), 'sp-d-frameless sp-f-frameless', 'the note is read with the control');
  assert.ok(app.querySelectorAll('.sp-flag').every((flag) => !/Applies after restart/.test(flag.textContent) || !logic.ROWS.find((r) => `sp-f-${r.id}` === flag.id)?.notYet));
});

test('a change saves the whole draft; a toggle answer shows "Saved"', () => {
  sent.length = 0;
  // --- layout (thread T6) --- the Vertical left preset tile (the preset tiles come first).
  const vertical = app.querySelector('.sp-tile[data-preset="vertical-left"]');
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
  const nav = app.querySelectorAll('.sp-nav-item').find((b) => b.dataset.section === 'privacy');
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
  assert.deepEqual(shown, ['searchDefault']);
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

// --- toolbars and feeds (thread T13) --- the toolbar editor: the three bars' lists as rows, each change saved as the
// whole new list (the address bar's through Save, the other two through `baropt`), reset per bar, locked when the
// file cannot be read. The HTML toolbar, strip and rail drawing them is tested in test/bars.mjs.
const editorRows = (bar) => app.querySelector(`section.sp-tbe-bar[data-bar="${bar}"]`).querySelectorAll('.sp-tb-item').filter((row) => row.dataset.item !== undefined);
const editorButton = (row, name) => row.querySelectorAll('button').find((b) => b.getAttribute('aria-label')?.startsWith(name));
const barReset = (bar) => app.querySelector(`section.sp-tbe-bar[data-bar="${bar}"]`).querySelector('.sp-tbe-head').querySelector('button');
const toolbarSave = () => sent.filter((m) => m.a === 'save').map((m) => /\ntoolbar_buttons=([^\n]*)\n/.exec(m.text)?.[1]);
const commandsFixture0 = JSON.parse(readFileSync(join(here, 'fixtures', 'commands-snapshot.json'), 'utf8'));

test('the toolbar editor opens from one row, lists each bar and is no longer in "Not active yet"', () => {
  deliver(fixture.snap);
  assert.equal(logic.ROWS.find((row) => row.id === 'toolbarButtons').notYet, undefined);
  assert.equal(logic.rowTier(logic.ROWS.find((row) => row.id === 'toolbarButtons')), 'page');
  assert.equal(app.querySelector('[data-opens="toolbarButtons"]').querySelector('.bw-button').textContent, 'Edit toolbars…');
  assert.ok(!app.querySelector('section[data-area="later"]').contains(app.querySelector('.sp-row[data-row="toolbarButtons"]')));
  assert.deepEqual(editorRows('address').map((row) => row.dataset.item), ['back', 'forward', 'reload', 'site-info', 'split', 'reader', 'extensions', 'downloads']);
  assert.deepEqual(editorRows('tabs').map((row) => row.dataset.item), ['closed-tabs']);
  assert.deepEqual(editorRows('panel').map((row) => row.dataset.item), []);
  const headings = app.querySelectorAll('.sp-tbe-title').map((h) => h.textContent);
  assert.deepEqual(headings, ['Address bar', 'Tab bar', 'Panel bar'], 'there is no status bar to edit');
});

test('moving, removing and resetting an address bar button each save the whole list', () => {
  deliver(fixture.snap);
  sent.length = 0;
  // Move Forward up past Back: back and forward swap.
  editorButton(editorRows('address')[1], 'Move Forward up').click();
  assert.deepEqual(toolbarSave(), ['forward,back,reload,site-info,split,reader,extensions,downloads']);
  deliver(fixture.snap);
  sent.length = 0;
  // Remove Reload.
  editorButton(editorRows('address')[2], 'Remove Reload').click();
  assert.deepEqual(toolbarSave(), ['back,forward,site-info,split,reader,extensions,downloads']);
  deliver(fixture.snap);
  sent.length = 0;
  // The first button of a zone cannot move up, the last cannot move down, and a button never crosses into another zone.
  assert.equal(editorButton(editorRows('address')[0], 'Move Back up').disabled, true);
  assert.equal(editorButton(editorRows('address')[2], 'Move Reload and stop down').disabled, true, 'reload is the last before the address field');
  assert.equal(editorButton(editorRows('address')[3], 'Move Site information up').disabled, true, 'site information is the first inside the field');
  // Reset returns the default list (here it already is the default, so the button is off).
  assert.equal(barReset('address').disabled, true);
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, toolbarButtons: 'back,newtab,cmd:new-tab' } });
  sent.length = 0;
  assert.equal(barReset('address').disabled, false);
  barReset('address').click();
  assert.deepEqual(toolbarSave(), ['back,forward,reload,site-info,split,reader,extensions,downloads']);
  deliver(fixture.snap);
});

test('the tab bar and panel bar lists are their own baropt commands, never part of save', () => {
  deliver(fixture.snap);
  sent.length = 0;
  editorButton(editorRows('tabs')[0], 'Remove Closed tabs').click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'baropt', text: 'toolbar_tabs=' }]);
  assert.ok(!sent.some((m) => m.a === 'save'));
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, bars: { ...fixture.snap.d.bars, tabs: 'cmd:new-tab,closed-tabs', panel: 'feeds,chain:2' } } });
  assert.deepEqual(editorRows('tabs').map((row) => row.dataset.item), ['cmd:new-tab', 'closed-tabs']);
  assert.deepEqual(editorRows('panel').map((row) => row.dataset.item), ['feeds', 'chain:2']);
  sent.length = 0;
  editorButton(editorRows('tabs')[1], 'Move Closed tabs up').click();
  editorButton(editorRows('panel')[0], 'Move Feeds down').click();
  assert.deepEqual(sent, [
    { v: 1, t: 'set', a: 'baropt', text: 'toolbar_tabs=closed-tabs,cmd:new-tab' },
    { v: 1, t: 'set', a: 'baropt', text: 'toolbar_panel=chain:2,feeds' },
  ]);
  sent.length = 0;
  barReset('panel').click();
  barReset('tabs').click();
  assert.deepEqual(sent, [
    { v: 1, t: 'set', a: 'baropt', text: 'toolbar_panel=' },
    { v: 1, t: 'set', a: 'baropt', text: 'toolbar_tabs=closed-tabs' },
  ]);
  deliver(fixture.snap);
});

test('command buttons take the name of their command once the commands arrive', () => {
  deliver(fixture.snap);
  deliver({ v: 1, t: 'snap', p: 'commands', d: JSON.parse(JSON.stringify(commandsFixture0.d)) });
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, bars: { ...fixture.snap.d.bars, tabs: 'cmd:new-tab,cmd:no-such' } } });
  const names = editorRows('tabs').map((row) => row.querySelector('.sp-label').textContent);
  assert.equal(names[0], 'New tab');
  assert.equal(names[1], 'no such', 'a command Rust does not list is named from its id');
  deliver(fixture.snap);
});

test('the feed rows: open the panel, the address bar button and when Feeds fetches', () => {
  deliver(fixture.snap);
  const rowOf = (id) => app.querySelector(`.sp-row[data-row="${id}"]`);
  sent.length = 0;
  rowOf('feedsOpen').querySelector('button').click();
  const detect = rowOf('feedsDetect').querySelector('[role="switch"]');
  assert.equal(detect.getAttribute('aria-checked'), 'true');
  detect.click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'openfeeds' }, { v: 1, t: 'set', a: 'baropt', text: 'feeds_detect=false' }]);
  assert.ok(!sent.some((m) => m.a === 'save'));
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, bars: { ...fixture.snap.d.bars, feedsDetect: false, feedsRefresh: 180 } } });
  assert.equal(detect.getAttribute('aria-checked'), 'false');
  assert.equal(rowOf('feedsRefresh').querySelector('.bw-select-value').textContent, 'After 3 hours');
  deliver(fixture.snap);
  assert.equal(rowOf('feedsRefresh').querySelector('.bw-select-value').textContent, 'After 1 hour');
  assert.equal(logic.rowTier(logic.ROWS.find((row) => row.id === 'feedsOpen')), 'core');
  assert.equal(logic.rowTier(logic.ROWS.find((row) => row.id === 'feedsDetect')), 'more');
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
  // --- toolbars and feeds (thread T13) --- the toolbar editor and the feed rows lock too.
  for (const button of app.querySelector('.sp-tbe').querySelectorAll('button')) assert.equal(button.disabled, true, button.getAttribute('aria-label') ?? button.textContent);
  assert.equal(app.querySelector('.sp-row[data-row="feedsDetect"]').querySelector('[role="switch"]').disabled, true);
  assert.equal(app.querySelector('.sp-row[data-row="feedsRefresh"]').querySelector('button').disabled, true);
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
  const dash = app.querySelector('.sp-dash[data-row="dashboard"]');
  assert.equal(dash.dataset.row, 'dashboard');
  deliver(privacy);
  const headings = dash.querySelectorAll('.sp-group-h').map((h) => h.textContent);
  for (const title of ['This site', 'Site permissions', 'Blocked requests']) assert.ok(headings.includes(title), title);
  assert.ok(!headings.includes('Clear browsing data'), 'Delete browsing data has its own block above the dashboard');
  assert.equal(dash.querySelectorAll('.sp-card').length, headings.length, 'every dashboard group is a settings card');
  const clear = dash.querySelectorAll('button').find((b) => b.textContent.includes('Clear statistics'));
  assert.ok(clear, 'the statistics button is on the page');
  sent.length = 0;
  clear.click();
  assert.deepEqual(sent, [{ v: 1, t: 'priv', a: 'clear-stats' }]);
});

test('the Extensions section is part of the page: its snapshot fills the block and its commands go out as set', () => {
  const extensions = JSON.parse(readFileSync(join(here, 'fixtures', 'extensions-messages.json'), 'utf8'));
  const block = app.querySelector('.sp-ext');
  assert.equal(block.dataset.row, 'extensions');
  assert.equal(block.parent, app.querySelector('section[data-section="extensions"]'));
  assert.ok(block.querySelector('.sp-ext-state.is-loading'), 'loading until Bowser answers');
  deliver(extensions.snap);
  assert.equal(block.querySelectorAll('.sp-ext-item').length, 2);
  sent.length = 0;
  block.querySelector('.sp-ext-item[data-id="ext-0123456789abcdef"]').querySelector('[role="switch"]').click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'extension', text: 'ext-0123456789abcdef', on: false }]);
  assert.ok(logic.matchRows('addon').has('extensions'));
  assert.ok(logic.matchRows('manifest permissions').has('extensions'));
});

// --- keyboard and quick commands (docs/commands.md) ---

test('the three shortcut rows that used to say "no effect yet" now say nothing, and sit in the shortcuts group', () => {
  for (const id of ['ctrlSVerticalTabs', 'shiftRightClickContext', 'ctrlShiftCCopyLink']) {
    const row = logic.ROWS.find((r) => r.id === id);
    assert.equal(row.notYet, undefined, `${id} acts, so it carries no "no effect yet" flag`);
    const node = app.querySelector(`.sp-row[data-row="${id}"]`);
    assert.equal(node.querySelector('.sp-flag'), null, id);
    const control = node.querySelector('[role="switch"]');
    assert.equal(control.getAttribute('aria-describedby'), `sp-d-${id}`, 'only its description is read with it');
  }
  // A change to one of them is part of the whole-draft save, like every other profile switch.
  sent.length = 0;
  app.querySelector('.sp-row[data-row="shiftRightClickContext"]').querySelector('[role="switch"]').click();
  assert.equal(sent[0].a, 'save');
  assert.match(sent[0].text, /\nshift_right_click_context=true\n/);
  deliver(fixture.snap);
});

const commandsFixture = JSON.parse(readFileSync(join(here, 'fixtures', 'commands-snapshot.json'), 'utf8'));
const commandsSnap = (patch = {}) => ({ ...commandsFixture, d: { ...commandsFixture.d, ...patch } });
// --- settings slim --- Keyboard and Quick commands are one area; the shortcut list and the chains are sub-pages.
const keyboard = () => app.querySelector('section[data-area="keyboard"]');
const quick = keyboard;
const kbPage = () => app.querySelector('section[data-page="kbEditor"]');
const chainsPage = () => app.querySelector('section[data-page="chains"]');
const rowIn = (root, id) => root.querySelector(`.sp-row[data-row="${id}"]`);
const kbRow = (id) => app.querySelector(`.sp-kb-row[data-command="${id}"]`);
const buttonNamed = (root, text) => root.querySelectorAll('button').find((b) => b.textContent.trim() === text);
const captureHandlers = [];
document.addEventListener = (type, cb) => { if (type === 'keydown') captureHandlers.push(cb); };
document.removeEventListener = (type, cb) => { const at = captureHandlers.indexOf(cb); if (type === 'keydown' && at >= 0) captureHandlers.splice(at, 1); };
const press = (init) => captureHandlers.at(-1)({ key: '', code: '', ctrlKey: false, shiftKey: false, altKey: false, isComposing: false, preventDefault() {}, stopPropagation() {}, ...init });

test('Keyboard and Quick commands are two sections whose controls land where Rust put them', () => {
  assert.ok(keyboard() && quick());
  assert.deepEqual(app.querySelectorAll('.sp-nav-item').map((b) => b.dataset.section), ['profile', 'appearance', 'tabs', 'privacy', 'performance', 'address', 'keyboard', 'downloads', 'media', 'extensions', 'updates', 'about']);
  assert.deepEqual(keyboard().querySelectorAll('section.sp-part').map((s) => s.dataset.section), ['keyboard', 'commands']);
  assert.equal(kbPage().querySelector('.sp-kb').dataset.row, 'kbEditor');
  assert.equal(chainsPage().querySelector('.sp-chains').dataset.row, 'chains');
  deliver(commandsSnap());
  const on = (root, id) => rowIn(root, id).querySelector('[role="switch"]').getAttribute('aria-checked');
  assert.deepEqual(['kbEnabled', 'kbSingleKey', 'kbAltMenu', 'kbFocusCycling'].map((id) => on(keyboard(), id)), ['true', 'false', 'false', 'false']);
  assert.deepEqual(['qcKeepLast', 'qcNewTab', 'qcNotesPanel', 'qcNickname'].map((id) => on(quick(), id)), ['false', 'false', 'true', 'true']);
  assert.equal(rowIn(quick(), 'qcMax').querySelector('input').value, '9');
  // Every option sends its own flag name and the new state.
  for (const [root, id, name] of [
    [keyboard(), 'kbEnabled', 'shortcuts_enabled'], [keyboard(), 'kbSingleKey', 'single_key_shortcuts'], [keyboard(), 'kbAltMenu', 'alt_opens_menu'],
    [keyboard(), 'kbFocusCycling', 'focus_cycling'], [quick(), 'qcKeepLast', 'palette_keep_last'], [quick(), 'qcNewTab', 'palette_open_new_tab'],
    [quick(), 'qcNotesPanel', 'palette_notes_in_panel'], [quick(), 'qcNickname', 'palette_open_nickname'],
  ]) {
    sent.length = 0;
    const control = rowIn(root, id).querySelector('[role="switch"]');
    const before = control.getAttribute('aria-checked') === 'true';
    control.click();
    assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'kboption', text: name, on: !before }], id);
  }
  deliver(commandsSnap({ shortcutsEnabled: false, singleKey: true }));
  assert.equal(on(keyboard(), 'kbEnabled'), 'false');
  assert.equal(on(keyboard(), 'kbSingleKey'), 'true');
  assert.equal(keyboard().querySelector('.sp-cmd-note').hidden, false, 'turning shortcuts off says how to get them back');
  deliver(commandsSnap());
  assert.equal(keyboard().querySelector('.sp-cmd-note').hidden, true);
});

test('the shortcut list: chips, remove one, clear, restore, browser priority, filter, cheat sheet, restore all', () => {
  deliver(commandsSnap());
  const closeTab = kbRow('close-tab');
  assert.deepEqual(closeTab.querySelectorAll('.sp-kb-keys').map((k) => k.textContent), ['Ctrl+W', 'Ctrl+F4', 'Ctrl+Shift+X']);
  assert.deepEqual(kbRow('new-private-tab').querySelectorAll('.sp-kb-keys'), []);
  assert.equal(kbRow('new-private-tab').querySelector('.sp-kb-none').textContent, 'No shortcut');
  sent.length = 0;
  closeTab.querySelectorAll('.sp-kb-chip-remove')[2].click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'kbremove', text: 'close-tab\tCtrl+Shift+X' });
  const tool = (row, label) => row.querySelectorAll('button').find((b) => b.getAttribute('aria-label') === label);
  tool(closeTab, 'Clear the shortcuts of Close tab').click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'kbclear', text: 'close-tab' });
  tool(closeTab, 'Restore the default shortcuts of Close tab').click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'kbrestore', text: 'close-tab' });
  assert.equal(tool(kbRow('reload'), 'Restore the default shortcuts of Reload'), undefined, 'a command on its defaults has nothing to restore');
  assert.equal(tool(kbRow('new-private-tab'), 'Clear the shortcuts of New private tab'), undefined, 'a command with no shortcut has nothing to clear');
  // Browser priority: the shield shows the command's current flag and a click sends the opposite one.
  const shield = tool(kbRow('history'), 'Browser priority for History');
  assert.equal(shield.getAttribute('aria-pressed'), 'true');
  shield.click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'kbpriority', text: 'history', on: false });
  assert.equal(tool(kbRow('chain:1'), 'Browser priority for Morning'), undefined, 'a chain has no browser priority');
  assert.equal(tool(kbRow('reload'), 'Browser priority for Reload').getAttribute('aria-pressed'), 'false', 'a command off the list shows it');
  // Categories keep the registry's order, chains last.
  assert.deepEqual(kbPage().querySelectorAll('.sp-card-h').map((h) => h.textContent).at(-1), 'Command chains');
  // The filter matches names, categories and the shortcuts themselves.
  const filter = kbPage().querySelector('.sp-kb-filter');
  filter.value = 'ctrl+j';
  filter.dispatch('input');
  assert.deepEqual(app.querySelectorAll('.sp-kb-row').map((r) => r.dataset.command), ['downloads']);
  filter.value = 'zzzz nothing';
  filter.dispatch('input');
  assert.equal(app.querySelectorAll('.sp-kb-row').length, 0);
  assert.match(app.querySelector('.sp-kb-empty').textContent, /No command or shortcut matches/);
  filter.value = '';
  filter.dispatch('input');
  assert.ok(app.querySelectorAll('.sp-kb-row').length > 50);
  // The cheat sheet and restore all (which asks first).
  sent.length = 0;
  buttonNamed(kbPage(), 'Cheat sheet').click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'kbsheet' }]);
  sent.length = 0;
  const restoreAll = buttonNamed(kbPage(), 'Restore all');
  restoreAll.click();
  assert.deepEqual(sent, [], 'the first click only asks');
  assert.equal(restoreAll.textContent, 'Restore every shortcut');
  restoreAll.click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'kbrestoreall' }]);
});

test('adding a shortcut waits for the keys, cancels on Escape, and offers to reassign a taken chord', () => {
  deliver(commandsSnap());
  sent.length = 0;
  const add = (id) => kbRow(id).querySelector('.sp-kb-add');
  const capture = (on) => ({ v: 1, t: 'set', a: 'kbcapture', on });
  add('history').click();
  assert.ok(kbRow('history').querySelector('.sp-kb-capture'), 'the row asks for a key press');
  assert.deepEqual(sent, [capture(true)], 'while it waits, Bowser hands every key to the page (Ctrl+T must not open a tab)');
  press({ key: 'Control', code: 'ControlLeft', ctrlKey: true });
  assert.deepEqual(sent, [capture(true)], 'a modifier alone keeps waiting');
  press({ key: 'Escape', code: 'Escape' });
  assert.equal(kbRow('history').querySelector('.sp-kb-capture'), null);
  assert.deepEqual(sent, [capture(true), capture(false)], 'Escape cancels and gives the keys back');
  sent.length = 0;
  add('history').click();
  press({ key: 'K', code: 'KeyK', ctrlKey: true, shiftKey: true });
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'kbassign', text: 'history\tCtrl+Shift+K', on: false });
  assert.deepEqual(sent, [capture(true), capture(false)], 'the chord ends the wait before it is sent');
  // Bowser says another command has it: the row names it and offers to take it.
  deliver(commandsSnap({ ack: 'conflict', conflictChord: 'Ctrl+Shift+K', conflictId: 'command-palette', conflictLabel: 'Quick commands' }));
  const conflict = kbRow('history').querySelector('.sp-kb-conflict');
  assert.equal(conflict.querySelector('span').textContent, 'Ctrl+Shift+K is already used by Quick commands.');
  buttonNamed(conflict, 'Reassign').click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'kbassign', text: 'history\tCtrl+Shift+K', on: true });
  deliver(commandsSnap({ ack: 'saved' }));
  assert.equal(kbRow('history').querySelector('.sp-kb-conflict'), null);
  assert.equal(app.querySelector('.sp-toast').textContent, 'Saved');
  // A refusal is a toast with Bowser's words, and the editor stays usable.
  add('history').click();
  press({ key: 'c', code: 'KeyC', ctrlKey: true });
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'kbassign', text: 'history\tCtrl+C', on: false });
  deliver(commandsSnap({ ack: 'failed', error: 'That shortcut is kept for editing and Windows.' }));
  assert.equal(app.querySelector('.sp-toast').textContent, 'That shortcut is kept for editing and Windows.');
  assert.equal(app.querySelector('.sp-toast').classList.contains('is-error'), true);
  assert.equal(captureHandlers.length, 0, 'no key listener is left behind');
});

test('sites that prioritise their own shortcuts: add normalises, a duplicate is refused here, remove sends', () => {
  deliver(commandsSnap());
  const row = rowIn(keyboard(), 'kbSites');
  const input = row.querySelector('input');
  const add = buttonNamed(row, 'Add');
  assert.deepEqual(row.querySelectorAll('.sp-site-name').map((n) => n.textContent), ['docs.example.com']);
  sent.length = 0;
  input.value = 'https://www.Docs.Example.com/page';
  add.click();
  assert.deepEqual(sent, [], 'already on the list');
  assert.equal(row.querySelector('.sp-field-error').textContent, 'docs.example.com is already on the list.');
  input.value = 'not a host';
  add.click();
  assert.equal(row.querySelector('.sp-field-error').textContent, 'Enter a site such as example.com.');
  input.value = ' https://WWW.Other.example:8080/x ';
  add.click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'kbsiteadd', text: 'other.example' }]);
  sent.length = 0;
  row.querySelector('.sp-site-remove').click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'kbsiteremove', text: 'docs.example.com' }]);
});

test('Quick commands: result types show, hide, reorder and reset; the limit clamps', () => {
  deliver(commandsSnap());
  const items = () => rowIn(quick(), 'qcTypes').querySelectorAll('.sp-qc-type');
  const names = () => items().map((li) => li.querySelector('.sp-label').textContent);
  assert.deepEqual(names(), ['Commands', 'Open tabs', 'Calculator', 'Recently closed tabs', 'Workspaces', 'Page actions', 'Bookmarks', 'Notes', 'History'], 'shown ones first, in order');
  assert.deepEqual(items().map((li) => li.classList.contains('is-off')), [false, false, false, true, true, true, true, true, true]);
  const move = (at, which) => items()[at].querySelectorAll('.sp-qc-move')[which];
  assert.equal(move(0, 0).disabled, true, 'the first cannot go up');
  assert.equal(move(2, 1).disabled, true, 'the last shown cannot go down');
  assert.equal(move(3, 0).disabled && move(3, 1).disabled, true, 'a hidden type has no order');
  sent.length = 0;
  move(0, 1).click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'qctypes', text: 'tab,command,calc' });
  move(2, 0).click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'qctypes', text: 'command,calc,tab' });
  items()[1].querySelector('[role="switch"]').click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'qctypes', text: 'command,calc' }, 'hiding one keeps the rest in order');
  items()[7].querySelector('[role="switch"]').click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'qctypes', text: 'command,tab,calc,note' }, 'showing one puts it last');
  // The last shown type cannot be hidden.
  deliver(commandsSnap({ paletteTypes: ['tab'] }));
  items()[0].querySelector('[role="switch"]').click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'qctypes', text: 'tab' });
  // Reset shows only while the order is not the default.
  const reset = buttonNamed(rowIn(quick(), 'qcTypes'), 'Reset order');
  assert.equal(reset.hidden, false);
  reset.click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'qctypesreset' });
  deliver(commandsSnap({ paletteTypes: ['tab', 'closed', 'workspace', 'command', 'action', 'bookmark', 'note', 'history', 'calc'], paletteDefaultOrder: true }));
  assert.equal(reset.hidden, true);
  const max = rowIn(quick(), 'qcMax').querySelector('input');
  for (const [typed, sentValue] of [['40', 30], ['1', 3], ['12', 12], ['abc', 3]]) {
    max.value = typed;
    max.dispatch('change');
    assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'qcmax', n: sentValue }, typed);
    assert.equal(max.value, String(sentValue));
  }
});

test('command chains: the list, clone, delete with a confirmation, and the editor (name, delays, order, test, save)', () => {
  deliver(commandsSnap());
  const block = chainsPage().querySelector('.sp-chains');
  const chainRows = () => block.querySelectorAll('.sp-chain-row');
  assert.equal(chainRows().length, 1);
  assert.equal(chainRows()[0].querySelector('.sp-label').textContent, 'Morning');
  assert.equal(chainRows()[0].querySelector('.sp-desc').textContent, 'New tab → History');
  sent.length = 0;
  const tool = (label) => chainRows()[0].querySelectorAll('button').find((b) => b.getAttribute('aria-label') === label);
  tool('Clone Morning').click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'chainclone', n: 1 });
  tool('Test Morning').click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'chaintest', text: '1\tMorning\n0\tnew-tab\n250\thistory' });
  tool('Delete Morning').click();
  assert.deepEqual(sent, [], 'delete asks first');
  assert.match(chainRows()[0].textContent, /Delete Morning\?/);
  buttonNamed(chainRows()[0], 'Cancel').click();
  assert.equal(sent.length, 0);
  tool('Delete Morning').click();
  buttonNamed(chainRows()[0], 'Delete').click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'chaindelete', n: 1 });

  // Edit: the editor opens on the chain, with its steps in order.
  const editor = block.querySelector('.sp-chain-editor');
  assert.equal(editor.hidden, true);
  tool('Edit Morning').click();
  assert.equal(editor.hidden, false);
  assert.equal(editor.querySelector('.sp-chain-name').value, 'Morning');
  const steps = () => editor.querySelectorAll('.sp-chain-step');
  assert.deepEqual(steps().map((s) => s.querySelector('.bw-select-value').textContent), ['New tab', 'History']);
  assert.deepEqual(steps().map((s) => s.querySelector('.sp-chain-delay').value), ['0', '250']);
  // Delays clamp; order moves; a step removes.
  const delay = steps()[1].querySelector('.sp-chain-delay');
  delay.value = '99999999';
  delay.dispatch('change');
  assert.equal(delay.value, '60000');
  steps()[1].querySelector('.sp-chain-delay').value = '300';
  steps()[1].querySelector('.sp-chain-delay').dispatch('change');
  const stepMove = (at, label) => steps()[at].querySelectorAll('button').find((b) => b.getAttribute('aria-label').startsWith(label));
  stepMove(1, 'Move command 2 up').click();
  assert.deepEqual(steps().map((s) => s.querySelector('.bw-select-value').textContent), ['History', 'New tab']);
  stepMove(0, 'Move command 1 down').click();
  assert.deepEqual(steps().map((s) => s.querySelector('.bw-select-value').textContent), ['New tab', 'History']);
  // Test sends the draft without saving; Save sends it as an edit of chain 1.
  sent.length = 0;
  buttonNamed(editor, 'Test chain').click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'chaintest', text: '1\tMorning\n0\tnew-tab\n300\thistory' });
  buttonNamed(editor, 'Save chain').click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'chainsave', text: '1\tMorning\n0\tnew-tab\n300\thistory' });
  // Bowser's refusal stays in the editor; its "saved" closes it.
  deliver(commandsSnap({ ack: 'failed', error: 'There is already a chain called Morning.' }));
  assert.equal(editor.hidden, false);
  assert.equal(editor.querySelector('.sp-chain-error').textContent, 'There is already a chain called Morning.');
  buttonNamed(editor, 'Save chain').click();
  deliver(commandsSnap({ ack: 'saved' }));
  assert.equal(editor.hidden, true);
  assert.equal(app.querySelector('.sp-toast').textContent, 'Saved');
  // Test is acknowledged as a start, not a save.
  deliver(commandsSnap({ ack: 'ran' }));
  assert.equal(app.querySelector('.sp-toast').textContent, 'Chain started');
});

test('a new chain is checked here before it is sent: a name, a command, whole-millisecond delays', () => {
  deliver(commandsSnap());
  const block = chainsPage().querySelector('.sp-chains');
  const editor = block.querySelector('.sp-chain-editor');
  sent.length = 0;
  buttonNamed(block, 'New chain').click();
  assert.equal(editor.hidden, false);
  assert.equal(editor.querySelector('.sp-chain-name').value, '');
  buttonNamed(editor, 'Create chain').click();
  assert.equal(editor.querySelector('.sp-chain-error').textContent, 'Give the chain a name.');
  editor.querySelector('.sp-chain-name').value = 'morning';
  editor.querySelector('.sp-chain-name').dispatch('input');
  buttonNamed(editor, 'Create chain').click();
  assert.equal(editor.querySelector('.sp-chain-error').textContent, 'There is already a chain called morning.', 'names are unique, ignoring case');
  editor.querySelector('.sp-chain-name').value = 'Evening';
  editor.querySelector('.sp-chain-name').dispatch('input');
  buttonNamed(editor, 'Test chain').click();
  assert.equal(editor.querySelector('.sp-chain-error').textContent, 'Add at least one command to the chain.');
  assert.deepEqual(sent, [], 'nothing invalid is sent');
  buttonNamed(editor, 'Cancel').click();
  assert.equal(editor.hidden, true);
});

test('the new rows join the page search and the Keyboard and Quick commands blocks hide with it', () => {
  assert.ok(logic.matchRows('rebind shortcut').has('kbEditor'));
  assert.ok(logic.matchRows('prefix bkm tb cmd').has('qcTypes'));
  assert.ok(logic.matchRows('macro delay milliseconds').has('chains'));
  assert.ok(logic.matchRows('alt menu').has('kbAltMenu'));
  assert.ok(logic.matchRows('f6 focus').has('kbFocusCycling'));
  assert.ok(logic.matchRows('website priority').has('kbSites'));
  assert.ok(logic.matchRows('single key').has('kbSingleKey'));
  assert.ok(logic.matchRows('notes manager').has('qcNotesPanel'));
  // Typing in the page search hides the blocks whose rows do not match.
  const search = app.querySelector('.sp-search-input');
  search.value = 'macro';
  search.dispatch('input');
  assert.equal(chainsPage().querySelector('.sp-chains').hidden, false);
  assert.equal(app.querySelector('[data-opens="chains"]').hidden, false, 'the row that opens the chains is the hit');
  assert.equal(kbPage().querySelector('.sp-kb').hidden, true);
  assert.equal(app.querySelector('section[data-section="keyboard"]').hidden, true, 'a part with no match is hidden');
  assert.equal(app.querySelector('section[data-section="commands"]').hidden, false);
  search.value = '';
  search.dispatch('input');
  assert.equal(kbPage().querySelector('.sp-kb').hidden, false);
});

test('a bad commands snapshot is dropped and leaves the sections as they were', () => {
  deliver(commandsSnap());
  const before = app.querySelectorAll('.sp-kb-row').length;
  deliver(commandsSnap({ commands: 'nope' }));
  deliver({ ...commandsFixture, d: { ...commandsFixture.d, extra: 1 } });
  assert.equal(app.querySelectorAll('.sp-kb-row').length, before);
});
// --- end keyboard and quick commands ---

// ---- the Start page section (docs/design/start-page.md) ----

test('the Start page section: the snapshot parses, every row exists once and the keys are the ones Rust owns', () => {
  const s = logic.parseSettingsSnapshot(fixture.snap.d);
  assert.ok(s, 'the Rust snapshot carries the start object');
  assert.equal(s.start.columns, 6);
  assert.equal(s.start.startup, 'start_page');
  const ids = logic.ROWS.filter((row) => row.section === 'startpage').map((row) => row.id);
  assert.deepEqual(ids, [
    'startGroupReopen', 'startTitles', 'startAddButton', 'startColumns', 'startThumbSize', 'startDragReorder', 'startHomepage', 'startupMode',
    'startupPages', 'startWidgets', 'startWidgetColumns', 'startWidgetHeight', 'startWidgetBg', 'startWidgetColor', 'startWidgetDrag',
  ]);
  for (const row of logic.ROWS.filter((r) => r.section === 'startpage')) {
    assert.ok(!row.notYet, `${row.id} acts, so it carries no "no effect yet" note`);
    assert.ok(!row.restart, `${row.id} applies at once`);
  }
  // The page's save text carries every start key once, with Rust's own key names.
  const text = logic.saveText(logic.draftOf(s));
  const rustKeys = [...read(join(root, 'src', 'start_page.rs')).matchAll(/^    "([a-z_]+)",$/gm)].map((m) => m[1]);
  for (const key of rustKeys) assert.equal(text.split('\n').filter((l) => l.startsWith(`${key}=`)).length, 1, key);
  assert.ok(rustKeys.length === 15);
  // A start object that Rust would never write is refused.
  const { start } = fixture.snap.d;
  for (const bad of [{ ...start, columns: 13 }, { ...start, columns: 0 }, { ...start, thumbSize: 6 }, { ...start, titles: 'sometimes' },
    { ...start, startup: 'restore' }, { ...start, homepage: 'ftp://x.example/' }, { ...start, startupPages: ['not a url'] },
    { ...start, startupPages: Array.from({ length: 9 }, (_, i) => `https://p${i}.example/`) }, { ...start, widgetColor: 'red' },
    { ...start, widgetColumns: 5 }, { ...start, extra: 1 }, { ...start, widgetDrag: 'yes' }]) {
    assert.equal(logic.parseSettingsSnapshot({ ...fixture.snap.d, start: bad }), null, JSON.stringify(bad).slice(0, 100));
  }
  const { widgetDrag: _drop, ...missing } = start;
  assert.equal(logic.parseSettingsSnapshot({ ...fixture.snap.d, start: missing }), null);
});

test('the Start page section: each control saves its own key, and a snapshot moves the controls', () => {
  deliver(fixture.snap);
  const row = (id) => app.querySelector(`.sp-row[data-row="${id}"]`);
  const lastSave = () => sent.filter((m) => m.t === 'set' && m.a === 'save').at(-1).text.split('\n');
  const value = (key) => lastSave().find((l) => l.startsWith(`${key}=`));
  const radios = (id) => row(id).querySelectorAll('[role="radio"]');
  const flip = (id, key, expected) => {
    sent.length = 0;
    row(id).querySelector('[role="switch"]').click();
    assert.equal(value(key), `${key}=${expected}`, id);
    assert.equal(sent.length, 1, `${id} sends one save`);
    deliver({ ...fixture.snap, d: { ...fixture.snap.d, ack: 'saved' } });
  };
  // Switches (the page starts from the Rust fixture: add button on, drag on, widgets off, widget drag on).
  flip('startAddButton', 'start_add_button', 'false');
  flip('startDragReorder', 'start_drag_reorder', 'false');
  flip('startWidgets', 'start_widgets', 'true');
  // The widget rows follow the widgets switch once Bowser answers with it on.
  const widgetsOn = { ...fixture.snap.d, start: { ...fixture.snap.d.start, widgets: true } };
  assert.equal(radios('startWidgetColumns').every((b) => b.disabled), true, 'off while widgets are off');
  deliver({ ...fixture.snap, d: widgetsOn });
  assert.equal(radios('startWidgetColumns').every((b) => b.disabled), false);
  flip('startWidgetDrag', 'start_widget_drag', 'false');
  deliver({ ...fixture.snap, d: widgetsOn });

  // Segmented controls: titles (always / when needed / never), group reopen, tile size, widget columns / height / background.
  const pick = (id, index, key, expected) => {
    sent.length = 0;
    radios(id)[index].click();
    assert.equal(value(key), `${key}=${expected}`, id);
    deliver({ ...fixture.snap, d: widgetsOn });
  };
  assert.deepEqual(radios('startTitles').map((b) => b.textContent), ['Always', 'When needed', 'Never']);
  pick('startTitles', 1, 'start_titles', 'auto');
  pick('startTitles', 2, 'start_titles', 'never');
  pick('startGroupReopen', 1, 'start_group_reopen', 'first');
  assert.deepEqual(radios('startThumbSize').map((b) => b.textContent), ['Tiny', 'Small', 'Medium', 'Large', 'Huge'], 'five thumbnail sizes');
  for (const [i, size] of [[0, '1'], [1, '2'], [3, '4'], [4, '5']]) pick('startThumbSize', i, 'start_thumb_size', size);
  for (const [i, n] of [[0, '1'], [2, '3'], [3, '4']]) pick('startWidgetColumns', i, 'start_widget_columns', n);
  pick('startWidgetHeight', 1, 'start_widget_height', 'tall');
  assert.deepEqual(radios('startWidgetBg').map((b) => b.textContent), ['Solid', 'Semi-transparent', 'Transparent', 'Custom']);
  for (const [i, bg] of [[0, 'solid'], [2, 'transparent'], [3, 'custom']]) pick('startWidgetBg', i, 'start_widget_bg', bg);

  // The custom colour only matters (and only enables) while the background is Custom.
  assert.equal(row('startWidgetColor').querySelector('input').disabled, true);
  deliver({ ...fixture.snap, d: { ...widgetsOn, start: { ...widgetsOn.start, widgetBg: 'custom' } } });
  const colour = row('startWidgetColor').querySelector('input');
  assert.equal(colour.disabled, false);
  sent.length = 0;
  colour.value = '#336699';
  colour.dispatch('change');
  assert.equal(value('start_widget_color'), 'start_widget_color=#336699');

  // Homepage: an address is normalised and saved; nonsense is refused with the toast and nothing is sent.
  const home = row('startHomepage').querySelector('input');
  sent.length = 0;
  home.value = 'example.com/start';
  home.dispatch('change');
  assert.equal(value('homepage'), 'homepage=https://example.com/start');
  assert.equal(home.value, 'https://example.com/start');
  sent.length = 0;
  home.value = 'javascript:alert(1)';
  home.dispatch('change');
  assert.equal(sent.length, 0, 'refused');
  assert.equal(app.querySelector('.sp-toast').textContent, 'Enter an http or https address.');
  sent.length = 0;
  home.value = '';
  home.dispatch('change');
  assert.equal(value('homepage'), 'homepage=');

  // Specific pages: only while that startup mode is chosen; one address per line, at most eight.
  const pages = row('startupPages').querySelector('textarea');
  assert.equal(pages.disabled, true, 'greyed unless startup is "specific pages"');
  deliver({ ...fixture.snap, d: { ...widgetsOn, start: { ...widgetsOn.start, startup: 'specific_pages' } } });
  assert.equal(pages.disabled, false);
  sent.length = 0;
  pages.value = 'https://a.example/\n\n b.example \n';
  pages.dispatch('change');
  assert.equal(value('startup_pages'), 'startup_pages=https://a.example/ https://b.example');
  sent.length = 0;
  pages.value = Array.from({ length: 9 }, (_, i) => `https://p${i}.example/`).join('\n');
  pages.dispatch('change');
  assert.equal(sent.length, 0, 'nine pages are refused');
  pages.value = 'ftp://nope.example/';
  pages.dispatch('change');
  assert.equal(sent.length, 0);

  // The rows are found by search.
  for (const [query, id] of [['speed dial columns', 'startColumns'], ['homepage', 'startHomepage'], ['where you left off', 'startupMode'], ['sticky note', 'startWidgets'], ['widget background', 'startWidgetBg']]) {
    assert.ok(logic.matchRows(query).has(id), query);
  }
  deliver(fixture.snap);
});

test('the Start page section follows the snapshot: values, the startup mode and the unreadable-file lock', () => {
  const tuned = { ...fixture.snap.d, start: { ...fixture.snap.d.start, columns: 9, startup: 'homepage', homepage: 'https://home.example/', widgets: true, widgetBg: 'custom', widgetColor: '#336699' } };
  deliver({ ...fixture.snap, d: tuned });
  const row = (id) => app.querySelector(`.sp-row[data-row="${id}"]`);
  assert.equal(row('startColumns').querySelector('.bw-select-button').textContent, '9');
  assert.equal(row('startupMode').querySelector('.bw-select-button').textContent, 'Open the homepage');
  assert.equal(row('startHomepage').querySelector('input').value, 'https://home.example/');
  assert.equal(row('startWidgetColor').querySelector('input').value, '#336699');
  assert.equal(row('startWidgetBg').querySelector('[role="radio"][aria-checked="true"]').textContent, 'Custom');
  deliver({ ...fixture.snap, d: { ...tuned, readable: false } });
  for (const id of ['startAddButton', 'startWidgets']) assert.equal(row(id).querySelector('[role="switch"]').disabled, true, id);
  assert.equal(row('startHomepage').querySelector('input').disabled, true);
  deliver(fixture.snap);
});

// ---- Address bar, search engine and bookmark sections (src/address-section.ts) ----

const addressFixture = JSON.parse(readFileSync(join(here, 'fixtures', 'address-messages.json'), 'utf8'));

test('Address bar section: every switch sends one addrset command, never part of save', () => {
  deliver(addressFixture.snap);
  const rowOf = (id) => app.querySelector(`.sp-row[data-row="${id}"]`);
  const full = rowOf('addrFull').querySelector('[role="switch"]');
  assert.equal(full.getAttribute('aria-checked'), 'false');
  assert.equal(rowOf('addrAutocomplete').querySelector('[role="switch"]').getAttribute('aria-checked'), 'true');
  sent.length = 0;
  full.click();
  rowOf('ddTyping').querySelector('[role="switch"]').click();
  rowOf('secHttps').querySelector('[role="switch"]').click();
  assert.deepEqual(sent, [
    { v: 1, t: 'set', a: 'addrset', text: 'address_show_full=true' },
    { v: 1, t: 'set', a: 'addrset', text: 'dd_show_while_typing=false' },
    { v: 1, t: 'set', a: 'addrset', text: 'sec_always_https=true' },
  ]);
  assert.ok(!sent.some((m) => m.a === 'save'));
  // Numbers and text are checked before they are sent.
  const max = rowOf('ddMax').querySelector('input');
  assert.equal(max.value, '8');
  sent.length = 0;
  max.value = '12';
  max.dispatch('change');
  max.value = '99';
  max.dispatch('change');
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'addrset', text: 'dd_max_results=12' }], 'out of range is refused on the page');
  assert.equal(max.value, '12', 'and the field goes back to the last good value');
  const suffix = rowOf('addrExpansion').querySelector('input');
  sent.length = 0;
  suffix.value = '.co.uk';
  suffix.dispatch('change');
  suffix.value = 'bad suffix';
  suffix.dispatch('change');
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'addrset', text: 'domain_expansion_suffix=co.uk' }]);
});

test('The suggestion sources list in order, turn off and move with one command each', () => {
  deliver(addressFixture.snap);
  const items = app.querySelectorAll('.ad-source');
  assert.deepEqual(items.map((i) => i.dataset.source), ['bookmarks', 'history', 'direct', 'nicknames', 'search', 'search-history', 'calculator']);
  assert.deepEqual(items.map((i) => i.classList.contains('is-off')), [false, false, true, true, true, true, true]);
  sent.length = 0;
  items[1].querySelector('[role="switch"]').click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'addrset', text: 'dd_sources=bookmarks' });
  items[2].querySelector('[role="switch"]').click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'addrset', text: 'dd_sources=bookmarks,history,direct' });
  items[1].querySelector('[aria-label="Move History and open tabs up"]').click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'addrset', text: 'dd_sources=history,bookmarks' });
  assert.equal(items[0].querySelector('[aria-label="Move Bookmarks up"]').disabled, true, 'the first cannot go up');
  assert.equal(items[2].querySelector('[aria-label="Move Direct match up"]').disabled, true, 'a source that is off has nothing to move');
});

// --- wave-1 fixes ---
test('The engine list shows nicknames only while "Show the nickname" is on', () => {
  const withNick = (on) => ({ ...addressFixture.snap, d: { ...addressFixture.snap.d, s: { ...addressFixture.snap.d.s, search_show_nickname: on } } });
  deliver(withNick(false));
  assert.equal(app.querySelector('.ad-engine').querySelector('.sp-label').textContent, 'Optimised');
  deliver(withNick(true));
  assert.equal(app.querySelector('.ad-engine').querySelector('.sp-label').textContent, 'Optimised (opt)');
  deliver(addressFixture.snap);
});

test('The engine list shows every engine with its badges, and each action is one engine command', () => {
  deliver(addressFixture.snap);
  const items = app.querySelectorAll('.ad-engine');
  assert.equal(items.length, 7);
  assert.equal(items[0].dataset.engine, 'optimised');
  assert.deepEqual(items[0].querySelectorAll('.sp-pill').map((b) => b.textContent), ['Default']);
  assert.deepEqual(app.querySelector('.ad-engine[data-engine="duckduckgo"]').querySelectorAll('.sp-pill').map((b) => b.textContent), ['Private']);
  assert.deepEqual(app.querySelector('.ad-engine[data-engine="google"]').querySelectorAll('.sp-pill').map((b) => b.textContent), ['Images']);
  assert.equal(items[0].querySelector('.sp-label').textContent, 'Optimised (opt)');
  assert.equal(items[0].querySelector('[aria-label="Remove Optimised"]').disabled, true, 'the default engine stays');
  assert.equal(items[0].querySelector('[aria-label="Move Optimised up"]').disabled, true);
  sent.length = 0;
  app.querySelector('.ad-engine[data-engine="bing"]').querySelector('[aria-label="Remove Bing"]').click();
  items[1].querySelector('[aria-label="Move Google down"]').click();
  assert.deepEqual(sent, [
    { v: 1, t: 'set', a: 'engine', text: 'remove\tbing' },
    { v: 1, t: 'set', a: 'engine', text: 'move\tgoogle\t2' },
  ]);
  const restore = [...app.querySelectorAll('.bw-button')].find((b) => b.textContent === 'Restore default engines');
  sent.length = 0;
  restore.click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'engine', text: 'restore' }]);
});

test('Adding an engine checks the form first, then sends one add command; editing prefills it', () => {
  deliver(addressFixture.snap);
  const form = app.querySelector('.ad-form');
  assert.equal(form.hidden, true);
  const add = [...app.querySelectorAll('.bw-button')].find((b) => b.textContent === 'Add a search engine');
  add.click();
  assert.equal(form.hidden, false);
  const fields = form.querySelectorAll('input');
  assert.equal(fields.length, 7);
  const [name, nick, url] = fields;
  sent.length = 0;
  name.value = 'Mine';
  nick.value = 'g'; // Google's
  url.value = 'https://mine.example/?q=%s';
  form.dispatch('submit');
  assert.deepEqual(sent, [], 'a taken nickname is refused on the page');
  assert.match(form.querySelector('.sp-field-error').textContent, /already used by Google/);
  nick.value = 'mn';
  form.dispatch('submit');
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'engine', text: 'add\tMine\tmn\thttps://mine.example/?q=%s\t\t\t\t' }]);
  assert.equal(form.hidden, true, 'the form closes once the change is sent');
  app.querySelector('.ad-engine[data-engine="bing"]').querySelector('[aria-label="Edit Bing"]').click();
  assert.equal(form.hidden, false);
  assert.equal(form.querySelector('.ad-form-title').textContent, 'Edit Bing');
  assert.equal(form.querySelectorAll('input')[2].value, 'https://www.bing.com/search?q=%s');
  assert.match(form.querySelectorAll('input')[6].placeholder, /img=%s/);
});

test('Search field, scoped nicknames and bookmark rows send their own keys; bar rows join the bookmarks bar card', () => {
  deliver(addressFixture.snap);
  const rowOf = (id) => app.querySelector(`.sp-row[data-row="${id}"]`);
  sent.length = 0;
  rowOf('searchField').querySelectorAll('[role="radio"]')[2].click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'addrset', text: 'search_field=button' });
  rowOf('searchNewTab').querySelector('[role="switch"]').click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'addrset', text: 'search_new_tab=true' });
  const scope = rowOf('nickTabs');
  assert.equal(scope.querySelector('[role="switch"]').getAttribute('aria-checked'), 'true');
  scope.querySelector('[role="switch"]').click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'addrset', text: 'nick_tabs=' }, 'a scope turns off with an empty word');
  const word = scope.querySelector('input');
  word.value = '@t';
  word.dispatch('change');
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'addrset', text: 'nick_tabs=t' });
  const confirmCount = rowOf('bmConfirm').querySelector('input');
  confirmCount.value = '1';
  confirmCount.dispatch('change');
  assert.deepEqual(sent, [], 'one is not a count that asks');
  confirmCount.value = '5';
  confirmCount.dispatch('change');
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'addrset', text: 'bm_confirm_open=5' });
  rowOf('bmBarDisplay').querySelectorAll('[role="radio"]')[3].click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'addrset', text: 'bm_bar_display=bookmark-icon' });
  rowOf('bmBarPosition').querySelectorAll('[role="radio"]')[1].click();
  assert.deepEqual(sent.pop(), { v: 1, t: 'set', a: 'addrset', text: 'bm_bar_position=bottom' });
  const headingOf = (id) => rowOf(id).parent.parent.querySelector('.sp-group-h').textContent;
  assert.equal(headingOf('bmBarDisplay'), headingOf('bookmarksBar'), 'under the page\'s own Bookmarks bar heading');
  assert.equal(headingOf('bmBarDisplay'), 'Bookmarks bar');
  assert.equal(rowOf('bmBarFolder').querySelector('.bw-select-button') !== null, true);
  // A bad snapshot is dropped and leaves the page as it was.
  deliver({ ...addressFixture.snap, d: { ...addressFixture.snap.d, def: 'gone' } });
  assert.equal(app.querySelectorAll('.ad-engine').length, 7);
});

// ---- wave-1 integration: one coherent page ----

test('integrated page: sixteen sections in order, no duplicate rows or keys, and the moved tab rows still save', () => {
  assert.deepEqual(logic.SECTIONS.map((s) => s.key), ['profile', 'appearance', 'tabs', 'startpage', 'privacy', 'performance', 'address', 'search', 'bookmarks', 'keyboard', 'commands', 'downloads', 'media', 'extensions', 'updates', 'about']);
  const ids = logic.ROWS.map((r) => r.id);
  assert.equal(new Set(ids).size, ids.length, 'no row twice');
  const labels = logic.ROWS.map((r) => r.label.toLowerCase());
  assert.equal(new Set(labels).size, labels.length, 'no two rows with one label');
  // "Open new tabs next to the active tab" was a second control for `new_tab_beside_active`, which
  // Tabs > New tab position owns now; it is gone from the page (the key stays in the save text).
  assert.equal(logic.ROWS.find((r) => r.id === 'newTabBesideActive'), undefined);
  assert.equal(app.querySelector('.sp-row[data-row="newTabBesideActive"]'), null);
  // The Ctrl+Tab order and the tab previews sit in Tabs > Tab bar, after T1's own rows.
  const tabsSection = app.querySelector('section[data-area="tabs"]');
  for (const id of ['ctrlTabMru', 'tabPreviewImages', 'tabPreviewMemory']) {
    assert.equal(logic.ROWS.find((r) => r.id === id).section, 'tabs', id);
    assert.ok(tabsSection.querySelector(`.sp-row[data-row="${id}"]`), `${id} is drawn in Tabs`);
  }
  // Each still saves its own key through the whole-draft save.
  deliver(fixture.snap);
  for (const [id, key] of [['ctrlTabMru', 'ctrl_tab_mru'], ['tabPreviewImages', 'tab_preview_images'], ['tabPreviewMemory', 'tab_preview_memory']]) {
    sent.length = 0;
    tabsSection.querySelector(`.sp-row[data-row="${id}"]`).querySelector('[role="switch"]').click();
    assert.equal(sent[0].a, 'save', id);
    assert.match(sent[0].text, new RegExp(`\\n${key}=false\\n`), id);
    deliver(fixture.snap);
  }
  // The new tab page's four choices sit on the Tabs section's first row.
  const newTab = tabsSection.querySelector('.sp-row[data-row="tabs_new_tab_page"]');
  assert.ok(newTab);
  assert.ok(logic.matchRows('blank page').has('tabs_new_tab_page'));
  assert.ok(logic.matchRows('nickname').has('qcNickname'));
});
// --- wave-1 fixes --- descriptions say what the rows really do.
test('row descriptions match what the rows do', () => {
  const desc = (id) => logic.ROWS.find((row) => row.id === id).description;
  const row = (id) => logic.ROWS.find((r) => r.id === id);
  assert.match(desc('reset'), /Not changed: how long history is kept/);
  for (const reset of ['appearance', 'start page', 'address bar', 'search engines', 'downloads', 'translation']) assert.ok(desc('reset').toLowerCase().includes(reset), reset);
  assert.match(desc('blockYoutubeVideoAds'), /Trackers and ads/);
  assert.match(desc('showHomeButton'), /rail/);
  assert.match(desc('minimalUrl'), /path/);
  assert.match(desc('forceDark'), /open tabs at once/);
  assert.match(desc('qcNotesPanel'), /does not yet go to the picked note/);
  // --- layout (thread T6) --- the side panel's edge is a choice of the Layout block's edges row and acts at once.
  assert.equal(row('sidePanelPosition'), undefined, 'no separate side panel row any more');
  assert.match(row('layoutBars').label, /panels/i);
  assert.ok(!row('layoutBars').restart && !row('layoutBars').notYet, 'it applies without a restart and is live');
  assert.match(desc('statusBar'), /Bowser has no status bar of its own/);
  assert.match(desc('addressBarVisible'), /Ctrl\+L/);
});
// ---- settings slim: a short core list per area, one "More options", sub-pages, "Not active yet" ----

const areaNode = (key) => app.querySelector(`section[data-area="${key}"]`);
const moreOf = (key) => areaNode(key).querySelector('.sp-more');
const rowNode = (id) => app.querySelector(`[data-row="${id}"]`);
const tierOf = (id) => logic.rowTier(logic.ROWS.find((r) => r.id === id));
const searchFor = (text) => { const input = app.querySelector('.sp-search-input'); input.value = text; input.dispatch('input'); };

test('slim: each area shows a short core list and puts the rest behind one closed "More options" that remembers itself', () => {
  deliver(fixture.snap);
  searchFor('');
  for (const area of logic.AREAS) {
    const node = areaNode(area.key);
    const inArea = logic.ROWS.filter((row) => area.parts.includes(row.section) && tierOf(row.id) !== 'later');
    const core = inArea.filter((row) => tierOf(row.id) === 'core' || tierOf(row.id) === 'page');
    const more = inArea.filter((row) => tierOf(row.id) === 'more');
    assert.ok(core.length > 0 && core.length <= 10, `${area.key}: ${core.length} rows in its main view`);
    for (const row of core) {
      const drawn = tierOf(row.id) === 'page' ? app.querySelector(`[data-opens="${row.id}"]`) : rowNode(row.id);
      assert.ok(node.contains(drawn), `${row.id} is in ${area.key}`);
      assert.ok(!(moreOf(area.key)?.contains(drawn) ?? false), `${row.id} is not behind More options`);
    }
    if (more.length === 0) { assert.equal(moreOf(area.key), null, `${area.key} has no long tail`); continue; }
    const d = moreOf(area.key);
    assert.equal(node.querySelectorAll('.sp-more').length, 1, 'one disclosure per area');
    assert.equal(d.querySelector('.sp-label').textContent, logic.MORE_LABEL);
    assert.equal(d.querySelector('.sp-desc').textContent, logic.moreCountText(more.length));
    for (const row of more) assert.ok(d.contains(rowNode(row.id)), `${row.id} is behind More options`);
    // Closed by default; Privacy was left open last time.
    const open = area.key === 'privacy';
    assert.equal(d.querySelector('.sp-more-btn').getAttribute('aria-expanded'), String(open), area.key);
    assert.equal(d.querySelector('.sp-more-body').hidden, !open, area.key);
  }
  // Opening and closing is remembered.
  const button = moreOf('tabs').querySelector('.sp-more-btn');
  button.click();
  assert.equal(button.getAttribute('aria-expanded'), 'true');
  assert.equal(moreOf('tabs').querySelector('.sp-more-body').hidden, false);
  assert.deepEqual(JSON.parse(stored.get('bowser.settings.more')).sort(), ['privacy', 'tabs']);
  button.click();
  assert.equal(moreOf('tabs').querySelector('.sp-more-body').hidden, true);
  assert.deepEqual(JSON.parse(stored.get('bowser.settings.more')), ['privacy']);
  // A row behind the disclosure still saves as before.
  sent.length = 0;
  rowNode('tabs_mute_mode').querySelectorAll('[role="radio"]')[1].click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'tabopt', text: 'tabs_mute_mode=active' }]);
});

test('slim: the search finds rows behind a closed disclosure and opens it; clearing closes it unless a found row was used', () => {
  deliver(fixture.snap);
  const d = moreOf('tabs');
  const button = d.querySelector('.sp-more-btn');
  const body = d.querySelector('.sp-more-body');
  assert.equal(button.getAttribute('aria-expanded'), 'false');
  searchFor('tab audio');
  assert.equal(rowNode('tabs_mute_mode').hidden, false);
  assert.equal(button.getAttribute('aria-expanded'), 'true', 'the hit opens its disclosure');
  assert.equal(body.hidden, false);
  assert.equal(d.hidden, false);
  assert.equal(areaNode('tabs').hidden, false);
  assert.equal(app.querySelector('.sp-nav-item[aria-current="true"]').dataset.section, 'tabs');
  // Disclosures with no hit stay out of the way.
  assert.equal(moreOf('appearance').hidden, true);
  searchFor('');
  assert.equal(button.getAttribute('aria-expanded'), 'false', 'only opened for the search');
  assert.equal(body.hidden, true);
  searchFor('tab audio');
  body.dispatch('focusin');
  searchFor('');
  assert.equal(button.getAttribute('aria-expanded'), 'true', 'a row used from the search stays in view');
  button.click();
  // Rows that do nothing yet are found too, in their own closed section.
  searchFor('frameless');
  assert.equal(areaNode('later').hidden, false);
  assert.equal(areaNode('later').querySelector('.sp-more-btn').getAttribute('aria-expanded'), 'true');
  assert.equal(rowNode('frameless').hidden, false);
  assert.equal(areaNode('appearance').hidden, true, 'not in Appearance any more');
  searchFor('');
  assert.equal(areaNode('later').querySelector('.sp-more-body').hidden, true);
});

test('slim: no row that does nothing yet renders as a live control', () => {
  deliver(fixture.snap);
  const later = areaNode('later');
  assert.equal(later.querySelector('.sp-h').textContent, logic.LATER_TITLE);
  assert.equal(later.querySelector('.sp-more-btn').getAttribute('aria-expanded'), 'false', 'collapsed by default');
  assert.equal(later.querySelector('.sp-more-btn').querySelector('.sp-desc').textContent, 'Frameless window');
  for (const row of logic.ROWS) {
    const node = rowNode(row.id);
    assert.equal(later.contains(node), row.notYet === true, `${row.id} is in Not active yet only when it does nothing yet`);
    assert.equal(tierOf(row.id) === 'later', row.notYet === true, row.id);
    if (!row.notYet) continue;
    const controls = node.querySelectorAll('button, input');
    assert.ok(controls.length > 0);
    assert.ok(controls.every((c) => c.disabled && c.getAttribute('aria-disabled') === 'true'), `${row.id} has no live control`);
    assert.equal(node.querySelector('.sp-flag').textContent, logic.NOT_YET_NOTE);
  }
  sent.length = 0;
  rowNode('frameless').querySelector('[role="switch"]').click();
  assert.deepEqual(sent, []);
  // A fresh snapshot (which enables every working control) leaves them locked.
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, readable: true } });
  assert.equal(rowNode('frameless').querySelector('[role="switch"]').disabled, true);
});

test('slim: every editor opens on its own sub-page from one row, and Back, a nav pick or a search returns', () => {
  deliver(fixture.snap);
  for (const [id, spec] of Object.entries(logic.PAGE_ROWS)) {
    const opener = app.querySelector(`[data-opens="${id}"]`);
    assert.ok(opener, id);
    assert.equal(app.querySelectorAll(`[data-opens="${id}"]`).length, 1, `one row opens ${id}`);
    const area = logic.areaOf(logic.ROWS.find((r) => r.id === id).section);
    assert.ok(areaNode(area.key).contains(opener), `${id} opens from ${area.key}`);
    const button = opener.querySelector('.bw-button');
    assert.equal(button.textContent, spec.button);
    const subPage = app.querySelector(`section[data-page="${id}"]`);
    assert.equal(subPage.hidden, true);
    assert.ok(subPage.contains(rowNode(id)), `${id} lives on its sub-page`);
    button.click();
    assert.equal(subPage.hidden, false, id);
    assert.ok(logic.AREAS.every((a) => areaNode(a.key).hidden), 'the page shows only the editor');
    assert.equal(subPage.querySelector('.sp-h').textContent, logic.ROWS.find((r) => r.id === id).label);
    assert.equal(app.querySelector('.sp-nav-item[aria-current="true"]').dataset.section, area.key);
    subPage.querySelector('.sp-back').click();
    assert.equal(subPage.hidden, true);
    assert.ok(logic.AREAS.every((a) => !areaNode(a.key).hidden));
    assert.equal(document.activeElement, button, 'Back returns to the row that opened it');
  }
  // A nav pick or a search leaves the sub-page too.
  const engines = app.querySelector('[data-opens="engines"]').querySelector('.bw-button');
  engines.click();
  deliver(fixture.go);
  assert.equal(app.querySelector('section[data-page="engines"]').hidden, true);
  assert.equal(areaNode('privacy').hidden, false);
  engines.click();
  searchFor('img');
  assert.equal(app.querySelector('section[data-page="engines"]').hidden, true);
  assert.equal(rowNode('searchDefault').hidden, false);
  // A hit inside a sub-page shows the row that opens it.
  searchFor('reorder nickname suggest');
  assert.equal(app.querySelector('[data-opens="engines"]').hidden, false);
  searchFor('');
  // The editors still act from their sub-page.
  sent.length = 0;
  app.querySelector('.ad-engine[data-engine="bing"]').querySelector('[aria-label="Remove Bing"]').click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'engine', text: 'remove\tbing' }]);
});

test('slim: a deep link to a merged section lands in its area; areas name their parts', () => {
  sent.length = 0;
  deliver({ v: 1, t: 'settingsgo', s: 'search' });
  assert.deepEqual(sent, []);
  assert.equal(app.querySelector('.sp-nav-item[aria-current="true"]').dataset.section, 'address');
  deliver({ v: 1, t: 'settingsgo', s: 'startpage' });
  assert.equal(app.querySelector('.sp-nav-item[aria-current="true"]').dataset.section, 'tabs');
  for (const section of logic.SETTINGS_SECTIONS) assert.ok(logic.areaOf(section).parts.includes(section), section);
  assert.deepEqual(logic.AREAS.flatMap((a) => a.parts).sort(), [...logic.SETTINGS_SECTIONS].sort(), 'every Rust section is in exactly one area');
  // A merged area draws each part's core rows as one card under the part's own title.
  const parts = areaNode('address').querySelectorAll('section.sp-part').map((p) => p.querySelector('.sp-group-h').textContent);
  assert.deepEqual(parts, ['Address bar', 'Search engine', 'Bookmarks']);
});

test('slim: a row nobody placed defaults to More options, so new rows never lengthen the main view', () => {
  const base = { section: 'appearance', label: 'x', description: '', keywords: '' };
  assert.equal(logic.rowTier({ ...base, id: 'someNewRow' }), 'more');
  assert.equal(logic.rowTier({ ...base, id: 'someNewRow', tier: 'core' }), 'core');
  assert.equal(logic.rowTier({ ...base, id: 'someNewRow', tier: 'core', notYet: true }), 'later', 'a row that does nothing is never shown as live');
  assert.equal(logic.rowTier({ ...base, id: 'engines' }), 'page');
  assert.equal(logic.moreCountText(1), '1 more setting');
  assert.equal(logic.moreCountText(12), '12 more settings');
});

// ---- layout (thread T6): presets, the three edges, the long tail behind More options ----
const layoutLogic = await bundle('layout-logic.ts');

test('layout: the snapshot carries the Layout block, refuses anything else, and the save lists its keys', () => {
  const d = fixture.snap.d;
  const s = logic.parseSettingsSnapshot(d);
  assert.deepEqual(s.layout, { tabBarVisible: true, addressBarPosition: 'top', addressBarVisible: true, showTitleBar: false, statusBar: 'overlay', uiDensity: 'regular', compactMenus: false, uiZoom: 100, saved: '' });
  assert.deepEqual([...logic.TAB_LAYOUTS], ['horizontal', 'tabs-top', 'single-row', 'vertical', 'vertical-right', 'bottom']);
  for (const layout of ['vertical-right', 'bottom']) assert.ok(logic.parseSettingsSnapshot({ ...d, tabLayout: layout }), layout);
  const bad = (layout) => ({ ...d, layout: { ...d.layout, ...layout } });
  for (const broken of [
    bad({ tabBarVisible: 'yes' }), bad({ addressBarPosition: 'middle' }), bad({ statusBar: 'show' }), bad({ uiDensity: 'dense' }), bad({ compactMenus: 1 }),
    bad({ uiZoom: 49 }), bad({ uiZoom: 201 }), bad({ uiZoom: 100.5 }), bad({ saved: 'tabs=sideways' }), bad({ saved: 'x'.repeat(161) }), bad({ extra: true }),
    { ...d, layout: { tabBarVisible: true } }, { ...d, layout: null },
  ]) assert.equal(logic.parseSettingsSnapshot(broken), null, JSON.stringify(broken.layout));
  const { layout: _dropped, ...without } = d;
  assert.equal(logic.parseSettingsSnapshot(without), null, 'the layout object is required');
  const draft = logic.draftOf(s);
  assert.deepEqual(draft.layout, s.layout);
  assert.notEqual(draft.layout, s.layout, 'the draft owns a copy');
  const text = logic.saveText({ ...draft, tabLayout: 'bottom', sidePanelPosition: 'right', layout: { ...draft.layout, tabBarVisible: false, addressBarPosition: 'bottom', showTitleBar: true, statusBar: 'hidden', uiDensity: 'compact', compactMenus: true, uiZoom: 150, saved: layoutLogic.layoutConfigKey(layoutLogic.presetConfig('bottom')) } });
  assert.match(text, /^tab_layout=bottom\n/);
  assert.match(text, /\nside_panel_position=right\n/);
  assert.ok(text.endsWith('\ntab_bar_visible=false\naddress_bar_position=bottom\naddress_bar_visible=true\nshow_title_bar=true\nstatus_bar=hidden\nui_density=compact\ncompact_menus=true\nui_zoom=150\nlayout_saved=tabs=bottom,tabbar=true,address=top,addressbar=true,panel=left,status=overlay,title=false'), text);
  for (const broken of [{ uiZoom: 30 }, { statusBar: 'show' }, { addressBarPosition: 'left' }, { saved: 'nope' }]) {
    assert.equal(logic.saveText({ ...draft, layout: { ...draft.layout, ...broken } }), null, JSON.stringify(broken));
  }
  // The side panel's edge stays the older key; the row that carried it alone is gone.
  assert.equal(logic.ROWS.find((row) => row.id === 'sidePanelPosition'), undefined);
  for (const id of ['layoutPreset', 'layoutBars']) assert.equal(logic.rowTier(logic.ROWS.find((row) => row.id === id)), 'core', id);
  for (const id of ['layout', 'sidebarWidth', 'tabBarVisible', 'addressBarVisible', 'showTitleBar', 'statusBar', 'uiDensity', 'compactMenus', 'uiZoom']) {
    assert.equal(logic.rowTier(logic.ROWS.find((row) => row.id === id)), 'more', id);
  }
});

test('layout: presets choose a configuration, the edges derive from the tab layout, and the user’s own arrangement is kept as Saved', () => {
  const L = layoutLogic;
  assert.deepEqual([...L.PRESETS], ['simple', 'classic', 'vertical-left', 'vertical-right', 'bottom']);
  assert.equal(L.presetConfig('simple').tabLayout, 'single-row');
  assert.equal(L.presetConfig('simple').statusBar, 'hidden');
  assert.equal(L.presetConfig('classic').tabLayout, 'tabs-top');
  assert.equal(L.presetConfig('vertical-left').sidePanelPosition, 'left');
  assert.equal(L.presetConfig('vertical-right').tabLayout, 'vertical-right');
  assert.equal(L.presetConfig('vertical-right').sidePanelPosition, 'right');
  assert.equal(L.presetConfig('bottom').tabLayout, 'bottom');
  for (const preset of L.PRESETS) {
    const key = L.layoutConfigKey(L.presetConfig(preset));
    assert.deepEqual(L.parseLayoutConfig(key), L.presetConfig(preset), preset);
    assert.equal(L.matchingPreset(L.presetConfig(preset)), preset);
    assert.ok(L.isLayoutSaved(key));
  }
  assert.equal(L.layoutConfigKey(L.DEFAULT_LAYOUT_CONFIG), 'tabs=horizontal,tabbar=true,address=top,addressbar=true,panel=left,status=overlay,title=false', 'the same text Rust keeps');
  assert.equal(L.matchingPreset(L.DEFAULT_LAYOUT_CONFIG), null, 'toolbar above tabs is Bowser’s own default, not a preset');
  for (const bad of ['', 'tabs=horizontal', 'tabs=horizontal,tabbar=true,address=top,addressbar=true,panel=left,status=overlay,title=false,title=true', 'TABS=horizontal,tabbar=true,address=top,addressbar=true,panel=left,status=overlay,title=false']) {
    assert.equal(L.parseLayoutConfig(bad), null, bad);
  }
  assert.ok(L.isLayoutSaved(''), 'empty means none');
  // Edges.
  assert.deepEqual(['horizontal', 'tabs-top', 'single-row', 'vertical', 'vertical-right', 'bottom'].map(L.tabBarEdge), ['top', 'top', 'top', 'left', 'right', 'bottom']);
  assert.equal(L.tabLayoutForEdge('top', 'single-row'), 'single-row', 'a top edge keeps the top style');
  assert.equal(L.tabLayoutForEdge('top', 'vertical'), 'horizontal', 'from a side, the first top style');
  assert.equal(L.tabLayoutForEdge('left', 'horizontal'), 'vertical');
  assert.equal(L.tabLayoutForEdge('right', 'horizontal'), 'vertical-right');
  assert.equal(L.tabLayoutForEdge('bottom', 'tabs-top'), 'bottom');
  // Saved: a change away from every preset is remembered; a preset pick leaves it alone.
  const draft = logic.draftOf(logic.parseSettingsSnapshot(fixture.snap.d));
  assert.equal(L.selectedTile(draft), null);
  L.applyLayoutConfig(draft, L.presetConfig('vertical-right'));
  assert.equal(draft.tabLayout, 'vertical-right');
  assert.equal(draft.sidePanelPosition, 'right');
  assert.equal(L.selectedTile(draft), 'vertical-right');
  L.rememberOwnLayout(draft);
  assert.equal(draft.layout.saved, '', 'a preset is not Saved');
  draft.sidePanelPosition = 'left';
  L.rememberOwnLayout(draft);
  assert.equal(draft.layout.saved, 'tabs=vertical-right,tabbar=true,address=top,addressbar=true,panel=left,status=overlay,title=false');
  assert.equal(L.selectedTile(draft), 'saved');
  L.applyLayoutConfig(draft, L.presetConfig('classic'));
  assert.equal(L.selectedTile(draft), 'classic');
  assert.equal(draft.layout.saved, 'tabs=vertical-right,tabbar=true,address=top,addressbar=true,panel=left,status=overlay,title=false', 'kept while a preset is tried');
  L.applyLayoutConfig(draft, L.parseLayoutConfig(draft.layout.saved));
  assert.equal(L.selectedTile(draft), 'saved', 'the Saved tile brings it back');
  // UI zoom typed values.
  assert.equal(L.parseUiZoom('125'), 125);
  assert.equal(L.parseUiZoom(' 125% '), 125);
  assert.equal(L.parseUiZoom('123'), 125, 'rounded to the slider’s step');
  assert.equal(L.parseUiZoom('50'), 50);
  assert.equal(L.parseUiZoom('200'), 200);
  for (const bad of ['', '49', '201', 'big', '100.5', '-100']) assert.equal(L.parseUiZoom(bad), null, bad);
  assert.equal(L.uiZoomText(110), '110%');
});

test('layout: the page shows the preset thumbnails and the three edges in view; each pick saves the whole draft', () => {
  deliver(fixture.snap);
  searchFor('');
  const presets = app.querySelector('.sp-tiles.is-presets');
  const tiles = presets.querySelectorAll('.sp-tile');
  assert.deepEqual(tiles.map((tile) => tile.dataset.preset), ['simple', 'classic', 'vertical-left', 'vertical-right', 'bottom', 'saved']);
  assert.deepEqual(tiles.map((tile) => tile.querySelector('.sp-tile-name').textContent), ['Simple', 'Classic', 'Vertical left', 'Vertical right', 'Bottom', 'Saved']);
  assert.ok(tiles.every((tile) => tile.querySelector('svg')?.getAttribute('class') === 'sp-art'), 'every tile has a thumbnail in the layout-tile style');
  assert.ok(tiles.every((tile) => tile.getAttribute('aria-checked') === 'false'), 'toolbar above tabs is no preset');
  const saved = tiles[5];
  assert.equal(saved.disabled, true, 'nothing saved yet');
  assert.equal(app.querySelectorAll('.sp-tile').length, 9, 'six presets plus the three top styles');
  // Both core rows sit in Appearance's main view, not behind More options.
  for (const id of ['layoutPreset', 'layoutBars']) {
    assert.ok(areaNode('appearance').contains(rowNode(id)), id);
    assert.ok(!moreOf('appearance').contains(rowNode(id)), id);
  }
  for (const id of ['layout', 'sidebarWidth', 'tabBarVisible', 'addressBarVisible', 'showTitleBar', 'statusBar', 'uiDensity', 'compactMenus', 'uiZoom']) {
    assert.ok(moreOf('appearance').contains(rowNode(id)), `${id} is behind More options`);
  }
  // A preset: every key of its configuration in one save.
  sent.length = 0;
  tiles[4].click();
  assert.equal(sent.length, 1);
  assert.match(sent[0].text, /^tab_layout=bottom\n/);
  assert.match(sent[0].text, /\nside_panel_position=left\n/);
  assert.match(sent[0].text, /\nstatus_bar=overlay\n/);
  assert.match(sent[0].text, /\nlayout_saved=$/, 'a preset is not Saved');
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, tabLayout: 'bottom', ack: 'saved' } });
  assert.equal(tiles[4].getAttribute('aria-checked'), 'true');
  sent.length = 0;
  tiles[0].click();
  assert.match(sent[0].text, /^tab_layout=single-row\n/);
  assert.match(sent[0].text, /\nstatus_bar=hidden\n/, 'Simple hides the link overlay');
  // The edges: tab bar right makes a right sidebar; with the panel still left that is the user's own arrangement, kept as Saved.
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, ack: 'saved' } });
  const bars = rowNode('layoutBars');
  const groups = bars.querySelectorAll('[role="radiogroup"]');
  assert.deepEqual(groups.map((g) => g.getAttribute('aria-label')), ['Tab bar', 'Address bar', 'Panels']);
  assert.deepEqual(groups[0].querySelectorAll('[role="radio"]').map((b) => b.textContent), ['Top', 'Left', 'Right', 'Bottom']);
  assert.equal(groups[0].querySelector('[aria-checked="true"]').textContent, 'Top');
  assert.equal(groups[2].querySelector('[aria-checked="true"]').textContent, 'Left');
  sent.length = 0;
  groups[0].querySelectorAll('[role="radio"]')[2].click();
  assert.match(sent[0].text, /^tab_layout=vertical-right\n/);
  const own = 'tabs=vertical-right,tabbar=true,address=top,addressbar=true,panel=left,status=overlay,title=false';
  assert.match(sent[0].text, new RegExp(`\\nlayout_saved=${own.replace(/[-,=]/g, '\\$&')}$`));
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, tabLayout: 'vertical-right', layout: { ...fixture.snap.d.layout, saved: own }, ack: 'saved' } });
  assert.equal(saved.disabled, false, 'the Saved tile has something to bring back');
  assert.equal(saved.getAttribute('aria-checked'), 'true');
  assert.equal(groups[0].querySelector('[aria-checked="true"]').textContent, 'Right');
  assert.equal(rowNode('layout').querySelector('[aria-checked="true"]'), null, 'no top style while the tabs are on a side');
  // Address bar bottom, panels right: one key each.
  sent.length = 0;
  groups[1].querySelectorAll('[role="radio"]')[1].click();
  assert.match(sent[0].text, /\naddress_bar_position=bottom\n/);
  groups[2].querySelectorAll('[role="radio"]')[1].click();
  assert.match(sent[1].text, /\nside_panel_position=right\n/);
  // Back to a preset through the Saved tile and the top-style tiles.
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, layout: { ...fixture.snap.d.layout, saved: own }, ack: 'saved' } });
  sent.length = 0;
  saved.click();
  assert.match(sent[0].text, /^tab_layout=vertical-right\n/);
  rowNode('layout').querySelectorAll('.sp-tile')[1].click();
  assert.match(sent[1].text, /^tab_layout=tabs-top\n/);
});

test('layout: the rows behind More options each save their key; the UI zoom takes the slider, a typed value and Reset', () => {
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, ack: 'saved' } });
  const sw = (id) => rowNode(id).querySelector('[role="switch"]');
  const seg = (id, index) => rowNode(id).querySelectorAll('[role="radio"]')[index];
  const expectSave = (line) => { assert.equal(sent.length, 1); assert.equal(sent[0].a, 'save'); assert.match(sent[0].text, line); sent.length = 0; deliver({ ...fixture.snap, d: { ...fixture.snap.d, ack: 'saved' } }); };
  sent.length = 0;
  sw('tabBarVisible').click();
  expectSave(/\ntab_bar_visible=false\n/);
  sw('addressBarVisible').click();
  expectSave(/\naddress_bar_visible=false\n/);
  sw('showTitleBar').click();
  expectSave(/\nshow_title_bar=true\n/);
  assert.deepEqual(rowNode('statusBar').querySelectorAll('[role="radio"]').map((b) => b.textContent), ['Info overlay only', 'Hidden']);
  seg('statusBar', 1).click();
  expectSave(/\nstatus_bar=hidden\n/);
  seg('uiDensity', 1).click();
  expectSave(/\nui_density=compact\n/);
  sw('compactMenus').click();
  expectSave(/\ncompact_menus=true\n/);
  // A hidden tab bar is the user's own arrangement: Saved follows.
  sw('tabBarVisible').click();
  assert.match(sent[0].text, /\nlayout_saved=tabs=horizontal,tabbar=false,/);
  sent.length = 0;
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, ack: 'saved' } });
  // UI zoom.
  const zoom = rowNode('uiZoom');
  const range = zoom.querySelector('.sp-range');
  const value = zoom.querySelector('.sp-ui-zoom-value');
  const reset = zoom.querySelector('.bw-button');
  assert.equal(range.min, '50');
  assert.equal(range.max, '200');
  assert.equal(range.value, '100');
  assert.equal(value.value, '100%');
  assert.equal(reset.disabled, true, 'nothing to reset at 100%');
  range.value = '150';
  range.dispatch('input');
  assert.equal(value.value, '150%', 'the value follows the slider as it moves');
  assert.equal(sent.length, 0, 'nothing is saved until the slider is let go');
  range.dispatch('change');
  expectSave(/\nui_zoom=150\n/);
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, layout: { ...fixture.snap.d.layout, uiZoom: 150 }, ack: 'saved' } });
  assert.equal(reset.disabled, false);
  value.value = '123%';
  value.dispatch('change');
  assert.equal(value.value, '125%', 'typed values round to the step');
  expectSave(/\nui_zoom=125\n/);
  value.value = 'lots';
  value.dispatch('change');
  assert.equal(sent.length, 0, 'a bad value is not saved');
  assert.match(app.querySelector('.sp-toast').textContent, /Enter a zoom from 50 to 200 percent/);
  assert.equal(value.value, '100%', 'the field goes back to the saved value');
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, layout: { ...fixture.snap.d.layout, uiZoom: 150 }, ack: 'saved' } });
  reset.click();
  expectSave(/\nui_zoom=100\n/);
  // An unreadable settings file locks the block too.
  deliver({ ...fixture.snap, d: { ...fixture.snap.d, readable: false } });
  for (const control of [...rowNode('layoutPreset').querySelectorAll('.sp-tile'), ...rowNode('layoutBars').querySelectorAll('[role="radio"]'), sw('tabBarVisible'), range, value]) assert.equal(control.disabled, true);
});

test('layout: the view applies the density and compact menus to the root for the row-height tokens', () => {
  const stateParse = parseInbound(JSON.stringify({ t: 'state', scale: 1, solid: true, dark: true, back: false, forward: false, loading: false, progress: 0, security: 'none', label: '', density: 'compact', compactMenus: true, theme: {} }));
  assert.equal(stateParse.density, 'compact');
  assert.equal(stateParse.compactMenus, true);
  assert.equal(parseInbound(JSON.stringify({ t: 'state', scale: 1, solid: true, dark: true, back: false, forward: false, loading: false, progress: 0, security: 'none', label: '', density: 'dense', theme: {} })), null);
  assert.equal(parseInbound(JSON.stringify({ t: 'state', scale: 1, solid: true, dark: true, back: false, forward: false, loading: false, progress: 0, security: 'none', label: '', compactMenus: 'yes', theme: {} })), null);
  const tokens = read(join(srcDir, 'styles', 'tokens.css'));
  assert.match(tokens, /:root\[data-density="compact"\]\s*\{[^}]*--bw-row-h: 28px;[^}]*--bw-side-row-h: 22px;/);
  assert.match(tokens, /:root\[data-compact-menus="true"\]\s*\{[^}]*--bw-menu-row-h: 24px;/);
  assert.match(read(join(srcDir, 'styles', 'components.css')), /\.bw-row \{[^}]*min-height: var\(--bw-row-h\)/);
  assert.match(read(join(srcDir, 'styles', 'menu.css')), /--menu-item-h: var\(--bw-menu-row-h\)/);
  assert.match(read(join(srcDir, 'styles', 'sidebar.css')), /height: var\(--bw-side-row-h\)/);
  assert.match(read(join(srcDir, 'view.ts')), /root\.dataset\.density = density/);
});