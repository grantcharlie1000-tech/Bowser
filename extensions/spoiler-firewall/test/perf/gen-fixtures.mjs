// Generates the synthetic binary fixtures (photos 600x400 PNG/JPEG, avatars, a tiny muted WebM) with the local
// Playwright Chromium (headless, muted). Deterministic (seeded). Run once: node test/perf/gen-fixtures.mjs
import { writeFileSync, mkdirSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import os from "node:os";
import { loadPlaywright, findChromium } from "../chrome/harness.mjs";

try { os.setPriority(process.pid, os.constants.priority.PRIORITY_BELOW_NORMAL); } catch { /* ignore */ }
const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = join(HERE, "fixtures", "img");
mkdirSync(OUT, { recursive: true });
if (existsSync(join(OUT, "clip.webm")) && !process.argv.includes("--force")) { console.log("fixtures exist (use --force)"); process.exit(0); }
const pw = loadPlaywright();
const browser = await pw.chromium.launch({ executablePath: findChromium(pw), headless: true, args: ["--headless=new", "--mute-audio"] });
const page = await browser.newPage();
const PHOTOS = 36, AVATARS = 8;
const out = await page.evaluate(async ({ PHOTOS, AVATARS }) => {
  let s = 12345; const rnd = () => ((s = (s * 1664525 + 1013904223) >>> 0) / 4294967296);
  const res = { photos: [], avatars: [], webm: null };
  const toB64 = (blob) => new Promise((r) => { const f = new FileReader(); f.onload = () => r(f.result.split(",")[1]); f.readAsDataURL(blob); });
  function draw(w, h) {
    const c = document.createElement("canvas"); c.width = w; c.height = h; const g = c.getContext("2d");
    const grad = g.createLinearGradient(0, 0, w, h); grad.addColorStop(0, `hsl(${rnd() * 360},60%,45%)`); grad.addColorStop(1, `hsl(${rnd() * 360},60%,25%)`);
    g.fillStyle = grad; g.fillRect(0, 0, w, h);
    for (let i = 0; i < 70; i++) { g.fillStyle = `hsla(${rnd() * 360},70%,${30 + rnd() * 50}%,${0.25 + rnd() * 0.5})`; g.beginPath(); if (rnd() < 0.5) g.arc(rnd() * w, rnd() * h, 5 + rnd() * w / 6, 0, 7); else g.rect(rnd() * w, rnd() * h, 10 + rnd() * w / 4, 10 + rnd() * h / 4); g.fill(); }
    const id = g.getImageData(0, 0, w, h), d = id.data; for (let i = 0; i < d.length; i += 4) { const n = (rnd() - 0.5) * 18; d[i] += n; d[i + 1] += n; d[i + 2] += n; } g.putImageData(id, 0, 0);
    return c;
  }
  for (let i = 0; i < PHOTOS; i++) { const png = i % 3 === 0; const c = draw(600, 400); const b = await new Promise((r) => c.toBlob(r, png ? "image/png" : "image/jpeg", 0.82)); res.photos.push({ ext: png ? "png" : "jpg", b64: await toB64(b) }); }
  for (let i = 0; i < AVATARS; i++) { const c = draw(96, 96); const b = await new Promise((r) => c.toBlob(r, "image/jpeg", 0.8)); res.avatars.push(await toB64(b)); }
  try {
    const c = document.createElement("canvas"); c.width = 320; c.height = 180; const g = c.getContext("2d");
    const stream = c.captureStream(15); const rec = new MediaRecorder(stream, { mimeType: "video/webm;codecs=vp8", videoBitsPerSecond: 250000 });
    const chunks = []; rec.ondataavailable = (e) => chunks.push(e.data);
    const done = new Promise((r) => (rec.onstop = r)); rec.start(); const t0 = performance.now();
    await new Promise((r) => { (function f() { const t = (performance.now() - t0) / 1000; g.fillStyle = `hsl(${t * 90 % 360},60%,40%)`; g.fillRect(0, 0, 320, 180); g.fillStyle = "#fff"; g.fillRect(20 + (t * 80) % 260, 70, 40, 40); if (t < 2.5) requestAnimationFrame(f); else r(); })(); });
    rec.stop(); await done; res.webm = await toB64(new Blob(chunks, { type: "video/webm" }));
  } catch (e) { res.webmError = String(e); }
  return res;
}, { PHOTOS, AVATARS });
out.photos.forEach((p, i) => writeFileSync(join(OUT, `photo-${i}.${p.ext}`), Buffer.from(p.b64, "base64")));
out.avatars.forEach((b, i) => writeFileSync(join(OUT, `avatar-${i}.jpg`), Buffer.from(b, "base64")));
if (out.webm) writeFileSync(join(OUT, "clip.webm"), Buffer.from(out.webm, "base64")); else console.log("webm failed", out.webmError);
writeFileSync(join(OUT, "manifest.json"), JSON.stringify({ photos: out.photos.map((p) => p.ext), avatars: AVATARS }));
await browser.close();
console.log("generated", out.photos.length, "photos,", out.avatars.length, "avatars, webm", out.webm ? Buffer.from(out.webm, "base64").length + " bytes" : "none");
