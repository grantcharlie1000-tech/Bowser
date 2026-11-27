// Comment and whitespace stripper for the engine text in the Bowser bundle (Node 18+, no dependencies).
//
// The Bowser bundle is injected with AddScriptToExecuteOnDocumentCreated, which gets no V8 code cache: every document and
// every frame parses and compiles all of it before its own HTML. This removes comments, indentation and the spaces between
// tokens that do not need one. It never renames, reorders or rewrites a token, and it keeps a line break wherever the
// source had one between two tokens, so automatic semicolon insertion and every restricted production ("return\n x",
// "x\n++y") parse exactly as before.
//
// Safety checks (build fails on any of them): the output re-tokenizes to exactly the input's token sequence (same kinds,
// same text), every regular-expression literal compiles, and the output parses.
import vm from "node:vm";

// After these keywords a "/" starts a regular expression; after any other word or a literal it is a division.
const REGEX_AFTER_WORD = new Set(["return", "typeof", "instanceof", "in", "of", "new", "delete", "void", "throw", "case", "do", "else", "yield", "await"]);
// Punctuators, longest first.
const PUNCT = [">>>=", "...", "===", "!==", "**=", "<<=", ">>=", ">>>", "&&=", "||=", "??=",
  "=>", "==", "!=", "<=", ">=", "&&", "||", "??", "?.", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<", ">>", "**",
  "{", "}", "(", ")", "[", "]", ";", ",", "<", ">", "+", "-", "*", "/", "%", "&", "|", "^", "!", "~", "?", ":", "=", ".", "@", "#"];

function isIdStart(c) {
  return (c >= 65 && c <= 90) || (c >= 97 && c <= 122) || c === 36 || c === 95 || c === 92 || c > 127;
}
function isIdPart(c) {
  return isIdStart(c) || (c >= 48 && c <= 57);
}
function isLineTerm(c) {
  return c === 10 || c === 13 || c === 0x2028 || c === 0x2029;
}
function isSpace(c) {
  return c === 32 || c === 9 || c === 11 || c === 12 || c === 0xa0 || c === 0xfeff || (c > 127 && /\s/.test(String.fromCharCode(c)) && !isLineTerm(c));
}

