// Builds dist/: src/main.ts -> dist/app.js (esbuild), the settings and history pages ->
// dist/settings-page.js and dist/history-page.js (ES modules app.js imports only when that view
// opens, so every other view loads less script), src/styles/app.css -> dist/app.css
// (Tailwind CSS v4 through PostCSS, minified), and copies src/index.html. The output is static and
// depends only on src/, so the same sources always give byte-identical dist files.
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';
import postcss from 'postcss';
import tailwind from '@tailwindcss/postcss';

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

// Each page is self-contained: what it shares with app.js is bundled again rather than split into
// chunks, so assets.rs serves a fixed set of files. Every file here must be listed in assets.rs.
await esbuild.build({
  entryPoints: [join(srcDir, 'settings-page.ts'), join(srcDir, 'history-page.ts'), join(srcDir, 'start-page.ts')],
  bundle: true,
  minify: true,
  format: 'esm',
  platform: 'browser',
  target: 'chrome110',
  legalComments: 'none',
  outdir: distDir,
  entryNames: '[name]',
  logLevel: 'info',
});

const cssEntry = join(srcDir, 'styles', 'app.css');
const css = await postcss([tailwind({ base: srcDir, optimize: { minify: true } })]).process(
  readFileSync(cssEntry, 'utf8'),
  { from: cssEntry, to: join(distDir, 'app.css'), map: false },
);
for (const warning of css.warnings()) console.warn(`app.css: ${warning.toString()}`);
// Normalise line endings so a CRLF checkout of src/ builds the same bytes as an LF one.
writeFileSync(join(distDir, 'app.css'), `${css.css.replace(/\r\n/g, '\n').trimEnd()}\n`);
console.log(`  dist/app.css  ${(Buffer.byteLength(css.css) / 1024).toFixed(1)}kb`);

copyFileSync(join(srcDir, 'index.html'), join(distDir, 'index.html'));
