// Extensions (docs/extensions.md "Managing extensions"): the snapshot and toolbar-button contracts
// against the Rust fixture, the words (no store, no enforcement claims), the commands, the settings
// section in a fake document (loading, empty, error, list, toggling, confirming a removal, removing)
// and the toolbar button (hidden until an enabled extension has a popup, named after it, its slot).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, '..', 'src');
const read = (path) => readFileSync(path, 'utf8').replace(/\r\n/g, '\n');
const fixture = JSON.parse(read(join(here, 'fixtures', 'extensions-messages.json')));
async function bundle(file) {
  const out = esbuild.buildSync({ entryPoints: [join(srcDir, file)], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
}
const logic = await bundle('extensions-logic.ts');
const protocol = await bundle('protocol.ts');
const ID = 'ext-0123456789abcdef';
const OTHER = 'ext-fedcba9876543210';

// ---- contract ----

test('every snapshot Rust writes passes the strict contract', () => {
  for (const key of ['snap', 'busy', 'empty', 'error']) {
    const inbound = protocol.parseInbound(JSON.stringify(fixture[key]));
    assert.equal(inbound?.t, 'snap', key);
    assert.equal(inbound.p, 'extensions', key);
    assert.ok(logic.parseExtensionsSnapshot(inbound.d), key);
  }
  const snap = logic.parseExtensionsSnapshot(fixture.snap.d);
  assert.equal(snap.items.length, 2);
  const [reader, notes] = snap.items;
  assert.equal(reader.name, 'Reader "<b>"', 'text arrives as text');
  assert.equal(reader.all, true);
  assert.deepEqual(reader.declared.scripts, { items: ['<all_urls>'], total: 1 });
  assert.deepEqual(reader.declared.optional, { items: ['https://news.example/*', 'downloads'], total: 2 });
  assert.equal(notes.enabled, false);
  assert.equal(notes.failure, 'Load failed: 0x80070002');
  assert.equal(logic.parseExtensionsSnapshot(fixture.busy.d).busy, ID);
  assert.equal(logic.parseExtensionsSnapshot(fixture.empty.d).ack, 'removed');
  assert.equal(logic.parseExtensionsSnapshot(fixture.error.d).state, 'error');
});

test('anything else is refused: extra or missing keys, bad ids, padded lists, contradictions', () => {
  const d = fixture.snap.d;
  const item = d.items[0];
  const withItem = (patch) => ({ ...d, items: [{ ...item, ...patch }] });
  const bad = [
    { ...d, extra: 1 },
    { ...d, state: 'off' },
    { ...d, ack: 'granted' },
    { ...d, busy: 'ext-0123' },
    { ...d, busy: '..\\ext-0123456789abcdef' },
    { ...d, error: 'line\nbreak' },
    { ...d, items: [item, item] },
    { ...d, state: 'error', error: '' },
    { ...d, state: 'error', error: 'x' },
    { ...d, items: Array.from({ length: 65 }, (_, i) => ({ ...item, id: `ext-${i.toString(16).padStart(16, '0')}` })) },
    withItem({ id: 'EXT-0123456789ABCDEF' }),
    withItem({ id: 'chrome-extension://abc' }),
    withItem({ name: '' }),
    withItem({ name: 'x'.repeat(129) }),
    withItem({ name: 'a\u0007b' }),
    withItem({ enabled: 'yes' }),
    withItem({ extra: true }),
    withItem({ perms: ['a', 'b'], permsN: 1 }),
    withItem({ perms: ['a'], permsN: 3 }),
    withItem({ perms: Array(9).fill('p'), permsN: 9 }),
    withItem({ perms: [7], permsN: 1 }),
    withItem({ perms: ['x'.repeat(65)], permsN: 1 }),
    withItem({ readable: false }),
  ];
  for (const value of bad) assert.equal(logic.parseExtensionsSnapshot(value), null, JSON.stringify(value).slice(0, 160));
  const unsent = { ...item };
  delete unsent.optionalN;
  assert.equal(logic.parseExtensionsSnapshot({ ...d, items: [unsent] }), null, 'every key is required');
  const full = withItem({ perms: Array(8).fill('p'), permsN: 200 });
  assert.equal(logic.parseExtensionsSnapshot(full).items[0].declared.perms.total, 200, 'eight shown, the rest counted');
});

test('the toolbar button message: shown exactly when there is a popup, named only when there is one', () => {
  assert.deepEqual(protocol.parseInbound(JSON.stringify(fixture.buttonOff)), { t: 'extbtn', on: false, n: 0, name: '' });
  assert.deepEqual(protocol.parseInbound(JSON.stringify(fixture.buttonOne)), { t: 'extbtn', on: true, n: 1, name: 'Reader "<b>"' });
  assert.deepEqual(protocol.parseInbound(JSON.stringify(fixture.buttonMany)), { t: 'extbtn', on: true, n: 2, name: '' });
  for (const raw of [
    { v: 1, t: 'extbtn', on: true, n: 0, name: '' },
    { v: 1, t: 'extbtn', on: false, n: 1, name: 'A' },
    { v: 1, t: 'extbtn', on: true, n: 1, name: '' },
    { v: 1, t: 'extbtn', on: true, n: 2, name: 'A' },
    { v: 1, t: 'extbtn', on: true, n: 65, name: '' },
    { v: 1, t: 'extbtn', on: true, n: 1, name: 'x'.repeat(129) },
    { v: 1, t: 'extbtn', on: true, n: 1 },
    { v: 1, t: 'extbtn', on: true, n: 1, name: 'A', badge: '3' },
    { v: 2, t: 'extbtn', on: false, n: 0, name: '' },
  ]) assert.equal(protocol.parseInbound(JSON.stringify(raw)), null, JSON.stringify(raw));
  assert.deepEqual(logic.extButtonText({ n: 1, name: 'Reader' }), { label: 'Open Reader', tip: 'Reader', menu: false });
  assert.deepEqual(logic.extButtonText({ n: 3, name: '' }), { label: 'Extensions', tip: 'Extensions (3)', menu: true });
});

// ---- words and commands ----

test('the words say what Bowser does and nothing it does not', () => {
  const [reader, notes] = logic.parseExtensionsSnapshot(fixture.snap.d).items;
  assert.equal(logic.statusText(reader, false), 'Version 1.2.3 · On');
  assert.equal(logic.statusText(notes, false), 'Version 0.4 · Off');
  assert.equal(logic.statusText(reader, true), 'Version 1.2.3 · Removing…');
  assert.equal(logic.statusText({ ...reader, failure: 'Load failed: 0x80070002' }, false), 'Version 1.2.3 · On · not running', 'on but failed to load is not plain On');
  assert.equal(logic.statusText({ ...notes, failure: 'Load failed: 0x80070002' }, false), 'Version 0.4 · Off', 'off stays Off whatever the last failure was');
  assert.equal(logic.statusText({ ...reader, failure: 'x' }, true), 'Version 1.2.3 · Removing…');
  assert.equal(logic.reachText(reader), 'Can read and change every website you visit.');
  assert.equal(logic.reachText(notes), 'Declares no site access. 1 permission.');
  const narrow = { ...reader, all: false, declared: { ...reader.declared, scripts: { items: [], total: 0 }, sites: { items: ['https://a.example/*', 'https://b.example/*'], total: 2 } } };
  assert.equal(logic.reachText(narrow), 'Can read and change pages that match 2 patterns.');
  const bare = { ...notes, declared: { sites: { items: [], total: 0 }, scripts: { items: [], total: 0 }, perms: { items: [], total: 0 }, optional: { items: [], total: 0 } } };
  assert.equal(logic.reachText(bare), 'Declares no site access and no permissions.');
  assert.match(logic.reachText({ ...bare, readable: false }), /could not read its manifest/);
  assert.deepEqual(logic.declarationGroups(notes).map((g) => g.key), ['perms'], 'empty lists are left out');
  assert.deepEqual(logic.declarationGroups(reader).map((g) => [g.label, g.items.length, g.more]), [['Site access', 1, 0], ['Runs scripts on', 1, 0], ['Permissions', 2, 0], ['Optional', 2, 0]]);
  assert.equal(logic.ackToast('none', ''), null);
  assert.deepEqual(logic.ackToast('removed', ''), { text: 'Extension removed', ok: true });
  assert.deepEqual(logic.ackToast('failed', 'That extension is not installed.'), { text: 'That extension is not installed.', ok: false });
  assert.match(logic.removeQuestion(reader), /^Remove Reader "<b>"\? Bowser unloads it and deletes its copied files\./);
  // Every sentence the section can show, in one place: no store, no enforcement, no badges.
  const words = [
    logic.EXTENSIONS_NOTE, logic.removeQuestion(reader), logic.reachText(reader), logic.reachText(notes), logic.reachText(narrow), logic.reachText(bare),
    ...logic.EXTENSION_ACKS.map((ack) => logic.ackToast(ack, '')?.text ?? ''),
    ...Object.values(logic.DECLARATION_LABELS),
  ].join('\n');
  assert.doesNotMatch(words, /web store|chrome store|one-click/i);
  assert.doesNotMatch(words, /\benforc/i);
  assert.doesNotMatch(words, /\bbadge/i);
  assert.match(logic.EXTENSIONS_NOTE, /cannot narrow them/);
  assert.equal(logic.EXTENSIONS_NOTE, 'Assume an extension that is on has everything it declares. Bowser shows these but cannot narrow them.');
  assert.doesNotMatch(logic.EXTENSIONS_NOTE, /WebView2|optional/i, 'what the runtime grants is not stated, so it is not claimed');
});

test('commands carry one well-formed id and nothing else; install names no path', () => {
  assert.deepEqual(logic.setEnabledCommand(ID, false), { a: 'extension', text: ID, on: false });
  assert.deepEqual(logic.removeCommand(ID), { a: 'extremove', text: ID });
  assert.deepEqual(logic.popupCommand(ID), { a: 'extpopup', text: ID });
  assert.deepEqual(logic.INSTALL_COMMAND, { a: 'extinstall' });
  assert.deepEqual(logic.INSTALL_CRX_COMMAND, { a: 'extinstallcrx' }, 'the file is chosen in Bowser’s picker, never named by the page');
  for (const bad of ['', 'ext-0123', '../ext-0123456789abcdef', 'ext-0123456789ABCDEF', 'C:\\Extensions']) {
    assert.equal(logic.setEnabledCommand(bad, true), null, bad);
    assert.equal(logic.removeCommand(bad), null, bad);
    assert.equal(logic.popupCommand(bad), null, bad);
  }
  // Mirrors chrome_host/extensions_page.rs: the wire spellings Rust maps.
  const rust = read(join(here, '..', '..', 'src', 'chrome_host', 'extensions_page.rs'));
  for (const action of ['extension', 'extremove', 'extpopup', 'extinstall', 'extinstallcrx']) assert.ok(rust.includes(`("${action}",`), action);
});

test('the CRX install words say it is a file on disk and never a store install', () => {
  assert.equal(logic.INSTALL_CRX_LABEL, 'Install CRX file…');
  assert.equal(logic.INSTALL_CRX_TIP, 'Choose a .crx file on this computer. Bowser checks its signature, then asks before copying anything.');
  const store = /\bstore\b|chrome web|web store|add to chrome|one-click|from the web|download/i;
  for (const text of [logic.INSTALL_CRX_LABEL, logic.INSTALL_CRX_TIP, logic.EMPTY_DETAIL]) assert.doesNotMatch(text, store, text);
  assert.match(logic.INSTALL_CRX_LABEL, /\bfile\b/);
  assert.match(logic.EMPTY_DETAIL, /\.crx file you already have on this computer/);
});

// ---- a small fake DOM ----

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
  return not.every((inner) => !matchOne(node, inner));
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
    this.classList = new ClassList(this); this.dataset = {}; this.hidden = false; this.disabled = false; this.tabIndex = 0;
    this.ownText = text; this.id = ''; this.type = ''; this.title = ''; this.dir = ''; this.offsetWidth = 28;
    const props = new Map();
    this.style = { setProperty: (k, v) => props.set(k, String(v)), getPropertyValue: (k) => props.get(k) ?? '', removeProperty: (k) => props.delete(k) };
  }
  get children() { return this.childList.filter((c) => c.tagName !== '#text'); }
  get textContent() { return this.ownText ?? this.childList.map((c) => c.textContent).join(''); }
  set textContent(v) { this.childList = []; this.ownText = null; if (v !== '') this.append(new Node('#text', String(v))); }
  get firstChild() { return this.childList[0] ?? null; }
  get lastChild() { return this.childList[this.childList.length - 1] ?? null; }
  get nextSibling() { const p = this.parent; return p ? p.childList[p.childList.indexOf(this) + 1] ?? null : null; }
  setAttribute(k, v) { this.attrs.set(k, String(v)); if (k === 'id') this.id = String(v); }
  getAttribute(k) {
    if (k === 'id') return this.id || null;
    if (k.startsWith('data-')) { const v = this.dataset[k.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase())]; if (v !== undefined) return v; }
    return this.attrs.has(k) ? this.attrs.get(k) : null;
  }
  hasAttribute(k) { return this.getAttribute(k) !== null; }
  removeAttribute(k) { this.attrs.delete(k); }
  toggleAttribute(k, force) { const on = force ?? !this.attrs.has(k); if (on) this.attrs.set(k, ''); else this.attrs.delete(k); return on; }
  addEventListener(type, cb) { (this.listeners[type] ??= []).push(cb); }
  removeEventListener() {}
  dispatch(type, init = {}) {
    let stopped = false;
    const event = { type, target: this, key: '', preventDefault() {}, stopPropagation() { stopped = true; }, ...init };
    for (let node = this; node && !stopped; node = node.parent) for (const cb of node.listeners[type] ?? []) cb(event);
    return event;
  }
  click() { if (!this.disabled) this.dispatch('click'); }
  detach(node) { if (node.parent) node.parent.childList = node.parent.childList.filter((c) => c !== node); node.parent = this; }
  append(...nodes) { for (const n of nodes) { const node = typeof n === 'string' ? new Node('#text', n) : n; this.ownText = null; this.detach(node); this.childList.push(node); } }
  after(node) { const p = this.parent; p.detach(node); p.childList.splice(p.childList.indexOf(this) + 1, 0, node); }
  replaceChildren(...nodes) { for (const c of this.childList) c.parent = null; this.childList = []; this.ownText = null; this.append(...nodes); }
  remove() { if (this.parent) this.parent.childList = this.parent.childList.filter((c) => c !== this); this.parent = null; }
  all() { return this.childList.flatMap((c) => [c, ...c.all()]); }
  matches(s) { return s.split(',').some((x) => matchOne(this, x.trim())); }
  querySelectorAll(s) { return this.all().filter((n) => n.tagName !== '#text' && n.matches(s)); }
  querySelector(s) { return this.querySelectorAll(s)[0] ?? null; }
  focus() { if (!this.disabled) globalThis.document.activeElement = this; }
  contains(n) { return n === this || this.all().includes(n); }
  getBoundingClientRect() { return { left: 768, top: 1, right: 796, bottom: 29, width: 28, height: 28 }; }
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
let timers = [];
let timerId = 0;
Object.assign(globalThis, {
  window: globalThis,
  matchMedia: () => ({ matches: false }),
  requestAnimationFrame: (cb) => { cb(); return 1; },
  addEventListener() {}, removeEventListener() {},
  chrome: { webview: { postMessage: (wire) => sent.push(JSON.parse(wire)), addEventListener: (_t, cb) => listeners.push(cb) } },
  setTimeout: (cb, ms = 0) => { timers.push({ cb, ms, id: ++timerId }); return timerId; },
  clearTimeout: (id) => { timers = timers.filter((t) => t.id !== id); },
});
const flush = () => { const run = timers; timers = []; for (const t of run) t.cb(); };
globalThis.Element = Node;
globalThis.HTMLElement = Node;
globalThis.HTMLButtonElement = Node;
globalThis.HTMLInputElement = class {};

