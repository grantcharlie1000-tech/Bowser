// Unit test of the Bowser host's native OCR bridge (hosts/bowser/bowser-host.js). It runs the host source in a
// bare vm context with a stub engine, no browser and no window: `node --test extensions/spoiler-firewall/test/bowser/ocr-host.test.mjs`.
import { test } from "node:test";
import assert from "node:assert/strict";
import vm from "node:vm";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HOST = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "..", "..", "hosts", "bowser", "bowser-host.js"), "utf8");
const HOOK = "__sfw_" + "0123456789abcdef".repeat(2);
const KEY = "fedcba9876543210".repeat(2);

function boot(config) {
  const timers = [];
  const started = [];
  const sandbox = {
    SFW: { hooks: { install() {} }, runtime: { start(host) { started.push(host); } } },
    URL,
    Promise,
    JSON,
    Map,
    Symbol,
    Object,
    Array,
    console: { debug() {} },
    location: { href: "https://x.com/home" },
  };
  sandbox.setTimeout = (fn, ms) => { timers.push({ fn, ms }); return timers.length; };
  sandbox.clearTimeout = (id) => { if (timers[id - 1]) timers[id - 1].fn = null; };
  const context = vm.createContext(sandbox);
  // The host reads `globalThis` and the substituted config constant.
  const source = HOST.replace("/*__SFW_CONFIG__*/null", JSON.stringify(config));
  vm.runInContext("globalThis.top = globalThis;\n" + source, context);
  return { context, timers, host: started[0], win: context };
}

const ON = { enabled: true, mode: "strict", ocr: true, ocrHook: HOOK, ocrKey: KEY };

test("ocr is null unless the config enables it with well-formed credentials", () => {
  assert.equal(boot({ enabled: true, mode: "strict", ocr: false }).host.ocr, null);
  assert.equal(boot({ enabled: true, mode: "strict" }).host.ocr, null);
  assert.equal(boot({ ...ON, ocrHook: "__sfw_nothex" }).host.ocr, null);
  assert.equal(boot({ ...ON, ocrKey: "short" }).host.ocr, null);
  assert.equal(boot({ ...ON, ocrHook: "evil\"];x" }).host.ocr, null);
  assert.equal(typeof boot(ON).host.ocr.request, "function");
});

test("the hook is one non-enumerable, non-writable, non-configurable frozen property", () => {
  const { win } = boot(ON);
  const descriptor = vm.runInContext(`JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, ${JSON.stringify(HOOK)}), ['enumerable','writable','configurable'])`, win);
  assert.equal(descriptor, '{"enumerable":false,"writable":false,"configurable":false}');
  assert.equal(vm.runInContext(`Object.isFrozen(globalThis[${JSON.stringify(HOOK)}])`, win), true);
  assert.equal(vm.runInContext(`Object.keys(globalThis).includes(${JSON.stringify(HOOK)})`, win), false);
  assert.equal(vm.runInContext(`Object.keys(globalThis[${JSON.stringify(HOOK)}]).sort().join()`, win), "deliver,drain,feed");
  // a page cannot redefine it
  assert.throws(() => vm.runInContext(`Object.defineProperty(globalThis, ${JSON.stringify(HOOK)}, { value: 1 })`, win), TypeError);
});

test("requests are enqueued, drained once, and settled by deliver", async () => {
  const { host, win } = boot(ON);
  const a = host.ocr.request({ url: "https://pbs.twimg.com/media/a.jpg#frag" });
  const b = host.ocr.request({ url: "/relative.png" });
  const c = host.ocr.request({ url: "https://i.ytimg.com/vi/id/hqdefault.jpg" });
  assert.equal(host.ocr.youtubeVisual, true);
  const call = (expr) => vm.runInContext(`globalThis[${JSON.stringify(HOOK)}].${expr}`, win);
  const drained = JSON.parse(call(`drain(${JSON.stringify(KEY)})`));
  assert.deepEqual(drained.map((d) => d.url), ["https://pbs.twimg.com/media/a.jpg", "https://x.com/relative.png", "https://i.ytimg.com/vi/id/hqdefault.jpg"]);
  assert.deepEqual(JSON.parse(call(`drain(${JSON.stringify(KEY)})`)), [], "handed out once");
  const results = [
    { id: drained[0].id, status: "ok", text: "hello" },
    { id: drained[1].id, status: "unavailable" },
    { id: drained[2].id, status: "visual" },
  ];
  call(`deliver(${JSON.stringify(KEY)}, ${JSON.stringify(JSON.stringify(results))})`);
  assert.deepEqual(JSON.parse(JSON.stringify(await a)), { status: "ok", text: "hello" });
  assert.deepEqual(JSON.parse(JSON.stringify(await b)), { status: "unavailable" });
  assert.deepEqual(JSON.parse(JSON.stringify(await c)), { status: "visual" });
});

