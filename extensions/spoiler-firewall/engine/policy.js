// SFW.policy: maps a classifier Result + context to "shield" | "clear" | "hold" (DESIGN 4.5 + 10).
//   Lockdown  any franchise/entity/marker, spoiler/mention level, or a weak GTA name/org/place together with a story
//             signal (cue, game-register word) -> shield; a bare weak name (Jason, Lucia, Cal, Brian) on a cold
//             page with no game/story signal is not; hot=2 -> shield every item.
//   Strict    (default) strong mention alone -> shield unless short safe-logistics talk; hot context
//             plus any cue/weak entity -> shield; media in hot context or flagged unit -> shield.
//   Balanced  needs spoiler level, or hot context plus a cue; media in hot context/flagged unit shields.
// A missing/invalid result fails closed (shield). "hold" = media on a high-risk host in cold context
// awaiting inspection (Lockdown/Strict only; Balanced relies on metadata signals).
SFW.policy = (function () {
  const MODES = ["lockdown", "strict", "balanced"];
  const DEFAULT_MODE = "strict";
  const SHORT_SAFE_MAX = 140;

  function decide(result, opts) {
    const o = opts || {};
    const mode = MODES.indexOf(o.mode) >= 0 ? o.mode : DEFAULT_MODE;
    const hot = o.hot >= 2 ? 2 : o.hot >= 1 ? 1 : 0;
    const hasMedia = !!o.hasMedia || o.kind === "media";
    if (!result || typeof result !== "object" || !result.flags || typeof result.level !== "string") return "shield";
    const f = result.flags;
    const lvl = result.level;
    const flagged = lvl !== "none";
    const anyCue = f.cue || f.cueWeak;

    if (hasMedia && (hot >= 1 || flagged)) return "shield";
    // text beyond the classifier's per-call cap/time budget was not looked at: budget overrun fails closed (DESIGN 4.5)
    if (result.truncated === true) return "shield";

    if (mode === "lockdown") {
      if (lvl === "spoiler" || lvl === "mention" || f.franchise || f.entity) return "shield";
      if (hot >= 1 && (flagged || f.entityWeak || f.marker)) return "shield";
      if (hot === 2) return "shield";
      if (hot === 1 && anyCue) return "shield";
      if (flagged && (f.entityWeak || f.marker)) {
        // A bare weak name/org/place or a bare spoiler marker on a cold page is not GTA evidence by itself.
        // It needs a game-register signal (mission, cutscene, playthrough ...), a spoiler-shaped sentence, or both
        // a weak name and a spoiler marker.
        let gameReg = false;
        const sg = result.signals;
        if (sg) for (let i = 0; i < sg.length; i++) if (sg[i].kind === "game_register" || sg[i].kind === "spoiler_shape") gameReg = true;
        if (gameReg || (f.entityWeak && f.marker)) return "shield";
      }
    } else if (mode === "strict") {
      if (lvl === "spoiler") return "shield";
      if (lvl === "mention") {
        const len = typeof result.len === "number" ? result.len : Infinity;
        const shortSafe = f.safe && !anyCue && !f.marker && !f.entity && !f.entityWeak && len <= SHORT_SAFE_MAX;
        if (!shortSafe) return "shield";
      }
      if (f.franchise || f.entity) {
        // a strong signal that scored below "mention" cannot happen; stay protective anyway
        if (lvl !== "mention") return "shield";
      }
      if (hot >= 1 && (anyCue || f.entityWeak || f.marker || flagged)) return "shield";
    } else {
      if (lvl === "spoiler") return "shield";
      if (hot >= 1 && (f.cue || f.marker || f.entity || (f.entityWeak && f.cueWeak))) return "shield";
    }

    if (hasMedia && o.highRisk && !o.ocrDone && hot === 0 && mode !== "balanced") return "hold";
    return "clear";
  }

  function decideAll(result, opts) {
    const out = {};
    for (let i = 0; i < MODES.length; i++) out[MODES[i]] = decide(result, Object.assign({}, opts, { mode: MODES[i] }));
    return out;
  }

  return { MODES, DEFAULT_MODE, decide, decideAll };
})();
