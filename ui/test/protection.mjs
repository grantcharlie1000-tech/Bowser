// The Privacy section's blocking, cookie-notice, history and website-appearance controls
// (panels/protection.ts, panels/protection-logic.ts): the snapshot contract against the Rust
// fixture, the exact commands, the words, and the controls in a fake document (every control sends
// the right command, nothing is sent that Rust would refuse, saved sign-ins are never deleted
// without the second yes, an unreadable settings file disables the controls).
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
const fixture = JSON.parse(read(join(here, 'fixtures', 'protection-snapshot.json')));
async function bundle(file) {
  const out = esbuild.buildSync({ entryPoints: [join(srcDir, file)], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
}
const logic = await bundle('panels/protection-logic.ts');
const protocol = await bundle('protocol.ts');

// ---- contract ----

test('the Rust protection snapshot passes the strict contract', () => {
  const inbound = protocol.parseInbound(JSON.stringify(fixture));
  assert.equal(inbound?.t, 'snap');
  assert.equal(inbound.p, 'protection');
  const snap = logic.parseProtectionSnapshot(inbound.d);
  assert.ok(snap, 'the fixture must parse');
  assert.equal(snap.level, 'trackers');
  assert.equal(snap.cookieNotices, true);
  assert.equal(snap.retention, '3m');
  assert.equal(snap.websiteAppearance, 'dark');
  assert.equal(snap.forceDark, true);
  assert.equal(snap.privateTheme, 'follow');
  assert.deepEqual(snap.current, { host: 'example.com', level: 'ads', cookies: false, private: false });
  assert.deepEqual(snap.sites, [{ site: 'a.test', level: 'none', cookies: 'default' }, { site: 'b.test', level: 'default', cookies: 'hide' }]);
  assert.deepEqual(snap.allow, ['allowed.test']);
  assert.deepEqual(snap.sources.map((s) => [s.id, s.kind, s.origin, s.removable, s.enabled]), [
    ['bowser-trackers', 'trackers', 'builtin', false, true], ['u1', 'cookies', 'url', true, false], ['u2', 'ads', 'file', true, true],
  ]);
  assert.equal(snap.sources[1].name, 'My "list"', 'text arrives as text');
  assert.deepEqual(snap.blocked, { page: 3, session: 40 });
  assert.deepEqual(snap.result, { seq: 2, ok: true, text: 'Added.' });
});

test('anything else is refused: extra or missing keys, unknown values, contradictions', () => {
  const d = fixture.d;
  assert.ok(logic.parseProtectionSnapshot({ ...d, current: null, result: null, sites: [], allow: [], sources: [] }), 'the quiet snapshot is fine');
  const refused = [
    { ...d, extra: 1 },
    { ...d, level: 'everything' },
    { ...d, retention: '2w' },
    { ...d, websiteAppearance: 'sepia' },
    { ...d, privateTheme: 'blue' },
    { ...d, forceDark: 'yes' },
    { ...d, busy: 1 },
    { ...d, current: { ...d.current, host: 'https://example.com' } },
    { ...d, current: { ...d.current, extra: 1 } },
    { ...d, sites: [{ site: 'a.test', level: 'none' }] },
    { ...d, sites: [{ ...d.sites[0], level: 'ads ' }] },
    { ...d, sites: [d.sites[0], d.sites[0]] },
    { ...d, sites: [{ ...d.sites[0], site: 'a b.test' }] },
    { ...d, sites: Array.from({ length: logic.MAX_SITES_SHOWN + 1 }, (_, n) => ({ site: `s${n}.test`, level: 'none', cookies: 'default' })) },
    { ...d, allow: ['ok.test', 'has space.test'] },
    { ...d, allow: Array.from({ length: logic.MAX_ALLOW_SHOWN + 1 }, (_, n) => `a${n}.test`) },
    { ...d, sources: [{ ...d.sources[1], url: '' }] },
    { ...d, sources: [{ ...d.sources[0], url: 'https://x.example/a.txt' }] },
    { ...d, sources: [{ ...d.sources[1], url: 'http://lists.example/x.txt' }] },
    { ...d, sources: [{ ...d.sources[0], id: '../x' }] },
    { ...d, sources: [{ ...d.sources[0], rules: -1 }] },
    { ...d, sources: [d.sources[0], d.sources[0]] },
    { ...d, sources: [{ ...d.sources[0], kind: 'ads2' }] },
    { ...d, sources: [{ ...d.sources[0], name: 'bad\nname' }] },
    { ...d, blocked: { page: 1 } },
    { ...d, blocked: { page: -1, session: 0 } },
    { ...d, result: { seq: 1, ok: true } },
    { ...d, result: { seq: 1, ok: 'yes', text: 'x' } },
    { ...d, result: { seq: 1, ok: true, text: 'x'.repeat(logic.MAX_RESULT * 2 + 1) } },
  ];
  for (const bad of refused) assert.equal(logic.parseProtectionSnapshot(bad), null, JSON.stringify(bad).slice(0, 120));
  const { busy, ...withoutBusy } = d;
  assert.equal(logic.parseProtectionSnapshot(withoutBusy), null, 'busy is required');
});

test('commands carry exactly their own fields; the Rust spellings are the ones the page uses', () => {
  const ok = (a, f, expected) => assert.deepEqual(logic.protectionCommand(a, f), expected ?? { a, ...f }, a);
  const no = (a, f) => assert.equal(logic.protectionCommand(a, f), null, `${a} ${JSON.stringify(f)}`);
  ok('blocklevel', { text: 'trackers' });
  ok('blocklevel', { text: 'none' });
  no('blocklevel', { text: 'default' });
  no('blocklevel', {});
  no('blocklevel', { text: 'ads', on: true });
  ok('cookienotices', { on: false });
  no('cookienotices', { text: 'on' });
  ok('retention', { text: 'forever' });
  no('retention', { text: '2w' });
  ok('webappearance', { text: 'auto' });
  no('webappearance', { text: 'sepia' });
  ok('forcedark', { on: true });
  ok('privatetheme', { text: 'follow' });
  no('privatetheme', { text: 'blue' });
  ok('site-level', { text: 'example.com', n: 3 });
  no('site-level', { text: 'example.com', n: 4 });
  no('site-level', { text: 'localhost', n: 0 });
  no('site-level', { text: 'https://example.com', n: 0 });
  no('site-level', { text: 'a b.com', n: 0 });
  no('site-level', { text: 'example.com' });
  ok('site-cookies', { text: 'example.com', n: 2 });
  no('site-cookies', { text: 'example.com', n: 3 });
  ok('site-remove', { text: 'example.com' });
  no('site-remove', { text: '' });
  ok('src-toggle', { text: 'bowser-ads', on: false });
  no('src-toggle', { text: '../x', on: false });
  no('src-toggle', { text: 'u1' });
  ok('src-add-url', { text: 'cookies', url: 'https://lists.example/c.txt' });
  no('src-add-url', { text: 'cookies', url: '   ' });
  no('src-add-url', { text: 'mystery', url: 'https://lists.example/c.txt' });
  no('src-add-url', { text: 'ads', url: 'https://x.example/' + 'a'.repeat(logic.MAX_URL) });
  ok('src-add-file', { text: 'trackers' });
  no('src-add-file', { text: 'trackers', url: 'C:\\list.txt' });
  ok('src-update', { text: 'u3' });
  ok('src-remove', { text: 'u3' });
  no('src-remove', { text: 'U3' });
  no('anything', {});
  // Mirrors chrome_host/panels/protection.rs: the wire spellings Rust maps.
  const rust = read(join(root, 'src', 'chrome_host', 'panels', 'protection.rs'));
  for (const action of ['blocklevel', 'cookienotices', 'site-level', 'site-cookies', 'site-remove', 'src-toggle', 'src-add-url', 'src-add-file', 'src-update', 'src-remove', 'retention', 'webappearance', 'forcedark', 'privatetheme', 'delete-data']) {
    assert.ok(rust.includes(`"${action}"`), action);
  }
  assert.ok(rust.includes(`MAX_SITES_SHOWN: usize = ${logic.MAX_SITES_SHOWN}`));
  assert.ok(rust.includes(`MAX_ALLOW_SHOWN: usize = ${logic.MAX_ALLOW_SHOWN}`));
});

test('delete-data names kinds and a period by the numbers Rust uses, and saved sign-ins only with their confirmation', () => {
  const bits = logic.kindsBits(['history', 'cookies', 'storage', 'cache']);
  assert.equal(bits, 15);
  assert.deepEqual(logic.protectionCommand('delete-data', { id: bits, n: 1, on: false }), { a: 'delete-data', id: 15, n: 1, on: false });
  const withLogins = logic.kindsBits(['history', 'logins']);
  assert.equal(withLogins, 1 | 16);
  assert.equal(logic.protectionCommand('delete-data', { id: withLogins, n: 4, on: false }), null, 'sign-ins without the confirmation');
  assert.equal(logic.protectionCommand('delete-data', { id: bits, n: 4, on: true }), null, 'the confirmation without sign-ins');
  assert.deepEqual(logic.protectionCommand('delete-data', { id: withLogins, n: 4, on: true }), { a: 'delete-data', id: withLogins, n: 4, on: true });
  for (const bad of [{ id: 0, n: 0, on: false }, { id: 128, n: 0, on: false }, { id: 1, n: 5, on: false }, { id: 1, n: -1, on: false }, { id: 1.5, n: 0, on: false }, { id: 1, n: 0 }, { n: 0, on: false }]) {
    assert.equal(logic.protectionCommand('delete-data', bad), null, JSON.stringify(bad));
  }
  // Mirrors src/browsing_data.rs: the bit order is DataKind::ALL, the periods are Range::ALL.
  const rust = read(join(root, 'src', 'browsing_data.rs'));
  const order = /pub const ALL: \[DataKind; 7\] = \[([^\]]*)\]/.exec(rust)?.[1].match(/Self::(\w+)/g).map((m) => m.slice(6));
  assert.deepEqual(order, ['History', 'Cookies', 'Storage', 'Cache', 'Passwords', 'Autofill', 'SiteSettings']);
  assert.deepEqual(logic.DELETE_KINDS.map((k) => k.key), ['history', 'cookies', 'storage', 'cache', 'logins', 'autofill', 'sitesettings']);
  assert.deepEqual(/pub const ALL: \[Range; 5\] = \[([^\]]*)\]/.exec(rust)?.[1].match(/Self::(\w+)/g).map((m) => m.slice(6)), ['Hour', 'Day', 'Week', 'FourWeeks', 'Everything']);
  assert.deepEqual(logic.DELETE_RANGES.map(([, label]) => label), ['Past hour', 'Past day', 'Past week', 'Past 4 weeks', 'Everything']);
});

