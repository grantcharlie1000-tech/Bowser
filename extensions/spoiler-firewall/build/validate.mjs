// Release validation for dist/chrome (the unpacked MV3 extension). Node 18+, no dependencies.
//
//   node build/validate.mjs [dir]      exits non-zero on any error
//
// The checks encode the Chrome Web Store posture of this extension: MV3, the "storage" and "alarms"
// install-time permissions (plus exactly one OPTIONAL permission, "offscreen", for the opt-in image-text scan), no remote
// code, no network calls outside the offscreen scanner, no eval, no test/source-map leftovers, and no
// story-revealing text in any UI string. validateDist() is exported for unit tests.
//
// Vendored third-party code (vendor/) is not blanket-exempt: every vendored script must match a pinned SHA-256
// (VENDOR_SCRIPT_PINS). Only pinned files may contain the idioms that are dead code in those libraries
// (new Function / Function("...") fallbacks, Emscripten loader branches); everything else under vendor/ and all
// of our own code is held to the full rule set below.
import { readFileSync, readdirSync, statSync, existsSync } from "node:fs";
import { createHash } from "node:crypto";
import { join, dirname, relative, extname, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

export const ALLOWED_PERMISSIONS = ["storage", "alarms"]; // alarms: the 12-hour term-update check (no install warning)
export const ALLOWED_OPTIONAL_PERMISSIONS = ["offscreen"];
export const ALLOWED_EXTENSION_PAGES_CSP = "script-src 'self' 'wasm-unsafe-eval'; object-src 'self'";
export const REQUIRED_ICON_SIZES = [16, 32, 48, 128];
export const MIN_CHROME_VERSION = 120;
export const MAX_UNPACKED_BYTES = 30 * 1024 * 1024;
export const MAX_VENDOR_BYTES = 25 * 1024 * 1024;
// Reviewed vendored scripts (see vendor/README.md). Update deliberately when upgrading Tesseract.js.
export const VENDOR_SCRIPT_PINS = {
  "vendor/tesseract/tesseract.min.js": "6795ee3bde03681e64fa4ddd747c4d9acb388bb9f5e35870fe71dd22feaa1ea1",
  "vendor/tesseract/worker.min.js": "f113187fae220d9f895d7bd28d2b94d7640261ae91031b0d6005c2773fcb9861",
  "vendor/tesseract/core/tesseract-core-relaxedsimd-lstm.wasm.js": "861a536cf9ef8e63cb644d57bab39c388f37f7d6b6f60024b741c5f6b39a59b3",
  // @noble/ed25519 3.2.0 classic wrapper (feed/tools/vendor-noble.mjs; vendor/README.md): verifies the term-update signature.
  "vendor/noble-ed25519/noble-ed25519.js": "86ff547e5bb9a9c7ee77acf853646c8a07a403c4f9b9abb5f67601fdd6e2cf90",
  "vendor/tesseract/core/tesseract-core-simd-lstm.wasm.js": "c58b46a4c796c0b8afccf77591d5b875b6896b45d402bbce8caa6f5362447b38",
};
// Where network-capable / extension-API calls are allowed in our own code (everything else may not use them).
export const FETCH_ALLOWED = { "offscreen.js": 1, "content.js": 1, "feed-update.js": 1 }; // max fetch( calls per file; the first two only read image bytes, feed-update.js downloads the signed term list
export const API_ALLOWED_FILES = {
  offscreen: ["service-worker.js"], // chrome.offscreen
  permissions: ["service-worker.js", "common.js"], // chrome.permissions
};
export const MIN_UNPACKED_BYTES = 2 * 1024;

// Words that must never appear in extension UI strings (the term "GTA VI" itself is fine).
const SPOILERISH = /\b(lucia|jason|caminos|duval|leonida|vice city|ending|endings|dies|died|killed|betray\w*|plot twist|final mission|post-credits)\b/i;
const UI_FILES = /^(popup|options|common)\.(html|js)$|^service-worker\.js$/;

const FORBIDDEN_DIRS = new Set(["node_modules", "test", "tests", "fixtures", ".git", "__tests__"]);
const FORBIDDEN_EXT = new Set([".map", ".ts", ".tsx", ".ps1", ".sh", ".cmd", ".bat", ".exe", ".dll", ".md", ".log"]);
const FORBIDDEN_NAME = /\.(test|spec)\.[cm]?js$|^\.|thumbs\.db$/i;

function walk(dir, base = dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    if (statSync(full).isDirectory()) out.push(...walk(full, base));
    else out.push(relative(base, full).split("\\").join("/"));
  }
  return out;
}

