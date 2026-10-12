// DOM helpers. Everything is built with createElement/createElementNS; no HTML strings are parsed.

const SVG_NS = 'http://www.w3.org/2000/svg';

export function el<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  return node;
}

export type IconName = 'back' | 'forward' | 'reload' | 'stop' | 'plus' | 'menu' | 'lock' | 'warning' | 'info';

const ICON_PATHS: Readonly<Record<IconName, readonly string[]>> = {
  back: ['M10 3.5 5.5 8l4.5 4.5'],
  forward: ['M6 3.5 10.5 8 6 12.5'],
  reload: ['M12.8 8a4.8 4.8 0 1 1-1.4-3.4', 'M12.8 2.8v2.6h-2.6'],
  stop: ['M4.5 4.5l7 7', 'M11.5 4.5l-7 7'],
  plus: ['M8 3v10', 'M3 8h10'],
  menu: ['M8 3.25v.01', 'M8 8v.01', 'M8 12.75v.01'],
  lock: ['M3.5 7.5h9v6h-9z', 'M5.5 7.5V5.25a2.5 2.5 0 0 1 5 0V7.5'],
  warning: ['M8 2.5 14 13H2z', 'M8 6.5v3', 'M8 11.25v.01'],
  info: ['M8 1.75a6.25 6.25 0 1 0 0 12.5 6.25 6.25 0 1 0 0-12.5z', 'M8 7.25v4', 'M8 5v.01'],
};

const ICON_ATTRS: readonly [string, string][] = [
  ['viewBox', '0 0 16 16'],
  ['width', '14'],
  ['height', '14'],
  ['fill', 'none'],
  ['stroke', 'currentColor'],
  ['stroke-width', '1.5'],
  ['stroke-linecap', 'round'],
  ['stroke-linejoin', 'round'],
  ['aria-hidden', 'true'],
  ['focusable', 'false'],
];

/** Small inline SVG icon, stroked with currentColor. */
export function icon(name: IconName): SVGSVGElement {
  const svg = document.createElementNS(SVG_NS, 'svg');
  for (const [key, value] of ICON_ATTRS) svg.setAttribute(key, value);
  for (const d of ICON_PATHS[name]) {
    const path = document.createElementNS(SVG_NS, 'path');
    path.setAttribute('d', d);
    svg.append(path);
  }
  return svg;
}
