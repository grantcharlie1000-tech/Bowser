// Tabs settings (src/tab_options.rs, ui/src/tab-options-logic.ts, tab-marks-logic.ts): the option table
// against the Rust table, the snapshot and command contracts against the Rust fixtures, the strip's
// marks, selection and wheel rules, the strip messages and the Sessions panel's wire.
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
const options = await bundle('tab-options-logic.ts');
const marks = await bundle('tab-marks-logic.ts');
const sessions = await bundle('panels/sessions-logic.ts');
const { parseInbound } = await bundle('protocol.ts');
const settingsFixture = JSON.parse(read(join(here, 'fixtures', 'settings-messages.json')));
const stripFixture = JSON.parse(read(join(here, 'fixtures', 'tab-options-messages.json')));
const sessionsFixture = JSON.parse(read(join(here, 'fixtures', 'sessions-messages.json')));

// ---- the option table ----

test('every option the page lists is one Rust stores, and the other way round', () => {
  const rust = read(join(root, 'src', 'tab_options.rs'));
  const table = rust.slice(rust.indexOf('tab_options! {'), rust.indexOf('// ---- virtual options'));
  const rustKeys = [...table.matchAll(/"(tabs_[a-z_]+)";/g)].map((m) => m[1]);
  assert.ok(rustKeys.length >= 30, `found ${rustKeys.length} Rust options`);
  // The related and stack flags only ever travel as part of the position, which Rust keeps in older settings.
  const expected = [...rustKeys.filter((key) => key !== 'tabs_new_tab_related' && key !== 'tabs_new_tab_stack'), options.POSITION_KEY].sort();
  assert.deepEqual([...options.TAB_OPTION_KEYS].sort(), expected);
  assert.equal(new Set(options.TAB_OPTION_KEYS).size, options.TAB_OPTION_KEYS.length, 'no key twice');
  for (const spec of options.TAB_OPTIONS) {
    assert.ok(options.TAB_CARDS.includes(spec.card), spec.key);
    assert.ok(spec.label !== '' && spec.description !== '' && spec.keywords !== '', spec.key);
    assert.doesNotMatch(spec.description, /restart/i, `${spec.key}: every Tabs option applies at once`);
  }
  for (const card of options.TAB_CARDS) assert.ok(options.cardSpecs(card).length > 0, `${card} has rows`);
});

test('the choices the page offers are exactly the words Rust parses', () => {
  const rust = read(join(root, 'src', 'tab_options.rs'));
  const choices = new Map();
  for (const m of rust.matchAll(/choice! \{[^{]*?(\w+) \{ ([^}]*) \}\s*\}/gs)) choices.set(m[1], [...m[2].matchAll(/= "([a-z-]+)"/g)].map((x) => x[1]));
  const byKey = {
    tabs_new_tab_page: 'NewTabPage', tabs_duplicate_position: 'DuplicatePosition', tabs_close_activation: 'CloseActivation', tabs_double_click: 'DoubleClick',
    tabs_load_indicator: 'LoadIndicator', tabs_mute_mode: 'MuteMode', tabs_pinned_close: 'PinnedClose',
    tabs_stack_style: 'StackStyle', // --- tab stacks (T10) ---
  };
  for (const [key, name] of Object.entries(byKey)) {
    const spec = options.TAB_OPTIONS.find((s) => s.key === key);
    assert.deepEqual(spec.choices.map(([value]) => value).sort(), [...choices.get(name)].sort(), key);
  }
  const position = options.TAB_OPTIONS.find((s) => s.key === options.POSITION_KEY);
  // --- tab stacks (T10) --- "as stack with related" is offered now that stacks exist (T1's waiting choice).
  assert.deepEqual(position.choices.map(([value]) => value), ['related', 'active', 'last', 'stack']);
});