test('the closed value sets match the Rust settings', () => {
  const rust = read(join(root, 'src', 'settings_privacy.rs'));
  for (const key of [...logic.RETENTIONS, ...logic.APPEARANCES, ...logic.PRIVATE_THEMES, 'trackers', 'ads']) assert.ok(rust.includes(`"${key}"`), key);
  const blocker = read(join(root, 'src', 'blocker.rs'));
  for (const key of logic.LEVELS) assert.ok(blocker.includes(`"${key}"`), key);
  assert.deepEqual([...logic.RETENTIONS], ['session', '1d', '1w', '1m', '3m', '6m', '1y', 'forever']);
  assert.equal(logic.RETENTION_LABELS['3m'], '3 months');
  assert.equal(logic.RETENTION_LABELS.session, 'Session');
});

// ---- words ----

test('the words say what will happen', () => {
  assert.equal(logic.deleteSummary([], '0'), 'Choose what to delete');
  assert.equal(logic.deleteSummary(['history'], '0'), 'Delete history from the past hour');
  assert.equal(logic.deleteSummary(['history', 'cookies'], '3'), 'Delete history and cookies from the past 4 weeks');
  assert.equal(logic.deleteSummary(['cache', 'cookies', 'autofill'], '4'), 'Delete cookies, cache and autofill');
  assert.equal(logic.blockedText(1, 2500), '1 blocked on this page · 2,500 this session');
  assert.equal(logic.levelDescription('none', true), 'Nothing is blocked. Cookie notices are still hidden.');
  assert.equal(logic.levelDescription('none', false), logic.LEVEL_HINTS.none);
  assert.equal(logic.levelDescription('ads', true), logic.LEVEL_HINTS.ads);
  const [builtin, url, file] = logic.parseProtectionSnapshot(fixture.d).sources;
  assert.equal(logic.sourceDetail(builtin), 'Trackers · Built in · 412 rules');
  assert.equal(logic.sourceDetail(url), 'Cookie notices · lists.example · Off');
  assert.equal(logic.sourceDetail(file), 'Ads · From a file · 3 rules');
  assert.equal(logic.rulesText({ ...file, rules: 1 }), '1 rule');
  assert.equal(logic.siteFromInput('  https://WWW.Example.org/path?x=1 '), 'example.org');
  assert.equal(logic.siteFromInput('news.example.co.uk:8080'), 'news.example.co.uk');
  for (const bad of ['', 'localhost', 'not a site', 'a,b.com', 'javascript:alert(1)']) assert.equal(logic.siteFromInput(bad), null, bad);
  assert.equal(logic.listUrlProblem('https://easylist.to/easylist/easylist.txt'), null);
  for (const [bad, why] of [['', /Enter the address/], ['http://x.example/l.txt', /https/], ['ftp://x.example', /https/], ['https://user:pw@x.example/l', /user name/], ['https://x.example:8443/l', /port/], ['https://localhost/l', /public/], ['https://127.0.0.1/l', /public/], ['https://[::1]/l', /public/]]) {
    assert.match(logic.listUrlProblem(bad) ?? '', why, bad);
  }
  assert.match(logic.listUrlProblem(`https://x.example/${'a'.repeat(logic.MAX_URL)}`) ?? '', /too long/);
  // Every sentence the page can show: no claim of automatic updates, and no word for the store the chrome must not name.
  const words = [
    ...Object.values(logic.LEVEL_HINTS), ...Object.values(logic.LEVEL_LABELS), ...Object.values(logic.SITE_LEVEL_LABELS), ...Object.values(logic.RETENTION_LABELS),
    ...logic.DELETE_KINDS.flatMap((k) => [k.label, k.hint]),
  ].join('\n');
  assert.doesNotMatch(words, /password|vault|credential/i);
});

