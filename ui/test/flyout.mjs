// Rail flyouts (docs/design/flyouts.md): the `flyout` message, the rail's click/hover commands with
// their anchor rectangle, and the hover-switch rule. The wire strings match the Rust tests in
// src/chrome_host/protocol.rs (flyout_json, parses_rail_click_and_menu).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const load = async (file) => {
  const built = esbuild.buildSync({
    entryPoints: [join(here, '..', 'src', file)],
    bundle: true,
    format: 'esm',
    platform: 'neutral',
    write: false,
    logLevel: 'silent',
  });
  return import(`data:text/javascript;base64,${Buffer.from(built.outputFiles[0].contents).toString('base64')}`);
};
const { parseInbound, sendRail } = await load('protocol.ts');
const { FLYOUT_HOVER_MS, flyoutOf, hoverSwitches } = await load('rail-logic.ts');

test('the flyout message carries a caret offset or -1 and nothing else', () => {
  assert.deepEqual(parseInbound('{"v":1,"t":"flyout","caret":22}'), { t: 'flyout', caret: 22 });
  assert.deepEqual(parseInbound('{"v":1,"t":"flyout","caret":-1}'), { t: 'flyout', caret: -1 });
  for (const bad of [
    '{"v":1,"t":"flyout","caret":-2}',
    '{"v":1,"t":"flyout","caret":4001}',
    '{"v":1,"t":"flyout","caret":2.5}',
    '{"v":1,"t":"flyout","caret":"22"}',
    '{"v":1,"t":"flyout"}',
    '{"v":1,"t":"flyout","caret":22,"x":1}',
    '{"v":2,"t":"flyout","caret":22}',
  ]) {
    assert.equal(parseInbound(bad), null, bad);
  }
});

test('rail clicks carry the button rectangle; hover needs one', () => {
  const sent = [];
  globalThis.window = { chrome: { webview: { postMessage: (wire) => sent.push(wire) } } };
  try {
    const rect = { left: 4.4, top: 640, right: 32.2, bottom: 666 };
    assert.equal(sendRail('click', 'history', rect), true);
    assert.equal(sendRail('click', 'home'), true);
    assert.equal(sendRail('hover', 'bookmarks', rect), true);
    assert.equal(sendRail('hover', 'bookmarks'), false, 'a hover without its anchor is not sent');
    assert.deepEqual(sent, [
      '{"v":1,"t":"rail","a":"click","k":"history","r":"4,640,29,26"}',
      '{"v":1,"t":"rail","a":"click","k":"home"}',
      '{"v":1,"t":"rail","a":"hover","k":"bookmarks","r":"4,640,29,26"}',
    ]);
  } finally {
    delete globalThis.window;
  }
});

test('flyout buttons map to their flyout; the more button opens Customize', () => {
  assert.equal(flyoutOf('history'), 'history');
  assert.equal(flyoutOf('bookmarks'), 'bookmarks');
  assert.equal(flyoutOf('customize'), 'customize');
  assert.equal(flyoutOf('menu'), 'customize');
  for (const key of ['home', 'ws:1', 'app:2', 'addws', 'addapp']) assert.equal(flyoutOf(key), null, key);
});

test('resting on a flyout button is reported; Rust decides whether a flyout switches', () => {
  for (const key of ['history', 'bookmarks', 'customize', 'menu']) assert.equal(hoverSwitches(key), true, key);
  for (const key of ['home', 'ws:1', 'app:2', 'addws', 'addapp']) assert.equal(hoverSwitches(key), false, key);
  assert.equal(FLYOUT_HOVER_MS, 120);
});
