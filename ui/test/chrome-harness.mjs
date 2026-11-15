import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/${name}-messages.json`, import.meta.url), 'utf8'));
export function chromeMessages(view) {
  const rust = fixture('rust');
  if (view === 'rail') return [rust.state, rust.rail, rust.railfav];
  if (view === 'tabs') {
    const sidebar = fixture('sidebar');
    return [rust.state, sidebar.layoutHorizontal, sidebar.workspace, fixture('tab-options').defaults, rust.tabs, rust.fav, fixture('split').pairNone];
  }
  const address = fixture('address');
  return [rust.state, address.addr, address.sfield, rust.setText, fixture('downloads').dlbtnOff, fixture('extensions').buttonOff, fixture('split').pairNone];
}

export function chromeFrames(ready, rev = 1, messages = chromeMessages(ready.view)) {
  const identity = { v: 1, view: ready.view, doc: ready.doc, rev };
  return [
    { ...identity, t: 'chromeBegin', count: messages.length },
    ...messages.map((state, i) => ({ ...identity, t: 'chromePart', i, state })),
    { ...identity, t: 'chromeCommit' },
  ];
}

export function decodeChromeCommand(wire) {
  const message = JSON.parse(wire);
  return message.t === 'chromeAction' ? JSON.parse(message.text) : message;
}

/** Only platform facilities are fake: the actual entrypoint, parsers, receiver, state handlers,
 * keyed reconciliation and render completion run unmodified. */
export function prepareChromeDom(Node) {
  document.fonts = { ready: Promise.resolve() };
  Node.prototype.decode ??= function () { return Promise.resolve(); };
  Node.prototype.getBoundingClientRect ??= function () { return { left: 0, top: 0, right: 800, bottom: 40, width: 800, height: 40 }; };
}

export async function initializeChrome(sent, deliver, view) {
  const ready = sent.findLast((m) => m.t === 'chromeReady' && m.view === view);
  assert.ok(ready, `${view} registers its document before receiving state`);
  const original = globalThis.requestAnimationFrame;
  const frames = [];
  globalThis.requestAnimationFrame = (cb) => { frames.push(cb); return frames.length; };
  try {
    for (const frame of chromeFrames(ready)) deliver(frame);
    for (let turn = 0; turn < 40 && !sent.some((m) => m.t === 'chromeApplied' && m.doc === ready.doc); turn++) {
      await new Promise((resolve) => setImmediate(resolve));
      const pending = frames.splice(0);
      for (const frame of pending) frame(0);
    }
    assert.ok(sent.some((m) => m.t === 'chromeApplied' && m.doc === ready.doc), `${view} applies its real initial bundle`);
  } finally { globalThis.requestAnimationFrame = original; }
}
