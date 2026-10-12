// Fingerprint source and built assets using the same path/NUL/bytes FNV-1a 64 as build.rs.
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const mask = (1n << 64n) - 1n;

function filesUnder(root, dir = root) {
  return readdirSync(dir, { withFileTypes: true }).flatMap(entry => {
    const path = join(dir, entry.name);
    return entry.isDirectory() ? filesUnder(root, path) : entry.isFile() ? [path] : [];
  });
}

function fingerprint(name) {
  const root = join(here, name);
  const files = filesUnder(root).map(path => [relative(root, path).split(sep).join('/'), path]);
  files.sort((a, b) => Buffer.compare(Buffer.from(a[0]), Buffer.from(b[0])));
  let hash = 0xcbf29ce484222325n;
  for (const [name, path] of files) {
    for (const byte of Buffer.concat([Buffer.from(name, 'utf8'), Buffer.from([0]), readFileSync(path)])) {
      hash = ((hash ^ BigInt(byte)) * 0x100000001b3n) & mask;
    }
  }
  return hash.toString(16).padStart(16, '0');
}

writeFileSync(join(here, 'dist.manifest'), `src=${fingerprint('src')}\ndist=${fingerprint('dist')}\n`);