// ---- the settings section ----

const { mountExtensionsSection } = await bundle('extensions-section.ts');
const toasts = [];
const section = mountExtensionsSection({ send: (fields) => protocol.sendPanel('set', fields), toast: (text, ok) => toasts.push({ text, ok }) });
const host = new Node('section');
host.append(section.root);
const receive = (message) => section.receive(protocol.parseInbound(JSON.stringify(message)));
const card = () => section.root.querySelector('.sp-ext-card');
const rowOf = (id) => section.root.querySelector(`.sp-ext-item[data-id="${id}"]`);
const button = (root, text) => root.querySelectorAll('button').find((b) => b.textContent === text || b.getAttribute('aria-label') === text);
const visibleText = () => section.root.all().filter((n) => n.tagName === '#text' && !n.parent?.hidden).map((n) => n.textContent).join('\n');

test('section: loading, then the list; a labelled card, a row per extension, the honest note', () => {
  assert.equal(card().getAttribute('aria-busy'), 'true');
  assert.equal(card().querySelector('.sp-ext-state.is-loading').getAttribute('role'), 'status');
  assert.equal(section.root.querySelector('.sp-ext-install').hidden, true, 'nothing to add to yet');
  assert.equal(section.root.querySelector('.sp-ext-install-crx').hidden, true, 'the CRX button hides with its neighbour');
  receive(fixture.snap);
  assert.equal(card().getAttribute('aria-busy'), 'false');
  assert.equal(card().getAttribute('aria-labelledby'), 'sp-ext-h');
  assert.equal(section.root.querySelectorAll('.sp-ext-item').length, 2);
  const reader = rowOf(ID);
  assert.equal(reader.querySelector('.sp-ext-name').textContent, 'Reader "<b>"');
  assert.equal(reader.querySelector('.sp-ext-status').textContent, 'Version 1.2.3 · On');
  assert.ok(reader.querySelector('.sp-ext-reach').classList.contains('is-broad'));
  const toggle = reader.querySelector('[role="switch"]');
  assert.equal(toggle.getAttribute('aria-checked'), 'true');
  assert.equal(toggle.getAttribute('aria-label'), 'Reader "<b>" on');
  assert.equal(toggle.getAttribute('aria-describedby'), `sp-ext-r-${ID}`);
  assert.ok(button(reader, 'Open Reader "<b>" popup'), 'an enabled extension with a popup offers it');
  const notes = rowOf(OTHER);
  assert.ok(notes.classList.contains('is-off'));
  assert.equal(button(notes, 'Open Tab Notes popup'), undefined, 'no popup button when it declares none or is off');
  assert.equal(notes.querySelector('.sp-ext-failure').textContent, 'Load failed: 0x80070002');
  assert.equal(section.root.querySelector('.sp-ext-install').hidden, false);
  assert.match(section.root.querySelector('.sp-ext-note').textContent, /cannot narrow them/);
  assert.doesNotMatch(visibleText(), /web store|\benforc|badge/i);
});

