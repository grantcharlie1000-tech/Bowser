import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, '..', 'src', 'panels', 'settings-logic.ts'), 'utf8').replace("import { MAX_TEXT } from '../protocol';", 'const MAX_TEXT = 2048;');
const { code } = esbuild.transformSync(source, { loader: 'ts', format: 'esm' });
const logic = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);

const snapshot = {
  tabLayout: 'vertical', sidebarWidth: 260, sleepEnabled: true, sleepAfterSecs: 300,
  blockerEnabled: true, blockerAllow: ['example.com'], searchEngine: 'optimised',
  searchFallback: 'default', automaticTabGrouping: true,
};

test('settings snapshot accepts exactly the fields backed by Settings and search-provider state', () => {
  assert.deepEqual(logic.parseSettingsSnapshot(snapshot), snapshot);
  assert.equal(logic.parseSettingsSnapshot({ ...snapshot, sidebarWidth: 179 }), null);
  assert.equal(logic.parseSettingsSnapshot({ ...snapshot, sleepAfterSecs: 29 }), null);
  assert.equal(logic.parseSettingsSnapshot({ ...snapshot, searchEngine: 'custom' }), null);
  assert.equal(logic.parseSettingsSnapshot({ ...snapshot, extra: true }), null);
  assert.equal(logic.parseSettingsSnapshot({ ...snapshot, blockerAllow: ['bad site.example'] }), null);
});

test('settings action serializes a complete validated value through the existing flat set channel', () => {
  assert.deepEqual(logic.settingsCommand('save', snapshot), { a: 'save', text: 'tab_layout=vertical\nsidebar_width=260\nsleep_enabled=true\nsleep_after_secs=300\nblocker_enabled=true\nblocker_allow=example.com\nautomatic_tab_grouping=true\nsearch_engine=optimised' });
  assert.deepEqual(logic.settingsCommand('reset'), { a: 'reset' });
  assert.equal(logic.settingsCommand('save', { ...snapshot, sidebarWidth: 999 }), null);
  assert.equal(logic.settingsCommand('unknown'), null);
});
