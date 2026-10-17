// X promoted-post hider (src/blocker/x_promoted.js + the rule in x_promoted.rs), run for real in headless
// Edge against an X-like timeline. Skips itself when Edge is not installed. The page installs the same
// <style id="bowser-cosmetic"> the engine does, runs the script, inserts more posts later (SPA scroll) and
// reports which timeline cells are display:none.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const dir = join(here, '..', '..', 'src', 'blocker');
const SCRIPT = readFileSync(join(dir, 'x_promoted.js'), 'utf8');
const CSS = /pub const CSS: &str = "([^"]+)";/.exec(readFileSync(join(dir, 'x_promoted.rs'), 'utf8'))[1];
const EDGE = [
  process.env.BOWSER_TEST_EDGE,
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Microsoft/Edge/Application/msedge.exe',
].find((path) => path && existsSync(path));
const skip = EDGE ? false : 'Microsoft Edge is not installed';

const cell = (id, header, body, extra = '') => `<div data-testid="cellInnerDiv" id="${id}"><article data-testid="tweet">
  <div><div data-testid="User-Name"><span>${header.name}</span><span>@${header.handle}</span></div>${header.label ? `<div><span class="x1">${header.label}</span></div>` : ''}</div>
  <div data-testid="tweetText"><span>${body}</span></div>${extra}</article></div>`;
const normal = (id, body = 'hello') => cell(id, { name: 'Someone', handle: 'someone' }, body);
const ad = (id, label = 'Ad') => cell(id, { name: 'Brand', handle: 'brand', label }, 'Buy now');

const PAGE = (initial, later) => `<!doctype html><html><body><div id="tl">${initial}</div><script>
  var out = { errors: [] };
  function hidden() { var r = {}; document.querySelectorAll('[data-testid=cellInnerDiv]').forEach(function (c) { r[c.id] = getComputedStyle(c).display === 'none'; }); return r; }
  try {
    var s = document.createElement('style'); s.id = 'bowser-cosmetic'; s.textContent = ${JSON.stringify(CSS)}; document.documentElement.appendChild(s);
    ${SCRIPT.trim()};
    out.first = hidden();
    setTimeout(function () {
      document.getElementById('tl').insertAdjacentHTML('beforeend', ${JSON.stringify(later)});
      setTimeout(function () {
        out.second = hidden();
        s.remove();
        document.getElementById('tl').insertAdjacentHTML('beforeend', ${JSON.stringify(ad('late'))});
        setTimeout(function () {
          out.afterOff = hidden(); out.marked = document.querySelectorAll('[data-bowser-ad]').length; out.flag = window.__bowserXPromoted;
          finish();
        }, 700);
      }, 700);
    }, 400);
  } catch (e) { out.errors.push(String(e && e.stack || e)); finish(); }
  function finish() { var m = document.createElement('meta'); m.name = 'out'; m.content = encodeURIComponent(JSON.stringify(out)); document.head.appendChild(m); }
</script></body></html>`;

function run(html) {
  const tmp = mkdtempSync(join(tmpdir(), 'bowser-x-test-'));
  try {
    const file = join(tmp, 'page.html');
    writeFileSync(file, html);
    const profile = join(tmp, 'profile');
    mkdirSync(profile);
    const result = spawnSync(EDGE, [
      '--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check',
      `--user-data-dir=${profile}`, '--virtual-time-budget=6000', '--dump-dom', pathToFileURL(file).href,
    ], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, timeout: 60000 });
    const match = /name="out" content="([^"]*)"/.exec(result.stdout ?? '');
    assert.ok(match, `no report (exit ${result.status}) ${String(result.stderr).slice(0, 300)}`);
    const report = JSON.parse(decodeURIComponent(match[1]));
    assert.deepEqual(report.errors, []);
    return report;
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}

test('only promoted cells are hidden, on load and for posts inserted later', { skip }, () => {
  const initial = normal('n1') + ad('a1') + normal('n2')
    // A normal post whose text is just "Ad", a name that is "Ad", and a quoted post with the label word in it.
    + cell('n3', { name: 'Someone', handle: 'someone' }, 'Ad')
    + cell('n4', { name: 'Ad', handle: 'ad' }, 'my name is Ad')
    + cell('n5', { name: 'Someone', handle: 'someone' }, 'quote', '<div data-testid="quoteTweet"><span>Promoted</span></div>')
    + '<div data-testid="cellInnerDiv" id="n6"><div>Who to follow</div></div>';
  const later = normal('n7') + ad('a2', 'Promoted') + ad('a3', 'Sponsored') + normal('n8', 'Ad hoc');
  const r = run(PAGE(initial, later));
  assert.deepEqual(r.first, { n1: false, a1: true, n2: false, n3: false, n4: false, n5: false, n6: false });
  assert.deepEqual(r.second, { ...r.first, n7: false, a2: true, a3: true, n8: false });
});

test('removing the cosmetic style stops the script and unmarks every cell', { skip }, () => {
  const r = run(PAGE(normal('n1') + ad('a1'), normal('n2')));
  assert.equal(r.afterOff.a1, false);
  assert.equal(r.afterOff.late, false);
  assert.equal(r.marked, 0);
  assert.equal(r.flag, false);
});