test('section: Install CRX file… sits beside Install unpacked… with the same weight, and they hide together', () => {
  const head = section.root.querySelector('.sp-ext-head');
  const unpacked = button(head, 'Install unpacked…');
  const crx = button(head, 'Install CRX file…');
  assert.ok(unpacked && crx);
  assert.equal(crx.parent, unpacked.parent, 'side by side');
  assert.equal(crx.className.replace('sp-ext-install-crx', '').trim(), unpacked.className.replace('sp-ext-install', '').trim(), 'same visual weight');
  assert.equal(crx.hidden, unpacked.hidden);
  assert.equal(crx.getAttribute('aria-describedby'), 'sp-ext-note');
  assert.doesNotMatch(head.textContent, /\bstore\b|chrome web|add to chrome|one-click/i);
});

test('section: the switch sends its id and waits for the answer; the answer is a toast', () => {
  sent.length = 0;
  const toggle = rowOf(ID).querySelector('[role="switch"]');
  toggle.focus();
  toggle.click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'extension', text: ID, on: false }]);
  assert.equal(toggle.getAttribute('aria-disabled'), 'true', 'held until Bowser answers');
  assert.equal(toggle.disabled, false, 'without dropping the keyboard');
  toggle.click();
  assert.equal(sent.length, 1, 'one command per answer');
  receive({ ...fixture.snap, d: { ...fixture.snap.d, ack: 'disabled', items: [{ ...fixture.snap.d.items[0], enabled: false }, fixture.snap.d.items[1]] } });
  assert.deepEqual(toasts.at(-1), { text: 'Extension turned off', ok: true });
  const after = rowOf(ID).querySelector('[role="switch"]');
  assert.equal(after.getAttribute('aria-disabled'), 'false');
  assert.equal(after.getAttribute('aria-checked'), 'false');
  assert.equal(globalThis.document.activeElement, after, 'the keyboard stays on the switch across the redraw');
  receive(fixture.snap);
});

