import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const { code } = esbuild.transformSync(readFileSync(join(here, '..', 'src', 'panels', 'customize-logic.ts'), 'utf8'), { loader: 'ts', format: 'esm' });
const logic = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);

test('customize snapshot parser accepts only bounded visibility, workspace, and app records', () => {
  const value = {
    visible: { workspaces: true, apps: false, bookmarks: true, history: false },
    workspaces: [{ id: 1, name: 'Work', icon: 'briefcase', colour: 'blue', active: true, tabs: 3 }],
    apps: [{ id: 9, name: 'Mail', url: 'https://mail.example/', position: 0 }],
  };
  assert.deepEqual(logic.parseCustomizeSnapshot(value), value);
  assert.ok(logic.parseCustomizeSnapshot({ ...value, workspaces: [{ ...value.workspaces[0], id: 0 }] }));
  assert.equal(logic.parseCustomizeSnapshot({ ...value, extra: true }), null);
  assert.equal(logic.parseCustomizeSnapshot({ ...value, apps: [{ ...value.apps[0], id: 0 }] }), null);
  assert.equal(logic.parseCustomizeSnapshot({ ...value, apps: [{ ...value.apps[0], url: 'javascript:alert(1)' }] }), null);
});

test('customize commands use the existing flat cust field families and validate ids', () => {
  assert.deepEqual(logic.customizeCommand('visibility', { text: 'apps', on: true }), { a: 'visibility', text: 'apps', on: true });
  assert.deepEqual(logic.customizeCommand('workspace-rename', { id: 1, text: 'Work' }), { a: 'workspace-rename', id: 1, text: 'Work' });
  assert.deepEqual(logic.customizeCommand('workspace-move', { id: 1, before: 2 }), { a: 'workspace-move', id: 1, before: 2 });
  assert.deepEqual(logic.customizeCommand('workspace-delete', { id: 0 }), { a: 'workspace-delete', id: 0 });
  assert.equal(logic.customizeCommand('app-remove', { id: 0 }), null);
  // Create and edit go through the anchored composer, not the Customize bridge.
  for (const a of ['workspace-create', 'app-add', 'app-update']) assert.equal(logic.customizeCommand(a, { id: 9, text: 'Mail', url: 'https://mail.example/' }), null, a);
  assert.equal(logic.customizeCommand('arbitrary', {}), null);
});

test('customize deletion copy describes both workspace tabs and pinned app data scope', () => {
  assert.match(logic.customizeDeleteDescription('workspace', 'Work'), /tabs.*move/i);
  assert.match(logic.customizeDeleteDescription('app', 'Mail'), /remove.*rail/i);
  assert.match(logic.customizeDeleteDescription('app', 'Mail'), /saved data stays/i);
});

test('moveBefore and stepDest match the drop-slot convention used by the rail', () => {
  const ids = [10, 20, 30, 40];
  assert.equal(logic.moveBefore(ids, 10, 0), 20, 'dest 0 among the others is before the first other');
  assert.equal(logic.moveBefore(ids, 10, 3), undefined, 'dest past the last is the end');
  assert.equal(logic.moveBefore(ids, 40, 1), 20);
  assert.equal(logic.stepDest(4, 0, -1), null);
  assert.equal(logic.stepDest(4, 3, 1), null);
  assert.equal(logic.stepDest(4, 1, 1), 2);
  assert.equal(logic.moveBefore(ids, 20, logic.stepDest(4, 1, 1)), 40, 'one step down lands before the item two below');
  assert.equal(logic.moveBefore(ids, 30, logic.stepDest(4, 2, -1)), 20, 'one step up lands before the one above');
});

test('workspace swatches cover every colour and every sidebar section has a description', () => {
  for (const colour of logic.WORKSPACE_COLOURS) assert.match(logic.WORKSPACE_HEX[colour], /^#[0-9a-f]{6}$/);
  for (const key of logic.RAIL_KEYS) assert.ok(logic.RAIL_DESCRIPTIONS[key].length > 0);
  assert.equal(logic.sentenceCase('briefcase'), 'Briefcase');
});
