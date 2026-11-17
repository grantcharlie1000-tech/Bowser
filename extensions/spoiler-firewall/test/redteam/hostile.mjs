// Hostile main world: the REAL Bowser bundle (dist/bowser/spoiler-firewall.js, config substituted like Bowser does) loaded into
// pages whose builtins are patched the way core-js / Prototype.js / MooTools / zone.js / SES-style lockdown do.
//   mode "post": the patch runs as the first page script (after the init-script bundle) -- the realistic Bowser order.
//   mode "pre" : the patch runs BEFORE the bundle -- models an earlier-injected polyfill or a patched frame.
// Expectation: protection still works, or fails CLOSED on hot pages; never throws into the page; never breaks the page.
// A "cold" ordinary page must stay clean (no shields, no page shield, no errors) under every patch.
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { launch, FrameRecorder, PROBE } from "../e2e/lib.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const S = "GTA 6 ending spoiler: the hero dies in the final mission";

export function bowserBundle(config = { enabled: true, mode: "strict", reducedEffects: false, version: "redteam" }) {
  const src = readFileSync(join(HERE, "..", "..", "dist", "bowser", "spoiler-firewall.js"), "utf8");
  if (src.split("/*__SFW_CONFIG__*/null").length !== 2) throw new Error("bowser bundle placeholder not found exactly once");
  return src.replace("/*__SFW_CONFIG__*/null", JSON.stringify(config));
}