test('section: declarations open under the row, capped, with how many were left out', () => {
  const reader = rowOf(ID);
  const more = reader.querySelector('.sp-ext-toggle-details');
  assert.equal(more.getAttribute('aria-expanded'), 'false');
  assert.equal(reader.querySelector('.sp-ext-details'), null);
  more.click();
  const open = rowOf(ID);
  assert.equal(open.querySelector('.sp-ext-toggle-details').getAttribute('aria-expanded'), 'true');
  const details = open.querySelector('.sp-ext-details');
  assert.equal(open.querySelector('.sp-ext-toggle-details').getAttribute('aria-controls'), details.id);
  assert.deepEqual(details.querySelectorAll('dt').map((n) => n.textContent), ['Site access', 'Runs scripts on', 'Permissions', 'Optional']);
  assert.deepEqual(details.querySelectorAll('.sp-ext-chip').map((n) => n.textContent), ['https://example.com/*', '<all_urls>', 'storage', 'tabs', 'https://news.example/*', 'downloads']);
  const item = fixture.snap.d.items[0];
  receive({ ...fixture.snap, d: { ...fixture.snap.d, items: [{ ...item, perms: Array.from({ length: 8 }, (_, i) => `perm${i}`), permsN: 30 }, fixture.snap.d.items[1]] } });
  assert.equal(rowOf(ID).querySelector('.sp-ext-more').textContent, 'and 22 more', 'stays open across a snapshot');
  rowOf(ID).querySelector('.sp-ext-toggle-details').click();
  assert.equal(rowOf(ID).querySelector('.sp-ext-details'), null);
  receive(fixture.snap);
});

