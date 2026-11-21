// Shared harness pieces: browser launch (headless-shell only, muted, below-normal priority), the in-page DOM
// invariant probe + audit, and the beginFrame frame recorder.
import { chromium } from "playwright";
import { HEADLESS_SHELL, CHROMIUM, BASE_ARGS } from "./env.mjs";
import { testBundle } from "./bundle.mjs";
import { decodePng, countSentinel } from "./png.mjs";

export const BF_ARGS = ["--enable-begin-frame-control", "--run-all-compositor-stages-before-draw", "--disable-new-content-rendering-timeout", "--disable-threaded-animation", "--disable-threaded-scrolling", "--disable-checker-imaging", "--disable-image-animation-resync"];

export async function launch({ beginFrame = false } = {}) {
  return chromium.launch({ headless: true, executablePath: process.env.BOWSER_TEST_EDGE || HEADLESS_SHELL || CHROMIUM, args: beginFrame ? [...BASE_ARGS, ...BF_ARGS] : BASE_ARGS });
}

// In-page script, injected AFTER the engine bundle: per-frame DOM invariant probe + audit helper.
export const PROBE = `(() => {
  const P = (window.__probe = { frames: 0, unsafe: 0, leak: 0, examples: [], maxSeen: 0, seenFirstAt: null, shieldedFirstAt: null });
  const SEL = '.sentinel,.sbox,img[src*="magenta"],img[src*="gta6-ending"],canvas.sentinel';
  const hooks = () => { try { return window.__sfw && window.__sfw.SFW.hooks; } catch (e) { return null; } };
  const rootOf = (e) => { try { const h = hooks(); return (h && h.rootOf(e)) || e.shadowRoot || null; } catch (e2) { return null; } };
  const shielded = (n) => {
    for (let x = n, i = 0; x && i < 120; i++) {
      if (x.nodeType === 1) {
        const v = x.getAttribute('data-sfw');
        if (v && v.split(' ').indexOf('shield') >= 0) return true;
        if (x === document.documentElement && (x.getAttribute('data-sfw-page') || '').indexOf('shield') >= 0) return true;
      }
      const p = x.parentNode; x = p && p.nodeType === 11 ? p.host : p;
    }
    return false;
  };
  const rendered = (e) => {
    const cs = getComputedStyle(e);
    if (cs.display === 'none' || cs.visibility !== 'visible' || +cs.opacity === 0) return false;
    const r = e.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  };
  function visit(root, out) {
    root.querySelectorAll(SEL).forEach((e) => out.push(e));
    root.querySelectorAll('*').forEach((e) => { const r = rootOf(e); if (r) visit(r, out); });
  }
  function scan() {
    const list = []; visit(document, list);
    let unsafe = 0, leak = 0;
    for (const e of list) {
      if (!e.isConnected) continue;
      if (!rendered(e)) continue;
      if (shielded(e)) { leak++; } else { unsafe++; }
      if ((unsafe + leak) && P.examples.length < 4) P.examples.push({ frame: P.frames, shielded: shielded(e), cls: e.className, text: (e.textContent || '').slice(0, 16) });
    }
    return { total: list.length, unsafe, leak };
  }
  function tick() {
    P.frames++;
    try {
      const s = scan();
      if (s.total > P.maxSeen) P.maxSeen = s.total;
      if (s.total && P.seenFirstAt == null) P.seenFirstAt = performance.now();
      if (s.unsafe) P.unsafe++;
      if (s.leak) P.leak++;
    } catch (e) { P.error = String(e && e.message); }
    requestAnimationFrame(tick);
  }
  requestAnimationFrame(tick);
  // Task-boundary probe: a frame can only be produced between tasks, so scanning at EVERY task boundary is
  // strictly denser than scanning per rAF. A MessageChannel ping-pong yields one task per iteration.
  P.taskScans = 0; P.taskUnsafe = 0; P.taskLeak = 0;
  const mc = new MessageChannel();
  mc.port1.onmessage = () => {
    if (P.stopTasks) return;
    P.taskScans++;
    try { const s = scan(); if (s.unsafe) P.taskUnsafe++; if (s.leak) P.taskLeak++; } catch (e) { P.error = String(e && e.message); }
    setTimeout(() => mc.port2.postMessage(0), 0);
  };
  mc.port2.postMessage(0);

  // audit(): expectations declared in the fixture markup (data-expect="shield"|"clear")
  window.__audit = () => {
    const out = { shieldExpected: 0, shieldedOk: 0, missed: [], clearExpected: 0, falseShield: 0, falseList: [], evasiveMissed: 0, evasiveTotal: 0 };
    const roots = [document];
    const stack = [document];
    while (stack.length) { const r = stack.pop(); r.querySelectorAll('*').forEach((e) => { const s = rootOf(e); if (s) { roots.push(s); stack.push(s); } }); }
    const inShield = (e) => shielded(e);
    const SH = '[data-sfw~="shield"]';
    const hasInner = (e) => { if (e.querySelector(SH)) return true; const r0 = rootOf(e); if (r0 && r0.querySelector(SH)) return true; for (const k of e.querySelectorAll('*')) { const r = rootOf(k); if (r && (r.querySelector(SH) || r.host.matches(SH))) return true; } return false; };
    for (const r of roots) {
      r.querySelectorAll('[data-expect]').forEach((e) => {
        if (!e.isConnected) return;
        const want = e.getAttribute('data-expect');
        const evasive = e.getAttribute('data-detect') === 'evasive';
        const got = inShield(e) || hasInner(e);
        if (want === 'shield') {
          if (evasive) { out.evasiveTotal++; if (!got) out.evasiveMissed++; return; }
          out.shieldExpected++;
          if (got) out.shieldedOk++; else out.missed.push((e.id || e.tagName) + ':' + (e.textContent || '').trim().slice(0, 20));
        } else if (want === 'clear') {
          out.clearExpected++;
          if (got) { out.falseShield++; out.falseList.push((e.id || e.tagName) + ':' + (e.textContent || '').trim().slice(0, 20)); }
        }
      });
    }
    return out;
  };
})();`;

