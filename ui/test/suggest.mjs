// --- omnibox suggest ---
// Omnibox dropdown contract: the Rust fixture (written by protocol.rs suggest_messages_write_the_ui_fixture)
// parses, malformed messages are rejected, outbound commands are validated, and the selection rules hold.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
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
const { parseInbound, sendSuggestPick, sendSuggestPreview, sendSuggestEnd } = await bundle('protocol.ts');
const { moveSuggestSelection, suggestPreviewText, suggestGlyph, suggestAction, suggestNeedle, suggestMatch, suggestEmphasis } = await bundle('suggest-logic.ts');
const { ICON_NAMES } = await bundle('icons.gen.ts');
const fixture = JSON.parse(readFileSync(join(here, 'fixtures', 'suggest-messages.json'), 'utf8'));
const take = () => posted.splice(0, posted.length);

test('Rust suggest fixture parses as list, toolbar copy, selection and close', () => {
  const list = parseInbound(fixture.suggest);
  assert.equal(list?.t, 'suggest');
  assert.equal(list.tok, 3);
  assert.equal(list.seq, 12);
  assert.equal(list.w, 560);
  assert.equal(list.sel, 1);
  assert.equal(list.q, 'Exa', 'the hover copy carries the typed text');
  assert.equal(parseInbound(fixture.toolbar).q, '');
  assert.deepEqual(list.items.map((i) => i.k), ['url', 'tab', 'bm', 'hist', 'search']);
  assert.equal(list.items[1].title, 'Guide <b>', 'markup in a title stays text');
  assert.equal(list.items[4].url, '');
  assert.equal(parseInbound(fixture.toolbar).tok, 0);
  assert.deepEqual(parseInbound(fixture.sel), { t: 'suggestSel', seq: 12, sel: 2 });
  const close = parseInbound(fixture.close);
  assert.equal(close.items.length, 0);
  assert.deepEqual(parseInbound(JSON.stringify(fixture.suggest)), list);
});

test('Rust suggestOpen fixture parses as the pill flag; anything else is rejected', () => {
  assert.deepEqual(parseInbound(fixture.open), { t: 'suggestOpen', on: true });
  assert.deepEqual(parseInbound(fixture.shut), { t: 'suggestOpen', on: false });
  assert.deepEqual(parseInbound(JSON.stringify(fixture.open)), { t: 'suggestOpen', on: true });
  for (const bad of [
    { ...fixture.open, on: 1 },
    { ...fixture.open, on: 'true' },
    { ...fixture.open, v: 2 },
    { ...fixture.open, seq: 3 },
    { v: 1, t: 'suggestOpen' },
    (({ v, ...rest }) => rest)(fixture.open),
  ]) {
    assert.equal(parseInbound(bad), null, JSON.stringify(bad));
  }
});

test('malformed suggest messages are rejected', () => {
  const row = fixture.suggest.items[0];
  for (const bad of [
    { ...fixture.suggest, extra: 1 },
    { ...fixture.suggest, v: 2 },
    { ...fixture.suggest, w: 4001 },
    { ...fixture.suggest, seq: -1 },
    { ...fixture.suggest, sel: 8 },
    { ...fixture.suggest, sel: 5 },
    { ...fixture.suggest, items: Array.from({ length: 9 }, () => row) },
    { ...fixture.suggest, items: [{ ...row, k: 'weird' }] },
    { ...fixture.suggest, items: [{ ...row, extra: 1 }] },
    { ...fixture.suggest, items: [{ k: 'url', text: 'a', title: 'a' }] },
    { ...fixture.suggest, items: [{ ...row, text: 'x'.repeat(257) }] },
    { ...fixture.suggest, items: [{ ...row, fav: 'https://example.com/x.png' }] },
    { ...fixture.suggest, items: [{ ...row, fav: 'data:image/gif;base64,AAAA' }] },
    { ...fixture.suggest, q: 1 },
    { ...fixture.suggest, q: 'q'.repeat(513) },
    (({ q, ...rest }) => rest)(fixture.suggest),
    { ...fixture.sel, sel: 8 },
    { ...fixture.sel, extra: true },
  ]) {
    assert.equal(parseInbound(bad), null, JSON.stringify(bad).slice(0, 120));
  }
});