test('section: remove asks in the row first; Cancel and Escape back out; Remove sends once', () => {
  sent.length = 0;
  button(rowOf(ID), 'Remove Reader "<b>"').click();
  assert.deepEqual(sent, [], 'nothing is sent before the question is answered');
  let ask = rowOf(ID).querySelector('.sp-ext-confirm');
  assert.equal(ask.getAttribute('aria-label'), 'Confirm removing Reader "<b>"');
  assert.match(ask.textContent, /Bowser unloads it and deletes its copied files/);
  assert.equal(globalThis.document.activeElement?.textContent, 'Cancel', 'the safe answer has the keyboard');
  button(ask, 'Cancel').click();
  assert.equal(rowOf(ID).querySelector('.sp-ext-confirm'), null);
  button(rowOf(ID), 'Remove Reader "<b>"').click();
  ask = rowOf(ID).querySelector('.sp-ext-confirm');
  ask.dispatch('keydown', { key: 'Escape' });
  assert.equal(rowOf(ID).querySelector('.sp-ext-confirm'), null);
  button(rowOf(ID), 'Remove Reader "<b>"').click();
  ask = rowOf(ID).querySelector('.sp-ext-confirm');
  const confirm = ask.querySelectorAll('button').find((b) => b.classList.contains('is-danger'));
  assert.ok(confirm, 'the confirming button is the danger one');
  confirm.click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'extremove', text: ID }]);
  assert.equal(rowOf(ID).querySelector('.sp-ext-confirm'), null);
});

