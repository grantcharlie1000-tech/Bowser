// DOM helpers. Everything is built with createElement/createElementNS; no HTML strings are parsed.

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

// Icons live in icons.ts (one generated family); re-exported here for existing imports.
export { icon, type IconName } from './icons';
