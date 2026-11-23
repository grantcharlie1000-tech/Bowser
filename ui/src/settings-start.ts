import { el, icon } from './dom';
import { selectField, type SelectField } from './panels/select';
import {
  DIM_STEPS, GROUP_REOPEN, MAX_COLUMNS, MAX_STARTUP_PAGES, MAX_URL, STARTUP_CHOICES, WALL_ART, WIDGET_BGS, dimStep, normaliseAddress, startupChoice,
  type GroupReopen, type StartSettings, type StartupChoice, type StartupMode, type TitleMode, type Wallpaper, type WidgetBg,
} from './start-page-logic';
import type { TabOptionValues } from './tab-options-logic';

interface Ctl<T> { readonly root: HTMLElement; set(value: T): void; disable(off: boolean): void }

export interface StartSectionHost {
  card(section: 'startpage', heading?: string): HTMLElement;
  row(into: HTMLElement, id: string, control?: HTMLElement, options?: { stack?: boolean }): { readonly root: HTMLElement };
  change(edit: (start: StartSettings) => void): void;
  switchControl(onChange: (on: boolean) => void): Ctl<boolean>;
  segmentedControl<T extends string>(label: string, options: ReadonlyArray<readonly [T, string] | readonly [T, string, string]>, onSelect: (value: T) => void): Ctl<T | null>;
  toast(text: string, ok: boolean): void;
}

export const GROUP_REOPEN_LABELS: Readonly<Record<GroupReopen, string>> = { last: 'Last used', first: 'First group' };
export const GROUP_REOPEN_ARIA: Readonly<Record<GroupReopen, string>> = { last: 'The group you used last', first: 'The first group' };
export const TITLE_LABELS: Readonly<Record<TitleMode, string>> = { always: 'Always', auto: 'If no image', never: 'Never' };
export const TITLE_ARIA: Readonly<Record<TitleMode, string>> = {
  always: 'Always',
  auto: 'Under tiles without a chosen image, on hover for the rest',
  never: 'Never',
};
export const START_OFF_NOTE = 'Not shown: New tab page is Blank';
export const STARTUP_LABELS: Readonly<Record<StartupChoice, string>> = {
  last_session: 'Your tabs',
  homepage: 'Tabs + homepage',
  specific_pages: 'Tabs + pages',
};
export const THUMB_LABELS: readonly string[] = ['Tiny', 'Small', 'Medium', 'Large', 'Huge'];
export const BG_LABELS: Readonly<Record<WidgetBg, string>> = { solid: 'Solid', translucent: 'Tinted', transparent: 'Clear', custom: 'Custom' };
export const BG_ARIA: Readonly<Record<WidgetBg, string>> = { solid: 'Solid', translucent: 'Semi-transparent', transparent: 'Transparent', custom: 'Custom' };

export const WALLPAPER_OPTIONS: ReadonlyArray<{ value: Wallpaper; label: string }> = [
  { value: 'none', label: 'Glass (no picture)' },
  ...(Object.entries(WALL_ART) as Array<[Wallpaper, { label: string }]>).map(([value, art]) => ({ value, label: art.label })),
  { value: 'custom', label: 'Your picture' },
];

export function startPageShown(tabOptions: TabOptionValues): boolean {
  return tabOptions.tabs_new_tab_page !== 'blank';
}

export function parsePageLines(text: string): string[] | null {
  const lines = text.split(/\r?\n/).map((line) => line.trim()).filter((line) => line !== '');
  if (lines.length > MAX_STARTUP_PAGES) return null;
  const out: string[] = [];
  for (const line of lines) {
    const url = normaliseAddress(line);
    if (url === null) return null;
    out.push(url);
  }
  return out;
}

function rowNote(row: HTMLElement, id: string, text: string): HTMLElement {
  const note = el('span', 'sp-flag sp-start-note');
  note.id = `sp-f-${id}`;
  note.hidden = true;
  const words = el('span');
  words.textContent = text;
  note.append(icon('info', { size: 12, fine: true }), words);
  row.querySelector('.sp-text')?.append(note);
  return note;
}