export async function instrumentedContext(browser, { config = {}, detection = "real", probe = true, viewport = { width: 640, height: 420 } } = {}) {
  const ctx = await browser.newContext({ viewport, deviceScaleFactor: 1 });
  await ctx.addInitScript({ content: testBundle(config, detection) });
  if (probe) await ctx.addInitScript({ content: PROBE });
  return ctx;
}

// Frame-exact recorder on top of HeadlessExperimental.beginFrame. Every frame with damage is screenshotted and
// decoded; sentinel-coloured pixels are counted. A frame is "unsafe" when it contains >= minPixels sentinel pixels.
export class FrameRecorder {
  constructor(ctx, page, { minPixels = 3, keepUnsafe = true } = {}) {
    this.ctx = ctx; this.page = page; this.minPixels = minPixels; this.keepUnsafe = keepUnsafe;
    this.frames = 0; this.shots = 0; this.unsafe = 0; this.maxPixels = 0; this.firstUnsafe = null; this.errors = 0;
    this.running = false; this.sentinelFrames = [];
  }
  async start() {
    this.cdp = await this.ctx.newCDPSession(this.page);
    this.running = true;
    this.loop = (async () => {
      while (this.running) {
        try {
          const r = await this.cdp.send("HeadlessExperimental.beginFrame", { interval: 16.666, screenshot: { format: "png" } });
          this.frames++;
          if (r.screenshotData) {
            this.shots++;
            const n = countSentinel(decodePng(Buffer.from(r.screenshotData, "base64")));
            if (n > this.maxPixels) this.maxPixels = n;
            if (n >= this.minPixels) {
              this.unsafe++;
              if (!this.firstUnsafe && this.keepUnsafe) this.firstUnsafe = { frame: this.frames, pixels: n, png: r.screenshotData };
            }
          }
        } catch (e) {
          this.errors++;
          await new Promise((res) => setTimeout(res, 30));
        }
      }
    })();
  }
  async waitFrames(n, timeoutMs = 4000) {
    const target = this.frames + n;
    const t0 = Date.now();
    while (this.frames < target && Date.now() - t0 < timeoutMs) await new Promise((r) => setTimeout(r, 20));
  }
  async stop() {
    this.running = false;
    await this.loop;
  }
}

export function pct(arr, p) {
  if (!arr.length) return 0;
  const s = arr.slice().sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.floor(s.length * p))];
}
