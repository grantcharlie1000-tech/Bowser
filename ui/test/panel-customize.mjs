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
  assert.deepEqual(logic.customizeCommand('app-update', { id: 9, text: 'Mail', url: 'https://mail.example/' }), { a: 'app-update', id: 9, text: 'Mail', url: 'https://mail.example/' });
  assert.deepEqual(logic.customizeCommand('workspace-move', { id: 1, before: 2 }), { a: 'workspace-move', id: 1, before: 2 });
  assert.deepEqual(logic.customizeCommand('workspace-delete', { id: 0 }), { a: 'workspace-delete', id: 0 });
  assert.equal(logic.customizeCommand('app-remove', { id: 0 }), null);
  assert.equal(logic.customizeCommand('app-add', { name: '<img>', url: 'javascript:alert(1)' }), null);
  assert.equal(logic.customizeCommand('arbitrary', {}), null);
});

test('customize deletion copy describes both workspace tabs and pinned app data scope', () => {
  assert.match(logic.customizeDeleteDescription('workspace', 'Work'), /tabs.*move/i);
  assert.match(logic.customizeDeleteDescription('app', 'Mail'), /remove.*rail/i);
  assert.match(logic.customizeDeleteDescription('app', 'Mail'), /saved data stays/i);
});
