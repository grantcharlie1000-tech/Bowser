// The segmented thumb's drag target: which option a pointer is over, including the gaps between them.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const built = await esbuild.build({
  entryPoints: [join(here, '..', 'src', 'segment-thumb.ts')],
  bundle: true,
  format: 'esm',
  write: false,
  platform: 'neutral',
});
const thumb = await import(`data:text/javascript;base64,${Buffer.from(built.outputFiles[0].text).toString('base64')}`);

const group = { left: 0, right: 200, top: 10, bottom: 40, disabled: false };
// Centres at 20, 100 and 160. The gap from 40 to 80 splits at 60, halfway between the first two centres.
const segments = [
  { left: 0, right: 40, top: 12, bottom: 38, disabled: false },
  { left: 80, right: 120, top: 12, bottom: 38, disabled: false },
  { left: 140, right: 180, top: 12, bottom: 38, disabled: false },
];

test('a drag selects the segment under the pointer and the nearer side of a gap', () => {
  assert.equal(thumb.segmentIndex(segments, group, 10, 20), 0);
  assert.equal(thumb.segmentIndex(segments, group, 90, 20), 1);
  assert.equal(thumb.segmentIndex(segments, group, 50, 20), 0, 'left half of the gap stays with the first');
  assert.equal(thumb.segmentIndex(segments, group, 70, 20), 1, 'right half of the gap belongs to the nearer centre');
  assert.equal(thumb.segmentIndex(segments, group, 170, 30), 2);
  assert.equal(thumb.segmentIndex(segments, group, 10, 0), 0, 'a little above the control still counts');
});

test('a drag that leaves the control selects nothing, and a disabled segment is not a target', () => {
  assert.equal(thumb.segmentIndex(segments, group, 80, 80), -1, 'well below the control');
  assert.equal(thumb.segmentIndex(segments, group, 400, 20), -1, 'well to the right');
  assert.equal(thumb.segmentIndex([], group, 10, 20), -1);
  const blocked = segments.map((box, i) => (i === 1 ? { ...box, disabled: true } : box));
  assert.equal(thumb.segmentIndex(blocked, group, 80, 20), -1, 'the pointer is on a disabled segment');
  assert.equal(thumb.segmentIndex(blocked, group, 160, 20), 2, 'another segment still works');
});
