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
// --- end omnibox suggest ---
