// Tab lift (docs/design/tab-drag.md): the wire contract with Rust (src/chrome_host/tab_drag.rs,
// tab_drag_messages_match_committed_fixture), the attached/floating state machine, the gap offsets,
// the drop decision, and the page-side ghost session against a fake host bridge.
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
const { parseTabGhost, validTabDragCommand, startCommand, settleCommand, dropCommand } = await load('tab-drag-protocol.ts');
const logic = await load('tab-drag-logic.ts');
const { parseInbound } = await load('protocol.ts');
const { overPage, overPageBeside, DRAG_OUT_PX } = await load('split-logic.ts');
const fixture = JSON.parse(readFileSync(join(here, 'fixtures', 'tab-drag-messages.json'), 'utf8'));

test('Rust tabghost messages parse across the bridge; anything else is dropped', () => {
  assert.deepEqual(parseInbound(fixture.shown), { t: 'tabghost', a: 'shown', id: 4 });
  assert.deepEqual(parseInbound(fixture.land), { t: 'tabghost', a: 'land', id: 4 });
  assert.deepEqual(parseInbound(fixture.gone), { t: 'tabghost', a: 'gone', id: 4 });
  for (const bad of [
    '{"v":1,"t":"tabghost","a":"shown"}',
    '{"v":1,"t":"tabghost","a":"shown","id":0}',
    '{"v":1,"t":"tabghost","a":"lift","id":4}',
    '{"v":2,"t":"tabghost","a":"shown","id":4}',
    '{"v":1,"t":"tabghost","a":"shown","id":4,"x":1}',
    '{"v":1,"t":"tabghost","a":"shown","id":"4"}',
  ]) assert.equal(parseInbound(bad), null, bad);
  assert.equal(parseTabGhost({ v: 1, t: 'tabghost', a: 'land', id: 1.5 }), null);
});

test('page commands match what Rust accepts and rejects', () => {
  for (const raw of fixture.accepted) assert.equal(validTabDragCommand(JSON.parse(raw)), true, raw);
  for (const raw of fixture.rejected) assert.equal(validTabDragCommand(JSON.parse(raw)), false, raw);
  assert.equal(JSON.stringify(startCommand(3, 'tab', 180, 26, 40, 12, true)), fixture.accepted[0]);
  assert.equal(JSON.stringify(startCommand(9, 'tile', 32, 32, 32, 0, false)), fixture.accepted[1]);
  assert.equal(JSON.stringify(settleCommand(3, 120, 7)), fixture.accepted[3]);
  assert.equal(JSON.stringify(dropCommand(3, 40, 4)), fixture.accepted[4]);
  // Every message stays within Rust's six-field cap.
  for (const raw of fixture.accepted) assert.ok(Object.keys(JSON.parse(raw)).length <= 6, raw);
});

