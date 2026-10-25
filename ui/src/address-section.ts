// The settings page's "Address bar", "Search engine" and "Bookmarks" sections and the bookmarks bar's
// own rows. Rust sends `{t:"snap",p:"address"}` (src/chrome_host/address_page.rs); this module draws
// it into the sections the page made, sends one `set` command per change, and applies each answer in
// place so focus never jumps. Words and checks live in address-settings-logic.ts. Text from Rust
// reaches the DOM through textContent only.
import { el, icon, type IconName } from './dom';
import { selectField, type SelectField } from './panels/select';
import { textButton } from './panels/kit';
import type { PanelFields, SettingsSection, SnapshotMsg } from './protocol';
import {
  BAR_DISPLAYS, BAR_DISPLAY_LABELS, BAR_POSITIONS, MAX_CONFIRM, MAX_ENGINES, MAX_RESULTS, MIN_CONFIRM, MIN_RESULTS, SEARCH_FIELD_MODES, SOURCE_LABELS,
  EMPTY_ENGINE, draftOfEngine, engineChoiceLabel, engineFields, engineLabel, engineProblem, isNickWord, isSuffix, monogram, moveSource, parseAddressSnapshot, setFields, sourceList, sourceRows, toggleSource,
  type AddressKey, type AddressSnapshot, type BarDisplay, type BarPosition, type EngineDraft, type EngineOp, type EngineView, type SearchFieldMode, type SourceId,
} from './address-settings-logic';

/** What the settings page lends: its card and row builders and its controls. */
export interface AddressHost {
  /** Sends one `set` command; false when the bridge refused it. */
  readonly send: (fields: PanelFields) => boolean;
  readonly toast: (text: string, ok: boolean) => void;
  readonly card: (section: SettingsSection, heading?: string) => HTMLElement;
  readonly row: (into: HTMLElement, id: string, control?: HTMLElement, options?: { stack?: boolean }) => { readonly root: HTMLElement; readonly desc: HTMLElement | null };
  /** The card of the page's existing "Bookmarks bar" rows (Appearance), where the bar's own rows join them. */
  readonly barCard: HTMLElement;
  readonly switchControl: (onChange: (on: boolean) => void) => { readonly root: HTMLElement; set(value: boolean): void; disable(off: boolean): void };
  readonly segmentedControl: <T extends string>(label: string, options: ReadonlyArray<readonly [T, string]>, onSelect: (value: T) => void) => { readonly root: HTMLElement; set(value: T | null): void; disable(off: boolean): void };
}

export interface AddressSection {
  /** Applies an `address` snapshot. */
  receive(message: SnapshotMsg): void;
}

interface Painter { (snapshot: AddressSnapshot): void }

