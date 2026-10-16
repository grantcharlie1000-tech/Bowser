// Quick Commands (the command palette page): the strict payload contract against the Rust fixture, filter
// prefixes, result types and their order, the visible-result limit, the calculator, inline completion,
// the cheat sheet mode, and the page itself in a fake document (typing, Right arrow and Shift completion,
// copying a calculation, keeping the last typed value, picking). Search results are checked against the
// same cases Rust's native fallback is (test/fixtures/palette-cases.txt), so the two cannot drift.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';
import { FakeDocument, Node } from './fake-dom.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, '..', 'src');
async function bundle(file) {
  const out = esbuild.buildSync({ entryPoints: [join(srcDir, file)], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
}
const logic = await bundle('palette-logic.ts');
const calc = await bundle('calc.ts');
const fixture = JSON.parse(readFileSync(join(here, 'fixtures', 'popup-pages.json'), 'utf8'));
const read = (path) => readFileSync(path, 'utf8').replace(/\r\n/g, '\n');

// ---- shared cases ----

function sharedCases() {
  const items = [];
  const cases = [];
  for (const line of read(join(here, 'fixtures', 'palette-cases.txt')).split('\n')) {
    if (line.startsWith('#') || line.trim() === '') continue;
    const parts = line.split('\t');
    if (parts[0] === 'item') items.push({ id: items.length, s: parts[1], label: parts[2], detail: parts[3], action: parts[4] });
    else if (parts[0] === 'case') cases.push({ query: parts[1], types: parts[2], limit: Number(parts[3]), expected: parts[4] ? parts[4].split('|') : [] });
  }
  return { items, cases };
}

/** Rust's `Options::palette_ranked`: the enabled types keep the default relative order. */
const rankedFor = (types) => {
  let last = -1;
  for (const type of types) {
    const at = logic.DEFAULT_TYPES.indexOf(type);
    if (at < last) return false;
    last = at;
  }
  return true;
};

test('the page filters exactly as the native fallback does, case for case', () => {
  const { items, cases } = sharedCases();
  assert.ok(cases.length >= 30, `${cases.length} cases`);
  for (const { query, types, limit, expected } of cases) {
    const shown = types === '*' ? [...logic.DEFAULT_TYPES] : types.split(',');
    const groups = logic.filterPalette(items, query, { types: shown, ranked: rankedFor(shown), limit });
    const actions = logic.flatten(groups).map((item) => (item.id === logic.CALC_ID ? `copy:${item.label}` : items[item.id].action));
    assert.deepEqual(actions, expected, `query ${JSON.stringify(query)} types ${types} limit ${limit}`);
  }
});

// ---- contract ----

test('the Rust palette page carries its options and passes the strict contract', () => {
  const page = fixture.palette;
  assert.equal(logic.validPalettePayload(page), true);
  assert.equal(page.mode, 'palette');
  assert.deepEqual(page.types, ['tab', 'closed', 'workspace', 'command', 'action', 'bookmark', 'note', 'history', 'calc']);
  assert.equal(page.ranked, true);
  assert.equal(page.limit, 12);
  assert.equal(page.keep, false);
  const sections = [...new Set(page.items.map((item) => item.s))];
  assert.deepEqual(sections, ['tab', 'closed', 'workspace', 'command', 'action', 'bookmark', 'note', 'history']);
  const reload = page.items.find((item) => item.label === 'Reload');
  assert.deepEqual([reload.s, reload.acc], ['action', 'F5'], 'page actions are their own type and carry the live shortcut');
  assert.equal(page.items.find((item) => item.label === 'Shopping').detail, 'milk and tea');
  assert.equal(page.items.find((item) => item.s === 'workspace').detail, '2 tabs');
  const bad = (patch) => logic.validPalettePayload({ ...page, ...patch });
  for (const patch of [
    { mode: 'other' }, { mode: undefined }, { types: ['tab', 'tab'] }, { types: ['tab', 'bogus'] }, { types: 'tab' }, { ranked: 1 }, { limit: -1 }, { limit: 31 },
    { limit: 1.5 }, { keep: 'yes' }, { extra: 1 },
    { items: [{ id: 1, s: 'calc', label: '4', detail: '' }] },
    { items: [{ id: 1, s: 'tab', label: 'x', detail: '', g: 'x'.repeat(65) }] },
    { items: [{ id: 1, s: 'tab', label: 'x', detail: '', acc: 'x'.repeat(97) }] },
    { items: [{ id: 1, s: 'tab', label: 'x', detail: '', onclick: 'x' }] },
  ]) assert.equal(bad(patch), false, JSON.stringify(patch).slice(0, 60));
});

// ---- prefixes, completion, calculator ----

test('prefixes and their short forms, and what a prefix leaves of the text', () => {
  const cases = [
    ['bookmark: tea', ['bookmark'], 'tea'], ['bkm:tea', ['bookmark'], 'tea'], ['BKM: tea', ['bookmark'], 'tea'], ['note: milk', ['note'], 'milk'],
    ['tab: set', ['tab', 'closed'], 'set'], ['tb: set', ['tab', 'closed'], 'set'], ['history:news', ['history'], 'news'], ['command: set', ['command'], 'set'],
    ['cmd: set', ['command'], 'set'], ['action: rel', ['action'], 'rel'], ['workspace: wo', ['workspace'], 'wo'], ['  tb:', ['tab', 'closed'], ''],
  ];
  for (const [query, kinds, text] of cases) assert.deepEqual(logic.parsePrefix(query), { kinds, text }, query);
  for (const plain of ['settings', 'foo: bar', 'https://example.com', '12:30', 'calc: 1+1', ':tab', 'constructor: x', '__proto__: x']) {
    assert.deepEqual(logic.parsePrefix(plain), { kinds: null, text: plain }, plain);
  }
});

test('Shift or Right arrow complete the top result after any prefix, keeping what was typed', () => {
  assert.equal(logic.completion('goo', 'Google'), 'Google');
  assert.equal(logic.completion('tab: goo', 'Google'), 'tab: Google');
  assert.equal(logic.completion('TB:   goo', 'Google'), 'TB:   Google');
  assert.equal(logic.completion('GOO', 'google'), 'google', 'case is ignored, the label is what is put in');
  for (const none of [['google', 'Google'], ['gx', 'Google'], ['', 'Google'], ['tab:', 'Google']]) assert.equal(logic.completion(...none), null, none.join(' / '));
});

test('the calculator gives the same answers as Rust for every shared case', () => {
  const lines = read(join(here, 'fixtures', 'calc-cases.json')).split('\n');
  const cases = JSON.parse(lines.join('\n')).cases;
  assert.ok(cases.length >= 30);
  for (const [expr, result] of cases) assert.equal(calc.resultText(expr), result, expr);
  assert.equal(calc.resultText('round(-2.5)'), '-3', 'halves round away from zero, as Rust does');
  assert.equal(calc.resultText('round(2.5)'), '3');
  assert.equal(calc.resultText('-2^2'), '-4');
  assert.equal(calc.resultText('2^3^2'), '512');
  assert.equal(calc.resultText('('.repeat(80)), null);
});

test('the cheat sheet groups by heading and filters on every word of label, group, description and shortcuts', () => {
  const items = [
    { id: 0, s: 'command', label: 'New tab', detail: 'Open a blank tab', acc: 'Ctrl+T', g: 'Tabs' },
    { id: 1, s: 'command', label: 'Close tab', detail: 'Close the current tab', acc: 'Ctrl+W   Ctrl+F4', g: 'Tabs' },
    { id: 2, s: 'command', label: 'Reload', detail: 'Reload the current page', acc: 'F5   Ctrl+R', g: 'Navigation' },
  ];
  assert.deepEqual(logic.filterShortcuts(items, '').map((g) => [g.label, g.items.length]), [['Tabs', 2], ['Navigation', 1]]);
  assert.deepEqual(logic.flatten(logic.filterShortcuts(items, 'ctrl+r')).map((i) => i.id), [2]);
  assert.deepEqual(logic.flatten(logic.filterShortcuts(items, 'tabs ctrl')).map((i) => i.id), [0, 1], 'the group name matches too');
  assert.deepEqual(logic.filterShortcuts(items, 'zzz'), []);
  const sheet = { ...fixture.palette, mode: 'shortcuts', types: [], limit: 0, items: [{ id: 0, s: 'command', label: 'New tab', detail: 'x', acc: 'Ctrl+T', g: 'Tabs' }] };
  assert.equal(logic.validPalettePayload(sheet), true);
});

// ---- the page in a fake document ----

const sent = [];
const listeners = [];
const doc = new FakeDocument();
globalThis.document = doc;
globalThis.window = globalThis;
Object.assign(globalThis, {
  requestAnimationFrame: (cb) => { cb(); return 1; },
  chrome: { webview: { postMessage: (wire) => sent.push(JSON.parse(wire)), addEventListener: (_t, cb) => listeners.push(cb) } },
});
globalThis.Element = Node;
globalThis.HTMLElement = Node;
let clipboard = null;
let clipboardWorks = true;
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { clipboard: { writeText: async (text) => { if (!clipboardWorks) throw new Error('denied'); clipboard = text; } } },
});
const { createPaletteView } = await bundle('palette.ts');