test('the Rust snapshot carries every option, with the defaults the page names', () => {
  const rustOptions = settingsFixture.snap.d.tabOptions;
  assert.deepEqual(options.parseTabOptions(rustOptions), options.TAB_OPTION_DEFAULTS, 'Rust defaults and the page defaults agree');
  assert.equal(rustOptions.tabs_new_tab_position, 'active');
  assert.equal(rustOptions.tabs_keep_window, true);
  assert.equal(rustOptions.tabs_confirm_close, 0);
  const guest = options.parseTabOptions(settingsFixture.guest.d.tabOptions);
  assert.ok(guest);
});

test('the option snapshot is refused unless it is whole and every value is legal', () => {
  const good = { ...settingsFixture.snap.d.tabOptions };
  assert.ok(options.parseTabOptions(good));
  for (const key of Object.keys(good)) {
    const missing = { ...good };
    delete missing[key];
    assert.equal(options.parseTabOptions(missing), null, `missing ${key}`);
  }
  assert.equal(options.parseTabOptions({ ...good, extra: true }), null);
  assert.equal(options.parseTabOptions(null), null);
  assert.equal(options.parseTabOptions([]), null);
  const bad = {
    tabs_new_tab_page: 'speeddial', tabs_double_click: 'minimise', tabs_close_activation: 'Right', tabs_mute_mode: true, tabs_load_indicator: 'ring',
    tabs_confirm_close: 100, tabs_active_min_width: 401, tabs_inactive_days: 366, tabs_keep_window: 'yes', tabs_pinned_close: 'stack',
    tabs_new_tab_url: 'javascript:alert(1)', tabs_scroll: 1, tabs_new_tab_position: 'stacked',
    // --- tab stacks (T10) --- only the styles Rust parses; the delay has Rust's range.
    tabs_stack_style: 'two_level', tabs_stack_drop_delay: 2001, tabs_workspaces: 'on',
    tabs_stack_compact_row: 'yes', // --- two-level stacks (T10b) ---
  };
  for (const [key, value] of Object.entries(bad)) assert.equal(options.parseTabOptions({ ...good, [key]: value }), null, `${key}=${value}`);
  assert.ok(options.parseTabOptions({ ...good, tabs_new_tab_url: 'https://example.com/a?b=c' }));
  assert.ok(options.parseTabOptions({ ...good, tabs_confirm_close: 99, tabs_active_min_width: 400, tabs_inactive_days: 365 }));
});

test('a change is one key=value text, only for a legal value', () => {
  assert.equal(options.assignmentText('tabs_close_activation', 'recent'), 'tabs_close_activation=recent');
  assert.equal(options.assignmentText('tabs_keep_window', false), 'tabs_keep_window=false');
  assert.equal(options.assignmentText('tabs_confirm_close', 5), 'tabs_confirm_close=5');
  assert.equal(options.assignmentText('tabs_new_tab_url', 'https://example.com/?a=b'), 'tabs_new_tab_url=https://example.com/?a=b');
  assert.equal(options.assignmentText('tabs_new_tab_url', ''), 'tabs_new_tab_url=');
  assert.equal(options.assignmentText('tabs_new_tab_position', 'related'), 'tabs_new_tab_position=related');
  for (const [key, value] of [['tabs_close_activation', 'middle'], ['tabs_keep_window', 'false'], ['tabs_confirm_close', 1.5], ['tabs_confirm_close', 100], ['nope', true], ['tabs_new_tab_url', 'ftp://x'], ['tabs_new_tab_url', 'https://a\nb']]) {
    assert.equal(options.assignmentText(key, value), null, `${key}=${value}`);
  }
});

test('numbers read in words, clamp to their range, and one tab is not a bulk close', () => {
  const confirm = options.TAB_OPTIONS.find((s) => s.key === 'tabs_confirm_close');
  const width = options.TAB_OPTIONS.find((s) => s.key === 'tabs_active_min_width');
  const days = options.TAB_OPTIONS.find((s) => s.key === 'tabs_inactive_days');
  assert.equal(options.numberLabel(confirm, 0), 'Never ask');
  assert.equal(options.numberLabel(confirm, 3), '3 tabs');
  assert.equal(options.numberLabel(days, 1), '1 day');
  assert.equal(options.numberLabel(days, 0), 'Off');
  assert.equal(options.numberLabel(width, 120), '120 px');
  assert.equal(options.numberLabel(width, 0), 'Automatic');
  assert.equal(options.clampNumber(confirm, 1), 2, 'closing one tab is not closing many');
  assert.equal(options.clampNumber(confirm, 500), 99);
  assert.equal(options.clampNumber(confirm, -4), 0);
  assert.equal(options.clampNumber(width, 12.6), 13);
  assert.equal(options.clampNumber(width, Number.NaN), 0);
});

