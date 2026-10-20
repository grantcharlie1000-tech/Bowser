// The split view host page (split.ts) in the shared fake document: which surface it shows for each
// message from Rust, and that it puts the one it leaves away. The start page bundle is a stub here
// (the real one is covered by start-page.mjs); it records when it is shown and hidden.
import { after, test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import * as esbuild from 'esbuild';
import { FakeDocument, Node } from './fake-dom.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, '..', 'src');
const fixture = (name) => JSON.parse(readFileSync(join(here, 'fixtures', name), 'utf8'));

Object.defineProperty(Node.prototype, 'parentElement', { get() { return this.parent; } });
Object.defineProperty(Node.prototype, 'firstChild', { get() { return this.children[0] ?? null; } });
Node.prototype.cloneNode = function cloneNode(deep) {
  const copy = new Node(this.tagName);
  copy.className = this.className;
  copy.attrs = new Map(this.attrs);
  if (deep) for (const child of this.children) copy.append(child.cloneNode(true));
  return copy;
};
Node.prototype.closest =function closest(s) { for (let n = this; n; n = n.parent) if (n.matches?.(s)) return n; return null; };
const sent = [];
const listeners = [];
globalThis.document = new FakeDocument();
globalThis.window = globalThis;
Object.assign(globalThis, {
  innerWidth: 1280, innerHeight: 860, devicePixelRatio: 1,
  matchMedia: () => ({ matches: false, addEventListener() {} }),
  requestAnimationFrame: (cb) => { cb(); return 0; },
  cancelAnimationFrame() {},
  getComputedStyle: () => ({}),
  addEventListener() {}, removeEventListener() {},
  MutationObserver: class { observe() {} disconnect() {} },
  ResizeObserver: class { observe() {} disconnect() {} },
  chrome: { webview: { postMessage: (wire) => sent.push(JSON.parse(wire)), addEventListener: (_t, cb) => listeners.push(cb) } },
});
globalThis.Element = Node;
globalThis.HTMLElement = Node;
globalThis.HTMLInputElement = class {};
globalThis.HTMLTextAreaElement = class {};
const deliver = (msg) => { for (const cb of listeners) cb({ data: JSON.stringify(msg) }); };

// The host imports `./start-page.js` next to itself at run time: bundle it into a temporary folder
// with a stub start page beside it.
const out = mkdtempSync(join(tmpdir(), 'bowser-split-host-'));
esbuild.buildSync({ entryPoints: [join(srcDir, 'split.ts')], bundle: true, format: 'esm', platform: 'neutral', outfile: join(out, 'split.js'), logLevel: 'silent' });
writeFileSync(join(out, 'start-page.js'), `export function buildStartPage() {
  const root = document.createElement('div');
  root.className = 'stub-start';
  const log = globalThis.__startLog;
  return { root, show(m) { log.push('show'); }, hidden() { log.push('hidden'); } };
}\n`);
globalThis.__startLog = [];
const { startSplit } = await import(pathToFileURL(join(out, 'split.js')).href);
after(() => rmSync(out, { recursive: true, force: true }));

const root = new Node('div');
root.id = 'app';
startSplit(root);
const settle = () => new Promise((resolve) => setTimeout(resolve, 20));
const startSnap = fixture('start-page-messages.json').page;
const split = fixture('split-messages.json');

test('from the start page straight to the private page, the start page is put away (U9)', async () => {
  deliver(startSnap);
  await settle();
  assert.equal(root.classList.contains('is-start'), true, 'the start page shows');
  assert.deepEqual(globalThis.__startLog, ['show']);
  deliver(split.private);
  assert.equal(root.classList.contains('is-private'), true);
  assert.equal(root.classList.contains('is-start'), false, 'no start page look over the private page');
  assert.equal(root.querySelector('.stub-start'), null);
  assert.deepEqual(globalThis.__startLog, ['show', 'hidden'], 'the start page is told it is hidden');
  // And back: the start page again, the private page put away.
  deliver(startSnap);
  assert.equal(root.classList.contains('is-private'), false);
  assert.equal(root.classList.contains('is-start'), true);
  deliver(split.hide);
  assert.equal(root.classList.contains('is-start'), false);
  assert.deepEqual(globalThis.__startLog, ['show', 'hidden', 'show', 'hidden']);
});
