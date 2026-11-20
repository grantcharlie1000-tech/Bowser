import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { existsSync, readFileSync } from 'node:fs';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const logicPath = join(here, '..', 'src', 'panels', 'bookmarks-logic.ts');
const { code } = esbuild.transformSync(
  existsSync(logicPath) ? readFileSync(logicPath, 'utf8') : '',
  { loader: 'ts', format: 'esm' },
);
const logic = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);

test('bookmark snapshot validation accepts folders, links and search paths', () => {
  const snapshot = { items: [
    { id: 1, parent: null, title: 'Reading', kind: 'folder' },
    { id: 2, parent: 1, title: 'Spec', kind: 'link', url: 'https://example.test', folderPath: ['Reading'] },
  ], query: 'spec', currentFolder: 1 };
  const defaults = { singleClick: true, other: logic.OTHER_WIRE, expanded: [], total: 0, error: '', undo: '', canAdd: false, favs: {} };
  assert.deepEqual(logic.validateBookmarkSnapshot(snapshot), { ...snapshot, ...defaults }, 'an older host gets the defaults');
});

test('the newer snapshot fields are checked strictly', () => {
  const base = { items: [], query: '', currentFolder: null };
  const ok = logic.validateBookmarkSnapshot({ ...base, other: 7, expanded: [3, 4], total: 12, error: 'No.', undo: 'Deleted “A”', canAdd: true, favs: { 'a.example': 'data:image/png;base64,AAAA' } });
  assert.equal(ok.other, 7);
  assert.deepEqual(ok.expanded, [3, 4]);
  assert.equal(ok.undo, 'Deleted “A”');
  assert.equal(ok.favs['a.example'], 'data:image/png;base64,AAAA');
  for (const bad of [{ other: 0 }, { expanded: [0] }, { total: -1 }, { error: 7 }, { canAdd: 'yes' }, { favs: { 'a.example': 'javascript:alert(1)' } }, { favs: [] }]) {
    assert.equal(logic.validateBookmarkSnapshot({ ...base, ...bad }), null, JSON.stringify(bad));
  }
  assert.equal(logic.validateBookmarkSnapshot({ ...base, items: [{ id: 1, parent: null, title: 'F', kind: 'folder', n: -1 }] }), null);
});

test('the tree has two roots: the bar (the top level) and Other bookmarks', () => {
  const snap = logic.validateBookmarkSnapshot({ items: [
    { id: 1, parent: null, title: 'News', kind: 'link', url: 'https://news.example/' },
    { id: 2, parent: null, title: 'Other bookmarks', kind: 'folder', n: 2 },
    { id: 3, parent: 2, title: 'Recipes', kind: 'link', url: 'https://food.example/' },
    { id: 4, parent: null, title: 'Work', kind: 'folder', n: 5 },
  ], query: '', currentFolder: null, other: 2 });
  assert.deepEqual(logic.childrenOf(snap, logic.BAR_ROOT).map((i) => i.id), [1, 4], 'Other bookmarks is not on the bar');
  assert.deepEqual(logic.childrenOf(snap, 2).map((i) => i.id), [3]);
  assert.equal(logic.countOf(snap, 4), 5, 'a folder counts what the snapshot left out');
  assert.equal(logic.rootOf(snap, snap.items[2]), 2);
  assert.equal(logic.rootOf(snap, snap.items[0]), logic.BAR_ROOT);
  assert.equal(logic.isWithin(snap, 3, 2), true);
  assert.equal(logic.isWithin(snap, 1, 2), false);
  assert.equal(logic.wireParent(logic.BAR_ROOT), 0);
});

test('a typed address becomes a web address the way other browsers fix it', () => {
  assert.equal(logic.normalizeAddress('  example.com/path '), 'https://example.com/path');
  assert.equal(logic.normalizeAddress('localhost:8080'), 'https://localhost:8080');
  assert.equal(logic.normalizeAddress('//cdn.example/x'), 'https://cdn.example/x');
  assert.equal(logic.normalizeAddress('http://plain.example/'), 'http://plain.example/');
  assert.equal(logic.normalizeAddress('ftp://files.example/'), 'ftp://files.example/', 'Bowser says why it refuses');
  assert.equal(logic.normalizeAddress('   '), '');
});

