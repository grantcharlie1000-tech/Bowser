// The term-update feed: signing, verification, rollback, expiry, size limits, garbage input.
// Runs the SHIPPED client code (hosts/chrome/feed.js + vendored noble-ed25519) against feeds signed by the
// independent node:crypto implementation with the committed TEST key.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { generateKeyPairSync } from "node:crypto";
import { FEED_DIR, loadClient, loadPrivateKey, makeEnvelope, nodeVerifyRaw, productionKeys, publicKeyBase64, readPublicKeys, SFW_ROOT } from "../../feed/tools/lib.mjs";
import { signFeed } from "../../feed/tools/sign.mjs";
import { generate, rawPublicKeyBase64 } from "../../feed/tools/keygen.mjs";

const client = loadClient();
const F = client.SFWFeed;
const testKey = loadPrivateKey(join(FEED_DIR, "test-keys", "test-feed-key.pem"));
const TEST_PUB = publicKeyBase64(testKey);
const LEX = { add: { entityStrong: ["zz sample entity"], cueStrong: ["zz sample cue"] }, raise: { "zz raised term": "cueStrong" } };

function payload(over = {}) {
  return JSON.stringify({ format: 1, seq: 5, issued: "2026-11-17", expires: "2026-12-17", lexicon: LEX, ...over });
}
const sign = (text, key = testKey) => makeEnvelope(text, key);
const verify = (body, keys = [TEST_PUB], opts = {}) => F.verifyEnvelope(body, keys, opts);

test("a correctly signed feed verifies and returns the checked payload", async () => {
  const r = await verify(sign(payload()));
  assert.equal(r.ok, true);
  assert.equal(r.feed.seq, 5);
  assert.equal(r.feed.issued, "2026-11-17");
  assert.deepEqual(JSON.parse(JSON.stringify(r.feed.lexicon)), LEX); // JSON round trip: the client runs in another vm realm
  assert.equal(r.current, false);
});

test("the committed sample feed verifies under the test key and is rejected by the production key", async () => {
  const body = readFileSync(join(FEED_DIR, "sample", "feed.json"), "utf8");
  assert.equal((await verify(body)).ok, true);
  const prod = await verify(body, productionKeys());
  assert.deepEqual([prod.ok, prod.code], [false, "signature"]);
});

test("the client (noble) and node:crypto agree on valid and invalid signatures", async () => {
  const text = payload();
  const env = JSON.parse(sign(text));
  const bytes = Buffer.from(text);
  assert.equal(nodeVerifyRaw(bytes, Buffer.from(env.sig, "base64"), TEST_PUB), true);
  const flipped = Buffer.from(env.sig, "base64");
  flipped[10] ^= 1;
  assert.equal(nodeVerifyRaw(bytes, flipped, TEST_PUB), false);
  assert.equal((await verify(JSON.stringify({ ...env, sig: flipped.toString("base64") }))).code, "signature");
});

test("any change to the payload, the signature or the key is refused", async () => {
  const env = JSON.parse(sign(payload()));
  const bytes = Buffer.from(env.payload, "base64");
  for (let i = 0; i < bytes.length; i += 17) {
    const t = Buffer.from(bytes);
    t[i] ^= 0x01;
    const r = await verify(JSON.stringify({ ...env, payload: t.toString("base64") }));
    assert.equal(r.ok, false, `payload byte ${i}`);
    assert.ok(["signature", "envelope"].includes(r.code));
  }
  const other = publicKeyBase64(loadPrivateKey(generate().privatePem));
  assert.equal((await verify(JSON.stringify(env), [other])).code, "signature");
  assert.equal((await verify(JSON.stringify(env), [])).code, "nokey");
  assert.equal((await verify(JSON.stringify(env), ["not base64!!", "AAAA"])).code, "nokey");
});

