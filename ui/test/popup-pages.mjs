// Command palette, site info and rename dialog: payload contracts (against the Rust fixture) and
// the pure logic behind each page.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { filterPalette, flatten, moveSelection, scoreItem, subsequenceGap, validPalettePayload, MAX_RESULTS } from '../src/palette-logic.ts';
import { validSiteInfoPayload, cookieText, blockerText, ACT_BLOCKER, ACT_CLEAR_COOKIES, ACT_RESET, CHOICES } from '../src/siteinfo-logic.ts';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
// protocol.ts and rename.ts import without extensions, so they are bundled rather than loaded alone.
async function bundle(file) {
  const out = esbuild.buildSync({ entryPoints: [join(here, '..', 'src', file)], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
  return import(`data:text/javascript;base64,${Buffer.from(out.outputFiles[0].contents).toString('base64')}`);
}
const { parseInbound } = await bundle('protocol.ts');
const { parseRenameSnapshot, localRenameError } = await bundle('rename.ts');
const fixture = JSON.parse(readFileSync(join(here, 'fixtures', 'popup-pages.json'), 'utf8'));

test('Rust palette page passes the strict palette contract', () => {
  assert.equal(validPalettePayload(fixture.palette), true);
  const sections = new Set(fixture.palette.items.map((item) => item.s));
  assert.deepEqual([...sections], ['tab', 'command', 'bookmark', 'history']);
  assert.ok(fixture.palette.items.some((item) => item.acc === 'Ctrl+T'));
  for (const bad of [
    { ...fixture.palette, extra: 1 },
    { ...fixture.palette, tok: -1 },
    { ...fixture.palette, items: [...fixture.palette.items, fixture.palette.items[0]] },
    { ...fixture.palette, items: [{ id: 1, s: 'evil', label: 'x', detail: '' }] },
    { ...fixture.palette, items: [{ id: 1, s: 'tab', label: 'x'.repeat(129), detail: '' }] },
    { ...fixture.palette, items: [{ id: 1, s: 'tab', label: 'x', detail: '', onclick: 'x' }] },
  ]) assert.equal(validPalettePayload(bad), false);
});

test('palette ranks exact, prefix, word, detail, then fuzzy matches and groups by section', () => {
  const items = [
    { id: 0, s: 'tab', label: 'Settings guide', detail: 'https://docs.example/' },
    { id: 1, s: 'command', label: 'Settings', detail: 'Open browser settings' },
    { id: 2, s: 'command', label: 'Save session', detail: 'Save the open tabs' },
    { id: 3, s: 'history', label: 'News', detail: 'https://set.example/' },
    { id: 4, s: 'bookmark', label: 'Sweet tea', detail: 'https://tea.example/' },
  ];
  const needle = 'settings';
  assert.equal(scoreItem(items[1], needle), 3000);
  assert.ok(scoreItem(items[0], needle) > 1000);
  assert.equal(scoreItem(items[3], 'news'), 3000);
  assert.ok(scoreItem(items[3], 'https://set') > 0, 'detail prefix');
  assert.equal(subsequenceGap('st', 'sweet tea'), 4);
  const groups = filterPalette(items, 'set');
  assert.equal(groups[0].section, 'command', 'the best match leads');
  assert.deepEqual(groups.map((g) => g.section), ['command', 'tab', 'bookmark', 'history'], 'fuzzy label beats fuzzy detail');
  assert.deepEqual(flatten(groups).map((item) => item.id).slice(0, 3), [1, 2, 0], 'rows stay under their section');
  assert.deepEqual(filterPalette(items, '').map((g) => g.section), ['tab', 'command']);
  assert.equal(filterPalette(items, 'zzzz').length, 0);
  const many = Array.from({ length: 40 }, (_, id) => ({ id, s: 'history', label: `Page ${id}`, detail: '' }));
  assert.equal(flatten(filterPalette(many, 'page')).length, MAX_RESULTS);
});

test('palette keyboard movement wraps single steps and clamps pages', () => {
  assert.equal(moveSelection(4, 5, 'ArrowDown'), 0);
  assert.equal(moveSelection(0, 5, 'ArrowUp'), 4);
  assert.equal(moveSelection(1, 20, 'PageDown'), 6);
  assert.equal(moveSelection(2, 20, 'PageUp'), 0);
  assert.equal(moveSelection(0, 0, 'ArrowDown'), -1);
});

test('Rust site info page passes the strict site info contract', () => {
  const page = fixture.siteinfo;
  assert.equal(validSiteInfoPayload(page), true);
  assert.deepEqual(page.perms.map((p) => p.k), ['camera', 'mic', 'location', 'notifications']);
  assert.equal(page.perms.find((p) => p.k === 'camera').state, 'allow');
  assert.equal(page.perms.find((p) => p.k === 'mic').state, 'default');
  // The act the page sends for "camera: Allow" is the one Rust parses.
  const camera = page.perms.find((p) => p.k === 'camera');
  assert.deepEqual(fixture.act, { v: 1, t: 'act', tok: page.tok, id: camera.id, n: CHOICES.indexOf('allow') });
  assert.equal(camera.id, 2, 'ids follow Permission::ALL (microphone, camera, location, notifications)');
  assert.deepEqual([ACT_CLEAR_COOKIES, ACT_BLOCKER, ACT_RESET], [10, 11, 12]);
  for (const bad of [
    { ...page, perms: [{ ...page.perms[0], state: 'ask' }] },
    { ...page, cookies: { ...page.cookies, extra: true } },
    { ...page, security: 'evil' },
    { ...page, blocker: { state: 'on' } },
    (({ blockedHere, ...rest }) => rest)(page),
  ]) assert.equal(validSiteInfoPayload(bad), false);
  assert.equal(cookieText({ state: 'count', n: 1, clear: true }), '1 cookie sent to this site');
  assert.equal(blockerText({ state: 'on', n: 3 }), 'On · 3 requests blocked here');
});

test('state carries the optional permblocked flag, strictly typed', () => {
  const base = { t: 'state', scale: 1, solid: false, dark: true, back: false, forward: false, loading: false, progress: 0, security: 'secure', label: '', theme: {} };
  assert.equal(parseInbound({ ...base, permblocked: true }).permblocked, true);
  assert.equal(parseInbound(base).permblocked, undefined);
  assert.equal(parseInbound({ ...base, permblocked: 'yes' }), null);
});

test('rename snapshot is strict and local validation mirrors the shared Rust rules', () => {
  const d = { title: 'Rename profile', save: 'Save', value: 'Work', hint: '1 to 32 letters', error: '', max: 256 };
  assert.deepEqual(parseRenameSnapshot(d), d);
  assert.equal(parseRenameSnapshot({ ...d, extra: 1 }), null);
  assert.equal(parseRenameSnapshot({ ...d, max: 0 }), null);
  assert.equal(localRenameError('   ', 256), 'Enter a name.');
  assert.equal(localRenameError('a\u0007', 256), 'Names cannot contain control characters.');
  assert.equal(localRenameError('abcd', 3), 'Use at most 3 characters.');
  assert.equal(localRenameError(' Work ', 256), '');
});
