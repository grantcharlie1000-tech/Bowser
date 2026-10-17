// The Privacy section's blocking, cookie-notice, history and website-appearance controls, drawn for
// the settings page and for the Privacy dashboard window. Rust sends `{t:"snap",p:"protection"}`;
// every change goes back as one `priv` command (protection-logic.ts checks it before it is sent and
// Rust checks it again). Text from Rust reaches the DOM through textContent only.
//
// The module builds the controls and three blocks (sites, lists, delete data); the host decides
// where they go. The settings page puts a control in its own row; `sections()` lays everything out
// as settings-style sections for a host with no rows of its own.
import { el } from '../dom';
import { sendPanel, type Inbound, type PanelFields, type SnapshotMsg } from '../protocol';
import { iconButton, pageSection, segmented, settingRow, textButton, type PageSection } from './kit';
import { selectField, type SelectField } from './select';
import {
  APPEARANCES, APPEARANCE_LABELS, DEFAULT_DELETE_KINDS, DELETE_KINDS, DELETE_RANGES, LEVELS, LEVEL_LABELS, LOGINS_BIT, PRIVATE_THEMES, PRIVATE_THEME_LABELS,
  RETENTIONS, RETENTION_LABELS, SITE_COOKIES, SITE_COOKIE_LABELS, SITE_COOKIE_N, SITE_LEVELS, SITE_LEVEL_LABELS, SITE_LEVEL_N, SOURCE_KINDS, SOURCE_KIND_LABELS,
  blockedText, deleteSummary, kindsBits, levelDescription, listUrlProblem, parseProtectionSnapshot, protectionCommand, siteFromInput, sourceDetail,
  type Appearance, type DeleteKind, type DeleteRangeKey, type Level, type PrivateThemeKey, type ProtectionSnapshot, type Retention, type SiteCookies, type SiteLevel,
  type SourceKind, type SourceRow,
} from './protection-logic';

/** A control the host places in a row: its element, a setter and a way to switch it off. */
export interface ProtectionControl<T> { readonly root: HTMLElement; set(value: T): void; disable(off: boolean): void }

export interface ProtectionOptions {
  /** Sends one `priv` command; false when the bridge refused it. */
  readonly send?: (fields: PanelFields) => boolean;
  /** The host's toast ("Added …", "Deleted …", or what went wrong). */
  readonly toast: (text: string, ok: boolean) => void;
  /** How a titled group of rows looks; the dashboard's plain sections by default. */
  readonly section?: (title: string) => PageSection;
  /** The blocking level changed (so the settings page's own draft stays in step). */
  readonly onLevel?: (level: Level) => void;
}

export interface Protection {
  readonly level: ProtectionControl<Level>;
  readonly cookieNotices: ProtectionControl<boolean>;
  readonly retention: ProtectionControl<Retention>;
  readonly websiteAppearance: ProtectionControl<Appearance>;
  readonly forceDark: ProtectionControl<boolean>;
  readonly privateTheme: ProtectionControl<PrivateThemeKey>;
  /** The block of saved per-site choices. */
  readonly sites: HTMLElement;
  /** The block of rule lists. */
  readonly lists: HTMLElement;
  /** The Delete Browsing Data block. */
  readonly deleteData: HTMLElement;
  /** Every control and block as settings-style sections, for a host with no rows of its own. */
  sections(): HTMLElement[];
  /** Applies a `protection` snapshot; other messages are ignored. */
  receive(message: Inbound): void;
}

function switchControl(label: string, onChange: (on: boolean) => void): ProtectionControl<boolean> {
  const button = el('button', 'bw-toggle');
  button.type = 'button';
  button.setAttribute('role', 'switch');
  button.setAttribute('aria-checked', 'false');
  button.setAttribute('aria-label', label);
  button.addEventListener('click', () => {
    const next = button.getAttribute('aria-checked') !== 'true';
    button.setAttribute('aria-checked', String(next));
    onChange(next);
  });
  return {
    root: button,
    set: (on) => button.setAttribute('aria-checked', String(on)),
    disable: (off) => { button.disabled = off; button.setAttribute('aria-disabled', String(off)); },
  };
}

