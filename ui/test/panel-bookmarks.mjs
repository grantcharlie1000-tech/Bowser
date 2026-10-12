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
  assert.deepEqual(logic.validateBookmarkSnapshot(snapshot), snapshot);
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
