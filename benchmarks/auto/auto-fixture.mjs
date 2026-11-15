// Extra loopback routes for the automated timing harness (benchmarks/auto). Hooked into
// production-fixture-server.mjs. Synthetic transport only; nothing here touches the browser.
import fs from 'node:fs';

const ROOT = new URL('../scripts/', import.meta.url);

// Observer module for the conversation page. It drives the page itself (open conversation 1,
// then start streaming on the first keydown) so the harness only has to send real keys and wheel
// notches, and it posts Event Timing / long-animation-frame / frame-gap data back to the server.
const OBSERVER = `
const token = new URL(import.meta.url).searchParams.get('token');
const byId = id => document.getElementById(id);
const events = [], loafs = [], gaps = [];
let armed = false, typed = 0, scrolls = 0, streamStart = 0, posted = false;
try { new PerformanceObserver(list => { for (const e of list.getEntries()) events.push([e.name, +e.startTime.toFixed(1), +e.duration.toFixed(1), +(e.processingStart - e.startTime).toFixed(1)]); }).observe({ type: 'event', durationThreshold: 16, buffered: true }); } catch {}
let loafSupported = true;
try { new PerformanceObserver(list => { for (const e of list.getEntries()) loafs.push([+e.startTime.toFixed(1), +e.duration.toFixed(1), +(e.blockingDuration || 0).toFixed(1)]); }).observe({ type: 'long-animation-frame', buffered: true }); } catch { loafSupported = false; }
let last = 0;
function frame(now) { if (armed) { if (last) gaps.push(+(now - last).toFixed(1)); last = now; requestAnimationFrame(frame); } }
function post(extra) {
  if (posted) return; posted = true;
  armed = false;
  const body = JSON.stringify({ phase: 'stats', events, loafs, loafSupported, gaps, typed, scrolls, streamStart, ...extra });
  fetch('/auto-stats?token=' + token, { method: 'POST', body, keepalive: true, headers: { 'Content-Type': 'application/json' } });
}
function note(phase) { fetch('/auto-stats?token=' + token, { method: 'POST', body: JSON.stringify({ phase }), keepalive: true }); }
// Wait for the client to be ready, open conversation 1, then report readiness.
const status = byId('fixture-status');
const wait = (check, ms = 20000) => new Promise((resolve, reject) => { const t0 = performance.now(); const poll = () => check() ? resolve() : performance.now() - t0 > ms ? reject(new Error('timeout')) : setTimeout(poll, 50); poll(); });
(async () => {
  try {
    await wait(() => status.dataset.action === 'home_ready');
    byId('conversations').querySelector('button').click();
    await wait(() => status.dataset.action === 'conversation_ready' && !byId('composer-form').hidden);
    byId('composer').addEventListener('focus', () => note('composer_focus'));
    byId('composer').focus();
    const transcript = byId('transcript');
    transcript.addEventListener('scroll', () => { if (armed) scrolls++; }, { passive: true });
    byId('composer').addEventListener('input', () => { if (armed) typed++; });
    byId('composer').addEventListener('keydown', () => {
      if (armed) return;
      armed = true; streamStart = performance.now(); requestAnimationFrame(frame);
      byId('stream').click();
      new MutationObserver(() => { if (status.dataset.action === 'stream_complete' || status.dataset.action === 'failed') setTimeout(() => post({ action: status.dataset.action }), 500); }).observe(status, { attributes: true, childList: true });
      setTimeout(() => post({ action: 'timeout' }), 15000);
    }, { once: true });
    note('ready');
  } catch (error) { note('failed'); }
})();
`;

export function serveAutoFixture(req, res, url, token, logFile) {
  if (url.pathname === '/auto-conversation') {
    const html = fs.readFileSync(new URL('conversation-fixture.html', ROOT), 'utf8')
      .replace('FIXTURE_TOKEN', token)
      .replace('</html>', `<script type="module" src="/auto-observer.mjs?token=${token}"></script>\n</html>`);
    res.writeHead(200, { 'Content-Type': 'text/html;charset=utf-8', 'Content-Security-Policy': "default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; connect-src 'self'" });
    res.end(html);
    return true;
  }
  if (url.pathname === '/auto-observer.mjs') {
    res.writeHead(200, { 'Content-Type': 'application/javascript;charset=utf-8' });
    res.end(OBSERVER);
    return true;
  }
  if (url.pathname === '/auto-stats' && req.method === 'POST') {
    const chunks = []; let size = 0;
    req.on('data', chunk => { size += chunk.length; if (size <= 1_000_000) chunks.push(chunk); });
    req.on('end', () => {
      let payload = null;
      try { payload = JSON.parse(Buffer.concat(chunks).toString('utf8')); } catch {}
      fs.appendFileSync(logFile, JSON.stringify({ path: '/auto-stats', token, at_ns: process.hrtime.bigint().toString(), payload }) + '\n');
      res.writeHead(204).end();
    });
    return true;
  }
  return false;
}