const WRAP_ALL = (obj, names) => names.map((n) => `(function(){var o=${obj}.${n};if(typeof o==='function'){${obj}.${n}=function(){return o.apply(this,arguments)}}})();`).join("");
export const SCENARIOS = [
  { id: "baseline", patch: "" },
  { id: "array-includes-find-broken", patch: `Array.prototype.includes=function(){return false};Array.prototype.find=function(){return undefined};Array.prototype.indexOf=function(){return -1};Array.prototype.findIndex=function(){return -1};Array.prototype.some=function(){return false}` },
  { id: "array-benign-wrappers", patch: WRAP_ALL("Array.prototype", ["map", "filter", "forEach", "slice", "push", "join", "some", "every", "concat", "indexOf", "includes", "find", "sort", "splice", "reduce"]) },
  { id: "prototypejs-enumerable-pollution", patch: `Array.prototype.each=function(){};Array.prototype.first=function(){return this[0]};Array.prototype.last=function(){return this[this.length-1]};Object.prototype.extend=function(){return this};Object.prototype.toQueryString=function(){return ''};String.prototype.strip=function(){return this.trim()};Object.prototype.polluted='x';Object.prototype.length=3;Object.prototype.hot=2;Object.prototype.flags={franchise:true}` },
  { id: "string-normalize-identity", patch: `String.prototype.normalize=function(){return String(this)}` },
  { id: "string-normalize-throws", patch: `String.prototype.normalize=function(){throw new RangeError('The normalization form should be one of NFC, NFD, NFKC, NFKD.')}` },
  { id: "string-codepoint-missing", patch: `String.prototype.codePointAt=undefined;String.fromCodePoint=undefined;String.prototype.at=undefined;String.prototype.padStart=undefined;String.prototype.matchAll=undefined;String.prototype.replaceAll=undefined` },
  { id: "string-benign-wrappers", patch: WRAP_ALL("String.prototype", ["replace", "split", "slice", "indexOf", "toLowerCase", "charCodeAt", "trim", "match", "search", "substring", "includes", "startsWith", "endsWith", "codePointAt"]) + WRAP_ALL("RegExp.prototype", ["exec", "test"]) },
  { id: "regexp-test-broken", sabotage: true, patch: `RegExp.prototype.test=function(){return false}` },
  { id: "regexp-exec-null", sabotage: true, patch: `var __e=RegExp.prototype.exec;RegExp.prototype.exec=function(){return null}` },
  { id: "attachshadow-wrapped", patch: `var __a=Element.prototype.attachShadow;Element.prototype.attachShadow=function(o){return __a.call(this,o)}` },
  { id: "attachshadow-forces-open", patch: `var __b=Element.prototype.attachShadow;Element.prototype.attachShadow=function(o){return __b.call(this,Object.assign({},o,{mode:'open'}))}` },
  { id: "mutationobserver-dead", patch: `window.MutationObserver=function(){this.observe=function(){};this.disconnect=function(){};this.takeRecords=function(){return[]}}` },
  { id: "mutationobserver-throws", patch: `window.MutationObserver=function(){throw new Error('MO blocked')}` },
  { id: "dom-mutators-wrapped", patch: `['appendChild','insertBefore','removeChild','replaceChild','cloneNode','contains','getRootNode'].forEach(function(n){var o=Node.prototype[n];Node.prototype[n]=function(){return o.apply(this,arguments)}});['append','prepend','remove','setAttribute','removeAttribute','getAttribute','hasAttribute','insertAdjacentHTML','querySelector','querySelectorAll','closest','matches','getBoundingClientRect'].forEach(function(n){var o=Element.prototype[n];if(o)Element.prototype[n]=function(){return o.apply(this,arguments)}});var __ce=Document.prototype.createElement;Document.prototype.createElement=function(){return __ce.apply(this,arguments)}` },
  { id: "dom-queries-broken", sabotage: true, patch: `Element.prototype.closest=function(){return null};Element.prototype.matches=function(){return false};Element.prototype.querySelectorAll=function(){return []};Document.prototype.querySelectorAll=function(){return []}` },
  { id: "gbcr-getcomputedstyle-broken", sabotage: true, patch: `Element.prototype.getBoundingClientRect=function(){return {x:0,y:0,width:0,height:0,top:0,left:0,right:0,bottom:0}};window.getComputedStyle=function(){return {getPropertyValue:function(){return ''},display:'',position:'',visibility:'',opacity:''}}` },
  { id: "window-sfw-predefined", patch: `window.SFW={classify:{text:function(){return {score:0,level:'none',flags:{},signals:[]}}},policy:{decide:function(){return 'clear'}}};window.__sfw={};window.sfw={};Object.defineProperty(window,'__SFW_TEST_CONFIG__',{value:{enabled:false}})` },
  { id: "sfw-guard-symbol-preset", sabotage: true, patch: `try{Object.defineProperty(window,Symbol.for('sfw.bowser.guard'),{value:true})}catch(e){}` },
  { id: "raf-never-fires", patch: `window.requestAnimationFrame=function(){return 0};window.cancelAnimationFrame=function(){}` },
  { id: "raf-settimeout-polyfill", patch: `window.requestAnimationFrame=function(cb){return setTimeout(function(){cb(performance.now())},16)};` },
  { id: "timers-performance-stubbed", sabotage: true, patch: `window.setTimeout=function(f,t){return 0};window.setInterval=function(){return 0};performance.now=function(){return 0};Date.now=function(){return 0}` },
  { id: "cssstylesheet-missing", patch: `window.CSSStyleSheet=undefined;try{delete Document.prototype.adoptedStyleSheets;delete ShadowRoot.prototype.adoptedStyleSheets}catch(e){}` },
  { id: "customelements-weakmap-broken", sabotage: true, patch: `window.customElements=undefined;window.WeakMap=function(){throw new Error('WeakMap polyfill broke')};window.WeakSet=undefined` },
  { id: "map-set-promise-polyfilled", sabotage: true, patch: `(function(){var M=window.Map,S=window.Set;window.Map=function(){return new M()};window.Map.prototype=M.prototype;window.Set=function(){return new S()};window.Set.prototype=S.prototype;var P=window.Promise;window.Promise=function(f){return new P(f)};window.Promise.prototype=P.prototype;window.Promise.resolve=P.resolve.bind(P)})()` },
  { id: "function-bind-call-apply-wrapped", patch: `(function(){var R=Reflect.apply,sl=Array.prototype.slice,b=Function.prototype.bind;Function.prototype.bind=function(){return R(b,this,arguments)};Function.prototype.call=function(t){return R(this,t,R(sl,arguments,[1]))};Function.prototype.apply=function(t,a){return R(this,t,a||[])}})()` },
  { id: "object-defineproperty-throws", sabotage: true, patch: `Object.defineProperty=function(){throw new Error('defineProperty blocked')};Object.defineProperties=function(){throw new Error('x')}` },
  { id: "object-keys-assign-broken", sabotage: true, patch: `Object.keys=function(){return []};Object.assign=function(t){return t};Object.entries=function(){return []};Object.freeze=function(o){return o}` },
  { id: "json-broken", sabotage: true, patch: `JSON.stringify=function(){return 'null'};JSON.parse=function(){return null}` },
  { id: "ses-frozen-intrinsics", patch: `Object.freeze(Object.prototype);Object.freeze(Array.prototype);Object.freeze(String.prototype);Object.freeze(Function.prototype);Object.freeze(Node.prototype);Object.freeze(Element.prototype)` },
  { id: "zonejs-like-wrap-everything", patch: `(function(){['setTimeout','setInterval','requestAnimationFrame','queueMicrotask'].forEach(function(n){var o=window[n];window[n]=function(f){var a=[].slice.call(arguments);if(typeof f==='function'){var g=f;a[0]=function(){return g.apply(this,arguments)}}return o.apply(window,a)}});var ael=EventTarget.prototype.addEventListener;EventTarget.prototype.addEventListener=function(t,l,o){var l2=typeof l==='function'?function(){return l.apply(this,arguments)}:l;return ael.call(this,t,l2,o)};var MO=window.MutationObserver;window.MutationObserver=function(cb){return new MO(function(){return cb.apply(this,arguments)})};window.MutationObserver.prototype=MO.prototype;var then=Promise.prototype.then;Promise.prototype.then=function(a,b){return then.call(this,a,b)}})()` },
  { id: "symbol-iterator-broken", sabotage: true, patch: `Array.prototype[Symbol.iterator]=function(){return {next:function(){return {done:true}}}};Set.prototype[Symbol.iterator]=Array.prototype[Symbol.iterator];Map.prototype[Symbol.iterator]=Array.prototype[Symbol.iterator]` },
];

