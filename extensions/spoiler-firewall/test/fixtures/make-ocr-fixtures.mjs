// Generates the OCR fixture images (test/fixtures/ocr-images/*.png) and ocr-expected.json with a headless
// Chromium screenshot of generated HTML: no network, no external fonts, fictional text only (the "ending" is invented).
//
//   node test/fixtures/make-ocr-fixtures.mjs        (needs playwright-core: npm install --prefix test/chrome)
//
// The PNGs are committed; regenerate only to change them. Text is drawn in "Impact" with a white fill and a black
// outline over a busy picture-like background (gradients + noise), the classic meme look, because that is what the
// scanner has to read on social sites.
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { loadPlaywright, findChromium } from "../chrome/harness.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = join(HERE, "ocr-images");
mkdirSync(OUT, { recursive: true });

const W = 640, H = 400;
const BG = (seed) => `
<canvas id="bg" width="${W}" height="${H}" style="position:absolute;inset:0"></canvas>
<script>(function () {
  // busy photo-like background: layered gradients, blobs and per-pixel noise (seeded: reproducible)
  let s = ${seed};
  const rnd = () => ((s = (Math.imul(s, 1664525) + 1013904223) >>> 0) / 4294967296);
  const c = document.getElementById('bg'), g = c.getContext('2d');
  const grad = g.createLinearGradient(0, 0, ${W}, ${H});
  const hue = ${seed % 360};
  grad.addColorStop(0, 'hsl(' + hue + ',70%,45%)'); grad.addColorStop(.5, 'hsl(' + (hue + 70) + ',60%,30%)'); grad.addColorStop(1, 'hsl(' + (hue + 160) + ',75%,55%)');
  g.fillStyle = grad; g.fillRect(0, 0, ${W}, ${H});
  for (let i = 0; i < 26; i++) { g.fillStyle = 'hsla(' + (rnd() * 360) + ',70%,' + (20 + rnd() * 55) + '%,.55)'; g.beginPath(); g.ellipse(rnd() * ${W}, rnd() * ${H}, 30 + rnd() * 120, 20 + rnd() * 80, rnd() * 3, 0, 7); g.fill(); }
  const d = g.getImageData(0, 0, ${W}, ${H}); for (let i = 0; i < d.data.length; i += 4) { const n = (rnd() - .5) * 46; d.data[i] += n; d.data[i + 1] += n; d.data[i + 2] += n; } g.putImageData(d, 0, 0);
})();</script>`;

const memeCss = `
  html,body{margin:0;width:${W}px;height:${H}px;overflow:hidden;position:relative;background:#222}
  .cap{position:absolute;left:12px;right:12px;text-align:center;font:900 44px/1.08 Impact,"Arial Narrow Bold","Arial Black",sans-serif;text-transform:uppercase;color:#fff;
    -webkit-text-stroke:3px #000;paint-order:stroke fill;text-shadow:3px 3px 0 #000,-3px -3px 0 #000,3px -3px 0 #000,-3px 3px 0 #000;letter-spacing:.5px}
  .top{top:10px}.bottom{bottom:12px}`;

const meme = (seed, top, bottom) => `<!doctype html><meta charset=utf-8><style>${memeCss}</style><body>${BG(seed)}<div class="cap top">${top}</div><div class="cap bottom">${bottom}</div></body>`;

const tweet = (name, handle, text, extra = "") => `<!doctype html><meta charset=utf-8><style>
  html,body{margin:0;width:${W}px;height:${H}px;background:#fff;font:16px/1.35 "Segoe UI",Arial,sans-serif;color:#0f1419}
  .t{padding:22px 26px}.h{display:flex;align-items:center;gap:12px}.av{width:48px;height:48px;border-radius:50%;background:linear-gradient(135deg,#5b7cfa,#b04bd9)}
  .n{font-weight:700}.u{color:#536471}.x{font-size:26px;margin:16px 0 14px;line-height:1.3}.m{color:#536471;font-size:14px;border-top:1px solid #eff3f4;padding-top:10px;margin-top:16px}</style>
  <body><div class="t"><div class="h"><div class="av"></div><div><div class="n">${name}</div><div class="u">${handle}</div></div></div><div class="x">${text}</div>${extra}<div class="m">10:42 AM &middot; Nov 18, 2026 &middot; 48K Views</div></div></body>`;

const textfree = `<!doctype html><meta charset=utf-8><style>html,body{margin:0;width:${W}px;height:${H}px;overflow:hidden;position:relative}</style><body>${BG(77)}</body>`;

// file -> { html, spoiler: expected verdict of the whole pipeline (true = must be shielded), says: what the image says }
const FIXTURES = {
  "ocr-meme-spoiler": { html: meme(17, "GTA 6 ENDING:", "JASON TAKES THE FALL FOR LUCIA"), spoiler: true, says: "GTA 6 ENDING: JASON TAKES THE FALL FOR LUCIA" },
  "ocr-meme-leet": { html: meme(203, "GT4 6 3ND1NG SP01L3R:", "LU(1A D13S 4T TH3 D0CKS"), spoiler: true, says: "GT4 6 3ND1NG SP01L3R: LU(1A D13S 4T TH3 D0CKS (leetspeak)" },
  "ocr-meme-cooking": { html: meme(41, "ME WAITING FOR THE", "PASTA TO BOIL"), spoiler: false, says: "ME WAITING FOR THE PASTA TO BOIL" },
  "ocr-meme-cooking2": { html: meme(290, "WHEN THE OVEN BEEPS", "AND THE BREAD IS PERFECT"), spoiler: false, says: "WHEN THE OVEN BEEPS AND THE BREAD IS PERFECT" },
  "ocr-tweet-spoiler": { html: tweet("Leaky McLeakface", "@totally_fake_leaks", "GTA 6 ending leaked: the hero dies in the final mission and the partner takes the money.", '<div style="height:60px"></div>'), spoiler: true, says: "GTA 6 ending leaked: the hero dies in the final mission ..." },
  "ocr-tweet-clean": { html: tweet("Garden Gnome", "@gnome_gardens", "Tomatoes are finally ripe. Sharing a recipe for the weekend: slow roasted, olive oil, basil.", '<div style="height:60px"></div>'), spoiler: false, says: "Tomatoes are finally ripe ..." },
  "ocr-textfree": { html: textfree, spoiler: false, says: "(no text)" },
};

const pw = loadPlaywright();
const exe = findChromium(pw);
const browser = await pw.chromium.launch({ executablePath: exe, headless: true });
const page = await browser.newPage({ viewport: { width: W, height: H }, deviceScaleFactor: 1 });
const expected = {};
for (const [name, f] of Object.entries(FIXTURES)) {
  page.removeAllListeners("pageerror"); page.on("pageerror", (e) => console.log("pageerror", name, e.message));
  await page.setContent(f.html, { waitUntil: "load" });
  await page.waitForTimeout(150);
  await page.screenshot({ path: join(OUT, `${name}.png`), type: "png" });
  expected[`${name}.png`] = { spoiler: f.spoiler, says: f.says };
  console.log("wrote", name);
}
await browser.close();
writeFileSync(join(HERE, "ocr-expected.json"), JSON.stringify(expected, null, 2) + "\n");
