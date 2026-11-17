// Minimal stand-in for normalize/lexicon/classify/policy so the runtime can be exercised while the real
// detection core is in flux (bundle option detection:"stub"). Deliberately simple; never shipped.
SFW.normalize = (function () {
  function clean(t) { return String(t).normalize("NFKC").replace(/[​-‏⁠﻿­]/g, "").toLowerCase(); }
  function views(t) { const c = clean(t); return { spaced: c.replace(/[^\p{L}\p{N}]+/gu, " ").trim() }; }
  return { clean, views };
})();
SFW.lexicon = (function () {
  return { version: "stub", franchise: [/\bgta\s*(?:6|vi|six)\b/, /\bgrand theft auto\s*(?:6|vi|six)\b/], cue: [/\b(?:spoilers?|ending|dies|died|killed|leak|leaked|betrays|twist)\b/] };
})();
SFW.classify = (function () {
  function text(t, ctx) {
    const s = SFW.normalize.views(t).spaced;
    const franchise = SFW.lexicon.franchise.some((r) => r.test(s));
    const cue = SFW.lexicon.cue.some((r) => r.test(s));
    const hot = (ctx && ctx.hot) || 0;
    const signals = [];
    if (franchise) signals.push({ kind: "franchise_strong", source: (ctx && ctx.source) || "text" });
    if (cue) signals.push({ kind: "cue_strong", source: (ctx && ctx.source) || "text" });
    let score = 0;
    if (franchise && cue) score = 0.97; else if (franchise) score = 0.6; else if (cue && hot === 2) score = 0.9; else if (cue) score = 0.2;
    const level = score >= 0.9 ? "spoiler" : score >= 0.6 ? "mention" : score > 0 ? "risk" : "none";
    return { score, level, signals, flags: { franchise, entity: false, entityWeak: false, cue, cueWeak: false, marker: false, safe: false } };
  }
  function url(u) { try { const x = new URL(u); return text(decodeURIComponent(x.pathname + " " + x.search).replace(/[-_+/]/g, " "), { source: "url" }); } catch (e) { return text("", {}); } }
  function isHotText(t) { return text(t).flags.franchise; }
  return { text, url, isHotText };
})();
SFW.policy = (function () {
  const MODES = ["lockdown", "strict", "balanced"];
  function decide(r, o) {
    const mode = (o && o.mode) || "strict";
    if (r.level === "spoiler") return "shield";
    if (r.level === "mention" && mode !== "balanced") return "shield";
    if (o && o.hot === 2 && r.level !== "none") return "shield";
    return "clear";
  }
  function decideAll(r, o) { const out = {}; for (const m of MODES) out[m] = decide(r, Object.assign({}, o, { mode: m })); return out; }
  return { MODES, DEFAULT_MODE: "strict", decide, decideAll };
})();
