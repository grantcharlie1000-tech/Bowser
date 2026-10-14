// Pure keyboard and command logic used by the HTML history panel.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const { code } = esbuild.transformSync(
  await import('node:fs').then(({ readFileSync }) => readFileSync(join(here, '..', 'src', 'panels', 'history-logic.ts'), 'utf8')),
  { loader: 'ts', format: 'esm' },
);
const logic = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);

test('history arrow navigation skips headings and clamps at both ends', () => {
  assert.equal(logic.moveHistorySelection([4, 9, 12], null, 'ArrowDown'), 4);
  assert.equal(logic.moveHistorySelection([4, 9, 12], 4, 'ArrowDown'), 9);
  assert.equal(logic.moveHistorySelection([4, 9, 12], 12, 'ArrowDown'), 12);
  assert.equal(logic.moveHistorySelection([4, 9, 12], 4, 'ArrowUp'), 4);
  assert.equal(logic.moveHistorySelection([4, 9, 12], 9, 'ArrowUp'), 4);
  assert.equal(logic.moveHistorySelection([], null, 'ArrowDown'), null);
});

test('panel Escape dismisses only when a child has not consumed it for inline state', () => {
  assert.equal(logic.shouldDismissPanelEscape(false), true);
  assert.equal(logic.shouldDismissPanelEscape(true), false);
});

test('history open commands carry only a validated positive row id', () => {
  assert.deepEqual(logic.historyOpenCommand(17, true), { v: 1, t: 'hist', a: 'newtab', id: 17 });
  assert.equal(logic.historyOpenCommand(0, false), null);
  assert.equal(logic.historyOpenCommand(0x1_0000_0000, false), null);
  assert.equal(logic.historyOpenCommand(Number.MAX_SAFE_INTEGER + 1, false), null);
});

test('ctrl, cmd and middle clicks open a history entry in a new tab; a plain click does not', () => {
  assert.equal(logic.historyClickOpensNewTab({ ctrlKey: false, metaKey: false, button: 0 }), false);
  assert.equal(logic.historyClickOpensNewTab({ ctrlKey: true, metaKey: false, button: 0 }), true);
  assert.equal(logic.historyClickOpensNewTab({ ctrlKey: false, metaKey: true, button: 0 }), true);
  assert.equal(logic.historyClickOpensNewTab({ ctrlKey: false, metaKey: false, button: 1 }), true);
});