test('section: while one is removed it says so, holds every other action, then the list is empty', () => {
  receive(fixture.busy);
  const row = rowOf(ID);
  assert.ok(row.classList.contains('is-busy'));
  assert.equal(row.querySelector('.sp-ext-status').textContent, 'Version 1.2.3 · Removing…');
  assert.ok(row.querySelector('.bw-spinner'));
  assert.equal(row.querySelector('[role="switch"]'), null);
  receive({ ...fixture.busy, d: { ...fixture.busy.d, items: fixture.snap.d.items } });
  const other = rowOf(OTHER);
  assert.equal(other.querySelector('[role="switch"]').disabled, true, 'one removal at a time');
  assert.equal(other.querySelector('.sp-ext-remove').disabled, true);
  receive(fixture.empty);
  assert.deepEqual(toasts.at(-1), { text: 'Extension removed', ok: true });
  const empty = card().querySelector('.sp-ext-state.is-empty');
  assert.ok(empty);
  assert.match(empty.textContent, /No extensions installed/);
  assert.match(empty.textContent, /manifest\.json/);
  assert.equal(section.root.querySelector('.sp-ext-install').hidden, true, 'the empty state has its own button');
});

test('section: install goes through Bowser’s picker; the button waits for the answer', () => {
  sent.length = 0;
  const choose = button(card(), 'Choose folder…');
  const chooseCrx = button(card(), 'Install CRX file…');
  assert.ok(chooseCrx, 'the empty state offers the file too');
  assert.equal(choose.className, chooseCrx.className, 'same visual weight');
  assert.equal(choose.classList.contains('is-primary'), false);
  choose.click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'extinstall' }]);
  assert.equal(button(card(), 'Choose folder…').disabled, true);
  assert.equal(button(card(), 'Install CRX file…').disabled, true, 'one picker at a time');
  button(card(), 'Choose folder…').click();
  button(card(), 'Install CRX file…').click();
  assert.equal(sent.length, 1);
  receive({ ...fixture.empty, d: { ...fixture.empty.d, ack: 'none' } });
  assert.equal(button(card(), 'Choose folder…').disabled, false, 'a cancelled picker frees it');
  assert.equal(button(card(), 'Install CRX file…').disabled, false);
});