test('emoji, CJK and RTL rows from Rust survive validation; no row is dropped', () => {
  const list = parseInbound(fixture.heavy);
  assert.equal(list?.t, 'suggest', 'a list of over-long non-Latin rows must still parse');
  assert.equal(list.items.length, 4);
  assert.equal(list.sel, 2);
  for (const item of list.items) {
    assert.ok(item.text.length <= 256 && item.url.length <= 256, 'cut to the page limit in UTF-16 units');
    assert.ok(item.text.length > 200, 'and not cut more than needed');
    assert.ok(!/[\uD800-\uDBFF]$/.test(item.text) && !/^[\uDC00-\uDFFF]/.test(item.text), 'never mid-surrogate');
  }
  assert.ok(list.q.length <= 512);
});

test('one invalid row is dropped, not the whole list, and the selection follows its row', () => {
  const rows = fixture.suggest.items;
  const bad = { ...rows[0], text: 'x'.repeat(257) };
  const mixed = (items, sel) => parseInbound({ ...fixture.suggest, items, sel });
  let list = mixed([rows[0], bad, rows[2], rows[3]], 2);
  assert.deepEqual(list.items.map((i) => i.k), ['url', 'bm', 'hist']);
  assert.equal(list.sel, 1, 'row 2 (bm) is now at 1');
  list = mixed([bad, rows[1], rows[2]], 0);
  assert.deepEqual(list.items.map((i) => i.k), ['tab', 'bm']);
  assert.equal(list.sel, 0, 'a dropped selected row falls back to the first');
  list = mixed([rows[0], rows[1], { ...rows[2], extra: 1 }], 1);
  assert.deepEqual(list.items.map((i) => i.k), ['url', 'tab']);
  assert.equal(list.sel, 1);
  assert.equal(mixed([bad, bad], 0), null, 'a list with no valid row at all is rejected');
  assert.equal(mixed([rows[0], bad], 2), null, 'the selection must still point inside the message');
  assert.equal(mixed([], 0).items.length, 0, 'an empty list still closes the dropdown');
});

test('state accepts the split and bookmarked flags and rejects other types', () => {
  const state = { t: 'state', scale: 1, solid: true, dark: false, back: false, forward: false, loading: false, progress: 0, security: 'secure', label: 'x', theme: {} };
  assert.equal(parseInbound(state).split, undefined);
  const flags = parseInbound({ ...state, split: true, bookmarked: false });
  assert.equal(flags.split, true);
  assert.equal(flags.bookmarked, false);
  assert.equal(parseInbound({ ...state, split: 1 }), null);
});

test('outbound suggest commands are validated', () => {
  take();
  assert.equal(sendSuggestPick(12, 7), true);
  assert.equal(sendSuggestPreview(0, 0), true);
  assert.equal(sendSuggestEnd(), true);
  assert.deepEqual(take(), [
    { v: 1, t: 'suggestPick', seq: 12, i: 7 },
    { v: 1, t: 'suggestPreview', seq: 0, i: 0 },
    { v: 1, t: 'suggestEnd' },
  ]);
  for (const [seq, i] of [[1, 8], [1, -1], [1, 1.5], [-1, 0], [0x80000000, 0]]) {
    assert.equal(sendSuggestPick(seq, i), false);
    assert.equal(sendSuggestPreview(seq, i), false);
  }
  assert.deepEqual(take(), []);
});

test('arrow keys wrap and the default row shows what was typed', () => {
  assert.equal(moveSuggestSelection(0, 5, 'ArrowDown'), 1);
  assert.equal(moveSuggestSelection(4, 5, 'ArrowDown'), 0);
  assert.equal(moveSuggestSelection(0, 5, 'ArrowUp'), 4);
  assert.equal(moveSuggestSelection(3, 5, 'ArrowUp'), 2);
  assert.equal(moveSuggestSelection(9, 3, 'ArrowDown'), 0, 'an out-of-range selection is clamped first');
  assert.equal(moveSuggestSelection(0, 0, 'ArrowDown'), 0);
  assert.equal(suggestPreviewText('exa', 0, 'example.com/'), 'exa');
  assert.equal(suggestPreviewText('exa', 2, 'example.org/'), 'example.org/');
});

