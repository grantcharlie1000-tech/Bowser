// The Tabs options in a fake document: the settings page's Tabs section, the Sessions panel, the tab
// strip's marks and inputs, and the hover card's thumbnail and memory. Each test names what a click or
// a snapshot must do, and what crosses the bridge to Rust.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, '..', 'src');
const read = (path) => readFileSync(path, 'utf8').replace(/\r\n/g, '\n');
const fixture = (name) => JSON.parse(read(join(here, 'fixtures', name)));

async function bundle(file) {
  const out = esbuild.buildSync({ entryPoints: [join(srcDir, file)], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
}

// ---- a small fake document ----

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
    this.ownText = text; this.id = ''; this.type = ''; this.offsetTop = 0; this.offsetHeight = 34; this.offsetWidth = 120;
    this.scrollTop = 0; this.clientHeight = 800; this.clientWidth = 800; this.scrollHeight = 4000; this.scrollLeft = 0;
    const props = new Map();
    this.style = { setProperty: (k, v) => (v === '' ? props.delete(k) : props.set(k, v)), getPropertyValue: (k) => props.get(k) ?? '', props };
  }
  get textContent() { return this.ownText ?? this.children.map((c) => c.textContent).join(''); }
  set textContent(v) { this.children = []; this.ownText = null; if (v !== '') this.append(new Node('#text', String(v))); }
  get childElementCount() { return this.children.filter((c) => c.tagName !== '#text').length; }
  get firstChild() { return this.children[0] ?? null; }
  get lastChild() { return this.children[this.children.length - 1] ?? null; }
  get nextSibling() { const p = this.parent; return p ? p.children[p.children.indexOf(this) + 1] ?? null : null; }
  get parentElement() { return this.parent; }
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
  dispatch(type, event = {}) {
    const e = { target: this, type, preventDefault() {}, stopPropagation() {}, key: '', ctrlKey: false, shiftKey: false, metaKey: false, detail: 1, ...event };
    for (let node = this; node; node = node.parent) for (const cb of node.listeners[type] ?? []) cb({ ...e, currentTarget: node });
  }
  click(event = {}) { if (!this.disabled) this.dispatch('click', event); }
  detach(node) { if (node.parent) node.parent.children = node.parent.children.filter((c) => c !== node); node.parent = this; }
  append(...nodes) { for (const n of nodes) { const node = typeof n === 'string' ? new Node('#text', n) : n; this.ownText = null; this.detach(node); this.children.push(node); } }
  prepend(node) { this.detach(node); this.children.unshift(node); }
  insertBefore(node, ref) { this.detach(node); const at = ref ? this.children.indexOf(ref) : -1; if (at < 0) this.children.push(node); else this.children.splice(at, 0, node); return node; }
  after(node) { const p = this.parent; p.detach(node); p.children.splice(p.children.indexOf(this) + 1, 0, node); }
  replaceWith(node) { const p = this.parent; const at = p.children.indexOf(this); p.detach(node); p.children.splice(at, 1, node); this.parent = null; }
  replaceChildren(...nodes) { this.children = []; this.ownText = null; this.append(...nodes); }
  remove() { if (this.parent) this.parent.children = this.parent.children.filter((c) => c !== this); this.parent = null; }
  all() { return this.children.flatMap((c) => [c, ...c.all()]); }
  matches(s) { return sel(s).some((x) => matchOne(this, x)); }
  closest(s) { for (let node = this; node; node = node.parent) if (node.tagName !== '#text' && node.matches(s)) return node; return null; }
  querySelectorAll(s) {
    const parts = s.trim().split(/\s+/);
    if (parts.length > 1 && !s.includes(',') && !s.includes(':not(')) {
      // A descendant chain: each step searches inside what the step before matched.
      return parts.reduce((scope, part) => [...new Set(scope.flatMap((node) => node.querySelectorAll(part)))], [this]);
    }
    return this.all().filter((n) => n.tagName !== '#text' && n.matches(s));
  }
  querySelector(s) { return this.querySelectorAll(s)[0] ?? null; }
  focus() { globalThis.document.activeElement = this; }
  select() {}
  scrollTo(o) { this.scrollTop = o.top ?? 0; }
  scrollIntoView() {}
  setPointerCapture() {}
  hasPointerCapture() { return false; }
  releasePointerCapture() {}
  contains(n) { return n === this || this.all().includes(n); }
  getBoundingClientRect() { return { left: 0, top: 0, right: this.offsetWidth, bottom: 26, width: this.offsetWidth, height: 26 }; }
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
const intervals = new Map();
let nextInterval = 1;
Object.assign(globalThis, {
  innerWidth: 1280, innerHeight: 860, devicePixelRatio: 1,
  matchMedia: () => ({ matches: false }),
  requestAnimationFrame: (cb) => { cb(); return 0; },
  cancelAnimationFrame() {},
  getComputedStyle: () => ({ columnGap: '4px' }),
  addEventListener() {}, removeEventListener() {},
  setInterval: (cb) => { intervals.set(nextInterval, cb); return nextInterval++; },
  clearInterval: (id) => { intervals.delete(id); },
  IntersectionObserver: class { observe() {} disconnect() {} },
  ResizeObserver: class { observe() {} disconnect() {} },
  MutationObserver: class { observe() {} disconnect() {} },
  chrome: { webview: { postMessage: (wire) => sent.push(JSON.parse(wire)), addEventListener: (_t, cb) => listeners.push(cb) } },
});
globalThis.Element = Node;
globalThis.HTMLElement = Node;
globalThis.HTMLInputElement = class {};
globalThis.HTMLTextAreaElement = class {};
globalThis.Event = class { constructor(type) { this.type = type; } };
const deliver = (msg) => { for (const cb of listeners) cb({ data: JSON.stringify(msg) }); };

const protocol = await bundle('protocol.ts');
const options = await bundle('tab-options-logic.ts');

// ---- the settings page's Tabs section ----

const { startSettingsPage } = await bundle('settings-page.ts');
const settingsFixture = fixture('settings-messages.json');
const page = new Node('div');
startSettingsPage(page);
deliver(settingsFixture.snap);
const rowOf = (key) => page.querySelector(`.sp-row[data-row="${key}"]`);

test('the Tabs section lists every option in cards, each with its own control and a plain label', () => {
  // --- settings slim --- Tabs is the first part of the Tabs and start page area; its core rows sit in
  // one Tabs card and every other option keeps its card heading behind the area's More options.
  const section = page.querySelector('section[data-area="tabs"]');
  assert.ok(section, 'a Tabs area');
  assert.equal(section.querySelector('h2').textContent, 'Tabs and start page');
  assert.ok(section.querySelector('section[data-section="tabs"]'), 'with a Tabs part');
  const cards = section.querySelector('.sp-more').querySelectorAll('.sp-group-h').map((h) => h.textContent);
  for (const card of options.TAB_CARDS) assert.ok(cards.includes(options.TAB_CARD_TITLES[card]), card);
  for (const spec of options.TAB_OPTIONS) {
    const row = rowOf(spec.key);
    assert.ok(row, spec.key);
    assert.equal(row.querySelector('.sp-label').textContent, spec.label, spec.key);
    assert.equal(row.querySelector('.sp-desc').textContent, spec.description, spec.key);
    assert.equal(row.querySelector('.sp-flag'), null, `${spec.key} acts now: no "no effect yet" note`);
    const control = spec.kind === 'toggle' ? row.querySelector('[role="switch"]') : spec.kind === 'choice' ? row.querySelector('[role="radiogroup"]') : row.querySelector('input');
    assert.ok(control, `${spec.key} has a ${spec.kind} control`);
  }
  const nav = page.querySelector('.sp-nav-item[data-section="tabs"]');
  assert.ok(nav);
  assert.equal(nav.textContent, 'Tabs and start page');
});

test('a switch, a choice, a number and the address each send one tabopt command and nothing else', () => {
  sent.length = 0;
  // Switch.
  const keep = rowOf('tabs_keep_window').querySelector('[role="switch"]');
  assert.equal(keep.getAttribute('aria-checked'), 'true');
  keep.click();
  assert.deepEqual(sent.splice(0), [{ v: 1, t: 'set', a: 'tabopt', text: 'tabs_keep_window=false' }]);
  // Choice.
  const activation = rowOf('tabs_close_activation').querySelector('[role="radiogroup"]');
  const radios = activation.querySelectorAll('[role="radio"]');
  assert.deepEqual(radios.map((r) => r.textContent), ['Tab on the right', 'Tab on the left', 'Recently used']);
  assert.deepEqual(radios.map((r) => r.getAttribute('aria-checked')), ['true', 'false', 'false']);
  radios[2].click();
  assert.deepEqual(sent.splice(0), [{ v: 1, t: 'set', a: 'tabopt', text: 'tabs_close_activation=recent' }]);
  // New tab position: the three positions, no stack.
  const position = rowOf('tabs_new_tab_position').querySelectorAll('[role="radio"]');
  assert.deepEqual(position.map((r) => r.textContent), ['After related', 'After active', 'As last', 'As stack with related']); // --- T10 ---
  position[0].click();
  assert.deepEqual(sent.splice(0), [{ v: 1, t: 'set', a: 'tabopt', text: 'tabs_new_tab_position=related' }]);
  // Number: typed, clamped, sent; one tab is not a bulk close.
  const confirm = rowOf('tabs_confirm_close').querySelector('input');
  confirm.value = '5';
  confirm.dispatch('change');
  assert.deepEqual(sent.splice(0), [{ v: 1, t: 'set', a: 'tabopt', text: 'tabs_confirm_close=5' }]);
  confirm.value = '1';
  confirm.dispatch('change');
  assert.deepEqual(sent.splice(0), [{ v: 1, t: 'set', a: 'tabopt', text: 'tabs_confirm_close=2' }]);
  assert.equal(confirm.value, '2');
  confirm.value = '999';
  confirm.dispatch('change');
  assert.deepEqual(sent.splice(0), [{ v: 1, t: 'set', a: 'tabopt', text: 'tabs_confirm_close=99' }]);
  // Address: trimmed; a script address never leaves the page.
  const address = rowOf('tabs_new_tab_url').querySelector('input');
  address.value = '  https://example.com/start  ';
  address.dispatch('change');
  assert.deepEqual(sent.splice(0), [{ v: 1, t: 'set', a: 'tabopt', text: 'tabs_new_tab_url=https://example.com/start' }]);
  address.value = 'javascript:alert(1)';
  address.dispatch('change');
  assert.deepEqual(sent.filter((m) => m.a === 'tabopt'), [], 'refused in the page, before Rust sees it');
  assert.ok(!sent.some((m) => m.a === 'save'), 'none of these is part of save');
});

test('the next snapshot is the truth: it moves every control, and the address follows the new tab page', () => {
  const values = { ...settingsFixture.snap.d.tabOptions, tabs_keep_window: false, tabs_close_activation: 'left', tabs_confirm_close: 7, tabs_new_tab_page: 'url', tabs_new_tab_url: 'https://start.example/', tabs_mute_mode: 'active', tabs_new_tab_position: 'last' };
  deliver({ ...settingsFixture.snap, d: { ...settingsFixture.snap.d, tabOptions: values } });
  assert.equal(rowOf('tabs_keep_window').querySelector('[role="switch"]').getAttribute('aria-checked'), 'false');
  const checked = (key) => rowOf(key).querySelectorAll('[role="radio"]').find((r) => r.getAttribute('aria-checked') === 'true')?.textContent;
  assert.equal(checked('tabs_close_activation'), 'Tab on the left');
  assert.equal(checked('tabs_mute_mode'), 'Only the active tab');
  assert.equal(checked('tabs_new_tab_position'), 'As last');
  assert.equal(rowOf('tabs_confirm_close').querySelector('input').value, '7');
  assert.equal(rowOf('tabs_confirm_close').querySelector('.sp-value').textContent, '7 tabs');
  const address = rowOf('tabs_new_tab_url').querySelector('input');
  assert.equal(address.value, 'https://start.example/');
  assert.equal(address.disabled, false, 'a specific page: the address is live');
  deliver({ ...settingsFixture.snap, d: { ...settingsFixture.snap.d, tabOptions: { ...values, tabs_new_tab_page: 'start' } } });
  assert.equal(address.disabled, true, 'the start page needs no address');
  // An unreadable settings file disables the Tabs controls too.
  deliver({ ...settingsFixture.snap, d: { ...settingsFixture.snap.d, readable: false } });
  assert.equal(rowOf('tabs_keep_window').querySelector('[role="switch"]').disabled, true);
  assert.equal(rowOf('tabs_confirm_close').querySelector('input').disabled, true);
  assert.ok(rowOf('tabs_close_activation').querySelectorAll('[role="radio"]').every((r) => r.disabled));
  deliver(settingsFixture.snap);
  assert.equal(rowOf('tabs_keep_window').querySelector('[role="switch"]').disabled, false);
});

// --- two-level stacks (T10b) ---
test('two-level is a choice of the Tab stacks row, and its compact option waits behind More options until it is picked', () => {
  const style = rowOf('tabs_stack_style');
  const radios = style.querySelectorAll('[role="radio"]');
  assert.deepEqual(radios.map((r) => r.textContent), ['Islands', 'Compact', 'Accordion', 'Two-level', 'Off'], 'one more value, not a new row');
  sent.length = 0;
  radios[3].click();
  assert.deepEqual(sent.splice(0), [{ v: 1, t: 'set', a: 'tabopt', text: 'tabs_stack_style=two-level' }]);
  const compact = rowOf('tabs_stack_compact_row');
  assert.ok(compact.closest('.sp-more'), 'behind the area’s More options');
  assert.equal(compact.querySelector('.sp-label').textContent, 'Compact second row');
  const sw = compact.querySelector('[role="switch"]');
  assert.equal(sw.disabled, true, 'Islands: there is no second row to make smaller');
  const values = { ...settingsFixture.snap.d.tabOptions, tabs_stack_style: 'two-level' };
  deliver({ ...settingsFixture.snap, d: { ...settingsFixture.snap.d, tabOptions: values } });
  assert.equal(sw.disabled, false, 'two-level: the option acts');
  sw.click();
  assert.deepEqual(sent.splice(0), [{ v: 1, t: 'set', a: 'tabopt', text: 'tabs_stack_compact_row=true' }]);
  deliver({ ...settingsFixture.snap, d: { ...settingsFixture.snap.d, readable: false, tabOptions: values } });
  assert.equal(sw.disabled, true, 'an unreadable settings file disables it too');
  deliver(settingsFixture.snap);
  assert.equal(sw.disabled, true);
});

test('search finds the tab options by the words people use', () => {
  const settings = page.querySelector('.sp-search-input');
  const find = (text) => {
    settings.value = text;
    settings.dispatch('input');
    return options.TAB_OPTIONS.filter((spec) => !rowOf(spec.key).hidden).map((spec) => spec.key);
  };
  assert.ok(find('close button').includes('tabs_close_left'));
  assert.ok(find('hibernated').includes('tabs_dim_hibernated'));
  assert.ok(find('pinned').includes('tabs_pinned_close'));
  assert.ok(find('mute').includes('tabs_mute_mode'));
  assert.ok(find('lazy').includes('tabs_lazy_restore'));
  assert.deepEqual(find('zzzz'), []);
  settings.value = '';
  settings.dispatch('input');
});

test('the tab preview rows act now: no "has no effect yet" note on them', () => {
  for (const id of ['tabPreviewImages', 'tabPreviewMemory']) {
    const row = page.querySelector(`.sp-row[data-row="${id}"]`);
    assert.ok(row, id);
    assert.equal(row.querySelector('.sp-flag'), null, id);
  }
});

// ---- the Sessions panel ----

const { startSessionsPanel } = await bundle('panels/sessions.ts');
const sessionsFixture = fixture('sessions-messages.json');
const panelRoot = new Node('div');
const receive = startSessionsPanel(panelRoot);
const snapMsg = (d) => ({ t: 'snap', p: 'sessions', d });
const snapOf = () => protocol.parseInbound(JSON.stringify(sessionsFixture.snap));
receive(snapOf());
const rows = () => panelRoot.querySelectorAll('.ps-row');
const sentSess = () => sent.filter((m) => m.t === 'sess');

test('the panel lists the saved sessions with their size and sites, and says how many tabs would be saved', () => {
  assert.equal(panelRoot.querySelector('.pn-title').textContent, 'Sessions');
  assert.equal(panelRoot.querySelector('.ps-info').textContent, '4 open tabs, 3 selected');
  assert.deepEqual(rows().map((r) => r.querySelector('.bw-row-title').textContent), ['Work', 'Reading list']);
  assert.deepEqual(rows().map((r) => r.querySelector('.bw-row-meta').textContent), ['2 tabs · example.com, news.example', '1 tab · one.example']);
  assert.equal(panelRoot.querySelector('.ps-list').getAttribute('aria-label'), 'Saved sessions');
});

test('a double click opens the session in this window and Enter does too; the buttons replace or open', () => {
  sent.length = 0;
  rows()[0].dispatch('dblclick');
  assert.deepEqual(sentSess().splice(0), [{ v: 1, t: 'sess', a: 'open', text: 'Work', n: 0 }]);
  sent.length = 0;
  rows()[1].dispatch('keydown', { key: 'Enter', target: rows()[1] });
  assert.deepEqual(sentSess(), [{ v: 1, t: 'sess', a: 'open', text: 'Reading list', n: 0 }]);
  sent.length = 0;
  const buttons = rows()[0].querySelectorAll('.bw-icon-btn');
  assert.deepEqual(buttons.map((b) => b.getAttribute('aria-label')), ['Open in this window', 'Replace the open tabs', 'Delete session']);
  buttons[1].click();
  assert.deepEqual(sentSess(), [{ v: 1, t: 'sess', a: 'open', text: 'Work', n: 1 }]);
});

test('Save Tabs as Session asks for a name, saves all or the selected tabs, and refuses a blank name', () => {
  sent.length = 0;
  const save = panelRoot.querySelector('.ps-bar .bw-button');
  assert.equal(save.textContent, 'Save tabs as session');
  const form = panelRoot.querySelector('.ps-form');
  assert.equal(form.hidden, true);
  save.click();
  assert.equal(form.hidden, false);
  const name = form.querySelector('input');
  assert.equal(name.value, 'Session 1', 'a free name is suggested');
  const which = form.querySelectorAll('[role="radio"]');
  assert.deepEqual(which.map((r) => r.textContent), ['All 4 tabs', '3 selected tabs']);
  assert.deepEqual(which.map((r) => r.getAttribute('aria-checked')), ['false', 'true'], 'tabs are selected: they are what is saved');
  name.value = '   ';
  form.dispatch('submit');
  assert.equal(form.querySelector('.ps-error').textContent, 'Enter a name.');
  assert.deepEqual(sentSess(), []);
  name.value = '  Research  ';
  which[0].click();
  form.dispatch('submit');
  assert.deepEqual(sentSess(), [{ v: 1, t: 'sess', a: 'save', text: 'Research', n: 0 }]);
  assert.equal(form.hidden, true, 'the form closes after a save');
  sent.length = 0;
  // Selected tabs.
  save.click();
  name.value = 'Pair';
  form.dispatch('submit');
  assert.deepEqual(sentSess().splice(0), [{ v: 1, t: 'sess', a: 'save', text: 'Pair', n: 1 }]);
});

test('saving over an existing name asks once, and Escape leaves the form', () => {
  sent.length = 0;
  const save = panelRoot.querySelector('.ps-bar .bw-button');
  const form = panelRoot.querySelector('.ps-form');
  save.click();
  const name = form.querySelector('input');
  name.value = 'Work';
  form.dispatch('submit');
  assert.match(form.querySelector('.ps-error').textContent, /already saved/);
  assert.deepEqual(sentSess(), []);
  form.dispatch('submit');
  assert.equal(sentSess().length, 1, 'the second Save replaces it');
  save.click();
  form.dispatch('keydown', { key: 'Escape' });
  assert.equal(form.hidden, true);
  assert.equal(panelRoot.querySelector('.ps-bar').hidden, false);
});

test('Delete takes two clicks, and the second goes to Rust', () => {
  sent.length = 0;
  const del = () => rows()[1].querySelectorAll('.bw-icon-btn')[2];
  del().click();
  assert.deepEqual(sentSess(), [], 'the first click only asks');
  assert.equal(rows()[1].classList.contains('is-confirming'), true);
  assert.equal(del().getAttribute('aria-label'), 'Confirm: delete this session');
  del().click();
  assert.deepEqual(sentSess(), [{ v: 1, t: 'sess', a: 'delete', text: 'Reading list' }]);
});

test('a guest has no sessions, an empty list says what to do, and a bad snapshot changes nothing', () => {
  const good = snapOf().d;
  receive(snapMsg({ ...good, allowed: false, items: [] }));
  assert.match(panelRoot.querySelector('.pn-empty').textContent, /guest window does not keep sessions/i);
  assert.equal(panelRoot.querySelector('.ps-bar .bw-button').disabled, true);
  receive(snapMsg({ ...good, items: [] }));
  assert.match(panelRoot.querySelector('.pn-empty').textContent, /No saved sessions yet/);
  receive(snapMsg({ ...good, items: 'broken' }));
  assert.match(panelRoot.querySelector('.pn-empty').textContent, /No saved sessions yet/, 'the last good list stays');
  receive(snapOf());
  assert.equal(rows().length, 2);
  receive({ t: 'snap', p: 'privacy', d: {} });
  assert.equal(rows().length, 2, 'other panels snapshots are not this one');
});

// ---- the strip's marks ----

const strip = await bundle('tabs.ts');
const item = (id, extra = {}) => ({ id, title: `Tab ${id}`, loading: false, pinned: false, muted: false, private: false, marker: -1, g: 0, gl: false, fav: false, ...extra });

test('a plain tab keeps its plain DOM; marks add their nodes and removing them takes the nodes away', () => {
  const node = strip.createTab(7);
  const plainChildren = node.root.children.map((c) => c.className);
  strip.updateTab(node, item(7), false, '');
  assert.deepEqual(node.root.children.map((c) => c.className), plainChildren, 'nothing is added to a plain tab');
  assert.equal(node.root.getAttribute('aria-label'), 'Tab 7');

  strip.updateTab(node, item(7, { unread: true, notify: 3, rl: 30, asleep: true, idle: true }), false, '', { indicator: 'spinner', picked: true, now: 1000 });
  assert.ok(node.root.querySelector('.unread') && node.root.querySelector('.notify') && node.root.querySelector('.rl'));
  assert.equal(node.root.querySelector('.notify').textContent, '3');
  assert.equal(node.root.querySelector('.rl').textContent, '30s');
  for (const cls of ['asleep', 'idle', 'picked']) assert.equal(node.root.classList.contains(cls), true, cls);
  assert.equal(node.root.getAttribute('aria-label'), 'Tab 7, 3 notifications, unread, reloads in 30s');
  const order = node.root.children.map((c) => c.className.split(' ')[0]);
  assert.ok(order.indexOf('notify') < order.indexOf('audio') && order.indexOf('unread') < order.indexOf('audio'), 'the marks sit before the buttons');

  strip.updateTab(node, item(7, { notify: 120 }), false, '');
  assert.equal(node.root.querySelector('.notify').textContent, '99+');
  assert.equal(node.root.querySelector('.unread'), null);
  assert.equal(node.root.querySelector('.rl'), null);
  for (const cls of ['asleep', 'idle', 'picked']) assert.equal(node.root.classList.contains(cls), false, cls);
  strip.updateTab(node, item(7), false, '');
  assert.deepEqual(node.root.children.map((c) => c.className), plainChildren);
});

test('page load shows as a spinner, a bar, a favicon ring or nothing, as the option says', () => {
  const node = strip.createTab(8);
  const modeOf = () => node.root.dataset.icon;
  strip.updateTab(node, item(8, { loading: true, pct: 45 }), false, 'data:image/png;base64,AAAA', { indicator: 'spinner', picked: false, now: 0 });
  assert.equal(modeOf(), 'spin');
  assert.equal(node.root.querySelector('.loadbar'), null);
  strip.updateTab(node, item(8, { loading: true, pct: 45 }), false, 'data:image/png;base64,AAAA', { indicator: 'bar', picked: false, now: 0 });
  assert.equal(modeOf(), 'fav', 'the favicon stays while a bar shows progress');
  assert.equal(node.root.querySelector('.loadbar').style.getPropertyValue('--pct'), '0.45');
  strip.updateTab(node, item(8, { loading: true, pct: 75 }), false, 'data:image/png;base64,AAAA', { indicator: 'bar', picked: false, now: 0 });
  assert.equal(node.root.querySelector('.loadbar').style.getPropertyValue('--pct'), '0.75');
  strip.updateTab(node, item(8, { loading: true, pct: 60 }), false, 'data:image/png;base64,AAAA', { indicator: 'favicon', picked: false, now: 0 });
  assert.equal(node.root.querySelector('.loadbar'), null);
  assert.equal(node.root.querySelector('.ring').style.getPropertyValue('--pct'), '60');
  strip.updateTab(node, item(8, { loading: true, pct: 60 }), false, '', { indicator: 'none', picked: false, now: 0 });
  assert.equal(modeOf(), 'glyph');
  assert.equal(node.root.querySelector('.ring'), null);
  strip.updateTab(node, item(8, { loading: true }), false, '', { indicator: 'bar', picked: false, now: 0 });
  assert.equal(modeOf(), 'spin', 'no progress figure yet: the spinner stands in so a load is never invisible');
  strip.updateTab(node, item(8, { loading: false, pct: 100 }), false, '', { indicator: 'bar', picked: false, now: 0 });
  assert.equal(node.root.querySelector('.loadbar'), null, 'a finished load shows nothing');
});

test('the reload countdown counts down locally from what the host said', () => {
  const node = strip.createTab(9);
  strip.updateTab(node, item(9, { rl: 30 }), false, '', { indicator: 'spinner', picked: false, now: 10_000 });
  strip.tickReloads([node], 10_000 + 9_100);
  assert.equal(node.root.querySelector('.rl').textContent, '21s');
  strip.tickReloads([node], 10_000 + 31_000);
  assert.equal(node.root.querySelector('.rl').textContent, '0s');
  strip.updateTab(node, item(9, { rl: 3600 }), false, '', { indicator: 'spinner', picked: false, now: 99_000 });
  assert.equal(node.root.querySelector('.rl').textContent, '1h');
});

// ---- the whole strip: options, selection, the closed tabs button ----

const stripRoot = new Node('div');
stripRoot.id = 'app';
strip.startTabs(stripRoot);
const tabsMsg = (items, active) => ({ v: 1, t: 'tabs', active, items });
deliver(tabsMsg([item(1, { pinned: true }), item(2), item(3), item(4)], 2));
const tabNode = (id) => stripRoot.querySelector(`.tab[data-id="${id}"]`);
const opts = (extra = {}) => ({ ...fixture('tab-options-messages.json').defaults, ...extra });

test('the strip follows the host: classes, the pinned separator and the Closed tabs button', () => {
  assert.equal(stripRoot.querySelector('.closed-btn'), null, 'nothing closed: no button');
  deliver(opts({ closeLeft: true, scroll: true, separator: true, minWidth: 220, closed: 3 }));
  for (const cls of ['close-left', 'scrolling', 'pin-sep']) assert.equal(stripRoot.classList.contains(cls), true, cls);
  assert.equal(stripRoot.classList.contains('close-always'), false);
  assert.equal(stripRoot.style.getPropertyValue('--tab-active-min'), '220px');
  assert.ok(stripRoot.querySelector('.pinsep'), 'a line where the pinned tabs end');
  const order = stripRoot.children.map((c) => c.className.split(' ')[0]);
  assert.ok(order.indexOf('pinsep') > order.indexOf('tab') && order.indexOf('pinsep') < order.lastIndexOf('tab'), 'between the pinned tab and the others');
  const closed = stripRoot.querySelector('.closed-btn');
  assert.ok(closed);
  assert.equal(closed.getAttribute('aria-label'), 'Closed tabs');
  sent.length = 0;
  closed.click();
  assert.deepEqual(sent, [{ v: 1, t: 'closedtabs' }]);
  deliver(opts({ closed: 0, maximised: true, removeSpacing: true }));
  assert.equal(stripRoot.querySelector('.closed-btn'), null);
  assert.equal(stripRoot.classList.contains('flush'), true, 'maximised with the spacing removed');
  assert.equal(stripRoot.classList.contains('scrolling'), false);
  assert.equal(stripRoot.style.getPropertyValue('--tab-active-min'), '');
});

// --- toolbars and feeds (thread T13) --- the tab bar's list: the Closed tabs button only when it is listed, and
// the command buttons the user put there, each sending its item and its place in the list.
test('the tab bar’s list decides the Closed tabs button and adds command buttons that send their place', () => {
  const box = () => stripRoot.querySelector('.strip-tools');
  const labels = () => (box()?.children ?? []).map((b) => b.getAttribute('aria-label'));
  deliver(opts({ closed: 2, closedButton: true, tools: [] }));
  assert.deepEqual(labels(), ['Closed tabs']);
  deliver(opts({ closed: 2, closedButton: false, tools: [] }));
  assert.deepEqual(labels(), [], 'off the list, the button is gone even with closed tabs to list');
  assert.equal(box(), null);
  deliver(opts({ closed: 0, closedButton: true, tools: [{ k: 'cmd:new-window', l: 'New window', i: 1 }] }));
  assert.deepEqual(labels(), ['New window']);
  deliver(opts({ closed: 3, closedButton: true, tools: [{ k: 'cmd:new-window', l: 'New window', i: 1 }, { k: 'chain:2', l: 'Morning', i: 2 }] }));
  assert.deepEqual(labels(), ['Closed tabs', 'New window', 'Morning']);
  sent.length = 0;
  box().children[1].click();
  box().children[2].click();
  box().children[0].click();
  assert.deepEqual(sent, [{ v: 1, t: 'tool', b: 'tabs', k: 'cmd:new-window', i: 1 }, { v: 1, t: 'tool', b: 'tabs', k: 'chain:2', i: 2 }, { v: 1, t: 'closedtabs' }]);
  deliver(opts({ closed: 0 }));
  assert.equal(box(), null);
});
// --- end toolbars and feeds ---

test('Ctrl and Shift click select tabs and tell Rust; a plain click clears and selects', () => {
  deliver(opts({ selection: true }));
  sent.length = 0;
  tabNode(4).click({ ctrlKey: true });
  assert.deepEqual(sent, [{ v: 1, t: 'tabsel', ids: '2,4' }], 'the active tab joins first; no tab was selected by a Ctrl click');
  assert.equal(tabNode(4).classList.contains('picked'), true);
  assert.equal(tabNode(2).classList.contains('picked'), true);
  assert.ok(!sent.some((m) => m.t === 'tab'), 'the page being read stays');
  sent.length = 0;
  tabNode(3).click({ shiftKey: true });
  assert.deepEqual(sent, [{ v: 1, t: 'tabsel', ids: '3,4' }], 'a range from the anchor (the tab last clicked, 4) to tab 3, both ends included');
  sent.length = 0;
  tabNode(3).click();
  assert.deepEqual(sent, [{ v: 1, t: 'tabsel', ids: '' }, { v: 1, t: 'tab', a: 'select', id: 3 }]);
  assert.equal(tabNode(4).classList.contains('picked'), false);
  // Escape drops a selection.
  tabNode(4).click({ ctrlKey: true });
  sent.length = 0;
  stripRoot.dispatch('keydown', { key: 'Escape', target: tabNode(4) });
  assert.deepEqual(sent, [{ v: 1, t: 'tabsel', ids: '' }]);
});

test('with selection off a Ctrl click is just a click', () => {
  deliver(opts({ selection: false }));
  sent.length = 0;
  tabNode(4).click({ ctrlKey: true });
  assert.deepEqual(sent, [{ v: 1, t: 'tab', a: 'select', id: 4 }]);
  deliver(opts({ selection: true }));
});

test('a closed tab leaves the selection', () => {
  deliver(opts({ selection: true }));
  tabNode(2).click();
  tabNode(4).click({ ctrlKey: true });
  sent.length = 0;
  deliver(tabsMsg([item(1, { pinned: true }), item(2), item(3)], 2));
  assert.deepEqual(sent, [{ v: 1, t: 'tabsel', ids: '2' }]);
});

test('a double click on a tab goes to Rust, which decides what it does; the close button is not a tab', () => {
  sent.length = 0;
  tabNode(3).dispatch('dblclick');
  assert.deepEqual(sent, [{ v: 1, t: 'tab', a: 'dblclick', id: 3 }]);
  sent.length = 0;
  tabNode(3).querySelector('.close').dispatch('dblclick');
  assert.deepEqual(sent, []);
});

test('the wheel switches tabs only when asked, a notch at a time', () => {
  const wheel = (deltaY) => stripRoot.dispatch('wheel', { deltaY, deltaX: 0, shiftKey: false });
  sent.length = 0;
  wheel(100);
  assert.deepEqual(sent, [], 'off: the wheel scrolls the strip as before');
  deliver(opts({ wheel: true }));
  wheel(100);
  assert.deepEqual(sent, [{ v: 1, t: 'tab', a: 'select', id: 3 }], 'down: the next tab');
  sent.length = 0;
  wheel(-100);
  assert.deepEqual(sent, [{ v: 1, t: 'tab', a: 'select', id: 1 }], 'up: the previous tab');
  sent.length = 0;
  wheel(20);
  assert.deepEqual(sent, [], 'a small movement is not a step');
  deliver(opts({ wheel: false }));
});

test('closing by mouse holds the tab width until the pointer leaves, when asked', () => {
  deliver(opts({ alignClose: true }));
  tabNode(3).querySelector('.close').click();
  assert.equal(stripRoot.classList.contains('locked'), true);
  assert.equal(stripRoot.style.getPropertyValue('--tab-lock'), '120px');
  stripRoot.dispatch('pointerleave');
  assert.equal(stripRoot.classList.contains('locked'), false);
  assert.equal(stripRoot.style.getPropertyValue('--tab-lock'), '');
  deliver(opts({ alignClose: false }));
  tabNode(3).querySelector('.close').click();
  assert.equal(stripRoot.classList.contains('locked'), false, 'off: widths close up as before');
});

test('tabs with marks show them in the strip and a reload countdown ticks once a second', () => {
  deliver(tabsMsg([item(1, { pinned: true }), item(2, { notify: 3, unread: true }), item(3, { rl: 45 })], 2));
  assert.equal(tabNode(2).querySelector('.notify').textContent, '3');
  assert.ok(tabNode(2).querySelector('.unread'));
  assert.equal(tabNode(3).querySelector('.rl').textContent, '45s');
  assert.equal(intervals.size, 1, 'one timer while a tab counts down');
  deliver(tabsMsg([item(1, { pinned: true }), item(2), item(3)], 2));
  assert.equal(intervals.size, 0, 'the timer stops with the last countdown');
});

// ---- two-level stacks (T10b): the second row ----

test('two-level stacks: the tab row shows the stack as its open ball and the second row shows its tabs', () => {
  const lead = { gname: 'Docs', gcol: 2, gcollapsed: true, gsize: 3 };
  const stacked = (active) => tabsMsg([item(1), item(2, { g: 9, gl: true, ...lead }), item(3, { g: 9 }), item(4, { g: 9 }), item(5)], active);
  const row = document.body.querySelector('.stack-row');
  assert.ok(row, 'the strip page has a second row');
  assert.equal(row.hidden, true, 'hidden until Rust lays one out');
  assert.equal(row.classList.contains('strip'), true, 'its tabs are the strip’s own islands');
  // Compact style (no second row): the folded stack shows its ball and the tab in use, as before.
  deliver(opts({ stackRow: 'off' }));
  deliver(stacked(3));
  const ball = stripRoot.querySelector('.ball');
  assert.equal(ball.getAttribute('aria-expanded'), 'false');
  assert.equal(tabNode(3).hidden, false);
  assert.equal(tabNode(3).parent.classList.contains('group'), true, 'the tab in use stays in the tab row');
  assert.equal(row.children.length, 0);
  assert.equal(document.body.dataset.stackRow, undefined);
  // Rust laid a second row out: the stack is its open ball alone, its tabs move to the second row.
  deliver(opts({ stackRow: 'regular' }));
  assert.equal(row.hidden, false);
  assert.equal(row.dataset.size, 'regular');
  assert.equal(document.body.dataset.stackRow, 'regular', 'the page splits into the two rows');
  assert.deepEqual(row.children.map((c) => c.dataset.id), ['2', '3', '4'], 'every tab of the stack, in order');
  assert.ok(row.children.every((c) => !c.hidden));
  assert.equal(ball.getAttribute('aria-expanded'), 'true', 'the ball shows the stack open');
  assert.deepEqual(ball.parent.children, [ball], 'the tab row holds the ball alone');
  assert.equal(ball.parent.style.getPropertyValue('--gn'), '0', 'and gives it no tab share');
  assert.equal(tabNode(3), null, 'the tab in use is no longer in the tab row');
  assert.equal(row.children[1].classList.contains('active'), true, 'it is the second row’s selected tab');
  // A click, a middle click and a menu on a second-row tab go to Rust like the tab row's.
  sent.length = 0;
  row.children[0].click();
  row.children[2].querySelector('.close').click();
  row.children[1].dispatch('auxclick', { button: 1 });
  row.children[1].dispatch('contextmenu');
  assert.deepEqual(sent.filter((m) => m.t === 'tab'), [
    { v: 1, t: 'tab', a: 'select', id: 2 },
    { v: 1, t: 'tab', a: 'close', id: 4 },
    { v: 1, t: 'tab', a: 'close', id: 3 },
    { v: 1, t: 'tab', a: 'menu', id: 3 },
  ]);
  // The keyboard walks the second row on its own.
  row.children[1].focus();
  row.children[1].dispatch('keydown', { key: 'ArrowRight', target: row.children[1] });
  assert.equal(document.activeElement, row.children[2]);
  // "Switch tabs by scrolling" over the second row steps among the stack's tabs.
  deliver(opts({ stackRow: 'regular', wheel: true }));
  const wheelRow = (deltaY) => row.dispatch('wheel', { deltaY, deltaX: 0, shiftKey: false });
  sent.length = 0;
  wheelRow(100);
  wheelRow(-100);
  assert.deepEqual(sent.filter((m) => m.t === 'tab'), [
    { v: 1, t: 'tab', a: 'select', id: 4 },
    { v: 1, t: 'tab', a: 'select', id: 2 },
  ], 'the next and previous tab of the stack, not of the tab row');
  sent.length = 0;
  deliver(stacked(4));
  wheelRow(100);
  assert.deepEqual(sent.filter((m) => m.t === 'tab'), [], 'the stack’s last tab stops there (tab 5 is outside it)');
  deliver(stacked(3));
  deliver(opts({ stackRow: 'regular', wheel: false }));
  sent.length = 0;
  wheelRow(100);
  assert.deepEqual(sent.filter((m) => m.t === 'tab'), [], 'off: no switching');
  // Compact second row: the same tabs, a smaller row.
  deliver(opts({ stackRow: 'compact' }));
  assert.equal(row.dataset.size, 'compact');
  assert.equal(document.body.dataset.stackRow, 'compact');
  // The tab in use leaves the stack: no second row, and the stack is a ball like any other.
  deliver(stacked(5));
  assert.equal(row.hidden, true, 'a tab in no stack shows no second row');
  assert.equal(row.children.length, 0);
  assert.equal(document.body.dataset.stackRow, undefined);
  assert.equal(ball.getAttribute('aria-expanded'), 'false');
  assert.equal(tabNode(3).hidden, true, 'the stack is folded in the tab row');
  // Back to no second row at all.
  deliver(opts({ stackRow: 'off' }));
  deliver(stacked(3));
  assert.equal(row.hidden, true);
  assert.equal(tabNode(3).parent.classList.contains('group'), true);
  deliver(tabsMsg([item(1, { pinned: true }), item(2), item(3)], 2));
});

// ---- the hover card ----

const hover = await bundle('hover.ts');
const preview = fixture('hover-preview-messages.json');

test('the hover card shows a thumbnail and memory only when Rust sent them', () => {
  const withBoth = protocol.parseInbound(JSON.stringify(preview.card));
  assert.equal(withBoth?.kind, 'card');
  assert.match(withBoth.thumb, /^data:image\/jpeg;base64,/);
  assert.equal(withBoth.mem, 412);
  const memoryOnly = protocol.parseInbound(JSON.stringify(preview.memory));
  assert.equal(memoryOnly.thumb, undefined);
  assert.equal(memoryOnly.mem, 88);
  const plain = protocol.parseInbound(JSON.stringify(fixture('hover-messages.json').card));
  assert.ok(!('thumb' in plain) && !('mem' in plain), 'a card without them reads as before');
  for (const bad of [
    { ...preview.card, thumb: 'data:image/png;base64,AAAA' },
    { ...preview.card, thumb: 'javascript:alert(1)' },
    { ...preview.card, thumb: `data:image/jpeg;base64,${'A'.repeat(protocol.MAX_THUMB_URI)}` },
    { ...preview.card, thumb: 'data:image/jpeg;base64,not base64!' },
    { ...preview.card, mem: -1 },
    { ...preview.card, mem: 100000 },
    { ...preview.card, mem: 1.5 },
  ]) assert.equal(protocol.parseInbound(JSON.stringify(bad)), null, JSON.stringify(bad).slice(0, 80));
  assert.equal(hover.memoryText(41), '41 MB');
  assert.equal(hover.memoryText(999), '999 MB');
  assert.equal(hover.memoryText(1536), '1.5 GB');
});
