// Theme options (src/theme_custom.rs, ui/src/theme-options-logic.ts, ui/src/settings-themes.ts): the option
// table against the Rust table and its snapshot fixture, the strict snapshot and command contracts, and the
// rows in a fake document: what each control sends over the bridge, what a snapshot does to it, and where the
// rows sit in the slimmed Appearance area.
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
const options = await bundle('theme-options-logic.ts');
const logic = await bundle('settings-page-logic.ts');
const fixture = JSON.parse(read(join(here, 'fixtures', 'settings-messages.json')));
const rustFile = read(join(root, 'src', 'theme_custom.rs'));

// ---- the option table ----

test('every option the page lists is one Rust stores, in Rust order, with the same defaults', () => {
  const rustKeys = [...rustFile.slice(rustFile.indexOf('pub const KEYS')).split('];')[0].matchAll(/"(theme_[a-z_]+)"/g)].map((m) => m[1]);
  assert.equal(rustKeys.length, 11);
  assert.deepEqual([...options.THEME_OPTION_KEYS], rustKeys);
  assert.equal(new Set(options.THEME_OPTION_KEYS).size, options.THEME_OPTION_KEYS.length);
  // The Rust snapshot carries exactly these options and their defaults.
  assert.deepEqual(options.parseThemeOptions(fixture.snap.d.themeOptions), options.THEME_OPTION_DEFAULTS, 'Rust defaults and the page defaults agree');
  assert.ok(options.parseThemeOptions(fixture.guest.d.themeOptions));
  assert.deepEqual(Object.keys(fixture.snap.d.themeOptions), rustKeys, 'the snapshot lists them in the same order');
});

test('the choices the page offers are exactly the words Rust parses', () => {
  const rustChoices = (name) => {
    const block = rustFile.match(new RegExp(`${name} \\{([^}]*)\\}`))?.[1] ?? '';
    return [...block.matchAll(/= "([a-z_]+)"/g)].map((m) => m[1]);
  };
  assert.deepEqual(options.SCHEDULES.map(([value]) => value).sort(), rustChoices('Schedule').sort());
  assert.deepEqual(options.COLOURINGS.map(([value]) => value).sort(), rustChoices('Colouring').sort());
  assert.deepEqual(options.CONTRASTS.map(([value]) => value).sort(), rustChoices('Contrast').sort());
  assert.deepEqual(options.LOOKS.map(([value]) => value), ['light', 'dark']);
});

test('the option snapshot is refused unless it is whole and every value is legal', () => {
  const good = { ...fixture.snap.d.themeOptions };
  assert.ok(options.parseThemeOptions(good));
  for (const key of Object.keys(good)) {
    const missing = { ...good };
    delete missing[key];
    assert.equal(options.parseThemeOptions(missing), null, `missing ${key}`);
  }
  assert.equal(options.parseThemeOptions({ ...good, theme_extra: 'x' }), null);
  assert.equal(options.parseThemeOptions(null), null);
  assert.equal(options.parseThemeOptions([]), null);
  for (const [key, value] of [
    ['theme_schedule', 'weekly'], ['theme_look', 'grey'], ['theme_light_from', '7:00'], ['theme_light_from', '24:00'], ['theme_dark_from', '12:60'],
    ['theme_background', 'red'], ['theme_foreground', '#12345'], ['theme_highlight', '#gggggg'], ['theme_colouring', 'rainbow'],
    ['theme_accent_page', 'true'], ['theme_saturation', 101], ['theme_saturation', 4.5], ['theme_saturation', '50'], ['theme_contrast', 'max'],
  ]) assert.equal(options.parseThemeOptions({ ...good, [key]: value }), null, `${key}=${value}`);
  assert.equal(options.parseThemeOptions({ ...good, theme_dark_from: good.theme_light_from }), null, 'the two times never meet');
  assert.ok(options.parseThemeOptions({ ...good, theme_background: '#A0b0C0', theme_saturation: 0, theme_accent_page: true }));
});

test('a change becomes one key=value text, and anything Rust would refuse never leaves the page', () => {
  assert.equal(options.themeAssignment('theme_schedule', 'time'), 'theme_schedule=time');
  assert.equal(options.themeAssignment('theme_background', ''), 'theme_background=');
  assert.equal(options.themeAssignment('theme_highlight', '#ffcc00'), 'theme_highlight=#ffcc00');
  assert.equal(options.themeAssignment('theme_saturation', 40), 'theme_saturation=40');
  assert.equal(options.themeAssignment('theme_accent_page', true), 'theme_accent_page=true');
  for (const [key, value] of [['theme_schedule', 'x'], ['theme_background', 'red'], ['theme_saturation', 101], ['theme_light_from', '7:00'], ['theme_nothing', 1], ['theme', 'system'], ['theme_accent', '#112233']]) {
    assert.equal(options.themeAssignment(key, value), null, `${key}=${value}`);
  }
});

