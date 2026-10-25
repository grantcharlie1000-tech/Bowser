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
const STOP = /pub const STOP_SCRIPT: &str = "([^"]+)";/.exec(readFileSync(join(dir, 'x_promoted.rs'), 'utf8'))[1];
const CSS = /pub const CSS: &str = "([^"]+)";/.exec(readFileSync(join(dir, 'x_promoted.rs'), 'utf8'))[1];
const EDGE = [
  process.env.BOWSER_TEST_EDGE,
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Microsoft/Edge/Application/msedge.exe',
].find((path) => path && existsSync(path));
const skip = EDGE ? false : 'Microsoft Edge is not installed';

// The structure of a For-you post as of the report from the live timeline: the name and handle are links in
// User-Name, the grey "Ad" sits at the right of the header row left of the "..." menu button, the post text is
// tweetText, and a link card ends in a "From domain" line (normal posts with link cards have it too).
const cell = (id, header, body, extra = '', wrap = true) => {
  const post = `<article data-testid="tweet" role="article"><div class="hdr">
  <div data-testid="User-Name"><a href="/${header.handle}" role="link"><span>${header.name}</span></a><a href="/${header.handle}"><span>@${header.handle}</span></a></div>
  ${header.label ?? ''}<button data-testid="caret" aria-label="More"><svg></svg></button></div>
  <div data-testid="tweetText"><span>${body}</span></div>${extra}</article>`;
  return wrap ? `<div data-testid="cellInnerDiv" id="${id}">${post}</div>` : post.replace('<article', `<article id="${id}"`);
};
const normal = (id, body = 'hello', extra = '') => cell(id, { name: 'Someone', handle: 'someone' }, body, extra);
const label = (text, style) => `<div class="x2">${style === 'wrapped' ? `<div><span>${text}</span><svg></svg></div>` : style === 'bare' ? text : `<span class="x1">${text}</span>`}</div>`;
const ad = (id, text = 'Ad', style = 'span') => cell(id, { name: 'riffle', handle: 'rifflerdotstudio', label: label(text, style) }, 'Buy now', '<div><a href="https://riffle.studio">From riffle.studio</a></div>');

