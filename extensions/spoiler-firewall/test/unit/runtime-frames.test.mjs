// Pure helpers behind the sandboxed-iframe rule (red-team C-1): the cheap HTML-to-text tokenizer for srcdoc / data: frames.
// The parent never parses untrusted frame HTML into the live document; these functions only read strings.
import test from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const SFW = createRequire(import.meta.url)(join(ROOT, "dist", "node", "engine.cjs"));
const R = SFW.runtime;

test("srcdocText: text, attribute words and file names are read; style/script bodies and comments are not", () => {
  const t = R.srcdocText("<style>.a{color:red}</style><p class=\"x\">GTA &amp; <b>6</b> ending</p><!-- hidden spoiler --><img alt=\"Final mission\" src=\"/a/gta-six-ending.png\"><script>var secret='x'</script>tail &#65; &lt;ok&gt;");
  assert.match(t, /GTA & {1,3}6 {1,3}ending/);
  assert.match(t, /Final mission/);
  assert.match(t, /gta six ending/);
  assert.match(t, /tail A <ok>/);
  assert.ok(!/hidden spoiler|secret|color:red/.test(t));
});

test("srcdocText: tags split a word into parts the unit text joins without separators (tight view is the extractor's job); nothing throws on junk", () => {
  for (const junk of ["", "<", "<<<>>>", "<p", "<p class='unterminated", "</", "<!--", "<script>", "<a href=", "&", "&#;", "&#xZZ;", "<p\u0000>x", "a<b", "\u0000"]) {
    assert.equal(typeof R.srcdocText(junk), "string");
  }
  assert.equal(R.srcdocText("plain text"), "plain text");
});

test("srcdocText: linear and bounded on large hostile input", () => {
  const big = "<p>".repeat(300000) + "x".repeat(200000);
  const t0 = Date.now();
  const out = R.srcdocText(big);
  assert.equal(typeof out, "string");
  assert.ok(Date.now() - t0 < 2000, "tokenizer stays fast on 1.1 MB of tags");
  const quotes = "<a title=\"".repeat(100000);
  const t1 = Date.now();
  R.srcdocText(quotes);
  assert.ok(Date.now() - t1 < 2000, "unterminated quotes do not go quadratic");
});

test("decodeEntities: named, decimal and hex; unknown entities stay", () => {
  assert.equal(R.decodeEntities("a &amp; b &lt;c&gt; &quot;d&quot; &#65;&#x42; &nbsp;e &bogus;"), "a & b <c> \"d\" AB  e &bogus;");
  assert.equal(R.decodeEntities("&#0; &#1114112; &#xD800;").length > 0, true);
});

test("dataUrlHtml: text/html data URLs (percent-encoded and base64) decode; other types and non-data URLs do not", () => {
  assert.equal(R.dataUrlHtml("data:text/html,%3Cp%3Ehi%3C%2Fp%3E"), "<p>hi</p>");
  assert.equal(R.dataUrlHtml("data:text/html;charset=utf-8;base64," + Buffer.from("<p>héllo</p>").toString("base64")), "<p>héllo</p>");
  assert.equal(R.dataUrlHtml("data:image/png;base64,AAAA"), null);
  assert.equal(R.dataUrlHtml("https://example.com/x.html"), null);
  assert.equal(typeof R.dataUrlHtml("data:text/html,%E0%A4%A"), "string", "malformed percent-encoding falls back to the raw text");
});

test("hasToken: sandbox token list matching is case-insensitive and exact", () => {
  assert.equal(R.hasToken("allow-forms allow-scripts", "allow-scripts"), true);
  assert.equal(R.hasToken("ALLOW-SCRIPTS", "allow-scripts"), true);
  assert.equal(R.hasToken("allow-scripts-not", "allow-scripts"), false);
  assert.equal(R.hasToken("", "allow-scripts"), false);
});

test("engine bundle loads without any DOM globals (natives are captured defensively)", () => {
  assert.equal(typeof SFW.runtime.start, "function");
  assert.equal(typeof SFW.shield.emergencyNotice, "function");
  assert.equal(typeof SFW.hooks.watchDocument, "function");
});