// ---- a small fake DOM ----

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
    this.ownText = text; this.id = ''; this.type = ''; this.offsetTop = 0; this.offsetHeight = 34; this.offsetWidth = 0; this.selectionStart = 0; this.selectionEnd = 0;
    const props = new Map();
    this.style = { setProperty: (k, v) => props.set(k, v), getPropertyValue: (k) => props.get(k) ?? '' };
  }
  get textContent() { return this.ownText ?? this.children.map((c) => c.textContent).join(''); }
  set textContent(v) { for (const c of this.children) c.parent = null; this.children = []; this.ownText = null; if (v !== '') this.append(new Node('#text', String(v))); }
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
  dispatch(type, event = {}) { for (const cb of this.listeners[type] ?? []) cb({ target: this, preventDefault() {}, stopPropagation() {}, key: '', ...event }); }
  click() { if (!this.disabled) this.dispatch('click'); }
  detach(node) { if (node.parent) node.parent.children = node.parent.children.filter((c) => c !== node); node.parent = this; }
  append(...nodes) { for (const n of nodes) { const node = typeof n === 'string' ? new Node('#text', n) : n; this.ownText = null; this.detach(node); this.children.push(node); } }
  replaceWith(node) { const p = this.parent; const at = p.children.indexOf(this); p.detach(node); p.children.splice(at, 1, node); this.parent = null; }
  replaceChildren(...nodes) { for (const c of this.children) c.parent = null; this.children = []; this.ownText = null; this.append(...nodes); }
  remove() { if (this.parent) this.parent.children = this.parent.children.filter((c) => c !== this); this.parent = null; }
  all() { return this.children.flatMap((c) => [c, ...c.all()]); }
  matches(s) { return sel(s).some((x) => matchOne(this, x)); }
  querySelectorAll(s) { return this.all().filter((n) => n.tagName !== '#text' && n.matches(s)); }
  querySelector(s) { return this.querySelectorAll(s)[0] ?? null; }
  focus() { globalThis.document.activeElement = this; }
  scrollIntoView() {}
  setSelectionRange() {}
  contains(n) { return n === this || this.all().includes(n); }
  getBoundingClientRect() { return { left: 0, top: 0, right: 0, bottom: 0, width: 0, height: 0 }; }
}
const sent = [];
const listeners = [];
globalThis.document = {
  documentElement: new Node('html'), body: new Node('body'), activeElement: null,
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
  chrome: { webview: { postMessage: (wire) => sent.push(JSON.parse(wire)), addEventListener: (_t, cb) => listeners.push(cb) } },
});
globalThis.Element = Node;
globalThis.HTMLElement = Node;
globalThis.HTMLInputElement = class {};
const { mountProtection } = await bundle('panels/protection.ts');

