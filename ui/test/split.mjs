// Split view contract and logic: the Rust fixture (src/chrome_host/split.rs,
// split_messages_match_committed_fixture) parses here, malformed messages are rejected, outbound
// commands are validated, and the strip's pair ordering and weights hold.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const load = async (file) => {
  const out = esbuild.buildSync({ entryPoints: [join(here, '..', 'src', file)], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
};
const { parseSplitInbound, validSplitCommand } = await load('split-protocol.ts');
const { pairOrder, pairWeight, pairRole, filterPick, moveSelection, overPage, wirePoint, pairDescription, SHOWN_HALF_WEIGHT, PARKED_HALF_WEIGHT } = await load('split-logic.ts');
const { parseInbound } = await load('protocol.ts');

const fixture = JSON.parse(readFileSync(join(here, 'fixtures', 'split-messages.json'), 'utf8'));

test('Rust split fixture messages parse across the bridge', () => {
  assert.deepEqual(parseInbound(fixture.pair), { t: 'pair', l: 3, r: 8, f: 'r', shown: true });
  assert.deepEqual(parseInbound(fixture.pairNone), { t: 'pair', l: 0, r: 0, f: 'l', shown: false });
  const pick = parseInbound(fixture.pick);
  assert.equal(pick.t, 'splitpick');
  assert.equal(pick.side, 'r');
  assert.equal(pick.tabs.length, 2);
  assert.equal(pick.tabs[0].title, 'Docs <home>');
  assert.match(pick.tabs[0].fav, /^data:image\/png;base64,/);
  assert.deepEqual(pick.tabs[1], { id: 9, title: '', host: '', fav: '', private: true });
  assert.deepEqual(pick.closed, [{ n: 0, title: 'Old page 😀', host: 'old.example' }]);
  assert.deepEqual(parseInbound(fixture.drop), { t: 'splitdrop', side: 'l', title: 'Example' });
  assert.deepEqual(parseInbound(fixture.hide), { t: 'splithide' });
  assert.deepEqual(parseInbound(fixture.private), { t: 'privatepage', id: 4, n: 2 });
});

test('malformed split messages are rejected', () => {
  const bad = [
    { v: 1, t: 'pair', l: 3, r: 3, f: 'l', shown: true },
    { v: 1, t: 'pair', l: 3, r: 0, f: 'l', shown: false },
    { v: 1, t: 'pair', l: 0, r: 0, f: 'l', shown: true },
    { v: 1, t: 'pair', l: 3, r: 4, f: 'x', shown: true },
    { v: 1, t: 'pair', l: 3, r: 4, f: 'l', shown: true, extra: 1 },
    { v: 1, t: 'pair', l: -1, r: 4, f: 'l', shown: true },
    { v: 2, t: 'pair', l: 3, r: 4, f: 'l', shown: true },
    { v: 1, t: 'splitpick', side: 'r', tabs: [{ id: 1, title: 't', host: 'h', fav: 'javascript:x', private: false }], closed: [] },
    { v: 1, t: 'splitpick', side: 'r', tabs: [{ id: 1, title: 't', host: 'h', fav: '', private: false }, { id: 1, title: 't', host: 'h', fav: '', private: false }], closed: [] },
    { v: 1, t: 'splitpick', side: 'r', tabs: [{ id: 0, title: 't', host: 'h', fav: '', private: false }], closed: [] },
    { v: 1, t: 'splitpick', side: 'r', tabs: [{ id: 1, title: 't', host: 'h', fav: '' }], closed: [] },
    { v: 1, t: 'splitpick', side: 'r', tabs: [], closed: [{ n: 1, title: 'a', host: 'b' }] },
    { v: 1, t: 'splitpick', side: 'r', tabs: Array.from({ length: 101 }, (_, i) => ({ id: i + 1, title: 't', host: 'h', fav: '', private: false })), closed: [] },
    { v: 1, t: 'splitpick', side: 'r', tabs: [], closed: Array.from({ length: 9 }, (_, n) => ({ n, title: 't', host: 'h' })) },
    { v: 1, t: 'splitpick', side: 'r', tabs: [{ id: 1, title: 'x'.repeat(513), host: 'h', fav: '', private: false }], closed: [] },
    { v: 1, t: 'splitdrop', side: 'l' },
    { v: 1, t: 'splithide', x: 1 },
    { v: 1, t: 'privatepage', id: 4 },
    { v: 1, t: 'privatepage', id: 0, n: 1 },
    { v: 1, t: 'privatepage', id: 4, n: 0 },
    { v: 1, t: 'privatepage', id: 4, n: 10001 },
    { v: 1, t: 'privatepage', id: 4, n: 1.5 },
    { v: 1, t: 'privatepage', id: 4, n: 1, title: 'x' },
  ];
  for (const raw of bad) assert.equal(parseInbound(raw), null, JSON.stringify(raw).slice(0, 120));
  assert.equal(parseSplitInbound({ v: 1, t: 'nope' }), null);
});

test('outbound split commands carry exactly their fields and stay in range', () => {
  const ok = [
    { v: 1, t: 'split', a: 'pick', id: 4 },
    { v: 1, t: 'split', a: 'reopen', n: 7 },
    { v: 1, t: 'split', a: 'go', text: 'example.com' },
    { v: 1, t: 'split', a: 'close' },
    { v: 1, t: 'split', a: 'focus' },
    { v: 1, t: 'split', a: 'closeprivate' },
    { v: 1, t: 'split', a: 'over', id: 2, x: 0, y: 16384 },
    { v: 1, t: 'split', a: 'drop', id: 2 },
    { v: 1, t: 'split', a: 'end', id: 2 },
  ];
  for (const cmd of ok) assert.ok(validSplitCommand(cmd), JSON.stringify(cmd));
  const bad = [
    { v: 1, t: 'split', a: 'pick', id: 0 },
    { v: 1, t: 'split', a: 'pick', id: 1.5 },
    { v: 1, t: 'split', a: 'pick', id: 4, n: 1 },
    { v: 1, t: 'split', a: 'reopen', n: 8 },
    { v: 1, t: 'split', a: 'go', text: '  ' },
    { v: 1, t: 'split', a: 'go', text: 'x'.repeat(2049) },
    { v: 1, t: 'split', a: 'close', id: 1 },
    { v: 1, t: 'split', a: 'closeprivate', n: 1 },
    { v: 1, t: 'split', a: 'over', id: 2, x: 16385, y: 0 },
    { v: 1, t: 'split', a: 'over', id: 2, x: -1, y: 0 },
    { v: 1, t: 'split', a: 'swap' },
    { v: 2, t: 'split', a: 'focus' },
  ];
  for (const cmd of bad) assert.equal(validSplitCommand(cmd), false, JSON.stringify(cmd).slice(0, 80));
  assert.deepEqual(wirePoint(-5.4, 20000), { x: 0, y: 16384 });
  assert.deepEqual(wirePoint(Number.NaN, 12.6), { x: 0, y: 13 });
});

const tab = (id, extra = {}) => ({ id, title: `T${id}`, loading: false, pinned: false, muted: false, private: false, marker: -1, g: 0, gl: false, fav: false, ...extra });

test('the strip shows the pair as adjacent halves in the left tab\'s group', () => {
  const items = [tab(1), tab(2, { g: 7, gl: true, gname: 'Docs', gcol: 2, gcollapsed: false, gsize: 2 }), tab(3, { g: 7 }), tab(4)];
  const pair = { t: 'pair', l: 4, r: 2, f: 'l', shown: true };
  const out = pairOrder(items, pair);
  assert.deepEqual(out.map((item) => item.id), [1, 3, 4, 2]);
  const right = out.find((item) => item.id === 2);
  assert.equal(right.g, 0);
  assert.equal(right.gl, false);
  assert.equal(right.gname, undefined);
  // Its old group keeps a leader with the group fields.
  const heir = out.find((item) => item.id === 3);
  assert.equal(heir.gl, true);
  assert.equal(heir.gname, 'Docs');
  assert.equal(heir.gsize, 1);
  // No pair, or a pair whose tab is missing: unchanged.
  assert.equal(pairOrder(items, null), items);
  assert.equal(pairOrder(items, { t: 'pair', l: 0, r: 0, f: 'l', shown: false }), items);
  assert.equal(pairOrder(items, { t: 'pair', l: 1, r: 99, f: 'l', shown: true }), items);
  // A pinned left half pins the right half for display.
  const pinned = pairOrder([tab(1, { pinned: true }), tab(2), tab(3)], { t: 'pair', l: 1, r: 3, f: 'r', shown: false });
  assert.deepEqual(pinned.map((item) => [item.id, item.pinned]), [[1, true], [3, true], [2, false]]);
});

test('pair weights keep the shown pair widest and a parked pair narrower than the active tab', () => {
  const shown = { t: 'pair', l: 4, r: 2, f: 'l', shown: true };
  const parked = { ...shown, shown: false };
  assert.equal(pairWeight(4, shown, 1.5), SHOWN_HALF_WEIGHT);
  assert.equal(pairWeight(2, parked, 1), PARKED_HALF_WEIGHT);
  assert.equal(pairWeight(9, shown, 1), 1);
  assert.ok(SHOWN_HALF_WEIGHT * 2 > 1.5);
  assert.ok(PARKED_HALF_WEIGHT * 2 < 1.5);
  assert.equal(pairRole(4, shown), 'l');
  assert.equal(pairRole(2, shown), 'r');
  assert.equal(pairRole(5, shown), null);
  assert.equal(pairDescription('l', shown), 'Left side of split view, focused');
  assert.equal(pairDescription('r', shown), 'Right side of split view');
  assert.equal(pairDescription('l', parked), 'Left side of split view');
});

test('the empty side filters by every word and moves its highlight with wrap', () => {
  const tabs = [
    { id: 1, title: 'Rust docs', host: 'docs.rs', fav: '', private: false },
    { id: 2, title: 'News', host: 'example.com', fav: '', private: false },
  ];
  const closed = [{ n: 0, title: 'Old docs', host: 'old.example' }];
  assert.deepEqual(filterPick(tabs, closed, '').tabs.length, 2);
  assert.deepEqual(filterPick(tabs, closed, 'DOCS').tabs.map((t) => t.id), [1]);
  assert.deepEqual(filterPick(tabs, closed, 'docs').closed.length, 1);
  assert.deepEqual(filterPick(tabs, closed, 'example news').tabs.map((t) => t.id), [2]);
  assert.deepEqual(filterPick(tabs, closed, 'zzz'), { tabs: [], closed: [] });
  assert.equal(moveSelection(3, 0, 'ArrowDown'), 1);
  assert.equal(moveSelection(3, 2, 'ArrowDown'), 0);
  assert.equal(moveSelection(3, 0, 'ArrowUp'), 2);
  assert.equal(moveSelection(3, 1, 'Home'), 0);
  assert.equal(moveSelection(3, 1, 'End'), 2);
  assert.equal(moveSelection(0, 0, 'ArrowDown'), -1);
  assert.equal(overPage(60, 40), true);
  assert.equal(overPage(50, 40), false);
});