test('rows use family glyphs that say what picking them does', () => {
  const glyphs = { search: 'search', url: 'globe', hist: 'history', bm: 'bookmark', tab: 'tab-switch' };
  for (const [kind, name] of Object.entries(glyphs)) {
    assert.equal(suggestGlyph(kind), name);
    assert.ok(ICON_NAMES.includes(name), `${name} is in the icon family`);
  }
  assert.equal(suggestAction('tab'), 'Switch to tab');
  for (const kind of ['search', 'url', 'hist', 'bm']) assert.equal(suggestAction(kind), '');
});

test('the typed text is found in rows the way Rust matches it', () => {
  assert.equal(suggestNeedle('  wiki '), 'wiki');
  assert.equal(suggestNeedle('HTTPS://www.Example.com/a'), 'Example.com/a');
  assert.equal(suggestNeedle('http://www.'), '');
  assert.equal(suggestNeedle('www.rust'), 'rust');
  assert.equal(suggestNeedle('wwwx'), 'wwwx');
  assert.deepEqual(suggestMatch('en.wikipedia.org/wiki/Web', 'WIKI'), [3, 7]);
  assert.deepEqual(suggestMatch('Rust Programming', 'rust'), [0, 4]);
  assert.equal(suggestMatch('kiwi', 'wiki'), null);
  assert.equal(suggestMatch('abc', ''), null);
  assert.equal(suggestMatch('ab', 'abc'), null);
  // Only ASCII folds, so offsets stay valid for text whose case maps change length.
  assert.deepEqual(suggestMatch('İstanbul wiki', 'wiki'), [9, 13]);
  assert.equal(suggestMatch('É', 'é'), null);
  // A row that is nothing but the typed text (the search row) has nothing to single out.
  assert.equal(suggestEmphasis('wiki', 'WIKI'), null);
  assert.deepEqual(suggestEmphasis('wikihow.com/', 'wiki'), [0, 4]);
  assert.equal(suggestEmphasis('example.org/', 'wiki'), null);
});

test('the open pill and the dropdown are one surface: same material and edge, no moving container', () => {
  const styles = (name) => readFileSync(join(here, '..', 'src', 'styles', name), 'utf8');
  const rule = (css, selector) => {
    const at = css.indexOf(`${selector} {`) >= 0 ? css.indexOf(`${selector} {`) : css.indexOf(`${selector},`);
    assert.ok(at >= 0, `${selector} exists`);
    return css.slice(css.indexOf('{', at), css.indexOf('}', at));
  };
  const material = 'background: linear-gradient(var(--bw-overlay), var(--bw-overlay)), var(--bw-window-opaque);';
  const pill = rule(styles('toolbar.css'), '.pill.is-open');
  const list = rule(styles('hover.css'), '.bw-suggest');
  for (const body of [pill, list]) {
    assert.ok(body.includes(material), 'shared opaque overlay material');
    assert.match(body, /border(-color)?: (1px solid )?var\(--bw-pill-edge\)/, 'shared pill edge');
  }
  assert.match(pill, /border-bottom-left-radius: 0;/);
  assert.match(pill, /border-bottom-right-radius: 0;/);
  assert.match(pill, /border-bottom-color: transparent;/, 'the pill opens into the list: no seam line');
  assert.match(list, /border-top: 0;/);
  assert.match(list, /border-radius: 0 0 var\(--bw-r-omnibox\) var\(--bw-r-omnibox\);/, "bottom = the pill's radius");
  // The pill's own corner is explicit (half its height), so squaring the bottom never rescales the top.
  assert.match(rule(styles('toolbar.css'), '.pill'), /border-radius: var\(--bw-r-omnibox\);/);
  assert.doesNotMatch(pill, /border-top-(left|right)-radius|border-radius:/, 'the open state leaves the top corners alone');
  assert.match(styles('tokens.css'), /--bw-r-omnibox: calc\(var\(--bw-pill-h\) \/ 2\);/);
  // The container never moves (its edges must stay on the pill's); only rows animate, first open only.
  assert.doesNotMatch(list, /transform|animation/);
  assert.doesNotMatch(styles('hover.css'), /\.bw-suggest\.bw-enter|bw-suggest[^{]*is-down/);
});
// --- end omnibox suggest ---
