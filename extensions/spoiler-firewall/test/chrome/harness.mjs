// Shared helpers for the real-Chrome test and the store graphics scripts.
// playwright-core is installed locally under test/chrome (see package.json there); nothing else in
// the repository depends on it.
import { createRequire } from "node:module";
import { existsSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
export const ROOT = join(HERE, "..", "..");
export const DIST_CHROME = process.env.SFW_EXTENSION_DIR || join(ROOT, "dist", "chrome");
const require = createRequire(import.meta.url);

export function loadPlaywright() {
  try {
    return require("playwright-core");
  } catch {
    throw new Error("playwright-core is not installed. Run: npm install --prefix extensions/spoiler-firewall/test/chrome");
  }
}

// Branded Chrome no longer honours --load-extension, so use a Playwright-managed Chromium build.
// SFW_CHROMIUM overrides; otherwise try Playwright's expected path, then any installed chromium-*.
export function findChromium(playwright) {
  if (process.env.SFW_CHROMIUM && existsSync(process.env.SFW_CHROMIUM)) return process.env.SFW_CHROMIUM;
  try {
    const expected = playwright.chromium.executablePath();
    if (expected && existsSync(expected)) return expected;
  } catch { /* fall through */ }
  const base = process.env.PLAYWRIGHT_BROWSERS_PATH || (process.platform === "win32"
    ? join(process.env.LOCALAPPDATA || join(homedir(), "AppData", "Local"), "ms-playwright")
    : process.platform === "darwin" ? join(homedir(), "Library", "Caches", "ms-playwright") : join(homedir(), ".cache", "ms-playwright"));
  if (!existsSync(base)) return null;
  const rel = process.platform === "win32" ? ["chrome-win64", "chrome.exe"]
    : process.platform === "darwin" ? ["chrome-mac", "Chromium.app", "Contents", "MacOS", "Chromium"] : ["chrome-linux", "chrome"];
  const dirs = readdirSync(base).filter((d) => /^chromium-\d+$/.test(d)).sort().reverse();
  for (const d of dirs) {
    const p = join(base, d, ...rel);
    if (existsSync(p)) return p;
  }
  return null;
}

export async function launchWithExtension(playwright, { userDataDir, extensionDir = DIST_CHROME, headless = true, viewport = { width: 1280, height: 800 }, extraArgs = [] }) {
  const executablePath = findChromium(playwright);
  if (!executablePath) throw new Error("No Playwright Chromium found. Run: npx playwright-core install chromium");
  const args = [`--disable-extensions-except=${extensionDir}`, `--load-extension=${extensionDir}`, "--no-first-run", "--no-default-browser-check", "--disable-features=DisableLoadExtensionCommandLineSwitch"];
  args.push("--mute-audio", ...extraArgs);
  if (headless) args.push("--headless=new"); // always headless in automation: no visible windows, no focus stealing
  const context = await playwright.chromium.launchPersistentContext(userDataDir, {
    executablePath,
    headless: false, // real headless flag is passed via args (new headless supports extensions)
    args,
    viewport,
    ignoreDefaultArgs: ["--disable-extensions", "--headless"],
  });
  return context;
}

export async function getServiceWorker(context, timeoutMs = 15000) {
  let [sw] = context.serviceWorkers();
  if (!sw) sw = await context.waitForEvent("serviceworker", { timeout: timeoutMs });
  const extensionId = new URL(sw.url()).host;
  return { sw, extensionId };
}
