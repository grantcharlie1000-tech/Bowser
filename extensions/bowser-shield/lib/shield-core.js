// Bowser Shield engine: the SRL lexer, compiler and matcher.
//
// A classic script with no imports. It touches no chrome.* API, no DOM and no timers, and is loaded
// three ways (see the export lines at the bottom): require() from the Node tests, importScripts() in
// the service worker, and a <script> tag or content-script entry everywhere else.

const ShieldCore = (function () {
  "use strict";

  var VERSION = 1;
  var TYPE_NAMES = ["script", "image", "frame", "xhr", "media", "font", "style", "ping", "other"];
  var ALL_TYPES = 511;
  var LIMITS = {
    bytes: 256 * 1024,
    rules: 2000,
    line: 512,
    list: 16,
    off: 500,
    labels: 24,
    errors: 100
  };

  var VERBS = {
    block: { kinds: { host: 1, label: 1, path: 1 }, quals: { kind: 1, party: 1, only: 1, except: 1 } },
    allow: { kinds: { host: 1, label: 1, path: 1 }, quals: { kind: 1, party: 1, only: 1, except: 1 } },
    hide: { kinds: { css: 1 }, quals: { only: 1, except: 1 } },
    off: { kinds: { site: 1 }, quals: {} }
  };
  var KINDS = { host: 1, label: 1, path: 1, css: 1, site: 1 };
  var QUALIFIER_WORDS = { kind: 1, party: 1, only: 1, except: 1 };
  var PARTIES = { any: 0, first: 1, third: 2 };

  function has(obj, key) {
    return Object.prototype.hasOwnProperty.call(obj, key);
  }

  // ---- lexical rules (section 3.3) --------------------------------------------------------------

  var HOST_LABEL = /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/;

  function validHost(v) {
    if (v.length < 4 || v.length > 253) return false;
    var labels = v.split(".");
    if (labels.length < 2) return false;
    for (var i = 0; i < labels.length; i++) if (!HOST_LABEL.test(labels[i])) return false;
    return true;
  }

  function labelProblem(v) {
    if (v.length === 0 || !/^[a-z0-9-]+$/.test(v)) return ["bad-label", "a label may only use a-z, 0-9 and -"];
    if (v.length < 4) return ["label-too-short", "a label must be at least 4 characters"];
    if (v.length > 40) return ["bad-label", "a label can be at most 40 characters"];
    if (v.charAt(0) === "-" || v.charAt(v.length - 1) === "-") {
      return ["bad-label", "a label cannot start or end with -"];
    }
    if (/^[0-9]+$/.test(v)) return ["label-all-digits", "a label cannot be all digits"];
    if (!/[a-z]/.test(v)) return ["bad-label", "a label needs at least one letter"];
    return null;
  }

  function pathProblem(v) {
    if (v.charAt(0) !== "/") return ["path-no-slash", "a path must start with /"];
    if (v.length < 3 || v.length > 200) return ["bad-path", "a path must be 3 to 200 characters"];
    if (/[^\x21-\x7e]|[#"\\]/.test(v)) {
      return ["bad-path", "a path may not contain spaces, #, \" or \\ or non-ASCII characters"];
    }
    return null;
  }

  function cssProblem(v) {
    if (v.indexOf(",") >= 0) return ["css-comma", "a selector cannot contain a comma; use one rule per selector"];
    if (/[{};]/.test(v)) return ["bad-css", "a selector cannot contain { } or ;"];
    if (!/[#.\[]/.test(v)) return ["css-too-broad", "selector must be narrowed by an id, class or attribute"];
    if (v.length < 3 || v.length > 200) return ["css-length", "a selector must be 3 to 200 characters"];
    if (/[\x00-\x20\x7f]/.test(v)) return ["bad-css", "a selector cannot contain spaces or control characters"];
    return null;
  }

  // ---- the lexer and parser (sections 3.1 and 4.7) ----------------------------------------------

  function utf8Length(s) {
    var n = 0;
    for (var i = 0; i < s.length; i++) {
      var c = s.charCodeAt(i);
      if (c < 0x80) n += 1;
      else if (c < 0x800) n += 2;
      else if (c >= 0xd800 && c <= 0xdbff && i + 1 < s.length) {
        var d = s.charCodeAt(i + 1);
        if (d >= 0xdc00 && d <= 0xdfff) {
          n += 4;
          i++;
        } else n += 3;
      } else n += 3;
    }
    return n;
  }

  function tokenize(line) {
    var toks = [];
    var i = 0;
    var n = line.length;
    while (i < n) {
      while (i < n && line.charCodeAt(i) === 32) i++;
      if (i >= n) break;
      var start = i;
      while (i < n && line.charCodeAt(i) !== 32) i++;
      toks.push({ text: line.slice(start, i), col: start + 1 });
    }
    return toks;
  }

  function isBlankOrComment(line) {
    var i = 0;
    while (i < line.length && line.charCodeAt(i) === 32) i++;
    return i === line.length || line.charAt(i) === "#";
  }

  function bad(code, col, message) {
    return { err: { code: code, column: col, message: message } };
  }

  function parseHostList(tok, what) {
    var parts = tok.text.split(",");
    if (parts.length > LIMITS.list) {
      return bad("list-too-long", tok.col, what + " takes at most " + LIMITS.list + " hosts");
    }
    for (var i = 0; i < parts.length; i++) {
      if (!validHost(parts[i])) return bad("bad-host", tok.col, "not a valid host in " + what + ": " + parts[i]);
    }
    return { value: parts };
  }

  function parseRule(line) {
    var toks = tokenize(line);
    var verbTok = toks[0];
    if (!has(VERBS, verbTok.text)) {
      return bad("bad-verb", verbTok.col, "expected block, allow, hide or off at the start of the line");
    }
    var verb = verbTok.text;
    var spec = VERBS[verb];
    var targetTok = toks[1];
    if (!targetTok || targetTok.text.indexOf(":") < 0) {
      return bad(
        "bad-target",
        targetTok ? targetTok.col : verbTok.col + verb.length,
        "expected kind:value after " + verb + ", such as host:ads.example"
      );
    }
    var colon = targetTok.text.indexOf(":");
    var kind = targetTok.text.slice(0, colon);
    var value = targetTok.text.slice(colon + 1);
    if (!has(KINDS, kind)) {
      return bad("bad-kind", targetTok.col, "unknown kind \"" + kind + "\"; use host, label, path, css or site");
    }
    if (!has(spec.kinds, kind)) {
      return bad("kind-not-allowed", targetTok.col, verb + " does not take " + kind + ":");
    }

    var problem = null;
    if (kind === "host") {
      if (!validHost(value)) problem = ["bad-host", "not a valid host: " + value];
    } else if (kind === "site") {
      if (!validHost(value)) problem = ["bad-site", "not a valid site: " + value];
    } else if (kind === "label") problem = labelProblem(value);
    else if (kind === "path") problem = pathProblem(value);
    else if (kind === "css") problem = cssProblem(value);
    if (problem) return bad(problem[0], targetTok.col + colon + 1, problem[1]);

    var rule = {
      i: -1,
      line: 0,
      verb: verb,
      kind: kind,
      value: value,
      types: ALL_TYPES,
      party: verb === "block" || verb === "allow" ? 2 : 0,
      only: null,
      except: null,
      list: ""
    };

    var seen = {};
    var k = 2;
    while (k < toks.length) {
      var word = toks[k];
      if (!has(QUALIFIER_WORDS, word.text)) {
        if (kind === "css") {
          return bad("css-space", word.col, "a selector cannot contain spaces; \"" + word.text + "\" is not a qualifier");
        }
        return bad("bad-qualifier", word.col, "unknown qualifier \"" + word.text + "\"");
      }
      if (!has(spec.quals, word.text)) {
        return bad("qualifier-not-allowed", word.col, verb + " does not take \"" + word.text + "\"");
      }
      if (has(seen, word.text)) {
        return bad("duplicate-qualifier", word.col, "\"" + word.text + "\" appears more than once");
      }
      seen[word.text] = true;
      var arg = toks[k + 1];
      if (!arg) {
        return bad("qualifier-no-value", word.col + word.text.length, "\"" + word.text + "\" needs a value");
      }
      if (word.text === "kind") {
        var names = arg.text.split(",");
        var mask = 0;
        for (var t = 0; t < names.length; t++) {
          var bit = TYPE_NAMES.indexOf(names[t]);
          if (bit < 0) return bad("bad-type", arg.col, "unknown request kind \"" + names[t] + "\"");
          mask |= 1 << bit;
        }
        rule.types = mask;
      } else if (word.text === "party") {
        if (!has(PARTIES, arg.text)) return bad("bad-party", arg.col, "party must be first, third or any");
        rule.party = PARTIES[arg.text];
      } else {
        var list = parseHostList(arg, word.text);
        if (list.err) return list;
        rule[word.text] = list.value;
      }
      k += 2;
    }

    return {
      rule: rule,
      key: toks
        .map(function (t) {
          return t.text;
        })
        .join(" ")
    };
  }

  function parseList(src, tag) {
    var out = { tag: tag, rules: [], errors: [], warnings: [] };
    var seen = {};
    var stopped = false;
    var offCount = 0;
    var bytes = 0;
    var lines = src.split("\n");

    function fatal(code, lineNo, message) {
      out.errors.push({ line: lineNo, column: 1, code: code, message: message, list: tag });
      stopped = true;
    }

    for (var k = 0; k < lines.length && !stopped; k++) {
      var lineNo = k + 1;
      var line = lines[k];
      bytes += utf8Length(line) + (k > 0 ? 1 : 0);
      if (bytes > LIMITS.bytes) {
        fatal("list-too-large", lineNo, "list is over the 256 KiB limit at line " + lineNo);
        break;
      }
      if (line.charCodeAt(line.length - 1) === 13) line = line.slice(0, -1);
      if (isBlankOrComment(line)) continue;

      var res;
      var tab = line.indexOf("\t");
      if (tab >= 0) {
        res = bad("tab", tab + 1, "tabs are not allowed; separate words with spaces");
      } else if (line.length > LIMITS.line) {
        res = bad("line-too-long", LIMITS.line + 1, "a line can be at most " + LIMITS.line + " characters");
      } else if (line.charCodeAt(0) === 32) {
        res = bad("leading-space", 1, "a rule must start in the first column");
      } else {
        res = parseRule(line);
      }

      if (res.err) {
        if (out.errors.length >= LIMITS.errors) {
          fatal("too-many-errors", lineNo, "stopped after " + LIMITS.errors + " errors, at line " + lineNo);
        } else {
          out.errors.push({
            line: lineNo,
            column: res.err.column,
            code: res.err.code,
            message: res.err.message,
            list: tag
          });
        }
        continue;
      }

      if (has(seen, res.key)) {
        out.warnings.push({
          line: lineNo,
          column: 1,
          code: "duplicate-rule",
          message: "same as line " + seen[res.key] + "; ignored",
          list: tag
        });
        continue;
      }
      if (out.rules.length >= LIMITS.rules) {
        fatal("too-many-rules", lineNo, "list has more than " + LIMITS.rules + " rules, passed at line " + lineNo);
        break;
      }
      if (res.rule.verb === "off" && offCount >= LIMITS.off) {
        fatal("too-many-off", lineNo, "list has more than " + LIMITS.off + " off rules, passed at line " + lineNo);
        break;
      }
      seen[res.key] = lineNo;
      if (res.rule.verb === "off") offCount++;
      res.rule.line = lineNo;
      res.rule.list = tag;
      out.rules.push(res.rule);
    }
    return out;
  }

  // ---- compile (section 4.7) --------------------------------------------------------------------

  function fnv1a(str) {
    var h = 0x811c9dc5;
    for (var i = 0; i < str.length; i++) {
      h ^= str.charCodeAt(i);
      h = Math.imul(h, 0x01000193);
    }
    return (h >>> 0).toString(16).padStart(8, "0");
  }

  function normaliseSource(src) {
    return src.replace(/\r\n/g, "\n").replace(/\n+$/, "");
  }

  // ---- SuffixMap: the reversed-label trie (section 4.2) -----------------------------------------

  function suffixInsert(map, host, idx) {
    var labels = host.split(".");
    var node = map;
    for (var i = labels.length - 1; i >= 0; i--) {
      var label = labels[i];
      if (!has(node, label)) node[label] = {};
      node = node[label];
    }
    if (!has(node, "$")) node.$ = [];
    node.$.push(idx);
  }

  // Walks from the last label inward and appends the rule indexes of every terminal passed on the
  // way. Labels are cut from the host string one at a time, so no array of labels is built.
  // `$` is the key that holds rule indexes, so a request label spelled `$` ends the walk.
  function suffixWalk(map, host, out) {
    var node = map;
    var end = host.length;
    while (end >= 0) {
      var dot = end === 0 ? -1 : host.lastIndexOf(".", end - 1);
      var label = host.slice(dot + 1, end);
      if (label === "$" || !has(node, label)) break;
      node = node[label];
      if (has(node, "$")) {
        var ids = node.$;
        for (var j = 0; j < ids.length; j++) out.push(ids[j]);
      }
      if (dot < 0) break;
      end = dot;
    }
    return out;
  }

  var SuffixMap = {
    create: function () {
      return {};
    },
    insert: suffixInsert,
    walk: suffixWalk
  };

  // ---- AC: the Aho-Corasick automaton (section 4.3) ---------------------------------------------

  function acBuild(patterns) {
    var next = [null];
    var fail = [0];
    var out = [null];
    var p, node, i, c, m, to;
    for (p = 0; p < patterns.length; p++) {
      var text = patterns[p][0];
      if (text.length === 0) continue;
      node = 0;
      for (i = 0; i < text.length; i++) {
        c = text.charCodeAt(i);
        m = next[node];
        if (m === null) m = next[node] = new Map();
        to = m.get(c);
        if (to === undefined) {
          to = next.length;
          next.push(null);
          fail.push(0);
          out.push(null);
          m.set(c, to);
        }
        node = to;
      }
      if (out[node] === null) out[node] = [];
      out[node].push(patterns[p][1]);
    }

    // Breadth-first: a node's failure target is always shallower, so its output list is final by the
    // time it is merged into the deeper node.
    var queue = [];
    if (next[0] !== null) {
      next[0].forEach(function (v) {
        queue.push(v);
      });
    }
    for (var qi = 0; qi < queue.length; qi++) {
      var u = queue[qi];
      var edges = next[u];
      if (edges === null) continue;
      edges.forEach(function (v, ch) {
        var f = fail[u];
        while (f !== 0 && (next[f] === null || !next[f].has(ch))) f = fail[f];
        var g = next[f] === null ? undefined : next[f].get(ch);
        fail[v] = g === undefined ? 0 : g;
        var inherited = out[fail[v]];
        if (inherited !== null) out[v] = out[v] === null ? inherited : out[v].concat(inherited);
        queue.push(v);
      });
    }
    return { next: next, fail: fail, out: out, size: next.length };
  }

  // Appends the rule index of every pattern occurrence to `hits`. O(|subject| + hits).
  function acRun(ac, subject, hits) {
    if (ac.size === 1) return hits;
    var next = ac.next;
    var fail = ac.fail;
    var out = ac.out;
    var node = 0;
    for (var i = 0; i < subject.length; i++) {
      var c = subject.charCodeAt(i);
      for (;;) {
        var m = next[node];
        var to = m === null ? undefined : m.get(c);
        if (to !== undefined) {
          node = to;
          break;
        }
        if (node === 0) break;
        node = fail[node];
      }
      var o = out[node];
      if (o !== null) for (var j = 0; j < o.length; j++) hits.push(o[j]);
    }
    return hits;
  }

  var AC = { build: acBuild, run: acRun };

  // ---- indexing a compiled list (section 4.1) ---------------------------------------------------

  function buildIndexes(rules) {
    var block = { hosts: SuffixMap.create(), labels: null, paths: null };
    var allow = { hosts: SuffixMap.create(), labels: null, paths: null };
    var pending = { block: { labels: [], paths: [] }, allow: { labels: [], paths: [] } };
    var siteOff = SuffixMap.create();
    var cosmetic = { global: [], byOnly: new Map(), byExcept: [] };

    for (var n = 0; n < rules.length; n++) {
      var r = rules[n];
      if (r.verb === "block" || r.verb === "allow") {
        var index = r.verb === "block" ? block : allow;
        if (r.kind === "host") suffixInsert(index.hosts, r.value, r.i);
        else if (r.kind === "label") pending[r.verb].labels.push([r.value, r.i]);
        else pending[r.verb].paths.push([r.value.toLowerCase(), r.i]);
      } else if (r.verb === "off") {
        suffixInsert(siteOff, r.value, r.i);
      } else if (r.except) {
        cosmetic.byExcept.push(r.i);
      } else if (r.only) {
        for (var k = 0; k < r.only.length; k++) {
          var list = cosmetic.byOnly.get(r.only[k]);
          if (!list) cosmetic.byOnly.set(r.only[k], (list = []));
          list.push(r.i);
        }
      } else {
        cosmetic.global.push(r.i);
      }
    }
    block.labels = acBuild(pending.block.labels);
    block.paths = acBuild(pending.block.paths);
    allow.labels = acBuild(pending.allow.labels);
    allow.paths = acBuild(pending.allow.paths);
    return { block: block, allow: allow, siteOff: siteOff, cosmetic: cosmetic };
  }

  // sources: [{ tag, src }]. Each list is parsed on its own, under its own limits, then the rules are
  // concatenated in order and indexed together.
  function compileAll(sources) {
    var rules = [];
    var errors = [];
    var warnings = [];
    var hashInput = [];
    var counts = { block: 0, allow: 0, hide: 0, off: 0 };
    for (var s = 0; s < sources.length; s++) {
      var parsed = parseList(sources[s].src, sources[s].tag || "");
      hashInput.push((sources[s].tag || "") + "\u0000" + normaliseSource(sources[s].src));
      for (var r = 0; r < parsed.rules.length; r++) {
        parsed.rules[r].i = rules.length;
        counts[parsed.rules[r].verb]++;
        rules.push(parsed.rules[r]);
      }
      Array.prototype.push.apply(errors, parsed.errors);
      Array.prototype.push.apply(warnings, parsed.warnings);
    }
    var indexes = buildIndexes(rules);
    return {
      version: VERSION,
      sourceHash: fnv1a(hashInput.join("\u0001")),
      rules: rules,
      block: indexes.block,
      allow: indexes.allow,
      siteOff: indexes.siteOff,
      cosmetic: indexes.cosmetic,
      errors: errors,
      warnings: warnings,
      counts: counts
    };
  }

  function compile(src) {
    return compileAll([{ tag: "", src: src }]);
  }

  // ---- request normalisation and party (sections 4.4 and 4.5) -----------------------------------

  function normalise(url) {
    var u;
    try {
      u = new URL(url);
    } catch (e) {
      return null;
    }
    var scheme = u.protocol;
    if (scheme !== "http:" && scheme !== "https:") return null;
    var host = u.hostname.toLowerCase();
    if (host.charAt(host.length - 1) === ".") host = host.slice(0, -1);
    return { scheme: scheme.slice(0, -1), host: host, pathq: (u.pathname + u.search).toLowerCase() };
  }

  function normaliseSite(site) {
    var host = String(site == null ? "" : site).toLowerCase();
    if (host.charAt(host.length - 1) === ".") host = host.slice(0, -1);
    return host;
  }

  // host equals v, or ends with "." + v. The label-boundary suffix test used by host:, site:, only
  // and except.
  function hostMatches(host, v) {
    if (host === v) return true;
    var cut = host.length - v.length - 1;
    return cut >= 0 && host.charCodeAt(cut) === 46 && host.endsWith(v);
  }

  function sameParty(a, b) {
    return hostMatches(a, b) || hostMatches(b, a);
  }

  function labelCount(host) {
    var n = 1;
    for (var i = host.indexOf("."); i >= 0; i = host.indexOf(".", i + 1)) n++;
    return n;
  }

  // ---- cosmetic data (section 6.1) --------------------------------------------------------------

  // Plain data for chrome.storage: { global: [sel], byOnly: { host: [sel] }, byExcept: [{ sel, except, only? }] }.
  // A rule that carries an except list goes in byExcept whatever else it carries, with its only list
  // alongside, so that except-before-only still holds when the content script applies it.
  function buildCosmetic(compiled) {
    var rules = compiled.rules;
    var cosmetic = compiled.cosmetic;
    var seen = Object.create(null);
    var data = { global: [], byOnly: {}, byExcept: [] };
    var i, sel;

    for (i = 0; i < cosmetic.global.length; i++) {
      sel = rules[cosmetic.global[i]].value;
      if (seen[sel]) continue;
      seen[sel] = true;
      data.global.push(sel);
    }
    cosmetic.byOnly.forEach(function (indexes, host) {
      var list = [];
      for (var k = 0; k < indexes.length; k++) {
        var s = rules[indexes[k]].value;
        if (list.indexOf(s) < 0) list.push(s);
      }
      data.byOnly[host] = list;
    });
    for (i = 0; i < cosmetic.byExcept.length; i++) {
      var rule = rules[cosmetic.byExcept[i]];
      var entry = { sel: rule.value, except: rule.except.slice() };
      if (rule.only) entry.only = rule.only.slice();
      data.byExcept.push(entry);
    }
    return data;
  }

  // The selectors that apply on a top-level site. Except is tested before only.
  function cosmeticFor(data, siteHost) {
    var out = [];
    if (!data) return out;
    var site = normaliseSite(siteHost);
    var seen = Object.create(null);
    function add(sel) {
      if (seen[sel]) return;
      seen[sel] = true;
      out.push(sel);
    }
    var i, k;
    var global = data.global || [];
    for (i = 0; i < global.length; i++) add(global[i]);

    var byOnly = data.byOnly || {};
    var hosts = Object.keys(byOnly);
    for (i = 0; i < hosts.length; i++) {
      if (!hostMatches(site, hosts[i])) continue;
      for (k = 0; k < byOnly[hosts[i]].length; k++) add(byOnly[hosts[i]][k]);
    }

    var byExcept = data.byExcept || [];
    for (i = 0; i < byExcept.length; i++) {
      var entry = byExcept[i];
      var skip = false;
      for (k = 0; k < entry.except.length; k++) {
        if (hostMatches(site, entry.except[k])) {
          skip = true;
          break;
        }
      }
      if (skip) continue;
      if (entry.only) {
        var found = false;
        for (k = 0; k < entry.only.length; k++) {
          if (hostMatches(site, entry.only[k])) {
            found = true;
            break;
          }
        }
        if (!found) continue;
      }
      add(entry.sel);
    }
    return out;
  }

  // ---- the decision procedure (section 4.6) -----------------------------------------------------

  var scratch = [];

  function applies(rule, typeBit, isFirst, site) {
    if (((rule.types >> typeBit) & 1) === 0) return false;
    if (rule.party === 1 && !isFirst) return false;
    if (rule.party === 2 && isFirst) return false;
    var i;
    var except = rule.except;
    if (except !== null) {
      for (i = 0; i < except.length; i++) if (hostMatches(site, except[i])) return false;
    }
    var only = rule.only;
    if (only !== null) {
      var found = false;
      for (i = 0; i < only.length; i++) {
        if (hostMatches(site, only[i])) {
          found = true;
          break;
        }
      }
      if (!found) return false;
    }
    return true;
  }

  // Returns the lowest rule index in the index that matches the request, or -1.
  function query(index, rules, host, pathq, typeBit, isFirst, site) {
    scratch.length = 0;
    suffixWalk(index.hosts, host, scratch);
    acRun(index.labels, host, scratch);
    acRun(index.paths, pathq, scratch);
    var best = -1;
    for (var i = 0; i < scratch.length; i++) {
      var idx = scratch[i];
      if (best !== -1 && idx >= best) continue;
      if (applies(rules[idx], typeBit, isFirst, site)) best = idx;
    }
    return best;
  }

  // True when an off rule covers the top-level site (the same walk decide() does first).
  function siteIsOff(compiled, siteHost) {
    var site = normaliseSite(siteHost);
    if (site === "" || labelCount(site) > LIMITS.labels) return false;
    scratch.length = 0;
    suffixWalk(compiled.siteOff, site, scratch);
    return scratch.length > 0;
  }

  function verdictFor(verdict, idx, rules) {
    if (idx < 0) return { verdict: "pass", ruleIndex: -1, line: 0, list: "" };
    return { verdict: verdict, ruleIndex: idx, line: rules[idx].line, list: rules[idx].list };
  }

  // req: { url, siteHost, type } where type is one of TYPE_NAMES or its bit number.
  function decide(compiled, req) {
    var rules = compiled.rules;
    var n = normalise(req.url);
    if (n === null) return verdictFor("pass", -1, rules);
    var site = normaliseSite(req.siteHost);
    if (labelCount(n.host) > LIMITS.labels || labelCount(site) > LIMITS.labels) {
      return verdictFor("pass", -1, rules);
    }

    if (compiled.counts.off > 0) {
      scratch.length = 0;
      suffixWalk(compiled.siteOff, site, scratch);
      if (scratch.length > 0) {
        var off = scratch[0];
        for (var s = 1; s < scratch.length; s++) if (scratch[s] < off) off = scratch[s];
        return verdictFor("off", off, rules);
      }
    }

    var typeBit = typeof req.type === "number" ? req.type : TYPE_NAMES.indexOf(req.type);
    if (typeBit < 0 || typeBit > 8) typeBit = 8;
    var isFirst = sameParty(n.host, site);

    var hit = compiled.counts.allow > 0 ? query(compiled.allow, rules, n.host, n.pathq, typeBit, isFirst, site) : -1;
    if (hit >= 0) return verdictFor("allow", hit, rules);
    hit = query(compiled.block, rules, n.host, n.pathq, typeBit, isFirst, site);
    if (hit >= 0) return verdictFor("block", hit, rules);
    return verdictFor("pass", -1, rules);
  }

  return {
    VERSION: VERSION,
    TYPE_NAMES: TYPE_NAMES,
    ALL_TYPES: ALL_TYPES,
    LIMITS: LIMITS,
    compile: compile,
    compileAll: compileAll,
    decide: decide,
    normalise: normalise,
    hostMatches: hostMatches,
    sameParty: sameParty,
    isValidSite: validHost,
    siteIsOff: siteIsOff,
    buildCosmetic: buildCosmetic,
    cosmeticFor: cosmeticFor,
    SuffixMap: SuffixMap,
    AC: AC
  };
})();

if (typeof module === "object" && module.exports) module.exports = ShieldCore; // Node tests
if (typeof self === "object") self.ShieldCore = self.ShieldCore || ShieldCore; // SW + content
if (typeof window === "object") window.ShieldCore = ShieldCore; // popup/options
