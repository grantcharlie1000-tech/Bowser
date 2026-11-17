// Unit tests for build/package.mjs: zip round-trip, determinism, safety, and packaging a dist dir.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, cpSync, writeFileSync, readFileSync, rmSync, readdirSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { buildZip, readZip, crc32, packageChrome } from "../../build/package.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const VERSION = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8")).version;

test("crc32 matches the known check value", () => {
  assert.equal(crc32(Buffer.from("123456789")), 0xcbf43926);
});

test("zip round-trips text, binary, empty and nested files", () => {
  const entries = [
    { name: "manifest.json", data: Buffer.from('{"a":1}\n'.repeat(200)) },
    { name: "icons/icon16.png", data: randomBytes(900) }, // incompressible -> stored
    { name: "empty.txt", data: Buffer.alloc(0) },
    { name: "a/b/c/deep.js", data: Buffer.from("x".repeat(5000)) },
  ];
  const back = readZip(buildZip(entries));
  assert.deepEqual(back.map((e) => e.name), ["a/b/c/deep.js", "empty.txt", "icons/icon16.png", "manifest.json"]);
  for (const e of entries) assert.ok(back.find((b) => b.name === e.name).data.equals(e.data), e.name);
});

test("zip output is deterministic regardless of input order", () => {
  const a = { name: "a.txt", data: Buffer.from("alpha".repeat(50)) };
  const b = { name: "b.txt", data: Buffer.from("beta".repeat(50)) };
  assert.ok(buildZip([a, b]).equals(buildZip([b, a])));
});

test("zip rejects unsafe entry names", () => {
  for (const name of ["../evil.js", "/abs.js", "back\\slash.js", "spa ce.js", ""]) {
    assert.throws(() => buildZip([{ name, data: Buffer.from("x") }]), /unsafe zip entry name/, name);
  }
});

test("readZip detects corruption", () => {
  const zip = buildZip([{ name: "f.txt", data: Buffer.from("hello world ".repeat(40)) }]);
  const bad = Buffer.from(zip);
  bad[40] ^= 0xff; // flip a byte inside the data region
  assert.throws(() => readZip(bad));
});

test("an external unzip tool can read our archive (tar/bsdtar, when available)", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "sfw-zip-"));
  try {
    const entries = [
      { name: "manifest.json", data: Buffer.from('{"ok":true}\n'.repeat(100)) },
      { name: "icons/i.png", data: randomBytes(300) },
    ];
    writeFileSync(join(dir, "t.zip"), buildZip(entries));
    mkdirSync(join(dir, "out"));
    const r = spawnSync(process.platform === "win32" ? join(process.env.SystemRoot || "C:/Windows", "System32", "tar.exe") : "tar", ["-xf", "t.zip", "-C", "out"], { encoding: "utf8", cwd: dir });
    if (r.error || r.status !== 0) { t.skip("tar cannot extract zip here: " + (r.error ? r.error.message : r.stderr)); return; }
    for (const e of entries) assert.ok(readFileSync(join(dir, "out", e.name)).equals(e.data), e.name);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

function makeValidDist() {
  const dir = mkdtempSync(join(tmpdir(), "sfw-pkgdist-"));
  cpSync(join(ROOT, "hosts", "chrome"), dir, { recursive: true });
  cpSync(join(ROOT, "vendor"), join(dir, "vendor"), { recursive: true }); // the OCR engine is part of every real package
  const manifest = JSON.parse(readFileSync(join(dir, "manifest.json"), "utf8"));
  manifest.version = VERSION;
  writeFileSync(join(dir, "manifest.json"), JSON.stringify(manifest, null, 2));
  writeFileSync(join(dir, "content.js"), "(function(){ 'use strict'; })();\n".repeat(60));
  writeFileSync(join(dir, "content-main.js"), "(function(){ 'use strict'; })();\n");
  return dir;
}

test("packageChrome writes a deterministic zip and a matching sha256 file", () => {
  const dist = makeValidDist();
  const out1 = mkdtempSync(join(tmpdir(), "sfw-out-"));
  const out2 = mkdtempSync(join(tmpdir(), "sfw-out-"));
  try {
    const a = packageChrome({ distDir: dist, outDir: out1 });
    const b = packageChrome({ distDir: dist, outDir: out2 });
    assert.equal(a.sha256, b.sha256);
    assert.match(a.zipPath, new RegExp(`spoiler-firewall-chrome-${VERSION.replace(/\./g, "\\.")}\\.zip$`));
    const bytes = readFileSync(a.zipPath);
    assert.equal(createHash("sha256").update(bytes).digest("hex"), a.sha256);
    assert.equal(readFileSync(`${a.zipPath}.sha256`, "utf8").trim().split(/\s+/)[0], a.sha256);
    const names = readZip(bytes).map((e) => e.name);
    assert.ok(names.includes("manifest.json"), "manifest.json must be at the zip root");
    assert.ok(names.every((n) => !n.includes("/") || !n.startsWith("dist")), "no dist/ prefix");
    assert.ok(names.includes("icons/icon128.png"));
  } finally { for (const d of [dist, out1, out2]) rmSync(d, { recursive: true, force: true }); }
});

test("packageChrome refuses an invalid dist", () => {
  const dist = makeValidDist();
  const out = mkdtempSync(join(tmpdir(), "sfw-out-"));
  try {
    const m = JSON.parse(readFileSync(join(dist, "manifest.json"), "utf8"));
    m.permissions = ["storage", "tabs"];
    writeFileSync(join(dist, "manifest.json"), JSON.stringify(m));
    assert.throws(() => packageChrome({ distDir: dist, outDir: out }), /validation failed/);
    assert.equal(existsSync(out) ? readdirSync(out).length : 0, 0);
  } finally { for (const d of [dist, out]) rmSync(d, { recursive: true, force: true }); }
});
