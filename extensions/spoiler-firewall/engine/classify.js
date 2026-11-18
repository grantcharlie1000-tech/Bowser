// SFW.classify: spoiler classification (DESIGN 3, 4.3, 4.4, 10).
//
// Pipeline: fold -> anchor prefilter -> windows (<=4000 chars, 500 overlap) -> token view ->
// one trie walk over every lexicon category (leet/wildcard/stutter tolerant, token aligned,
// sentence bounded) -> fuzzy name scan -> ROT13/reversed/flipped views (franchise + entities only) ->
// rule table (noisy-OR is only the convenience score; the rules decide).
// Signals carry categories and sources only, never matched text.
SFW.classify = (function () {
  const WINDOW = 4000, OVERLAP = 500;
  // Hard bound per call: at most MAX_CHARS characters are looked at (head + tail halves) and the window loop
  // stops after BUDGET_MS; whatever was not scanned is reported as result.truncated (policy fails closed on it).
  const MAX_CHARS = 200000, BUDGET_MS = 30, MAX_HITS = 1500;
  // per-call work guard (long text only): trie walks stop at the deadline or when a window produces absurdly many hits
  let DEADLINE = 0, OVER = false;
  const ORG_OK = { gta: 1, grandtheftauto: 1, rockstar: 1, rockstargames: 1, rockstarnorth: 1 };
  const RSTAR_RE = /(^|[^a-z0-9])r[*\u2605\u2606\u2736](?![a-z0-9*])/g;
  const stats = { calls: 0, rejected: 0 }; // counters only; never any text
  const LEET_EQ = { 0: "o", 1: "il", 3: "e", 4: "a", 5: "s", 6: "g", 7: "t", "@": "a", $: "s", "!": "il", "|": "il" };
  const MID_OK = { franchise: 1, franchiseCtx: 1, entity: 1, cueS: 1, marker: 1 };
  const STRONG_SPLIT = { franchise: 1, franchiseCtx: 1, entity: 1, marker: 1 };
  const LW_CATS = { franchise: 1, franchiseCtx: 1, entity: 1, weak: 1, cueS: 1, marker: 1 };

  function newFlags() {
    return { franchise: false, entity: false, entityWeak: false, cue: false, cueWeak: false, marker: false, safe: false };
  }
  function noneResult(len) {
    return { score: 0, level: "none", signals: [], flags: newFlags(), len: len | 0 };
  }

  // ------------------------------------------------------------------ matching
  function isLetterCode(c) {
    return c >= 97 && c <= 122;
  }

  function accept(v, t, s, e, mid, lex) {
    const f = v.f, sid = v.sid, n = v.n;
    if (sid[s] !== sid[e - 1]) return false;
    if (t.sub) return true;
    if (mid) {
      if (!MID_OK[t.cat] || t.n < 4) return false;
    } else if (!(f[s] & 1)) return false;
    if (!(f[e - 1] & 2)) {
      if (f[e - 1] & 4) {
        // hashtag token: a term may be a prefix of it
      } else {
        if (t.ed === undefined) t.ed = /[0-9]$/.test(t.c);
        if (!(t.ed && e < n && isLetterCode(v.s.charCodeAt(e)))) return false;
      }
    }
    if (!mid) {
      let k = 1;
      for (let i = s + 1; i < e; i++) if (f[i] & 1) k++;
      if (k >= 2 && e - s === t.n && t.offs.length >= 2) {
        // the text may merge the term's tokens ("gta" for "g t a") or split them further, but not both at once
        // ("gta v i" is not "g t a" + "vi")
        let extra = false;
        for (let i = s + 1; i < e && !extra; i++) if ((f[i] & 1) && t.offs.indexOf(i - s) < 0) extra = true;
        if (extra) for (let q = 1; q < t.offs.length; q++) if (!(f[s + t.offs[q]] & 1)) return false;
      }
      if (k >= 2 && t.dn && t.n === 4 && t.offs.length >= 2 && e - s === 4 && !(f[s + 3] & 1)) return false; // "gt a6" (Audi, phones) is not "gta 6"
      if (t.ss && k < t.offs.length) return false; // spaced-letter forms ("g t a v i") need every letter spaced
      if (k > t.offs.length) {
        const strong = STRONG_SPLIT[t.cat] === 1;
        if (!(strong && t.n >= 9)) {
          const lim = strong ? 2 : 1;
          let len = 0;
          for (let i = s; i < e; i++) {
            if (f[i] & 1) len = 0;
            len++;
            if (len > lim) return false;
          }
        }
      }
    }
    if (t.unit && e < n && (f[e] & 1)) {
      let j = e;
      while (j < n && !(f[j] & 2)) j++;
      if (lex.unitSet.has(v.s.slice(e, j + 1))) return false;
    }
    return true;
  }

  function hasLetter(S, a, b) {
    for (let k = a; k < b; k++) {
      const c = S.charCodeAt(k);
      if ((c >= 97 && c <= 122) || c >= 128) return true;
    }
    return false;
  }

  function walk(root, v, s0, allowEq, mid, hits, lex) {
    const S = v.s, n = v.n;
    function rec(node, i, stars, leets, sk) {
      const tl = node.t;
      if (tl) {
        for (let j = 0; j < tl.length; j++) {
          const t = tl[j];
          if (sk && t.cat !== "franchise") continue;
          if ((stars || leets) && (!LW_CATS[t.cat] || t.n < 4 || t.sub)) continue;
          if (stars && (stars * 3 > t.n || t.n - stars < 2)) continue;
          // a leet/wildcard match must contain at least one real letter ("6746", "1337", "6 7 4 6" are numbers, not gta)
          if ((stars || leets) && !hasLetter(S, s0, i)) continue;
          if (accept(v, t, s0, i, mid, lex)) hits.push({ t: t, s: s0, e: i });
        }
      }
      if (i >= n) return;
      const ch = S.charAt(i);
      let nx = node.c[ch];
      if (nx) {
        rec(nx, i + 1, stars, leets, sk);
        const code = S.charCodeAt(i);
        if (S.charCodeAt(i + 1) === code) {
          let j = i + 1;
          while (j < n && S.charCodeAt(j) === code) j++;
          if ((code >= 48 && code <= 57 && j - i >= 2) || j - i >= 3) {
            rec(nx, j, stars, leets, sk);
            const nx2 = nx.c[ch];
            if (nx2) rec(nx2, j, stars, leets, sk);
          }
        }
      }
      if (allowEq) {
        let eq = LEET_EQ[ch];
        // a number standing alone after a real word ("gta 5 est") is a number, not a letter ("G.7.A" keeps working:
        // its previous token is a single letter too)
        if (eq && i > s0 && (v.f[i] & 3) === 3) {
          const cc = S.charCodeAt(i);
          if (cc >= 48 && cc <= 57) {
            let j = i - 1;
            while (j > s0 && !(v.f[j] & 1)) j--;
            if (i - j >= 2) eq = undefined;
          }
        }
        if (eq) {
          for (let k = 0; k < eq.length; k++) {
            nx = node.c[eq.charAt(k)];
            if (nx) rec(nx, i + 1, stars, leets + 1, sk);
          }
        } else if (ch === "*") {
          for (const key in node.c) rec(node.c[key], i + 1, stars + 1, leets, sk);
        }
        if (i > s0 && (ch === "*" || ch === "|") && i + 1 < n) rec(node, i + 1, stars, leets, 1);
      }
    }
    rec(root, s0, 0, 0, 0);
  }

  function walkAll(root, v, anyStart, allowEq, hits, lex) {
    const f = v.f, n = v.n;
    for (let i = 0; i < n; i++) {
      if ((i & 63) === 0 && (hits.length > MAX_HITS || (DEADLINE !== 0 && Date.now() > DEADLINE))) { OVER = true; return; }
      const fl = f[i];
      if (anyStart) walk(root, v, i, allowEq, false, hits, lex);
      else if (fl & 1) walk(root, v, i, allowEq, false, hits, lex);
      else if (fl & 4) walk(root, v, i, allowEq, true, hits, lex);
    }
  }

  // Damerau-Levenshtein with an early exit once a whole row exceeds max (rows are reused, lengths are < 48)
  const DR0 = new Int32Array(48), DR1 = new Int32Array(48), DR2 = new Int32Array(48);
  function dl(a, b, max) {
    const la = a.length, lb = b.length;
    if (la - lb > max || lb - la > max) return max + 1;
    if (la > 46 || lb > 46) return max + 1;
    let prev2 = DR0, prev = DR1, cur = DR2;
    for (let j = 0; j <= lb; j++) prev[j] = j;
    for (let i = 1; i <= la; i++) {
      cur[0] = i;
      let rowMin = i;
      const ai = a.charCodeAt(i - 1);
      for (let j = 1; j <= lb; j++) {
        const cost = ai === b.charCodeAt(j - 1) ? 0 : 1;
        let x = prev[j] + 1;
        const y = cur[j - 1] + 1;
        if (y < x) x = y;
        const z = prev[j - 1] + cost;
        if (z < x) x = z;
        if (i > 1 && j > 1 && ai === b.charCodeAt(j - 2) && a.charCodeAt(i - 2) === b.charCodeAt(j - 1)) {
          const w = prev2[j - 2] + 1;
          if (w < x) x = w;
        }
        cur[j] = x;
        if (x < rowMin) rowMin = x;
      }
      if (rowMin > max) return max + 1;
      const t = prev2;
      prev2 = prev;
      prev = cur;
      cur = t;
    }
    return prev[lb];
  }

  // Damerau-Levenshtein <= 1 without allocations of DP rows
  function within1(a, b) {
    const la = a.length, lb = b.length;
    if (la - lb > 1 || lb - la > 1) return false;
    let i = 0;
    while (i < la && i < lb && a.charCodeAt(i) === b.charCodeAt(i)) i++;
    if (i === la && i === lb) return true;
    if (la === lb) {
      if (a.slice(i + 1) === b.slice(i + 1)) return true; // substitution
      return i + 1 < la && a.charCodeAt(i) === b.charCodeAt(i + 1) && a.charCodeAt(i + 1) === b.charCodeAt(i) && a.slice(i + 2) === b.slice(i + 2); // transposition
    }
    return la > lb ? a.slice(i + 1) === b.slice(i) : b.slice(i + 1) === a.slice(i);
  }

  // Fuzzy: terms >=7 letters, same first letter, distance <=1 (<=2 for >=11 letters), token aligned.
  // The franchise name is fuzzy but its numeral must match exactly.
  function fuzzyScan(v, lex, hits) {
    const S = v.s, f = v.f, sid = v.sid, n = v.n;
    for (let z = 0; z < lex.fuzzy.length; z++) {
      if (OVER || (DEADLINE !== 0 && Date.now() > DEADLINE)) { OVER = true; return; }
      const fz = lex.fuzzy[z];
      const L = fz.c.length;
      if (!fz.term) fz.term = { c: fz.c, n: L, offs: [0], cat: fz.cat, kind: "", sub: false };
      const c1 = fz.c.charCodeAt(1), c2 = fz.c.charCodeAt(2);
      for (let s = 0; s < n; s++) {
        if (!(f[s] & 1) || S.charAt(s) !== fz.first) continue;
        // cheap necessary condition for <= 1 edit near the start (insert/delete/substitute/transpose of char 1 or 2)
        if (fz.d === 1) {
          const b1 = S.charCodeAt(s + 1), b2 = S.charCodeAt(s + 2);
          if (b1 !== c1 && b1 !== c2 && b2 !== c1 && b2 !== c2) continue;
        }
        for (let len = L - fz.d; len <= L + fz.d; len++) {
          const e = s + len;
          if (e > n) break;
          if (sid[s] !== sid[e - 1]) continue;
          const cand = S.slice(s, e);
          if (cand === fz.c) continue; // exact forms are handled by the trie
          if (fz.d === 1 ? !within1(fz.c, cand) : dl(fz.c, cand, fz.d) > fz.d) continue;
          if (lex.stopSet.has(cand)) continue;
          if (fz.nums) {
            for (let q = 0; q < fz.nums.length; q++) {
              const u = fz.nums[q];
              const ue = e + u.length;
              if (ue <= n && S.startsWith(u, e) && (f[ue - 1] & 2) && sid[ue - 1] === sid[s]) {
                hits.push({ t: lex.fuzzyFranchise || (lex.fuzzyFranchise = { c: "~", n: 9, offs: [0], cat: "franchise", kind: "", sub: false }), s: s, e: ue });
                break;
              }
            }
          } else if (f[e - 1] & 2) {
            hits.push({ t: fz.term, s: s, e: e });
          }
        }
      }
    }
  }

  // ------------------------------------------------------------------ per-view analysis
  function newAcc() {
    return {
      F: false, E: false, C: false, c: false, M: false, S: false, G: false, L: false, L2: false, LS: false, Cn: false,
      names: new Set(), Wn: false, Wo: false, Wp: false, Wr: false, Wd: false, Wpd: false, roleCore: false,
      shapeCore: false, shapeG: false, shapeRole: false, shapeGen: false, shapeId: false, Wrock: false,
    };
  }
  function merge(a, b) {
    for (const k of ["F", "E", "C", "c", "M", "S", "G", "L", "L2", "LS", "Cn", "Wn", "Wo", "Wp", "Wr", "Wd", "Wpd", "roleCore", "shapeCore", "shapeG", "shapeRole", "shapeGen", "shapeId", "Wrock"]) if (b[k]) a[k] = true;
    for (const x of b.names) a.names.add(x);
  }

  function analyze(v, hits, st) {
    const A = newAcc();
    const negs = [], antis = [], weaks = [], ctxF = [], marks = [], games = [], sverbs = [], sends = [];
    const cuesS = [], cuesW = [], softF = [], fcues = [], ldates = [], lrels = [], sidents = [], gstricts = [];
    for (let i = 0; i < hits.length; i++) {
      const h = hits[i];
      switch (h.t.cat) {
        case "franchise": if (h.t.soft) softF.push(h); else A.F = true; break;
        case "franchiseCtx": ctxF.push(h); break;
        case "entity": A.E = true; break;
        case "weak": weaks.push(h); break;
        case "cueS": cuesS.push(h); break;
        case "cueW": cuesW.push(h); break;
        case "marker": marks.push(h); break;
        case "anti": antis.push(h); break;
        case "neg": negs.push(h); break;
        case "game": games.push(h); break;
        case "sverb": sverbs.push(h); break;
        case "send": sends.push(h); break;
        case "safe": A.S = true; break;
        case "fcue": fcues.push(h); break;
        case "ldate": ldates.push(h); break;
        case "lrel": lrels.push(h); break;
        case "sident": sidents.push(h); break;
        case "gstrict": gstricts.push(h); break;
        default: break;
      }
    }
    const covered = (h) => {
      for (let i = 0; i < antis.length; i++) if (h.s < antis[i].e && antis[i].s < h.e) return true;
      return false;
    };
    const killed = (h) => {
      for (let i = 0; i < negs.length; i++) {
        const g = negs[i];
        const w = g.t.near || 200;
        if (g.t.kills && g.t.kills.has(h.t.c) && h.s <= g.e + w && g.s <= h.e + w) return true;
      }
      return false;
    };
    const killedAny = (h) => {
      for (let i = 0; i < negs.length; i++) {
        const g = negs[i];
        if (g.t.kills && g.t.kills.has("*") && h.s <= g.e + 200 && g.s <= h.e + 200) return true;
      }
      return false;
    };
    const strongSpans = [];
    for (let i = 0; i < hits.length; i++) { const c = hits[i].t.cat; if (c === "franchise" || c === "franchiseCtx" || c === "entity") strongSpans.push(hits[i]); }
    const inStrong = (h) => { for (let i = 0; i < strongSpans.length; i++) if (h.s < strongSpans[i].e && strongSpans[i].s < h.e) return true; return false; };
    const liveWeak = [];
    for (let i = 0; i < weaks.length; i++) {
      const h = weaks[i];
      if (h.t.low && !st.hotLike) continue;
      if (killed(h) || inStrong(h)) continue;
      liveWeak.push(h);
      const k = h.t.kind;
      if (k === "name") { A.Wn = true; A.names.add(h.t.c); }
      else if (k === "org") { A.Wo = true; if (ORG_OK[h.t.c] === 1 && !v.s.startsWith("online", h.e)) A.Wrock = true; }
      else if (k === "place") { A.Wp = true; if (h.t.distinct) A.Wpd = true; }
      else if (k === "role") {
        A.Wr = true;
        if (h.t.c === "femaleprotag" || h.t.c === "femaleprotagonist") A.roleCore = true;
      }
    }
    for (let i = 0; i < softF.length; i++) {
      const h = softF[i];
      let dead = false;
      for (let j = 0; j < negs.length && !dead; j++) {
        const g = negs[j];
        if (g.t.kills && (g.t.kills.has("gta") || g.t.kills.has("grandtheftauto")) && h.s <= g.e + 200 && g.s <= h.e + 200) dead = true;
      }
      if (!dead) A.F = true;
    }
    if (st.rstar) A.Wo = true;
    if (ctxF.length && A.Wo) A.F = true;
    const lkFirst = new Set();
    for (let i = 0; i < cuesS.length; i++) {
      if (covered(cuesS[i])) continue;
      A.C = true;
      if (cuesS[i].t.lk) {
        if (A.L && !lkFirst.has(cuesS[i].t.c)) A.L2 = true; // two different leak cues
        A.L = true;
        lkFirst.add(cuesS[i].t.c);
      }
      if (cuesS[i].t.ls) A.LS = true;
      if (!cuesS[i].t.lk) A.Cn = true;
    }
    for (let i = 0; i < cuesW.length; i++) if (!covered(cuesW[i])) { A.c = true; break; }
    for (let i = 0; i < marks.length; i++) if (!covered(marks[i])) { A.M = true; break; }
    if (games.length) A.G = true;
    // ambiguous nicknames / numerals count as the franchise only next to a strong cue (launch-only ones also need the launch prior)
    if (A.C && fcues.length) {
      for (let i = 0; i < fcues.length; i++) {
        const h = fcues[i];
        if (h.t.launch && !st.launchLike) continue;
        if (killedAny(h)) continue;
        A.F = true;
        break;
      }
    }
    // "the game that comes out on the 19th": date + release/game word close together + strong cue, launch prior only
    if (A.C && (st.launchLike || A.L) && ldates.length && lrels.length) {
      outer: for (let i = 0; i < ldates.length; i++) {
        for (let j = 0; j < lrels.length; j++) {
          if (v.sid[ldates[i].s] === v.sid[lrels[j].s] && Math.abs(ldates[i].s - lrels[j].s) <= 60) { A.F = true; break outer; }
        }
      }
    }

    // cold-context spoiler-shaped sentence: <name> verb ... end-phrase, one sentence, close together
    if (sends.length && sverbs.length) {
      for (let a = 0; a < liveWeak.length && !A.shapeCore; a++) {
        const nm = liveWeak[a];
        if (nm.t.kind !== "name" && nm.t.kind !== "role") continue;
        for (let b = 0; b < sverbs.length && !A.shapeCore; b++) {
          const vb = sverbs[b];
          if (v.sid[vb.s] !== v.sid[nm.s]) continue;
          for (let c2 = 0; c2 < sends.length; c2++) {
            const en = sends[c2];
            if (v.sid[en.s] !== v.sid[nm.s]) continue;
            const lo = Math.min(nm.s, vb.s, en.s), hi = Math.max(nm.e, vb.e, en.e);
            if (hi - lo > 200) continue;
            A.shapeCore = true;
            if (nm.t.kind === "role" && (nm.t.c === "femaleprotag" || nm.t.c === "femaleprotagonist")) A.shapeRole = true;
            for (let g = 0; g < games.length; g++) {
              const gm = games[g];
              const ov = (x) => gm.s < x.e && x.s < gm.e;
              if (!ov(en) && !ov(vb) && !ov(nm)) { A.shapeG = true; break; }
            }
            break;
          }
        }
      }
    }
    // unnamed cold-context shape: "<someone> dies/betrays ... final mission/at the end" plus a game-register word in
    // the same sentence ("a spoiler for some game" is an acceptable false positive in the launch window)
    if (sends.length && sverbs.length && gstricts.length) {
      const otherWork = (x) => {
        for (let i = 0; i < negs.length; i++) {
          const g = negs[i];
          if (g.t.kills && g.t.kills.has("*") && x.s <= g.e + 200 && g.s <= x.e + 200) return true;
        }
        return false;
      };
      gen: for (let b = 0; b < sverbs.length; b++) {
        const vb = sverbs[b];
        if (otherWork(vb)) continue;
        for (let c2 = 0; c2 < sends.length; c2++) {
          const en = sends[c2];
          if (v.sid[vb.s] !== v.sid[en.s]) continue;
          const lo = Math.min(vb.s, en.s) - 100, hi = Math.max(vb.e, en.e) + 100;
          if (Math.max(vb.e, en.e) - Math.min(vb.s, en.s) > 200) continue;
          for (let g = 0; g < gstricts.length; g++) {
            const gm = gstricts[g];
            if (gm.s >= lo && gm.e <= hi && v.sid[gm.s] === v.sid[vb.s]) { A.shapeGen = true; break gen; }
          }
        }
      }
    }
    // "<distinctive name> dies / 's death / betrays": the verb straight after the name (a surname in between, as in
    // "Lucia Santos betrayed investors", is a real person in the news)
    if (sverbs.length && liveWeak.length) {
      dn: for (let a = 0; a < liveWeak.length; a++) {
        const nm = liveWeak[a];
        if (!nm.t.distinct || nm.t.kind !== "name") continue;
        for (let b = 0; b < sverbs.length; b++) {
          const vb = sverbs[b];
          if (v.sid[vb.s] !== v.sid[nm.s] || vb.s < nm.e) continue;
          const gap = v.s.slice(nm.e, vb.s);
          if (gap.length <= 3 && /^(?:[^a-z0-9]*s?[^a-z0-9]*)$/.test(gap)) { A.Wd = true; break dn; }
        }
      }
    }
    // "<name> is the rat/traitor/..." (identity reveal) with a live weak name in the same sentence
    if (sidents.length && liveWeak.length) {
      id: for (let a = 0; a < liveWeak.length; a++) {
        const nm = liveWeak[a];
        if (nm.t.kind !== "name") continue;
        for (let b = 0; b < sidents.length; b++) {
          const sd = sidents[b];
          if (v.sid[sd.s] === v.sid[nm.s] && Math.abs(sd.s - nm.e) <= 60) { A.shapeId = true; break id; }
        }
      }
    }
    return A;
  }

  // ------------------------------------------------------------------ rules
  function bandLevel(score) {
    return score >= 0.9 ? "spoiler" : score >= 0.6 ? "mention" : score >= 0.2 ? "risk" : "none";
  }

  function rules(A, st) {
    const hot = st.hot;
    const W = A.Wn || A.Wr || A.Wp;
    const Wany = W || A.Wo;
    const any = A.F || A.E || Wany || A.C || A.c || A.M;
    let p = 0;
    const or = (x) => { p = 1 - (1 - p) * (1 - x); };
    if (A.F) or(0.62);
    if (A.E) or(0.62);
    if (A.Wn || A.Wr) or(0.18);
    if (A.Wp) or(0.1);
    if (A.Wo) or(0.06);
    const evidence = A.F || A.E || Wany || A.M || hot > 0;
    if (A.C && evidence) or(0.22);
    if (A.c && evidence) or(0.06);
    if (A.M) or(0.3);
    if (any && hot === 1) or(0.1);
    if (any && hot === 2) or(0.25);

    let floor = 0;
    const up = (x) => { if (x > floor) floor = x; };
    const strong = A.F || A.E;
    if (A.F) up(0.62);
    if (A.E) up(0.65);
    if (A.F && A.E) up(0.85);
    if (A.F && A.C) up(0.97);
    if (A.E && A.C) up(0.96);
    if (strong && A.M) up(0.96);
    if (A.F && A.c) up(0.75);
    if (A.E && A.c) up(0.78);
    if (A.M && (Wany || hot > 0)) up(0.95);
    else if (A.M) up(0.4);
    if (hot === 2 && A.C) up(0.93);
    if (hot === 1 && A.C) up(0.75);
    if ((A.Wn || A.Wr) && hot >= 1) up(0.6);
    if (A.Wn && A.c && hot >= 1) up(0.9);
    if (A.Wn || A.Wr) up(0.25);
    // names + cues
    const twoNames = A.names.size >= 2;
    if (A.Wn && A.C) {
      if (twoNames) up(0.9);
      else up(0.45);
    }
    let shape = false;
    if (A.shapeCore) {
      const cond = twoNames || A.shapeG || A.shapeRole || st.reply || st.launch;
      if (cond) { up(0.92); shape = true; } else up(0.5);
    }
    // launch prior: a weak GTA signal (name / org / role / place) with a specific leak cue ("spoiler", "leaked", "final mission")
    if (A.Wrock && A.L && (A.Cn || A.L2 || st.launchLike)) up(0.92); // "GTA/Rockstar/R* leak: the ending", in any context
    if (st.launchLike && A.L && A.Wr) up(0.92);
    if (st.launchLike && A.LS && A.Wn) up(0.92); // "Lucia ... spoiler" in the launch window
    if (st.launchLike && A.Wrock && (A.Wn || A.Wr) && A.Cn) up(0.92); // "R* really killed off Jason"
    // a distinctive name (Lucia, Duval) straight before a death / betrayal verb in the launch window ("Lucia dies 😭")
    if (st.launchLike && A.Wd) up(0.6);
    // the game's setting with a story cue or a game-register word in the launch window ("Vice City ending", "the last
    // mission in Vice City"); the bare place name stays weak (the Miami Heat's "Vice City" jerseys, the old game)
    if (st.launchLike && A.Wpd && (A.C || A.G)) up(0.6);
    // unnamed "<someone> dies ... final mission/at the end" with a game-register word
    if (A.shapeGen) {
      if (st.launchLike) { up(0.92); shape = true; } else up(0.5);
    }
    // "<name> is the rat/traitor": an identity reveal is a spoiler by itself in the launch window
    if (A.shapeId) {
      if (st.launchLike) { up(0.92); shape = true; } else up(0.5);
    }
    let score = Math.max(p, floor);
    if (score > 0.99) score = 0.99;
    score = Math.round(score * 1000) / 1000;
    return { score, shape };
  }

  function flagsOf(A, hot) {
    const f = newFlags();
    f.franchise = A.F;
    f.entity = A.E;
    f.entityWeak = A.Wn || A.Wr || A.Wp || A.Wo;
    f.cue = A.C;
    f.cueWeak = A.c;
    f.marker = A.M;
    f.safe = A.S;
    return f;
  }

  // ------------------------------------------------------------------ prefilter
  const RAW_MARK = /(?:^|[^a-z0-9])r\*(?![a-z0-9*])/;
  function prefilter(base, lex, hotLike) {
    const sk = SFW.normalize.skeleton(base);
    const s = sk.sk;
    let bits = 0;
    if (lex.anchorBase.test(s)) bits |= 1;
    if (hotLike && lex.anchorHot.test(s)) bits |= 4;
    if (lex.hasRR && lex.anchorRR.test(s)) bits |= 2;
    if (sk.wild) bits |= 8;
    if (base.indexOf(">!") >= 0 || base.indexOf("||") >= 0) bits |= 16;
    if (base.indexOf("r*") >= 0 && RAW_MARK.test(base)) bits |= 32;
    if (!bits && lex.anchorTypo.test(s)) bits = 64; // possible misspelt entity name: needs the cheap fuzzy probe
    return bits;
  }

  // Cheap typo probe: tokenise (capped) and run only the fuzzy name scan.
  function typoProbe(base, lex) {
    const v = SFW.normalize.build(base, false);
    const hits = [];
    fuzzyScan(v, lex, hits);
    return hits.length > 0;
  }

  function rawMarker(win) {
    const a = win.indexOf(">!");
    if (a >= 0 && win.indexOf("!<", a + 2) > a) return true;
    if (/\|\|[^|]{1,500}\|\|/.test(win)) return true;
    if (/[\[(]\s*spoilers?\s*[\])]/.test(win)) return true;
    return false;
  }

  // ------------------------------------------------------------------ one window
  function runWindow(win, st, lex) {
    const N = SFW.normalize;
    const v = N.build(win, false);
    const hits = [];
    walkAll(lex.trieTok, v, false, true, hits, lex);
    if (N.SCRIPTIO_RE.test(v.s)) walkAll(lex.trieSub, v, true, false, hits, lex);
    if (st.bits & 3) fuzzyScan(v, lex, hits);
    const A = analyze(v, hits, st);
    if (rawMarker(win)) A.M = true;
    if (st.bits & 8) {
      const v2 = N.build(win, true);
      const h2 = [];
      walkAll(lex.trieTok, v2, false, true, h2, lex);
      merge(A, analyze(v2, h2, st));
    }
    if ((st.bits & 2) && lex.hasRR) {
      const views = [N.rot13(win), N.reverse(win), N.reverseWords(win), N.reverseAlnum(win)];
      if (st.upside) views.push(N.flip(win));
      for (let i = 0; i < views.length; i++) {
        if (OVER || (DEADLINE !== 0 && Date.now() > DEADLINE)) { OVER = true; break; }
        const vr = N.build(views[i], false);
        const hr = [];
        walkAll(lex.trieRR, vr, false, false, hr, lex);
        for (let j = 0; j < hr.length; j++) {
          const cat = hr[j].t.cat;
          if (cat === "franchise") A.F = true;
          else if (cat === "entity") A.E = true;
        }
      }
    }
    return A;
  }

  function hotOf(ctx) {
    let h = ctx && typeof ctx.hot === "number" ? ctx.hot : 0;
    if (!(h > 0)) h = 0;
    return h >= 2 ? 2 : h >= 1 ? 1 : 0;
  }

  function text(input, ctx) {
    let str = typeof input === "string" ? input : input == null ? "" : String(input);
    const src = ctx && typeof ctx.source === "string" ? ctx.source : "text";
    const fullLen = str.length;
    if (fullLen < 2) return noneResult(fullLen);
    let truncated = false;
    if (fullLen > MAX_CHARS) {
      // head + tail; the middle is not looked at and the result says so
      str = str.slice(0, MAX_CHARS >> 1) + "\n\n" + str.slice(fullLen - (MAX_CHARS >> 1));
      truncated = true;
    }
    const hot = hotOf(ctx);
    const reply = !!(ctx && ctx.reply === true), launch = !!(ctx && ctx.launchWindow === true);
    const hotLike = hot > 0 || reply || launch;
    const lex = SFW.lexicon.compile();
    const N = SFW.normalize;
    stats.calls++;

    // Long text is folded, prefiltered and analysed one window at a time (so one call costs at most about
    // BUDGET_MS plus one window); windows that were not reached because of the budget make the result "truncated".
    const L = str.length;
    const t0 = L > WINDOW ? Date.now() : 0;
    DEADLINE = t0 ? t0 + BUDGET_MS : 0;
    OVER = false;
    let best = null;
    const total = newAcc();
    let bestScore = -1, shapeAny = false, analysed = false;
    for (let start = 0; ; start += WINDOW - OVERLAP) {
      const raw = L <= WINDOW ? str : str.slice(start, start + WINDOW);
      let base = N.fold(raw);
      if (base.indexOf("r*") >= 0 || base.indexOf("r\u2605") >= 0 || base.indexOf("r\u2606") >= 0 || base.indexOf("r\u2736") >= 0) base = base.replace(RSTAR_RE, "$1rockstar ");
      let bits = prefilter(base, lex, hotLike);
      // only a typo-tolerant anchor matched: run the cheap fuzzy-name probe before the full classifier
      if (bits === 64) bits = typoProbe(base, lex) ? 1 : 0;
      if (bits) {
        const st = { hot, reply, launch, hotLike, launchLike: launch || reply || hot >= 1, bits, upside: N.UPSIDE.test(raw), rstar: !!(bits & 32) };
        const A = runWindow(base, st, lex);
        const r = rules(A, st);
        analysed = true;
        if (r.score > bestScore) { bestScore = r.score; best = A; }
        if (r.shape) shapeAny = true;
        merge(total, A);
      }
      if (OVER) { truncated = true; break; }
      if (L <= WINDOW || start + WINDOW >= L) break;
      if (Date.now() - t0 > BUDGET_MS) { truncated = true; break; }
    }
    DEADLINE = 0;
    OVER = false;
    if (!analysed) {
      stats.rejected++;
      const r0 = noneResult(fullLen);
      if (truncated) r0.truncated = true;
      return r0;
    }
    const flags = flagsOf(total, hot);
    const lv = bestScore;
    const out = { score: lv < 0 ? 0 : lv, level: bandLevel(lv), signals: [], flags, len: fullLen };
    if (truncated) out.truncated = true;
    const sg = out.signals;
    const push = (kind) => sg.push({ kind, source: src });
    if (flags.franchise) push("franchise_strong");
    if (flags.entity) push("entity_strong");
    if (flags.entityWeak) push("entity_weak");
    if (flags.cue) push("cue_strong");
    if (flags.cueWeak) push("cue_weak");
    if (flags.marker) push("marker");
    if (flags.safe) push("safe_logistics");
    if (total.G) push("game_register");
    if (shapeAny) push("spoiler_shape");
    if (out.level !== "none") {
      if (hot > 0) push("hot_context");
      if (reply) push("reply_context");
      if (launch) push("launch_window");
    }
    return out;
  }

  // ------------------------------------------------------------------ url
  const QUERY_KEYS = /^(?:q|query|search|search_query|s|k|p|text|term|terms|keyword|keywords|wd|w|query_text|searchterm|ref|title|v)$/i;
  function url(u) {
    const s = typeof u === "string" ? u : u == null ? "" : String(u);
    let parts = "";
    try {
      const x = new URL(s);
      let path = x.pathname;
      try { path = decodeURIComponent(path); } catch (e) { /* keep raw */ }
      parts = x.hostname + " " + path;
      x.searchParams.forEach((val, key) => {
        if (QUERY_KEYS.test(key) || val.length > 3) parts += " " + val;
      });
      let hs = x.hash.length > 1 ? x.hash.slice(1) : "";
      try { hs = decodeURIComponent(hs); } catch (e) { /* keep raw */ }
      if (hs) parts += " " + hs;
    } catch (e) {
      parts = s;
    }
    if (parts.length > 4000) parts = parts.slice(0, 4000);
    return text(parts.replace(/[+_]/g, " "), { source: "url" });
  }

  function isHotText(input) {
    const r = text(input, { hot: 0 });
    return r.flags.franchise || r.flags.entity;
  }

  function explain(r) {
    if (!r || !r.flags) return "No spoiler signals";
    const f = r.flags;
    const kinds = {};
    for (const s of r.signals || []) kinds[s.kind] = 1;
    const parts = [];
    if (f.franchise) parts.push("GTA VI reference");
    else if (f.entity) parts.push("GTA VI name or place");
    else if (f.entityWeak) parts.push("possible GTA VI name");
    if (f.cue) parts.push("story cue");
    else if (f.cueWeak) parts.push("story-related word");
    if (f.marker) parts.push("spoiler marker");
    if (kinds.spoiler_shape) parts.push("spoiler-shaped sentence");
    if (kinds.hot_context) parts.push("GTA VI context");
    if (kinds.reply_context) parts.push("reply to hidden item");
    if (!parts.length) return f.safe ? "Release logistics only" : "No spoiler signals";
    return parts.join(" + ");
  }

  return { text, url, isHotText, explain, stats, WINDOW, OVERLAP };
})();
