// The settings page's Extensions section (docs/extensions.md "Managing extensions"). Rust sends
// `{t:"snap",p:"extensions"}` to the settings page; this module draws it into the section and sends
// the section's commands through the page's `set` family. Words and checks live in
// extensions-logic.ts. Text from Rust reaches the DOM through textContent only.
//
// Nothing here grants anything: turning an extension on reuses Bowser's copy from install, removing
// asks first in the row, and installing goes through Bowser's own folder or file picker and the native
// confirmation that lists what the manifest declares.
import { el, icon } from './dom';
import { textButton } from './panels/kit';
import { setTip } from './tooltip';
import {
  EMPTY_DETAIL, EXTENSIONS_NOTE, INSTALL_COMMAND, INSTALL_CRX_COMMAND, INSTALL_CRX_LABEL, INSTALL_CRX_TIP, ackToast, declarationGroups, parseExtensionsSnapshot, popupCommand, reachText, removeCommand, removeQuestion,
  setEnabledCommand, statusText, switchLabel, type ExtensionItem, type ExtensionsSnapshot,
} from './extensions-logic';
import { storeUrlCommand } from './store-logic';
import type { PanelFields, SnapshotMsg } from './protocol';

export interface ExtensionsSectionOptions {
  readonly idPrefix?: string;
  /** Sends one `set` command; false when the bridge refused it. */
  readonly send: (fields: PanelFields) => boolean;
  /** The settings page's toast. */
  readonly toast: (text: string, ok: boolean) => void;
}

export interface ExtensionsSection {
  /** The block the settings page puts in its section (and that search treats as one row). */
  readonly root: HTMLElement;
  /** Applies an `extensions` snapshot. */
  receive(message: SnapshotMsg): void;
}

