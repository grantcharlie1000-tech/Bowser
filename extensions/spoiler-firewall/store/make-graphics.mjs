// Generates the extension icons and the store promo tile from original SVG artwork (a shield with
// a crossed-out eye). No Rockstar or Take-Two marks, fonts or imagery are used.
//
//   node store/make-graphics.mjs
//
// Output (committed): hosts/chrome/icons/icon{16,32,48,128}.png, store/assets/store-icon-128.png,
// store/assets/promo-small-440x280.png. Needs playwright-core (npm install --prefix test/chrome).
import { mkdirSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { loadPlaywright, findChromium } from "../test/chrome/harness.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const ICON_DIR = join(ROOT, "hosts", "chrome", "icons");
const ASSET_DIR = join(ROOT, "store", "assets");

// Shield with a crossed-out eye. `small` thickens strokes for 16/32 px so it stays legible.
export function iconSvg(size) {
  const small = size <= 32;
  const eyeStroke = small ? 11 : 7;
  const slashStroke = small ? 13 : 8;
  const gap = slashStroke + (small ? 7 : 7);
  const pupil = small ? 0 : 10;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 128 128">
  <defs>
    <linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#7a5cf2"/><stop offset="1" stop-color="#3b2a9e"/></linearGradient>
  </defs>
  <path d="M64 6 L112 24 V62 C112 93 91 114 64 124 C37 114 16 93 16 62 V24 Z" fill="url(#g)"/>
  <path d="M64 6 L112 24 V62 C112 93 91 114 64 124 C37 114 16 93 16 62 V24 Z" fill="none" stroke="#ffffff" stroke-opacity="0.35" stroke-width="3"/>
  <path d="M28 64 Q64 30 100 64 Q64 98 28 64 Z" fill="none" stroke="#ffffff" stroke-width="${eyeStroke}" stroke-linejoin="round"/>
  ${pupil ? `<circle cx="64" cy="64" r="${pupil}" fill="#ffffff"/>` : ""}
  <line x1="36" y1="96" x2="92" y2="32" stroke="#4a37bd" stroke-width="${gap}" stroke-linecap="round"/>
  <line x1="36" y1="96" x2="92" y2="32" stroke="#ffffff" stroke-width="${slashStroke}" stroke-linecap="round"/>
</svg>`;
}

function promoHtml() {
  return `<!doctype html><meta charset="utf-8"><style>
  html,body{margin:0;width:440px;height:280px;overflow:hidden}
  body{font:600 16px/1.3 "Segoe UI",system-ui,sans-serif;color:#fff;
    background:radial-gradient(120% 140% at 0% 0%,#4a37bd 0%,#241a63 55%,#14102f 100%);
    display:grid;grid-template-columns:150px 1fr;align-items:center;padding:0 28px;gap:12px;box-sizing:border-box}
  h1{font-size:30px;line-height:1.05;margin:0 0 8px;letter-spacing:-.02em;font-weight:750}
  p{margin:0;font-weight:500;font-size:15px;color:#d8d2ff}
  small{display:block;margin-top:14px;font-size:11.5px;color:#a79fe0;font-weight:500}
  </style><body><div>${iconSvg(128).replace('width="128" height="128"', 'width="132" height="132"')}</div>
  <div><h1>Spoiler Firewall</h1><p>Hides GTA VI spoilers while you browse. Private and on-device.</p>
  <small>Unofficial fan tool. Not affiliated with Rockstar Games.</small></div></body>`;
}

async function main() {
  const playwright = loadPlaywright();
  const executablePath = findChromium(playwright);
  if (!executablePath) throw new Error("No Chromium found");
  mkdirSync(ICON_DIR, { recursive: true });
  mkdirSync(ASSET_DIR, { recursive: true });
  const browser = await playwright.chromium.launch({ executablePath });
  try {
    for (const size of [16, 32, 48, 128]) {
      const page = await browser.newPage({ viewport: { width: size, height: size }, deviceScaleFactor: 1 });
      await page.setContent(`<!doctype html><style>html,body{margin:0;background:transparent}svg{display:block}</style>${iconSvg(size)}`);
      const png = await page.screenshot({ omitBackground: true, clip: { x: 0, y: 0, width: size, height: size } });
      writeFileSync(join(ICON_DIR, `icon${size}.png`), png);
      if (size === 128) writeFileSync(join(ASSET_DIR, "store-icon-128.png"), png);
      await page.close();
    }
    const promo = await browser.newPage({ viewport: { width: 440, height: 280 }, deviceScaleFactor: 1 });
    await promo.setContent(promoHtml());
    writeFileSync(join(ASSET_DIR, "promo-small-440x280.png"), await promo.screenshot({ clip: { x: 0, y: 0, width: 440, height: 280 } }));
    writeFileSync(join(ASSET_DIR, "icon.svg"), iconSvg(128));
  } finally {
    await browser.close();
  }
  console.log("Wrote icons to hosts/chrome/icons and graphics to store/assets");
}

main().catch((e) => { console.error(e); process.exit(1); });
