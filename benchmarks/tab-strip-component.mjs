// Node 22+, Chromium. Run with two immutable ui/dist directories and a new output path.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { once } from 'node:events';
import { chromeMessages } from '../ui/test/chrome-harness.mjs';

const [browser, before, after, output] = process.argv.slice(2);
if (!output) throw Error('Usage: node benchmarks/tab-strip-component.mjs CHROMIUM BEFORE_DIST AFTER_DIST NEW_OUTPUT.json');
const roots = { before: resolve(before), after: resolve(after) };
const hashes = async (root, folder = '') => {
  const rows = {};
  for (const entry of await readdir(join(root, folder), { withFileTypes: true })) {
    const path = join(folder, entry.name);
    if (entry.isDirectory()) Object.assign(rows, await hashes(root, path));
    else rows[path] = createHash('sha256').update(await readFile(join(root, path))).digest('hex');
  }
  return rows;
};
const report = { assets: { before: await hashes(roots.before), after: await hashes(roots.after) }, attempts: [] };
await mkdir(dirname(resolve(output)), { recursive: true });
await writeFile(output, JSON.stringify(report), { flag: 'wx' });
const save = () => writeFile(output, JSON.stringify(report, null, 2) + '\n');
const server = createServer(async (req, res) => {
  const [label, ...rest] = new URL(req.url, 'http://127.0.0.1').pathname.slice(1).split('/');
  const path = rest.join('/');
  try {
    if (!Object.hasOwn(roots, label)) throw Error('unknown build');
    if (path === 'bench-shim.js') {
      res.writeHead(200, { 'Content-Type': 'text/javascript' }).end(shim);
      return;
    }
    if (!Object.hasOwn(report.assets[label], path)) throw Error('unknown asset');
    const mime = path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : 'text/html';
    res.writeHead(200, { 'Content-Type': mime, 'Cache-Control': 'no-store' });
    const contents = await readFile(join(roots[label], path));
    res.end(path === 'index.html'
      ? contents.toString().replace('<script ', '<script src="bench-shim.js"></script><script ')
      : contents);
  } catch {
    res.writeHead(404).end();
  }
});
server.listen(0, '127.0.0.1');
await once(server, 'listening');
const profile = await mkdtemp(join(tmpdir(), 'bowser-tab-strip-'));
const child = spawn(browser, [
  '--headless', '--no-first-run', '--no-default-browser-check', '--window-size=1280,720',
  '--remote-debugging-pipe', `--user-data-dir=${profile}`, 'about:blank',
], { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] });
const pending = new Map();
let nextId = 0;
let buffer = '';
const launchEnded = new Promise((resolveEnded) => {
  child.once('error', resolveEnded);
  child.once('close', resolveEnded);
});
launchEnded.then(() => {
  for (const [id, finish] of pending) finish({ error: { message: `Chromium closed during command ${id}` } });
});
child.stdio[3].on('error', () => {});
child.stdio[4].setEncoding('utf8').on('data', (chunk) => {
  buffer += chunk;
  let boundary;
  while ((boundary = buffer.indexOf('\0')) >= 0) {
    const { id, error, result } = JSON.parse(buffer.slice(0, boundary));
    buffer = buffer.slice(boundary + 1);
    pending.get(id)?.({ error, result });
  }
});
const send = (method, params = {}, sessionId) => new Promise((ok, fail) => {
  const id = ++nextId;
  const timer = setTimeout(() => {
    pending.delete(id);
    fail(Error(`${method} timeout`));
  }, 8000);
  pending.set(id, ({ error, result }) => {
    clearTimeout(timer);
    pending.delete(id);
    if (error) fail(Error(`${method}: ${error.message}`));
    else ok(result);
  });
  child.stdio[3].write(JSON.stringify({ id, method, params, sessionId }) + '\0');
});
const shim = `
  const listeners = [];
  let ready = false;
  let identity = null;
  let handshake = null;
  (window.chrome ??= {}).webview = {
    addEventListener(type, cb) { if (type === 'message') listeners.push(cb); },
    postMessage(value) {
      const msg = typeof value === 'string' ? JSON.parse(value) : value;
      if (msg.t === 'ready') ready = true;
      if (msg.t === 'chromeReady' && msg.view === 'tabs' && /^[0-9a-f]{32}$/.test(msg.doc)) {
        identity = { v: 1, view: msg.view, doc: msg.doc, rev: 1 };
        ready = true;
      }
      if (identity && msg.view === identity.view && msg.doc === identity.doc && msg.rev === identity.rev) handshake = msg.t;
    }
  };
  const deliver = (data) => listeners.forEach((cb) => cb({ data }));
  const tabs = (items) => ({ v: 1, t: 'tabs', active: 1, items });
  const issue = (items) => deliver(tabs(items));
  const frame = () => new Promise((done) => requestAnimationFrame(done));
  const initialize = async (items) => {
    if (identity === null) {
      issue(items);
      return;
    }
    const messages = [...${JSON.stringify(chromeMessages('tabs').filter(({ t }) => !['tabs', 'fav'].includes(t)))}, tabs(items)];
    deliver({ ...identity, t: 'chromeBegin', count: messages.length });
    messages.forEach((state, i) => deliver({ ...identity, t: 'chromePart', i, state }));
    deliver({ ...identity, t: 'chromeCommit' });
    for (let tick = 0; handshake === null && tick < 120; tick++) await frame();
    if (handshake !== 'chromeApplied') throw Error('tab strip did not apply its initial chrome state');
  };
  const check = (items) => {
    const rendered = [...document.querySelectorAll('#app .tab[role=tab]')];
    if (!ready || rendered.length !== items.length || rendered.some((n, i) =>
      Number(n.dataset.id) !== items[i].id || n.querySelector('.title')?.textContent !== items[i].title
      || n.getAttribute('aria-selected') !== String(items[i].id === 1))) throw Error('tab DOM differs from input');
  };
`;
const item = (id) => ({ id, title: `Tab ${id}`, pinned: false, muted: false, private: false, loading: false, marker: -1, g: 0, gl: false, fav: false });
const metrics = ['LayoutCount', 'LayoutDuration', 'RecalcStyleCount', 'RecalcStyleDuration', 'ScriptDuration'];

