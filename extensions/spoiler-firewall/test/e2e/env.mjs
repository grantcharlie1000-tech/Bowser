// Shared harness environment: browser executables from the local Playwright cache (the installed playwright
// package may expect a newer browser revision than the cached one; executablePath sidesteps that), and the
// hostname -> loopback mapping that lets fixtures run under real site hostnames (adapters match by hostname).
import { readdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";
import os from "node:os";

// Never interfere with the user: below-normal priority (children inherit it), headless only, always muted.
try { os.setPriority(process.pid, os.constants.priority.PRIORITY_BELOW_NORMAL); } catch (e) { /* ignore */ }

const CACHE = process.env.PLAYWRIGHT_BROWSERS_PATH || join(process.env.LOCALAPPDATA || join(homedir(), "AppData", "Local"), "ms-playwright");

function newest(prefix, sub) {
  if (!existsSync(CACHE)) return null;
  const dirs = readdirSync(CACHE).filter((d) => d.startsWith(prefix + "-")).sort((a, b) => Number(b.split("-").pop()) - Number(a.split("-").pop()));
  for (const d of dirs) {
    const p = join(CACHE, d, sub);
    if (existsSync(p)) return p;
  }
  return null;
}

// Windows first (the harness's home), then Linux builds (CI containers).
export const CHROMIUM = newest("chromium", join("chrome-win64", "chrome.exe")) || newest("chromium", join("chrome-linux64", "chrome")) || newest("chromium", join("chrome-linux", "chrome"));
export const HEADLESS_SHELL =
  newest("chromium_headless_shell", join("chrome-headless-shell-win64", "chrome-headless-shell.exe")) ||
  newest("chromium_headless_shell", join("chrome-headless-shell-linux64", "chrome-headless-shell")) ||
  newest("chromium_headless_shell", join("chrome-linux", "headless_shell"));

export const HOSTS = [
  "x.com", "twitter.com", "www.reddit.com", "old.reddit.com", "www.youtube.com", "www.google.com", "www.bing.com", "duckduckgo.com",
  "i.imgur.com", "imgur.com", "gta.fandom.com", "en.wikipedia.org", "www.ign.com", "news.example.test", "blog.example.test", "forum.example.test", "frame.test", "other.test", "example.test",
];
export const RESOLVER_RULES = HOSTS.map((h) => `MAP ${h} 127.0.0.1`).join(", ");
export const BASE_ARGS = ["--mute-audio", "--disable-gpu-vsync", `--host-resolver-rules=${RESOLVER_RULES}`, "--autoplay-policy=no-user-gesture-required", "--disable-features=PrivacySandboxSettings4"];

// Always headless; there is no headed mode in this harness.
export const HEADLESS = true;
