// Address bar, search and bookmark options: the pure rules (what Enter variants do, what a paste and a
// copy become, the engine editor's checks), the QR encoder proved with an independent decoder, and the
// wire contract with Rust (fixtures written by src/chrome_host/address_page.rs).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const bundle = (entry) => {
  const out = esbuild.buildSync({ entryPoints: [join(here, '..', 'src', entry)], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
};

const posted = [];
globalThis.window = { chrome: { webview: { postMessage: (m) => posted.push(JSON.parse(m)), addEventListener() {} } } };
const logic = await bundle('address-logic.ts');
const rows = await bundle('address-settings-logic.ts');
const { qrMatrix, qrSvgPath } = await bundle('qr.ts');
const { parseInbound, sendAddr } = await bundle('protocol.ts');
const suggest = await bundle('suggest-logic.ts');
const fixture = JSON.parse(readFileSync(join(here, 'fixtures', 'address-messages.json'), 'utf8'));
const take = () => posted.splice(0, posted.length);

// ---- what Enter does ----

test('Alt+Enter and Shift+Enter open a new tab, Ctrl+Enter expands, anything else is a plain Enter', () => {
  const keys = (extra) => ({ altKey: false, shiftKey: false, ctrlKey: false, metaKey: false, ...extra });
  assert.equal(logic.submitKind(keys({})), 'plain');
  assert.equal(logic.submitKind(keys({ altKey: true })), 'newtab');
  assert.equal(logic.submitKind(keys({ shiftKey: true })), 'newtab');
  assert.equal(logic.submitKind(keys({ ctrlKey: true })), 'expand');
  assert.equal(logic.submitKind(keys({ ctrlKey: true, shiftKey: true })), 'plain');
  assert.equal(logic.submitKind(keys({ ctrlKey: true, altKey: true })), 'plain');
  assert.equal(logic.submitKind(keys({ metaKey: true, altKey: true })), 'plain');
});

// ---- paste, copy ----

test('a pasted javascript: prefix is removed however it is spaced or cased, and only a prefix', () => {
  const strip = logic.stripPastedJavascript;
  assert.equal(strip('javascript:alert(1)'), 'alert(1)');
  assert.equal(strip('  JaVaScRiPt:alert(1)'), 'alert(1)');
  assert.equal(strip('java\tscript:\nalert(1)'), 'alert(1)');
  assert.equal(strip('javascript:javascript:alert(1)'), 'alert(1)', 'repeated prefixes go too');
  assert.equal(strip('https://example.com/?q=javascript:x'), 'https://example.com/?q=javascript:x');
  assert.equal(strip('see javascript: here'), 'see javascript: here');
  assert.equal(strip('javascript'), 'javascript');
  assert.equal(strip(''), '');
  assert.equal(strip('\u0001javascript:x'), 'x');
});

test('copying or cutting the whole address puts its encoded form on the clipboard; partial or non-address text is left alone', () => {
  const value = 'https://example.com/a b/é?x=1 2';
  const all = { enabled: true, value, selStart: 0, selEnd: value.length };
  assert.equal(logic.clipboardText(all), 'https://example.com/a%20b/%C3%A9?x=1%202');
  assert.equal(logic.clipboardText({ ...all, enabled: false }), null, 'the option off leaves the browser copy');
  assert.equal(logic.clipboardText({ ...all, selEnd: 5 }), null, 'a part of the address is copied as written');
  assert.equal(logic.clipboardText({ ...all, value: 'rust lang', selEnd: 9 }), null);
  assert.equal(logic.clipboardText({ ...all, value: 'https://example.com/', selEnd: 20 }), null, 'already encoded text changes nothing');
  assert.equal(logic.clipboardText({ ...all, value: '', selEnd: 0 }), null);
  assert.equal(logic.encodedAddress('javascript:alert(1)'), null);
});

// ---- display ----

test('the base domain is split out of the host, with the port after it', () => {
  assert.deepEqual(logic.splitHost('mail.example.co.uk', 'example.co.uk'), { before: 'mail.', base: 'example.co.uk', after: '' });
  assert.deepEqual(logic.splitHost('www.example.com:8080', 'example.com'), { before: 'www.', base: 'example.com', after: ':8080' });
  assert.deepEqual(logic.splitHost('example.com', 'example.com'), { before: '', base: 'example.com', after: '' });
  assert.deepEqual(logic.splitHost('evilexample.com', 'example.com'), { before: '', base: 'evilexample.com', after: '' }, 'a lookalike is not a subdomain');
  assert.deepEqual(logic.splitHost('localhost', ''), { before: '', base: 'localhost', after: '' });
  assert.deepEqual(logic.splitHost('[::1]:3000', ''), { before: '', base: '[::1]', after: ':3000' });
});

test('badges: always shows one on every row but the default; otherwise only a bookmark with a favicon', () => {
  assert.equal(logic.wantsBadge('hist', false, true, false), true);
  assert.equal(logic.wantsBadge('hist', false, true, true), false);
  assert.equal(logic.wantsBadge('bm', true, false, false), true);
  assert.equal(logic.wantsBadge('bm', false, false, false), false);
  assert.equal(logic.wantsBadge('tab', true, false, false), false);
  assert.deepEqual(suggest.suggestBadge('hist', false, true), { icon: 'history', label: 'History' });
  assert.deepEqual(suggest.suggestBadge('tab', false, true), { icon: 'tab-switch', label: 'Open tab' });
  assert.equal(suggest.suggestBadge('hist', false, false), null);
  assert.equal(suggest.suggestBadge('url', true, true), null, 'a typed address needs no badge');
  assert.equal(suggest.suggestGlyph('calc'), 'calculator');
});

test('typed history shows on focus only when enabled and the field is empty or fully selected', () => {
  const on = { enabled: true, value: '', allSelected: false };
  assert.equal(logic.showsTypedHistoryOnFocus(on), true);
  assert.equal(logic.showsTypedHistoryOnFocus({ ...on, enabled: false }), false);
  assert.equal(logic.showsTypedHistoryOnFocus({ ...on, value: 'rust' }), false);
  assert.equal(logic.showsTypedHistoryOnFocus({ ...on, value: 'https://a.example/', allSelected: true }), true);
});

// ---- the wire ----

test('Rust addr and sfield messages parse; partial, extended and mistyped ones do not', () => {
  const addr = parseInbound(fixture.addr);
  assert.equal(addr.t, 'addr');
  assert.equal(addr.base, 'example.co.uk');
  assert.equal(addr.typing, true);
  assert.equal(addr.highlight, false);
  assert.deepEqual(Object.keys(addr).sort(), ['t', ...logic.ADDR_FLAG_KEYS].sort());
  for (const bad of [{ ...fixture.addr, extra: 1 }, { ...fixture.addr, v: 2 }, { ...fixture.addr, show: 'yes' }, { ...fixture.addr, base: 5 }, { ...fixture.addr, base: 'x'.repeat(254) },
    (({ show, ...rest }) => rest)(fixture.addr)]) {
    assert.equal(parseInbound(bad), null, JSON.stringify(bad).slice(0, 100));
  }
  const field = parseInbound(fixture.sfield);
  assert.equal(field.t, 'sfield');
  assert.equal(field.mode, 'field');
  assert.equal(field.engine, 1);
  assert.equal(field.engines[0].n, 'Optimised');
  for (const bad of [{ ...fixture.sfield, mode: 'both' }, { ...fixture.sfield, engine: 7 }, { ...fixture.sfield, engines: [] }, { ...fixture.sfield, extra: true },
    { ...fixture.sfield, engines: [{ n: 'x' }] }, { ...fixture.sfield, engines: [{ n: 'x', k: 'y', z: 1 }] }, { ...fixture.sfield, keep: 1 }]) {
    assert.equal(parseInbound(bad), null, JSON.stringify(bad).slice(0, 100));
  }
});

test('address commands are the flat panel family and are validated', () => {
  take();
  assert.equal(sendAddr({ a: 'submit', text: 'rust', n: 1 }), true);
  assert.equal(sendAddr({ a: 'typed', n: 4 }), true);
  assert.equal(sendAddr({ a: 'qr' }), true);
  assert.equal(sendAddr({ a: 'search', text: 'q', id: 2, n: 0 }), true);
  assert.deepEqual(take(), [
    { v: 1, t: 'addr', a: 'submit', text: 'rust', n: 1 },
    { v: 1, t: 'addr', a: 'typed', n: 4 },
    { v: 1, t: 'addr', a: 'qr' },
    { v: 1, t: 'addr', a: 'search', text: 'q', id: 2, n: 0 },
  ]);
  assert.equal(sendAddr({ a: 'Bad Action' }), false);
  assert.equal(sendAddr({ a: 'submit', text: 'x'.repeat(5000), n: 0 }), false);
  assert.equal(sendAddr({ a: 'typed', n: 1.5 }), false);
  assert.deepEqual(take(), []);
});

test('suggest rows accept the calculator kind and the badge flag', () => {
  const row = { k: 'calc', text: '8', title: '= 8', url: '2+2*3', bd: true };
  const list = parseInbound({ v: 1, t: 'suggest', tok: 0, seq: 1, w: 0, sel: 0, q: '', items: [row] });
  assert.equal(list.items[0].k, 'calc');
  assert.equal(list.items[0].bd, true);
  assert.equal(parseInbound({ v: 1, t: 'suggest', tok: 0, seq: 1, w: 0, sel: 0, q: '', items: [{ ...row, bd: 'yes' }] }), null);
  const twenty = Array.from({ length: 20 }, () => ({ k: 'hist', text: 'a', title: 'a', url: 'a' }));
  assert.equal(parseInbound({ v: 1, t: 'suggest', tok: 0, seq: 1, w: 0, sel: 19, q: '', items: twenty }).items.length, 20, 'twenty rows, the largest maximum result');
});

// ---- the settings snapshot, commands and the engine editor ----

test('the Rust address snapshot parses and carries every option, engine and folder', () => {
  const snap = rows.parseAddressSnapshot(fixture.snap.d);
  assert.ok(snap);
  assert.equal(snap.s.dd_max_results, 8);
  assert.equal(snap.s.search_field, 'field');
  assert.deepEqual(rows.sourceList(snap.s.dd_sources), ['bookmarks', 'history']);
  assert.equal(snap.engines.length, 7);
  assert.equal(snap.def, 'optimised');
  assert.equal(snap.priv, 'duckduckgo');
  assert.deepEqual(snap.folders.map((f) => [f.id, f.depth]), [[4, 0], [9, 1]]);
  assert.equal(parseInbound(fixture.snap).t, 'snap');
  const d = fixture.snap.d;
  for (const bad of [{ ...d, extra: 1 }, { ...d, s: { ...d.s, dd_max_results: 21 } }, { ...d, s: { ...d.s, search_field: 'x' } }, { ...d, def: 'gone' }, { ...d, engines: [] },
    { ...d, folders: [{ id: 0, title: 'x', depth: 0 }] }, { ...d, ack: 'later' }, { ...d, guest: 1 }, { ...d, s: { ...d.s, dd_sources: 5 } }]) {
    assert.equal(rows.parseAddressSnapshot(bad), null, JSON.stringify(bad).slice(0, 80));
  }
  const missing = { ...d, s: { ...d.s } };
  delete missing.s.dd_max_results;
  assert.equal(rows.parseAddressSnapshot(missing), null);
});

test('every row the sections draw has a spec, and keys are checked before they are sent', () => {
  const ids = rows.ADDRESS_ROWS.map((r) => r.id);
  assert.equal(new Set(ids).size, ids.length, 'unique ids');
  assert.deepEqual(rows.setFields('dd_max_results', 12), { a: 'addrset', text: 'dd_max_results=12' });
  assert.deepEqual(rows.setFields('address_show_full', true), { a: 'addrset', text: 'address_show_full=true' });
  assert.equal(rows.setFields('dd_max_results', 0), null);
  assert.equal(rows.setFields('dd_max_results', 21), null);
  assert.equal(rows.setFields('bm_confirm_open', 1), null);
  assert.deepEqual(rows.setFields('bm_confirm_open', 0), { a: 'addrset', text: 'bm_confirm_open=0' });
  assert.equal(rows.setFields('domain_expansion_suffix', 'c om'), null);
  assert.deepEqual(rows.setFields('domain_expansion_suffix', 'co.uk'), { a: 'addrset', text: 'domain_expansion_suffix=co.uk' });
  assert.equal(rows.setFields('nick_tabs', 'Bad Word'), null);
  assert.equal(rows.setFields('nick_tabs', 't'.repeat(17)), null);
  assert.equal(rows.setFields('tab_layout', 'vertical'), null, 'only this section\'s keys');
  assert.equal(rows.setFields('dd_sources', 'a\nb'), null);
});

test('the source list toggles on at the end, off in place, and moves one place at a time', () => {
  const all = ['direct', 'bookmarks', 'history'];
  assert.equal(rows.toggleSource(all, 'bookmarks', false), 'direct,history');
  assert.equal(rows.toggleSource(['direct'], 'calculator', true), 'direct,calculator');
  assert.equal(rows.moveSource(all, 'history', -1), 'direct,history,bookmarks');
  assert.equal(rows.moveSource(all, 'direct', -1), 'direct,bookmarks,history', 'the first cannot go up');
  assert.equal(rows.moveSource(all, 'history', 1), 'direct,bookmarks,history', 'the last cannot go down');
  assert.deepEqual(rows.sourceRows(['history']).slice(0, 2), [{ id: 'history', on: true }, { id: 'direct', on: false }]);
  assert.deepEqual(rows.sourceList('direct,nonsense,history'), ['direct', 'history']);
});

test('the engine editor checks an engine the way Bowser will, and sends one tab-separated command', () => {
  const engines = [{ id: 'google', name: 'Google', nick: 'g', url: 'https://www.google.com/search?q=%s', post: '', suggest: '', image: '', imagePost: '', builtin: true }];
  const ok = { name: 'Mine', nick: 'mn', url: 'https://m.example/?q=%s', post: '', suggest: 'https://m.example/s?q=%s', image: '', imagePost: '' };
  assert.equal(rows.engineProblem(ok, engines, null), null);
  const problem = (change) => rows.engineProblem({ ...ok, ...change }, engines, null);
  assert.match(problem({ name: '' }), /name/);
  assert.match(problem({ nick: 'g' }), /already used by Google/);
  assert.equal(rows.engineProblem({ ...ok, nick: 'g' }, engines, 'google'), null, 'an engine may keep its own nickname');
  assert.match(problem({ nick: 'Bad Nick' }), /nickname/);
  assert.match(problem({ url: 'http://m.example/?q=%s' }), /https/);
  assert.match(problem({ url: 'https://m.example/' }), /%s once/);
  assert.match(problem({ url: 'https://m.example/?q=%s&r=%s' }), /%s once/);
  assert.match(problem({ post: 'q=1' }), /POST/);
  assert.equal(problem({ url: 'https://m.example/find', post: 'q=%s' }), null, 'a POST engine needs the placeholder in its fields');
  assert.match(problem({ suggest: 'https://m.example/s' }), /suggestions/);
  assert.match(problem({ image: 'ftp://m.example/%s' }), /image/);
  assert.match(problem({ imagePost: 'u=%s' }), /Image POST/);
  assert.equal(problem({ image: 'https://m.example/i', imagePost: 'u=%s' }), null);
  assert.deepEqual(rows.engineFields({ op: 'add', draft: ok }), { a: 'engine', text: 'add\tMine\tmn\thttps://m.example/?q=%s\t\thttps://m.example/s?q=%s\t\t' });
  assert.deepEqual(rows.engineFields({ op: 'update', id: 'google', draft: ok }).text.split('\t').slice(0, 3), ['update', 'google', 'Mine']);
  assert.deepEqual(rows.engineFields({ op: 'move', id: 'google', index: 2 }), { a: 'engine', text: 'move\tgoogle\t2' });
  assert.deepEqual(rows.engineFields({ op: 'private', id: '' }), { a: 'engine', text: 'private\t' });
  assert.deepEqual(rows.engineFields({ op: 'restore' }), { a: 'engine', text: 'restore' });
  assert.equal(rows.engineFields({ op: 'remove', id: 'Bad Id' }), null);
  assert.equal(rows.engineLabel({ name: 'Google', nick: 'g' }, true), 'Google (g)');
  assert.equal(rows.engineLabel({ name: 'Google', nick: 'g' }, false), 'Google');
  assert.equal(rows.engineLabel({ name: 'Plain', nick: '' }, true), 'Plain');
  assert.equal(rows.monogram('  duck'), 'D');
  assert.equal(rows.monogram(''), '?');
});

// ---- the QR encoder, proved by an independent decoder ----

/** Level-M byte-mode capacity per version (ISO/IEC 18004 table 7). */
const CAPACITY = [14, 26, 42, 62, 84, 106, 122, 152, 180, 213];
const ECC = [0, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26];
const BLOCKS = [0, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5];

const EXP = new Array(512).fill(0);
const LOG = new Array(256).fill(0);
for (let i = 0, x = 1; i < 255; i += 1) { EXP[i] = x; LOG[x] = i; x <<= 1; if (x & 0x100) x ^= 0x11d; }
for (let i = 255; i < 512; i += 1) EXP[i] = EXP[i - 255];
const mul = (a, b) => (a === 0 || b === 0 ? 0 : EXP[LOG[a] + LOG[b]]);

function rawModules(version) {
  let result = (16 * version + 128) * version + 64;
  if (version >= 2) { const n = Math.floor(version / 7) + 2; result -= (25 * n - 10) * n - 55; if (version >= 7) result -= 36; }
  return result;
}

/** Decodes a matrix: format information, unmasking, de-interleaving, Reed-Solomon syndromes, byte mode. */
function decode(matrix) {
  const size = matrix.length;
  const version = (size - 17) / 4;
  const bit = (x, y) => (matrix[y][x] ? 1 : 0);
  // Format information, both copies, must agree and carry level M with a valid BCH code.
  const first = [];
  for (let i = 0; i <= 5; i += 1) first.push(bit(8, i));
  first.push(bit(8, 7), bit(8, 8), bit(7, 8));
  for (let i = 9; i < 15; i += 1) first.push(bit(14 - i, 8));
  const second = [];
  for (let i = 0; i < 8; i += 1) second.push(bit(size - 1 - i, 8));
  for (let i = 8; i < 15; i += 1) second.push(bit(8, size - 15 + i));
  assert.deepEqual(first, second, 'both format copies agree');
  const word = first.reduce((value, b, i) => value | (b << i), 0) ^ 0x5412;
  assert.equal(word >> 13, 0, 'error correction level M');
  const mask = (word >> 10) & 7;
  let rem = word >> 10;
  for (let i = 0; i < 10; i += 1) rem = (rem << 1) ^ ((rem >>> 9) * 0x537);
  assert.equal(rem & 0x3ff, word & 0x3ff, 'format BCH check');
  assert.equal(bit(8, size - 8), 1, 'the dark module');
  // Function-pattern map from a throwaway encode of the same version (finder, timing, alignment, format, version).
  const isFunction = Array.from({ length: size }, () => new Array(size).fill(false));
  const mark = (x, y) => { if (x >= 0 && y >= 0 && x < size && y < size) isFunction[y][x] = true; };
  for (let i = 0; i < size; i += 1) { mark(6, i); mark(i, 6); }
  for (const [cx, cy] of [[3, 3], [size - 4, 3], [3, size - 4]]) for (let dy = -4; dy <= 4; dy += 1) for (let dx = -4; dx <= 4; dx += 1) mark(cx + dx, cy + dy);
  if (version > 1) {
    const n = Math.floor(version / 7) + 2;
    const step = Math.ceil((version * 4 + 4) / (n * 2 - 2)) * 2;
    const pos = [6];
    for (let p = size - 7; pos.length < n; p -= step) pos.splice(1, 0, p);
    pos.forEach((cy, i) => pos.forEach((cx, j) => {
      if ((i === 0 && j === 0) || (i === 0 && j === n - 1) || (i === n - 1 && j === 0)) return;
      for (let dy = -2; dy <= 2; dy += 1) for (let dx = -2; dx <= 2; dx += 1) mark(cx + dx, cy + dy);
    }));
  }
  for (let i = 0; i < 9; i += 1) { mark(8, i); mark(i, 8); }
  for (let i = 0; i < 8; i += 1) { mark(size - 1 - i, 8); mark(8, size - 1 - i); }
  if (version >= 7) for (let i = 0; i < 6; i += 1) for (let j = 0; j < 3; j += 1) { mark(size - 11 + j, i); mark(i, size - 11 + j); }
  const invert = [
    (x, y) => (x + y) % 2 === 0, (x, y) => y % 2 === 0, (x) => x % 3 === 0, (x, y) => (x + y) % 3 === 0,
    (x, y) => (Math.floor(x / 3) + Math.floor(y / 2)) % 2 === 0, (x, y) => ((x * y) % 2) + ((x * y) % 3) === 0,
    (x, y) => (((x * y) % 2) + ((x * y) % 3)) % 2 === 0, (x, y) => (((x + y) % 2) + ((x * y) % 3)) % 2 === 0,
  ][mask];
  const bits = [];
  for (let right = size - 1; right >= 1; right -= 2) {
    if (right === 6) right = 5;
    for (let v = 0; v < size; v += 1) for (let j = 0; j < 2; j += 1) {
      const x = right - j;
      const y = ((right + 1) & 2) === 0 ? size - 1 - v : v;
      if (!isFunction[y][x]) bits.push(bit(x, y) ^ (invert(x, y) ? 1 : 0));
    }
  }
  const raw = Math.floor(rawModules(version) / 8);
  const codewords = [];
  for (let i = 0; i < raw; i += 1) codewords.push(bits.slice(i * 8, i * 8 + 8).reduce((v, b) => (v << 1) | b, 0));
  // De-interleave into blocks and check each one: its syndromes at alpha^0..alpha^(ecc-1) are all zero.
  const blocks = BLOCKS[version], ecc = ECC[version];
  const shortBlocks = blocks - (raw % blocks), shortLen = Math.floor(raw / blocks);
  const parts = Array.from({ length: blocks }, () => []);
  let at = 0;
  // Data first (long blocks have one more byte), then the error-correction bytes, each round-robin.
  for (let i = 0; i < shortLen - ecc; i += 1) for (let j = 0; j < blocks; j += 1) parts[j].push(codewords[at++]);
  for (let j = shortBlocks; j < blocks; j += 1) parts[j].push(codewords[at++]);
  for (let i = 0; i < ecc; i += 1) for (let j = 0; j < blocks; j += 1) parts[j].push(codewords[at++]);
  assert.equal(at, raw, 'every codeword was placed in a block');
  const data = [];
  parts.forEach((block, index) => {
    for (let s = 0; s < ecc; s += 1) {
      let syndrome = 0;
      for (const byte of block) syndrome = mul(syndrome, EXP[s]) ^ byte;
      assert.equal(syndrome, 0, `block ${index} syndrome ${s}`);
    }
    data.push(...block.slice(0, block.length - ecc));
  });
  // Byte mode: 0100, a count, the bytes, a terminator and the 0xEC/0x11 pad.
  const stream = data.flatMap((byte) => Array.from({ length: 8 }, (_, i) => (byte >> (7 - i)) & 1));
  const take = (n) => stream.splice(0, n).reduce((v, b) => (v << 1) | b, 0);
  assert.equal(take(4), 0b0100, 'byte mode');
  const length = take(version < 10 ? 8 : 16);
  const bytes = Array.from({ length }, () => take(8));
  assert.equal(take(4), 0, 'terminator');
  stream.splice(0, stream.length % 8);
  const pad = []; while (stream.length) pad.push(take(8));
  pad.forEach((byte, i) => assert.equal(byte, i % 2 === 0 ? 0xec : 0x11, 'pad bytes'));
  return { version, text: new TextDecoder().decode(new Uint8Array(bytes)), mask };
}

test('the QR encoder round-trips text through an independent decoder at every version boundary', () => {
  CAPACITY.forEach((capacity, index) => {
    for (const length of [capacity, capacity - 3, ...(index > 0 ? [CAPACITY[index - 1] + 1] : [])]) {
      const text = Array.from({ length }, (_, i) => 'abcdefghijklmnopqrstuvwxyz0123456789:/?=&.-'[(i * 7 + index) % 43]).join('');
      const matrix = qrMatrix(text);
      assert.ok(matrix, `length ${length}`);
      const got = decode(matrix);
      assert.equal(got.text, text, `length ${length}`);
      const wanted = CAPACITY.findIndex((c) => length <= c) + 1;
      assert.equal(got.version, wanted, `smallest version for ${length} bytes`);
      assert.equal(matrix.length, wanted * 4 + 17);
    }
  });
});

test('the QR encoder handles UTF-8, real addresses and refuses what does not fit', () => {
  for (const text of ['https://www.rust-lang.org/', 'https://example.com/search?q=caf%C3%A9&lang=en#top', 'café ☕ 日本語 😀', 'a']) {
    assert.equal(decode(qrMatrix(text)).text, text);
  }
  assert.equal(qrMatrix(''), null);
  assert.equal(qrMatrix('x'.repeat(214)), null, 'past version 10 level M');
  assert.ok(qrMatrix('x'.repeat(213)));
  assert.equal(qrMatrix('日'.repeat(72)), null, 'bytes, not characters, are counted');
});

test('the QR finder, timing and alignment patterns are where the standard puts them', () => {
  const m = qrMatrix('https://example.com/');
  const size = m.length;
  const finder = (x0, y0) => { for (let y = 0; y < 7; y += 1) for (let x = 0; x < 7; x += 1) { const ring = Math.max(Math.abs(x - 3), Math.abs(y - 3)); assert.equal(m[y0 + y][x0 + x], ring !== 2, `finder ${x0},${y0} ${x},${y}`); } };
  finder(0, 0); finder(size - 7, 0); finder(0, size - 7);
  for (let i = 8; i < size - 8; i += 1) { assert.equal(m[6][i], i % 2 === 0, `timing row ${i}`); assert.equal(m[i][6], i % 2 === 0, `timing column ${i}`); }
  assert.equal(size, 25, 'version 2 for a 20-byte address');
  for (let dy = -2; dy <= 2; dy += 1) for (let dx = -2; dx <= 2; dx += 1) assert.equal(m[18 + dy][18 + dx], Math.max(Math.abs(dx), Math.abs(dy)) !== 1, 'alignment pattern at 18,18');
});

test('the QR path draws exactly the dark modules, one run at a time', () => {
  const matrix = [[true, true, false], [false, true, true], [false, false, false]];
  const { size, d } = qrSvgPath(matrix, 1);
  assert.equal(size, 5);
  assert.equal(d, 'M1 1h2v1h-2zM2 2h2v1h-2z');
  const real = qrMatrix('https://example.com/');
  const path = qrSvgPath(real);
  let area = 0;
  for (const match of path.d.matchAll(/h(\d+)v1/g)) area += Number(match[1]);
  assert.equal(area, real.flat().filter(Boolean).length, 'the path covers every dark module once');
  assert.equal(path.size, real.length + 8);
});

// ---- the bookmarks bar ----

const barLogic = await bundle('bookmark-bar-logic.ts');
const dialogLogic = await bundle('address-dialog-logic.ts');

test('the bookmarks bar snapshot from Rust parses, and malformed ones do not', () => {
  const bar = barLogic.parseBarSnapshot(fixture.bar.d);
  assert.equal(bar.display, 'text-icon');
  assert.deepEqual(bar.items.map((i) => [i.id, i.kind, i.title]), [[1, 'link', 'Rust'], [2, 'folder', 'Reading']]);
  assert.deepEqual(bar.groups, [{ key: 'example.com', name: 'Example', pinned: true, live: false, colour: -1 }]);
  assert.equal(parseInbound(fixture.bar).t, 'snap');
  const d = fixture.bar.d;
  const item = d.items[0];
  for (const bad of [{ ...d, extra: 1 }, { ...d, display: 'big' }, { ...d, items: [item, item] }, { ...d, items: [{ ...item, kind: 'tab' }] }, { ...d, items: [{ ...item, id: 0 }] },
    { ...d, items: [{ ...d.items[1], url: 'https://x.example/' }] }, { ...d, items: [{ ...item, fav: 'https://x.example/f.png' }] }, { ...d, items: [{ ...item, title: 'a\nb' }] },
    { ...d, groups: [{ ...d.groups[0], key: 'bad key' }] }, { ...d, groups: [{ ...d.groups[0], colour: 300 }] }, { ...d, groups: [{ ...d.groups[0], pinned: 1 }] },
    { ...d, items: Array.from({ length: 201 }, (_, i) => ({ ...item, id: i + 1 })) }]) {
    assert.equal(barLogic.parseBarSnapshot(bad), null, JSON.stringify(bad).slice(0, 90));
  }
});

test('each display mode shows what its name says, folders keep their names unless only icons show', () => {
  const link = { kind: 'link' };
  const folder = { kind: 'folder' };
  assert.deepEqual(barLogic.faceOf(link, 'text-icon'), { icon: 'site', text: true });
  assert.deepEqual(barLogic.faceOf(link, 'text'), { icon: null, text: true });
  assert.deepEqual(barLogic.faceOf(link, 'icon'), { icon: 'site', text: false });
  assert.deepEqual(barLogic.faceOf(link, 'bookmark-icon'), { icon: 'bookmark', text: false });
  assert.deepEqual(barLogic.faceOf(folder, 'text-icon'), { icon: 'folder', text: true });
  assert.deepEqual(barLogic.faceOf(folder, 'text'), { icon: null, text: true });
  assert.deepEqual(barLogic.faceOf(folder, 'icon'), { icon: 'folder', text: false });
  assert.deepEqual(barLogic.faceOf(folder, 'bookmark-icon'), { icon: 'folder', text: true }, 'a folder keeps its name with bookmark icons only');
});

test('the bar sends flat commands and fits as many items as the row holds', () => {
  assert.deepEqual(barLogic.openFields(4, true), { a: 'baropen', id: 4, n: 1 });
  assert.deepEqual(barLogic.folderFields(4, '1,2,3,4'), { a: 'barfolder', id: 4, text: '1,2,3,4' });
  assert.deepEqual(barLogic.contextFields(4), { a: 'barctx', id: 4 });
  assert.deepEqual(barLogic.groupFields('example.com'), { a: 'bargroup', text: 'example.com' });
  assert.deepEqual(barLogic.overflowFields(3, '1,2,3,4'), { a: 'baroverflow', n: 3, text: '1,2,3,4' });
  take();
  for (const fields of [barLogic.openFields(4, false), barLogic.folderFields(4, '1,2,3,4'), barLogic.contextFields(4), barLogic.groupContextFields('a.example'), barLogic.overflowFields(1, '1,2,3,4')]) {
    assert.equal(sendAddr(fields), true, JSON.stringify(fields));
  }
  assert.equal(take().length, 5);
  // Five items of 50px with 2px gaps need 258px: all fit in 300, two fit in 170 with the 28px overflow button.
  assert.equal(barLogic.fitCount([50, 50, 50, 50, 50], 300, 2, 28), 5);
  assert.equal(barLogic.fitCount([50, 50, 50, 50, 50], 170, 2, 28), 2);
  assert.equal(barLogic.fitCount([50, 50, 50, 50, 50], 10, 2, 28), 0);
  assert.equal(barLogic.fitCount([], 100, 2, 28), 0);
});

test('the address dialogs: snapshots parse strictly and fields are cleaned before they are sent', () => {
  const qr = dialogLogic.parseDialogSnapshot(fixture.qr.d);
  assert.deepEqual(qr, { kind: 'qr', text: 'https://example.com/a b' });
  const bookmark = dialogLogic.parseDialogSnapshot(fixture.bookmark.d);
  assert.equal(bookmark.kind, 'bookmark');
  assert.equal(bookmark.existing, true);
  assert.equal(bookmark.folder, 9);
  assert.equal(bookmark.nickname, 'rl');
  assert.deepEqual(bookmark.folders.map((f) => f.id), [4, 9]);
  const d = fixture.bookmark.d;
  for (const bad of [{ ...d, kind: 'other' }, { ...d, extra: 1 }, { ...d, folder: -1 }, { ...d, existing: 'yes' }, { ...d, folders: [{ id: 0, title: 'x', depth: 0 }] }, { ...d, name: 'a\tb' }, { kind: 'qr', text: 'x', extra: 1 }]) {
    assert.equal(dialogLogic.parseDialogSnapshot(bad), null, JSON.stringify(bad).slice(0, 80));
  }
  assert.equal(dialogLogic.cleanField('  a\tb\nc  '), 'a b c');
  assert.equal(dialogLogic.MAX_QR_BYTES, 213);
  assert.ok(qrMatrix('x'.repeat(dialogLogic.MAX_QR_BYTES)) !== null && qrMatrix('x'.repeat(dialogLogic.MAX_QR_BYTES + 1)) === null, 'the dialog limit is the encoder limit');
});