test('the new tab address only matters while the new tab page is a specific page', () => {
  const url = options.TAB_OPTIONS.find((s) => s.key === 'tabs_new_tab_url');
  assert.equal(options.textApplies(url, { tabs_new_tab_page: 'start' }), false);
  assert.equal(options.textApplies(url, { tabs_new_tab_page: 'url' }), true);
  assert.equal(options.textApplies(url, { tabs_new_tab_page: 'homepage' }), false, 'the homepage is set in Start page');
  assert.equal(options.textApplies(url, { tabs_new_tab_page: 'blank' }), false);
});

test('the new tab page offers the four choices, and homepage and blank are legal values the page sends', () => {
  const spec = options.TAB_OPTIONS.find((s) => s.key === 'tabs_new_tab_page');
  assert.deepEqual(spec.choices.map(([value]) => value), ['start', 'homepage', 'blank', 'url']);
  const good = { ...settingsFixture.snap.d.tabOptions };
  for (const value of ['homepage', 'blank']) {
    assert.ok(options.parseTabOptions({ ...good, tabs_new_tab_page: value }), value);
    assert.equal(options.assignmentText('tabs_new_tab_page', value), `tabs_new_tab_page=${value}`);
  }
});

// ---- the strip's marks and inputs ----

test('a load shows in the way the option says, and never goes unseen', () => {
  assert.equal(marks.loadMark('spinner', true, 40), 'spin');
  assert.equal(marks.loadMark('spinner', false, 40), 'none');
  assert.equal(marks.loadMark('bar', true, 40), 'bar');
  assert.equal(marks.loadMark('bar', true, undefined), 'spin', 'no progress figure yet: the spinner stands in');
  assert.equal(marks.loadMark('favicon', true, 0), 'ring');
  assert.equal(marks.loadMark('favicon', true, undefined), 'spin');
  assert.equal(marks.loadMark('none', true, 40), 'none');
  assert.equal(marks.loadMark('bar', false, 100), 'none', 'a finished load shows nothing');
});

test('counts and countdowns read in a few characters', () => {
  assert.equal(marks.notifyLabel(1), '1');
  assert.equal(marks.notifyLabel(99), '99');
  assert.equal(marks.notifyLabel(100), '99+');
  assert.equal(marks.reloadLabel(45), '45s');
  assert.equal(marks.reloadLabel(60), '1m');
  assert.equal(marks.reloadLabel(61), '2m');
  assert.equal(marks.reloadLabel(3600), '1h');
  assert.equal(marks.reloadLabel(0), '0s');
  assert.equal(marks.reloadRemaining(30, 0), 30);
  assert.equal(marks.reloadRemaining(30, 29_100), 1);
  assert.equal(marks.reloadRemaining(30, 90_000), 0, 'never negative');
  assert.equal(marks.markedName('Inbox', 3, true, 30), 'Inbox, 3 notifications, unread, reloads in 30s');
  assert.equal(marks.markedName('Inbox', 1, false, undefined), 'Inbox, 1 notification');
  assert.equal(marks.markedName('Inbox', undefined, undefined, undefined), 'Inbox', 'an unmarked tab keeps its plain name');
});

