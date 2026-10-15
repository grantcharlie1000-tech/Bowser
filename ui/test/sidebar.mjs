// Pure sidebar ordering, drag and scroll geometry tests.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const bundled = await esbuild.build({
  entryPoints: [join(here, '..', 'src', 'sidebar-logic.ts')],
  bundle: true,
  format: 'esm',
  platform: 'neutral',
  write: false,
  logLevel: 'silent',
});
const logic = await import(`data:text/javascript;base64,${Buffer.from(bundled.outputFiles[0].contents).toString('base64')}`);

const tab = (id, extra = {}) => ({
  id, title: `t${id}`, loading: false, pinned: false, muted: false, private: false,
  marker: -1, g: 0, gl: false, fav: false, ...extra,
});

test('vertical drop slot compares row centres along y', () => {
  assert.equal(logic.dropIndexVertical([10, 30, 50, 70], 0, 5), 0);
  assert.equal(logic.dropIndexVertical([10, 30, 50, 70], 0, 35), 1);
  assert.equal(logic.dropIndexVertical([10, 30, 50, 70], 3, 20), 1);
});

test('sidebar group headers derive label, colour, collapse and count from the leader', () => {
  const entries = logic.buildSidebarEntries([
    tab(1, { pinned: true }),
    tab(2, { g: 9, gl: true, gname: 'Research', gcol: 3, gcollapsed: false, gsize: 2 }),
    tab(3, { g: 9 }),
  ], 2);
  assert.deepEqual(entries.map((entry) => entry.kind), ['tab', 'group', 'tab', 'tab']);
  assert.deepEqual(
    (({ kind, key, name, colour, collapsed, count }) => ({ kind, key, name, colour, collapsed, count }))(entries[1]),
    { kind: 'group', key: 9, name: 'Research', colour: 3, collapsed: false, count: 2 },
  );
});

test('pinned tiles drop on the nearest grid slot and neighbours step to the slot they take', () => {
  // A 3-up grid of 32px tiles, 4px apart: two rows.
  const tiles = [[16, 16], [52, 16], [88, 16], [16, 52], [52, 52]];
  assert.equal(logic.nearestTile(tiles, 20, 18), 0);
  assert.equal(logic.nearestTile(tiles, 80, 20), 2);
  assert.equal(logic.nearestTile(tiles, 50, 60), 4);
  // Tile 0 dragged onto slot 2: tiles 1 and 2 step back one slot; the second row stays.
  assert.deepEqual(logic.tileShift(tiles, 1, 0, 2), [-36, 0]);
  assert.deepEqual(logic.tileShift(tiles, 2, 0, 2), [-36, 0]);
  assert.deepEqual(logic.tileShift(tiles, 3, 0, 2), [0, 0]);
  // Tile 4 dragged onto slot 1: tile 2 wraps down to the start of the second row, tile 3 steps right.
  assert.deepEqual(logic.tileShift(tiles, 2, 4, 1), [-72, 36]);
  assert.deepEqual(logic.tileShift(tiles, 3, 4, 1), [36, 0]);
  // Landing last among the pins means "before the first unpinned tab", so it stays pinned-first.
  assert.equal(logic.pinnedBeforeId([11, 12], 0, 30), 11);
  assert.equal(logic.pinnedBeforeId([11, 12], 2, 30), 30);
  assert.equal(logic.pinnedBeforeId([11, 12], 2, 0), 0);
});

test('scroll-into-view delta is zero for visible rows and minimal for clipped rows', () => {
  assert.equal(logic.scrollIntoViewDelta(20, 100, 40, 20), 0);
  assert.equal(logic.scrollIntoViewDelta(20, 100, 10, 20), -10);
  assert.equal(logic.scrollIntoViewDelta(20, 100, 110, 20), 10);
});

test('workspace glyph ids map to the workspace-* icon family, unknown ids to the generic stack', () => {
  assert.equal(logic.workspaceIconName('g:code'), 'workspace-code');
  assert.equal(logic.workspaceIconName('g:leaf'), 'workspace-leaf');
  assert.equal(logic.workspaceIconName('g:nope'), 'workspace');
  assert.equal(logic.workspaceIconName('javascript:1'), 'workspace');
});

test('roving focus clamps at both ends and ignores other keys', () => {
  assert.equal(logic.rovingTarget('ArrowUp', 0, 5), 0);
  assert.equal(logic.rovingTarget('ArrowDown', 4, 5), 4);
  assert.equal(logic.rovingTarget('ArrowDown', 1, 5), 2);
  assert.equal(logic.rovingTarget('Home', 3, 5), 0);
  assert.equal(logic.rovingTarget('End', 1, 5), 4);
  assert.equal(logic.rovingTarget('x', 2, 5), 2);
});

test('private rows sort and group exactly like normal rows (never inside a group)', () => {
  const items = [tab(1, { pinned: true, private: true }), tab(2, { g: 9, gl: true, gname: 'R', gcol: 1, gcollapsed: false, gsize: 1 }), tab(3, { private: true })];
  const plain = items.map((t) => ({ ...t, private: false }));
  const shape = (entries) => entries.map((e) => `${e.kind}:${e.kind === 'tab' ? e.item.id : e.key}`);
  assert.deepEqual(shape(logic.buildSidebarEntries(items, 3)), shape(logic.buildSidebarEntries(plain, 3)));
});
