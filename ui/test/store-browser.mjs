// Headless Edge verifies the rendered UI with a simulated native message boundary.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { captureStore, STORE_TEST_EDGE } from '../scripts/capture-store.mjs';
test('rendered Store journey and light/dark desktop/narrow screenshots', { skip: STORE_TEST_EDGE ? false : 'Microsoft Edge is not installed; real browser UI rendering not verified', timeout: 300000 }, () => {
  assert.equal(captureStore(mkdtempSync(join(tmpdir(), 'bowser-store-render-'))), 8);
});
