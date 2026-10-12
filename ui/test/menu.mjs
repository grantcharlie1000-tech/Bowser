import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, '..', 'src', 'menu-logic.ts'), 'utf8');
const { code } = esbuild.transformSync(source, { loader: 'ts', format: 'esm' });
const logic = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);

test('menu navigation wraps and skips disabled rows and separators', () => {
  const entries = [{ id: 1, label: 'A', enabled: true, checked: false }, { separator: true }, { id: 2, label: 'B', enabled: false, checked: false }, { id: 3, label: 'C', enabled: true, checked: false }];
  assert.equal(logic.stepMenu(entries, 0, 1), 3);
  assert.equal(logic.stepMenu(entries, 3, 1), 0);
  assert.equal(logic.stepMenu(entries, 0, -1), 3);
  assert.equal(logic.stepMenu(entries, -1, -1), 3);
});

test('Home and End find the first and last enabled item', () => {
  const entries = [{ separator: true }, { id: 1, label: 'A', enabled: false, checked: false }, { id: 2, label: 'B', enabled: true, checked: false }];
  assert.equal(logic.edgeMenu(entries, false), 2);
  assert.equal(logic.edgeMenu(entries, true), 2);
  assert.equal(logic.edgeMenu([{ separator: true }], true), -1);
});

test('menu payload caps nested entries and validates selectable row fields', () => {
  const msg = { v: 1, t: 'menu', tok: 4, kind: 'sample', entries: [{ id: 1, label: 'One', enabled: true, checked: false }] };
  assert.equal(logic.validMenuPayload(msg), true);
  assert.equal(logic.validMenuPayload({ ...msg, entries: Array.from({ length: 129 }, (_, id) => ({ id, label: 'x', enabled: true, checked: false })) }), false);
  assert.equal(logic.validMenuPayload({ ...msg, entries: [{ ...msg.entries[0], label: 'x'.repeat(129) }] }), false);
  assert.equal(logic.validMenuPayload({ ...msg, entries: [{ ...msg.entries[0], enabled: 'yes' }] }), false);
});
