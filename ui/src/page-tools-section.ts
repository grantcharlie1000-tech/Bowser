// The settings page's "Page tools" section. Rust sends `{t:"snap",p:"pagetools"}`
// (src/chrome_host/page_tools_page.rs); this module draws it into the cards the page made, sends one
// `set` command per change, and applies each answer in place so focus never jumps. The few rows a person
// uses often are the page's core rows; the rest sit behind its "More options" (settings-page-logic.ts
// decides which, by row id). Words and checks live in page-tools-logic.ts.
import { el } from './dom';
import type { PanelFields, SettingsSection, SnapshotMsg } from './protocol';
import {
  CAPTURE_FORMATS, READER_FONTS, READER_LINE, READER_SIZE, READER_THEMES, READER_WIDTH, ZOOM_SCOPES, isCaptureDir, isCaptureName, parsePageToolsSnapshot, setFields,
  MAX_DIR, MAX_TEMPLATE, type CaptureFormat, type PageToolsKey, type PageToolsSnapshot, type ReaderFont, type ReaderTheme, type ZoomScope,
} from './page-tools-logic';

export interface PageToolsHost {
  readonly send: (fields: PanelFields) => boolean;
  readonly toast: (text: string, ok: boolean) => void;
  readonly card: (section: SettingsSection, heading?: string) => HTMLElement;
  readonly row: (into: HTMLElement, id: string, control?: HTMLElement, options?: { stack?: boolean }) => { readonly root: HTMLElement; readonly desc: HTMLElement | null };
  readonly switchControl: (onChange: (on: boolean) => void) => { readonly root: HTMLElement; set(value: boolean): void; disable(off: boolean): void };
  readonly segmentedControl: <T extends string>(label: string, options: ReadonlyArray<readonly [T, string]>, onSelect: (value: T) => void) => { readonly root: HTMLElement; set(value: T | null): void; disable(off: boolean): void };
}

export interface PageToolsSection { receive(message: SnapshotMsg): void }

interface Field<T> { readonly root: HTMLElement; set(value: T): void }

const CAPTURE_LABELS: Readonly<Record<CaptureFormat, string>> = { png: 'PNG', jpeg: 'JPEG' };
const SCOPE_LABELS: Readonly<Record<ZoomScope, string>> = { site: 'The site', tab: 'The tab' };
const FONT_LABELS: Readonly<Record<ReaderFont, string>> = { serif: 'Serif', sans: 'Sans', mono: 'Mono' };
const THEME_LABELS: Readonly<Record<ReaderTheme, string>> = { auto: 'Auto', light: 'Light', sepia: 'Sepia', dark: 'Dark' };

/** A number box. `scale` 10 shows tenths as decimals (16 as 1.6); the command always carries the whole number. */
function numberField(label: string, min: number, max: number, scale: number, onCommit: (value: number) => void): Field<number> {
  const input = el('input', 'bw-input ad-number');
  input.type = 'number';
  input.min = String(min / scale);
  input.max = String(max / scale);
  input.step = String(1 / scale);
  input.setAttribute('aria-label', label);
  let last = min;
  const show = (n: number): string => String(n / scale);
  input.addEventListener('change', () => {
    const n = Math.round(Number(input.value) * scale);
    if (!Number.isFinite(n) || n < min || n > max) { input.value = show(last); return; }
    onCommit(n);
    last = n;
  });
  return { root: input, set(value) { last = value; if (document.activeElement !== input) input.value = show(value); } };
}

