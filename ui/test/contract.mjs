// Cross-language contract test: the exact strings below are copied from the Rust unit tests in
// src/chrome_host/protocol.rs (tabs_json_*, fav_json_*). If Rust output changes, update both sides.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, '..', 'src', 'protocol.ts'), 'utf8');
const { code } = esbuild.transformSync(source, { loader: 'ts', format: 'esm' });
const { parseInbound } = await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);

test('empty tabs message from Rust parses', () => {
  const m = parseInbound('{"v":1,"t":"tabs","active":0,"items":[]}');
  assert.deepEqual(m, { t: 'tabs', active: 0, items: [] });
});

test('one tab from Rust parses', () => {
  const m = parseInbound(
    '{"v":1,"t":"tabs","active":7,"items":[{"id":7,"title":"Home","loading":true,"pinned":true,"muted":true,"marker":2,"g":0,"gl":false,"fav":true}]}',
  );
  assert.equal(m?.t, 'tabs');
  assert.equal(m.items[0].title, 'Home');
});

test('group leader and members from Rust parse', () => {
  const m = parseInbound(
    '{"v":1,"t":"tabs","active":11,"items":[' +
      '{"id":10,"title":"Mail","loading":false,"pinned":false,"muted":false,"marker":-1,"g":5,"gl":true,"gname":"Work","gcol":3,"gcollapsed":true,"gsize":2,"fav":false},' +
      '{"id":11,"title":"Calendar","loading":false,"pinned":false,"muted":false,"marker":-1,"g":5,"gl":false,"fav":false},' +
      '{"id":12,"title":"Loose","loading":false,"pinned":false,"muted":false,"marker":-1,"g":0,"gl":false,"fav":false}]}',
  );
  assert.equal(m?.items.length, 3);
  assert.equal(m.items[0].gname, 'Work');
});

test('favicon message from Rust parses', () => {
  const m = parseInbound('{"v":1,"t":"fav","id":5,"uri":"data:image/png;base64,iVBORw0KGgo="}');
  assert.equal(m?.t, 'fav');
});

test('wrong version is rejected', () => {
  assert.equal(parseInbound('{"v":2,"t":"tabs","active":0,"items":[]}'), null);
});
