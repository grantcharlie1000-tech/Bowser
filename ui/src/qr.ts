// A small QR code encoder (ISO/IEC 18004): byte mode, error correction level M, versions 1 to 10.
// It makes the code for a page address on this device, from nothing but the address; no service is
// involved. Pure: no DOM, no imports. test/address.mjs proves it with an independent decoder
// (format information, Reed-Solomon syndromes and the decoded text).

/** Error-correction codewords per block and block counts at level M, indexed by version (1..10). */
const ECC_PER_BLOCK = [0, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26] as const;
const BLOCKS = [0, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5] as const;
/** Level M's two format-information bits. */
const FORMAT_M = 0;
const MAX_VERSION = 10;

const sizeOf = (version: number): number => version * 4 + 17;

function alignmentPositions(version: number): number[] {
  if (version === 1) return [];
  const count = Math.floor(version / 7) + 2;
  const step = Math.ceil((version * 4 + 4) / (count * 2 - 2)) * 2;
  const result = [6];
  for (let position = sizeOf(version) - 7; result.length < count; position -= step) result.splice(1, 0, position);
  return result;
}

/** Modules that carry data or error correction (everything but the function patterns). */
function rawDataModules(version: number): number {
  let result = (16 * version + 128) * version + 64;
  if (version >= 2) {
    const count = Math.floor(version / 7) + 2;
    result -= (25 * count - 10) * count - 55;
    if (version >= 7) result -= 36;
  }
  return result;
}

function dataCodewords(version: number): number {
  return Math.floor(rawDataModules(version) / 8) - ECC_PER_BLOCK[version]! * BLOCKS[version]!;
}

// ---- Reed-Solomon over GF(256), x^8 + x^4 + x^3 + x^2 + 1 ----

function gfMultiply(x: number, y: number): number {
  let z = 0;
  for (let i = 7; i >= 0; i -= 1) {
    z = (z << 1) ^ ((z >>> 7) * 0x11d);
    z ^= ((y >>> i) & 1) * x;
  }
  return z;
}

function generator(degree: number): number[] {
  const result = new Array<number>(degree).fill(0);
  result[degree - 1] = 1;
  let root = 1;
  for (let i = 0; i < degree; i += 1) {
    for (let j = 0; j < result.length; j += 1) {
      result[j] = gfMultiply(result[j]!, root);
      if (j + 1 < result.length) result[j] = result[j]! ^ result[j + 1]!;
    }
    root = gfMultiply(root, 0x02);
  }
  return result;
}

function remainder(data: readonly number[], divisor: readonly number[]): number[] {
  const result = new Array<number>(divisor.length).fill(0);
  for (const byte of data) {
    const factor = byte ^ result.shift()!;
    result.push(0);
    divisor.forEach((coefficient, i) => { result[i] = result[i]! ^ gfMultiply(coefficient, factor); });
  }
  return result;
}

// ---- the symbol ----

function utf8(text: string): number[] {
  return Array.from(new TextEncoder().encode(text));
}

function chooseVersion(length: number): number | null {
  for (let version = 1; version <= MAX_VERSION; version += 1) {
    const countBits = version < 10 ? 8 : 16;
    if (4 + countBits + length * 8 <= dataCodewords(version) * 8) return version;
  }
  return null;
}

function codewordsOf(bytes: readonly number[], version: number): number[] {
  const bits: number[] = [];
  const push = (value: number, count: number): void => { for (let i = count - 1; i >= 0; i -= 1) bits.push((value >>> i) & 1); };
  push(0b0100, 4);
  push(bytes.length, version < 10 ? 8 : 16);
  for (const byte of bytes) push(byte, 8);
  const capacity = dataCodewords(version) * 8;
  push(0, Math.min(4, capacity - bits.length));
  push(0, (8 - (bits.length % 8)) % 8);
  for (let pad = 0xec; bits.length < capacity; pad ^= 0xec ^ 0x11) push(pad, 8);
  const codewords = new Array<number>(bits.length / 8).fill(0);
  bits.forEach((bit, i) => { codewords[i >>> 3] = codewords[i >>> 3]! | (bit << (7 - (i & 7))); });
  return codewords;
}

