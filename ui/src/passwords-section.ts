// The settings page's "Passwords and autofill" section (bowser://settings/passwords). Rust sends
// `{t:"snap",p:"passwords"}` (src/chrome_host/passwords_page.rs); this module draws it and sends the
// section's `pw-…` and `af-…` commands. Words and checks live in passwords-logic.ts. Text from Rust
// reaches the DOM through textContent only.
//
// Secrets: a password is shown only after "Show", arrives once in the next snapshot, and is cleared
// from the row again when it is hidden, after 30 seconds, or when the page is hidden. Copying happens
// in Bowser, which clears the clipboard after 30 seconds.
import { el, icon, type IconName } from './dom';
import { iconButton, textButton } from './panels/kit';
import type { PanelFields, SnapshotMsg } from './protocol';
import {
  ADDRESS_KEYS, ADDRESS_LABELS, DEFAULT_GENERATOR, GENERATOR_MAX, GENERATOR_MIN, PROVIDERS, STRENGTH_LABELS, ackToast, cardBrand, cardExpired,
  cardExpiry, charKinds, cmd, filterLogins, formatCardNumber, healthText, importSummary, luhnValid, monogram, parsePasswordsSnapshot, plural,
  saveAddress, saveCard, saveLogin, strength, type Address, type AddressKey, type GeneratorOptions, type Login, type LoginFilter,
  type PasswordsSnapshot, type Prefs, type SavedCard,
} from './passwords-logic';

export interface PasswordsSectionOptions {
  readonly send: (fields: PanelFields) => boolean;
  readonly toast: (text: string, ok: boolean) => void;
  readonly switchControl: (onChange: (on: boolean) => void) => { readonly root: HTMLElement; set(value: boolean): void; disable(off: boolean): void };
}

export interface PasswordsSection {
  readonly root: HTMLElement;
  receive(message: SnapshotMsg): void;
}

const REVEAL_MS = 30_000;
const reduceMotion = (): boolean => matchMedia('(prefers-reduced-motion: reduce)').matches;

/** A group: heading (with optional tools on the right) above a card. */
function group(title: string, id: string): { root: HTMLElement; head: HTMLElement; tools: HTMLElement; card: HTMLElement } {
  const root = el('div', 'sp-group pw-group');
  const head = el('div', 'pw-head');
  const h = el('h3', 'sp-group-h');
  h.id = `pw-h-${id}`;
  h.textContent = title;
  const tools = el('div', 'pw-tools');
  head.append(h, tools);
  const card = el('div', 'sp-card pw-card');
  card.setAttribute('role', 'group');
  card.setAttribute('aria-labelledby', h.id);
  root.append(head, card);
  return { root, head, tools, card };
}

/** A labelled text field for the inline editors. */
function field(label: string, value: string, options: { type?: string; mono?: boolean; autocomplete?: string; wide?: boolean; max?: number } = {}): { root: HTMLElement; input: HTMLInputElement } {
  const root = el('label', `pw-field${options.wide ? ' is-wide' : ''}`);
  const name = el('span', 'pw-field-label');
  name.textContent = label;
  const input = el('input', `bw-input${options.mono ? ' pw-mono' : ''}`);
  input.type = options.type ?? 'text';
  input.value = value;
  input.spellcheck = false;
  input.autocomplete = (options.autocomplete ?? 'off') as AutoFill;
  if (options.max) input.maxLength = options.max;
  root.append(name, input);
  return { root, input };
}

/** An icon button whose glyph swaps to a check for a moment after it worked (copy, save). */
function confirmButton(name: IconName, label: string, onClick: () => boolean): HTMLButtonElement {
  const button = iconButton(name, label, () => {
    if (!onClick()) return;
    button.classList.remove('is-done');
    void button.offsetWidth;
    button.classList.add('is-done');
    window.setTimeout(() => button.classList.remove('is-done'), 1400);
  }, 'pw-act pw-confirm');
  const done = icon('check');
  done.classList.add('pw-done-glyph');
  button.append(done);
  return button;
}

/** The four-step strength meter with its label. */
function meter(): { root: HTMLElement; set(score: number | null): void } {
  const root = el('div', 'pw-meter');
  const bars = el('div', 'pw-meter-bars');
  bars.setAttribute('aria-hidden', 'true');
  for (let i = 0; i < 4; i++) bars.append(el('span', 'pw-meter-seg'));
  const label = el('span', 'pw-meter-label');
  root.append(bars, label);
  return {
    root,
    set(score) {
      root.dataset.score = score === null ? '' : String(score);
      label.textContent = score === null ? '' : STRENGTH_LABELS[score] ?? '';
      root.setAttribute('aria-label', score === null ? '' : `Strength: ${STRENGTH_LABELS[score]}`);
    },
  };
}

