// Page translation scripts, run for real in headless Edge (no window is shown; the test skips itself when
// Edge is not installed):
//   1. src/translate/translate.js against a fixture page (what it collects, skips, applies and restores);
//   2. the reader probe's new language field (src/reader/reader.js);
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const src = join(here, '..', '..', 'src');
const SCRIPT = readFileSync(join(src, 'translate', 'translate.js'), 'utf8');
const READER = readFileSync(join(src, 'reader', 'reader.js'), 'utf8');

const EDGE = [
  process.env.BOWSER_TEST_EDGE,
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Microsoft/Edge/Application/msedge.exe',
].find((path) => path && existsSync(path));
const skip = EDGE ? false : 'Microsoft Edge is not installed';

/** Runs headless Edge on `url` and returns the dumped DOM. */
function dump(url, extra = []) {
  const dir = mkdtempSync(join(tmpdir(), 'bowser-translate-test-'));
  try {
    const profile = join(dir, 'profile');
    mkdirSync(profile);
    const result = spawnSync(EDGE, [
      '--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check',
      `--user-data-dir=${profile}`, '--window-size=1000,800', ...extra, '--dump-dom', url,
    ], { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024, timeout: 180000 });
    return { stdout: result.stdout ?? '', stderr: String(result.stderr ?? ''), status: result.status };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

/** Runs `body` (a function body with `R`, the script, and `out`, the report) on `html`, in a file page. */
function runPage(html, script, body) {
  const dir = mkdtempSync(join(tmpdir(), 'bowser-translate-page-'));
  try {
    const driver = `
(function () {
  var R = ${script.trim().replace(/;$/, '')};
  var out = { errors: [] };
  try { ${body} } catch (e) { out.errors.push(String(e && e.stack || e)); }
  var m = document.createElement('meta'); m.name = 'tr-out'; m.content = encodeURIComponent(JSON.stringify(out));
  document.head.appendChild(m);
})();`;
    const file = join(dir, 'page.html');
    writeFileSync(file, html.replace('</body>', () => `<script>${driver}</script></body>`));
    const { stdout, stderr, status } = dump(pathToFileURL(file).href);
    const match = /name="tr-out" content="([^"]*)"/.exec(stdout);
    assert.ok(match, `no report (exit ${status}) ${stderr.slice(0, 300)}`);
    const report = JSON.parse(decodeURIComponent(match[1]));
    assert.deepEqual(report.errors, [], 'driver errors');
    return report;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const PAGE = `<!doctype html><html lang="fr"><head><title>Titre de la page</title></head><body>
<h1 id="h">Bonjour le monde</h1>
<p id="a">  Ceci est un paragraphe avec <b id="b">du gras</b> et des espaces.  </p>
<p id="num">12345 67 ·</p>
<p id="url">https://example.com/page</p>
<p class="notranslate" id="nt">Ne pas traduire</p>
<p translate="no" id="tn">Ne pas traduire non plus <i id="tn2">ni ceci</i></p>
<code id="c">const valeur = 1</code><pre id="pre">pré texte ici</pre>
<textarea id="ta">Zone de texte</textarea>
<div contenteditable="true" id="ce">Éditable ici</div>
<select id="sel"><option>Choix un</option></select>
<script>var ignored = "texte du script";</script><style>p { color: red }</style>
<img id="img" alt="Une image" title="Titre de l'image" src="data:image/gif;base64,R0lGODlhAQABAAAAACw=">
<input id="in" placeholder="Rechercher ici" value="valeur saisie">
<button id="btn" aria-label="Fermer la fenêtre">X</button>
<svg><text id="svgt">Texte svg</text></svg>
<div id="far" style="margin-top:6000px">Texte très loin en bas de la page</div>
</body></html>`;

test('translate.js: collects readable text, skips what must not change, applies in batches and restores exactly', { skip }, () => {
  const r = runPage(PAGE, SCRIPT, `
    var before = document.documentElement.outerHTML;
    var first = R({ a: 'collect' });
    out.first = first;
    var tok = first.k, idx = 0, seen = [], reply = first, calls = 0;
    while (reply.s.length && calls++ < 20) {
      var tr = reply.s.map(function (s) { return '«' + s.toUpperCase() + '»'; });
      seen = seen.concat(reply.s);
      reply = R({ a: 'apply', k: tok, i: idx, s: tr });
      idx = reply.i;
    }
    out.seen = seen; out.end = reply;
    function t(id) { return document.getElementById(id); }
    out.text = {
      h: t('h').textContent, a: t('a').textContent, b: t('b').textContent, num: t('num').textContent, url: t('url').textContent,
      nt: t('nt').textContent, tn: t('tn').textContent, tn2: t('tn2').textContent, c: t('c').textContent, pre: t('pre').textContent,
      ta: t('ta').value, ce: t('ce').textContent, sel: t('sel').textContent, svgt: t('svgt').textContent, far: t('far').textContent,
      title: document.title, alt: t('img').getAttribute('alt'), imgTitle: t('img').getAttribute('title'),
      placeholder: t('in').getAttribute('placeholder'), value: t('in').value, label: t('btn').getAttribute('aria-label'),
      script: document.querySelector('body script').textContent.slice(0, 20)
    };
    out.during = document.documentElement.outerHTML !== before;
    out.again = R({ a: 'collect' });
    out.stale = R({ a: 'apply', k: 'nope', i: 0, s: ['x'] });
    out.badIndex = R({ a: 'apply', k: tok, i: 9999, s: ['x'] });
    out.restore = R({ a: 'restore' });
    out.restored = document.documentElement.outerHTML === before;
    out.restoreTwice = R({ a: 'restore' });
    out.recollect = R({ a: 'collect' }).c;
    R({ a: 'restore' });
    out.restoredAgain = document.documentElement.outerHTML === before;
    out.unknown = R({ a: 'bogus' }).c;
  `);
  // What is collected: text nodes and the four attributes; never code, markup in opt-outs, fields or numbers.
  assert.equal(r.first.c, 'ok');
  assert.equal(r.first.v, 1);
  assert.match(r.first.k, /^[a-z0-9]{1,16}$/);
  assert.equal(r.first.i, 0);
  const seen = r.seen;
  for (const expected of ['Bonjour le monde', 'Ceci est un paragraphe avec', 'du gras', 'et des espaces.', 'Texte très loin en bas de la page', 'Une image', 'Rechercher ici', 'Fermer la fenêtre']) {
    assert.ok(seen.includes(expected), `collects ${expected}: ${JSON.stringify(seen)}`);
  }
  for (const banned of ['12345 67 ·', 'https://example.com/page', 'Ne pas traduire', 'Ne pas traduire non plus', 'ni ceci', 'const valeur = 1', 'pré texte ici', 'Zone de texte', 'Éditable ici', 'Choix un', 'Texte svg', 'texte du script', 'valeur saisie', 'valeur']) {
    assert.ok(!seen.some((s) => s.includes(banned)), `skips ${banned}`);
  }
  assert.ok(seen.every((s) => s === s.trim()), 'batches carry trimmed text');
  // Viewport text first: the heading comes before the far-away block.
  assert.ok(seen.indexOf('Bonjour le monde') < seen.indexOf('Texte très loin en bas de la page'), 'viewport first');
  // Applied: translated, whitespace kept around, nothing skipped touched.
  assert.equal(r.text.h, '«BONJOUR LE MONDE»');
  assert.equal(r.text.b, '«DU GRAS»');
  assert.ok(r.text.a.startsWith('  «CECI EST UN PARAGRAPHE AVEC» '), JSON.stringify(r.text.a));
  assert.ok(r.text.a.endsWith('«ET DES ESPACES.»  '), JSON.stringify(r.text.a));
  assert.equal(r.text.far, '«TEXTE TRÈS LOIN EN BAS DE LA PAGE»');
  assert.equal(r.text.alt, '«UNE IMAGE»');
  assert.equal(r.text.imgTitle, '«TITRE DE L\'IMAGE»');
  assert.equal(r.text.placeholder, '«RECHERCHER ICI»');
  assert.equal(r.text.label, '«FERMER LA FENÊTRE»');
  for (const [key, was] of Object.entries({ num: '12345 67 ·', url: 'https://example.com/page', nt: 'Ne pas traduire', tn: 'Ne pas traduire non plus ni ceci', tn2: 'ni ceci', c: 'const valeur = 1', pre: 'pré texte ici', ta: 'Zone de texte', ce: 'Éditable ici', sel: 'Choix un', svgt: 'Texte svg', value: 'valeur saisie', title: 'Titre de la page' })) {
    assert.equal(key === 'tn' ? r.text.tn : r.text[key], was, `${key} is untouched`);
  }
  assert.equal(r.text.script, 'var ignored = "texte');
  assert.ok(r.during, 'the page really changed');
  assert.equal(r.end.c, 'ok');
  assert.deepEqual(r.end.s, [], 'the last batch is empty');
  assert.equal(r.end.i, r.end.n, 'every item was handed out');
  // Guards.
  assert.equal(r.again.c, 'on', 'a translated page is not collected again');
  assert.equal(r.stale.c, 'stale', 'a token that is not this page\'s');
  assert.equal(r.badIndex.c, 'err', 'an index past the end');
  // Restore: byte for byte, safe twice, and usable again.
  assert.ok(r.restored, 'restored exactly');
  assert.equal(r.restore.c, 'ok');
  assert.equal(r.restoreTwice.c, 'ok');
  assert.equal(r.recollect, 'ok');
  assert.ok(r.restoredAgain);
  assert.equal(r.unknown, 'err');
});

test('translate.js: large pages come in bounded batches and every item is handed out once', { skip }, () => {
  const items = Array.from({ length: 1000 }, (_, i) => `<p>Phrase numéro ${i} ici</p>`).join('\n');
  const r = runPage(`<!doctype html><html lang="fr"><body>${items}</body></html>`, SCRIPT, `
    var before = document.documentElement.outerHTML;
    var reply = R({ a: 'collect' }), tok = reply.k, idx = 0, sizes = [], seen = 0, calls = 0;
    while (reply.s.length && calls++ < 50) {
      sizes.push(reply.s.length); seen += reply.s.length;
      reply = R({ a: 'apply', k: tok, i: idx, s: reply.s.map(function (s) { return s.toUpperCase(); }) });
      idx = reply.i;
    }
    out.sizes = sizes; out.seen = seen; out.n = reply.n;
    out.upper = document.querySelectorAll('p')[999].textContent;
    R({ a: 'restore' });
    out.restored = document.documentElement.outerHTML === before;
  `);
  assert.equal(r.seen, 1000);
  assert.equal(r.n, 1000);
  assert.ok(r.sizes.length >= 4 && r.sizes.every((n) => n <= 300), `batches are bounded: ${r.sizes}`);
  assert.equal(r.upper, 'PHRASE NUMÉRO 999 ICI');
  assert.ok(r.restored);
});

test('translate.js: text the page changed since it was collected is never overwritten or reverted', { skip }, () => {
  const r = runPage(`<!doctype html><html lang="fr"><body><p id="x">Premier texte</p><p id="y">Deuxième texte</p></body></html>`, SCRIPT, `
    var reply = R({ a: 'collect' }), tok = reply.k;
    document.getElementById('x').firstChild.data = 'Texte changé par la page';
    reply = R({ a: 'apply', k: tok, i: 0, s: reply.s.map(function (s) { return '[' + s + ']'; }) });
    out.x = document.getElementById('x').textContent;
    out.y = document.getElementById('y').textContent;
    document.getElementById('y').firstChild.data = 'Autre changement';
    R({ a: 'restore' });
    out.xAfter = document.getElementById('x').textContent;
    out.yAfter = document.getElementById('y').textContent;
  `);
  assert.equal(r.x, 'Texte changé par la page', 'the page\'s newer text wins');
  assert.equal(r.y, '[Deuxième texte]');
  assert.equal(r.xAfter, 'Texte changé par la page');
  assert.equal(r.yAfter, 'Autre changement', 'restore leaves text the page changed again');
});

test('translate.js: a page with nothing to translate reports none and changes nothing', { skip }, () => {
  const r = runPage(`<!doctype html><html lang="fr"><body><p>123 456</p><code>x = 1</code></body></html>`, SCRIPT, `
    var before = document.documentElement.outerHTML;
    out.c = R({ a: 'collect' });
    out.same = document.documentElement.outerHTML === before;
    out.apply = R({ a: 'apply', k: 'abc', i: 0, s: [] }).c;
  `);
  assert.equal(r.c.c, 'none');
  assert.deepEqual(r.c.s, []);
  assert.ok(r.same);
  assert.equal(r.apply, 'stale');
});

test('reader probe: reports the page language as 2 to 3 letters or nothing', { skip }, () => {
  const cases = [
    ['<html lang="fr-CA">', 'fr'],
    ['<html lang="EN_us">', 'en'],
    ['<html lang="fil">', 'fil'],
    ['<html>', ''],
    ['<html lang="x-klingon">', ''],
    ['<html lang="<script>alert(1)</script>">', ''],
    ['<html><head><meta http-equiv="Content-Language" content="de"></head>', 'de'],
    ['<html lang="es"><head><meta http-equiv="content-language" content="de"></head>', 'es'],
  ];
  const results = cases.map(([open, want]) => {
    const r = runPage(`<!doctype html>${open}<body><p>hello</p></body></html>`, READER, `
      out.probe = R({ a: 'probe', m: 'reader', t: 'auto', s: 19, d: false });
    `);
    return { open, want, probe: r.probe };
  });
  for (const { open, want, probe } of results) {
    assert.match(probe, /^r2\|(ok|none)\|off\|\d+\|\d+\|auto\|19\|[a-z]{0,3}$/, `${open}: ${probe}`);
    assert.equal(probe.split('|')[7], want, open);
  }
});