function interleave(data: readonly number[], version: number): number[] {
  const blocks = BLOCKS[version]!;
  const eccLength = ECC_PER_BLOCK[version]!;
  const rawCodewords = Math.floor(rawDataModules(version) / 8);
  const shortBlocks = blocks - (rawCodewords % blocks);
  const shortLength = Math.floor(rawCodewords / blocks);
  const divisor = generator(eccLength);
  const all: number[][] = [];
  for (let i = 0, k = 0; i < blocks; i += 1) {
    const chunk = data.slice(k, k + shortLength - eccLength + (i < shortBlocks ? 0 : 1));
    k += chunk.length;
    const ecc = remainder(chunk, divisor);
    if (i < shortBlocks) chunk.push(0);
    all.push([...chunk, ...ecc]);
  }
  const result: number[] = [];
  for (let i = 0; i < all[0]!.length; i += 1) {
    all.forEach((block, j) => { if (i !== shortLength - eccLength || j >= shortBlocks) result.push(block[i]!); });
  }
  return result;
}

class Code {
  readonly size: number;
  readonly modules: boolean[][];
  readonly isFunction: boolean[][];

  constructor(readonly version: number) {
    this.size = sizeOf(version);
    this.modules = Array.from({ length: this.size }, () => new Array<boolean>(this.size).fill(false));
    this.isFunction = Array.from({ length: this.size }, () => new Array<boolean>(this.size).fill(false));
    this.drawFunctionPatterns();
  }

  private set(x: number, y: number, dark: boolean): void {
    this.modules[y]![x] = dark;
    this.isFunction[y]![x] = true;
  }

  private drawFunctionPatterns(): void {
    const { size } = this;
    for (let i = 0; i < size; i += 1) {
      this.set(6, i, i % 2 === 0);
      this.set(i, 6, i % 2 === 0);
    }
    this.finder(3, 3);
    this.finder(size - 4, 3);
    this.finder(3, size - 4);
    const positions = alignmentPositions(this.version);
    const last = positions.length - 1;
    positions.forEach((cy, i) => positions.forEach((cx, j) => {
      if ((i === 0 && j === 0) || (i === 0 && j === last) || (i === last && j === 0)) return;
      for (let dy = -2; dy <= 2; dy += 1) for (let dx = -2; dx <= 2; dx += 1) this.set(cx + dx, cy + dy, Math.max(Math.abs(dx), Math.abs(dy)) !== 1);
    }));
    this.formatBits(0); // reserves the format areas; the real mask is drawn last
    this.versionBits();
  }

  private finder(cx: number, cy: number): void {
    for (let dy = -4; dy <= 4; dy += 1) {
      for (let dx = -4; dx <= 4; dx += 1) {
        const distance = Math.max(Math.abs(dx), Math.abs(dy));
        const x = cx + dx, y = cy + dy;
        if (x >= 0 && x < this.size && y >= 0 && y < this.size) this.set(x, y, distance !== 2 && distance !== 4);
      }
    }
  }

  formatBits(mask: number): void {
    const data = (FORMAT_M << 3) | mask;
    let rem = data;
    for (let i = 0; i < 10; i += 1) rem = (rem << 1) ^ ((rem >>> 9) * 0x537);
    const bits = ((data << 10) | rem) ^ 0x5412;
    const bit = (i: number): boolean => ((bits >>> i) & 1) !== 0;
    const { size } = this;
    for (let i = 0; i <= 5; i += 1) this.set(8, i, bit(i));
    this.set(8, 7, bit(6));
    this.set(8, 8, bit(7));
    this.set(7, 8, bit(8));
    for (let i = 9; i < 15; i += 1) this.set(14 - i, 8, bit(i));
    for (let i = 0; i < 8; i += 1) this.set(size - 1 - i, 8, bit(i));
    for (let i = 8; i < 15; i += 1) this.set(8, size - 15 + i, bit(i));
    this.set(8, size - 8, true);
  }

  private versionBits(): void {
    if (this.version < 7) return;
    let rem = this.version;
    for (let i = 0; i < 12; i += 1) rem = (rem << 1) ^ ((rem >>> 11) * 0x1f25);
    const bits = (this.version << 12) | rem;
    for (let i = 0; i < 18; i += 1) {
      const dark = ((bits >>> i) & 1) !== 0;
      const a = this.size - 11 + (i % 3);
      const b = Math.floor(i / 3);
      this.set(a, b, dark);
      this.set(b, a, dark);
    }
  }

