// Bundles src/main.ts into dist/app.js and copies the static assets into dist/.
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, 'src');
const distDir = join(here, 'dist');

mkdirSync(distDir, { recursive: true });

await esbuild.build({
  entryPoints: [join(srcDir, 'main.ts')],
  bundle: true,
  minify: true,
  format: 'iife',
  platform: 'browser',
  target: 'chrome110',
  legalComments: 'none',
  outfile: join(distDir, 'app.js'),
  logLevel: 'info',
});

for (const file of ['index.html', 'app.css']) {
  copyFileSync(join(srcDir, file), join(distDir, file));
}

// --- hover layer ---
// app.css @imports the stylesheets under src/styles/; inline them into the one dist/app.css the CSP
// and the embedded asset table allow.
await esbuild.build({
  entryPoints: [join(srcDir, 'app.css')],
  bundle: true,
  outfile: join(distDir, 'app.css'),
  logLevel: 'info',
});
// --- end hover layer ---
