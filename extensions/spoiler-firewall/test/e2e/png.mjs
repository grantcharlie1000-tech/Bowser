// Minimal PNG decoder (8-bit RGB/RGBA/gray, non-interlaced) — enough for CDP screenshots. No dependencies.
import { inflateSync } from "node:zlib";

export function decodePng(buf) {
  if (buf.readUInt32BE(0) !== 0x89504e47) throw new Error("not a png");
  let off = 8, width = 0, height = 0, depth = 0, ctype = 0;
  const idat = [];
  while (off < buf.length) {
    const len = buf.readUInt32BE(off);
    const type = buf.toString("latin1", off + 4, off + 8);
    const data = buf.subarray(off + 8, off + 8 + len);
    if (type === "IHDR") { width = data.readUInt32BE(0); height = data.readUInt32BE(4); depth = data[8]; ctype = data[9]; if (data[12] !== 0) throw new Error("interlaced"); }
    else if (type === "IDAT") idat.push(data);
    else if (type === "IEND") break;
    off += 12 + len;
  }
  if (depth !== 8) throw new Error("depth " + depth);
  const bpp = ctype === 6 ? 4 : ctype === 2 ? 3 : ctype === 0 ? 1 : 0;
  if (!bpp) throw new Error("ctype " + ctype);
  const raw = inflateSync(Buffer.concat(idat));
  const stride = width * bpp;
  const out = Buffer.alloc(height * stride);
  let prev = Buffer.alloc(stride);
  for (let y = 0; y < height; y++) {
    const f = raw[y * (stride + 1)];
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    const cur = out.subarray(y * stride, (y + 1) * stride);
    for (let i = 0; i < stride; i++) {
      const a = i >= bpp ? cur[i - bpp] : 0, b = prev[i], c = i >= bpp ? prev[i - bpp] : 0;
      let v = line[i];
      if (f === 1) v += a; else if (f === 2) v += b; else if (f === 3) v += (a + b) >> 1;
      else if (f === 4) { const p = a + b - c, pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c); v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c; }
      cur[i] = v & 255;
    }
    prev = cur;
  }
  return { width, height, channels: bpp, data: out };
}

// Count pixels close to the sentinel colour (default #ff00ff).
export function countSentinel(img, target = [255, 0, 255], tol = 56) {
  const { data, channels } = img;
  let n = 0;
  if (channels < 3) return 0;
  for (let i = 0; i < data.length; i += channels) {
    if (Math.abs(data[i] - target[0]) < tol && Math.abs(data[i + 1] - target[1]) < tol && Math.abs(data[i + 2] - target[2]) < tol) n++;
  }
  return n;
}