test('the rows cover every option once, are searchable, and only the schedule is in the main view', () => {
  const covered = Object.values(options.THEME_ROW_KEYS).flat();
  assert.deepEqual([...covered].sort(), [...options.THEME_OPTION_KEYS].sort(), 'every option has a row and no row names another');
  for (const row of options.THEME_ROWS) {
    assert.ok(row.label !== '' && row.description === '' && row.keywords !== '', row.id);
    assert.doesNotMatch(row.description, /restart/i, `${row.id} acts at once`);
    assert.ok(logic.ROWS.some((r) => r.id === row.id), `${row.id} is in the page's index`);
  }
  assert.deepEqual(options.THEME_ROWS.map((row) => row.id), [...options.THEME_ROW_IDS]);
  const tiers = Object.fromEntries(options.THEME_ROWS.map((row) => [row.id, logic.rowTier(logic.ROWS.find((r) => r.id === row.id))]));
  assert.deepEqual(Object.entries(tiers).filter(([, tier]) => tier === 'core').map(([id]) => id), ['themeSchedule'], 'light or dark is the one theme row in the main view');
  assert.ok(Object.entries(tiers).filter(([id]) => id !== 'themeSchedule').every(([, tier]) => tier === 'more'), JSON.stringify(tiers));
  assert.ok(logic.matchRows('night').has('themeSchedule'));
  assert.ok(logic.matchRows('saturation').has('themeSaturation'));
  assert.ok(logic.matchRows('export theme').has('themeFile'));
  assert.ok(logic.matchRows('background colour').has('themeColours'));
  assert.ok(logic.matchRows('theme-color').has('themeAccentPage'));
  assert.ok(logic.matchRows('prefer system colour').has('theme'));
});

test('the schedule note and the rows that only matter for one schedule follow the values', () => {
  const base = options.THEME_OPTION_DEFAULTS;
  assert.equal(options.scheduleNote(base), 'Follows the Windows app mode.');
  assert.equal(options.scheduleNote({ ...base, theme_schedule: 'none', theme_look: 'dark' }), 'Always dark.');
  assert.equal(options.scheduleNote({ ...base, theme_schedule: 'none' }), 'Always light.');
  assert.equal(options.scheduleNote({ ...base, theme_schedule: 'time', theme_light_from: '06:30', theme_dark_from: '21:00' }), 'Light from 06:30, dark from 21:00.');
  assert.deepEqual([options.lookApplies(base), options.timesApply(base)], [false, false]);
  assert.deepEqual([options.lookApplies({ ...base, theme_schedule: 'none' }), options.timesApply({ ...base, theme_schedule: 'none' })], [true, false]);
  assert.deepEqual([options.lookApplies({ ...base, theme_schedule: 'time' }), options.timesApply({ ...base, theme_schedule: 'time' })], [false, true]);
  assert.equal(options.clockMinutes('07:30'), 450);
  assert.equal(options.clockMinutes('7:30'), null);
});

test('the settings page reads the options in the snapshot and refuses a snapshot without them', () => {
  const snap = logic.parseSettingsSnapshot(fixture.snap.d);
  assert.deepEqual(snap.themeOptions, options.THEME_OPTION_DEFAULTS);
  const { themeOptions, ...without } = fixture.snap.d;
  assert.equal(logic.parseSettingsSnapshot(without), null);
  assert.equal(logic.parseSettingsSnapshot({ ...fixture.snap.d, themeOptions: { ...themeOptions, theme_contrast: 'x' } }), null);
  // The theme options are their own commands: never part of the page's save text.
  assert.doesNotMatch(logic.saveText(logic.draftOf(snap)), /theme_(schedule|background|colouring|contrast|saturation)/);
});

// ---- the rows in a fake document ----

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
  before(node) { const p = this.parent; p.detach(node); p.children.splice(p.children.indexOf(this), 0, node); }
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
const stored = new Map([['bowser.settings.more', JSON.stringify(['privacy'])], ['bowser.settings.view', 'all']]);
globalThis.localStorage = { getItem: (k) => stored.get(k) ?? null, setItem: (k, v) => stored.set(k, String(v)), removeItem: (k) => stored.delete(k) };
globalThis.Element = Node;
globalThis.HTMLElement = Node;
globalThis.HTMLInputElement = class {};