const items = [
  { id: 0, s: 'tab', label: 'Settings guide', detail: 'https://docs.example/' },
  { id: 1, s: 'command', label: 'Settings', detail: 'Open browser settings', acc: 'Ctrl+,' },
  { id: 2, s: 'bookmark', label: 'Sweet tea', detail: 'https://tea.example/' },
  { id: 3, s: 'note', label: 'Shopping', detail: 'milk and tea' },
  { id: 4, s: 'history', label: 'News', detail: 'https://set.example/' },
];
const message = (patch = {}) => ({
  v: 1, t: 'palette', tok: 7, kind: 'palette', placeholder: 'Search', mode: 'palette', types: [...logic.DEFAULT_TYPES], ranked: true, limit: 12, keep: false, items, ...patch,
});
const root = new Node('div');
const view = createPaletteView(root);
const page = () => root.children[0];
const input = () => page().querySelector('.bw-palette-input');
const rows = () => page().querySelectorAll('.bw-palette-row');
const labels = () => rows().map((r) => r.querySelector('.bw-palette-label').textContent);
const headings = () => page().querySelectorAll('.bw-palette-section').map((h) => h.textContent);
const type = (text) => { input().value = text; input().selectionStart = text.length; input().selectionEnd = text.length; input().dispatch('input'); };
const key = (name, extra = {}) => { const event = { key: name, isComposing: false, repeat: false, stopPropagation() {}, ...extra }; page().dispatch('keydown', event); return event; };
const keyUp = (name) => page().dispatch('keyup', { key: name, stopPropagation() {} });
const settle = () => new Promise((resolve) => setImmediate(resolve));