test("unsigned, mis-shaped and garbage inputs are ignored without throwing", async () => {
  const env = JSON.parse(sign(payload()));
  const bad = [
    "", "   ", "null", "[]", "42", '"text"', "{}", "<html><body>404: Not Found</body></html>", "\u0000\u0001\u0002", "{".repeat(10000),
    JSON.stringify({ format: 1, payload: env.payload }), // unsigned
    JSON.stringify({ format: 1, payload: env.payload, sig: "" }),
    JSON.stringify({ format: 1, sig: env.sig }),
    JSON.stringify({ ...env, format: 2 }),
    JSON.stringify({ ...env, sig: env.sig.slice(0, -4) }), // short signature
    JSON.stringify({ ...env, sig: " " + env.sig }), // whitespace in base64
    JSON.stringify({ ...env, payload: env.payload + "\n" }),
    JSON.stringify({ ...env, payload: 5 }),
    JSON.stringify(JSON.parse(payload())), // the bare payload, no envelope
  ];
  for (const body of bad) {
    const r = await verify(body);
    assert.equal(r.ok, false, JSON.stringify(body.slice(0, 40)));
    assert.equal(typeof r.code, "string");
  }
  for (const body of [undefined, null, 5, {}, []]) assert.equal((await verify(body)).code, "size");
});

test("a signed payload that is not a valid feed is refused after the signature check", async () => {
  const cases = {
    format: [{ format: 2 }, { format: "1" }],
    seq: [{ seq: 0 }, { seq: -3 }, { seq: 1.5 }, { seq: "7" }, { seq: 2 ** 31 }, { seq: null }, { seq: 1e999 }],
    dates: [{ issued: "yesterday" }, { expires: 12345 }, { issued: "2026-12-30", expires: "2026-12-01" }, { issued: "2026-02-30T99:00:00Z" }, { expires: undefined }],
    lexicon: [
      { lexicon: null }, { lexicon: [] }, { lexicon: "x" },
      { lexicon: { remove: ["spoiler"] } }, { lexicon: { negative: ["gta v"] } }, { lexicon: { safe: ["spoiler"] } },
      { lexicon: { add: { safe: ["release date"] } } }, { lexicon: { add: { negative: ["x y"] } } }, { lexicon: { add: { cueStrong: "oops" } } },
      { lexicon: { add: { cueStrong: [5] } } }, { lexicon: { raise: { "some term": "negative" } } }, { lexicon: { raise: { "some term": 3 } } },
      { lexicon: { version: "has space" } }, { lexicon: { version: "x".repeat(41) } },
      { lexicon: { add: { cueWeak: Array.from({ length: 5001 }, (_, i) => "zz term " + i) } } }, // more terms than the engine accepts
      { lexicon: { add: { cueWeak: Array.from({ length: 3000 }, (_, i) => "zz term " + i) }, raise: Object.fromEntries(Array.from({ length: 2001 }, (_, i) => ["zz raised " + i, "cueStrong"])) } },
    ],
  };
  for (const [code, list] of Object.entries(cases)) {
    for (const over of list) {
      const text = JSON.stringify({ format: 1, seq: 5, issued: "2026-11-17", expires: "2026-12-17", lexicon: LEX, ...over });
      const r = await verify(sign(text));
      assert.equal(r.code, code, `${code}: ${JSON.stringify(over).slice(0, 60)}`);
    }
  }
  assert.equal((await verify(sign("not json at all"))).code, "payload");
  assert.equal((await verify(sign("[1,2,3]"))).code, "payload");
  const bad = Buffer.concat([Buffer.from('{"format":1,"seq":1,"issued":"2026-11-17","expires":"2026-12-17","lexicon":{"version":"'), Buffer.from([0xff, 0xfe]), Buffer.from('"}}')]);
  assert.equal((await verify(makeEnvelope(bad.toString("latin1"), testKey))).ok, false);
});

test("rollback: a lower seq is refused, the same seq is a no-op, a higher seq is accepted", async () => {
  const at = (seq) => sign(payload({ seq }));
  assert.deepEqual(await verify(at(4), [TEST_PUB], { highestSeq: 5 }).then((r) => [r.ok, r.code]), [false, "rollback"]);
  const same = await verify(at(5), [TEST_PUB], { highestSeq: 5 });
  assert.deepEqual([same.ok, same.current], [true, true]);
  const up = await verify(at(6), [TEST_PUB], { highestSeq: 5 });
  assert.deepEqual([up.ok, up.current], [true, false]);
  assert.equal((await verify(at(1), [TEST_PUB], { highestSeq: 0 })).ok, true, "nothing stored yet");
});