const { startSettingsPage } = await bundle('settings-page.ts');
const app = new Node('div');
startSettingsPage(app);
const deliver = (msg) => { for (const cb of listeners) cb({ data: JSON.stringify(msg) }); };
const defaults = options.THEME_OPTION_DEFAULTS;
const snapshotWith = (extra = {}, more = {}) => ({ ...fixture.snap, d: { ...fixture.snap.d, ...more, themeOptions: { ...defaults, ...extra } } });
deliver(snapshotWith());
const rowNode = (id) => app.querySelector(`[data-row="${id}"]`);
const radios = (id) => rowNode(id).querySelectorAll('[role="radio"]');
const checked = (id) => radios(id).find((r) => r.getAttribute('aria-checked') === 'true')?.textContent;
const themeCommands = () => sent.splice(0).filter((m) => m.t === 'set');
const searchFor = (text) => { const input = app.querySelector('.sp-search-input'); input.value = text; input.dispatch('input'); };
const areaNode = app.querySelector('section[data-area="appearance"]');
const more = areaNode.querySelector('.sp-more');

test('the theme rows are drawn once each, labelled, with their own control', () => {
  for (const spec of options.THEME_ROWS) {
    const row = rowNode(spec.id);
    assert.ok(row, spec.id);
    assert.equal(app.querySelectorAll(`[data-row="${spec.id}"]`).length, 1, `${spec.id} once`);
    assert.equal(row.querySelector('.sp-label').textContent, spec.label);
    assert.equal(row.querySelector('.sp-desc'), null, spec.id);
    assert.equal(row.querySelector('.sp-flag'), null, `${spec.id} acts now: no "no effect yet" note`);
  }
  for (const id of ['themeSchedule', 'themeLook', 'themeColouring', 'themeContrast']) assert.ok(rowNode(id).querySelector('[role="radiogroup"]'), id);
  assert.ok(rowNode('themeAccentPage').querySelector('[role="switch"]'));
  assert.equal(rowNode('themeSaturation').querySelector('input').type, 'range');
  assert.equal(rowNode('themeTimes').querySelectorAll('input').length, 2);
  assert.equal(rowNode('themeColours').querySelectorAll('input').length, 3);
  assert.deepEqual(rowNode('themeFile').querySelectorAll('button').map((b) => b.textContent).filter((t) => t !== 'Cancel'), ['Export…', 'Import…', 'Reset']);
});

test('every theme row sits behind More options; the main view keeps its ten rows', () => {
  // --- final --- the schedule joined Appearance's main view on its own branch; with the layout
  // presets and the auto-hide switch merged, Appearance keeps the ten core rows PR #34 had.
  for (const id of options.THEME_ROW_IDS) {
    if (id === 'themeSchedule') assert.ok(!more.contains(rowNode(id)), 'light or dark stays in the main view');
    else assert.ok(more.contains(rowNode(id)), `${id} is behind More options`);
  }
  const core = areaNode.querySelectorAll('.sp-row').filter((row) => !more.contains(row)).map((row) => row.dataset.row).filter((id) => typeof id === 'string');
  assert.deepEqual(core.filter((id) => id.startsWith('theme')), ['theme', 'themeAccent', 'themeSchedule']);
  assert.ok(core.length <= 10, `${core.length} core rows in Appearance`);
  // Search finds a hidden row and opens the disclosure it is in.
  searchFor('saturation');
  assert.equal(rowNode('themeSaturation').hidden, false);
  assert.equal(more.querySelector('.sp-more-body').hidden, false, 'the disclosure opens for a hit');
  searchFor('');
});

