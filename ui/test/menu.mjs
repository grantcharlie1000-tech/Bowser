import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, '..', 'src', 'menu-logic.ts'), 'utf8');
const { code } = esbuild.transformSync(source, { loader: 'ts', format: 'esm' });
const logic = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);

test('menu navigation wraps and skips disabled rows and separators', () => {
  const entries = [{ id: 1, label: 'A', enabled: true, checked: false }, { separator: true }, { id: 2, label: 'B', enabled: false, checked: false }, { id: 3, label: 'C', enabled: true, checked: false }];
  assert.equal(logic.stepMenu(entries, 0, 1), 3);
  assert.equal(logic.stepMenu(entries, 3, 1), 0);
  assert.equal(logic.stepMenu(entries, 0, -1), 3);
  assert.equal(logic.stepMenu(entries, -1, -1), 3);
});

test('Home and End find the first and last enabled item', () => {
  const entries = [{ separator: true }, { id: 1, label: 'A', enabled: false, checked: false }, { id: 2, label: 'B', enabled: true, checked: false }];
  assert.equal(logic.edgeMenu(entries, false), 2);
  assert.equal(logic.edgeMenu(entries, true), 2);
  assert.equal(logic.edgeMenu([{ separator: true }], true), -1);
});

test('menu payload caps nested entries and validates selectable row fields', () => {
  const msg = { v: 1, t: 'menu', tok: 4, kind: 'sample', entries: [{ id: 1, label: 'One', enabled: true, checked: false }] };
  assert.equal(logic.validMenuPayload(msg), true);
  assert.equal(logic.validMenuPayload({ ...msg, entries: Array.from({ length: 129 }, (_, id) => ({ id, label: 'x', enabled: true, checked: false })) }), false);
  assert.equal(logic.validMenuPayload({ ...msg, entries: [{ ...msg.entries[0], label: 'x'.repeat(129) }] }), false);
  assert.equal(logic.validMenuPayload({ ...msg, entries: [{ ...msg.entries[0], enabled: 'yes' }] }), false);
});

const item = (id, label, extra = {}) => ({ id, label, enabled: true, checked: false, ...extra });

test('arrow keys, Home and End move the selection on the current page', () => {
  const nav = logic.createNav([item(1, 'Alpha'), { separator: true }, item(2, 'Beta', { enabled: false }), item(3, 'Gamma')]);
  assert.equal(logic.currentPage(nav).selected, 0);
  assert.deepEqual(logic.menuKey(nav, 'ArrowDown', {}, 0), { kind: 'select' });
  assert.equal(logic.currentPage(nav).selected, 3);
  assert.deepEqual(logic.menuKey(nav, 'ArrowDown', {}, 0), { kind: 'select' });
  assert.equal(logic.currentPage(nav).selected, 0);
  logic.menuKey(nav, 'End', {}, 0);
  assert.equal(logic.currentPage(nav).selected, 3);
  assert.deepEqual(logic.menuKey(nav, 'End', {}, 0), { kind: 'none' });
  logic.menuKey(nav, 'Home', {}, 0);
  assert.equal(logic.currentPage(nav).selected, 0);
  logic.menuKey(nav, 'Tab', { shift: true }, 0);
  assert.equal(logic.currentPage(nav).selected, 3);
});

test('Enter and Space pick commands; Escape dismisses', () => {
  const nav = logic.createNav([item(7, 'Open'), item(8, 'Close')]);
  assert.deepEqual(logic.menuKey(nav, 'Enter', {}, 0), { kind: 'pick', id: 7 });
  logic.menuKey(nav, 'ArrowDown', {}, 0);
  assert.deepEqual(logic.menuKey(nav, ' ', {}, 0), { kind: 'pick', id: 8 });
  assert.deepEqual(logic.menuKey(nav, 'Escape', {}, 0), { kind: 'dismiss' });
  assert.deepEqual(logic.menuKey(logic.createNav([item(1, 'Off', { enabled: false })]), 'Enter', {}, 0), { kind: 'none' });
});

test('submenus open as drill-in pages with Right/Enter and close with Left/Backspace', () => {
  const nav = logic.createNav([item(1, 'Plain'), item(2, 'More', { submenu: [item(3, 'Nested'), item(4, 'Other')] })]);
  assert.deepEqual(logic.menuKey(nav, 'ArrowRight', {}, 0), { kind: 'none' });
  logic.menuKey(nav, 'ArrowDown', {}, 0);
  assert.deepEqual(logic.menuKey(nav, 'ArrowRight', {}, 0), { kind: 'page' });
  assert.equal(nav.pages.length, 2);
  assert.equal(logic.currentPage(nav).label, 'More');
  assert.equal(logic.currentPage(nav).selected, 0);
  logic.menuKey(nav, 'ArrowDown', {}, 0);
  assert.deepEqual(logic.menuKey(nav, 'Enter', {}, 0), { kind: 'pick', id: 4 });
  assert.deepEqual(logic.menuKey(nav, 'ArrowLeft', {}, 0), { kind: 'page' });
  assert.equal(nav.pages.length, 1);
  assert.equal(logic.currentPage(nav).selected, 1, 'the parent row stays selected');
  assert.deepEqual(logic.menuKey(nav, 'ArrowLeft', {}, 0), { kind: 'none' });
  assert.deepEqual(logic.menuKey(nav, 'Enter', {}, 0), { kind: 'page' });
  assert.deepEqual(logic.menuKey(nav, 'Backspace', {}, 0), { kind: 'page' });
});

test('typeahead matches prefixes, cycles on a repeated letter and resets after a pause', () => {
  const nav = logic.createNav([item(1, 'New tab'), item(2, 'Notes'), item(3, 'Settings'), item(4, 'Saved sessions'), item(5, 'Sleep', { enabled: false })]);
  assert.deepEqual(logic.menuKey(nav, 's', {}, 0), { kind: 'select' });
  assert.equal(logic.currentPage(nav).selected, 2);
  logic.menuKey(nav, 's', {}, 100);
  assert.equal(logic.currentPage(nav).selected, 3, 'same letter cycles');
  logic.menuKey(nav, 's', {}, 200);
  assert.equal(logic.currentPage(nav).selected, 2, 'disabled rows are skipped and the cycle wraps');
  logic.menuKey(nav, 'n', {}, 2000);
  assert.equal(logic.currentPage(nav).selected, 0);
  logic.menuKey(nav, 'o', {}, 2100);
  assert.equal(logic.currentPage(nav).selected, 1, 'two letters refine to "no"');
  assert.deepEqual(logic.menuKey(nav, 'x', { ctrl: true }, 2200), { kind: 'none' });
  assert.equal(logic.typeahead([item(1, '  Padded')], -1, 'pa', false), 0);
  assert.equal(logic.typeahead([], -1, 'a', false), -1);
});

test('payload accepts the main menu header and footer and rejects bad ones', () => {
  const msg = { v: 1, t: 'menu', tok: 4, kind: 'main', header: { name: 'Default', caption: 'Current profile', avatar: 'D', accent: '#3366ff' }, footer: 'Bowser 1.0', entries: [item(1, 'One')] };
  assert.equal(logic.validMenuPayload(msg), true);
  assert.equal(logic.validMenuPayload({ ...msg, header: { ...msg.header, accent: 'red' } }), false);
  assert.equal(logic.validMenuPayload({ ...msg, header: { ...msg.header, extra: 1 } }), false);
  assert.equal(logic.validMenuPayload({ ...msg, footer: 'x'.repeat(257) }), false);
  assert.equal(logic.validMenuPayload({ ...msg, entries: [item(1, 'x', { icon: 'javascript' })] }), false);
});
