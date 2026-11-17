// Writes fixtures/ocr-text.png and fixtures/ocr-spoiler.png: black block letters (a 5x7 pixel font, scaled 8x) on white, spelling a harmless
// fixed phrase. No dependencies, no window, no network. The Rust test `the_real_engine_reads_the_png_fixture`
// (src/spoiler_ocr.rs, `cargo test spoiler_ocr -- --ignored --nocapture`) and the live run read it back.
//   node extensions/spoiler-firewall/test/bowser/make-ocr-fixture.mjs
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { deflateSync } from "node:zlib";

export const PHRASE = "SPOILER TEST 123";
const SCALE = 12;
const GAP = 2; // columns between glyphs, in font pixels
const PAD = 4; // font pixels around the text

const FONT = {
  " ": ["00000", "00000", "00000", "00000", "00000", "00000", "00000"],
  S: ["01111", "10000", "10000", "01110", "00001", "00001", "11110"],
  P: ["11110", "10001", "10001", "11110", "10000", "10000", "10000"],
  O: ["01110", "10001", "10001", "10001", "10001", "10001", "01110"],
  I: ["01110", "00100", "00100", "00100", "00100", "00100", "01110"],
  L: ["10000", "10000", "10000", "10000", "10000", "10000", "11111"],
  E: ["11111", "10000", "10000", "11110", "10000", "10000", "11111"],
  R: ["11110", "10001", "10001", "11110", "10100", "10010", "10001"],
  T: ["11111", "00100", "00100", "00100", "00100", "00100", "00100"],
  1: ["00100", "01100", "00100", "00100", "00100", "00100", "01110"],
  2: ["01110", "10001", "00001", "00010", "00100", "01000", "11111"],
  3: ["11110", "00001", "00001", "01110", "00001", "00001", "11110"],
  G: ["01110", "10001", "10000", "10111", "10001", "10001", "01111"],
  A: ["01110", "10001", "10001", "11111", "10001", "10001", "10001"],
  N: ["10001", "11001", "10101", "10011", "10001", "10001", "10001"],
  D: ["11110", "10001", "10001", "10001", "10001", "10001", "11110"],
  K: ["10001", "10010", "10100", "11000", "10100", "10010", "10001"],
  6: ["00110", "01000", "10000", "11110", "10001", "10001", "01110"],
};

// Harmless fixed text for the engine test, and a spoiler-shaped phrase the live run expects the shield to catch.
export const FIXTURES = [["ocr-text.png", PHRASE], ["ocr-spoiler.png", "GTA 6 ENDING LEAK"]];

// Hard-edged block letters read poorly; a small box blur gives the soft edges of real rendered text.
function soften(gray, width, height, radius = 4) {
  const stride = width + 1;
  const tmp = new Float32Array(width * height);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      let sum = 0, n = 0;
      for (let k = -radius; k <= radius; k++) { const xx = x + k; if (xx >= 0 && xx < width) { sum += gray[y * stride + 1 + xx]; n++; } }
      tmp[y * width + x] = sum / n;
    }
  }
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      let sum = 0, n = 0;
      for (let k = -radius; k <= radius; k++) { const yy = y + k; if (yy >= 0 && yy < height) { sum += tmp[yy * width + x]; n++; } }
      gray[y * stride + 1 + x] = Math.round(sum / n);
    }
  }
}

export function renderPhrase(text = PHRASE) {
  const cols = text.length * (5 + GAP) - GAP + 2 * PAD;
  const rows = 7 + 2 * PAD;
  const width = cols * SCALE, height = rows * SCALE;
  const gray = Buffer.alloc(height * (width + 1), 255); // filter byte 0 per row, then white pixels
  for (let y = 0; y < height; y++) gray[y * (width + 1)] = 0;
  [...text].forEach((ch, index) => {
    const glyph = FONT[ch];
    if (!glyph) throw new Error(`no glyph for ${JSON.stringify(ch)}`);
    for (let gy = 0; gy < 7; gy++) {
      for (let gx = 0; gx < 5; gx++) {
        if (glyph[gy][gx] !== "1") continue;
        const x0 = (PAD + index * (5 + GAP) + gx) * SCALE, y0 = (PAD + gy) * SCALE;
        for (let y = y0; y < y0 + SCALE; y++) gray.fill(0, y * (width + 1) + 1 + x0, y * (width + 1) + 1 + x0 + SCALE);
      }
    }
  });
  soften(gray, width, height);
  return { width, height, gray };
}

const CRC = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; table[n] = c >>> 0; }
  return (buf) => { let c = 0xffffffff; for (const b of buf) c = table[(c ^ b) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
})();

function chunk(type, data) {
  const body = Buffer.concat([Buffer.from(type, "latin1"), data]);
  const out = Buffer.alloc(8 + data.length + 4);
  out.writeUInt32BE(data.length, 0);
  body.copy(out, 4);
  out.writeUInt32BE(CRC(body), 8 + data.length);
  return out;
}

export function encodePng({ width, height, gray }) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0); ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; ihdr[9] = 0; // 8-bit grayscale
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(gray)),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const dir = join(dirname(fileURLToPath(import.meta.url)), "fixtures");
  mkdirSync(dir, { recursive: true });
  for (const [name, text] of FIXTURES) {
    const png = encodePng(renderPhrase(text));
    writeFileSync(join(dir, name), png);
    console.log(`wrote fixtures/${name} (${png.length} bytes, text ${JSON.stringify(text)})`);
  }
}
