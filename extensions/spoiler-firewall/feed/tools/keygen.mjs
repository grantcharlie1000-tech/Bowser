// Generates an Ed25519 feed-signing keypair.
//
//   node feed/tools/keygen.mjs [--out <private-key.pem>] [--force]
//
// The PRIVATE key (PKCS#8 PEM) is written OUTSIDE the repository, by default to
//   %USERPROFILE%\.spoiler-firewall\feed-signing-key.pem   (~/.spoiler-firewall/feed-signing-key.pem elsewhere)
// with owner-only permissions where the OS supports it. An existing file is never overwritten without --force.
// The PUBLIC key is printed as base64 (the raw 32 bytes): that line is what feed/public-key.txt holds.
// The private key is never printed. Whoever holds it can make every installed copy add terms.
import { generateKeyPairSync, createPublicKey } from "node:crypto";
import { mkdirSync, writeFileSync, existsSync, chmodSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve, relative, isAbsolute } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");

export function defaultKeyPath() {
  return join(process.env.USERPROFILE || homedir(), ".spoiler-firewall", "feed-signing-key.pem");
}

export function rawPublicKeyBase64(keyObject) {
  const pub = keyObject.type === "private" ? createPublicKey(keyObject) : keyObject;
  const jwk = pub.export({ format: "jwk" });
  return Buffer.from(jwk.x, "base64url").toString("base64");
}

export function generate() {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  return { privatePem: privateKey.export({ type: "pkcs8", format: "pem" }), publicB64: rawPublicKeyBase64(publicKey) };
}

function insideRepo(path) {
  const rel = relative(ROOT, resolve(path));
  return rel === "" || (!rel.startsWith("..") && !isAbsolute(rel));
}

function main() {
  const args = process.argv.slice(2);
  const at = args.indexOf("--out");
  const out = resolve(at >= 0 ? args[at + 1] : defaultKeyPath());
  if (at >= 0 && !args[at + 1]) { console.error("--out needs a path"); process.exit(2); }
  if (insideRepo(out) && !args.includes("--allow-in-repo")) {
    console.error(`Refusing to write a private key inside the repository (${out}). Pick a path outside it.`);
    process.exit(2);
  }
  if (existsSync(out) && !args.includes("--force")) {
    console.error(`${out} already exists; refusing to overwrite it (pass --force only if you mean to replace the signing key).`);
    process.exit(2);
  }
  const { privatePem, publicB64 } = generate();
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, privatePem, { mode: 0o600 });
  try { chmodSync(out, 0o600); } catch { /* Windows: the profile folder is already per-user */ }
  console.log(`Private key written to: ${out}`);
  console.log("WARNING: back this file up somewhere safe and private. Never commit it, paste it, or share it.");
  console.log("         Losing it means a client release is needed to rotate the key; leaking it lets anyone add terms for every user.");
  console.log("\nPublic key (base64, put on its own line in feed/public-key.txt):");
  console.log(publicB64);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) main();