test("non-http(s) and malformed requests settle unavailable at once and are never queued", async () => {
  const { host, win } = boot(ON);
  for (const url of ["data:image/png;base64,AAAA", "blob:https://x.com/1", "javascript:alert(1)", "file:///C:/a.png", "", 5, null, undefined, "https://" + "a".repeat(3000) + ".com/"]) {
    assert.deepEqual(JSON.parse(JSON.stringify(await host.ocr.request({ url }))), { status: "unavailable" }, String(url).slice(0, 30));
  }
  assert.deepEqual(JSON.parse(JSON.stringify(await host.ocr.request(null))), { status: "unavailable" });
  assert.equal(vm.runInContext(`globalThis[${JSON.stringify(HOOK)}].drain(${JSON.stringify(KEY)})`, win), "[]");
});

test("the wrong key neither drains nor settles", async () => {
  const { host, win } = boot(ON);
  const p = host.ocr.request({ url: "https://i.redd.it/a.png" });
  const call = (expr) => vm.runInContext(`globalThis[${JSON.stringify(HOOK)}].${expr}`, win);
  assert.equal(call(`drain("nope")`), null);
  const id = JSON.parse(call(`drain(${JSON.stringify(KEY)})`))[0].id;
  call(`deliver("nope", ${JSON.stringify(JSON.stringify([{ id, status: "ok", text: "forged" }]))})`);
  let settled = false;
  p.then(() => { settled = true; });
  await new Promise((r) => setImmediate(r));
  assert.equal(settled, false);
});

test("hostile deliver payloads are ignored or settle error, never throw", async () => {
  const { host, win } = boot(ON);
  const call = (expr) => vm.runInContext(`globalThis[${JSON.stringify(HOOK)}].${expr}`, win);
  const ask = () => host.ocr.request({ url: "https://www.youtube.com/watch?v=1" });
  const idOf = () => JSON.parse(call(`drain(${JSON.stringify(KEY)})`))[0].id;
  const send = (payload) => call(`deliver(${JSON.stringify(KEY)}, ${JSON.stringify(typeof payload === "string" ? payload : JSON.stringify(payload))})`);
  for (const junk of ["", "not json", "{}", "null", "42", "[null]", "[1,2]", `[{"id":5}]`, `[{"id":"nope","status":"ok","text":"x"}]`, "[".repeat(10000), "x".repeat(300000)]) {
    assert.doesNotThrow(() => send(junk));
  }
  let p = ask(); let id = idOf();
  send([{ id, status: "weird" }]);
  assert.deepEqual(JSON.parse(JSON.stringify(await p)), { status: "error" });
  p = ask(); id = idOf();
  send([{ id, status: "ok", text: 42 }]);
  assert.deepEqual(JSON.parse(JSON.stringify(await p)), { status: "error" });
  p = ask(); id = idOf();
  send([{ id, status: "ok", text: "x".repeat(4097) }]);
  assert.deepEqual(JSON.parse(JSON.stringify(await p)), { status: "error" });
  p = ask(); id = idOf();
  send([{ id, status: "skipped", text: "ignored" }]);
  assert.deepEqual(JSON.parse(JSON.stringify(await p)), { status: "skipped" });
  // settling twice is harmless
  assert.doesNotThrow(() => send([{ id, status: "ok", text: "again" }]));
});

test("an unanswered request settles error after 30 s", async () => {
  const { host, timers } = boot(ON);
  const p = host.ocr.request({ url: "https://www.twitch.tv/x" });
  const timer = timers.find((t) => t.ms === 30000);
  assert.ok(timer, "a 30 s timer exists");
  timer.fn();
  assert.deepEqual(JSON.parse(JSON.stringify(await p)), { status: "error" });
});

test("pending is capped at 256 and a drain returns at most 64", async () => {
  const { host, win } = boot(ON);
  const calls = [];
  for (let i = 0; i < 300; i++) calls.push(host.ocr.request({ url: `https://i.redd.it/${i}.png` }));
  const overflow = await Promise.all(calls.slice(256));
  assert.ok(overflow.every((r) => r.status === "unavailable"), "overflow answers unavailable");
  const first = JSON.parse(vm.runInContext(`globalThis[${JSON.stringify(HOOK)}].drain(${JSON.stringify(KEY)})`, win));
  assert.equal(first.length, 64);
  const urls = first.map((i) => i.url);
  assert.ok(urls.every((u) => u.length <= 2048));
});

test("the host starts the runtime once and passes ocr through the contract", () => {
  const { host } = boot(ON);
  assert.equal(host.kind, "bowser");
  assert.equal(host.configReady, null);
  assert.equal(host.config.ocrHook, HOOK, "the runtime sees the config object");
});