export function mountAddressSections(host: AddressHost): AddressSection {
  let snapshot: AddressSnapshot | null = null;
  const painters: Painter[] = [];

  /** One option change. The control keeps its new state; the next snapshot confirms or reverts it. */
  const put = (key: AddressKey, value: boolean | number | string): void => {
    const fields = setFields(key, value);
    if (!fields) { host.toast('That value is not allowed.', false); return; }
    host.send(fields);
  };

  const toggle = (cardNode: HTMLElement, id: string, key: AddressKey, options: { stack?: boolean } = {}): void => {
    const control = host.switchControl((on) => put(key, on));
    host.row(cardNode, id, control.root, options);
    painters.push((s) => control.set(Boolean(s.s[key])));
  };

  // ---- Address bar ----
  const fieldCard = host.card('address', 'Address field');
  toggle(fieldCard, 'addrShow', 'address_bar_show');
  toggle(fieldCard, 'addrAutocomplete', 'address_autocomplete');
  toggle(fieldCard, 'addrFull', 'address_show_full');
  toggle(fieldCard, 'addrSelect', 'address_select_on_activate');
  toggle(fieldCard, 'addrEncoded', 'address_copy_encoded');
  toggle(fieldCard, 'addrQr', 'address_qr_button');

  const dropCard = host.card('address', 'Drop-down');
  toggle(dropCard, 'ddTyping', 'dd_show_while_typing');
  toggle(dropCard, 'ddBadge', 'dd_always_badge');
  toggle(dropCard, 'ddTypedHistory', 'dd_typed_history');
  toggle(dropCard, 'ddTypedSearches', 'dd_typed_include_searches');
  toggle(dropCard, 'ddHistoryButton', 'dd_history_button');
  const maxInput = numberInput('Maximum results', MIN_RESULTS, MAX_RESULTS, (n) => put('dd_max_results', n));
  host.row(dropCard, 'ddMax', maxInput.root);
  painters.push((s) => maxInput.set(s.s.dd_max_results));
  toggle(dropCard, 'ddPriorityBookmarks', 'dd_prioritise_bookmarks');
  toggle(dropCard, 'ddPriorityDirect', 'dd_prioritise_direct');

  // Sources: each on or off, and in the order their rows come.
  const sourcesCard = host.card('address', 'Suggestion sources and order');
  const sourcesList = el('ul', 'ad-sources');
  sourcesList.setAttribute('aria-label', 'Suggestion sources, in the order their rows come');
  host.row(sourcesCard, 'ddSources', sourcesList, { stack: true });
  painters.push((s) => paintSources(sourcesList, s.s.dd_sources, (text) => put('dd_sources', text)));

  const securityCard = host.card('address', 'Security');
  toggle(securityCard, 'secHttps', 'sec_always_https');
  toggle(securityCard, 'secJs', 'sec_strip_js');
  toggle(securityCard, 'secBase', 'sec_highlight_base');
  const suffix = textInput('Domain expansion suffix', 24, 'com', (value) => {
    if (!isSuffix(value)) { host.toast('Use letters, digits, dots and dashes, such as com or co.uk.', false); return false; }
    put('domain_expansion_suffix', value.replace(/^\./, '').toLowerCase());
    return true;
  });
  host.row(securityCard, 'addrExpansion', suffix.root);
  painters.push((s) => suffix.set(s.s.domain_expansion_suffix));

  // ---- Search engine ----
  const enginesCard = host.card('search', 'Engines');
  const editor = mountEngineEditor(host, () => snapshot, enginesCard);
  painters.push((s) => editor.paint(s));

  const chooseCard = host.card('search', 'Which engine');
  const defaultSelect = engineSelect(host, chooseCard, 'searchDefault', 'Default search engine', false, (id) => host.send(engineFields({ op: 'default', id }) ?? { a: 'engine', text: '' }));
  const privateSelect = engineSelect(host, chooseCard, 'searchPrivate', 'Private window search engine', true, (id) => host.send(engineFields({ op: 'private', id }) ?? { a: 'engine', text: '' }));
  const imageSelect = engineSelect(host, chooseCard, 'searchImage', 'Image search engine', false, (id) => host.send(engineFields({ op: 'image', id }) ?? { a: 'engine', text: '' }));
  painters.push((s) => {
    defaultSelect.paint(s, s.def);
    privateSelect.paint(s, s.priv);
    imageSelect.paint(s, s.img, (e) => e.image !== '');
  });

  const fieldOptionsCard = host.card('search', 'Search field');
  const mode = host.segmentedControl<SearchFieldMode>('Search field', SEARCH_FIELD_MODES.map((m) => [m, m === 'off' ? 'Off' : m === 'field' ? 'Text field' : 'Button'] as const), (value) => put('search_field', value));
  host.row(fieldOptionsCard, 'searchField', mode.root);
  painters.push((s) => mode.set(s.s.search_field));
  toggle(fieldOptionsCard, 'searchNickname', 'search_show_nickname');
  toggle(fieldOptionsCard, 'searchIcon', 'search_show_icon');
  toggle(fieldOptionsCard, 'searchKeepEngine', 'search_keep_engine');
  toggle(fieldOptionsCard, 'searchKeepText', 'search_keep_text');
  toggle(fieldOptionsCard, 'searchSuggestField', 'search_suggest_field');

  const behaviourCard = host.card('search', 'Searching');
  toggle(behaviourCard, 'searchNewTab', 'search_new_tab');
  toggle(behaviourCard, 'searchInAddress', 'search_in_address');
  toggle(behaviourCard, 'searchSelection', 'search_selection_background');
  toggle(behaviourCard, 'searchSuggestAddress', 'search_suggest_address');

  const scopeCard = host.card('search', 'Scoped nicknames');
  const scope = (id: string, key: 'nick_bookmarks' | 'nick_history' | 'nick_tabs', fallback: string): void => {
    const wrap = el('div', 'ad-scope');
    const sw = host.switchControl((on) => put(key, on ? input.value.trim().replace(/^@/, '') || fallback : ''));
    sw.root.setAttribute('aria-label', `Turn ${fallback} scope on or off`);
    const at = el('span', 'ad-at');
    at.textContent = '@';
    at.setAttribute('aria-hidden', 'true');
    const input = el('input', 'bw-input ad-scope-input');
    input.type = 'text';
    input.maxLength = 16;
    input.autocomplete = 'off';
    input.spellcheck = false;
    input.setAttribute('aria-label', `Word for the ${fallback} scope`);
    input.addEventListener('change', () => {
      const word = input.value.trim().replace(/^@/, '').toLowerCase();
      if (!isNickWord(word) || word === '') { host.toast('Use up to 16 letters, digits, dashes or underscores.', false); input.value = snapshot?.s[key] ?? fallback; return; }
      put(key, word);
    });
    wrap.append(sw.root, at, input);
    host.row(scopeCard, id, wrap);
    painters.push((s) => {
      sw.set(s.s[key] !== '');
      input.disabled = s.s[key] === '';
      if (document.activeElement !== input) input.value = s.s[key] === '' ? fallback : s.s[key];
    });
  };
  scope('nickBookmarks', 'nick_bookmarks', 'bookmarks');
  scope('nickHistory', 'nick_history', 'history');
  scope('nickTabs', 'nick_tabs', 'tabs');

  // ---- Bookmarks ----
  const bookmarksCard = host.card('bookmarks', 'Opening');
  toggle(bookmarksCard, 'bmNewTab', 'bm_open_new_tab');
  toggle(bookmarksCard, 'bmSingleClick', 'bm_panel_single_click');
  const confirmInput = numberInput('Ask before opening this many bookmarks', 0, MAX_CONFIRM, (n) => {
    if (n !== 0 && n < MIN_CONFIRM) { host.toast(`Use 0, or ${MIN_CONFIRM} to ${MAX_CONFIRM}.`, false); return false; }
    put('bm_confirm_open', n);
    return true;
  });
  host.row(bookmarksCard, 'bmConfirm', confirmInput.root);
  painters.push((s) => confirmInput.set(s.s.bm_confirm_open));
  const savingCard = host.card('bookmarks', 'Saving');
  toggle(savingCard, 'bmMetadata', 'bm_fetch_metadata');
  toggle(savingCard, 'bmDialog', 'bm_always_dialog');

  // ---- The bookmarks bar's own rows, beside the page's existing ones ----
  const folderHost = el('div', 'sp-select-host');
  let folderSelect: SelectField<string> | null = null;
  let folderKey = '';
  host.row(host.barCard, 'bmBarFolder', folderHost);
  painters.push((s) => {
    const key = s.folders.map((f) => `${f.id}:${f.title}`).join('|');
    const value = String(s.folders.some((f) => f.id === s.s.bm_bar_folder) ? s.s.bm_bar_folder : 0);
    if (!folderSelect || key !== folderKey) {
      folderKey = key;
      folderSelect?.close();
      folderSelect = selectField<string>({
        options: [{ value: '0', label: 'All bookmarks' }, ...s.folders.map((f) => ({ value: String(f.id), label: `${' '.repeat(f.depth)}${f.title}` }))],
        value,
        label: 'Bookmarks bar folder',
        placeholder: 'Bookmarks bar folder',
        onChange: (next) => put('bm_bar_folder', Number(next)),
      });
      folderSelect.button.setAttribute('aria-labelledby', 'sp-l-bmBarFolder');
      folderHost.replaceChildren(folderSelect.root);
    } else {
      folderSelect.set(value);
    }
  });
  const position = host.segmentedControl<BarPosition>('Bookmarks bar position', BAR_POSITIONS.map((p) => [p, p === 'top' ? 'Top' : 'Bottom'] as const), (value) => put('bm_bar_position', value));
  host.row(host.barCard, 'bmBarPosition', position.root);
  painters.push((s) => position.set(s.s.bm_bar_position));
  const display = host.segmentedControl<BarDisplay>('Bookmarks bar display', BAR_DISPLAYS.map((d) => [d, BAR_DISPLAY_LABELS[d]] as const), (value) => put('bm_bar_display', value));
  host.row(host.barCard, 'bmBarDisplay', display.root);
  painters.push((s) => display.set(s.s.bm_bar_display));

  return {
    receive(message) {
      const parsed = parseAddressSnapshot(message.d);
      if (!parsed) return;
      snapshot = parsed;
      for (const paint of painters) paint(parsed);
      // The answer to the change (Saved, or why not) comes with the settings snapshot's own toast.
    },
  };
}

