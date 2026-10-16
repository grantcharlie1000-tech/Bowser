// The settings page's Keyboard and Quick commands sections (docs/commands.md). Rust sends
// `{t:"snap",p:"commands"}` to the settings page; this module draws both sections from it and sends
// their commands through the page's `set` family. Words, checks and wire texts live in
// commands-logic.ts. Text from Rust reaches the DOM through textContent only.
//
// Keyboard: the on/off options, the sites that prioritise their own shortcuts, and every command with
// its shortcuts (add by pressing the keys, remove one, clear, restore, browser priority).
// Quick commands: which result types show and in what order, the visible-result limit, the three
// behaviour switches, and the command chains (create, clone, edit, delete, test).
import { el, icon } from './dom';
import { selectField, type SelectField } from './panels/select';
import { iconButton, textButton } from './panels/kit';
import {
  CHEAT_SHEET_COMMAND, captureModeCommand, MAX_CHAIN_NAME, MAX_DELAY_MS, MAX_RESULTS, MAX_STEPS, MIN_RESULTS, RESTORE_ALL_COMMAND, RESULT_TYPES, RESULT_TYPE_HINTS, RESULT_TYPE_LABELS,
  SHORTCUTS_OFF_NOTE, TYPES_RESET_COMMAND, ackToast, assignCommand, captureChord, chainCloneCommand, chainDeleteCommand, chainSaveCommand, chainTestCommand, clearCommand,
  conflictText, draftError, flagCommand, groupCommands, moveItem, parseCommandsSnapshot, parseDelay, priorityCommand, removeChordCommand, restoreCommand, resultsMaxCommand,
  siteAddCommand, siteError, siteRemoveCommand, toggleType, typesCommand,
  type ChainDraft, type ChainInfo, type CommandInfo, type CommandsSnapshot, type FlagName, type ResultTypeKey,
} from './commands-logic';
import type { PanelFields, SnapshotMsg } from './protocol';
import type { RowSpec } from './settings-page-logic';
import { ROWS, normaliseSite } from './settings-page-logic';

export interface CommandsSectionsOptions {
  /** Sends one `set` command; false when the bridge refused it. */
  readonly send: (fields: PanelFields) => boolean;
  /** The settings page's toast. */
  readonly toast: (text: string, ok: boolean) => void;
}

/** A row the page's search can hide and mark: the same shape as the page's own rows. */
export interface SearchRow {
  readonly spec: RowSpec;
  readonly root: HTMLElement;
  readonly label: HTMLElement;
  readonly desc: HTMLElement | null;
  readonly card: HTMLElement;
}
export interface SearchGroup { readonly root: HTMLElement; readonly heading: HTMLElement | null; readonly card: HTMLElement }

export interface CommandsSections {
  /** What goes in the Keyboard section, then the Quick commands section, in order. */
  readonly keyboard: readonly HTMLElement[];
  readonly commands: readonly HTMLElement[];
  readonly rows: readonly SearchRow[];
  readonly groups: readonly SearchGroup[];
  /** Applies a `commands` snapshot. */
  receive(message: SnapshotMsg): void;
}

interface Switch { readonly root: HTMLButtonElement; set(on: boolean): void }

function switchControl(label: string, onChange: (on: boolean) => void): Switch {
  const button = el('button', 'bw-toggle sp-switch');
  button.type = 'button';
  button.setAttribute('role', 'switch');
  button.setAttribute('aria-checked', 'false');
  button.setAttribute('aria-label', label);
  button.addEventListener('click', () => {
    const next = button.getAttribute('aria-checked') !== 'true';
    button.setAttribute('aria-checked', String(next));
    onChange(next);
  });
  return { root: button, set: (on) => button.setAttribute('aria-checked', String(on)) };
}

const specOf = (id: string): RowSpec => {
  const spec = ROWS.find((row) => row.id === id);
  if (!spec) throw new Error(`no row ${id}`);
  return spec;
};

