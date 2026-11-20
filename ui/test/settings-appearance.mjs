// Settings > Appearance: every row acts, and a row that cannot act in the state other rows leave it in
// says so by resting (disabled and dimmed, `is-off`) or, when it only tunes something switched off,
// by leaving the page (`is-na`; a search still lists it). Also the layout rules those rows follow,
// mirrored from src/chrome/layout.rs. The page runs in the shared fake document (test/fake-dom.mjs).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';
import { Node, FakeDocument } from './fake-dom.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const read = (path) => JSON.parse(readFileSync(join(here, 'fixtures', path), 'utf8'));
const fixture = read('settings-messages.json');
const addressFixture = read('address-messages.json');

async function bundle(file) {
  const out = esbuild.buildSync({ entryPoints: [join(here, '..', 'src', file)], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
}
const logic = await bundle('settings-page-logic.ts');
const layoutLogic = await bundle('layout-logic.ts');

/** The shared fake node with the few tree moves the settings page makes. */
class PageNode extends Node {
  detach(node) {
    if (node.parent) {
      const siblings = node.parent.children;
      const index = siblings.indexOf(node);
      if (index >= 0) siblings.splice(index, 1);
    }
    node.parent = this;
  }
  get childNodes() { return this.children; }
  get lastElementChild() { return [...this.children].reverse().find((node) => node.tagName !== '#text') ?? null; }
  after(node) { const parent = this.parent; parent.detach(node); parent.children.splice(parent.children.indexOf(this) + 1, 0, node); }
  before(node) { const parent = this.parent; parent.detach(node); parent.children.splice(parent.children.indexOf(this), 0, node); }
  replaceWith(node) { const parent = this.parent; const at = parent.children.indexOf(this); parent.detach(node); parent.children.splice(at, 1, node); this.parent = null; }
}

const sent = [];
const listeners = [];
const document = new FakeDocument();
document.createElement = (tag) => new PageNode(tag);
document.createElementNS = (_ns, tag) => new PageNode(tag);
document.createTextNode = (text) => new PageNode('#text', text);
Object.assign(globalThis, {
  document, window: globalThis, Element: Node, HTMLElement: Node, HTMLInputElement: class {},
  innerWidth: 1280, innerHeight: 860,
  matchMedia: () => ({ matches: false }),
  requestAnimationFrame: (cb) => { cb(); return 1; },
  cancelAnimationFrame() {},
  addEventListener() {}, removeEventListener() {},
  IntersectionObserver: class { observe() {} disconnect() {} },
  ResizeObserver: class { observe() {} disconnect() {} },
  localStorage: { getItem: (key) => (key === 'bowser.settings.view' ? 'all' : null), setItem() {}, removeItem() {} },
  chrome: { webview: { postMessage: (wire) => sent.push(JSON.parse(wire)), addEventListener: (_t, cb) => listeners.push(cb) } },
});
const { startSettingsPage } = await bundle('settings-page.ts');
const app = new PageNode('div');
startSettingsPage(app);
const deliver = (msg) => { for (const cb of listeners) cb({ data: JSON.stringify(msg) }); };
/** The settings snapshot with `d` changed. */
const snap = (d = {}) => deliver({ ...fixture.snap, d: { ...fixture.snap.d, ...d } });
const layoutOf = (layout) => ({ layout: { ...fixture.snap.d.layout, ...layout } });
deliver(fixture.snap);
deliver(addressFixture.snap);

const rowNode = (id) => app.querySelector(`[data-row="${id}"]`);
const controlOf = (id) => rowNode(id).querySelector('[role="switch"]') ?? rowNode(id).querySelector('input') ?? rowNode(id).querySelector('.bw-select-button');
const radiosOf = (id) => rowNode(id).querySelectorAll('[role="radio"]');
const disabled = (id) => {
  const radios = radiosOf(id);
  return radios.length > 0 ? radios.every((b) => b.disabled) : controlOf(id).disabled;
};
const state = (id) => ({ disabled: disabled(id), off: rowNode(id).classList.contains('is-off'), gone: rowNode(id).classList.contains('is-na') });
const live = { disabled: false, off: false, gone: false };
const resting = { disabled: true, off: true, gone: false };
const away = { disabled: true, off: true, gone: true };

test('Title bar is gone: show_title_bar never changed the window (it lays its title row out by itself)', () => {
  assert.equal(logic.ROWS.find((row) => row.id === 'showTitleBar'), undefined);
  assert.equal(rowNode('showTitleBar'), null);
  assert.ok(!logic.AREAS.flatMap((area) => area.groups.flatMap((g) => g.rows)).includes('showTitleBar'));
  // The key itself stays in the save, unchanged, for older files.
  snap({ layout: { ...fixture.snap.d.layout, showTitleBar: true } });
  sent.length = 0;
  controlOf('compactMenus').click();
  assert.match(sent.at(-1).text, /\nshow_title_bar=true\n/);
  snap();
});

test('the sidebar width and pin leave the page unless the tabs are a sidebar on screen', () => {
  assert.equal(layoutLogic.sidebarShown('vertical', true), true);
  assert.equal(layoutLogic.sidebarShown('vertical-right', true), true);
  assert.equal(layoutLogic.sidebarShown('vertical', false), false, 'a hidden tab bar has no sidebar');
  for (const layout of ['horizontal', 'tabs-top', 'single-row', 'bottom']) assert.equal(layoutLogic.sidebarShown(layout, true), false, layout);
  snap({ tabLayout: 'horizontal' });
  for (const id of ['sidebarWidth', 'sidebarPinned']) assert.deepEqual(state(id), away, id);
  snap({ tabLayout: 'vertical' });
  for (const id of ['sidebarWidth', 'sidebarPinned']) assert.deepEqual(state(id), live, id);
  snap({ tabLayout: 'vertical-right', ...layoutOf({ tabBarVisible: false }) });
  for (const id of ['sidebarWidth', 'sidebarPinned']) assert.deepEqual(state(id), away, `${id}: tab bar hidden`);
  // An unreadable file locks them without dimming (the banner says why).
  snap({ tabLayout: 'vertical', readable: false });
  for (const id of ['sidebarWidth', 'sidebarPinned']) assert.deepEqual(state(id), { disabled: true, off: false, gone: false }, id);
  snap();
});

test('Centre address rests where the layout gives the address field no room of its own', () => {
  const applies = layoutLogic.centringApplies;
  assert.equal(applies('horizontal', true, 'top'), true, 'toolbar above the tabs');
  assert.equal(applies('horizontal', true, 'bottom'), false, 'laid out as Tabs above the toolbar');
  assert.equal(applies('tabs-top', true, 'top'), false);
  assert.equal(applies('single-row', true, 'top'), false);
  assert.equal(applies('vertical', true, 'top'), false, 'in the sidebar');
  assert.equal(applies('vertical-right', true, 'top'), false);
  assert.equal(applies('bottom', true, 'top'), true);
  assert.equal(applies('bottom', true, 'bottom'), true);
  assert.equal(applies('vertical', false, 'top'), true, 'a hidden tab bar leaves the toolbar row alone');
  snap({ tabLayout: 'horizontal' });
  assert.deepEqual(state('centredAddressBar'), live);
  for (const tabLayout of ['tabs-top', 'single-row', 'vertical']) {
    snap({ tabLayout });
    assert.deepEqual(state('centredAddressBar'), resting, tabLayout);
  }
  snap({ tabLayout: 'horizontal', ...layoutOf({ addressBarPosition: 'bottom' }) });
  assert.deepEqual(state('centredAddressBar'), resting, 'address bar along the bottom');
  snap();
});

test('Short address rests while Full address is on, and follows the address snapshot as it comes', () => {
  snap();
  const full = (on) => deliver({ ...addressFixture.snap, d: { ...addressFixture.snap.d, s: { ...addressFixture.snap.d.s, address_show_full: on } } });
  full(false);
  assert.deepEqual(state('minimalUrl'), live);
  full(true);
  assert.deepEqual(state('minimalUrl'), resting, 'the full address wins (omnibox.rs address_display)');
  snap();
  assert.deepEqual(state('minimalUrl'), resting, 'a settings snapshot keeps it resting');
  full(false);
  assert.deepEqual(state('minimalUrl'), live);
  assert.match(logic.ROWS.find((row) => row.id === 'minimalUrl').keywords, /Full address/);
});

test('the bookmarks bar rows leave the page while the bar never shows; Pin groups needs groups on the bar', () => {
  const barRows = ['bookmarksBarTabGroups', 'bmBarFolder', 'bmBarPosition', 'bmBarDisplay', 'pinNewTabGroups'];
  snap({ bookmarksBar: 'never' });
  for (const id of barRows) assert.deepEqual(state(id), away, id);
  snap({ bookmarksBar: 'always', bookmarksBarTabGroups: false });
  for (const id of barRows.filter((id) => id !== 'pinNewTabGroups')) assert.deepEqual(state(id), live, id);
  assert.deepEqual(state('pinNewTabGroups'), away, 'nothing to pin to while groups are not on the bar');
  snap({ bookmarksBar: 'newtab', bookmarksBarTabGroups: true });
  for (const id of barRows) assert.deepEqual(state(id), live, id);
  // A fresh address snapshot (the folder list redrawn) keeps the rows as the bar says.
  snap({ bookmarksBar: 'never' });
  deliver({ ...addressFixture.snap, d: { ...addressFixture.snap.d, folders: [] } });
  for (const id of ['bmBarFolder', 'bmBarPosition', 'bmBarDisplay']) assert.deepEqual(state(id), away, id);
  // Search still finds them.
  assert.ok(logic.matchRows('bookmarks bar position').has('bmBarPosition'));
  deliver(addressFixture.snap);
  snap();
});

test('Hidden bars leave the page while auto-hide is off', () => {
  snap({ frameless: false });
  assert.equal(rowNode('autoHideBars').classList.contains('is-na'), true);
  assert.ok(rowNode('autoHideBars').querySelectorAll('[role="switch"]').every((b) => b.disabled));
  snap({ frameless: true });
  assert.equal(rowNode('autoHideBars').classList.contains('is-na'), false);
  assert.ok(rowNode('autoHideBars').querySelectorAll('[role="switch"]').every((b) => !b.disabled));
  assert.deepEqual(state('autoHideFullscreen'), live, 'full screen hides the bars with the switch off too');
  snap();
});

test('the theme Look and Times rows leave the page outside their own schedule', () => {
  const themed = (theme_schedule) => snap({ themeOptions: { ...fixture.snap.d.themeOptions, theme_schedule } });
  themed('system');
  assert.deepEqual(state('themeLook'), away);
  assert.equal(rowNode('themeTimes').classList.contains('is-na'), true);
  themed('none');
  assert.deepEqual(state('themeLook'), live);
  assert.equal(rowNode('themeTimes').classList.contains('is-na'), true);
  themed('time');
  assert.equal(rowNode('themeLook').classList.contains('is-na'), true);
  assert.equal(rowNode('themeTimes').classList.contains('is-na'), false);
  assert.ok(rowNode('themeTimes').querySelectorAll('input').every((input) => !input.disabled));
  snap();
});

test('Palette: a custom accent wins over either palette, so picking one puts the accent back to the palette', () => {
  const buttons = () => radiosOf('theme');
  assert.deepEqual(buttons().map((b) => b.textContent), ['Helium', 'Windows'], '"Windows", not a second "System" beside Mode');
  assert.deepEqual(buttons().map((b) => b.getAttribute('aria-label')), ['Helium colours', 'Windows accent colour']);
  snap({ theme: 'helium', themeAccent: '' });
  assert.deepEqual(buttons().map((b) => b.getAttribute('aria-checked')), ['true', 'false']);
  snap({ theme: 'helium', themeAccent: '#abcdef', ack: 'saved' });
  assert.deepEqual(buttons().map((b) => b.getAttribute('aria-checked')), ['false', 'false'], 'no palette is in force under a custom accent');
  sent.length = 0;
  buttons()[0].click();
  const text = sent.at(-1).text;
  assert.match(text, /^theme=helium$/m, 'the palette already saved can be picked again');
  assert.match(text, /^theme_accent=$/m, 'and the custom accent goes');
  snap({ theme: 'helium', themeAccent: '#abcdef', ack: 'saved' });
  sent.length = 0;
  buttons()[1].click();
  assert.match(sent.at(-1).text, /^theme=system$/m);
  assert.match(sent.at(-1).text, /^theme_accent=$/m);
  snap();
});

test('labels and search words say what the rows really do', () => {
  const words = (id) => logic.ROWS.find((row) => row.id === id).keywords;
  assert.match(words('fontSize'), /bars, menus, panels and pages/);
  assert.match(words('winMonoIcons'), /section you are on/);
  assert.match(words('centredAddressBar'), /not beside vertical tabs/);
  assert.match(words('theme'), /custom accent back to the palette/);
  for (const group of logic.AREAS.find((area) => area.key === 'appearance').groups) {
    for (const id of group.rows) {
      const row = logic.ROWS.find((r) => r.id === id);
      assert.ok(row, `${group.key}: ${id} is a row`);
      assert.equal(row.description, '', `${id}: one line, no sub-text`);
      assert.equal(row.notYet, undefined, `${id} acts`);
    }
  }
});
