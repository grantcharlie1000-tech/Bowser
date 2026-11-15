import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { runInNewContext } from 'node:vm';

const engine = readFileSync(new URL('../../src/engine/webview2.rs', import.meta.url), 'utf8');
const script = /const SLEEP_BLOCK_SCRIPT: &str = r#"([\s\S]*?)"#;/.exec(engine)?.[1];
assert.ok(script, 'the production sleep probe must be available');

function page(media = []) {
  return { document: { querySelectorAll: selector => selector === 'video,audio' ? media : [] }, frames: [] };
}
const blocksSleep = window => runInNewContext(script, { window });

test('sleep preserves URL-backed playback intent while audio or muted video buffers', () => {
  for (const readyState of [0, 1, 2, 3, 4]) {
    for (const muted of [false, true]) {
      assert.equal(blocksSleep(page([{ paused: false, ended: false, readyState, muted, srcObject: null }])), true,
        `playback remains protected at readyState=${readyState}, muted=${muted}`);
    }
  }
});

test('ordinary paused or ended URL-backed media does not indefinitely pin a tab', () => {
  for (const readyState of [0, 2, 4]) {
    assert.equal(blocksSleep(page([{ paused: true, ended: false, readyState, srcObject: null }])), false);
    assert.equal(blocksSleep(page([{ paused: false, ended: true, readyState, srcObject: null }])), false);
  }
});

test('live capture and picture-in-picture retain their independent sleep protection', () => {
  const media = { paused: true, ended: false, readyState: 0, srcObject: { getTracks: () => [{ readyState: 'live' }] } };
  assert.equal(blocksSleep(page([media])), true);
  const pip = page(); pip.document.pictureInPictureElement = {};
  assert.equal(blocksSleep(pip), true);
});

test('sleep still fails closed when media or a child frame cannot be inspected', () => {
  const media = { get paused() { throw new Error('unreadable'); } };
  assert.equal(blocksSleep(page([media])), true);
  const parent = page();
  parent.frames = [{ get document() { throw new Error('cross-origin'); } }];
  assert.equal(blocksSleep(parent), true);
});
