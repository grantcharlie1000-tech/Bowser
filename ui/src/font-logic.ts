// Pure logic for `font_size` x `ui_zoom` and `ui_font` in the HTML chrome: the two inputs view.ts
// applyTheme writes on <html> for the type tokens in styles/tokens.css. No DOM here; test/styles.mjs
// and test/chrome-styles.mjs cover it.

/** CSS generic families: kept as keywords, not quoted into a family name that matches nothing. */
const GENERIC_FAMILIES = new Set([
  'serif', 'sans-serif', 'monospace', 'cursive', 'fantasy', 'system-ui', 'ui-serif', 'ui-sans-serif', 'ui-monospace', 'ui-rounded', 'math', 'emoji', 'fangsong',
]);

/** `--bw-font-scale` for the host's `fontScale` (a percentage, 100 = the design sizes). */
export function fontScaleValue(fontScale: number): string {
  return String(fontScale / 100);
}

/**
 * The `--bw-font` value for `ui_font`: each comma-separated face (quotes optional) as a CSS string,
 * generic families as keywords, then the stock stack for anything the faces lack. `null` when
 * `ui_font` names nothing, so the stock stack applies unchanged.
 */
export function uiFontStack(uiFont: string): string | null {
  const faces = uiFont
    .split(',')
    .map((face) => face.trim().replace(/^(["'])(.*)\1$/, '$2').trim())
    .filter((face) => face !== '');
  if (faces.length === 0) return null;
  const quoted = faces.map((face) => (GENERIC_FAMILIES.has(face.toLowerCase()) ? face.toLowerCase() : `"${face.replace(/["\\]/g, '\\$&')}"`));
  return [...quoted, 'var(--bw-font-stock)'].join(', ');
}
