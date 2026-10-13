// Page zoom values: the toolbar chip and the site information row share these (src/zoom-logic.ts).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const { code } = esbuild.transformSync(readFileSync(join(here, '..', 'src', 'zoom-logic.ts'), 'utf8'), { loader: 'ts', format: 'esm' });
const zoom = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);

test('zoom limits match Rust (25 to 500, default 100)', () => {
  assert.deepEqual([zoom.MIN_ZOOM, zoom.MAX_ZOOM, zoom.DEFAULT_ZOOM], [25, 500, 100]);
  const rust = readFileSync(join(here, '..', '..', 'src', 'site_prefs.rs'), 'utf8');
  assert.match(rust, /pub const ZOOM_MIN: u16 = 25;/);
  assert.match(rust, /pub const ZOOM_MAX: u16 = 500;/);
  assert.match(rust, /pub const ZOOM_DEFAULT: u16 = 100;/);
});

test('only whole percents inside the range are valid', () => {
  for (const ok of [25, 100, 125, 500]) assert.equal(zoom.validZoom(ok), true, String(ok));
  for (const bad of [24, 501, 0, -1, 99.5, NaN, Infinity, '100', null, undefined, true, {}]) assert.equal(zoom.validZoom(bad), false, String(bad));
});

test('the chip shows for any valid zoom except 100% and an absent value', () => {
  assert.equal(zoom.isZoomed(undefined), false);
  assert.equal(zoom.isZoomed(100), false);
  assert.equal(zoom.isZoomed(125), true);
  assert.equal(zoom.isZoomed(25), true);
  assert.equal(zoom.isZoomed(500), true);
  assert.equal(zoom.isZoomed(9000), false, 'out of range never shows');
});

test('chip text, name and tooltip', () => {
  assert.equal(zoom.zoomText(125), '125%');
  assert.equal(zoom.zoomChipLabel(80), 'Zoom 80%. Reset to 100%');
  assert.equal(zoom.zoomChipTip(80), 'Zoom 80% (Ctrl+0 resets)');
});
