// Verifies a feed file the way a client does.
//
//   node feed/tools/verify.mjs <feed.json> [--key <base64 public key>|--key-file <public-key.txt>] [--min-seq N]
//
// Default trust: the production keys in feed/public-key.txt. Uses the shipped client code (hosts/chrome/feed.js
// with the vendored noble-ed25519) and cross-checks the signature with node:crypto. When the built engine is
// present it also confirms SFW.lexicon.extend accepts every entry. Exit code 0 = accepted, 1 = rejected.
import { readFileSync, existsSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { checkLexiconWithEngine, loadClient, nodeVerifyRaw, productionKeys, readPublicKeys } from "./lib.mjs";

export async function verifyFile(path, { keys, minSeq = 0 } = {}) {
  const client = loadClient();
  const body = readFileSync(path, "utf8");
  const trusted = keys || productionKeys();
  const result = await client.SFWFeed.verifyEnvelope(body, trusted, { highestSeq: minSeq });
  if (!result.ok) return { ok: false, code: result.code };
  const env = JSON.parse(body);
  const payload = Buffer.from(env.payload, "base64");
  const sig = Buffer.from(env.sig, "base64");
  const nodeAgrees = trusted.some((k) => { try { return nodeVerifyRaw(payload, sig, k); } catch { return false; } });
  if (!nodeAgrees) return { ok: false, code: "node-crypto-disagrees" };
  let engine = null;
  try { engine = checkLexiconWithEngine(result.feed.lexicon); } catch (e) { engine = { errors: [e.message], warnings: [], added: 0 }; }
  return { ok: engine.errors.length === 0, code: engine.errors.length ? "engine" : "ok", feed: result.feed, engine, bytes: Buffer.byteLength(body) };
}

async function main() {
  const args = process.argv.slice(2);
  const file = args.find((a, i) => !a.startsWith("--") && (i === 0 || !["--key", "--key-file", "--min-seq"].includes(args[i - 1])));
  if (!file) { console.error("usage: node feed/tools/verify.mjs <feed.json> [--key <b64>|--key-file <file>] [--min-seq N]"); process.exit(2); }
  if (!existsSync(file)) { console.error(`not found: ${file}`); process.exit(2); }
  const val = (n) => { const i = args.indexOf(n); return i >= 0 ? args[i + 1] : undefined; };
  const keys = val("--key") ? [val("--key")] : val("--key-file") ? readPublicKeys(readFileSync(val("--key-file"), "utf8")) : undefined;
  const r = await verifyFile(resolve(file), { keys, minSeq: Number(val("--min-seq") || 0) });
  if (!r.ok) {
    console.error(`REJECTED (${r.code})${r.engine ? ": " + r.engine.errors.join("; ") : ""}`);
    process.exit(1);
  }
  console.log(`OK: feed #${r.feed.seq}, issued ${r.feed.issued}, expires ${r.feed.expires}, ${r.bytes} bytes, engine accepts ${r.engine.added} term change(s).`);
  for (const w of r.engine.warnings) console.warn(`warning: ${w}`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  main().catch((e) => { console.error(`error: ${e.message}`); process.exit(1); });
}
