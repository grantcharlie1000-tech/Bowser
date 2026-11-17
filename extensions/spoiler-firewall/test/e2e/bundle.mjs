// Builds the init-script bundle used by the e2e harness: engine + hooks + test host, same fragment order as
// build/build.mjs. (build.mjs runs main() on import, so the concatenation is re-implemented here.)
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const HERE = dirname(fileURLToPath(import.meta.url));
const ORDER = ["normalize.js", "lexicon.js", "classify.js", "policy.js", "adapters.js", "styles.js", "shield.js", "hooks.js", "runtime.js"];
const DETECTION = new Set(["normalize.js", "lexicon.js", "classify.js", "policy.js"]);

function read(rel) {
  return readFileSync(join(ROOT, rel), "utf8").replace(/\r\n/g, "\n");
}

// detection: "real" (engine/ sources) or "stub" (test/e2e/stub-detection.js replaces the four detection fragments)
export function engineText(detection = "real") {
  const parts = [];
  for (const f of ORDER) {
    if (detection === "stub" && DETECTION.has(f)) {
      if (f === "normalize.js") parts.push(`// ---- stub detection ----\n${readFileSync(join(HERE, "stub-detection.js"), "utf8")}`);
      continue;
    }
    parts.push(`// ---- engine/${f} ----\n${read("engine/" + f)}`);
  }
  return parts.join("\n");
}

// Returns the script text. `config` is injected as globalThis.__SFW_TEST_CONFIG__ ahead of the bundle.
export function testBundle(config = {}, detection = "real") {
  const pre = `globalThis.__SFW_TEST_CONFIG__ = ${JSON.stringify(config)};\n`;
  return (
    pre +
    `(function () {\n"use strict";\nconst SFW = { version: "test" };\n${engineText(detection)}\n` +
    `// ---- hosts/test/test-host.js ----\n${read("hosts/test/test-host.js")}\n})();\n`
  );
}