const PAGE = (patch, kind) => `<!doctype html><html><head><meta charset="utf-8"><title>${kind}</title><style>body{font:16px Arial;margin:8px}article{border:1px solid #ccc;margin:6px;padding:8px;display:block}.sentinel{color:#ff00ff!important;font:700 26px Arial!important;-webkit-text-fill-color:#ff00ff}</style><script>${patch}</script></head><body>
<article id="c1">Lovely weather today, going for a long hike with the dog.</article>
${kind === "hot" ? `<article id="s1" class="spoilerunit"><span class="sentinel">${S}</span></article>` : ""}
<article id="c2">Five pasta tips that will change your dinner.</article>
<div id="host"></div>
<script>
window.__pageOk=false;window.__errs=[];window.addEventListener('error',function(e){window.__errs.push(String(e.message))});window.addEventListener('unhandledrejection',function(e){window.__errs.push('rej:'+String(e.reason))});
try{setTimeout(function(){var a=document.createElement('article');a.id='dyn';${kind === "hot" ? `a.className='spoilerunit';a.innerHTML='<span class="sentinel">GTA VI leaked ending: everyone dies at the finale</span>'` : `a.textContent='A later, ordinary, dynamic post about gardening.'`};document.getElementById('host').appendChild(a)},250)}catch(e){}
try{document.getElementById('c1').addEventListener('click',function(){window.__clicked=1})}catch(e){}
window.__pageOk=true;
</script></body></html>`;