function textField(label: string, max: number, placeholder: string, onCommit: (value: string) => boolean): Field<string> {
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

export function mountPageToolsSection(host: PageToolsHost): PageToolsSection {
  const painters: Array<(s: PageToolsSnapshot) => void> = [];
  const put = (key: PageToolsKey, value: boolean | number | string): void => {
    const fields = setFields(key, value);
    if (!fields) { host.toast('That value is not allowed.', false); return; }
    host.send(fields);
  };
  const toggle = (into: HTMLElement, id: string, key: 'pt_spatial_nav' | 'pt_wheel_zoom' | 'pt_pip_menu' | 'pt_reader_vertical' | 'pt_reader_icon' | 'pt_reader_time'): void => {
    const control = host.switchControl((on) => put(key, on));
    host.row(into, id, control.root);
    painters.push((s) => control.set(s.s[key]));
  };

  const capture = host.card('pagetools', 'Capture');
  const dir = textField('Capture folder', MAX_DIR, 'Pictures\\Bowser', (value) => {
    if (!isCaptureDir(value)) { host.toast('Use a folder on this PC, such as C:\\Users\\you\\Pictures.', false); return false; }
    put('pt_capture_dir', value);
    return true;
  });
  host.row(capture, 'ptCaptureDir', dir.root);
  painters.push((s) => dir.set(s.s.pt_capture_dir));
  const name = textField('Capture file name', MAX_TEMPLATE, '{host} {date} {time}', (value) => {
    if (!isCaptureName(value)) { host.toast('Use only {host}, {title}, {date}, {time} and {mode}, and no slashes.', false); return false; }
    put('pt_capture_name', value);
    return true;
  });
  host.row(capture, 'ptCaptureName', name.root);
  painters.push((s) => name.set(s.s.pt_capture_name));
  const format = host.segmentedControl<CaptureFormat>('Capture format', CAPTURE_FORMATS.map((f) => [f, CAPTURE_LABELS[f]] as const), (value) => put('pt_capture_format', value));
  host.row(capture, 'ptCaptureFormat', format.root);
  painters.push((s) => format.set(s.s.pt_capture_format));

  const pages = host.card('pagetools', 'Pages');
  toggle(pages, 'ptSpatial', 'pt_spatial_nav');
  const scope = host.segmentedControl<ZoomScope>('Page zoom belongs to', ZOOM_SCOPES.map((z) => [z, SCOPE_LABELS[z]] as const), (value) => put('pt_zoom_scope', value));
  host.row(pages, 'ptZoomScope', scope.root);
  painters.push((s) => scope.set(s.s.pt_zoom_scope));
  toggle(pages, 'ptWheelZoom', 'pt_wheel_zoom');
  toggle(pages, 'ptPip', 'pt_pip_menu');

  const reader = host.card('pagetools', 'Reader view');
  const font = host.segmentedControl<ReaderFont>('Reader view font', READER_FONTS.map((f) => [f, FONT_LABELS[f]] as const), (value) => put('pt_reader_font', value));
  host.row(reader, 'ptReaderFont', font.root);
  painters.push((s) => font.set(s.s.pt_reader_font));
  const size = numberField('Reader view text size', READER_SIZE.min, READER_SIZE.max, 1, (n) => put('pt_reader_size', n));
  host.row(reader, 'ptReaderSize', size.root);
  painters.push((s) => size.set(s.s.pt_reader_size));
  const line = numberField('Reader view line height', READER_LINE.min, READER_LINE.max, 10, (n) => put('pt_reader_line', n));
  host.row(reader, 'ptReaderLine', line.root);
  painters.push((s) => line.set(s.s.pt_reader_line));
  const width = numberField('Reader view column width', READER_WIDTH.min, READER_WIDTH.max, 1, (n) => put('pt_reader_width', n));
  host.row(reader, 'ptReaderWidth', width.root);
  painters.push((s) => width.set(s.s.pt_reader_width));
  const theme = host.segmentedControl<ReaderTheme>('Reader view colours', READER_THEMES.map((t) => [t, THEME_LABELS[t]] as const), (value) => put('pt_reader_theme', value));
  host.row(reader, 'ptReaderTheme', theme.root);
  painters.push((s) => theme.set(s.s.pt_reader_theme));
  toggle(reader, 'ptReaderVertical', 'pt_reader_vertical');
  toggle(reader, 'ptReaderIcon', 'pt_reader_icon');
  toggle(reader, 'ptReaderTime', 'pt_reader_time');

  return {
    receive(message) {
      const parsed = parsePageToolsSnapshot(message.d);
      if (!parsed) return;
      for (const paint of painters) paint(parsed);
    },
  };
}
