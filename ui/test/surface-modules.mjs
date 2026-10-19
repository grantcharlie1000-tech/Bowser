// Real HTTP module loading under the unchanged production CSP. This is a load/first-use
// regression smoke, not a native performance benchmark or a substitute for WebView2 verification.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const browser = [process.env.BOWSER_TEST_EDGE, 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe', 'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe'].find((p) => p && existsSync(p));
const views = ['toolbar', 'tabs', 'rail', 'panel', 'popup', 'bar', 'dialog', 'hover', 'split', 'settings', 'history', 'downloads'];
const startFixture = JSON.parse(readFileSync(join(root, 'test/fixtures/start-page-messages.json'), 'utf8')).empty;
const shim = `const listeners = []; window.chrome = { webview: { addEventListener(type, callback) { if (type === 'message') listeners.push(callback); }, postMessage(value) {
 const m = typeof value === 'string' ? JSON.parse(value) : value;
 if (m.t === 'ready') { document.documentElement.setAttribute('data-surface-ready', 'yes'); document.documentElement.setAttribute('data-surface-ready-ms', performance.now().toFixed(3));
 if (new URLSearchParams(location.search).get('view') === 'split') setTimeout(() => listeners.forEach((callback) => callback({ data: ${JSON.stringify(startFixture)} })), 0);
 }
} } }; const observer = new MutationObserver(() => {
 if (document.getElementById('app')?.classList.contains('is-start') && !document.documentElement.hasAttribute('data-start-page-ready-ms')) document.documentElement.setAttribute('data-start-page-ready-ms', performance.now().toFixed(3));
}); observer.observe(document.documentElement, { attributes: true, childList: true, subtree: true });
window.addEventListener('error', () => document.documentElement.setAttribute('data-surface-error', 'yes'));
window.addEventListener('unhandledrejection', () => document.documentElement.setAttribute('data-surface-error', 'yes'));`;

test('every surface starts through actual assets under the production CSP', { skip: !browser, timeout: 180000 }, async () => {
  const requested = new Set();
  const sources = { candidate: join(root, 'dist') };
  if (process.env.BOWSER_UI_BASELINE_DIST) sources.baseline = process.env.BOWSER_UI_BASELINE_DIST;
  const html = readFileSync(join(sources.candidate, 'index.html'), 'utf8');
  const reports = [];
  const csp = html.match(/Content-Security-Policy" content="([^"]+)"/)[1];
  for (const dir of Object.values(sources)) {
    assert.equal(readFileSync(join(dir, 'index.html'), 'utf8').match(/Content-Security-Policy" content="([^"]+)"/)[1], csp, 'identical production CSP for matched module loads');
  }
  const server = createServer((req, res) => {
    const [label, ...parts] = new URL(req.url, 'http://localhost').pathname.slice(1).split('/');
    const path = parts.join('/');
    if (!Object.hasOwn(sources, label)) { res.writeHead(404); res.end(); return; }
    requested.add(`${label}/${path}`);
    res.setHeader('Content-Security-Policy', csp);
    res.setHeader('X-Content-Type-Options', 'nosniff');
    res.setHeader('Cache-Control', 'no-store');
    if (path === 'shim.js') { res.setHeader('Content-Type', 'text/javascript'); res.end(shim); return; }
    if (path === 'index.html') { res.setHeader('Content-Type', 'text/html'); res.end(readFileSync(join(sources[label], 'index.html'), 'utf8').replace('<script ', '<script src="shim.js"></script><script ')); return; }
    if (!/^(?:[a-z-]+\.js|chunks\/chunk-[A-Z0-9]+\.js|app\.css)$/.test(path)) { res.writeHead(404); res.end(); return; }
    try { res.setHeader('Content-Type', path.endsWith('.css') ? 'text/css' : 'text/javascript'); res.end(readFileSync(join(sources[label], path))); }
    catch { res.writeHead(404); res.end(); }
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const profile = mkdtempSync(join(tmpdir(), 'bowser-module-smoke-'));
  try {
    for (const [index, view] of views.entries()) {
      for (const [useIndex, use] of ['first', 'warm'].entries()) {
        const order = Object.keys(sources);
        if ((index + useIndex) % 2) order.reverse();
        for (const label of order) {
          const started = performance.now();
          const args = ['--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check', `--user-data-dir=${join(profile, label)}`, '--virtual-time-budget=2500', '--dump-dom', `http://127.0.0.1:${server.address().port}/${label}/index.html?view=${view}${view === 'dialog' ? '&p=notes' : ''}`];
          // Container-only test runner constraint, never a product/benchmark launch flag.
          if (process.platform !== 'win32' && process.getuid?.() === 0) args.unshift('--no-sandbox');
          const result = await new Promise((resolve, reject) => {
            const child = spawn(browser, args); let out = '', err = '';
            const timer = setTimeout(() => { child.kill(); reject(new Error(`${view}: browser timeout`)); }, 12000);
            child.stdout.on('data', (d) => { out += d; }); child.stderr.on('data', (d) => { err += d; });
            child.on('error', reject); child.on('close', (code) => { clearTimeout(timer); resolve({ out, err, code }); });
          });
          assert.equal(result.code, 0, `${view}: ${result.err.slice(-1000)}`);
          assert.match(result.out, /data-surface-ready="yes"/, `${view}: module did not register and send ready`);
          assert.doesNotMatch(result.out, /data-surface-error="yes"|page-load-failed|csp-not-enforced/, view);
          const readyMs = Number(/data-surface-ready-ms="([0-9.]+)"/.exec(result.out)?.[1]);
          assert.ok(Number.isFinite(readyMs), `${view}: missing document-navigation-to-ready endpoint`);
          const startMs = view === 'split' ? Number(/data-start-page-ready-ms="([0-9.]+)"/.exec(result.out)?.[1]) : null;
          if (view === 'split') assert.ok(Number.isFinite(startMs), `${label}: lazy start-page module did not render`);
          reports.push({ label, view, use, virtual_document_navigation_to_start_page_proxy_ms: startMs, virtual_document_navigation_to_bridge_ready_ms: readyMs, external_browser_launch_load_proxy_ms: performance.now() - started });
          console.log(`module smoke ${label} ${view} ${use}: ${(performance.now() - started).toFixed(1)}ms external browser launch/load proxy (not native benchmark)`);
        }
      }
    }
    assert.ok(requested.has('candidate/app.js'), 'actual chrome entry fetched');
    for (const view of ['settings', 'history']) assert.ok(requested.has(`candidate/${view}-page.js`), `${view}: actual page entry fetched`);
    for (const label of Object.keys(sources)) assert.ok(requested.has(`${label}/start-page.js`), `${label}: actual lazy start-page entry fetched`);
    if (process.env.BOWSER_UI_SURFACE_REPORT) writeFileSync(process.env.BOWSER_UI_SURFACE_REPORT, `${JSON.stringify({ scope: 'Exploratory load correctness smoke. External spawn endpoint is wall-clock and includes full Edge launch; in-page performance.now endpoints use virtual time, not wall-clock latency. Not native interaction benchmarks or statistical no-regression proof', sources, alternating_order: true, reports }, null, 2)}\n`);
  } finally {
    await new Promise((resolve) => server.close(resolve));
    rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
  }
});