test('expand, undo, copy, sort and add-this-page commands carry exactly their fields', () => {
  assert.deepEqual(logic.bookmarkExpandCommand(4, true), { v: 1, t: 'bm', a: 'expand', id: 4, on: true });
  assert.equal(logic.bookmarkExpandCommand(logic.OTHER_WIRE, true), null, 'the virtual Other bookmarks has nothing to load');
  assert.deepEqual(logic.bookmarkActionCommand('undo'), { v: 1, t: 'bm', a: 'undo' });
  assert.deepEqual(logic.bookmarkActionCommand('copy', 9), { v: 1, t: 'bm', a: 'copy', id: 9 });
  assert.deepEqual(logic.bookmarkActionCommand('sort', undefined, { parent: logic.BAR_ROOT }), { v: 1, t: 'bm', a: 'sort', parent: 0 });
  assert.deepEqual(logic.bookmarkActionCommand('addcurrent', undefined, { parent: logic.OTHER_WIRE }), { v: 1, t: 'bm', a: 'addcurrent', parent: logic.OTHER_WIRE });
  assert.deepEqual(logic.bookmarkMoveCommand(8, logic.BAR_ROOT, 0), { v: 1, t: 'bm', a: 'move', id: 8, parent: 0, before: 0 });
});

test('the single-click option comes from Rust and must be a boolean', () => {
  const base = { items: [], query: '', currentFolder: null };
  assert.equal(logic.validateBookmarkSnapshot({ ...base, singleClick: true }).singleClick, true);
  assert.equal(logic.validateBookmarkSnapshot({ ...base, singleClick: false }).singleClick, false);
  assert.equal(logic.validateBookmarkSnapshot({ ...base, singleClick: 'no' }), null);
});

test('open-all is a bookmark action that names one folder id', () => {
  assert.deepEqual(logic.bookmarkActionCommand('openall', 4), { v: 1, t: 'bm', a: 'openall', id: 4 });
  assert.equal(logic.bookmarkActionCommand('openall'), null);
  assert.equal(logic.bookmarkActionCommand('openall', 0), null);
});
test('bookmark snapshot validation rejects unsafe or malformed identifiers and records', () => {
  assert.equal(logic.validateBookmarkSnapshot({ items: [{ id: 0, parent: null, title: 'bad', kind: 'folder' }], query: '', currentFolder: null }), null);
  assert.equal(logic.validateBookmarkSnapshot({ items: [{ id: 1, parent: 1, title: 'bad', kind: 'link', url: 7 }], query: '', currentFolder: null }), null);
  assert.equal(logic.validateBookmarkSnapshot({ items: [], query: '', currentFolder: Number.MAX_SAFE_INTEGER + 1 }), null);
});

test('bookmark move commands use stable ids and before zero for the end', () => {
  assert.deepEqual(logic.bookmarkMoveCommand(8, 3, 4), { v: 1, t: 'bm', a: 'move', id: 8, parent: 3, before: 4 });
  assert.deepEqual(logic.bookmarkMoveCommand(8, null, 0), { v: 1, t: 'bm', a: 'move', id: 8, parent: 0, before: 0 });
  assert.equal(logic.bookmarkMoveCommand(0, null, 0), null);
});

test('bookmark action commands keep the native bridge flat and validate ids', () => {
  assert.deepEqual(logic.bookmarkActionCommand('rename', 8, { text: 'Saved' }), { v: 1, t: 'bm', a: 'rename', id: 8, text: 'Saved' });
  assert.deepEqual(logic.bookmarkActionCommand('addlink', null, { parent: null, text: 'Saved', url: 'https://example.test' }), { v: 1, t: 'bm', a: 'addlink', parent: 0, text: 'Saved', url: 'https://example.test' });
  assert.equal(logic.bookmarkActionCommand('delete', Number.MAX_SAFE_INTEGER + 1), null);
});
