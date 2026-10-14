"use strict";
// Bowser Shield engine tests. `node --test extensions/bowser-shield/test/run-tests.js`
// Node 18+, node:test and node:assert only. The tests load the shipped files in lib/.

const { test, describe } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const ROOT = path.join(__dirname, "..");
const ShieldCore = require("../lib/shield-core.js");
const reference = require("./reference-matcher.js");

function readCases(name) {
  return JSON.parse(fs.readFileSync(path.join(__dirname, "cases", name), "utf8"));
}

function pick(actual, expected) {
  const out = {};
  for (const key of Object.keys(expected)) out[key] = actual[key];
  return out;
}

// ---------------------------------------------------------------------------
// 9.1 Parser
// ---------------------------------------------------------------------------

describe("parser: table cases", () => {
  for (const c of readCases("parser.json")) {
    test(c.name, () => {
      const compiled = ShieldCore.compile(c.src);
      const expectedErrors = c.expect.errors || [];
      assert.equal(compiled.errors.length, expectedErrors.length, "error count");
      assert.deepEqual(
        compiled.errors.map((e, i) => pick(e, expectedErrors[i] || {})),
        expectedErrors,
        "errors"
      );
      if (c.expect.rules) {
        assert.equal(compiled.rules.length, c.expect.rules.length, "rule count");
        c.expect.rules.forEach((want, i) => {
          assert.deepEqual(pick(compiled.rules[i], want), want, "rule " + i);
        });
      }
      if (c.expect.warnings) {
        assert.deepEqual(
          compiled.warnings.map((w, i) => pick(w, c.expect.warnings[i] || {})),
          c.expect.warnings
        );
      }
    });
  }
});

describe("parser: limits and generated cases", () => {
  test("a 17-entry only list is rejected as list-too-long and 16 is accepted", () => {
    const hosts = (n) => Array.from({ length: n }, (_, i) => "h" + i + ".example").join(",");
    const bad = ShieldCore.compile("block host:a.example only " + hosts(17));
    assert.equal(bad.rules.length, 0);
    assert.deepEqual(bad.errors.map((e) => [e.line, e.code]), [[1, "list-too-long"]]);
    const ok = ShieldCore.compile("block host:a.example only " + hosts(16));
    assert.equal(ok.errors.length, 0);
    assert.equal(ok.rules[0].only.length, 16);
  });

  test("a 17-entry except list is rejected as list-too-long", () => {
    const hosts = Array.from({ length: 17 }, (_, i) => "h" + i + ".example").join(",");
    const bad = ShieldCore.compile("hide css:div.a except " + hosts);
    assert.deepEqual(bad.errors.map((e) => e.code), ["list-too-long"]);
  });

  test("a 513-character line is rejected as line-too-long and 512 is accepted", () => {
    const long = "block label:" + "a".repeat(501);
    assert.equal(long.length, 513);
    const bad = ShieldCore.compile(long);
    assert.deepEqual(bad.errors.map((e) => [e.line, e.code]), [[1, "line-too-long"]]);
    const fits = ShieldCore.compile("block label:" + "a".repeat(40) + " only " + "b".repeat(100) + ".example");
    assert.ok(fits.errors.every((e) => e.code !== "line-too-long"));
  });

  test("a 513-character line does not stop other lines compiling", () => {
    const src = ["block host:a.example", "block label:" + "a".repeat(501), "block host:b.example"].join("\n");
    const c = ShieldCore.compile(src);
    assert.equal(c.rules.length, 2);
    assert.equal(c.errors.length, 1);
  });

  test("2000 rules compile and the 2001st stops the compile with too-many-rules", () => {
    const lines = Array.from({ length: 2001 }, (_, i) => "block host:h" + i + ".example");
    const c = ShieldCore.compile(lines.join("\n"));
    assert.equal(c.rules.length, 2000);
    assert.deepEqual(c.errors.map((e) => [e.line, e.code]), [[2001, "too-many-rules"]]);
    const exact = ShieldCore.compile(lines.slice(0, 2000).join("\n"));
    assert.equal(exact.errors.length, 0);
  });

  test("a hard limit stops the compile: lines after it are not read", () => {
    const lines = Array.from({ length: 2001 }, (_, i) => "block host:h" + i + ".example");
    lines.push("block host:nodots");
    const c = ShieldCore.compile(lines.join("\n"));
    assert.deepEqual(c.errors.map((e) => e.code), ["too-many-rules"]);
  });

  test("duplicate lines do not count toward the 2000 rule limit", () => {
    const lines = Array.from({ length: 2000 }, (_, i) => "block host:h" + i + ".example");
    lines.push("block host:h0.example");
    const c = ShieldCore.compile(lines.join("\n"));
    assert.equal(c.errors.length, 0);
    assert.equal(c.warnings.length, 1);
  });

  test("501 off rules stop the compile with too-many-off", () => {
    const lines = Array.from({ length: 501 }, (_, i) => "off site:s" + i + ".example");
    const c = ShieldCore.compile(lines.join("\n"));
    assert.equal(c.counts.off, 500);
    assert.deepEqual(c.errors.map((e) => [e.line, e.code]), [[501, "too-many-off"]]);
  });

  test("list text over 256 KiB stops the compile at the line that passes it", () => {
    // Line 1 is 20 bytes, every other line is 100 bytes, each followed by a newline.
    // Bytes through line k: 21 + (k - 2) * 101 + 100. That is 262115 at k = 2596 and 262216 at k = 2597.
    const line = "# " + "x".repeat(98);
    const lines = Array.from({ length: 3000 }, () => line);
    lines[0] = "block host:a.example";
    const c = ShieldCore.compile(lines.join("\n"));
    assert.deepEqual(c.errors.map((e) => [e.line, e.code]), [[2597, "list-too-large"]]);
    assert.equal(c.rules.length, 1);
  });

  test("256 KiB of comments exactly is accepted", () => {
    const src = "#".repeat(262144);
    const c = ShieldCore.compile(src);
    assert.equal(c.errors.length, 0);
  });

  test("the size limit counts UTF-8 bytes, not characters", () => {
    const src = "#" + "é".repeat(131072);
    assert.equal(src.length, 131073);
    const c = ShieldCore.compile(src);
    assert.deepEqual(c.errors.map((e) => e.code), ["list-too-large"]);
  });

  test("giving up after 100 recorded errors", () => {
    const lines = Array.from({ length: 150 }, () => "bogus");
    const c = ShieldCore.compile(lines.join("\n"));
    assert.equal(c.errors.length, 101);
    assert.ok(c.errors.slice(0, 100).every((e) => e.code === "bad-verb"));
    assert.deepEqual([c.errors[100].line, c.errors[100].code], [101, "too-many-errors"]);
  });

  test("exactly 100 bad lines are all recorded and the compile carries on", () => {
    const lines = Array.from({ length: 100 }, () => "bogus");
    lines.push("block host:a.example");
    const c = ShieldCore.compile(lines.join("\n"));
    assert.equal(c.errors.length, 100);
    assert.equal(c.rules.length, 1);
  });

  test("errors carry line, column, code and a readable message", () => {
    const c = ShieldCore.compile("block host:a.example\nblock domain:x.example");
    assert.equal(c.errors.length, 1);
    const e = c.errors[0];
    assert.equal(e.line, 2);
    assert.equal(e.column, 7);
    assert.equal(e.code, "bad-kind");
    assert.equal(typeof e.message, "string");
    assert.ok(e.message.length > 5);
  });

  test("the css-too-broad message tells the user how to fix it", () => {
    const c = ShieldCore.compile("hide css:div");
    assert.equal(c.errors[0].message, "selector must be narrowed by an id, class or attribute");
  });

  test("counts are reported per verb", () => {
    const c = ShieldCore.compile(
      "block host:a.example\nblock label:adserver\nallow host:b.example\nhide css:div.a\nhide css:div.b\noff site:c.example"
    );
    assert.deepEqual(c.counts, { block: 2, allow: 1, hide: 2, off: 1 });
  });

  test("rules are indexed in file order", () => {
    const c = ShieldCore.compile("# c\nblock host:a.example\n\nblock host:b.example");
    assert.deepEqual(c.rules.map((r) => [r.i, r.line]), [[0, 2], [1, 4]]);
  });

  test("compiling the same text twice gives the same sourceHash, and CRLF does not change it", () => {
    const a = ShieldCore.compile("block host:a.example\nblock host:b.example\n");
    const b = ShieldCore.compile("block host:a.example\r\nblock host:b.example\r\n");
    const c = ShieldCore.compile("block host:a.example\nblock host:c.example\n");
    assert.match(a.sourceHash, /^[0-9a-f]{8}$/);
    assert.equal(a.sourceHash, b.sourceHash);
    assert.notEqual(a.sourceHash, c.sourceHash);
  });

  test("the example user list from section 3.8 compiles with no errors", () => {
    const src = [
      "# My list.",
      "",
      "# Block an ad host and everything under it, including its own first-party requests.",
      "block host:ads.shop.example party any",
      "",
      "# A measurement endpoint, but only scripts, and not on the site that pays for my work.",
      "block path:/metrics/collect kind script except client.example",
      "",
      "# A tracker that one site genuinely needs.",
      "block label:visitor-track",
      "allow label:visitor-track only intranet.example",
      "",
      "# Hide a leftover container the block leaves behind.",
      "hide css:div[data-slot-state=\"empty\"] only news.example",
      "",
      "# My bank, untouched.",
      "off site:bank.example",
      ""
    ].join("\n");
    const c = ShieldCore.compile(src);
    assert.deepEqual(c.errors, []);
    assert.deepEqual(c.counts, { block: 3, allow: 1, hide: 1, off: 1 });
  });
});

// ---------------------------------------------------------------------------
// 4.2 and 4.3: the two data structures on their own
// ---------------------------------------------------------------------------

describe("SuffixMap", () => {
  const { SuffixMap } = ShieldCore;

  test("stores reversed labels as nested plain objects with rule indexes under $", () => {
    const map = SuffixMap.create();
    SuffixMap.insert(map, "ad-delivery.example", 0);
    SuffixMap.insert(map, "cdn.ad-delivery.example", 1);
    assert.deepEqual(JSON.parse(JSON.stringify(map)), { example: { "ad-delivery": { $: [0], cdn: { $: [1] } } } });
  });

  test("walks from the last label inward and collects every terminal on the way", () => {
    const map = SuffixMap.create();
    SuffixMap.insert(map, "ad-delivery.example", 0);
    SuffixMap.insert(map, "cdn.ad-delivery.example", 1);
    assert.deepEqual(SuffixMap.walk(map, "img.cdn.ad-delivery.example", []), [0, 1]);
    assert.deepEqual(SuffixMap.walk(map, "ad-delivery.example", []), [0]);
    assert.deepEqual(SuffixMap.walk(map, "x.ad-delivery.example", []), [0]);
  });

  test("stops on a label boundary, so a longer first label collects nothing", () => {
    const map = SuffixMap.create();
    SuffixMap.insert(map, "ad-delivery.example", 0);
    assert.deepEqual(SuffixMap.walk(map, "notad-delivery.example", []), []);
    assert.deepEqual(SuffixMap.walk(map, "ad-delivery.example.co", []), []);
    assert.deepEqual(SuffixMap.walk(map, "example", []), []);
  });

  test("keeps several rule indexes that end at the same node", () => {
    const map = SuffixMap.create();
    SuffixMap.insert(map, "a.example", 3);
    SuffixMap.insert(map, "a.example", 7);
    assert.deepEqual(SuffixMap.walk(map, "a.example", []), [3, 7]);
  });

  test("is not fooled by labels named like Object.prototype members", () => {
    const map = SuffixMap.create();
    SuffixMap.insert(map, "a.example", 0);
    for (const host of ["constructor.example", "__proto__.example", "toString.a.example", "$.a.example", "hasOwnProperty"]) {
      assert.doesNotThrow(() => SuffixMap.walk(map, host, []));
    }
    assert.deepEqual(SuffixMap.walk(map, "constructor.example", []), []);
    assert.deepEqual(SuffixMap.walk(map, "__proto__.a.example", []), [0]);
  });
});

describe("Aho-Corasick", () => {
  const { AC } = ShieldCore;
  const hits = (patterns, subject) => AC.run(AC.build(patterns), subject, []).sort((a, b) => a - b);

  test("finds the classic overlapping set through failure links", () => {
    const patterns = [["he", 0], ["she", 1], ["his", 2], ["hers", 3]];
    assert.deepEqual(hits(patterns, "ushers"), [0, 1, 3]);
  });

  test("reports every occurrence, not just the first", () => {
    assert.deepEqual(hits([["a", 0], ["aa", 1]], "aaaa"), [0, 0, 0, 0, 1, 1, 1]);
  });

  test("reports each rule that shares one pattern text", () => {
    assert.deepEqual(hits([["track", 4], ["track", 9]], "xtrackx"), [4, 9]);
  });

  test("reports a pattern that is a suffix of another when the longer one fails halfway", () => {
    assert.deepEqual(hits([["abcd", 0], ["bce", 1], ["ce", 2]], "abce"), [1, 2]);
  });

  test("returns nothing for an empty automaton or an empty subject", () => {
    assert.deepEqual(hits([], "anything"), []);
    assert.deepEqual(hits([["abc", 0]], ""), []);
    assert.deepEqual(hits([["abc", 0]], "ab"), []);
  });
});

// ---------------------------------------------------------------------------
// 4.1: the compiled shape
// ---------------------------------------------------------------------------

describe("compiled shape", () => {
  test("buckets rules into block, allow, siteOff and cosmetic", () => {
    const c = ShieldCore.compile(
      [
        "hide css:div.a",
        "hide css:div.b only x.example,y.example",
        "hide css:div.c except z.example",
        "block host:ad-delivery.example",
        "block host:cdn.ad-delivery.example",
        "block label:adserver",
        "block path:/ad-slot/",
        "allow host:ok.example",
        "off site:bank.example"
      ].join("\n")
    );
    assert.equal(c.version, 1);
    assert.deepEqual(JSON.parse(JSON.stringify(c.block.hosts)), {
      example: { "ad-delivery": { $: [3], cdn: { $: [4] } } }
    });
    assert.deepEqual(JSON.parse(JSON.stringify(c.allow.hosts)), { example: { ok: { $: [7] } } });
    assert.deepEqual(JSON.parse(JSON.stringify(c.siteOff)), { example: { bank: { $: [8] } } });
    assert.deepEqual(c.cosmetic.global, [0]);
    assert.deepEqual(Array.from(c.cosmetic.byOnly.entries()), [["x.example", [1]], ["y.example", [1]]]);
    assert.deepEqual(c.cosmetic.byExcept, [2]);
    assert.ok(c.block.labels && c.block.paths && c.allow.labels && c.allow.paths);
  });
});

// ---------------------------------------------------------------------------
// 9.2 Matcher
// ---------------------------------------------------------------------------

describe("matcher: table cases", () => {
  for (const c of readCases("matcher.json")) {
    test(c.name, () => {
      const compiled = ShieldCore.compile(c.rules.join("\n"));
      const req = { url: c.url, siteHost: c.site, type: c.type };
      const got = ShieldCore.decide(compiled, req);
      assert.equal(got.verdict, c.expect, "verdict");
      if (c.line !== undefined) assert.equal(got.line, c.line, "line");
      const oracle = reference.decide(compiled.rules, req);
      assert.equal(oracle.verdict, c.expect, "the reference matcher agrees with the table");
      if (c.line !== undefined) assert.equal(oracle.line, c.line, "the reference matcher agrees on the line");
    });
  }

  test("a pass decision has no rule", () => {
    const compiled = ShieldCore.compile("block host:a.example");
    const d = ShieldCore.decide(compiled, { url: "https://b.example/", siteHost: "s.example", type: "script" });
    assert.deepEqual({ verdict: d.verdict, ruleIndex: d.ruleIndex, line: d.line }, { verdict: "pass", ruleIndex: -1, line: 0 });
  });

  test("a block decision names the rule index and the file line", () => {
    const compiled = ShieldCore.compile("# c\n\nblock host:a.example");
    const d = ShieldCore.decide(compiled, { url: "https://a.example/", siteHost: "s.example", type: "script" });
    assert.deepEqual({ verdict: d.verdict, ruleIndex: d.ruleIndex, line: d.line }, { verdict: "block", ruleIndex: 0, line: 3 });
  });

  test("the type may be given as a name or as its bit number", () => {
    const compiled = ShieldCore.compile("block host:a.example kind image");
    const url = "https://a.example/";
    assert.equal(ShieldCore.decide(compiled, { url, siteHost: "s.example", type: "image" }).verdict, "block");
    assert.equal(ShieldCore.decide(compiled, { url, siteHost: "s.example", type: 1 }).verdict, "block");
    assert.equal(ShieldCore.decide(compiled, { url, siteHost: "s.example", type: 0 }).verdict, "pass");
  });

  test("the site host is normalised: case and a trailing dot are ignored", () => {
    const compiled = ShieldCore.compile("off site:s.example");
    const d = ShieldCore.decide(compiled, { url: "https://a.example/", siteHost: "WWW.S.Example.", type: "script" });
    assert.equal(d.verdict, "off");
  });

  test("rules from several lists decide together; the earlier list wins a tie", () => {
    const c = ShieldCore.compileAll([
      { tag: "starter", src: "block host:a.example" },
      { tag: "user", src: "block host:a.example kind script\nblock host:b.example" }
    ]);
    const req = { url: "https://a.example/", siteHost: "s.example", type: "script" };
    const d = ShieldCore.decide(c, req);
    assert.deepEqual([d.verdict, d.ruleIndex, d.line, d.list], ["block", 0, 1, "starter"]);
    const e = ShieldCore.decide(c, { url: "https://b.example/", siteHost: "s.example", type: "script" });
    assert.deepEqual([e.verdict, e.ruleIndex, e.line, e.list], ["block", 2, 2, "user"]);
  });
});

// ---------------------------------------------------------------------------
// 9.3 Differential test against the naive oracle
// ---------------------------------------------------------------------------