test('each control sends one themeopt command with its key=value and nothing else', () => {
  sent.length = 0;
  radios('themeSchedule')[2].click();
  assert.deepEqual(themeCommands(), [{ v: 1, t: 'set', a: 'themeopt', text: 'theme_schedule=time' }]);
  radios('themeSchedule')[1].click();
  assert.deepEqual(themeCommands(), [{ v: 1, t: 'set', a: 'themeopt', text: 'theme_schedule=none' }]);
  deliver(snapshotWith({ theme_schedule: 'none' }));
  assert.deepEqual(radios('themeLook').map((r) => r.textContent), ['Light', 'Dark']);
  radios('themeLook')[1].click();
  assert.deepEqual(themeCommands(), [{ v: 1, t: 'set', a: 'themeopt', text: 'theme_look=dark' }]);
  assert.deepEqual(radios('themeColouring').map((r) => r.textContent), ['Off', 'Tabs', 'Address', 'Both']);
  for (const [index, value] of ['off', 'tab_bar', 'address_bar', 'unified'].entries()) {
    if (index === 0) deliver(snapshotWith({ theme_colouring: 'tab_bar' })); // so that Off is a change
    radios('themeColouring')[index].click();
    assert.deepEqual(themeCommands(), [{ v: 1, t: 'set', a: 'themeopt', text: `theme_colouring=${value}` }], value);
  }
  assert.deepEqual(radios('themeContrast').map((r) => r.textContent), ['Standard', 'High']);
  radios('themeContrast')[1].click();
  assert.deepEqual(themeCommands(), [{ v: 1, t: 'set', a: 'themeopt', text: 'theme_contrast=high' }]);
  deliver(snapshotWith());
  const page = rowNode('themeAccentPage').querySelector('[role="switch"]');
  assert.equal(page.getAttribute('aria-checked'), 'false');
  page.click();
  assert.deepEqual(themeCommands(), [{ v: 1, t: 'set', a: 'themeopt', text: 'theme_accent_page=true' }]);
  const slider = rowNode('themeSaturation').querySelector('input');
  slider.value = '35';
  slider.dispatch('input');
  assert.deepEqual(sent, [], 'dragging only repaints');
  assert.equal(rowNode('themeSaturation').querySelector('output').textContent, '35%');
  slider.dispatch('change');
  assert.deepEqual(themeCommands(), [{ v: 1, t: 'set', a: 'themeopt', text: 'theme_saturation=35' }]);
  assert.ok(!sent.some((m) => m.a === 'save'), 'none of these is part of save');
});

test('the colours send a lower-case hex, and Default sends nothing back to Bowser’s own', () => {
  sent.length = 0;
  const [background, foreground, highlight] = rowNode('themeColours').querySelectorAll('input');
  background.value = '#A0B0C0';
  background.dispatch('change');
  assert.deepEqual(themeCommands(), [{ v: 1, t: 'set', a: 'themeopt', text: 'theme_background=#a0b0c0' }]);
  foreground.value = '#101010';
  foreground.dispatch('change');
  highlight.value = '#ffcc00';
  highlight.dispatch('change');
  assert.deepEqual(themeCommands().map((m) => m.text), ['theme_foreground=#101010', 'theme_highlight=#ffcc00']);
  const defaultButtons = rowNode('themeColours').querySelectorAll('button');
  assert.ok(defaultButtons.every((b) => b.disabled), 'nothing chosen: nothing to put back');
  deliver(snapshotWith({ theme_background: '#a0b0c0' }));
  assert.equal(background.value, '#a0b0c0');
  assert.deepEqual(defaultButtons.map((b) => b.disabled), [false, true, true]);
  defaultButtons[0].click();
  assert.deepEqual(themeCommands(), [{ v: 1, t: 'set', a: 'themeopt', text: 'theme_background=' }]);
  deliver(snapshotWith());
  assert.equal(rowNode('themeColours').querySelectorAll('.sp-theme-hex')[0].textContent, 'Default');
});

test('the times are sent when they are good, and put back when they are not', () => {
  deliver(snapshotWith({ theme_schedule: 'time' }));
  const [light, dark] = rowNode('themeTimes').querySelectorAll('input');
  assert.equal(light.value, '07:00');
  assert.equal(dark.value, '19:00');
  assert.ok(!light.disabled && !dark.disabled, 'by time: the times are live');
  sent.length = 0;
  light.value = '06:30';
  light.dispatch('change');
  assert.deepEqual(themeCommands(), [{ v: 1, t: 'set', a: 'themeopt', text: 'theme_light_from=06:30' }]);
  dark.value = '07:00'; // equal to the light time in the snapshot
  dark.dispatch('change');
  assert.deepEqual(themeCommands(), [], 'two equal times are never sent');
  assert.equal(dark.value, '19:00', 'and the field goes back');
  light.value = '';
  light.dispatch('change');
  assert.deepEqual(themeCommands(), []);
  assert.equal(light.value, '07:00');
  deliver(snapshotWith());
  assert.ok(light.disabled && dark.disabled, 'follow Windows: the times are not used');
  assert.ok(radios('themeLook').every((r) => r.disabled), 'and neither is the fixed look');
});

