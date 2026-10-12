// --- hover layer ---
// Hover layer contract and logic: the Rust fixture (written by protocol.rs
// hover_messages_write_the_ui_fixture) parses, malformed content is rejected, outbound tip/hover/hact
// commands are validated before they cross the bridge, and the tooltip timing rules hold.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const bundle = (entry) => {
  const out = esbuild.buildSync({
    entryPoints: [join(here, '..', 'src', entry)],
    bundle: true,
    format: 'esm',
    platform: 'neutral',
    write: false,
    logLevel: 'silent',
  });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
};

const posted = [];
globalThis.window = { chrome: { webview: { postMessage: (m) => posted.push(JSON.parse(m)), addEventListener() {} } } };
const { parseInbound, rectWire, isRectWire, sendTip, sendTipOff, sendHover, sendHoverAct, MAX_TIP } = await bundle('protocol.ts');
const { TooltipTiming, HoverReport, TIP_DELAY_MS, TIP_WARM_MS } = await bundle('tooltip-logic.ts');
const fixture = JSON.parse(readFileSync(join(here, 'fixtures', 'hover-messages.json'), 'utf8'));

const take = () => posted.splice(0, posted.length);

test('Rust hover fixture parses as tooltip, card, island and hide', () => {
  assert.deepEqual(parseInbound(fixture.tooltip), { t: 'hover', tok: 6, kind: 'tooltip', text: 'Back' });
  const card = parseInbound(fixture.card);
  assert.equal(card?.kind, 'card');
  assert.equal(card.marker, 2);
  assert.equal(card.markers.length, 5);
  assert.deepEqual(card.rows, [{ title: 'Other', active: true, linked: true, fav: '' }]);
  const island = parseInbound(fixture.island);
  assert.equal(island?.kind, 'island');
  assert.equal(island.hint, 'Example');
  assert.equal(island.close, 'Close Tab Island · 1 Tab');
  assert.deepEqual(parseInbound(fixture.hide), { t: 'hoverhide' });
  // The JSON text form parses the same way.
  assert.deepEqual(parseInbound(JSON.stringify(fixture.card)), card);
});

test('malformed hover content is rejected', () => {
  const card = fixture.card;
  const island = fixture.island;
  for (const bad of [
    { ...fixture.tooltip, extra: 1 },
    { ...fixture.tooltip, tok: 0 },
    { ...fixture.tooltip, v: 2 },
    { ...fixture.tooltip, text: 'x'.repeat(MAX_TIP + 1) },
    { ...fixture.tooltip, kind: 'menu' },
    { ...card, marker: 5 },
    { ...card, markers: ['url(x)'] },
    { ...card, fav: 'data:image/gif;base64,AAAA' },
    { ...card, rows: Array.from({ length: 13 }, () => card.rows[0]) },
    { ...card, rows: [{ ...card.rows[0], extra: true }] },
    { ...card, rows: [{ title: 'x', active: true, linked: false }] },
    { ...card, more: -1 },
    (() => { const { sleeping, ...rest } = card; return rest; })(),
    { ...island, slot: 2 },
    { ...island, name: 'n'.repeat(65) },
    { ...island, swatches: Array.from({ length: 33 }, () => '#fff') },
    { ...island, swatches: ['expression(1)'] },
    { ...fixture.hide, x: 1 },
  ]) {
    assert.equal(parseInbound(bad), null, JSON.stringify(bad).slice(0, 120));
  }
});

test('rectangles are clamped, rounded outwards and match the Rust grammar', () => {
  assert.equal(rectWire({ left: 10.4, top: 2.6, right: 38.2, bottom: 30 }), '10,2,29,28');
  assert.equal(rectWire({ left: -5, top: -1, right: 20, bottom: 10 }), '0,0,20,10');
  assert.equal(rectWire({ left: 0, top: 0, right: 99999, bottom: 1 }), '0,0,16384,1');
  assert.equal(rectWire({ left: NaN, top: 0, right: 1, bottom: 1 }), null);
  for (const ok of ['0,0,0,0', '12,6,28,34', '16384,16384,16384,16384']) assert.equal(isRectWire(ok), true, ok);
  for (const bad of ['', '1,2,3', '-1,2,3,4', '01,2,3,4', '1, 2,3,4', '16385,0,0,0', '1,2,3,4,']) assert.equal(isRectWire(bad), false, bad);
});

