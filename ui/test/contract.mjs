// Cross-language contract test: the exact strings below are copied from the Rust unit tests in
// src/chrome_host/protocol.rs (tabs_json_*, fav_json_*). If Rust output changes, update both sides.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';
import * as esbuild from 'esbuild';

// protocol.ts imports the rail grammar from rail-logic.ts, so it is bundled rather than transpiled alone.
const here = dirname(fileURLToPath(import.meta.url));
const bundled = esbuild.buildSync({
  entryPoints: [join(here, '..', 'src', 'protocol.ts')],
  bundle: true,
  format: 'esm',
  platform: 'neutral',
  write: false,
  logLevel: 'silent',
});
const bundledUrl = `data:text/javascript;base64,${Buffer.from(bundled.outputFiles[0].contents).toString('base64')}`;
const { parseInbound, sendPanel, sendPopupSize, sendPopupPick, sendPopupDismiss } = await import(bundledUrl);

test('foundation Rust fixture messages parse across the bridge', () => {
  const fixture = JSON.parse(readFileSync(join(here, 'fixtures', 'rust-messages.json'), 'utf8'));
  assert.deepEqual(parseInbound(fixture.state), {
    t: 'state', scale: 1.25, pillx: 0, pillw: 624, menux: 627,
    solid: true, dark: true, back: false, forward: false, loading: false,
    progress: 0, security: 'secure', label: 'Secure', theme: { bg: '#123456', text: '#fff' },
  });
  assert.deepEqual(parseInbound(fixture.panelshow), { t: 'panelshow', p: 'history' });
  assert.deepEqual(parseInbound(fixture.layout), { t: 'layout', mode: 'vertical' });
  assert.deepEqual(parseInbound(fixture.snap), { t: 'snap', p: 'history', d: { items: [], count: 0 } });
});

test('foundation messages reject unknown fields, depth, size and key overflow', () => {
  assert.equal(parseInbound('{"t":"panelshow","p":"history","x":1}'), null);
  assert.equal(parseInbound('{"t":"panelshow","p":"History"}'), null);
  assert.equal(parseInbound('{"t":"layout","mode":"grid"}'), null);
  assert.equal(parseInbound('{"v":2,"t":"snap","p":"history","d":{}}'), null);
  assert.equal(parseInbound(JSON.stringify({ v: 1, t: 'snap', p: 'history', d: { x: [[[[[[[[[]]]]]]]]] } })), null);
  assert.equal(parseInbound(JSON.stringify({ v: 1, t: 'snap', p: 'history', d: { x: 'x'.repeat(256 * 1024) } })), null);
  assert.equal(parseInbound(JSON.stringify({ v: 1, t: 'snap', p: 'history', d: Object.fromEntries(Array.from({length: 129}, (_, i) => [`k${i}`, i])) })), null);
  const many = Object.fromEntries(Array.from({ length: 33 }, (_, group) => [
    `g${group}`, Object.fromEntries(Array.from({ length: 128 }, (_, i) => [`k${i}`, i])),
  ]));
  assert.equal(parseInbound(JSON.stringify({ v: 1, t: 'snap', p: 'history', d: many })), null);
  assert.equal(parseInbound({ v: 1, t: 'snap', p: 'history', d: { bad: undefined } }), null);
});

test('typed foundation senders enforce wire ranges and field cap', () => {
  const wires = [];
  globalThis.window = { chrome: { webview: { postMessage: wire => wires.push(JSON.parse(wire)) } } };
  assert.equal(sendPanel('hist', { a: 'open', id: Number.MAX_SAFE_INTEGER, n: -1 }), true);
  assert.equal(sendPanel('hist', { a: 'Open' }), false);
  assert.equal(sendPanel('hist', { a: 'open', id: 1, parent: 2, before: 3, n: 4, on: true }), false);
  assert.equal(sendPopupSize(1, 4000, 1), true);
  assert.equal(sendPopupSize(1, 4001, 1), false);
  assert.equal(sendPopupPick(1, 2), true);
  assert.equal(sendPopupDismiss(1), true);
  assert.deepEqual(wires[0], { v: 1, t: 'hist', a: 'open', id: Number.MAX_SAFE_INTEGER, n: -1 });
  assert.deepEqual(wires.slice(1), [
    { v: 1, t: 'size', tok: 1, w: 4000, h: 1 },
    { v: 1, t: 'pick', tok: 1, id: 2 },
    { v: 1, t: 'dismiss', tok: 1 },
  ]);
  delete globalThis.window;
});

