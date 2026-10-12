// DOM helpers. Everything is built with createElement/createElementNS; no HTML strings are parsed.

const SVG_NS = 'http://www.w3.org/2000/svg';

export function el<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  return node;
}

/** Makes `parent` contain exactly `nodes`, in order, moving only what is out of place. */
export function place(parent: HTMLElement, nodes: readonly HTMLElement[]): void {
  let ref: ChildNode | null = parent.firstChild;
  for (const node of nodes) {
    if (node === ref) {
      ref = ref.nextSibling;
      continue;
    }
    parent.insertBefore(node, ref);
  }
  while (ref) {
    const next: ChildNode | null = ref.nextSibling;
    ref.remove();
    ref = next;
  }
}

export type IconName ='back' | 'forward' | 'reload' | 'stop' | 'menu' | 'split' | 'plus' | 'lock' | 'unlock' | 'warning' | 'info' | 'close' | 'person' | 'grid' | 'key' | 'shield' | 'gear' | 'check' | 'mic' | 'camera' | 'location' | 'bell' | 'cookie' | 'reset';

const ICON_PATHS: Readonly<Record<IconName, readonly string[]>> = {
  close: ['M4.5 4.5l7 7', 'M11.5 4.5l-7 7'],
  back: ['M9.5 2.75 4.25 8l5.25 5.25'],
  forward: ['M6.5 2.75 11.75 8 6.5 13.25'],
  reload: ['M12.8 8a4.8 4.8 0 1 1-1.4-3.4', 'M12.8 2.8v2.6h-2.6'],
  stop: ['M4.5 4.5l7 7', 'M11.5 4.5l-7 7'],
  menu: ['M3.5 4.5h9', 'M3.5 8h9', 'M3.5 11.5h9'],
  split: ['M2.75 3.25h4.5v9.5h-4.5z', 'M8.75 3.25h4.5v9.5h-4.5z'],
  plus: ['M8 3.5v9', 'M3.5 8h9'],
  lock: ['M3.5 7.5h9v6h-9z', 'M5.5 7.5V5.25a2.5 2.5 0 0 1 5 0V7.5'],
  warning: ['M8 2.5 14 13H2z', 'M8 6.5v3', 'M8 11.25v.01'],
  info: ['M8 1.75a6.25 6.25 0 1 0 0 12.5 6.25 6.25 0 1 0 0-12.5z', 'M8 7.25v4', 'M8 5v.01'],
  unlock: ['M3.5 7.5h9v6h-9z', 'M5.5 7.5V5.25a2.5 2.5 0 0 1 4.6-1.35'],
  person: ['M8 8a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5z', 'M3 13c.6-2.1 2.3-3.2 5-3.2s4.4 1.1 5 3.2'],
  grid: ['M2.5 2.5h4v4h-4z', 'M9.5 2.5h4v4h-4z', 'M2.5 9.5h4v4h-4z', 'M9.5 9.5h4v4h-4z'],
  key: ['M9 6a3 3 0 1 0-2.8 3.9L8 11.7h2v-2h2v-2h1.5V6H9z'],
  shield: ['M8 1.8 13 3.7v3.8c0 3-2.1 5.3-5 6.7-2.9-1.4-5-3.7-5-6.7V3.7z', 'm5.5 7.8 1.6 1.6 3.4-3.5'],
  gear: ['M8 2.2v1.2', 'M8 12.6v1.2', 'M2.2 8h1.2', 'M12.6 8h1.2', 'm3.9 3.9.9.9', 'm11.2 11.2.9.9', 'm12.1 3.9-.9.9', 'm4.8 11.2-.9.9', 'M10.5 8a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0z'],
  check: ['m3.5 8.2 3 3 6-6.5'],
  mic: ['M8 10a2 2 0 0 0 2-2V4a2 2 0 0 0-4 0v4a2 2 0 0 0 2 2z', 'M4.5 7.5a3.5 3.5 0 0 0 7 0', 'M8 11v2.5', 'M6 13.5h4'],
  camera: ['M2.5 5h3l1-1.5h3L10.5 5h3v7.5h-11z', 'M10.5 8.7a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0z'],
  location: ['M8 14s4.5-4.6 4.5-8A4.5 4.5 0 0 0 3.5 6c0 3.4 4.5 8 4.5 8z', 'M9.5 6a1.5 1.5 0 1 1-3 0 1.5 1.5 0 0 1 3 0z'],
  bell: ['M3.5 11.5h9l-1.2-1.7V6.5a3.3 3.3 0 0 0-6.6 0v3.3z', 'M6.5 13a1.7 1.7 0 0 0 3 0'],
  cookie: ['M13.4 8.5a5.5 5.5 0 1 1-5.9-5.9 2.6 2.6 0 0 0 5.9 5.9z', 'M5.2 7h.01', 'M8.1 10h.01', 'M5.8 11h.01'],
  reset: ['M3 6V2.8', 'M3 6h3.2', 'M3.5 5.5A5.5 5.5 0 1 1 3 9'],
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
