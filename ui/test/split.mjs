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
const { pairOrder, pairWeight, pairRole, filterPick, moveSelection, overPage, overPageBeside, wirePoint, pairDescription, pairStacked, roleEdge, splitGlyph, dropCard, pickHeading, SHOWN_HALF_WEIGHT, PARKED_HALF_WEIGHT } = await load('split-logic.ts');
const { parseInbound } = await load('protocol.ts');

const fixture = JSON.parse(readFileSync(join(here, 'fixtures', 'split-messages.json'), 'utf8'));

test('Rust split fixture messages parse across the bridge', () => {
  assert.deepEqual(parseInbound(fixture.pair), { t: 'pair', l: 3, r: 8, f: 'r', shown: true, o: 'h' });
  assert.deepEqual(parseInbound(fixture.pairStacked), { t: 'pair', l: 3, r: 8, f: 'l', shown: true, o: 'v' });
  assert.deepEqual(parseInbound(fixture.pairNone), { t: 'pair', l: 0, r: 0, f: 'l', shown: false, o: 'h' });
  const pick = parseInbound(fixture.pick);
  assert.equal(pick.t, 'splitpick');
  assert.equal(pick.side, 'r');
  assert.equal(pick.tabs.length, 2);
  assert.equal(pick.tabs[0].title, 'Docs <home>');
  assert.match(pick.tabs[0].fav, /^data:image\/png;base64,/);
  assert.deepEqual(pick.tabs[1], { id: 9, title: '', host: '', fav: '', private: true });
  assert.deepEqual(pick.closed, [{ n: 0, title: 'Old page 😀', host: 'old.example' }]);
  assert.deepEqual(parseInbound(fixture.drop), { t: 'splitdrop', side: 'l', title: 'Example' });
  assert.deepEqual(parseInbound(fixture.dropBottom), { t: 'splitdrop', side: 'b', title: 'Below' });
  assert.deepEqual(parseInbound(fixture.pickTop), { t: 'splitpick', side: 't', tabs: [], closed: [] });
  assert.deepEqual(parseInbound(fixture.hide), { t: 'splithide' });
  assert.deepEqual(parseInbound(fixture.private), { t: 'privatepage', id: 4, n: 2 });
});

