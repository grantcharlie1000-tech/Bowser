import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as esbuild from 'esbuild';
import { fileURLToPath } from 'node:url';
const bundle = async (entry) => {
  const out = esbuild.buildSync({ entryPoints: [fileURLToPath(new URL('../src/' + entry, import.meta.url))], bundle: true, format: 'esm', platform: 'neutral', write: false });
  return import('data:text/javascript;base64,' + Buffer.from(out.outputFiles[0].contents).toString('base64'));
};
const { installInteractionDiagnostic, interactionFlags } = await bundle('interaction-diagnostic.ts');
const { parseInbound } = await bundle('protocol.ts');
test('diagnostic enable message has a strict non-action schema', () => {
  assert.deepEqual(parseInbound({ v: 1, t: 'diagnosticInteraction', enabled: true }), { t: 'diagnosticInteraction', enabled: true });
  for (const x of [{ v: 1, t: 'diagnosticInteraction', enabled: false }, { v: 1, t: 'diagnosticInteraction', enabled: true, text: 'url' }, { v: 2, t: 'diagnosticInteraction', enabled: true }]) assert.equal(parseInbound(x), null);
});
test('observer is opt-in, idempotent, passive and emits only fixed labels/flags', () => {
  const listeners = []; const messages = [];
  const doc = { activeElement: null, hasFocus: () => true };
  const input = { ownerDocument: doc, value: 'bowser://settings', selectionStart: 0, selectionEnd: 17,
    addEventListener: (...args) => listeners.push(args) };
  doc.activeElement = input;
  assert.equal(listeners.length, 0);
  const report = (event, flags) => messages.push({ event, flags });
  installInteractionDiagnostic(input, report); installInteractionDiagnostic(input, report);
  assert.equal(listeners.length, 4); assert.deepEqual(messages.map(x => x.event), ['armed']);
  assert.ok(listeners.every(x => x[2].passive));
  const event = { key: 'Enter', isComposing: false, ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, defaultPrevented: false, keyCode: 13 };
  listeners[0][1](event); event.defaultPrevented = true; listeners[1][1](event);
  assert.deepEqual(messages.map(x => x.event), ['armed', 'enter_capture', 'enter_bubble']);
  assert.equal(messages[2].flags ^ messages[1].flags, 128);
  assert.ok(messages.every(x => Number.isInteger(x.flags) && x.flags >= 0 && x.flags <= 4095));
  assert.equal(input.value, 'bowser://settings'); assert.equal(doc.activeElement, input);
  assert.equal(interactionFlags(input, { ...event, keyCode: 229 }) & 2048, 2048);
});

test('module-state observation is masked to non-sensitive sixteen-bit flags', () => {
  const doc = { activeElement: null, hasFocus: () => false };
  const input = { ownerDocument: doc, value: 'arbitrary-private-value-not-logged', selectionStart: 1, selectionEnd: 1, addEventListener() {} };
  const reports = []; installInteractionDiagnostic(input, (event, flags) => reports.push({ event, flags }), () => 0xffffffff);
  assert.equal(reports[0].flags, 0xf000); assert.equal(JSON.stringify(reports).includes(input.value), false);
});