export function mountCommandsSections(options: CommandsSectionsOptions): CommandsSections {
  let snapshot: CommandsSnapshot | null = null;
  const rows: SearchRow[] = [];
  const groups: SearchGroup[] = [];
  let cardSeq = 0;

  const group = (heading?: string): { root: HTMLElement; card: HTMLElement } => {
    const root = el('div', 'sp-group');
    const card = el('div', 'sp-card');
    card.setAttribute('role', 'group');
    let h: HTMLElement | null = null;
    if (heading) {
      h = el('h3', 'sp-group-h');
      h.id = `sp-cmd-card-${++cardSeq}`;
      h.textContent = heading;
      card.setAttribute('aria-labelledby', h.id);
      root.append(h);
    }
    root.append(card);
    groups.push({ root, heading: h, card });
    return { root, card };
  };

  /** A settings row (label, description, control) registered with the page's search. */
  const row = (into: HTMLElement, id: string, control?: HTMLElement, rowOptions: { stack?: boolean } = {}): SearchRow => {
    const spec = specOf(id);
    const node = el('div', `sp-row${rowOptions.stack ? ' is-stack' : ''}`);
    node.dataset.row = id;
    const text = el('div', 'sp-text');
    const label = el('span', 'sp-label');
    label.id = `sp-l-${id}`;
    label.textContent = spec.label;
    text.append(label);
    let desc: HTMLElement | null = null;
    if (spec.description !== '') {
      desc = el('span', 'sp-desc');
      desc.id = `sp-d-${id}`;
      desc.textContent = spec.description;
      text.append(desc);
    }
    node.append(text);
    if (control) {
      const wrap = el('div', 'sp-ctl');
      wrap.append(control);
      const target = control.matches('button, input') ? control : control.querySelector<HTMLElement>('button, input');
      if (target && !target.hasAttribute('aria-label')) target.setAttribute('aria-labelledby', label.id);
      if (target && desc) target.setAttribute('aria-describedby', desc.id);
      node.append(wrap);
    }
    into.append(node);
    const made = { spec, root: node, label, desc, card: into };
    rows.push(made);
    return made;
  };

  const send = (fields: PanelFields): boolean => options.send(fields);

  // ======================= Keyboard =======================
  const optionsGroup = group('Options');
  const flags: Array<{ id: string; flag: FlagName; control: Switch; read: (s: CommandsSnapshot) => boolean }> = [];
  const addFlag = (id: string, flag: FlagName, read: (s: CommandsSnapshot) => boolean): void => {
    const control = switchControl(specOf(id).label, (on) => { send(flagCommand(flag, on)); });
    row(optionsGroup.card, id, control.root);
    flags.push({ id, flag, control, read });
  };
  addFlag('kbEnabled', 'shortcuts_enabled', (s) => s.shortcutsEnabled);
  addFlag('kbSingleKey', 'single_key_shortcuts', (s) => s.singleKey);
  addFlag('kbAltMenu', 'alt_opens_menu', (s) => s.altMenu);
  addFlag('kbFocusCycling', 'focus_cycling', (s) => s.focusCycling);
  const offNote = el('p', 'sp-flag sp-cmd-note');
  const offNoteText = el('span');
  offNoteText.textContent = SHORTCUTS_OFF_NOTE;
  offNote.append(icon('info', { size: 12, fine: true }), offNoteText);
  offNote.hidden = true;
  optionsGroup.root.append(offNote);

  // Sites that prioritise their own shortcuts.
  const sitesGroup = group('Websites');
  const siteList = el('ul', 'sp-sites');
  siteList.setAttribute('aria-label', 'Sites that prioritise their own shortcuts');
  const siteForm = el('div', 'sp-allow-add');
  const siteInput = el('input', 'bw-input sp-allow-input');
  siteInput.type = 'text';
  siteInput.placeholder = 'example.com';
  siteInput.autocomplete = 'off';
  siteInput.spellcheck = false;
  siteInput.maxLength = 253;
  siteInput.setAttribute('aria-label', 'Add a site that prioritises its own shortcuts');
  const siteError$ = el('p', 'sp-field-error');
  siteError$.setAttribute('role', 'alert');
  const addSite = (): void => {
    if (!snapshot) return;
    const site = normaliseSite(siteInput.value);
    const error = siteError(snapshot.sites, site);
    if (error !== null || site === null) { siteError$.textContent = error ?? ''; return; }
    siteError$.textContent = '';
    siteInput.value = '';
    send(siteAddCommand(site));
  };
  const siteAdd = textButton('Add', addSite, { icon: 'plus' });
  siteInput.addEventListener('keydown', (event) => { if (event.key === 'Enter') { event.preventDefault(); addSite(); } });
  siteInput.addEventListener('input', () => { siteError$.textContent = ''; });
  siteForm.append(siteInput, siteAdd);
  const sitesRow = row(sitesGroup.card, 'kbSites', siteForm);
  sitesRow.root.classList.add('is-wrap', 'is-tight');
  sitesRow.root.append(siteList, siteError$);
  const paintSites = (sites: readonly string[]): void => {
    siteList.replaceChildren(...sites.map((site) => {
      const item = el('li', 'sp-site');
      const name = el('span', 'sp-site-name');
      name.textContent = site;
      name.dir = 'auto';
      const remove = el('button', 'bw-icon-btn sp-site-remove');
      remove.type = 'button';
      remove.setAttribute('aria-label', `Remove ${site}`);
      remove.setAttribute('data-tip', 'Remove');
      remove.append(icon('close', { size: 12, fine: true }));
      remove.addEventListener('click', () => { send(siteRemoveCommand(site)); siteInput.focus(); });
      item.append(name, remove);
      return item;
    }));
    siteList.hidden = sites.length === 0;
  };

  // The shortcut list.
  const editor = el('div', 'sp-kb');
  editor.dataset.row = 'kbEditor';
  const editorSpec = specOf('kbEditor');
  const editorHead = el('div', 'sp-kb-head');
  const editorHeading = el('h3', 'sp-group-h');
  editorHeading.id = 'sp-kb-h';
  editorHeading.textContent = editorSpec.label;
  const editorTools = el('div', 'sp-kb-tools');
  const sheetButton = textButton('Cheat sheet', () => { send(CHEAT_SHEET_COMMAND); }, { icon: 'command' });
  sheetButton.setAttribute('aria-label', 'Show the keyboard cheat sheet');
  let confirmingRestore = false;
  const cancelRestore = textButton('Cancel', () => { confirmingRestore = false; paintRestore(); });
  const restoreAll = textButton('Restore all', () => {
    if (!confirmingRestore) { confirmingRestore = true; paintRestore(); cancelRestore.focus(); return; }
    confirmingRestore = false;
    paintRestore();
    send(RESTORE_ALL_COMMAND);
  }, { icon: 'reset' });
  const paintRestore = (): void => {
    restoreAll.lastChild!.textContent = confirmingRestore ? 'Restore every shortcut' : 'Restore all';
    restoreAll.classList.toggle('is-danger', confirmingRestore);
    cancelRestore.hidden = !confirmingRestore;
  };
  editorTools.append(sheetButton, cancelRestore, restoreAll);
  paintRestore();
  editorHead.append(editorHeading, editorTools);
  const editorNote = el('p', 'sp-desc sp-kb-lead');
  editorNote.id = 'sp-kb-d';
  editorNote.textContent = editorSpec.description;
  const filter = el('input', 'bw-input sp-kb-filter');
  filter.type = 'search';
  filter.placeholder = 'Filter shortcuts';
  filter.autocomplete = 'off';
  filter.spellcheck = false;
  filter.maxLength = 64;
  filter.setAttribute('aria-label', 'Filter shortcuts');
  const editorList = el('div', 'sp-kb-list');
  editor.append(editorHead, editorNote, filter, editorList);

  /** The command waiting for a key press, and a chord waiting for the user to say "reassign". */
  let capturing: string | null = null;
  let pending: { id: string; chord: string } | null = null;
  /** Bowser said the chord is another command's: its chord and that command's name. */
  let conflict: { chord: string; label: string } | null = null;
  let editorQuery = '';
  filter.addEventListener('input', () => { editorQuery = filter.value; paintEditor(); });

  const onCaptureKey = (event: KeyboardEvent): void => {
    if (capturing === null || event.isComposing) return;
    const result = captureChord(event);
    // Keys the page itself uses (Tab to move on, a bare modifier) are left alone.
    if (result.kind === 'wait') { event.preventDefault(); event.stopPropagation(); return; }
    event.preventDefault();
    event.stopPropagation();
    const id = capturing;
    capturing = null;
    document.removeEventListener('keydown', onCaptureKey, true);
    send(captureModeCommand(false));
    if (result.kind === 'chord') {
      pending = { id, chord: result.chord };
      send(assignCommand(id, result.chord, false));
    }
    paintEditor();
  };
  const stopCapture = (): void => {
    if (capturing === null) return;
    capturing = null;
    document.removeEventListener('keydown', onCaptureKey, true);
    send(captureModeCommand(false));
    paintEditor();
  };
  // Switching windows while waiting for a key cancels the wait, so Bowser's own shortcuts come back.
  window.addEventListener('blur', stopCapture);

  const chip = (command: CommandInfo, chord: string): HTMLElement => {
    const node = el('span', 'sp-kb-chip');
    const text = el('kbd', 'sp-kb-keys');
    text.textContent = chord;
    text.dir = 'ltr';
    const remove = el('button', 'bw-icon-btn sp-kb-chip-remove');
    remove.type = 'button';
    remove.setAttribute('aria-label', `Remove ${chord} from ${command.label}`);
    remove.setAttribute('data-tip', 'Remove');
    remove.append(icon('close', { size: 12, fine: true }));
    remove.addEventListener('click', () => { touched(command.id, removeChordCommand(command.id, chord)); });
    node.append(text, remove);
    return node;
  };

  const commandRow = (command: CommandInfo): HTMLElement => {
    const item = el('div', 'sp-kb-row');
    item.dataset.command = command.id;
    const name = el('div', 'sp-kb-name');
    const label = el('span', 'sp-label');
    label.textContent = command.label;
    name.append(label);
    const keys = el('div', 'sp-kb-keylist');
    if (command.chords.length === 0) {
      const none = el('span', 'sp-kb-none');
      none.textContent = 'No shortcut';
      keys.append(none);
    }
    for (const chord of command.chords) keys.append(chip(command, chord));
    if (capturing === command.id) {
      const box = el('span', 'sp-kb-capture');
      box.tabIndex = 0;
      box.setAttribute('role', 'status');
      box.textContent = 'Press the new shortcut…';
      const cancel = el('button', 'bw-icon-btn');
      cancel.type = 'button';
      cancel.setAttribute('aria-label', 'Cancel');
      cancel.setAttribute('data-tip', 'Cancel');
      cancel.append(icon('close', { size: 12, fine: true }));
      cancel.addEventListener('click', stopCapture);
      box.append(cancel);
      keys.append(box);
      queueMicrotask(() => box.focus({ preventScroll: true }));
    } else {
      const add = el('button', 'bw-icon-btn sp-kb-add');
      add.type = 'button';
      add.disabled = command.chords.length >= 6;
      add.setAttribute('aria-label', `Add a shortcut to ${command.label}`);
      add.setAttribute('data-tip', 'Add a shortcut');
      add.append(icon('plus', { size: 12, fine: true }));
      add.addEventListener('click', () => {
        stopCapture();
        pending = null;
        capturing = command.id;
        document.addEventListener('keydown', onCaptureKey, true);
        send(captureModeCommand(true));
        paintEditor();
      });
      keys.append(add);
    }
    const tools = el('div', 'sp-kb-rowtools');
    if (command.chords.length > 0) tools.append(iconButton('trash', `Clear the shortcuts of ${command.label}`, () => { touched(command.id, clearCommand(command.id)); }));
    if (command.changed) tools.append(iconButton('reset', `Restore the default shortcuts of ${command.label}`, () => { touched(command.id, restoreCommand(command.id)); }));
    if (!command.chain) {
      // Browser priority: on a site that prioritises its own shortcuts, this command still works.
      const priority = iconButton('shield', `Browser priority for ${command.label}`, () => { touched(command.id, priorityCommand(command.id, !command.priority)); }, 'sp-kb-priority');
      priority.setAttribute('aria-pressed', String(command.priority));
      priority.setAttribute('data-tip', command.priority ? 'Browser priority: still works on sites that use their own shortcuts' : 'Not browser priority: a site that uses its own shortcuts gets this key');
      tools.append(priority);
    }
    item.append(name, keys, tools);
    if (conflict && pending?.id === command.id && conflict.chord === pending.chord) {
      const warning = el('div', 'sp-kb-conflict');
      warning.setAttribute('role', 'alert');
      const message = el('span');
      message.textContent = conflictText({ conflictChord: conflict.chord, conflictLabel: conflict.label });
      const take = textButton('Reassign', () => {
        const taking = pending;
        pending = null;
        if (taking) send(assignCommand(taking.id, taking.chord, true));
        paintEditor();
      }, { primary: true });
      const keep = textButton('Cancel', () => { pending = null; paintEditor(); });
      warning.append(message, take, keep);
      item.append(warning);
    }
    return item;
  };

  /** The command whose row the user last acted on: its row gets the keyboard back after the list is redrawn. */
  let lastRow: string | null = null;
  const touched = (id: string, fields: PanelFields): void => { lastRow = id; send(fields); };

  function paintEditor(): void {
    if (!snapshot) { editorList.replaceChildren(); return; }
    const hadFocus = editorList.contains(document.activeElement);
    const grouped = groupCommands(snapshot.commands, editorQuery);
    if (grouped.length === 0) {
      const empty = el('p', 'sp-desc sp-kb-empty');
      empty.textContent = editorQuery.trim() === '' ? 'No commands.' : `No command or shortcut matches “${editorQuery.trim()}”.`;
      editorList.replaceChildren(empty);
      return;
    }
    editorList.replaceChildren(...grouped.map((entry) => {
      const box = el('section', 'sp-kb-cat');
      const heading = el('h4', 'sp-card-h');
      heading.textContent = entry.category;
      const card = el('div', 'sp-card');
      for (const command of entry.commands) card.append(commandRow(command));
      box.append(heading, card);
      return box;
    }));
    // The button that was clicked is gone with the old list: keep the keyboard on that command's row.
    if (hadFocus && lastRow !== null) {
      const row = editorList.querySelector<HTMLElement>(`.sp-kb-row[data-command="${lastRow}"]`);
      (row?.querySelector<HTMLElement>('button:not([disabled])') ?? null)?.focus({ preventScroll: true });
    }
  }
  const editorRow: SearchRow = { spec: editorSpec, root: editor, label: editorHeading, desc: null, card: editor };
  rows.push(editorRow);

  // ======================= Quick commands =======================
  const resultsGroup = group('Results');
  const typesList = el('ul', 'sp-qc-types');
  typesList.setAttribute('aria-label', 'Result types, in the order they show');
  const typesReset = textButton('Reset order', () => { send(TYPES_RESET_COMMAND); }, { icon: 'reset' });
  const typesRow = row(resultsGroup.card, 'qcTypes', typesList, { stack: true });
  const typesCorner = el('div', 'sp-corner');
  typesCorner.append(typesReset);
  typesRow.root.append(typesCorner);
  const paintTypes = (s: CommandsSnapshot): void => {
    const shown = s.paletteTypes;
    const hidden = RESULT_TYPES.filter((t) => !shown.includes(t));
    const item = (type: ResultTypeKey, at: number): HTMLElement => {
      const on = shown.includes(type);
      const li = el('li', `sp-qc-type${on ? '' : ' is-off'}`);
      const sw = switchControl(`Show ${RESULT_TYPE_LABELS[type]}`, (next) => { send(typesCommand(toggleType(shown, type, next))); });
      sw.set(on);
      const text = el('span', 'sp-qc-type-text');
      const name = el('span', 'sp-label');
      name.textContent = RESULT_TYPE_LABELS[type];
      const hint = el('span', 'sp-desc');
      hint.textContent = RESULT_TYPE_HINTS[type];
      text.append(name, hint);
      const up = iconButton('chevron-up', `Move ${RESULT_TYPE_LABELS[type]} up`, () => { send(typesCommand(moveItem(shown, at, at - 1))); }, 'sp-qc-move');
      const down = iconButton('chevron-down', `Move ${RESULT_TYPE_LABELS[type]} down`, () => { send(typesCommand(moveItem(shown, at, at + 1))); }, 'sp-qc-move');
      up.disabled = !on || at === 0;
      down.disabled = !on || at === shown.length - 1;
      li.append(sw.root, text, up, down);
      return li;
    };
    typesList.replaceChildren(...shown.map((type, at) => item(type, at)), ...hidden.map((type) => item(type, -1)));
    typesReset.hidden = s.paletteDefaultOrder;
  };

  const maxInput = el('input', 'bw-input sp-zoom-input');
  maxInput.type = 'number';
  maxInput.min = String(MIN_RESULTS);
  maxInput.max = String(MAX_RESULTS);
  maxInput.step = '1';
  maxInput.setAttribute('aria-label', specOf('qcMax').label);
  maxInput.addEventListener('change', () => {
    const n = Math.min(MAX_RESULTS, Math.max(MIN_RESULTS, Math.round(Number(maxInput.value)) || MIN_RESULTS));
    maxInput.value = String(n);
    send(resultsMaxCommand(n));
  });
  row(resultsGroup.card, 'qcMax', maxInput);
  addFlagIn(resultsGroup.card, 'qcKeepLast', 'palette_keep_last', (s) => s.paletteKeepLast);
  addFlagIn(resultsGroup.card, 'qcNewTab', 'palette_open_new_tab', (s) => s.paletteNewTab);
  addFlagIn(resultsGroup.card, 'qcNotesPanel', 'palette_notes_in_panel', (s) => s.paletteNotesInPanel);
  function addFlagIn(card: HTMLElement, id: string, flag: FlagName, read: (s: CommandsSnapshot) => boolean): void {
    const control = switchControl(specOf(id).label, (on) => { send(flagCommand(flag, on)); });
    row(card, id, control.root);
    flags.push({ id, flag, control, read });
  }

  // ---- command chains ----
  const chainsBlock = el('div', 'sp-chains');
  chainsBlock.dataset.row = 'chains';
  const chainsSpec = specOf('chains');
  const chainsHead = el('div', 'sp-kb-head');
  const chainsHeading = el('h3', 'sp-group-h');
  chainsHeading.id = 'sp-chains-h';
  chainsHeading.textContent = chainsSpec.label;
  const newChain = textButton('New chain', () => { openEditor({ id: 0, name: '', steps: [] }); }, { icon: 'plus' });
  chainsHead.append(chainsHeading, newChain);
  const chainsNote = el('p', 'sp-desc sp-kb-lead');
  chainsNote.textContent = chainsSpec.description;
  const chainsCard = el('div', 'sp-card sp-chains-card');
  chainsCard.setAttribute('role', 'group');
  chainsCard.setAttribute('aria-labelledby', chainsHeading.id);
  const editorCard = el('div', 'sp-card sp-chain-editor');
  editorCard.hidden = true;
  editorCard.setAttribute('role', 'group');
  editorCard.setAttribute('aria-label', 'Edit chain');
  chainsBlock.append(chainsHead, chainsNote, chainsCard, editorCard);
  rows.push({ spec: chainsSpec, root: chainsBlock, label: chainsHeading, desc: null, card: chainsBlock });

  let draft: ChainDraft | null = null;
  let draftError$ = '';
  let saving = false;
  let confirmingDelete: number | null = null;
  const commandSelects: SelectField<string>[] = [];

  function openEditor(next: ChainDraft): void {
    draft = { id: next.id, name: next.name, steps: next.steps.map((step) => ({ ...step })) };
    draftError$ = '';
    saving = false;
    paintEditorCard();
    editorCard.querySelector<HTMLElement>('.sp-chain-name')?.focus();
  }
  function closeEditor(): void {
    draft = null;
    draftError$ = '';
    saving = false;
    for (const select of commandSelects.splice(0)) select.close();
    editorCard.hidden = true;
    editorCard.replaceChildren();
  }

  function paintEditorCard(): void {
    if (!draft || !snapshot) return;
    const current = draft;
    for (const select of commandSelects.splice(0)) select.close();
    editorCard.hidden = false;
    const available = snapshot.commands.filter((command) => !command.chain);
    const nameRow = el('div', 'sp-chain-line is-tight');
    const nameText = el('div', 'sp-text');
    const nameLabel = el('label', 'sp-label');
    nameLabel.textContent = 'Name';
    const nameInput = el('input', 'bw-input sp-chain-name');
    nameInput.type = 'text';
    nameInput.value = current.name;
    nameInput.maxLength = MAX_CHAIN_NAME;
    nameInput.autocomplete = 'off';
    nameInput.spellcheck = false;
    nameInput.placeholder = 'Morning routine';
    nameInput.id = 'sp-chain-name';
    nameLabel.htmlFor = nameInput.id;
    nameInput.addEventListener('input', () => { current.name = nameInput.value; });
    nameText.append(nameLabel);
    const nameCtl = el('div', 'sp-ctl');
    nameCtl.append(nameInput);
    nameRow.append(nameText, nameCtl);

    const steps = el('ol', 'sp-chain-steps');
    steps.setAttribute('aria-label', 'Commands in this chain, in order');
    current.steps.forEach((step, at) => {
      const li = el('li', 'sp-chain-step');
      const number = el('span', 'sp-chain-num');
      number.textContent = String(at + 1);
      const select = selectField<string>({
        options: available.map((command) => ({ value: command.id, label: command.label })),
        value: available.some((command) => command.id === step.command) ? step.command : null,
        label: `Command ${at + 1}`,
        placeholder: 'Choose a command',
        onChange: (value) => { current.steps[at] = { ...current.steps[at]!, command: value }; },
      });
      commandSelects.push(select);
      const delay = el('input', 'bw-input sp-chain-delay');
      delay.type = 'number';
      delay.min = '0';
      delay.max = String(MAX_DELAY_MS);
      delay.step = '50';
      delay.value = String(step.delay);
      delay.setAttribute('aria-label', `Delay before command ${at + 1}, in milliseconds`);
      delay.addEventListener('change', () => {
        const n = parseDelay(delay.value);
        delay.value = String(n);
        current.steps[at] = { ...current.steps[at]!, delay: n };
      });
      const unit = el('span', 'sp-value');
      unit.textContent = 'ms';
      const up = iconButton('chevron-up', `Move command ${at + 1} up`, () => { current.steps = moveItem(current.steps, at, at - 1); paintEditorCard(); }, 'sp-qc-move');
      const down = iconButton('chevron-down', `Move command ${at + 1} down`, () => { current.steps = moveItem(current.steps, at, at + 1); paintEditorCard(); }, 'sp-qc-move');
      up.disabled = at === 0;
      down.disabled = at === current.steps.length - 1;
      const remove = iconButton('close', `Remove command ${at + 1}`, () => { current.steps = current.steps.filter((_, i) => i !== at); paintEditorCard(); }, 'sp-qc-move');
      li.append(number, select.root, delay, unit, up, down, remove);
      steps.append(li);
    });
    if (current.steps.length === 0) {
      const none = el('p', 'sp-desc sp-chain-none');
      none.textContent = 'No commands yet. Add the first one below.';
      steps.append(none);
    }
    const adder = selectField<string>({
      options: available.map((command) => ({ value: command.id, label: command.label })),
      value: null,
      label: 'Add a command',
      placeholder: 'Add a command…',
      momentary: true,
      onChange: (value) => {
        if (current.steps.length >= MAX_STEPS) { draftError$ = `A chain holds at most ${MAX_STEPS} commands.`; paintEditorCard(); return; }
        current.steps = [...current.steps, { delay: 0, command: value }];
        paintEditorCard();
      },
    });
    commandSelects.push(adder);
    const delayNote = el('p', 'sp-desc sp-chain-lead');
    delayNote.textContent = 'Each delay is how long to wait before that command runs. A chain runs its commands one after another.';
    const stepsRow = el('div', 'sp-chain-line is-stack');
    stepsRow.append(delayNote, steps, adder.root);

    const error = el('p', 'sp-field-error sp-chain-error');
    error.setAttribute('role', 'alert');
    error.textContent = draftError$;
    const actions = el('div', 'sp-chain-actions');
    const test = textButton('Test chain', () => {
      const problem = draftError(current, snapshot?.chains ?? []);
      if (problem !== null) { draftError$ = problem; paintEditorCard(); return; }
      draftError$ = '';
      error.textContent = '';
      send(chainTestCommand(current));
    }, { icon: 'play' });
    const cancel = textButton('Cancel', closeEditor);
    const save = textButton(current.id === 0 ? 'Create chain' : 'Save chain', () => {
      const problem = draftError(current, snapshot?.chains ?? []);
      if (problem !== null) { draftError$ = problem; paintEditorCard(); return; }
      draftError$ = '';
      saving = true;
      if (!send(chainSaveCommand(current))) { saving = false; draftError$ = 'That could not be sent.'; }
      paintEditorCard();
    }, { primary: true });
    save.disabled = saving;
    actions.append(test, cancel, save);
    const footer = el('div', 'sp-chain-line sp-chain-footer');
    footer.append(error, actions);
    editorCard.replaceChildren(nameRow, stepsRow, footer);
  }

  function paintChains(s: CommandsSnapshot): void {
    if (s.chains.length === 0) {
      const empty = el('div', 'sp-chain-line sp-chains-empty');
      const text = el('div', 'sp-text');
      const label = el('span', 'sp-label');
      label.textContent = 'No chains yet';
      const desc = el('span', 'sp-desc');
      desc.textContent = 'A chain runs several commands one after another. Make one, then run it from Quick commands or give it a shortcut.';
      text.append(label, desc);
      empty.append(text);
      chainsCard.replaceChildren(empty);
      return;
    }
    const labels = new Map(s.commands.map((command) => [command.id, command.label]));
    chainsCard.replaceChildren(...s.chains.map((chain: ChainInfo) => {
      const item = el('div', 'sp-chain-line sp-chain-row');
      const text = el('div', 'sp-text');
      const name = el('span', 'sp-label');
      name.textContent = chain.name;
      name.dir = 'auto';
      const desc = el('span', 'sp-desc');
      desc.textContent = chain.steps.map((step) => labels.get(step.command) ?? step.command).join(' → ');
      text.append(name, desc);
      const tools = el('div', 'sp-ctl sp-chain-tools');
      if (confirmingDelete === chain.id) {
        const ask = el('span', 'sp-desc');
        ask.textContent = `Delete ${chain.name}?`;
        const yes = textButton('Delete', () => { confirmingDelete = null; send(chainDeleteCommand(chain.id)); paintChains(s); }, { icon: 'trash' });
        yes.classList.add('is-danger');
        const no = textButton('Cancel', () => { confirmingDelete = null; paintChains(s); });
        tools.append(ask, no, yes);
      } else {
        tools.append(
          iconButton('play', `Test ${chain.name}`, () => { send(chainTestCommand({ id: chain.id, name: chain.name, steps: [...chain.steps] })); }),
          iconButton('edit', `Edit ${chain.name}`, () => { openEditor({ id: chain.id, name: chain.name, steps: [...chain.steps] }); }),
          iconButton('copy', `Clone ${chain.name}`, () => { send(chainCloneCommand(chain.id)); }),
          iconButton('trash', `Delete ${chain.name}`, () => { confirmingDelete = chain.id; paintChains(s); }),
        );
      }
      item.append(text, tools);
      return item;
    }));
  }

  // ======================= applying a snapshot =======================
  const apply = (s: CommandsSnapshot): void => {
    for (const entry of flags) entry.control.set(entry.read(s));
    offNote.hidden = s.shortcutsEnabled;
    paintSites(s.sites);
    paintTypes(s);
    maxInput.value = String(s.paletteMax);
    paintChains(s);
    if (confirmingDelete !== null && !s.chains.some((chain) => chain.id === confirmingDelete)) confirmingDelete = null;
    if (draft) {
      // A saved draft closes its editor; a refused one stays, with the reason.
      if (saving && s.ack === 'saved') closeEditor();
      else if (saving && s.ack === 'failed') { saving = false; draftError$ = s.error; paintEditorCard(); }
    }
    if (s.ack === 'conflict') conflict = { chord: s.conflictChord, label: s.conflictLabel };
    else if (s.ack === 'saved' || s.ack === 'failed') { conflict = null; pending = null; }
    paintEditor();
  };

  return {
    keyboard: [optionsGroup.root, sitesGroup.root, editor],
    commands: [resultsGroup.root, chainsBlock],
    rows,
    groups,
    receive(message: SnapshotMsg): void {
      const parsed = parseCommandsSnapshot(message.d);
      if (!parsed) return;
      snapshot = parsed;
      apply(parsed);
      const toast = ackToast(parsed);
      if (toast) options.toast(toast.text, toast.ok);
    },
  };
}
