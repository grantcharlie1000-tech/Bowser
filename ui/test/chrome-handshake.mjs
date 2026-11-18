import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as esbuild from 'esbuild';
import { chromeFrames, chromeMessages } from './chrome-harness.mjs';

const bundle = esbuild.buildSync({ entryPoints: [new URL('../src/chrome-handshake.ts', import.meta.url).pathname.replace(/^\/([A-Z]:)/, '$1')], bundle: true, format: 'esm', platform: 'neutral', write: false, logLevel: 'silent' });
const { chromeReceiver, settleChrome } = await import(`data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].contents).toString('base64')}`);
const doc = '0123456789abcdef0123456789abcdef';
const tick = () => new Promise((resolve) => setImmediate(resolve));
function controlled(view = 'toolbar') {
  const sent = [], applied = [];
  let complete;
  const waiting = new Promise((resolve) => { complete = resolve; });
  const receiver = chromeReceiver({ view, document: doc, apply: (m) => { applied.push(m); }, settle: () => waiting, post: (m) => { sent.push(m); return true; } });
  return { receiver, sent, applied, complete, frames: chromeFrames({ view, doc }) };
}

for (const view of ['toolbar', 'tabs', 'rail']) {
  test(`${view} acknowledges the complete applied state only after render completion`, async () => {
    const c = controlled(view);
    for (const frame of c.frames.slice(0, -1)) c.receiver.receive(frame);
    assert.deepEqual(c.applied, []);
    assert.deepEqual(c.sent, []);
    c.receiver.receive(c.frames.at(-1));
    await tick();
    assert.equal(c.applied.length, chromeMessages(view).length);
    assert.deepEqual(c.sent, []);
    c.complete(); await tick();
    assert.deepEqual(c.sent, [{ v: 1, t: 'chromeApplied', view, doc, rev: 1 }]);
    c.receiver.receive(c.frames.at(-1)); await tick();
    assert.equal(c.sent.length, 1, 'duplicate commit does not acknowledge twice');
  });
}

test('wrong document/view/revision and late completion after navigation cannot acknowledge', async () => {
  const c = controlled();
  for (const frame of c.frames) {
    c.receiver.receive({ ...frame, doc: 'f'.repeat(32) });
    c.receiver.receive({ ...frame, view: 'rail' });
    c.receiver.receive({ ...frame, rev: 0 });
  }
  assert.deepEqual(c.applied, []);
  for (const frame of c.frames) c.receiver.receive(frame);
  await tick(); c.receiver.close(); c.complete(); await tick();
  assert.deepEqual(c.sent, []);
  for (const frame of c.frames) c.receiver.receive(frame);
  assert.deepEqual(c.sent, []);
});

test('incomplete bundles, missing favicon state and reordered or duplicate parts fail closed', async () => {
  const variants = [
    chromeFrames({ view: 'toolbar', doc }, 1, chromeMessages('toolbar').filter((m) => m.t !== 'sfield')),
    chromeFrames({ view: 'tabs', doc }, 1, chromeMessages('tabs').filter((m) => m.t !== 'fav')),
  ];
  for (const frames of variants) {
    const c = controlled(frames[0].view);
    for (const frame of frames) c.receiver.receive(frame);
    c.complete(); await tick();
    assert.deepEqual(c.applied, []);
    assert.equal(c.sent[0]?.t, 'chromeFailed');
  }
  for (const order of [[0, 2, 1], [0, 1, 1]]) {
    const c = controlled();
    for (const index of order) c.receiver.receive(c.frames[index]);
    assert.equal(c.sent[0]?.t, 'chromeFailed');
    assert.deepEqual(c.applied, []);
  }
});

test('throwing asynchronous handlers do not produce an applied acknowledgement', async () => {
  const sent = [];
  const receiver = chromeReceiver({ view: 'toolbar', document: doc, apply: async (m) => { if (m.t === 'sfield') throw Error('application failed'); }, settle: async () => {}, post: (m) => { sent.push(m); return true; } });
  for (const frame of chromeFrames({ view: 'toolbar', doc })) receiver.receive(frame);
  await tick();
  assert.deepEqual(sent.map((m) => m.t), ['chromeFailed']);
});

test('new revision invalidates an older asynchronous application', async () => {
  const c = controlled();
  for (const frame of c.frames) c.receiver.receive(frame);
  await tick();
  for (const frame of chromeFrames({ view: 'toolbar', doc }, 2)) c.receiver.receive(frame);
  c.complete(); await tick();
  assert.deepEqual(c.sent.map((m) => m.rev), [2]);
  for (const frame of c.frames) c.receiver.receive(frame);
  await tick(); assert.deepEqual(c.sent.map((m) => m.rev), [2]);
});

test('render settling waits for reconciliation, fonts and image decode before two frame opportunities', async () => {
  const calls = [], frames = [];
  let fonts, image;
  globalThis.document = { fonts: { ready: new Promise((resolve) => { fonts = resolve; }) } };
  globalThis.requestAnimationFrame = (cb) => { frames.push(cb); return frames.length; };
  const decoded = new Promise((resolve) => { image = resolve; });
  const root = { querySelectorAll: () => [{ getAttribute: () => 'data:image/png;base64,x', decode: () => { calls.push('decode'); return decoded; } }], getBoundingClientRect: () => { calls.push('layout'); } };
  let settled = false;
  const done = settleChrome(root, () => calls.push('reconcile')).then(() => { settled = true; });
  await tick(); assert.deepEqual(calls, ['reconcile', 'layout']);
  fonts(); await tick(); assert.deepEqual(calls, ['reconcile', 'layout', 'decode']);
  assert.equal(frames.length, 0);
  image(); await tick(); assert.deepEqual(calls, ['reconcile', 'layout', 'decode', 'layout']);
  frames.shift()(); await tick(); assert.equal(settled, false);
  frames.shift()(); await done; assert.equal(settled, true);
});

test('an image that cannot be decoded still settles, so one damaged favicon cannot fail the view', async () => {
  const frames = [];
  globalThis.document = { fonts: { ready: Promise.resolve() } };
  globalThis.requestAnimationFrame = (cb) => { frames.push(cb); return frames.length; };
  const broken = { getAttribute: () => 'data:image/png;base64,iVBORw0KGgo=', decode: () => Promise.reject(new Error('EncodingError')) };
  const root = { querySelectorAll: () => [broken], getBoundingClientRect: () => {} };
  let outcome = 'pending';
  const done = settleChrome(root, () => {}).then(() => { outcome = 'settled'; }, () => { outcome = 'failed'; });
  for (let i = 0; i < 5 && frames.length === 0; i++) await tick();
  frames.shift()(); await tick();
  frames.shift()(); await done;
  assert.equal(outcome, 'settled');
});