test('Ctrl and Shift click select tabs and a plain click clears the selection', () => {
  const order = [1, 2, 3, 4, 5];
  const click = marks.clickSelection;
  // Plain click: nothing selected, the tab becomes active.
  assert.deepEqual(click([2, 3], order, 2, 1, 4, { ctrl: false, shift: false }), { selected: [], anchor: 4, activate: true });
  // Ctrl: the active tab joins first, then each click toggles.
  assert.deepEqual(click([], order, 0, 1, 3, { ctrl: true, shift: false }), { selected: [1, 3], anchor: 3, activate: false });
  assert.deepEqual(click([1, 3], order, 3, 1, 5, { ctrl: true, shift: false }).selected, [1, 3, 5]);
  assert.deepEqual(click([1, 3, 5], order, 5, 1, 3, { ctrl: true, shift: false }).selected, [1, 5], 'a second Ctrl click takes it out');
  assert.deepEqual(click([1, 3], order, 3, 1, 3, { ctrl: true, shift: false }).selected, [1]);
  // Shift: a range from the anchor (the active tab before any click), both ends included, either direction.
  assert.deepEqual(click([], order, 0, 2, 4, { ctrl: false, shift: true }), { selected: [2, 3, 4], anchor: 2, activate: false });
  assert.deepEqual(click([], order, 4, 2, 1, { ctrl: false, shift: true }).selected, [1, 2, 3, 4], 'back from the anchor');
  assert.deepEqual(click([2, 3], order, 99, 5, 3, { ctrl: false, shift: true }).selected, [3, 4, 5], 'an anchor that is gone falls back to the active tab');
  assert.deepEqual(click([2], order, 2, 1, 99, { ctrl: false, shift: true }), { selected: [2], anchor: 2, activate: false }, 'an unknown tab changes nothing');
  assert.deepEqual(marks.pruneSelection([1, 2, 9], new Set([2, 3])), [2]);
});

// --- wave-1 fixes ---
test('with Include the active tab off, the active tab never joins a Ctrl or Shift selection', () => {
  const order = [1, 2, 3, 4, 5];
  const click = marks.clickSelection;
  assert.deepEqual(click([], order, 0, 1, 3, { ctrl: true, shift: false }, false), { selected: [3], anchor: 3, activate: false }, 'Ctrl+click selects only the clicked tab');
  assert.deepEqual(click([3], order, 3, 1, 5, { ctrl: true, shift: false }, false).selected, [3, 5]);
  assert.deepEqual(click([3], order, 3, 1, 1, { ctrl: true, shift: false }, false).selected, [3], 'Ctrl+click on the active tab does not add it');
  assert.deepEqual(click([], order, 0, 2, 4, { ctrl: false, shift: true }, false).selected, [3, 4], 'a range from the active tab leaves it out');
  assert.deepEqual(click([], order, 5, 3, 1, { ctrl: false, shift: true }, false).selected, [1, 2, 4, 5], 'a range across the active tab skips it');
  assert.deepEqual(click([], order, 0, 2, 4, { ctrl: false, shift: true }, true).selected, [2, 3, 4], 'on: unchanged');
});

test('the wheel steps through tabs a notch at a time and stops at the ends', () => {
  assert.deepEqual(marks.wheelSteps(0, 100), { steps: 1, rest: 40 });
  assert.deepEqual(marks.wheelSteps(0, 20), { steps: 0, rest: 20 });
  assert.deepEqual(marks.wheelSteps(40, 30), { steps: 1, rest: 10 }, 'a trackpad adds up');
  assert.deepEqual(marks.wheelSteps(0, -150), { steps: -2, rest: -30 });
  const order = [10, 20, 30];
  assert.equal(marks.stepTab(order, 20, 1), 30);
  assert.equal(marks.stepTab(order, 20, -1), 10);
  assert.equal(marks.stepTab(order, 30, 1), null, 'the last tab stays');
  assert.equal(marks.stepTab(order, 10, -3), null);
  assert.equal(marks.stepTab(order, 10, 5), 30, 'a long scroll stops at the end');
  assert.equal(marks.stepTab(order, 99, 1), null);
  assert.equal(marks.stepTab(order, 20, 0), null);
});

