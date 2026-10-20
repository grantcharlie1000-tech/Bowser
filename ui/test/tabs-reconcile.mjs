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
const { flipInvert, landingFirst, landingPhase, beforeSlotId, groupColourIndex, ACTIVE_WEIGHT, groupWeight, privateTabName, beforeId, buildEntries, dragScrollDelta, dropIndex, hiddenForTab, pruneFavs, rememberFav, scrollDeltaForWheel, shiftTarget, slotShift } = logic;

const tab = (id, extra = {}) => ({
  id,
  title: `t${id}`,
  loading: false,
  pinned: false,
  muted: false,
  private: false,
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

test('a keyboard step over a collapsed group lands before its first member, as a drag does (U2)', () => {
  // Visible [1, 11, 2]: group 7 = 10, 11 (shown, active), 12 is collapsed.
  const visible = [1, 11, 2];
  const leaders = new Map([[11, 10]]);
  assert.equal(logic.stepMoveBefore(visible, 2, -1, leaders), 10, 'not between the hidden members');
  assert.equal(logic.stepMoveBefore(visible, 2, -1, leaders), beforeSlotId([1, 11], 1, leaders), 'same as a drag');
  assert.equal(logic.stepMoveBefore(visible, 0, 1, leaders), 2, 'past the group is before the next tab');
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

test('private tabs weigh, order and lay out exactly like normal tabs (the badge takes no room)', () => {
  const plain = [
    tab(1, { pinned: true }),
    tab(2, { g: 5, gl: true, gname: 'Work', gcol: 2, gcollapsed: false, gsize: 2 }),
    tab(3),
    tab(4, { g: 5 }),
    tab(5),
    tab(6),
  ];
  // Private tabs are never grouped: mark the pinned one and two loose ones.
  const mixed = plain.map((t) => ([1, 3, 6].includes(t.id) ? { ...t, private: true } : t));
  for (const active of [1, 3, 4, 6, 99]) {
    const a = buildEntries(plain, active);
    const b = buildEntries(mixed, active);
    assert.equal(logic.layoutSignature(b), logic.layoutSignature(a), `signature, active ${active}`);
    assert.deepEqual(b.map((e) => e.kind), a.map((e) => e.kind));
    const ids = (entries) => entries.map((e) => (e.kind === 'tab' ? e.slot.item.id : e.members.map((s) => s.item.id)));
    assert.deepEqual(ids(b), ids(a));
    const weights = (entries) => entries.map((e) => (e.kind === 'group' ? groupWeight(e.members, active) : e.slot.item.id === active ? ACTIVE_WEIGHT : 1));
    assert.deepEqual(weights(b), weights(a), `fill-the-row weights, active ${active}`);
  }
  assert.equal(privateTabName('Mail'), 'Mail (Private)');
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

test('landingPhase: only a changed layout reorders; the layout the drop left is stale', () => {
  assert.equal(landingPhase(null, 'a b c'), 'none');
  assert.equal(landingPhase('a b c', 'a b c'), 'stale');
  assert.equal(landingPhase('a b c', 'b a c'), 'reorder');
});

// Slots of 100px: tabs a b c at 0, 100, 200. Tab a is dragged 250px right and dropped after c.
// Release: the dropped tab sits at home + offset (0 + 250); the host reorders to b c a, so a's slot is 200.
test('drop FLIP: a dragged tab travels from the release point to its slot, never back through the origin', () => {
  const home = 0;
  const dx = 250;
  const first = landingFirst(home, dx);
  assert.equal(first, 250);
  const last = 200; // new slot, measured once the drag transform is cleared and the order applied
  const invert = flipInvert(first, last);
  assert.equal(invert, 50);
  // The tab starts at last + invert (the release point) and eases to last: every position in between
  // lies between 250 and 200, so it never reaches its origin slot (0).
  for (let t = 0; t <= 1; t += 0.25) {
    const x = last + invert * (1 - t);
    assert.ok(x >= last && x <= first, 'monotonic between release point and slot');
    assert.ok(x > home, 'does not pass the origin');
  }
  // The old bug re-read the leftover transform: first = rect incl. dx, last = newHome + dx, which inverted to
  // oldHome - newHome (the origin). The explicit offset arithmetic does not.
  const buggyInvert = flipInvert(home + dx, 200 + dx);
  assert.equal(buggyInvert, -200);
  assert.notEqual(invert, buggyInvert);
});

test('drop FLIP: a leftward drag and a neighbour close up by their own displacement', () => {
  // Tab c at 200 dragged 150 left to 50 and dropped before a; reordered to c a b: c's slot is 0.
  assert.equal(flipInvert(landingFirst(200, -150), 0), 50);
  // Neighbour a, shifted +100 by the slide, reads its rect at 100 and lands at 100: no residual.
  assert.equal(flipInvert(100, 100), 0);
});

test('tabs.ts: one landing flag, an honest shift and a single 167ms ease-out (no second trip through the origin)', () => {
  const ts = readFileSync(join(here, '..', 'src', 'tabs.ts'), 'utf8');
  const css = readFileSync(join(here, '..', 'src', 'styles', 'tabs.css'), 'utf8');
  assert.doesNotMatch(ts, /settling|settleTimer/, 'the settling flag and its timer are replaced by landing');
  assert.match(ts, /let landing: Landing \| null/);
  assert.match(ts, /d\.node\.shift = d\.dx;/, 'the dragged tab tracks its transform in shift');
  assert.match(ts, /n\.root\.style\.transform === ''/, 'setShift(n, 0) clears the dragged tab even when shift was 0');
  assert.match(ts, /landingPhase\(landing\?\.signature \?\? null, nextSignature\)/, 'a stale render never touches transforms');
  assert.match(ts, /classList\.add\('landing'\);\s*d\.node\.root\.classList\.remove\('dragging'\);/, '.dragging hands over to .landing, never both dropped');
  assert.match(ts, /scale\(\$\{LIFT_SCALE\}\)/, 'the lift rides inside the landing transform');
  assert.match(ts, /ev\.propertyName === 'transform'/);
  assert.match(css, /\.strip \.tab\.landing \{ z-index: 5; \}/);
  assert.match(css, /transition: transform var\(--bw-dur-base\) var\(--bw-ease-out\), box-shadow var\(--bw-dur-base\) var\(--bw-ease-out\);/);
  assert.doesNotMatch(css, /\.tab\.landing[^}]*transition/, 'landing adds no second transition');
});
