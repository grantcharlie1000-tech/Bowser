// Test host (e2e harness only; never shipped). The harness sets globalThis.__SFW_TEST_CONFIG__ before this
// bundle runs: {enabled, mode, reducedEffects, delayConfigMs, noHooks, ocr, ocrMaybe, ocrOff, ocrOnWithConfig}.
// ocr (optional) installs a FAKE host.ocr for the e2e suite: {delayMs, manual, default, rules:[{match, status, text, delayMs}]}.
//   status: ok | unavailable | error | skipped | throw (request() throws) | hang (never settles). `manual: true` leaves every
//   request pending until the test settles it through __sfw.ocr.settle(match, result).
{
  const win = globalThis;
  const tc = win.__SFW_TEST_CONFIG__ || {};
  const reports = Object.create(null);
  let cb = null;
  const config = { enabled: tc.enabled !== false, mode: tc.mode || "strict", reducedEffects: !!tc.reducedEffects, version: "test" };
  let configReady = null;
  if (tc.delayConfigMs) {
    configReady = new Promise(function (res) { setTimeout(function () { if (tc.ocrOnWithConfig) ocrOn = !!tc.ocr; res(config); }, tc.delayConfigMs); });
  }
  let ocrOn = !!tc.ocr && !tc.ocrOff && !tc.ocrOnWithConfig; // ocrOnWithConfig: host.ocr appears together with the delayed config
  const ocrLog = [];
  const ocrPending = [];
  const ocrCfg = tc.ocr || {};
  const fakeOcr = tc.ocr ? {
    youtubeVisual: !!ocrCfg.youtubeVisual,
    request: function (req) {
      const rules = ocrCfg.rules || [];
      let rule = null;
      for (let i = 0; i < rules.length; i++) if (req.url.indexOf(rules[i].match) >= 0) { rule = rules[i]; break; }
      rule = rule || ocrCfg.default || { status: "ok", text: "" };
      ocrLog.push({ url: req.url, kind: req.kind, w: req.width, h: req.height, nw: req.naturalWidth, nh: req.naturalHeight, at: performance.now() });
      if (rule.status === "throw") throw new Error("fake ocr threw");
      return new Promise(function (resolve) {
        const answer = function () {
          if (rule.status === "hang") return;
          resolve(rule.status === "ok" ? { status: "ok", text: rule.text || "" } : { status: rule.status });
        };
        if (ocrCfg.manual) ocrPending.push({ url: req.url, resolve: resolve });
        else setTimeout(answer, rule.delayMs != null ? rule.delayMs : ocrCfg.delayMs || 0);
      });
    },
  } : null;
  const host = {
    kind: "test",
    config: tc.delayConfigMs ? { enabled: true, mode: "strict", reducedEffects: false, version: "test" } : config,
    configReady: configReady,
    get ocr() { return ocrOn ? fakeOcr : null; },
    ocrMaybe: !!tc.ocrMaybe,
    report: function (code, n) { reports[code] = (reports[code] || 0) + (n || 1); },
    onConfigChange: function (f) { cb = f; },
    privileged: null,
    frameIsTop: win.top === win,
  };
  if (!tc.noHooks) SFW.hooks.install(win, { sameWorld: true }); // as the Bowser host
  let ctl = null;
  try { ctl = SFW.runtime.start(host); } catch (e) { reports["error.start.throw"] = 1; }
  Object.defineProperty(win, "__sfw", {
    value: {
      SFW: SFW, host: host, ctl: ctl, reports: reports, setConfig: function (c) { if (cb) cb(c); },
      ocr: {
        log: ocrLog, pending: ocrPending,
        // settle every pending request whose URL contains `match` with `result` (manual mode)
        settle: function (match, result) { let n = 0; for (let i = ocrPending.length - 1; i >= 0; i--) if (ocrPending[i].url.indexOf(match) >= 0) { ocrPending[i].resolve(result); ocrPending.splice(i, 1); n++; } return n; },
        // switch the fake host on/off at runtime (config-change path)
        enable: function (on) { ocrOn = !!on; if (cb) cb(config); },
        setMaybe: function (v) { host.ocrMaybe = !!v; },
      },
    },
    enumerable: false, configurable: true,
  });
}