export async function runHostile({ log = console.log, only = null } = {}) {
  let cur = { patch: "", kind: "hot" };
  const srv = createServer((req, res) => {
    const u = new URL(req.url, "http://x");
    res.setHeader("Content-Type", "text/html; charset=utf-8");
    const kind = u.searchParams.get("kind") || "hot";
    const sc = SCENARIOS.find((s) => s.id === u.searchParams.get("sc")) || SCENARIOS[0];
    res.end(PAGE(u.searchParams.get("mode") === "post" ? sc.patch : "", kind));
  });
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const port = srv.address().port;
  const browser = await launch({ beginFrame: true });
  const bundle = bowserBundle();
  const rows = [];
  const controls = new Map();
  const list = SCENARIOS.filter((s) => !only || only.includes(s.id));
  for (const sc of list) {
    for (const mode of ["post", "pre"]) {
      for (const kind of ["hot", "cold"]) {
        const ckey = sc.id + "|" + kind; // control (no engine) is mode-independent: the patch behaves the same either way
        if (!controls.has(ckey)) {
          const cctx = await browser.newContext({ viewport: { width: 640, height: 420 } });
          const cp = await cctx.newPage();
          const cerrs = [];
          cp.on("pageerror", (e) => cerrs.push(e.message));
          try { await cp.goto(`http://example.test:${port}/p?kind=${kind}&sc=${sc.id}&mode=post`, { waitUntil: "load", timeout: 20000 }); await sleep(500); } catch (e) { /* ignore */ }
          let ok = null; try { ok = await cp.evaluate(() => ({ pageOk: window.__pageOk, errs: (window.__errs || []).length })); } catch (e) { ok = { pageOk: false, errs: 0 }; }
          controls.set(ckey, { pageOk: !!(ok && ok.pageOk), errs: (ok ? ok.errs : 0) + cerrs.length });
          await cctx.close();
        }
        const ctl = controls.get(ckey);
        const ctx = await browser.newContext({ viewport: { width: 640, height: 420 }, deviceScaleFactor: 1 });
        if (mode === "pre" && sc.patch) await ctx.addInitScript({ content: sc.patch });
        await ctx.addInitScript({ content: bundle });
        await ctx.addInitScript({ content: PROBE });
        const page = await ctx.newPage();
        const errs = [];
        page.on("pageerror", (e) => errs.push(e.message));
        await page.goto("about:blank");
        const rec = new FrameRecorder(ctx, page);
        await rec.start();
        let navErr = null;
        try { await page.goto(`http://example.test:${port}/p?kind=${kind}&sc=${sc.id}&mode=${mode}`, { waitUntil: "load", timeout: 20000 }); } catch (e) { navErr = e.message; }
        await sleep(900);
        await rec.waitFrames(15, 2500);
        let info = null;
        try {
          info = await page.evaluate(() => {
            const sh = (e) => !!(e && (e.closest("[data-sfw~=shield]") || e.querySelector("[data-sfw~=shield]") || document.documentElement.getAttribute("data-sfw-page") === "shield"));
            const p = window.__probe || {};
            return { pageOk: window.__pageOk, clicked: (document.getElementById("c1").click(), window.__clicked === 1), pageErrs: window.__errs, shieldCount: document.querySelectorAll("[data-sfw~=shield]").length, pageShield: document.documentElement.getAttribute("data-sfw-page"), spoilerShielded: [...document.querySelectorAll(".spoilerunit")].map(sh), cleanShielded: ["c1", "c2"].filter((id) => document.getElementById(id).closest("[data-sfw~=shield]")).length, probeUnsafe: (p.unsafe || 0) + (p.taskUnsafe || 0), probeLeak: (p.leak || 0) + (p.taskLeak || 0), dyn: !!document.getElementById("dyn"), hasEmergency: document.documentElement.hasAttribute("data-sfw-emergency") };
          });
        } catch (e) { info = { evalError: String(e.message).split("\n")[0] }; }
        await rec.stop();
        await ctx.close();
        const problems = [];
        let severity = null;
        if (navErr) problems.push("navigation error");
        if (!info || info.evalError) problems.push("page evaluate failed: " + (info && info.evalError));
        else {
          if (!info.pageOk && ctl.pageOk) { problems.push("page script did not complete (engine broke the page)"); severity = "high"; }
          if ((info.pageErrs || []).length + errs.length > ctl.errs) { problems.push("errors surfaced in the page: " + [...(info.pageErrs || []), ...errs].slice(0, 2).join(" | ")); severity = severity || "high"; }
          if (kind === "hot") {
            // pixels are ground truth; a DOM-probe violation with zero sentinel pixels is reported as inconclusive (medium), not critical
            if (rec.unsafe > 0) { problems.push(`UNSHIELDED spoiler pixels: px frames=${rec.unsafe}, dom=${info.probeUnsafe}/${info.probeLeak}`); severity = "critical"; }
            else if (info.probeUnsafe > 0) { problems.push(`DOM probe saw an unshielded rendered spoiler element but 0 sentinel pixels were painted (inconclusive; page may be hidden another way): dom=${info.probeUnsafe}`); severity = severity || "medium"; }
          } else {
            if (info.shieldCount || (info.pageShield && info.pageShield !== "") || info.hasEmergency) { problems.push(`cold ordinary page was shielded/blocked (shields=${info.shieldCount}, page=${info.pageShield}, emergency=${info.hasEmergency})`); severity = severity || "medium"; }
            if (rec.unsafe) { problems.push("sentinel pixels on cold page"); }
          }
        }
        const row = { scenario: sc.id, mode, kind, sabotage: !!sc.sabotage, ok: problems.length === 0, severity, problems, shields: info && info.shieldCount, pageShield: info && info.pageShield, px: rec.unsafe, frames: rec.frames };
        rows.push(row);
        log(`${row.ok ? "ok  " : "FAIL"} ${sc.id.padEnd(34)} ${mode.padEnd(4)} ${kind.padEnd(4)} shields=${row.shields} page=${row.pageShield} px=${rec.unsafe}/${rec.frames}${problems.length ? " :: " + problems.join("; ") : ""}`);
      }
    }
  }
  await browser.close();
  await new Promise((r) => srv.close(r));
  return rows;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const only = (process.argv.find((a) => a.startsWith("--only=")) || "").split("=")[1];
  await runHostile({ only: only ? only.split(",") : null });
}
