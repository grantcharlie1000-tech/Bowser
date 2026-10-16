// The settings page's "Start page" section (docs/design/start-page.md): speed dials, the homepage and
// startup, and the dashboard widgets. settings-page.ts hands this module its own card, row and change
// helpers (so the rows look and behave like every other row) and calls `apply` with each snapshot.
import { el } from './dom';
import { selectField, type SelectField } from './panels/select';
import {
  GROUP_REOPEN, MAX_COLUMNS, MAX_STARTUP_PAGES, STARTUP_MODES, WIDGET_BGS, WIDGET_HEIGHTS, normaliseAddress,
  type GroupReopen, type StartSettings, type StartupMode, type TitleMode, type WidgetBg, type WidgetHeight,
} from './start-page-logic';

interface Ctl<T> { readonly root: HTMLElement; set(value: T): void; disable(off: boolean): void }

/** What settings-page.ts lends this module. */
export interface StartSectionHost {
  card(section: 'startpage', heading?: string): HTMLElement;
  row(into: HTMLElement, id: string, control?: HTMLElement, options?: { stack?: boolean }): { readonly root: HTMLElement };
  /** Edits the draft's start settings and saves. */
  change(edit: (start: StartSettings) => void): void;
  switchControl(onChange: (on: boolean) => void): Ctl<boolean>;
  segmentedControl<T extends string>(label: string, options: ReadonlyArray<readonly [T, string]>, onSelect: (value: T) => void): Ctl<T | null>;
  toast(text: string, ok: boolean): void;
}

export const GROUP_REOPEN_LABELS: Readonly<Record<GroupReopen, string>> = { last: 'The group you used last', first: 'The first group' };
export const TITLE_LABELS: Readonly<Record<TitleMode, string>> = { always: 'Always', auto: 'When needed', never: 'Never' };
export const STARTUP_LABELS: Readonly<Record<StartupMode, string>> = {
  start_page: 'Open the start page',
  last_session: 'Continue where you left off',
  homepage: 'Open the homepage',
  specific_pages: 'Open specific pages',
};
export const THUMB_LABELS: readonly string[] = ['Tiny', 'Small', 'Medium', 'Large', 'Huge'];
export const HEIGHT_LABELS: Readonly<Record<WidgetHeight, string>> = { regular: 'Regular', tall: 'Tall' };
export const BG_LABELS: Readonly<Record<WidgetBg, string>> = { solid: 'Solid', translucent: 'Semi-transparent', transparent: 'Transparent', custom: 'Custom' };

/** The lines of a "specific pages" box as addresses, or `null` when one is not a web address or there are too many. */
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