// ---- the controls ----

const toasts = [];
const levels = [];
const protection = mountProtection({ toast: (text, ok) => toasts.push({ text, ok }), onLevel: (level) => levels.push(level) });
const deliver = (d, p = 'protection') => protection.receive({ t: 'snap', p, d });
const snap = (changes = {}) => ({ ...fixture.d, ...changes });
const last = () => sent.at(-1);
const button = (root, label) => root.querySelectorAll('button').find((b) => b.textContent === label);
const rowOf = (root, label) => root.querySelectorAll('.pg-row').find((row) => row.querySelector('.pg-label')?.textContent === label);
const radios = (control) => control.root.querySelectorAll('[role="radio"]');
const checked = (control) => radios(control).filter((b) => b.getAttribute('aria-checked') === 'true').map((b) => b.textContent);

test('before Bowser answers the blocks exist, with their headings, and nothing is sent', () => {
  assert.equal(sent.length, 0);
  for (const [block, title] of [[protection.sites, 'Site rules'], [protection.lists, 'Blocking lists'], [protection.deleteData, 'Clear data']]) {
    assert.equal(block.querySelector('.pg-heading').textContent, title);
  }
  assert.equal(protection.sections().length, 6, 'blocking, sites, lists, history, delete, appearance');
  deliver({ nonsense: true });
  assert.deepEqual(toasts.at(-1), { text: 'Privacy settings could not be loaded. The response was invalid.', ok: false });
  toasts.length = 0;
  protection.receive({ t: 'state' });
  protection.receive({ t: 'snap', p: 'privacy', d: {} });
  assert.equal(toasts.length, 0, 'other messages are none of its business');
});

