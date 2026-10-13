import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const { code } = esbuild.transformSync(readFileSync(join(here, '..', 'src', 'panels', 'privacy-logic.ts'), 'utf8'), { loader: 'ts', format: 'esm' });
const logic = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);

test('privacy parser accepts exact bounded statistics, settings, and permission records', () => {
  const value = { profile: 'Default', guest: false, stats: { allTime: 4, session: 2, currentSite: 1, topSites: [{ host: 'example.test', count: 4 }], days: [{ day: '2026-10-12', count: 4 }] }, site: 'example.test', security: 'secure', blocking: true, tracking: 'balanced', smartScreen: true, blockerSetting: true, sleeping: false, sleepAfter: 30, permissions: [{ origin: 'https://example.test', permission: 'camera', decision: 'block' }], status: null };
  assert.deepEqual(logic.parsePrivacySnapshot(value), value);
  assert.equal(logic.parsePrivacySnapshot({ ...value, extra: true }), null);
  assert.equal(logic.parsePrivacySnapshot({ ...value, stats: { ...value.stats, session: -1 } }), null);
  assert.equal(logic.parsePrivacySnapshot({ ...value, permissions: [{ ...value.permissions[0], decision: 'allow', extra: true }] }), null);
});

test('privacy commands are limited to explicit flat panel fields', () => {
  assert.deepEqual(logic.privacyCommand('clear-stats'), { a: 'clear-stats' });
  assert.deepEqual(logic.privacyCommand('clear-data', { n: 2, id: 3 }), { a: 'clear-data', n: 2, id: 3 });
  assert.deepEqual(logic.privacyCommand('permission', { id: 0, n: 2 }), { a: 'permission', id: 0, n: 2 });
  assert.equal(logic.privacyCommand('permission', { id: 0, n: 3 }), null);
  assert.equal(logic.privacyCommand('clear-data', { n: 4 }), null);
  assert.equal(logic.privacyCommand('anything'), null);
});

test('the Private tabs note says, in plain words, what private tabs keep and block', () => {
  assert.equal(logic.PRIVATE_TABS_TITLE, 'Private tabs');
  const text = logic.PRIVATE_TABS_NOTE.map(([label, body]) => `${label}. ${body}`).join(' ');
  for (const phrase of [
    'Ctrl+Shift+P', 'New private tab', 'no history, cookies, site data, permission choices or blocker statistics on disk',
    'memory only', 'last private tab closes', 'not restored after a restart or crash', 'never grouped',
    'Strict tracking prevention and the blocker are always on', 'Per-site blocker exceptions are ignored',
    'Every permission request and download in a private tab is blocked',
  ]) assert.ok(text.includes(phrase), phrase);
  // Sentence case, no uppercase eyebrows.
  for (const [label] of logic.PRIVATE_TABS_NOTE) assert.notEqual(label, label.toUpperCase());
  assert.notEqual(logic.PRIVATE_TABS_TITLE, logic.PRIVATE_TABS_TITLE.toUpperCase());
});