function mulberry32(seed) {
  let a = seed >>> 0;
  return function () {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const GEN = {
  hostWords: ["adserver", "track", "click", "news", "shop", "cdn", "img", "ad-x", "a1", "b2", "www", "api", "serve-x"],
  labelWords: ["adse", "erve", "adserver", "track", "rack", "click", "lick", "pixe", "news", "shop", "bann", "ad-x1", "x-ad1", "serve", "cdn1", "erve-x", "rver"],
  tlds: ["example", "test", "net", "co"],
  pathWords: ["ad", "track", "ad-slot", "a", "b", "x", "pix", "collect", "AdBanner", "id=1", "Track"],
  sites: ["news.test", "shop.example", "www.shop.example", "a.cdn.test", "cdn.test", "example.net", "shop.example.net", "track.example"],
  types: ["script", "image", "frame", "xhr", "media", "font", "style", "ping", "other"]
};

function makeGen(rand) {
  const pick = (arr) => arr[Math.floor(rand() * arr.length)];
  const chance = (p) => rand() < p;
  const hostValue = () => {
    const labels = [];
    const n = 1 + Math.floor(rand() * 2);
    for (let i = 0; i < n; i++) labels.push(pick(GEN.hostWords));
    labels.push(pick(GEN.tlds));
    return labels.join(".");
  };
  const pathValue = () => {
    const segs = [];
    const n = 1 + Math.floor(rand() * 2);
    for (let i = 0; i < n; i++) segs.push(pick(GEN.pathWords));
    return "/" + segs.join("/") + (chance(0.5) ? "/" : "");
  };
  const siteList = () => {
    const n = 1 + Math.floor(rand() * 3);
    const out = [];
    for (let i = 0; i < n; i++) out.push(pick(GEN.sites));
    return out.join(",");
  };
  const quals = () => {
    const parts = [];
    if (chance(0.35)) {
      const n = 1 + Math.floor(rand() * 3);
      const names = [];
      for (let i = 0; i < n; i++) names.push(pick(GEN.types));
      parts.push("kind " + names.join(","));
    }
    if (chance(0.4)) parts.push("party " + pick(["first", "third", "any"]));
    if (chance(0.2)) parts.push("only " + siteList());
    if (chance(0.2)) parts.push("except " + siteList());
    for (let i = parts.length - 1; i > 0; i--) {
      const j = Math.floor(rand() * (i + 1));
      [parts[i], parts[j]] = [parts[j], parts[i]];
    }
    return parts.join(" ");
  };
  const ruleLine = () => {
    const r = rand();
    if (r < 0.1) return "off site:" + pick(GEN.sites);
    if (r < 0.2) return "hide css:div.promo" + Math.floor(rand() * 5) + (chance(0.3) ? " only " + siteList() : "");
    const verb = r < 0.6 ? "block" : "allow";
    const k = rand();
    const target = k < 0.4 ? "host:" + hostValue() : k < 0.75 ? "label:" + pick(GEN.labelWords) : "path:" + pathValue();
    return (verb + " " + target + " " + quals()).trim();
  };
  const ruleList = () => {
    const n = Math.floor(rand() * 301);
    const lines = [];
    for (let i = 0; i < n; i++) {
      const r = rand();
      lines.push(r < 0.03 ? "" : r < 0.05 ? "# note" : r < 0.07 ? "block host:nodots" : ruleLine());
    }
    return lines;
  };
  const urlFor = (rules) => {
    const r = rand();
    if (r < 0.03) return pick(["data:text/plain,hi", "blob:https://a.example/1", "about:blank", "ms-browser-extension://x/y"]);
    let host;
    let path = "/" + (chance(0.7) ? pick(GEN.pathWords) + "/" : "") + (chance(0.5) ? pick(GEN.pathWords) : "");
    const rule = rules.length && chance(0.4) ? pick(rules) : null;
    if (rule && rule.kind === "host") {
      host = (chance(0.5) ? pick(GEN.hostWords) + "." : "") + rule.value;
    } else if (rule && rule.kind === "label") {
      host = (chance(0.5) ? pick(GEN.hostWords) + "." : "") + pick(["", "x", "not"]) + rule.value + pick(["", "9", "-z"]) + "." + pick(GEN.tlds);
    } else if (rule && rule.kind === "path") {
      path = "/" + pick(["", "q/"]) + rule.value.slice(1) + pick(["", "z"]);
      host = pick(GEN.hostWords) + "." + pick(GEN.tlds);
    } else {
      const labels = [];
      const n = 1 + Math.floor(rand() * 3);
      for (let i = 0; i < n; i++) labels.push((chance(0.15) ? "not" : "") + pick(GEN.hostWords));
      labels.push(pick(GEN.tlds));
      host = labels.join(".");
    }
    if (chance(0.05)) host += ".";
    if (chance(0.1)) host = host.toUpperCase();
    const port = chance(0.1) ? ":" + (1000 + Math.floor(rand() * 9000)) : "";
    const query = chance(0.3) ? "?" + pick(GEN.pathWords) + "=" + pick(GEN.pathWords) : "";
    const frag = chance(0.2) ? "#" + pick(GEN.pathWords) : "";
    return pick(["https", "https", "http"]) + "://" + host + port + path + query + frag;
  };
  const siteFor = (url) => {
    if (chance(0.25)) {
      const m = /^[a-z]+:\/\/([^/:?#]+)/i.exec(url);
      if (m) {
        const parts = m[1].toLowerCase().replace(/\.$/, "").split(".");
        return parts.slice(Math.floor(rand() * parts.length)).join(".") || pick(GEN.sites);
      }
    }
    return pick(GEN.sites);
  };
  return { ruleList, urlFor, siteFor, pick };
}

describe("matcher: differential test against the naive reference", () => {
  const SEED = 20240611;

  test("500 seeded random lists x 200 random requests agree on verdict, rule and line", () => {
    let decisions = 0;
    let nonPass = 0;
    for (let l = 0; l < 500; l++) {
      const rand = mulberry32(SEED + l);
      const gen = makeGen(rand);
      const src = gen.ruleList().join("\n");
      const compiled = ShieldCore.compile(src);
      for (let u = 0; u < 200; u++) {
        const url = gen.urlFor(compiled.rules);
        const req = { url, siteHost: gen.siteFor(url), type: gen.pick(GEN.types) };
        const got = ShieldCore.decide(compiled, req);
        const want = reference.decide(compiled.rules, req);
        decisions++;
        if (want.verdict !== "pass") nonPass++;
        if (got.verdict !== want.verdict || got.ruleIndex !== want.ruleIndex || got.line !== want.line) {
          assert.fail(
            "seed " + (SEED + l) + " list " + l + " request " + u + ": " + JSON.stringify(req) +
              "\n core " + JSON.stringify(got) + "\n want " + JSON.stringify(want)
          );
        }
      }
    }
    assert.equal(decisions, 100000);
    // The generator has to actually exercise matches, or agreement proves nothing.
    assert.ok(nonPass > 10000, "only " + nonPass + " non-pass decisions out of " + decisions);
  });

  test("the generator itself is reproducible from its seed", () => {
    const a = makeGen(mulberry32(7)).ruleList().join("\n");
    const b = makeGen(mulberry32(7)).ruleList().join("\n");
    const c = makeGen(mulberry32(8)).ruleList().join("\n");
    assert.equal(a, b);
    assert.notEqual(a, c);
  });
});

// ---------------------------------------------------------------------------
// 9.4 and 9.5 Translation to declarativeNetRequest, and the caps
// ---------------------------------------------------------------------------

const ShieldDnr = require("../lib/shield-dnr.js");

function translate(src, source) {
  return ShieldDnr.translate(ShieldCore.compile(src), { source: source || "user" });
}

// A Rule record built by hand, so a test can go past the format's own 2000-rule cap.
function synthetic(i, verb, kind, value) {
  return { i, line: i + 1, verb, kind, value, types: 511, party: 2, only: null, except: null, list: "" };
}

const ACTIONS = ["block", "allow", "allowAllRequests"];

function schemaProblems(rule) {
  const bad = [];
  if (!Number.isInteger(rule.id)) bad.push("id is not an integer");
  if (!Number.isInteger(rule.priority)) bad.push("priority is not an integer");
  if (!rule.action || !ACTIONS.includes(rule.action.type)) bad.push("action.type is not allowed");
  const c = rule.condition;
  if (!c || !(c.requestDomains || c.urlFilter || c.regexFilter)) bad.push("condition has no matcher");
  if (c && c.requestDomains && !(Array.isArray(c.requestDomains) && c.requestDomains.length > 0)) bad.push("requestDomains is empty");
  return bad;
}

function bandOf(id) {
  if (id === 999) return "probe";
  if (id >= 1000 && id <= 1999) return "starter-block";
  if (id >= 2000 && id <= 2999) return "starter-allow";
  if (id >= 10000 && id <= 19999) return "user-block";
  if (id >= 20000 && id <= 29999) return "user-allow";
  if (id >= 30000 && id <= 30499) return "off";
  return "none";
}

describe("translation: table cases", () => {
  for (const c of readCases("dnr.json")) {
    test(c.name, () => {
      const result = translate(c.src, c.source);
      assert.deepEqual(result.rules, c.expect);
      assert.deepEqual(result.dropped, []);
    });
  }
});

describe("translation: shape", () => {
  const MIXED = [
    "block host:a.example",
    "block label:adserver party any",
    "block path:/ad-slot/ kind image",
    "allow host:b.example only x.example",
    "allow label:visitor-track except y.example",
    "off site:bank.example",
    "off site:other.example",
    "hide css:div.promo"
  ].join("\n");

  test("every generated rule passes the schema check", () => {
    const { rules } = translate(MIXED);
    assert.equal(rules.length, 7);
    for (const r of rules) assert.deepEqual(schemaProblems(r), [], JSON.stringify(r));
  });

  test("ids are unique and land in the band for the verb and source", () => {
    const user = translate(MIXED).rules;
    assert.equal(new Set(user.map((r) => r.id)).size, user.length);
    for (const r of user) {
      const band = bandOf(r.id);
      if (r.action.type === "block") assert.equal(band, "user-block");
      else if (r.action.type === "allow") assert.equal(band, "user-allow");
      else assert.equal(band, "off");
    }
    const starter = translate(MIXED.split("\n").filter((l) => !l.startsWith("off")).join("\n"), "starter").rules;
    for (const r of starter) assert.equal(bandOf(r.id), r.action.type === "block" ? "starter-block" : "starter-allow");
  });

  test("priorities state the precedence: block 1, allow 2, off 3", () => {
    for (const r of translate(MIXED).rules) {
      const want = r.action.type === "block" ? 1 : r.action.type === "allow" ? 2 : 3;
      assert.equal(r.priority, want);
    }
  });

  test("the label regex matches a host with the token inside one label and not one that straddles a dot", () => {
    const { rules } = translate("block label:click-track");
    const re = new RegExp(rules[0].condition.regexFilter, rules[0].condition.isUrlFilterCaseSensitive ? "" : "i");
    assert.ok(re.test("https://eu.click-tracker.example/a"));
    assert.ok(re.test("https://click-track.example/a"));
    assert.ok(re.test("http://x.y.preclick-track9.example:8080/a"));
    assert.ok(re.test("https://EU.CLICK-TRACK.example/a"));
    assert.ok(!re.test("https://click.track.example/a"));
    assert.ok(!re.test("https://example.com/click-track/a"));
    assert.ok(!re.test("https://example.com/?u=https://click-track.example/"));
  });

  test("the label regex agrees with the matcher over a set of hosts", () => {
    const hosts = ["adserver.example", "x.adserver.example", "myadserver1.example", "ad.server.example", "adserve.example", "example.adserver", "ad-server.example"];
    const { rules } = translate("block label:adserver party any");
    const re = new RegExp(rules[0].condition.regexFilter, "i");
    const compiled = ShieldCore.compile("block label:adserver party any");
    for (const h of hosts) {
      const url = "https://" + h + "/p";
      const matcher = ShieldCore.decide(compiled, { url, siteHost: "s.example", type: "script" }).verdict === "block";
      // The regex needs a dot after the label, so a token in the last label is the one known gap.
      const expected = h === "example.adserver" ? false : matcher;
      assert.equal(re.test(url), expected, h);
    }
  });

  test("each off rule produces exactly one allowAllRequests rule on main_frame", () => {
    const { rules } = translate("off site:a.example\noff site:b.example\nblock host:c.example");
    const off = rules.filter((r) => r.action.type === "allowAllRequests");
    assert.equal(off.length, 2);
    for (const r of off) assert.deepEqual(r.condition.resourceTypes, ["main_frame"]);
  });

  test("a block or allow rule never names main_frame", () => {
    for (const r of translate(MIXED).rules) {
      if (r.action.type !== "allowAllRequests") assert.ok(!(r.condition.resourceTypes || []).includes("main_frame"));
    }
    const most = translate("block host:a.example kind script,image,frame,xhr,media,font,style,ping").rules[0];
    assert.ok(!most.condition.resourceTypes.includes("main_frame"));
  });

  test("a list that applies to every kind leaves resourceTypes out, which excludes main_frame by default", () => {
    const { rules } = translate("block host:a.example");
    assert.equal("resourceTypes" in rules[0].condition, false);
  });

  test("translating the same source twice is deep-equal", () => {
    assert.deepEqual(translate(MIXED), translate(MIXED));
    assert.equal(JSON.stringify(translate(MIXED)), JSON.stringify(translate(MIXED)));
  });

  test("translating does not change the compiled list", () => {
    const c = ShieldCore.compile(MIXED);
    const before = JSON.stringify(c.rules);
    ShieldDnr.translate(c, { source: "user" });
    assert.equal(JSON.stringify(c.rules), before);
  });

  test("a path with a pattern character cannot be a plain urlFilter, so it is reported and not translated", () => {
    const result = translate("block path:/a*b/\nblock path:/c|d/\nblock path:/e^f/\nblock path:/plain/");
    assert.equal(result.rules.length, 1);
    assert.equal(result.rules[0].condition.urlFilter, "/plain/");
    assert.deepEqual(result.dropped.map((d) => [d.line, d.reason]), [[1, "path-pattern-char"], [2, "path-pattern-char"], [3, "path-pattern-char"]]);
  });

  test("a path that starts with // would become a urlFilter that can match the host, so it is reported and not translated", () => {
    const result = translate("block path://ads.example/x\nblock path:/plain/\nallow path://cdn/a/");
    assert.equal(result.rules.length, 1);
    assert.equal(result.rules[0].condition.urlFilter, "/plain/");
    assert.deepEqual(result.dropped.map((d) => [d.line, d.reason]), [[1, "path-scheme-slash"], [3, "path-scheme-slash"]]);
    // The core matcher still reads the path literally.
    const compiled = ShieldCore.compile("block path://ads.example/x");
    const hit = ShieldCore.decide(compiled, { url: "https://a.example//ads.example/x", siteHost: "s.example", type: "script" });
    assert.equal(hit.verdict, "block");
  });

  test("isBlockRuleId is true only for the two block bands", () => {
    for (const id of [1000, 1500, 1999, 10000, 19999]) assert.equal(ShieldDnr.isBlockRuleId(id), true, String(id));
    for (const id of [999, 2000, 2999, 9999, 20000, 29999, 30000, 30499, 0, -1]) {
      assert.equal(ShieldDnr.isBlockRuleId(id), false, String(id));
    }
  });

  test("the probe rule is id 999 and can never match a real request", () => {
    assert.deepEqual(ShieldDnr.PROBE_RULE, {
      id: 999,
      priority: 1,
      action: { type: "block" },
      condition: { requestDomains: ["probe.bowser-shield.invalid"], resourceTypes: ["image"] }
    });
  });
});

describe("translation: caps", () => {
  test("6000 translatable rules give exactly 5000 rules and 1000 dropped in ascending line order", () => {
    const rules = Array.from({ length: 6000 }, (_, i) => synthetic(i, "block", "host", "h" + i + ".example"));
    const result = ShieldDnr.translate({ rules }, { source: "user" });
    assert.equal(result.rules.length, 5000);
    assert.equal(result.dropped.length, 1000);
    assert.deepEqual(result.dropped.map((d) => d.line), Array.from({ length: 1000 }, (_, i) => 5001 + i));
    assert.ok(result.dropped.every((d) => d.reason === "budget"));
    assert.equal(new Set(result.rules.map((r) => r.id)).size, 5000);
    assert.equal(result.rules[0].id, 10000);
    assert.equal(result.rules[4999].id, 14999);
  });

  test("the budget is exactly 5000: 5000 rules drop nothing", () => {
    const rules = Array.from({ length: 5000 }, (_, i) => synthetic(i, "block", "host", "h" + i + ".example"));
    const result = ShieldDnr.translate({ rules }, { source: "user" });
    assert.equal(result.rules.length, 5000);
    assert.deepEqual(result.dropped, []);
  });

  test("under pressure, off and allow rules that come last in the file are kept and block rules are dropped", () => {
    const rules = [];
    for (let i = 0; i < 5200; i++) rules.push(synthetic(rules.length, "block", "host", "h" + i + ".example"));
    for (let i = 0; i < 100; i++) rules.push(synthetic(rules.length, "allow", "host", "ok" + i + ".example"));
    for (let i = 0; i < 50; i++) rules.push(synthetic(rules.length, "off", "site", "s" + i + ".example"));
    const result = ShieldDnr.translate({ rules }, { source: "user" });
    assert.equal(result.rules.length, 5000);
    assert.equal(result.rules.filter((r) => r.action.type === "allowAllRequests").length, 50);
    assert.equal(result.rules.filter((r) => r.action.type === "allow").length, 100);
    assert.equal(result.rules.filter((r) => r.action.type === "block").length, 4850);
    assert.equal(result.dropped.length, 350);
    assert.ok(result.dropped.every((d) => d.line <= 5200), "only block rules are dropped");
    assert.deepEqual(result.dropped.map((d) => d.line), Array.from({ length: 350 }, (_, i) => 4851 + i));
  });

  test("1100 label rules give 1000 regex rules and 100 dropped as regex-cap", () => {
    const rules = Array.from({ length: 1100 }, (_, i) => synthetic(i, "block", "label", "lab" + String(i).padStart(4, "x")));
    const result = ShieldDnr.translate({ rules }, { source: "user" });
    assert.equal(result.rules.filter((r) => r.condition.regexFilter).length, 1000);
    assert.equal(result.rules.length, 1000);
    assert.equal(result.dropped.length, 100);
    assert.ok(result.dropped.every((d) => d.reason === "regex-cap"));
    assert.deepEqual(result.dropped.map((d) => d.line), Array.from({ length: 100 }, (_, i) => 1001 + i));
  });

  test("the regex cap leaves non-regex rules alone", () => {
    const rules = [];
    for (let i = 0; i < 1010; i++) rules.push(synthetic(rules.length, "block", "label", "lab" + String(i).padStart(4, "x")));
    for (let i = 0; i < 20; i++) rules.push(synthetic(rules.length, "block", "host", "h" + i + ".example"));
    const result = ShieldDnr.translate({ rules }, { source: "user" });
    assert.equal(result.rules.length, 1020);
    assert.equal(result.dropped.length, 10);
  });

  test("a smaller regexCap option is honoured, so a caller can leave room for the static ruleset", () => {
    const rules = Array.from({ length: 30 }, (_, i) => synthetic(i, "block", "label", "lab" + String(i).padStart(4, "x")));
    const result = ShieldDnr.translate({ rules }, { source: "user", regexCap: 20 });
    assert.equal(result.rules.length, 20);
    assert.equal(result.dropped.length, 10);
  });

  test("lineById maps each generated id back to the file line that produced it", () => {
    const result = translate("# c\nblock host:a.example\nhide css:div.x\nallow host:b.example\noff site:s.example\nblock label:adserver");
    const byId = JSON.parse(JSON.stringify(result.lineById));
    assert.deepEqual(byId, { 30000: 5, 20000: 4, 10000: 2, 10001: 6 });
    for (const r of result.rules) assert.ok(r.id in byId);
    const lists = ShieldDnr.translate(
      ShieldCore.compileAll([{ tag: "site", src: "off site:a.example" }, { tag: "user", src: "block host:b.example" }]),
      { source: "user" }
    ).listById;
    assert.deepEqual(JSON.parse(JSON.stringify(lists)), { 30000: "site", 10000: "user" });
  });

  test("dropped entries name the line, a reason and the list they came from", () => {
    const rules = [synthetic(0, "block", "label", "labxxxxa"), synthetic(1, "block", "label", "labxxxxb")];
    rules[1].list = "user";
    const result = ShieldDnr.translate({ rules }, { source: "user", regexCap: 1 });
    assert.deepEqual(result.dropped, [{ line: 2, reason: "regex-cap", list: "user" }]);
  });

  test("a format-legal list never reaches the budget", () => {
    const lines = Array.from({ length: 2000 }, (_, i) => "block host:h" + i + ".example");
    const result = translate(lines.join("\n"));
    assert.equal(result.rules.length, 2000);
    assert.deepEqual(result.dropped, []);
  });
});

describe("the starter list", () => {
  const starterSrl = fs.readFileSync(path.join(ROOT, "rules", "starter.srl"), "utf8");

  test("is exactly the block in section 3.7 of the design", () => {
    const design = fs.readFileSync(path.join(ROOT, "DESIGN.md"), "utf8").replace(/\r\n/g, "\n");
    const anchor = design.indexOf("`rules/starter.srl`. These are written");
    assert.ok(anchor > 0, "section 3.7 not found in DESIGN.md");
    const open = design.indexOf("```\n", anchor) + 4;
    const close = design.indexOf("\n```", open);
    assert.equal(starterSrl.replace(/\r\n/g, "\n").replace(/\n+$/, ""), design.slice(open, close));
  });

  test("compiles with no errors and no warnings", () => {
    const c = ShieldCore.compile(starterSrl);
    assert.deepEqual(c.errors, []);
    assert.deepEqual(c.warnings, []);
  });

  test("has 38 block rules and 10 hide rules", () => {
    const c = ShieldCore.compile(starterSrl);
    assert.deepEqual(c.counts, { block: 38, allow: 0, hide: 10, off: 0 });
  });

  test("names no vendor: every host-shaped target is a generic role word", () => {
    assert.ok(!/^(block|allow) host:/m.test(starterSrl), "the starter list has no host rules");
  });

  test("translating rules/starter.srl deep-equals the committed rules/starter-dnr.json", () => {
    const committed = JSON.parse(fs.readFileSync(path.join(ROOT, "rules", "starter-dnr.json"), "utf8"));
    const result = ShieldDnr.translate(ShieldCore.compile(starterSrl), { source: "starter" });
    assert.deepEqual(result.dropped, []);
    assert.deepEqual(result.rules, committed);
  });

  test("the committed ruleset is the translation byte for byte", () => {
    const raw = fs.readFileSync(path.join(ROOT, "rules", "starter-dnr.json"), "utf8").replace(/\r\n/g, "\n");
    const result = ShieldDnr.translate(ShieldCore.compile(starterSrl), { source: "starter" });
    assert.equal(raw, JSON.stringify(result.rules, null, 2) + "\n");
  });

  test("the committed ruleset is valid and sits in the starter block band", () => {
    const committed = JSON.parse(fs.readFileSync(path.join(ROOT, "rules", "starter-dnr.json"), "utf8"));
    assert.equal(committed.length, 38);
    committed.forEach((r, i) => {
      assert.deepEqual(schemaProblems(r), []);
      assert.equal(r.id, 1000 + i);
      assert.equal(r.priority, 1);
      assert.equal(r.action.type, "block");
    });
    assert.equal(committed.filter((r) => r.condition.regexFilter).length, 26);
  });
});

// ---------------------------------------------------------------------------
// 6.1 Cosmetic data: what the service worker stores and the content script reads
// ---------------------------------------------------------------------------

describe("cosmetic", () => {
  const SRC = [
    "hide css:div.a",
    "hide css:div.b only x.example,y.example",
    "hide css:div.c except z.example",
    "hide css:div.d only x.example except www.x.example",
    "hide css:div.a",
    "block host:a.example"
  ].join("\n");

  test("buildCosmetic groups selectors into global, byOnly and byExcept, one selector per entry", () => {
    const data = ShieldCore.buildCosmetic(ShieldCore.compile(SRC));
    assert.deepEqual(data.global, ["div.a"]);
    assert.deepEqual(data.byOnly, { "x.example": ["div.b"], "y.example": ["div.b"] });
    assert.deepEqual(data.byExcept, [
      { sel: "div.c", except: ["z.example"] },
      { sel: "div.d", except: ["www.x.example"], only: ["x.example"] }
    ]);
  });

  test("the data survives a JSON round trip, as it does through chrome.storage", () => {
    const data = ShieldCore.buildCosmetic(ShieldCore.compile(SRC));
    assert.deepEqual(JSON.parse(JSON.stringify(data)), data);
  });

  test("cosmeticFor returns the global selectors on any site", () => {
    const data = ShieldCore.buildCosmetic(ShieldCore.compile(SRC));
    assert.deepEqual(ShieldCore.cosmeticFor(data, "news.example").sort(), ["div.a", "div.c"]);
  });

  test("cosmeticFor honours only with label-boundary suffixes", () => {
    const data = ShieldCore.buildCosmetic(ShieldCore.compile(SRC));
    assert.ok(ShieldCore.cosmeticFor(data, "x.example").includes("div.b"));
    assert.ok(ShieldCore.cosmeticFor(data, "www.x.example").includes("div.b"));
    assert.ok(!ShieldCore.cosmeticFor(data, "x.example.net").includes("div.b"));
    assert.ok(!ShieldCore.cosmeticFor(data, "notx.example").includes("div.b"));
  });

  test("cosmeticFor applies except before only", () => {
    const data = ShieldCore.buildCosmetic(ShieldCore.compile(SRC));
    assert.ok(!ShieldCore.cosmeticFor(data, "z.example").includes("div.c"));
    assert.ok(!ShieldCore.cosmeticFor(data, "www.x.example").includes("div.d"));
    assert.ok(ShieldCore.cosmeticFor(data, "x.example").includes("div.d"));
    assert.ok(!ShieldCore.cosmeticFor(data, "other.example").includes("div.d"));
  });

  test("a selector named by two lists appears once", () => {
    const c = ShieldCore.compileAll([
      { tag: "starter", src: "hide css:div.same" },
      { tag: "user", src: "hide css:div.same\nhide css:div.other" }
    ]);
    assert.deepEqual(ShieldCore.buildCosmetic(c).global, ["div.same", "div.other"]);
  });

  test("cosmeticFor copes with empty or missing data", () => {
    assert.deepEqual(ShieldCore.cosmeticFor({}, "a.example"), []);
    assert.deepEqual(ShieldCore.cosmeticFor(undefined, "a.example"), []);
    assert.deepEqual(ShieldCore.cosmeticFor({ global: ["div.a"] }, "a.example"), ["div.a"]);
  });

  test("cosmeticFor ignores a byOnly key that looks like an Object.prototype member", () => {
    const data = { global: [], byOnly: JSON.parse('{"__proto__":["div.x"]}') };
    assert.doesNotThrow(() => ShieldCore.cosmeticFor(data, "a.example"));
  });

  test("the starter list gives ten global selectors", () => {
    const starter = fs.readFileSync(path.join(ROOT, "rules", "starter.srl"), "utf8");
    const data = ShieldCore.buildCosmetic(ShieldCore.compile(starter));
    assert.equal(data.global.length, 10);
    assert.deepEqual(data.byExcept, []);
  });

  test("siteIsOff answers from the off rules by label-boundary suffix", () => {
    const c = ShieldCore.compile("off site:bank.example\nblock host:a.example");
    assert.equal(ShieldCore.siteIsOff(c, "bank.example"), true);
    assert.equal(ShieldCore.siteIsOff(c, "www.bank.example"), true);
    assert.equal(ShieldCore.siteIsOff(c, "WWW.Bank.Example."), true);
    assert.equal(ShieldCore.siteIsOff(c, "notbank.example"), false);
    assert.equal(ShieldCore.siteIsOff(c, "bank.example.net"), false);
    assert.equal(ShieldCore.siteIsOff(c, ""), false);
    assert.equal(ShieldCore.siteIsOff(ShieldCore.compile(""), "bank.example"), false);
    const deep = "a.b.c.d.e.f.g.h.i.j.k.l.m.n.o.p.q.r.s.t.u.v.w.x.bank.example";
    assert.equal(ShieldCore.siteIsOff(c, deep), false, "more than 24 labels is a pass, as in decide");
  });

  test("site validation helper agrees with the site lexical rule", () => {
    assert.equal(ShieldCore.isValidSite("bank.example"), true);
    assert.equal(ShieldCore.isValidSite("192.168.1.1"), true);
    assert.equal(ShieldCore.isValidSite("localhost"), false);
    assert.equal(ShieldCore.isValidSite("Bank.example"), false);
    assert.equal(ShieldCore.isValidSite("a.b"), false);
    assert.equal(ShieldCore.isValidSite("bank.example:8080"), false);
  });
});

// ---------------------------------------------------------------------------
// 7.2 The panel: the summary state table, the chord, and the renderer's wiring
// ---------------------------------------------------------------------------

const ShieldPanel = require("../ui/panel.js");

function baseState(over) {
  return Object.assign(
    {
      enabled: true,
      starterEnabled: true,
      userRules: "",
      siteOff: [],
      engineMode: "network",
      countsAvailable: true,
      readOnly: false,
      page: { supported: true, host: "news.example", siteOff: false, canExempt: true, blocked: 37, hidden: 0 },
      session: { blocked: 124, hidden: 0 },
      counts: { starter: { block: 38, allow: 0, hide: 10, off: 0 }, user: { block: 0, allow: 0, hide: 0, off: 0 } },
      errors: [],
      warnings: [],
      dropped: [],
      badSelectors: 0,
      starterSrl: "# starter\nblock label:adserver\n",
      chord: "Alt+Shift+S"
    },
    over
  );
}

const lineText = (l) => (l.n === null || l.n === undefined ? "" : l.n + " ") + l.text;

describe("panel: summary state table (7.2)", () => {
  test("on, network mode: both counts and 'Network blocking: active'", () => {
    const m = ShieldPanel.summaryModel(baseState());
    assert.equal(m.toggle.on, true);
    assert.equal(m.siteName, "news.example");
    assert.deepEqual(m.lines.map(lineText), ["37 blocked on this site", "124 blocked this session"]);
    assert.equal(m.status, "Network blocking: active");
    assert.equal(m.siteButton.label, "Don't filter this site");
    assert.equal(m.greyed, false);
  });

  test("on, network mode, no feedback: 'Blocked count unavailable in this runtime' replaces both counts", () => {
    const m = ShieldPanel.summaryModel(baseState({ countsAvailable: false }));
    assert.deepEqual(m.lines.map(lineText), ["Blocked count unavailable in this runtime"]);
    assert.equal(m.status, "Network blocking: active");
  });

  test("on, dom mode: 'N hidden on this site' and the honest status", () => {
    const m = ShieldPanel.summaryModel(
      baseState({ engineMode: "dom", page: { supported: true, host: "news.example", siteOff: false, canExempt: true, blocked: 0, hidden: 12 } })
    );
    assert.deepEqual(m.lines.map(lineText), ["12 hidden on this site"]);
    assert.equal(m.status, "Network blocking unavailable — hiding in the page only");
    assert.ok(!m.lines.some((l) => /blocked/.test(l.text)), "dom mode never says blocked");
  });

  test("off: toggle off, counts greyed, 'Shield is off'", () => {
    const m = ShieldPanel.summaryModel(baseState({ enabled: false }));
    assert.equal(m.toggle.on, false);
    assert.equal(m.greyed, true);
    assert.equal(m.status, "Shield is off");
    assert.deepEqual(m.lines.map(lineText), ["37 blocked on this site", "124 blocked this session"]);
  });

  test("site never filtered: says so and the button reads 'Filter this site'", () => {
    const m = ShieldPanel.summaryModel(
      baseState({ page: { supported: true, host: "bank.example", siteOff: true, canExempt: true, blocked: 0, hidden: 0 } })
    );
    assert.equal(m.status, "This site is never filtered");
    assert.equal(m.siteButton.label, "Filter this site");
    assert.equal(m.siteButton.disabled, false);
  });

  test("non-http page: 'Nothing to filter here', no counts, no site button, toggle still works", () => {
    const m = ShieldPanel.summaryModel(baseState({ page: { supported: false, host: "", siteOff: false, canExempt: false, blocked: 0, hidden: 0 } }));
    assert.equal(m.status, "Nothing to filter here");
    assert.deepEqual(m.lines, []);
    assert.equal(m.siteButton, null);
    assert.equal(m.toggle.on, true);
    assert.equal(m.toggle.disabled, false);
    const off = ShieldPanel.summaryModel(baseState({ enabled: false, page: { supported: false, host: "", siteOff: false, canExempt: false, blocked: 0, hidden: 0 } }));
    assert.equal(off.toggle.on, false);
    assert.equal(off.toggle.disabled, false);
  });

  test("no page context at all is treated as a non-http page", () => {
    const m = ShieldPanel.summaryModel(baseState({ page: null }));
    assert.equal(m.status, "Nothing to filter here");
    assert.equal(m.siteButton, null);
  });

  test("when several states hold, off beats never-filtered, which beats the mode line", () => {
    const siteOffPage = { supported: true, host: "bank.example", siteOff: true, canExempt: true, blocked: 0, hidden: 0 };
    assert.equal(ShieldPanel.summaryModel(baseState({ enabled: false, page: siteOffPage })).status, "Shield is off");
    assert.equal(ShieldPanel.summaryModel(baseState({ engineMode: "dom", page: siteOffPage })).status, "This site is never filtered");
  });

  test("an address that cannot be a site rule disables the button and says why", () => {
    const m = ShieldPanel.summaryModel(baseState({ page: { supported: true, host: "localhost", siteOff: false, canExempt: false, blocked: 0, hidden: 0 } }));
    assert.equal(m.siteButton.disabled, true);
    assert.match(m.siteButton.reason, /two or more/i);
  });

  test("a site covered only by an off rule in My rules cannot be undone from the button, and says where to fix it", () => {
    const m = ShieldPanel.summaryModel(
      baseState({ page: { supported: true, host: "bank.example", siteOff: true, siteOffRuleOnly: true, canExempt: true, blocked: 0, hidden: 0 } })
    );
    assert.equal(m.status, "This site is never filtered");
    assert.equal(m.siteButton.disabled, true);
    assert.match(m.siteButton.reason, /My rules/);
  });

  test("a read-only state disables the controls and explains", () => {
    const m = ShieldPanel.summaryModel(baseState({ readOnly: true }));
    assert.equal(m.toggle.disabled, true);
    assert.equal(m.siteButton.disabled, true);
    assert.match(m.readOnlyNote, /newer/i);
  });

  test("the model never reads the counts of the layer that is not live", () => {
    const network = ShieldPanel.summaryModel(baseState({ page: { supported: true, host: "a.example", siteOff: false, canExempt: true, blocked: 5, hidden: 99 } }));
    assert.ok(network.lines.map(lineText).join("|").includes("5 blocked"));
    assert.ok(!network.lines.map(lineText).join("|").includes("99"));
  });
});

describe("panel: status and banner text", () => {
  test("network mode states what the probe proved and what it did not", () => {
    const s = ShieldPanel.statusModel(baseState());
    assert.equal(s.engine, "Network blocking: API present, enforcement unverified in this runtime");
    assert.match(s.check, /count/);
  });

  test("dom mode says hidden is not blocked", () => {
    const s = ShieldPanel.statusModel(baseState({ engineMode: "dom" }));
    assert.equal(s.engine, "Network blocking unavailable — hiding in the page only");
    assert.match(s.wording, /hidden/);
    assert.match(s.wording, /blocked/);
  });

  test("rule counts are listed for both lists", () => {
    const s = ShieldPanel.statusModel(baseState());
    assert.equal(s.rules[0], "Starter list: 38 block, 0 allow, 10 hide, 0 off");
    assert.equal(s.rules[1], "Your rules: 0 block, 0 allow, 0 hide, 0 off");
  });

  test("no banner when nothing was dropped", () => {
    assert.equal(ShieldPanel.droppedModel(baseState()), null);
  });

  test("the dropped banner names the count and the first line", () => {
    const m = ShieldPanel.droppedModel(baseState({ dropped: [{ line: 5, reason: "budget" }, { line: 9, reason: "budget" }] }));
    assert.equal(m.text, "2 rules are not being enforced, starting at line 5.");
    assert.equal(m.regexText, null);
    const one = ShieldPanel.droppedModel(baseState({ dropped: [{ line: 3, reason: "budget" }] }));
    assert.equal(one.text, "1 rule is not being enforced, starting at line 3.");
  });

  test("the first dropped line is the lowest, whatever order the list arrives in", () => {
    const m = ShieldPanel.droppedModel(baseState({ dropped: [{ line: 9, reason: "budget" }, { line: 5, reason: "budget" }] }));
    assert.match(m.text, /line 5\.$/);
  });

  test("regex-cap drops get the plain sentence about there being no fallback", () => {
    const m = ShieldPanel.droppedModel(baseState({ dropped: [{ line: 2, reason: "regex-cap" }, { line: 3, reason: "regex-cap" }] }));
    assert.match(m.regexText, /2 label rules/);
    assert.match(m.regexText, /no fallback/);
    assert.match(m.regexText, /do not apply/);
  });
});

describe("panel: chord", () => {
  test("parses the default chord", () => {
    assert.deepEqual(ShieldPanel.parseChord("Alt+Shift+S"), { alt: true, shift: true, ctrl: false, meta: false, code: "KeyS" });
  });

  test("is case-insensitive and accepts digits and function keys", () => {
    assert.equal(ShieldPanel.parseChord("ctrl+alt+9").code, "Digit9");
    assert.equal(ShieldPanel.parseChord("Alt+F5").code, "F5");
    assert.equal(ShieldPanel.parseChord("alt+shift+s").code, "KeyS");
  });

  test("rejects a chord with no modifier, no key, two keys or a repeated modifier", () => {
    for (const bad of ["S", "Shift+S", "Alt+Shift", "Alt+SS", "Alt+Alt+S", "", "Alt+", "Alt+Shift+Enter", null, undefined]) {
      assert.equal(ShieldPanel.parseChord(bad), null, String(bad));
    }
  });

  test("matches exactly the default event of section 7.1 and nothing looser", () => {
    const chord = ShieldPanel.parseChord("Alt+Shift+S");
    const ev = (o) => Object.assign({ code: "KeyS", altKey: true, shiftKey: true, ctrlKey: false, metaKey: false }, o);
    assert.equal(ShieldPanel.matchChord(chord, ev()), true);
    assert.equal(ShieldPanel.matchChord(chord, ev({ code: "KeyD" })), false);
    assert.equal(ShieldPanel.matchChord(chord, ev({ altKey: false })), false);
    assert.equal(ShieldPanel.matchChord(chord, ev({ shiftKey: false })), false);
    assert.equal(ShieldPanel.matchChord(chord, ev({ ctrlKey: true })), false);
    assert.equal(ShieldPanel.matchChord(chord, ev({ metaKey: true })), false);
    assert.equal(ShieldPanel.matchChord(null, ev()), false);
  });

  test("is ignored while focus is in an editable element", () => {
    assert.equal(ShieldPanel.isEditable({ tagName: "INPUT" }), true);
    assert.equal(ShieldPanel.isEditable({ tagName: "TEXTAREA" }), true);
    assert.equal(ShieldPanel.isEditable({ tagName: "SELECT" }), true);
    assert.equal(ShieldPanel.isEditable({ tagName: "DIV", isContentEditable: true }), true);
    assert.equal(ShieldPanel.isEditable({ tagName: "DIV", getAttribute: (k) => (k === "role" ? "textbox" : null) }), true);
    assert.equal(ShieldPanel.isEditable({ tagName: "DIV", isContentEditable: false, getAttribute: () => null }), false);
    assert.equal(ShieldPanel.isEditable({ tagName: "BODY" }), false);
    assert.equal(ShieldPanel.isEditable(null), false);
  });
});

// A just-enough DOM for the renderer: elements, attributes, listeners, text.
class FakeNode {
  constructor(tag) {
    this.tagName = String(tag).toUpperCase();
    this.children = [];
    this.attrs = {};
    this.listeners = {};
    this.className = "";
    this._text = "";
    this.parentNode = null;
    this.value = "";
    this.checked = false;
    this.disabled = false;
    this.hidden = false;
    this.focused = false;
    this.style = {};
  }
  appendChild(c) {
    c.parentNode = this;
    this.children.push(c);
    return c;
  }
  get firstChild() {
    return this.children[0] || null;
  }
  removeChild(c) {
    this.children = this.children.filter((x) => x !== c);
  }
  set textContent(v) {
    this.children = [];
    this._text = String(v);
  }
  get textContent() {
    return this._text + this.children.map((c) => c.textContent).join("");
  }
  setAttribute(k, v) {
    this.attrs[k] = String(v);
    if (k === "class") this.className = String(v);
  }
  getAttribute(k) {
    return Object.prototype.hasOwnProperty.call(this.attrs, k) ? this.attrs[k] : null;
  }
  removeAttribute(k) {
    delete this.attrs[k];
  }
  addEventListener(type, fn) {
    (this.listeners[type] = this.listeners[type] || []).push(fn);
  }
  dispatch(type, extra) {
    const ev = Object.assign({ type, target: this, preventDefault() {}, stopPropagation() {} }, extra);
    (this.listeners[type] || []).forEach((fn) => fn(ev));
    return ev;
  }
  focus() {
    this.focused = true;
  }
  all(pred, out = []) {
    for (const c of this.children) {
      if (pred(c)) out.push(c);
      c.all(pred, out);
    }
    return out;
  }
  byKey(key) {
    return this.all((n) => n.getAttribute("data-key") === key)[0] || null;
  }
}

function fakeDocument() {
  return { createElement: (t) => new FakeNode(t), activeElement: null };
}

// A stand-in for the service worker: answers getState and applies the messages the panel sends.
function fakeBackend(initial) {
  let state = initial;
  const sent = [];
  const send = async (msg) => {
    sent.push(msg);
    if (msg.type === "setEnabled") state = Object.assign({}, state, { enabled: msg.value });
    if (msg.type === "setStarterEnabled") state = Object.assign({}, state, { starterEnabled: msg.value });
    if (msg.type === "setSiteOff") {
      state = Object.assign({}, state, { page: Object.assign({}, state.page, { siteOff: msg.off }) });
    }
    if (msg.type === "saveSiteOff") state = Object.assign({}, state, { siteOff: msg.list });
    if (msg.type === "saveRules") state = Object.assign({}, state, { userRules: msg.text });
    return state;
  };
  return { send, sent, get state() { return state; } };
}

const flush = () => new Promise((r) => setImmediate(r));

async function mountPanel(view, state) {
  const root = new FakeNode("div");
  const backend = fakeBackend(state || baseState());
  const doc = fakeDocument();
  const panel = await ShieldPanel.mount(root, { view, send: backend.send, doc, core: ShieldCore });
  return { root, backend, panel, doc };
}

describe("panel: renderer wiring (summary view)", () => {
  test("renders the site, the counts, the status and the controls", async () => {
    const { root } = await mountPanel("summary");
    const text = root.textContent;
    for (const want of ["Bowser Shield", "news.example", "37", "blocked on this site", "124", "blocked this session", "Network blocking: active", "Don't filter this site", "Settings…", "Alt+Shift+S"]) {
      assert.ok(text.includes(want), "missing " + JSON.stringify(want) + " in " + JSON.stringify(text));
    }
  });

  test("asks the background for state once, on mount", async () => {
    const { backend } = await mountPanel("summary");
    assert.deepEqual(backend.sent, [{ type: "getState" }]);
  });

  test("the switch is a labelled switch that reflects the state", async () => {
    const { root } = await mountPanel("summary");
    const sw = root.byKey("enabled");
    assert.equal(sw.getAttribute("role"), "switch");
    assert.equal(sw.getAttribute("aria-checked"), "true");
    assert.ok(sw.getAttribute("aria-label"));
  });

  test("clicking the switch sends setEnabled and re-renders as off", async () => {
    const { root, backend } = await mountPanel("summary");
    root.byKey("enabled").dispatch("click");
    await flush();
    assert.deepEqual(backend.sent[1], { type: "setEnabled", value: false });
    assert.equal(root.byKey("enabled").getAttribute("aria-checked"), "false");
    assert.ok(root.textContent.includes("Shield is off"));
  });

  test("the toggle says what it applies to", async () => {
    const { root } = await mountPanel("summary");
    assert.ok(root.textContent.includes("every site"));
  });

  test("'Don't filter this site' sends the host and says when it takes effect", async () => {
    const { root, backend } = await mountPanel("summary");
    root.byKey("site-toggle").dispatch("click");
    await flush();
    assert.deepEqual(backend.sent[1], { type: "setSiteOff", host: "news.example", off: true });
    const notice = root.byKey("notice").textContent;
    assert.ok(notice.includes("next time news.example loads"), notice);
    assert.ok(root.textContent.includes("This site is never filtered"));
    assert.ok(root.byKey("site-toggle").textContent === "Filter this site");
  });

  test("'Filter this site' undoes it", async () => {
    const { root, backend } = await mountPanel(
      "summary",
      baseState({ page: { supported: true, host: "bank.example", siteOff: true, canExempt: true, blocked: 0, hidden: 0 } })
    );
    root.byKey("site-toggle").dispatch("click");
    await flush();
    assert.deepEqual(backend.sent[1], { type: "setSiteOff", host: "bank.example", off: false });
  });

  test("a non-http page shows the message and still renders a working switch", async () => {
    const { root, backend } = await mountPanel("summary", baseState({ page: { supported: false, host: "", siteOff: false, canExempt: false, blocked: 0, hidden: 0 } }));
    assert.ok(root.textContent.includes("Nothing to filter here"));
    assert.equal(root.byKey("site-toggle"), null);
    root.byKey("enabled").dispatch("click");
    await flush();
    assert.deepEqual(backend.sent[1], { type: "setEnabled", value: false });
  });

  test("'Settings…' switches to the options view without asking the background, and Back returns", async () => {
    const { root, backend } = await mountPanel("summary");
    root.byKey("settings").dispatch("click");
    await flush();
    assert.ok(root.byKey("rules"), "options view shows the rules editor");
    assert.equal(backend.sent.length, 1);
    root.byKey("back").dispatch("click");
    await flush();
    assert.ok(root.byKey("enabled") && !root.byKey("rules"));
  });

  test("an error the background reports is shown as the notice and the state is kept", async () => {
    const root = new FakeNode("div");
    const state = baseState();
    const send = async (msg) => (msg.type === "getState" ? state : Object.assign({}, state, { error: "Rules are over the 256 KiB limit." }));
    await ShieldPanel.mount(root, { view: "summary", send, doc: fakeDocument(), core: ShieldCore });
    root.byKey("site-toggle").dispatch("click");
    await flush();
    assert.equal(root.byKey("notice").textContent, "Rules are over the 256 KiB limit.");
    assert.ok(root.textContent.includes("news.example"));
  });

  test("shows a plain message when the background does not answer", async () => {
    const root = new FakeNode("div");
    await ShieldPanel.mount(root, {
      view: "summary",
      send: async () => {
        throw new Error("Could not establish connection");
      },
      doc: fakeDocument(),
      core: ShieldCore
    });
    assert.ok(/did not answer/.test(root.textContent), root.textContent);
  });

  test("builds no innerHTML: text from the page is only ever set as text", async () => {
    const { root } = await mountPanel(
      "summary",
      baseState({ page: { supported: true, host: "<img src=x onerror=alert(1)>.example", siteOff: false, canExempt: false, blocked: 0, hidden: 0 } })
    );
    assert.ok(root.textContent.includes("<img src=x onerror=alert(1)>.example"));
    assert.equal(root.all((n) => n.tagName === "IMG").length, 0);
  });
});

describe("panel: renderer wiring (options view)", () => {
  test("renders the four sections in order", async () => {
    const { root } = await mountPanel("options");
    const headings = root.all((n) => n.tagName === "H2").map((n) => n.textContent);
    assert.deepEqual(headings, ["Status", "My rules", "Sites never filtered", "Built-in starter list"]);
  });

  test("the status section shows the probe caveat, counts and the shortcut", async () => {
    const { root } = await mountPanel("options");
    const text = root.textContent;
    assert.ok(text.includes("API present, enforcement unverified in this runtime"));
    assert.ok(text.includes("Starter list: 38 block, 0 allow, 10 hide, 0 off"));
    assert.ok(root.byKey("chord").value === "Alt+Shift+S");
  });

  test("Re-check sends recheck", async () => {
    const { root, backend } = await mountPanel("options");
    root.byKey("recheck").dispatch("click");
    await flush();
    assert.deepEqual(backend.sent[1], { type: "recheck" });
  });

  test("the rules textarea holds the saved text and a counter against the 256 KiB cap", async () => {
    const { root } = await mountPanel("options", baseState({ userRules: "block host:a.example\n" }));
    assert.equal(root.byKey("rules").value, "block host:a.example\n");
    assert.match(root.byKey("rules-count").textContent, /21 of 262,144/);
  });

  test("typing compiles in the page and lists errors as 'line N: message'", async () => {
    const { root, backend } = await mountPanel("options");
    const area = root.byKey("rules");
    area.value = "block host:a.example\nblock host:nodots";
    area.dispatch("input");
    const errors = root.byKey("errors");
    assert.ok(errors.textContent.includes("line 2: "), errors.textContent);
    assert.equal(backend.sent.length, 1, "typing sends nothing");
  });

  test("clicking an error puts the cursor on that line", async () => {
    const { root } = await mountPanel("options");
    const area = root.byKey("rules");
    area.value = "block host:a.example\nblock host:nodots";
    area.dispatch("input");
    let selected = null;
    area.setSelectionRange = (a, b) => (selected = [a, b]);
    root.byKey("errors").all((n) => n.tagName === "BUTTON")[0].dispatch("click");
    assert.deepEqual(selected, [21, 38]);
    assert.equal(area.focused, true);
  });

  test("Save sends the text even when it has errors, and the errors stay visible", async () => {
    const { root, backend } = await mountPanel("options");
    const area = root.byKey("rules");
    area.value = "block host:a.example\nblock host:nodots";
    area.dispatch("input");
    root.byKey("save-rules").dispatch("click");
    await flush();
    assert.deepEqual(backend.sent[1], { type: "saveRules", text: "block host:a.example\nblock host:nodots" });
    assert.ok(root.byKey("errors").textContent.includes("line 2: "));
    assert.equal(root.byKey("rules").value, "block host:a.example\nblock host:nodots");
  });

  test("the dropped-rules banner sits above the rules section", async () => {
    const { root } = await mountPanel("options", baseState({ dropped: [{ line: 7, reason: "budget" }] }));
    const text = root.textContent;
    const banner = text.indexOf("1 rule is not being enforced, starting at line 7.");
    assert.ok(banner >= 0);
    assert.ok(banner < text.indexOf("My rules"));
  });

  test("no banner and no text about dropped rules when none were dropped", async () => {
    const { root } = await mountPanel("options");
    assert.ok(!root.textContent.includes("not being enforced"));
  });

  test("Format help is a disclosure that holds the tables and the party default", async () => {
    const { root } = await mountPanel("options");
    const details = root.all((n) => n.tagName === "DETAILS")[0];
    assert.ok(details.textContent.includes("Format help"));
    assert.ok(details.textContent.includes("party any"));
    assert.ok(details.textContent.includes("third"));
    assert.ok(details.textContent.includes("punycode"));
  });

  test("sites never filtered: lists, adds a valid host and removes one", async () => {
    const { root, backend } = await mountPanel("options", baseState({ siteOff: ["bank.example"] }));
    assert.ok(root.textContent.includes("bank.example"));
    root.byKey("site-input").value = "intranet.example";
    root.byKey("site-add").dispatch("click");
    await flush();
    assert.deepEqual(backend.sent[1], { type: "saveSiteOff", list: ["bank.example", "intranet.example"] });
    root.byKey("site-remove:bank.example").dispatch("click");
    await flush();
    assert.deepEqual(backend.sent[2], { type: "saveSiteOff", list: ["intranet.example"] });
  });

  test("an invalid site is refused with a reason and nothing is sent", async () => {
    const { root, backend } = await mountPanel("options");
    root.byKey("site-input").value = "localhost";
    root.byKey("site-add").dispatch("click");
    await flush();
    assert.equal(backend.sent.length, 1);
    assert.match(root.byKey("site-error").textContent, /two or more/i);
  });

  test("a site is lower-cased and trimmed before it is checked", async () => {
    const { root, backend } = await mountPanel("options");
    root.byKey("site-input").value = "  Bank.Example ";
    root.byKey("site-add").dispatch("click");
    await flush();
    assert.deepEqual(backend.sent[1], { type: "saveSiteOff", list: ["bank.example"] });
  });

  test("the list is capped at 500", async () => {
    const full = Array.from({ length: 500 }, (_, i) => "s" + i + ".example");
    const { root, backend } = await mountPanel("options", baseState({ siteOff: full }));
    root.byKey("site-input").value = "extra.example";
    root.byKey("site-add").dispatch("click");
    await flush();
    assert.equal(backend.sent.length, 1);
    assert.match(root.byKey("site-error").textContent, /500/);
  });

  test("a site already in the list is not added twice", async () => {
    const { root, backend } = await mountPanel("options", baseState({ siteOff: ["bank.example"] }));
    root.byKey("site-input").value = "bank.example";
    root.byKey("site-add").dispatch("click");
    await flush();
    assert.equal(backend.sent.length, 1);
    assert.ok(root.byKey("site-error").textContent.length > 0);
  });

  test("the starter checkbox sends setStarterEnabled, and the starter text is shown read-only", async () => {
    const { root, backend } = await mountPanel("options");
    const box = root.byKey("starter");
    assert.equal(box.checked, true);
    const view = root.byKey("starter-text");
    assert.ok(view.textContent.includes("block label:adserver"));
    assert.ok(view.getAttribute("readonly") !== null || view.readOnly === true);
    box.checked = false;
    box.dispatch("change");
    await flush();
    assert.deepEqual(backend.sent[1], { type: "setStarterEnabled", value: false });
  });

  test("changing the shortcut validates it before sending", async () => {
    const { root, backend } = await mountPanel("options");
    root.byKey("chord").value = "S";
    root.byKey("chord-save").dispatch("click");
    await flush();
    assert.equal(backend.sent.length, 1);
    assert.ok(root.byKey("chord-error").textContent.length > 0);
    root.byKey("chord").value = "Alt+Shift+K";
    root.byKey("chord-save").dispatch("click");
    await flush();
    assert.deepEqual(backend.sent[1], { type: "setChord", value: "Alt+Shift+K" });
  });

  test("the bad-selector tally is shown when it is not zero", async () => {
    const { root } = await mountPanel("options", baseState({ badSelectors: 3 }));
    assert.ok(/3 selectors? .*skipped/i.test(root.textContent), root.textContent);
  });

  test("a read-only state disables Save", async () => {
    const { root } = await mountPanel("options", baseState({ readOnly: true }));
    assert.equal(root.byKey("save-rules").disabled, true);
  });
});

// ---------------------------------------------------------------------------
// The service worker, run against a fake chrome. Section 9.7 says a real browser is the real check;
// this covers the pipeline's own logic (probe, sync, caps, counting) so only the engine's behaviour
// is left for a hand check.
// ---------------------------------------------------------------------------

const vm = require("node:vm");

function makeChrome(o = {}) {
  const store = JSON.parse(JSON.stringify(o.storage || {}));
  const events = {};
  const calls = { updateDynamicRules: [], updateEnabledRulesets: [], badge: [], tabMessages: [] };
  const ev = (name) => {
    events[name] = [];
    return { addListener: (fn) => events[name].push(fn), removeListener() {}, hasListener: () => false };
  };
  const fire = (name, ...args) => Promise.all((events[name] || []).map((fn) => fn(...args)));
  const clone = (x) => (x === undefined ? undefined : JSON.parse(JSON.stringify(x)));
  const dyn = { rules: (o.dynamicRules || []).slice() };
  let enabledRulesets = o.staticEnabled === false ? [] : ["starter"];

  const dnr = o.noDnr
    ? undefined
    : {
        updateDynamicRules: async ({ removeRuleIds = [], addRules = [] }) => {
          calls.updateDynamicRules.push({ removeRuleIds: removeRuleIds.slice(), addRules: clone(addRules) });
          if (o.dynamicThrows) throw new Error("dynamic rules unavailable");
          const bad = addRules.find((r) => (o.rejectIds || []).includes(r.id));
          if (bad) throw new Error("rule " + bad.id + " rejected");
          dyn.rules = dyn.rules.filter((r) => !removeRuleIds.includes(r.id)).concat(clone(addRules));
        },
        getDynamicRules: async () => (o.dynamicIgnores ? [] : clone(dyn.rules)),
        updateEnabledRulesets: async ({ enableRulesetIds = [], disableRulesetIds = [] }) => {
          calls.updateEnabledRulesets.push({ enableRulesetIds, disableRulesetIds });
          enabledRulesets = enabledRulesets.filter((x) => !disableRulesetIds.includes(x));
          for (const x of enableRulesetIds) if (!enabledRulesets.includes(x)) enabledRulesets.push(x);
        },
        getEnabledRulesets: async () => enabledRulesets.slice(),
        onRuleMatchedDebug: o.noFeedback ? undefined : ev("matched")
      };

  const chrome = {
    runtime: {
      id: "ext-id",
      getURL: (p) => "ext:///" + p,
      onInstalled: ev("installed"),
      onStartup: ev("startup"),
      onMessage: ev("message")
    },
    storage: {
      local: {
        get: async (keys) => {
          if (keys === null || keys === undefined) return clone(store);
          const list = typeof keys === "string" ? [keys] : Array.isArray(keys) ? keys : Object.keys(keys);
          const out = {};
          for (const k of list) {
            if (k in store) out[k] = clone(store[k]);
            else if (!Array.isArray(keys) && typeof keys === "object") out[k] = clone(keys[k]);
          }
          return out;
        },
        set: async (obj) => {
          if (o.storageThrows) throw new Error("quota");
          const changes = {};
          for (const k of Object.keys(obj)) changes[k] = { oldValue: clone(store[k]), newValue: clone(obj[k]) };
          for (const k of Object.keys(obj)) store[k] = clone(obj[k]);
          await fire("storageChanged", changes, "local");
        }
      },
      onChanged: ev("storageChanged")
    },
    declarativeNetRequest: dnr,
    tabs: {
      query: async () => (o.activeTab ? [o.activeTab] : []),
      onUpdated: ev("tabUpdated"),
      onRemoved: ev("tabRemoved"),
      sendMessage: async (id, msg, opts) => {
        calls.tabMessages.push({ id, msg, opts });
      }
    },
    action: {
      setBadgeText: async (a) => {
        calls.badge.push(a);
      },
      setBadgeBackgroundColor: async () => {}
    }
  };
  return {
    chrome,
    store,
    calls,
    fire,
    dynamicRules: () => clone(dyn.rules),
    enabledRulesets: () => enabledRulesets.slice(),
    send(msg, sender) {
      return new Promise((resolve) => {
        const listener = events.message[0];
        const ret = listener(msg, sender || { id: "ext-id" }, resolve);
        if (ret !== true) resolve(undefined);
      });
    }
  };
}

const SW_SOURCE = fs.readFileSync(path.join(ROOT, "background", "service-worker.js"), "utf8");

async function bootWorker(opts) {
  const fake = makeChrome(opts);
  const selfObj = {};
  const importScripts = (...files) => {
    for (const f of files) {
      const src = fs.readFileSync(path.join(ROOT, "background", f), "utf8");
      vm.runInThisContext("(function(self){" + src + "\n})", { filename: f })(selfObj);
    }
  };
  const fetchFn = async (url) => ({
    ok: true,
    text: async () => fs.readFileSync(path.join(ROOT, url.replace("ext:///", "")), "utf8")
  });
  vm.runInThisContext("(function(self, chrome, importScripts, fetch){" + SW_SOURCE + "\n})", { filename: "service-worker.js" })(
    selfObj,
    fake.chrome,
    importScripts,
    fetchFn
  );
  if (!(opts && opts.skipInstall)) await fake.fire("installed", { reason: "install" });
  return fake;
}

const topFrame = (tabId, url) => ({ id: "ext-id", tab: { id: tabId, url }, frameId: 0, url });
const subFrame = (tabId, tabUrl, frameId, url) => ({ id: "ext-id", tab: { id: tabId, url: tabUrl }, frameId, url });
const j = (x) => JSON.parse(JSON.stringify(x));

describe("service worker: first run and the probe", () => {
  test("writes every default from section 7.4 and finishes with the engine mode set", async () => {
    const w = await bootWorker();
    assert.equal(w.store["shield.version"], 1);
    assert.equal(w.store["shield.enabled"], true);
    assert.equal(w.store["shield.starterEnabled"], true);
    assert.equal(w.store["shield.userRules"], "");
    assert.deepEqual(w.store["shield.siteOff"], []);
    assert.equal(w.store["shield.engineMode"], "network");
    assert.equal(w.store["shield.panelChord"], "Alt+Shift+S");
    assert.equal(w.store["shield.cosmetic"].global.length, 10);
    assert.deepEqual(Object.keys(w.store["shield.compiled"]).sort(), ["counts", "dropped", "errors", "sourceHash", "warnings"]);
  });

  test("the probe round-trips rule 999 and leaves nothing behind", async () => {
    const w = await bootWorker();
    const first = w.calls.updateDynamicRules[0];
    assert.deepEqual(first.removeRuleIds, [999]);
    assert.equal(first.addRules[0].id, 999);
    assert.equal(first.addRules[0].condition.requestDomains[0], "probe.bowser-shield.invalid");
    assert.ok(!w.dynamicRules().some((r) => r.id === 999));
  });

  test("a runtime with no declarativeNetRequest is dom mode", async () => {
    const w = await bootWorker({ noDnr: true });
    assert.equal(w.store["shield.engineMode"], "dom");
  });

  test("a runtime whose dynamic update throws is dom mode", async () => {
    const w = await bootWorker({ dynamicThrows: true });
    assert.equal(w.store["shield.engineMode"], "dom");
  });

  test("a runtime that accepts a rule and never returns it is dom mode", async () => {
    const w = await bootWorker({ dynamicIgnores: true });
    assert.equal(w.store["shield.engineMode"], "dom");
  });

  test("the probe does not run again on a later start when the mode is already stored", async () => {
    const w = await bootWorker({
      skipInstall: true,
      storage: { "shield.version": 1, "shield.engineMode": "network", "shield.compiled": { sourceHash: "x", counts: {}, errors: [], warnings: [], dropped: [] } }
    });
    await w.send({ type: "getState" });
    assert.equal(w.calls.updateDynamicRules.length, 0);
  });

  test("Re-check forces the probe again and stores the result", async () => {
    const w = await bootWorker();
    const before = w.calls.updateDynamicRules.length;
    const state = await w.send({ type: "recheck" });
    assert.ok(w.calls.updateDynamicRules.length > before);
    assert.equal(state.engineMode, "network");
  });
});

describe("service worker: syncing rules", () => {
  test("saveRules writes the user rules into the dynamic bands and stores the compile summary", async () => {
    const w = await bootWorker();
    const state = await w.send({
      type: "saveRules",
      text: "block host:ads.shop.example party any\nallow label:visitor-track only intranet.example\nblock host:nodots\nhide css:div.promo"
    });
    assert.equal(w.store["shield.userRules"].split("\n").length, 4);
    const ids = w.dynamicRules().map((r) => r.id).sort((a, b) => a - b);
    assert.deepEqual(ids, [10000, 20000]);
    assert.deepEqual(j(w.store["shield.compiled"].counts), { block: 1, allow: 1, hide: 1, off: 0 });
    assert.deepEqual(w.store["shield.compiled"].errors.map((e) => [e.line, e.code]), [[3, "bad-host"]]);
    assert.deepEqual(state.errors.map((e) => e.line), [3]);
    assert.ok(w.store["shield.cosmetic"].global.includes("div.promo"));
    assert.equal(w.store["shield.cosmetic"].global.length, 11);
  });

  test("removes every dynamic id that is there, including an orphan it did not write", async () => {
    const orphan = { id: 12345, priority: 1, action: { type: "block" }, condition: { requestDomains: ["old.example"] } };
    const w = await bootWorker();
    await w.chrome.declarativeNetRequest.updateDynamicRules({ removeRuleIds: [], addRules: [orphan] });
    assert.ok(w.dynamicRules().some((r) => r.id === 12345));
    await w.send({ type: "saveRules", text: "block host:a.example" });
    const last = w.calls.updateDynamicRules[w.calls.updateDynamicRules.length - 1];
    assert.ok(last.removeRuleIds.includes(12345));
    assert.deepEqual(w.dynamicRules().map((r) => r.id), [10000]);
  });

  test("the dynamic rules go in and the old ones come out in one call", async () => {
    const w = await bootWorker();
    await w.send({ type: "saveRules", text: "block host:a.example" });
    const last = w.calls.updateDynamicRules[w.calls.updateDynamicRules.length - 1];
    assert.equal(last.addRules.length, 1);
    assert.ok(Array.isArray(last.removeRuleIds));
  });

  test("in dom mode no dynamic rule is ever written and the static starter ruleset stays on while the shield and the starter checkbox are on", async () => {
    const w = await bootWorker({ noDnr: true });
    await w.send({ type: "saveRules", text: "block host:a.example" });
    assert.deepEqual(w.dynamicRules(), []);
    const dom = await bootWorker({ dynamicThrows: true });
    await dom.send({ type: "saveRules", text: "block host:a.example" });
    assert.deepEqual(dom.dynamicRules(), []);
    assert.equal(dom.store["shield.engineMode"], "dom");
    assert.equal(dom.enabledRulesets().includes("starter"), true);
    await dom.send({ type: "setStarterEnabled", value: false });
    assert.equal(dom.enabledRulesets().includes("starter"), false);
    await dom.send({ type: "setStarterEnabled", value: true });
    assert.equal(dom.enabledRulesets().includes("starter"), true);
    await dom.send({ type: "setEnabled", value: false });
    assert.equal(dom.enabledRulesets().includes("starter"), false);
  });

  test("the starter ruleset follows the switch and the starter checkbox", async () => {
    const w = await bootWorker();
    assert.deepEqual(w.enabledRulesets(), ["starter"]);
    await w.send({ type: "setEnabled", value: false });
    assert.deepEqual(w.enabledRulesets(), []);
    await w.send({ type: "setEnabled", value: true });
    assert.deepEqual(w.enabledRulesets(), ["starter"]);
    await w.send({ type: "setStarterEnabled", value: false });
    assert.deepEqual(w.enabledRulesets(), []);
    assert.equal(w.store["shield.starterEnabled"], false);
  });

  test("turning the shield off removes the user's dynamic rules, and on puts them back", async () => {
    const w = await bootWorker();
    await w.send({ type: "saveRules", text: "block host:a.example" });
    await w.send({ type: "setEnabled", value: false });
    assert.deepEqual(w.dynamicRules(), []);
    assert.equal(w.store["shield.enabled"], false);
    await w.send({ type: "setEnabled", value: true });
    assert.deepEqual(w.dynamicRules().map((r) => r.id), [10000]);
  });

  test("without the starter list the cosmetic data holds only the user's selectors", async () => {
    const w = await bootWorker();
    await w.send({ type: "saveRules", text: "hide css:div.mine" });
    await w.send({ type: "setStarterEnabled", value: false });
    assert.deepEqual(w.store["shield.cosmetic"].global, ["div.mine"]);
  });

  test("over-budget labels are reported as dropped, leaving room for the starter's own regexes", async () => {
    const w = await bootWorker();
    const lines = [];
    for (let i = 0; i < 1000; i++) lines.push("block label:trk" + String(i).padStart(4, "x"));
    const state = await w.send({ type: "saveRules", text: lines.join("\n") });
    const regexRules = w.dynamicRules().filter((r) => r.condition.regexFilter).length;
    assert.equal(regexRules, 974, "1000 minus the 26 label rules of the static starter ruleset");
    assert.equal(state.dropped.length, 26);
    assert.ok(state.dropped.every((d) => d.reason === "regex-cap"));
    assert.equal(w.store["shield.compiled"].dropped.length, 26);
    assert.equal(state.dropped[0].line, 975);
  });

  test("with the starter off, the whole 1000-regex allowance is the user's", async () => {
    const w = await bootWorker();
    await w.send({ type: "setStarterEnabled", value: false });
    const lines = [];
    for (let i = 0; i < 1000; i++) lines.push("block label:trk" + String(i).padStart(4, "x"));
    const state = await w.send({ type: "saveRules", text: lines.join("\n") });
    assert.equal(state.dropped.length, 0);
  });

  test("a rule the browser rejects is dropped on its own and reported, and the rest still apply", async () => {
    const w = await bootWorker({ rejectIds: [10001] });
    const state = await w.send({ type: "saveRules", text: "block host:a.example\nblock host:b.example\nblock host:c.example" });
    assert.deepEqual(w.dynamicRules().map((r) => r.id).sort(), [10000, 10002]);
    assert.deepEqual(j(state.dropped), [{ line: 2, reason: "rejected-by-browser", list: "user" }]);
  });

  test("saving text over 256 KiB is refused with a message and changes nothing", async () => {
    const w = await bootWorker();
    await w.send({ type: "saveRules", text: "block host:a.example" });
    const state = await w.send({ type: "saveRules", text: "#".repeat(262145) });
    assert.match(state.error, /256 KiB/);
    assert.equal(w.store["shield.userRules"], "block host:a.example");
  });

  test("saveRules with something that is not text is refused", async () => {
    const w = await bootWorker();
    const state = await w.send({ type: "saveRules", text: 42 });
    assert.ok(state.error);
    assert.equal(w.store["shield.userRules"], "");
  });

  test("a failing storage write is reported and does not leave a half-applied state", async () => {
    const w = await bootWorker();
    w.chrome.storage.local.set = async () => {
      throw new Error("quota");
    };
    const state = await w.send({ type: "saveRules", text: "block host:a.example" });
    assert.ok(state.error);
    assert.deepEqual(w.dynamicRules(), []);
  });
});

describe("service worker: site exceptions", () => {
  test("setSiteOff adds the host and writes an allowAllRequests rule in the off band", async () => {
    const w = await bootWorker();
    await w.send({ type: "setSiteOff", host: "bank.example", off: true });
    assert.deepEqual(w.store["shield.siteOff"], ["bank.example"]);
    const off = w.dynamicRules().filter((r) => r.action.type === "allowAllRequests");
    assert.deepEqual(off.map((r) => [r.id, r.condition.requestDomains[0]]), [[30000, "bank.example"]]);
  });

  test("frameContext says filtered false for the exempt site and its subdomains, true elsewhere", async () => {
    const w = await bootWorker();
    await w.send({ type: "setSiteOff", host: "bank.example", off: true });
    const bank = await w.send({ type: "frameContext" }, topFrame(1, "https://www.bank.example/login"));
    assert.equal(bank.filtered, false);
    assert.equal(bank.siteHost, "www.bank.example");
    const news = await w.send({ type: "frameContext" }, topFrame(2, "https://news.example/"));
    assert.equal(news.filtered, true);
  });

  test("a subframe is judged by the top-level site, not its own host", async () => {
    const w = await bootWorker();
    await w.send({ type: "setSiteOff", host: "bank.example", off: true });
    const inBank = await w.send({ type: "frameContext" }, subFrame(1, "https://bank.example/", 3, "https://ads.partner.test/frame"));
    assert.equal(inBank.filtered, false);
    assert.equal(inBank.siteHost, "bank.example");
    const inNews = await w.send({ type: "frameContext" }, subFrame(2, "https://news.example/", 3, "https://bank.example/frame"));
    assert.equal(inNews.filtered, true);
    assert.equal(inNews.siteHost, "news.example");
  });

  test("'Filter this site' removes every exemption that covers the host", async () => {
    const w = await bootWorker({ storage: { "shield.version": 1, "shield.siteOff": ["news.example", "other.example"] } });
    await w.send({ type: "setSiteOff", host: "www.news.example", off: false });
    assert.deepEqual(w.store["shield.siteOff"], ["other.example"]);
  });

  test("exempting a site that is already covered does not add a duplicate", async () => {
    const w = await bootWorker();
    await w.send({ type: "setSiteOff", host: "news.example", off: true });
    await w.send({ type: "setSiteOff", host: "www.news.example", off: true });
    assert.deepEqual(w.store["shield.siteOff"], ["news.example"]);
  });

  test("an off rule in My rules also exempts the site, and getState says it cannot be undone from the button", async () => {
    const w = await bootWorker();
    await w.send({ type: "saveRules", text: "off site:rule.example" });
    const ctx = await w.send({ type: "frameContext" }, topFrame(1, "https://rule.example/"));
    assert.equal(ctx.filtered, false);
    const state = await w.send({ type: "getState" }, topFrame(1, "https://rule.example/"));
    assert.equal(state.page.siteOff, true);
    assert.equal(state.page.siteOffRuleOnly, true);
  });

  test("an invalid host is refused", async () => {
    const w = await bootWorker();
    const state = await w.send({ type: "setSiteOff", host: "localhost", off: true });
    assert.ok(state.error);
    assert.deepEqual(w.store["shield.siteOff"], []);
  });

  test("saveSiteOff validates, lower-cases and dedupes, and caps at 500", async () => {
    const w = await bootWorker();
    await w.send({ type: "saveSiteOff", list: ["B.example", "b.example", "c.example"] });
    assert.deepEqual(w.store["shield.siteOff"], ["b.example", "c.example"]);
    const bad = await w.send({ type: "saveSiteOff", list: ["ok.example", "nodots"] });
    assert.ok(bad.error);
    assert.deepEqual(w.store["shield.siteOff"], ["b.example", "c.example"]);
    const tooMany = await w.send({ type: "saveSiteOff", list: Array.from({ length: 501 }, (_, i) => "s" + i + ".example") });
    assert.match(tooMany.error, /500/);
    const notArray = await w.send({ type: "saveSiteOff", list: "x.example" });
    assert.ok(notArray.error);
  });

  test("a disabled shield filters nothing", async () => {
    const w = await bootWorker();
    await w.send({ type: "setEnabled", value: false });
    const ctx = await w.send({ type: "frameContext" }, topFrame(1, "https://news.example/"));
    assert.equal(ctx.enabled, false);
    assert.equal(ctx.filtered, false);
  });
});

describe("service worker: frameContext", () => {
  test("answers from the top-level tab URL, with the mode", async () => {
    const w = await bootWorker();
    const ctx = await w.send({ type: "frameContext" }, topFrame(1, "https://News.Example:8443/a?b=1"));
    assert.deepEqual(j(ctx), { siteHost: "news.example", enabled: true, mode: "network", filtered: true });
  });

  test("in dom mode the reply carries the starter source for the content script's matcher", async () => {
    const w = await bootWorker({ noDnr: true });
    const ctx = await w.send({ type: "frameContext" }, topFrame(1, "https://news.example/"));
    assert.equal(ctx.mode, "dom");
    assert.ok(ctx.starterSrl.includes("block label:adserver"));
    await w.send({ type: "setStarterEnabled", value: false });
    const without = await w.send({ type: "frameContext" }, topFrame(1, "https://news.example/"));
    assert.equal(without.starterSrl, "");
  });

  test("a page that is not http or https is not filtered", async () => {
    const w = await bootWorker();
    const ctx = await w.send({ type: "frameContext" }, topFrame(1, "chrome://newtab/"));
    assert.equal(ctx.filtered, false);
    assert.equal(ctx.siteHost, null);
  });

  test("a message with no tab is judged by the sender URL", async () => {
    const w = await bootWorker();
    const ctx = await w.send({ type: "frameContext" }, { id: "ext-id", url: "https://news.example/x" });
    assert.equal(ctx.siteHost, "news.example");
  });
});

describe("service worker: counting", () => {
  const matched = (w, ruleId, requestId, tabId, rulesetId) =>
    w.fire("matched", { rule: { ruleId, rulesetId: rulesetId || "_dynamic" }, request: { requestId, tabId, url: "https://x.example/", type: "script" } });

  test("counts only block-band matches, not allow or allowAllRequests", async () => {
    const w = await bootWorker();
    await w.fire("tabUpdated", 7, { url: "https://news.example/" }, { id: 7 });
    await matched(w, 1005, "r1", 7, "starter");
    await matched(w, 10003, "r2", 7);
    await matched(w, 2000, "r3", 7, "starter");
    await matched(w, 20001, "r4", 7);
    await matched(w, 30000, "r5", 7);
    const state = await w.send({ type: "getState" }, topFrame(7, "https://news.example/"));
    assert.equal(state.page.blocked, 2);
    assert.equal(state.session.blocked, 2);
  });

  test("a repeated request id is counted once", async () => {
    const w = await bootWorker();
    await w.fire("tabUpdated", 7, { url: "https://news.example/" }, { id: 7 });
    await matched(w, 1005, "same", 7, "starter");
    await matched(w, 1005, "same", 7, "starter");
    const state = await w.send({ type: "getState" }, topFrame(7, "https://news.example/"));
    assert.equal(state.page.blocked, 1);
  });

  test("the seen-set per tab is bounded to 4096 request ids", async () => {
    const w = await bootWorker();
    await w.fire("tabUpdated", 7, { url: "https://news.example/" }, { id: 7 });
    for (let i = 0; i < 4100; i++) await matched(w, 1005, "id" + i, 7, "starter");
    await matched(w, 1005, "id0", 7, "starter");
    const state = await w.send({ type: "getState" }, topFrame(7, "https://news.example/"));
    assert.equal(state.page.blocked, 4101, "id0 fell out of the window, so it counts again");
    await matched(w, 1005, "id4099", 7, "starter");
    const after = await w.send({ type: "getState" }, topFrame(7, "https://news.example/"));
    assert.equal(after.page.blocked, 4101, "a recent id is still remembered");
  });

  test("requests with no tab are ignored", async () => {
    const w = await bootWorker();
    await matched(w, 1005, "r1", -1, "starter");
    const state = await w.send({ type: "getState" }, topFrame(7, "https://news.example/"));
    assert.equal(state.session.blocked, 0);
  });

  test("tabs are counted separately and the session adds them up", async () => {
    const w = await bootWorker();
    await w.fire("tabUpdated", 1, { url: "https://a.example/" }, { id: 1 });
    await w.fire("tabUpdated", 2, { url: "https://b.example/" }, { id: 2 });
    await matched(w, 1000, "a", 1, "starter");
    await matched(w, 1000, "b", 2, "starter");
    await matched(w, 1000, "c", 2, "starter");
    const one = await w.send({ type: "getState" }, topFrame(1, "https://a.example/"));
    const two = await w.send({ type: "getState" }, topFrame(2, "https://b.example/"));
    assert.deepEqual([one.page.blocked, two.page.blocked, two.session.blocked], [1, 2, 3]);
  });

  test("a committed navigation to a different host resets the tab, the same host does not", async () => {
    const w = await bootWorker();
    await w.fire("tabUpdated", 7, { url: "https://news.example/a" }, { id: 7 });
    await matched(w, 1000, "a", 7, "starter");
    await w.fire("tabUpdated", 7, { url: "https://news.example/b" }, { id: 7 });
    let state = await w.send({ type: "getState" }, topFrame(7, "https://news.example/b"));
    assert.equal(state.page.blocked, 1);
    await w.fire("tabUpdated", 7, { url: "https://other.example/" }, { id: 7 });
    state = await w.send({ type: "getState" }, topFrame(7, "https://other.example/"));
    assert.equal(state.page.blocked, 0);
    assert.equal(state.session.blocked, 1, "the session total is not reset");
  });

  test("a tab that closes is forgotten", async () => {
    const w = await bootWorker();
    await w.fire("tabUpdated", 7, { url: "https://news.example/" }, { id: 7 });
    await matched(w, 1000, "a", 7, "starter");
    await w.fire("tabRemoved", 7, {});
    const state = await w.send({ type: "getState" }, topFrame(7, "https://news.example/"));
    assert.equal(state.page.blocked, 0);
  });

  test("the badge shows the tab's blocked count", async () => {
    const w = await bootWorker();
    await w.fire("tabUpdated", 7, { url: "https://news.example/" }, { id: 7 });
    await matched(w, 1000, "a", 7, "starter");
    await matched(w, 1000, "b", 7, "starter");
    assert.deepEqual(w.calls.badge[w.calls.badge.length - 1], { tabId: 7, text: "2" });
  });

  test("when onRuleMatchedDebug is missing, counts are reported unavailable and never estimated", async () => {
    const w = await bootWorker({ noFeedback: true });
    const state = await w.send({ type: "getState" }, topFrame(7, "https://news.example/"));
    assert.equal(state.countsAvailable, false);
    assert.equal(state.page.blocked, 0);
  });

  test("with the feedback event present, counts are available in network mode", async () => {
    const w = await bootWorker();
    const state = await w.send({ type: "getState" }, topFrame(7, "https://news.example/"));
    assert.equal(state.countsAvailable, true);
  });

  test("dom mode counts hidden URLs from the content script, and never the network events", async () => {
    const w = await bootWorker({ noDnr: true });
    await w.fire("tabUpdated", 7, { url: "https://news.example/" }, { id: 7 });
    await w.send({ type: "hidden", urls: ["https://a.test/1", "https://a.test/2", "https://a.test/3"] }, subFrame(7, "https://news.example/", 2, "https://x.test/"));
    const state = await w.send({ type: "getState" }, topFrame(7, "https://news.example/"));
    assert.equal(state.engineMode, "dom");
    assert.equal(state.page.hidden, 3);
    assert.equal(state.page.blocked, 0);
    assert.equal(state.session.hidden, 3);
  });

  test("a hidden report that is not a list of strings is ignored", async () => {
    const w = await bootWorker({ noDnr: true });
    await w.send({ type: "hidden", urls: "nope" }, topFrame(7, "https://news.example/"));
    await w.send({ type: "hidden" }, topFrame(7, "https://news.example/"));
    const state = await w.send({ type: "getState" }, topFrame(7, "https://news.example/"));
    assert.equal(state.page.hidden, 0);
  });

  test("bad selectors are summed over the tab's frames, each frame reporting its own total", async () => {
    const w = await bootWorker();
    await w.send({ type: "badSelectors", count: 2 }, subFrame(7, "https://news.example/", 0, "https://news.example/"));
    await w.send({ type: "badSelectors", count: 1 }, subFrame(7, "https://news.example/", 4, "https://x.test/"));
    await w.send({ type: "badSelectors", count: 3 }, subFrame(7, "https://news.example/", 0, "https://news.example/"));
    const state = await w.send({ type: "getState" }, topFrame(7, "https://news.example/"));
    assert.equal(state.badSelectors, 4);
  });
});

describe("service worker: state for the panel", () => {
  test("getState from a page describes the page, the session, the lists and the caveats", async () => {
    const w = await bootWorker();
    await w.send({ type: "saveRules", text: "block host:a.example" });
    const s = await w.send({ type: "getState" }, topFrame(7, "https://news.example/story"));
    assert.equal(s.enabled, true);
    assert.equal(s.starterEnabled, true);
    assert.equal(s.userRules, "block host:a.example");
    assert.equal(s.engineMode, "network");
    assert.equal(s.readOnly, false);
    assert.deepEqual(j(s.page), { supported: true, host: "news.example", siteOff: false, siteOffRuleOnly: false, canExempt: true, blocked: 0, hidden: 0 });
    assert.deepEqual(j(s.counts.starter), { block: 38, allow: 0, hide: 10, off: 0 });
    assert.deepEqual(j(s.counts.user), { block: 1, allow: 0, hide: 0, off: 0 });
    assert.ok(s.starterSrl.includes("block label:adserver"));
    assert.equal(s.chord, "Alt+Shift+S");
  });

  test("from the popup (no tab on the sender) it uses the active tab", async () => {
    const w = await bootWorker({ activeTab: { id: 3, url: "https://active.example/" } });
    const s = await w.send({ type: "getState" }, { id: "ext-id", url: "ext:///ui/popup.html" });
    assert.equal(s.page.host, "active.example");
  });

  test("a non-http tab is reported as not supported", async () => {
    const w = await bootWorker();
    const s = await w.send({ type: "getState" }, topFrame(7, "about:blank"));
    assert.equal(s.page.supported, false);
  });

  test("no tab at all gives no page", async () => {
    const w = await bootWorker();
    const s = await w.send({ type: "getState" }, { id: "ext-id", url: "ext:///ui/options.html" });
    assert.equal(s.page, null);
  });

  test("a host that cannot be a site rule cannot be exempted", async () => {
    const w = await bootWorker();
    const s = await w.send({ type: "getState" }, topFrame(7, "http://localhost:3000/"));
    assert.equal(s.page.supported, true);
    assert.equal(s.page.canExempt, false);
  });

  test("a mutation answers with the fresh state", async () => {
    const w = await bootWorker();
    const s = await w.send({ type: "setEnabled", value: false }, topFrame(7, "https://news.example/"));
    assert.equal(s.enabled, false);
    assert.equal(s.page.host, "news.example");
  });
});

describe("service worker: the chord and messaging", () => {
  test("a chord pressed in a subframe is forwarded to the top frame", async () => {
    const w = await bootWorker();
    await w.send({ type: "chord" }, subFrame(7, "https://news.example/", 3, "https://x.test/"));
    assert.deepEqual(j(w.calls.tabMessages), [{ id: 7, msg: { type: "togglePanel" }, opts: { frameId: 0 } }]);
  });

  test("setChord stores a valid chord and refuses an invalid one", async () => {
    const w = await bootWorker();
    const ok = await w.send({ type: "setChord", value: "Alt+Shift+K" });
    assert.equal(ok.chord, "Alt+Shift+K");
    assert.equal(w.store["shield.panelChord"], "Alt+Shift+K");
    const bad = await w.send({ type: "setChord", value: "K" });
    assert.ok(bad.error);
    assert.equal(w.store["shield.panelChord"], "Alt+Shift+K");
  });

  test("setChord accepts exactly the chords the panel's parseChord accepts", async () => {
    const candidates = [
      "Alt+Shift+S", "Alt+S", "Ctrl+K", "Control+K", "Meta+F12", "ctrl+alt+7", "Alt + S", "Alt+Shift+F1",
      "Shift+S", "Alt+Alt+S", "Shift+Shift+Alt+S", "Alt+", "Alt", "S", "Alt+Shift", "Alt+F13", "Alt+F0",
      "Alt+SS", "Alt+S+D", "Alt+Ctrl+Control+S", "Alt+;", "Alt+Space"
    ];
    for (const value of candidates) {
      const w = await bootWorker();
      const reply = await w.send({ type: "setChord", value });
      const accepted = !reply.error;
      assert.equal(accepted, ShieldPanel.parseChord(value.trim()) !== null, JSON.stringify(value));
    }
  });

  test("setChord refuses a chord with no Alt, Ctrl or Meta, and one that repeats a modifier", async () => {
    const w = await bootWorker();
    for (const value of ["Shift+S", "Alt+Alt+S"]) {
      const bad = await w.send({ type: "setChord", value });
      assert.ok(bad.error, value);
      assert.equal(w.store["shield.panelChord"], "Alt+Shift+S", value);
    }
  });

  test("an unknown message type gets an error reply, not silence", async () => {
    const w = await bootWorker();
    const r = await w.send({ type: "nope" });
    assert.ok(r && r.error);
    const none = await w.send(null);
    assert.ok(none && none.error);
  });

  test("a message from another extension is refused", async () => {
    const w = await bootWorker();
    const r = await w.send({ type: "saveRules", text: "block host:evil.example" }, { id: "someone-else" });
    assert.ok(r.error);
    assert.equal(w.store["shield.userRules"], "");
  });

  test("mutations are applied one at a time, in order", async () => {
    const w = await bootWorker();
    await Promise.all([
      w.send({ type: "saveRules", text: "block host:a.example" }),
      w.send({ type: "saveRules", text: "block host:b.example" }),
      w.send({ type: "saveRules", text: "block host:c.example" })
    ]);
    assert.equal(w.store["shield.userRules"], "block host:c.example");
    assert.equal(w.dynamicRules().length, 1);
    assert.deepEqual(w.dynamicRules()[0].condition.requestDomains, ["c.example"]);
  });
});

describe("service worker: schema version", () => {
  test("a newer schema is never written to: the state is read-only and the message says why", async () => {
    const w = await bootWorker({
      skipInstall: true,
      storage: { "shield.version": 2, "shield.userRules": "block host:keep.example", "shield.futureThing": "x" }
    });
    const before = JSON.stringify(w.store);
    const state = await w.send({ type: "saveRules", text: "block host:other.example" });
    assert.equal(state.readOnly, true);
    assert.match(state.error, /newer/);
    await w.send({ type: "setEnabled", value: false });
    await w.send({ type: "setSiteOff", host: "a.example", off: true });
    await w.fire("installed", { reason: "update" });
    assert.equal(JSON.stringify(w.store), before);
    assert.deepEqual(w.dynamicRules(), []);
    assert.deepEqual(w.calls.updateDynamicRules, []);
  });

  test("a first run (no version key) writes the defaults, and an existing install keeps its data", async () => {
    const w = await bootWorker({
      skipInstall: true,
      storage: { "shield.version": 1, "shield.userRules": "block host:keep.example", "shield.enabled": false }
    });
    await w.fire("installed", { reason: "update" });
    assert.equal(w.store["shield.userRules"], "block host:keep.example");
    assert.equal(w.store["shield.enabled"], false);
    assert.equal(w.store["shield.starterEnabled"], true, "a missing key is filled with its default");
  });
});

// ---------------------------------------------------------------------------
// The content script, run against a fake DOM, a fake MutationObserver and a fake chrome.
// ---------------------------------------------------------------------------

class DomNode extends FakeNode {
  constructor(tag) {
    super(tag);
    this.isRoot = false;
    this.style = {
      props: {},
      setProperty(name, value, priority) {
        this.props[name] = { value, priority: priority || "" };
      }
    };
    if (this.tagName === "STYLE") {
      this.sheet = {
        cssRules: [],
        insertRule(rule, index) {
          if (/INVALID/.test(rule)) throw new SyntaxError("Failed to parse the rule");
          this.cssRules.splice(index, 0, { cssText: rule });
          return index;
        },
        deleteRule(index) {
          this.cssRules.splice(index, 1);
        }
      };
    }
  }
  get isConnected() {
    let n = this;
    while (n.parentNode) n = n.parentNode;
    return n.isRoot === true;
  }
  removeChild(c) {
    super.removeChild(c);
    c.parentNode = null;
  }
  remove() {
    if (this.parentNode) this.parentNode.removeChild(this);
  }
  hasAttribute(k) {
    return this.getAttribute(k) !== null;
  }
  querySelectorAll(sel) {
    const tags = sel.split(",").map((s) => s.trim().toUpperCase());
    return this.all((n) => tags.includes(n.tagName));
  }
  attachShadow(init) {
    this.shadow = new DomNode("#shadow");
    this.shadow.mode = init.mode;
    this.shadow.host = this;
    return this.shadow;
  }
  get hostElement() {
    return this;
  }
}

class FakeObserver {
  constructor(cb) {
    this.cb = cb;
    this.options = null;
    this.target = null;
    this.disconnected = false;
    FakeObserver.all.push(this);
  }
  observe(target, options) {
    this.target = target;
    this.options = options;
  }
  disconnect() {
    this.disconnected = true;
  }
}
FakeObserver.all = [];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const CONTENT_SOURCE = fs.readFileSync(path.join(ROOT, "content", "shield-content.js"), "utf8");

// opts: { url (the frame's own), top (is this the top frame), siteHost (tab host), storage, background(msg) -> reply | throw }
async function loadContent(opts) {
  opts = opts || {};
  FakeObserver.all = [];
  const win = {};
  win.self = win;
  win.window = win;
  win.top = opts.top === false ? {} : win;
  win.requestAnimationFrame = (cb) => setTimeout(cb, 0);
  win.location = { hostname: opts.hostname || "news.example", href: opts.url || "https://news.example/" };
  const html = new DomNode("html");
  html.isRoot = true;
  if (opts.prepare) opts.prepare(html);
  const docListeners = {};
  const winListeners = {};
  win.addEventListener = (t, fn, capture) => (winListeners[t] = winListeners[t] || []).push({ fn, capture });
  const doc = {
    documentElement: html,
    readyState: opts.readyState || "loading",
    baseURI: opts.url || "https://news.example/",
    activeElement: null,
    createElement: (t) => new DomNode(t),
    addEventListener: (t, fn) => (docListeners[t] = docListeners[t] || []).push(fn),
    removeEventListener() {},
    querySelectorAll: (s) => html.querySelectorAll(s)
  };
  const store = JSON.parse(JSON.stringify(opts.storage || {}));
  const runtimeListeners = [];
  const changeListeners = [];
  const sent = [];
  const chrome = {
    runtime: {
      lastError: undefined,
      sendMessage(msg, cb) {
        sent.push(JSON.parse(JSON.stringify(msg)));
        let reply;
        try {
          reply = opts.background ? opts.background(msg) : undefined;
          chrome.runtime.lastError = undefined;
        } catch (e) {
          chrome.runtime.lastError = { message: String(e.message || e) };
          reply = undefined;
        }
        if (cb) setTimeout(() => cb(reply), 0);
      },
      onMessage: { addListener: (fn) => runtimeListeners.push(fn) }
    },
    storage: {
      local: {
        get: async (keys) => {
          const list = typeof keys === "string" ? [keys] : keys || Object.keys(store);
          const out = {};
          for (const k of list) if (k in store) out[k] = JSON.parse(JSON.stringify(store[k]));
          return out;
        }
      },
      onChanged: { addListener: (fn) => changeListeners.push(fn) }
    }
  };
  const run = (file) => {
    const src = fs.readFileSync(path.join(ROOT, file), "utf8");
    vm.runInThisContext("(function(self, window, document, chrome){" + src + "\n})", { filename: file })(win, win, doc, chrome);
  };
  const fetchLog = [];
  const fetchFn = (url) => {
    fetchLog.push(url);
    return opts.fetch ? opts.fetch(url) : Promise.reject(new Error("no fetch in this test"));
  };
  run("lib/shield-core.js");
  run("content/panel-css.js");
  run("ui/panel.js");
  chrome.runtime.getURL = (p) => "ext:///" + p;
  vm.runInThisContext(
    "(function(self, window, document, location, chrome, MutationObserver, fetch){" + CONTENT_SOURCE + "\n})",
    { filename: "shield-content.js" }
  )(win, win, doc, win.location, chrome, FakeObserver, fetchFn);
  const env = {
    win,
    doc,
    html,
    store,
    sent,
    fetchLog,
    winListeners,
    docListeners,
    sentOfType: (t) => sent.filter((m) => m.type === t),
    style: () => html.children.find((c) => c.tagName === "STYLE" && c.getAttribute("id") === "bowser-shield") || null,
    rules: () => {
      const s = env.style();
      return s ? s.sheet.cssRules.map((r) => r.cssText) : null;
    },
    key(type, ev) {
      const event = Object.assign(
        { type, code: "", key: "", altKey: false, shiftKey: false, ctrlKey: false, metaKey: false, target: html, defaultPrevented: false, composedPath: () => [html] },
        ev
      );
      event.preventDefault = () => (event.defaultPrevented = true);
      event.stopPropagation = () => (event.propagationStopped = true);
      (winListeners[type] || []).forEach((l) => l.fn(event));
      (docListeners[type] || []).forEach((fn) => fn(event));
      return event;
    },
    changed(changes) {
      for (const [k, v] of Object.entries(changes)) store[k] = v;
      const shaped = {};
      for (const [k, v] of Object.entries(changes)) shaped[k] = { newValue: v };
      changeListeners.forEach((fn) => fn(shaped, "local"));
    },
    fromBackground(msg) {
      runtimeListeners.forEach((fn) => fn(msg, { id: "ext-id" }, () => {}));
    },
    // The host carries no attribute a page could select: it is the div that owns a shadow root.
    overlay: () => html.children.find((c) => c.tagName === "DIV" && c.shadow) || null
  };
  await sleep(20);
  return env;
}

const NEWS_CTX = { siteHost: "news.example", enabled: true, mode: "network", filtered: true };
const COSMETIC = {
  global: ["div[data-ad-slot]", "div.promo"],
  byOnly: { "news.example": ["div.only-news"], "other.example": ["div.only-other"] },
  byExcept: [{ sel: "div.not-here", except: ["news.example"] }, { sel: "div.mostly", except: ["x.example"] }]
};
const ctxBackground = (ctx) => (msg) => (msg.type === "frameContext" ? ctx : { ok: true });

function mutationFor(node) {
  return { type: "childList", addedNodes: [node], target: node.parentNode };
}

describe("content script: the cosmetic layer", () => {
  test("creates an empty style#bowser-shield on the document element at once", async () => {
    const env = await loadContent({ background: () => new Promise(() => {}) });
    assert.ok(env.style());
    assert.equal(env.style().parentNode, env.html);
  });

  test("fills it with one display:none rule per applicable selector, never a grouped selector", async () => {
    const env = await loadContent({ background: ctxBackground(NEWS_CTX), storage: { "shield.cosmetic": COSMETIC } });
    assert.deepEqual(env.rules().sort(), [
      "div.mostly{display:none!important}",
      "div.only-news{display:none!important}",
      "div.promo{display:none!important}",
      "div[data-ad-slot]{display:none!important}"
    ]);
    assert.ok(env.rules().every((r) => !r.includes(",")));
  });

  test("uses the top-level site from the service worker, so a subframe applies the site's only and except", async () => {
    const env = await loadContent({
      top: false,
      hostname: "ads.partner.test",
      url: "https://ads.partner.test/frame",
      background: ctxBackground(NEWS_CTX),
      storage: { "shield.cosmetic": COSMETIC }
    });
    assert.ok(env.rules().includes("div.only-news{display:none!important}"));
    assert.ok(!env.rules().includes("div.not-here{display:none!important}"));
  });

  test("a selector the engine refuses is skipped and counted, and the rest still apply", async () => {
    const env = await loadContent({
      background: ctxBackground(NEWS_CTX),
      storage: { "shield.cosmetic": { global: ["div.good", "div.INVALID[", "span.also-good"] } }
    });
    assert.deepEqual(env.rules(), ["div.good{display:none!important}", "span.also-good{display:none!important}"]);
    assert.deepEqual(env.sentOfType("badSelectors"), [{ type: "badSelectors", count: 1 }]);
  });

  test("a clean sheet reports nothing about bad selectors", async () => {
    const env = await loadContent({ background: ctxBackground(NEWS_CTX), storage: { "shield.cosmetic": COSMETIC } });
    assert.equal(env.sentOfType("badSelectors").length, 0);
  });

  test("removes its style element when the site is never filtered, and installs no observer", async () => {
    const env = await loadContent({
      background: ctxBackground({ siteHost: "news.example", enabled: true, mode: "dom", filtered: false }),
      storage: { "shield.cosmetic": COSMETIC }
    });
    assert.equal(env.style(), null);
    assert.equal(FakeObserver.all.length, 0);
  });

  test("does nothing when the extension is off", async () => {
    const env = await loadContent({
      background: ctxBackground({ siteHost: "news.example", enabled: false, mode: "network", filtered: false }),
      storage: { "shield.cosmetic": COSMETIC }
    });
    assert.equal(env.style(), null);
  });

  test("in network mode the content script installs no observer and reports nothing", async () => {
    const env = await loadContent({ background: ctxBackground(NEWS_CTX), storage: { "shield.cosmetic": COSMETIC } });
    assert.equal(FakeObserver.all.length, 0);
    assert.equal(env.sentOfType("hidden").length, 0);
  });

  test("rebuilds the sheet in place when the stored cosmetic data changes", async () => {
    const env = await loadContent({ background: ctxBackground(NEWS_CTX), storage: { "shield.cosmetic": COSMETIC } });
    env.changed({ "shield.cosmetic": { global: ["div.fresh"] } });
    await sleep(90);
    assert.deepEqual(env.rules(), ["div.fresh{display:none!important}"]);
    assert.equal(env.html.children.filter((c) => c.tagName === "STYLE").length, 1, "one style element, edited in place");
  });

  test("a later change that turns the shield off removes the style, and turning it on puts it back", async () => {
    let ctx = NEWS_CTX;
    const env = await loadContent({ background: (m) => (m.type === "frameContext" ? ctx : { ok: true }), storage: { "shield.cosmetic": COSMETIC } });
    ctx = { siteHost: "news.example", enabled: false, mode: "network", filtered: false };
    env.changed({ "shield.enabled": false });
    await sleep(90);
    assert.equal(env.style(), null);
    ctx = NEWS_CTX;
    env.changed({ "shield.enabled": true });
    await sleep(90);
    assert.ok(env.style());
    assert.equal(env.rules().length, 4);
  });

  test("with no service worker it reads storage directly and uses the page's own host", async () => {
    const env = await loadContent({
      background: () => {
        throw new Error("Could not establish connection. Receiving end does not exist.");
      },
      storage: { "shield.cosmetic": COSMETIC, "shield.engineMode": "network" }
    });
    assert.ok(env.rules().includes("div.only-news{display:none!important}"));
    assert.equal(FakeObserver.all.length, 0);
  });

  test("with no service worker, a subframe applies only the global selectors, because it cannot verify the site", async () => {
    const env = await loadContent({
      top: false,
      hostname: "ads.partner.test",
      background: () => {
        throw new Error("no worker");
      },
      storage: { "shield.cosmetic": COSMETIC, "shield.engineMode": "network" }
    });
    assert.deepEqual(env.rules().sort(), ["div.promo{display:none!important}", "div[data-ad-slot]{display:none!important}"]);
  });

  test("with no service worker and the extension switched off in storage, nothing is applied", async () => {
    const env = await loadContent({
      background: () => {
        throw new Error("no worker");
      },
      storage: { "shield.cosmetic": COSMETIC, "shield.enabled": false }
    });
    assert.equal(env.style(), null);
  });

  test("with no service worker, a site exception stored in the list still applies in the top frame", async () => {
    const env = await loadContent({
      background: () => {
        throw new Error("no worker");
      },
      storage: { "shield.cosmetic": COSMETIC, "shield.siteOff": ["news.example"], "shield.engineMode": "network" }
    });
    assert.equal(env.style(), null);
  });
});

describe("content script: the dom-mode scrubber", () => {
  const domCtx = { siteHost: "news.example", enabled: true, mode: "dom", filtered: true, starterSrl: "block label:adserver\n" };

  function element(tag, attrs, parent) {
    const el = new DomNode(tag);
    for (const [k, v] of Object.entries(attrs || {})) el.setAttribute(k, v);
    if (parent) parent.appendChild(el);
    return el;
  }

  async function domEnv(extra) {
    return loadContent(Object.assign({ background: ctxBackground(domCtx), storage: { "shield.userRules": "block host:ads.partner.test" } }, extra));
  }

  test("installs a MutationObserver on the document element with the attribute filter from section 6.4", async () => {
    await domEnv();
    assert.equal(FakeObserver.all.length, 1);
    const o = FakeObserver.all[0];
    assert.deepEqual(o.options, { childList: true, subtree: true, attributes: true, attributeFilter: ["src", "href", "data-src"] });
    assert.ok(o.target.isRoot);
  });

  test("strips a blocked URL from an added image, hides it, and leaves other images alone", async () => {
    const env = await domEnv();
    const bad = element("img", { src: "https://x.adserver9.example/p.gif" }, env.html);
    const good = element("img", { src: "https://cdn.news.example/p.png" }, env.html);
    FakeObserver.all[0].cb([mutationFor(bad), mutationFor(good)]);
    assert.equal(bad.getAttribute("src"), null);
    assert.equal(bad.hidden, true);
    assert.equal(bad.style.display, "none");
    assert.equal(good.getAttribute("src"), "https://cdn.news.example/p.png");
    assert.equal(good.hidden, false);
  });

  test("a user rule applies as well as the starter list", async () => {
    const env = await domEnv();
    const script = element("script", { src: "https://ads.partner.test/a.js" }, env.html);
    FakeObserver.all[0].cb([mutationFor(script)]);
    assert.equal(script.getAttribute("src"), null);
  });

  test("a first-party request is not touched, by the third-party default", async () => {
    const env = await domEnv();
    const img = element("img", { src: "https://img.adserver9.news.example/p.gif" }, env.html);
    FakeObserver.all[0].cb([mutationFor(img)]);
    assert.equal(img.getAttribute("src"), "https://img.adserver9.news.example/p.gif");
  });

  test("resolves a relative URL against the document base", async () => {
    const env = await domEnv({ url: "https://news.example/a/" });
    const img = element("img", { src: "//ads.partner.test/p.gif" }, env.html);
    FakeObserver.all[0].cb([mutationFor(img)]);
    assert.equal(img.getAttribute("src"), null);
  });

  test("finds a blocked element nested in an added subtree", async () => {
    const env = await domEnv();
    const wrapper = element("div", {}, env.html);
    const img = element("img", { src: "https://ads.partner.test/n.gif" }, wrapper);
    FakeObserver.all[0].cb([mutationFor(wrapper)]);
    assert.equal(img.getAttribute("src"), null);
  });

  test("an attribute change to a blocked URL is stripped, and a data-src is covered too", async () => {
    const env = await domEnv();
    const img = element("img", { src: "https://cdn.news.example/ok.png" }, env.html);
    img.setAttribute("src", "https://ads.partner.test/swap.gif");
    FakeObserver.all[0].cb([{ type: "attributes", attributeName: "src", target: img }]);
    assert.equal(img.getAttribute("src"), null);
    const lazy = element("img", { "data-src": "https://ads.partner.test/lazy.gif" }, env.html);
    FakeObserver.all[0].cb([{ type: "attributes", attributeName: "data-src", target: lazy }]);
    assert.equal(lazy.getAttribute("data-src"), null);
    assert.equal(lazy.hidden, true);
  });

  test("infers the request kind from the tag: an image-only rule skips a script", async () => {
    const env = await domEnv({ storage: { "shield.userRules": "block host:ads.partner.test kind image" } });
    const script = element("script", { src: "https://ads.partner.test/a.js" }, env.html);
    const img = element("img", { src: "https://ads.partner.test/a.gif" }, env.html);
    FakeObserver.all[0].cb([mutationFor(script), mutationFor(img)]);
    assert.equal(script.getAttribute("src"), "https://ads.partner.test/a.js");
    assert.equal(img.getAttribute("src"), null);
  });

  test("iframes count as frames and video sources as media", async () => {
    const env = await domEnv({ storage: { "shield.userRules": "block host:ads.partner.test kind frame\nblock host:media.partner.test kind media" } });
    const frame = element("iframe", { src: "https://ads.partner.test/f.html" }, env.html);
    const video = element("video", {}, env.html);
    const source = element("source", { src: "https://media.partner.test/v.mp4" }, video);
    FakeObserver.all[0].cb([mutationFor(frame), mutationFor(video)]);
    assert.equal(frame.getAttribute("src"), null);
    assert.equal(source.getAttribute("src"), null);
  });

  test("a link counts only for preload, prefetch, preconnect and stylesheet", async () => {
    const env = await domEnv();
    const preload = element("link", { rel: "preload", as: "script", href: "https://ads.partner.test/p.js" }, env.html);
    const icon = element("link", { rel: "icon", href: "https://ads.partner.test/i.png" }, env.html);
    FakeObserver.all[0].cb([mutationFor(preload), mutationFor(icon)]);
    assert.equal(preload.getAttribute("href"), null);
    assert.equal(icon.getAttribute("href"), "https://ads.partner.test/i.png");
  });

  test("non-http URLs and tags that load nothing are ignored", async () => {
    const env = await domEnv();
    const data = element("img", { src: "data:image/gif;base64,R0lGOD" }, env.html);
    const div = element("div", { src: "https://ads.partner.test/x" }, env.html);
    FakeObserver.all[0].cb([mutationFor(data), mutationFor(div)]);
    assert.equal(data.getAttribute("src"), "data:image/gif;base64,R0lGOD");
    assert.equal(div.getAttribute("src"), "https://ads.partner.test/x");
  });

  test("reports hidden URLs once each, in one batch", async () => {
    const env = await domEnv();
    const a = element("img", { src: "https://ads.partner.test/1.gif" }, env.html);
    const b = element("img", { src: "https://ads.partner.test/1.gif" }, env.html);
    const c = element("img", { src: "https://ads.partner.test/2.gif" }, env.html);
    FakeObserver.all[0].cb([mutationFor(a), mutationFor(b), mutationFor(c)]);
    assert.equal(b.hidden, true, "the repeated URL is still hidden on the second element");
    await sleep(40);
    assert.deepEqual(env.sentOfType("hidden"), [{ type: "hidden", urls: ["https://ads.partner.test/1.gif", "https://ads.partner.test/2.gif"] }]);
  });

  test("a URL already reported is not reported again", async () => {
    const env = await domEnv();
    const a = element("img", { src: "https://ads.partner.test/1.gif" }, env.html);
    FakeObserver.all[0].cb([mutationFor(a)]);
    await sleep(40);
    const again = element("img", { src: "https://ads.partner.test/1.gif" }, env.html);
    FakeObserver.all[0].cb([mutationFor(again)]);
    await sleep(300);
    assert.equal(env.sentOfType("hidden").length, 1);
  });

  test("sweeps the existing tree once on DOMContentLoaded, for anything inserted before the observer caught up", async () => {
    let early;
    const env = await domEnv({
      prepare: (html) => {
        early = element("img", { src: "https://ads.partner.test/early.gif" }, html);
      }
    });
    assert.equal(early.getAttribute("src"), "https://ads.partner.test/early.gif", "the observer never reports what was already there");
    env.key("DOMContentLoaded", {});
    assert.equal(early.getAttribute("src"), null);
    assert.equal(early.hidden, true);
  });

  test("when the document has already loaded, the sweep runs straight away", async () => {
    let early;
    await domEnv({
      readyState: "complete",
      prepare: (html) => {
        early = element("img", { src: "https://ads.partner.test/early.gif" }, html);
      }
    });
    assert.equal(early.getAttribute("src"), null);
    assert.equal(early.hidden, true);
  });

  test("with no service worker and a stored dom mode, it scrubs with the user's rules", async () => {
    const env = await loadContent({
      background: () => {
        throw new Error("no worker");
      },
      storage: { "shield.engineMode": "dom", "shield.userRules": "block host:ads.partner.test" }
    });
    assert.equal(FakeObserver.all.length, 1);
    const img = element("img", { src: "https://ads.partner.test/1.gif" }, env.html);
    FakeObserver.all[0].cb([mutationFor(img)]);
    assert.equal(img.getAttribute("src"), null);
  });

  describe("with no frame context from the service worker", () => {
    const noWorker = () => {
      throw new Error("no worker");
    };
    const starterReply = (text) => async () => ({ ok: true, text: async () => text });
    const domStorage = { "shield.engineMode": "dom", "shield.userRules": "block host:ads.partner.test" };

    test("a top frame scrubs with the packaged starter list, fetched from the extension", async () => {
      const env = await loadContent({ background: noWorker, storage: domStorage, fetch: starterReply("block label:adserver\n") });
      assert.deepEqual(env.fetchLog, ["ext:///rules/starter.srl"]);
      const starter = element("img", { src: "https://x.adserver9.example/p.gif" }, env.html);
      const user = element("img", { src: "https://ads.partner.test/p.gif" }, env.html);
      FakeObserver.all[0].cb([mutationFor(starter), mutationFor(user)]);
      assert.equal(starter.getAttribute("src"), null, "the starter rule applies");
      assert.equal(user.getAttribute("src"), null, "the user rule still applies");
    });

    test("respects the starter checkbox", async () => {
      const env = await loadContent({
        background: noWorker,
        storage: Object.assign({ "shield.starterEnabled": false }, domStorage),
        fetch: starterReply("block label:adserver\n")
      });
      const starter = element("img", { src: "https://x.adserver9.example/p.gif" }, env.html);
      FakeObserver.all[0].cb([mutationFor(starter)]);
      assert.equal(starter.getAttribute("src"), "https://x.adserver9.example/p.gif");
    });

    test("fetches the list once per content script, however many times it refreshes", async () => {
      const env = await loadContent({ background: noWorker, storage: domStorage, fetch: starterReply("block label:adserver\n") });
      env.changed({ "shield.userRules": "block host:ads.partner.test" });
      await sleep(90);
      env.changed({ "shield.userRules": "block host:other.partner.test" });
      await sleep(90);
      assert.equal(env.fetchLog.length, 1);
      assert.equal(FakeObserver.all.length, 3);
    });

    test("a failed fetch scrubs with the user's rules only and does not throw", async () => {
      const env = await loadContent({
        background: noWorker,
        storage: domStorage,
        fetch: () => Promise.reject(new Error("offline"))
      });
      assert.equal(FakeObserver.all.length, 1);
      const starter = element("img", { src: "https://x.adserver9.example/p.gif" }, env.html);
      const user = element("img", { src: "https://ads.partner.test/p.gif" }, env.html);
      FakeObserver.all[0].cb([mutationFor(starter), mutationFor(user)]);
      assert.equal(starter.getAttribute("src"), "https://x.adserver9.example/p.gif");
      assert.equal(user.getAttribute("src"), null);
    });

    test("a fetch that answers with an error status is treated as a failed fetch", async () => {
      const env = await loadContent({ background: noWorker, storage: domStorage, fetch: async () => ({ ok: false, text: async () => "block label:adserver\n" }) });
      const starter = element("img", { src: "https://x.adserver9.example/p.gif" }, env.html);
      FakeObserver.all[0].cb([mutationFor(starter)]);
      assert.equal(starter.getAttribute("src"), "https://x.adserver9.example/p.gif");
    });

    test("a subframe fetches nothing, because it applies only the global selectors", async () => {
      const env = await loadContent({ top: false, background: noWorker, storage: domStorage, fetch: starterReply("block label:adserver\n") });
      assert.deepEqual(env.fetchLog, []);
      assert.equal(FakeObserver.all.length, 0);
    });

    test("when the service worker answers, the content script does not fetch the list itself", async () => {
      const env = await domEnv({ fetch: starterReply("block label:other\n") });
      assert.deepEqual(env.fetchLog, []);
    });
  });

  test("leaving dom mode tears the observer down", async () => {
    let ctx = domCtx;
    const env = await loadContent({ background: (m) => (m.type === "frameContext" ? ctx : { ok: true }), storage: { "shield.userRules": "" } });
    const first = FakeObserver.all[0];
    ctx = { siteHost: "news.example", enabled: true, mode: "network", filtered: true };
    env.changed({ "shield.engineMode": "network" });
    await sleep(90);
    assert.equal(first.disconnected, true);
  });
});

describe("content script: the chord and the overlay", () => {
  const chordEvent = (o) => Object.assign({ code: "KeyS", key: "S", altKey: true, shiftKey: true }, o);

  test("Alt+Shift+S in the top frame opens the panel in a closed shadow root and renders the panel into it", async () => {
    const env = await loadContent({
      background: (m) => (m.type === "getState" ? baseState() : ctxBackground(NEWS_CTX)(m)),
      storage: { "shield.cosmetic": COSMETIC }
    });
    const ev = env.key("keydown", chordEvent());
    assert.equal(ev.defaultPrevented, true);
    const host = env.overlay();
    assert.ok(host, "overlay host added to the document element");
    assert.equal(host.shadow.mode, "closed");
    await sleep(40);
    assert.ok(host.shadow.textContent.includes("Bowser Shield"));
    assert.ok(host.shadow.textContent.includes("news.example"));
  });

  test("the overlay's stylesheet comes from panel-css.js", async () => {
    const env = await loadContent({ background: (m) => (m.type === "getState" ? baseState() : ctxBackground(NEWS_CTX)(m)) });
    env.key("keydown", chordEvent());
    const shadow = env.overlay().shadow;
    const css = shadow.children.find((c) => c.tagName === "STYLE");
    const adopted = shadow.adoptedStyleSheets && shadow.adoptedStyleSheets[0];
    const text = css ? css.textContent : adopted && adopted.text;
    assert.ok(text && text.includes(".bs-panel"));
  });

  test("Escape closes it", async () => {
    const env = await loadContent({ background: (m) => (m.type === "getState" ? baseState() : ctxBackground(NEWS_CTX)(m)) });
    env.key("keydown", chordEvent());
    assert.ok(env.overlay());
    env.key("keydown", { code: "Escape", key: "Escape" });
    assert.equal(env.overlay(), null);
  });

  test("a click outside closes it and a click inside does not", async () => {
    const env = await loadContent({ background: (m) => (m.type === "getState" ? baseState() : ctxBackground(NEWS_CTX)(m)) });
    env.key("keydown", chordEvent());
    const host = env.overlay();
    env.key("pointerdown", { target: host, composedPath: () => [host, env.html] });
    assert.ok(env.overlay(), "a click that lands in the panel keeps it open");
    env.key("pointerdown", { target: env.html, composedPath: () => [env.html] });
    assert.equal(env.overlay(), null);
  });

  test("the chord again toggles it closed", async () => {
    const env = await loadContent({ background: (m) => (m.type === "getState" ? baseState() : ctxBackground(NEWS_CTX)(m)) });
    env.key("keydown", chordEvent());
    env.key("keydown", chordEvent());
    assert.equal(env.overlay(), null);
  });

  test("is ignored while focus is in an editable element", async () => {
    const env = await loadContent({ background: (m) => (m.type === "getState" ? baseState() : ctxBackground(NEWS_CTX)(m)) });
    env.doc.activeElement = new DomNode("textarea");
    const ev = env.key("keydown", chordEvent());
    assert.equal(env.overlay(), null);
    assert.equal(ev.defaultPrevented, false, "the page still gets the keystroke");
  });

  test("other keys and other modifiers do nothing", async () => {
    const env = await loadContent({ background: (m) => (m.type === "getState" ? baseState() : ctxBackground(NEWS_CTX)(m)) });
    env.key("keydown", chordEvent({ code: "KeyD" }));
    env.key("keydown", chordEvent({ ctrlKey: true }));
    env.key("keydown", chordEvent({ shiftKey: false }));
    assert.equal(env.overlay(), null);
  });

  test("works on a site that is never filtered, so the shield can be turned back on", async () => {
    const env = await loadContent({
      background: (m) => (m.type === "getState" ? baseState() : ctxBackground({ siteHost: "bank.example", enabled: true, mode: "network", filtered: false })(m))
    });
    env.key("keydown", chordEvent());
    assert.ok(env.overlay());
  });

  test("works while the shield is off", async () => {
    const env = await loadContent({
      background: (m) => (m.type === "getState" ? baseState({ enabled: false }) : ctxBackground({ siteHost: "news.example", enabled: false, mode: "network", filtered: false })(m))
    });
    env.key("keydown", chordEvent());
    assert.ok(env.overlay());
  });

  test("in a subframe the chord asks the background to open the top frame's panel, and opens nothing itself", async () => {
    const env = await loadContent({ top: false, hostname: "ads.partner.test", background: ctxBackground(NEWS_CTX) });
    env.key("keydown", chordEvent());
    assert.equal(env.overlay(), null);
    assert.deepEqual(env.sentOfType("chord"), [{ type: "chord" }]);
  });

  test("togglePanel from the background opens the panel in the top frame only", async () => {
    const top = await loadContent({ background: (m) => (m.type === "getState" ? baseState() : ctxBackground(NEWS_CTX)(m)) });
    top.fromBackground({ type: "togglePanel" });
    assert.ok(top.overlay());
    const sub = await loadContent({ top: false, background: ctxBackground(NEWS_CTX) });
    sub.fromBackground({ type: "togglePanel" });
    assert.equal(sub.overlay(), null);
  });

  test("a stored custom chord replaces the default", async () => {
    const env = await loadContent({
      background: (m) => (m.type === "getState" ? baseState() : ctxBackground(NEWS_CTX)(m)),
      storage: { "shield.panelChord": "Alt+Shift+K" }
    });
    env.key("keydown", chordEvent());
    assert.equal(env.overlay(), null);
    env.key("keydown", chordEvent({ code: "KeyK" }));
    assert.ok(env.overlay());
  });

  test("an unusable stored chord falls back to the default", async () => {
    const env = await loadContent({
      background: (m) => (m.type === "getState" ? baseState() : ctxBackground(NEWS_CTX)(m)),
      storage: { "shield.panelChord": "nonsense" }
    });
    env.key("keydown", chordEvent());
    assert.ok(env.overlay());
  });

  test("the chord and the outside click are listened for on window in the capture phase, so a page listener on window cannot swallow them", async () => {
    const env = await loadContent({ background: ctxBackground(NEWS_CTX) });
    for (const type of ["keydown", "pointerdown"]) {
      assert.equal((env.winListeners[type] || []).length, 1, type + " has one window listener");
      assert.equal(env.winListeners[type][0].capture, true, type + " is captured");
      assert.equal(env.docListeners[type], undefined, type + " is not on document");
    }
  });

  test("the overlay host leaves no stable attribute in the page, and its layout is set with important priority", async () => {
    const env = await loadContent({ background: (m) => (m.type === "getState" ? baseState() : ctxBackground(NEWS_CTX)(m)) });
    env.key("keydown", chordEvent());
    const host = env.overlay();
    assert.ok(host);
    assert.deepEqual(Object.keys(host.attrs), [], "no attribute on the host");
    const layout = { all: "initial", position: "fixed", top: "12px", right: "12px", "z-index": "2147483647", display: "block" };
    for (const [name, value] of Object.entries(layout)) {
      assert.deepEqual(host.style.props[name], { value, priority: "important" }, name);
    }
    assert.ok(!CONTENT_SOURCE.includes("data-bowser-shield-panel"));
  });
});

// ---------------------------------------------------------------------------
// The shipped package: manifest, pages, stylesheet copies, and Bowser's installer caps (section 0)
// ---------------------------------------------------------------------------

function walk(dir, out = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    out.push({ full, rel: path.relative(ROOT, full).split(path.sep).join("/"), entry });
    if (entry.isDirectory()) walk(full, out);
  }
  return out;
}

describe("package: manifest", () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(ROOT, "manifest.json"), "utf8"));

  test("is section 2 of the design, with one deliberate difference: ui/panel.js in the content script list", () => {
    const design = fs.readFileSync(path.join(ROOT, "DESIGN.md"), "utf8").replace(/\r\n/g, "\n");
    const anchor = design.indexOf("## 2. manifest.json");
    const open = design.indexOf("```json\n", anchor) + 8;
    const close = design.indexOf("\n```", open);
    const spec = JSON.parse(design.slice(open, close));
    spec.content_scripts[0].js = ["lib/shield-core.js", "content/panel-css.js", "ui/panel.js", "content/shield-content.js"];
    assert.deepEqual(manifest, spec);
  });

  test("declares exactly the three permission strings and the two host patterns", () => {
    assert.deepEqual(manifest.permissions, ["declarativeNetRequest", "declarativeNetRequestFeedback", "storage"]);
    assert.deepEqual(manifest.host_permissions, ["http://*/*", "https://*/*"]);
    assert.equal(manifest.web_accessible_resources, undefined);
    assert.equal(manifest.content_security_policy, undefined);
  });

  test("content scripts list the library first and run at document_start in every frame", () => {
    const cs = manifest.content_scripts[0];
    assert.equal(cs.js[0], "lib/shield-core.js");
    assert.equal(cs.run_at, "document_start");
    assert.equal(cs.all_frames, true);
    assert.ok(cs.js.indexOf("ui/panel.js") < cs.js.indexOf("content/shield-content.js"));
    assert.ok(cs.js.indexOf("content/panel-css.js") < cs.js.indexOf("content/shield-content.js"));
  });

  test("the service worker is a classic script, not a module", () => {
    assert.equal(manifest.background.type, undefined);
  });

  test("every file the manifest names exists, apart from the icons", () => {
    const named = [
      manifest.background.service_worker,
      manifest.action.default_popup,
      manifest.options_ui.page,
      ...manifest.content_scripts[0].js,
      ...manifest.declarative_net_request.rule_resources.map((r) => r.path)
    ];
    for (const rel of named) assert.ok(fs.existsSync(path.join(ROOT, rel)), rel + " is missing");
  });

  test("the four icon files are present", { skip: ["icons/icon-16.png", "icons/icon-32.png", "icons/icon-48.png", "icons/icon-128.png"].some((f) => !fs.existsSync(path.join(ROOT, f))) && "the PNG icons could not be written with a text editor; see VERIFICATION.md" }, () => {
    for (const size of ["16", "32", "48", "128"]) {
      const file = path.join(ROOT, manifest.icons[size]);
      const head = fs.readFileSync(file).subarray(0, 8);
      assert.deepEqual(Array.from(head), [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], size + " is not a PNG");
    }
  });

  test("the package respects Bowser's installer caps from section 0", () => {
    const all = walk(ROOT);
    assert.ok(all.filter((f) => f.entry.isFile()).length <= 2000);
    let total = 0;
    for (const f of all) {
      assert.equal(f.entry.isSymbolicLink(), false, f.rel + " is a link");
      assert.ok(f.rel.split("/").length <= 12, f.rel + " is too deep");
      const name = path.basename(f.full);
      assert.ok(!/[<>:"/\\|?*\u0000-\u001f]/.test(name), name + " has a forbidden character");
      assert.ok(!/[. ]$/.test(name), name + " ends in a dot or a space");
      if (f.entry.isFile()) total += fs.statSync(f.full).size;
    }
    assert.ok(total < 128 * 1024 * 1024);
    assert.ok(fs.statSync(path.join(ROOT, "manifest.json")).size <= 1024 * 1024);
    assert.ok(manifest.name.length >= 1 && manifest.name.length <= 128);
    assert.match(manifest.version, /^\d+(\.\d+){0,3}$/);
    for (const key of ["permissions", "host_permissions"]) {
      assert.ok(manifest[key].length <= 256);
      for (const entry of manifest[key]) assert.ok(typeof entry === "string" && entry.length <= 256);
    }
  });

  test("the file tree is the one in section 1 (plus VERIFICATION.md), and nothing else ships", () => {
    const files = walk(ROOT).filter((f) => f.entry.isFile()).map((f) => f.rel).sort();
    const expected = [
      "DESIGN.md", "VERIFICATION.md", "manifest.json",
      "lib/shield-core.js", "lib/shield-dnr.js",
      "rules/starter.srl", "rules/starter-dnr.json",
      "background/service-worker.js",
      "content/panel-css.js", "content/shield-content.js",
      "ui/panel.js", "ui/popup.html", "ui/popup.js", "ui/options.html", "ui/options.js", "ui/ui.css",
      "test/run-tests.js", "test/reference-matcher.js",
      "test/cases/parser.json", "test/cases/matcher.json", "test/cases/dnr.json"
    ];
    const icons = ["icons/icon-16.png", "icons/icon-32.png", "icons/icon-48.png", "icons/icon-128.png"].filter((f) => files.includes(f));
    assert.deepEqual(files, expected.concat(icons).sort());
  });
});

describe("package: pages and styles", () => {
  for (const page of ["popup", "options"]) {
    const html = fs.readFileSync(path.join(ROOT, "ui", page + ".html"), "utf8");

    test(page + ".html has a charset, a language, a title and a stylesheet", () => {
      assert.match(html, /<meta charset="utf-8">/i);
      assert.match(html, /<html lang="en">/);
      assert.match(html, /<title>[^<]+<\/title>/);
      assert.match(html, /<link rel="stylesheet" href="ui\.css">/);
      assert.match(html, new RegExp('<body data-page="' + page + '">'));
    });

    test(page + ".html loads the core, then the panel, then its own script, with no inline script or handler", () => {
      const srcs = [...html.matchAll(/<script src="([^"]+)"><\/script>/g)].map((m) => m[1]);
      assert.deepEqual(srcs, ["../lib/shield-core.js", "panel.js", page + ".js"]);
      assert.equal((html.match(/<script/g) || []).length, 3, "every script tag has a src");
      assert.ok(!/\son[a-z]+\s*=/i.test(html), "no inline event handlers");
      for (const s of srcs) assert.ok(fs.existsSync(path.join(ROOT, "ui", s)), s);
    });

    test(page + ".js mounts the right view and never builds markup from strings", () => {
      const js = fs.readFileSync(path.join(ROOT, "ui", page + ".js"), "utf8");
      assert.match(js, new RegExp('view: "' + (page === "popup" ? "summary" : "options") + '"'));
      assert.ok(!/innerHTML|outerHTML|insertAdjacentHTML|document\.write/.test(js));
    });
  }

  test("no shipped script uses innerHTML or eval, which the default policy forbids anyway", () => {
    for (const f of ["lib/shield-core.js", "lib/shield-dnr.js", "background/service-worker.js", "content/shield-content.js", "ui/panel.js"]) {
      const src = fs.readFileSync(path.join(ROOT, f), "utf8");
      assert.ok(!/innerHTML|outerHTML|insertAdjacentHTML|document\.write|\beval\(|new Function\(/.test(src), f);
    }
  });

  test("the overlay's stylesheet string is the same block as ui.css between the panel markers", () => {
    const css = fs.readFileSync(path.join(ROOT, "ui", "ui.css"), "utf8").replace(/\r\n/g, "\n");
    const begin = css.indexOf("/* panel:begin */") + "/* panel:begin */".length;
    const end = css.indexOf("/* panel:end */");
    assert.ok(begin > 20 && end > begin);
    const sandbox = {};
    vm.runInThisContext("(function(self){" + fs.readFileSync(path.join(ROOT, "content", "panel-css.js"), "utf8") + "\n})")(sandbox);
    assert.equal(sandbox.SHIELD_PANEL_CSS.replace(/\r\n/g, "\n").trim(), css.slice(begin, end).trim());
  });

  test("ui.css opens with the compact token set: four to six named hex colours and one type stack", () => {
    const css = fs.readFileSync(path.join(ROOT, "ui", "ui.css"), "utf8");
    const header = css.slice(0, css.indexOf("*/"));
    const named = [...header.matchAll(/^\s{4}(\w+)\s+(#[0-9a-f]{6})\b/gim)];
    assert.ok(named.length >= 4 && named.length <= 6, "named colours: " + named.length);
    assert.match(header, /type stack/i);
    assert.match(header, /memorable/i);
  });

  test("every colour used in the panel block is one of the named tokens", () => {
    const css = fs.readFileSync(path.join(ROOT, "ui", "ui.css"), "utf8");
    const header = css.slice(0, css.indexOf("*/"));
    const tokens = new Set([...header.matchAll(/#[0-9a-f]{6}\b/gi)].map((m) => m[0].toLowerCase()));
    const block = css.slice(css.indexOf("/* panel:begin */"), css.indexOf("/* panel:end */"));
    for (const hex of block.match(/#[0-9a-f]{3,8}\b/gi) || []) assert.ok(tokens.has(hex.toLowerCase()), hex + " is not a named token");
  });

  test("keyboard focus is visible and reduced motion is respected", () => {
    const css = fs.readFileSync(path.join(ROOT, "ui", "ui.css"), "utf8");
    assert.match(css, /:focus-visible\s*\{[^}]*outline:\s*2px solid/);
    assert.match(css, /@media \(prefers-reduced-motion: no-preference\)/);
    assert.ok(!/outline:\s*(none|0)\b/.test(css), "no rule removes the outline");
  });

  test("the panel fits at 320px: the overlay is at most 320px and nothing sets a wider fixed width", () => {
    const css = fs.readFileSync(path.join(ROOT, "ui", "ui.css"), "utf8");
    assert.match(css, /\.bs-overlay\s*\{[^}]*width:\s*min\(320px/);
    assert.match(css, /body\[data-page="popup"\]\s*\{\s*width:\s*320px/);
  });

  test("avoids the tells the design brief names: no all-caps labels, no cream and terracotta, no acid green", () => {
    const css = fs.readFileSync(path.join(ROOT, "ui", "ui.css"), "utf8");
    assert.ok(!/text-transform:\s*uppercase/.test(css));
    for (const banned of ["#f4f1ea", "#d97757", "#0b0b0b", "#111111"]) assert.ok(!css.toLowerCase().includes(banned), banned);
  });
});

describe("shared tables stay in step", () => {
  test("shield-dnr's type order is shield-core's type order", () => {
    assert.deepEqual(ShieldDnr.TYPE_ORDER, ShieldCore.TYPE_NAMES);
  });
});

// ---------------------------------------------------------------------------
// 9.6 Compile and decide budgets
// ---------------------------------------------------------------------------

// A format-legal 2000-rule user list that exercises every kind and qualifier.
function bigUserList() {
  const lines = [];
  for (let i = 0; i < 800; i++) lines.push("block host:h" + i + ".ads" + (i % 53) + ".example" + (i % 7 === 0 ? " party any" : ""));
  for (let i = 0; i < 500; i++) lines.push("block label:trackerx" + i + (i % 5 === 0 ? " kind script,image" : ""));
  for (let i = 0; i < 300; i++) lines.push("block path:/p" + i + "/seg" + (i % 4 === 0 ? " except site" + (i % 10) + ".example" : ""));
  for (let i = 0; i < 200; i++) lines.push("allow host:ok" + i + ".example" + (i % 3 === 0 ? " only site" + (i % 10) + ".example" : ""));
  for (let i = 0; i < 100; i++) lines.push("hide css:div.promo" + i + (i % 2 ? " only site" + (i % 10) + ".example" : ""));
  for (let i = 0; i < 100; i++) lines.push("off site:private" + i + ".example");
  return lines.join("\n");
}

describe("budgets", () => {
  const starterSrl = fs.readFileSync(path.join(ROOT, "rules", "starter.srl"), "utf8");
  const userSrc = bigUserList();

  test("the generated list is exactly 2000 rules with no errors", () => {
    const c = ShieldCore.compile(userSrc);
    assert.deepEqual(c.errors, []);
    assert.equal(c.rules.length, 2000);
  });

  test("the starter list plus 2000 user rules compile in under 50 ms", () => {
    const sources = [{ tag: "starter", src: starterSrl }, { tag: "user", src: userSrc }];
    // Best of three, so one scheduler hiccup on a busy machine does not fail an O(N) check.
    const runs = [];
    let c;
    for (let r = 0; r < 3; r++) {
      const t0 = process.hrtime.bigint();
      c = ShieldCore.compileAll(sources);
      runs.push(Number(process.hrtime.bigint() - t0) / 1e6);
    }
    const ms = Math.min(...runs);
    assert.equal(c.rules.length, 2048);
    assert.deepEqual(c.errors, []);
    assert.ok(ms < 50, "compile took " + runs.map((x) => x.toFixed(1)).join(", ") + " ms");
  });

  test("10000 decide calls over a fixed URL set take under 50 ms", () => {
    const c = ShieldCore.compileAll([{ tag: "starter", src: starterSrl }, { tag: "user", src: userSrc }]);
    const urls = [
      "https://news.example/story/123?ref=home",
      "https://cdn.news.example/app.js",
      "https://h17.ads17.example/pixel",
      "https://x.trackerx42.example/c.js",
      "https://img.example.net/p12/seg/a.png",
      "https://ok5.example/x",
      "https://adserver.partner.test/serve?id=5",
      "https://media.example.org/video/seg-001.ts",
      "https://static.example.com/fonts/inter.woff2",
      "https://analytics.example.net/collect/beacon?v=1",
      "https://www.example.com/index.html",
      "https://fonts.example.com/css?family=Inter",
      "https://api.example.com/v2/items?page=3&limit=50",
      "https://h400.ads8.example/banner.gif",
      "https://x.ad-delivery.test/impression/1",
      "https://tiles.maps.example/12/1204/1539.png",
      "https://example.com/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q.js",
      "https://sub.domain.tld.example.co/path?x=1",
      "https://cdn.shop.example/assets/main.css",
      "https://counter.example.net/track/click?u=1"
    ];
    // Warm up the compiled paths once, then time the real batch.
    for (const url of urls) ShieldCore.decide(c, { url, siteHost: "site3.example", type: "script" });
    // Best of three batches, for the same reason as the compile check.
    const runs = [];
    let blocked = 0;
    for (let r = 0; r < 3; r++) {
      blocked = 0;
      const t0 = process.hrtime.bigint();
      for (let n = 0; n < 10000; n++) {
        const d = ShieldCore.decide(c, { url: urls[n % urls.length], siteHost: "site3.example", type: "script" });
        if (d.verdict === "block") blocked++;
      }
      runs.push(Number(process.hrtime.bigint() - t0) / 1e6);
    }
    assert.ok(blocked > 0, "the URL set should hit some rules");
    assert.ok(Math.min(...runs) < 50, "10000 decisions took " + runs.map((x) => x.toFixed(1)).join(", ") + " ms");
  });

  test("decide cost does not grow with the number of rules", () => {
    const small = ShieldCore.compile("block host:a.example\nblock label:adserver\nblock path:/ad-slot/");
    const big = ShieldCore.compileAll([{ tag: "starter", src: starterSrl }, { tag: "user", src: userSrc }]);
    const url = "https://x.example.net/some/long/path/with/many/segments/and/a/query?alpha=1&beta=2&gamma=3";
    const time = (c) => {
      for (let n = 0; n < 2000; n++) ShieldCore.decide(c, { url, siteHost: "site3.example", type: "script" });
      const t0 = process.hrtime.bigint();
      for (let n = 0; n < 20000; n++) ShieldCore.decide(c, { url, siteHost: "site3.example", type: "script" });
      return Number(process.hrtime.bigint() - t0) / 1e6;
    };
    const tSmall = time(small);
    const tBig = time(big);
    // A scan of the rules would make the big list ~700 times slower; the automaton keeps it near 1x.
    assert.ok(tBig < tSmall * 6 + 20, "small " + tSmall.toFixed(1) + " ms, big " + tBig.toFixed(1) + " ms");
  });
});
