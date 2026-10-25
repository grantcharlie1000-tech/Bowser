// Pure logic of the settings page's "Page tools" section (src/chrome_host/page_tools_page.rs): the
// snapshot contract, the rows the nav search indexes, and the command text. No DOM here;
// test/page-tools.mjs covers all of it.
import type { RowSpec } from './settings-page-logic';
import type { SnapshotMsg } from './protocol';

export const CAPTURE_FORMATS = ['png', 'jpeg'] as const;
export type CaptureFormat = typeof CAPTURE_FORMATS[number];
export const ZOOM_SCOPES = ['site', 'tab'] as const;
export type ZoomScope = typeof ZOOM_SCOPES[number];
export const READER_FONTS = ['serif', 'sans', 'mono'] as const;
export type ReaderFont = typeof READER_FONTS[number];
export const READER_THEMES = ['auto', 'light', 'sepia', 'dark'] as const;
export type ReaderTheme = typeof READER_THEMES[number];
export const READER_SIZE = { min: 14, max: 30 } as const;
export const READER_LINE = { min: 12, max: 24 } as const;
export const READER_WIDTH = { min: 40, max: 120 } as const;
export const MAX_TEMPLATE = 80;
export const MAX_DIR = 240;

export interface PageToolsValues {
  pt_spatial_nav: boolean; pt_capture_dir: string; pt_capture_name: string; pt_capture_format: CaptureFormat;
  pt_zoom_scope: ZoomScope; pt_wheel_zoom: boolean; pt_pip_menu: boolean;
  pt_reader_font: ReaderFont; pt_reader_size: number; pt_reader_line: number; pt_reader_width: number;
  pt_reader_theme: ReaderTheme; pt_reader_vertical: boolean; pt_reader_icon: boolean; pt_reader_time: boolean;
}
export type PageToolsKey = keyof PageToolsValues;

const BOOLS = ['pt_spatial_nav', 'pt_wheel_zoom', 'pt_pip_menu', 'pt_reader_vertical', 'pt_reader_icon', 'pt_reader_time'] as const;
const INTS: ReadonlyArray<readonly [PageToolsKey, { readonly min: number; readonly max: number }]> = [
  ['pt_reader_size', READER_SIZE], ['pt_reader_line', READER_LINE], ['pt_reader_width', READER_WIDTH],
];
export const PAGE_TOOLS_KEYS: readonly PageToolsKey[] = [
  'pt_spatial_nav', 'pt_capture_dir', 'pt_capture_name', 'pt_capture_format', 'pt_zoom_scope', 'pt_wheel_zoom', 'pt_pip_menu',
  'pt_reader_font', 'pt_reader_size', 'pt_reader_line', 'pt_reader_width', 'pt_reader_theme', 'pt_reader_vertical', 'pt_reader_icon', 'pt_reader_time',
];

export interface PageToolsSnapshot { readonly s: PageToolsValues; readonly guest: boolean; readonly ack: 'none' | 'saved' | 'failed'; readonly error: string }

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const oneOf = <T extends string>(list: readonly T[], v: unknown): v is T => typeof v === 'string' && (list as readonly string[]).includes(v);

/** Strict validation of the `pagetools` snapshot; `null` for anything partial, extended or malformed. */
export function parsePageToolsSnapshot(d: SnapshotMsg['d']): PageToolsSnapshot | null {
  const keys = ['s', 'guest', 'ack', 'error'];
  if (Object.keys(d).length !== keys.length || Object.keys(d).some((key) => !keys.includes(key))) return null;
  const v = d.s;
  if (!isRecord(v) || Object.keys(v).length !== PAGE_TOOLS_KEYS.length || Object.keys(v).some((key) => !(PAGE_TOOLS_KEYS as readonly string[]).includes(key))) return null;
  for (const key of BOOLS) if (typeof v[key] !== 'boolean') return null;
  for (const [key, range] of INTS) if (typeof v[key] !== 'number' || !Number.isInteger(v[key]) || (v[key] as number) < range.min || (v[key] as number) > range.max) return null;
  if (typeof v.pt_capture_dir !== 'string' || v.pt_capture_dir.length > MAX_DIR || typeof v.pt_capture_name !== 'string' || v.pt_capture_name.length > MAX_TEMPLATE) return null;
  if (!oneOf(CAPTURE_FORMATS, v.pt_capture_format) || !oneOf(ZOOM_SCOPES, v.pt_zoom_scope) || !oneOf(READER_FONTS, v.pt_reader_font) || !oneOf(READER_THEMES, v.pt_reader_theme)) return null;
  if (typeof d.guest !== 'boolean' || !oneOf(['none', 'saved', 'failed'] as const, d.ack) || typeof d.error !== 'string' || d.error.length > 400) return null;
  return { s: v as unknown as PageToolsValues, guest: d.guest, ack: d.ack, error: d.error };
}