test('the width lock and the strip classes follow the options', () => {
  assert.equal(marks.lockedWidth(118.4), 118);
  assert.equal(marks.lockedWidth(10), 32, 'never narrower than an icon');
  assert.equal(marks.lockedWidth(Number.NaN), 32);
  const off = { closeLeft: false, closeAlways: false, scroll: false, separator: false, removeSpacing: false, maximised: false };
  assert.deepEqual(marks.stripClasses(off), []);
  assert.deepEqual(marks.stripClasses({ ...off, closeLeft: true, closeAlways: true, scroll: true, separator: true }), ['close-left', 'close-always', 'scrolling', 'pin-sep']);
  assert.deepEqual(marks.stripClasses({ ...off, removeSpacing: true }), [], 'only while maximised');
  assert.deepEqual(marks.stripClasses({ ...off, removeSpacing: true, maximised: true }), ['flush']);
});

// ---- the strip's wire ----

test('the Rust tabs message carries marks only where set, and the page keeps exactly those', () => {
  const inbound = parseInbound(JSON.stringify(stripFixture.tabs));
  assert.equal(inbound?.t, 'tabs');
  const [plain, marked, bar] = inbound.items;
  for (const key of ['unread', 'notify', 'asleep', 'pct', 'rl', 'idle']) assert.ok(!(key in plain), `a plain tab has no ${key}`);
  assert.deepEqual({ unread: marked.unread, notify: marked.notify, asleep: marked.asleep, pct: marked.pct, rl: marked.rl, idle: marked.idle }, { unread: true, notify: 3, asleep: true, pct: 45, rl: 30, idle: true });
  assert.equal(bar.pct, 75);
  const raw = stripFixture.tabs;
  const bads = [
    { notify: 0 }, { notify: 1000 }, { notify: 1.5 }, { pct: 101 }, { pct: -1 }, { rl: 86401 }, { unread: 'yes' }, { asleep: 1 }, { idle: null }, { extra: 1 },
  ];
  for (const bad of bads) {
    const msg = { ...raw, items: [{ ...raw.items[0], ...bad }] };
    assert.equal(parseInbound(JSON.stringify(msg)), null, JSON.stringify(bad));
  }
});

test('the strip options message is whole or refused', () => {
  const opts = parseInbound(JSON.stringify(stripFixture.opts));
  assert.deepEqual(opts, {
    t: 'tabopts', closeLeft: true, closeAlways: true, scroll: true, minWidth: 180, indicator: 'bar', wheel: true, separator: true, alignClose: true,
    selection: true, selectionActive: false, removeSpacing: true, maximised: true, closed: 4, stackDrag: false, stackDelay: 250,
    stackRow: 'regular', // --- two-level stacks (T10b) ---
    closedButton: true, tools: [{ k: 'cmd:new-tab', l: 'New tab', i: 1 }], // --- toolbars and feeds (thread T13) ---
  });
  assert.equal(parseInbound(JSON.stringify(stripFixture.defaults)).stackRow, 'off', 'no second row unless Rust laid one out');
  assert.equal(parseInbound(JSON.stringify({ ...stripFixture.opts, stackRow: 'compact' })).stackRow, 'compact');
  // --- tab stacks (T10) --- stacking by drag is on by default, after Rust's 400 ms.
  assert.equal(parseInbound(JSON.stringify(stripFixture.defaults)).stackDrag, true);
  assert.equal(parseInbound(JSON.stringify(stripFixture.defaults)).stackDelay, 400);
  const defaults = parseInbound(JSON.stringify(stripFixture.defaults));
  assert.equal(defaults.indicator, 'spinner');
  assert.equal(defaults.selection, true);
  assert.equal(defaults.selectionActive, true, 'the active tab is included by default');
  assert.equal(defaults.closed, 0);
  const raw = stripFixture.opts;
  for (const key of Object.keys(raw).filter((k) => k !== 'v' && k !== 't')) {
    const missing = { ...raw };
    delete missing[key];
    assert.equal(parseInbound(JSON.stringify(missing)), null, `missing ${key}`);
  }
  for (const bad of [{ indicator: 'ring' }, { minWidth: 401 }, { closed: 100 }, { closeLeft: 1 }, { wheel: 'yes' }, { extra: 1 }, { stackDelay: 2001 }, { stackDrag: 1 }, { stackRow: 'tall' }, { stackRow: true }]) {
    assert.equal(parseInbound(JSON.stringify({ ...raw, ...bad })), null, JSON.stringify(bad));
  }
});

