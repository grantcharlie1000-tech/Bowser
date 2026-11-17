// Unit tests for the OCR-specific release rules in build/validate.mjs: optional permissions, the wasm CSP, and the
// pinned-vendor policy. Each bad package differs from a known-good one by exactly one change.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, cpSync, writeFileSync, readFileSync, rmSync, mkdirSync, appendFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { validateDist, VENDOR_SCRIPT_PINS, ALLOWED_EXTENSION_PAGES_CSP } from "../../build/validate.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const VERSION = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8")).version;

function makeDist(mutate) {
  const dir = mkdtempSync(join(tmpdir(), "sfw-ocrval-"));
  cpSync(join(ROOT, "hosts", "chrome"), dir, { recursive: true });
  cpSync(join(ROOT, "vendor"), join(dir, "vendor"), { recursive: true });
  const m = JSON.parse(readFileSync(join(dir, "manifest.json"), "utf8"));
  m.version = VERSION;
  writeFileSync(join(dir, "manifest.json"), JSON.stringify(m, null, 2));
  writeFileSync(join(dir, "content.js"), "(function(){ 'use strict'; var shielded = 0; return shielded; })();\n".repeat(40));
  writeFileSync(join(dir, "content-main.js"), "(function(){ 'use strict'; })();\n");
  if (mutate) mutate(dir);
  return dir;
}
const withManifest = (dir, f) => { const m = JSON.parse(readFileSync(join(dir, "manifest.json"), "utf8")); f(m); writeFileSync(join(dir, "manifest.json"), JSON.stringify(m, null, 2)); };
function run(mutate) {
  const dir = makeDist(mutate);
  try { return validateDist(dir, { version: VERSION }); } finally { rmSync(dir, { recursive: true, force: true }); }
}
const failsWith = (r, re) => assert.ok(r.errors.some((e) => re.test(e)), `expected ${re}, got ${JSON.stringify(r.errors)}`);

test("the real manifest + vendor tree pass with no errors", () => {
  assert.deepEqual(run().errors, []);
});

test("the only optional permission is offscreen; no host permissions of any kind", () => {
  failsWith(run((d) => withManifest(d, (m) => { delete m.optional_permissions; })), /optional_permissions must be exactly/);
  failsWith(run((d) => withManifest(d, (m) => { m.optional_permissions = ["offscreen", "tabs"]; })), /optional_permissions must be exactly/);
  failsWith(run((d) => withManifest(d, (m) => { m.optional_host_permissions = ["<all_urls>"]; })), /optional_host_permissions/);
  failsWith(run((d) => withManifest(d, (m) => { m.optional_host_permissions = ["https://*/*"]; })), /optional_host_permissions/);
  failsWith(run((d) => withManifest(d, (m) => { m.host_permissions = ["<all_urls>"]; })), /host_permissions/);
});

test("the CSP is exactly the wasm-only extension_pages policy", () => {
  assert.equal(ALLOWED_EXTENSION_PAGES_CSP, "script-src 'self' 'wasm-unsafe-eval'; object-src 'self'");
  failsWith(run((d) => withManifest(d, (m) => { m.content_security_policy = { extension_pages: "script-src 'self' 'wasm-unsafe-eval' 'unsafe-eval'; object-src 'self'" }; })), /content_security_policy/);
  failsWith(run((d) => withManifest(d, (m) => { m.content_security_policy = { extension_pages: "script-src 'self' https://cdn.example; object-src 'self'" }; })), /content_security_policy/);
  failsWith(run((d) => withManifest(d, (m) => { m.content_security_policy = { extension_pages: "script-src 'self' 'wasm-unsafe-eval'; object-src 'self'", sandbox: "script-src 'unsafe-eval'" }; })), /content_security_policy/);
  failsWith(run((d) => withManifest(d, (m) => { delete m.content_security_policy; })), /content_security_policy missing/);
});

test("every vendored script is pinned: a modified or unknown vendored file is refused", () => {
  failsWith(run((d) => appendFileSync(join(d, "vendor", "tesseract", "worker.min.js"), "\n;1+1;\n")), /differs from the reviewed, pinned file/);
  failsWith(run((d) => writeFileSync(join(d, "vendor", "tesseract", "extra.js"), "console.log(1)")), /not pinned/);
  failsWith(run((d) => { mkdirSync(join(d, "vendor", "other")); writeFileSync(join(d, "vendor", "other", "lib.js"), "var x=1"); }), /not pinned/);
});

test("pinned vendored files may keep their documented dead-code idioms, but our own code may not", () => {
  const r = run();
  assert.deepEqual(r.errors, []);
  const worker = readFileSync(join(ROOT, "vendor", "tesseract", "worker.min.js"), "utf8");
  assert.match(worker, /new Function\("return this"\)/, "precondition: the idiom is really present in the vendored worker");
  assert.ok(Object.keys(VENDOR_SCRIPT_PINS).length >= 4);
  failsWith(run((d) => writeFileSync(join(d, "offscreen.js"), "var f = new Function('return 1');\n".repeat(80))), /new Function/);
  failsWith(run((d) => writeFileSync(join(d, "offscreen.js"), "eval('1');\n".repeat(80))), /eval/);
});

test("fetch() is confined to the offscreen scanner and the content script's blob reader", () => {
  failsWith(run((d) => writeFileSync(join(d, "service-worker.js"), readFileSync(join(d, "service-worker.js"), "utf8") + "\nfetch(location.href);\n")), /fetch\(\) is not allowed/);
  failsWith(run((d) => writeFileSync(join(d, "popup.js"), "fetch(u);\n")), /fetch\(\) is not allowed/);
  failsWith(run((d) => writeFileSync(join(d, "offscreen.js"), readFileSync(join(d, "offscreen.js"), "utf8") + "\nfetch(a);fetch(b);\n")), /fetch\(\) is not allowed/);
  failsWith(run((d) => writeFileSync(join(d, "offscreen.js"), readFileSync(join(d, "offscreen.js"), "utf8") + "\nfetch('https://evil.example/x');\n")), /remote fetch/);
});

test("chrome.offscreen / chrome.permissions only where they belong", () => {
  failsWith(run((d) => writeFileSync(join(d, "content.js"), "chrome.offscreen.createDocument({});\n".repeat(40))), /chrome\.offscreen outside/);
  failsWith(run((d) => writeFileSync(join(d, "popup.js"), "chrome.permissions.request({});\n")), /chrome\.permissions outside/);
});

test("vendor/ stays within its size budget and the whole package within the unpacked limit", () => {
  failsWith(run((d) => writeFileSync(join(d, "vendor", "tesseract", "lang", "pad.bin"), Buffer.alloc(6 * 1024 * 1024))), /vendor\/ is/);
});

test("the offscreen page is local-only: inline script or remote script is refused", () => {
  failsWith(run((d) => writeFileSync(join(d, "offscreen.html"), readFileSync(join(d, "offscreen.html"), "utf8").replace("</body>", "<script>1</script></body>"))), /inline <script>/);
  failsWith(run((d) => writeFileSync(join(d, "offscreen.html"), readFileSync(join(d, "offscreen.html"), "utf8").replace("</body>", '<script src="https://cdn.example/t.js"></script></body>'))), /remote/);
});