function rest(row: HTMLElement, off: boolean): void {
  row.classList.toggle('is-off', off);
}

function away(row: HTMLElement, gone: boolean): void {
  row.classList.toggle('is-na', gone);
}

export function mountStartSection(host: StartSectionHost): { apply(start: StartSettings, readable: boolean, shown?: boolean): void } {
  const look = host.card('startpage', 'Start page');
  const clockOn = host.switchControl((on) => host.change((s) => { s.clock = on; }));
  const clockRow = host.row(look, 'startClock', clockOn.root).root;
  const searchOn = host.switchControl((on) => host.change((s) => { s.search = on; }));
  const searchRow = host.row(look, 'startSearch', searchOn.root).root;
  const wallHost = el('div', 'sp-select-host');
  let wallSelect: SelectField<string> | null = null;
  const paintWall = (value: Wallpaper): void => {
    if (wallSelect) {
      wallSelect.set(value);
      return;
    }
    wallSelect = selectField<string>({
      options: WALLPAPER_OPTIONS,
      value,
      label: 'Start page background',
      placeholder: 'Background',
      minListWidth: 200,
      onChange: (next) => host.change((s) => { s.wallpaper = next as Wallpaper; }),
    });
    wallSelect.button.setAttribute('aria-labelledby', 'sp-l-startWallpaper');
    wallHost.replaceChildren(wallSelect.root);
  };
  const wallRow = host.row(look, 'startWallpaper', wallHost).root;
  const dim = host.segmentedControl<string>('Dim background', DIM_STEPS.map(([n, label]) => [String(n), label] as const), (value) => host.change((s) => { s.wallpaperDim = Number(value); }));
  const dimRow = host.row(look, 'startWallpaperDim', dim.root).root;

  const dials = host.card('startpage', 'Dials');
  const dialsOn = host.switchControl((on) => host.change((s) => { s.dials = on; }));
  const dialsRow = host.row(dials, 'startDials', dialsOn.root).root;
  const dialsNote = rowNote(dialsRow, 'startDials', START_OFF_NOTE);
  const reopen = host.segmentedControl<GroupReopen>('Open the start page on', GROUP_REOPEN.map((key) => [key, GROUP_REOPEN_LABELS[key], GROUP_REOPEN_ARIA[key]] as const), (value) => host.change((s) => { s.groupReopen = value; }));
  const reopenRow = host.row(dials, 'startGroupReopen', reopen.root).root;
  const titles = host.segmentedControl<TitleMode>('Show titles', (['always', 'auto', 'never'] as const).map((key) => [key, TITLE_LABELS[key], TITLE_ARIA[key]] as const), (value) => host.change((s) => { s.titles = value; }));
  const titlesRow = host.row(dials, 'startTitles', titles.root).root;
  const addButton = host.switchControl((on) => host.change((s) => { s.addButton = on; }));
  const addRow = host.row(dials, 'startAddButton', addButton.root).root;
  const columnsHost = el('div', 'sp-select-host');
  let columnsSelect: SelectField<string> | null = null;
  const paintColumns = (value: number): void => {
    if (columnsSelect) {
      columnsSelect.set(String(value));
      return;
    }
    columnsSelect = selectField<string>({
      options: Array.from({ length: MAX_COLUMNS }, (_, i) => ({ value: String(i + 1), label: String(i + 1) })),
      value: String(value),
      label: 'Maximum columns',
      placeholder: 'Columns',
      onChange: (next) => host.change((s) => { s.columns = Number(next); }),
    });
    columnsSelect.button.setAttribute('aria-labelledby', 'sp-l-startColumns');
    columnsHost.replaceChildren(columnsSelect.root);
  };
  const columnsRow = host.row(dials, 'startColumns', columnsHost).root;
  const size = host.segmentedControl<string>('Tile size', THUMB_LABELS.map((label, i) => [String(i + 1), label] as const), (value) => host.change((s) => { s.thumbSize = Number(value); }));
  const sizeRow = host.row(dials, 'startThumbSize', size.root).root;
  const drag = host.switchControl((on) => host.change((s) => { s.dragReorder = on; }));
  const dragRow = host.row(dials, 'startDragReorder', drag.root).root;

  const home = host.card('startpage', 'Startup');
  const homepage = el('input', 'bw-input sp-text-input');
  homepage.type = 'text';
  homepage.placeholder = '';
  homepage.maxLength = MAX_URL;
  homepage.autocomplete = 'off';
  homepage.spellcheck = false;
  homepage.setAttribute('aria-labelledby', 'sp-l-startHomepage');
  homepage.addEventListener('change', () => {
    const text = homepage.value.trim();
    if (text === '') {
      homepage.removeAttribute('aria-invalid');
      host.change((s) => { s.homepage = ''; });
      return;
    }
    const url = normaliseAddress(text);
    if (url === null) {
      homepage.setAttribute('aria-invalid', 'true');
      host.toast('Enter an http or https address.', false);
      return;
    }
    homepage.removeAttribute('aria-invalid');
    homepage.value = url;
    host.change((s) => { s.homepage = url; });
  });
  homepage.addEventListener('keydown', (event) => { if (event.key === 'Enter') homepage.blur(); });
  host.row(home, 'startHomepage', homepage);
  const startupHost = el('div', 'sp-select-host');
  let startupSelect: SelectField<string> | null = null;
  const paintStartup = (mode: StartupMode): void => {
    const value = startupChoice(mode);
    if (startupSelect) {
      startupSelect.set(value);
      return;
    }
    startupSelect = selectField<string>({
      options: STARTUP_CHOICES.map((key) => ({ value: key, label: STARTUP_LABELS[key] })),
      value,
      label: 'Startup',
      placeholder: 'Startup',
      minListWidth: 240,
      onChange: (next) => host.change((s) => { s.startup = next as StartupMode; }),
    });
    startupSelect.button.setAttribute('aria-labelledby', 'sp-l-startupMode');
    startupHost.replaceChildren(startupSelect.root);
  };
  host.row(home, 'startupMode', startupHost);
  const pages = el('textarea', 'bw-input sp-pages');
  pages.rows = 4;
  pages.placeholder = '';
  pages.spellcheck = false;
  pages.maxLength = MAX_STARTUP_PAGES * (MAX_URL + 2);
  pages.setAttribute('aria-labelledby', 'sp-l-startupPages');
  pages.addEventListener('change', () => {
    const list = parsePageLines(pages.value);
    if (list === null) {
      pages.setAttribute('aria-invalid', 'true');
      host.toast(`Use up to ${MAX_STARTUP_PAGES} http or https addresses, one per line.`, false);
      return;
    }
    pages.removeAttribute('aria-invalid');
    pages.value = list.join('\n');
    host.change((s) => { s.startupPages = list; });
  });
  const pagesRow = host.row(home, 'startupPages', pages, { stack: true }).root;

  const widgets = host.card('startpage', 'Widgets');
  const widgetsOn = host.switchControl((on) => host.change((s) => { s.widgets = on; }));
  const widgetsRow = host.row(widgets, 'startWidgets', widgetsOn.root).root;
  const widgetsNote = rowNote(widgetsRow, 'startWidgets', START_OFF_NOTE);
  const wcols = host.segmentedControl<string>('List columns', ['1', '2', '3', '4'].map((n) => [n, n] as const), (value) => host.change((s) => { s.widgetColumns = Number(value); }));
  const wcolsRow = host.row(widgets, 'startWidgetColumns', wcols.root).root;
  const wbg = host.segmentedControl<WidgetBg>('List background', WIDGET_BGS.map((key) => [key, BG_LABELS[key], BG_ARIA[key]] as const), (value) => host.change((s) => { s.widgetBg = value; }));
  const wbgRow = host.row(widgets, 'startWidgetBg', wbg.root).root;
  const wcolor = el('input', 'sp-color');
  wcolor.type = 'color';
  wcolor.setAttribute('aria-label', 'Custom list colour');
  wcolor.addEventListener('change', () => host.change((s) => { s.widgetColor = wcolor.value.toLowerCase(); }));
  const wcolorRow = host.row(widgets, 'startWidgetColor', wcolor).root;
  const wdrag = host.switchControl((on) => host.change((s) => { s.widgetDrag = on; }));
  const wdragRow = host.row(widgets, 'startWidgetDrag', wdrag.root).root;

  const paintNote = (control: HTMLElement, note: HTMLElement, show: boolean): void => {
    if (note.hidden === show) note.hidden = !show;
    const now = control.getAttribute('aria-describedby') ?? '';
    const ids = now.split(' ').filter((id) => id !== '' && id !== note.id);
    if (show) ids.push(note.id);
    const next = ids.join(' ');
    if (next === now) return;
    if (next !== '') control.setAttribute('aria-describedby', next);
    else control.removeAttribute('aria-describedby');
  };

  return {
    apply(start, readable, shown = true): void {
      clockOn.set(start.clock);
      searchOn.set(start.search);
      paintWall(start.wallpaper);
      dim.set(String(dimStep(start.wallpaperDim)));
      dialsOn.set(start.dials);
      reopen.set(start.groupReopen);
      titles.set(start.titles);
      addButton.set(start.addButton);
      paintColumns(start.columns);
      size.set(String(start.thumbSize));
      drag.set(start.dragReorder);
      if (document.activeElement !== homepage) homepage.value = start.homepage;
      paintStartup(start.startup);
      if (document.activeElement !== pages) pages.value = start.startupPages.join('\n');
      widgetsOn.set(start.widgets);
      wcols.set(String(start.widgetColumns));
      wbg.set(start.widgetBg);
      wcolor.value = start.widgetColor;
      wdrag.set(start.widgetDrag);
      const live = readable && shown;
      for (const control of [clockOn, searchOn]) control.disable(!live);
      wallSelect?.button.toggleAttribute('disabled', !live);
      for (const row of [clockRow, searchRow, wallRow]) rest(row, readable && !shown);
      const dimLive = live && start.wallpaper !== 'none';
      dim.disable(!dimLive);
      rest(dimRow, readable && !dimLive);
      away(dimRow, readable && !dimLive);
      paintNote(dialsOn.root, dialsNote, readable && !shown);
      paintNote(widgetsOn.root, widgetsNote, readable && !shown);
      for (const control of [dialsOn, widgetsOn]) control.disable(!live);
      for (const row of [dialsRow, widgetsRow]) rest(row, readable && !shown);
      const dialsLive = live && start.dials;
      for (const control of [reopen, titles, addButton, size, drag]) control.disable(!dialsLive);
      columnsSelect?.button.toggleAttribute('disabled', !dialsLive);
      for (const row of [reopenRow, titlesRow, addRow, columnsRow, sizeRow, dragRow]) {
        rest(row, readable && !dialsLive);
        away(row, readable && !dialsLive);
      }
      const widgetsLive = live && start.widgets;
      for (const control of [wcols, wbg, wdrag]) control.disable(!widgetsLive);
      for (const row of [wcolsRow, wbgRow, wdragRow]) {
        rest(row, readable && !widgetsLive);
        away(row, readable && !widgetsLive);
      }
      wcolor.disabled = !widgetsLive || start.widgetBg !== 'custom';
      rest(wcolorRow, readable && wcolor.disabled);
      away(wcolorRow, readable && wcolor.disabled);
      homepage.disabled = !readable;
      startupSelect?.button.toggleAttribute('disabled', !readable);
      pages.disabled = !readable || start.startup !== 'specific_pages';
      rest(pagesRow, readable && pages.disabled);
      away(pagesRow, readable && pages.disabled);
    },
  };
}