test('the page lists tabs and commands for an empty search, and each section has its own heading', () => {
  assert.equal(view.show(message()), true);
  assert.deepEqual(headings(), ['Open tabs', 'Commands']);
  assert.deepEqual(labels(), ['Settings guide', 'Settings']);
  assert.equal(rows()[1].querySelector('.bw-palette-accel').textContent, 'Ctrl+,');
  assert.equal(view.show({ ...message(), mode: 'nope' }), false, 'an invalid message changes nothing');
  assert.equal(rows().length, 2);
  type('tea');
  assert.deepEqual(headings(), ['Bookmarks', 'Notes', 'History', 'Open tabs'], 'every type that matches, the best match first');
  type('note: tea');
  assert.deepEqual(labels(), ['Shopping']);
  type('bkm: tea');
  assert.deepEqual(labels(), ['Sweet tea']);
  type('');
  assert.deepEqual(labels(), ['Settings guide', 'Settings']);
});

test('result types and their order, and the visible-result limit, come from the message', () => {
  view.show(message({ types: ['history', 'command', 'tab'], ranked: false }));
  type('set');
  assert.deepEqual(headings(), ['History', 'Commands', 'Open tabs'], 'the user\'s order, not the best match\'s');
  view.show(message({ types: ['tab', 'command'] }));
  type('milk');
  assert.deepEqual(labels(), [], 'hidden types do not show');
  assert.equal(page().querySelector('.bw-palette-empty').textContent, 'No matches');
  const many = Array.from({ length: 20 }, (_, id) => ({ id, s: 'bookmark', label: `Page ${id}`, detail: '' }));
  view.show(message({ items: many, limit: 5 }));
  type('page');
  assert.equal(rows().length, 5);
  view.show(message({ items: many, limit: 30 }));
  type('page');
  assert.equal(rows().length, 20);
});

test('a sum shows a calculator row first; picking it copies the result and closes', async () => {
  view.show(message());
  type('2+2*3');
  assert.deepEqual(headings(), ['Calculator']);
  assert.equal(labels()[0], '8');
  assert.match(rows()[0].querySelector('.bw-palette-detail').textContent, /2\+2\*3 = 8\. Press Enter to copy\./);
  sent.length = 0;
  key('Enter');
  await settle();
  assert.equal(clipboard, '8');
  assert.deepEqual(sent, [{ v: 1, t: 'dismiss', tok: 7 }], 'copied, so the palette closes; no pick is sent');
  // When the clipboard is refused, the result goes into the box to copy by hand and the palette stays.
  view.show(message());
  type('10/4');
  sent.length = 0;
  clipboardWorks = false;
  doc.execCommandResult = false;
  rows()[0].dispatch('click');
  await settle();
  clipboardWorks = true;
  assert.equal(input().value, '2.5');
  assert.deepEqual(sent.filter((m) => m.t === 'dismiss' || m.t === 'pick'), []);
});