test('section: Install CRX file… asks Bowser to open its file picker, once, and waits for the answer', () => {
  sent.length = 0;
  button(card(), 'Install CRX file…').click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'extinstallcrx' }]);
  assert.equal(button(card(), 'Install CRX file…').disabled, true);
  assert.equal(button(card(), 'Choose folder…').disabled, true);
  button(card(), 'Install CRX file…').click();
  assert.equal(sent.length, 1);
  receive({ ...fixture.empty, d: { ...fixture.empty.d, ack: 'failed', error: 'The extension was not installed. The file did not verify as a signed CRX package: CRX signature does not verify.' } });
  assert.deepEqual(toasts.at(-1), { text: 'The extension was not installed. The file did not verify as a signed CRX package: CRX signature does not verify.', ok: false }, 'a failure uses the existing error toast');
  assert.equal(button(card(), 'Install CRX file…').disabled, false);
  assert.doesNotMatch(visibleText(), /\bstore\b|chrome web|add to chrome|one-click/i);
});

test('section: an unreadable list and an invalid snapshot each say so and offer to try again', () => {
  receive(fixture.error);
  let state = card().querySelector('.sp-ext-state.is-error');
  assert.equal(state.getAttribute('role'), 'alert');
  assert.match(state.textContent, /Extensions could not be read/);
  assert.match(state.textContent, /The installed extension list is damaged\./);
  sent.length = 0;
  button(state, 'Try again').click();
  assert.deepEqual(sent, [{ v: 1, t: 'set', a: 'refresh' }]);
  section.receive({ t: 'snap', p: 'extensions', d: { ...fixture.snap.d, items: 'nope' } });
  state = card().querySelector('.sp-ext-state.is-error');
  assert.match(state.textContent, /could not be shown/);
  receive(fixture.snap);
  assert.equal(section.root.querySelectorAll('.sp-ext-item').length, 2);
});