export function mountPasswordsSection(options: PasswordsSectionOptions): PasswordsSection {
  const { send, toast } = options;
  let snap: PasswordsSnapshot | null = null;
  let filter: LoginFilter = 'all';
  let query = '';
  let searchTimer = 0;
  /** Rows showing their password, and the value Bowser sent for each. */
  const revealed = new Map<number, { value: string; timer: number }>();
  let editing: number | 'new' | null = null;
  let confirmingDelete: number | null = null;
  let addressEditing: number | 'new' | null = null;
  let cardEditing: number | 'new' | null = null;
  let provider = PROVIDERS[0]!.id;
  let exportConfirm = false;
  const gen: GeneratorOptions = { ...DEFAULT_GENERATOR };
  let generated = '';
  let genTimer = 0;
  let genAsked = false;
  /** Open editors, kept across snapshots so typing is never lost to a refresh. */
  const editors = new Map<string, HTMLElement>();
  const editor = (key: string, make: () => HTMLElement): HTMLElement => {
    let node = editors.get(key);
    if (!node) { node = make(); editors.set(key, node); }
    return node;
  };
  const closeEditors = (prefix: string): void => { for (const key of [...editors.keys()]) if (key.startsWith(prefix)) editors.delete(key); };
  /** Runs a re-render and puts focus back where it was if the focused node survived it. */
  const keepFocus = (render: () => void): void => {
    const active = document.activeElement;
    render();
    if (active instanceof HTMLElement && active !== document.body && active.isConnected && document.activeElement !== active) active.focus({ preventScroll: true });
  };
  let lastSig = { logins: '', addr: '', cards: '', prefs: '', move: '' };

  const root = el('div', 'pw');
  root.dataset.row = 'passwords';

  // ---------- unavailable (guest) ----------
  const off = el('div', 'sp-card pw-off');
  off.hidden = true;
  const offText = el('p', 'pw-empty-text');
  off.append(icon('private', { size: 20 }), offText);

  // ---------- saved passwords ----------
  const saved = group('Saved passwords', 'saved');
  const addLogin = textButton('Add', () => { editing = 'new'; confirmingDelete = null; renderLogins(); }, { icon: 'plus', aria: 'Add a password' });
  saved.tools.append(addLogin);
  const bar = el('div', 'pw-bar');
  const searchWrap = el('label', 'pw-search');
  searchWrap.append(icon('search', { size: 14 }));
  const search = el('input', 'pw-search-input');
  search.type = 'search';
  search.placeholder = 'Search sites and usernames';
  search.setAttribute('aria-label', 'Search saved passwords');
  search.autocomplete = 'off';
  search.spellcheck = false;
  searchWrap.append(search);
  const filters = el('div', 'bw-segmented pw-filters');
  filters.setAttribute('role', 'radiogroup');
  filters.setAttribute('aria-label', 'Show');
  const filterButtons = new Map<LoginFilter, HTMLButtonElement>();
  for (const [key, label] of [['all', 'All'], ['weak', 'Weak'], ['reused', 'Reused']] as const) {
    const b = el('button');
    b.type = 'button';
    b.setAttribute('role', 'radio');
    b.dataset.filter = key;
    const text = el('span');
    text.textContent = label;
    const count = el('span', 'pw-count');
    b.append(text, count);
    b.addEventListener('click', () => { filter = key; renderLogins(); });
    filterButtons.set(key, b);
    filters.append(b);
  }
  bar.append(searchWrap, filters);
  const health = el('div', 'pw-health');
  const healthIcon = el('span', 'pw-health-icon');
  healthIcon.append(icon('shield-check', { size: 16 }));
  const healthText_ = el('span', 'pw-health-text');
  health.append(healthIcon, healthText_);
  const list = el('ul', 'pw-list');
  list.setAttribute('aria-label', 'Saved passwords');
  const empty = el('div', 'pw-empty');
  saved.card.append(bar, health, list, empty);

  search.addEventListener('input', () => {
    query = search.value;
    window.clearTimeout(searchTimer);
    // The page holds the whole list unless the vault is larger than one snapshot carries.
    if (snap && snap.total > snap.logins.length) searchTimer = window.setTimeout(() => send(cmd.search(query)), 200);
    else renderLogins();
  });

  const hideSecret = (id: number): void => {
    const shown = revealed.get(id);
    if (shown) window.clearTimeout(shown.timer);
    revealed.delete(id);
  };
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) { for (const id of [...revealed.keys()]) hideSecret(id); renderLogins(); }
  });

  function loginEditor(login: Login | null): HTMLElement {
    const form = el('form', 'pw-editor');
    form.noValidate = true;
    const site = field('Website', login?.origin ?? '', { autocomplete: 'off', wide: true });
    site.input.placeholder = 'https://example.com';
    const user = field('Username or email', login?.user ?? '', { autocomplete: 'off' });
    const pass = field('Password', login && revealed.get(login.id)?.value || '', { type: 'password', mono: true, autocomplete: 'new-password' });
    if (login && !revealed.has(login.id)) {
      pass.input.placeholder = 'Unchanged';
    }
    const strengthMeter = meter();
    const passRow = el('div', 'pw-pass-row');
    const show = iconButton('eye', 'Show password', () => {
      const hidden = pass.input.type === 'password';
      pass.input.type = hidden ? 'text' : 'password';
      show.replaceChildren(icon(hidden ? 'eye-off' : 'eye'));
      show.setAttribute('aria-label', hidden ? 'Hide password' : 'Show password');
    }, 'pw-act');
    const suggest = iconButton('sparkle', 'Use a strong password', () => {
      if (generated) {
        pass.input.value = generated;
        pass.input.type = 'text';
        pass.input.dispatchEvent(new Event('input'));
        pass.input.classList.remove('is-flash');
        void pass.input.offsetWidth;
        pass.input.classList.add('is-flash');
        regenerate();
      }
    }, 'pw-act pw-sparkle');
    passRow.append(pass.root, show, suggest);
    pass.input.addEventListener('input', () => strengthMeter.set(pass.input.value ? strength(pass.input.value) : null));
    strengthMeter.set(pass.input.value ? strength(pass.input.value) : null);
    const actions = el('div', 'pw-editor-actions');
    const cancel = textButton('Cancel', () => { editing = null; renderLogins(); }, { subtle: true });
    const save = textButton(login ? 'Save changes' : 'Save password', () => form.requestSubmit(), { primary: true });
    actions.append(cancel, save);
    form.append(site.root, user.root, passRow, strengthMeter.root, actions);
    form.addEventListener('submit', (event) => {
      event.preventDefault();
      // An edit that leaves the password blank keeps it: Bowser needs it to rewrite the entry, so
      // the row asks for it first.
      const password = pass.input.value || (login ? revealed.get(login.id)?.value ?? '' : '');
      if (login && !password) {
        send(cmd.reveal(login.id));
        toast('Showing the saved password first. Then save again.', false);
        return;
      }
      const fields = saveLogin(login ? login.id : null, site.input.value, user.input.value, password);
      if (!fields) { toast('Fill in the website, username and password.', false); return; }
      if (send(fields) && login) hideSecret(login.id);
    });
    form.addEventListener('keydown', (event) => { if (event.key === 'Escape') { event.stopPropagation(); editing = null; renderLogins(); } });
    window.requestAnimationFrame(() => (login ? user.input : site.input).focus({ preventScroll: true }));
    return form;
  }

  function loginRow(login: Login, index: number): HTMLLIElement {
    const li = el('li', 'pw-item');
    li.style.setProperty('--i', String(Math.min(index, 12)));
    const tile = el('span', 'pw-tile');
    tile.textContent = monogram(login.host);
    tile.setAttribute('aria-hidden', 'true');
    const who = el('div', 'pw-who');
    const host = el('span', 'pw-host');
    host.textContent = login.host;
    const user = el('span', 'pw-user');
    user.textContent = login.user;
    const meta = el('span', 'pw-meta');
    meta.append(user);
    who.append(host, meta);
    const secret = el('span', 'pw-secret pw-mono');
    const shown = revealed.get(login.id);
    secret.textContent = shown ? shown.value : '•'.repeat(10);
    secret.classList.toggle('is-shown', !!shown);
    const badges = el('span', 'pw-badges');
    if (login.score < 2) {
      const b = el('span', 'pw-badge is-warn');
      b.append(icon('warning', { size: 12, fine: true }));
      b.append(document.createTextNode('Weak'));
      badges.append(b);
    }
    if (login.reused) {
      const b = el('span', 'pw-badge');
      b.append(icon('chain', { size: 12, fine: true }));
      b.append(document.createTextNode('Reused'));
      badges.append(b);
    }
    if (badges.childElementCount) meta.append(badges);
    const actions = el('div', 'pw-actions');
    if (confirmingDelete === login.id) {
      li.classList.add('is-confirming');
      const ask = el('span', 'pw-ask');
      ask.textContent = 'Delete this password?';
      const yes = textButton('Delete', () => { if (send(cmd.remove(login.id))) { li.classList.add('bw-exit'); confirmingDelete = null; } }, { primary: true });
      yes.classList.add('is-danger');
      const no = textButton('Keep', () => { confirmingDelete = null; renderLogins(); }, { subtle: true });
      actions.append(ask, no, yes);
      window.requestAnimationFrame(() => no.focus({ preventScroll: true }));
    } else {
      const eye = iconButton(shown ? 'eye-off' : 'eye', shown ? 'Hide password' : 'Show password', () => {
        if (revealed.has(login.id)) { hideSecret(login.id); renderLogins(); } else send(cmd.reveal(login.id));
      }, 'pw-act');
      eye.setAttribute('aria-pressed', String(!!shown));
      actions.append(
        eye,
        confirmButton('person', 'Copy username', () => send(cmd.copy(login.id, false))),
        confirmButton('copy', 'Copy password', () => send(cmd.copy(login.id, true))),
        iconButton('edit', 'Edit', () => { editing = login.id; confirmingDelete = null; renderLogins(); }, 'pw-act'),
        iconButton('trash', 'Delete', () => { confirmingDelete = login.id; renderLogins(); }, 'pw-act pw-danger'),
      );
    }
    const main = el('div', 'pw-row');
    main.append(tile, who, secret, actions);
    li.append(main);
    if (editing === login.id) li.append(editor(`login:${login.id}`, () => loginEditor(login)));
    return li;
  }

  function renderLogins(): void {
    if (!snap) return;
    for (const key of [...editors.keys()]) if (key.startsWith('login:') && key !== `login:${String(editing)}`) editors.delete(key);
    const all = snap.logins;
    for (const [key, b] of filterButtons) {
      b.setAttribute('aria-checked', String(filter === key));
      const n = key === 'weak' ? all.filter((l) => l.score < 2).length : key === 'reused' ? all.filter((l) => l.reused).length : all.length;
      (b.querySelector('.pw-count') as HTMLElement).textContent = n ? String(n) : '';
    }
    const weak = all.some((l) => l.score < 2 || l.reused);
    health.classList.toggle('is-warn', weak);
    healthIcon.replaceChildren(icon(weak ? 'warning' : 'shield-check', { size: 16 }));
    healthText_.textContent = healthText(all) + (snap.total > all.length ? ` (showing ${all.length} of ${snap.total})` : '');
    const shownLogins = filterLogins(all, snap.total > all.length ? '' : query, filter);
    const rows: HTMLElement[] = [];
    if (editing === 'new') {
      const li = el('li', 'pw-item is-new');
      li.append(editor('login:new', () => loginEditor(null)));
      rows.push(li);
    }
    shownLogins.forEach((login, index) => rows.push(loginRow(login, index)));
    list.replaceChildren(...rows);
    empty.replaceChildren();
    empty.hidden = shownLogins.length > 0 || editing === 'new';
    if (!empty.hidden) {
      const art = el('span', 'pw-empty-art');
      art.append(icon(all.length ? 'search' : 'key', { size: 24 }));
      const text = el('p', 'pw-empty-text');
      text.textContent = all.length ? 'Nothing matches.' : 'Passwords you save while you browse appear here.';
      empty.append(art, text);
      if (!all.length) empty.append(textButton('Bring passwords from another app', () => moveGroup.root.scrollIntoView({ behavior: reduceMotion() ? 'auto' : 'smooth', block: 'start' }), { icon: 'import', subtle: true }));
    }
  }

  // ---------- generator ----------
  const genGroup = group('Password generator', 'gen');
  const genOut = el('div', 'pw-gen-out pw-mono');
  genOut.setAttribute('role', 'status');
  genOut.setAttribute('aria-live', 'polite');
  genOut.setAttribute('aria-label', 'Generated password');
  const genTools = el('div', 'pw-gen-tools');
  const regen = iconButton('reload', 'Generate another', () => regenerate(true), 'pw-act pw-regen');
  const copyGen = confirmButton('copy', 'Copy password', () => send(cmd.copyGenerated()));
  genTools.append(regen, copyGen);
  const genTop = el('div', 'pw-gen-top');
  genTop.append(genOut, genTools);
  const genMeter = meter();
  const lengthRow = el('div', 'pw-gen-length');
  const lengthLabel = el('label', 'pw-field-label');
  lengthLabel.textContent = 'Length';
  lengthLabel.htmlFor = 'pw-gen-len';
  const lengthInput = el('input', 'sp-range pw-range');
  lengthInput.type = 'range';
  lengthInput.id = 'pw-gen-len';
  lengthInput.min = String(GENERATOR_MIN);
  lengthInput.max = String(GENERATOR_MAX);
  lengthInput.value = String(gen.length);
  const lengthValue = el('output', 'pw-gen-len-value');
  lengthValue.textContent = String(gen.length);
  lengthRow.append(lengthLabel, lengthInput, lengthValue);
  const updateRange = (): void => {
    const t = (gen.length - GENERATOR_MIN) / (GENERATOR_MAX - GENERATOR_MIN);
    lengthInput.style.setProperty('--sp-fill', `${Math.round(t * 100)}%`);
  };
  updateRange();
  lengthInput.addEventListener('input', () => {
    gen.length = Number(lengthInput.value);
    lengthValue.textContent = lengthInput.value;
    updateRange();
    regenerate();
  });
  const chips = el('div', 'pw-chips');
  const chipSpec: ReadonlyArray<readonly [keyof GeneratorOptions, string, string]> = [
    ['upper', 'A–Z', 'Capital letters'], ['lower', 'a–z', 'Small letters'], ['digits', '0–9', 'Numbers'],
    ['symbols', '!#$', 'Symbols'], ['avoidAmbiguous', 'No look-alikes', 'Leave out characters that are easy to misread, such as l, 1, O and 0'],
  ];
  for (const [key, label, aria] of chipSpec) {
    const chip = el('button', 'pw-chip');
    chip.type = 'button';
    chip.setAttribute('aria-pressed', String(gen[key]));
    chip.setAttribute('aria-label', aria);
    chip.dataset.tip = aria;
    chip.append(icon('check', { size: 12, fine: true }), document.createTextNode(label));
    chip.addEventListener('click', () => {
      const next = !gen[key];
      const classes = (['upper', 'lower', 'digits', 'symbols'] as const).filter((k) => (k === key ? next : gen[k]));
      if (key !== 'avoidAmbiguous' && classes.length === 0) { chip.classList.remove('is-refused'); void chip.offsetWidth; chip.classList.add('is-refused'); return; }
      (gen[key] as boolean) = next;
      chip.setAttribute('aria-pressed', String(next));
      regenerate();
    });
    chips.append(chip);
  }
  genGroup.card.append(genTop, genMeter.root, lengthRow, chips);

  function regenerate(now = false): void {
    window.clearTimeout(genTimer);
    const go = (): void => { if (send(cmd.generate(gen))) { genAsked = true; regen.classList.remove('is-spinning'); void regen.offsetWidth; regen.classList.add('is-spinning'); } };
    if (now) go(); else genTimer = window.setTimeout(go, 120);
  }

  function showGenerated(value: string): void {
    generated = value;
    const spans = charKinds(value).map((kind, i) => {
      const s = el('span', `pw-ch is-${kind}`);
      s.textContent = value[i] ?? '';
      s.style.setProperty('--i', String(i));
      return s;
    });
    genOut.replaceChildren(...spans);
    genOut.setAttribute('aria-label', `Generated password, ${value.length} characters`);
    genMeter.set(strength(value));
  }

  // ---------- moving from another app ----------
  const moveGroup = group('Move from another app', 'move');
  const providerRow = el('div', 'pw-providers');
  providerRow.setAttribute('role', 'radiogroup');
  providerRow.setAttribute('aria-label', 'Where your passwords are now');
  const steps = el('ol', 'pw-steps');
  const providerButtons: HTMLButtonElement[] = [];
  for (const p of PROVIDERS) {
    const b = el('button', 'pw-provider');
    b.type = 'button';
    b.setAttribute('role', 'radio');
    b.textContent = p.name;
    b.addEventListener('click', () => { provider = p.id; renderSteps(); });
    providerButtons.push(b);
    providerRow.append(b);
  }
  const renderSteps = (): void => {
    const p = PROVIDERS.find((x) => x.id === provider) ?? PROVIDERS[0]!;
    providerButtons.forEach((b, i) => b.setAttribute('aria-checked', String(PROVIDERS[i]!.id === p.id)));
    steps.replaceChildren(...p.steps.map((text, i) => {
      const li = el('li', 'pw-step');
      li.style.setProperty('--i', String(i));
      const n = el('span', 'pw-step-n');
      n.textContent = String(i + 1);
      const t = el('span');
      t.textContent = text;
      li.append(n, t);
      return li;
    }));
  };
  renderSteps();
  const importRow = el('div', 'pw-import-row');
  const choose = textButton('Choose CSV file…', () => send(cmd.importFile()), { icon: 'import', primary: true });
  const exportButton = textButton('Export passwords…', () => { exportConfirm = true; renderMove(); }, { icon: 'export', subtle: true });
  importRow.append(choose, exportButton);
  const importPanel = el('div', 'pw-import');
  importPanel.setAttribute('aria-live', 'polite');
  const note = el('p', 'pw-note');
  note.append(icon('info', { size: 14, fine: true }));
  note.append(document.createTextNode('Export files hold passwords as plain text. Delete the file once the import is done. Passkeys stay in Windows Hello.'));
  moveGroup.card.append(providerRow, steps, importRow, importPanel, note);

  function renderMove(): void {
    const state = snap?.importState ?? null;
    importPanel.replaceChildren();
    importPanel.className = 'pw-import';
    if (exportConfirm) {
      importPanel.classList.add('is-open', 'is-warn');
      const t = el('div', 'pw-import-text');
      const h = el('strong');
      h.textContent = 'Export every saved password?';
      const p = el('span');
      p.textContent = 'The file is not encrypted. Anyone who can open it can read your passwords.';
      t.append(h, p);
      const acts = el('div', 'pw-import-acts');
      acts.append(
        textButton('Cancel', () => { exportConfirm = false; renderMove(); }, { subtle: true }),
        textButton('Export', () => { exportConfirm = false; send(cmd.exportFile()); renderMove(); }, { primary: true, icon: 'export' }),
      );
      importPanel.append(icon('warning', { size: 20 }), t, acts);
      return;
    }
    if (!state) return;
    importPanel.classList.add('is-open');
    const t = el('div', 'pw-import-text');
    const h = el('strong');
    const p = el('span');
    if (state.stage === 'preview') {
      h.textContent = `${state.provider} export`;
      p.textContent = importSummary(state);
      t.append(h, p);
      const acts = el('div', 'pw-import-acts');
      const go = textButton(state.fresh ? `Import ${plural(state.fresh, 'password')}` : 'Nothing new to import', () => send(cmd.importConfirm()), { primary: true });
      go.disabled = state.fresh === 0;
      acts.append(textButton('Cancel', () => send(cmd.importCancel()), { subtle: true }), go);
      importPanel.append(icon('file-text', { size: 20 }), t, acts);
    } else {
      importPanel.classList.add('is-done');
      h.textContent = 'All done';
      p.textContent = importSummary(state);
      t.append(h, p);
      const mark = el('span', 'pw-done-mark');
      mark.append(icon('check', { size: 16 }));
      importPanel.append(mark, t);
    }
  }

  // ---------- addresses ----------
  const addrGroup = group('Addresses', 'addr');
  addrGroup.tools.append(textButton('Add', () => { addressEditing = 'new'; renderAddresses(); }, { icon: 'plus', aria: 'Add an address' }));
  const addrList = el('ul', 'pw-list pw-addr-list');
  addrGroup.card.append(addrList);

  function addressEditor(address: Address | null): HTMLElement {
    const form = el('form', 'pw-editor pw-grid');
    form.noValidate = true;
    const inputs = new Map<AddressKey, HTMLInputElement>();
    const auto: Readonly<Record<AddressKey, string>> = { name: 'name', organization: 'organization', email: 'email', tel: 'tel', line1: 'address-line1', line2: 'address-line2', city: 'address-level2', region: 'address-level1', postal: 'postal-code', country: 'country-name' };
    for (const key of ADDRESS_KEYS) {
      const f = field(ADDRESS_LABELS[key], address ? address[key === 'postal' ? 'postal' : key] : '', { type: key === 'email' ? 'email' : key === 'tel' ? 'tel' : 'text', autocomplete: auto[key], wide: key === 'line1' || key === 'line2', max: 200 });
      inputs.set(key, f.input);
      form.append(f.root);
    }
    const actions = el('div', 'pw-editor-actions is-wide');
    actions.append(
      textButton('Cancel', () => { addressEditing = null; renderAddresses(); }, { subtle: true }),
      textButton('Save address', () => form.requestSubmit(), { primary: true }),
    );
    form.append(actions);
    form.addEventListener('submit', (event) => {
      event.preventDefault();
      const values: Partial<Record<AddressKey, string>> = {};
      for (const [k, input] of inputs) values[k] = input.value;
      const fields = saveAddress(address ? address.id : null, values);
      if (!fields) { toast('Fill in at least one field.', false); return; }
      send(fields);
    });
    form.addEventListener('keydown', (event) => { if (event.key === 'Escape') { event.stopPropagation(); addressEditing = null; renderAddresses(); } });
    window.requestAnimationFrame(() => inputs.get('name')?.focus({ preventScroll: true }));
    return form;
  }

  function renderAddresses(): void {
    if (!snap) return;
    for (const key of [...editors.keys()]) if (key.startsWith('addr:') && key !== `addr:${String(addressEditing)}`) editors.delete(key);
    const rows: HTMLElement[] = [];
    if (addressEditing === 'new') { const li = el('li', 'pw-item is-new'); li.append(editor('addr:new', () => addressEditor(null))); rows.push(li); }
    snap.addresses.forEach((a, index) => {
      const li = el('li', 'pw-item');
      li.style.setProperty('--i', String(Math.min(index, 12)));
      const row = el('div', 'pw-row');
      const tile = el('span', 'pw-tile is-icon');
      tile.append(icon('contact', { size: 16 }));
      const who = el('div', 'pw-who');
      const name = el('span', 'pw-host');
      name.textContent = a.name || a.organization || a.email || 'Address';
      const meta = el('span', 'pw-user');
      meta.textContent = [a.line1, a.city, a.postal, a.country].filter(Boolean).join(', ') || [a.email, a.tel].filter(Boolean).join(' · ');
      who.append(name, meta);
      const actions = el('div', 'pw-actions');
      actions.append(
        iconButton('edit', 'Edit address', () => { addressEditing = a.id; renderAddresses(); }, 'pw-act'),
        iconButton('trash', 'Delete address', () => { if (send(cmd.deleteAddress(a.id))) li.classList.add('bw-exit'); }, 'pw-act pw-danger'),
      );
      row.append(tile, who, actions);
      li.append(row);
      if (addressEditing === a.id) li.append(editor(`addr:${a.id}`, () => addressEditor(a)));
      rows.push(li);
    });
    if (rows.length === 0) {
      const li = el('li', 'pw-empty is-inline');
      const t = el('p', 'pw-empty-text');
      t.textContent = 'Save an address to fill delivery and sign-up forms in one step.';
      li.append(t);
      rows.push(li);
    }
    addrList.replaceChildren(...rows);
  }

  // ---------- payment cards ----------
  const cardGroup = group('Payment methods', 'cards');
  cardGroup.tools.append(textButton('Add', () => { cardEditing = 'new'; renderCards(); }, { icon: 'plus', aria: 'Add a card' }));
  const cardList = el('div', 'pw-cc-list');
  const cardEditorHost = el('div', 'pw-cc-editor-host');
  const cardNote = el('p', 'pw-note');
  cardNote.append(icon('lock', { size: 14, fine: true }));
  cardNote.append(document.createTextNode('Card numbers are encrypted for your Windows account. Security codes are never saved.'));
  cardGroup.card.append(cardList, cardEditorHost, cardNote);

  function cardArt(card: { brand: string; last4: string; expiry: string; name: string; nick?: string }, live = false): HTMLElement {
    const art = el('div', `pw-cc${live ? ' is-live' : ''}`);
    art.dataset.brand = card.brand.toLowerCase().replace(/\s+/g, '-');
    const brand = el('span', 'pw-cc-brand');
    brand.textContent = card.brand || 'Card';
    const chip = el('span', 'pw-cc-chip');
    chip.setAttribute('aria-hidden', 'true');
    const number = el('span', 'pw-cc-number pw-mono');
    number.textContent = `•••• ${card.last4 || '••••'}`;
    const foot = el('span', 'pw-cc-foot');
    const name = el('span', 'pw-cc-name');
    name.textContent = card.nick || card.name || ' ';
    const exp = el('span', 'pw-cc-exp pw-mono');
    exp.textContent = card.expiry;
    foot.append(name, exp);
    art.append(brand, chip, number, foot);
    return art;
  }

  function cardEditor(card: SavedCard | null): HTMLElement {
    const form = el('form', 'pw-editor pw-cc-editor');
    form.noValidate = true;
    const preview = cardArt({ brand: card?.brand ?? '', last4: card?.last4 ?? '', expiry: card ? cardExpiry(card) : '', name: card?.name ?? '' }, true);
    const fields = el('div', 'pw-grid');
    const number = field('Card number', '', { mono: true, autocomplete: 'cc-number', wide: true, max: 23 });
    number.input.inputMode = 'numeric';
    number.input.placeholder = card ? `Unchanged (•••• ${card.last4})` : '1234 5678 9012 3456';
    const valid = el('span', 'pw-valid');
    valid.setAttribute('aria-live', 'polite');
    number.root.append(valid);
    const name = field('Name on card', card?.name ?? '', { autocomplete: 'cc-name', wide: true, max: 100 });
    const month = field('Month', card?.month ? String(card.month).padStart(2, '0') : '', { autocomplete: 'cc-exp-month', max: 2 });
    month.input.inputMode = 'numeric';
    month.input.placeholder = 'MM';
    const year = field('Year', card?.year ? String(card.year) : '', { autocomplete: 'cc-exp-year', max: 4 });
    year.input.inputMode = 'numeric';
    year.input.placeholder = 'YYYY';
    const nick = field('Nickname (optional)', card?.nick ?? '', { wide: true, max: 60 });
    fields.append(number.root, name.root, month.root, year.root, nick.root);
    const update = (): void => {
      const digits = number.input.value.replace(/\D/g, '');
      const brand = digits ? cardBrand(digits) : card?.brand ?? '';
      (preview.querySelector('.pw-cc-brand') as HTMLElement).textContent = brand || 'Card';
      preview.dataset.brand = brand.toLowerCase().replace(/\s+/g, '-');
      (preview.querySelector('.pw-cc-number') as HTMLElement).textContent = digits ? formatCardNumber(digits) : `•••• ${card?.last4 ?? '••••'}`;
      (preview.querySelector('.pw-cc-name') as HTMLElement).textContent = nick.input.value || name.input.value || ' ';
      const m = month.input.value.padStart(2, '0');
      const y = year.input.value.slice(-2);
      (preview.querySelector('.pw-cc-exp') as HTMLElement).textContent = month.input.value && year.input.value ? `${m}/${y}` : '';
      const ok = digits.length >= 12 && luhnValid(digits);
      valid.className = `pw-valid${digits.length >= 12 ? (ok ? ' is-ok' : ' is-bad') : ''}`;
      valid.replaceChildren();
      if (digits.length >= 12) { valid.append(icon(ok ? 'check' : 'warning', { size: 12, fine: true }), document.createTextNode(ok ? brand : 'Check the number')); }
    };
    number.input.addEventListener('input', () => {
      const caretAtEnd = number.input.selectionStart === number.input.value.length;
      number.input.value = formatCardNumber(number.input.value);
      if (caretAtEnd) number.input.setSelectionRange(number.input.value.length, number.input.value.length);
      update();
    });
    for (const f of [name, month, year, nick]) f.input.addEventListener('input', update);
    const actions = el('div', 'pw-editor-actions');
    actions.append(
      textButton('Cancel', () => { cardEditing = null; renderCards(); }, { subtle: true }),
      textButton('Save card', () => form.requestSubmit(), { primary: true }),
    );
    const side = el('div', 'pw-cc-form');
    side.append(fields, actions);
    form.append(preview, side);
    form.addEventListener('submit', (event) => {
      event.preventDefault();
      const command = saveCard(card ? card.id : null, { name: name.input.value, number: number.input.value, month: month.input.value, year: year.input.value, nick: nick.input.value });
      if (!command) { toast('Check the card number.', false); number.input.focus(); return; }
      send(command);
    });
    form.addEventListener('keydown', (event) => { if (event.key === 'Escape') { event.stopPropagation(); cardEditing = null; renderCards(); } });
    update();
    window.requestAnimationFrame(() => (card ? name.input : number.input).focus({ preventScroll: true }));
    return form;
  }

  function renderCards(): void {
    if (!snap) return;
    for (const key of [...editors.keys()]) if (key.startsWith('card:') && key !== `card:${String(cardEditing)}`) editors.delete(key);
    const now = new Date();
    const tiles = snap.cards.map((c, index) => {
      const wrap = el('div', 'pw-cc-item');
      wrap.style.setProperty('--i', String(Math.min(index, 12)));
      const art = cardArt({ brand: c.brand, last4: c.last4, expiry: cardExpiry(c), name: c.name, nick: c.nick });
      if (cardExpired(c, now)) {
        art.classList.add('is-expired');
        const flag = el('span', 'pw-badge is-warn pw-cc-flag');
        flag.textContent = 'Expired';
        art.append(flag);
      }
      const actions = el('div', 'pw-actions');
      actions.append(
        iconButton('edit', `Edit ${c.brand} ending ${c.last4}`, () => { cardEditing = c.id; renderCards(); }, 'pw-act'),
        iconButton('trash', `Delete ${c.brand} ending ${c.last4}`, () => { if (send(cmd.deleteCard(c.id))) wrap.classList.add('bw-exit'); }, 'pw-act pw-danger'),
      );
      wrap.append(art, actions);
      return wrap;
    });
    if (tiles.length === 0 && cardEditing === null) {
      const t = el('p', 'pw-empty-text pw-cc-empty');
      t.textContent = 'Save a card at checkout, or add one here, to pay in one step.';
      tiles.push(t);
    }
    cardList.replaceChildren(...tiles);
    const editingCard = typeof cardEditing === 'number' ? snap.cards.find((c) => c.id === cardEditing) ?? null : null;
    cardEditorHost.replaceChildren(...(cardEditing !== null ? [editor(`card:${String(cardEditing)}`, () => cardEditor(editingCard))] : []));
  }

  // ---------- settings ----------
  const prefsGroup = group('Autofill and saving', 'prefs');
  const prefRows: Array<[keyof Prefs, string, string, IconName]> = [
    ['save', 'Offer to save passwords', 'Ask after you sign in or sign up on a site', 'key'],
    ['addresses', 'Save and fill addresses', 'Names, addresses, email and phone numbers', 'contact'],
    ['cards', 'Save and fill payment methods', 'Card number, name and expiry date', 'card'],
  ];
  const switches = new Map<keyof Prefs, ReturnType<PasswordsSectionOptions['switchControl']>>();
  for (const [key, label, desc, glyph] of prefRows) {
    const row = el('div', 'sp-row pw-pref');
    const tile = el('span', 'pw-tile is-icon');
    tile.append(icon(glyph, { size: 16 }));
    const text = el('div', 'sp-text');
    const l = el('span', 'sp-label');
    l.id = `pw-pref-${key}`;
    l.textContent = label;
    const d = el('span', 'sp-desc');
    d.textContent = desc;
    text.append(l, d);
    const control = options.switchControl((on) => send(cmd.pref(key, on)));
    control.root.setAttribute('aria-labelledby', l.id);
    const ctl = el('div', 'sp-ctl');
    ctl.append(control.root);
    row.append(tile, text, ctl);
    prefsGroup.card.append(row);
    switches.set(key, control);
  }
  const shortcut = el('div', 'sp-row pw-pref');
  const shortcutTile = el('span', 'pw-tile is-icon');
  shortcutTile.append(icon('keyboard', { size: 16 }));
  const shortcutText = el('div', 'sp-text');
  const sl = el('span', 'sp-label');
  sl.textContent = 'Fill from the keyboard';
  const sd = el('span', 'sp-desc');
  sd.textContent = 'In a sign-in, address or card field, press Alt+Down (or Down in an empty field), or select the small button inside the field';
  shortcutText.append(sl, sd);
  shortcut.append(shortcutTile, shortcutText);
  prefsGroup.card.append(shortcut);
  const neverBox = el('div', 'pw-never');
  prefsGroup.card.append(neverBox);

  function renderPrefs(): void {
    if (!snap) return;
    for (const [key, control] of switches) { control.set(snap.prefs[key]); control.disable(!snap.available); }
    neverBox.replaceChildren();
    neverBox.hidden = snap.never.length === 0;
    if (!snap.never.length) return;
    const h = el('div', 'pw-never-h');
    h.textContent = 'Never saved for';
    const ul = el('ul', 'pw-never-list');
    for (const origin of snap.never) {
      const li = el('li', 'pw-never-item');
      const name = el('span');
      name.textContent = origin.replace(/^https?:\/\//, '');
      li.append(name, iconButton('close', `Allow saving for ${name.textContent}`, () => { if (send(cmd.forgetNever(origin))) li.classList.add('bw-exit'); }, 'pw-act'));
      ul.append(li);
    }
    neverBox.append(h, ul);
  }

  root.append(off, saved.root, genGroup.root, moveGroup.root, addrGroup.root, cardGroup.root, prefsGroup.root);

  return {
    root,
    receive(message) {
      const next = parsePasswordsSnapshot(message);
      if (!next) return;
      snap = next;
      const enabled = next.available && !next.guest;
      off.hidden = enabled;
      offText.textContent = next.guest ? 'Guest windows never save passwords, addresses or cards.' : 'Saved passwords could not be opened for this profile.';
      for (const g of [saved, genGroup, moveGroup, addrGroup, cardGroup, prefsGroup]) g.root.hidden = !enabled;
      if (next.reveal) {
        const { id, value } = next.reveal;
        hideSecret(id);
        revealed.set(id, { value, timer: window.setTimeout(() => { hideSecret(id); renderLogins(); }, REVEAL_MS) });
      }
      if (next.generated !== null) showGenerated(next.generated);
      else if (!genAsked && enabled) regenerate(true);
      const t = ackToast(next.ack, next.error);
      if (t) toast(t.text, t.ok);
      // A save or delete closes the editor it came from once Bowser has answered.
      if (next.ack === 'saved' || next.ack === 'deleted') { editing = null; addressEditing = null; cardEditing = null; confirmingDelete = null; closeEditors(''); }
      // Only what changed is drawn again, so a generator refresh never disturbs an open editor.
      const sig = {
        logins: JSON.stringify([next.logins, next.total, next.reveal?.id ?? null, next.ack]),
        addr: JSON.stringify([next.addresses, next.ack]),
        cards: JSON.stringify([next.cards, next.ack]),
        prefs: JSON.stringify([next.prefs, next.never, next.available]),
        move: JSON.stringify(next.importState),
      };
      keepFocus(() => {
        if (sig.logins !== lastSig.logins) renderLogins();
        if (sig.move !== lastSig.move) renderMove();
        if (sig.addr !== lastSig.addr) renderAddresses();
        if (sig.cards !== lastSig.cards) renderCards();
        if (sig.prefs !== lastSig.prefs) renderPrefs();
      });
      lastSig = sig;
    },
  };
}