test('outbound tip, hover and hact commands are validated', () => {
  take();
  assert.equal(sendTip('Back', { left: 4, top: 6, right: 32, bottom: 34 }), true);
  assert.equal(sendTip('   ', { left: 0, top: 0, right: 1, bottom: 1 }), false);
  assert.equal(sendTip('😀'.repeat(MAX_TIP + 5), { left: 0, top: 0, right: 1, bottom: 1 }), true);
  assert.equal(sendTipOff(), true);
  assert.equal(sendHover('tab', 3, { left: 0, top: 0, right: 200, bottom: 34 }), true);
  assert.equal(sendHover('ball', -1, { left: 0, top: 0, right: 1, bottom: 1 }), false);
  assert.equal(sendHover('tab', 3), false);
  assert.equal(sendHover('none'), true);
  const sent = take();
  assert.deepEqual(sent[0], { v: 1, t: 'tip', text: 'Back', r: '4,6,28,28' });
  assert.equal(Array.from(sent[1].text).length, MAX_TIP);
  assert.deepEqual(sent[2], { v: 1, t: 'tipoff' });
  assert.deepEqual(sent[3], { v: 1, t: 'hover', k: 'tab', id: 3, r: '0,0,200,34' });
  assert.deepEqual(sent[4], { v: 1, t: 'hover', k: 'none' });
  for (const sentFields of Object.values(sent)) assert.ok(Object.keys(sentFields).length <= 6);

  assert.equal(sendHoverAct(4, { a: 'marker', n: 4 }), true);
  assert.equal(sendHoverAct(4, { a: 'marker', n: 5 }), false);
  assert.equal(sendHoverAct(4, { a: 'row', n: 64 }), false);
  assert.equal(sendHoverAct(4, { a: 'swatch', n: 8 }), true);
  assert.equal(sendHoverAct(4, { a: 'name', text: 'n'.repeat(65) }), false);
  assert.equal(sendHoverAct(4, { a: 'name', text: 'Docs' }), true);
  assert.equal(sendHoverAct(-1, { a: 'next' }), false);
  assert.equal(sendHoverAct(4, { a: 'pick' }), false);
  assert.deepEqual(take(), [
    { v: 1, t: 'hact', tok: 4, a: 'marker', n: 4 },
    { v: 1, t: 'hact', tok: 4, a: 'swatch', n: 8 },
    { v: 1, t: 'hact', tok: 4, a: 'name', text: 'Docs' },
  ]);
});

function fakeClock() {
  let now = 0;
  let next = 1;
  const timers = new Map();
  return {
    now: () => now,
    set: (fn, ms) => { const id = next++; timers.set(id, { at: now + ms, fn }); return id; },
    clear: (id) => timers.delete(id),
    advance(ms) {
      now += ms;
      for (const [id, timer] of [...timers]) {
        if (timer.at <= now) { timers.delete(id); timer.fn(); }
      }
    },
  };
}

test('tooltips wait 500 ms, then move between neighbours at once', () => {
  const clock = fakeClock();
  const log = [];
  const timing = new TooltipTiming(clock, (t) => log.push(`show ${t}`), () => log.push('hide'));
  timing.enter('back');
  clock.advance(TIP_DELAY_MS - 1);
  assert.deepEqual(log, []);
  clock.advance(1);
  assert.deepEqual(log, ['show back']);
  timing.enter('forward');
  assert.deepEqual(log, ['show back', 'show forward'], 'a neighbour shows with no delay');
  timing.enter('forward');
  assert.equal(log.length, 2, 'the same target is not shown twice');
  timing.leave();
  assert.deepEqual(log.at(-1), 'hide');
  clock.advance(TIP_WARM_MS - 1);
  timing.enter('reload');
  assert.deepEqual(log.at(-1), 'show reload', 'still warm just after leaving');
  timing.leave();
  clock.advance(TIP_WARM_MS + 1);
  timing.enter('menu');
  assert.deepEqual(log.at(-1), 'hide', 'cold again: waits for the delay');
  clock.advance(TIP_DELAY_MS);
  assert.deepEqual(log.at(-1), 'show menu');
});

test('leaving before the delay, or dismissing, shows nothing', () => {
  const clock = fakeClock();
  const log = [];
  const timing = new TooltipTiming(clock, (t) => log.push(`show ${t}`), () => log.push('hide'));
  timing.enter('back');
  clock.advance(200);
  timing.leave();
  clock.advance(1000);
  assert.deepEqual(log, []);
  timing.enter('back');
  clock.advance(TIP_DELAY_MS);
  timing.dismiss();
  assert.deepEqual(log, ['show back', 'hide']);
  // Still over the same element after a click: no tooltip until the pointer leaves it.
  timing.enter('back');
  clock.advance(TIP_DELAY_MS * 2);
  assert.deepEqual(log, ['show back', 'hide']);
  // Dismissal also ends the warm period.
  timing.enter('forward');
  assert.equal(log.length, 2);
  clock.advance(TIP_DELAY_MS);
  assert.deepEqual(log.at(-1), 'show forward');
  timing.dismiss();
  timing.leave();
  timing.enter('back');
  assert.deepEqual(log.at(-1), 'hide', 'no warm start after a dismissal');
});

test('hover targets are reported once per change', () => {
  const report = new HoverReport();
  assert.equal(report.changed('tab:1'), true);
  assert.equal(report.changed('tab:1'), false);
  assert.equal(report.changed('ball:9'), true);
  assert.equal(report.changed(''), true);
  assert.equal(report.changed(''), false);
  report.reset();
  assert.equal(report.changed('tab:1'), true);
});

test('views use data-tip, never the title attribute, for tooltips', () => {
  const src = join(here, '..', 'src');
  const files = [];
  const walk = (dir) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (path.endsWith('.ts')) files.push(path);
    }
  };
  walk(src);
  for (const file of files) {
    const text = readFileSync(file, 'utf8');
    for (const [index, line] of text.split(/\r?\n/).entries()) {
      const element = /\.title\s*=(?!=)/.test(line) && !/document\.title\s*=/.test(line);
      const attribute = /setAttribute\(\s*['"]title['"]/.test(line);
      assert.ok(!element && !attribute, `${file}:${index + 1} sets a title tooltip`);
    }
  }
});
// --- end hover layer ---