// ---- the toolbar button ----

const { buildView } = await bundle('view.ts');
const bar = new Node('div');
const view = buildView(bar);
const ext = view.extensions.button;

test('toolbar: hidden until an enabled extension has a popup; left of the download button; named after it', () => {
  assert.equal(ext.hidden, true);
  assert.equal(ext.parent, bar.querySelector('.actions'));
  assert.equal(ext.nextSibling, view.downloads.button, 'the download button keeps its place by the menu');
  assert.ok(ext.querySelector('svg'), 'Bowser’s puzzle glyph');
  view.extensions.apply(protocol.parseInbound(JSON.stringify(fixture.buttonOne)));
  assert.equal(ext.hidden, false);
  assert.ok(bar.classList.contains('has-ext'));
  assert.ok(ext.classList.contains('is-in'));
  assert.equal(ext.getAttribute('aria-label'), 'Open Reader "<b>"');
  assert.equal(ext.getAttribute('aria-haspopup'), null, 'one popup opens straight away');
  view.extensions.apply(protocol.parseInbound(JSON.stringify(fixture.buttonMany)));
  assert.equal(ext.getAttribute('aria-label'), 'Extensions');
  assert.equal(ext.getAttribute('aria-haspopup'), 'menu', 'several: Rust lists them under the button');
  view.extensions.apply(protocol.parseInbound(JSON.stringify(fixture.buttonOff)));
  assert.ok(ext.classList.contains('is-out'));
  assert.equal(ext.hidden, false, 'it fades before it goes');
  flush();
  assert.equal(ext.hidden, true);
  assert.equal(bar.classList.contains('has-ext'), false);
});

test('toolbar: a click sends only the button’s rectangle; the slot matches Rust', () => {
  sent.length = 0;
  assert.ok(protocol.sendExtensionsButton(ext.getBoundingClientRect()));
  assert.deepEqual(sent, [{ v: 1, t: 'ext', a: 'open', text: '768,1,28,28' }]);
  const css = read(join(srcDir, 'styles', 'toolbar.css'));
  assert.match(css, /#app\.placed\.has-ext \.actions \{\s*left: calc\(var\(--menu-x\) - 32px\);/, 'EXT_BUTTON_SLOT');
  assert.match(css, /#app\.placed\.has-ext\.has-dl \.actions \{\s*left: calc\(var\(--menu-x\) - 64px\);/, 'both slots');
  const rust = read(join(here, '..', '..', 'src', 'chrome_host', 'mod.rs'));
  assert.match(rust, /pub const EXT_BUTTON_SLOT: i32 = 32;/);
  const main = read(join(srcDir, 'main.ts'));
  assert.match(main, /view\.extensions\.button\.addEventListener\('click', \(\) => \{ sendExtensionsButton\(view\.extensions\.button\.getBoundingClientRect\(\)\); \}\);/);
  assert.match(main, /case 'extbtn':\s*view\.extensions\.apply\(msg\);/);
});

test('settings.css: the section moves only colour and keeps to the page tokens', () => {
  const css = read(join(srcDir, 'styles', 'settings.css'));
  const block = css.slice(css.indexOf('/* ---- Extensions'), css.indexOf('/* ---- "Saved" toast'));
  assert.ok(block.length > 1000);
  assert.doesNotMatch(block, /@keyframes|animation:|infinite/);
  for (const m of block.matchAll(/transition:\s*([^;]+);/g)) {
    for (const part of m[1].split(',')) assert.ok(['background-color', 'color'].includes(part.trim().split(/\s+/)[0]), part);
  }
  assert.doesNotMatch(block, /var\(--(bg|fill|border|text|placeholder|hover)\)/, 'host inputs only through --bw-* tokens');
  assert.doesNotMatch(block, /#(?!c42b1c\b)[0-9a-f]{3,8}\b/i, 'no new literal colours (the page’s danger red is reused)');
});