test('the single controls show the snapshot and each sends its own command', () => {
  deliver(snap({ result: null }));
  assert.deepEqual(checked(protection.level), ['Trackers']);
  assert.equal(protection.cookieNotices.root.getAttribute('aria-checked'), 'true');
  assert.equal(protection.retention.root.querySelector('.bw-select-value').textContent, '3 months');
  assert.deepEqual(checked(protection.websiteAppearance), ['Dark']);
  assert.equal(protection.forceDark.root.getAttribute('aria-checked'), 'true');
  assert.deepEqual(checked(protection.privateTheme), ['Match']);
  assert.deepEqual(radios(protection.level).map((b) => b.textContent), ['None', 'Trackers', 'Ads']);

  sent.length = 0;
  radios(protection.level)[0].click();
  assert.deepEqual(sent, [{ v: 1, t: 'priv', a: 'blocklevel', text: 'none' }]);
  assert.deepEqual(levels, ['none'], 'the settings page keeps its own draft in step');
  radios(protection.level)[2].click();
  assert.deepEqual(last(), { v: 1, t: 'priv', a: 'blocklevel', text: 'ads' });
  protection.cookieNotices.root.click();
  assert.deepEqual(last(), { v: 1, t: 'priv', a: 'cookienotices', on: false });
  protection.forceDark.root.click();
  assert.deepEqual(last(), { v: 1, t: 'priv', a: 'forcedark', on: false });
  radios(protection.websiteAppearance)[1].click();
  assert.deepEqual(last(), { v: 1, t: 'priv', a: 'webappearance', text: 'light' });
  radios(protection.privateTheme)[2].click();
  assert.deepEqual(last(), { v: 1, t: 'priv', a: 'privatetheme', text: 'dark' });
  const select = protection.retention.root.querySelector('.bw-select-button');
  select.click();
  const options = protection.retention.root.querySelectorAll('[role="option"]');
  assert.deepEqual(options.map((o) => o.querySelector('.bw-menu-label').textContent), ['Session', '1 day', '1 week', '1 month', '3 months', '6 months', '1 year', 'Forever']);
  options[2].click();
  assert.deepEqual(last(), { v: 1, t: 'priv', a: 'retention', text: '1w' });
  assert.equal(sent.length, 7, 'one command per change, nothing else');

  // The next snapshot is the truth: it moves every control.
  deliver(snap({ level: 'none', cookieNotices: false, retention: 'forever', websiteAppearance: 'auto', forceDark: false, privateTheme: 'light', result: null }));
  assert.deepEqual(checked(protection.level), ['None']);
  assert.equal(protection.cookieNotices.root.getAttribute('aria-checked'), 'false');
  assert.equal(protection.retention.root.querySelector('.bw-select-value').textContent, 'Forever');
  assert.deepEqual(checked(protection.websiteAppearance), ['Auto']);
  assert.equal(protection.forceDark.root.getAttribute('aria-checked'), 'false');
  assert.deepEqual(checked(protection.privateTheme), ['Light']);
  deliver(snap({ result: null }));
});

test('an unreadable settings file disables every control and the Delete button', () => {
  deliver(snap({ readable: false, result: null }));
  assert.equal(radios(protection.level).every((b) => b.disabled), true);
  assert.equal(protection.cookieNotices.root.disabled, true);
  assert.equal(protection.forceDark.root.disabled, true);
  assert.equal(protection.retention.root.querySelector('.bw-select-button').disabled, true);
  assert.equal(radios(protection.websiteAppearance).every((b) => b.disabled), true);
  assert.equal(button(protection.deleteData, 'Delete').disabled, true);
  assert.equal(button(protection.sites, 'Add').disabled, true);
  assert.equal(button(protection.lists, 'Add').disabled, true);
  assert.equal(protection.lists.querySelectorAll('[role="switch"]').every((sw) => sw.disabled), true);
  deliver(snap({ result: null }));
  assert.equal(protection.cookieNotices.root.disabled, false);
  assert.equal(radios(protection.level).some((b) => b.disabled), false);
});