/** Radio-style segmented control. The group is redrawn only when its value or its availability changes, so a click keeps the keyboard where it is. */
function segmentedControl<T extends string>(label: string, values: readonly T[], labels: Readonly<Record<T, string>>, onSelect: (value: T) => void): ProtectionControl<T> {
  const options = values.map((value) => [value, labels[value]] as const);
  let current: T | null = null;
  let off = false;
  const choose = (value: T): void => { current = value; onSelect(value); };
  const build = (): HTMLElement => {
    const group = segmented(options, current, choose, label);
    group.classList.add('sp-seg');
    group.querySelectorAll('button').forEach((button) => { button.disabled = off; });
    return group;
  };
  let group = build();
  const holder = el('div', 'pg-seg-host');
  holder.append(group);
  const paint = (): void => { const next = build(); group.replaceWith(next); group = next; };
  return {
    root: holder,
    set: (value) => { if (value !== current) { current = value; paint(); } },
    disable: (disabled) => { if (disabled !== off) { off = disabled; paint(); } },
  };
}

function selectControl<T extends string>(label: string, values: readonly T[], labels: Readonly<Record<T, string>>, onChange: (value: T) => void): ProtectionControl<T> {
  const field: SelectField<T> = selectField<T>({ options: values.map((value) => ({ value, label: labels[value] })), value: null, label, placeholder: label, onChange });
  return {
    root: field.root,
    set: (value) => field.set(value),
    disable: (off) => { field.button.toggleAttribute('disabled', off); },
  };
}

/** Rebuilds `container`'s content with `rebuild` and puts the keyboard back on the control (keyed by `data-focus`) that had it, caret included. */
function withFocus(container: HTMLElement, rebuild: () => void): void {
  const active = document.activeElement;
  const key = active instanceof HTMLElement && container.contains(active) ? (active.dataset.focus ?? null) : null;
  const isField = (node: Element | null): node is HTMLInputElement => node !== null && node.tagName.toLowerCase() === 'input';
  const caret = isField(active) ? ([active.selectionStart, active.selectionEnd] as const) : null;
  rebuild();
  if (key === null) return;
  const again = container.querySelector<HTMLElement>(`[data-focus="${key}"]`);
  if (!again || (again as HTMLButtonElement).disabled) return;
  again.focus({ preventScroll: true });
  if (caret && isField(again) && caret[0] !== null && caret[1] !== null) again.setSelectionRange(caret[0], caret[1]);
}

const focusKey = <E extends HTMLElement>(node: E, key: string): E => { node.dataset.focus = key; return node; };

