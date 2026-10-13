"use strict";
// A deliberately naive matcher, used only as a test oracle (section 9.3).
//
// It keeps the rules in an array and, for every request, loops over every rule in order doing plain
// string comparisons. No tries, no automaton, no early index. It is obviously correct and obviously
// slow, and it exists to be disagreed with.
//
// It takes the Rule records the parser produced (compiled.rules) so that the two matchers are being
// compared on matching, not on parsing; the parser has its own table of cases.

const TYPES = ["script", "image", "frame", "xhr", "media", "font", "style", "ping", "other"];

function normalise(url) {
  let u;
  try {
    u = new URL(url);
  } catch (e) {
    return null;
  }
  if (u.protocol !== "http:" && u.protocol !== "https:") return null;
  let host = u.hostname.toLowerCase();
  if (host.endsWith(".")) host = host.slice(0, -1);
  return { host, pathq: (u.pathname + u.search).toLowerCase() };
}

function suffixMatch(host, value) {
  return host === value || host.endsWith("." + value);
}

function sameParty(a, b) {
  return a === b || a.endsWith("." + b) || b.endsWith("." + a);
}

function applies(rule, n, site, typeBit, isFirst) {
  if (rule.kind === "host") {
    if (!suffixMatch(n.host, rule.value)) return false;
  } else if (rule.kind === "label") {
    if (!n.host.split(".").some((label) => label.includes(rule.value))) return false;
  } else if (rule.kind === "path") {
    if (!n.pathq.includes(rule.value.toLowerCase())) return false;
  } else {
    return false;
  }
  if (((rule.types >> typeBit) & 1) === 0) return false;
  if (rule.party === 1 && !isFirst) return false;
  if (rule.party === 2 && isFirst) return false;
  if (rule.except && rule.except.some((h) => suffixMatch(site, h))) return false;
  if (rule.only && !rule.only.some((h) => suffixMatch(site, h))) return false;
  return true;
}

function decide(rules, req) {
  const pass = { verdict: "pass", ruleIndex: -1, line: 0 };
  const n = normalise(req.url);
  if (!n) return pass;
  let site = String(req.siteHost || "").toLowerCase();
  if (site.endsWith(".")) site = site.slice(0, -1);
  if (n.host.split(".").length > 24 || site.split(".").length > 24) return pass;

  for (const rule of rules) {
    if (rule.verb === "off" && suffixMatch(site, rule.value)) {
      return { verdict: "off", ruleIndex: rule.i, line: rule.line };
    }
  }
  const typeBit = typeof req.type === "number" ? req.type : TYPES.indexOf(req.type);
  const isFirst = sameParty(n.host, site);
  for (const verb of ["allow", "block"]) {
    for (const rule of rules) {
      if (rule.verb === verb && applies(rule, n, site, typeBit, isFirst)) {
        return { verdict: verb, ruleIndex: rule.i, line: rule.line };
      }
    }
  }
  return pass;
}

module.exports = { decide, normalise };
