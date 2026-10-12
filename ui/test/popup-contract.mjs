import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { validMenuPayload } from '../src/menu-logic.ts';

const here = dirname(fileURLToPath(import.meta.url));
const fixture = JSON.parse(readFileSync(join(here, 'fixtures', 'popup-messages.json'), 'utf8'));

test('Rust popup fixture uses the menu payload contract and enabled item set', () => {
  assert.equal(validMenuPayload(fixture.menu), true);
  assert.equal(fixture.menu.entries[1].submenu[0].label, 'Nested');
  assert.equal(fixture.menu.entries[2].enabled, false);
});