/** A folder the page may ask for: a drive-letter path on this PC, nothing that names another machine or a device. */
export function isCaptureDir(text: string): boolean {
  if (text === '') return true;
  if (text.length > MAX_DIR || /[\u0000-\u001f<>"|?*]/.test(text) || !/^[A-Za-z]:[\\/]/.test(text) || text.slice(2).includes(':')) return false;
  return !text.slice(2).split(/[\\/]/).some((part) => part === '..' || part === '.' || part.endsWith(' ') || (part.length > 1 && part.endsWith('.')));
}

/** A file name template: known {tokens} only, no path separators. */
export function isCaptureName(text: string): boolean {
  if (text === '' || text.length > MAX_TEMPLATE || /[\u0000-\u001f\\/]/.test(text)) return false;
  const tokens = text.match(/[{}]/g) ?? [];
  if (tokens.length % 2 !== 0) return false;
  const found = text.match(/\{[^{}]*\}/g) ?? [];
  if (found.length * 2 !== tokens.length) return false;
  return found.every((token) => ['{host}', '{title}', '{date}', '{time}', '{mode}'].includes(token));
}

/** The `set` fields that change one option, `null` for a value Rust would refuse. */
export function setFields(key: PageToolsKey, value: boolean | number | string): { a: 'ptset'; text: string } | null {
  if (!PAGE_TOOLS_KEYS.includes(key)) return null;
  const text = String(value);
  if (/[\n\r\t]/.test(text)) return null;
  const range = INTS.find(([name]) => name === key)?.[1];
  if (range && !(Number.isInteger(value) && (value as number) >= range.min && (value as number) <= range.max)) return null;
  if (key === 'pt_capture_dir' && !isCaptureDir(text)) return null;
  if (key === 'pt_capture_name' && !isCaptureName(text)) return null;
  return { a: 'ptset', text: `${key}=${text}` };
}

/** "1.6" for 16: the line height as a person reads it. */
export const lineText = (tenths: number): string => (tenths / 10).toFixed(1);

const S = (id: string, label: string, description: string, keywords: string): RowSpec => ({ id, section: 'pagetools', label, description, keywords });

export const PAGE_TOOLS_ROWS: readonly RowSpec[] = [
  S('ptCaptureDir', 'Folder', '', 'capture screenshot folder directory save pictures location Capture folder Where captures are saved. Empty is Pictures\\Bowser. A folder on this PC, such as C:\\Users\\you\\Pictures.'),
  S('ptCaptureName', 'File name', '', 'capture screenshot file name template pattern date time Capture file name Use {host}, {title}, {date}, {time} and {mode} (visible, full or selection).'),
  S('ptCaptureFormat', 'Format', '', 'capture screenshot format png jpeg jpg image type Capture format PNG keeps every pixel. JPEG makes smaller files.'),
  S('ptSpatial', 'Link keys', '', 'spatial navigation links keyboard arrow shift focus move Move between links with Shift and the arrow keys Shift+arrow keys jump to the nearest link, Enter opens it and Ctrl+Enter opens it in a new tab. Not used while you type in a text field.'),
  S('ptZoomScope', 'Zoom scope', '', 'zoom tab site scope page remember per Page zoom belongs to The site keeps its zoom for every tab and the next visit. The tab keeps the zoom you set for every page it shows.'),
  S('ptWheelZoom', 'Wheel zoom', '', 'zoom ctrl scroll wheel mouse Zoom with Ctrl and the mouse wheel Off, Ctrl+scroll does nothing to the page. The zoom shortcuts still work.'),
  S('ptPip', 'Picture in picture', '', 'picture in picture pop out video popout pip button Offer picture in picture in the tab menu A tab playing a video lists Picture in picture in its menu. The shortcut works either way.'),
  S('ptReaderFont', 'Font', '', 'reader view font face serif sans mono typeface Reader view font The typeface reader view sets the text in.'),
  S('ptReaderSize', 'Text size', '', 'reader view font size text Reader view text size In pixels. The buttons on the page change it for the page you are reading.'),
  S('ptReaderLine', 'Line height', '', 'reader view line height spacing leading Reader view line height The space between lines, as a multiple of the text size.'),
  S('ptReaderWidth', 'Column width', '', 'reader view column width measure characters Reader view column width How wide a line of text may be, in characters.'),
  S('ptReaderTheme', 'Colours', '', 'reader view colour scheme theme light sepia dark Reader view colours Auto follows Bowser. The buttons on the page change it for the page you are reading.'),
  S('ptReaderVertical', 'Vertical text', '', 'reader view vertical text writing mode Reader view vertical text Text runs top to bottom, lines from right to left, as in traditional Chinese and Japanese.'),
  S('ptReaderIcon', 'On-page controls', '', 'reader view settings icon controls bar page Show the reader controls on the page A small bar on the page changes the text size and colours. Off, reading is undisturbed.'),
  S('ptReaderTime', 'Reading time', '', 'reader view reading time counter minutes Show the reading time The reader controls say how many minutes the page takes to read.'),
];

/** The rows a person changes often enough to see without opening "More options". */
export const PAGE_TOOLS_CORE: readonly string[] = ['ptCaptureDir', 'ptCaptureName', 'ptSpatial', 'ptZoomScope', 'ptWheelZoom'];