  placeData(codewords: readonly number[]): void {
    let i = 0;
    for (let right = this.size - 1; right >= 1; right -= 2) {
      if (right === 6) right = 5;
      for (let vertical = 0; vertical < this.size; vertical += 1) {
        for (let j = 0; j < 2; j += 1) {
          const x = right - j;
          const upward = ((right + 1) & 2) === 0;
          const y = upward ? this.size - 1 - vertical : vertical;
          if (!this.isFunction[y]![x] && i < codewords.length * 8) {
            this.modules[y]![x] = ((codewords[i >>> 3]! >>> (7 - (i & 7))) & 1) !== 0;
            i += 1;
          }
        }
      }
    }
  }

  applyMask(mask: number): void {
    for (let y = 0; y < this.size; y += 1) {
      for (let x = 0; x < this.size; x += 1) {
        if (this.isFunction[y]![x]) continue;
        let invert: boolean;
        switch (mask) {
          case 0: invert = (x + y) % 2 === 0; break;
          case 1: invert = y % 2 === 0; break;
          case 2: invert = x % 3 === 0; break;
          case 3: invert = (x + y) % 3 === 0; break;
          case 4: invert = (Math.floor(x / 3) + Math.floor(y / 2)) % 2 === 0; break;
          case 5: invert = ((x * y) % 2) + ((x * y) % 3) === 0; break;
          case 6: invert = (((x * y) % 2) + ((x * y) % 3)) % 2 === 0; break;
          default: invert = (((x + y) % 2) + ((x * y) % 3)) % 2 === 0; break;
        }
        if (invert) this.modules[y]![x] = !this.modules[y]![x];
      }
    }
  }

  penalty(): number {
    const { size, modules } = this;
    let score = 0;
    const lines: boolean[][] = [];
    for (let i = 0; i < size; i += 1) {
      lines.push(modules[i]!);
      lines.push(modules.map((row) => row[i]!));
    }
    for (const line of lines) {
      let run = 1;
      for (let i = 1; i <= line.length; i += 1) {
        if (i < line.length && line[i] === line[i - 1]) { run += 1; continue; }
        if (run >= 5) score += 3 + (run - 5);
        run = 1;
      }
      const text = line.map((dark) => (dark ? '1' : '0')).join('');
      for (const pattern of ['00001011101', '10111010000'] as const) {
        for (let at = text.indexOf(pattern); at >= 0; at = text.indexOf(pattern, at + 1)) score += 40;
      }
    }
    for (let y = 0; y < size - 1; y += 1) {
      for (let x = 0; x < size - 1; x += 1) {
        const colour = modules[y]![x];
        if (colour === modules[y]![x + 1] && colour === modules[y + 1]![x] && colour === modules[y + 1]![x + 1]) score += 3;
      }
    }
    const dark = modules.reduce((sum, row) => sum + row.filter(Boolean).length, 0);
    const total = size * size;
    score += (Math.ceil(Math.abs(dark * 20 - total * 10) / total) - 1) * 10;
    return score;
  }
}

/**
 * The QR code for `text` (UTF-8, level M, the smallest version that fits). `matrix[row][column]` is
 * true for a dark module; there is no quiet zone. `null` for empty text or text that needs more than
 * version 10 (213 bytes).
 */
export function qrMatrix(text: string): boolean[][] | null {
  const bytes = utf8(text);
  if (bytes.length === 0) return null;
  const version = chooseVersion(bytes.length);
  if (version === null) return null;
  const codewords = interleave(codewordsOf(bytes, version), version);
  let best: Code | null = null;
  let bestScore = Infinity;
  for (let mask = 0; mask < 8; mask += 1) {
    const symbol = new Code(version);
    symbol.placeData(codewords);
    symbol.applyMask(mask);
    symbol.formatBits(mask);
    const score = symbol.penalty();
    if (score < bestScore) { best = symbol; bestScore = score; }
  }
  return best ? best.modules : null;
}

/**
 * The matrix as one SVG path of unit squares merged per horizontal run, in a `size` x `size` box
 * that includes a `quiet` module border.
 */
export function qrSvgPath(matrix: readonly (readonly boolean[])[], quiet = 4): { readonly size: number; readonly d: string } {
  const parts: string[] = [];
  matrix.forEach((row, y) => {
    for (let x = 0; x < row.length;) {
      if (!row[x]) { x += 1; continue; }
      let end = x;
      while (end < row.length && row[end]) end += 1;
      parts.push(`M${x + quiet} ${y + quiet}h${end - x}v1h-${end - x}z`);
      x = end;
    }
  });
  return { size: matrix.length + quiet * 2, d: parts.join('') };
}
