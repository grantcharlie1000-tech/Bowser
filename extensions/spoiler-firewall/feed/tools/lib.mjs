// Shared helpers for the feed tools and tests. Node 18+.
//
// loadClient() runs the SHIPPED client code (hosts/chrome/feed.js + feed-config.js on top of the vendored
// noble-ed25519 classic wrapper) inside a vm sandbox, so verify.mjs and the tests check exactly what the
// Chrome extension runs. Signing uses node:crypto, an independent Ed25519 implementation.
import { readFileSync } from "node:fs";
import { createPrivateKey, createPublicKey, sign as nodeSign, verify as nodeVerify } from "node:crypto";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

export const SFW_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
export const FEED_DIR = join(SFW_ROOT, "feed");
export const MAX_ENVELOPE_BYTES = 262144;
export const MAX_LEXICON_JSON = 200000;

export function loadClient() {
  const sandbox = { TextEncoder, TextDecoder, atob, btoa, crypto: globalThis.crypto, console, Promise, URL };
  sandbox.self = sandbox;
  sandbox.globalThis = sandbox;
  vm.createContext(sandbox);
  for (const rel of ["vendor/noble-ed25519/noble-ed25519.js", "hosts/chrome/feed-config.js", "hosts/chrome/feed.js"]) {
    vm.runInContext(readFileSync(join(SFW_ROOT, rel), "utf8"), sandbox, { filename: rel });
  }
  // `var` declarations of classic scripts become properties of the sandbox global.
  return { SFWFeed: sandbox.SFWFeed, SFWFeedConfig: sandbox.SFWFeedConfig, noble: sandbox.nobleEd25519 };
}

// The built engine (dist/node/engine.cjs), a fresh instance per call so extend() calls never leak between uses.
export function loadEngine() {
  const path = join(SFW_ROOT, "dist", "node", "engine.cjs");
  const require = createRequire(import.meta.url);
  delete require.cache[path];
  try {
    return require(path);
  } catch (e) {
    throw new Error(`cannot load ${path}: run "node build/build.mjs" first (${e.message})`);
  }
}

export function readPublicKeys(text) {
  return text.split(/\r?\n/).map((l) => l.trim()).filter((l) => l && !l.startsWith("#"));
}

export function productionKeys() {
  return readPublicKeys(readFileSync(join(FEED_DIR, "public-key.txt"), "utf8"));
}

export function loadPrivateKey(pemOrPath) {
  const pem = /-----BEGIN/.test(pemOrPath) ? pemOrPath : readFileSync(pemOrPath, "utf8");
  return createPrivateKey(pem);
}

export function publicKeyBase64(privateKey) {
  return Buffer.from(createPublicKey(privateKey).export({ format: "jwk" }).x, "base64url").toString("base64");
}

// Signs payload text exactly as given and wraps it in the envelope.
export function makeEnvelope(payloadText, privateKey) {
  const bytes = Buffer.from(payloadText, "utf8");
  const sig = nodeSign(null, bytes, privateKey);
  return JSON.stringify({ format: 1, payload: bytes.toString("base64"), sig: sig.toString("base64") }) + "\n";
}

// Independent check with node:crypto (the client uses noble): true when `sig` verifies `payload` under the raw 32-byte key.
export function nodeVerifyRaw(payload, sig, rawKeyB64) {
  const key = createPublicKey({ key: { kty: "OKP", crv: "Ed25519", x: Buffer.from(rawKeyB64, "base64").toString("base64url") }, format: "jwk" });
  return nodeVerify(null, payload, key, sig);
}

export function todayUtc() {
  return new Date().toISOString().slice(0, 10);
}

export function addDays(day, days) {
  return new Date(Date.parse(day) + days * 86400000).toISOString().slice(0, 10);
}

// What sign.mjs proves about a lexicon object before signing it. Returns {errors, warnings, added}.
// Uses the built engine's own extend(), so "accepted" means "this engine will take every term".
export function checkLexiconWithEngine(lexicon) {
  const errors = [];
  const warnings = [];
  const SFW = loadEngine();
  const res = SFW.lexicon.extend(JSON.parse(JSON.stringify(lexicon)));
  if (!res.ok) errors.push(`engine rejected the lexicon (reason: ${res.reason || "unknown"})`);
  if (res.rejected > 0) errors.push(`engine rejected ${res.rejected} entr${res.rejected === 1 ? "y" : "ies"} (a term must be 2-64 chars without control characters; a raise must target a category above the term's current one in the same family)`);
  if (res.ok && res.added === 0) warnings.push("the engine added nothing: every term is already present at that strength");
  return { errors, warnings, added: res.added, rejected: res.rejected };
}