// ---- small controls ----

interface Field<T> { readonly root: HTMLElement; set(value: T): void }

function numberInput(label: string, min: number, max: number, onCommit: (value: number) => boolean | void): Field<number> {
  const input = el('input', 'bw-input ad-number');
  input.type = 'number';
  input.min = String(min);
  input.max = String(max);
  input.step = '1';
  input.setAttribute('aria-label', label);
  let last = min;
  input.addEventListener('change', () => {
    const n = Math.round(Number(input.value));
    if (!Number.isFinite(n) || n < min || n > max) { input.value = String(last); return; }
    if (onCommit(n) === false) { input.value = String(last); return; }
    last = n;
  });
  return { root: input, set(value) { last = value; if (document.activeElement !== input) input.value = String(value); } };
}

function textInput(label: string, max: number, placeholder: string, onCommit: (value: string) => boolean): Field<string> {
  const input = el('input', 'bw-input ad-text');
  input.type = 'text';
  input.maxLength = max;
  input.placeholder = placeholder;
  input.autocomplete = 'off';
  input.spellcheck = false;
  input.setAttribute('aria-label', label);
  let last = '';
  input.addEventListener('change', () => {
    const value = input.value.trim();
    if (!onCommit(value)) { input.value = last; return; }
    last = value;
  });
  return { root: input, set(value) { last = value; if (document.activeElement !== input) input.value = value; } };
}