export function mountExtensionsSection(options: ExtensionsSectionOptions): ExtensionsSection {
  const prefix = options.idPrefix ?? 'sp';
  let snapshot: ExtensionsSnapshot | null = null;
  let invalid = false;
  /** Switches clicked and not yet answered: they stay put until the next snapshot. */
  const pending = new Set<string>();
  /** Rows whose declarations are open, and the row asking to confirm a removal. */
  const expanded = new Set<string>();
  let confirming: string | null = null;
  /** Install asked; the buttons wait for Bowser's answer (the picker is modal to the window). */
  let installing = false;

  const root = el('div', 'sp-ext');
  root.dataset.row = 'extensions';
  const group = el('div', 'sp-group');
  const head = el('div', 'sp-ext-head');
  const heading = el('h3', 'sp-group-h');
  heading.id = `${prefix}-ext-h`;
  heading.textContent = 'Installed';
  const install = textButton('Install unpacked…', () => sendInstall(), { icon: 'folder' });
  install.classList.add('sp-ext-install');
  install.setAttribute('aria-describedby', `${prefix}-ext-note`);
  const installCrx = crxButton();
  installCrx.classList.add('sp-ext-install-crx');
  installCrx.setAttribute('aria-describedby', `${prefix}-ext-note`);
  const installs = el('div', 'sp-ext-installs');
  installs.append(textButton('Open Chrome Web Store', () => options.send({ a: 'storeopen' }), { icon: 'puzzle', primary: true }), install, installCrx);
  head.append(heading, installs);
  const box = el('div', 'sp-card sp-ext-card');
  box.setAttribute('role', 'group');
  box.setAttribute('aria-labelledby', heading.id);
  box.setAttribute('aria-busy', 'true');
  group.append(head, box);
  const note = el('p', 'sp-flag sp-ext-note');
  note.id = `${prefix}-ext-note`;
  const noteText = el('span');
  noteText.textContent = EXTENSIONS_NOTE;
  note.append(icon('info', { size: 12, fine: true }), noteText);
  const fromLink = el('details', 'sp-ext-link');
  const linkSummary = el('summary'); linkSummary.textContent = 'Install from a store link';
  const linkLabel = el('label'); linkLabel.textContent = 'Chrome Web Store listing URL'; linkLabel.htmlFor = `${prefix}-ext-url`;
  const linkInput = el('input', 'bw-input'); linkInput.id = `${prefix}-ext-url`; linkInput.type = 'url'; linkInput.maxLength = 2048; linkInput.placeholder = 'https://chromewebstore.google.com/detail/…';
  const linkError = el('p', 'sp-desc'); linkError.hidden = true; linkError.setAttribute('role', 'alert');
  const linkInstall = textButton('Review extension', () => {
    const fields = storeUrlCommand(linkInput.value);
    if (!fields) { linkError.hidden = false; linkError.textContent = 'Enter a full HTTPS Chrome Web Store extension listing URL.'; linkInput.focus(); return; }
    linkError.hidden = true; sendInstall(fields);
  });
  fromLink.append(linkSummary, linkLabel, linkInput, linkInstall, linkError);
  root.append(group, note, fromLink);

  const sendInstall = (command: PanelFields = INSTALL_COMMAND): void => {
    if (installing) return;
    if (options.send(command)) { installing = true; render(); }
  };
  /** A file on disk, not a store: the tip says so. Same weight as the folder button next to it. */
  function crxButton(): HTMLButtonElement {
    const button = textButton(INSTALL_CRX_LABEL, () => sendInstall(INSTALL_CRX_COMMAND), { icon: 'puzzle' });
    setTip(button, INSTALL_CRX_TIP);
    return button;
  }

  /** The message in the card when there is no list to show: loading, unreadable, invalid or empty. */
  const stateBlock = (kind: 'loading' | 'error' | 'empty', title: string, detail: string, action?: HTMLElement): HTMLElement => {
    const block = el('div', `sp-ext-state is-${kind}`);
    const mark = el('span', 'sp-ext-state-mark');
    if (kind === 'loading') { const spin = el('span', 'bw-spinner'); spin.setAttribute('aria-hidden', 'true'); mark.append(spin); }
    else mark.append(icon(kind === 'error' ? 'warning' : 'puzzle', { size: 20 }));
    const text = el('div', 'sp-text');
    const label = el('span', 'sp-label');
    label.textContent = title;
    text.append(label);
    if (detail !== '') { const desc = el('span', 'sp-desc'); desc.textContent = detail; text.append(desc); }
    block.append(mark, text);
    if (action) block.append(action);
    if (kind !== 'empty') block.setAttribute('role', kind === 'error' ? 'alert' : 'status');
    return block;
  };

  const declarations = (item: ExtensionItem): HTMLElement => {
    const details = el('div', 'sp-ext-details');
    details.id = `${prefix}-ext-d-${item.id}`;
    const groups = declarationGroups(item);
    if (groups.length === 0) {
      const none = el('p', 'sp-desc');
      none.textContent = item.readable ? 'It declares nothing.' : 'Its manifest could not be read.';
      details.append(none);
      return details;
    }
    const list = el('dl', 'sp-ext-decl');
    for (const entry of groups) {
      const term = el('dt');
      term.textContent = entry.label;
      const values = el('dd');
      for (const value of entry.items) {
        const chip = el('span', 'sp-ext-chip');
        chip.textContent = value;
        chip.dir = 'ltr';
        values.append(chip);
      }
      if (entry.more > 0) {
        const more = el('span', 'sp-ext-more');
        more.textContent = `and ${entry.more} more`;
        values.append(more);
      }
      list.append(term, values);
    }
    details.append(list);
    return details;
  };

  const itemRow = (item: ExtensionItem, busy: string): HTMLElement => {
    const removing = busy === item.id;
    const held = busy !== '';
    const row = el('div', `sp-ext-item${item.enabled ? '' : ' is-off'}${removing ? ' is-busy' : ''}`);
    row.dataset.id = item.id;
    const main = el('div', 'sp-ext-main');
    const tile = el('span', 'sp-ext-tile');
    tile.setAttribute('aria-hidden', 'true');
    tile.append(icon('puzzle'));
    const text = el('div', 'sp-text');
    const name = el('span', 'sp-label sp-ext-name');
    name.id = `${prefix}-ext-n-${item.id}`;
    name.textContent = item.name;
    const status = el('span', 'sp-desc sp-ext-status');
    status.textContent = statusText(item, removing);
    const reach = el('span', `sp-desc sp-ext-reach${item.all ? ' is-broad' : ''}`);
    reach.id = `${prefix}-ext-r-${item.id}`;
    if (item.all) reach.append(icon('warning', { size: 12, fine: true }));
    reach.append(document.createTextNode(reachText(item)));
    text.append(name, status, reach);
    if (item.failure !== '') {
      const failure = el('p', 'sp-flag is-later sp-ext-failure');
      const failureText = el('span');
      failureText.textContent = item.failure;
      failure.append(icon('warning', { size: 12, fine: true }), failureText);
      text.append(failure);
    }
    const open = expanded.has(item.id);
    const more = el('button', 'sp-ext-toggle-details');
    more.type = 'button';
    more.dataset.focus = `${item.id}:details`;
    more.setAttribute('aria-expanded', String(open));
    more.setAttribute('aria-controls', `${prefix}-ext-d-${item.id}`);
    more.append(document.createTextNode(open ? 'Hide what it declares' : 'What it declares'), icon(open ? 'chevron-up' : 'chevron-down', { size: 12, fine: true }));
    more.addEventListener('click', () => {
      if (expanded.has(item.id)) expanded.delete(item.id); else expanded.add(item.id);
      render();
    });
    text.append(more);

    const actions = el('div', 'sp-ctl sp-ext-actions');
    if (removing) {
      const spin = el('span', 'bw-spinner');
      spin.setAttribute('aria-hidden', 'true');
      actions.append(spin);
    } else {
      // A popup that failed to load this session is not running, so there is nothing to open.
      if (item.enabled && item.popup && item.failure === '') {
        const popup = textButton('Open popup', () => { const fields = popupCommand(item.id); if (fields) options.send(fields); }, { subtle: true, aria: `Open ${item.name} popup` });
        popup.dataset.focus = `${item.id}:popup`;
        popup.disabled = held;
        actions.append(popup);
      }
      for (const [action, label] of [['extoptions', 'Options'], ['extupdate', 'Check update']] as const) {
        const button = textButton(label, () => options.send({ a: action, text: item.id }), { subtle: true, aria: `${label} for ${item.name}` });
        button.dataset.focus = `${item.id}:${action}`;
        button.disabled = held;
        actions.append(button);
      }
      const remove = el('button', 'bw-icon-btn is-lg sp-ext-remove');
      remove.type = 'button';
      remove.dataset.focus = `${item.id}:remove`;
      remove.setAttribute('aria-label', `Remove ${item.name}`);
      setTip(remove, 'Remove');
      remove.append(icon('trash'));
      remove.disabled = held;
      remove.addEventListener('click', () => { confirming = item.id; render(`${item.id}:cancel`); });
      const toggle = el('button', 'bw-toggle sp-switch');
      toggle.type = 'button';
      toggle.dataset.focus = `${item.id}:toggle`;
      toggle.setAttribute('role', 'switch');
      toggle.setAttribute('aria-checked', String(item.enabled));
      toggle.setAttribute('aria-label', switchLabel(item));
      toggle.setAttribute('aria-describedby', reach.id);
      // Waiting for an answer, the switch keeps the keyboard (aria-disabled, not disabled) and
      // ignores clicks; while another extension is removed it is plainly disabled.
      toggle.disabled = held;
      toggle.setAttribute('aria-disabled', String(held || pending.has(item.id)));
      toggle.addEventListener('click', () => {
        if (toggle.getAttribute('aria-disabled') === 'true') return;
        const next = toggle.getAttribute('aria-checked') !== 'true';
        const fields = setEnabledCommand(item.id, next);
        if (!fields || !options.send(fields)) return;
        pending.add(item.id);
        toggle.setAttribute('aria-checked', String(next));
        toggle.setAttribute('aria-disabled', 'true');
      });
      actions.append(remove, toggle);
    }
    main.append(tile, text, actions);
    row.append(main);
    if (open) row.append(declarations(item));

    if (confirming === item.id && !removing) {
      const ask = el('div', 'sp-ext-confirm');
      ask.setAttribute('role', 'group');
      ask.setAttribute('aria-label', `Confirm removing ${item.name}`);
      const question = el('p', 'sp-desc');
      question.textContent = removeQuestion(item);
      const buttons = el('div', 'sp-reset');
      const cancel = textButton('Cancel', () => { confirming = null; render(`${item.id}:remove`); });
      cancel.dataset.focus = `${item.id}:cancel`;
      const confirm = textButton('Remove', () => {
        const fields = removeCommand(item.id);
        confirming = null;
        if (fields) options.send(fields);
        render(`${item.id}:toggle`);
      }, { aria: `Remove ${item.name}`, icon: 'trash' });
      confirm.classList.add('is-danger');
      confirm.dataset.focus = `${item.id}:confirm`;
      buttons.append(cancel, confirm);
      ask.append(question, buttons);
      ask.addEventListener('keydown', (event) => {
        if (event.key !== 'Escape') return;
        event.stopPropagation();
        confirming = null;
        render(`${item.id}:remove`);
      });
      row.append(ask);
    }
    return row;
  };

  /** Draws the card from the snapshot, keeping the keyboard on the same control (or `focusKey`). */
  const render = (focusKey?: string): void => {
    const active = document.activeElement as HTMLElement | null;
    const keep = focusKey ?? (active && root.contains(active) ? active.dataset.focus : undefined);
    // The empty state has its own, larger install button; an unreadable list has nothing to add to.
    install.hidden = !snapshot || invalid || snapshot.state !== 'on' || snapshot.items.length === 0;
    install.disabled = installing;
    linkInstall.disabled = installing; linkInput.disabled = installing;
    installCrx.hidden = install.hidden;
    installCrx.disabled = installing;
    box.setAttribute('aria-busy', String(!snapshot && !invalid));
    if (invalid) {
      box.replaceChildren(stateBlock('error', 'Extensions could not be shown', 'Bowser sent a list this page does not understand.', textButton('Try again', () => options.send({ a: 'refresh' }))));
    } else if (!snapshot) {
      box.replaceChildren(stateBlock('loading', 'Reading installed extensions…', ''));
    } else if (snapshot.state === 'error') {
      const retry = textButton('Try again', () => options.send({ a: 'refresh' }));
      retry.dataset.focus = 'retry';
      box.replaceChildren(stateBlock('error', 'Extensions could not be read', snapshot.error, retry));
    } else if (snapshot.items.length === 0) {
      const choose = textButton('Choose folder…', () => sendInstall(), { icon: 'folder' });
      choose.dataset.focus = 'choose';
      choose.disabled = installing;
      const chooseCrx = crxButton();
      chooseCrx.dataset.focus = 'choose-crx';
      chooseCrx.disabled = installing;
      const choices = el('div', 'sp-ext-state-actions');
      choices.append(choose, chooseCrx);
      box.replaceChildren(stateBlock('empty', 'No extensions installed', EMPTY_DETAIL, choices));
    } else {
      const busy = snapshot.busy;
      box.replaceChildren(...snapshot.items.map((item) => itemRow(item, busy)));
    }
    if (keep) root.querySelector<HTMLElement>(`[data-focus="${keep}"]`)?.focus();
  };

  render();
  return {
    root,
    receive(message: SnapshotMsg): void {
      const parsed = parseExtensionsSnapshot(message.d);
      if (!parsed) { invalid = true; snapshot = null; render(); return; }
      invalid = false;
      // Every answer comes with a snapshot: the switches and the install button are current again.
      pending.clear();
      installing = false;
      if (confirming !== null && !parsed.items.some((item) => item.id === confirming)) confirming = null;
      for (const id of [...expanded]) if (!parsed.items.some((item) => item.id === id)) expanded.delete(id);
      snapshot = parsed;
      const answer = ackToast(parsed.ack, parsed.error);
      if (answer) options.toast(answer.text, answer.ok);
      render();
    },
  };
}