const PAGE = (initial, later, change = '', off = 's.remove();') => `<!doctype html><html><body><div id="tl">${initial}</div><script>
  var out = { errors: [] };
  function hidden() { var r = {}; document.querySelectorAll('[data-testid=cellInnerDiv]').forEach(function (c) { r[c.id] = getComputedStyle(c).display === 'none'; }); return r; }
  try {
    var s = document.createElement('style'); s.id = 'bowser-cosmetic'; s.textContent = ${JSON.stringify(CSS)}; document.documentElement.appendChild(s);
    ${SCRIPT.trim()};
    out.first = hidden();
    setTimeout(function () {
      document.getElementById('tl').insertAdjacentHTML('beforeend', ${JSON.stringify(later)});
      ${change}
      setTimeout(function () {
        out.second = hidden(); out.statsHidden = window.__bowserXPromoted.stats.hidden;
        ${off}
        document.getElementById('tl').insertAdjacentHTML('beforeend', ${JSON.stringify(ad('late'))});
        setTimeout(function () {
          out.afterOff = hidden(); out.marked = document.querySelectorAll('[data-bowser-ad]').length; out.flag = window.__bowserXPromoted;
          s.textContent = ${JSON.stringify(CSS)}; if (!s.isConnected) document.documentElement.appendChild(s);
          ${SCRIPT.trim()};
          setTimeout(function () { out.afterOn = hidden(); finish(); }, 700);
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
    // A normal post whose text is just "Ad", a name that is "Ad", a quoted post with the label word, a link card.
    + cell('n3', { name: 'Someone', handle: 'someone' }, 'Ad')
    + cell('n4', { name: 'Ad', handle: 'ad' }, 'my name is Ad')
    + normal('n5', 'quote', '<div role="link" data-testid="quoteTweet"><span>Promoted</span></div>')
    + normal('n9', 'card', '<div role="link"><span>Ad</span></div><a href="https://x.example">From x.example</a>')
    + '<div data-testid="cellInnerDiv" id="n6"><div>Who to follow</div></div>'
    + ad('a4', 'Ad', 'wrapped');
  const later = normal('n7') + ad('a2', 'Promoted') + ad('a3', 'Sponsored', 'bare') + normal('n8', 'Ad hoc')
    + cell('a5', { name: 'riffle', handle: 'r', label: label('Ad') }, 'no cell wrapper', '', false);
  const r = run(PAGE(initial, later));
  assert.deepEqual(r.first, { n1: false, a1: true, n2: false, n3: false, n4: false, n5: false, n9: false, n6: false, a4: true });
  assert.deepEqual(r.second, { ...r.first, n7: false, a2: true, a3: true, n8: false });
  assert.equal(r.statsHidden, 5);
});

test('a poll option or other body text "Ads" does not hide a post, and recycled cells are checked again', { skip }, () => {
  const poll = '<div data-testid="cardPoll"><div role="radio"><span>Ads</span></div><div role="radio"><span>Promoted</span></div></div>';
  const initial = normal('p1', 'Which do you skip?', poll)
    + normal('p2', 'media', '<div data-testid="tweetPhoto"><div><span>Ad</span></div></div>')
    + ad('r1') + normal('r2') + ad('r3')
    // The header-row label still hides an ad that carries media or a poll of its own.
    + cell('a6', { name: 'riffle', handle: 'r', label: label('Ad') }, 'Buy now', '<div data-testid="tweetPhoto"><div><span>Ads</span></div></div>' + poll);
  // X reuses cells while scrolling: r1's ad becomes a normal post, r2's normal post becomes an ad,
  // and r3's ad cell is emptied into a non-post row.
  const change = `
    document.getElementById('r1').innerHTML = ${JSON.stringify(normal('x').replace(/^<div[^>]*>|<\/div>$/g, ''))};
    document.getElementById('r2').innerHTML = ${JSON.stringify(ad('x').replace(/^<div[^>]*>|<\/div>$/g, ''))};
    document.getElementById('r3').innerHTML = '<div>Who to follow</div>';`;
  const r = run(PAGE(initial, '', change));
  assert.deepEqual(r.first, { p1: false, p2: false, r1: true, r2: false, r3: true, a6: true });
  assert.deepEqual(r.second, { p1: false, p2: false, r1: false, r2: true, r3: false, a6: true });
  assert.equal(r.statsHidden, 2);
});

test('removing the cosmetic style stops the script and unmarks every cell', { skip }, () => {
  const r = run(PAGE(normal('n1') + ad('a1'), normal('n2')));
  assert.equal(r.afterOff.a1, false);
  assert.equal(r.afterOff.late, false);
  assert.equal(r.marked, 0);
  assert.ok(!r.flag);
});


test('split and attribute-only badges work without caret, and attribute recycling reveals a normal post', { skip }, () => {
  const split = cell('split', { name: 'Someone', handle: 'fixture', label: '<div><span>Pro</span><span>moted</span></div>' }, 'fixture');
  const attr = cell('attr', { name: 'Someone', handle: 'fixture', label: '<span title="Sponsored"></span>' }, 'fixture').replace('data-testid="caret"', 'data-testid="menu"');
  const change = `document.querySelector('#attr [title]').setAttribute('title', 'Normal');`;
  const r = run(PAGE(split + attr + cell('author', { name: 'Ad', handle: 'ad' }, 'hello'), '', change));
  assert.deepEqual(r.first, { split: true, attr: true, author: false });
  assert.deepEqual(r.second, { split: true, attr: false, author: false });
});

test('explicit off with cookie CSS remaining stops the script, reveals ads and supports reenable', { skip }, () => {
  const off = `s.textContent = '.cookie-notice{display:none!important}'; ${STOP};`;
  const r = run(PAGE(ad('ad'), '', '', off));
  assert.equal(r.first.ad, true);
  assert.equal(r.afterOff.ad, false);
  assert.equal(r.afterOff.late, false);
  assert.equal(r.marked, 0);
  assert.ok(!r.flag);
  assert.equal(r.afterOn.ad, true);
  assert.equal(r.afterOn.late, true);
});

test('wrapped header links remain visible with either anchor while split badges still hide', { skip }, () => {
  const wrapped = '<div><a href="/fixture"><span>Ad</span></a></div>';
  const username = cell('username', { name: 'Someone', handle: 'fixture', label: wrapped }, 'ordinary')
    .replace('data-testid="caret"', 'data-testid="menu"');
  const caret = cell('caret', { name: 'Ad', handle: 'fixture', label: wrapped }, 'ordinary')
    .replace('data-testid="User-Name"', 'data-testid="legacy-name"');
  const split = cell('split', { name: 'Someone', handle: 'fixture', label: '<div><span>A</span><span>d</span></div>' }, 'promoted');
  const r = run(PAGE(username + caret + split, ''));
  assert.deepEqual(r.first, { username: false, caret: false, split: true });
  assert.deepEqual(r.second, r.first);
  assert.deepEqual(r.afterOn, { username: false, caret: false, split: true, late: true });
});
