// Sidebar customisation panel: setting rows with switches, reorderable workspace and web app lists.
// Text reaches the DOM only through textContent and attributes. Styles live in styles/rail.css
// under `.panel-customize`; controls are the shared .bw-* components.
import { sendComposerOpen, sendPanel, type ComposerOpenFields, type Inbound, type SnapshotMsg } from '../protocol';
import { icon, type IconName } from '../icons';
import { dropSlot } from '../rail-logic';
import { setTip } from '../tooltip';
import { selectField, type SelectField } from './select';
import { panelHeader } from './kit';
import {
  customizeCommand, customizeDeleteDescription, moveBefore, parseCustomizeSnapshot, sentenceCase, stepDest,
  RAIL_DESCRIPTIONS, RAIL_KEYS, WORKSPACE_COLOURS, WORKSPACE_HEX, WORKSPACE_ICONS,
  type CustomizeApp, type CustomizeSnapshot, type CustomizeWorkspace,
} from './customize-logic';

type DeleteTarget = { kind: 'app' | 'workspace'; id: number; label: string } | null;
const labels: Record<typeof RAIL_KEYS[number], string> = { workspaces: 'Workspaces', apps: 'Web apps', bookmarks: 'Bookmarks', history: 'History' };
const DRAG_START_PX = 4;
const PRESETS = [
  ['Spotify', 'https://open.spotify.com/'],
  ['ChatGPT', 'https://chatgpt.com/'],
  ['WhatsApp', 'https://web.whatsapp.com/'],
  ['Notion', 'https://www.notion.so/'],
] as const;

function h<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string, text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