test('empty tabs message from Rust parses', () => {
  const m = parseInbound('{"v":1,"t":"tabs","active":0,"items":[]}');
  assert.deepEqual(m, { t: 'tabs', active: 0, items: [] });
});

test('one tab from Rust parses', () => {
  const m = parseInbound(
    '{"v":1,"t":"tabs","active":7,"items":[{"id":7,"title":"Home","loading":true,"pinned":true,"muted":true,"marker":2,"g":0,"gl":false,"fav":true}]}',
  );
  assert.equal(m?.t, 'tabs');
  assert.equal(m.items[0].title, 'Home');
});

test('group leader and members from Rust parse', () => {
  const m = parseInbound(
    '{"v":1,"t":"tabs","active":11,"items":[' +
      '{"id":10,"title":"Mail","loading":false,"pinned":false,"muted":false,"marker":-1,"g":5,"gl":true,"gname":"Work","gcol":3,"gcollapsed":true,"gsize":2,"fav":false},' +
      '{"id":11,"title":"Calendar","loading":false,"pinned":false,"muted":false,"marker":-1,"g":5,"gl":false,"fav":false},' +
      '{"id":12,"title":"Loose","loading":false,"pinned":false,"muted":false,"marker":-1,"g":0,"gl":false,"fav":false}]}',
  );
  assert.equal(m?.items.length, 3);
  assert.equal(m.items[0].gname, 'Work');
});

test('favicon message from Rust parses', () => {
  const m = parseInbound('{"v":1,"t":"fav","id":5,"uri":"data:image/png;base64,iVBORw0KGgo="}');
  assert.equal(m?.t, 'fav');
});

test('wrong version is rejected', () => {
  assert.equal(parseInbound('{"v":2,"t":"tabs","active":0,"items":[]}'), null);
});

// Milestone 3 (left rail) shapes, exactly as docs/html-chrome-protocol.md documents them.
const RAIL_SAMPLE =
  '{"v":1,"t":"rail","items":[' +
  '{"k":"home","icon":"home","label":"Home","active":false,"dot":false,"accent":"","fav":false},' +
  '{"k":"customize","icon":"customize","label":"Customize","active":false,"dot":false,"accent":"","fav":false},' +
  '{"k":"addws","icon":"plus","label":"New workspace","active":false,"dot":false,"accent":"","fav":false},' +
  '{"k":"ws:3","icon":"g:briefcase","label":"Work","active":true,"dot":false,"accent":"#e5484d","fav":false},' +
  '{"k":"app:12","icon":"letter","label":"Mail","active":false,"dot":true,"accent":"","fav":false},' +
  '{"k":"app:13","icon":"apptile","label":"Apps","active":false,"dot":false,"accent":"","fav":true},' +
  '{"k":"addapp","icon":"plus","label":"Add app","active":false,"dot":false,"accent":"","fav":false},' +
  '{"k":"bookmarks","icon":"heart","label":"Bookmarks","active":false,"dot":false,"accent":"","fav":false},' +
  '{"k":"history","icon":"clock","label":"History","active":false,"dot":false,"accent":"","fav":false},' +
  '{"k":"menu","icon":"dots","label":"Menu","active":false,"dot":false,"accent":"","fav":false}]}';
const RAILFAV_SAMPLE = '{"v":1,"t":"railfav","k":"app:13","uri":"data:image/png;base64,iVBORw0KGgo="}';

/** One rail item JSON with the given overrides, for negative cases. */
function railItem(overrides = {}) {
  return JSON.stringify({ k: 'ws:1', icon: 'g:star', label: 'Star', active: false, dot: false, accent: '', fav: false, ...overrides });
}
function railMsg(items, extra = {}) {
  return JSON.stringify({ v: 1, t: 'rail', items: items.map((s) => JSON.parse(s)), ...extra });
}

test('the documented rail message parses into typed items', () => {
  const m = parseInbound(RAIL_SAMPLE);
  assert.equal(m?.t, 'rail');
  assert.equal(m.items.length, 10);
  assert.equal(m.items[3].k, 'ws:3');
  assert.equal(m.items[3].icon, 'g:briefcase');
  assert.equal(m.items[3].accent, '#e5484d');
  assert.equal(m.items[4].label, 'Mail');
});

test('the documented railfav message parses', () => {
  assert.deepEqual(parseInbound(RAILFAV_SAMPLE), { t: 'railfav', k: 'app:13', uri: 'data:image/png;base64,iVBORw0KGgo=' });
});