export function readPngSize(buf) {
  const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  if (buf.length < 24 || !buf.subarray(0, 8).equals(sig) || buf.toString("ascii", 12, 16) !== "IHDR") return null;
  return { width: buf.readUInt32BE(16), height: buf.readUInt32BE(20) };
}

function isRemote(url) {
  return /^(?:[a-z][a-z0-9+.-]*:)?\/\//i.test(url.trim()) || /^(?:https?|ftp|wss?):/i.test(url.trim());
}

function stringsOf(manifest) {
  const out = [];
  (function visit(v) {
    if (typeof v === "string") out.push(v);
    else if (Array.isArray(v)) v.forEach(visit);
    else if (v && typeof v === "object") Object.values(v).forEach(visit);
  })(manifest);
  return out;
}

function stripHtmlTags(html) {
  return html.replace(/<script[\s\S]*?<\/script>/gi, " ").replace(/<style[\s\S]*?<\/style>/gi, " ").replace(/<[^>]+>/g, " ");
}

export function validateDist(dir, { version, rootDir = ROOT } = {}) {
  const errors = [];
  const warnings = [];
  const err = (m) => errors.push(m);
  const warn = (m) => warnings.push(m);

  if (!existsSync(dir)) return { errors: [`dist directory not found: ${dir}`], warnings, files: [] };
  const files = walk(dir);
  const has = (p) => files.includes(p);
  let totalBytes = 0;
  for (const f of files) totalBytes += statSync(join(dir, f)).size;
  if (totalBytes > MAX_UNPACKED_BYTES) err(`unpacked size ${totalBytes} exceeds ${MAX_UNPACKED_BYTES} bytes`);
  if (totalBytes < MIN_UNPACKED_BYTES) err(`unpacked size ${totalBytes} is implausibly small`);

  let vendorBytes = 0;
  for (const f of files) if (f.startsWith("vendor/")) vendorBytes += statSync(join(dir, f)).size;
  if (vendorBytes > MAX_VENDOR_BYTES) err(`vendor/ is ${vendorBytes} bytes (max ${MAX_VENDOR_BYTES})`);

  // ---- manifest -------------------------------------------------------------------------
  let manifest = null;
  if (!has("manifest.json")) err("manifest.json missing");
  else {
    try { manifest = JSON.parse(readFileSync(join(dir, "manifest.json"), "utf8")); } catch (e) { err(`manifest.json is not valid JSON: ${e.message}`); }
  }

  if (manifest) {
    if (manifest.manifest_version !== 3) err("manifest_version must be 3");
    if (typeof manifest.name !== "string" || !manifest.name.trim()) err("name missing");
    else if (manifest.name.length > 75) err(`name is ${manifest.name.length} chars (max 75)`);
    else {
      if (!/unofficial/i.test(manifest.name)) err('name must say "Unofficial" (no implied affiliation)');
      if (/rockstar|take-?two/i.test(manifest.name)) err("name must not contain a publisher's name");
    }
    if (typeof manifest.description !== "string" || !manifest.description.trim()) err("description missing");
    else if (manifest.description.length > 132) err(`description is ${manifest.description.length} chars (max 132)`);
    if (typeof manifest.version !== "string" || !/^\d{1,5}(\.\d{1,5}){0,3}$/.test(manifest.version)) err(`invalid version "${manifest.version}"`);
    else if (manifest.version.split(".").some((n) => Number(n) > 65535)) err("version component exceeds 65535");
    if (version && manifest.version !== version) err(`manifest version ${manifest.version} does not match package.json ${version}`);

    const minV = Number(String(manifest.minimum_chrome_version || "").split(".")[0]);
    if (!minV || minV < MIN_CHROME_VERSION) err(`minimum_chrome_version must be >= ${MIN_CHROME_VERSION}`);

    // Permissions: exactly the allowlist, nothing optional, no host permissions.
    const perms = Array.isArray(manifest.permissions) ? [...manifest.permissions].sort() : [];
    if (JSON.stringify(perms) !== JSON.stringify([...ALLOWED_PERMISSIONS].sort())) err(`permissions must be exactly ${JSON.stringify(ALLOWED_PERMISSIONS)}, got ${JSON.stringify(manifest.permissions)}`);
    const optPerms = Array.isArray(manifest.optional_permissions) ? [...manifest.optional_permissions].sort() : [];
    if (JSON.stringify(optPerms) !== JSON.stringify([...ALLOWED_OPTIONAL_PERMISSIONS].sort())) err(`optional_permissions must be exactly ${JSON.stringify(ALLOWED_OPTIONAL_PERMISSIONS)}, got ${JSON.stringify(manifest.optional_permissions)}`);
    for (const key of ["host_permissions", "optional_host_permissions", "externally_connectable", "web_accessible_resources", "update_url", "devtools_page", "chrome_url_overrides", "commands", "declarative_net_request", "sandbox", "oauth2", "key"]) {
      if (key in manifest) err(`unexpected manifest key: ${key}`);
    }
    if (!("content_security_policy" in manifest)) err("content_security_policy missing: the offscreen OCR page needs 'wasm-unsafe-eval'");
    else {
      // The one permitted loosening: WebAssembly for the bundled OCR engine ('wasm-unsafe-eval'). Nothing else may change.
      const csp = manifest.content_security_policy;
      const keys = csp && typeof csp === "object" ? Object.keys(csp) : [];
      if (JSON.stringify(keys) !== '["extension_pages"]' || csp.extension_pages !== ALLOWED_EXTENSION_PAGES_CSP) err(`content_security_policy must be exactly {"extension_pages": ${JSON.stringify(ALLOWED_EXTENSION_PAGES_CSP)}}`);
    }

    // Background: classic service worker.
    const bg = manifest.background;
    if (!bg || typeof bg.service_worker !== "string") err("background.service_worker missing");
    else if (bg.type === "module") err("service worker must be a classic script (no type: module)");

    // Content scripts: one MAIN-world and one isolated script, all frames, document_start.
    const cs = Array.isArray(manifest.content_scripts) ? manifest.content_scripts : [];
    if (cs.length !== 2) err(`expected exactly 2 content_scripts entries, got ${cs.length}`);
    const main = cs.find((c) => c.world === "MAIN");
    const iso = cs.find((c) => c.world === undefined || c.world === "ISOLATED");
    if (!main) err("MAIN-world content script missing");
    else if (JSON.stringify(main.js) !== JSON.stringify(["content-main.js"])) err("MAIN content script must be content-main.js only");
    if (!iso) err("isolated content script missing");
    else if (JSON.stringify(iso.js) !== JSON.stringify(["content.js"])) err("isolated content script must be content.js only");
    for (const c of cs) {
      if (JSON.stringify(c.matches) !== JSON.stringify(["<all_urls>"])) err("content script matches must be exactly <all_urls>");
      if (c.run_at !== "document_start") err("content scripts must run at document_start");
      if (c.all_frames !== true) err("content scripts must set all_frames");
      if (c.match_about_blank !== true) err("content scripts must set match_about_blank");
      if (c.match_origin_as_fallback !== true) err("content scripts must set match_origin_as_fallback");
      if (c.css) err("content script css is not expected");
    }

    // Referenced files must exist and be local.
    const refs = new Set();
    const ref = (p, what) => {
      if (typeof p !== "string" || !p) { err(`${what} missing`); return; }
      if (isRemote(p)) { err(`${what} must be a local file, got ${p}`); return; }
      refs.add(p);
      if (!has(p)) err(`${what} references missing file ${p}`);
    };
    if (bg && bg.service_worker) ref(bg.service_worker, "background.service_worker");
    for (const c of cs) for (const js of c.js || []) ref(js, "content script");
    if (manifest.action) {
      if (manifest.action.default_popup) ref(manifest.action.default_popup, "action.default_popup");
      for (const p of Object.values(manifest.action.default_icon || {})) ref(p, "action.default_icon");
    } else err("action missing");
    if (!manifest.options_ui || !manifest.options_ui.page) err("options_ui.page missing");
    else ref(manifest.options_ui.page, "options_ui.page");

    // Icons: each size present, valid PNG, exact dimensions.
    for (const size of REQUIRED_ICON_SIZES) {
      const p = manifest.icons && manifest.icons[String(size)];
      if (!p) { err(`icons.${size} missing`); continue; }
      ref(p, `icons.${size}`);
      if (has(p)) {
        const dim = readPngSize(readFileSync(join(dir, p)));
        if (!dim) err(`${p} is not a valid PNG`);
        else if (dim.width !== size || dim.height !== size) err(`${p} is ${dim.width}x${dim.height}, expected ${size}x${size}`);
      }
    }
    for (const [size, p] of Object.entries((manifest.action && manifest.action.default_icon) || {})) {
      if (has(p)) {
        const dim = readPngSize(readFileSync(join(dir, p)));
        if (!dim || dim.width !== Number(size) || dim.height !== Number(size)) err(`action icon ${p} is not ${size}x${size}`);
      }
    }

    // UI strings in the manifest.
    for (const s of stringsOf(manifest)) {
      if (SPOILERISH.test(s)) err(`manifest string contains spoiler-ish wording: "${s.slice(0, 60)}"`);
    }

    // ---- HTML pages: no inline script, no remote subresources, local refs exist ----------
    for (const f of files.filter((x) => x.endsWith(".html"))) {
      const html = readFileSync(join(dir, f), "utf8");
      const base = posix.dirname(f);
      const local = (u, what) => {
        if (isRemote(u)) err(`${f}: ${what} must not load remote resources (${u})`);
        else if (!u.startsWith("#") && !/^[a-z][a-z0-9+.-]*:/i.test(u)) {
          const target = posix.normalize(posix.join(base, u.split("?")[0].split("#")[0]));
          if (!has(target)) err(`${f}: ${what} references missing file ${u}`);
          else refs.add(target);
        }
      };
      for (const m of html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/gi)) {
        const src = /\bsrc\s*=\s*["']([^"']+)["']/i.exec(m[1]);
        if (!src) err(`${f}: inline <script> is not allowed`);
        else local(src[1], "script src");
        if (src && m[2].trim()) err(`${f}: script with src must be empty`);
      }
      for (const m of html.matchAll(/<link\b[^>]*\bhref\s*=\s*["']([^"']+)["'][^>]*>/gi)) local(m[1], "link href");
      for (const m of html.matchAll(/<(?:img|iframe|frame|embed|object|source|video|audio)\b[^>]*\b(?:src|data)\s*=\s*["']([^"']+)["']/gi)) local(m[1], "resource");
      for (const m of html.matchAll(/<a\b[^>]*\bhref\s*=\s*["']([^"']+)["']/gi)) {
        if (isRemote(m[1])) warn(`${f}: external link ${m[1]} (review for policy)`);
        else local(m[1], "a href");
      }
      if (/\son[a-z]+\s*=/i.test(html)) err(`${f}: inline event handler attributes are not allowed`);
      if (/javascript:/i.test(html)) err(`${f}: javascript: URLs are not allowed`);
      if (/<style\b/i.test(html)) warn(`${f}: inline <style> block`);
      if (UI_FILES.test(posix.basename(f))) {
        const text = stripHtmlTags(html);
        const hit = SPOILERISH.exec(text);
        if (hit) err(`${f}: UI text contains spoiler-ish wording "${hit[0]}"`);
      }
    }

    // Files nobody references (informational): engine.js is built for extension pages.
    for (const f of files) {
      if (f.endsWith(".js") && !f.startsWith("vendor/") && !refs.has(f) && f !== "service-worker.js" && !/^(content|content-main)\.js$/.test(f)) {
        const usedByHtml = files.some((h) => (h.endsWith(".html") || h === "service-worker.js") && readFileSync(join(dir, h), "utf8").includes(posix.basename(f))); // pages, or importScripts() in the service worker
        if (!usedByHtml) warn(`${f} is not referenced by the manifest or any page (unused code in the package)`);
      }
    }
  }

  // ---- file inventory -----------------------------------------------------------------------
  for (const f of files) {
    const parts = f.split("/");
    if (parts.slice(0, -1).some((d) => FORBIDDEN_DIRS.has(d.toLowerCase()))) err(`forbidden directory in package: ${f}`);
    if (FORBIDDEN_EXT.has(extname(f).toLowerCase()) && f !== "vendor/README.md") err(`forbidden file type in package: ${f}`);
    if (FORBIDDEN_NAME.test(parts[parts.length - 1])) err(`forbidden file name in package: ${f}`);
  }

  // ---- code scan: no remote code, no network, no eval ----------------------------------------
  const BAD_CODE = [
    [/(?<![\w$.])eval\s*\(/, "eval()"],
    [/\bnew\s+Function\s*\(/, "new Function()"],
    [/(?<![\w$.])Function\s*\(\s*["'`]/, "Function(string)"],
    [/\bset(?:Timeout|Interval)\s*\(\s*["'`]/, "string passed to setTimeout/setInterval"],
    [/\b(?:fetch|importScripts|import)\s*\(\s*["'`](?:https?:)?\/\//, "remote fetch/import"],
    [/\bnew\s+(?:WebSocket|EventSource|XMLHttpRequest)\b/, "network API"],
    [/\bsendBeacon\b/, "navigator.sendBeacon"],
    [/\bXMLHttpRequest\b/, "XMLHttpRequest"],
    // fetch( is counted per file below: only the offscreen scanner and the content script's blob: reader may use it
    [/\bchrome\.offscreen\b/, "chrome.offscreen outside the service worker", (f) => !API_ALLOWED_FILES.offscreen.includes(f)],
    [/\bchrome\.permissions\b/, "chrome.permissions outside the service worker and the settings UI", (f) => !API_ALLOWED_FILES.permissions.includes(f)],
    [/\bchrome\.(?:scripting|webRequest|cookies|history|tabs\.executeScript|declarativeNetRequest)\b/, "API outside the permission allowlist"],
    [/\/\/[#@]\s*sourceMappingURL/, "source map reference"],
    [/<script[^>]+src\s*=\s*["']https?:/i, "remote script tag"],
    [/\bSTUB\b/, "STUB marker: an unfinished placeholder module is in the build"],
  ];
  for (const f of files.filter((x) => x.endsWith(".js"))) {
    const buf = readFileSync(join(dir, f));
    const code = buf.toString("utf8");
    if (f.startsWith("vendor/")) {
      // vendored script: allowed only as an exact reviewed file; then only the idioms documented in vendor/README.md
      const pin = VENDOR_SCRIPT_PINS[f];
      if (!pin) err(`${f}: vendored script is not pinned in VENDOR_SCRIPT_PINS (review it, then pin its SHA-256)`);
      else if (createHash("sha256").update(buf).digest("hex") !== pin) err(`${f}: content differs from the reviewed, pinned file; re-review and update VENDOR_SCRIPT_PINS`);
      else {
        for (const [re, what] of [[/(?<![\w$.])eval\s*\(/, "eval()"], [/\/\/[#@]\s*sourceMappingURL/, "source map reference"], [/\bsendBeacon\b/, "navigator.sendBeacon"], [/\bnew\s+(?:WebSocket|EventSource)\b/, "network API"]]) {
          if (re.test(code)) err(`${f}: forbidden pattern in vendored code: ${what}`);
        }
      }
      continue;
    }
    // Chrome refuses to load a content script containing Unicode noncharacters (e.g. a raw U+FFFF).
    for (const ch of code) {
      const c = ch.codePointAt(0);
      if (c === 0 || (c >= 0xfdd0 && c <= 0xfdef) || (c & 0xfffe) === 0xfffe) { err(`${f}: contains a Unicode noncharacter or NUL (U+${c.toString(16).toUpperCase()}); Chrome refuses to load the file. Write it as an escape sequence in source.`); break; }
    }
    for (const [re, what, applies] of BAD_CODE) {
      if ((!applies || applies(f)) && re.test(code)) err(`${f}: forbidden pattern: ${what}`);
    }
    const fetches = (code.match(/(?<![\w$.])fetch\s*\(/g) || []).length;
    if (fetches > (FETCH_ALLOWED[f] || 0)) err(`${f}: fetch() is not allowed here (${fetches} call(s); allowed: ${FETCH_ALLOWED[f] || 0})`);
    if (UI_FILES.test(posix.basename(f))) {
      const hit = SPOILERISH.exec(code);
      if (hit) err(`${f}: UI strings contain spoiler-ish wording "${hit[0]}"`);
    }
    if (/\.innerHTML\s*=|insertAdjacentHTML|document\.write\s*\(/.test(code) && UI_FILES.test(posix.basename(f))) err(`${f}: HTML string injection in a UI script`);
  }

  return { errors, warnings, files, totalBytes };
}

function main() {
  const dir = process.argv[2] ? join(process.cwd(), process.argv[2]) : join(ROOT, "dist", "chrome");
  const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));
  const { errors, warnings, files, totalBytes } = validateDist(dir, { version: pkg.version });
  for (const w of warnings) console.warn(`warning: ${w}`);
  if (errors.length) {
    for (const e of errors) console.error(`error: ${e}`);
    console.error(`\nValidation FAILED with ${errors.length} error(s).`);
    process.exit(1);
  }
  console.log(`Validation passed: ${files.length} files, ${totalBytes} bytes, version ${pkg.version}, ${warnings.length} warning(s).`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) main();