test('Right arrow at the end of the text, or Shift tapped on its own, completes to the top result', () => {
  view.show(message());
  type('sett');
  const ghost = page().querySelector('.bw-palette-ghost');
  assert.equal(ghost.hidden, false);
  assert.equal(page().querySelector('.bw-palette-ghost-typed').textContent, 'sett');
  assert.equal(page().querySelector('.bw-palette-ghost-rest').textContent, 'ings', 'the rest of the top label, muted');
  // Right arrow away from the end moves the caret as usual.
  input().selectionStart = 2;
  input().selectionEnd = 2;
  let event = key('ArrowRight');
  assert.equal(input().value, 'sett');
  assert.equal(event.defaultPrevented, undefined);
  input().selectionStart = 4;
  input().selectionEnd = 4;
  event = key('ArrowRight', { preventDefault() { this.defaultPrevented = true; } });
  assert.equal(event.defaultPrevented, true);
  assert.equal(input().value, 'Settings');
  assert.equal(ghost.hidden, true, 'nothing left to complete');
  // Shift tapped alone completes; Shift held for a capital letter does not.
  type('sett');
  key('Shift');
  key('S', { shiftKey: true });
  keyUp('S');
  keyUp('Shift');
  assert.equal(input().value, 'sett', 'a capital letter was typed with Shift, not a completion');
  key('Shift');
  keyUp('Shift');
  assert.equal(input().value, 'Settings');
  // A held Shift (auto-repeat) is not a tap, and after a prefix only the text after it completes.
  type('tab: sett');
  key('Shift', { repeat: true });
  keyUp('Shift');
  assert.equal(input().value, 'tab: sett');
  key('Shift');
  keyUp('Shift');
  assert.equal(input().value, 'tab: Settings guide');
});

test('Enter picks the highlighted row by its id; arrows move; Escape and a click outside dismiss', () => {
  view.show(message());
  type('set');
  sent.length = 0;
  key('ArrowDown');
  assert.equal(rows()[1].getAttribute('aria-selected'), 'true');
  key('Enter');
  assert.deepEqual(sent.at(-1), { v: 1, t: 'pick', tok: 7, id: Number(rows()[1].dataset.id) });
  view.show(message({ tok: 8 }));
  sent.length = 0;
  key('Escape');
  assert.deepEqual(sent, [{ v: 1, t: 'dismiss', tok: 8 }]);
  key('Escape');
  assert.equal(sent.length, 1, 'a second Escape does nothing: the palette is already closed');
});

test('the last typed text is kept only when asked, and comes back selected', () => {
  view.show(message({ keep: false }));
  type('settings');
  view.show(message({ keep: false }));
  assert.equal(input().value, '', 'off: opens empty');
  view.show(message({ keep: true }));
  assert.equal(input().value, 'settings', 'on: opens with the last search');
  assert.deepEqual([input().selectionStart, input().selectionEnd], [0, 8], 'selected, so the first key replaces it');
  assert.deepEqual(labels(), ['Settings', 'Settings guide'], 'and its results are already showing');
  type('tea');
  view.show(message({ keep: true }));
  assert.equal(input().value, 'tea');
});

test('the cheat sheet filters, groups, and nothing in it can be picked', () => {
  const sheet = {
    ...message(), mode: 'shortcuts', placeholder: 'Filter shortcuts', types: [], limit: 0,
    items: [
      { id: 0, s: 'command', label: 'New tab', detail: 'Open a blank tab', acc: 'Ctrl+T', g: 'Tabs' },
      { id: 1, s: 'command', label: 'Reload', detail: 'Reload the current page', acc: 'F5   Ctrl+R', g: 'Navigation' },
    ],
  };
  assert.equal(view.show(sheet), true);
  assert.equal(input().placeholder, 'Filter shortcuts');
  assert.deepEqual(headings(), ['Tabs', 'Navigation']);
  assert.equal(rows()[1].querySelector('.bw-palette-accel').textContent, 'F5   Ctrl+R');
  type('ctrl+r');
  assert.deepEqual(labels(), ['Reload']);
  assert.equal(page().querySelector('.bw-palette-ghost').hidden, true, 'no completion in the sheet');
  sent.length = 0;
  key('Enter');
  rows()[0].dispatch('click');
  assert.deepEqual(sent.filter((m) => m.t === 'pick'), []);
  key('Escape');
  assert.deepEqual(sent.filter((m) => m.t === 'dismiss'), [{ v: 1, t: 'dismiss', tok: 7 }]);
});
