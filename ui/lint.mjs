// Fails (exit 1) if any file under src/ uses APIs the CSP-locked chrome must not use.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const srcDir = join(root, 'src');

const RULES = [
  { name: 'innerHTML', re: /innerHTML/ },
  { name: 'outerHTML', re: /outerHTML/ },
  { name: 'insertAdjacentHTML', re: /insertAdjacentHTML/ },
  { name: 'eval(', re: /eval\(/ },
  { name: 'new Function', re: /new Function/ },
  { name: 'document.write', re: /document\.write/ },
  { name: 'inline event handler attribute', re: /\son[a-z]+=/ },
];

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
for (const file of walk(srcDir)) {
  const lines = readFileSync(file, 'utf8').split(/\r?\n/);
  lines.forEach((line, index) => {
    for (const rule of RULES) {
      if (rule.re.test(line)) {
        violations += 1;
        console.error(`${relative(root, file)}:${index + 1}: forbidden ${rule.name}`);
      }
    }
  });
}

if (violations > 0) {
  console.error(`lint: ${violations} violation(s)`);
  process.exit(1);
}
console.log('lint: ok');
