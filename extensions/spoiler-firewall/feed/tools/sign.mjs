// Validates a lexicon source file, proves the built engine accepts it, and signs it into a feed envelope.
//
//   node feed/tools/sign.mjs [--lexicon feed/source/lexicon.json] [--key <private.pem>] [--out feed/published/feed.json]
//                            [--seq N] [--issued YYYY-MM-DD] [--expires YYYY-MM-DD | --expires-days 30] [--prev <feed.json>]
//
// The source file is the `lexicon` object: { "add": { "<category>": ["term", ...] }, "raise": { "term": "<category>" } }
// (see feed/schema.md). The envelope is written to --out; the default is the path Bowser and the extension fetch
// from, but nothing is published until a maintainer commits that file.
//
//   seq: must be higher than the highest seq already published. By default it is read from --prev (default: the
//        --out file, if it exists) and bumped by one; pass --seq to choose, the tool refuses anything not higher.
//   key: defaults to %USERPROFILE%\.spoiler-firewall\feed-signing-key.pem
//
// Needs the built engine (node build/build.mjs). The private key is read, never printed, never copied.
import { readFileSync, writeFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { addDays, checkLexiconWithEngine, FEED_DIR, loadClient, loadPrivateKey, makeEnvelope, MAX_ENVELOPE_BYTES, MAX_LEXICON_JSON, nodeVerifyRaw, publicKeyBase64, todayUtc } from "./lib.mjs";
import { defaultKeyPath } from "./keygen.mjs";

function arg(args, name) {
  const i = args.indexOf(name);
  if (i < 0) return undefined;
  if (i + 1 >= args.length) throw new Error(`${name} needs a value`);
  return args[i + 1];
}

function previousSeq(path, client) {
  if (!path || !existsSync(path)) return 0;
  const env = JSON.parse(readFileSync(path, "utf8"));
  const payload = JSON.parse(Buffer.from(env.payload, "base64").toString("utf8"));
  const checked = client.SFWFeed.checkPayload(payload);
  if (!checked.ok) throw new Error(`previous feed ${path} is not a valid feed (${checked.code})`);
  return checked.feed.seq;
}

// Builds and signs. Throws Error with a plain message on any problem. Returns {envelope, payloadText, seq, added, warnings}.
export function signFeed({ lexicon, privateKey, seq, issued, expires, prevSeq = 0 }) {
  const client = loadClient();
  if (!lexicon || typeof lexicon !== "object" || Array.isArray(lexicon)) throw new Error("the lexicon source must be a JSON object");
  const wanted = seq === undefined ? prevSeq + 1 : seq;
  if (!Number.isInteger(wanted) || wanted < 1 || wanted > client.SFWFeed.MAX_SEQ) throw new Error(`seq ${wanted} is not an integer in 1..${client.SFWFeed.MAX_SEQ}`);
  if (wanted <= prevSeq) throw new Error(`seq ${wanted} is not higher than the already published seq ${prevSeq}; clients would ignore it`);
  const lex = { ...lexicon };
  if (lex.version === undefined) lex.version = `f${wanted}`;
  const payload = { format: 1, seq: wanted, issued, expires, lexicon: lex };
  const schema = client.SFWFeed.checkPayload(payload);
  if (!schema.ok) throw new Error(`the payload does not satisfy the schema (${schema.code}); see feed/schema.md`);
  if (JSON.stringify(lex).length > MAX_LEXICON_JSON) throw new Error(`the lexicon is larger than ${MAX_LEXICON_JSON} bytes of JSON`);
  const proof = checkLexiconWithEngine(lex);
  if (proof.errors.length) throw new Error(proof.errors.join("; "));
  const payloadText = JSON.stringify(payload);
  const envelope = makeEnvelope(payloadText, privateKey);
  if (Buffer.byteLength(envelope) > MAX_ENVELOPE_BYTES) throw new Error(`the signed feed is ${Buffer.byteLength(envelope)} bytes, over the ${MAX_ENVELOPE_BYTES} byte limit`);
  // Round trip through the client's own verifier with this key's public half: never emit something clients would drop.
  return { envelope, payloadText, seq: wanted, added: proof.added, warnings: proof.warnings, publicKey: publicKeyBase64(privateKey), client };
}

async function main() {
  const args = process.argv.slice(2);
  const sourcePath = resolve(arg(args, "--lexicon") || join(FEED_DIR, "source", "lexicon.json"));
  const keyPath = resolve(arg(args, "--key") || defaultKeyPath());
  const out = resolve(arg(args, "--out") || join(FEED_DIR, "published", "feed.json"));
  const issued = arg(args, "--issued") || todayUtc();
  const expires = arg(args, "--expires") || addDays(issued, Number(arg(args, "--expires-days") || 30));
  const seqArg = arg(args, "--seq");
  if (!existsSync(sourcePath)) throw new Error(`lexicon source not found: ${sourcePath}`);
  if (!existsSync(keyPath)) throw new Error(`signing key not found: ${keyPath} (create one with: node feed/tools/keygen.mjs)`);
  const client = loadClient();
  const prevSeq = previousSeq(resolve(arg(args, "--prev") || out), client);
  const lexicon = JSON.parse(readFileSync(sourcePath, "utf8"));
  const privateKey = loadPrivateKey(keyPath);
  const result = signFeed({ lexicon, privateKey, seq: seqArg === undefined ? undefined : Number(seqArg), issued, expires, prevSeq });
  // The shipped verifier, with the signing key's own public half, must accept what we are about to write.
  const check = await result.client.SFWFeed.verifyEnvelope(result.envelope, [result.publicKey], { highestSeq: prevSeq });
  if (!check.ok) throw new Error(`self-check failed (${check.code}); nothing written`);
  if (!nodeVerifyRaw(Buffer.from(result.payloadText), Buffer.from(JSON.parse(result.envelope).sig, "base64"), result.publicKey)) throw new Error("node:crypto disagrees with the signature; nothing written");
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, result.envelope);
  for (const w of result.warnings) console.warn(`warning: ${w}`);
  const prod = readFileSync(join(FEED_DIR, "public-key.txt"), "utf8");
  if (!prod.includes(result.publicKey)) console.warn("warning: this signing key is NOT listed in feed/public-key.txt; shipped clients will reject the feed (fine for a test key).");
  console.log(`Signed feed #${result.seq} (${result.added} term change${result.added === 1 ? "" : "s"} accepted by the engine, issued ${issued}, expires ${expires}) -> ${out}`);
  console.log(`Envelope: ${Buffer.byteLength(result.envelope)} bytes. Verify with: node feed/tools/verify.mjs "${out}"`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  main().catch((e) => { console.error(`error: ${e.message}`); process.exit(1); });
}