// Tokens: {k: "ws"|"nl"|"com"|"id"|"num"|"str"|"tpl"|"re"|"p", v: text}. "nl" is whitespace or a comment that holds a line break.
export function tokenize(src) {
  const out = [];
  const n = src.length;
  let i = 0;
  let prev = null; // last significant token
  const tplDepth = []; // brace depth at which each open template substitution resumes its template
  let depth = 0;
  const parens = []; // per open "(": did a statement keyword (if / while / for / with) precede it?
  const regexOk = () => {
    if (!prev) return true;
    if (prev.k === "id") return REGEX_AFTER_WORD.has(prev.v);
    if (prev.k === "num" || prev.k === "str" || prev.k === "re") return false;
    if (prev.k === "tpl") return !/`$/.test(prev.v);
    if (prev.k === "p") {
      if (prev.v === ")") return !!prev.stmt; // "if (x) /re/" starts a regex; "(a + b) / 2" divides
      if (prev.v === "}") throw new Error("minify: '/' after '}' is ambiguous (block or object literal); add parentheses at " + i);
      return prev.v !== "]";
    }
    return true;
  };
  const push = (k, v) => { const t = { k, v }; out.push(t); if (k !== "ws" && k !== "nl" && k !== "com") prev = t; };
  // template chunk starting at a backtick or at the "}" that closes a substitution; returns end index (exclusive)
  const tplChunk = (start) => {
    let j = start + 1;
    for (;;) {
      if (j >= n) throw new Error("unterminated template literal at " + start);
      const c = src.charCodeAt(j);
      if (c === 92) { j += 2; continue; }
      if (c === 96) return { end: j + 1, open: false };
      if (c === 36 && src.charCodeAt(j + 1) === 123) return { end: j + 2, open: true };
      j++;
    }
  };
  while (i < n) {
    const c = src.charCodeAt(i);
    // whitespace / line breaks / comments
    if (isSpace(c) || isLineTerm(c)) {
      let j = i, nl = false;
      while (j < n) { const d = src.charCodeAt(j); if (isLineTerm(d)) nl = true; else if (!isSpace(d)) break; j++; }
      push(nl ? "nl" : "ws", src.slice(i, j));
      i = j;
      continue;
    }
    if (c === 47 && src.charCodeAt(i + 1) === 47) {
      let j = i + 2;
      while (j < n && !isLineTerm(src.charCodeAt(j))) j++;
      push("com", src.slice(i, j));
      i = j;
      continue;
    }
    if (c === 47 && src.charCodeAt(i + 1) === 42) {
      const e = src.indexOf("*/", i + 2);
      if (e < 0) throw new Error("unterminated block comment at " + i);
      const v = src.slice(i, e + 2);
      let nl = false;
      for (let j = 0; j < v.length && !nl; j++) nl = isLineTerm(v.charCodeAt(j));
      push(nl ? "nl" : "com", v);
      i = e + 2;
      continue;
    }
    if (c === 34 || c === 39) {
      let j = i + 1;
      for (;;) {
        if (j >= n) throw new Error("unterminated string at " + i);
        const d = src.charCodeAt(j);
        if (d === 92) { j += 2; continue; }
        if (d === c) break;
        if (d === 10 || d === 13) throw new Error("line break in string at " + i);
        j++;
      }
      push("str", src.slice(i, j + 1));
      i = j + 1;
      continue;
    }
    if (c === 96) {
      const r = tplChunk(i);
      if (r.open) { tplDepth.push(depth); depth++; }
      push("tpl", src.slice(i, r.end));
      i = r.end;
      continue;
    }
    if (c === 125 && tplDepth.length && tplDepth[tplDepth.length - 1] === depth - 1) {
      // "}" closing a template substitution: the template continues
      tplDepth.pop();
      depth--;
      const r = tplChunk(i);
      if (r.open) { tplDepth.push(depth); depth++; }
      push("tpl", src.slice(i, r.end));
      i = r.end;
      continue;
    }
    if (isIdStart(c)) {
      let j = i;
      while (j < n) {
        const d = src.charCodeAt(j);
        if (d === 92 && src.charCodeAt(j + 1) === 117) { j += 2; if (src.charCodeAt(j) === 123) { j = src.indexOf("}", j) + 1; } else j += 4; continue; }
        if (!isIdPart(d)) break;
        j++;
      }
      push("id", src.slice(i, j));
      i = j;
      continue;
    }
    if ((c >= 48 && c <= 57) || (c === 46 && src.charCodeAt(i + 1) >= 48 && src.charCodeAt(i + 1) <= 57)) {
      const m = /^(?:0[xX][0-9a-fA-F_]+n?|0[oO][0-7_]+n?|0[bB][01_]+n?|(?:\d[\d_]*\.?[\d_]*|\.\d[\d_]*)(?:[eE][+-]?\d[\d_]*)?n?)/.exec(src.slice(i, i + 64));
      if (!m) throw new Error("bad number at " + i);
      push("num", m[0]);
      i += m[0].length;
      continue;
    }
    if (c === 47 && regexOk()) {
      let j = i + 1, cls = false;
      for (;;) {
        if (j >= n) throw new Error("unterminated regex at " + i);
        const d = src.charCodeAt(j);
        if (d === 10 || d === 13) throw new Error("line break in regex at " + i);
        if (d === 92) { j += 2; continue; }
        if (d === 91) cls = true;
        else if (d === 93) cls = false;
        else if (d === 47 && !cls) break;
        j++;
      }
      j++;
      while (j < n && isIdPart(src.charCodeAt(j))) j++;
      push("re", src.slice(i, j));
      i = j;
      continue;
    }
    let p = null;
    for (const q of PUNCT) if (src.startsWith(q, i)) { p = q; break; }
    if (!p) throw new Error("unexpected character " + JSON.stringify(src[i]) + " at " + i);
    // "?." followed by a digit is "?" then a number (a ? .5 : b)
    if (p === "?." && src.charCodeAt(i + 2) >= 48 && src.charCodeAt(i + 2) <= 57) p = "?";
    if (p === "(") parens.push(!!prev && prev.k === "id" && (prev.v === "if" || prev.v === "while" || prev.v === "for" || prev.v === "with"));
    const stmt = p === ")" ? !!parens.pop() : false;
    if (p === "{" || p === "(" || p === "[") depth++;
    else if (p === "}" || p === ")" || p === "]") depth--;
    push("p", p);
    if (stmt) out[out.length - 1].stmt = true;
    i += p.length;
  }
  if (tplDepth.length) throw new Error("unterminated template substitution");
  return out;
}

// Two adjacent significant tokens that would merge (or change meaning) without a space between them.
function needsSpace(a, b) {
  const x = a.v.charCodeAt(a.v.length - 1), y = b.v.charCodeAt(0);
  if (isIdPart(x) && isIdPart(y)) return true; // words, numbers, "in x", "return x", "1 in"
  if (a.k === "num" && (y === 46 || isIdPart(y))) return true; // "1 .toString()"
  if ((x === 43 || x === 45) && (y === 43 || y === 45)) return true; // "a + +b", "a - -b", "x++ + y"
  if (x === 47 && (y === 47 || y === 42)) return true; // never form a comment
  if (b.k === "re" && x === 47) return true;
  if (x === 60 && y === 33) return true; // "<!" (HTML-like comment)
  if (x === 45 && y === 62) return true; // "-->"
  return false;
}

export function minifyJs(src) {
  const toks = tokenize(src);
  let out = "";
  let prev = null, gapNl = false, gap = false;
  for (const t of toks) {
    if (t.k === "ws" || t.k === "com") { gap = true; continue; }
    if (t.k === "nl") { gap = true; gapNl = true; continue; }
    if (prev) {
      if (gapNl) out += "\n";
      else if (gap && needsSpace(prev, t)) out += " ";
      else if (!gap && needsSpace(prev, t)) throw new Error("tokens without a gap would merge: " + prev.v + " " + t.v);
    }
    out += t.v;
    prev = t;
    gap = false;
    gapNl = false;
  }
  verify(src, toks, out);
  return out;
}

function sig(toks) {
  return toks.filter((t) => t.k !== "ws" && t.k !== "nl" && t.k !== "com");
}

function verify(src, toks, out) {
  const a = sig(toks);
  const b = sig(tokenize(out));
  if (a.length !== b.length) throw new Error(`minify: token count changed (${a.length} -> ${b.length})`);
  for (let i = 0; i < a.length; i++) {
    if (a[i].k !== b[i].k || a[i].v !== b[i].v) throw new Error(`minify: token ${i} changed: ${a[i].k} ${JSON.stringify(a[i].v)} -> ${b[i].k} ${JSON.stringify(b[i].v)}`);
  }
  for (const t of a) {
    if (t.k !== "re") continue;
    const end = t.v.lastIndexOf("/");
    try { new RegExp(t.v.slice(1, end), t.v.slice(end + 1)); } catch (e) { throw new Error("minify: a token read as a regular expression does not compile: " + t.v); }
  }
}

// The parse check for a whole script (the bundle): throws a SyntaxError if V8 rejects it.
export function assertParses(text, label) {
  try { new vm.Script(text, { filename: label }); } catch (e) { throw new Error(`${label}: minified output does not parse: ${e.message}`); }
}