test('attached <-> floating: detach past DETACH_PX, re-attach within REATTACH_PX (hysteresis)', () => {
  const { nextPhase, stripBand, sidebarBand, bandDistance, DETACH_PX, REATTACH_PX } = logic;
  assert.ok(DETACH_PX > REATTACH_PX);
  assert.ok(REATTACH_PX >= DRAG_OUT_PX, 'a floating tab is always over the page for the split zones');
  const strip = stripBand({ left: 0, top: 0, right: 900, bottom: 34 });
  // Inside, and far along the strip in x: always attached (the strip spans the window).
  assert.equal(nextPhase('attached', strip, 450, 20), 'attached');
  assert.equal(nextPhase('attached', strip, -500, 20), 'attached');
  assert.equal(nextPhase('attached', strip, 5000, 20), 'attached');
  // Down: still attached inside the tolerance, floating beyond it.
  assert.equal(nextPhase('attached', strip, 450, 34 + DETACH_PX), 'attached');
  assert.equal(nextPhase('attached', strip, 450, 34 + DETACH_PX + 1), 'floating');
  // Up, above the strip (out of a restored window): floating too.
  assert.equal(nextPhase('attached', strip, 450, -DETACH_PX - 1), 'floating');
  // Hysteresis: between the two thresholds the phase is kept either way.
  const between = 34 + (DETACH_PX + REATTACH_PX) / 2;
  assert.equal(nextPhase('attached', strip, 450, between), 'attached');
  assert.equal(nextPhase('floating', strip, 450, between), 'floating');
  assert.equal(nextPhase('floating', strip, 450, 34 + REATTACH_PX), 'attached');
  // The floating band agrees with the split logic: floating below the strip is over the page.
  for (let y = 35; y < 200; y++) {
    if (nextPhase('floating', strip, 450, y) === 'floating') assert.equal(overPage(y, 34), true, `y=${y}`);
  }
  // The sidebar detaches only to the right (towards the page); up and down it auto-scrolls.
  const side = sidebarBand({ left: 0, top: 40, right: 240, bottom: 800 });
  assert.equal(nextPhase('attached', side, 120, -300), 'attached');
  assert.equal(nextPhase('attached', side, 120, 5000), 'attached');
  assert.equal(nextPhase('attached', side, 240 + DETACH_PX + 1, 300), 'floating');
  assert.equal(overPageBeside(240 + DETACH_PX + 1, 240), true);
  assert.equal(nextPhase('floating', side, 240 + REATTACH_PX, 300), 'attached');
  assert.equal(bandDistance(side, 100, 100), 0);
  assert.equal(bandDistance(strip, 10, 50), 16);
});

test('grab offset: the point pressed stays under the cursor, clamped to the tab', () => {
  const { grabOffset, ghostOrigin } = logic;
  const rect = { left: 100.4, top: 4, width: 180.2, height: 26 };
  assert.deepEqual(grabOffset(140, 16, rect), { gx: 40, gy: 12 });
  assert.deepEqual(grabOffset(50, -10, rect), { gx: 0, gy: 0 });
  assert.deepEqual(grabOffset(900, 90, rect), { gx: 180, gy: 26 });
  assert.deepEqual(grabOffset(Number.NaN, 5, rect), { gx: 0, gy: 1 });
  const grab = grabOffset(140, 16, rect);
  // Anywhere the pointer goes (down over the page, diagonally), the tab's origin keeps the offset.
  for (const [x, y] of [[140, 16], [400, 300], [-20, 700]]) {
    const o = ghostOrigin(x, y, grab);
    assert.deepEqual({ x: x - o.x, y: y - o.y }, { x: 40, y: 12 });
  }
});

test('gap offsets: neighbours open the slot at dest, the hidden copy sits in it, floating closes it', () => {
  const { gapShifts } = logic;
  // Four tabs with a 4px gap; index 1 is the wider active tab.
  const widths = [100, 150, 100, 100];
  const lefts = [0, 104, 258, 362];
  assert.deepEqual(gapShifts(lefts, widths, 4, 1, 1, false), [0, 0, 0, 0]);
  // The wide tab to the end: 2 and 3 move left by its width + gap; its slot ends flush at the right.
  assert.deepEqual(gapShifts(lefts, widths, 4, 1, 3, false), [0, 208, -154, -154]);
  // The last tab to slot 1: 1 and 2 move right by its width + gap; its slot starts where 1 started.
  assert.deepEqual(gapShifts(lefts, widths, 4, 3, 1, false), [0, 104, 104, -258]);
  // The resulting boxes never overlap and keep the 4px gap.
  const boxes = (shifts) => lefts.map((l, i) => [l + shifts[i], l + shifts[i] + widths[i]]).sort((a, b) => a[0] - b[0]);
  for (const [from, dest] of [[1, 3], [3, 1], [0, 2], [2, 0]]) {
    const b = boxes(gapShifts(lefts, widths, 4, from, dest, false));
    for (let i = 1; i < b.length; i++) assert.equal(b[i][0] - b[i - 1][1], 4, `${from}->${dest}`);
  }
  // Floating: the tabs after it close the gap; the strip keeps its order.
  assert.deepEqual(gapShifts(lefts, widths, 4, 1, 3, true), [0, 0, -154, -154]);
  assert.deepEqual(gapShifts(lefts, widths, 4, 3, 0, true), [0, 0, 0, 0]);
  // Out-of-range input is harmless.
  assert.deepEqual(gapShifts(lefts, widths, 4, 9, 0, false), [0, 0, 0, 0]);
  assert.deepEqual(gapShifts(lefts, widths, 4, 0, 99, false), [362, -104, -104, -104]);
});