// ---- the Sessions panel's wire ----

test('the Rust sessions snapshot is the one the panel parses', () => {
  const inbound = parseInbound(JSON.stringify(sessionsFixture.snap));
  assert.equal(inbound?.t, 'snap');
  assert.equal(inbound.p, 'sessions');
  const snap = sessions.parseSessionsSnapshot(inbound.d);
  assert.deepEqual(snap, {
    allowed: true, tabs: 4, selected: 3, suggest: 'Session 1',
    items: [{ name: 'Work', count: 2, hosts: ['example.com', 'news.example'] }, { name: 'Reading list', count: 1, hosts: ['one.example'] }],
  });
  const guest = sessions.parseSessionsSnapshot(parseInbound(JSON.stringify(sessionsFixture.guest)).d);
  assert.equal(guest.allowed, false);
  assert.deepEqual(guest.items, []);
});

test('a sessions snapshot is refused unless it is exactly what Rust sends', () => {
  const d = parseInbound(JSON.stringify(sessionsFixture.snap)).d;
  assert.ok(sessions.parseSessionsSnapshot(d));
  for (const key of Object.keys(d)) {
    const missing = { ...d };
    delete missing[key];
    assert.equal(sessions.parseSessionsSnapshot(missing), null, `missing ${key}`);
  }
  const item = d.items[0];
  for (const bad of [
    { ...d, extra: 1 }, { ...d, allowed: 1 }, { ...d, tabs: -1 }, { ...d, selected: 1.5 }, { ...d, suggest: 3 }, { ...d, items: 'x' },
    { ...d, items: [{ ...item, extra: 1 }] }, { ...d, items: [{ ...item, name: '' }] }, { ...d, items: [{ ...item, count: 101 }] },
    { ...d, items: [{ ...item, hosts: ['a', 'b', 'c', 'd'] }] }, { ...d, items: [item, item] }, { ...d, items: Array.from({ length: 33 }, (_, i) => ({ ...item, name: `s${i}` })) },
  ]) {
    assert.equal(sessions.parseSessionsSnapshot(bad), null, JSON.stringify(bad).slice(0, 100));
  }
});

test('the panel sends only commands Rust accepts', () => {
  assert.deepEqual(sessions.saveCommand('  Work  ', false), { a: 'save', text: 'Work', n: 0 });
  assert.deepEqual(sessions.saveCommand('Work', true), { a: 'save', text: 'Work', n: 1 });
  assert.deepEqual(sessions.openCommand('Work', true), { a: 'open', text: 'Work', n: 1 });
  assert.deepEqual(sessions.openCommand('Work', false), { a: 'open', text: 'Work', n: 0 });
  assert.deepEqual(sessions.deleteCommand('Work'), { a: 'delete', text: 'Work' });
  for (const name of ['', '   ', 'a\nb', 'x'.repeat(101)]) {
    assert.equal(sessions.saveCommand(name, false), null, JSON.stringify(name));
    assert.equal(sessions.openCommand(name, false), null);
    assert.equal(sessions.deleteCommand(name), null);
  }
  assert.ok(sessions.saveCommand('x'.repeat(100), false));
  assert.equal(sessions.summaryText({ name: 'a', count: 1, hosts: [] }), '1 tab');
  assert.equal(sessions.summaryText({ name: 'a', count: 5, hosts: ['a.example', 'b.example'] }), '5 tabs · a.example, b.example');
  assert.equal(sessions.openTabsText(1, 0), '1 open tab');
  assert.equal(sessions.openTabsText(6, 3), '6 open tabs, 3 selected');
  assert.equal(sessions.openTabsText(6, 1), '6 open tabs', 'one tab is not a selection');
});
