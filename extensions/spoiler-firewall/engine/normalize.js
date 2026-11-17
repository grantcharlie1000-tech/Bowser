// SFW.normalize: adversarial text normalisation for the detection core.
//
// fold(text)   one pass: NFKC, invisible/format stripping, camelCase/hashtag split marks (U+0001),
//              lowercase, mark stripping (Latin/Greek/Cyrillic/Arabic/Hebrew only: kana
//              dakuten and Thai/Indic marks are kept), confusable fold (Cyrillic/Greek/Armenian/
//              Cherokee look-alikes, enclosed/squared/regional letters, small caps, upside-down
//              letters), katakana->hiragana, non-ASCII digits -> ASCII digits.
// build(base)  token view: compact string + per-char flags (token start/end/hashtag) + sentence ids.
//              Matches in this view must be token aligned and must not cross sentence punctuation.
// skeleton()   cheap letters/digits/leet string used by the anchor prefilter.
// views(text)  human/test friendly lazily computed views (spaced, compact, leet, squashed, ...).
//
// No DOM, no eval, no globals beyond builtins. Pure functions; safe in any realm.
SFW.normalize = (function () {
  // ------------------------------------------------------------------ tables (lazy)
  let T = null;

  const CONF_SRC = {
    a: "аɑαᴀɐ⍺",
    b: "вβʙɓƅ",
    c: "сϲᴄ",
    d: "ԁɗᴅ",
    e: "еεɛᴇǝ",
    f: "ƒꜰɟ",
    g: "ɡɢƃցɠ\u050c\u050d",
    h: "нһհʜɥ",
    i: "іɪıᴉɩι",
    j: "јϳᴊɾ",
    k: "кκᴋʞ",
    l: "ӏǀʟן",
    m: "мᴍɯ",
    n: "ηոɴ",
    o: "оοօᴏσ",
    p: "рρᴘ",
    q: "ԛɋǫ",
    r: "ʀɹ",
    s: "ѕꜱƽ",
    t: "тτᴛʇ",
    u: "υսμᴜ",
    v: "νѵᴠʌ",
    w: "ԝᴡʍω",
    x: "хχ",
    y: "уγʏʎү",
    z: "ᴢƶʐ",
  };
  // Cherokee capital letters that resemble Latin letters (lowercased forms are registered too).
  const CHEROKEE = { a: "Ꭺ", b: "Ᏼ", c: "Ꮯ", d: "Ꭰ", e: "Ꭼ", g: "Ꮐ", h: "Ꮋ", i: "Ꭵ", j: "Ꭻ", k: "Ꮶ", l: "Ꮮ", m: "Ꮇ", p: "Ꮲ", r: "Ꭱ", s: "Ꮪ", t: "Ꭲ", v: "Ꮩ", w: "Ꮤ", z: "Ꮓ" };
  const SPECIAL = {
    "ß": "ss", "ẞ": "ss", "æ": "ae", "œ": "oe", "ø": "o", "đ": "d", "ð": "d", "þ": "th", "ł": "l", "ħ": "h", "ŧ": "t",
    "ĸ": "k", "ŋ": "n", "ƿ": "w", "ɨ": "i", "ɫ": "l", "ſ": "s", "ȷ": "j",
    "أ": "ا", "إ": "ا", "آ": "ا", "ٱ": "ا", "ى": "ي", "ة": "ه", "ؤ": "و", "ئ": "ي", "ک": "ك", "ی": "ي", "ھ": "ه",
  };
  // Zero code points of decimal digit blocks (non-ASCII \p{Nd}).
  const DIGIT_ZEROS = [0x660, 0x6f0, 0x7c0, 0x966, 0x9e6, 0xa66, 0xae6, 0xb66, 0xbe6, 0xc66, 0xce6, 0xd66, 0xde6, 0xe50, 0xed0, 0xf20, 0x1040, 0x1090, 0x17e0, 0x1810, 0xa620, 0xa8d0, 0xa900, 0xa9d0, 0xaa50, 0xabf0];

  function tables() {
    if (T) return T;
    const conf = new Map();
    for (const k in CONF_SRC) for (const ch of CONF_SRC[k]) conf.set(ch.codePointAt(0), k);
    for (const k in CHEROKEE) {
      const up = CHEROKEE[k];
      conf.set(up.codePointAt(0), k);
      conf.set(up.toLowerCase().codePointAt(0), k);
    }
    for (const k in SPECIAL) conf.set(k.codePointAt(0), SPECIAL[k]);
    T = { conf, decomp: new Map() };
    return T;
  }

  function baseOf(cp, ch) {
    const t = tables();
    let b = t.decomp.get(cp);
    if (b === undefined) {
      const d = ch.normalize("NFD");
      b = d === ch ? ch : String.fromCodePoint(d.codePointAt(0));
      t.decomp.set(cp, b);
    }
    return b;
  }

  function isDropMark(cp) {
    return (
      (cp >= 0x300 && cp <= 0x36f) || (cp >= 0x483 && cp <= 0x489) || (cp >= 0x591 && cp <= 0x5c7) ||
      (cp >= 0x610 && cp <= 0x61a) || (cp >= 0x64b && cp <= 0x65f) || cp === 0x670 || cp === 0x640 ||
      (cp >= 0x6d6 && cp <= 0x6ed) || (cp >= 0x1ab0 && cp <= 0x1aff) || (cp >= 0x1dc0 && cp <= 0x1dff) ||
      (cp >= 0x20d0 && cp <= 0x20ff) || (cp >= 0xfe20 && cp <= 0xfe2f)
    );
  }

  function mapCp(cp, ch) {
    if (cp >= 0x1f1e6 && cp <= 0x1f1ff) return String.fromCharCode(97 + cp - 0x1f1e6);
    if (cp >= 0x1f170 && cp <= 0x1f189) return String.fromCharCode(97 + cp - 0x1f170);
    if (cp >= 0x1f150 && cp <= 0x1f169) return String.fromCharCode(97 + cp - 0x1f150);
    if (cp >= 0x1f130 && cp <= 0x1f149) return String.fromCharCode(97 + cp - 0x1f130);
    if (cp >= 0x24f5 && cp <= 0x24fd) return String.fromCharCode(49 + cp - 0x24f5); // double circled 1-9
    if (cp >= 0x2776 && cp <= 0x277e) return String.fromCharCode(49 + cp - 0x2776);
    if (cp >= 0x2780 && cp <= 0x2788) return String.fromCharCode(49 + cp - 0x2780);
    if (cp >= 0x278a && cp <= 0x2792) return String.fromCharCode(49 + cp - 0x278a);
    if (isDropMark(cp)) return "";
    if (cp >= 0x660) {
      for (let i = 0; i < DIGIT_ZEROS.length; i++) {
        const z = DIGIT_ZEROS[i];
        if (cp >= z && cp < z + 10) return String.fromCharCode(48 + cp - z);
      }
    }
    if (cp >= 0x30a1 && cp <= 0x30f6) return String.fromCharCode(cp - 0x60); // katakana -> hiragana
    const conf = tables().conf;
    let m = conf.get(cp);
    if (m !== undefined) return m;
    if (
      (cp >= 0xc0 && cp <= 0x24f) || (cp >= 0x1e00 && cp <= 0x1eff) || (cp >= 0x386 && cp <= 0x3ce) ||
      (cp >= 0x1f00 && cp <= 0x1fff) || (cp >= 0x400 && cp <= 0x4ff)
    ) {
      const b = baseOf(cp, ch);
      if (b !== ch) {
        m = conf.get(b.codePointAt(0));
        return m !== undefined ? m : b;
      }
    }
    return ch;
  }

  function mapRun(run) {
    let out = "";
    for (const ch of run) out += mapCp(ch.codePointAt(0), ch);
    return out;
  }

  const NONASCII = /[^\x00-\x7f]/;
  const NONASCII_RUN = /[^\x00-\x7f]+/g;
  // \p{Cf} + other invisible fillers + variation selectors (incl. supplement) + tag characters (in Cf).
  const INVISIBLE = /[\p{Cf}͏ᅟᅠ឴឵᠋-᠏ㅤ︀-️ﾠ\u{e0100}-\u{e01ef}]/gu;
  const CAMEL1 = /([a-z0-9])([A-Z])/g;
  const CAMEL2 = /([A-Z])([A-Z][a-z])/g;

  // hashtags, @handles and u/ r/ names: camelCase words are split ("@JasonDuvalDies", "u/LuciaDiesAtTheEnd")
  const HASHTAG = /(?:[#@]|\b[ur]\/)[A-Za-z0-9]+/g;
  const FR_GLUE = /(gta|auto)(vi|6|six)(?=[a-z])(?!ce)/gi;
  function camelHash(m) {
    return m.replace(FR_GLUE, "$1$2\u0001").replace(CAMEL1, "$1\u0001$2").replace(CAMEL2, "$1\u0001$2");
  }
  // literal HTML entities that carry invisible/space characters ("GT&shy;A 6")
  const ENT_RE = /&(?:shy|zwnj|zwj|zwsp|nbsp|ensp|emsp|thinsp|#x?[0-9a-f]{1,6});/gi;
  function entFn(m) {
    const b = m.slice(1, -1).toLowerCase();
    if (b === "shy" || b === "zwnj" || b === "zwj" || b === "zwsp") return "";
    if (b === "nbsp" || b === "ensp" || b === "emsp" || b === "thinsp") return " ";
    const cp = b.charAt(1) === "x" ? parseInt(b.slice(2), 16) : parseInt(b.slice(1), 10);
    if (!(cp > 0 && cp <= 0x10ffff) || (cp >= 0xd800 && cp <= 0xdfff)) return m;
    return String.fromCodePoint(cp);
  }

  function fold(input) {
    let s = typeof input === "string" ? input : input == null ? "" : String(input);
    if (!s) return "";
    if (s.indexOf("&") >= 0 && s.indexOf(";") > 0) s = s.replace(ENT_RE, entFn);
    const nonAscii = NONASCII.test(s);
    if (nonAscii) {
      s = s.normalize("NFKC").replace(INVISIBLE, "");
    }
    // camelCase / hashtag split marks (before lowercasing; ASCII only)
    if (s.indexOf("#") >= 0 || s.indexOf("@") >= 0 || s.indexOf("/") >= 0) s = s.replace(HASHTAG, camelHash);
    s = s.toLowerCase();
    if (nonAscii) s = s.replace(NONASCII_RUN, mapRun);
    return s;
  }

  function clean(text) {
    return fold(text).replace(/\u0001/g, "");
  }

  // ------------------------------------------------------------------ token view
  const WCACHE = new Uint8Array(65536);
  const WORD_RE = /[\p{L}\p{N}\p{M}]/u;
  function isWord(c) {
    if (c < 128) return (c >= 97 && c <= 122) || (c >= 48 && c <= 57);
    let v = WCACHE[c];
    if (v === 0) {
      v = c >= 0xd800 && c <= 0xdfff ? 2 : WORD_RE.test(String.fromCharCode(c)) ? 1 : 2;
      WCACHE[c] = v;
    }
    return v === 1;
  }
  // Scripts written without spaces (or with attached particles): matched by substring, and they
  // always break tokens against adjacent Latin/digit runs.
  function isScriptio(c) {
    return (
      (c >= 0x3040 && c <= 0x30ff) || (c >= 0x3400 && c <= 0x4dbf) || (c >= 0x4e00 && c <= 0x9fff) ||
      (c >= 0xf900 && c <= 0xfaff) || (c >= 0xac00 && c <= 0xd7af) || (c >= 0x1100 && c <= 0x11ff) ||
      (c >= 0x3130 && c <= 0x318f) || (c >= 0xe00 && c <= 0xeff) || (c >= 0x1780 && c <= 0x17ff) ||
      (c >= 0x1000 && c <= 0x109f)
    );
  }
  const SCRIPTIO_RE = /[぀-ヿ㐀-䶿一-鿿豈-﫿가-힯ᄀ-ᇿ㄰-㆏฀-໿ក-៿က-႟]/;

  const ABBREV = new Set(["st", "mt", "dr", "mr", "mrs", "ms", "jr", "sr", "vs", "ft", "gen", "sgt", "col", "lt", "cpt", "prof"]);
  const FL_START = 1, FL_END = 2, FL_HASH = 4;
  // Symbols that stand for a letter when sandwiched inside a token: kept verbatim (@ $ ! |) or as
  // a wildcard ('*'). In "wild" views further symbols also become wildcards.
  const WILDX = new Set([95, 63, 35, 0x2022, 0xb7, 126, 94, 43, 61, 37, 38]); // _ ? # • · ~ ^ + = % &

  function build(base, wild) {
    const n = base.length;
    const s = new Uint16Array(n);
    const f = new Uint8Array(n);
    const sid = new Int32Array(n);
    let k = 0, sent = 0, inTok = false, hardPending = false, hashPending = false, hashTok = false;
    let tokStart = 0, lastTokLen = 0, sepDot = false, dotBefore = false, prevScr = false;
    for (let i = 0; i < n; i++) {
      const c = base.charCodeAt(i);
      if (isWord(c)) {
        const scr = isScriptio(c);
        if (inTok && scr !== prevScr) {
          f[k - 1] |= FL_END;
          inTok = false;
          lastTokLen = k - tokStart;
        }
        if (!inTok) {
          f[k] |= FL_START;
          inTok = true;
          tokStart = k;
          hashTok = hashPending;
          hashPending = false;
          dotBefore = sepDot;
          sepDot = false;
          if (hardPending) {
            sent++;
            hardPending = false;
          }
        }
        prevScr = scr;
        s[k] = c;
        sid[k] = sent;
        if (hashTok) f[k] |= FL_HASH;
        k++;
        continue;
      }
      // not a word character
      if (c === 64 && inTok && !prevScr && k - tokStart >= 2 && (i + 1 >= n || !isWord(base.charCodeAt(i + 1)))) {
        // trailing '@' standing for the letter a ("gt@ 6", "sp0il3rs@")
        s[k] = 64;
        sid[k] = sent;
        if (hashTok) f[k] |= FL_HASH;
        k++;
        continue;
      }
      if (inTok && i + 1 < n && isWord(base.charCodeAt(i + 1)) && !isScriptio(base.charCodeAt(i + 1)) && !prevScr) {
        let g = 0;
        if (c === 64 || c === 36 || c === 33 || c === 124 || c === 42) g = c;
        else if (wild && WILDX.has(c)) g = 42;
        if (g) {
          s[k] = g;
          sid[k] = sent;
          if (hashTok) f[k] |= FL_HASH;
          k++;
          continue;
        }
      }
      if (inTok) {
        f[k - 1] |= FL_END;
        inTok = false;
        lastTokLen = k - tokStart;
      }
      let hard = false;
      switch (c) {
        case 35: // '#': hashtag start
          if (i + 1 < n && isWord(base.charCodeAt(i + 1))) hashPending = true;
          break;
        case 10: case 13: case 0x2028: { // a single line break is a soft wrap; a blank line is a paragraph break
          let j = i + 1;
          while (j < n) {
            const d = base.charCodeAt(j);
            if (d === 32 || d === 9) j++;
            else break;
          }
          const nd = j < n ? base.charCodeAt(j) : 10;
          if (c === 13 && nd === 10 && j === i + 1) break; // CRLF: the LF decides
          if (nd === 10 || nd === 13 || j >= n) hard = true;
          break;
        }
        case 0x2029:
        case 33: case 63: case 59:
        case 0x3002: case 0x0964: case 0x06d4: case 0x061f: case 0x061b: case 0x0589:
          hard = true;
          break;
        case 58: { // ':' is sentence punctuation except in "GTA: VI" / "Grand Theft Auto: VI"
          hard = true;
          if (lastTokLen === 3 || lastTokLen === 4) {
            const w = String.fromCharCode.apply(null, s.subarray(k - lastTokLen, k));
            if (w === "gta" || w === "auto") hard = false;
          }
          break;
        }
        case 46: { // '.'
          if (i + 1 < n && isWord(base.charCodeAt(i + 1))) { sepDot = true; break; } // "g.t.a", "3.5", "a.com"
          hard = true;
          if (i > 0 && (base.charCodeAt(i - 1) === 32 || base.charCodeAt(i - 1) === 9)) { hard = false; sepDot = true; break; } // " . " as a separator
          {
            let j = i + 1;
            while (j < n && !isWord(base.charCodeAt(j)) && base.charCodeAt(j) !== 32 && base.charCodeAt(j) !== 9 && base.charCodeAt(j) !== 10 && base.charCodeAt(j) !== 13) j++;
            const pc = i > 0 ? base.charCodeAt(i - 1) : 32;
            if (j < n && j > i + 1 && isWord(base.charCodeAt(j)) && (isWord(pc) || pc === 46 || pc === 45 || pc === 95)) { hard = false; sepDot = true; break; } // "G...T", "G-.-T"
          }
          if (lastTokLen >= 1 && lastTokLen <= 3 && ABBREV.has(String.fromCharCode.apply(null, s.subarray(k - lastTokLen, k)))) { hard = false; break; } // "St. Lucia"
          if (lastTokLen === 2 || lastTokLen === 3) {
            // "GTA no. 6", "GTA nr. 6": a numero sign before a digit is not a sentence end
            const w = String.fromCharCode.apply(null, s.subarray(k - lastTokLen, k));
            if (w === "no" || w === "nr" || w === "num") {
              let j = i + 1;
              while (j < n && (base.charCodeAt(j) === 32 || base.charCodeAt(j) === 9)) j++;
              if (j < n && base.charCodeAt(j) >= 48 && base.charCodeAt(j) <= 57) { hard = false; break; }
            }
          }
          if (lastTokLen === 1) {
            if (dotBefore) hard = false; // "g.t.a. 6"
            else {
              let j = i + 1;
              while (j < n && (base.charCodeAt(j) === 32 || base.charCodeAt(j) === 9)) j++;
              if (j + 1 < n && isWord(base.charCodeAt(j)) && base.charCodeAt(j + 1) === 46) hard = false; // "g. t. a. 6"
            }
          }
          sepDot = true;
          break;
        }
        default:
          break;
      }
      if (hard) hardPending = true;
    }
    if (inTok) f[k - 1] |= FL_END;
    let str = "";
    for (let i = 0; i < k; i += 8192) str += String.fromCharCode.apply(null, s.subarray(i, Math.min(k, i + 8192)));
    return { n: k, s: str, f, sid };
  }

  // ------------------------------------------------------------------ prefilter skeleton
  const WILD_CAND = /[\p{L}\p{N}][*_?#•\xb7~^+=%&]+[\p{L}\p{N}]/u;
  const SK_STRIP = /[^\p{L}\p{N}\p{M}@$!|*]+/gu;
  const SK_MAP = { l: "i", 1: "i", "|": "i", "!": "i", 0: "o", 3: "e", 4: "a", 5: "s", 6: "g", 7: "t", 8: "b", "@": "a", $: "s" };
  const STRAY_TEST = /[@$!|*]/;
  // stray symbols are separators; a trailing '@' after two or more letters stays ("gt@ 6")
  const STRAY = /(?<![\p{L}\p{N}])[@$!|*]+|[$!|*]+(?![\p{L}\p{N}])|(?<![\p{L}\p{N}][\p{L}\p{N}])@+(?![\p{L}\p{N}])/gu;
  const SK_RE = /[l01345678@$!|]/g;
  function skFoldFn(m) {
    return SK_MAP[m];
  }
  function skFold(str) {
    return str.replace(SK_RE, skFoldFn);
  }
  // returns { sk, wild } where wild means "sandwiched censor-style symbols present"
  function skeleton(base) {
    const wild = WILD_CAND.test(base);
    let t = base;
    // leet/wildcard symbols only count when sandwiched inside a token; stray ones are separators
    if (STRAY_TEST.test(t)) t = t.replace(STRAY, " ");
    t = t.replace(SK_STRIP, "");
    t = t.replace(SK_RE, skFoldFn);
    return { sk: t, wild };
  }

  // ------------------------------------------------------------------ other transforms
  function rot13(s) {
    return s.replace(/[a-z]/g, (c) => {
      const k = c.charCodeAt(0) - 97;
      return String.fromCharCode(97 + ((k + 13) % 26));
    });
  }
  function reverse(s) {
    let o = "";
    for (let i = s.length - 1; i >= 0; i--) o += s.charAt(i);
    return o;
  }
  function reverseWords(s) {
    return s.replace(/\S+/g, reverse);
  }
  function reverseAlnum(s) {
    return s.replace(/[\p{L}\p{N}]+/gu, reverse);
  }
  // 180-degree rotated text: reverse and swap letters that are their own mirror partner.
  const FLIP = { u: "n", n: "u", d: "p", p: "d", b: "q", q: "b", 6: "9", 9: "6" };
  function flip(s) {
    return reverse(s).replace(/[undpbq69]/g, (c) => FLIP[c]);
  }
  const UPSIDE = /[ɐɔǝɟƃɥᴉɾʞןɯɹʇʌʍʎ]/;

  // ------------------------------------------------------------------ lexicon term compilation
  // A lexicon term goes through exactly the same fold + token view as page text.
  function term(str) {
    const base = fold(str);
    const v = build(base, false);
    const offs = [];
    for (let i = 0; i < v.n; i++) if (v.f[i] & FL_START) offs.push(i);
    let hasDigit = false;
    for (let i = 0; i < v.n; i++) {
      const c = v.s.charCodeAt(i);
      if (c >= 48 && c <= 57) hasDigit = true;
    }
    return { c: v.s, offs, hasDigit, sub: SCRIPTIO_RE.test(v.s) };
  }

  // ------------------------------------------------------------------ test-friendly views
  const LEET = { 0: "o", 1: "i", 3: "e", 4: "a", 5: "s", 7: "t", "@": "a", $: "s", "!": "i", "|": "l" };
  function leetFold(str) {
    return str.replace(/[013457@$!|]/g, (c) => LEET[c]);
  }
  function squash(str) {
    return str.replace(/([a-z\u0080-\uffff])\1{2,}/g, "$1").replace(/(\d)\1+/g, "$1");
  }
  function spacedOf(base) {
    return base.replace(/[^\p{L}\p{N}\p{M}]+/gu, " ").trim();
  }

  function views(text) {
    const base = fold(text);
    const cache = {};
    function lazy(name, fn) {
      Object.defineProperty(out, name, {
        enumerable: true,
        get() {
          if (!(name in cache)) cache[name] = fn();
          return cache[name];
        },
      });
    }
    const out = {};
    lazy("base", () => base);
    lazy("clean", () => base.replace(/\u0001/g, ""));
    lazy("spaced", () => spacedOf(base));
    lazy("split", () => spacedOf(base.replace(/\u0001/g, " ")));
    lazy("token", () => build(base, false));
    lazy("compact", () => out.token.s);
    lazy("leet", () => leetFold(out.compact));
    lazy("squashed", () => squash(out.spaced));
    lazy("rot13", () => rot13(out.spaced));
    lazy("reversed", () => reverse(out.spaced));
    lazy("flipped", () => flip(out.spaced));
    lazy("skeleton", () => skeleton(base).sk);
    return out;
  }

  return {
    clean, fold, build, skeleton, skFold, rot13, reverse, reverseWords, reverseAlnum, flip, term, views,
    isWord, isScriptio, leetFold, squash, UPSIDE, SCRIPTIO_RE,
    FL_START, FL_END, FL_HASH,
  };
})();