function iconButton(name: IconName, label: string, onClick: () => void): HTMLButtonElement {
  const button = el('button', 'bw-icon-btn ad-icon-btn');
  button.type = 'button';
  button.setAttribute('aria-label', label);
  button.setAttribute('data-tip', label);
  button.append(icon(name, { size: 14, fine: true }));
  button.addEventListener('click', onClick);
  return button;
}

function paintSources(list: HTMLElement, text: string, commit: (text: string) => void): void {
  const enabled = sourceList(text);
  list.replaceChildren(...sourceRows(enabled).map(({ id, on }) => sourceItem(id, on, enabled, commit)));
}

function sourceItem(id: SourceId, on: boolean, enabled: readonly SourceId[], commit: (text: string) => void): HTMLElement {
  const item = el('li', `ad-source${on ? '' : ' is-off'}`);
  item.dataset.source = id;
  const text = el('span', 'sp-text');
  const name = el('span', 'sp-label');
  name.textContent = SOURCE_LABELS[id].label;
  const hint = el('span', 'sp-desc');
  hint.textContent = SOURCE_LABELS[id].hint;
  text.append(name, hint);
  const toggle = el('button', 'bw-toggle sp-switch');
  toggle.type = 'button';
  toggle.setAttribute('role', 'switch');
  toggle.setAttribute('aria-checked', String(on));
  toggle.setAttribute('aria-label', SOURCE_LABELS[id].label);
  toggle.addEventListener('click', () => commit(toggleSource(enabled, id, !on)));
  const index = enabled.indexOf(id);
  const up = iconButton('chevron-up', `Move ${SOURCE_LABELS[id].label} up`, () => commit(moveSource(enabled, id, -1)));
  const down = iconButton('chevron-down', `Move ${SOURCE_LABELS[id].label} down`, () => commit(moveSource(enabled, id, 1)));
  up.disabled = !on || index <= 0;
  down.disabled = !on || index < 0 || index >= enabled.length - 1;
  const tools = el('span', 'ad-tools');
  tools.append(up, down, toggle);
  item.append(text, tools);
  return item;
}

