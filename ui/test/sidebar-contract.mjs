// Cross-language contract test for the vertical sidebar messages.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const bundled = await esbuild.build({
  entryPoints: [join(here, '..', 'src', 'protocol.ts')],
  bundle: true,
  format: 'esm',
  platform: 'neutral',
  write: false,
  logLevel: 'silent',
});
const { parseInbound } = await import(`data:text/javascript;base64,${Buffer.from(bundled.outputFiles[0].contents).toString('base64')}`);

test('Rust vertical-sidebar fixture messages parse with the strict TS protocol', () => {
  const fixture = JSON.parse(readFileSync(join(here, 'fixtures', 'sidebar-messages.json'), 'utf8'));
  assert.deepEqual(parseInbound(fixture.layoutVertical), { t: 'layout', mode: 'vertical' });
  assert.deepEqual(parseInbound(fixture.layoutHorizontal), { t: 'layout', mode: 'horizontal' });
  assert.deepEqual(parseInbound(fixture.workspace), { t: 'workspace', name: 'Work & Play', icon: 'g:code' });
});

test('workspace sidebar metadata rejects extra, unsafe, and malformed fields', () => {
  assert.equal(parseInbound('{"v":1,"t":"workspace","name":"Work","icon":"g:code","extra":1}'), null);
  assert.equal(parseInbound('{"v":1,"t":"workspace","name":"Work","icon":"javascript:alert(1)"}'), null);
  assert.equal(parseInbound('{"v":1,"t":"workspace","name":3,"icon":"g:code"}'), null);
});
