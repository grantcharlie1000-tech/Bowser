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
const { beforeSlotId, groupColourIndex, ACTIVE_WEIGHT, groupWeight, beforeId, buildEntries, dragScrollDelta, dropIndex, hiddenForTab, pruneFavs, rememberFav, scrollDeltaForWheel, shiftTarget, slotShift } = logic;

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

test('overflow helpers cover wheel, edge drag and group visibility decisions', () => {
  assert.equal(scrollDeltaForWheel(0, 12, true), 12);
  assert.equal(scrollDeltaForWheel(4, 12, false), 4);
  assert.equal(dragScrollDelta(5, 0, 200), -19);
  assert.equal(dragScrollDelta(195, 0, 200), 19);
  assert.equal(dragScrollDelta(100, 0, 200), 0);
  assert.equal(hiddenForTab(true, true, false), true);
  assert.equal(hiddenForTab(true, true, true), false);
  assert.equal(hiddenForTab(false, true, false), false);
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
  const live = new Set([1, 2, 3]);
  rememberFav(favs, 1, 'a', live, 2);
  rememberFav(favs, 2, 'b', live, 2);
  rememberFav(favs, 1, 'a2', live, 2);
  rememberFav(favs, 3, 'c', live, 2);
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

test('favicon capacity matches all protocol tabs and prunes missing ids before eviction', () => {
  assert.equal(logic.FAV_CACHE_MAX, 512);
  const favs = new Map([[1, 'a'], [2, 'b']]);
  rememberFav(favs, 3, 'c', new Set([2, 3]), 2);
  assert.deepEqual([...favs.keys()], [2, 3]);
});

test('layoutSignature changes with order, grouping and collapse but not with titles', () => {
  const items = [tab(1), tab(2, { g: 5, gl: true, gname: 'A', gcol: 1, gcollapsed: false, gsize: 2 }), tab(3, { g: 5 })];
  const sig = (list, active = 1) => logic.layoutSignature(buildEntries(list, active));
  assert.equal(sig(items), sig(items.map((t) => ({ ...t, title: 'renamed' }))));
  assert.notEqual(sig(items), sig([items[1], items[0], items[2]]));
  assert.notEqual(sig(items), sig(items.map((t) => (t.gl ? { ...t, gcollapsed: true } : t))));
  assert.notEqual(sig(items), sig([items[0], items[1]]));
});

test('stepMoveBefore gives the host the id to the right of the new slot', () => {
  const ids = [10, 11, 12, 13];
  assert.equal(logic.stepMoveBefore(ids, 1, -1), 10);
  assert.equal(logic.stepMoveBefore(ids, 1, 1), 13);
  assert.equal(logic.stepMoveBefore(ids, 2, 1), 0);
  assert.equal(logic.stepMoveBefore(ids, 0, -1), null);
  assert.equal(logic.stepMoveBefore(ids, 3, 1), null);
});

test('groupWeight sums visible members: active 1.5, others 1, hidden none', () => {
  const t = (id, hidden = false) => ({ item: tab(id), hidden });
  assert.equal(ACTIVE_WEIGHT, 1.5);
  assert.equal(groupWeight([t(1), t(2), t(3)], 2), 3.5);
  assert.equal(groupWeight([t(1), t(2), t(3)], 99), 3);
  assert.equal(groupWeight([t(1, true), t(2, true), t(3)], 3), 1.5);
  assert.equal(groupWeight([t(1, true)], 99), 0);
});

test('a group of eight keeps its active tab visible and heaviest', () => {
  const items = Array.from({ length: 8 }, (_, i) => tab(i + 1, { g: 4, gl: i === 0, gname: 'Site', gcol: 1, gcollapsed: false }));
  const [entry] = buildEntries(items, 5);
  assert.equal(entry.kind, 'group');
  assert.equal(entry.visible, 8);
  assert.equal(groupWeight(entry.members, 5), 8.5);
  // Collapsed: only the active member stays, still weighing 1.5.
  const collapsed = items.map((it) => (it.gl ? { ...it, gcollapsed: true } : it));
  const [folded] = buildEntries(collapsed, 5);
  assert.equal(folded.visible, 1);
  assert.equal(groupWeight(folded.members, 5), 1.5);
});

test('tabs.css keeps the strip filled: no tab max-width, active weighs 1.5, groups grow by weight', () => {
  const css = readFileSync(join(here, '..', 'src', 'styles', 'tabs.css'), 'utf8');
  assert.match(css, /\.strip \.tab \{[^}]*flex: 1 1 0;/);
  assert.match(css, /\.strip \.tab\.active \{[^}]*flex-grow: 1\.5;/);
  assert.match(css, /\.strip \.group \{[^}]*flex: var\(--gw, 1\) 1 calc\(/);
  const tab = /\.strip \.tab \{[^}]*\}/.exec(css)?.[0] ?? '';
  assert.doesNotMatch(tab, /max-width/);
});

test('a collapsed group is one drop slot: dropping before it targets its first member', () => {
  // Group g1 = tabs 2,3,4 with 3 (active) the only visible member; tab 5 follows.
  const leaders = new Map([[3, 2]]);
  assert.equal(beforeSlotId([1, 3, 5], 1, leaders), 2, 'before the group, not between its members');
  assert.equal(beforeSlotId([1, 3, 5], 2, leaders), 5, 'after the group is before the next tab');
  assert.equal(beforeSlotId([1, 3, 5], 3, leaders), 0, 'past the last slot means last');
  assert.equal(beforeSlotId([1, 3, 5], 0, leaders), 1);
});

test('automatic groups (no colour id) get a stable palette colour, explicit ids are kept', () => {
  assert.equal(groupColourIndex(5, 1, 'x'), 5);
  assert.equal(groupColourIndex(9, 1, 'x'), 1, 'ids wrap into the 8-colour palette');
  const a = groupColourIndex(-1, 4, 'wikipedia.org');
  assert.equal(a, groupColourIndex(undefined, 99, 'wikipedia.org'), 'depends on the name, not the key');
  assert.ok(a >= 0 && a < 8);
  const used = new Set(['wikipedia.org', 'github.com', 'bbc.com', 'rust-lang.org', 'docs.rs', 'crates.io'].map((n) => groupColourIndex(-1, 1, n)));
  assert.ok(used.size >= 3, 'different sites spread over the palette');
});
