import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const { code } = esbuild.transformSync(readFileSync(join(here, '..', 'src', 'panels', 'notes-logic.ts'), 'utf8'), { loader: 'ts', format: 'esm' });
const logic = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);

test('notes parser accepts bounded notes and tasks with distinct safe ids', () => {
  const value = { notes: [{ id: 1, title: 'Plan', body: 'Draft' }], tasks: [{ id: 2, text: 'Review', complete: false }] };
  assert.deepEqual(logic.parseNotesSnapshot(value), value);
  assert.equal(logic.parseNotesSnapshot({ ...value, tasks: [{ ...value.tasks[0], id: 1 }] }), null);
  assert.equal(logic.parseNotesSnapshot({ ...value, extra: true }), null);
  assert.equal(logic.parseNotesSnapshot({ ...value, notes: [{ id: 0, title: 'Bad', body: '' }] }), null);
});

test('note and task command builders constrain ids, action and field count', () => {
  assert.deepEqual(logic.notesCommand('delete-note', 1), { a: 'delete-note', id: 1 });
  assert.deepEqual(logic.notesCommand('toggle-task', 2, { on: true }), { a: 'toggle-task', id: 2, on: true });
  assert.equal(logic.notesCommand('delete-note', 0), null);
  assert.equal(logic.notesCommand('invented', 1), null);
});
