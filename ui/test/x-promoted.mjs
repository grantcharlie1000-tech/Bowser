// X promoted-post hider (src/blocker/x_promoted.js + the rule in x_promoted.rs), run for real in headless
// Edge against an X-like timeline. Skips itself when Edge is not installed. The page installs the same
// <style id="bowser-cosmetic"> the engine does, runs the script, inserts more posts later (SPA scroll) and
// reports which timeline cells are display:none.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
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

const PAGE = (initial, later, change = '') => `<!doctype html><html><body><div id="tl">${initial}</div><script>
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

// The timeline data path in a real browser: X's GraphQL responses come from a local server and are read through
// XMLHttpRequest (text and json, one request already sent before the script ran) and fetch (text and json).
test('promoted entries are dropped from GraphQL timeline responses read through XHR and fetch', { skip }, async () => {
  const entry = (entryId, promoted) => ({ entryId, sortIndex: '1', content: { entryType: 'TimelineTimelineItem', itemContent: { itemType: 'TimelineTweet', tweet_results: { result: { rest_id: entryId } }, ...(promoted ? { promotedMetadata: { impressionId: 'x' } } : {}) } } });
  const timeline = JSON.stringify({ data: { home: { home_timeline_urt: { instructions: [{ type: 'TimelineAddEntries', entries: [
    entry('tweet-1'), entry('promoted-tweet-2-ab', true), entry('tweet-3', true), entry('tweet-4'),
  ] }] } } } });
  const page = `<!doctype html><html><body><div id="tl"></div><script>
    var out = { errors: [] };
    var ids = function (d) { return d.data.home.home_timeline_urt.instructions[0].entries.map(function (e) { return e.entryId; }).join(','); };
    var api = '/i/api/graphql/abc/HomeTimeline';
    function xhr(type) { return new Promise(function (resolve) { var r = new XMLHttpRequest(); r.open('GET', api); r.responseType = type; r.onload = function () { resolve(type === 'json' ? r.response : JSON.parse(r.responseText)); }; r.send(); }); }
    // Sent before the script runs, like a request X starts before ContentLoading's script lands.
    var early = new XMLHttpRequest(); early.open('GET', api); early.send();
    try {
      var s = document.createElement('style'); s.id = 'bowser-cosmetic'; s.textContent = ${JSON.stringify(CSS)}; document.documentElement.appendChild(s);
      ${SCRIPT.trim()};
      Promise.all([
        new Promise(function (r) { if (early.readyState === 4) r(JSON.parse(early.responseText)); else early.onload = function () { r(JSON.parse(early.responseText)); }; }),
        xhr(''), xhr('json'),
        fetch(api).then(function (r) { return r.json(); }),
        fetch(api).then(function (r) { return r.text(); }).then(JSON.parse),
        fetch('/other.json').then(function (r) { return r.json(); }),
      ]).then(function (all) { out.ids = all.map(ids); out.removed = window.__bowserXPromotedNet.stats.removed; finish(); },
        function (e) { out.errors.push(String(e)); finish(); });
    } catch (e) { out.errors.push(String(e && e.stack || e)); finish(); }
    function finish() { var m = document.createElement('meta'); m.name = 'out'; m.content = encodeURIComponent(JSON.stringify(out)); document.head.appendChild(m); }
  </script></body></html>`;
  const server = createServer((req, res) => {
    res.writeHead(200, { 'content-type': req.url === '/' ? 'text/html' : 'application/json' });
    res.end(req.url === '/' ? page : timeline);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const tmp = mkdtempSync(join(tmpdir(), 'bowser-x-test-'));
  try {
    const profile = join(tmp, 'profile');
    mkdirSync(profile);
    // Asynchronous: this process serves the page while the browser loads it.
    const child = spawn(EDGE, [
      '--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check',
      `--user-data-dir=${profile}`, '--virtual-time-budget=6000', '--dump-dom', `http://127.0.0.1:${server.address().port}/`,
    ]);
    let stdout = '';
    child.stdout.on('data', (chunk) => { stdout += chunk; });
    await new Promise((resolve) => child.on('close', resolve));
    const match = /name="out" content="([^"]*)"/.exec(stdout);
    assert.ok(match, 'no report');
    const report = JSON.parse(decodeURIComponent(match[1]));
    assert.deepEqual(report.errors, []);
    const kept = 'tweet-1,tweet-4';
    assert.deepEqual(report.ids, [kept, kept, kept, kept, kept, 'tweet-1,promoted-tweet-2-ab,tweet-3,tweet-4']);
    assert.equal(report.removed, 10);
  } finally {
    server.close();
    rmSync(tmp, { recursive: true, force: true });
  }
});