test('malformed split messages are rejected', () => {
  const bad = [
    { v: 1, t: 'pair', l: 3, r: 3, f: 'l', shown: true, o: 'h' },
    { v: 1, t: 'pair', l: 3, r: 0, f: 'l', shown: false, o: 'h' },
    { v: 1, t: 'pair', l: 0, r: 0, f: 'l', shown: true, o: 'h' },
    { v: 1, t: 'pair', l: 3, r: 4, f: 'x', shown: true, o: 'h' },
    { v: 1, t: 'pair', l: 3, r: 4, f: 'l', shown: true, o: 'h', extra: 1 },
    { v: 1, t: 'pair', l: -1, r: 4, f: 'l', shown: true, o: 'h' },
    { v: 2, t: 'pair', l: 3, r: 4, f: 'l', shown: true, o: 'h' },
    // The orientation is required and is exactly h or v; the focused slot stays l/r when stacked.
    { v: 1, t: 'pair', l: 3, r: 4, f: 'l', shown: true },
    { v: 1, t: 'pair', l: 3, r: 4, f: 'l', shown: true, o: 'x' },
    { v: 1, t: 'pair', l: 3, r: 4, f: 'l', shown: true, o: 'V' },
    { v: 1, t: 'pair', l: 3, r: 4, f: 't', shown: true, o: 'v' },
    { v: 1, t: 'splitdrop', side: 'x', title: 'a' },
    { v: 1, t: 'splitdrop', side: 'top', title: 'a' },
    { v: 1, t: 'splitpick', side: 'B', tabs: [], closed: [] },
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

test('stacked pairs name their halves top and bottom; the edges map one to one', () => {
  const stacked = { t: 'pair', l: 4, r: 2, f: 'r', shown: true, o: 'v' };
  const side = { ...stacked, o: 'h' };
  assert.equal(pairStacked(stacked), true);
  assert.equal(pairStacked(side), false);
  assert.equal(pairStacked({ t: 'pair', l: 4, r: 2, f: 'r', shown: true }), false, 'no o reads as side by side');
  assert.equal(pairStacked(null), false);
  assert.deepEqual(['l', 'r'].map((role) => roleEdge(role, stacked)), ['t', 'b']);
  assert.deepEqual(['l', 'r'].map((role) => roleEdge(role, side)), ['l', 'r']);
  assert.equal(pairDescription('l', stacked), 'Top side of split view');
  assert.equal(pairDescription('r', stacked), 'Bottom side of split view, focused');
  assert.equal(pairDescription('r', side), 'Right side of split view, focused');
  // The toolbar glyph fills the focused half; a parked pair or none shows the plain glyph.
  assert.equal(splitGlyph(stacked), 'split-bottom');
  assert.equal(splitGlyph({ ...stacked, f: 'l' }), 'split-top');
  assert.equal(splitGlyph(side), 'split-right');
  assert.equal(splitGlyph({ ...side, f: 'l' }), 'split-left');
  assert.equal(splitGlyph({ ...stacked, shown: false }), 'split');
  assert.equal(splitGlyph(null), 'split');
  assert.equal(splitGlyph({ t: 'pair', l: 0, r: 0, f: 'l', shown: false, o: 'h' }), 'split');
  // The drop card and the empty side's heading name all four halves.
  assert.deepEqual(['l', 'r', 't', 'b'].map((edge) => dropCard(edge).label), ['Open on the left', 'Open on the right', 'Open at the top', 'Open at the bottom']);
  assert.deepEqual(['l', 'r', 't', 'b'].map((edge) => dropCard(edge).glyph), ['split-left', 'split-right', 'split-top', 'split-bottom']);
  assert.deepEqual(['l', 'r', 't', 'b'].map(pickHeading), [
    'Choose a tab for the left side', 'Choose a tab for the right side', 'Choose a tab for the top side', 'Choose a tab for the bottom side',
  ]);
});

test('a drag leaves the strip downwards or the sidebar to the right before it is over the page', () => {
  // Horizontal strip: below its bottom edge by more than the slop.
  assert.equal(overPage(52, 40), false);
  assert.equal(overPage(53, 40), true);
  // Vertical sidebar: right of its edge by more than the slop.
  assert.equal(overPageBeside(250, 240), false);
  assert.equal(overPageBeside(253, 240), true);
  assert.equal(overPageBeside(-400, 240), false);
  // The point sent to Rust is clamped into the wire range wherever the pointer is.
  for (const [x, y] of [[-1e9, -1e9], [1e9, 1e9], [Number.POSITIVE_INFINITY, Number.NaN]]) {
    const p = wirePoint(x, y);
    assert.ok(p.x >= 0 && p.x <= 16384 && p.y >= 0 && p.y <= 16384, JSON.stringify(p));
  }
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

// ----- layouts of three or four panes (corner drops) -----

const { layoutIds, paneAt, paneFocused, paneDescription, SHOWN_PANE_WEIGHT } = await load('split-logic.ts');

test('layout pair messages from Rust parse with every pane and the focused place', () => {
  const quad = parseInbound(fixture.quad);
  assert.deepEqual(quad, {
    t: 'pair', l: 3, r: 8, f: 'r', shown: true, o: 'h',
    panes: [{ id: 3, at: 'tl' }, { id: 5, at: 'bl' }, { id: 8, at: 'tr' }, { id: 9, at: 'br' }],
    fp: 'br',
  });
  const tee = parseInbound(fixture.tee);
  assert.deepEqual(tee.panes.map((pane) => pane.at), ['tl', 'tr', 'b']);
  assert.equal(tee.shown, false);
  assert.deepEqual(parseInbound(fixture.pickCorner), { t: 'splitpick', side: 'bl', tabs: [], closed: [] });
  assert.deepEqual(parseInbound(fixture.dropCorner), { t: 'splitdrop', side: 'tr', title: 'Corner' });
});

test('malformed layouts are rejected', () => {
  const base = { v: 1, t: 'pair', l: 3, r: 8, f: 'l', shown: true, o: 'h' };
  const quad = [{ id: 3, at: 'tl' }, { id: 5, at: 'bl' }, { id: 8, at: 'tr' }, { id: 9, at: 'br' }];
  assert.ok(parseSplitInbound({ ...base, panes: quad, fp: 'tl' }));
  for (const bad of [
    { ...base, panes: quad },                                                        // fp missing
    { ...base, fp: 'tl' },                                                           // panes missing
    { ...base, panes: quad, fp: 'l' },                                               // fp not a pane's place
    { ...base, panes: quad.slice(0, 2), fp: 'tl' },                                  // a pair never carries panes
    { ...base, panes: [...quad, { id: 10, at: 'l' }], fp: 'tl' },                    // five panes
    { ...base, panes: [{ id: 3, at: 'l' }, { id: 5, at: 'tl' }, { id: 8, at: 'r' }], fp: 'l' }, // overlapping places
    { ...base, panes: [{ id: 3, at: 'tl' }, { id: 5, at: 'bl' }, { id: 8, at: 'tr' }], fp: 'tl' }, // a quarter uncovered
    { ...base, panes: [{ id: 3, at: 'tl' }, { id: 3, at: 'bl' }, { id: 8, at: 'r' }], fp: 'tl' },  // a tab twice
    { ...base, panes: [{ id: 4, at: 'tl' }, { id: 5, at: 'bl' }, { id: 8, at: 'r' }], fp: 'tl' },  // l not among them
    { ...base, panes: [{ id: 3, at: 'tl', x: 1 }, { id: 5, at: 'bl' }, { id: 8, at: 'r' }], fp: 'tl' },
    { ...base, panes: [{ id: 3, at: 'TL' }, { id: 5, at: 'bl' }, { id: 8, at: 'r' }], fp: 'tl' },
    { ...base, l: 0, r: 0, shown: false, panes: quad, fp: 'tl' },
    { v: 1, t: 'splitdrop', side: 'lt', title: 'a' },
    { v: 1, t: 'splitpick', side: 'tlr', tabs: [], closed: [] },
  ]) {
    assert.equal(parseSplitInbound(bad), null, JSON.stringify(bad));
  }
});

test('a layout of N panes is one joined capsule in tree order', () => {
  const tab = (id, extra = {}) => ({ id, title: `T${id}`, loading: false, pinned: false, muted: false, private: false, marker: 0, g: 0, gl: false, fav: '', ...extra });
  const items = [tab(1), tab(3), tab(2), tab(5), tab(8), tab(9)];
  const quad = parseInbound(fixture.quad); // panes 3, 5, 8, 9
  assert.deepEqual(layoutIds(quad), [3, 5, 8, 9]);
  assert.deepEqual(pairOrder(items, quad).map((item) => item.id), [1, 3, 5, 8, 9, 2]);
  assert.deepEqual([3, 5, 8, 9].map((id) => pairRole(id, quad)), ['l', 'm', 'm', 'r']);
  assert.equal(pairRole(2, quad), null);
  // A pair keeps its two roles.
  assert.deepEqual([3, 8].map((id) => pairRole(id, fixture.pair)), ['l', 'r']);
  // Weights: shown panes 1 each, parked 0.7, others unchanged.
  assert.equal(pairWeight(5, quad, 1.5), SHOWN_PANE_WEIGHT);
  assert.equal(pairWeight(5, { ...quad, shown: false }, 1.5), PARKED_HALF_WEIGHT);
  assert.equal(pairWeight(2, quad, 1.5), 1.5);
  // A member missing from the list: unchanged.
  assert.equal(pairOrder(items.filter((item) => item.id !== 9), quad).length, 5);
  // Groups: members join the first tab's group; a member that led another group hands it on.
  const grouped = pairOrder([tab(3, { g: 1, gl: true }), tab(5, { g: 2, gl: true, gname: 'G' }), tab(6, { g: 2 }), tab(8), tab(9)], quad);
  assert.deepEqual(grouped.map((item) => [item.id, item.g, item.gl]), [[3, 1, true], [5, 1, false], [8, 1, false], [9, 1, false], [6, 2, true]]);
  assert.equal(grouped[4].gname, 'G');
});

test('panes of a layout are named and the focused one is marked', () => {
  const quad = parseInbound(fixture.quad);
  assert.deepEqual([3, 5, 8, 9].map((id) => paneAt(id, quad)), ['tl', 'bl', 'tr', 'br']);
  assert.equal(paneFocused(9, quad), true);
  assert.equal(paneFocused(3, quad), false);
  assert.equal(paneDescription(3, quad), 'Top-left pane of split view');
  assert.equal(paneDescription(9, quad), 'Bottom-right pane of split view, focused');
  assert.equal(paneDescription(9, { ...quad, shown: false }), 'Bottom-right pane of split view');
  const tee = parseInbound(fixture.tee);
  assert.equal(paneDescription(8, tee), 'Bottom pane of split view');
  // Pairs keep their wording.
  assert.equal(paneDescription(8, fixture.pair), 'Right side of split view, focused');
  assert.equal(paneAt(3, parseInbound(fixture.pairStacked)), 't');
  assert.equal(splitGlyph(quad), 'split-bottom-right');
  assert.equal(splitGlyph({ ...quad, fp: 'tl' }), 'split-top-left');
  assert.equal(splitGlyph({ ...tee, shown: true }), 'split-top-left');
  assert.equal(splitGlyph({ ...tee, shown: true, fp: 'b' }), 'split-bottom');
  assert.equal(splitGlyph({ ...quad, shown: false }), 'split');
});

test('corner drop cards and corner pickers have their own words and glyphs', () => {
  assert.deepEqual(['tl', 'tr', 'bl', 'br'].map((side) => dropCard(side).label), ['Open at the top left', 'Open at the top right', 'Open at the bottom left', 'Open at the bottom right']);
  assert.deepEqual(['tl', 'tr', 'bl', 'br'].map((side) => dropCard(side).glyph), ['split-top-left', 'split-top-right', 'split-bottom-left', 'split-bottom-right']);
  assert.equal(pickHeading('br'), 'Choose a tab for the bottom-right pane');
  assert.equal(pickHeading('l'), 'Choose a tab for the left side');
  // The drop point sent from the strip keeps its wire shape and clamping.
  assert.deepEqual(wirePoint(-4.6, 99999), { x: 0, y: 16384 });
  assert.ok(validSplitCommand({ v: 1, t: 'split', a: 'over', id: 3, x: 10, y: 20 }));
});