test('drop outcome: reorder, settle, split or cancel', () => {
  const { dropOutcome } = logic;
  assert.equal(dropOutcome(true, 'attached', false, 2, 4), 'reorder');
  assert.equal(dropOutcome(true, 'attached', false, 2, 2), 'settle');
  assert.equal(dropOutcome(true, 'floating', true, 2, 2), 'split');
  assert.equal(dropOutcome(true, 'floating', false, 2, 2), 'cancel', 'released off the strip, not over the page');
  assert.equal(dropOutcome(false, 'attached', false, 2, 4), 'cancel', 'Escape or pointercancel');
  assert.equal(dropOutcome(false, 'floating', true, 2, 2), 'cancel');
});

test('settle points and announcements', () => {
  const { settlePoint, dragAnnouncement } = logic;
  assert.deepEqual(settlePoint(-4.6, 3.5), { x: 0, y: 4 });
  assert.deepEqual(settlePoint(99999, Number.NaN), { x: 16384, y: 0 });
  assert.match(dragAnnouncement('lift', 'Inbox'), /Picked up Inbox.*Escape to cancel/);
  assert.equal(dragAnnouncement('drop', 'Inbox', 3, 7), 'Inbox moved to position 3 of 7.');
  assert.equal(dragAnnouncement('drop', ''), 'Tab dropped.');
  assert.match(dragAnnouncement('cancel', 'Inbox'), /cancelled/);
});

// ----- the page-side session against a fake bridge -----

const sent = [];
const timers = new Map();
let nextTimer = 1;
globalThis.window = {
  chrome: { webview: { postMessage: (wire) => sent.push(JSON.parse(wire)) } },
  setInterval: (fn, ms) => { const id = nextTimer++; timers.set(id, { fn, ms, repeat: true }); return id; },
  clearInterval: (id) => timers.delete(id),
  setTimeout: (fn, ms) => { const id = nextTimer++; timers.set(id, { fn, ms, repeat: false }); return id; },
  clearTimeout: (id) => timers.delete(id),
};
const fire = (ms) => {
  for (const [id, t] of [...timers]) {
    if (t.ms !== ms) continue;
    if (!t.repeat) timers.delete(id);
    t.fn();
  }
};
const { GhostSession, routeGhostMessage } = await load('tab-drag.ts');

const hooks = () => {
  const calls = [];
  return { calls, shown: () => calls.push('shown'), lost: () => calls.push('lost'), reveal: () => calls.push('reveal') };
};

test('session: start, shown, phase changes only, settle, land reveals', () => {
  sent.length = 0;
  const h = hooks();
  const s = GhostSession.start(5, 'tab', 180.4, 26, 40, 12, true, 'mouse', h);
  assert.ok(s);
  assert.deepEqual(sent[0], { v: 1, t: 'tabdrag', a: 'start', id: 5, k: 'tab', r: '180,26,40,12,1' });
  assert.equal(s.live, false, 'not live until Rust says shown');
  routeGhostMessage({ t: 'tabghost', a: 'shown', id: 99 });
  assert.deepEqual(h.calls, [], 'another tab id is ignored');
  routeGhostMessage({ t: 'tabghost', a: 'shown', id: 5 });
  assert.deepEqual(h.calls, ['shown']);
  assert.equal(s.live, true);
  s.phase(false);
  assert.equal(sent.length, 1, 'no change, nothing sent');
  s.phase(true);
  s.phase(true);
  assert.deepEqual(sent.slice(1), [{ v: 1, t: 'tabdrag', a: 'move', id: 5, f: true }]);
  fire(logic.GHOST_KEEPALIVE_MS);
  assert.deepEqual(sent.at(-1), { v: 1, t: 'tabdrag', a: 'move', id: 5, f: true }, 'keepalive');
  s.end('settle', 120.4, 7);
  assert.deepEqual(sent.at(-1), { v: 1, t: 'tabdrag', a: 'end', id: 5, m: 'settle', p: '120,7' });
  assert.equal(s.landing, true);
  assert.deepEqual(h.calls, ['shown'], 'the copy stays hidden while the ghost glides');
  s.retarget(130, 7);
  assert.deepEqual(sent.at(-1), { v: 1, t: 'tabdrag', a: 'end', id: 5, m: 'settle', p: '130,7' });
  const count = sent.length;
  fire(logic.GHOST_KEEPALIVE_MS);
  assert.equal(sent.length, count, 'no keepalive after the release');
  routeGhostMessage({ t: 'tabghost', a: 'land', id: 5 });
  assert.deepEqual(h.calls, ['shown', 'reveal']);
  assert.equal(s.landing, false);
  routeGhostMessage({ t: 'tabghost', a: 'land', id: 5 });
  assert.deepEqual(h.calls, ['shown', 'reveal'], 'revealed once');
});

