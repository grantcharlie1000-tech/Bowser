// Pure logic of the shared panel kit: list navigation, previews, drop zones.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readFileSync, readdirSync } from 'node:fs';
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

// ---- HTML select (select-logic.ts) and the no-native-controls contract.
const { code: selectCode } = esbuild.transformSync(readFileSync(join(here, '..', 'src', 'panels', 'select-logic.ts'), 'utf8'), { loader: 'ts', format: 'esm' });
const select = await import(`data:text/javascript;base64,${Buffer.from(selectCode).toString('base64')}`);

test('typeaheadIndex jumps to the next label with the typed prefix and cycles repeated letters', () => {
  const labels = ['Blue', 'Violet', 'Teal', 'Amber', 'Rose', 'Red', 'Green', 'Slate'];
  assert.equal(select.typeaheadIndex(labels, 0, 'g'), 6);
  assert.equal(select.typeaheadIndex(labels, 4, 'r'), 5, 'repeating from Rose moves on to Red');
  assert.equal(select.typeaheadIndex(labels, 5, 'rr'), 4, 'a repeated letter cycles forward and wraps');
  assert.equal(select.typeaheadIndex(labels, 0, 'sl'), 7);
  assert.equal(select.typeaheadIndex(labels, 0, 'q'), -1);
  assert.equal(select.typeaheadIndex([], 0, 'a'), -1);
  assert.equal(select.typeaheadIndex(labels, 0, ''), -1);
});

test('select keys: opening keys, and printable characters feed typeahead', () => {
  for (const key of ['Enter', ' ', 'ArrowDown', 'ArrowUp', 'Home', 'End']) assert.equal(select.opensSelect(key), true, key);
  for (const key of ['Tab', 'Escape', 'a']) assert.equal(select.opensSelect(key), false, key);
  const plain = { ctrlKey: false, metaKey: false, altKey: false };
  assert.equal(select.isTypeKey({ key: 'a', ...plain }), true);
  assert.equal(select.isTypeKey({ key: ' ', ...plain }), false);
  assert.equal(select.isTypeKey({ key: 'a', ...plain, ctrlKey: true }), false);
  assert.equal(select.isTypeKey({ key: 'Enter', ...plain }), false);
});

test('placeList opens below, flips above only when it fits better, and stays inside the viewport', () => {
  const view = { width: 380, height: 900 };
  const below = select.placeList({ top: 100, bottom: 132, left: 16, width: 200 }, view, 288);
  assert.equal(below.above, false);
  assert.equal(below.top, 136);
  assert.equal(below.width, 200);
  const flipped = select.placeList({ top: 700, bottom: 732, left: 16, width: 200 }, view, 288);
  assert.equal(flipped.above, true);
  assert.equal(flipped.top, 700 - 4 - 288);
  const cramped = select.placeList({ top: 40, bottom: 72, left: 16, width: 200 }, { width: 380, height: 160 }, 288);
  assert.equal(cramped.above, false, 'more room below than above keeps it below');
  assert.ok(cramped.maxHeight >= 48 && cramped.maxHeight <= 160);
  const edge = select.placeList({ top: 100, bottom: 132, left: 300, width: 200 }, view, 100);
  assert.ok(edge.left + edge.width <= view.width - 8, 'clamped to the right edge');
  const narrow = select.placeList({ top: 100, bottom: 132, left: 16, width: 100 }, view, 100, 4, 8, 160);
  assert.equal(narrow.width, 160, 'minimum list width wins over a narrow trigger');
});

test('panels and dialogs build no native select, checkbox, radio, colour or range controls', () => {
  const srcDir = join(here, '..', 'src');
  const files = [...readdirSync(join(srcDir, 'panels')).map((f) => join(srcDir, 'panels', f)), join(srcDir, 'dialog.ts'), join(srcDir, 'rename.ts'), join(srcDir, 'sidebar.ts')];
  for (const file of files.filter((f) => f.endsWith('.ts'))) {
    const text = readFileSync(file, 'utf8');
    assert.doesNotMatch(text, /(createElement|\bh|\bel)\(\s*['"](select|option)['"]/, `${file} creates a native select`);
    assert.doesNotMatch(text, /type\s*=\s*['"](checkbox|radio|color|range|date|file)['"]/, `${file} creates a native ${text.match(/type\s*=\s*['"](\w+)['"]/)?.[1]} input`);
  }
});
