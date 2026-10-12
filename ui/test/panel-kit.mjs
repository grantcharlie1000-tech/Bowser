// Pure logic of the shared panel kit: list navigation, previews, drop zones.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const { code } = esbuild.transformSync(readFileSync(join(here, '..', 'src', 'panels', 'kit-logic.ts'), 'utf8'), { loader: 'ts', format: 'esm' });
const logic = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);

test('navIndex clamps at both ends and handles Home and End', () => {
  assert.equal(logic.navIndex(3, -1, 'ArrowDown'), 0);
  assert.equal(logic.navIndex(3, -1, 'ArrowUp'), 2);
  assert.equal(logic.navIndex(3, 2, 'ArrowDown'), 2);
  assert.equal(logic.navIndex(3, 0, 'ArrowUp'), 0);
  assert.equal(logic.navIndex(3, 1, 'Home'), 0);
  assert.equal(logic.navIndex(3, 1, 'End'), 2);
  assert.equal(logic.navIndex(0, -1, 'ArrowDown'), -1);
  assert.equal(logic.navIndex(3, 1, 'x'), 1);
});

test('isNavKey owns only the four list keys', () => {
  for (const key of ['ArrowDown', 'ArrowUp', 'Home', 'End']) assert.equal(logic.isNavKey(key), true);
  for (const key of ['ArrowLeft', 'Enter', 'Tab', 'a']) assert.equal(logic.isNavKey(key), false);
});

test('preview collapses whitespace and cuts long text with an ellipsis', () => {
  assert.equal(logic.preview('a\n\n  b   c'), 'a b c');
  assert.equal(logic.preview('x'.repeat(100), 10), `${'x'.repeat(9)}…`);
  assert.equal(logic.preview('   '), '');
});

test('tileLetter uses the first letter or digit, upper-cased', () => {
  assert.equal(logic.tileLetter('example.test'), 'E');
  assert.equal(logic.tileLetter('  9lives'), '9');
  assert.equal(logic.tileLetter('...'), '·');
});

test('dropZone splits folders into three zones and links into two', () => {
  assert.equal(logic.dropZone(2, 32, true), 'before');
  assert.equal(logic.dropZone(16, 32, true), 'into');
  assert.equal(logic.dropZone(30, 32, true), 'after');
  assert.equal(logic.dropZone(4, 32, false), 'before');
  assert.equal(logic.dropZone(20, 32, false), 'after');
  assert.equal(logic.dropZone(5, 0, true), 'after');
});