export function mountProtection(options: ProtectionOptions): Protection {
  const send = options.send ?? ((fields: PanelFields): boolean => sendPanel('priv', fields));
  const section = options.section ?? pageSection;
  let snapshot: ProtectionSnapshot | null = null;
  /** The sequence number of the last result seen, so a snapshot that repeats one never toasts twice. */
  let shownSeq: number | null = null;

  const command = (a: string, fields: Omit<PanelFields, 'a'> = {}): void => {
    const out = protectionCommand(a, fields);
    if (out) send(out);
  };

  // ---- the single controls ----
  const level = segmentedControl<Level>('Blocking level', LEVELS, LEVEL_LABELS, (value) => { command('blocklevel', { text: value }); options.onLevel?.(value); });
  const cookieNotices = switchControl('Hide cookie notices', (on) => command('cookienotices', { on }));
  const retention = selectControl<Retention>('Keep history for', RETENTIONS, RETENTION_LABELS, (value) => command('retention', { text: value }));
  const websiteAppearance = segmentedControl<Appearance>('Website appearance', APPEARANCES, APPEARANCE_LABELS, (value) => command('webappearance', { text: value }));
  const forceDark = switchControl('Force dark theme on all websites', (on) => command('forcedark', { on }));
  const privateTheme = segmentedControl<PrivateThemeKey>('Private tab theme', PRIVATE_THEMES, PRIVATE_THEME_LABELS, (value) => command('privatetheme', { text: value }));

  // ---- the three blocks ----
  const sites = el('div', 'pg-protect pg-protect-sites');
  const lists = el('div', 'pg-protect pg-protect-lists');
  const deleteData = el('div', 'pg-protect pg-protect-delete');

  // Local state: nothing here is sent until the user confirms.
  const chosen = new Set<DeleteKind>(DEFAULT_DELETE_KINDS);
  let range: DeleteRangeKey = '1';
  let armed = false;
  let understood = false;
  let siteDraft = '';
  let siteLevelDraft: SiteLevel = 'none';
  let listUrl = '';
  let listKind: SourceKind = 'ads';
  let siteError = '';
  /** The problem line under a field, kept so typing can take it away without a redraw. */
  let siteProblem: HTMLElement | null = null;
  let listProblem: HTMLElement | null = null;
  let listError = '';

  function textField(label: string, placeholder: string, value: string, key: string, onInput: (value: string) => void, onEnter: () => void): HTMLInputElement {
    const input = el('input', 'bw-input pg-protect-input sp-allow-input');
    input.type = 'text';
    input.placeholder = placeholder;
    input.autocomplete = 'off';
    input.spellcheck = false;
    input.maxLength = 2048;
    input.value = value;
    input.setAttribute('aria-label', label);
    input.addEventListener('input', () => onInput(input.value));
    input.addEventListener('keydown', (event) => { if (event.key === 'Enter') { event.preventDefault(); onEnter(); } });
    return focusKey(input, key);
  }

  const errorLine = (text: string): HTMLElement => {
    const problem = el('p', 'pg-protect-error');
    problem.setAttribute('role', 'alert');
    problem.textContent = text;
    return problem;
  };

  function paintSites(): void {
    withFocus(sites, () => {
      const data = snapshot;
      const block = section('Sites with their own choices');
      sites.replaceChildren(block.root);
      if (!data) return;
      const off = !data.readable;
      const addSite = (): void => {
        const site = siteFromInput(siteDraft);
        if (site === null) { siteError = 'Enter a site such as example.com.'; paintSites(); return; }
        siteError = '';
        siteDraft = '';
        command('site-level', { text: site, n: SITE_LEVEL_N[siteLevelDraft] });
        paintSites();
      };
      // Add a site: its name, the level it gets, Add.
      const input = textField('Site to add', 'example.com', siteDraft, 'site-input', (value) => { siteDraft = value; siteError = ''; siteProblem?.remove(); siteProblem = null; }, addSite);
      const levelSelect = selectControl<SiteLevel>('Level for the new site', ['none', 'trackers', 'ads'], SITE_LEVEL_LABELS, (value) => { siteLevelDraft = value; });
      levelSelect.set(siteLevelDraft);
      const add = focusKey(textButton('Add', addSite, { icon: 'plus' }), 'site-add');
      const adder = el('div', 'pg-pair pg-protect-add');
      adder.append(input, levelSelect.root, add);
      if (off) { input.disabled = true; add.disabled = true; levelSelect.disable(true); }
      block.body.append(settingRow('Add a site', 'Choose how much is blocked on one site, whatever the level is everywhere else.', adder));
      siteProblem = null;
      if (siteError !== '') { siteProblem = errorLine(siteError); block.body.append(siteProblem); }

      if (data.current && !data.current.private) {
        const here = data.current;
        const fill = textButton(`Use ${here.host}`, () => { siteDraft = here.host; paintSites(); }, { subtle: true });
        block.body.append(settingRow('The site you are on', `${here.host}: ${LEVEL_LABELS[here.level].toLowerCase()}, cookie notices ${here.cookies ? 'hidden' : 'shown'}`, fill));
      }

      for (const row of data.sites) {
        const levelPick = selectControl<SiteLevel>(`Blocking on ${row.site}`, SITE_LEVELS, SITE_LEVEL_LABELS, (value) => command('site-level', { text: row.site, n: SITE_LEVEL_N[value] }));
        levelPick.set(row.level);
        const cookiePick = selectControl<SiteCookies>(`Cookie notices on ${row.site}`, SITE_COOKIES, SITE_COOKIE_LABELS, (value) => command('site-cookies', { text: row.site, n: SITE_COOKIE_N[value] }));
        cookiePick.set(row.cookies);
        const remove = focusKey(iconButton('close', `Forget the choices for ${row.site}`, () => command('site-remove', { text: row.site })), `site-remove:${row.site}`);
        const controls = el('div', 'pg-pair pg-protect-site-controls');
        controls.append(levelPick.root, cookiePick.root, remove);
        if (off) { levelPick.disable(true); cookiePick.disable(true); remove.disabled = true; }
        const line = settingRow(row.site, null, controls);
        line.querySelector('.pg-label')?.setAttribute('dir', 'auto');
        block.body.append(line);
      }
      if (data.sites.length === 0) block.body.append(settingRow('No saved choices yet', 'Sites you add here, switch with Ctrl+Shift+B or change in the shield appear in this list.'));
      if (data.allow.length > 0) block.body.append(settingRow('Skipped in settings', data.allow.join(', ')));
    });
  }

  function sourceRow(source: SourceRow, busy: boolean, off: boolean): HTMLElement {
    const toggle = switchControl(`Use ${source.name}`, (on) => command('src-toggle', { text: source.id, on }));
    toggle.set(source.enabled);
    focusKey(toggle.root, `src:${source.id}`);
    if (off) toggle.disable(true);
    const controls = el('div', 'pg-pair pg-protect-source');
    if (source.origin === 'url') {
      const update = focusKey(textButton('Update', () => command('src-update', { text: source.id }), { subtle: true, aria: `Fetch ${source.name} again` }), `src-update:${source.id}`);
      update.disabled = busy || off;
      controls.append(update);
    }
    if (source.removable) {
      const remove = focusKey(iconButton('trash', `Remove ${source.name}`, () => command('src-remove', { text: source.id })), `src-remove:${source.id}`);
      remove.disabled = off;
      controls.append(remove);
    }
    controls.append(toggle.root);
    const row = settingRow(source.name, sourceDetail(source), controls);
    row.classList.toggle('is-off', !source.enabled);
    row.querySelector('.pg-label')?.setAttribute('dir', 'auto');
    return row;
  }

  function paintLists(): void {
    withFocus(lists, () => {
      const data = snapshot;
      const block = section('Blocking lists');
      lists.replaceChildren(block.root);
      if (!data) return;
      const off = !data.readable;
      for (const source of data.sources) block.body.append(sourceRow(source, data.busy, off));
      const addFromUrl = (): void => {
        const issue = listUrlProblem(listUrl);
        if (issue !== null) { listError = issue; paintLists(); return; }
        listError = '';
        command('src-add-url', { text: listKind, url: listUrl.trim() });
        listUrl = '';
        paintLists();
      };
      const input = textField('Address of a filter list', 'https://example.org/list.txt', listUrl, 'list-input', (value) => { listUrl = value; listError = ''; listProblem?.remove(); listProblem = null; }, addFromUrl);
      const kindPick = selectControl<SourceKind>('What the list blocks', SOURCE_KINDS, SOURCE_KIND_LABELS, (value) => { listKind = value; });
      kindPick.set(listKind);
      const addUrl = focusKey(textButton(data.busy ? 'Fetching…' : 'Add', addFromUrl, { icon: 'plus' }), 'list-add');
      const adder = el('div', 'pg-pair pg-protect-add');
      adder.append(input, kindPick.root, addUrl);
      if (off || data.busy) { input.disabled = true; addUrl.disabled = true; kindPick.disable(true); }
      block.body.append(settingRow('Add a list from an address', 'Fetched once, now, over https. Bowser never checks it again unless you press Update.', adder));
      listProblem = null;
      if (listError !== '') { listProblem = errorLine(listError); block.body.append(listProblem); }
      const fileKind = selectControl<SourceKind>('What the file blocks', SOURCE_KINDS, SOURCE_KIND_LABELS, (value) => { listKind = value; });
      fileKind.set(listKind);
      const addFile = focusKey(textButton('Choose file…', () => command('src-add-file', { text: listKind }), { icon: 'folder' }), 'list-file');
      const fileControls = el('div', 'pg-pair');
      fileControls.append(fileKind.root, addFile);
      if (off || data.busy) { fileKind.disable(true); addFile.disabled = true; }
      block.body.append(settingRow('Add a list from a file', 'A text file on this computer in the usual filter list format. Bowser keeps a copy.', fileControls));
    });
  }

  function paintDelete(): void {
    withFocus(deleteData, () => {
      const data = snapshot;
      const block = section('Delete browsing data');
      deleteData.replaceChildren(block.root);
      if (!data) return;
      const withLogins = chosen.has('logins') && !data.guest;
      const periodControl = segmented(DELETE_RANGES, range, (value) => { range = value; armed = false; paintDelete(); }, 'Delete browsing data from');
      periodControl.classList.add('sp-seg');
      block.body.append(settingRow('Period', null, periodControl));
      for (const kind of DELETE_KINDS) {
        const gone = kind.key === 'logins' && data.guest;
        const toggle = switchControl(kind.label, (on) => {
          if (on) chosen.add(kind.key); else chosen.delete(kind.key);
          if (kind.key === 'logins' && !on) understood = false;
          armed = false;
          paintDelete();
        });
        toggle.set(chosen.has(kind.key) && !gone);
        focusKey(toggle.root, `kind:${kind.key}`);
        if (gone) toggle.disable(true);
        block.body.append(settingRow(kind.label, gone ? 'Guest windows keep no saved sign-ins.' : kind.hint, toggle.root));
      }
      if (withLogins) {
        // The one irreversible kind needs its own yes, beside the Delete button's.
        const sure = switchControl('I understand that deleting saved sign-ins cannot be undone', (on) => { understood = on; armed = false; paintDelete(); });
        sure.set(understood);
        focusKey(sure.root, 'logins-sure');
        block.body.append(settingRow('Confirm deleting saved sign-ins', 'Every login Bowser has saved for you is deleted for good, whatever period is chosen.', sure.root));
      }
      const effective = DELETE_KINDS.filter((kind) => chosen.has(kind.key) && !(data.guest && kind.key === 'logins')).map((kind) => kind.key);
      const ready = effective.length > 0 && (!withLogins || understood);
      const summary = deleteSummary(effective, range);
      const run = focusKey(textButton(armed ? 'Confirm' : 'Delete', () => {
        if (!armed) { armed = true; paintDelete(); return; }
        armed = false;
        const bits = kindsBits(effective);
        command('delete-data', { id: bits, n: Number(range), on: (bits & LOGINS_BIT) !== 0 });
        understood = false;
        paintDelete();
      }, { primary: armed, aria: armed ? `Confirm: ${summary}` : summary }), 'delete-run');
      run.disabled = !ready || !data.readable;
      run.classList.toggle('is-danger', armed);
      const cancel = focusKey(textButton('Cancel', () => { armed = false; paintDelete(); }), 'delete-cancel');
      cancel.hidden = !armed;
      const actions = el('div', 'pg-pair');
      actions.append(cancel, run);
      block.body.append(settingRow(summary, armed ? 'This cannot be undone.' : 'Nothing is deleted until you confirm.', actions));
    });
  }

  const sectionsFor = (): HTMLElement[] => {
    const blocking = section('Blocking');
    blocking.body.append(
      settingRow('Blocking level', snapshot ? levelDescription(snapshot.level, snapshot.cookieNotices) : null, level.root),
      settingRow('Hide cookie notices', 'Hides cookie-consent banners, with or without blocking. A site can switch it off.', cookieNotices.root),
    );
    if (snapshot) blocking.body.append(settingRow('Blocked', null, blockedText(snapshot.blocked.page, snapshot.blocked.session)));
    const history = section('History');
    history.body.append(settingRow('Keep history for', 'Older visits are deleted from the history list.', retention.root));
    const appearance = section('Website appearance');
    appearance.body.append(
      settingRow('Websites look', 'The colour scheme sites are asked to use.', websiteAppearance.root),
      settingRow('Force dark theme on all websites', 'Darkens pages that have no dark theme of their own.', forceDark.root),
      settingRow('Private tab theme', 'How Bowser’s own windows look while a private tab is shown.', privateTheme.root),
    );
    return [blocking.root, sites, lists, history.root, deleteData, appearance.root];
  };

  function paintAll(): void {
    const data = snapshot;
    if (data) {
      level.set(data.level);
      cookieNotices.set(data.cookieNotices);
      retention.set(data.retention);
      websiteAppearance.set(data.websiteAppearance);
      forceDark.set(data.forceDark);
      privateTheme.set(data.privateTheme);
      const off = !data.readable;
      level.disable(off);
      cookieNotices.disable(off);
      retention.disable(off);
      websiteAppearance.disable(off);
      forceDark.disable(off);
      privateTheme.disable(off);
    }
    paintSites();
    paintLists();
    paintDelete();
  }
  paintAll();

  return {
    level, cookieNotices, retention, websiteAppearance, forceDark, privateTheme, sites, lists, deleteData,
    sections: sectionsFor,
    receive(message: Inbound): void {
      if (message.t !== 'snap' || message.p !== 'protection') return;
      const parsed = parseProtectionSnapshot((message as SnapshotMsg).d);
      if (!parsed) { snapshot = null; options.toast('Privacy settings could not be loaded. The response was invalid.', false); paintAll(); return; }
      const first = snapshot === null && shownSeq === null;
      snapshot = parsed;
      // A result Bowser reports once: shown when it is new, never for one that was already there when the page opened.
      if (parsed.result !== null && parsed.result.seq !== shownSeq && !first) options.toast(parsed.result.text, parsed.result.ok);
      if (parsed.result !== null || first) shownSeq = parsed.result?.seq ?? 0;
      paintAll();
    },
  };
}
