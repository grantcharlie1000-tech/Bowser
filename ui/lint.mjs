// Fails (exit 1) if any file under src/, or the built dist/app.js, uses APIs the CSP-locked chrome
// must not use. The rules live in lint-rules.mjs. dist/app.js must exist: run `npm run build` first.
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { findViolations } from './lint-rules.mjs';

const root = dirname(fileURLToPath(import.meta.url));
const srcDir = join(root, 'src');
const bundle = join(root, 'dist', 'app.js');

function walk(dir) {
  const files = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) files.push(...walk(path));
    else files.push(path);
  }
  return files;
}

let violations = 0;
const report = (file, found) => {
  for (const { line, rule } of found) {
    violations += 1;
    console.error(`${relative(root, file)}:${line}: forbidden ${rule}`);
  }
};

for (const file of walk(srcDir)) {
  report(file, findViolations(readFileSync(file, 'utf8')));
}

if (!existsSync(bundle)) {
  violations += 1;
  console.error('dist/app.js is missing: run npm run build before lint');
} else {
  report(bundle, findViolations(readFileSync(bundle, 'utf8')));
}

if (violations > 0) {
  console.error(`lint: ${violations} violation(s)`);
  process.exit(1);
}
console.log('lint: ok');