test('sites: saved choices are listed with their level and cookie choice; each change is one command', () => {
  const rows = protection.sites.querySelectorAll('.pg-row');
  assert.deepEqual(rows.map((r) => r.querySelector('.pg-label').textContent).slice(0, 5), ['Add a site', 'This site', 'a.test', 'b.test', 'Skipped in settings']);
  assert.equal(rowOf(protection.sites, 'Skipped in settings').querySelector('.pg-desc').textContent, 'allowed.test');
  const a = rowOf(protection.sites, 'a.test');
  const [levelPick, cookiePick] = a.querySelectorAll('.bw-select-button');
  assert.equal(levelPick.querySelector('.bw-select-value').textContent, 'None');
  assert.equal(cookiePick.querySelector('.bw-select-value').textContent, 'Default');
  assert.equal(rowOf(protection.sites, 'b.test').querySelectorAll('.bw-select-button')[1].querySelector('.bw-select-value').textContent, 'Hide');
  sent.length = 0;
  levelPick.click();
  const choose = (label) => a.querySelectorAll('[role="option"]').find((o) => o.querySelector('.bw-menu-label').textContent === label).click();
  choose('Ads');
  assert.deepEqual(sent, [{ v: 1, t: 'priv', a: 'site-level', text: 'a.test', n: 2 }]);
  cookiePick.click();
  choose('Show');
  assert.deepEqual(last(), { v: 1, t: 'priv', a: 'site-cookies', text: 'a.test', n: 0 });
  a.querySelectorAll('button').find((b) => b.getAttribute('aria-label') === 'Forget the choices for a.test').click();
  assert.deepEqual(last(), { v: 1, t: 'priv', a: 'site-remove', text: 'a.test' });
  // The site you are on can be offered to the field.
  button(protection.sites, 'Use example.com').click();
  assert.equal(protection.sites.querySelector('input').value, 'example.com');
});

test('sites: a typed site is cleaned up first, and text that is not a site sends nothing', () => {
  sent.length = 0;
  const type = (text) => { const input = protection.sites.querySelector('input'); input.value = text; input.dispatch('input'); };
  type('not a site');
  button(protection.sites, 'Add').click();
  assert.equal(sent.length, 0);
  assert.equal(protection.sites.querySelector('.pg-protect-error').textContent, 'Enter a site.');
  assert.equal(protection.sites.querySelector('.pg-protect-error').getAttribute('role'), 'alert');
  type('https://www.Shop.Example.org/cart');
  assert.equal(protection.sites.querySelector('.pg-protect-error'), null, 'typing clears the problem');
  button(protection.sites, 'Add').click();
  assert.deepEqual(sent, [{ v: 1, t: 'priv', a: 'site-level', text: 'shop.example.org', n: 0 }]);
  assert.equal(protection.sites.querySelector('input').value, '', 'the field is empty again');
  type('other.test');
  protection.sites.querySelector('input').dispatch('keydown', { key: 'Enter' });
  assert.equal(last().text, 'other.test', 'Enter adds too');
});

test('lists: every source is a row; switching, updating and removing send their own commands', () => {
  const [trackers, url, file] = ['Bowser tracker list', 'My "list"', 'my-filters.txt'].map((name) => rowOf(protection.lists, name));
  assert.equal(trackers.querySelector('.pg-desc').textContent, 'Trackers · Built in · 412 rules');
  assert.equal(url.querySelector('.pg-desc').textContent, 'Cookie notices · lists.example · Off');
  assert.equal(url.classList.contains('is-off'), true, 'a list that is off reads as off');
  // Built-in lists can be switched but never removed; only a list from an address can be updated.
  assert.deepEqual(trackers.querySelectorAll('button').map((b) => b.getAttribute('aria-label')), ['Use Bowser tracker list']);
  assert.deepEqual(url.querySelectorAll('button').map((b) => b.getAttribute('aria-label')), ['Fetch My "list" again', 'Remove My "list"', 'Use My "list"']);
  assert.deepEqual(file.querySelectorAll('button').map((b) => b.getAttribute('aria-label')), ['Remove my-filters.txt', 'Use my-filters.txt']);
  sent.length = 0;
  trackers.querySelector('[role="switch"]').click();
  url.querySelector('[role="switch"]').click();
  url.querySelectorAll('button').find((b) => b.textContent === 'Update').click();
  file.querySelectorAll('button').find((b) => b.getAttribute('aria-label') === 'Remove my-filters.txt').click();
  assert.deepEqual(sent, [
    { v: 1, t: 'priv', a: 'src-toggle', text: 'bowser-trackers', on: false },
    { v: 1, t: 'priv', a: 'src-toggle', text: 'u1', on: true },
    { v: 1, t: 'priv', a: 'src-update', text: 'u1' },
    { v: 1, t: 'priv', a: 'src-remove', text: 'u2' },
  ]);
  deliver(snap({ result: null }));
});