// ---- engine pickers ----

interface EnginePicker { paint(snapshot: AddressSnapshot, value: string, only?: (engine: EngineView) => boolean): void }

function engineSelect(host: AddressHost, into: HTMLElement, id: string, label: string, allowDefault: boolean, onPick: (id: string) => void): EnginePicker {
  const hostNode = el('div', 'sp-select-host');
  host.row(into, id, hostNode);
  let select: SelectField<string> | null = null;
  let key = '';
  return {
    paint(snapshot, value, only) {
      const engines = snapshot.engines.filter((e) => only?.(e) ?? true);
      const nextKey = `${allowDefault}|${engines.map((e) => `${e.id}:${e.name}`).join('|')}`;
      const current = allowDefault && value === '' ? '' : value;
      if (!select || nextKey !== key) {
        key = nextKey;
        select?.close();
        select = selectField<string>({
          options: [...(allowDefault ? [{ value: '', label: 'Same as the default engine' }] : []), ...engines.map((e) => ({ value: e.id, label: engineChoiceLabel(e.id, e.name) }))],
          value: current,
          label,
          placeholder: label,
          onChange: (next) => onPick(next),
        });
        select.button.setAttribute('aria-labelledby', `sp-l-${id}`);
        hostNode.replaceChildren(select.root);
      } else {
        select.set(current);
      }
    },
  };
}

// ---- the engine editor ----

interface Editor { paint(snapshot: AddressSnapshot): void }

function mountEngineEditor(host: AddressHost, current: () => AddressSnapshot | null, into: HTMLElement): Editor {
  const list = el('ul', 'ad-engines');
  list.setAttribute('aria-label', 'Search engines');
  const form = el('form', 'ad-form');
  form.hidden = true;
  form.noValidate = true;
  const fields = new Map<keyof EngineDraft, HTMLInputElement>();
  const inputFor = (key: keyof EngineDraft, label: string, hint: string, placeholder: string, max: number): HTMLElement => {
    const wrap = el('label', 'ad-field');
    const name = el('span', 'sp-label');
    name.textContent = label;
    const input = el('input', 'bw-input');
    input.type = 'text';
    input.maxLength = max;
    input.placeholder = placeholder;
    input.autocomplete = 'off';
    input.spellcheck = false;
    input.dir = 'ltr';
    const note = el('span', 'sp-desc');
    note.textContent = hint;
    wrap.append(name, input, note);
    fields.set(key, input);
    return wrap;
  };
  const problem = el('p', 'sp-field-error');
  problem.setAttribute('role', 'alert');
  const title = el('h4', 'ad-form-title');
  let editing: string | null = null;
  const actions = el('div', 'ad-form-actions');
  const save = textButton('Save', () => submit(), { primary: true });
  save.type = 'submit';
  const cancel = textButton('Cancel', () => closeForm(), { subtle: true });
  actions.append(save, cancel);
  form.append(
    title,
    inputFor('name', 'Name', '', 'My search', 40),
    inputFor('nick', 'Nickname', 'Type it before a query to use this engine (optional).', 'ms', 16),
    inputFor('url', 'Search address', 'Use %s where the search text goes.', 'https://example.com/search?q=%s', 1024),
    inputFor('post', 'POST parameters', 'Only for engines that take the query as a form post. Use %s once. Leave empty for a plain address.', 'q=%s&lang=en', 512),
    inputFor('suggest', 'Suggestions address', 'Optional. Use %s where the text goes; the engine is asked only after its nickname or in the search field.', 'https://example.com/suggest?q=%s', 1024),
    inputFor('image', 'Image search address', 'Optional. Use %s where the address of the image goes.', 'https://example.com/lens?url=%s', 1024),
    inputFor('imagePost', 'Image POST parameters', 'Optional. Only with an image search address that takes a form post.', 'img=%s', 512),
    problem,
    actions,
  );
  const openForm = (engine: EngineView | null): void => {
    editing = engine?.id ?? null;
    title.textContent = engine ? `Edit ${engine.name}` : 'Add a search engine';
    const draft = engine ? draftOfEngine(engine) : EMPTY_ENGINE;
    for (const [key, input] of fields) input.value = draft[key];
    problem.textContent = '';
    form.hidden = false;
    fields.get('name')?.focus();
    form.scrollIntoView({ block: 'nearest' });
  };
  const closeForm = (): void => {
    form.hidden = true;
    editing = null;
    problem.textContent = '';
  };
  const submit = (): void => {
    const snapshot = current();
    if (!snapshot) return;
    const draft: EngineDraft = { ...EMPTY_ENGINE };
    for (const [key, input] of fields) draft[key] = input.value;
    const why = engineProblem(draft, snapshot.engines, editing);
    if (why) { problem.textContent = why; return; }
    const change: EngineOp = editing === null ? { op: 'add', draft } : { op: 'update', id: editing, draft };
    const sent = engineFields(change);
    if (sent && host.send(sent)) closeForm();
  };
  form.addEventListener('submit', (event) => { event.preventDefault(); submit(); });
  form.addEventListener('keydown', (event) => { if (event.key === 'Escape') { event.stopPropagation(); closeForm(); } });

  const add = textButton('Add a search engine', () => openForm(null), { icon: 'plus' });
  const restore = textButton('Restore default engines', () => { const sent = engineFields({ op: 'restore' }); if (sent) host.send(sent); }, { icon: 'reset', subtle: true });
  const bar = el('div', 'ad-engine-bar');
  bar.append(add, restore);
  const rowNode = host.row(into, 'engines', undefined, { stack: true });
  rowNode.root.append(list, form, bar);

  return {
    paint(snapshot) {
      list.replaceChildren(...snapshot.engines.map((engine, index) => engineItem(engine, index, snapshot, host, openForm)));
      add.disabled = snapshot.engines.length >= MAX_ENGINES;
    },
  };
}

