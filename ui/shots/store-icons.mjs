// Real PNG extension icons for the installation shots and tests, drawn here (nothing is downloaded):
// 128x128 RGBA, 4x4 supersampled, encoded with node:zlib. Each one stresses the identity tile:
//   brand  red shield on transparency (the common case, like uBlock Origin Lite)
//   tile   opaque blue rounded square with a white shield (a full-bleed icon)
//   dark   near-black shield on transparency (would vanish on a dark tile without help)
//   white  opaque white square with a green shield (would vanish on a white page without an edge)
import { deflateSync } from 'node:zlib';

const SIZE = 128;
const crcTable = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
const crc = (buf) => { let c = 0xffffffff; for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
const chunk = (type, data) => {
  const out = Buffer.alloc(12 + data.length);
  out.writeUInt32BE(data.length, 0); out.write(type, 4, 'ascii'); data.copy(out, 8);
  out.writeUInt32BE(crc(out.subarray(4, 8 + data.length)), 8 + data.length); return out;
};
function png(rgba) {
  const header = Buffer.alloc(13);
  header.writeUInt32BE(SIZE, 0); header.writeUInt32BE(SIZE, 4); header[8] = 8; header[9] = 6; // 8-bit RGBA
  const raw = Buffer.alloc(SIZE * (SIZE * 4 + 1));
  for (let y = 0; y < SIZE; y++) rgba.copy(raw, y * (SIZE * 4 + 1) + 1, y * SIZE * 4, (y + 1) * SIZE * 4);
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk('IHDR', header), chunk('IDAT', deflateSync(raw, { level: 9 })), chunk('IEND', Buffer.alloc(0))]);
}

/** A heater shield in unit coordinates, centred on (cx, cy) at scale s. */
const shield = (cx, cy, s) => (x, y) => {
  const u = (x - cx) / s; const v = (y - cy) / s; // v in [-0.5, 0.5]
  if (v < -0.4 + 0.1 * (u / 0.36) ** 2 || v > 0.46) return false; // top edge dips toward the corners
  const t = Math.max(0, (v + 0.02) / 0.48);
  return Math.abs(u) <= 0.36 * Math.pow(Math.max(0, 1 - t * t), 0.75);
};
const square = (inset, radius) => (x, y) => {
  const dx = Math.max(Math.abs(x - 0.5) - (0.5 - inset - radius), 0); const dy = Math.max(Math.abs(y - 0.5) - (0.5 - inset - radius), 0);
  return dx * dx + dy * dy <= radius * radius;
};
/** Layers painted in order, each a [shape, [r, g, b]]; the background is transparent. */
function draw(layers) {
  const out = Buffer.alloc(SIZE * SIZE * 4);
  for (let py = 0; py < SIZE; py++) for (let px = 0; px < SIZE; px++) {
    let r = 0; let g = 0; let b = 0; let a = 0;
    for (const [shape, [lr, lg, lb]] of layers) {
      let hits = 0;
      for (let sy = 0; sy < 4; sy++) for (let sx = 0; sx < 4; sx++) if (shape((px + (sx + 0.5) / 4) / SIZE, (py + (sy + 0.5) / 4) / SIZE)) hits++;
      const k = hits / 16; if (!k) continue;
      // Straight-alpha "over": the new layer at coverage k on top of what is there.
      const na = k + a * (1 - k);
      r = (lr * k + r * a * (1 - k)) / na; g = (lg * k + g * a * (1 - k)) / na; b = (lb * k + b * a * (1 - k)) / na; a = na;
    }
    const i = (py * SIZE + px) * 4;
    out[i] = Math.round(r); out[i + 1] = Math.round(g); out[i + 2] = Math.round(b); out[i + 3] = Math.round(a * 255);
  }
  return out;
}
const DESIGNS = {
  brand: [[shield(0.5, 0.5, 0.92), [196, 40, 36]], [shield(0.5, 0.49, 0.56), [255, 255, 255]], [shield(0.5, 0.48, 0.36), [196, 40, 36]]],
  tile: [[square(0.04, 0.2), [37, 99, 235]], [shield(0.5, 0.52, 0.62), [255, 255, 255]]],
  dark: [[shield(0.5, 0.5, 0.92), [24, 26, 31]], [shield(0.5, 0.49, 0.5), [70, 76, 88]]],
  white: [[square(0, 0.0001), [255, 255, 255]], [shield(0.5, 0.52, 0.7), [22, 150, 90]]],
};
/** `data:image/png;base64,…` for each design, as Rust would send it in the snapshot's `icon`. */
export function storeIcons() {
  return Object.fromEntries(Object.entries(DESIGNS).map(([name, layers]) => [name, `data:image/png;base64,${png(draw(layers)).toString('base64')}`]));
}