async function trial(row) {
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  try {
    const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
    const call = (method, params) => send(method, params, sessionId);
    const evaluate = async (expression) => {
      const { result, exceptionDetails } = await call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
      if (exceptionDetails) throw Error(exceptionDetails.exception?.description ?? exceptionDetails.text);
      return result.value;
    };
    await call('Page.enable');
    await call('Page.navigate', { url: `http://127.0.0.1:${server.address().port}/${row.arm}/index.html?view=tabs` });
    for (let poll = 0; poll < 100; poll++) {
      if (await evaluate('typeof ready !== "undefined" && ready')) break;
      await new Promise((done) => setTimeout(done, 20));
    }
    assert.ok(await evaluate('typeof ready !== "undefined" && ready'), 'tab strip never sent ready');
    const initial = Array.from({ length: row.tabs }, (_, i) => item(i + 1));
    const split = Math.floor(row.tabs / 2);
    const next = row.scenario === 'reorder'
      ? [initial[0], ...initial.slice(split), ...initial.slice(1, split)]
      : initial.map((tab, i) => i === 1 ? { ...tab, title: 'Navigation complete' } : tab);
    await evaluate(`(async () => { const items = ${JSON.stringify(initial)}; await initialize(items); await frame(); await frame(); check(items); })()`);
    await call('Performance.enable');
    const snapshot = async () => Object.fromEntries((await call('Performance.getMetrics')).metrics.map(({ name, value }) => [name, value]));
    const first = await snapshot();
    await evaluate(`(async () => { const items = ${JSON.stringify(next)}; issue(items); await frame(); await frame(); check(items); })()`);
    const last = await snapshot();
    row.metrics = Object.fromEntries(metrics.map((key) => [key, last[key] - first[key]]));
    assert.ok(Object.values(row.metrics).every(Number.isFinite), 'missing Chromium counters');
    row.valid = true;
  } finally {
    await send('Target.closeTarget', { targetId });
  }
}

try {
  report.browser = await send('Browser.getVersion');
  for (const tabs of [16, 64]) {
    for (const scenario of ['reorder', 'update']) {
      for (let pair = 1; pair <= 12; pair++) {
        const order = pair % 2 ? ['before', 'after'] : ['after', 'before'];
        for (const arm of order) {
          const row = { tabs, scenario, pair, arm, valid: false };
          report.attempts.push(row);
          try {
            await trial(row);
          } catch (error) {
            row.valid = false;
            row.error = String(error);
          }
          await save();
          if (!row.valid) throw Error(row.error);
        }
      }
    }
  }
} finally {
  try { await send('Browser.close'); } catch { child.kill(); }
  await launchEnded;
  await new Promise((done) => server.close(done));
  await rm(profile, { recursive: true, force: true, maxRetries: 5 });
}