test('lists: an address is checked before it is sent; a file is chosen in Bowser, never named by the page', () => {
  sent.length = 0;
  const type = (text) => { const input = protection.lists.querySelector('input'); input.value = text; input.dispatch('input'); };
  for (const [bad, why] of [['', /Enter the address/], ['http://lists.example/x.txt', /https/], ['https://localhost/x', /public/], ['https://u:p@lists.example/x', /user name/]]) {
    type(bad);
    button(protection.lists, 'Add').click();
    assert.equal(sent.length, 0, bad);
    assert.match(protection.lists.querySelector('.pg-protect-error').textContent, why, bad);
  }
  type('https://lists.example/cookies.txt');
  const kinds = protection.lists.querySelectorAll('.bw-select-button')[0];
  kinds.click();
  protection.lists.querySelectorAll('[role="option"]').find((o) => o.querySelector('.bw-menu-label').textContent === 'Cookie notices').click();
  type('https://lists.example/cookies.txt');
  button(protection.lists, 'Add').click();
  assert.deepEqual(sent, [{ v: 1, t: 'priv', a: 'src-add-url', text: 'cookies', url: 'https://lists.example/cookies.txt' }]);
  assert.equal(protection.lists.querySelector('input').value, '');
  button(protection.lists, 'File').click();
  assert.deepEqual(last(), { v: 1, t: 'priv', a: 'src-add-file', text: 'cookies' }, 'a kind and nothing that names a file');
  // While a fetch runs the add controls wait.
  deliver(snap({ busy: true, result: null }));
  assert.equal(button(protection.lists, 'Fetching…').disabled, true);
  assert.equal(button(protection.lists, 'File').disabled, true);
  assert.equal(protection.lists.querySelector('input').disabled, true);
  assert.equal(button(protection.lists, 'Update').disabled, true);
  deliver(snap({ result: null }));
  assert.equal(button(protection.lists, 'Add').disabled, false);
});

test('Delete browsing data: nothing is sent until the second click, and the summary says exactly what it will do', () => {
  const root = protection.deleteData;
  const run = () => button(root, 'Delete') ?? button(root, 'Cannot be undone');
  assert.equal(rowOf(root, 'Delete').querySelector('.pg-desc'), null);
  assert.equal(button(root, 'Delete').getAttribute('aria-label'), 'Delete history, cookies, storage and cache from the past day');
  sent.length = 0;
  button(root, 'Delete').click();
  assert.equal(sent.length, 0, 'the first click only asks');
  assert.ok(button(root, 'Cannot be undone'), 'the warning is the confirming button');
  assert.equal(button(root, 'Cannot be undone').classList.contains('is-danger'), true);
  assert.equal(button(root, 'Cancel').hidden, false);
  assert.equal(rowOf(root, 'Delete').querySelector('.pg-desc'), null);
  button(root, 'Cancel').click();
  assert.equal(sent.length, 0);
  assert.ok(button(root, 'Delete'), 'cancel puts it back');
  button(root, 'Delete').click();
  button(root, 'Cannot be undone').click();
  assert.deepEqual(sent, [{ v: 1, t: 'priv', a: 'delete-data', id: 15, n: 1, on: false }]);
  assert.ok(button(root, 'Delete'), 'and it is Delete again afterwards');
});

test('Delete browsing data: changing the period or a kind withdraws the question; choosing nothing disables the button', () => {
  const root = protection.deleteData;
  const switchOf = (label) => root.querySelectorAll('.pg-row').find((row) => row.querySelector('.pg-label').textContent === label).querySelector('[role="switch"]');
  assert.deepEqual(root.querySelectorAll('.pg-row').map((r) => r.querySelector('.pg-label').textContent).slice(1, 8), ['History', 'Cookies', 'Storage', 'Cache', 'Sign-ins', 'Autofill', 'Site settings']);
  sent.length = 0;
  button(root, 'Delete').click();
  radios({ root }).find((b) => b.textContent === 'Past week').click();
  assert.ok(button(root, 'Delete'), 'the period changed, so the question is withdrawn');
  button(root, 'Delete').click();
  switchOf('Cookies').click();
  assert.ok(button(root, 'Delete'), 'a kind changed, so the question is withdrawn');
  assert.deepEqual([...root.querySelectorAll('[role="switch"]')].map((sw) => sw.getAttribute('aria-checked')), ['true', 'false', 'true', 'true', 'false', 'false', 'false']);
  button(root, 'Delete').click();
  button(root, 'Cannot be undone').click();
  assert.deepEqual(sent, [{ v: 1, t: 'priv', a: 'delete-data', id: 1 | 4 | 8, n: 2, on: false }], 'history, storage and cache for the past week');
  for (const label of ['History', 'Storage', 'Cache']) switchOf(label).click();
  assert.equal(button(root, 'Delete').disabled, true, 'nothing chosen');
  assert.equal(button(root, 'Delete').getAttribute('aria-label'), 'Choose what to delete');
  for (const label of ['History', 'Cookies', 'Storage', 'Cache']) switchOf(label).click();
  assert.equal(button(root, 'Delete').disabled, false);
  radios({ root }).find((b) => b.textContent === 'Past day').click();
});