test('an empty rail parses', () => {
  assert.deepEqual(parseInbound('{"v":1,"t":"rail","items":[]}'), { t: 'rail', items: [] });
});

test('rail messages reject a wrong version, extra or missing keys and non-array items', () => {
  assert.equal(parseInbound(RAIL_SAMPLE.replace('"v":1', '"v":2')), null);
  assert.equal(parseInbound('{"v":1,"t":"rail","items":[],"extra":1}'), null);
  assert.equal(parseInbound('{"v":1,"t":"rail"}'), null);
  assert.equal(parseInbound('{"v":1,"t":"rail","items":{}}'), null);
  assert.equal(parseInbound(railMsg([railItem()], { extra: 1 })), null);
  assert.equal(parseInbound('{"v":1,"t":"railfav","k":"app:1","uri":"data:image/png;base64,AA==","x":1}'), null);
  assert.equal(parseInbound('{"v":2,"t":"railfav","k":"app:1","uri":"data:image/png;base64,AA=="}'), null);
});

test('rail items must have exactly the documented keys', () => {
  const missing = JSON.stringify({ k: 'home', icon: 'home', label: 'Home', active: false, dot: false, accent: '' });
  assert.equal(parseInbound(`{"v":1,"t":"rail","items":[${missing}]}`), null);
  assert.equal(parseInbound(railMsg([railItem({ extra: true })])), null);
});

test('rail keys follow the grammar and stay unique within a message', () => {
  for (const k of ['home', 'customize', 'addws', 'addapp', 'bookmarks', 'history', 'menu', 'ws:0', 'ws:' + '9'.repeat(20), 'app:1234567890']) {
    assert.ok(parseInbound(railMsg([railItem({ k })])), k);
  }
  for (const k of ['', 'Home', 'ws:', 'ws:' + '9'.repeat(21), 'app:' + '9'.repeat(11), 'app:x', 'ws:1 ', 'toolbar', 'addws ', 'ws:-1']) {
    assert.equal(parseInbound(railMsg([railItem({ k })])), null, JSON.stringify(k));
  }
  assert.equal(parseInbound(railMsg([railItem({ k: 'ws:1' }), railItem({ k: 'ws:1' })])), null);
});

test('rail icons come from the allowed list only', () => {
  for (const icon of ['home', 'customize', 'plus', 'heart', 'clock', 'dots', 'apptile', 'letter', 'g:briefcase', 'g:leaf']) {
    assert.ok(parseInbound(railMsg([railItem({ icon })])), icon);
  }
  for (const icon of ['', 'Home', 'g:', 'g:unknown', 'g:g:home', 'url(x)', 'star']) {
    assert.equal(parseInbound(railMsg([railItem({ icon })])), null, JSON.stringify(icon));
  }
});

test('rail labels are capped at 64 characters', () => {
  assert.ok(parseInbound(railMsg([railItem({ label: 'x'.repeat(64) })])));
  assert.equal(parseInbound(railMsg([railItem({ label: 'x'.repeat(65) })])), null);
});

test('rail accents accept only safe colours or the empty string', () => {
  for (const accent of ['', '#fff', '#e5484d', 'rgb(1, 2, 3)', 'rgba(0,0,0,0.5)']) {
    assert.equal(parseInbound(railMsg([railItem({ accent })])).items[0].accent, accent, accent);
  }
  for (const accent of ['red', 'url(x)', '#ggg', 'rgb(1,2,3);', 'expression(1)', `rgb(${'1,'.repeat(40)}1)`]) {
    assert.equal(parseInbound(railMsg([railItem({ accent })])), null, accent);
  }
});

test('a rail holds up to 128 items and no more', () => {
  const full = Array.from({ length: 128 }, (_, i) => railItem({ k: `app:${i + 1}`, icon: 'letter' }));
  assert.equal(parseInbound(railMsg(full)).items.length, 128);
  assert.equal(parseInbound(railMsg([...full, railItem({ k: 'app:999' })])), null);
});

test('railfav enforces the key grammar and the favicon rules', () => {
  assert.equal(parseInbound('{"v":1,"t":"railfav","k":"toolbar","uri":"data:image/png;base64,AA=="}'), null);
  assert.equal(parseInbound('{"v":1,"t":"railfav","k":"app:1","uri":"https://example.com/a.png"}'), null);
  assert.equal(parseInbound('{"v":1,"t":"railfav","k":"app:1","uri":"data:image/png;base64,A A"}'), null);
});
