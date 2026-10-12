import { sendPanel, type Inbound, type SnapshotMsg } from '../protocol';
import {
  customizeCommand, customizeDeleteDescription, parseCustomizeSnapshot,
  RAIL_KEYS, WORKSPACE_COLOURS, WORKSPACE_ICONS,
  type CustomizeApp, type CustomizeSnapshot, type CustomizeWorkspace,
} from './customize-logic';

type DeleteTarget = { kind: 'app' | 'workspace'; id: number; label: string } | null;
const labels: Record<typeof RAIL_KEYS[number], string> = { workspaces: 'Workspaces', apps: 'Web apps', bookmarks: 'Bookmarks', history: 'History' };

export function startCustomizePanel(root: HTMLElement): (message: Inbound) => void {
  let snapshot: CustomizeSnapshot | null = null;
  let deleting: DeleteTarget = null;
  let editingApp: number | null = null;
  const host = document.createElement('section');
  host.className = 'customize-panel';
  host.setAttribute('aria-label', 'Customize sidebar');
  const title = document.createElement('h1');
  title.textContent = 'Customize';
  const visibility = document.createElement('section');
  const visibilityTitle = document.createElement('h2');
  visibilityTitle.textContent = 'Sidebar';
  visibility.append(visibilityTitle);
  const workspaceSection = document.createElement('section');
  const workspaceTitle = document.createElement('h2');
  workspaceTitle.textContent = 'Workspaces';
  const workspaceRows = document.createElement('div');
  const createWorkspace = document.createElement('form');
  const workspaceName = document.createElement('input');
  workspaceName.maxLength = 128;
  workspaceName.required = true;
  workspaceName.placeholder = 'Workspace name';
  workspaceName.setAttribute('aria-label', 'New workspace name');
  const createButton = button('Create workspace', () => {
    const fields = customizeCommand('workspace-create', { text: workspaceName.value.trim() });
    if (fields) sendPanel('cust', fields);
  });
  createWorkspace.append(workspaceName, createButton);
  createWorkspace.addEventListener('submit', event => { event.preventDefault(); createButton.click(); });
  workspaceSection.append(workspaceTitle, workspaceRows, createWorkspace);
  const appSection = document.createElement('section');
  const appTitle = document.createElement('h2');
  appTitle.textContent = 'Pinned web apps';
  const appRows = document.createElement('div');
  const appForm = document.createElement('form');
  const appName = document.createElement('input');
  appName.maxLength = 128;
  appName.required = true;
  appName.placeholder = 'Name';
  appName.setAttribute('aria-label', 'Web app name');
  const appUrl = document.createElement('input');
  appUrl.type = 'url';
  appUrl.required = true;
  appUrl.placeholder = 'https://example.com/';
  appUrl.setAttribute('aria-label', 'Web app address');
  const preset = document.createElement('select');
  preset.setAttribute('aria-label', 'Choose a web app preset');
  const presetPrompt = document.createElement('option');
  presetPrompt.value = '';
  presetPrompt.textContent = 'Choose a preset';
  preset.append(presetPrompt);
  for (const [name, url] of ([
    ['Spotify', 'https://open.spotify.com/'],
    ['ChatGPT', 'https://chatgpt.com/'],
    ['WhatsApp', 'https://web.whatsapp.com/'],
    ['Notion', 'https://www.notion.so/'],
  ] as const)) {
    const option = document.createElement('option');
    option.value = url;
    option.textContent = name;
    preset.append(option);
  }
  preset.addEventListener('change', () => {
    const option = preset.selectedOptions[0];
    if (option && option.value) { appName.value = option.textContent ?? ''; appUrl.value = option.value; }
  });
  const addApp = button('Add web app', () => {
    const fields = customizeCommand('app-add', { text: appName.value.trim(), url: appUrl.value.trim() });
    if (fields) { sendPanel('cust', fields); appName.value = ''; appUrl.value = ''; }
  });
  appForm.append(preset, appName, appUrl, addApp);
  appForm.addEventListener('submit', event => { event.preventDefault(); addApp.click(); });
  appSection.append(appTitle, appRows, appForm);
  const confirm = document.createElement('p');
  confirm.setAttribute('role', 'status');
  const actions = document.createElement('section');
  const profile = button('Switch profile', () => sendAction('switch-profile'));
  const clearData = button('Clear web app data', () => sendAction('clear-app-data'));
  actions.append(profile, clearData);
  host.append(title, visibility, workspaceSection, appSection, confirm, actions);
  root.replaceChildren(host);

  function button(text: string, action: () => void): HTMLButtonElement {
    const control = document.createElement('button');
    control.type = 'button';
    control.textContent = text;
    control.addEventListener('click', action);
    return control;
  }
  function sendAction(a: string, payload: Parameters<typeof customizeCommand>[1] = {}): void {
    const fields = customizeCommand(a, payload);
    if (fields) sendPanel('cust', fields);
  }
  function askDelete(target: DeleteTarget): void {
    deleting = target;
    confirm.replaceChildren();
    if (!target) return;
    const message = document.createElement('span');
    message.textContent = customizeDeleteDescription(target.kind, target.label);
    const yes = button(target.kind === 'workspace' ? 'Delete workspace' : 'Remove web app', () => {
      sendAction(target.kind === 'workspace' ? 'workspace-delete' : 'app-remove', { id: target.id });
      deleting = null;
      renderConfirm();
    });
    const no = button('Cancel', () => { deleting = null; renderConfirm(); });
    confirm.append(message, yes, no);
  }
  function renderConfirm(): void {
    if (!deleting) confirm.replaceChildren();
    else askDelete(deleting);
  }
  function select<T extends string>(options: readonly T[], value: T | null, label: string, changed: (value: T | null) => void): HTMLSelectElement {
    const control = document.createElement('select');
    control.setAttribute('aria-label', label);
    const none = document.createElement('option');
    none.value = '';
    none.textContent = 'None';
    control.append(none);
    for (const option of options) {
      const item = document.createElement('option');
      item.value = option;
      item.textContent = option.charAt(0).toUpperCase() + option.slice(1);
      control.append(item);
    }
    control.value = value ?? '';
    control.addEventListener('change', () => changed(control.value === '' ? null : control.value as T));
    return control;
  }
  function render(): void {
    if (!snapshot) return;
    visibility.replaceChildren(visibilityTitle);
    for (const key of RAIL_KEYS) {
      const label = document.createElement('label');
      const toggle = document.createElement('input');
      toggle.type = 'checkbox';
      toggle.checked = snapshot.visible[key];
      toggle.addEventListener('change', () => sendAction('visibility', { text: key, on: toggle.checked }));
      label.append(toggle, document.createTextNode(labels[key]));
      visibility.append(label);
    }
    workspaceRows.replaceChildren();
    for (const workspace of snapshot.workspaces) renderWorkspace(workspace);
    appRows.replaceChildren();
    for (const app of snapshot.apps) renderApp(app, snapshot.apps);
    clearData.disabled = snapshot.apps.length === 0;
    renderConfirm();
  }
  function renderWorkspace(workspace: CustomizeWorkspace): void {
    const row = document.createElement('article');
    const heading = document.createElement('h3');
    heading.textContent = `${workspace.name}${workspace.active ? ' (current)' : ''}`;
    const details = document.createElement('p');
    details.textContent = `${workspace.tabs} ${workspace.tabs === 1 ? 'tab' : 'tabs'} · ${workspace.icon} · ${workspace.colour ?? 'no colour'}`;
    const rename = document.createElement('input');
    rename.value = workspace.name;
    rename.maxLength = 128;
    rename.setAttribute('aria-label', `Rename workspace ${workspace.name}`);
    const save = button('Save name', () => sendAction('workspace-rename', { id: workspace.id, text: rename.value.trim() }));
    const icon = select(WORKSPACE_ICONS, workspace.icon, `Icon for ${workspace.name}`, value => { if (value) sendAction('workspace-icon', { id: workspace.id, text: value }); });
    const colour = select(WORKSPACE_COLOURS, workspace.colour, `Colour for ${workspace.name}`, value => sendAction('workspace-colour', { id: workspace.id, n: value === null ? -1 : WORKSPACE_COLOURS.indexOf(value) }));
    const index = snapshot?.workspaces.findIndex(item => item.id === workspace.id) ?? -1;
    const up = button('Move up', () => moveWorkspace(workspace, index - 1));
    const down = button('Move down', () => moveWorkspace(workspace, index + 1));
    up.disabled = index <= 0;
    down.disabled = index < 0 || index >= (snapshot?.workspaces.length ?? 0) - 1;
    const remove = button('Delete', () => askDelete({ kind: 'workspace', id: workspace.id, label: workspace.name }));
    row.append(heading, details, rename, save, icon, colour, up, down, remove);
    workspaceRows.append(row);
  }
  function moveWorkspace(workspace: CustomizeWorkspace, target: number): void {
    const list = snapshot?.workspaces ?? [];
    const from = list.findIndex(item => item.id === workspace.id);
    const before = list[target > from ? target + 1 : target]?.id;
    sendAction('workspace-move', { id: workspace.id, ...(before ? { before } : {}) });
  }
  function renderApp(app: CustomizeApp, apps: readonly CustomizeApp[]): void {
    const row = document.createElement('article');
    const heading = document.createElement('h3');
    heading.textContent = app.name;
    const address = document.createElement('p');
    address.textContent = app.url;
    const index = apps.findIndex(item => item.id === app.id);
    const up = button('Move up', () => moveApp(app, index - 1, apps));
    const down = button('Move down', () => moveApp(app, index + 1, apps));
    up.disabled = index <= 0;
    down.disabled = index >= apps.length - 1;
    const edit = button(editingApp === app.id ? 'Cancel edit' : 'Edit', () => { editingApp = editingApp === app.id ? null : app.id; render(); });
    if (editingApp === app.id) {
      const name = document.createElement('input');
      name.value = app.name;
      name.maxLength = 128;
      name.setAttribute('aria-label', `Name for ${app.name}`);
      const url = document.createElement('input');
      url.type = 'url';
      url.value = app.url;
      url.setAttribute('aria-label', `Address for ${app.name}`);
      const save = button('Save web app', () => {
        const fields = customizeCommand('app-update', { id: app.id, text: name.value.trim(), url: url.value.trim() });
        if (fields) { sendPanel('cust', fields); editingApp = null; }
      });
      row.append(name, url, save);
    }
    const remove = button('Remove', () => askDelete({ kind: 'app', id: app.id, label: app.name }));
    row.append(heading, address, up, down, edit, remove);
    appRows.append(row);
  }
  function moveApp(app: CustomizeApp, target: number, apps: readonly CustomizeApp[]): void {
    const from = apps.findIndex(item => item.id === app.id);
    const before = apps[target > from ? target + 1 : target]?.id;
    sendAction('app-move', { id: app.id, ...(before ? { before } : {}) });
  }
  const receive = (message: SnapshotMsg): void => {
    if (message.p !== 'customize') return;
    snapshot = parseCustomizeSnapshot(message.d);
    if (snapshot) render();
  };
  return (message: Inbound) => { if (message.t === 'snap') receive(message); };
}