export function mountStartSection(host: StartSectionHost): { apply(start: StartSettings, readable: boolean): void } {
  // Speed dials
  const dials = host.card('startpage', 'Speed dials');
  const reopen = host.segmentedControl<GroupReopen>('Open the start page on', GROUP_REOPEN.map((key) => [key, GROUP_REOPEN_LABELS[key]] as const), (value) => host.change((s) => { s.groupReopen = value; }));
  host.row(dials, 'startGroupReopen', reopen.root);
  const titles = host.segmentedControl<TitleMode>('Show titles', (['always', 'auto', 'never'] as const).map((key) => [key, TITLE_LABELS[key]] as const), (value) => host.change((s) => { s.titles = value; }));
  host.row(dials, 'startTitles', titles.root);
  const addButton = host.switchControl((on) => host.change((s) => { s.addButton = on; }));
  host.row(dials, 'startAddButton', addButton.root);
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
  host.row(dials, 'startColumns', columnsHost);
  const size = host.segmentedControl<string>('Tile size', THUMB_LABELS.map((label, i) => [String(i + 1), label] as const), (value) => host.change((s) => { s.thumbSize = Number(value); }));
  host.row(dials, 'startThumbSize', size.root);
  const drag = host.switchControl((on) => host.change((s) => { s.dragReorder = on; }));
  host.row(dials, 'startDragReorder', drag.root);

  // Homepage and startup
  const home = host.card('startpage', 'Homepage and startup');
  const homepage = el('input', 'bw-input sp-text-input');
  homepage.type = 'text';
  homepage.placeholder = 'https://example.com';
  homepage.maxLength = 2048;
  homepage.autocomplete = 'off';
  homepage.spellcheck = false;
  homepage.setAttribute('aria-labelledby', 'sp-l-startHomepage');
  homepage.addEventListener('change', () => {
    const text = homepage.value.trim();
    if (text === '') {
      host.change((s) => { s.homepage = ''; });
      return;
    }
    const url = normaliseAddress(text);
    if (url === null) {
      host.toast('Enter an http or https address.', false);
      return;
    }
    homepage.value = url;
    host.change((s) => { s.homepage = url; });
  });
  homepage.addEventListener('keydown', (event) => { if (event.key === 'Enter') homepage.blur(); });
  host.row(home, 'startHomepage', homepage);
  const startupHost = el('div', 'sp-select-host');
  let startupSelect: SelectField<string> | null = null;
  const paintStartup = (value: StartupMode): void => {
    if (startupSelect) {
      startupSelect.set(value);
      return;
    }
    startupSelect = selectField<string>({
      options: STARTUP_MODES.map((key) => ({ value: key, label: STARTUP_LABELS[key] })),
      value,
      label: 'When Bowser starts',
      placeholder: 'When Bowser starts',
      minListWidth: 240,
      onChange: (next) => host.change((s) => { s.startup = next as StartupMode; }),
    });
    startupSelect.button.setAttribute('aria-labelledby', 'sp-l-startupMode');
    startupHost.replaceChildren(startupSelect.root);
  };
  host.row(home, 'startupMode', startupHost);
  const pages = el('textarea', 'bw-input sp-pages');
  pages.rows = 4;
  pages.placeholder = 'https://example.com';
  pages.spellcheck = false;
  pages.maxLength = 4096;
  pages.setAttribute('aria-labelledby', 'sp-l-startupPages');
  pages.addEventListener('change', () => {
    const list = parsePageLines(pages.value);
    if (list === null) {
      host.toast(`Use up to ${MAX_STARTUP_PAGES} http or https addresses, one per line.`, false);
      return;
    }
    pages.value = list.join('\n');
    host.change((s) => { s.startupPages = list; });
  });
  host.row(home, 'startupPages', pages, { stack: true });

  // Widgets
  const widgets = host.card('startpage', 'Dashboard widgets');
  const widgetsOn = host.switchControl((on) => host.change((s) => { s.widgets = on; }));
  host.row(widgets, 'startWidgets', widgetsOn.root);
  const wcols = host.segmentedControl<string>('Widget columns', ['1', '2', '3', '4'].map((n) => [n, n] as const), (value) => host.change((s) => { s.widgetColumns = Number(value); }));
  host.row(widgets, 'startWidgetColumns', wcols.root);
  const wheight = host.segmentedControl<WidgetHeight>('Widget height', WIDGET_HEIGHTS.map((key) => [key, HEIGHT_LABELS[key]] as const), (value) => host.change((s) => { s.widgetHeight = value; }));
  host.row(widgets, 'startWidgetHeight', wheight.root);
  const wbg = host.segmentedControl<WidgetBg>('Widget background', WIDGET_BGS.map((key) => [key, BG_LABELS[key]] as const), (value) => host.change((s) => { s.widgetBg = value; }));
  host.row(widgets, 'startWidgetBg', wbg.root);
  const wcolor = el('input', 'sp-color');
  wcolor.type = 'color';
  wcolor.setAttribute('aria-label', 'Custom widget colour');
  wcolor.addEventListener('change', () => host.change((s) => { s.widgetColor = wcolor.value.toLowerCase(); }));
  host.row(widgets, 'startWidgetColor', wcolor);
  const wdrag = host.switchControl((on) => host.change((s) => { s.widgetDrag = on; }));
  host.row(widgets, 'startWidgetDrag', wdrag.root);

  return {
    apply(start, readable): void {
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
      wheight.set(start.widgetHeight);
      wbg.set(start.widgetBg);
      wcolor.value = start.widgetColor;
      wdrag.set(start.widgetDrag);
      for (const control of [reopen, titles, addButton, size, drag, widgetsOn]) control.disable(!readable);
      homepage.disabled = !readable;
      columnsSelect?.button.toggleAttribute('disabled', !readable);
      startupSelect?.button.toggleAttribute('disabled', !readable);
      // The rows below only matter while what they tune is on.
      pages.disabled = !readable || start.startup !== 'specific_pages';
      for (const control of [wcols, wheight, wbg, wdrag]) control.disable(!readable || !start.widgets);
      wcolor.disabled = !readable || !start.widgets || start.widgetBg !== 'custom';
    },
  };
}
