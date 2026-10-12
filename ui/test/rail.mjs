// Tests for the pure left-rail logic (src/rail-logic.ts): key grammar, section split, drop slot, favicon cache.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, '..', 'src', 'rail-logic.ts'), 'utf8');
const { code } = esbuild.transformSync(source, { loader: 'ts', format: 'esm' });
const logic = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
const {
  RAIL_FAV_MAX,
  beforeKey,
  dropSlot,
  initialOf,
  isDraggable,
  isRailIcon,
  isRailKey,
  pruneRailFavs,
  rememberRailFav,
  sectionOf,
  splitSections,
} = logic;

const item = (k, extra = {}) => ({ k, icon: 'g:star', label: k, active: false, dot: false, accent: '', fav: false, ...extra });

test('the key grammar accepts exactly the documented keys', () => {
  for (const k of ['home', 'customize', 'addws', 'addapp', 'bookmarks', 'history', 'menu', 'ws:1', 'app:9']) {
    assert.ok(isRailKey(k), k);
  }
  for (const k of ['', 'Home', 'ws', 'ws:', 'ws:a', 'app:', 'ws:' + '1'.repeat(21), 'app:' + '1'.repeat(11), 'home ', 'toolbar', 'x:1']) {
    assert.ok(!isRailKey(k), JSON.stringify(k));
  }
});

test('icons are checked against the allowed names and glyphs', () => {
  assert.ok(isRailIcon('apptile'));
  assert.ok(isRailIcon('g:leaf'));
  assert.ok(!isRailIcon('g:nope'));
  assert.ok(!isRailIcon('leafy'));
  assert.ok(!isRailIcon(3));
});

test('every key maps to one section, and unknown keys to none', () => {
  assert.equal(sectionOf('home'), 'top');
  assert.equal(sectionOf('customize'), 'top');
  assert.equal(sectionOf('ws:4'), 'workspaces');
  assert.equal(sectionOf('addws'), 'workspaces');
  assert.equal(sectionOf('app:4'), 'apps');
  assert.equal(sectionOf('addapp'), 'apps');
  assert.equal(sectionOf('bookmarks'), 'bottom');
  assert.equal(sectionOf('history'), 'bottom');
  assert.equal(sectionOf('menu'), 'bottom');
  assert.equal(sectionOf('nope'), null);
});

test('only workspace and app buttons are draggable', () => {
  assert.ok(isDraggable('ws:1'));
  assert.ok(isDraggable('app:1'));
  for (const k of ['home', 'addws', 'addapp', 'menu', 'history']) assert.ok(!isDraggable(k), k);
});

test('splitSections keeps message order inside each section and drops unknown keys', () => {
  const items = [
    item('home'),
    item('ws:2'),
    item('app:7'),
    item('ws:1'),
    item('addws'),
    item('bogus'),
    item('addapp'),
    item('app:3'),
    item('menu'),
  ];
  const s = splitSections(items);
  assert.deepEqual(s.top.map((i) => i.k), ['home']);
  assert.deepEqual(s.workspaces.map((i) => i.k), ['ws:2', 'ws:1', 'addws']);
  assert.deepEqual(s.apps.map((i) => i.k), ['app:7', 'addapp', 'app:3']);
  assert.deepEqual(s.bottom.map((i) => i.k), ['menu']);
});

test('dropSlot counts the other items whose centre lies above the dragged centre', () => {
  const centres = [10, 40, 70, 100];
  assert.equal(dropSlot(centres, 0, 5), 0, 'above everything');
  assert.equal(dropSlot(centres, 0, 45), 1, 'past the second centre');
  assert.equal(dropSlot(centres, 0, 500), 3, 'below everything: last slot');
  assert.equal(dropSlot(centres, 2, 70), 2, 'dragged item stays put');
  assert.equal(dropSlot(centres, 2, 15), 1, 'after the first item only');
  assert.equal(dropSlot([10], 0, 99), 0, 'a lone item has one slot');
});

test('beforeKey names the following item, or the empty string for the end', () => {
  const others = ['ws:1', 'ws:5'];
  assert.equal(beforeKey(others, 0), 'ws:1');
  assert.equal(beforeKey(others, 1), 'ws:5');
  assert.equal(beforeKey(others, 2), '');
  assert.equal(beforeKey([], 0), '');
});

test('initialOf takes the first code point, upper-cased, or ? for an empty label', () => {
  assert.equal(initialOf('mail'), 'M');
  assert.equal(initialOf('  zoom'), 'Z');
  assert.equal(initialOf('\u{1F600}x'), '\u{1F600}');
  assert.equal(initialOf(''), '?');
  assert.equal(initialOf('   '), '?');
});

test('favicons are bounded, the newest kept, and an update moves the key to newest', () => {
  const favs = new Map();
  const live = new Set(Array.from({ length: RAIL_FAV_MAX + 10 }, (_, i) => `app:${i}`));
  for (let i = 0; i < RAIL_FAV_MAX + 10; i += 1) rememberRailFav(favs, `app:${i}`, `u${i}`, live);
  assert.equal(favs.size, RAIL_FAV_MAX);
  assert.ok(!favs.has('app:0'), 'oldest dropped');
  assert.equal(favs.get(`app:${RAIL_FAV_MAX + 9}`), `u${RAIL_FAV_MAX + 9}`);
  rememberRailFav(favs, 'app:10', 'new', live);
  assert.equal(favs.get('app:10'), 'new');
  const keys = [...favs.keys()];
  assert.equal(keys[keys.length - 1], 'app:10', 'refreshed key is newest');
});

test('pruneRailFavs drops favicons whose keys left the rail', () => {
  const favs = new Map([['app:1', 'a'], ['app:2', 'b'], ['ws:3', 'c']]);
  pruneRailFavs(favs, new Set(['app:2']));
  assert.deepEqual([...favs.entries()], [['app:2', 'b']]);
});