function engineItem(engine: EngineView, index: number, snapshot: AddressSnapshot, host: AddressHost, edit: (engine: EngineView) => void): HTMLElement {
  const item = el('li', 'ad-engine');
  item.dataset.engine = engine.id;
  const tile = el('span', 'ad-tile');
  tile.setAttribute('aria-hidden', 'true');
  tile.textContent = monogram(engine.name);
  const text = el('span', 'sp-text');
  const name = el('span', 'sp-label');
  name.textContent = engineLabel(engine, snapshot.s.search_show_nickname); // --- wave-1 fixes ---
  const detail = el('span', 'sp-desc ad-engine-url');
  detail.dir = 'ltr';
  detail.textContent = engine.url.replace(/^https:\/\//, '');
  text.append(name, detail);
  const badges = el('span', 'ad-badges');
  const badge = (label: string): void => { const b = el('span', 'sp-pill'); b.textContent = label; badges.append(b); };
  if (engine.id === snapshot.def) badge('Default');
  if (engine.id === snapshot.priv) badge('Private');
  if (engine.id === snapshot.img) badge('Images');
  const send = (change: EngineOp): void => { const fields = engineFields(change); if (fields) host.send(fields); };
  const tools = el('span', 'ad-tools');
  tools.append(
    iconButton('chevron-up', `Move ${engine.name} up`, () => send({ op: 'move', id: engine.id, index: index - 1 })),
    iconButton('chevron-down', `Move ${engine.name} down`, () => send({ op: 'move', id: engine.id, index: index + 1 })),
    iconButton('edit', `Edit ${engine.name}`, () => edit(engine)),
  );
  const [up, down] = [tools.children[0] as HTMLButtonElement, tools.children[1] as HTMLButtonElement];
  up.disabled = index === 0;
  down.disabled = index === snapshot.engines.length - 1;
  const remove = iconButton('trash', `Remove ${engine.name}`, () => send({ op: 'remove', id: engine.id }));
  remove.disabled = engine.id === snapshot.def || snapshot.engines.length <= 1;
  tools.append(remove);
  item.append(tile, text, badges, tools);
  return item;
}
