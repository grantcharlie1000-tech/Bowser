// The docked toolbar customiser (panels/toolbar.ts; src/win32/toolbar_panel.rs): the snapshot it accepts, how its
// rows are grouped and ordered, and what each switch, the keyboard move and Reset send (the whole new list of one
// bar, as a `tbar` `set`), in a fake document.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';
import { FakeDocument, Node } from './fake-dom.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, '..', 'src');

const sent = [];
globalThis.document = new FakeDocument();
Object.assign(globalThis, {
  window: globalThis,
  matchMedia: () => ({ matches: true }), // reduced motion: no Web Animations in the fake document
  CSS: { escape: (s) => s },
  chrome: { webview: { postMessage: (wire) => sent.push(JSON.parse(wire)), addEventListener() {} } },
});
globalThis.HTMLElement = Node;

const out = esbuild.buildSync({ entryPoints: [join(srcDir, 'panels', 'toolbar.ts')], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
const mod = await import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);

const commands = [
  { id: 'new-tab', label: 'New tab', chain: false },
  { id: 'history', label: 'History', chain: false },
  { id: 'capture-full', label: 'Capture full page', chain: false },
  { id: 'chain:2', label: 'Morning', chain: true },
];
const snap = (address, tabs = 'closed-tabs') => ({ t: 'snap', p: 'toolbar', d: { address, tabs, commands } });

const root = new Node('div');
const receive = mod.startToolbarPanel(root);
const rows = (section) => section.querySelectorAll('.tbp-row').map((row) => [row.dataset.item, row.querySelector('.bw-toggle').getAttribute('aria-checked') === 'true']);
const cards = () => root.querySelectorAll('.tbp-card');
const sections = () => cards()[0].querySelectorAll('.tbp-section');
const toggleOf = (id) => root.querySelectorAll('.tbp-row').find((row) => row.dataset.item === id).querySelector('.bw-toggle');
const lastSet = () => sent.filter((m) => m.t === 'tbar').at(-1);

test('the snapshot is read strictly', () => {
  assert.ok(mod.parseToolbarSnapshot(snap('back').d));
  assert.equal(mod.parseToolbarSnapshot({ address: 'back', tabs: '' }), null, 'no commands');
  assert.equal(mod.parseToolbarSnapshot({ address: 1, tabs: '', commands: [] }), null);
  assert.equal(mod.parseToolbarSnapshot({ address: '', tabs: '', commands: [{ id: 'x', label: 'X' }] }), null, 'chain flag missing');
});

test('each group lists the buttons that are on in bar order, then the ones that are off', () => {
  receive(snap('reload,back,site-info,cmd:history,bookmark'));
  assert.equal(root.querySelector('.tbp-title, .pn-title').textContent, 'Customise toolbar');
  const [left, pill, right] = sections();
  assert.deepEqual(rows(left), [['reload', true], ['back', true], ['forward', false]]);
  assert.deepEqual(rows(pill), [['site-info', true], ['split', false], ['reader', false]]);
  assert.deepEqual(rows(right), [['cmd:history', true], ['bookmark', true], ['newtab', false], ['downloads', false], ['extensions', false]]);
  // Commands that are on sit in their group; the commands card lists the others.
  assert.deepEqual(rows(cards()[1]).map(([id]) => id), ['cmd:new-tab', 'cmd:capture-full', 'chain:2']);
  assert.equal(cards()[1].querySelector('.tbp-count').textContent, '1 of 4 on the bar');
  // Buttons that only show on demand say when.
  const reader = root.querySelectorAll('.tbp-row').find((row) => row.dataset.item === 'reader');
  assert.equal(reader.querySelector('.tbp-hint').textContent, 'Appears on pages that can be read as an article');
  // Only rows that are on and have a neighbour to swap with get a grip.
  assert.equal(left.querySelectorAll('.tbp-row')[0].querySelector('.tbp-grip').childElementCount, 1);
  assert.equal(left.querySelectorAll('.tbp-row')[2].querySelector('.tbp-grip').childElementCount, 0);
});

test('a switch sends the whole new list of its bar, and a command joins the end of its place', () => {
  receive(snap('back,forward,reload,site-info'));
  sent.length = 0;
  toggleOf('back').click();
  assert.deepEqual(lastSet(), { v: 1, t: 'tbar', a: 'set', text: 'toolbar_buttons=forward,reload,site-info' });
  receive(snap('back,forward,reload,site-info'));
  toggleOf('cmd:history').click();
  assert.deepEqual(lastSet(), { v: 1, t: 'tbar', a: 'set', text: 'toolbar_buttons=back,forward,reload,site-info,cmd:history' });
  // The page shows the change at once, before the snapshot comes back.
  assert.equal(toggleOf('cmd:history').getAttribute('aria-checked'), 'true');
});

test('a switch that cannot come on says why, sends nothing and stays off', () => {
  // Four always-there buttons at the end: a fifth fits, a sixth does not.
  receive(snap('cmd:new-tab,cmd:history,bookmark,newtab'));
  sent.length = 0;
  toggleOf('cmd:capture-full').click();
  assert.equal(sent.length, 1, 'the fifth fits');
  receive(snap('cmd:new-tab,cmd:history,cmd:capture-full,bookmark,newtab'));
  sent.length = 0;
  toggleOf('chain:2').click();
  assert.equal(sent.length, 0, 'the end of the address bar holds five always-there buttons');
  assert.match(root.querySelector('.tbp-note').textContent, /at most 5/);
  assert.equal(toggleOf('chain:2').getAttribute('aria-checked'), 'false');
  // Downloads and Extensions take no room until they have something to show, so they always fit.
  toggleOf('downloads').click();
  assert.equal(sent.length, 1);
  assert.equal(root.querySelector('.tbp-note').textContent, '', 'the note clears on the next change');
});

test('Alt+Up and Alt+Down move a button within its place', () => {
  receive(snap('back,forward,reload,site-info'));
  sent.length = 0;
  toggleOf('reload').dispatch('keydown', { key: 'ArrowUp', altKey: true });
  assert.deepEqual(lastSet(), { v: 1, t: 'tbar', a: 'set', text: 'toolbar_buttons=back,reload,forward,site-info' });
  receive(snap('back,forward,reload,site-info'));
  sent.length = 0;
  toggleOf('reload').dispatch('keydown', { key: 'ArrowDown', altKey: true });
  assert.equal(sent.length, 0, 'reload is last before the address field and never crosses into it');
});

test('the filter narrows the commands, and the tab bar has its own list', () => {
  receive(snap('back'));
  const input = root.querySelector('.tbp-filter').querySelector('input');
  input.value = 'cap';
  input.dispatch('input');
  assert.deepEqual(rows(cards()[1]).map(([id]) => id), ['cmd:capture-full']);
  input.value = 'zzz';
  input.dispatch('input');
  assert.equal(cards()[1].querySelector('.tbp-none').textContent, 'No command matches.');
  input.value = '';
  input.dispatch('input');
  root.querySelector('.tbp-switch').querySelectorAll('button')[1].click();
  assert.deepEqual(rows(sections()[0]), [['closed-tabs', true]]);
  sent.length = 0;
  toggleOf('cmd:history').click();
  assert.deepEqual(lastSet(), { v: 1, t: 'tbar', a: 'set', text: 'toolbar_tabs=closed-tabs,cmd:history' });
  root.querySelector('.tbp-switch').querySelectorAll('button')[0].click();
});

test('Reset to default sends the default list, and is off while the bar already is', () => {
  receive(snap('back,forward,reload,site-info,split,reader,extensions,downloads'));
  const reset = root.querySelector('.tbp-reset');
  assert.equal(reset.disabled, true);
  receive(snap('back'));
  assert.equal(reset.disabled, false);
  sent.length = 0;
  reset.click();
  assert.deepEqual(lastSet(), { v: 1, t: 'tbar', a: 'set', text: 'toolbar_buttons=back,forward,reload,site-info,split,reader,extensions,downloads' });
});