export function startCustomizePanel(root: HTMLElement): (message: Inbound) => void {
  let snapshot: CustomizeSnapshot | null = null;
  let deleting: DeleteTarget = null;
  let editingWorkspace: number | null = null;
  let uid = 0;
  const nextId = (prefix: string): string => `${prefix}-${++uid}`;

  const host = h('section', 'panel-customize bw-scroll');
  host.setAttribute('aria-label', 'Customize sidebar');
  // One header, shared with History and Bookmarks: title and close button. The sections scroll below it.
  const header = panelHeader('Customize', 'Close customize');
  const body = h('div', 'cust-body bw-scroll');

  const visibility = section('Sidebar', 'Choose what appears in the left rail.');
  const workspaces = section('Workspaces', 'Reorder with the handle, or focus it and press the Up and Down arrow keys.');
  const workspaceRows = h('div', 'cust-list');
  workspaceRows.setAttribute('role', 'list');
  // New workspace, Add web app and Edit web app open one composer, anchored below-left of the
  // control that asked. Nothing is created here: Rust makes the change when the composer confirms.
  const openComposer = (mode: Parameters<typeof sendComposerOpen>[0], anchor: HTMLElement, fields?: ComposerOpenFields): void => {
    sendComposerOpen(mode, anchor.getBoundingClientRect(), fields);
  };
  const createWorkspace = h('div', 'cust-form');
  const newWorkspace = button('New workspace', () => openComposer('workspace-new', newWorkspace));
  createWorkspace.append(newWorkspace);
  workspaces.root.append(workspaceRows, createWorkspace);

  const apps = section('Pinned web apps', 'Sites pinned to the rail open in the side panel.');
  const appRows = h('div', 'cust-list');
  appRows.setAttribute('role', 'list');
  const appForm = h('div', 'cust-form is-stack');
  const preset = selectField({
    options: PRESETS.map(([name, url]) => ({ value: url, label: name })),
    value: null,
    label: 'Choose a web app preset',
    placeholder: 'Choose a preset',
    momentary: true,
    // A preset opens the same composer, prefilled.
    onChange: url => {
      const chosen = PRESETS.find(([, address]) => address === url);
      if (chosen) openComposer('app-add', preset.root, { name: chosen[0], url: chosen[1] });
    },
  });
  const addApp = button('Add web app', () => openComposer('app-add', addApp));
  appForm.append(preset.root, addApp);
  apps.root.append(appRows, appForm);

  const data = section('Profile and data');
  const profile = button('Switch', () => sendAction('switch-profile'));
  const clearData = button('Clear', () => sendAction('clear-app-data'));
  data.root.append(
    settingRow('Switch profile', 'Open another browser profile', profile, profile),
    settingRow('Clear web app data', 'Sign out of every pinned web app and remove its saved data', clearData, clearData),
  );

  // Confirmation bar: pinned to the bottom of the panel so it is never scrolled out of view.
  const confirm = h('div', 'cust-confirm');
  confirm.setAttribute('role', 'status');
  confirm.hidden = true;

  body.append(visibility.root, workspaces.root, apps.root, data.root);
  host.append(header.root, body, confirm);
  root.replaceChildren(host);

  function section(heading: string, hint?: string): { root: HTMLElement; list: HTMLElement } {
    const wrap = h('section', 'cust-section');
    const id = nextId('cust-h');
    const head = h('h2', 'cust-heading', heading);
    head.id = id;
    wrap.setAttribute('aria-labelledby', id);
    wrap.append(head);
    if (hint) wrap.append(h('p', 'cust-hint', hint));
    return { root: wrap, list: wrap };
  }
  function input(label: string, placeholder: string): HTMLInputElement {
    const control = h('input', 'bw-input');
    control.maxLength = 128;
    control.required = true;
    control.placeholder = placeholder;
    control.setAttribute('aria-label', label);
    return control;
  }
  function button(text: string, action: () => void, variant = ''): HTMLButtonElement {
    const control = h('button', `bw-button ${variant}`.trim(), text);
    control.type = 'button';
    control.addEventListener('click', action);
    return control;
  }
  function submitButton(text: string): HTMLButtonElement {
    const control = h('button', 'bw-button is-primary', text);
    control.type = 'submit';
    return control;
  }
  function iconButton(name: IconName, label: string, action: () => void): HTMLButtonElement {
    const control = h('button', 'bw-icon-btn');
    control.type = 'button';
    setTip(control, label);
    control.setAttribute('aria-label', label);
    control.append(icon(name));
    control.addEventListener('click', action);
    return control;
  }
  /** A 48px settings row: title and description on the left, the control on the right. */
  function settingRow(name: string, description: string, control: HTMLElement, labelled: HTMLElement): HTMLElement {
    const row = h('div', 'bw-row is-setting cust-row');
    row.setAttribute('role', 'listitem');
    const text = h('div', 'bw-row-text');
    const titleId = nextId('cust-t');
    const label = h('span', 'bw-row-title', name);
    label.id = titleId;
    text.append(label, h('span', 'bw-row-meta', description));
    labelled.setAttribute('aria-labelledby', titleId);
    row.append(text, control);
    return row;
  }
  function sendAction(a: string, payload: Parameters<typeof customizeCommand>[1] = {}): void {
    const fields = customizeCommand(a, payload);
    if (fields) sendPanel('cust', fields);
  }
  function askDelete(target: DeleteTarget): void {
    deleting = target;
    confirm.replaceChildren();
    confirm.hidden = target === null;
    if (!target) return;
    const message = h('span', 'cust-confirm-text', customizeDeleteDescription(target.kind, target.label));
    const actions = h('div', 'cust-confirm-actions');
    const yes = button(target.kind === 'workspace' ? 'Delete workspace' : 'Remove web app', () => {
      sendAction(target.kind === 'workspace' ? 'workspace-delete' : 'app-remove', { id: target.id });
      askDelete(null);
    }, 'is-primary');
    const no = button('Cancel', () => askDelete(null));
    actions.append(no, yes);
    confirm.append(message, actions);
    yes.focus();
  }
  function iconField(workspace: CustomizeWorkspace): SelectField<typeof WORKSPACE_ICONS[number]> {
    return selectField({
      options: WORKSPACE_ICONS.map(name => ({ value: name, label: sentenceCase(name), lead: () => icon(`workspace-${name}` as IconName) })),
      value: workspace.icon,
      label: `Icon for ${workspace.name}`,
      placeholder: 'Icon',
      minListWidth: 160,
      onChange: value => sendAction('workspace-icon', { id: workspace.id, text: value }),
    });
  }
  function colourField(workspace: CustomizeWorkspace): SelectField<typeof WORKSPACE_COLOURS[number] | ''> {
    return selectField<typeof WORKSPACE_COLOURS[number] | ''>({
      options: [
        { value: '', label: 'No colour', lead: () => swatch(null) },
        ...WORKSPACE_COLOURS.map(name => ({ value: name, label: sentenceCase(name), lead: () => swatch(name) })),
      ],
      value: workspace.colour ?? '',
      label: `Colour for ${workspace.name}`,
      placeholder: 'Colour',
      minListWidth: 160,
      onChange: value => sendAction('workspace-colour', { id: workspace.id, n: value === '' ? -1 : WORKSPACE_COLOURS.indexOf(value) }),
    });
  }
  function swatch(colour: typeof WORKSPACE_COLOURS[number] | null): HTMLElement {
    const dot = h('span', colour ? 'cust-swatch' : 'cust-swatch is-none');
    if (colour) dot.style.setProperty('--ws-c', WORKSPACE_HEX[colour]);
    return dot;
  }

  /** Pointer drag on a row's handle, and Up / Down arrows on the focused handle. */
  function wireHandle(handle: HTMLButtonElement, row: HTMLElement, list: HTMLElement, ids: readonly number[], id: number, onMove: (before: number | undefined) => void, focusKey: string): void {
    const from = ids.indexOf(id);
    handle.dataset.fk = focusKey;
    handle.addEventListener('keydown', event => {
      if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
      event.preventDefault();
      const dest = stepDest(ids.length, from, event.key === 'ArrowUp' ? -1 : 1);
      if (dest !== null) onMove(moveBefore(ids, id, dest));
    });
    handle.addEventListener('pointerdown', down => {
      if (down.button !== 0) return;
      down.preventDefault();
      const rows = Array.from(list.querySelectorAll<HTMLElement>(':scope > .cust-item'));
      const centres = rows.map(item => { const r = item.getBoundingClientRect(); return r.top + r.height / 2; });
      let dest = from;
      let active = false;
      try { handle.setPointerCapture(down.pointerId); } catch { /* window listeners still end the drag */ }
      const mark = (): void => {
        rows.forEach(item => item.classList.remove('drop-before', 'drop-end'));
        if (dest === from) return;
        const others = rows.filter(item => item !== row);
        const target = others[dest];
        if (target) target.classList.add('drop-before'); else others[others.length - 1]?.classList.add('drop-end');
      };
      const finish = (commit: boolean): void => {
        handle.removeEventListener('pointermove', move);
        handle.removeEventListener('pointerup', up);
        handle.removeEventListener('pointercancel', cancel);
        window.removeEventListener('keydown', esc, true);
        try { handle.releasePointerCapture(down.pointerId); } catch { /* already released */ }
        row.classList.remove('is-dragging');
        row.style.removeProperty('transform');
        rows.forEach(item => item.classList.remove('drop-before', 'drop-end'));
        if (active && commit && dest !== from) onMove(moveBefore(ids, id, dest));
      };
      const move = (ev: PointerEvent): void => {
        const dy = ev.clientY - down.clientY;
        if (!active) {
          if (Math.abs(dy) < DRAG_START_PX) return;
          active = true;
          row.classList.add('is-dragging');
          window.addEventListener('keydown', esc, true);
        }
        row.style.setProperty('transform', `translateY(${dy}px)`);
        dest = dropSlot(centres, from, (centres[from] ?? 0) + dy);
        mark();
      };
      const up = (): void => finish(true);
      const cancel = (): void => finish(false);
      const esc = (ev: KeyboardEvent): void => {
        if (ev.key !== 'Escape') return;
        ev.preventDefault();
        ev.stopPropagation();
        finish(false);
      };
      handle.addEventListener('pointermove', move);
      handle.addEventListener('pointerup', up);
      handle.addEventListener('pointercancel', cancel);
    });
  }

  function render(): void {
    if (!snapshot) return;
    const focused = document.activeElement instanceof HTMLElement ? document.activeElement.dataset.fk : undefined;
    visibility.root.replaceChildren(visibility.root.querySelector('h2') as Node, visibility.root.querySelector('p') as Node);
    for (const key of RAIL_KEYS) {
      const toggle = h('button', 'bw-toggle');
      toggle.type = 'button';
      toggle.setAttribute('role', 'switch');
      toggle.setAttribute('aria-checked', String(snapshot.visible[key]));
      toggle.dataset.fk = `vis:${key}`;
      toggle.addEventListener('click', () => {
        const on = toggle.getAttribute('aria-checked') !== 'true';
        toggle.setAttribute('aria-checked', String(on));
        sendAction('visibility', { text: key, on });
      });
      visibility.root.append(settingRow(labels[key], RAIL_DESCRIPTIONS[key], toggle, toggle));
    }
    workspaceRows.replaceChildren();
    const workspaceIds = snapshot.workspaces.map(item => item.id);
    for (const workspace of snapshot.workspaces) workspaceRows.append(renderWorkspace(workspace, workspaceIds));
    appRows.replaceChildren();
    const appIds = snapshot.apps.map(item => item.id);
    for (const app of snapshot.apps) appRows.append(renderApp(app, appIds));
    if (snapshot.apps.length === 0) appRows.append(h('p', 'cust-empty', 'No web apps pinned yet. Add one below.'));
    clearData.disabled = snapshot.apps.length === 0;
    if (deleting) askDelete(deleting);
    if (focused) host.querySelector<HTMLElement>(`[data-fk="${focused}"]`)?.focus();
  }

  function itemRow(name: string, meta: string, lead: HTMLElement, handle: HTMLButtonElement, actions: HTMLElement[]): { item: HTMLElement; row: HTMLElement } {
    const item = h('div', 'cust-item');
    item.setAttribute('role', 'listitem');
    const row = h('div', 'bw-row is-setting cust-row');
    const text = h('div', 'bw-row-text');
    text.append(h('span', 'bw-row-title', name), h('span', 'bw-row-meta', meta));
    const end = h('div', 'cust-actions');
    end.append(...actions);
    row.append(handle, lead, text, end);
    item.append(row);
    return { item, row };
  }
  function handleButton(label: string): HTMLButtonElement {
    const handle = h('button', 'bw-icon-btn cust-handle');
    handle.type = 'button';
    handle.setAttribute('aria-label', `Reorder ${label}`);
    setTip(handle, 'Drag to reorder, or use the Up and Down arrow keys');
    handle.append(icon('grip'));
    return handle;
  }

  function renderWorkspace(workspace: CustomizeWorkspace, ids: readonly number[]): HTMLElement {
    const lead = h('span', 'cust-lead');
    lead.append(icon(`workspace-${workspace.icon}` as IconName));
    if (workspace.colour) lead.style.setProperty('--ws-c', WORKSPACE_HEX[workspace.colour]);
    const meta = `${workspace.tabs} ${workspace.tabs === 1 ? 'tab' : 'tabs'}${workspace.active ? ' · Current workspace' : ''}`;
    const handle = handleButton(workspace.name);
    const editing = editingWorkspace === workspace.id;
    const edit = iconButton('edit', editing ? `Close editor for ${workspace.name}` : `Edit ${workspace.name}`, () => { editingWorkspace = editing ? null : workspace.id; render(); });
    edit.setAttribute('aria-expanded', String(editing));
    edit.dataset.fk = `ws-edit:${workspace.id}`;
    const remove = iconButton('trash', `Delete ${workspace.name}`, () => askDelete({ kind: 'workspace', id: workspace.id, label: workspace.name }));
    const { item, row } = itemRow(workspace.name, meta, lead, handle, [edit, remove]);
    wireHandle(handle, row, workspaceRows, ids, workspace.id, before => sendAction('workspace-move', { id: workspace.id, ...(before !== undefined ? { before } : {}) }), `ws-handle:${workspace.id}`);
    if (editing) {
      const editor = h('form', 'cust-editor');
      const rename = input(`Rename workspace ${workspace.name}`, 'Workspace name');
      rename.value = workspace.name;
      const iconSelect = iconField(workspace);
      const colour = colourField(workspace);
      editor.addEventListener('submit', event => {
        event.preventDefault();
        if (rename.value.trim() !== '') sendAction('workspace-rename', { id: workspace.id, text: rename.value.trim() });
      });
      const pair = h('div', 'cust-pair');
      pair.append(iconSelect.root, colour.root);
      editor.append(rename, pair, submitButton('Save name'));
      item.append(editor);
    }
    return item;
  }

  function renderApp(app: CustomizeApp, ids: readonly number[]): HTMLElement {
    const lead = h('span', 'cust-lead');
    lead.append(icon('grid'));
    const handle = handleButton(app.name);
    const edit = iconButton('edit', `Edit ${app.name}`, () => openComposer('app-edit', edit, { id: app.id }));
    edit.dataset.fk = `app-edit:${app.id}`;
    const remove = iconButton('trash', `Remove ${app.name}`, () => askDelete({ kind: 'app', id: app.id, label: app.name }));
    const { item, row } = itemRow(app.name, app.url, lead, handle, [edit, remove]);
    wireHandle(handle, row, appRows, ids, app.id, before => sendAction('app-move', { id: app.id, ...(before !== undefined ? { before } : {}) }), `app-handle:${app.id}`);
    return item;
  }

  const receive = (message: SnapshotMsg): void => {
    if (message.p !== 'customize') return;
    snapshot = parseCustomizeSnapshot(message.d);
    if (snapshot) render();
  };
  return (message: Inbound) => { if (message.t === 'snap') receive(message); };
}