test('session: no land within the fallback still reveals; a lost ghost hands the drag back', () => {
  sent.length = 0;
  const h = hooks();
  const s = GhostSession.start(6, 'row', 200, 26, 3, 3, true, 'mouse', h);
  routeGhostMessage({ t: 'tabghost', a: 'shown', id: 6 });
  routeGhostMessage({ t: 'tabghost', a: 'gone', id: 6 });
  assert.deepEqual(h.calls, ['shown', 'lost'], 'watchdog took it down mid-drag');
  assert.equal(s.live, false);
  routeGhostMessage({ t: 'tabghost', a: 'shown', id: 6 });
  assert.equal(s.live, true, 'a later shown takes it again');
  s.end('drop', 12.4, 3);
  assert.deepEqual(sent.at(-1), { v: 1, t: 'tabdrag', a: 'end', id: 6, m: 'drop', p: '12,3' });
  fire(logic.GHOST_REVEAL_FALLBACK_MS);
  assert.deepEqual(h.calls, ['shown', 'lost', 'shown', 'reveal']);
});

test('session: never shown ends silently; touch gets none; a new drag abandons the old one', () => {
  sent.length = 0;
  const h = hooks();
  const s = GhostSession.start(7, 'tab', 100, 26, 1, 1, false, 'mouse', h);
  s.end('settle', 0, 0);
  assert.deepEqual(h.calls, [], 'the copy was never hidden');
  assert.equal(s.landing, false);
  assert.equal(GhostSession.start(7, 'tab', 100, 26, 1, 1, false, 'touch', hooks()), null);
  assert.equal(GhostSession.start(7, 'tab', 100, 26, 1, 1, false, 'pen', hooks()), null);
  const h1 = hooks();
  const first = GhostSession.start(8, 'tab', 100, 26, 1, 1, false, 'mouse', h1);
  routeGhostMessage({ t: 'tabghost', a: 'shown', id: 8 });
  first.end('settle', 10, 0);
  const h2 = hooks();
  GhostSession.start(9, 'tab', 100, 26, 1, 1, false, 'mouse', h2);
  assert.deepEqual(h1.calls, ['shown', 'reveal'], 'the landing tab shows at once');
  assert.deepEqual(sent.at(-1), { v: 1, t: 'tabdrag', a: 'start', id: 9, k: 'tab', r: '100,26,1,1,0' });
  // Abandon during the drag sends the immediate end.
  const h3 = hooks();
  const third = GhostSession.start(10, 'tab', 100, 26, 1, 1, false, 'mouse', h3);
  routeGhostMessage({ t: 'tabghost', a: 'shown', id: 10 });
  third.abandon();
  assert.deepEqual(sent.at(-1), { v: 1, t: 'tabdrag', a: 'end', id: 10, m: 'none' });
  assert.deepEqual(h3.calls, ['shown', 'reveal']);
  // An invalid size never reaches the bridge.
  const before = sent.length;
  assert.equal(GhostSession.start(11, 'tab', 100, 26, 500, 1, false, 'mouse', hooks()), null);
  assert.equal(sent.length, before);
});