test('the schedule row says what it means now, and the next snapshot is the truth for every control', () => {
  deliver(snapshotWith());
  assert.equal(rowNode('themeSchedule').querySelector('.sp-schedule-note'), null);
  assert.equal(checked('themeSchedule'), 'System');
  deliver(snapshotWith({
    theme_schedule: 'time', theme_look: 'dark', theme_light_from: '06:15', theme_dark_from: '21:45', theme_background: '#102030', theme_foreground: '#eeeeee', theme_highlight: '#336699',
    theme_colouring: 'unified', theme_accent_page: true, theme_saturation: 45, theme_contrast: 'high',
  }));
  assert.equal(checked('themeSchedule'), 'Timed');
  assert.equal(checked('themeLook'), 'Dark');
  assert.equal(checked('themeColouring'), 'Both');
  assert.equal(checked('themeContrast'), 'High');
  assert.equal(rowNode('themeAccentPage').querySelector('[role="switch"]').getAttribute('aria-checked'), 'true');
  assert.equal(rowNode('themeSaturation').querySelector('input').value, '45');
  assert.equal(rowNode('themeSaturation').querySelector('output').textContent, '45%');
  assert.deepEqual(rowNode('themeTimes').querySelectorAll('input').map((i) => i.value), ['06:15', '21:45']);
  assert.deepEqual(rowNode('themeColours').querySelectorAll('input').map((i) => i.value), ['#102030', '#eeeeee', '#336699']);
  assert.equal(rowNode('themeColours').querySelectorAll('.sp-theme-hex')[1].textContent, '#eeeeee');
  // An unreadable settings file locks every theme control.
  deliver(snapshotWith({}, { readable: false }));
  for (const id of options.THEME_ROW_IDS) assert.ok(rowNode(id).querySelectorAll('input, button').every((c) => c.disabled), `${id} locked`);
  deliver(snapshotWith());
  assert.equal(radios('themeSchedule').every((r) => !r.disabled), true);
});

test('the theme file row opens Windows’ own dialogs by a bare command and resets only after a second click', () => {
  deliver(snapshotWith());
  sent.length = 0;
  const [exportButton, importButton, , reset] = rowNode('themeFile').querySelectorAll('button');
  exportButton.click();
  assert.deepEqual(themeCommands(), [{ v: 1, t: 'set', a: 'themeexport' }], 'no path from the page');
  importButton.click();
  assert.deepEqual(themeCommands(), [{ v: 1, t: 'set', a: 'themeimport' }]);
  reset.click();
  assert.deepEqual(themeCommands(), [], 'the first click only asks');
  assert.equal(reset.textContent, 'Confirm');
  const cancel = rowNode('themeFile').querySelectorAll('button')[2];
  assert.equal(cancel.hidden, false);
  cancel.click();
  assert.equal(reset.textContent, 'Reset');
  assert.deepEqual(themeCommands(), []);
  reset.click();
  reset.click();
  assert.deepEqual(themeCommands(), [{ v: 1, t: 'set', a: 'themereset' }]);
  assert.equal(reset.textContent, 'Reset', 'and the row is back to normal');
});

test('the theme rows touch no network or page API and add no inline styles beyond the slider fill', () => {
  const text = read(join(srcDir, 'settings-themes.ts')) + read(join(srcDir, 'theme-options-logic.ts'));
  for (const banned of [/fetch\(/, /XMLHttpRequest/, /WebSocket/, /innerHTML/, /eval\(/, /new Function/, /location\./, /window\.open/]) assert.doesNotMatch(text, banned);
  assert.deepEqual([...text.matchAll(/style\.setProperty\('([^']+)'/g)].map((m) => m[1]), ['--sp-fill']);
});

test('Buttons instead of sliders also controls theme saturation and bounded step commands', () => {
  const window = { ...fixture.snap.d.window, rangeButtons: true };
  deliver(snapshotWith({ theme_saturation: 95 }, { window }));
  const row = rowNode('themeSaturation');
  const wrap = row.querySelector('.sp-range-wrap');
  assert.ok(wrap.classList.contains('is-steps'));
  const [less, more] = row.querySelector('.sp-stepper').querySelectorAll('button');
  assert.equal(more.getAttribute('aria-label'), 'Increase accent saturation');
  sent.length = 0;
  more.click();
  assert.deepEqual(themeCommands(), [{ v: 1, t: 'set', a: 'themeopt', text: 'theme_saturation=100' }]);
  more.click();
  assert.deepEqual(themeCommands(), [{ v: 1, t: 'set', a: 'themeopt', text: 'theme_saturation=100' }]);
  deliver(snapshotWith({ theme_saturation: 0 }, { window }));
  less.click();
  assert.deepEqual(themeCommands(), [{ v: 1, t: 'set', a: 'themeopt', text: 'theme_saturation=0' }]);
  deliver(snapshotWith({}, { window: { ...window, rangeButtons: false } }));
  assert.ok(!wrap.classList.contains('is-steps'));
  deliver(snapshotWith({}, { readable: false, window }));
  assert.ok(less.disabled && more.disabled);
  deliver(snapshotWith());
});