test("an expired feed still verifies (it can only add protection); the UI is told it is out of date", async () => {
  const text = payload({ issued: "2026-01-01", expires: "2026-01-31" });
  const r = await verify(sign(text));
  assert.equal(r.ok, true);
  const day = Date.parse("2026-11-17T12:00:00Z");
  assert.equal(F.isExpired(r.feed, day), true);
  assert.equal(F.isExpired(r.feed, Date.parse("2026-01-31T23:59:59Z")), false, "a bare expiry date lasts through that UTC day");
  assert.equal(F.isExpired(r.feed, Date.parse("2026-02-01T00:00:01Z")), true);
  const d = F.describe({ builtin: "2026.11.17-1", enabled: true, feed: r.feed, status: { state: "ok" }, now: day });
  assert.equal(d.text, "Term list: built-in 2026.11.17-1 + update #5 (2026-01-01)");
  assert.deepEqual([...d.notes], ["Term updates out of date."]);
  assert.equal(d.warn, true);
});

test("size: a body over 256 KiB is refused before anything is parsed; a large valid feed is accepted", async () => {
  assert.equal(F.MAX_BYTES, 262144);
  assert.equal((await verify("x".repeat(262145))).code, "size");
  assert.equal((await verify("é".repeat(131073))).code, "size", "counted in UTF-8 bytes");
  const big = { add: { cueWeak: Array.from({ length: 4900 }, (_, i) => `zz term number ${i} filler`) } };
  const text = payload({ lexicon: big });
  assert.ok(text.length > 120000 && text.length < 195000, `payload is ${text.length} bytes`);
  const body = sign(text);
  assert.ok(Buffer.byteLength(body) < F.MAX_BYTES);
  assert.equal((await verify(body)).ok, true);
  const tooBig = sign(payload({ lexicon: { add: { cueWeak: Array.from({ length: 6500 }, (_, i) => `zz term number ${i} filler filler filler`) } } }));
  assert.deepEqual(await verify(tooBig).then((r) => [r.ok, r.code]), [false, "size"], "over 256 KiB once base64-wrapped");
});

test("key rotation: a feed signed by either listed key is accepted", async () => {
  const next = generate();
  const nextKey = loadPrivateKey(next.privatePem);
  const keys = [TEST_PUB, next.publicB64];
  assert.equal((await verify(sign(payload(), nextKey), keys)).ok, true);
  assert.equal((await verify(sign(payload(), testKey), keys)).ok, true);
  assert.equal((await verify(sign(payload(), nextKey), [TEST_PUB])).code, "signature");
});

test("sign tool: refuses a seq that is not higher, a bad lexicon, bad dates; defaults seq to previous + 1", () => {
  const base = { lexicon: LEX, privateKey: testKey, issued: "2026-11-17", expires: "2026-12-17" };
  assert.equal(signFeed({ ...base, prevSeq: 4 }).seq, 5);
  assert.equal(signFeed({ ...base }).seq, 1);
  assert.equal(signFeed({ ...base, seq: 9, prevSeq: 4 }).seq, 9);
  assert.throws(() => signFeed({ ...base, seq: 4, prevSeq: 4 }), /not higher/);
  assert.throws(() => signFeed({ ...base, seq: 3, prevSeq: 4 }), /not higher/);
  assert.throws(() => signFeed({ ...base, seq: 1.5 }), /integer/);
  assert.throws(() => signFeed({ ...base, issued: "nope" }), /schema/);
  assert.throws(() => signFeed({ ...base, expires: "2026-01-01" }), /schema/);
  assert.throws(() => signFeed({ ...base, lexicon: { add: { cueStrong: ["x"] } } }), /engine rejected/); // 1-char term
  assert.throws(() => signFeed({ ...base, lexicon: { add: { cueStrong: ["a\u0001b"] } } }), /engine rejected/);
  assert.throws(() => signFeed({ ...base, lexicon: { raise: { spoiler: "cueWeak" } } }), /engine rejected/); // would be a demotion
  assert.throws(() => signFeed({ ...base, lexicon: { add: { safe: ["release date"] } } }), /schema/);
  assert.throws(() => signFeed({ ...base, lexicon: { add: { cueWeak: Array.from({ length: 6000 }, (_, i) => `zz term ${i} filler`) } } }), /limit|larger|engine|schema/);
  assert.throws(() => signFeed({ ...base, lexicon: "x" }), /object/);
  const ok = signFeed({ ...base, prevSeq: 0 });
  assert.equal(JSON.parse(JSON.parse(ok.envelope).payload && Buffer.from(JSON.parse(ok.envelope).payload, "base64").toString()).lexicon.version, "f1", "version defaults to f<seq>");
  assert.equal(ok.added, 3);
});

