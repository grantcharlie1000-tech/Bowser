export type MenuEntry =
  | { separator: true; label?: string }
  | { section: string }
  | { id: number; label: string; enabled: boolean; checked: boolean; shortcut?: string; icon?: string; submenu?: MenuEntry[] };

export function stepMenu(entries: readonly MenuEntry[], from: number, direction: -1 | 1): number {
  if (entries.length === 0) return -1;
  const start = from < 0 ? (direction === 1 ? -1 : 0) : from;
  for (let n = 1; n <= entries.length; n += 1) {
    const index = ((start + direction * n) % entries.length + entries.length) % entries.length;
    const entry = entries[index];
    if (entry && !('separator' in entry) && !('section' in entry) && entry.enabled) return index;
  }
  return -1;
}

export function edgeMenu(entries: readonly MenuEntry[], end: boolean): number {
  const indices = end ? [...entries.keys()].reverse() : entries.keys();
  for (const index of indices) {
    const entry = entries[index];
    if (entry && !('separator' in entry) && !('section' in entry) && entry.enabled) return index;
  }
  return -1;
}

export function validMenuPayload(value: unknown): value is { t: 'menu'; tok: number; kind: string; entries: MenuEntry[] } {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const msg = value as Record<string, unknown>;
  if (Object.keys(msg).some(key => !['v', 't', 'tok', 'kind', 'entries'].includes(key)) || msg.v !== 1 || msg.t !== 'menu' ||
      !Number.isSafeInteger(msg.tok) || (msg.tok as number) < 0 || (msg.tok as number) > 0xffffffff ||
      typeof msg.kind !== 'string' || msg.kind.length < 1 || msg.kind.length > 32 || !Array.isArray(msg.entries) || msg.entries.length > 128) return false;
  let count = 0;
  const validEntries = (entries: unknown[], depth: number): boolean => {
    if (depth > 8) return false;
    for (const raw of entries) {
      count += 1;
      if (count > 256 || !raw || typeof raw !== 'object' || Array.isArray(raw)) return false;
      const item = raw as Record<string, unknown>;
      if (item.separator === true) {
        if (Object.keys(item).some(key => !['separator', 'label'].includes(key))) return false;
        if (item.label !== undefined && (typeof item.label !== 'string' || item.label.length > 128)) return false;
        continue;
      }
      if (typeof item.section === 'string') {
        if (Object.keys(item).length !== 1 || item.section.length > 128) return false;
        continue;
      }
      if (Object.keys(item).some(key => !['id', 'label', 'enabled', 'checked', 'shortcut', 'icon', 'submenu'].includes(key)) ||
          !Number.isSafeInteger(item.id) || (item.id as number) < 0 || (item.id as number) > 0xffffffff || typeof item.label !== 'string' || item.label.length > 128 ||
          typeof item.enabled !== 'boolean' || typeof item.checked !== 'boolean') return false;
      if (item.shortcut !== undefined && (typeof item.shortcut !== 'string' || item.shortcut.length > 48)) return false;
      if (item.icon !== undefined && (typeof item.icon !== 'string' || !['plus', 'person', 'grid', 'key', 'shield', 'gear', 'close', 'check', 'lock', 'unlock', 'warning', 'info', 'mic', 'camera', 'location', 'bell', 'cookie', 'reset'].includes(item.icon))) return false;
      if (item.submenu !== undefined && (!Array.isArray(item.submenu) || !validEntries(item.submenu, depth + 1))) return false;
    }
    return true;
  };
  return validEntries(msg.entries, 0);
}