test('Delete browsing data: saved sign-ins need their own yes beside the confirmation, and the flag goes out with them only', () => {
  const root = protection.deleteData;
  const switchOf = (label) => root.querySelectorAll('.pg-row').find((row) => row.querySelector('.pg-label').textContent === label).querySelector('[role="switch"]');
  assert.equal(rowOf(root, 'Confirm'), undefined, 'no second yes until sign-ins are chosen');
  switchOf('Sign-ins').click();
  const sure = rowOf(root, 'Confirm');
  assert.ok(sure, 'the second yes appears');
  assert.equal(sure.querySelector('[role="switch"]').getAttribute('aria-checked'), 'false');
  assert.equal(button(root, 'Delete').disabled, true, 'the button waits for it');
  sent.length = 0;
  sure.querySelector('[role="switch"]').click();
  assert.equal(button(root, 'Delete').disabled, false);
  button(root, 'Delete').click();
  assert.equal(sent.length, 0);
  button(root, 'Cannot be undone').click();
  const bits = 1 | 2 | 4 | 8 | 16;
  assert.deepEqual(sent, [{ v: 1, t: 'priv', a: 'delete-data', id: bits, n: 1, on: true }]);
  // The yes is spent: asking again needs it again.
  assert.equal(button(root, 'Delete').disabled, true);
  assert.equal(rowOf(root, 'Confirm').querySelector('[role="switch"]').getAttribute('aria-checked'), 'false');
  // Untoggling sign-ins removes the second yes and withdraws it.
  rowOf(root, 'Confirm').querySelector('[role="switch"]').click();
  switchOf('Sign-ins').click();
  assert.equal(rowOf(root, 'Confirm'), undefined);
  assert.equal(button(root, 'Delete').disabled, false);
  button(root, 'Delete').click();
  button(root, 'Cannot be undone').click();
  assert.deepEqual(last(), { v: 1, t: 'priv', a: 'delete-data', id: 15, n: 1, on: false }, 'without sign-ins no flag goes out');
});

test('Delete browsing data: a guest has no saved sign-ins to delete', () => {
  deliver(snap({ guest: true, result: null }));
  const root = protection.deleteData;
  const row = rowOf(root, 'Sign-ins');
  assert.equal(row.querySelector('.pg-desc'), null);
  assert.equal(row.querySelector('[role="switch"]').disabled, true);
  assert.equal(row.querySelector('[role="switch"]').getAttribute('aria-checked'), 'false');
  deliver(snap({ result: null }));
});

test('a result is shown once when it is new, and never for one that was there when the page opened', () => {
  toasts.length = 0;
  deliver(snap({ result: { seq: 7, ok: true, text: 'Deleted browsing history from the past hour.' } }));
  assert.deepEqual(toasts, [{ text: 'Deleted browsing history from the past hour.', ok: true }]);
  deliver(snap({ result: { seq: 7, ok: true, text: 'Deleted browsing history from the past hour.' } }));
  assert.equal(toasts.length, 1, 'the same result again is not a new one');
  deliver(snap({ result: { seq: 8, ok: false, text: 'That list has no rules Bowser can use.' } }));
  assert.deepEqual(toasts.at(-1), { text: 'That list has no rules Bowser can use.', ok: false });
  deliver(snap({ result: null }));
  assert.equal(toasts.length, 2);
  // A fresh page that opens on an old result stays quiet.
  const fresh = [];
  const other = mountProtection({ toast: (text) => fresh.push(text) });
  other.receive({ t: 'snap', p: 'protection', d: snap({ result: { seq: 3, ok: true, text: 'Added.' } }) });
  assert.equal(fresh.length, 0);
  other.receive({ t: 'snap', p: 'protection', d: snap({ result: { seq: 4, ok: true, text: 'Deleted cookies.' } }) });
  assert.deepEqual(fresh, ['Deleted cookies.']);
});

test('the keyboard stays on the control that was used when a snapshot redraws a block', () => {
  deliver(snap({ result: null }));
  const input = protection.sites.querySelector('input');
  input.focus();
  assert.equal(globalThis.document.activeElement, input);
  deliver(snap({ result: null }));
  const again = protection.sites.querySelector('input');
  assert.equal(globalThis.document.activeElement, again, 'the new field has the keyboard');
  assert.equal(again.dataset.focus, 'site-input');
  const toggle = protection.lists.querySelectorAll('[role="switch"]')[0];
  toggle.focus();
  toggle.click();
  deliver(snap({ result: null }));
  assert.equal(globalThis.document.activeElement.dataset.focus, 'src:bowser-trackers');
});
