export const RAIL_KEYS = ['workspaces', 'apps', 'bookmarks', 'history'] as const;
export const WORKSPACE_ICONS = ['briefcase', 'home', 'code', 'book', 'cart', 'game', 'music', 'heart', 'star', 'bolt', 'globe', 'leaf'] as const;
export const WORKSPACE_COLOURS = ['blue', 'violet', 'teal', 'amber', 'rose', 'red', 'green', 'slate'] as const;

export type CustomizeWorkspace = { id: number; name: string; icon: typeof WORKSPACE_ICONS[number]; colour: typeof WORKSPACE_COLOURS[number] | null; active: boolean; tabs: number };
export type CustomizeApp = { id: number; name: string; url: string; position: number };
export type CustomizeSnapshot = { visible: Record<typeof RAIL_KEYS[number], boolean>; workspaces: CustomizeWorkspace[]; apps: CustomizeApp[] };

const record = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const exact = (value: Record<string, unknown>, keys: readonly string[]): boolean => Object.keys(value).every(key => keys.includes(key)) && keys.every(key => key in value);
const id = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) > 0 && (value as number) <= 0xffffffff;
// Workspace zero is the stable id of the default workspace.
const workspaceId = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) >= 0 && (value as number) <= 0xffffffff;
const name = (value: unknown): value is string => typeof value === 'string' && value.length > 0 && value.trim() === value && value.length <= 128;
const webUrl = (value: unknown): value is string => typeof value === 'string' && value.length <= 2048 && /^https?:\/\//i.test(value) && !/[\u0000-\u0020\u007f]/.test(value);

export function parseCustomizeSnapshot(value: unknown): CustomizeSnapshot | null {
  if (!record(value) || !exact(value, ['visible', 'workspaces', 'apps']) || !record(value.visible) || !exact(value.visible, RAIL_KEYS) || !Array.isArray(value.workspaces) || !Array.isArray(value.apps)) return null;
  const visible = value.visible as Record<string, unknown>;
  if (!RAIL_KEYS.every(key => typeof visible[key] === 'boolean')) return null;
  if (value.workspaces.length > 128 || value.apps.length > 128) return null;
  const workspaces: CustomizeWorkspace[] = [];
  for (const raw of value.workspaces) {
    if (!record(raw) || !exact(raw, ['id', 'name', 'icon', 'colour', 'active', 'tabs']) || !workspaceId(raw.id) || !name(raw.name) || !WORKSPACE_ICONS.includes(raw.icon as never) || !(raw.colour === null || WORKSPACE_COLOURS.includes(raw.colour as never)) || typeof raw.active !== 'boolean' || !Number.isSafeInteger(raw.tabs) || (raw.tabs as number) < 0) return null;
    workspaces.push(raw as CustomizeWorkspace);
  }
  const apps: CustomizeApp[] = [];
  for (const raw of value.apps) {
    if (!record(raw) || !exact(raw, ['id', 'name', 'url', 'position']) || !id(raw.id) || !name(raw.name) || !webUrl(raw.url) || !Number.isSafeInteger(raw.position) || (raw.position as number) < 0) return null;
    apps.push(raw as CustomizeApp);
  }
  if (new Set(workspaces.map(item => item.id)).size !== workspaces.length || new Set(apps.map(item => item.id)).size !== apps.length) return null;
  return { visible: visible as CustomizeSnapshot['visible'], workspaces, apps };
}

type Payload = { id?: number; parent?: number; before?: number; n?: number; on?: boolean; text?: string; url?: string };
const ACTIONS = new Set(['visibility', 'app-add', 'app-update', 'app-remove', 'app-move', 'workspace-create', 'workspace-rename', 'workspace-delete', 'workspace-move', 'workspace-icon', 'workspace-colour', 'switch-profile', 'clear-app-data']);

/** Builds only commands expressible through the current flat PanelFields bridge. */
export function customizeCommand(a: string, payload: Payload): ({ a: string } & Payload) | null {
  if (!ACTIONS.has(a)) return null;
  const fields = Object.entries(payload);
  if (fields.some(([key]) => !['id', 'parent', 'before', 'n', 'on', 'text', 'url'].includes(key))) return null;
  if (fields.length > 3) return null;
  if (payload.id !== undefined && !id(payload.id) && !workspaceId(payload.id)) return null;
  if (payload.parent !== undefined && !id(payload.parent)) return null;
  if (payload.before !== undefined && !id(payload.before) && !workspaceId(payload.before)) return null;
  if (payload.n !== undefined && (!Number.isSafeInteger(payload.n) || payload.n < -1 || payload.n > 11)) return null;
  if (payload.on !== undefined && typeof payload.on !== 'boolean') return null;
  if (payload.text !== undefined && (typeof payload.text !== 'string' || payload.text.length > 2048)) return null;
  if (payload.url !== undefined && !webUrl(payload.url)) return null;
  if (a === 'visibility' && (!RAIL_KEYS.includes(payload.text as never) || typeof payload.on !== 'boolean')) return null;
  if (a === 'app-add' && (!name(payload.text) || !webUrl(payload.url))) return null;
  if (a === 'app-update' && (!id(payload.id) || !name(payload.text) || !webUrl(payload.url))) return null;
  if (a === 'app-remove' && !id(payload.id)) return null;
  if (a === 'workspace-delete' && !workspaceId(payload.id)) return null;
  if (a === 'app-move' && (!id(payload.id) || (payload.before !== undefined && !id(payload.before)))) return null;
  if (a === 'workspace-move' && (!workspaceId(payload.id) || (payload.before !== undefined && !workspaceId(payload.before)))) return null;
  if (['workspace-rename', 'workspace-icon', 'workspace-colour'].includes(a) && !workspaceId(payload.id)) return null;
  if (a === 'workspace-rename' && !name(payload.text)) return null;
  if (a === 'workspace-create' && !name(payload.text)) return null;
  if (a === 'workspace-icon' && !WORKSPACE_ICONS.includes(payload.text as never)) return null;
  if (a === 'workspace-colour' && payload.n !== -1 && (!Number.isInteger(payload.n) || (payload.n as number) < 0 || (payload.n as number) > 7)) return null;
  if (['switch-profile', 'clear-app-data'].includes(a) && fields.length !== 0) return null;
  return { a, ...payload };
}

export function customizeDeleteDescription(kind: 'workspace' | 'app', label: string): string {
  return kind === 'workspace'
    ? `Delete workspace “${label}”? Its tabs move to another workspace.`
    : `Remove “${label}” from the rail? Its saved data stays in its own window.`;
}