test("verify.mjs and sign.mjs work as command line tools", () => {
  const dir = mkdtempSync(join(tmpdir(), "sfw-feed-cli-"));
  try {
    const node = process.execPath;
    const run = (script, args) => spawnSync(node, [join(FEED_DIR, "tools", script), ...args], { encoding: "utf8" });
    const lex = join(dir, "lexicon.json");
    writeFileSync(lex, JSON.stringify(LEX));
    const out = join(dir, "pub", "feed.json");
    const keyPath = join(FEED_DIR, "test-keys", "test-feed-key.pem");
    let r = run("sign.mjs", ["--lexicon", lex, "--key", keyPath, "--out", out, "--issued", "2026-11-17", "--expires-days", "30"]);
    assert.equal(r.status, 0, r.stderr);
    assert.match(r.stdout, /Signed feed #1/);
    assert.match(r.stderr, /NOT listed in feed\/public-key\.txt/);
    r = run("sign.mjs", ["--lexicon", lex, "--key", keyPath, "--out", out, "--issued", "2026-11-18"]);
    assert.equal(r.status, 0, r.stderr);
    assert.match(r.stdout, /Signed feed #2/, "seq bumps from the previously written feed");
    r = run("sign.mjs", ["--lexicon", lex, "--key", keyPath, "--out", out, "--seq", "2"]);
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, /not higher/);
    r = run("verify.mjs", [out, "--key", TEST_PUB]);
    assert.equal(r.status, 0, r.stderr);
    assert.match(r.stdout, /OK: feed #2/);
    r = run("verify.mjs", [out, "--key", TEST_PUB, "--min-seq", "3"]);
    assert.equal(r.status, 1);
    assert.match(r.stderr, /REJECTED \(rollback\)/);
    r = run("verify.mjs", [out]); // production keys
    assert.equal(r.status, 1);
    assert.match(r.stderr, /REJECTED \(signature\)/);
    writeFileSync(join(dir, "garbage.json"), "<html>not a feed</html>");
    assert.equal(run("verify.mjs", [join(dir, "garbage.json")]).status, 1);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("keygen writes the private key only where asked, never in the repo, never over an existing file, and never prints it", () => {
  const dir = mkdtempSync(join(tmpdir(), "sfw-keygen-"));
  try {
    const keygen = join(FEED_DIR, "tools", "keygen.mjs");
    const run = (args) => spawnSync(process.execPath, [keygen, ...args], { encoding: "utf8" });
    const out = join(dir, "nested", "key.pem");
    let r = run(["--out", out]);
    assert.equal(r.status, 0, r.stderr);
    const pem = readFileSync(out, "utf8");
    assert.match(pem, /BEGIN PRIVATE KEY/);
    assert.ok(!r.stdout.includes("PRIVATE KEY") && !r.stdout.includes(pem.split("\n")[1]), "the private key is not printed");
    const pub = publicKeyBase64(loadPrivateKey(pem));
    assert.ok(r.stdout.includes(pub), "the public key is printed");
    assert.match(r.stdout, /WARNING/);
    assert.equal(Buffer.from(pub, "base64").length, 32);
    r = run(["--out", out]);
    assert.equal(r.status, 2);
    assert.match(r.stderr, /refusing to overwrite/);
    r = run(["--out", join(SFW_ROOT, "feed", "oops.pem")]);
    assert.equal(r.status, 2);
    assert.match(r.stderr, /inside the repository/);
    assert.ok(!existsSync(join(SFW_ROOT, "feed", "oops.pem")));
    const direct = generateKeyPairSync("ed25519");
    assert.equal(rawPublicKeyBase64(direct.publicKey), rawPublicKeyBase64(direct.privateKey));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("the production public key file parses to valid 32-byte keys and is not the test key", () => {
  const keys = productionKeys();
  assert.ok(keys.length >= 1);
  for (const k of keys) assert.equal(Buffer.from(k, "base64").length, 32);
  assert.ok(!keys.includes(TEST_PUB), "the throwaway test key must never be a production key");
  assert.deepEqual(readPublicKeys("# c\n\nAAAA\n  BBBB  \r\n"), ["AAAA", "BBBB"]);
});
