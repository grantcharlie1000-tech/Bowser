// Tests for the pure tab-strip logic (src/tabs-logic.ts). Uses node:test and the esbuild already in devDependencies.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, '..', 'src', 'tabs-logic.ts'), 'utf8');
const { code } = esbuild.transformSync(source, { loader: 'ts', format: 'esm' });
const logic = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
const { beforeId, buildEntries, dropIndex, pruneFavs, rememberFav, shiftTarget, slotShift } = logic;

const tab = (id, extra = {}) => ({
  id,
  title: `t${id}`,
  loading: false,
  pinned: false,
  muted: false,
  marker: -1,
  g: 0,
  gl: false,
  fav: false,
  ...extra,
});

test('dropIndex counts the other tabs left of the dragged centre', () => {
  const centers = [10, 30, 50, 70];
  assert.equal(dropIndex(centers, 0, 5), 0);
  assert.equal(dropIndex(centers, 0, 35), 1);
  assert.equal(dropIndex(centers, 0, 100), 3);
  assert.equal(dropIndex(centers, 3, 20), 1);
});

test('shiftTarget moves only the tabs between origin and destination', () => {
  // Dragging index 0 to 2: 1 and 2 slide left, 3 stays.
  assert.deepEqual([1, 2, 3].map((i) => shiftTarget(i, 0, 2)), [0, 1, 3]);
  // Dragging index 2 to 0: 0 and 1 slide right, 3 stays.
  assert.deepEqual([0, 1, 3].map((i) => shiftTarget(i, 2, 0)), [1, 2, 3]);
  assert.equal(shiftTarget(0, 0, 2), 0);
});

test('slotShift returns the px distance to the target slot centre', () => {
  const centers = [10, 30, 50, 70];
  assert.equal(slotShift(centers, 1, 0, 2), -20);
  assert.equal(slotShift(centers, 0, 2, 0), 20);
  assert.equal(slotShift(centers, 3, 0, 2), 0);
});

test('beforeId is the tab right of the drop slot, 0 when last', () => {
  assert.equal(beforeId([11, 12], 0), 11);
  assert.equal(beforeId([11, 12], 1), 12);
  assert.equal(beforeId([11, 12], 2), 0);
});

test('buildEntries puts pinned tabs first and gathers group members at the first member', () => {
  const items = [
    tab(1, { pinned: true }),
    tab(2, { g: 5, gl: true, gname: 'Work', gcol: 2, gcollapsed: false, gsize: 2 }),
    tab(3),
    tab(4, { g: 5 }),
  ];
  const entries = buildEntries(items, 3);
  assert.deepEqual(entries.map((e) => e.kind), ['tab', 'group', 'tab']);
  assert.equal(entries[0].slot.item.id, 1);
  const group = entries[1];
  assert.equal(group.key, 5);
  assert.equal(group.leader.gname, 'Work');
  assert.deepEqual(group.members.map((s) => s.item.id), [2, 4]);
  assert.equal(group.visible, 2);
  assert.equal(entries[2].slot.item.id, 3);
});

test('a collapsed group shows only the active member', () => {
  const items = [
    tab(2, { g: 5, gl: true, gname: 'Work', gcol: -1, gcollapsed: true, gsize: 2 }),
    tab(4, { g: 5 }),
  ];
  const withActive = buildEntries(items, 4)[0];
  assert.deepEqual(withActive.members.map((s) => s.hidden), [true, false]);
  assert.equal(withActive.visible, 1);

  const noneActive = buildEntries(items, 99)[0];
  assert.equal(noneActive.collapsed, true);
  assert.equal(noneActive.visible, 0);
});

test('rememberFav keeps the map bounded and refreshes re-set ids', () => {
  const favs = new Map();
  rememberFav(favs, 1, 'a', 2);
  rememberFav(favs, 2, 'b', 2);
  rememberFav(favs, 1, 'a2', 2);
  rememberFav(favs, 3, 'c', 2);
  assert.deepEqual([...favs.keys()], [1, 3]);
  assert.equal(favs.get(1), 'a2');
});

test('pruneFavs drops ids that are no longer tabs', () => {
  const favs = new Map([
    [1, 'a'],
    [2, 'b'],
  ]);
  pruneFavs(favs, new Set([2]));
  assert.deepEqual([...favs.keys()], [2]);
});
